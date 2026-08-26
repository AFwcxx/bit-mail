# M006 — Triage state, selections, and Knowledge

**Status:** Planned  
**Depends on:** M002, M004, M005 work-item creation  
**Outcome:** Complete offline local decision workflow without provider mutation.

## Work items

- [ ] Implement versioned work-item persistent schema.
- [ ] State enum exactly `pending | read | delete`.
- [ ] Only actionable unread Inbox messages have work items.
- [ ] Implement `work-items` human output.
- [ ] Implement `work-items --state ...`.
- [ ] Implement stable `work-items --json` including message ID and supported canonical path/context references.

## Stage / unstage

- [ ] Implement `stage <id...> read|delete`.
- [ ] Implement `stage --stdin read|delete` one UUID per line.
- [ ] Validate all bulk input before mutating any requested batch state.
- [ ] Implement `unstage <id...>`.
- [ ] Implement `unstage --stdin` if justified by consistent CLI design.
- [ ] Support selection-based stage/unstage.
- [ ] Require exact one-account resolution.
- [ ] Record audit events.

## Selections

- [ ] Define account-scoped persistent selection schema.
- [ ] `selection create/add/remove/show/delete` as required.
- [ ] References only; no content copies.
- [ ] Only actionable work-item messages may be members.
- [ ] Automatically prune missing/resolved members.
- [ ] Preserve empty selection until explicit deletion.
- [ ] Stable `--json` outputs.

## Knowledge

- [ ] Define one-Markdown-file-per-item schema/frontmatter.
- [ ] UUIDv7 identity.
- [ ] Repository-global scope.
- [ ] Account-specific scope.
- [ ] Implement `knowledge add/list/show/update/remove` as justified by CLI design.
- [ ] Ensure mutation goes through CLI with correct global/account lock.
- [ ] Audit Knowledge changes without leaking sensitive content into metadata-only audit; decide whether storing Knowledge ID/action only is sufficient.
- [ ] Preserve Knowledge across cache rebuild.

## Offline behavior

- [ ] Ensure all work-item/selection/Knowledge actions require no provider access.
- [ ] Ensure provider credentials are never required for offline triage.

## Tests

- [ ] State transition tests.
- [ ] Bulk all-or-no-local-mutation-on-invalid-input tests.
- [ ] Account-scope selection tests.
- [ ] Selection pruning tests.
- [ ] Global vs account Knowledge resolution tests.
- [ ] Concurrent same-account mutations lock correctly.
- [ ] Global Knowledge lock behavior.

## Exit criteria

- [ ] Human can perform complete local triage without AI or network.
- [ ] AI harness has deterministic commands for every persistence/mutation action and no reason to edit managed files.
