# M007 — Integrity and recovery

**Status:** Complete
**Depends on:** M004, M006  
**Outcome:** Fast scoped BLAKE3/Merkle integrity with provider-based recovery and cache lifecycle tools.

## BLAKE3/Merkle model

- [x] Define canonical domain-separated hashing encodings.
- [x] Define deterministic child ordering.
- [x] Define leaf/object/account/repository roots.
- [x] Define integrity storage schema and version.
- [x] Exclude SQLite/locks/temp state.
- [x] Include canonical/persistent managed files, Knowledge, config, audit, and trusted runtime skills/templates as applicable.
- [x] Implement incremental branch update after framework mutation.

## Scoped validation

- [x] Validate only affected branches before sensitive mutation.
- [x] Keep normal read commands free of repository-wide hashing.
- [x] Localize mismatch to object/file for diagnostics.
- [x] Detect missing and unexpected files in managed canonical directories as appropriate.
- [x] Expose the fail-closed affected-scope validation seam used by M008 push.

## Performance

- [x] Implement small-file hashing optimized for many independent files.
- [x] Parallelize across independent files/objects with bounded worker count.
- [x] Evaluate mmap/internal BLAKE3 parallel path for large attachments through benchmarks.
- [x] Record benchmark results/threshold policy in docs.

## Repair

- [x] Implement `repair <message-id>` provider-backed thread repair.
- [x] Acquire account lock.
- [x] Re-fetch complete authoritative provider thread.
- [x] Rebuild normalized cache and Merkle branch.
- [x] Invalidate staged decisions in affected thread.
- [x] Recreate `pending` only for provider-current `INBOX + UNREAD` work items.
- [x] Never change provider state merely to recreate previous local state.
- [x] Audit repair event.

## Garbage collection

- [x] Implement reference/reachability-driven thread cache cleanup.
- [x] `gc --dry-run`.
- [x] `gc`.
- [x] Preserve complete thread context while any actionable work item remains.
- [x] Remove canonical messages/attachments/thread manifests when unreachable.
- [x] Prune selection members as work items disappear.

## Cache rebuild

- [x] Implement account-scoped `cache rebuild`.
- [x] Refuse if staged read/delete exists.
- [x] Preserve account/config/credentials references/Knowledge/audit/stable message identity registry.
- [x] Discard provider-derived cache, pending work items, selections, cursors, SQLite, related Merkle cache state.
- [x] Next pull reuses stable message UUID mapping.

## Full validation

- [x] Implement `doctor --full` integrity scan/rebuild/compare seam (diagnostic UI may complete M010).

## Tests

- [x] Tampered `content.md` detection through the M008 push-preflight seam.
- [x] Tampered work item/selection/Knowledge detection.
- [x] Missing/unexpected file detection.
- [x] Scoped validation does not read unrelated account branches.
- [x] Repair resets affected staged intent correctly.
- [x] GC retains shared thread until final work item resolves.
- [x] Cache rebuild preserves UUID identities/Knowledge/audit.

## Exit criteria

- [x] Integrity protection does not make ordinary reads sluggish.
- [x] Provider truth can recover corrupted provider-derived cache without manual file surgery.

## Progress log

### 2026-08-30

Implemented schema-v1 domain-separated BLAKE3 manifests, bounded account-scoped
validation, mutation refreshes, full-scan/rebuild seams, provider-backed thread
repair, deterministic GC/dry-run, and identity-preserving cache rebuild. Added
public CLI routing and lightweight provider message/thread lookup. Benchmarking
confirmed mmap parallel hashing was faster for 64 MiB files but rejected it for
production because concurrent truncation can fault mapped input. Formatting,
strict Clippy, and all-target tests pass.

### 2026-08-30 remediation

Added explicit retryable repository-v2 integrity migration, made missing
post-migration manifests fail closed, made repair independent of corrupt cache,
completed GC across provider-derived representations, and retained removed
account Knowledge under orphan integrity roots. Provider-mutation wiring remains
owned by M008. Cache rebuild validates the work-item branch before trusting that
staged intent is absent while still permitting corrupted disposable cache.
