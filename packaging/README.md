# Development Debian package

This packaging preview builds only `aa-headunit-diagnostics`. It does not modify `/boot/firmware/config.txt`.

M5 adds an autostart template, `packaging/labwc/aa-headunit-autostart`, shipped at `/usr/share/aa-headunit/labwc-autostart`. This is a program installed onto an existing PiOS desktop, not appliance hardware with its own dedicated login account, so `postinst` detects whichever real account is already running the desktop (the active `seat0` session) at install time and, for that account: adds it to the `aa-headunit` permissions group (see below), copies the autostart template to its own `~/.config/labwc/autostart` (overwritten on every install/upgrade — it's package-managed content, not meant to be hand-edited), and — only if LightDM's `autologin-user` isn't already configured — points autologin at it. An earlier design ran a separately-managed `cage` kiosk compositor as a dedicated `aa-headunit` login account on a spare VT; that was dropped for two independent reasons: the operator wants a real desktop underneath the app, not a bare compositor, and this app should autologin as whoever already uses the machine, not a separate account. The autostart script itself runs `aa-headunit-diagnostics preflight` (ARCHITECTURE.md §9) and only launches the fullscreen app (`usb kiosk --allow-live-aap`) if that passes, otherwise leaving the plain desktop up. See `docs/development/appliance-recovery.md` for full detail and current status.

`aa-headunit` remains a dedicated system group/account, but no longer as a login identity — only as the permissions anchor described next.

The package installs no certificate or private key, and creates no `credentials` directory itself — the `credentials install` command creates it, once, the first time an authorised user runs it. `postinst` creates a dedicated `aa-headunit` system group and makes `/etc/aa-headunit` group-writable (`root:aa-headunit`, `0770`) so that first run, and every diagnostic command afterward, works entirely unprivileged for any operator in that group — no `sudo` needed. `postinst` now adds the detected active-desktop user to this group automatically (previously a manual step); [`docs/development/credential-provisioning.md`](../docs/development/credential-provisioning.md) still has the full provisioning workflow and the manual step for any other account that needs it. The private key file itself keeps its own strict, no-group/no-other `0600` permission check (`crates/credential-store`) unchanged; only the parent directory's own access is widened.

**Why this matters beyond convenience**: running the diagnostics CLI as root (previously required just to reach the root-only credentials directory) breaks real audio playback — PulseAudio/PipeWire refuse a connection from a uid that doesn't own the caller's own `XDG_RUNTIME_DIR`, confirmed on real hardware. Running unprivileged fixes that as a side effect, since the CLI then runs as the same uid as the operator's own desktop/PipeWire session.

The udev rule grants the active user/`plugdev` group access to MTP-class devices and the documented Google AOA accessory IDs. Some phones expose neither a matching MTP property nor accessory ID before the AOA transition; those devices will report a permission error until a narrowly reviewed rule is added. Do not solve this by running the diagnostic as root or granting access to all USB devices.

Build on Raspberry Pi OS Trixie with standard Debian source-package tooling from the repository root, using `packaging/debian` as the Debian metadata directory in the release builder.

## Release signing

`packaging/release-signing-key.asc` is this project's release-signing GPG public key (`Pi Auto Head Unit Release Signing <noreply@example.invalid>`, fingerprint `E2AD 0E68 BF8F F960 92ED  C7C1 E2EE F981 0D5B 36F5`, expires 2028-08-17) — safe to publish, and committed here so anyone verifying a release doesn't need to fetch it from a keyserver. The matching private key lives only in the maintainer's own GPG keyring on the build machine; it is never committed and never will be. This is a detached-signature-over-checksums scheme (matching a GitHub-Releases-style standalone `.deb` download, per the M8 checklist's "publish a Pi 5 preview `.deb`, checksums, source..."), not a full signed APT repository — a real option later if this project ever needs `apt install` from a hosted archive, but more infrastructure than a pre-1.0 project needs yet.

To sign a release build, from the repository root after `dpkg-buildpackage`:

```
packaging/sign-release.sh ../aa-headunit-diagnostics_X.Y.Z_arm64.deb
```

produces `SHA256SUMS` and `SHA256SUMS.asc` alongside it. Publish all three (the `.deb`, `SHA256SUMS`, `SHA256SUMS.asc`) together.

To verify a downloaded release (what an end user runs):

```
gpg --import packaging/release-signing-key.asc
sha256sum --check SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS
```

A `WARNING: The key's User ID is not certified with a trusted signature!` from `gpg --verify` is expected and not an error — it just means the verifier hasn't personally attested to the key, which anyone can do by manually checking the fingerprint above matches the key they imported.
