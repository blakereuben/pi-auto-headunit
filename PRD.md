# Product Requirements Document

## 1. Product summary

Working name: **Pi Auto Head Unit** (placeholder; a trademark review is required before naming the project).

Pi Auto Head Unit is an open-source, native Android Auto receiver for Raspberry Pi CM4, CM5, Pi 4, and Pi 5 running 64-bit Raspberry Pi OS. The first supported connection is wired USB. The product is intended to start automatically as a full-screen appliance, accept touchscreen input, and use the best low-latency media path available on each supported board.

The project is an independent, uncertified implementation. It must not be represented as Google-certified or as suitable for safety-critical vehicle functions.

## 2. Problem

Existing Raspberry Pi Android Auto projects commonly depend on obsolete Raspberry Pi multimedia APIs, old OS releases, C++ stacks that are hard to evolve, or image/container-first installation. The desired product needs a current 64-bit Raspberry Pi OS foundation, a maintainable Rust core, modern Linux media interfaces, deterministic startup, and normal Debian packaging.

## 3. Goals

1. Run natively on 64-bit Raspberry Pi OS Trixie on CM4, CM5, Pi 4, and Pi 5.
2. Establish and maintain a wired Android Auto session with supported Android phones.
3. Render projected video and play Android Auto audio with bounded latency and stable A/V synchronization.
4. Return calibrated touchscreen input to the phone accurately.
5. Start automatically and recover from cable removal, phone errors, and process failure.
6. Keep transport, protocol/session, media, UI, and board integration independently testable.
7. Install from an `arm64` `.deb` without Docker or a language toolchain on the target.
8. Make the repository understandable and approachable for external contributors.
9. Add wireless Android Auto after wired stability, preferring the CM4/CM5 module's built-in Wi-Fi and Bluetooth when present and supporting approved external USB Wi-Fi/Bluetooth adapters when either capability is absent.
10. Support commercial and custom CM4/CM5 carrier boards, beginning with a Waveshare development carrier and ending with a purpose-built single carrier PCB for the head unit.

## 4. Non-goals for the first release

- Wireless Android Auto.
- Android Automotive OS or an Android-based Raspberry Pi image.
- Apple CarPlay.
- Vehicle CAN-bus control, instrument-cluster integration, climate control, or safety-critical functions.
- Supporting boards other than CM4, CM5, Pi 4, and Pi 5.
- Supporting 32-bit Raspberry Pi OS.
- Google certification or commercial production approval.
- Reverse-engineering undocumented behavior by guesswork in production code.
- A full general-purpose desktop, media centre, radio, navigation shell, or phone mirroring system outside Android Auto.

## 5. Users and primary journeys

### Builder/installer

1. Writes current Raspberry Pi OS 64-bit to storage.
2. Installs the project `.deb` and its repository-resolved dependencies.
3. Runs a diagnostic/preflight command, selects display/audio/input settings, and enables the service.
4. Reboots into the full-screen head-unit experience.

### Driver/passenger

1. Powers the unit on.
2. Sees a ready screen quickly.
3. Connects and unlocks an Android phone over USB.
4. Completes any phone-side consent.
5. Uses Android Auto by touch with working media, navigation prompts, and microphone.
6. Disconnects and can reconnect without rebooting.

### Contributor

1. Builds and tests protocol-independent crates on a normal Linux development host.
2. Uses fake transports and recorded, sanitized fixtures for deterministic tests.
3. Runs explicitly marked hardware tests on a supported Raspberry Pi.

## 6. Functional requirements

| ID | Requirement | Release target |
|---|---|---|
| FR-001 | Detect supported Android USB devices and perform the documented Android Open Accessory transition. | Alpha |
| FR-002 | Establish a wired Android Auto session using only behavior backed by an approved protocol source or verified interoperability evidence. | Alpha |
| FR-003 | Negotiate display capabilities and render the projected stream full-screen without distortion. | Alpha |
| FR-004 | Map touch press, move, release, and multi-touch coordinates into the negotiated projection space. | Alpha |
| FR-005 | Play media, system/navigation, and speech audio with correct focus/mixing policy. | Beta |
| FR-006 | Capture microphone audio for voice interaction, with visible microphone state. | Beta |
| FR-007 | Recover to the ready screen after cable removal, session timeout, or recoverable protocol error. | Alpha |
| FR-008 | Expose device, transport, session, media, and health diagnostics without logging private content. | Alpha |
| FR-009 | Auto-start under systemd and be restartable without rebooting the OS. | Beta |
| FR-010 | Support runtime capability detection for CM4/Pi 4 and CM5/Pi 5 families. | Beta |
| FR-011 | Provide explicit configuration for display geometry, rotation, audio devices, microphone, and touch calibration. | Beta |
| FR-012 | Offer a software-decoding fallback when a negotiated codec lacks a usable hardware decoder. | Beta |
| FR-013 | Preserve configuration across package upgrades and remove generated state cleanly only on explicit purge. | 1.0 |
| FR-014 | Provide a safe shutdown/restart UI action that calls an isolated platform service, not arbitrary shell commands. | 1.0 |
| FR-015 | Detect Wi-Fi and Bluetooth capabilities at runtime instead of assuming that every CM4/CM5 variant includes wireless hardware. | Beta |
| FR-016 | Distinguish wireless hardware that is absent, ready, administratively disabled/rfkill-blocked, or unusable because its driver/firmware failed. | Beta |
| FR-017 | Keep wired Android Auto fully usable when either Wi-Fi or Bluetooth is absent or unavailable. | Beta |
| FR-018 | Add wireless Android Auto after the wired release, using onboard CM4/CM5 radios when available or supported external USB Wi-Fi/Bluetooth adapters when required, subject to approved protocol evidence. | Post-1.0 |
| FR-019 | Load carrier-specific configuration for USB topology, display, touch, audio, power control, GPIO, cooling, and antenna arrangement without changing protocol code. | Beta |
| FR-020 | Select Wi-Fi and Bluetooth providers independently, allowing onboard Wi-Fi plus USB Bluetooth, USB Wi-Fi plus onboard Bluetooth, or external providers for both. | Post-1.0 |
| FR-021 | Show which radio provider is active and give an actionable error when an attached USB adapter lacks a supported driver, firmware, operating mode, or adequate power. | Post-1.0 |
| FR-022 | Provide separate Wi-Fi and Bluetooth provider settings with `Auto`, `Onboard`, and each detected supported USB adapter as choices. | Post-1.0 |

## 7. Non-functional requirements

### Performance targets

These are engineering targets, not claims, until measured on all four board classes.

- Ready screen visible within 8 seconds after Linux userspace begins on the reference appliance image.
- Phone detection within 1 second of USB enumeration.
- Session-ready UI within 8 seconds after the phone has entered accessory mode and granted required consent.
- Sustained 720p30 on all supported boards; 1080p30 is the target where phone negotiation and board capability permit it.
- Glass-to-glass touch response p95 below 120 ms, measured with a repeatable camera-based procedure.
- Audio start latency below 150 ms and no audible underrun in a 60-minute soak test.
- Idle ready-screen memory below 250 MiB and connected steady-state memory below 600 MiB.
- Recover from 100 consecutive cable connect/disconnect cycles without service restart or leaked device handles.

### Reliability

- All long-running queues are bounded and have a documented overflow policy.
- One malformed or unexpected protocol message must end or reject the session safely, not panic the process.
- Persistent state writes are atomic and rate-limited for flash storage.
- A systemd watchdog and health reporting are introduced only after the core loop can distinguish healthy from wedged state.

### Security and privacy

- Run as a dedicated unprivileged account with only the device permissions required for USB, audio, input, rendering, and display.
- Do not run the application as root.
- Do not log microphone samples, projected video, message bodies, contact data, navigation content, or raw encrypted sessions by default.
- Debug capture is opt-in, time-bounded, visibly indicated, and documented as potentially sensitive.
- Parse all phone-originated data as untrusted input and impose frame/message/queue limits.
- Use maintained TLS/cryptographic libraries; do not create custom cryptography.

### Maintainability

- Rust is the default implementation language for transport, session, state machines, policy, and platform abstraction.
- UI and media may use mature system libraries through Rust bindings where this reduces platform risk.
- No board model checks outside the platform/capability layer.
- Public interfaces and protocol state transitions require tests and written invariants.
- CI gates formatting, linting, unit tests, dependency policy, license checks, and an `arm64` package build.

## 8. Supported platform baseline

- OS: Raspberry Pi OS 64-bit, Debian 13 (Trixie) baseline.
- Architecture: Debian `arm64` only.
- Boards: Raspberry Pi 4, Compute Module 4, Raspberry Pi 5, Compute Module 5.
- Display: HDMI is the first reference output. DSI is supported when it appears through the normal DRM/KMS and Linux input interfaces; carrier/display-specific enablement remains configuration.
- Input: evdev/libinput-compatible USB or DSI touchscreen.
- Audio: ALSA devices in appliance mode; a PipeWire-compatible path may be added for desktop coexistence.
- USB: the carrier must expose a port capable of acting as USB host. Carrier-board USB topology is an installer responsibility and must be checked by preflight.
- Wireless: CM4 and CM5 are sold in variants with and without onboard Wi-Fi/Bluetooth. The application must discover actual Linux capabilities at runtime and must not infer wireless availability solely from the module name. It prefers usable onboard radios, then selects approved external USB adapters for any missing capability.
- Wireless settings: `Auto` is the default. Users can independently force onboard or a named USB provider for Wi-Fi and Bluetooth. An unavailable forced provider produces an actionable warning and disables only wireless Android Auto, never wired operation.
- Carrier boards: the first development profile will cover the user's Waveshare board once its exact model/revision is recorded. A generic standards-based profile remains available for other carriers.

Bookworm may be evaluated later, but is not part of the initial compatibility contract. Pinning the baseline avoids silently maintaining two graphics/audio stacks while the protocol is still high risk.

### Initial reference hardware inventory

- **Primary development target:** Raspberry Pi 5, 8 GB RAM, NVMe storage.
- **Compatibility target:** Raspberry Pi 4, 8 GB RAM.
- **Compute Module target:** CM4, 4 GB RAM, eMMC variant without onboard Wi-Fi/Bluetooth.
- **Current CM4 carrier:** Waveshare CM4-IO-BASE-B, Mini Base Board (B), revision 3.1.
- **Current display:** original official Raspberry Pi 7-inch Touch Display, DSI, 800 × 480, ten-point touch.
- **Current wireless plan:** supported USB Wi-Fi and Bluetooth providers on the non-wireless CM4.
- **Current audio options:** USB sound card is available; Pi 4 analog audio may be tested; HDMI audio is usable only with a display/audio sink that exposes it. The official DSI display does not itself solve speaker output.
- **Other hardware pending identification:** exact CarPiHAT model/revision and capabilities.

The Waveshare carrier exposes a 15-pin DSI connector, two onboard USB 2.0 Type-A ports through a hub, an additional USB FFC expansion path, and an M.2 M-key NVMe slot. The reference integration must budget ports and power for the phone, USB Wi-Fi, USB Bluetooth, USB audio, and touchscreen peripherals; a powered hub or the carrier's USB expansion may be required.

The original 800 × 480 display is the first known-good profile, not the product's fixed resolution. Display discovery and configuration must support larger HDMI or DSI touchscreens with different aspect ratios, resolutions, rotations, bezels, and separate USB touch controllers.

## 9. Protocol certainty policy

The public Google documentation confirms that Desktop Head Unit 2.x can connect over USB using Android Open Accessory. It does not publicly specify the complete production head-unit session protocol, media channel messages, or certification requirements.

The project must not reuse shared credentials, certificates, private keys, authentication material, or security bypasses from another head-unit implementation. Google's documented developer-mode DHU/head-unit-server connection may be used as a lab transport, but it does not satisfy production authentication or the wired-release acceptance criteria.

Every protocol feature must be labelled with one of these provenance levels:

- **P0 — public specification:** directly supported by public Google/AOSP documentation.
- **P1 — licensed/authorized specification:** supported by documentation the project is legally permitted to implement, even if it cannot publish that source.
- **P2 — independently verified interoperability:** derived through a documented, legally reviewed clean-room process and backed by repeatable device traces/tests.
- **PX — unknown:** not implemented in release builds.

The project must not copy protocol definitions or implementation code from an incompatible source merely to make a test pass. A legal/license review is an exit criterion for any milestone that adopts reverse-engineered GPL code or data.

## 10. Release acceptance

Version 1.0 requires:

- Wired connection on a published phone/Android Auto compatibility matrix.
- Passing smoke, reconnect, suspend/power-loss, 60-minute media soak, touch calibration, and microphone tests on one reference setup from each SoC family: Pi 4/CM4 and Pi 5/CM5.
- At least one physical test on every named board, including a documented CM4 and CM5 carrier topology.
- Reproducible `arm64` `.deb` generation and clean install/upgrade/remove/purge tests on a fresh supported OS.
- No critical/high known security defect and no unresolved license-provenance blocker.
- Clearly documented fallback behavior when hardware H.264 decoding is unavailable.

## 10.1 Development and validation order

Raspberry Pi 5 is the reference development platform. Features are implemented and brought to end-to-end working state on Pi 5 first, including wired Android Auto, wireless Android Auto, appliance startup, packaging, and stability. Pi 4, CM4/Waveshare, and CM5 physical validation begins only after the Pi 5 completion gate passes.

This sequencing does not relax the final four-board support contract. Protocol, platform, media, UI, and carrier boundaries remain enforced from the start; Linux CI and capability-based code prevent deliberate Pi 5 coupling. A feature is **reference complete** when it passes on Pi 5 and **product complete** only after the required cross-board matrix passes.

## 11. Product decisions requiring approval

1. License the project under GPL-3.0-or-later. Preserve compatible third-party notices and corresponding source obligations in every distributed release.
2. Treat Raspberry Pi OS Trixie 64-bit as the only initial OS baseline.
3. Use GTK 4 for the native shell and GStreamer for media, subject to a short on-device latency/prototype gate.
4. Ship the service disabled by default until preflight succeeds, then provide a one-command enable/start path.
5. Position the software as an experimental aftermarket/R&D project, not a certified automotive product.
6. Permit file-attributed GPL-3.0-or-later behaviour from the pinned AASDK and OpenAuto revisions recorded in `docs/protocol/`, while permanently excluding shared credentials, authentication identities, security bypasses, trademarks, proprietary material, and bundled assets.
7. Treat onboard Wi-Fi/Bluetooth support as an optional discovered capability, never a mandatory property of CM4/CM5; support tested external USB replacements.
8. Design the future custom carrier against the documented CM4/CM5 common interface plus explicit per-module differences; the shared physical form factor is not treated as proof of full electrical/functional equivalence.

## 12. Authoritative references

- Google Desktop Head Unit testing and AOA connection: https://developer.android.com/training/cars/testing/dhu
- Android Open Accessory 1.0 control requests and bulk endpoints: https://source.android.com/docs/core/interaction/accessories/aoa
- Android Open Accessory 2.0 (including deprecated AOA audio mode): https://source.android.com/docs/core/interaction/accessories/aoa2
- Current Raspberry Pi OS baseline: https://www.raspberrypi.com/documentation/usage/raspberry-pi-os/raspberry-pi.html
- Current 64-bit OS board compatibility: https://www.raspberrypi.com/software/operating-systems/
- Compute Module variants, IO-board compatibility, and carrier design resources: https://www.raspberrypi.com/documentation/computers/compute-module.html
- CM4-to-CM5 transition guidance: https://pip-assets.raspberrypi.com/categories/1261-transitioning/documents/RP-008924-WP-1-Transitioning%2520from%2520Compute%2520Module%25204%2520to%2520Compute%2520Module%25205.pdf
- Pi 4 media capabilities: https://www.raspberrypi.com/products/raspberry-pi-4-model-b/specifications/
- Pi 5 media capabilities: https://www.raspberrypi.com/products/raspberry-pi-5/
