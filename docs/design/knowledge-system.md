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

One Markdown file per Knowledge item. Each item has stable UUIDv7 identity and deterministic metadata/frontmatter.

The filesystem is canonical. AI may read Knowledge directly but must mutate it only through `bit-mail knowledge ...`.

## Approval rule

AI may suggest that a recurring preference should be remembered, but may not persist it autonomously. Explicit user approval is always required. If scope is unclear, ask global vs account-specific.

## Relationship to skills

Skills define **how to operate `bit-mail` safely** and are framework-managed. Knowledge defines **what this user cares about** and is the supported customization surface.
