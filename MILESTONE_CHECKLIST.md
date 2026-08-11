# Milestone Checklist

This is the operational progress tracker for the project. Detailed requirements and exit criteria remain in `PRD.md`, `ARCHITECTURE.md`, and `MILESTONES.md`.

## Execution rule

**No physical Pi 4, CM4, or CM5 compatibility testing begins until the Pi 5 reference system has complete wired Android Auto, complete wireless Android Auto, appliance startup, packaging, and stability evidence.**

Architecture and automated tests must remain portable throughout development, but the Pi 5 is the only physical implementation target until the Pi 5 completion gate is checked.

## M0 — Project and Pi 5 foundation

- [x] Define product requirements, architecture, repository layout, risks, and packaging plan.
- [x] Create the public GitHub repository and continuous-integration workflow.
- [x] Record the AI-authorship/project-orchestration disclosure.
- [x] Prepare the Pi 5 8 GB/NVMe reference system for native Rust development.
- [x] Confirm 64-bit Raspberry Pi OS/Debian Trixie baseline and unprivileged hardware groups.
- [x] Select GPL-3.0-or-later and add the complete licence and third-party notices.
- [ ] Complete the protocol-source, trademark, and clean-room policy review needed for implementation.

## M1 — Pi 5 documented USB/AOA foundation

- [x] Detect USB devices without probing every device indiscriminately.
- [x] Perform the publicly documented Android Open Accessory transition.
- [x] Detect phone re-enumeration as `18d1:2d00` accessory mode.
- [x] Discover the bulk interface and IN/OUT endpoints.
- [x] Access the phone without running the diagnostic as root.
- [x] Handle a normal physical unplug cleanly.
- [x] Reconnect without rebooting the Pi or resetting the service.
- [x] Pass 100/100 repeated interface claim/release cycles.
- [x] Show no file-handle or resident-memory growth during the interface soak.
- [x] Pass native formatting, strict linting, and all 23 tests on the Pi 5.
- [x] Build, install, remove, purge, and cleanly reinstall the development `.deb`.
- [x] Upgrade from `0.0.9-1` to `0.1.0-1` while preserving a locally modified configuration.
- [x] Detect a physical unplug while the accessory bulk interface is actively claimed.
- [x] Inject unplug at every backend-driven AOA state in deterministic automated tests.
- [ ] Record a final Pi 5 M1 report and mark the milestone reference-complete.

## M2 — Lawful Android Auto session foundation on Pi 5

- [x] Survey official public documentation and open-source receiver/protocol candidates without copying protocol material.
- [x] Record why AASDK's GPLv3 declaration is licence-compatible but does not alone resolve protocol provenance.
- [x] Record the initial project-owner decision to approve GPLv3 AASDK while temporarily excluding OpenAuto code.
- [x] Record the 5 August 2026 approval of pinned GPLv3 OpenAuto code/behaviour with permanent credential, security-bypass, trademark, identity, and asset exclusions.
- [x] Pin the approved AASDK revision and record file-level attribution for the first framing scope.
- [x] Implement bounded Rust frame encoding/decoding without sending protocol messages to a phone.
- [x] Add bounded per-channel message reassembly with deterministic interleaving and malformed-sequence tests.
- [x] Implement the bounded version/TLS/authentication/service-discovery control state sequence against fake TLS data.
- [x] Add a replaceable, bounded OpenSSL TLS client with injected credentials and no embedded shared key.
- [x] Add offline validation and non-overwriting installation for authorised user-supplied credential files.
- [x] Build and install the credential-aware ARM64 `.deb` on Pi 5; verify empty-package, missing, mismatched, valid synthetic, configured status, and non-overwrite states.
- [x] Add an explicit live TLS bench probe using fresh in-memory credentials and a hard stop before authentication/service discovery.
- [x] Pass native formatting, strict linting, and all 78 workspace tests after adding runtime credential loading and the bounded authorised-identity probe.
- [ ] Identify an approved source for every required session/protocol behaviour.
- [x] Record the licence and source-adoption decision in the architecture and protocol records.
- [ ] Define message limits, timeouts, cancellation, and privacy-safe logging.
- [x] Exclude OpenAuto/OpenAuto Pro/AASDK shared credentials and security bypasses from implementation and distribution.
- [x] Add and Pi-verify the loopback-only TCP transport for the official developer-mode ADB-forwarded port 5277; the connection probe sends no protocol data.
- [x] Validate the bounded session skeleton against the user-enabled Android Auto head-unit server: version 1.6 accepted and TLS peer data received, followed by error-7 identity rejection.
- [ ] Build all required protocol parsing and framing behind transport interfaces.
- [x] Add the first deterministic fake-phone handshake test without sending new session messages to a real phone.
- [x] Parse the first service-discovery request into a bounded summary without retaining phone names, labels, icons, nested phone details, or raw payloads.
- [x] Model the service catalogue with bounded candidates, hardware-readiness filtering, unique roles, and non-conflicting channel identifiers.
- [x] Prove the frame codec, message assembler, and handshake state machine reach `ServiceDiscoveryReceived` with only a bounded summary and no response, driven over a real `SessionTransport` against a scripted fake phone, without changing the frozen `credential-probe`.
- [ ] Map the newer AASDK `Service` response schema field by field; do not reuse OpenAuto's older `ChannelDescriptor` wire shape. Mapped and contrasted against OpenAuto's `ChannelDescriptor`/`ServiceDiscoveryResponseMessage` for the `Service` and `ServiceDiscoveryResponse` envelopes and the five nested per-kind schemas the current catalogue models (sensor source, media sink, input source, media source, Bluetooth); the eight unmodeled nested service types and their leaf enum/config messages are recorded as not yet mapped. No wire encoder added; response encoding remains gated.
- [x] Add parser fuzz/property tests for untrusted phone input: frame decode, control-message decode, and service-discovery summarization never panic on arbitrary bytes, frame encode/decode round-trips for arbitrary valid payloads, and the service-discovery summary never leaks generated device text.
- [x] Reach and name `VersionAccepted` as the first repeatable live Android Auto session state on the Pi 5; authentication remains blocked.
- [x] Run the opt-in generated-credential probe and record its first sanitized Pi 5 result: version 1.6 accepted, TLS timed out cleanly.
- [x] Record the phone's Android Auto error 7 security rejection over both USB/AOA and the official developer tunnel; stop all generated/shared-identity experiments.
- [x] Prove the OpenSSL adapter presents its configured client certificate and completes mutual TLS when a synthetic verifier trusts that identity.
- [x] Research and document legitimate receiver-provisioning suppliers and their unresolved Raspberry Pi, licence, cost, and certification gates.
- [x] Record operator confirmation of an authorised external receiver identity while keeping all credential material and confidential provisioning details outside the repository.
- [x] Load the protected external identity at runtime and complete version negotiation plus TLS with a real phone; stop before authentication completion and service discovery.
- [ ] Add the gated `auth-discovery-probe` CLI subcommand (`developer auth-discovery-probe` / `usb auth-discovery-probe`, behind `--allow-live-aap`), reusing the frozen `credential-probe` TLS path to reach a bounded, byte-count-only service-discovery summary and stop before any response or media setup; implemented and Pi-verified on synthetic/unit-level checks, not yet run against a real phone.
- [ ] Prove clean timeout, malformed-message, unplug, and reconnect recovery.

## M3 — Pi 5 display, media, audio, microphone, and touch

- [x] Connect and identify the official 7-inch DSI touchscreen.
- [ ] Bring up a native full-screen development UI without relying on VNC.
- [x] Verify native touch press, move, release, coordinates, and two-finger multi-touch.
- [ ] Verify touch rotation and calibration in every supported screen orientation.
- [x] Render synthetic H.264 video and measure Pi 5 decode/presentation performance.
- [x] Select and document the Pi 5 software H.264 decode plus Wayland/GPU composition fallback path.
- [x] Implement and verify the GStreamer decoder-capability adapter on the Pi 5.
- [x] Detect Pi 5 audio routes and document that the DSI reference setup exposes no usable onboard/HDMI sink.
- [x] Test the USB sound card as an audio-output fallback.
- [ ] Select and test a microphone input.
- [ ] Measure video, audio, memory, CPU, and touch latency against provisional targets.

## M4 — Complete wired Android Auto on Pi 5

- [ ] Complete session negotiation and service/channel setup.
- [ ] Display projected video at the negotiated resolution without distortion.
- [ ] Return calibrated touch input to the phone.
- [ ] Play media, navigation/system, and speech audio correctly.
- [ ] Capture microphone audio for voice interaction.
- [ ] Show clear ready, connecting, connected, consent, and error states.
- [ ] Recover from unplug, phone rejection, timeout, and service restart.
- [ ] Complete a 30-minute interactive wired bench scenario.
- [ ] Complete a 60-minute wired media/audio soak without leaks or underruns.
- [ ] Pass 100 physical wired connect/disconnect cycles.

## M5 — Pi 5 appliance and package

- [ ] Add persistent settings for display, rotation, touch, audio, microphone, Wi-Fi, and Bluetooth.
- [ ] Run the application as a dedicated unprivileged system user.
- [ ] Add the preflight and main systemd services.
- [ ] Start directly into the full-screen head unit on boot.
- [ ] Provide a documented recovery/development path when the graphical service fails.
- [ ] Meet the measured boot-to-ready target or record justified revisions.
- [ ] Build a release-quality signed `arm64` `.deb` without requiring Rust on the target.
- [ ] Pass clean install, upgrade, remove, purge, and rollback tests.
- [ ] Produce privacy-safe diagnostics and bounded journald logging.

## M6 — Pi 5 wired hardening

- [ ] Test multiple supported phones and Android versions.
- [ ] Pass extended wired video/audio/touch/microphone soak testing.
- [ ] Pass power interruption, thermal, low-storage, and missing-device tests in wired mode.
- [ ] Complete parser fuzz/property testing and dependency/security review.
- [ ] Complete accessibility, privacy-safe diagnostics, and recovery review.
- [ ] Close every Pi 5 wired release-blocking defect.

## M7 — Complete wireless Android Auto on Pi 5

- [ ] Approve and document the wireless Android Auto protocol source and security model.
- [ ] Detect onboard Pi 5 Wi-Fi and Bluetooth capability and health.
- [ ] Detect supported external USB Wi-Fi and Bluetooth adapters by stable identity.
- [ ] Test onboard/onboard radio operation.
- [ ] Test USB/USB radio operation when suitable adapters are available.
- [ ] Test mixed onboard/USB and USB/onboard combinations where hardware permits.
- [ ] Provide independent `Auto`, `Onboard`, and named USB selections for both radios.
- [ ] Persist radio selections across reboot.
- [ ] Pair/onboard a phone without exposing credentials in logs.
- [ ] Establish, use, disconnect, and reconnect a wireless Android Auto session.
- [ ] Recover when a selected adapter is unplugged, disabled, blocked, or degraded.
- [ ] Confirm wired Android Auto remains usable when wireless hardware is unavailable.
- [ ] Complete 100 wireless reconnect cycles.
- [ ] Complete a multi-hour wireless video/audio/touch/microphone soak.
- [ ] Publish the initial tested USB radio compatibility list by chipset and USB ID.

## M8 — Pi 5 completion gate

- [ ] Wired and wireless modes both pass from a cold boot.
- [ ] Switching between wired and wireless operation is deterministic and recoverable.
- [ ] Display, touch, audio, microphone, settings, and safe shutdown work without VNC.
- [ ] Pass power interruption, low-storage, missing-device, thermal, and service-crash tests.
- [ ] Pass sustained wired and wireless use without unbounded resource growth.
- [ ] Complete security, dependency, licence, privacy, and protocol-provenance reviews.
- [ ] Publish the Pi 5 phone, display, audio, microphone, and USB-radio compatibility matrix.
- [ ] Publish a Pi 5 preview `.deb`, checksums, source, known limitations, and recovery steps.
- [ ] Mark the Pi 5 implementation complete.

## M9 — Other supported devices (starts only after M8)

- [ ] Freeze the Pi 5 reference behaviour and cross-board test procedure.
- [ ] Validate Raspberry Pi 4, including its display, video, audio, USB, boot, and thermal paths.
- [ ] Validate CM4 on the Waveshare Mini Base Board (B) Rev 3.1.
- [ ] Validate the non-wireless CM4 with external USB Wi-Fi and Bluetooth.
- [ ] Validate CM4 eMMC and carrier-board NVMe when available.
- [ ] Validate CM5 on a recorded carrier-board topology.
- [ ] Resolve differences only through platform, media, and carrier abstractions.
- [ ] Repeat wired and wireless functional, reconnect, soak, packaging, and appliance tests on every target.
- [ ] Publish the four-board/carrier compatibility matrix and limitations.

## M10 — Project 1.0 and community release

- [ ] Close every required PRD 1.0 acceptance item.
- [ ] Complete external security, licence, and safety review appropriate to a hobby release.
- [ ] Add contribution, issue, support-bundle, and release documentation.
- [ ] Publish signed packages, checksums, SBOM, source archive, and release notes.
- [ ] Document the experimental, independent, uncertified status prominently.
- [ ] Tag and publish version 1.0.

## Parallel track — Custom CM4/CM5 carrier PCB

- [ ] Freeze requirements from the proven Pi 5 and carrier-board test results.
- [ ] Define phone USB host/power, display, touch, audio, microphone, power, ignition, cooling, antenna, storage, and service interfaces.
- [ ] Verify every used CM4/CM5 pin and interface against both module specifications.
- [ ] Complete schematic, layout, electrical, RF, USB, thermal, protection, and manufacturing reviews.
- [ ] Produce fabrication-neutral KiCad, Gerber, drill, BOM, and placement files.
- [ ] Assemble prototypes through PCBWay or another manufacturer.
- [ ] Bring up and validate both CM4 and CM5 variants.
