# bit-mail milestones

Milestones are the execution boundary for development sessions, worktrees, and parallel developers. Each milestone must be independently understandable from the repository documentation and should update its own progress log while work is performed.

## Rules for milestone work

1. Read `docs/requirements.md` before changing product semantics.
2. Read the milestone file and its referenced architecture docs before coding.
3. Do not silently change a locked invariant. Record deliberate requirement/design changes in `docs/design/decisions.md` and the affected milestone.
4. Keep checklist state current (`[ ]`, `[x]`) during implementation.
5. Add dated progress-log entries for significant completed work, blockers, test results, and decisions.
6. Update `docs/user-manual.md` whenever public CLI behavior changes or lands.
7. Update `README.md` only for durable navigation/status information; do not turn it into the full manual.
8. New persistent formats require explicit schema/version consideration.
9. Public commands that become implemented must be reflected in the future `help --json` capability schema and tested.
10. A milestone is complete only when its exit criteria and required tests/docs are satisfied.

## Roadmap

| ID | Milestone | Status | Primary outcome |
|---|---|---|---|
| M000 | [Requirements baseline](000-requirements-baseline.md) | **Complete** | Product semantics, architecture boundaries, safety policies captured |
| M001 | [Project foundation](001-project-foundation.md) | **Complete** | Buildable Rust project, module skeleton, CI/quality baseline |
| M002 | [Repository and account core](002-repository-account-core.md) | **Complete** | `init`, discovery, config, account identity/scope/locking |
| M003 | [Credentials and Gmail connect](003-credentials-gmail-connect.md) | Completed | Secure keyring + BYO OAuth + account connection |
| M004 | [Canonical mail storage](004-canonical-mail-storage.md) | Planned | Provider-neutral canonical messages, normalization, threads, attachments |
| M005 | [Pull engine](005-pull-engine.md) | Planned | Bounded/incremental full-context Gmail pull and reconciliation |
| M006 | [Triage state, selections, Knowledge](006-triage-state-selections-knowledge.md) | Planned | Work items, stage/unstage, selections, Knowledge |
| M007 | [Integrity and recovery](007-integrity-recovery.md) | Planned | BLAKE3 Merkle integrity, repair, GC, cache rebuild |
| M008 | [Push engine](008-push-engine.md) | Planned | Reviewed/idempotent Gmail read/Trash commit path |
| M009 | [Harness runtime integration](009-harness-runtime-integration.md) | Planned | Embedded templates, skills, context/help JSON contracts |
| M010 | [Diagnostics and hardening](010-diagnostics-hardening.md) | Planned | `doctor`, permissions, Git tracking checks, robust diagnostics |
| M011 | [Testing and performance](011-testing-performance.md) | Planned | Provider contract tests, live opt-in tests, benchmarks, optimization |
| M012 | [v0.1 release](012-v0.1-release.md) | Planned | Docs polish, MSRV, release binaries, public release readiness |

## Dependency shape

```text
M000
  |
  v
M001 -> M002 -> M003
           |      |
           v      v
          M004 -> M005 -> M006 -> M007 -> M008
                                  |        |
                                  +----+---+
                                       v
                                      M009
                                       |
                                       v
                                      M010
                                       |
                                       v
                                      M011
                                       |
                                       v
                                      M012
```

Some implementation can overlap once contracts are stable, especially tests/docs, but no worktree should redefine shared persistent schemas without coordination.
