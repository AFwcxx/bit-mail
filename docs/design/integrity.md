# Integrity design

## Goal

Detect accidental/out-of-band modification of managed local state before `bit-mail` trusts that state for sensitive operations, especially `push`.

This is integrity/error detection, not protection against malicious same-user code capable of rewriting both content and integrity metadata.

## Algorithm

Use BLAKE3 throughout v1.

BLAKE3 provides fast 256-bit digests and is internally tree-structured for a byte stream. `bit-mail` additionally builds an application-level hierarchical Merkle tree over files and domain objects.

## Hierarchy

Illustrative structure:

```text
RepositoryRoot
├── GlobalKnowledgeRoot
└── AccountsRoot
    ├── AccountRoot A
    │   ├── Message roots
    │   ├── Thread roots
    │   ├── Work-item roots
    │   ├── Selection roots
    │   ├── Account Knowledge root
    │   └── Audit/config/provider-state roots
    └── AccountRoot B
```

SQLite, locks, and temporary/disposable state are excluded.

## Canonical parent hashing

Parent hashes must use:

- deterministic child ordering;
- canonical encoding;
- object identity/path where required;
- domain separation.

Example domains:

```text
bit-mail:file:v1
bit-mail:message:v1
bit-mail:thread:v1
bit-mail:work-item:v1
bit-mail:selection:v1
bit-mail:knowledge:v1
bit-mail:account:v1
bit-mail:repository:v1
```

Schema version 1 frames every domain, path/identity, and digest field as an
unsigned 64-bit big-endian byte length followed by the raw bytes. File leaves
also frame the repository-relative slash-separated UTF-8 path and declared byte
length before streaming the contents. Parent entries are sorted by path bytes.

Detailed account manifests live at
`.bit-mail/accounts/<account-uuid>/integrity/manifest.json`; repository/config
and global-Knowledge manifests live below `.bit-mail/integrity/`. The
repository root is derived from independently locked branch roots rather than
stored as a shared mutable file. This preserves unrelated-account concurrency.

Integrity metadata, locks, staging/temporary files, and SQLite are excluded.
Pre-M007 repository-schema-v1 repositories establish their first baseline only
through explicit `bit-mail migrate-integrity`. Migration advances repository
metadata to schema v2 and is retryable after interruption. Mutations refuse v1,
and missing manifests after v2 activation fail closed.

## Validation strategy

Do not hash the entire repository for normal commands.

- read-only commands should remain fast;
- a scoped mutation validates only objects/branches it relies on;
- `push --selection X` validates X plus affected work/message/thread branches;
- `doctor --full` explicitly performs a complete byte-level scan/rebuild/compare.

Merkle aggregation avoids hashing unrelated branches, but it does not eliminate the need to read a leaf file when proving that leaf has not changed outside `bit-mail`.

## Performance

Most metadata/content files are small. Prefer ordinary single-thread BLAKE3 per small file and parallelize across many independent files/objects.

For large attachments, mmap/internal parallel hashing may be used only when benchmarks demonstrate a benefit. Performance thresholds must be benchmark-driven rather than guessed.

The 2026-08-30 release-mode benchmark hashed 64 MiB five times: 128 KiB buffered
streaming took 117.68 ms and mmap plus BLAKE3 internal parallelism took 15.10
ms. mmap was not selected despite the speedup because concurrent out-of-band
truncation can fault a mapped file; deterministic error reporting is preferable
to that process-safety risk. The reproducible ignored benchmark remains in the
integrity tests. Production uses bounded four-worker cross-file parallelism and
buffered per-file hashing.

## Failure

Integrity mismatch before provider mutation => fail closed, no Gmail mutation.

A corrupted message in a thread triggers thread-level repair from authoritative provider content and invalidates staged decisions for actionable messages in that repaired thread.
Repair does not reuse disposable cached metadata or attachment bytes. GC
computes reachability across canonical messages, provider records, raw source,
diagnostics, and manifests while preserving stable identities.
