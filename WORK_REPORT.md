# Gmail 403 failures after cache rebuild

## Status

Implementation review: all findings are accepted.

Reviewed the current working-tree implementation on 2026-09-08. Findings 1,
2, 4, and 5 meet their stated intent. Findings 3 and 6 have now been completed
with their remaining acceptance checks covered.

Validated against `bit-mail` 0.1.3 at `b536125` (`Cargo.toml` reports 0.1.3).
The primary bug is confirmed by code inspection. The incident's exact Gmail
error reason cannot be proven from the supplied log because the client discards
the 403 response body. No live Gmail reproduction was attempted.

Priority: high. A valid, retryable Gmail response can be reported as an OAuth
failure, and a large resync can repeatedly perform the same provider work.

## Confirmed findings

### 1. Every Gmail 403 is incorrectly classified as authentication

Status: Accepted

Review result: GET and POST now reserve authentication classification for 401,
parse Gmail's structured 403 reason, and map non-rate-limit or malformed 403
responses to permanent errors without advising reauthorization
(`src/gmail.rs:275-305`, `427-449`, `506-543`). The focused tests cover both
request methods, `domainPolicy`, and malformed JSON.

`GmailClient::get` handles 401 and 403 in one branch and immediately returns
`ProviderErrorKind::Authentication` (`src/gmail.rs:249-265`). The equivalent
POST path does the same (`src/gmail.rs:391-397`). Neither branch reads Gmail's
JSON error `reason`.

This is incorrect for Gmail. Google documents 403 responses including
`rateLimitExceeded` and `userRateLimitExceeded`, and recommends exponential
backoff for both. Other 403 reasons, such as `domainPolicy` and
`dailyLimitExceeded`, are also not authentication failures:

- https://developers.google.com/workspace/gmail/api/guides/handle-errors
- https://developers.google.com/workspace/gmail/api/reference/quota

The observed mixture of successful and failed requests for the same operation,
plus successful online doctor checks before and after, is strong evidence
against an invalid access grant. Rate limiting is the most likely explanation,
but remains an inference until the Gmail `reason` is captured.

### 2. These 403s are not retried

Status: Accepted

Review result: both request paths retry only the documented rate-limit reasons,
increment the shared retry counter, use `Retry-After` when supplied, and
otherwise back off from one second (`src/gmail.rs:291-302`, `435-446`,
`546-553`). The retry tests pass for `userRateLimitExceeded` on GET and
`rateLimitExceeded` on POST.

Only 429 and 5xx responses enter the status retry branch
(`src/gmail.rs:279-311`, with the same behavior in POST at 407-430). A 403
returns on the first attempt, so it does not increment `GmailClient.retries`.
This explains `Retries: 0` despite hundreds of failures.

The existing backoff starts at 250 ms. Google's current guidance says to start
retry periods at least one second after the error. Existing `Retry-After`
support is useful, but caps even a server-provided delay at 30 seconds.

### 3. A rebuild creates a large initial resync, fetched four at a time without pacing

Status: Accepted

Review result: `GmailClient` now stores a mutex-protected cooldown and every GET
and POST attempt checks it (`src/gmail.rs:158-171`, `221-250`, `264`, `421`). This
provides the intended account-level reactive cooldown after a retryable status.
The shared request gate also reserves 200 ms slots proactively, and
`concurrent_requests_share_rate_limit_cooldown_and_pacing` proves concurrent
requests observe both controls (`src/gmail.rs:1452-1492`).

Cache rebuild removes provider state, work items, thread manifests, and cached
provider data (`src/recovery.rs:287-302`). Consequently, the next pull has no
history cursor and performs initial unread discovery (`src/pull.rs:157-164`,
`230-251`). This is documented behavior, not itself a defect.

Thread fetches use up to four continuously running workers. Each worker starts
its next request immediately after the previous one; there is no account-level
rate pacing or shared cooldown (`src/pull.rs:416-437`). Gmail currently charges
40 quota units for `threads.get` and applies per-user/per-project minute limits,
so a sustained cache refill can plausibly hit a per-user rate limit.

### 4. Failed thread work deliberately prevents cursor persistence

Status: Accepted — No Change Required

Review result: the implementation leaves the fail-closed checkpoint behavior
unchanged. Provider state is still written only when every thread succeeds
(`src/pull.rs:268-305`), and
`failed_thread_keeps_checkpoint_retryable_and_other_threads_complete` passes.

Pull counts each failed thread result, marks the account failed, and writes
provider state only when there are no failures (`src/pull.rs:268-305`). The
existing test `failed_thread_keeps_checkpoint_retryable_and_other_threads_complete`
explicitly asserts this contract (`src/pull.rs:1119-1155`). It also matches the
requirement that checkpoints must not advance past failed work
(`docs/requirements.md:568-570`).

After cache rebuild there is no prior provider state to retain, so the next pull
restarts initial discovery and re-fetches successful threads as well as failed
ones. That repetition is an impact of inadequate provider retry handling. Do
not fix it by advancing the cursor after partial failure; that could drop mail.

### 5. `Threads: 492` does not mean 492 successes

Status: Accepted

Review result: both human output paths now label this count `threads attempted`
(`src/main.rs:1206-1218`, `1237-1240`) while preserving the existing serialized
field and its meaning.

`result.threads` is assigned `fetched.len()`, which includes successful and
failed results (`src/pull.rs:261-264`). With 318 failures, the reported numbers
imply 174 successful thread fetches, not 492. The original report's “492 thread
fetches succeeded” interpretation is therefore incorrect, although 174
successes still support the valid-grant conclusion.

The human label is ambiguous and should say “Threads attempted”, or the report
should separately expose successful and attempted counts without silently
changing the existing JSON field's meaning.

### 6. `cache.reachability` discards its diagnostic cause

Status: Accepted

Review result: dry-run GC now preserves integrity mismatches as invalid-data
errors (`src/recovery.rs:172-179`), and doctor maps errors to redacted integrity,
malformed-metadata, filesystem, or fallback validation categories
(`src/diagnostics.rs:427-438`, `553-573`). Cache rebuild remediation is supplied
only for integrity or malformed metadata; filesystem and unknown validation
failures remain informational. The focused test verifies the actionable cause,
remediation, and JSON redaction (`src/diagnostics.rs:1027-1076`).

Doctor runs dry-run GC. Any error is collapsed to “cache reachability could not
be diagnosed” with no finding or remediation (`src/diagnostics.rs:410-435`).
Human, verbose, and JSON output cannot recover information that was discarded
there. Possible causes include integrity failure, malformed cache metadata,
locking, and filesystem I/O; the supplied output is insufficient to identify
which occurred.

This is a separate confirmed diagnostics defect. It may have prompted the cache
rebuild, but there is no evidence that btrfs, SELinux, or Secret Service caused
the 403 responses.

## Recommended implementation

Keep the change in the Gmail request layer, where all callers pass:

1. Parse Gmail's error envelope before classifying a non-success response.
2. Keep 401 as authentication.
3. Treat 403 `rateLimitExceeded` and `userRateLimitExceeded` as retryable using
   the existing bounded retry mechanism, with a minimum one-second exponential
   delay and `Retry-After` honored.
4. Give non-rate-limit 403 reasons accurate permanent/policy diagnostics rather
   than telling the user to reauthorize. Unknown or malformed 403 bodies must
   fail clearly and must not be guessed to be authentication.
5. Coordinate cooldown/pacing across the four workers for one Gmail client so
   one worker's rate-limit response slows its siblings. No new dependency is
   needed; use existing synchronization and time primitives.
6. Preserve the current checkpoint rule. Do not persist a cursor while any
   discovered thread remains failed.

Handle GET and POST consistently; both currently contain the same defective
classification. Avoid a broader provider API redesign unless implementation
shows it is necessary.

For `cache.reachability`, retain a content-redacted error category/message in
the check result and provide remediation only when the cause supports one. Do
not recommend cache rebuild for every failure: it is destructive to disposable
state and triggers a full resync.

## Acceptance checks

- A mock GET returning 403 `userRateLimitExceeded` and then 200 is retried and
  succeeds; the retry count increments.
- The same check covers 403 `rateLimitExceeded` and the POST path.
- A 401 remains an immediate authentication error with no retry.
- A 403 `domainPolicy` is not classified as authentication or retried.
- A malformed/unknown 403 is not reported as a revoked OAuth grant.
- Concurrent thread workers obey one shared cooldown after a rate-limit reply.
- Exhausted retryable 403s fail the pull and leave provider checkpoints
  unchanged; a later pull can recover.
- Pull output no longer invites interpreting attempted threads as successes.
- A forced dry-run GC failure produces a useful, redacted
  `cache.reachability` cause in human and JSON doctor output.

## Verification performed

`cargo fmt --check` passes. `cargo test` passes with localhost socket
permission: 136 passed, 0 failed, and 1 ignored. The ignored test is the manual
large-file benchmark. A sandboxed run failed only because mock HTTP tests could
not bind localhost sockets.

The suite covers reason-aware GET and POST 403 handling, 401 authentication,
four-worker pull concurrency, checkpoint preservation after failed threads,
and redacted `cache.reachability` output, including concurrent Gmail requests
observing shared cooldown and proactive pacing.
