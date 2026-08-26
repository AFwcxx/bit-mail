# M001 — Project foundation

**Status:** In progress  
**Depends on:** M000  
**Outcome:** A buildable, testable Rust foundation with clear module boundaries and CI quality gates.

## Scope

### Rust project

- [x] Create `Cargo.toml` using Rust 2024 edition.
- [x] Add `rust-toolchain.toml` using current stable toolchain policy.
- [x] Create minimal `src/main.rs` placeholder.
- [ ] Install/use Rust toolchain in development environment and verify `cargo check`.
- [ ] Define crate/module topology from architecture, avoiding premature abstraction.
- [ ] Decide whether core is one package initially or a small workspace; prefer simplest build that preserves test seams.
- [ ] Add baseline error type/error-reporting strategy.
- [ ] Add structured tracing/logging foundation with content-redaction policy.
- [ ] Add CLI parser foundation without claiming commands are implemented prematurely.

### Quality

- [ ] Add formatting/lint/test scripts or documented commands.
- [ ] Establish `cargo fmt --check` gate.
- [ ] Establish clippy warnings-as-errors gate where practical.
- [ ] Add baseline unit-test module/test fixture conventions.
- [ ] Add CI for Linux/macOS build/test/lint where available.
- [ ] Ensure no normal CI requires provider credentials.

### Repository

- [x] Initialize Git source repository.
- [x] Add source `.gitignore` protecting accidental runtime `/.bit-mail/`, `/data/`, `/knowledge/`.
- [x] Add full Apache-2.0 `LICENSE` file.
- [ ] Verify repository URL/package metadata once public GitHub location is known; avoid hard-coding an incorrect URL.

### Documentation

- [x] Create top-level README/navigation.
- [x] Create consolidated requirements and milestone system.
- [ ] Add contributor/development conventions if needed.

## Exit criteria

- [ ] `cargo check` succeeds on supported development toolchain.
- [ ] `cargo fmt --check` succeeds.
- [ ] `cargo test` succeeds without Gmail access.
- [ ] CI baseline exists or is intentionally deferred with documented reason.
- [ ] Foundation introduces no unrequired network/model dependency.

## Progress log

### 2026-08-25

Source repository, Rust metadata, README, documentation tree, skills/templates, and milestone system created. Current execution environment does not have `rustc`/`cargo`, so build verification is pending rather than falsely marked complete.
