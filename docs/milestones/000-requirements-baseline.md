# M000 — Requirements baseline

**Status:** Complete  
**Purpose:** Establish the product semantics and architecture boundaries before implementation.

## Completed scope

- [x] Rename project to `bit-mail`.
- [x] Establish Rust/Linux/macOS/no-server direction.
- [x] Establish runtime mail repository model via `bit-mail init`.
- [x] Separate source repository from runtime mail repositories.
- [x] Establish repository UUID and immutable account UUID + mutable alias.
- [x] Establish multi-account isolation/concurrency model.
- [x] Establish Gmail/Workspace Gmail as first provider.
- [x] Establish compile-time provider adapter abstraction and REST-first policy.
- [x] Establish BYO Google OAuth client model and secure OS credential storage.
- [x] Establish provider-as-source-of-truth rule.
- [x] Establish canonical-message/reference-only thread model.
- [x] Establish complete thread context requirement.
- [x] Establish deterministic normalization and attachment policy.
- [x] Establish `pending | read | delete` work-item states.
- [x] Replace `mark` with `stage` / `unstage`.
- [x] Replace `sync` with `pull` / `push`.
- [x] Establish account-scoped pull blocking when staged intent exists.
- [x] Establish selections and Knowledge models.
- [x] Establish no generic AI annotation store.
- [x] Establish no generic full-text search/FTS5 requirement in v1.
- [x] Establish BLAKE3 hierarchical Merkle integrity model.
- [x] Establish repair/GC/cache rebuild semantics.
- [x] Establish threaded-delete safety and human push boundary.
- [x] Establish no sending mail and narrow provider mutation surface.
- [x] Establish no telemetry and redacted diagnostics.
- [x] Establish offline-first post-pull triage.
- [x] Establish runtime skills/templates and binary-version-lock model.
- [x] Establish `help --json` and `context --json` requirements.
- [x] Establish mock/provider contract/live-opt-in testing model.
- [x] Establish GitHub Release + Cargo/source distribution and Apache-2.0 license.
- [x] Capture consolidated requirements in `docs/requirements.md`.
- [x] Create user manual/architecture/security/setup/development docs.
- [x] Create canonical source skills and runtime `AGENTS.md` template.

## Exit criteria

- [x] No known critical product unknown remains before implementation planning.
- [x] Locked requirements are recoverable from repository docs without chat history.
- [x] Implementation work is decomposed into independent milestone files.

## Progress log

### 2026-08-25

Requirements gathering completed iteratively. The consolidated baseline was materialized into the source repository before substantive Rust implementation. Product complexity was deliberately reduced where the framework would otherwise duplicate ordinary AI/filesystem capabilities or drift toward replicated-database semantics.
