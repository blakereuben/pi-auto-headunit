# Development Debian package

This packaging preview builds only `aa-headunit-diagnostics`. It does not modify `/boot/firmware/config.txt`.

M5 adds `aa-headunit-preflight.service` and the `aa-headunit-kiosk@.service` template (`packaging/systemd/`) to the package, but `postinst` only runs `systemctl daemon-reload` — it never enables or starts either. Enabling the kiosk unit (`systemctl enable --now aa-headunit-kiosk@<VT>.service`, instantiated against a currently-unused virtual terminal — not whichever one an interactive desktop session is already using) is a deliberate operator decision; see `aa-headunit-kiosk@.service`'s own doc comment for what is and isn't real-hardware-confirmed before doing that.

The package installs no certificate or private key, and creates no `credentials` directory itself — the `credentials install` command creates it, once, the first time an authorised user runs it. `postinst` creates a dedicated `aa-headunit` system group and makes `/etc/aa-headunit` group-writable (`root:aa-headunit`, `0770`) so that first run, and every diagnostic command afterward, works entirely unprivileged for any operator added to that group — no `sudo` needed. The private key file itself keeps its own strict, no-group/no-other `0600` permission check (`crates/credential-store`) unchanged; only the parent directory's own access is widened. See [`docs/development/credential-provisioning.md`](../docs/development/credential-provisioning.md) for the one-time group-membership step and the full provisioning workflow.

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
