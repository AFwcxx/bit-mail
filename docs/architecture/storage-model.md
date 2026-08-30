# Storage model

## Principle

Store each provider message exactly once per account. Relationships reference canonical IDs and never duplicate message content.

## Harness-facing data

```text
data/<account-uuid>/messages/<message-uuid>/
├── content.md
├── metadata.json
└── attachments/
```

`content.md` and `metadata.json` are stable harness-facing formats. Attachments are present when already received or explicitly fetched.

`metadata.json` schema version 1 contains the canonical message UUID,
received/sent Unix-millisecond timestamps, decoded subject and RFC Message-ID,
structured address lists, provider-independent Inbox/unread/Sent/Trash flags,
attachment descriptors, and `complete` or `partial` normalization status.
Attachment descriptors expose a stable MIME part ID, original filename, media
type, size, locality, and a relative path only when bytes are local. Provider
message/thread/attachment IDs never appear here.

`data/` is readable but framework-managed. Harnesses must not write it.

## Internal provider/framework state

```text
.bit-mail/accounts/<account-uuid>/
├── account.toml
├── provider/
├── provider-state.json
├── identities/
├── threads/
├── work-items/
├── selections/
├── integrity/
├── audit/
└── index.sqlite

.bit-mail/locks/
├── account-lifecycle.lock
├── accounts/<account-uuid>.lock
└── knowledge.lock
```

Each account's schema-v1 integrity manifest records sorted repository-relative
paths, BLAKE3 file digests, and the account root. Repository configuration and
global Knowledge use separate manifests below `.bit-mail/integrity/`; a full
validation derives the repository root from those independently locked roots.
Knowledge retained after account removal has an orphan branch below
`.bit-mail/integrity/orphaned-knowledge/` and remains in full validation.

Transient locks remain outside deletable account state so account removal cannot invalidate a held lock.

Provider-specific source records live at
`provider/messages/<message-uuid>.json`; raw source, when M005 implements the
provider-backed command, lives at `provider/raw/<message-uuid>.eml`. Remote
attachment IDs and content-redacted normalization diagnostics remain internal
and non-contractual for AI harnesses. Raw source is never required for normal
materialization or correctness. Materialization staging also stays below the
account's internal state, so temporary or crash-leftover objects never appear
in harness-facing `data/`.

## Canonical identity

Message UUIDv7 is provider-independent and stable for the repository lifetime.
Each `identities/messages/<message-uuid>.json` record maps provider + provider
message ID to that UUID and survives cache/index rebuilds.

## Threads

Versioned thread manifests contain provider identity plus canonical message UUID
references in provider conversation order. They do not contain message copies.

Full thread context is materialized for every actionable unread message. Context-only messages have no work item.

## Work items

Work items reference canonical message UUID and hold exactly one state:

```text
pending | read | delete
```

Only unread Inbox messages receive work items.

## Selections

Each account-scoped selection is one versioned JSON file containing the account
UUID, validated selection name, and a sorted set of actionable message UUID
references. Work-item removal prunes matching references while preserving the
empty selection.

## Normalization

Deterministic, preservation-oriented normalization produces UTF-8 `content.md`
with LF line endings and exactly one final newline. It prefers non-empty
non-attachment `text/plain` within each `multipart/alternative`, otherwise
converts that alternative's HTML to Markdown without network access.
Independent body parts retain MIME order; links, quoted history, signatures,
and footers are preserved. MIME media types are matched case-insensitively.
M005 maps Gmail's structured MIME payload; `mail-parser` supplies charset and
transfer decoding, and `htmd` handles HTML. Partial parsing creates
content-redacted diagnostics and preserves provider truth internally; messages
are never silently dropped.

## Attachments

If provider bytes are already present, store them below the canonical message
using `<part-id>--<sanitized-name>`; the unmodified name stays in metadata.
Otherwise keep the provider attachment reference internally for M005's fetch
operation. Part IDs and generated paths reject traversal and separators.
M005 can inspect attachment locality before provider I/O and atomically persist
fetched bytes through the canonical store; an already-local fetch is a no-op.
Routine re-materialization preserves matching fetched bytes unless the provider
delivers replacement bytes or changes the part's declared size. Attachment
cache follows message/thread reachability and is disposable through explicit
cache rebuild or garbage collection.

## Structural index

SQLite schema version 1 indexes provider/message identity, canonical paths,
thread manifest/order, and attachment locality. It is atomically rebuilt from
identity records, canonical metadata, and thread manifests. M006 may replace
the disposable schema to add work-item/selection relationships. No generic FTS
requirement exists in v1.
