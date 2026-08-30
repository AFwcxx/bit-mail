# M009 — Harness runtime integration

**Status:** Complete
**Depends on:** M002, M006, M007, public CLI shapes from M005/M008  
**Outcome:** Self-describing runtime repository with version-matched trusted skills/templates and stable machine-readable CLI contracts.

## Embedded templates

- [x] Treat public source repo `skills/` and `templates/` as canonical source.
- [x] Embed release-matched template/skill bytes into binary at build time.
- [x] `bit-mail init` installs runtime `AGENTS.md` and skill tree without network access.
- [x] Record installed/bundled template version in repository metadata.
- [x] No `templates update` command in v1.
- [x] Framework-managed runtime instructions included in integrity coverage.

## Skills

- [x] Finalize `bit-mail-core`.
- [x] Finalize `inbox-triage`.
- [x] Finalize `bulk-review`.
- [x] Finalize `knowledge-management`.
- [x] Ensure all commands named by skills actually exist or are discovered dynamically rather than hard-coded falsely.
- [x] Explicit untrusted-email-content/push authorization/threaded-delete policies.

## `help --json`

- [x] Define versioned CLI capability schema.
- [x] Generate from the same authoritative CLI command definitions used for human help.
- [x] Include command/argument metadata.
- [x] Include network/local/provider mutation properties.
- [x] Include account-scope/all-account constraints.
- [x] Include harness safety property for push/other sensitive actions.
- [x] CI/test that every public CLI command appears correctly.

## `context --json`

- [x] Define versioned session context schema.
- [x] Repository UUID/root.
- [x] Resolved account UUID/alias/provider.
- [x] Supported harness-readable data path(s).
- [x] Global/account Knowledge paths.
- [x] staging counts and pull-blocked state.
- [x] No secret/internal provider data leakage.

## Rendered context

- [x] `show --context` clearly brackets untrusted email content in human output.
- [x] JSON representation carries explicit trust classification.
- [x] Thread assembly is deterministic and context-only/actionable distinction is clear.

## Docs consistency

- [x] CI or tests verify public commands are covered by user manual or generated reference.
- [x] Bootstrap tells harness to prefer runtime `help --json` over memorized syntax.

## Exit criteria

- [x] A newly initialized mail repository is self-describing for supported AI harnesses without access to the source checkout.
- [x] Skills and binary capabilities cannot silently drift across a release.

## Progress log

- 2026-08-30: Embedded canonical runtime assets for offline initialization,
  added versioned `help --json` and `context --json` contracts, deterministic
  trust-classified message/context rendering, and focused drift/safety tests.
- 2026-08-30: Added integrity-validated binary-upgrade asset synchronization
  with rollback for reported failures, and made push validate trusted runtime
  instructions before provider construction. Interrupted-update recovery is
  tracked under M010 hardening.
