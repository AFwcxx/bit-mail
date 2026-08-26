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

## Failure

Integrity mismatch before provider mutation => fail closed, no Gmail mutation.

A corrupted message in a thread triggers thread-level repair from authoritative provider content and invalidates staged decisions for actionable messages in that repaired thread.
