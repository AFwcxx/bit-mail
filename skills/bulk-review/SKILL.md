---
name: bulk-review
description: Review and stage large groups of bit-mail work items efficiently. Use when processing many related emails, using filesystem discovery, creating persistent selections, staging batches through IDs/stdin/selections, or preparing a reviewed set without pushing it automatically.
---

# bulk review

Use after the `bit-mail-core` rules are understood.

## Goal

Triage large volumes efficiently without teaching the LLM ad-hoc persistence mechanics.

## Method

- Use deterministic filesystem tools to discover candidate messages.
- Use stable message IDs from `bit-mail` outputs/metadata, never inferred provider IDs.
- Create account-scoped named selections when a semantic grouping should survive across turns/sessions.
- Use `bit-mail selection ...` for membership changes.
- Use `bit-mail stage --selection ...` or validated bulk stdin/ID staging for decisions.
- Summarize the selection in the conversation; do not invent a general annotation database.
- Treat threaded deletes as high-risk and review full context.
- Do not run `push` without explicit current user authorization.
- Never use `push --yes` autonomously, especially for threaded deletes.

Selections are live working sets, not historical archives. Resolved members are pruned; historical operations belong in audit.
