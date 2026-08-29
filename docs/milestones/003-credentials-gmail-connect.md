# M003 — Credentials and Gmail connect

**Status:** Completed
**Depends on:** M002  
**Outcome:** Secure BYO-OAuth Gmail account onboarding with reusable OAuth client profiles.

## Credential abstraction

- [x] Define secure credential-store interface independent of provider domain model.
- [x] Implement macOS Keychain backend through an appropriate maintained Rust abstraction.
- [x] Implement Linux Secret Service backend.
- [x] Namespace secure items by repository UUID + credential profile/account UUID.
- [x] No plaintext token/client-secret fallback.
- [x] Add redacted credential diagnostics.

## OAuth client profiles

- [x] Define reusable non-secret profile references in repository config/internal state.
- [x] Import Google Desktop OAuth client JSON interactively.
- [x] Validate client JSON deterministically.
- [x] Persist client secret material only to OS credential store.
- [x] Reuse one OAuth client profile across multiple Gmail mailbox authorizations.

## Gmail OAuth

- [x] Use Authorization Code flow with PKCE/local redirect as appropriate for Desktop apps.
- [x] Request only `gmail.modify`.
- [x] Launch local browser; print authorization URL if launch fails.
- [x] Handle callback safely on workstation Linux/macOS.
- [x] Store refresh token securely; keep access token memory-only where practical.
- [x] Fetch authenticated Gmail profile/identity after authorization.
- [x] Reject mailbox duplicate within current repository.
- [x] Create account UUID/config only after complete successful authorization flow.
- [x] Support `connect --reauthorize <alias>`.
- [x] Ensure failed onboarding leaves no half-connected account.

## Google configuration docs

- [x] Verify current Google Cloud UI/docs before publishing setup instructions.
- [x] Document External profile for personal/mixed Gmail+Workspace.
- [x] Document Internal profile for Workspace-only use.
- [x] Document Testing-status refresh-token caveat and durable-use recommendation.
- [x] Document Linux Secret Service troubleshooting/manual prerequisites.

## Tests

- [x] Mock OAuth-independent profile identity logic.
- [x] Credential backend abstraction tests without exposing secrets.
- [x] Duplicate mailbox rejection tests.
- [x] Reauthorization state tests.
- [x] Failure cleanup tests.

## Exit criteria

- [x] Two Gmail accounts can reuse one OAuth client profile while retaining separate authorizations.
- [x] No repository file contains OAuth secrets/refresh tokens.
- [x] Missing secure credential store fails closed with useful documentation pointer.

## Progress log

### 2026-08-29

Implemented OS-keyring credential storage, reusable non-secret Google client
profiles, interactive PKCE/loopback Gmail authorization, transactional account
creation and reauthorization, and explicit Google-plus-local revocation.
Automated acceptance uses fake credential stores and deterministic local OAuth
callbacks; normal tests require neither Gmail credentials nor keyring access.
