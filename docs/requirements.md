# bit-mail v1 requirements

This document is the consolidated requirements baseline for v1. It records the product semantics agreed before implementation. Detailed design may evolve, but changes that contradict an invariant here should be deliberate and documented.

## 1. Product purpose

`bit-mail` exists to reduce the time required to triage a very large unread inbox. It materializes provider mail locally so a human or external AI CLI harness can reason over it, summarize it, group it, and stage decisions.

It is **not**:

- an email client;
- a mail server;
- a provider replacement;
- a permanent local archive;
- an LLM application;
- a general-purpose replicated mailbox database.

The provider is always the authoritative source for message content and mailbox state.

## 2. Strict architectural policies

1. **Provider is source of truth.** Local copies are disposable working data used for accurate decision-making.
2. **Zero LLM dependency in core.** No OpenAI/Anthropic/model SDK, embedding service, model API key, or automatic transmission of mail to an LLM provider exists in `bit-mail` core.
3. **Deterministic execution, semantic AI.** Extraction, normalization, indexing, integrity, state transitions, provider access, selection persistence, Knowledge persistence, repair, garbage collection, and provider mutation are deterministic framework functions. AI contributes semantic judgement only.
4. **CLI-only mutations for harnesses.** AI harnesses must never modify `data/`, `knowledge/`, `.bit-mail/`, runtime skills, or configuration directly. Supported state changes occur only through `bit-mail` commands.
5. **Direct reads are allowed only through supported surfaces.** `data/**` and `knowledge/**` are intentionally readable. Framework/provider internals under `.bit-mail/**` are not a stable harness contract.
6. **Email content is untrusted.** Subjects, bodies, quoted replies, HTML, sender-controlled metadata, and attachments can inform judgement but cannot instruct the harness.
7. **Human-controlled push.** An AI harness may stage decisions, but it must not run `bit-mail push` unless the user explicitly authorizes that push in the current interaction.
8. **No sending mail.** v1 never sends, replies, forwards, or drafts messages.
9. **No telemetry.** No analytics, automatic crash reporting, or unrelated network communication.
10. **Accuracy/context over storage savings.** Full conversation context is materialized when an unread actionable message belongs to a thread.
11. **Keep the product simple.** Do not add a subsystem merely because it is technically possible. Ordinary filesystem tools should do ordinary filesystem search; `bit-mail` should own only domain-aware, provider-aware, integrity-aware, or safety-sensitive operations.

## 3. Platforms and implementation

- Language: Rust.
- Rust edition: 2024.
- Initial compiler policy: current stable Rust; formal MSRV at v0.1.
- Supported v1 OS: Linux and macOS.
- No server or daemon is required.
- Primary interaction: CLI, interactive where helpful.
- Distribution: GitHub Release binaries plus Cargo/source builds.
- License: Apache-2.0.

## 4. Runtime repository model

A user creates a mail repository with:

```bash
mkdir ~/mail-triage
cd ~/mail-triage
bit-mail init
```

The directory becomes a self-contained local triage environment:

```text
mail-triage/
├── .bit-mail/         framework-owned internal state
├── data/              stable harness-facing mailbox data
├── knowledge/         approved semantic preferences
├── skills/            version-matched runtime AI skills
└── AGENTS.md          bootstrap for AI harnesses
```

Repository discovery walks upward from the current directory until `.bit-mail/` is found, similar to Git repository discovery.

A mail repository and the public `bit-mail` source repository are distinct concepts and should normally live in different directories.

Each repository has an immutable repository UUID generated at `init` time.

`bit-mail init`:

- can initialize a non-empty directory;
- never overwrites conflicting managed paths;
- has no `--force` in v1;
- creates private directories/files with secure default permissions;
- if inside a Git repository, offers to protect `/.bit-mail/`, `/data/`, and `/knowledge/` with ignore rules;
- warns if private runtime data becomes Git-tracked.

`bit-mail` itself does not depend on Git.

## 5. Accounts and isolation

A repository may contain multiple provider accounts.

Each account has:

- immutable account UUID;
- mutable human alias;
- provider binding;
- independent local data namespace;
- provider synchronization state;
- structural SQLite index;
- work items;
- selections;
- audit log;
- integrity branches;
- lock.

The same provider mailbox may only be connected once within a repository. The same mailbox in multiple separate repositories is technically possible but discouraged and not coordinated.

Account selection resolution order:

1. explicit `--account <alias>`;
2. current directory is inside an account-owned data path;
3. `BIT_MAIL_ACCOUNT` environment variable;
4. exactly one configured account;
5. otherwise error.

Conflicting implicit account contexts fail closed. Explicit `--account` resolves ambiguity.

Mutating operations resolve to exactly one account. Account-level locking permits unrelated accounts to operate concurrently.

`--all-accounts` is allowed for repository-wide inspection and `pull`, but never for `push`.

## 6. Provider adapters

v1 provider: Gmail, including Google Workspace Gmail through the same adapter.

Architecture:

```text
bit-mail core
    |
    +-- MailProvider trait
           |
           +-- GmailAdapter
           +-- future provider adapters
```

Adapters are compile-time Rust implementations, not runtime dynamic plugins.

Provider adapters use direct public REST/HTTP APIs by default using small `bit-mail`-owned DTOs. Provider SDK crates are introduced only if a concrete required capability cannot reasonably or safely be implemented via the public REST API.

No provider-specific SDK types escape the adapter boundary.

## 7. Gmail authorization

Target user model: local tool for technical users. Users bring their own Google OAuth client configuration.

- Gmail OAuth scope: `https://www.googleapis.com/auth/gmail.modify` only.
- No broader `https://mail.google.com/` scope.
- OAuth client profiles are reusable across multiple Gmail accounts.
- Every mailbox authorization has its own refresh token.
- `connect` owns OAuth profile import/reuse and account authorization end-to-end.
- Interactive workstation OAuth is the v1 target; browser launch may fall back to displaying the authorization URL.
- No service-account/unattended Gmail authentication in v1.
- OAuth client secrets and refresh tokens are stored automatically in the OS credential store (macOS Keychain / Linux Secret Service or equivalent secure backend).
- No plaintext credential fallback in v1. If secure storage is unavailable, fail closed and point to documentation.
- Access tokens should be memory-only where practical.
- Runtime repository config contains credential references, never secrets.
- Credentials are namespaced by repository UUID and account UUID to prevent collisions.

Supported Google configurations:

- External OAuth profile for personal Gmail or mixed personal + Workspace accounts.
- Internal OAuth profile for Workspace-only organizational use.
- External profiles intended for durable use should not remain in Google Testing status.

Repository data is path-portable. Credentials remain machine-local. Copying a repository to another machine requires account reauthorization before provider-facing operations; offline triage remains available.

## 8. Pull scope and backlog

`pull` means provider -> local.

By default Gmail seeds are messages that are both `INBOX` and `UNREAD`, regardless of Gmail category. Archived, Spam, Trash, and Sent mail are not independent pull seeds.

Read, archived, Sent, or otherwise non-actionable messages are still materialized when they belong to the complete thread of an actionable unread Inbox message.

Default pull is bounded and newest-first. Initial target default is 500 seed messages per account, configurable later. `--all` explicitly requests the entire unread backlog.

`--limit` counts seed unread messages, not the final work-item count. Full-thread materialization may discover additional unread Inbox messages; they become work items immediately, even if the final count exceeds the seed limit.

Per-account provider state combines:

- Gmail `historyId` cursor for incremental mailbox changes;
- a separate unread-backlog checkpoint so old unread mail not present in history can be progressively ingested.

If Gmail history has expired, the adapter falls back to the required full reconciliation path.

## 9. Pull blocking rule

An account may be pulled while it has ordinary `pending` work items.

An account **cannot** be pulled while it has any staged `read` or `delete` decisions. The user must either:

- `push` those decisions; or
- `unstage` them back to pending.

There is no `pull --force` in v1.

The rule is account-scoped. Staged changes in one account do not block another account.

`pull --all-accounts` skips staged/blocked accounts and continues clean accounts independently.

## 10. Canonical message and thread model

The individual provider message is the canonical message unit.

Each provider message is stored **exactly once per account** and is assigned a stable provider-independent UUIDv7 for the lifetime of the repository.

A durable identity registry maps `(account UUID, provider message ID)` to the stable `bit-mail` message UUID so cache rebuilds preserve message identity.

Thread relationships are references only. Thread manifests never duplicate message content.

Conceptual model:

```text
Canonical Message
    |\
    | +-- Thread membership (context relationship)
    |
    +---- Work item (only if actionable)
              |
              +-- pending | read | delete
```

Each `INBOX + UNREAD` message gets its own work item, even when multiple unread messages belong to the same thread.

Context-only messages have no work item and cannot be staged.

Message-level actions never implicitly apply to an entire Gmail thread.

## 11. Full thread context

When `pull` encounters an actionable unread message, it materializes the **complete Gmail thread** for accurate human/AI judgement.

Full context includes:

- read messages;
- archived messages;
- the user's own Sent messages;
- other conversation messages required to reconstruct the thread.

There is no automatic thread-size truncation in v1. Accuracy/context wins.

A thread manifest references canonical message UUIDs in conversation order. The same canonical message is never duplicated merely because it participates in relationships or selections.

`bit-mail show <message-id> --context` deterministically assembles the complete thread for a harness/human and clearly delimits email content as untrusted.

## 12. Local storage contract

`data/` is the stable harness-facing filesystem contract. Provider/framework internals live under `.bit-mail/`.

Conceptual runtime layout:

```text
data/<account-uuid>/messages/<message-uuid>/
├── content.md
├── metadata.json
└── attachments/

.bit-mail/accounts/<account-uuid>/
├── account.toml
├── provider/
├── provider-state.json
├── threads/
├── work-items/
├── selections/
├── identities/
├── integrity/
├── audit/
└── index.sqlite

.bit-mail/locks/
├── account-lifecycle.lock
├── accounts/<account-uuid>.lock
└── knowledge.lock
```

Mutation locks are transient and UUID-keyed outside deletable account state so
removing an account cannot invalidate a lock that is still held.

Harness-facing stable formats:

- `content.md` — deterministic normalized, preservation-oriented readable content.
- `metadata.json` — provider-independent stable metadata schema.
- fetched attachments under the canonical message directory.

Provider-specific message representation and synchronization details are framework internals under `.bit-mail/` and are not a stable harness contract.

## 13. Message normalization

Normalization uses deterministic Rust code, never LLM tokens.

Processing includes:

- MIME parsing;
- character-set decoding;
- quoted-printable/base64 decoding;
- plain-text extraction when suitable;
- HTML-to-readable-Markdown conversion otherwise;
- link preservation;
- no remote image/resource loading;
- attachment discovery.

v1 favors preservation and accuracy over aggressive cleanup. Quoted history, signatures, footers, and useful content are not heuristically discarded simply to reduce tokens.

If normalization is partial or fails:

- retain authoritative provider-derived representation internally;
- preserve whatever content can safely be recovered;
- record diagnostics;
- never silently drop the message.

Exact raw RFC/MIME content is optional and fetched explicitly with a future/required `raw fetch` capability rather than being mandatory during normal pull.

## 14. Attachments

Attachment behavior is opportunistically lazy:

- if Gmail already supplies attachment bytes during the message/thread fetch, persist them rather than discarding downloaded data;
- if Gmail provides only an attachment ID / requires another request, retain metadata and fetch on demand;
- `bit-mail attachment fetch <message-id> <part-id>` deterministically retrieves required remote attachments;
- an already-local attachment fetch is idempotent and makes no network request;
- the AI skill must fetch an attachment before judgement when that attachment is required to understand the message accurately;
- attachment contents are not automatically indexed/searchable in v1.

Fetched attachments follow the parent thread/message cache lifecycle and are removed when no active work item needs that conversation context.

## 15. Work item lifecycle and staging

Workflow state is a single enum:

- `pending` — requires decision;
- `read` — staged intent to mark provider message read;
- `delete` — staged intent to move provider message to Trash.

Terminology:

```text
pending -> bit-mail stage ... read   -> staged read
pending -> bit-mail stage ... delete -> staged delete
staged  -> bit-mail unstage ...      -> pending
```

There is no direct provider mutation during staging.

`delete` always means move the specific provider message to Trash. Permanent deletion is unsupported in v1.

`read` means remove Gmail's `UNREAD` state so other Gmail clients observe it as read.

## 16. Push

`push` means local staged intent -> provider.

There is no `sync` command in v1.

Default `push` behavior:

1. determine exact account/scope;
2. integrity-validate required local objects;
3. show summary/preview;
4. require confirmation;
5. perform lightweight provider-state preflight per staged message;
6. apply mark-read or move-to-Trash;
7. verify result as far as provider semantics allow;
8. remove successful work items/local cache that is no longer referenced;
9. keep failures staged;
10. write metadata-only audit records.

Supported safety/convenience forms include:

- `push --dry-run`;
- `push --yes` for deliberate non-harness automation;
- `push --message <id>`;
- `push --selection <name>`.

`push --all-accounts` is not supported.

Provider preflight is lightweight. `push` does not re-download/re-normalize full message content; content refresh belongs to `pull`.

Provider state is authoritative, but staged intent remains the requested action unless it is already satisfied or the provider object no longer exists.

Examples:

- staged read + already read => success/no-op;
- staged delete + already read => still move to Trash;
- staged delete + already in Trash => success/no-op;
- provider object missing => resolve locally with audit/warning.

## 17. Threaded deletion safety

`bit-mail` v1 has no thread-delete operation.

A staged delete always targets one specific message.

Because multi-message conversations are commonly important, harness guidance must:

- inspect full thread context first;
- generally prefer `read` over `delete` for genuine conversations;
- use delete only with strong evidence or explicit approved Knowledge;
- ask the user when uncertain.

`push` deterministically identifies deletes whose messages belong to multi-message threads and requires an additional review/confirmation step. Harness skills prohibit autonomous `push --yes` for threaded deletes.

## 18. Bulk operations and selections

Selections are persistent, named, account-scoped sets of actionable message IDs.

They:

- contain references only, never message copies;
- do not themselves alter provider state;
- can be created/updated through CLI only;
- can be used as bulk stage or partial push scopes;
- are automatically pruned when member work items disappear;
- remain as empty selections until explicitly removed.

Historical membership belongs in the audit log, not stale selection IDs.

Bulk staging supports:

- one message;
- multiple explicit IDs;
- newline-separated IDs from stdin;
- a named selection.

For a bulk local mutation, validate all input IDs before changing any state.

## 19. Knowledge system

Knowledge stores reusable, user-approved semantic preferences such as:

- “I don't care about cryptocurrency price movements.”
- “Infrastructure vendor product updates are usually worth reviewing on my work account.”

Knowledge scope:

- repository-global (`knowledge/global/`), applying to all accounts in that repository;
- account-specific (`knowledge/accounts/<account-uuid>/`).

There is no machine-global Knowledge in v1.

Each Knowledge item is a separate Markdown file with UUIDv7 identity and deterministic frontmatter. Filesystem is canonical.

Harnesses may read Knowledge directly but must never edit it. Persistence changes happen only via `bit-mail knowledge ...` commands.

AI may notice a recurring preference and suggest it, but **persistent Knowledge always requires explicit user approval**. If scope is ambiguous, ask whether it should be repository-global or account-specific.

Knowledge is preserved across provider cache rebuilds.

## 20. No general AI annotation store

v1 intentionally has no general-purpose persistent AI annotations/classifications/reasoning subsystem.

Semantic reasoning is ephemeral unless it produces one of the intentionally small durable artifacts:

- staged work-item intent;
- selection membership;
- approved Knowledge;
- audit event.

## 21. SQLite structural index

SQLite is a disposable derived structural/domain index, not canonical storage.

It may accelerate mappings such as:

- message UUID <-> provider IDs;
- message -> thread;
- work-item state;
- filesystem paths;
- selection membership.

There is **no generic FTS5 email search requirement in v1**. Content searching belongs to deterministic filesystem tooling such as `rg`, `find`, and normal harness capabilities.

If SQLite is missing/corrupt or its schema changes, rebuild it from canonical persistent state rather than migrate it as authoritative data.

## 22. Deterministic retrieval CLI

`bit-mail` should not duplicate ordinary filesystem search. It provides domain-aware retrieval the harness should not have to infer.

Required examples:

- `bit-mail status`;
- `bit-mail work-items` and `--json`;
- `bit-mail show <message-id>`;
- `bit-mail show <message-id> --context`;
- `bit-mail thread show <message-id>` where useful;
- `bit-mail selection show <name>`;
- `bit-mail knowledge list`;
- `bit-mail context --json`;
- `bit-mail help --json`.

`bit-mail context --json` is the deterministic session bootstrap for AI harnesses. It reports repository/account identity, supported data/Knowledge paths, and staging status.

`bit-mail help --json` is a versioned machine-readable capability API describing public commands, arguments, network behavior, mutation properties, and safety constraints. AI harnesses must not invent commands.

Human-readable `--help` remains available but is not the automation API.

## 23. Integrity model

v1 uses BLAKE3 and an application-level hierarchical Merkle integrity tree.

BLAKE3 is used for leaf/object hashing. Parent hashes are computed from canonical, domain-separated, deterministically ordered child representations.

Example domains may include:

- `bit-mail:file:v1`;
- `bit-mail:message:v1`;
- `bit-mail:thread:v1`;
- `bit-mail:account:v1`;
- `bit-mail:repository:v1`.

BLAKE3 itself is internally tree-structured per byte stream; `bit-mail` builds a separate object/directory Merkle hierarchy above file hashes.

Integrity goals:

- detect accidental/out-of-band managed-file modification;
- localize which object/file changed;
- fail closed before provider mutation;
- avoid validating unrelated branches during scoped operations;
- preserve fast user experience.

Integrity is **not** a defense against malicious same-user code capable of rewriting both content and integrity metadata. HMAC/authenticated local state is not a v1 requirement.

Performance policy:

- parallelize across many small independent files/objects;
- do not blindly enable per-file multithreading for small inputs;
- use mmap/internal BLAKE3 parallel paths for large inputs only when benchmarks justify it;
- ordinary read commands must remain fast and should not full-scan repository bytes;
- sensitive mutations validate only required branches;
- `bit-mail doctor --full` performs explicit repository-wide byte verification.

Transient/disposable data such as locks and SQLite are excluded from canonical Merkle coverage. Canonical/persistent framework state, Knowledge, configuration, audit, thread/work-item/selection state, and runtime trusted skills/templates are integrity-covered as appropriate.

## 24. Repair, garbage collection, and cache rebuild

If integrity failure affects a message in a conversation:

- provider is authoritative;
- repair re-fetches the complete thread;
- deterministic normalized representation and integrity state are rebuilt;
- all actionable staged decisions in that affected thread are invalidated;
- qualifying unread Inbox messages return to `pending`;
- provider state itself is not changed merely to match previous local state.

Thread context is reference-count/reachability driven:

- keep complete thread corpus while any actionable work item in the thread remains;
- once the final work item disappears, the thread manifest, canonical cached messages, and fetched attachments become garbage-collectable;
- `bit-mail gc --dry-run` / `bit-mail gc` provide deterministic cleanup/repair behavior.

`bit-mail cache rebuild` is account-scoped recovery:

- refuses if staged read/delete items exist;
- discards provider-derived message cache, thread manifests, pending work items, selections, provider cursors, disposable SQLite, and related cache Merkle state;
- preserves account identity/config, credentials, stable message identity registry, Knowledge, and audit history;
- next `pull` repopulates from provider;
- stable message UUIDs are reused through the identity registry.

## 25. Pull and push resilience

Provider I/O uses bounded concurrency.

Retries:

- transient network failures => bounded retry;
- HTTP 429/rate limits => backoff and respect `Retry-After` when present;
- 5xx => bounded retry;
- authentication failure => stop/report clearly;
- permanent per-message/provider error => keep affected local state retryable and continue unrelated items where safe.

`pull` materializes a complete thread/message set into temporary state first, normalizes it, generates integrity metadata, validates it, and only then publishes it atomically. Half-materialized canonical objects must never become visible to the harness.

Provider cursors/checkpoints advance conservatively so failed items cannot disappear from future consideration.

`push` commits successful messages independently. There is no fake cross-message transaction or rollback model. Failed operations remain staged.

## 26. Concurrency and locking

Every account mutation acquires that account's exclusive lock. Normal reads are lock-free.

Examples of account mutations:

- pull;
- stage/unstage;
- selection mutation;
- account-scoped Knowledge mutation;
- attachment fetch;
- repair;
- push;
- index rebuild;
- cache rebuild;
- garbage collection.

Repository-global Knowledge mutation uses a separate repository-level Knowledge lock.

Lock contention fails clearly rather than waiting indefinitely. Diagnostics should identify the lock holder/process when possible.

Filesystem writes use write-temp + atomic rename patterns where supported.

## 27. Audit

Audit logs are:

- append-only;
- metadata-only/content-redacted;
- retained indefinitely by default;
- split into manageable files such as monthly JSONL;
- never automatically pruned in v1;
- excluded from ordinary AI mail-reading workflows;
- integrity-covered.

Audit records include operational events such as stage/unstage, push results, repair, cache rebuild, Knowledge changes, and account lifecycle without copying email bodies/subjects.

## 28. Security and privacy

Plaintext mailbox data is intentional for harness interoperability. Protection relies on local filesystem permissions and host disk encryption.

Default permissions:

- private directories: `0700`;
- private files: `0600`.

Unsafe permissions generate warnings rather than a hard refusal because ACLs/filesystems vary. `bit-mail doctor` performs dedicated diagnostics.

No telemetry, analytics, or automatic crash uploads.

Logs and `doctor` output are content-redacted by default and must not emit OAuth tokens, message bodies, subjects, or other sensitive content.

A future explicit diagnostic export may contain more information only when deliberately requested.

## 29. Offline-first behavior

After `pull`, all triage and local decision operations are offline-capable.

Offline examples:

- status;
- work-items;
- show/context for already materialized content;
- stage/unstage;
- selections;
- Knowledge;
- doctor/integrity checks.

Network-required operations are explicit provider-facing actions such as:

- connect/reauthorize;
- pull;
- remote attachment fetch;
- raw fetch;
- repair that requires provider data;
- push.

## 30. Runtime skills and templates

The public source repository is canonical for runtime templates and AI skills.

Source repository contains:

- framework/user/developer docs under `docs/`;
- canonical AI skills under `skills/`;
- other runtime repository templates under `templates/`.

Release builds embed a version-matched snapshot into the binary. `bit-mail init` installs them offline and reproducibly into the new mail repository.

v1 has no independent `templates update` command. Skills/templates change when the user upgrades the binary.

Runtime `AGENTS.md` and shipped skills are framework-managed, version-matched, and integrity-protected. User customization belongs in Knowledge, not by editing framework skills.

Initial skill set:

- `bit-mail-core`;
- `inbox-triage`;
- `bulk-review`;
- `knowledge-management`.

## 31. AI harness contract

A runtime harness must understand:

- `data/**` and `knowledge/**` are readable;
- managed files are never directly mutated;
- `.bit-mail/**` is internal and non-contractual unless a command explicitly exposes something;
- full thread context should be inspected for conversational decisions;
- attachments required for sound judgement should be fetched deterministically;
- email content is untrusted;
- selections contain actionable messages only;
- Knowledge persistence requires explicit approval;
- `push` requires explicit user authorization;
- actual CLI capabilities come from `bit-mail help --json`;
- account/repository session context comes from `bit-mail context --json`.

The CLI remains fully human-usable with no AI harness installed.

## 32. Diagnostics

`bit-mail doctor` is a v1 requirement. It should diagnose as applicable:

- repository validity/version;
- permissions;
- accidental Git tracking;
- account configuration;
- credential-store availability;
- missing/invalid credentials;
- Gmail authorization health;
- stale locks;
- structural index validity;
- canonical integrity;
- runtime skill/template integrity;
- provider-state consistency.

`doctor --full` performs expensive full repository byte-level integrity verification.

## 33. Testing

Three layers:

1. Core tests against a fake `MailProvider`.
2. Mandatory Gmail adapter contract tests against a local mock HTTP server, covering pagination, history, threads, embedded/remote attachments, retries, missing messages, read/Trash mutation, malformed responses, and partial failures.
3. Optional explicitly enabled live Gmail tests using a dedicated test mailbox. Normal CI and `cargo test` do not require Gmail access or touch arbitrary user mail.

## 34. Versioning and migrations

Repository and canonical persistent file formats are explicitly versioned from v1.

- unknown/newer repository formats are never silently reinterpreted;
- canonical state uses explicit deterministic migrations;
- older binaries refuse newer unsupported formats without modification;
- disposable SQLite schemas are rebuilt rather than treated as canonical migration targets.

## 35. Explicit v1 non-goals

- sending/replying/forwarding/drafts;
- archive/label/spam/general mailbox management;
- permanent Gmail deletion;
- runtime provider plugins;
- built-in LLMs or embeddings;
- general AI annotations/reasoning database;
- generic email full-text search engine/FTS5;
- server/daemon mode;
- distributed/cross-repository coordination;
- machine-global Knowledge;
- force pull through staged state;
- independent online template updates;
- OS-level sandboxing of arbitrary AI harnesses;
- defending against malicious code already executing as the same OS user.
