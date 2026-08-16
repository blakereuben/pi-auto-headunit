# Development Debian package

This packaging preview builds only `aa-headunit-diagnostics`. It installs no always-on service and does not modify `/boot/firmware/config.txt`.

The package installs no certificate or private key, and creates no `credentials` directory itself — the `credentials install` command creates it, once, the first time an authorised user runs it. `postinst` creates a dedicated `aa-headunit` system group and makes `/etc/aa-headunit` group-writable (`root:aa-headunit`, `0770`) so that first run, and every diagnostic command afterward, works entirely unprivileged for any operator added to that group — no `sudo` needed. The private key file itself keeps its own strict, no-group/no-other `0600` permission check (`crates/credential-store`) unchanged; only the parent directory's own access is widened. See [`docs/development/credential-provisioning.md`](../docs/development/credential-provisioning.md) for the one-time group-membership step and the full provisioning workflow.

**Why this matters beyond convenience**: running the diagnostics CLI as root (previously required just to reach the root-only credentials directory) breaks real audio playback — PulseAudio/PipeWire refuse a connection from a uid that doesn't own the caller's own `XDG_RUNTIME_DIR`, confirmed on real hardware. Running unprivileged fixes that as a side effect, since the CLI then runs as the same uid as the operator's own desktop/PipeWire session.

The udev rule grants the active user/`plugdev` group access to MTP-class devices and the documented Google AOA accessory IDs. Some phones expose neither a matching MTP property nor accessory ID before the AOA transition; those devices will report a permission error until a narrowly reviewed rule is added. Do not solve this by running the diagnostic as root or granting access to all USB devices.

Build on Raspberry Pi OS Trixie with standard Debian source-package tooling from the repository root, using `packaging/debian` as the Debian metadata directory in the release builder.
