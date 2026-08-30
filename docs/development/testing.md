# Testing strategy

## Conventions

Keep focused unit tests beside their implementation in `#[cfg(test)]` modules. Put shared integration-test setup in `tests/common/`, adding helpers only when an integration test consumes them.

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

The live Gmail lifecycle test is compiled with `live-gmail-tests`, remains
ignored, verifies the authenticated mailbox before creating data, inserts its
own uniquely identified unread message, and only reads/modifies/Trashes the ID
returned by that insertion. Run it deliberately with a short-lived access
token for the named dedicated mailbox:

```bash
BIT_MAIL_LIVE_GMAIL_CONFIRM=I_UNDERSTAND_THIS_USES_A_DEDICATED_MAILBOX \
BIT_MAIL_LIVE_GMAIL_ACCOUNT=test-mailbox@example.com \
BIT_MAIL_LIVE_GMAIL_ACCESS_TOKEN=... \
cargo test --features live-gmail-tests --test live_gmail \
  controlled_message_lifecycle_never_lists_arbitrary_mail -- --ignored --exact
```

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

See [performance.md](performance.md) for fixture controls, reproducible commands,
current measurements, and optimization decisions.
