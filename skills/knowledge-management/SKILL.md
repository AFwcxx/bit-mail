---
name: knowledge-management
description: Use and maintain approved bit-mail Knowledge preferences. Use when an AI harness should apply existing global/account preferences, suggest a reusable preference from repeated triage behavior, clarify its scope, or persist/update/remove Knowledge only after explicit user approval.
---

# knowledge management

Use after the `bit-mail-core` rules are understood.

## Purpose

Use user-approved semantic preferences to improve future triage.

## Scope

- repository-global Knowledge applies to every account in the current repository;
- account Knowledge applies only to that account;
- there is no machine-global Knowledge in v1.

## Rules

- Read Knowledge directly from supported paths when useful.
- Never edit Knowledge files directly.
- If repeated behavior suggests a reusable preference, propose it to the user.
- Persist only after explicit user approval.
- If global vs account-specific scope is ambiguous, ask the user.
- Use `bit-mail knowledge ...` commands for creation/update/removal.

Knowledge is for what the user cares about. Framework operating rules belong in shipped skills, not Knowledge.
