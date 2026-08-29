# M004 — Canonical mail storage

**Status:** Complete
**Depends on:** M002, M003 account/provider identity seam; M005 owns Gmail REST DTOs
**Outcome:** Provider-independent canonical message/thread representation and deterministic normalization.

## Domain schemas

- [x] Define versioned stable `metadata.json` schema.
- [x] Define deterministic `content.md` generation contract.
- [x] Define internal provider-specific message representation location/schema.
- [x] Define thread manifest schema referencing canonical message UUIDs only.
- [x] Define durable message identity registry mapping provider IDs to stable UUIDv7.
- [x] Define attachment manifest/internal metadata model.
- [x] Define parse/normalization diagnostics schema.

## Normalization

- [x] Select and validate maintained Rust MIME parser.
- [x] Implement charset/MIME/transfer decoding deterministically.
- [x] Prefer useful text/plain, otherwise deterministic HTML -> Markdown/readable text.
- [x] Do not fetch remote HTML resources.
- [x] Preserve quoted history/signatures/footers unless clearly non-content and removal is lossless; default to preservation.
- [x] Preserve links.
- [x] Handle malformed/partial MIME with fail-preserving diagnostics.
- [x] Never silently drop a provider message due to parsing failure.

## Threads

- [x] Materialize each provider message exactly once per account.
- [x] Reuse canonical message across all relationships.
- [x] Build complete thread manifest in provider conversation order.
- [x] Include read/archived/Sent context messages when they belong to actionable thread.
- [x] Ensure context-only messages have no workflow state.
- [x] No v1 thread-size truncation.

## Attachments

- [x] Persist attachment bytes already delivered by provider.
- [x] Preserve metadata/reference for remote-only attachment parts.
- [x] Implement idempotent local attachment-path mapping without unsafe filenames/path traversal.
- [x] Design `attachment fetch` seam for M005/provider work.
- [x] Ensure fetched attachment is tied to canonical message lifecycle.

## Raw

- [x] Define explicit `raw fetch` behavior/location if included in v1 command surface.
- [x] Ensure raw source is not required for ordinary correctness.

## Index

- [x] Define disposable SQLite structural schema.
- [x] Index identity/thread/path and attachment-locality relationships; M006 owns work-item/selection indexing with their persistent schemas.
- [x] Implement rebuild-from-canonical/internal-persistent-state strategy.

## Tests

- [x] Multipart plain/HTML fixtures.
- [x] Encodings and malformed message fixtures.
- [x] Thread with Sent/read/archived/unread messages.
- [x] Duplicate canonical storage prevention.
- [x] Embedded vs remote attachment fixtures.
- [x] Unsafe filename/path tests.
- [x] Stable message UUID across cache re-materialization.

## Exit criteria

- [x] Harness-facing `data/` contains only stable supported representations.
- [x] Framework/provider internals are not leaked into stable harness contract.
- [x] Full thread can be reconstructed without duplicate stored message content.

## Progress log

### 2026-08-29

Implemented provider-neutral message/MIME inputs, deterministic canonical
normalization, UUIDv7 identity persistence, reference-only thread manifests,
safe local/remote attachment handling with idempotent fetched-byte persistence,
content-redacted diagnostics, internal-only staging, an attachment persistence
seam and raw storage-location contract for M005, and an atomically rebuilt
structural SQLite index. Gmail REST DTOs and
provider-backed attachment/raw commands remain correctly owned by M005;
work-item/selection persistence and derived indexing remain owned by M006.
Formatting, check, Clippy, and all-target tests passed.
