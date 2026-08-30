# Repository model

## Runtime repository

A runtime repository is a self-contained triage environment initialized by `bit-mail init`.

```text
repo-root/
├── .bit-mail/
│   ├── accounts/
│   ├── locks/
│   │   └── accounts/
│   ├── config.toml
│   └── repository.toml
├── data/
├── knowledge/
├── skills/
└── AGENTS.md
```

`.bit-mail/` is the repository marker and framework-owned internal namespace.
`bit-mail init` installs the binary-version-matched skill contents and
`AGENTS.md` bootstrap without network access.

## Versioned repository files

`.bit-mail/repository.toml` contains immutable repository identity:

```toml
schema_version = 2
id = "<uuid-v4>"
runtime_assets_version = "<bit-mail-version>"
```

Repositories created before runtime assets were embedded may omit
`runtime_assets_version`. Repository discovery installs a missing clean asset
set or replaces an integrity-valid older set with the binary's embedded version.
Legacy collisions and integrity mismatches fail without being overwritten.

`.bit-mail/config.toml` contains mutable framework configuration:

```toml
schema_version = 1

[pull]
default_limit = 500
```

Unknown fields and unsupported schema versions are rejected. The only M002
`config set` key is `pull.default-limit`, which must be a positive integer.

Pre-integrity repository schema v1 remains readable, but mutations require a
one-time `bit-mail migrate-integrity`. Once schema v2 is active, missing
integrity manifests fail closed instead of establishing a new baseline.

## Repository discovery

Starting at the current directory, walk upward until `.bit-mail/` is found. If no repository is found, commands that require one fail with a clear initialization hint.

## Repository identity

Every repository has an immutable UUID. It is non-secret and participates in namespacing secure credentials and persistent identities.

## Accounts

Each account has immutable UUID identity and mutable alias presentation.

Account configuration is UUID-owned at
`.bit-mail/accounts/<account-uuid>/account.toml`:

```toml
schema_version = 1
id = "<uuid>"
alias = "personal"
provider = "gmail"
provider_identity = "user@example.com" # optional until provider identity exists
credential_profile = "google-default"  # optional non-secret reference
```

Internal paths use account UUID, not alias, so alias rename is a configuration operation rather than a data migration.
Aliases are 1-32 lowercase ASCII letters/digits, `-`, or `_`; the first and
last characters must be alphanumeric.

## Account isolation

Each account owns independent provider state, cache, work items, selections, structural index, integrity branch, audit, and lock.

Mutations to unrelated accounts can run concurrently.

Transient account mutation locks use
`.bit-mail/locks/accounts/<account-uuid>.lock`. Repository-wide account
lifecycle changes use `.bit-mail/locks/account-lifecycle.lock`, and global
Knowledge uses `.bit-mail/locks/knowledge.lock`. Keeping these locks outside
deletable account state prevents removal from invalidating a held lock.

## Account selection

Resolution order:

1. explicit `--account`;
2. current directory inside account-owned data path;
3. `BIT_MAIL_ACCOUNT`;
4. exactly one account;
5. otherwise error.

Conflicting implicit contexts fail closed.

Only commands that support repository-wide account inspection expose
`--all-accounts`. In M002, `bit-mail path --all-accounts` prints each alias and
its UUID-owned data path; account mutation commands cannot parse the flag.

Removing an account never removes `knowledge/accounts/<account-uuid>/`.
Existing account Knowledge remains at its UUID-owned path for manual recovery.

## Git relationship

A `bit-mail` repository is not a Git repository and does not require Git. When initialized inside Git, private runtime paths must be ignored/protected and accidental tracking detected.

If Git is detected and required ignore coverage is missing, interactive `init`
offers to append only the missing repository-relative rules. Declining or
running non-interactively leaves `.gitignore` unchanged and prints the rules.
When the Git executable is available, broader existing ignore rules and
already-tracked private paths are verified directly.

## Source repository distinction

The public Rust source repository contains code/docs/templates/skills. It is separate from runtime mail repositories and should not normally contain mailbox data.
