# M008 — Push engine

**Status:** Complete
**Depends on:** M003, M006, M007  
**Outcome:** Safe, reviewed, idempotent local intent -> Gmail provider mutation.

## Scope and preview

- [x] Implement account-scoped `push` defaulting to all staged actions in selected account.
- [x] Implement `push --dry-run`.
- [x] Implement `push --message <id>`.
- [x] Implement `push --selection <name>`.
- [x] Implement explicit `--yes` for deliberate scripted use.
- [x] Prohibit `push --all-accounts` structurally.
- [x] Human preview distinguishes staged read/delete and threaded-delete risk.
- [x] Stable `--json` preview/result schema.

## Preflight

- [x] Integrity validate affected branches first.
- [x] Fetch only lightweight current provider state required for safe/idempotent operation.
- [x] No full content/thread re-normalization during push.
- [x] Missing provider object resolves locally with audit/warning.

## Gmail mutations

- [x] Staged read => remove `UNREAD` when needed.
- [x] Already read => success/no-op.
- [x] Staged delete => move exactly one message to Trash.
- [x] Already read does not cancel delete intent.
- [x] Already in Trash => success/no-op.
- [x] Permanent deletion unsupported.
- [x] No whole-thread delete API.

## Threaded delete safeguard

- [x] Detect staged-delete message in multi-message thread.
- [x] Separate them in preview.
- [x] Require additional confirmation after normal review.
- [x] Ensure `--yes` semantics are explicit in docs and skills prohibit autonomous harness use.

## Resilience

- [x] Bounded concurrent operations.
- [x] Retry transient network/429/5xx with backoff.
- [x] Auth failure stops clearly.
- [x] Permanent per-message failure remains staged.
- [x] Successful independent operations are committed/cleaned locally; no fake rollback.
- [x] Partial push leaves account pull-blocked while any staged actions remain.

## Cleanup/audit

- [x] Remove successful work items.
- [x] Trigger selection pruning and reachability GC as appropriate.
- [x] Write content-redacted audit events.
- [x] Track last successful push metadata.

## Tests

- [x] Read idempotency.
- [x] Trash idempotency.
- [x] Missing-message behavior.
- [x] Partial failures remain staged.
- [x] Threaded-delete double-confirmation path.
- [x] Partial push by message/selection does not broaden scope.
- [x] `--all-accounts` rejected.
- [x] Integrity mismatch prevents all provider mutation for affected scope.

## Exit criteria

- [x] Provider mutation surface contains only read + Trash.
- [x] Default human flow cannot accidentally bypass preview/confirmation.

## Progress log

- 2026-08-30: Implemented scoped integrity-first push, stable preview/results,
  double confirmation, bounded idempotent Gmail message mutations, independent
  local cleanup/audit/metadata, and focused core/Gmail/CLI coverage.
