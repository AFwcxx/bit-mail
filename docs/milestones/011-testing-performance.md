# M011 — Testing and performance

**Status:** Planned  
**Depends on:** Functional milestones substantially complete  
**Outcome:** Confidence that large inboxes remain correct and fast.

## Core/provider tests

- [ ] Fake-provider end-to-end pull/stage/push lifecycle tests.
- [ ] Gmail REST mock server contract suite complete.
- [ ] No normal test requires Gmail credentials/network.
- [ ] Optional live Gmail tests behind explicit opt-in and dedicated test mailbox.
- [ ] Live tests create/control their own test messages and never inspect arbitrary inbox mail.

## Property/invariant tests

- [ ] Canonical message stored once per account.
- [ ] Thread manifests reference canonical IDs only.
- [ ] Context-only messages never become stageable accidentally.
- [ ] Pull cannot run with staged intent.
- [ ] Push never broadens explicit scope.
- [ ] Account locks never block unrelated accounts.
- [ ] Cache rebuild preserves stable message UUID mapping.
- [ ] Selection pruning/reachability GC invariants.
- [ ] Integrity tree deterministic across runs/platforms for canonical encodings.

## Performance benchmarks

- [ ] Repository with tens/hundreds of thousands of small files/work items fixture generator.
- [ ] Startup/repository discovery latency.
- [ ] `status` / `work-items` structural lookup latency.
- [ ] BLAKE3 hashing across many small files.
- [ ] Merkle branch update/verification latency.
- [ ] `doctor --full` throughput.
- [ ] Large attachment hashing/mmap/internal parallel crossover.
- [ ] Large Gmail-like thread materialization/normalization.
- [ ] Bounded pull concurrency.
- [ ] Bounded push concurrency.
- [ ] SQLite rebuild performance.

## Optimization policy

- [ ] Optimize measured bottlenecks only.
- [ ] Preserve full context/accuracy rather than truncating thread content.
- [ ] Prefer scoped integrity verification over weakening integrity.
- [ ] Document chosen concurrency defaults and benchmark rationale.

## Security/regression

- [ ] Prompt-injection content fixture cannot become trusted CLI instruction through framework output.
- [ ] Path traversal/unsafe attachment filenames.
- [ ] Secret/content redaction regression tests.
- [ ] Git/private-path and permission diagnostics tests.

## Exit criteria

- [ ] Common interactive commands remain subjectively and measurably fast on a large synthetic repository.
- [ ] Performance results support BLAKE3/Merkle concurrency choices with evidence.
