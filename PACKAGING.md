# Raspberry Pi OS Packaging and Installation Plan

## Recommendation

Ship a normal Debian source package that builds one `arm64` binary package named `aa-headunit`. Use modern `debhelper`/`dh` packaging in a checked-in `packaging/debian/` tree, build in a clean Trixie environment, and publish signed `.deb`, checksum, SBOM, and source artifacts on GitHub Releases. Do not make `cargo-deb`, Docker, or a curl-to-shell installer the primary release path.

Docker may be used internally for reproducible CI build environments, but it is not installed or used on the Raspberry Pi at runtime.

## Target and compatibility

- Debian architecture: `arm64`.
- Distribution baseline: Raspberry Pi OS 64-bit based on Debian 13 Trixie.
- Hardware restriction is enforced by preflight/runtime capability checks, not by inventing Debian architecture names for Pi models.
- Package dependencies use Trixie package names and minimum versions established by the M3/M5 build.
- Bookworm packages, if later supported, are built and tested as a separate distribution target rather than claiming one untested binary works on both.

## Package contents

Planned filesystem placement:

| Content | Location |
|---|---|
| Main executable and diagnostics/setup entry points | `/usr/bin/` |
| Private helper, only if truly needed | `/usr/libexec/aa-headunit/` |
| Read-only assets and schemas | `/usr/share/aa-headunit/` |
| Copyright/changelog | `/usr/share/doc/aa-headunit/` |
| Administrator configuration | `/etc/aa-headunit/config.toml` |
| systemd units | `/usr/lib/systemd/system/` |
| udev rule | `/usr/lib/udev/rules.d/` |
| tmpfiles rule, if needed | `/usr/lib/tmpfiles.d/` |
| Persistent runtime state | `/var/lib/aa-headunit/` via systemd `StateDirectory=` |
| Logs | journald; no custom unbounded log directory |

Do not install application libraries into `/usr/local`, write user home directories, or silently edit `/boot/firmware/config.txt`.

## Service model

Install:

- `aa-headunit-preflight.service`: a oneshot validation dependency.
- `aa-headunit.service`: the supervised appliance runtime, running as dedicated user `aa-headunit`.

The main service should include:

- explicit ordering after the required local filesystems and device discovery, without waiting for network in wired mode;
- restart-on-failure with rate limits;
- journald logging;
- runtime/state directories managed by systemd;
- narrowly tested device access and systemd hardening;
- an optional watchdog only after meaningful health semantics exist;
- a deterministic stop timeout that drains/closes USB and media.

Do not run as root. Do not grant blanket device access. Udev rules should identify the minimum required AOA/accessory devices and grant the dedicated account access. Rendering/input/audio permissions must be separately documented and validated.

## Install and activation experience

Recommended stable flow:

```text
1. Install current Raspberry Pi OS Lite 64-bit (Trixie) and apply OS updates.
2. Download the release `.deb` and checksum/signature from GitHub Releases.
3. Install with apt so declared dependencies are resolved.
4. Run `aa-headunit-diagnostics preflight`; this reports carrier identity, every onboard/USB Wi-Fi and Bluetooth provider, its state, and the provider selected for each capability.
5. Review/select display, audio, microphone, rotation, and touch settings.
   For wireless-capable releases, independently select `Auto`, `Onboard`, or a detected USB adapter for Wi-Fi and Bluetooth.
6. Explicitly enable and start `aa-headunit.service`.
7. Reboot once to validate appliance startup and retain documented recovery access.
```

The exact commands and configuration UI are implementation deliverables, not specified prematurely here.

The service should be installed **disabled and not started automatically** until preflight passes. This avoids taking over the display or failing repeatedly during package installation. Debian integration must encode that policy explicitly (for example, the release's tested `debhelper` equivalent of `dh_installsystemd --no-enable --no-start`); it must not depend on maintainer convention. The eventual setup command can make activation one explicit action.

## Maintainer-script policy

- Keep `preinst`, `postinst`, `prerm`, and `postrm` minimal and idempotent.
- Let debhelper manage systemd integration where possible.
- Create/remove the dedicated system account using Debian-standard mechanisms, or use a static system user declaration if the chosen systemd/Debian tooling supports it reliably.
- Preserve `/etc/aa-headunit/config.toml` as a conffile across upgrades.
- Never overwrite administrator changes silently.
- `remove` leaves configuration and state; `purge` removes package-owned configuration and generated state after stopping the service.
- Do not start the graphical appliance during package upgrade.
- Udev reload should not detach an active unrelated USB device.

## Appliance session

The reference deployment is Raspberry Pi OS Lite plus only the required graphics/input/audio stack and a minimal Wayland compositor/session. The package should depend on distro packages rather than vendoring the compositor. The exact compositor and login/seat arrangement are locked only after M3 validates reliable unprivileged DRM, touch, GTK, and recovery behavior.

Provide two documented modes eventually:

- **Appliance mode (supported reference):** dedicated full-screen compositor/session, fastest measured boot, no desktop dependency.
- **Desktop development mode (best effort):** launch against an existing Wayland session for development; not the boot-time reference and not necessarily service-autostarted.

## Build and release pipeline

1. Pin Rust through `rust-toolchain.toml`; commit `Cargo.lock`.
2. Audit Rust and native dependencies/licenses.
3. Build Debian source and `arm64` binaries in a clean Trixie builder.
4. Run x86-64 architecture-independent tests, an `arm64` compile/package job, and physical Pi hardware tests.
5. Run package lint and install/upgrade/remove/purge tests on a fresh Trixie image.
6. Generate SBOM, source archive, license/copyright file, checksums, and signatures.
7. Publish immutable GitHub Release artifacts.
8. Later, add an APT repository with signed metadata for upgrades; GitHub `.deb` installation remains the first release path.

Cross-compilation is useful for fast feedback but does not replace a native/clean `arm64` package build and physical-device test, because native GStreamer/GTK/USB link and plugin availability matter.

## Dependency approach

- Prefer dynamic linking to Raspberry Pi OS/Debian libraries for libusb, GTK, GStreamer, ALSA, udev, and OpenSSL so security updates arrive through `apt`.
- Build dependencies include `libssl-dev`; the final binary package declares the automatically determined OpenSSL runtime package and must not vendor a private OpenSSL build.
- Package Rust code into the application binary; do not require Rust/Cargo on the target.
- Avoid bundling system multimedia plugins in `/opt` or private library paths.
- Declare required GStreamer plugins explicitly once the validated pipeline is known; do not install broad “everything” bundles without need.
- Use Debian dependency metadata rather than a handwritten installer that invokes package managers unpredictably.

## Upgrade and rollback

- Configuration has a schema version and forward-only migrations with a backup of the previous small config file.
- Runtime state is disposable or explicitly versioned; corrupted caches must not prevent the ready/error UI.
- Package upgrades stop the service, migrate offline if required, and leave it disabled if migration/preflight fails.
- Publish the previous compatible `.deb` and documented downgrade constraints.
- Protocol regressions should be rollbackable without reimaging the OS.

## Verification matrix

Before a stable release, verify:

- clean Trixie Lite install on Pi 4, CM4, Pi 5, and CM5;
- install without a phone/display/audio device attached;
- preflight pass/fail messages and exit status;
- correct wireless capability results on CM4/CM5 variants with and without onboard radios, including rfkill and missing-firmware cases;
- correct selection and diagnostics for supported USB and mixed onboard/USB radio arrangements, including USB hot-unplug and changing enumeration order;
- enable/start/restart/stop/disable behavior;
- package upgrade with unchanged and locally modified configuration;
- remove versus purge semantics;
- no changes outside package-owned/conffile paths;
- boot without network;
- recovery when the UI cannot open DRM/input/audio;
- udev behavior on connect/unplug and after package removal;
- reproducibility or documented remaining variance.

## Rationale and standards

Debian packages are the native distribution unit, and Debian Policy defines architecture, dependency, filesystem, service, and package metadata expectations. Modern `debhelper` automates many policy-sensitive details and is preferable to assembling an archive ad hoc.

References:

- Debian Policy: https://www.debian.org/doc/debian-policy/
- Debian packaging/debhelper guidance: https://www.debian.org/doc/manuals/debmake-doc/ch06.en.html
- Raspberry Pi OS Trixie status: https://www.raspberrypi.com/documentation/usage/raspberry-pi-os/raspberry-pi.html
- Raspberry Pi OS 64-bit compatibility list: https://www.raspberrypi.com/software/operating-systems/
