# M002 — Repository and account core

**Status:** Complete
**Depends on:** M001
**Outcome:** Deterministic runtime repository initialization/discovery/configuration and account identity/isolation primitives.

## Repository initialization

- [x] Implement `bit-mail init`.
- [x] Generate immutable repository UUID.
- [x] Create versioned `.bit-mail/repository.toml` and config schema.
- [x] Create `data/`, `knowledge/`, and framework runtime-template destinations atomically.
- [x] Apply secure default file/dir permissions (`0600`/`0700`) where meaningful.
- [x] Allow non-empty directory when managed paths do not conflict.
- [x] Fail without mutation on managed-path collision; no `--force`.
- [x] Detect enclosing Git repository.
- [x] Offer/verify ignore protection for private runtime paths without silently overwriting user ignore policy.
- [x] Add repository discovery by upward `.bit-mail/` search.
- [x] Omit `bit-mail root`; the retained CLI design uses account-scoped `path`.

## Config

- [x] Implement versioned framework-owned TOML config.
- [x] Implement `config show` / `config show --json`.
- [x] Implement validated `config set` for supported keys.
- [x] Ensure no secret values can be persisted in config.

## Accounts

- [x] Define immutable account UUID and mutable alias model.
- [x] Define account config schema/provider binding.
- [x] Implement alias validation and collision rules.
- [x] Implement `accounts` enumeration.
- [x] Implement `account rename` without moving UUID-owned internal data.
- [x] Implement conservative `account remove` semantics including explicit discard/revoke choices.
- [x] Reject duplicate provider mailbox identity within a repository once provider identity is available.

## Account resolution

- [x] Implement explicit `--account` resolution.
- [x] Implement account inference from current directory under supported account data path.
- [x] Implement `BIT_MAIL_ACCOUNT` resolution.
- [x] Implement single-account implicit selection.
- [x] Fail on conflicting implicit contexts.
- [x] Add `--all-accounts` framework where permitted; prohibit it structurally for mutating/provider-push commands.
- [x] Implement `bit-mail --account <alias> path`.

## Locking

- [x] Implement exclusive per-account mutation lock.
- [x] Implement repository-level global-Knowledge lock primitive.
- [x] Fail clearly on lock contention; expose holder/process metadata when reliable.
- [x] Handle stale-lock diagnosis safely.

## Tests

- [x] Init atomicity/collision tests.
- [x] Discovery tests across nested directories.
- [x] Account alias rename/isolation tests.
- [x] Account resolution precedence/conflict tests.
- [x] Multi-account concurrency lock tests.
- [x] File-permission tests on Linux/macOS where feasible.

## Docs

- [x] Update user manual commands when implemented.
- [x] Update repository-model doc with exact on-disk schemas.

## Exit criteria

- [x] Repository/account core works without any provider/LLM dependency.
- [x] Two account-scoped mutating operations on different accounts can run independently.
- [x] Same-account concurrent mutation is safely rejected/serialized by defined locking behavior.

## Progress log

### 2026-08-27

Implemented versioned repository/config/account schemas, secure collision-safe
initialization, upward discovery, consent-based Git ignore protection, validated
configuration, account identity/lifecycle/resolution, and independent account and
repository-Knowledge locks. M009 retains runtime template installation; M003
retains the real secure credential revocation backend behind the M002 policy seam.

Verified locally with Rust 1.94.0: formatting, check, Clippy warnings-as-errors,
tests, and whitespace validation pass. Tests cover initialization atomicity,
permissions, discovery, config validation, account isolation/resolution/removal,
locking, and executable CLI behavior.

Follow-up acceptance review made `.bit-mail` the final initialization commit
marker, moved account mutation locks outside deletable account state, serialized
repository-wide account lifecycle collision checks, and made all-account scope a
permitted-command API rather than a caller-controlled policy boolean. Added
forced rollback, threaded lock contention, lifecycle/removal safety, and
executable account command coverage; 18 local tests pass with none skipped.

The all-account framework is command-scoped and consumed by `path`, while
mutating account commands reject that flag structurally. Account removal
explicitly preserves UUID-owned account Knowledge and reports its recovery path.
