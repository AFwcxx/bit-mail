# Credentials

## Storage

OAuth client secret material and refresh tokens are stored in the OS credential store:

- macOS: Keychain;
- Linux: Secret Service/keyring backend.

No plaintext fallback exists in v1.

Repository configuration stores only stable credential references.

Credential keys are namespaced with immutable repository UUID and account UUID so aliases/path moves cannot cause collisions.

## Connect behavior

`bit-mail connect` should automate secure credential storage. If secure storage is unavailable, it fails closed and directs the user to OS-specific setup documentation.

## Portability

Moving a repository on the same machine retains access because credentials are keyed by repository/account identity rather than absolute path.

Copying a repository to another machine does not copy authentication capability. Offline triage works, but provider-facing operations require reauthorization.
