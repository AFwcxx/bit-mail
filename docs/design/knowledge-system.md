# Knowledge system

## Purpose

Knowledge is durable, user-approved semantic preference data for AI-assisted triage.

Examples:

- “I don't care about cryptocurrency price movements.”
- “Technical Bitcoin protocol changes are relevant.”
- “On my work account, infrastructure vendor release notes are usually worth reviewing.”

## Scope

```text
repository
├── knowledge/global/                  applies to every account here
└── knowledge/accounts/<account-id>/  applies only to that account
```

There is no machine-global Knowledge in v1.

## Persistence

One `<uuid-v7>.md` file per Knowledge item. Deterministic TOML frontmatter stores
schema version, UUID, `global` or `account` scope, optional account UUID, and
creation/update Unix-millisecond timestamps. The remaining UTF-8 Markdown body
is normalized to one final newline.

The filesystem is canonical. AI may read Knowledge directly but must mutate it only through `bit-mail knowledge ...`.

Mutation audit records contain only the Knowledge UUID, action, and scope; they
never copy the Markdown body.

## Approval rule

AI may suggest that a recurring preference should be remembered, but may not persist it autonomously. Explicit user approval is always required. If scope is unclear, ask global vs account-specific.

## Relationship to skills

Skills define **how to operate `bit-mail` safely** and are framework-managed. Knowledge defines **what this user cares about** and is the supported customization surface.
