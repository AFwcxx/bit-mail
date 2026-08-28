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

Transient locks remain outside deletable account state so account removal cannot invalidate a held lock.

Provider-specific message representations are internal and non-contractual for AI harnesses.

## Canonical identity

Message UUIDv7 is provider-independent and stable for the repository lifetime. A durable identity map retains `(provider message ID -> bit-mail UUID)` across cache rebuilds.

## Threads

Thread manifests are internal references in chronological/conversation order. They do not contain message copies.

Full thread context is materialized for every actionable unread message. Context-only messages have no work item.

## Work items

Work items reference canonical message UUID and hold exactly one state:

```text
pending | read | delete
```

Only unread Inbox messages receive work items.

## Normalization

Deterministic, preservation-oriented normalization produces `content.md`. Partial parsing creates diagnostics and preserves provider truth internally; messages are never silently dropped.

## Attachments

If provider bytes are already present, store them. Otherwise keep attachment metadata internally and fetch on demand. Attachment cache follows thread reachability and is disposable.

## Structural index

SQLite accelerates structural/domain lookup only. It is derived and rebuildable. No generic FTS requirement exists in v1.
