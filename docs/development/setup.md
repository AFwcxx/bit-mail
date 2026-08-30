# Development setup

## Toolchain

- Rust 2024 edition.
- Rust 1.88 minimum; current stable is recommended for development.

The repository includes `rust-toolchain.toml` using the `stable` channel with `rustfmt` and `clippy` components.

## Source/runtime separation

Do not place real mailbox runtime data into the source repository. Runtime repositories should be created elsewhere once `bit-mail init` exists.

## Quality gates

Baseline commands:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo +1.88.0 check --locked --all-targets --all-features
```

Provider contract tests must not require real Gmail credentials. Live Gmail tests are explicit/ignored/opt-in.

## Errors and logging

The binary reports fatal errors once at the process boundary and returns a failure exit code. Add domain-specific error types only when callers need to distinguish recovery behavior.

Structured logs must use content-free operational fields. Never log credentials, tokens, subjects, bodies, attachment contents, or other mailbox content.
