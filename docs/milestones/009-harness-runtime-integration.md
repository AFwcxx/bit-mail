# M009 — Harness runtime integration

**Status:** Planned  
**Depends on:** M002, M006, M007, public CLI shapes from M005/M008  
**Outcome:** Self-describing runtime repository with version-matched trusted skills/templates and stable machine-readable CLI contracts.

## Embedded templates

- [ ] Treat public source repo `skills/` and `templates/` as canonical source.
- [ ] Embed release-matched template/skill bytes into binary at build time.
- [ ] `bit-mail init` installs runtime `AGENTS.md` and skill tree without network access.
- [ ] Record installed/bundled template version in repository metadata.
- [ ] No `templates update` command in v1.
- [ ] Framework-managed runtime instructions included in integrity coverage.

## Skills

- [ ] Finalize `bit-mail-core`.
- [ ] Finalize `inbox-triage`.
- [ ] Finalize `bulk-review`.
- [ ] Finalize `knowledge-management`.
- [ ] Ensure all commands named by skills actually exist or are discovered dynamically rather than hard-coded falsely.
- [ ] Explicit untrusted-email-content/push authorization/threaded-delete policies.

## `help --json`

- [ ] Define versioned CLI capability schema.
- [ ] Generate from the same authoritative CLI command definitions used for human help.
- [ ] Include command/argument metadata.
- [ ] Include network/local/provider mutation properties.
- [ ] Include account-scope/all-account constraints.
- [ ] Include harness safety property for push/other sensitive actions.
- [ ] CI/test that every public CLI command appears correctly.

## `context --json`

- [ ] Define versioned session context schema.
- [ ] Repository UUID/root.
- [ ] Resolved account UUID/alias/provider.
- [ ] Supported harness-readable data path(s).
- [ ] Global/account Knowledge paths.
- [ ] staging counts and pull-blocked state.
- [ ] No secret/internal provider data leakage.

## Rendered context

- [ ] `show --context` clearly brackets untrusted email content in human output.
- [ ] JSON representation carries explicit trust classification.
- [ ] Thread assembly is deterministic and context-only/actionable distinction is clear.

## Docs consistency

- [ ] CI or tests verify public commands are covered by user manual or generated reference.
- [ ] Bootstrap tells harness to prefer runtime `help --json` over memorized syntax.

## Exit criteria

- [ ] A newly initialized mail repository is self-describing for supported AI harnesses without access to the source checkout.
- [ ] Skills and binary capabilities cannot silently drift across a release.
