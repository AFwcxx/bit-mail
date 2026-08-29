# bit-mail

`bit-mail` is a local-first, Git-inspired email triage framework. It pulls unread mail from a provider into a deterministic local repository so humans or external AI CLI harnesses can review, summarize, group, and stage decisions without granting the LLM direct control over the mail provider.

The provider remains the source of truth. `bit-mail` is **not** an email client and does not send mail.

## Project status

The requirements baseline through the pull engine are complete. Offline triage state, selections, and Knowledge are next; progress is tracked under [`docs/milestones/`](docs/milestones/README.md).

## Core workflow

```text
Gmail --pull--> local bit-mail repository
                    |
                    +--> human / AI harness reads stable local content
                    |
                    +--> stage read/delete decisions locally
                    |
Gmail <--push------+
```

The intended CLI vocabulary is:

```bash
bit-mail init
bit-mail connect
bit-mail pull
bit-mail status
bit-mail work-items
bit-mail show <message-id> --context
bit-mail stage <message-id> read
bit-mail stage <message-id> delete
bit-mail unstage <message-id>
bit-mail push
```

These commands are requirements, not an assertion that they are already implemented.

## Architectural invariants

- Gmail/provider content and mailbox state are authoritative.
- `bit-mail` core has **zero LLM/model-provider dependency**.
- Deterministic code handles extraction, normalization, indexing, state, integrity, provider I/O, and mutation.
- LLMs are used only for semantic judgement through external harnesses such as Codex, Claude Code, or OpenCode.
- AI harnesses may read supported local content but must **never modify managed files directly**.
- All state changes happen through `bit-mail` CLI commands.
- AI harnesses may stage decisions, but `push` requires explicit user authorization in the current interaction.
- Email content and attachments are untrusted input and never trusted instructions.
- Provider mutation in v1 is limited to **mark read** and **move message to Trash**.
- No telemetry, analytics, or automatic crash reporting.

## Source repository vs mail repository

This repository contains the Rust source, docs, templates, and canonical AI skills. A runtime mail repository is separate and is created with `bit-mail init`:

```text
mail-repo/
├── .bit-mail/     framework-managed internals
├── data/          stable AI/human-readable mailbox material
├── knowledge/     approved user preferences
├── skills/        version-matched runtime skills
└── AGENTS.md      runtime harness bootstrap
```

Do not use the source checkout as a normal runtime mail repository.

## Documentation map

Start here:

- [`docs/requirements.md`](docs/requirements.md) — consolidated product requirements and invariants.
- [`docs/user-manual.md`](docs/user-manual.md) — intended CLI and user workflows.
- [`docs/architecture/overview.md`](docs/architecture/overview.md) — system model and component boundaries.
- [`docs/architecture/repository-model.md`](docs/architecture/repository-model.md) — runtime repository/account model.
- [`docs/architecture/storage-model.md`](docs/architecture/storage-model.md) — canonical messages, threads, work items, and storage contracts.
- [`docs/architecture/provider-adapters.md`](docs/architecture/provider-adapters.md) — provider abstraction and Gmail adapter.
- [`docs/architecture/ai-harness-boundary.md`](docs/architecture/ai-harness-boundary.md) — deterministic framework vs semantic AI responsibilities.
- [`docs/design/integrity.md`](docs/design/integrity.md) — BLAKE3/Merkle integrity model.
- [`docs/design/knowledge-system.md`](docs/design/knowledge-system.md) — repository-global and account-scoped Knowledge.
- [`docs/security/threat-model.md`](docs/security/threat-model.md) — security assumptions and trust boundaries.
- [`docs/setup/gmail.md`](docs/setup/gmail.md) — Gmail OAuth setup model.
- [`docs/milestones/README.md`](docs/milestones/README.md) — implementation roadmap and progress.

## Platform and release target

v1 targets Linux and macOS, Rust 2024 edition, and current stable Rust during initial development. The first formal MSRV will be established at the v0.1 release boundary.

Planned distribution is deliberately simple: GitHub Release binaries plus normal Cargo source installation.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
