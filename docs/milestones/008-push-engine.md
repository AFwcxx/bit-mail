# M008 — Push engine

**Status:** Planned  
**Depends on:** M003, M006, M007  
**Outcome:** Safe, reviewed, idempotent local intent -> Gmail provider mutation.

## Scope and preview

- [ ] Implement account-scoped `push` defaulting to all staged actions in selected account.
- [ ] Implement `push --dry-run`.
- [ ] Implement `push --message <id>`.
- [ ] Implement `push --selection <name>`.
- [ ] Implement explicit `--yes` for deliberate scripted use.
- [ ] Prohibit `push --all-accounts` structurally.
- [ ] Human preview distinguishes staged read/delete and threaded-delete risk.
- [ ] Stable `--json` preview/result schema.

## Preflight

- [ ] Integrity validate affected branches first.
- [ ] Fetch only lightweight current provider state required for safe/idempotent operation.
- [ ] No full content/thread re-normalization during push.
- [ ] Missing provider object resolves locally with audit/warning.

## Gmail mutations

- [ ] Staged read => remove `UNREAD` when needed.
- [ ] Already read => success/no-op.
- [ ] Staged delete => move exactly one message to Trash.
- [ ] Already read does not cancel delete intent.
- [ ] Already in Trash => success/no-op.
- [ ] Permanent deletion unsupported.
- [ ] No whole-thread delete API.

## Threaded delete safeguard

- [ ] Detect staged-delete message in multi-message thread.
- [ ] Separate them in preview.
- [ ] Require additional confirmation after normal review.
- [ ] Ensure `--yes` semantics are explicit in docs and skills prohibit autonomous harness use.

## Resilience

- [ ] Bounded concurrent operations.
- [ ] Retry transient network/429/5xx with backoff.
- [ ] Auth failure stops clearly.
- [ ] Permanent per-message failure remains staged.
- [ ] Successful independent operations are committed/cleaned locally; no fake rollback.
- [ ] Partial push leaves account pull-blocked while any staged actions remain.

## Cleanup/audit

- [ ] Remove successful work items.
- [ ] Trigger selection pruning and reachability GC as appropriate.
- [ ] Write content-redacted audit events.
- [ ] Track last successful push metadata.

## Tests

- [ ] Read idempotency.
- [ ] Trash idempotency.
- [ ] Missing-message behavior.
- [ ] Partial failures remain staged.
- [ ] Threaded-delete double-confirmation path.
- [ ] Partial push by message/selection does not broaden scope.
- [ ] `--all-accounts` rejected.
- [ ] Integrity mismatch prevents all provider mutation for affected scope.

## Exit criteria

- [ ] Provider mutation surface contains only read + Trash.
- [ ] Default human flow cannot accidentally bypass preview/confirmation.
