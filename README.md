# Pi Auto Head Unit

**Project name:** Pi Auto Head Unit (working-name placeholder)

A native, open-source Android Auto head-unit project for Raspberry Pi Compute Module 4, Compute Module 5, Pi 4, and Pi 5, running 64-bit Raspberry Pi OS.

The aim is an appliance-style system that starts with the car, connects to an Android phone, displays Android Auto on a touchscreen, and uses Raspberry Pi hardware acceleration where available. The primary runtime is a normal Raspberry Pi OS application—not Docker—and the finished installer will be a Debian (`.deb`) package managed by systemd.

> [!NOTE]
> **AI authorship disclosure:** The code and documentation in this repository were created through an AI-assisted process using more than one AI coding tool: the project was originated with OpenAI Codex, and ongoing development (including the current Android Auto session/protocol work) continues using Claude (Anthropic), acting as Claude Code. Blake Reuben has acted as the project owner and orchestrator throughout: defining the goals and requirements, supplying the hardware and test environment, approving the work, and carrying out or supervising physical testing. See [AI_DISCLOSURE.md](AI_DISCLOSURE.md) for details.

> [!IMPORTANT]
> This project is experimental, uncertified software for bench development and must not control safety-critical vehicle functions. Wired and wireless Android Auto sessions — full protocol handshake, live video, audio, touch, and microphone — are real-hardware-confirmed (see below). What is **not** yet done: cold-boot and wired/wireless-switching determinism, sustained/soak testing, and the other M8 completion-gate items listed in [`MILESTONE_CHECKLIST.md`](MILESTONE_CHECKLIST.md). Treat this as a working preview, not a stress-tested release.

## Features and current status

Milestones 0 through 5 are functionally complete against real hardware. Milestone 6 (wired hardening) and Milestone 7 (wireless Android Auto) are substantially complete. Milestone 8 (the Pi 5 completion gate — cold-boot determinism, failure-mode and soak testing, the compatibility matrix, and a full security pass) is the current, open milestone — see [`MILESTONE_CHECKLIST.md`](MILESTONE_CHECKLIST.md) for the authoritative, item-by-item status.

Working today, real-hardware-confirmed:

- a full-screen GTK4 kiosk app (`usb kiosk`) that reconnects automatically, wired or wireless, with no operator intervention needed after first setup;
- complete wired **and** wireless Android Auto sessions — protocol version negotiation, TLS, authentication, service discovery, and channel setup, followed by live video, audio (media/system/speech), microphone capture, and touch (including two-finger multi-touch) all confirmed working end-to-end against a real phone on the official 7-inch touchscreen;
- wireless bootstrap (Wi-Fi access point + Bluetooth handoff) with no prior wired pairing required;
- a guided GTK4 credential setup wizard, and a single-entry-point installer (`packaging/setup.sh`) that collects the operator's certificate/private key before installing the `.deb`;
- a settings panel reachable by touch gesture: display rotation, themes, renamable EQ presets, gesture-to-action mapping, Wi-Fi/Bluetooth provider selection, night-mode GPIO input, and a "launch on boot" toggle (off by default — see the appliance-recovery doc for why);
- automatic USB reconnect/replug recovery, and a session supervisor that survives phone-side rejects, timeouts, and service restarts;
- board-independent video-decoder selection and a native GStreamer capability probe;
- an `arm64` Debian package with unprivileged USB/network access rules, a dedicated system user/group, persistent settings, and bounded journald logging.

Verified on the Pi 5 reference system:

- Raspberry Pi 5, 8 GB, booting from NVMe or SD card;
- Debian 13/Raspberry Pi OS Trixie, 64-bit;
- a real phone completing the full protocol handshake — version negotiation, TLS, authentication, service discovery, and channel setup — over both a wired USB/AOA connection and a wireless (Wi-Fi AP + Bluetooth handoff) connection, with no prior wired pairing needed for the wireless path;
- live H.264 video, all three audio channels (media/system/speech), and microphone capture, all confirmed streaming on the head unit's own physical display, not just in logs;
- touch, including two-finger multi-touch, confirmed working during a live session on the official 7-inch touchscreen;
- automatic reconnect after a phone disconnect, a USB replug, and a deliberate operator close/reopen;
- package install, remove, purge, and clean reinstall.

Not yet done, and explicitly tracked as open in [`MILESTONE_CHECKLIST.md`](MILESTONE_CHECKLIST.md)'s M8 section: cold-boot determinism, deterministic wired/wireless switching, sustained-use/soak testing, power-interruption and low-storage/thermal failure-mode testing, a full whole-application security pass, and the phone/display/audio/USB-radio compatibility matrix. Treat this release as functionally working but not yet stress-tested.

Detailed results are recorded in [the Pi 5 evidence report](docs/hardware/evidence/pi5-2026-08-04.md) and [`MILESTONE_CHECKLIST.md`](MILESTONE_CHECKLIST.md).

## Supported hardware target

| Hardware | Intended support | Current validation |
|---|---|---|
| Raspberry Pi 5 | Yes | Primary development/reference board |
| Raspberry Pi 4 | Yes | Physical testing follows the complete Pi 5 wired/wireless completion gate |
| Compute Module 5 | Yes | Carrier-board testing planned |
| Compute Module 4 | Yes | Waveshare Mini Base Board (B) Rev 3.1 planned |

Only 64-bit Raspberry Pi OS is targeted. Other Raspberry Pi models and 32-bit operating systems are outside the compatibility contract.

The first display is the official 7-inch Raspberry Pi touchscreen. The design is resolution-independent so larger DSI or HDMI displays, different aspect ratios, rotation, bezel sizes, and USB touch controllers can be configured later.

## Wi-Fi and Bluetooth design

The software checks what Linux can actually use; it does not assume every Compute Module includes wireless hardware.

- If onboard Wi-Fi or Bluetooth is present and working, `Auto` prefers it.
- If either capability is absent, a supported USB adapter can provide it.
- Wi-Fi and Bluetooth are selected independently, so mixed onboard/USB combinations are possible.
- A settings screen will eventually expose `Auto`, `Onboard`, and each detected supported USB device.
- A missing or broken radio disables only future wireless Android Auto; wired Android Auto remains available.

Wireless Android Auto is deliberately scheduled after a stable wired release. Adapter support will be published by tested chipset and USB ID rather than claimed for every dongle.

## Requirements and dependencies

- Raspberry Pi 5, Pi 4, CM5, or CM4; development and physical validation currently use the Pi 5 only.
- 64-bit Raspberry Pi OS based on Debian 13 (Trixie).
- A USB host port and data-capable USB cable for wired phone testing.
- Rust 1.85 or newer, Cargo, a C build toolchain, `pkg-config`, and the libusb, OpenSSL, and GStreamer development packages when building from source.
- An Android phone for hardware tests; no phone is required for ordinary unit tests.
- The Raspberry Pi OS `adb` package only for the optional official developer-mode head-unit-server workflow; it is not a release runtime dependency.

Current direct Rust and native dependencies and their licences are recorded in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Future display, audio, Bluetooth, and networking dependencies will be documented when they are actually added.

## Installation

Download the latest `arm64` `.deb`, `SHA256SUMS`, and `SHA256SUMS.asc` from the [Releases page](https://github.com/blakereuben/pi-auto-headunit/releases), then verify and install:

```bash
gpg --import packaging/release-signing-key.asc   # one-time: import the release-signing key
sha256sum --check SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS

./packaging/setup.sh aa-headunit-diagnostics_*.deb
```

`setup.sh` collects your Android Auto certificate/private key first (a guided GTK4 wizard), then installs the package — a single entry point, no separate manual steps. See [the current release's known limitations](MILESTONE_CHECKLIST.md) (M8 is still open: stress/soak testing and a few other completion-gate items remain) before relying on this for anything but bench testing.

To build from source instead, on a 64-bit Raspberry Pi OS development system:

```bash
sudo apt update
sudo apt install build-essential cargo libgstreamer1.0-dev libssl-dev libusb-1.0-0-dev pkg-config rustc
git clone https://github.com/blakereuben/pi-auto-headunit.git
cd pi-auto-headunit
cargo build --workspace --locked
```

See [PACKAGING.md](PACKAGING.md) for the release packaging plan.

## Usage

This is for development on a 64-bit Raspberry Pi OS system. Rust and the native build dependencies are currently required; end users should wait for a published `.deb` release.

```bash
cargo run -p aa-headunit-diagnostics -- preflight
cargo run -p aa-headunit-diagnostics -- wireless
cargo run -p aa-headunit-diagnostics -- media probe
cargo run -p aa-headunit-diagnostics -- developer tcp-probe
cargo run -p aa-headunit-diagnostics -- usb list
cargo run -p aa-headunit-diagnostics -- usb aoa --device BUS:ADDRESS
```

After a phone is already in accessory mode, developers can repeat the safe interface open/close check:

```bash
cargo run -p aa-headunit-diagnostics -- usb soak --device BUS:ADDRESS --cycles 100
cargo run -p aa-headunit-diagnostics -- usb hold --device BUS:ADDRESS --seconds 30
```

The AOA command requires an explicit USB bus/address. It does not send vendor control requests indiscriminately to every attached USB device. After an AOA transition, `usb hold` keeps the selected accessory interface open for a controlled physical-unplug test.

The repository retains explicitly guarded TLS bench diagnostics as reproducible evidence of the Android Auto error-7 security rejection, which occurred over both normal USB/AOA and Google's developer-mode ADB tunnel when using a temporary, project-generated identity. That generated-identity experiment is complete and permanently locked out in code; it must not be repeated or repurposed with credentials from another head-unit implementation. Session/protocol development against real hardware now instead uses an operator-authorized external credential, loaded only from a root-only local configuration file that is never committed to this repository, via the gated `usb auth-discovery-probe --device BUS:ADDRESS --allow-live-aap` command (also available as `developer auth-discovery-probe`). That path reaches version negotiation, TLS, authentication, service discovery, and full channel setup, and now hits Android Auto's "Error 2" — see [the Error 2 investigation](docs/protocol/error-2-investigation.md). Fake peers remain the default for everyday development and ordinary tests; only the explicitly gated live probes above touch a real phone.

`developer tcp-probe` only checks the loopback endpoint `127.0.0.1:5277`. It is intended for a user-enabled Android Auto head-unit server forwarded with ADB, and it neither sends Android Auto messages nor provides authentication. Developer mode did not make a project-generated identity acceptable to the phone.

## Architecture

The project keeps responsibilities separate so board changes do not leak into protocol or UI code:

```text
USB/wireless transport -> protocol and session -> media pipelines -> touchscreen UI
                              |
                              +-> platform and carrier-board services
```

Rust is preferred for transport, state machines, policy, and platform interfaces. Board-specific behavior sits behind capability interfaces. GStreamer is now the measured media backend; the full-screen UI toolkit remains to be selected after a Pi 5 spike.

Read [ARCHITECTURE.md](ARCHITECTURE.md) for component boundaries and [REPO_LAYOUT.md](REPO_LAYOUT.md) for the source-tree map.

## Roadmap

Development remains exclusively on the Pi 5 reference system until both wired and wireless Android Auto, appliance startup, packaging, and stability are complete. Only then does physical validation and adaptation begin on Pi 4, CM4, and CM5.

1. Documented USB/AOA diagnostic — complete
2. Lawful protocol feasibility and session skeleton — functionally complete; a live session reaches video `Start` but is blocked by "Error 2" (see above)
3. Hardware-accelerated media, audio, microphone, UI, and touch spike — partially complete (touch and H.264 software decode verified; UI shell, audio route, and microphone selection remain open)
4. Complete wired projection on Pi 5 — blocked on resolving Error 2
5. Appliance startup, systemd integration, and release packaging
6. Complete wireless Android Auto on Pi 5
7. Pi 5 wired/wireless completion gate
8. Pi 4, CM4, and CM5 porting and validation
9. Project 1.0 and custom CM4/CM5 carrier PCB track

Track progress in [MILESTONE_CHECKLIST.md](MILESTONE_CHECKLIST.md). See [MILESTONES.md](MILESTONES.md) for the supporting deliverables and exit gates.

## Protocol and legal policy

The public Android Open Accessory requests are implemented from AOSP documentation. Google does not publicly document the complete production Android Auto head-unit protocol.

Undocumented behavior is not guessed. Each protocol feature must be backed by a public specification or an approved licensed source. Proprietary Google code remains excluded. GPLv3/GPL-3.0-or-later AASDK-, OpenAuto-, and LIVI-derived behaviour is permitted only when its exact source files and attribution are added to the [AASDK](docs/protocol/aasdk-adoption.md), [OpenAuto](docs/protocol/openauto-adoption.md), or [LIVI](docs/protocol/livi-adoption.md) adoption record — each records the pinned upstream revision, licence declaration, and exactly which behaviour was derived from which file. Every adoption is behaviour reimplemented independently in this project's own Rust architecture, not code translated or copied line-by-line from the source project. Credentials, certificates, keys, authentication material, security bypasses, trademarks, and bundled assets from OpenAuto, OpenAuto Pro, AASDK, LIVI, Google, or another receiver remain permanently prohibited regardless of source-code licence — this project uses only its own operator-authorized credential, loaded exclusively from a root-only local file, never committed to this repository or derived from another project's embedded identity.

See the [product requirements](PRD.md) and [risk register](RISK_REGISTER.md) for the full policy.

## Repository guide

- [`apps/aa-headunit-diagnostics`](apps/aa-headunit-diagnostics) — current command-line diagnostic
- [`crates/transport-api`](crates/transport-api) — transport interfaces and AOA state machine
- [`crates/transport-usb`](crates/transport-usb) — Linux/libusb implementation
- [`crates/platform-api`](crates/platform-api) — board/platform capability interfaces
- [`crates/platform-linux`](crates/platform-linux) — Raspberry Pi OS discovery implementation
- [`crates/media-api`](crates/media-api) — board-independent decoder requirements and selection
- [`crates/media-gstreamer`](crates/media-gstreamer) — Linux GStreamer capability adapter
- [`crates/protocol-aap`](crates/protocol-aap) — bounded GPL-derived framing, discovery parsing, and wire-neutral service catalogue
- [`crates/security-openssl`](crates/security-openssl) — Linux OpenSSL adapter with injected credentials
- [`crates/credential-store`](crates/credential-store) — bounded local validation and installation of user-supplied credentials
- [`packaging/debian`](packaging/debian) — development Debian package metadata
- [`docs`](docs) — design decisions, protocol evidence, and hardware reports

## Development and build checks

The repository is a Rust workspace. Protocol, transport, platform, media, and UI responsibilities remain separated so hardware-specific work cannot silently enter protocol code. Hardware tests must be selected explicitly; ordinary contributor checks do not probe connected USB devices.

Run before submitting changes:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Contributing

Contributions and hardware test reports will be welcome as the project matures. Please start by reading [ARCHITECTURE.md](ARCHITECTURE.md), [MILESTONE_01.md](MILESTONE_01.md), and the protocol certainty rules in [PRD.md](PRD.md). Do not submit copied protocol definitions, private captures, secrets, phone content, or code with unclear licensing.

By contributing, you must have the right to submit the work under GPL-3.0-or-later and must preserve applicable third-party copyright and licence notices. The contribution process may gain a Developer Certificate of Origin sign-off before outside contributions are accepted.

## Documentation

- [Product requirements](PRD.md)
- [Architecture](ARCHITECTURE.md)
- [Repository layout](REPO_LAYOUT.md)
- [Milestones](MILESTONES.md)
- [Milestone checklist](MILESTONE_CHECKLIST.md)
- [Exact first milestone](MILESTONE_01.md)
- [Risk register](RISK_REGISTER.md)
- [Android Auto protocol source assessment](docs/protocol/source-assessment-2026-08-04.md)
- [AASDK adoption record](docs/protocol/aasdk-adoption.md)
- [OpenAuto adoption record](docs/protocol/openauto-adoption.md)
- [LIVI adoption record](docs/protocol/livi-adoption.md)
- [Protocol certainty matrix](docs/protocol/certainty-matrix.md)
- [Error 2 investigation](docs/protocol/error-2-investigation.md) — the current blocker to a complete wired session, every hypothesis tried, and what's ruled in/out
- [Receiver provisioning options](docs/protocol/receiver-provisioning-options.md)
- [Packaging and installation plan](PACKAGING.md)

## License and disclaimer

Copyright (C) 2026 Blake Reuben and contributors.

This project is free software licensed under the GNU General Public License, version 3 or (at your option) any later version (`GPL-3.0-or-later`). See [LICENSE](LICENSE), [COPYING](COPYING), and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Pi Auto Head Unit is a working name. This is an independent project and is not affiliated with, endorsed by, sponsored by, or certified by Google LLC. Android and Android Auto are trademarks of Google LLC. No Google logos or brand assets are distributed by this repository.
