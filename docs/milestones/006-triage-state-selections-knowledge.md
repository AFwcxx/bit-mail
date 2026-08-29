# M006 — Triage state, selections, and Knowledge

**Status:** Complete
**Depends on:** M002, M004, M005 work-item creation  
**Outcome:** Complete offline local decision workflow without provider mutation.

## Work items

- [x] Versioned work-item persistent schema created by M005 for reuse here.
- [x] State enum exactly `pending | read | delete`, created by M005.
- [x] M005 creates work items only for actionable unread Inbox messages.
- [x] Implement `work-items` human output.
- [x] Implement `work-items --state ...`.
- [x] Implement stable `work-items --json` including message ID and supported canonical path/context references.

## Stage / unstage

- [x] Implement `stage <id...> read|delete`.
- [x] Implement `stage --stdin read|delete` one UUID per line.
- [x] Validate all bulk input before mutating any requested batch state.
- [x] Implement `unstage <id...>`.
- [x] Skip `unstage --stdin`; explicit IDs and selections cover bulk unstage without adding undocumented syntax.
- [x] Support selection-based stage/unstage.
- [x] Require exact one-account resolution.
- [x] Record audit events.

## Selections

- [x] Define account-scoped persistent selection schema.
- [x] `selection create/add/remove/show/delete` as required.
- [x] References only; no content copies.
- [x] Only actionable work-item messages may be members.
- [x] Automatically prune missing/resolved members.
- [x] Preserve empty selection until explicit deletion.
- [x] Stable `--json` outputs.

## Knowledge

- [x] Define one-Markdown-file-per-item schema/frontmatter.
- [x] UUIDv7 identity.
- [x] Repository-global scope.
- [x] Account-specific scope.
- [x] Implement `knowledge add/list/show/update/remove` as justified by CLI design.
- [x] Ensure mutation goes through CLI with correct global/account lock.
- [x] Audit Knowledge ID/action/scope only; never store Knowledge content in audit.
- [x] Preserve Knowledge across cache rebuild by keeping it outside disposable account cache state.

## Offline behavior

- [x] Ensure all work-item/selection/Knowledge actions require no provider access.
- [x] Ensure provider credentials are never required for offline triage.

## Tests

- [x] State transition tests.
- [x] Bulk all-or-no-local-mutation-on-invalid-input tests.
- [x] Account-scope selection tests.
- [x] Selection pruning tests.
- [x] Global vs account Knowledge resolution tests.
- [x] Concurrent same-account mutations lock correctly.
- [x] Global Knowledge lock behavior.

## Exit criteria

- [x] Human can perform complete local triage without AI or network.
- [x] AI harness has deterministic commands for every persistence/mutation action and no reason to edit managed files.

## Progress log

### 2026-08-30

Implemented offline work-item listing and state transitions, account-scoped
selections with automatic pruning, global/account Knowledge with UUIDv7 TOML
frontmatter, metadata-only monthly audit JSONL, stable inspection JSON, and
account/global lock enforcement. Reused the M005 work-item schema and existing
repository/storage primitives; no provider access, new dependency, or SQLite
schema was added. Formatting, strict Clippy, and all-target tests passed.
