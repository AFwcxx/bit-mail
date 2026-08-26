# Repository model

## Runtime repository

A runtime repository is a self-contained triage environment initialized by `bit-mail init`.

```text
repo-root/
├── .bit-mail/
├── data/
├── knowledge/
├── skills/
└── AGENTS.md
```

`.bit-mail/` is the repository marker and framework-owned internal namespace.

## Repository discovery

Starting at the current directory, walk upward until `.bit-mail/` is found. If no repository is found, commands that require one fail with a clear initialization hint.

## Repository identity

Every repository has an immutable UUID. It is non-secret and participates in namespacing secure credentials and persistent identities.

## Accounts

Each account has immutable UUID identity and mutable alias presentation.

Conceptual config:

```toml
[accounts.personal]
id = "<uuid>"
provider = "gmail"
address = "user@example.com"
credential_profile = "google-default"
```

Internal paths use account UUID, not alias, so alias rename is a configuration operation rather than a data migration.

## Account isolation

Each account owns independent provider state, cache, work items, selections, structural index, integrity branch, audit, and lock.

Mutations to unrelated accounts can run concurrently.

## Account selection

Resolution order:

1. explicit `--account`;
2. current directory inside account-owned data path;
3. `BIT_MAIL_ACCOUNT`;
4. exactly one account;
5. otherwise error.

Conflicting implicit contexts fail closed.

## Git relationship

A `bit-mail` repository is not a Git repository and does not require Git. When initialized inside Git, private runtime paths must be ignored/protected and accidental tracking detected.

## Source repository distinction

The public Rust source repository contains code/docs/templates/skills. It is separate from runtime mail repositories and should not normally contain mailbox data.
