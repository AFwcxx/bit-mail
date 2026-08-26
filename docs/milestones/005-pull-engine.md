# M005 — Pull engine

**Status:** Planned  
**Depends on:** M003, M004  
**Outcome:** Robust bounded/incremental Gmail -> local synchronization for unread triage with complete context.

## Gmail REST client

- [ ] Implement narrow `reqwest`/`serde` Gmail REST client/DTOs.
- [ ] Injectable base URL for contract tests.
- [ ] Redacted request logging.
- [ ] Timeouts, bounded retry/backoff, `Retry-After`, 429 and 5xx handling.
- [ ] Authentication error mapping.

## Seed/backlog discovery

- [ ] Query `INBOX + UNREAD` seeds regardless of Gmail category.
- [ ] Newest-first bounded default pull.
- [ ] `--limit` seed semantics.
- [ ] `--all` semantics.
- [ ] Track unread-backlog checkpoint independently from Gmail history.
- [ ] Deduplicate by provider message identity/stable identity registry.

## Incremental reconciliation

- [ ] Persist/use Gmail `historyId` cursor.
- [ ] Handle history changes to locally known messages.
- [ ] Handle expired/invalid history cursor with documented full reconciliation fallback.
- [ ] Reconcile pending local work item removal when provider message becomes read/Trash/removed from Inbox externally.
- [ ] Preserve staged state rule by refusing pull before provider fetch if staged read/delete exists.

## Full threads

- [ ] For each seed, fetch complete Gmail thread.
- [ ] Materialize all thread messages exactly once.
- [ ] Create work item for every `INBOX + UNREAD` message discovered in fetched thread, even beyond seed limit.
- [ ] Include Sent/read/archived context automatically.
- [ ] No thread-content truncation.

## Atomic materialization

- [ ] Fetch/normalize/materialize in temp area.
- [ ] Build structural relationships/index/integrity before publication.
- [ ] Publish fully valid thread set atomically as feasible.
- [ ] Never expose half-materialized canonical objects.
- [ ] Advance history/backlog checkpoints conservatively so failures remain retryable.

## Multi-account

- [ ] Implement `pull --all-accounts` independent processing.
- [ ] Skip/report staged blocked accounts; continue clean accounts.
- [ ] Account locks remain independent.

## Output

- [ ] Human summary: seeds, threads, additional unread discovered, new work items, failures/retries, backlog.
- [ ] Stable `--json` output.

## Tests

- [ ] Mock HTTP pagination/history/full-thread flows.
- [ ] Seed-limit overflow by additional unread in thread.
- [ ] Expired history fallback.
- [ ] Partial thread fetch failure does not publish partial state.
- [ ] Checkpoint conservatism on failure.
- [ ] Pull blocking when staged state exists.
- [ ] Multi-account skip/continue behavior.

## Exit criteria

- [ ] Large unread inbox can be incrementally processed without repeatedly downloading the same newest seed page.
- [ ] Complete context is always available for materialized actionable threads.
