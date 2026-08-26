# Development setup

## Toolchain

- Rust 2024 edition.
- Current stable Rust during initial development.
- Formal MSRV chosen at v0.1 release.

The repository includes `rust-toolchain.toml` using the `stable` channel with `rustfmt` and `clippy` components.

## Source/runtime separation

Do not place real mailbox runtime data into the source repository. Runtime repositories should be created elsewhere once `bit-mail init` exists.

## Quality gates

Planned baseline:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Provider contract tests must not require real Gmail credentials. Live Gmail tests are explicit/ignored/opt-in.
