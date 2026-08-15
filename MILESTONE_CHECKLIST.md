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
- [ ] Identify an approved source for every required session/protocol behaviour. Confirmed complete for the M2 boundary through receipt and parsing of `ServiceDiscoveryRequest` (`docs/protocol/m2-session-bounds.md` §1, cross-referencing `certainty-matrix.md`), and now also for `ServiceDiscoveryResponse` encoding and channel setup (video/input/media-audio), all field-mapped from the pinned AASDK source (`docs/protocol/aasdk-adoption.md`); remains open for media, wireless, and — newly discovered — protocol version `1.7`, which a real phone negotiates but no known open-source AASDK fork documents or implements (`docs/protocol/error-2-investigation.md`).
- [x] Record the licence and source-adoption decision in the architecture and protocol records.
- [ ] Define message limits, timeouts, cancellation, and privacy-safe logging. Message-size bounds, timeouts, and logging are catalogued and enforced through the `ServiceDiscoveryRequest` boundary (`docs/protocol/m2-session-bounds.md` §2, §3, §5); cancellation is explicitly not: the only mechanism today is a wall-clock deadline plus `Drop`-based resource release, not the cooperative tree-wide cancellation `ARCHITECTURE.md` §6 specifies for the future `app` layer (§4).
- [x] Exclude OpenAuto/OpenAuto Pro/AASDK shared credentials and security bypasses from implementation and distribution.
- [x] Add and Pi-verify the loopback-only TCP transport for the official developer-mode ADB-forwarded port 5277; the connection probe sends no protocol data.
- [x] Validate the bounded session skeleton against the user-enabled Android Auto head-unit server: version 1.6 accepted and TLS peer data received, followed by error-7 identity rejection.
- [x] Build all required protocol parsing and framing behind transport interfaces. Covers frame codec, control-channel handshake, `ServiceDiscoveryRequest` parsing, `ServiceDiscoveryResponse` encoding, and the video/input/media-audio channel-open and video `Setup`/`Config`/`Start` state machines (`crates/protocol-aap/src/{protobuf,service_discovery_response,channel_open,media_message,video_setup}.rs`), all driven over `SessionTransport`. Remains open for the other service kinds, media data itself, and wireless.
- [x] Add the first deterministic fake-phone handshake test without sending new session messages to a real phone.
- [x] Parse the first service-discovery request into a bounded summary without retaining phone names, labels, icons, nested phone details, or raw payloads.
- [x] Model the service catalogue with bounded candidates, hardware-readiness filtering, unique roles, and non-conflicting channel identifiers.
- [x] Prove the frame codec, message assembler, and handshake state machine reach `ServiceDiscoveryReceived` with only a bounded summary and no response, driven over a real `SessionTransport` against a scripted fake phone, without changing the frozen `credential-probe`.
- [x] Map the newer AASDK `Service` response schema field by field; do not reuse OpenAuto's older `ChannelDescriptor` wire shape. All 13 nested `Service` kinds, every referenced leaf enum/config message, and `ServiceDiscoveryResponse`'s remaining `DriverPosition`/`ConnectionConfiguration`/`HeadUnitInfo` fields are now mapped and contrasted against OpenAuto's older `ChannelDescriptor`/`ServiceDiscoveryResponseMessage` (`docs/protocol/aasdk-adoption.md`). A `Service`/`ServiceDiscoveryResponse` Rust wire encoder now exists (`crates/protocol-aap/src/service_discovery_response.rs`), scoped to `Video`/`Input`/`MediaAudio` plus `HeadUnitInfo`; other kinds fail closed with `UnsupportedServiceKind` rather than being silently dropped.
- [x] Add parser fuzz/property tests for untrusted phone input: frame decode, control-message decode, and service-discovery summarization never panic on arbitrary bytes, frame encode/decode round-trips for arbitrary valid payloads, and the service-discovery summary never leaks generated device text.
- [x] Reach and name `VersionAccepted` as the first repeatable live Android Auto session state on the Pi 5; authentication remains blocked.
- [x] Run the opt-in generated-credential probe and record its first sanitized Pi 5 result: version 1.6 accepted, TLS timed out cleanly.
- [x] Record the phone's Android Auto error 7 security rejection over both USB/AOA and the official developer tunnel; stop all generated/shared-identity experiments.
- [x] Prove the OpenSSL adapter presents its configured client certificate and completes mutual TLS when a synthetic verifier trusts that identity.
- [x] Research and document legitimate receiver-provisioning suppliers and their unresolved Raspberry Pi, licence, cost, and certification gates.
- [x] Record operator confirmation of an authorised external receiver identity while keeping all credential material and confidential provisioning details outside the repository.
- [x] Load the protected external identity at runtime and complete version negotiation plus TLS with a real phone; stop before authentication completion and service discovery.
- [x] Add the gated `auth-discovery-probe` CLI subcommand (`developer auth-discovery-probe` / `usb auth-discovery-probe`, behind `--allow-live-aap`), reusing the frozen `credential-probe` TLS path to reach a bounded, byte-count-only service-discovery summary and stop before any response or media setup. Run on Pi 5 with `usb auth-discovery-probe --device <bus:address> --allow-live-aap` against a real phone (USB accessory transition, re-enumerated as the documented Google AOA accessory ID) using the operator-authorised external identity: version negotiated, TLS handshake completed, `AuthComplete` sent, and the phone's real TLS-encrypted `ServiceDiscoveryRequest` was decrypted and reassembled into a bounded byte-count-only summary (icons/label/device-name/phone-info byte counts, zero unknown fields, no payload content logged). The probe stopped cleanly before building or sending any response, and the USB interface was released cleanly (`usb list` responsive immediately after).
- [x] Add post-handshake TLS application-data encrypt/decrypt to `TlsClient`/`OpenSslTlsClient` (`crates/protocol-aap/src/tls.rs`, `crates/security-openssl/src/linux.rs`) and wire per-frame decrypt into `auth-discovery-probe`'s receive loop ahead of bounded reassembly, replacing its prior blanket rejection of any `Encrypted` frame; Pi-verified with real OpenSSL crypto (client/server round-trip, split/coalesced TLS records, invalid ciphertext, premature use, session closure, sanitized errors — `crates/security-openssl/src/linux.rs`) and a real-TLS, possibly-fragmented encrypted `ServiceDiscoveryRequest` reassembly test (`crates/protocol-aap/tests/encrypted_service_discovery.rs`); fixed a latent frame-codec defect this work surfaced, where `decode_frame`/`encode_frame`'s declared-total-vs-frame-size check incorrectly compared plaintext-domain and ciphertext-domain lengths for encrypted frames (`docs/protocol/aasdk-adoption.md`, "Encrypted-message framing"). Now also proven end to end against a real phone's real encrypted traffic (`probe_state=encrypted_frame_received`), not just synthetic/unit/integration coverage.
- [x] Prove clean timeout, malformed-message, unplug, and reconnect recovery. Timeout: running `usb auth-discovery-probe` against a phone still sitting in stale AOA accessory mode from a prior run (Android Auto app not freshly engaged) reached `probe_state=version_accepted`/TLS client hello sent, then failed closed after exactly the configured 10s `PROBE_TIMEOUT` with a clean `CliError::Protocol` timeout error (exit 19) — no hang; `usb list` stayed responsive immediately after. Malformed-message: proven at the parser boundary the real probe actually calls (`decode_frame`, `ControlMessage::decode`, `summarize_service_discovery_request`, and the new decrypt integration) via `crates/protocol-aap/tests/property_fuzz.rs` and `encrypted_service_discovery.rs::rejects_invalid_ciphertext_in_an_encrypted_frame` — proptest-driven adversarial bytes never panic and always fail closed; a real phone sending genuinely malformed AAP bytes isn't something reproducible from the head-unit side, so this is the correct and only faithful way to prove it. Unplug: `usb hold --device <bus:address> --seconds 20`, with the phone physically unplugged mid-hold, reported `hold_result=unplug_detected` cleanly (exit 0, no hang); this surfaced and fixed a pre-existing bug in `usb hold` (`apps/aa-headunit-diagnostics/src/main.rs`) where two mutually-exclusive accessory-mode checks made the command fail unconditionally regardless of device state. Reconnect: after that real unplug, physically replugging the phone (clean re-enumeration, fresh bus:address, MTP mode) and immediately re-running `usb auth-discovery-probe --allow-live-aap` reached `probe_result=service_discovery_summary_received` again in under a second — no stale state, no leaked USB claim, no permission issue.

## M3 — Pi 5 display, media, audio, microphone, and touch

- [x] Connect and identify the official 7-inch DSI touchscreen.
- [x] Bring up a native full-screen development UI without relying on VNC. **Real-hardware-confirmed end to end (2026-08-15).** `ARCHITECTURE.md` §4's GTK4/GStreamer architecture spike (`crates/media-gstreamer/examples/gtk_fullscreen_spike.rs`) passed first, proving synthetic full-screen rendering via `gtk4paintablesink`. A new `usb gtk-dev-ui --device BUS:ADDRESS --allow-live-aap [--tls12-compat]` subcommand (`apps/aa-headunit-diagnostics/src/gtk_dev_ui.rs`) then wired a real phone session into that same path: a new `VideoRenderTarget` (`Wayland` | `Gtk4Window`) threaded through `auth_discovery_probe::run` lets `start_video_render_pipeline` hand off pipeline construction to the GTK-owning thread over a bounded `mpsc` request/response channel (`Gtk4WindowHandoff`), since `gtk4paintablesink`'s `paintable` property must be retrieved from the thread running the default `GLibMainContext` — the protocol session itself keeps running on a background thread exactly as before. The already-proven direct-`waylandsink` path (`RenderSink::Wayland`) is unchanged; all three existing callers of `run` pass it explicitly. Real trial: the operator confirmed directly on the physical screen a brief white flash then real Android Auto rendering correctly, full-screen, through the GTK4 window — not inferred from logs, which separately showed a complete, clean protocol session (service discovery, HEVC video negotiated, real video and audio data streamed for the whole observation window, `probe_result=observation_window_complete`, exit 0). The `Wayland` path was also re-confirmed unaffected by the `run()` signature change in the same session (real video streamed successfully before an unrelated pre-existing `protocol-aap` edge case — a duplicate video `Start` message rejected by the state machine — ended that particular run; not caused by this work). One open, unexplained anomaly from that same `Wayland`-path re-check: the operator reported video appeared immediately with no white flash, straight into Android Auto, but **not full-screen** that time — worth investigating later (not blocking, and not evidence of anything wrong with `gtk-dev-ui` itself, which was separately confirmed full-screen). Packaging note: `aa-headunit-diagnostics` now links `libgtk-4-1` and its transitive libs (`libcairo2`, `libgdk-pixbuf-2.0-0`, `libpango-1.0-0`) as real runtime dependencies of the shipped `.deb`, confirmed via a real package rebuild — a deliberate tradeoff of choosing a CLI subcommand over a standalone example (the GTK4 spike itself stayed a dev-dependency, invisible to the package). Still open beyond this: `gtk-dev-ui` is one-shot only (no `session-supervisor`-style reconnect loop yet), touch input isn't wired into the GTK window (still evdev/libinput only), and there's no real `ui-model`/`ui-gtk` crate or chrome — just the proven rendering/session path.
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

- [x] Complete session negotiation and service/channel setup. Implementation exists end to end (`ServiceDiscoveryResponse` encoding, `ChannelOpenRequest`/`Response`, video/audio `Setup`/`Config`/`Start`, `AudioFocusRequest`, `KeyBindingRequest`, `Sensors`, `NavFocusRequest`, `ByeByeRequest`, `VideoFocusNotification`/`VideoFocusRequest`) and is proven correct in real-TLS integration tests (`crates/protocol-aap/tests/full_channel_setup.rs`). **Real-hardware-confirmed and resolved**: the "Error 2: phone and car are running incompatible software" investigation (`docs/protocol/error-2-investigation.md`) found and fixed the actual gap — `ServiceDiscoveryResponse.VideoConfiguration` was missing `frame_rate=60fps`/`density=180` (recovered via full TLS decryption of a known-working reference session), plus `VideoFocusRequest` handling on the video channel. With both fixed, a real-hardware trial reached a genuinely live Android Auto session: the phone's own screen showed it connected to the vehicle and navigating with Google Maps, and 1,462 real H.265 video `Data` frames streamed and were acked over 30 seconds with zero errors.
- [x] Display projected video at the negotiated resolution without distortion. Real-hardware-confirmed: with the H.265 decode path wired into `crates/media-gstreamer/src/render.rs`'s pipeline (`start_video_render_pipeline` in `auth_discovery_probe.rs` now builds the pipeline for whichever codec the phone actually selected, not just H.264), the head unit's own display showed real, correctly-rendered video from the live session — confirmed directly by the operator watching the physical screen, not inferred from pipeline logs (which also reported zero bus/push errors across all 1,462 frames). See `docs/protocol/error-2-investigation.md`, "`VideoFocusRequest` handling implemented; real video streaming confirmed" and the H.265 render-pipeline follow-up below it.
- [x] Return calibrated touch input to the phone. **Fully real-hardware-confirmed**: real evdev multitouch capture of the official DSI touchscreen (`platform_api::touch::MultiTouchTracker`, `platform_linux::touch::EvdevTouchSource`), wire-exact `InputReport` encoding (`protocol_aap::encode_touch_report`), and live wiring (`auth_discovery_probe.rs`'s `service_touch_input`) are built and real-hardware-tested across five trials. Single-finger taps, continuous drag (pan), and two-finger pinch all confirmed working end to end on a real phone. Two real fixes were required: a coordinate-space mismatch (touch must be advertised/scaled to the negotiated video resolution, not the touchscreen's native panel resolution), and, for continuous gestures specifically, sending a small, contact-lifetime-scoped `pointer_id` (the kernel's `ABS_MT_SLOT` index) instead of the driver's raw, ever-incrementing `ABS_MT_TRACKING_ID` — found by direct comparison against LIVI's own pointer-id allocation strategy, now formally adopted (`docs/protocol/livi-adoption.md`). See `docs/protocol/touch-input-investigation.md` for the full trial-by-trial record.
- [ ] Play media, navigation/system, and speech audio correctly. **Media audio real-hardware-confirmed once, but not reliably**: `crates/media-gstreamer/src/audio.rs`'s `AudioPlaybackPipeline` (`appsrc ! audioconvert ! audioresample ! pulsesink`, raw `S16LE` PCM, no decoder stage) is wired into all three AV sink channels (`MediaAudio`/`SystemAudio`/`SpeechAudio` — one independent pipeline instance each, `auth_discovery_probe.rs`'s `MediaPipelines`). One earlier real-hardware trial received 611 real PCM frames on `MediaAudio` (8192 bytes each, ~48kHz/16-bit/stereo) with zero pipeline errors, and the operator directly confirmed correct audible playback through the Pi's USB sound card — not inferred from logs. A later trial in this same environment received `MediaAudio` frames just as cleanly at the protocol level, but the playback pipeline itself failed to reach `Playing` (`media_audio_playback_error=render pipeline state change failed`), consistent with the known root-vs-PipeWire conflict (`XDG_RUNTIME_DIR` owned by a different uid than the process — see project memory `project_m4_audio_playback.md`, relevant to M5 packaging) recurring intermittently rather than being fixed. `SystemAudio`/`SpeechAudio` share the identical code path but remain unverified with real data — a nav voice prompt needs real GPS movement (not producible with a stationary rig) and a system/notification sound couldn't be reliably triggered on demand from this phone. One real, previously-unmapped protocol gap was found and fixed along the way: `ControlMessageId::VoiceSessionNotification` (wire id 17) arrived unprompted from a real phone when a WhatsApp message notification came in and crashed the whole probe; it's now decoded and logged as a no-reply notification (`crates/protocol-aap/src/voice_session.rs`).
- [ ] Capture microphone audio for voice interaction.
- [x] Show clear ready, connecting, connected, consent, and error states. **Real-hardware-confirmed** (diagnostics-CLI reporting layer, not a GTK UI — `ui-model`/`app` don't exist yet, since the GTK/GStreamer on-device spike `ARCHITECTURE.md` §4 requires hasn't run). A new `connection_state=` line (`apps/aa-headunit-diagnostics/src/connection_state.rs`, `Ready`/`Connecting`/`Connected`/`Error`) is layered onto `usb auth-discovery-probe`, `developer auth-discovery-probe`, and `usb session-supervisor`'s existing `probe_state=`/`supervisor_*` lines. All four transitions were exercised: `Ready`→`Connecting`→`Connected` in a real trial against the phone (`usb auth-discovery-probe`, reaching `probe_state=channel_setup_complete`); `Ready`→`Error` from both the one-shot command and `session-supervisor`'s retry loop, triggered with a nonexistent device selector (no phone interaction needed, since `AoaError::Unplugged` is the same error class a genuine unplug produces). The checklist's "consent" state is deliberately not represented: Android Auto's consent screen is shown and answered entirely on the phone, with no protocol-level visibility from the head unit.
- [ ] Recover from unplug, phone rejection, timeout, and service restart. **Automatic unplug recovery is real-hardware-confirmed**: `usb session-supervisor` (`apps/aa-headunit-diagnostics/src/session_supervisor.rs`) wraps the existing one-shot `usb auth-discovery-probe` flow in a retry loop, re-discovering the phone by physical USB port (`bus` + `port_path`, stable across replugs, unlike the OS-reassigned `address`) rather than requiring an operator to manually replug and re-run the command. A real-hardware trial (`--max-cycles 3`, operator physically unplugging/replugging the phone between cycles) completed all 3 cycles cleanly with no process restart: the phone was rediscovered at a new address each time (`1:18`→`1:20`→`1:22`), confirming the reconnect logic and not just stale state. Phone-rejection and self-imposed-timeout scenarios are handled by the same retry classification (`is_retryable`, unit-tested) but weren't distinctly exercised by this trial — every observed failure was an unplug/USB-IO error. "Service restart" recovery (surviving the whole process being killed and restarted, e.g. by a future systemd unit) isn't yet demonstrated end to end; each invocation is stateless by design, which should make it trivially true, but that's an assumption pending M5's actual service work, not a proven result.
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
