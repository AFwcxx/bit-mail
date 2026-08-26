# M010 — Diagnostics and hardening

**Status:** Planned  
**Depends on:** M002-M009  
**Outcome:** Actionable, privacy-preserving diagnosis of repository, security, provider, and integrity health.

## `doctor`

- [ ] Repository format/version validation.
- [ ] Repository/account configuration consistency.
- [ ] Secure permission diagnostics.
- [ ] Git tracking/ignore diagnostics for private runtime paths.
- [ ] Credential-store availability/lookup health without exposing secrets.
- [ ] Gmail authorization validity diagnostics where network is explicitly used/appropriate.
- [ ] Provider state/cursor structural checks.
- [ ] SQLite structural-index validation/rebuild recommendation.
- [ ] Lock/stale-lock diagnostics.
- [ ] Runtime template/skill integrity checks.
- [ ] Scoped canonical integrity check.
- [ ] `doctor --full` complete BLAKE3/Merkle byte validation.
- [ ] `doctor --all-accounts` repository-wide account iteration.
- [ ] Stable redacted `--json` diagnostic output.

## Logging

- [ ] Structured logs with IDs/error classes/timings but no body/subject/token leakage.
- [ ] Verify verbose mode remains content-redacted by default.
- [ ] Tests asserting common error paths do not log secrets/content.
- [ ] No telemetry/analytics/crash upload code path.

## Permissions

- [ ] Create private runtime paths with restrictive modes where supported.
- [ ] Warn rather than hard-fail on unsafe mode bits/ACL ambiguity.
- [ ] Document manual remediation.

## Recovery UX

- [ ] Doctor recommendations point to deterministic commands: reauthorize, repair, index rebuild, cache rebuild, gc, permission remediation.
- [ ] No recommendation requires manual editing of managed JSON/TOML except explicit advanced recovery documentation.

## Exit criteria

- [ ] Common broken-state scenarios can be diagnosed without exposing sensitive mail content in issue reports.
