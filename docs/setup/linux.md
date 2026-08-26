# Linux setup

v1 supports Linux.

Credential storage target: Secret Service / desktop keyring backend. `bit-mail connect` should automate storage when a usable session is available.

If Secret Service is unavailable/locked/unusable, `bit-mail` must not fall back to plaintext tokens. It should fail closed and provide actionable diagnostics/manual setup guidance.

Exact package-specific Secret Service examples should be added and verified during the implementation/release milestone.
