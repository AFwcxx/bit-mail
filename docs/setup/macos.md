# macOS setup

v1 supports macOS.

Credentials use the current user's macOS Keychain. `bit-mail connect` may
trigger the normal Keychain permission or unlock prompt. Run it from the same
login session that will run provider-facing commands.

Runtime private directories/files should be created with restrictive permissions even though user home directories may already be protected.

If access fails, unlock the login keychain in Keychain Access and retry. No
plaintext credential fallback is available. Supported v1 distribution remains
GitHub Release binaries plus Cargo/source build.
