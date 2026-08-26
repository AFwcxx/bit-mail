# M004 — Canonical mail storage

**Status:** Planned  
**Depends on:** M002, M003 provider DTO/test seam  
**Outcome:** Provider-independent canonical message/thread representation and deterministic normalization.

## Domain schemas

- [ ] Define versioned stable `metadata.json` schema.
- [ ] Define deterministic `content.md` generation contract.
- [ ] Define internal provider-specific message representation location/schema.
- [ ] Define thread manifest schema referencing canonical message UUIDs only.
- [ ] Define durable message identity registry mapping provider IDs to stable UUIDv7.
- [ ] Define attachment manifest/internal metadata model.
- [ ] Define parse/normalization diagnostics schema.

## Normalization

- [ ] Select and validate maintained Rust MIME parser.
- [ ] Implement charset/MIME/transfer decoding deterministically.
- [ ] Prefer useful text/plain, otherwise deterministic HTML -> Markdown/readable text.
- [ ] Do not fetch remote HTML resources.
- [ ] Preserve quoted history/signatures/footers unless clearly non-content and removal is lossless; default to preservation.
- [ ] Preserve links.
- [ ] Handle malformed/partial MIME with fail-preserving diagnostics.
- [ ] Never silently drop a provider message due to parsing failure.

## Threads

- [ ] Materialize each provider message exactly once per account.
- [ ] Reuse canonical message across all relationships.
- [ ] Build complete thread manifest in provider conversation order.
- [ ] Include read/archived/Sent context messages when they belong to actionable thread.
- [ ] Ensure context-only messages have no workflow state.
- [ ] No v1 thread-size truncation.

## Attachments

- [ ] Persist attachment bytes already delivered by provider.
- [ ] Preserve metadata/reference for remote-only attachment parts.
- [ ] Implement idempotent local attachment-path mapping without unsafe filenames/path traversal.
- [ ] Design `attachment fetch` seam for M005/provider work.
- [ ] Ensure fetched attachment is tied to canonical message lifecycle.

## Raw

- [ ] Define explicit `raw fetch` behavior/location if included in v1 command surface.
- [ ] Ensure raw source is not required for ordinary correctness.

## Index

- [ ] Define disposable SQLite structural schema.
- [ ] Index identity/thread/work-item/path relationships only; no generic FTS requirement.
- [ ] Implement rebuild-from-canonical/internal-persistent-state strategy.

## Tests

- [ ] Multipart plain/HTML fixtures.
- [ ] Encodings and malformed message fixtures.
- [ ] Thread with Sent/read/archived/unread messages.
- [ ] Duplicate canonical storage prevention.
- [ ] Embedded vs remote attachment fixtures.
- [ ] Unsafe filename/path tests.
- [ ] Stable message UUID across cache re-materialization.

## Exit criteria

- [ ] Harness-facing `data/` contains only stable supported representations.
- [ ] Framework/provider internals are not leaked into stable harness contract.
- [ ] Full thread can be reconstructed without duplicate stored message content.
