# M002 — Repository and account core

**Status:** Planned  
**Depends on:** M001  
**Outcome:** Deterministic runtime repository initialization/discovery/configuration and account identity/isolation primitives.

## Repository initialization

- [ ] Implement `bit-mail init`.
- [ ] Generate immutable repository UUID.
- [ ] Create versioned `.bit-mail/repository.toml` and config schema.
- [ ] Create `data/`, `knowledge/`, and framework runtime-template destinations atomically.
- [ ] Apply secure default file/dir permissions (`0600`/`0700`) where meaningful.
- [ ] Allow non-empty directory when managed paths do not conflict.
- [ ] Fail without mutation on managed-path collision; no `--force`.
- [ ] Detect enclosing Git repository.
- [ ] Offer/verify ignore protection for private runtime paths without silently overwriting user ignore policy.
- [ ] Add repository discovery by upward `.bit-mail/` search.
- [ ] Add `bit-mail root` if retained by CLI design.

## Config

- [ ] Implement versioned framework-owned TOML config.
- [ ] Implement `config show` / `config show --json`.
- [ ] Implement validated `config set` for supported keys.
- [ ] Ensure no secret values can be persisted in config.

## Accounts

- [ ] Define immutable account UUID and mutable alias model.
- [ ] Define account config schema/provider binding.
- [ ] Implement alias validation and collision rules.
- [ ] Implement `accounts` enumeration.
- [ ] Implement `account rename` without moving UUID-owned internal data.
- [ ] Implement conservative `account remove` semantics including explicit discard/revoke choices.
- [ ] Reject duplicate provider mailbox identity within a repository once provider identity is available.

## Account resolution

- [ ] Implement explicit `--account` resolution.
- [ ] Implement account inference from current directory under supported account data path.
- [ ] Implement `BIT_MAIL_ACCOUNT` resolution.
- [ ] Implement single-account implicit selection.
- [ ] Fail on conflicting implicit contexts.
- [ ] Add `--all-accounts` framework where permitted; prohibit it structurally for mutating/provider-push commands.
- [ ] Implement `bit-mail --account <alias> path`.

## Locking

- [ ] Implement exclusive per-account mutation lock.
- [ ] Implement repository-level global-Knowledge lock primitive.
- [ ] Fail clearly on lock contention; expose holder/process metadata when reliable.
- [ ] Handle stale-lock diagnosis safely.

## Tests

- [ ] Init atomicity/collision tests.
- [ ] Discovery tests across nested directories.
- [ ] Account alias rename/isolation tests.
- [ ] Account resolution precedence/conflict tests.
- [ ] Multi-account concurrency lock tests.
- [ ] File-permission tests on Linux/macOS where feasible.

## Docs

- [ ] Update user manual commands when implemented.
- [ ] Update repository-model doc with exact on-disk schemas.

## Exit criteria

- [ ] Repository/account core works without any provider/LLM dependency.
- [ ] Two account-scoped mutating operations on different accounts can run independently.
- [ ] Same-account concurrent mutation is safely rejected/serialized by defined locking behavior.
