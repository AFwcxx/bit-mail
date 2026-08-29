# M005 — Pull engine

**Status:** Complete
**Depends on:** M003, M004  
**Outcome:** Robust bounded/incremental Gmail -> local synchronization for unread triage with complete context.

## Gmail REST client

- [x] Implement narrow `reqwest`/`serde` Gmail REST client/DTOs.
- [x] Injectable base URL for contract tests.
- [x] Redacted request logging.
- [x] Timeouts, bounded retry/backoff, `Retry-After`, 429 and 5xx handling.
- [x] Authentication error mapping.

## On-demand provider content

- [x] Implement `attachment fetch <message-id> <part-id>` using M004 locality checks and persistence; skip provider I/O when already local.
- [x] Implement `raw fetch <message-id>` using M004's internal raw location contract; raw source remains optional for ordinary correctness.
- [x] Keep provider attachment/raw IDs and payloads behind the adapter boundary.

## Seed/backlog discovery

- [x] Query `INBOX + UNREAD` seeds regardless of Gmail category.
- [x] Newest-first bounded default pull.
- [x] `--limit` bounds backlog seeds while history reconciliation remains exhaustive.
- [x] `--all` semantics.
- [x] Track unread-backlog checkpoint independently from Gmail history.
- [x] Deduplicate by provider message identity/stable identity registry.

## Incremental reconciliation

- [x] Persist/use Gmail `historyId` cursor.
- [x] Handle history changes to locally known messages.
- [x] Handle expired/invalid history cursor with documented full reconciliation fallback.
- [x] Reconcile pending local work item removal when provider message becomes read/Trash/removed from Inbox externally.
- [x] Preserve staged state rule by refusing pull before provider fetch if staged read/delete exists.

## Full threads

- [x] For each seed, fetch complete Gmail thread.
- [x] Materialize all thread messages exactly once.
- [x] Create work item for every `INBOX + UNREAD` message discovered in fetched thread, even beyond seed limit.
- [x] Include Sent/read/archived context automatically.
- [x] No thread-content truncation.

## Atomic materialization

- [x] Fetch/normalize/materialize in temp area.
- [x] Validate complete thread input and stage each canonical message before publication; M007 owns cryptographic integrity.
- [x] Publish each canonical object atomically under the account lock; interrupted multi-message publication remains retryable.
- [x] Never expose half-written canonical message objects.
- [x] Publish thread manifest/index before new work items and advance checkpoints last.
- [x] Advance history/backlog checkpoints conservatively so failures remain retryable.

## Multi-account

- [x] Implement `pull --all-accounts` independent processing.
- [x] Skip/report staged blocked accounts; continue clean accounts.
- [x] Account locks remain independent.

## Output

- [x] Human summary: seeds, threads, additional unread discovered, new work items, failures/retries, backlog.
- [x] Stable `--json` output.

## Tests

- [x] Mock HTTP pagination/history/full-thread flows.
- [x] Seed-limit overflow by additional unread in thread.
- [x] Expired history fallback.
- [x] Partial thread fetch failure does not publish partial state.
- [x] Checkpoint conservatism on failure.
- [x] Pull blocking when staged state exists.
- [x] Multi-account skip/continue behavior.

## Exit criteria

- [x] Large unread inbox can be incrementally processed without repeatedly downloading the same newest seed page.
- [x] Complete context is always available for materialized actionable threads.

## Progress log

### 2026-08-29

Implemented the narrow Gmail REST adapter, bounded retries, access-token refresh,
complete-thread mapping, incremental history and unread-backlog checkpoints,
pending work-item creation/reconciliation, conservative publication, multi-account
pull reporting, and idempotent attachment/raw fetch commands. M006 retains the
offline triage command surface; M007 retains BLAKE3 integrity. Formatting, check,
strict Clippy, and all-target tests passed.

Closed review findings by making completed backlogs history-only, refreshing
known inactive messages from their complete thread, adding ID-free request
diagnostics, testing multi-account continuation, and documenting the v1
per-object atomic publication guarantee.
