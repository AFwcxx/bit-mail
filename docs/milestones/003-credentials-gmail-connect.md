# M003 — Credentials and Gmail connect

**Status:** Planned  
**Depends on:** M002  
**Outcome:** Secure BYO-OAuth Gmail account onboarding with reusable OAuth client profiles.

## Credential abstraction

- [ ] Define secure credential-store interface independent of provider domain model.
- [ ] Implement macOS Keychain backend through an appropriate maintained Rust abstraction.
- [ ] Implement Linux Secret Service backend.
- [ ] Namespace secure items by repository UUID + credential profile/account UUID.
- [ ] No plaintext token/client-secret fallback.
- [ ] Add redacted credential diagnostics.

## OAuth client profiles

- [ ] Define reusable non-secret profile references in repository config/internal state.
- [ ] Import Google Desktop OAuth client JSON interactively.
- [ ] Validate client JSON deterministically.
- [ ] Persist client secret material only to OS credential store.
- [ ] Reuse one OAuth client profile across multiple Gmail mailbox authorizations.

## Gmail OAuth

- [ ] Use Authorization Code flow with PKCE/local redirect as appropriate for Desktop apps.
- [ ] Request only `gmail.modify`.
- [ ] Launch local browser; print authorization URL if launch fails.
- [ ] Handle callback safely on workstation Linux/macOS.
- [ ] Store refresh token securely; keep access token memory-only where practical.
- [ ] Fetch authenticated Gmail profile/identity after authorization.
- [ ] Reject mailbox duplicate within current repository.
- [ ] Create account UUID/config only after complete successful authorization flow.
- [ ] Support `connect --reauthorize <alias>`.
- [ ] Ensure failed onboarding leaves no half-connected account.

## Google configuration docs

- [ ] Verify current Google Cloud UI/docs before publishing setup instructions.
- [ ] Document External profile for personal/mixed Gmail+Workspace.
- [ ] Document Internal profile for Workspace-only use.
- [ ] Document Testing-status refresh-token caveat and durable-use recommendation.
- [ ] Document Linux Secret Service troubleshooting/manual prerequisites.

## Tests

- [ ] Mock OAuth-independent profile identity logic.
- [ ] Credential backend abstraction tests without exposing secrets.
- [ ] Duplicate mailbox rejection tests.
- [ ] Reauthorization state tests.
- [ ] Failure cleanup tests.

## Exit criteria

- [ ] Two Gmail accounts can reuse one OAuth client profile while retaining separate authorizations.
- [ ] No repository file contains OAuth secrets/refresh tokens.
- [ ] Missing secure credential store fails closed with useful documentation pointer.
