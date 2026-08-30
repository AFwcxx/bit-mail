# Release policy

## v0.1 contract

bit-mail v0.1.0 supports Rust 1.88 or newer on Linux and macOS. Rust 1.88 is
the highest minimum required by the locked runtime dependency graph and is
verified by CI.

The v0.1 persistent and machine-readable versions are frozen as follows:

| Contract | Version |
|---|---:|
| Repository metadata | 2 |
| Canonical/config/account/provider files | 1 |
| Integrity encodings and manifests | 1 |
| `help --json`, context, diagnostics, and push JSON | 1 |
| Disposable SQLite index | 1 |
| Embedded runtime assets | binary version (`0.1.0`) |

Pre-integrity repository schema 1 is the only supported pre-release format.
It remains readable and upgrades explicitly with `bit-mail migrate-integrity`.
All other development formats are unsupported; unknown or newer versions fail
without mutation. SQLite is disposable and rebuilt rather than migrated.

## Artifacts

The release workflow builds native archives for:

- Linux x86_64 and arm64;
- macOS x86_64 and arm64.

Each release publishes `SHA256SUMS` and a GitHub/Sigstore build-provenance
attestation for every archive. Verify them with:

```bash
shasum -a 256 -c SHA256SUMS
gh attestation verify bit-mail-v0.1.0-<target>.tar.gz --repo AFwcxx/bit-mail
```

The workflow refuses a tag whose version differs from `Cargo.toml`. A pushed
`v0.1.0` tag builds the four archives and creates the GitHub Release with
generated release notes.
