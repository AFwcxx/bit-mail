# M001 — Project foundation

**Status:** Complete

**Depends on:** M000  
**Outcome:** A buildable, testable Rust foundation with clear module boundaries and CI quality gates.

## Scope

### Rust project

- [x] Create `Cargo.toml` using Rust 2024 edition.
- [x] Add `rust-toolchain.toml` using current stable toolchain policy.
- [x] Create minimal `src/main.rs` placeholder.
- [x] Install/use Rust toolchain in development environment and verify `cargo check`.
- [x] Define crate/module topology from architecture, avoiding premature abstraction.
- [x] Decide whether core is one package initially or a small workspace; prefer simplest build that preserves test seams.
- [x] Add baseline error type/error-reporting strategy.
- [x] Add structured tracing/logging foundation with content-redaction policy.
- [x] Add CLI parser foundation without claiming commands are implemented prematurely.

### Quality

- [x] Add formatting/lint/test scripts or documented commands.
- [x] Establish `cargo fmt --check` gate.
- [x] Establish clippy warnings-as-errors gate where practical.
- [x] Add baseline unit-test module/test fixture conventions.
- [x] Add CI for Linux/macOS build/test/lint where available.
- [x] Ensure no normal CI requires provider credentials.

### Repository

- [x] Initialize Git source repository.
- [x] Add source `.gitignore` protecting accidental runtime `/.bit-mail/`, `/data/`, `/knowledge/`.
- [x] Add full Apache-2.0 `LICENSE` file.
- [x] Verify repository URL/package metadata once public GitHub location is known; avoid hard-coding an incorrect URL.

### Documentation

- [x] Create top-level README/navigation.
- [x] Create consolidated requirements and milestone system.
- [x] Keep current development conventions in `docs/development/`; defer a separate contributor guide until needed.

## Exit criteria

- [x] `cargo check` succeeds on supported development toolchain.
- [x] `cargo fmt --check` succeeds.
- [x] `cargo test` succeeds without Gmail access.
- [x] CI baseline exists or is intentionally deferred with documented reason.
- [x] Foundation introduces no unrequired network/model dependency.

## Progress log

### 2026-08-25

Source repository, Rust metadata, README, documentation tree, skills/templates, and milestone system created. Current execution environment does not have `rustc`/`cargo`, so build verification is pending rather than falsely marked complete.

### 2026-08-26

Completed the single-package Rust foundation with a library/binary boundary, truthful help/version-only CLI, process-boundary error reporting, content-redacted structured logging policy, and Linux/macOS CI. Added a shared integration-test fixture convention, canonical GitHub repository metadata, and tracked dependency resolution in `Cargo.lock`.

Verified with Rust 1.94.0: `cargo check`, `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all pass. Tests: 2 passed, 0 failed, 0 ignored. Manual CLI checks confirmed bare invocation, help, version, and rejection of unimplemented commands. GitHub-hosted CI execution awaits the first pushed workflow run.
