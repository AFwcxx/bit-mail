# M010 — Diagnostics and hardening

**Status:** Complete
**Depends on:** M002-M009  
**Outcome:** Actionable, privacy-preserving diagnosis of repository, security, provider, and integrity health.

## `doctor`

- [x] Repository format/version validation.
- [x] Repository/account configuration consistency.
- [x] Secure permission diagnostics.
- [x] Git tracking/ignore diagnostics for private runtime paths.
- [x] Credential-store availability/lookup health without exposing secrets.
- [x] Gmail authorization validity diagnostics where network is explicitly used/appropriate.
- [x] Provider state/cursor structural checks.
- [x] SQLite structural-index validation/rebuild recommendation.
- [x] Lock/stale-lock diagnostics.
- [x] Runtime template/skill integrity checks.
- [x] Scoped canonical integrity check.
- [x] `doctor --full` complete BLAKE3/Merkle byte validation.
- [x] `doctor --all-accounts` repository-wide account iteration.
- [x] Stable redacted `--json` diagnostic output.

## Logging

- [x] Structured logs with IDs/error classes/timings but no body/subject/token leakage.
- [x] Verify verbose mode remains content-redacted by default.
- [x] Tests asserting common error paths do not log secrets/content.
- [x] No telemetry/analytics/crash upload code path.

## Permissions

- [x] Create private runtime paths with restrictive modes where supported.
- [x] Warn rather than hard-fail on unsafe mode bits/ACL ambiguity.
- [x] Document manual remediation.

## Recovery UX

- [x] Doctor recommendations point to deterministic commands: reauthorize, repair, index rebuild, cache rebuild, gc, permission remediation.
- [x] Detect interrupted runtime asset updates and restore the last integrity-valid asset set without manual managed-file editing.
- [x] No recommendation requires manual editing of managed JSON/TOML except explicit advanced recovery documentation.

## Exit criteria

- [x] Common broken-state scenarios can be diagnosed without exposing sensitive mail content in issue reports.

## Progress log

- 2026-08-30: Added offline-by-default versioned `doctor` diagnostics with
  explicit read-only `--online` authorization checks, scoped/full integrity,
  all-account iteration, index validation/rebuild, permission, Git, credential,
  cursor, and stale-lock checks. Hardened redacted verbose provider logs and
  private file creation, and added integrity-validated recovery for interrupted
  runtime asset updates.
- 2026-08-30: Redacted and stabilized filesystem diagnostics, constrained lock
  remediation to managed paths, and surfaced ACL ambiguity as a non-fatal
  warning.
