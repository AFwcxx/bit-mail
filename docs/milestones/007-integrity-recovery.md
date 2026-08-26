# M007 — Integrity and recovery

**Status:** Planned  
**Depends on:** M004, M006  
**Outcome:** Fast scoped BLAKE3/Merkle integrity with provider-based recovery and cache lifecycle tools.

## BLAKE3/Merkle model

- [ ] Define canonical domain-separated hashing encodings.
- [ ] Define deterministic child ordering.
- [ ] Define leaf/object/account/repository roots.
- [ ] Define integrity storage schema and version.
- [ ] Exclude SQLite/locks/temp state.
- [ ] Include canonical/persistent managed files, Knowledge, config, audit, and trusted runtime skills/templates as applicable.
- [ ] Implement incremental branch update after framework mutation.

## Scoped validation

- [ ] Validate only affected branches before sensitive mutation.
- [ ] Keep normal read commands free of repository-wide hashing.
- [ ] Localize mismatch to object/file for diagnostics.
- [ ] Detect missing and unexpected files in managed canonical directories as appropriate.
- [ ] Fail provider mutation closed on integrity mismatch.

## Performance

- [ ] Implement small-file hashing optimized for many independent files.
- [ ] Parallelize across independent files/objects with bounded worker count.
- [ ] Evaluate mmap/internal BLAKE3 parallel path for large attachments through benchmarks.
- [ ] Record benchmark results/threshold policy in docs.

## Repair

- [ ] Implement `repair <message-id>` provider-backed thread repair.
- [ ] Acquire account lock.
- [ ] Re-fetch complete authoritative provider thread.
- [ ] Rebuild normalized cache and Merkle branch.
- [ ] Invalidate staged decisions in affected thread.
- [ ] Recreate `pending` only for provider-current `INBOX + UNREAD` work items.
- [ ] Never change provider state merely to recreate previous local state.
- [ ] Audit repair event.

## Garbage collection

- [ ] Implement reference/reachability-driven thread cache cleanup.
- [ ] `gc --dry-run`.
- [ ] `gc`.
- [ ] Preserve complete thread context while any actionable work item remains.
- [ ] Remove canonical messages/attachments/thread manifests when unreachable.
- [ ] Prune selection members as work items disappear.

## Cache rebuild

- [ ] Implement account-scoped `cache rebuild`.
- [ ] Refuse if staged read/delete exists.
- [ ] Preserve account/config/credentials references/Knowledge/audit/stable message identity registry.
- [ ] Discard provider-derived cache, pending work items, selections, cursors, SQLite, related Merkle cache state.
- [ ] Next pull reuses stable message UUID mapping.

## Full validation

- [ ] Implement `doctor --full` integrity scan/rebuild/compare seam (diagnostic UI may complete M010).

## Tests

- [ ] Tampered `content.md` detection before push.
- [ ] Tampered work item/selection/Knowledge detection.
- [ ] Missing/unexpected file detection.
- [ ] Scoped validation does not read unrelated account branches.
- [ ] Repair resets affected staged intent correctly.
- [ ] GC retains shared thread until final work item resolves.
- [ ] Cache rebuild preserves UUID identities/Knowledge/audit.

## Exit criteria

- [ ] Integrity protection does not make ordinary reads sluggish.
- [ ] Provider truth can recover corrupted provider-derived cache without manual file surgery.
