# Performance evidence

M011 uses Criterion release-mode benchmarks in `benches/m011.rs`. Fixtures are
temporary, synthetic, content-free repositories. `BIT_MAIL_BENCH_ITEMS` controls
work-item count, `BIT_MAIL_BENCH_CANONICAL` controls canonical message/thread
count, and both accept `100000` for the large-repository gate.

## 2026-08-30 baseline

Environment: Rust 1.94.0, x86_64 Linux 7.1.10, Intel Core Ultra 7 265H, 16
online cores. Values below are Criterion median estimates.

| Workload | Fixture | Median | Throughput |
|---|---:|---:|---:|
| Process startup | `bit-mail --version` | 940 us | full subprocess |
| Repository discovery | 100,000 work items | 16.8 us | independent of repository size |
| Status aggregation | 10,000 / 100,000 work items | 12.5 ms / 105.4 ms | 949 K items/s at 100K |
| Structural work-items | 10,000 / 100,000 canonical items | 35.7 ms / 343.7 ms | 291 K items/s at 100K |
| SQLite rebuild | 10,000 canonical items | 283.8 ms | 35.2 K items/s |
| Account Merkle verification | 10,000 canonical items | 574.7 ms | 17.4 K items/s |
| `doctor --full` | 10,000 canonical items | 1.501 s | 6.66 K items/s |
| One-work-item Merkle branch update | one item | 1.70 ms | scoped mutation |
| Gmail-like thread materialization | 100 x 16 KiB bodies | 49.8 ms | 2.01 K messages/s |
| Bounded pull / push | eight operations | 34.8 ms / 12.7 ms | four workers maximum |
| 64 MiB buffered / mmap-parallel BLAKE3 | 64 MiB | 38.3 ms / 3.79 ms | 1.63 / 16.5 GiB/s |

The 10K-to-100K ratios are 8.4x for status and 9.6x for work-items. Both
100K interactive structural lookups remain below the local 500 ms gate. Full
scans/rebuilds scale near-linearly: the 1K-to-10K ratios were 9.9x for SQLite,
8.1x for Merkle verification, and 8.4x for `doctor --full`.

The 2026-08-31 revalidation measured 100K status at 106.5 ms (939 K items/s)
and work-items at 372.2 ms (269 K items/s). Criterion reported 1.7% and 8.3%
regressions from the baseline; both remain below the 500 ms gate.

Initial measurement exposed repeated sequential file parsing and an O(n x n)
thread-manifest lookup. The validated optimization reads work-item and thread
manifests through at most four workers, builds one deterministic context map,
and restores sorted results. It reduced 100K status from 439.7 ms to 105.4 ms
and 10K work-items from 129.1 ms to 35.7 ms without changing canonical formats
or depending on SQLite correctness.

Production retains four-worker cross-file, pull, and push concurrency. Buffered
hashing remains the safe large-attachment path: mmap is faster, but concurrent
file truncation can fault the process, so speed does not outweigh deterministic
error handling. Full context is never truncated and scoped verification is not
weakened.

Run the complete suite with `cargo bench --bench m011`. Reproduce the large
interactive gate with:

```bash
BIT_MAIL_BENCH_ITEMS=100000 BIT_MAIL_BENCH_CANONICAL=100000 \
cargo bench --bench m011 -- \
  'repository_structural/status|canonical_structural/work_items'
```

CI compiles all benchmark targets through strict all-target Clippy and tests,
but does not enforce wall-clock limits across heterogeneous runners.
