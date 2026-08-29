# Linux setup

v1 supports Linux.

Credentials use Secret Service through the desktop keyring. A D-Bus session,
a Secret Service provider such as GNOME Keyring or KWallet, and an unlocked
login collection must be available before running `bit-mail connect`.

If Secret Service is unavailable/locked/unusable, `bit-mail` must not fall back to plaintext tokens. It should fail closed and provide actionable diagnostics/manual setup guidance.

On a headless shell, run the command inside a graphical/login D-Bus session or
configure a Secret Service provider first. Check `DBUS_SESSION_BUS_ADDRESS`,
confirm the keyring is unlocked, and retry. The tool never writes a plaintext
fallback; its error points back to this document.
