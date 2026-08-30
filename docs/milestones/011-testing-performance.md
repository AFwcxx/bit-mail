# M011 — Testing and performance

**Status:** Complete
**Depends on:** Functional milestones substantially complete  
**Outcome:** Confidence that large inboxes remain correct and fast.

## Core/provider tests

- [x] Fake-provider end-to-end pull/stage/push lifecycle tests.
- [x] Gmail REST mock server contract suite complete.
- [x] No normal test requires Gmail credentials/network.
- [x] Optional live Gmail tests behind explicit opt-in and dedicated test mailbox.
- [x] Live tests create/control their own test messages and never inspect arbitrary inbox mail.

## Property/invariant tests

- [x] Canonical message stored once per account.
- [x] Thread manifests reference canonical IDs only.
- [x] Context-only messages never become stageable accidentally.
- [x] Pull cannot run with staged intent.
- [x] Push never broadens explicit scope.
- [x] Account locks never block unrelated accounts.
- [x] Cache rebuild preserves stable message UUID mapping.
- [x] Selection pruning/reachability GC invariants.
- [x] Integrity tree deterministic across runs/platforms for canonical encodings.

## Performance benchmarks

- [x] Repository with tens/hundreds of thousands of small files/work items fixture generator.
- [x] Startup/repository discovery latency.
- [x] `status` / `work-items` structural lookup latency.
- [x] BLAKE3 hashing across many small files.
- [x] Merkle branch update/verification latency.
- [x] `doctor --full` throughput.
- [x] Large attachment hashing/mmap/internal parallel crossover.
- [x] Large Gmail-like thread materialization/normalization.
- [x] Bounded pull concurrency.
- [x] Bounded push concurrency.
- [x] SQLite rebuild performance.

## Optimization policy

- [x] Optimize measured bottlenecks only.
- [x] Preserve full context/accuracy rather than truncating thread content.
- [x] Prefer scoped integrity verification over weakening integrity.
- [x] Document chosen concurrency defaults and benchmark rationale.

## Security/regression

- [x] Prompt-injection content fixture cannot become trusted CLI instruction through framework output.
- [x] Path traversal/unsafe attachment filenames.
- [x] Secret/content redaction regression tests.
- [x] Git/private-path and permission diagnostics tests.

## Exit criteria

- [x] Common interactive commands remain subjectively and measurably fast on a large synthetic repository.
- [x] Performance results support BLAKE3/Merkle concurrency choices with evidence.

## Progress log

- 2026-08-31: Closed the final verification gaps with real subprocess startup
  measurement and Gmail-backed mock-server partial pull/push lifecycle tests.
  Revalidated both 100K interactive lookups below the 500 ms gate.
- 2026-08-30: Added complete fake-provider lifecycle, Gmail REST contract,
  invariant, bounded-concurrency, controlled live-Gmail, and security regression
  coverage. Restored the required offline `status` command and truthful help
  capability metadata.
- 2026-08-30: Added Criterion fixtures and benchmarks through 100,000 work
  items. Measurement found repeated sequential parsing and quadratic thread
  lookup; bounded four-worker parsing plus a single deterministic context map
  brought 100K status/work-items to 105/344 ms without changing persistent
  formats or weakening context/integrity. Recorded full benchmark evidence and
  retained the safe buffered hashing policy.
