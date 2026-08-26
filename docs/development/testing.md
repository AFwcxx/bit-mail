# Testing strategy

## 1. Core tests

Core repository/pull/push/work-item/selection/Knowledge/integrity behavior targets a fake `MailProvider` so tests are deterministic and offline.

## 2. Gmail REST contract tests

Use a local mock HTTP server with injectable Gmail API base URL. Cover at minimum:

- account profile;
- message seed pagination;
- history cursor behavior and expired cursor fallback;
- full thread retrieval;
- embedded attachment bytes;
- remote attachment IDs;
- rate limiting / `Retry-After`;
- network/5xx retry;
- malformed provider responses;
- missing messages;
- mark-read idempotency;
- Trash idempotency;
- partial pull and partial push failures.

## 3. Live integration tests

Optional only, explicitly enabled, dedicated test mailbox. They do not run in normal `cargo test` or standard CI and must never operate on arbitrary inbox contents.

## 4. Performance tests

Benchmark:

- BLAKE3 hashing across many small files;
- large attachment hashing/mmap thresholds;
- Merkle branch updates;
- repository discovery;
- structural SQLite lookup;
- pull materialization throughput;
- large full-thread normalization;
- bounded push concurrency.

Do not choose integrity concurrency thresholds by assumption; use measured crossover points.
