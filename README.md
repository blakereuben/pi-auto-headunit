# Pi Auto Head Unit

**Project name:** Pi Auto Head Unit (working-name placeholder)

A native, open-source Android Auto head-unit project for Raspberry Pi Compute Module 4, Compute Module 5, Pi 4, and Pi 5, running 64-bit Raspberry Pi OS.

The aim is an appliance-style system that starts with the car, connects to an Android phone, displays Android Auto on a touchscreen, and uses Raspberry Pi hardware acceleration where available. The primary runtime is a normal Raspberry Pi OS application—not Docker—and the finished installer will be a Debian (`.deb`) package managed by systemd.

> [!NOTE]
> **AI authorship disclosure:** The current code and documentation in this repository were created by OpenAI Codex. Blake Reuben has acted as the project owner and orchestrator: defining the goals and requirements, supplying the hardware and test environment, approving the work, and carrying out or supervising physical testing. See [AI_DISCLOSURE.md](AI_DISCLOSURE.md) for details.

> [!IMPORTANT]
> This project is at an early hardware-diagnostics stage. It does **not** yet display Android Auto, play its audio, return touch input, or support wireless Android Auto. It is experimental, uncertified software for bench development and must not control safety-critical vehicle functions.

## Features and current status

Milestone 1 is building and testing the safe USB foundation needed before the Android Auto session itself.

Working today:

- native Rust build on 64-bit Raspberry Pi OS;
- Raspberry Pi model, OS, and architecture preflight checks;
- runtime detection of onboard and USB Wi-Fi/Bluetooth providers;
- independent `Auto`, `Onboard`, or explicit USB provider selection for Wi-Fi and Bluetooth;
- USB device discovery;
- documented Android Open Accessory (AOA) negotiation;
- phone re-enumeration detection and accessory bulk-endpoint discovery;
- clean unplug/reconnect handling;
- board-independent video-decoder selection and a native GStreamer capability probe;
- bounded AAP framing, reassembly, control-handshake logic, and a replaceable OpenSSL TLS client;
- an `arm64` development `.deb` with unprivileged USB access rules.

Verified on the Pi 5 reference system:

- Raspberry Pi 5, 8 GB, booting from NVMe;
- Debian 13/Raspberry Pi OS Trixie, 64-bit;
- Samsung phone entering AOA 2 accessory mode;
- reconnect without rebooting the Pi;
- 100/100 repeated interface claim/release cycles;
- no increase in open file handles or resident memory during that soak;
- package install, remove, purge, and clean reinstall.
- Pi 5 H.264 software decoding selected at 800x480/30 with Wayland output.

Detailed results are recorded in [the Pi 5 evidence report](docs/hardware/evidence/pi5-2026-08-04.md).

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

There is not yet an end-user release. The current diagnostic can be built on a 64-bit Raspberry Pi OS development system:

```bash
sudo apt update
sudo apt install build-essential cargo libgstreamer1.0-dev libssl-dev libusb-1.0-0-dev pkg-config rustc
git clone https://github.com/blakereuben/pi-auto-headunit.git
cd pi-auto-headunit
cargo build --workspace --locked
```

The finished release installation will use an `arm64` Debian package installed through `apt`, followed by preflight/setup and explicit systemd service activation. It will not require Docker or a Rust toolchain. See [PACKAGING.md](PACKAGING.md) for the release packaging plan.

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

The repository retains an explicitly guarded TLS bench diagnostic as reproducible evidence of the Android Auto error-7 security rejection. That generated-identity experiment is complete and must not be repeated or repurposed with credentials from another head-unit implementation. Continued session development will use fake peers and Google's documented developer-mode head-unit server path.

`developer tcp-probe` only checks the loopback endpoint `127.0.0.1:5277`. It is intended for a user-enabled Android Auto head-unit server forwarded with ADB, and it neither sends Android Auto messages nor provides production authentication.

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

1. Documented USB/AOA diagnostic — in progress
2. Lawful protocol feasibility and session skeleton
3. Hardware-accelerated media, audio, microphone, UI, and touch spike
4. Complete wired projection on Pi 5
5. Appliance startup, systemd integration, and release packaging
6. Complete wireless Android Auto on Pi 5
7. Pi 5 wired/wireless completion gate
8. Pi 4, CM4, and CM5 porting and validation
9. Project 1.0 and custom CM4/CM5 carrier PCB track

Track progress in [MILESTONE_CHECKLIST.md](MILESTONE_CHECKLIST.md). See [MILESTONES.md](MILESTONES.md) for the supporting deliverables and exit gates.

## Protocol and legal policy

The public Android Open Accessory requests are implemented from AOSP documentation. Google does not publicly document the complete production Android Auto head-unit protocol.

Undocumented behavior is not guessed. Each protocol feature must be backed by a public specification or an approved licensed source. Google and OpenAuto code remain excluded. GPLv3 AASDK-derived behaviour is permitted only when its exact source files and attribution are added to the [AASDK adoption record](docs/protocol/aasdk-adoption.md). Credentials, certificates, keys, authentication material, and security bypasses from OpenAuto, OpenAuto Pro, AASDK, or another receiver are prohibited regardless of source-code licence.

See the [product requirements](PRD.md) and [risk register](RISK_REGISTER.md) for the full policy.

## Repository guide

- [`apps/aa-headunit-diagnostics`](apps/aa-headunit-diagnostics) — current command-line diagnostic
- [`crates/transport-api`](crates/transport-api) — transport interfaces and AOA state machine
- [`crates/transport-usb`](crates/transport-usb) — Linux/libusb implementation
- [`crates/platform-api`](crates/platform-api) — board/platform capability interfaces
- [`crates/platform-linux`](crates/platform-linux) — Raspberry Pi OS discovery implementation
- [`crates/media-api`](crates/media-api) — board-independent decoder requirements and selection
- [`crates/media-gstreamer`](crates/media-gstreamer) — Linux GStreamer capability adapter
- [`crates/protocol-aap`](crates/protocol-aap) — bounded GPL-derived Android Auto frame codec
- [`crates/security-openssl`](crates/security-openssl) — Linux OpenSSL adapter with injected credentials
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
- [Packaging and installation plan](PACKAGING.md)

## License and disclaimer

Copyright (C) 2026 Blake Reuben and contributors.

This project is free software licensed under the GNU General Public License, version 3 or (at your option) any later version (`GPL-3.0-or-later`). See [LICENSE](LICENSE), [COPYING](COPYING), and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Pi Auto Head Unit is a working name. This is an independent project and is not affiliated with, endorsed by, sponsored by, or certified by Google LLC. Android and Android Auto are trademarks of Google LLC. No Google logos or brand assets are distributed by this repository.
