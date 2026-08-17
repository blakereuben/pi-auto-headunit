//! Gated authentication/service-discovery/channel-setup probe.
//!
//! Reuses the same frame codec, message assembler, `HandshakeStateMachine`,
//! and `OpenSslTlsClient` wiring as the frozen `credential-probe`
//! (`live_probe.rs`, unmodified). Beyond that, this probe lets
//! `HandshakeStateMachine::advance` run through to
//! `ServiceDiscoveryRequest`, then goes further still: it builds and sends
//! `ServiceDiscoveryResponse` advertising the full canonical eight-service
//! set (video, touch/input, media/system/speech audio, sensors, Bluetooth,
//! microphone — see `protocol_aap::service_discovery_response`), then
//! handles `AudioFocusRequest`/`AudioFocusNotification` on the control
//! channel (`protocol_aap::audio_focus`), then drives each channel's
//! `ChannelOpenRequest`/`ChannelOpenResponse` handshake
//! (`protocol_aap::channel_open`), then the video channel's
//! `Setup`→`Config`→`Start` handshake (`protocol_aap::video_setup`), the
//! input channel's `KeyBindingRequest`/`KeyBindingResponse` exchange
//! (`protocol_aap::input_message`), and the `MediaAudio`/`SystemAudio`/
//! `SpeechAudio` channels' own `Setup`→`Config`→`Start` handshakes
//! (`protocol_aap::audio_setup` — same message shape as video's, accepting
//! `MEDIA_CODEC_AUDIO_PCM` instead of H.264; the same `AudioSetupStateMachine`
//! is reused unmodified for all three audio channels, since each advertises
//! a single uncompressed PCM `AudioConfiguration`, just at different sample
//! rates). Once the video channel receives `Start` and the input channel
//! has opened, the probe no longer stops immediately — it keeps observing
//! the video channel for whatever the phone sends next, for the remainder
//! of `PROBE_TIMEOUT` (see `report_probe_outcome`'s doc comment). The video
//! channel is a `MediaSinkService`: the phone sends real video data to the
//! head unit, not the reverse (confirmed from AASDK's own
//! `IVideoServiceChannelEventHandler` — no head-unit-side "send video"
//! method exists), so this only ever *receives* `Data`
//! (`MEDIA_MESSAGE_DATA`, an 8-byte timestamp prefix plus raw encoded-frame
//! bytes) or `CodecConfig` (`MEDIA_MESSAGE_CODEC_CONFIG`, raw bytes, no
//! prefix) — the payload itself is never logged whole, only its length
//! (matching this project's no-raw-payload-logging rule), though as of the
//! real decode/render pipeline described below it is now handed to
//! `media_gstreamer::VideoRenderPipeline` in memory, never persisted. See
//! the channel-setup design record for the full scope boundary and
//! provenance trail.
//!
//! Every non-video channel, the populated `HeadUnitInfo`, `AudioFocusRequest`
//! handling, `KeyBindingRequest` handling, and the
//! `MediaAudio`/`SystemAudio`/`SpeechAudio` channels' `Setup`/`Config`/`Start`
//! handling are all experiments toward the same real-phone finding: Android
//! Auto's "phone and car are running incompatible software" (Error 2).
//! Advertising one audio channel, offering the phone's own reported protocol
//! version (`1.7`, versus the pinned source's `1.6`), and populating
//! `HeadUnitInfo` were each tried independently against the earliest form of
//! this failure (appearing immediately after `ServiceDiscoveryResponse`,
//! before any `ChannelOpenRequest` arrived) and each made no difference —
//! ruling out a simple missing-service, version-number-mismatch, or
//! missing-identity cause. Advertising the full canonical set instead —
//! motivated directly by this project's own already-approved `OpenAuto`
//! source (`ServiceFactory::create()`, revision
//! `aa90412bf93b5a5078495ea85ac9270c6297d369`): it unconditionally constructs
//! seven of these eight services (an eighth, `SpeechAudio`, is config-gated
//! but on by default), not a curated subset — was the first change that
//! altered real-phone behavior: the phone stopped rejecting
//! `ServiceDiscoveryResponse` and progressed into the session, first
//! requesting audio focus, then opening every channel and driving video
//! through `Setup`→`Config`, then requesting key bindings on the input
//! channel, then sending `Setup` on the `MediaAudio` channel, then on the
//! `SystemAudio` channel, then on the `SpeechAudio` channel. Error 2 still
//! appears at each new point reached (confirmed on the phone screen, not
//! inferred from probe output), but the failure boundary keeps moving
//! further into the session as each new message is handled — see
//! `docs/protocol/error-2-investigation.md` for the full, still-open
//! history. Every non-video, non-audio-setup channel besides input is
//! driven only to `ChannelOpenState::Open` — no further handshake (sensor
//! data, Bluetooth pairing, microphone capture) exists yet; that is separate
//! follow-on scope once a hypothesis is confirmed. The input channel's
//! `KeyBindingRequest` is answered `KeycodeNotBound` for any non-empty
//! request, matching this project's own `ServiceDiscoveryResponse` exactly
//! (it advertises zero supported keycodes — no button hardware exists yet),
//! mirroring `OpenAuto`'s `InputService::onBindingRequest`
//! validation-against-declared-capability behavior rather than fabricating
//! success.
//!
//! Once TLS completes, a real phone sends `AuthComplete`/
//! `ServiceDiscoveryRequest` as TLS-encrypted application data at the AAP
//! frame level (the `Encrypted` flag), not as more `EncapsulatedTls`
//! control messages. Each encrypted frame's payload is decrypted with
//! `TlsClient::decrypt_application_data` before it reaches bounded message
//! reassembly, matching AASDK's proven per-frame decrypt-before-dispatch
//! behaviour (`docs/protocol/aasdk-adoption.md`); an encrypted frame
//! arriving before TLS completes is rejected outright, since decryption
//! isn't yet possible. Outbound, `ServiceDiscoveryResponse`,
//! `ChannelOpenResponse`, and `Config` are all sent TLS-encrypted — verified
//! directly against the pinned AASDK C++ source
//! (`ControlServiceChannel.cpp`, `VideoMediaSinkService.cpp`) rather than
//! assumed, since `VersionRequest`/`EncapsulatedTls`/`AuthComplete` are sent
//! *plain* by that same source despite also happening post-handshake for
//! `AuthComplete` — encryption is message-specific, not simply
//! before/after TLS completion.
//!
//! Once every implemented handshake step succeeds (all three audio
//! channels' `Setup`/`Config`/`Start`, video through `Config`, input
//! opened), the probe now simply times out — no further message arrives to
//! react to, and raising `PROBE_TIMEOUT` to 30s made no difference (see
//! `docs/protocol/error-2-investigation.md`). `OpenAuto`'s
//! `AndroidAutoEntity::start()` (`AndroidAutoEntity.cpp`) reveals a
//! session-liveness mechanism this project had never implemented at all:
//! it proactively sends `PingRequest` every 5 seconds
//! (`AndroidAutoEntityFactory.cpp`'s hard-coded `Pinger` interval),
//! independent of handshake/channel-setup progress, starting immediately at
//! session start. This probe now does the same (`PING_INTERVAL`,
//! `send_ping_request`) and handles an incoming `PingResponse`
//! (`protocol_aap::ping`). Matching `OpenAuto`'s own scope exactly, an
//! incoming `PingRequest` from the phone is not handled — `OpenAuto` itself
//! only overrides `onPingResponse`, never `onPingRequest` — so one would
//! continue to fail closed via the existing catch-all in
//! `handle_post_discovery_control_message`, which is itself useful
//! information if it happens. The Ping increment's own real-hardware trials
//! were inconclusive at first: multiple runs hit a new, reproducible local
//! USB bulk-OUT write timeout right around when the first ping fired,
//! before the hypothesis could be fairly observed — reproduced twice more
//! later (Sensors and `NavFocusRequest`/`ByeByeRequest` trials, both
//! present-with-ping/absent-without-ping), strongly implicating the ping
//! write itself. The leading root-cause theory, now being tested: this
//! probe sent `PingRequest` **unencrypted**, the only post-handshake
//! message it ever sent that way (matching AASDK's own `sendPingRequest`,
//! accurate for the pinned `1.6` source — but this phone speaks
//! undocumented `1.7`). `send_ping_request` now sends it TLS-encrypted
//! instead, a deliberate, reversible deviation to test against the real
//! phone (see `PING_INTERVAL`'s doc comment).
//!
//! A systematic diff against `OpenAuto`'s full post-`ServiceDiscoveryResponse`
//! session lifecycle and service set (rather than continuing to test one
//! hypothesis at a time) surfaced a genuine capability-advertisement bug,
//! not just a missing handler: `ServiceDiscoveryResponse`'s
//! `Sensors` entry was being encoded as an **empty** `SensorSourceService`
//! message — this project never advertised support for any sensor type, so
//! a real phone had no way to know it could ever request one. `OpenAuto`'s
//! `SensorService::fillFeatures()` (`SensorService.cpp`, pre-approved path
//! per `docs/protocol/openauto-adoption.md`) advertises exactly two:
//! `DRIVING_STATUS` and `NIGHT_DATA`. Driving-status is described as
//! eager/near-mandatory in the research behind this increment — it gates
//! whether the phone shows a full or driving-restricted UI, and is normally
//! requested unconditionally at session bring-up rather than
//! user-triggered — a stronger candidate than Bluetooth pairing or
//! microphone capture (both more clearly user/voice-triggered). This probe
//! now advertises both sensor types and handles `SensorRequest` on the
//! Sensors channel (`protocol_aap::sensor`), always responding `OK`
//! (`SensorResponse`) plus a matching one-shot `SensorBatch`
//! (`DRIVE_STATUS_UNRESTRICTED`, or night mode `false` — this project has no
//! real driving-restriction or day/night sensor pipeline yet), matching
//! `OpenAuto`'s own `SensorService::onSensorStartRequest` behavior exactly.
//! Real-hardware result: also refuted — the phone opened the Sensors
//! channel but never sent a `SensorRequest` even once both types were
//! advertised (`docs/protocol/error-2-investigation.md`).
//!
//! The same `AndroidAutoEntity`/`ControlServiceChannel` audit also named two
//! remaining control-channel gaps, using this project's actually-pinned
//! AASDK fork's own names (`opencardev/aasdk` @ `9bf6adf933665dee26532201719fac14a047ccf1`,
//! `ControlMessageType.proto`) rather than the older fork's names an earlier
//! pass of this same research used for `OpenAuto`'s C++ *behavior* —
//! `NavFocusRequest`/`NavFocusNotification` (wire 13/14, not
//! "NavigationFocusRequest/Response") and `ByeByeRequest`/`ByeByeResponse`
//! (wire 15/16, not "ShutdownRequest/Response"); the wire values are
//! unchanged between forks, only the names differ. Both are now handled
//! (`protocol_aap::nav_focus`, `protocol_aap::byebye`): `NavFocusRequest`
//! always gets a hardcoded `Projected` answer (this project has no native
//! in-car navigation to contest focus with, matching `OpenAuto`'s own
//! hardcoded reply). `ByeByeRequest` — the protocol's own explicit
//! session-end signal, carrying a `reason` enum — is answered with an empty
//! `ByeByeResponse` and then treated as a clean, non-error probe stop
//! (`ProbeOutcome::PhoneEndedSession`), matching `OpenAuto`'s own
//! `triggerQuit()`-after-response behavior; its `reason` value is printed
//! since, if a real phone ever sends this, it could be directly diagnostic
//! about *why* it considers this head unit incompatible.
//!
//! Every real-hardware run through this session has stalled at the exact
//! same point regardless of which increment was under test: `Config` sent
//! on every channel, `Start` never received on any of them. A deep,
//! targeted comparison against `f-io/LIVI` (a separate, independently
//! implemented, GPL-3.0-or-later Android Auto client, not AASDK-derived,
//! confirmed working against modern phones) found `Config`'s own field
//! values already match exactly, but found one concrete gap: LIVI
//! proactively sends an **unsolicited** `VideoFocusNotification` granting
//! `Projected` video focus immediately after `Config`, before ever
//! expecting `Start` — not a reply to anything the phone sent. This
//! message pair (`MEDIA_MESSAGE_VIDEO_FOCUS_REQUEST`/`_NOTIFICATION`,
//! wire 32775/32776) is confirmed to already exist in this project's own
//! pinned AASDK schema (`protocol_aap::video_setup`,
//! `encode_video_focus_notification`) — a real, pre-existing part of the
//! protocol this project had simply never sent. `handle_video_channel_message`
//! now sends it right after `Config`, in the same `Setup` response.
//!
//! Real-hardware result: the phone finally sent `Start` on the video
//! channel — the deepest point this probe had ever reached — but Error 2
//! still appeared. Since this probe had always exited the instant `Start`
//! arrived, it had never actually observed what the phone does next. It
//! now keeps running past `Start` for the rest of `PROBE_TIMEOUT`,
//! decoding (but never rendering, and never logging content) any real
//! `Data`/`CodecConfig` the phone sends on the video channel — see the
//! earlier paragraph on `MediaSinkService` direction and
//! `report_probe_outcome`'s doc comment for the stop-condition change this
//! required (`ChannelSetupComplete` is now a milestone to keep observing
//! past, not a termination signal).
//!
//! At the user's request, this pass did a comprehensive audit of `f-io/LIVI`'s
//! **entire** session lifecycle — every channel, from connection through
//! active streaming — rather than another narrow, single-message
//! comparison. Two concrete, confirmed discrepancies came out of it: (1)
//! this probe never sent `MEDIA_MESSAGE_ACK` in reply to `Data`/
//! `CodecConfig`, despite advertising `Config.max_unacked = 1` on every
//! channel — LIVI acks every single frame, unconditionally, on video and
//! all three audio channels (`_sendAck()`, byte-identical in both
//! `VideoChannel.ts`/`AudioChannel.ts`: `session_id` echoed from `Start`,
//! `ack` always `1`), with the comment *"ACK every frame to avoid phone
//! triggering `CAR_NOT_RESPONDING` (>400 unacked)"* — with `max_unacked=1`
//! and no ack ever sent, the phone could send at most one frame. Now fixed
//! (`protocol_aap::media_message::encode_media_ack`, wired into every AV
//! channel's `Open` state — video and all three audio channels graduated
//! from a bare, machine-discarding `Ready` variant to `Open(...)` holding
//! the machine, the same restructuring already done for video). (2)
//! `KeyBindingResponse` was over-strict (see `evaluate_key_binding_request`'s
//! doc comment). Everything else the audit covered (sensors, Bluetooth,
//! microphone, navigation, video-focus timing) was confirmed to already
//! match what LIVI does — not gaps. Ping-cadence alignment (LIVI pings
//! every 1500ms, not this project's 5000ms, and advertises that interval
//! in `ServiceDiscoveryResponse` — never populated here) was a
//! lower-confidence finding, deliberately left for a later, separate pass.
//!
//! Real-hardware result for the ack/`KeyBindingResponse` batch: the
//! cleanest run in this investigation — every implemented step succeeded,
//! `channel_setup_complete`/`observing_for_post_start_media_traffic` both
//! printed, a `SensorRequest` arrived and was correctly answered *during*
//! the post-`Start` observation window, and the probe ran to a clean
//! `observation_window_complete` with no local error at all. Still no
//! `Data`/`CodecConfig` ever arrived on the video channel, and Error 2
//! still appeared. But `Start` typically lands late in the 10s
//! `PROBE_TIMEOUT` budget, leaving only ~1-2s of real observation time —
//! not enough to be confident the phone would never send media data.
//! `PROBE_TIMEOUT` is raised here for a longer post-`Start` window, the
//! same minimal, reversible technique already proven safe for the earlier
//! 10s→30s experiment (see `docs/protocol/error-2-investigation.md`,
//! "Suggested next steps").
//!
//! Real-hardware result for the 30s window: the full window elapsed
//! cleanly (no local error), one more `SensorRequest` arrived and was
//! answered mid-window, but `Data`/`CodecConfig` still never arrived, and
//! Error 2 still appeared. This refutes "the window was too short" —
//! see `docs/protocol/error-2-investigation.md`, "30-second observation
//! window".
//!
//! Separately, checking the AOA (Android Open Accessory) transport layer
//! itself surfaced one untested variable, isolated as its own single-change
//! experiment: `usb_auth_discovery_probe` (`main.rs`) now presents
//! `AoaIdentification::aasdk_compatibility_probe()` (the exact pinned-AASDK
//! reference `uri`/`serial` strings) instead of `receiver_probe()` (this
//! project's own `uri`/`serial`) — `manufacturer`/`model`/`description`/
//! `version` are unchanged and already identical between the two presets.
//! This combination (exact reference AOA identity + the real
//! operator-authorized TLS credential) was never tried before — the one
//! prior use of `aasdk_compatibility_probe()` was paired with a temporary
//! generated TLS credential and got Error 7, a TLS-trust rejection
//! unrelated to these strings (see `aasdk_compatibility_probe()`'s doc
//! comment, `crates/transport-api/src/lib.rs`). Stated honestly, this is a
//! low-confidence, cheap elimination, not a leading theory: Error 2 fires
//! well past the point (service discovery, all channels open, `Start`)
//! where AOA-level identity would plausibly matter. `PING_INTERVAL` stays
//! neutralized (`3600s`) for this trial too, so the still-unresolved ping
//! write-timeout confound doesn't contaminate this variable's result.
//!
//! Real-hardware result (two trials): still Error 2 both times. Neither
//! trial reached video `Start` (recent `receiver_probe()` trials had
//! reached it) — suggestive, but this investigation's own already-recorded
//! run-to-run variability (identical code, different message sequences
//! across runs) means two non-`Start` runs isn't strong evidence of a
//! regression, only that this identity showed no benefit. Reverted back to
//! `AoaIdentification::receiver_probe()`.
//!
//! **Ping cadence advertisement experiment.** The comprehensive LIVI audit
//! above found one gap it deliberately left out of that batch: LIVI
//! advertises its ping cadence (1500ms) in
//! `ServiceDiscoveryResponse.connection_configuration.ping_configuration`;
//! this project has never populated that field at all
//! (`crates/protocol-aap/src/service_discovery_response.rs`). This is a
//! single-variable, minimal, reversible test of whether the phone cares
//! that this field is present — distinct from, and not contingent on,
//! actually re-enabling ping sends (`PING_INTERVAL` stays neutralized at
//! `3600s`; only the advertised value, `ADVERTISED_PING_INTERVAL_MS`
//! below, changes). Stated honestly: low-to-moderate confidence — this is
//! the one concrete gap the comprehensive audit found and left untested,
//! but a missing optional advertisement field is a plausible-not-certain
//! cause for a phone to reject a session this late (after `Start`).
//!
//! **LIVI ping-model adoption.** At the user's explicit direction, LIVI was
//! formally adopted as a GPL-3.0-or-later source
//! (`docs/protocol/livi-adoption.md`) and its full session lifecycle
//! researched directly (not just the advertisement field above). This
//! found LIVI's actual ping *behavior* differs from the OpenAuto-derived
//! model this probe has used in every prior ping trial in two ways at
//! once: arm timing (immediately after `ServiceDiscoveryResponse`, not
//! before `VersionRequest`) and cadence (1500ms, not 5000ms) — plus a
//! local 5000ms watchdog `OpenAuto`'s `Pinger` has no equivalent of. No real
//! ping has ever reached the phone at this timing in this investigation.
//! Higher confidence than the advertisement-only experiment above: this
//! reframes the still-unresolved USB bulk-OUT write timeout (see "Ping
//! write-timeout diagnosis") as a possible *symptom* of the wrong ping
//! timing, not an independent confound that must be routed around by
//! neutralizing ping entirely. `PING_INTERVAL`/`PING_WATCHDOG_TIMEOUT`
//! below now implement this model; ping is armed in `handle_message` right
//! after `send_service_discovery_response` (see `PingState`).
//!
//! Error 2 is now resolved (`docs/protocol/error-2-investigation.md`): a
//! real phone genuinely streams `Data` on the video channel once
//! `ServiceDiscoveryResponse` correctly identifies the head unit as
//! compatible. `video_render` (a `VideoRenderState`, alongside
//! `video_channel`) builds a real `media_gstreamer::VideoRenderPipeline`
//! (`appsrc ! {h264,h265}parse ! avdec_{h264,h265} ! videoconvert !
//! waylandsink`, codec chosen by whichever `configuration_index` the phone
//! actually selects) lazily once `Start` is received, and pushes every
//! subsequent `Data`/`CodecConfig` payload into it. A pipeline
//! construction, start, or push/bus failure (most commonly: no reachable
//! Wayland compositor, e.g. an SSH session without `WAYLAND_DISPLAY`
//! forwarded) is logged and demotes `video_render` to `Failed` — it never
//! aborts the probe or affects `Ack`-sending, which stays entirely
//! protocol-driven. Real-hardware-confirmed working for both H.264 (self-
//! generated synthetic fixture, `media_gstreamer::render`'s tests) and
//! H.265 (real phone `Data`, real video on the head unit's own display —
//! see `docs/protocol/error-2-investigation.md`). Two framing details
//! remain genuine, explicitly-flagged assumptions, not confirmed facts:
//! `Data`/`CodecConfig` are assumed Annex-B byte-stream framing
//! (start-code-delimited NAL units, `CodecConfig` pushed through the same
//! `appsrc` ahead of frame data so the parser can extract in-band
//! parameter sets), and `Data`'s 8-byte timestamp is assumed microseconds
//! for PTS purposes — both would fail closed (a pipeline bus error, not
//! corrupted output) if wrong, but the real-hardware trial produced
//! correctly-rendered video, which is itself strong indirect evidence both
//! assumptions are right.
//!
//! The three PCM audio channels (`MediaAudio`/`SystemAudio`/`SpeechAudio`)
//! get the same treatment: `media_audio_playback`/`system_audio_playback`/
//! `speech_audio_playback` (each an `AudioPlaybackState`, mirroring
//! `video_render`) build an independent `media_gstreamer::
//! AudioPlaybackPipeline` (`appsrc ! audioconvert ! audioresample !
//! pulsesink`) lazily once that channel's own `Start` is received, and
//! push every subsequent `Data` payload into it — raw
//! `MEDIA_CODEC_AUDIO_PCM` bytes, no decoder stage. Unlike H.264/H.265,
//! raw PCM has no in-stream parser to reject a wrong sample-format
//! assumption (`S16LE` interleaved — the platform-standard layout,
//! unconfirmed against real phone bytes), so a wrong guess would produce
//! audible noise rather than a clean pipeline error; this must be
//! confirmed by ear on real hardware, not inferred from a clean bus alone.
//! `CodecConfig` on an audio channel is logged only, never pushed (see
//! `media_gstreamer::audio`'s doc comment).
//!
//! Touch is the reverse direction from every other channel handled so far:
//! the head unit is the `InputSourceService`, so it *sends* `InputReport`
//! (`protocol_aap::encode_touch_report`) rather than receiving `Data`.
//! `open_touch_source` discovers the DSI touchscreen's evdev node once
//! before the receive loop starts (`platform_linux::touch::
//! discover_touchscreen`, matching on both multitouch position axes rather
//! than any kernel-assigned device name) and opens it
//! (`EvdevTouchSource`), which spawns a background thread translating raw
//! Linux "protocol B" multitouch events into `platform_api::TouchFrame`s —
//! kept in a portable, evdev-free crate so `protocol-aap` and this probe
//! never depend on `evdev` types directly. `service_touch_input`, called
//! once per loop iteration exactly like `service_ping`, drains any queued
//! frames and sends each proactively once the input channel has reached
//! `ChannelOpenState::Open` — never before, since sending on an unopened
//! channel would be a protocol violation. A missing touchscreen or evdev
//! open failure is logged and demotes touch to permanently absent for the
//! rest of the run, exactly like `video_render`/`media_pipelines`'s own
//! failure posture: it never aborts the probe or affects protocol
//! correctness. `PointerAction`'s wire values (`ACTION_DOWN`/`_UP`/
//! `_MOVE`/`_POINTER_DOWN`/`_POINTER_UP` = 0/1/2/5/6) are exactly Android's
//! own `MotionEvent.ACTION_*` constants, not an AASDK-specific invention —
//! `MultiTouchTracker` (`platform_api::touch`) follows that same contract
//! for `action_index`/multi-finger `points` membership, verified by its own
//! unit tests rather than assumed.
//!
//! **Real-hardware result (first trial):** wire-level success, no visible
//! effect. A real phone accepted 93 `InputReport`s across a full session
//! (real touchscreen taps/drags, correct `Down`/`Moved`/`Up` sequencing)
//! with zero protocol errors — the session reached a clean
//! `observation_window_complete`, and video was genuinely rendering on the
//! head unit's own display throughout. But touching the screen produced no
//! observed reaction on the phone (confirmed by the operator watching the
//! screen, not inferred from logs). `InputReport` has no wire-level ack, so
//! this can't be diagnosed from protocol success alone. Leading hypothesis,
//! now fixed and awaiting its own real-hardware trial: a coordinate-space
//! mismatch. `TouchCapability` advertised the DSI panel's native 800x480
//! resolution while `VideoConfiguration` advertises 1280x720, and touch
//! reports were scaled into that same 800x480 space — but the pinned
//! `OpenAuto` source rescales raw touchscreen pixels into the *display*
//! (video) resolution before ever building an outgoing touch event
//! (`InputDevice::handleTouchEvent`), strongly suggesting the phone maps
//! touch against the frame buffer it renders, not the physical digitizer's
//! own resolution. `TOUCH_COORDINATE_SPACE_WIDTH`/`_HEIGHT` now advertise
//! and scale into 1280x720 instead — a single-variable, reversible change.
//!
//! **Real-hardware result (second trial, coordinate-space fix):** taps now
//! land correctly (confirmed by the operator: "pressing on the screen
//! worked"), proving the wire-level pipeline and coordinate scaling are
//! both genuinely correct end to end. Continuous drag (pan) and multitouch
//! (pinch) still did not register, despite 226 well-formed reports sent —
//! including 197 `Moved` and several `PointerDown`/`PointerUp` frames — with
//! zero protocol errors. Root-caused before a third trial: `action_index`
//! was left unset (`None`) for `Moved`, and — a second, related instance of
//! the same gap — for `Down`. `opencardev/openauto`'s current `main` branch
//! (same org as this project's pinned `aasdk` fork, unlike the separately
//! pinned `f1xpl/openauto`, already updated to this project's
//! `InputReport`/`sendInputReport` schema) shows `action_index` is
//! *always* sent, `0` for `Down`/plain movement — never omitted. See
//! `platform_api::touch::TouchFrame`'s doc comment for the full citation.
//! `action_index`/`action` are now non-optional (`u32`/`PointerAction`) in
//! both `TouchFrame` and `encode_touch_report`, closing off the class of
//! bug rather than just patching the two known instances.
//!
//! **Real-hardware result (third trial, `action_index` fix): no change.**
//! 108 well-formed reports sent cleanly, zero protocol errors, but the
//! operator confirmed drag/pinch still produced no visible effect —
//! refuting `action_index` omission as *the* (or *a*) blocking cause, though
//! the fix is independently correct and stayed in the code.
//!
//! **Fourth hypothesis, fixed and real-hardware-confirmed:** `pointer_id`
//! itself. `MultiTouchTracker::to_point` (`platform_api::touch`) was passing
//! the Linux kernel driver's raw `ABS_MT_TRACKING_ID` straight through as
//! `pointer_id` — an ever-incrementing counter across the touchscreen's
//! whole session lifetime, never reused. `f-io/LIVI`
//! (`docs/protocol/livi-adoption.md`, formally adopted)
//! `useProjectionTouch.ts`'s `alloc()`/`free()` does the opposite
//! deliberately: it maps the browser's own similarly arbitrary
//! `PointerEvent.pointerId` down to the smallest free non-negative integer
//! per active contact, recycled on lift, and never sends the browser's raw
//! id to the phone. `to_point` now uses the kernel's own `ABS_MT_SLOT`
//! index as `pointer_id` instead — already small, bounded by the
//! touchscreen's simultaneous-touch capability, and reused by the kernel
//! itself, so no manual allocate/free bookkeeping was needed on this side.
//! **Real-hardware result (fourth trial): CONFIRMED.** 257 single-finger and
//! 232 two-finger `Moved` reports sent cleanly, zero protocol errors, and
//! the operator directly confirmed **drag/swipe and pinch now work** on the
//! real phone screen — closing this investigation. See
//! `docs/protocol/touch-input-investigation.md` ("Trial 5") and
//! `docs/protocol/livi-adoption.md`'s adopted-scope item 7 for the full
//! record.
//!
//! **Attribution.** This probe's overall session-orchestration shape
//! (version → TLS → auth → service discovery → channel setup → running)
//! follows AASDK revision `9bf6adf933665dee26532201719fac14a047ccf1` and
//! `OpenAuto` revision `aa90412bf93b5a5078495ea85ac9270c6297d369`
//! (`docs/protocol/aasdk-adoption.md`, `docs/protocol/openauto-adoption.md`).
//! `evaluate_key_binding_request`'s unconditional-success policy and this
//! file's ping arm-timing/cadence/watchdog model (`PingState`,
//! `PING_INTERVAL`, `PING_WATCHDOG_TIMEOUT`) are derived from `f-io/LIVI`
//! revision `9000f308eec423c5c56ac0a14491a7c95ce5762d`
//! (`docs/protocol/livi-adoption.md`, "Adopted scope" items 3 and 5). No
//! AASDK/OpenAuto/LIVI code is reproduced in this file — only independently
//! reimplemented behaviour, cited to its source.

// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// Copyright (C) 2024-2026 Open Android Auto contributors (LIVI)
// SPDX-License-Identifier: GPL-3.0-or-later

use credential_store::CredentialMaterial;
use media_api::{DecoderCapability, DecoderKind, VideoCodec as DecoderVideoCodec};
use media_gstreamer::{
    AudioFormat, AudioPlaybackPipeline, AudioSink, GstreamerBackend, GstreamerError, RenderSink,
    VideoRenderPipeline,
};
use platform_api::{ArmedGestureDetector, GestureEvent, TouchPhase};
use platform_linux::touch::{EvdevTouchSource, Rotation, SharedRotation, discover_touchscreen};
use protocol_aap::{
    AASDK_MAX_FRAME_PAYLOAD_SIZE, AudioCapability, AudioFocusRequestType, AudioFocusStateType,
    AudioSetupAction, AudioSetupEvent, AudioSetupStateMachine, AudioStreamType,
    BluetoothCapability, BluetoothMessageId, BluetoothServiceMessage, ByeByeReason,
    ChannelOpenAction, ChannelOpenEvent, ChannelOpenState, ChannelOpenStateMachine, ControlMessage,
    ControlMessageId, DEFAULT_MAX_BLUETOOTH_MESSAGE_BODY_SIZE, DEFAULT_MAX_CONTROL_BODY_SIZE,
    DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE,
    DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE, DEFAULT_MAX_SERVICE_CANDIDATES, DecodedFrame, Encryption,
    FrameError, FrameHeader, FrameType, HandshakeAction, HandshakeEvent, HandshakeState,
    HandshakeStateMachine, HeadUnitInfo, InputMessage, InputMessageId, KeyBindingStatus, KeyCode,
    MediaMessageId, Message, MessageAssembler, MessageType, MicrophoneCapability, NavFocusType,
    PingConfiguration, PointerAction, ProtocolLimits, RadioCapability, RadioType, SensorCapability,
    SensorMessage, SensorMessageId, SensorType, ServiceAvailability, ServiceCandidate,
    ServiceCapabilities, ServiceCatalogue, ServiceDiscoveryRequestSummary, ServiceKind, TlsClient,
    TlsProgress, TouchCapability, TouchPointer, TouchScreenType, UiConfig, VideoCapability,
    VideoCodecResolution, VideoCodecType, VideoFocusMode, VideoFrameRate, VideoSetupAction,
    VideoSetupEvent, VideoSetupState, VideoSetupStateMachine, decode_audio_focus_request,
    decode_bluetooth_pairing_request, decode_byebye_request, decode_frame,
    decode_key_binding_request, decode_nav_focus_request, decode_ping_request,
    decode_ping_response, decode_sensor_request, decode_voice_session_notification,
    encode_audio_focus_notification, encode_bluetooth_pairing_response, encode_byebye_response,
    encode_driving_status_unrestricted_batch, encode_frame, encode_key_binding_response,
    encode_key_event, encode_nav_focus_notification, encode_night_mode_batch, encode_ping_request,
    encode_ping_response, encode_sensor_response, encode_service_discovery_response,
    encode_touch_report, encode_video_focus_notification,
};
use security_openssl::{OpenSslTlsClient, TlsVersionPolicy};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use transport_api::{SessionTransport, TransportError};

use crate::CliError;

/// Raised from 10s to 30s for a longer post-`Start` media-observation
/// window (see the module doc comment's "Real-hardware result for the
/// ack/`KeyBindingResponse` batch" paragraph) — `Start` typically arrives
/// late in a 10s budget, leaving too little time to tell whether the phone
/// would eventually send `Data`/`CodecConfig`. Directly analogous to the
/// earlier 10s→30s experiment already proven safe (no behavioral
/// difference observed then, beyond the intended longer window).
///
/// Was temporarily raised to 120s for a `SystemAudio`/`SpeechAudio`
/// verification trial (these two channels only carry data when the phone
/// actually has something to say, e.g. a nav voice prompt or a system
/// notification tone, unlike `MediaAudio` which starts as soon as any media
/// plays) — reverted back to 30s since neither channel could be reliably
/// triggered from this environment (no working microphone for voice
/// prompts; stationary rig, so no real turn-by-turn nav; no easy way to
/// generate a phone notification on demand). `SystemAudio`/`SpeechAudio`
/// remain built (same code path as the confirmed-working `MediaAudio`) but
/// unverified with real data. One real, previously-unmapped gap was found
/// and fixed along the way: a `WhatsApp` message notification made a real phone
/// send `ControlMessageId::VoiceSessionNotification` (wire id 17,
/// undocumented in this project until now), which crashed the whole probe
/// since any unmapped control message was a hard error — see
/// `crates/protocol-aap/src/voice_session.rs`'s module doc comment.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Extended observation budget used only when
/// `AA_HEADUNIT_SURVIVE_PROACTIVE_WRITE_TIMEOUT` is set (see that
/// constant's doc comment below). Each failed proactive write already costs
/// close to the full `BULK_SEND_TIMEOUT` (10s,
/// `crates/transport-usb/src/linux.rs`) before this probe can even attempt
/// a retry, so `PROBE_TIMEOUT`'s normal 30s budget only ever allows one or
/// two attempts past `Start`. Long enough for several `PING_INTERVAL`-spaced
/// attempts, to test whether the phone's endpoint ever answers a later
/// proactive write after failing an earlier one.
const PROACTIVE_WRITE_SURVIVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// Matches `OpenAuto`'s `AndroidAutoEntityFactory.cpp`, which constructs its
/// `Pinger` with a hard-coded 5000ms interval
/// (`std::make_shared<Pinger>(ioService_, 5000)`), armed from
/// `AndroidAutoEntity::start()` before even `VersionRequest` — a
/// session-liveness mechanism decoupled from handshake/channel-setup
/// progress that this probe has never previously implemented.
///
/// Was temporarily raised past `PROBE_TIMEOUT` twice more for the
/// NavFocus/ByeBye real-hardware trials: with `PING_INTERVAL` active at 5s,
/// a run reached `SensorRequest`/`SensorResponse`/`SensorBatch` for the
/// first time ever, then hit the same USB bulk-OUT write timeout as the
/// original Ping trial; neutralizing `PING_INTERVAL` reproduced the same
/// sensor exchange with a clean timeout instead — strong evidence the write
/// timeout is specifically tied to the ping write, not incidental USB
/// flakiness (see `docs/protocol/error-2-investigation.md`,
/// "Ping/liveness experiment"). Reverted back to 5s now that both trials
/// are recorded; the write-timeout root cause itself remains a separate,
/// still-open follow-up.
///
/// Leading root-cause hypothesis, now being tested: `PingRequest` is the
/// *only* message this probe sends unencrypted after the TLS handshake
/// completes (`send_ping_request` previously used `Encryption::Plain`,
/// matching AASDK's own `sendPingRequest` — accurate for the pinned `1.6`
/// source, but this phone speaks undocumented `1.7`). A real client's
/// post-handshake frame parser may expect an unbroken all-encrypted
/// bytestream and desync badly enough on a stray plain frame that it stops
/// draining its own USB OUT buffer — which would manifest exactly as our
/// bulk-OUT write eventually timing out. `send_ping_request` now sends
/// `PingRequest` TLS-encrypted instead, as a deliberate, reversible
/// deviation from the pinned source to test against this real phone. Also
/// refuted: even with a bulk-OUT write timeout matching AASDK's own 10s
/// reference value, the write still stalls — see `error-2-investigation.md`.
///
/// Was temporarily raised past `PROBE_TIMEOUT` again for the post-`Start`
/// media-observation trial (the write timeout struck again right as the
/// observation window opened) and again for the ack/`KeyBindingResponse`
/// batch — both trials completed cleanly with `PING_INTERVAL` neutralized,
/// reaching `Start` and running the full observation window with no local
/// error, but no `Data`/`CodecConfig` ever arrived either time. Reverted
/// back to 5s, then hit the identical write timeout again on the very next
/// trial (the 30s `PROBE_TIMEOUT` observation-window experiment) — still
/// unresolved, so raised past `PROBE_TIMEOUT` (3600s) once more to isolate
/// that trial's own variable (window length) from this still-open confound.
///
/// **Superseded by the formally-adopted LIVI ping model**
/// (`docs/protocol/livi-adoption.md`, "Adopted scope" item 5). LIVI's own
/// `Session.ts` arms its ping timer immediately after
/// `ServiceDiscoveryResponse` (not before `VersionRequest`, unlike the
/// OpenAuto-derived model above) and sends at exactly 1500ms — materially
/// different in both timing *and* cadence from every ping trial run so
/// far, all of which used the OpenAuto-derived 5000ms/session-start model.
/// No real ping has ever actually reached the phone at this timing; the
/// leading theory going into this trial is that the still-unresolved USB
/// write-timeout bug may itself be a symptom of the phone expecting this
/// exact cadence and never receiving it, not an independent confound to
/// route around. Value changed from the neutralized `3600s` to the real
/// `1500ms`, and arming moved from session-start to `ServiceDiscoveryResponse`
/// (see `PingState`).
const PING_INTERVAL: Duration = Duration::from_millis(1500);
/// LIVI's own local session watchdog (`Session.ts`): if no `PingResponse`
/// arrives within this long of the last one, LIVI closes the session
/// itself. Adopted alongside `PING_INTERVAL` above
/// (`docs/protocol/livi-adoption.md`, "Adopted scope" item 5) — this probe
/// does the same rather than silently drifting out of sync with what a
/// real head unit would do.
const PING_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(5);
/// `AA_HEADUNIT_PING_ISOLATION` (any value, checked via `env::var_os` so an
/// empty value still enables it) — an opt-in, real-hardware experiment
/// isolating *why* the second scheduled proactive send (real-hardware-
/// confirmed to hit a USB bulk-OUT write timeout in every trial so far,
/// `docs/protocol/error-2-investigation.md`) fails. The first scheduled
/// send always stays a real `PingRequest` (already proven to succeed and
/// get a real `PingResponse`); every send after that substitutes a
/// harmless, already-proven-safe duplicate `VideoFocusNotification`
/// instead (`send_control_probe_frame`). If that also hangs, the failure
/// is about proactive-write *position* in the session, not `Ping` content;
/// if it succeeds, the failure is specifically about a second
/// `PingRequest`. Off by default — normal probe runs are unaffected.
const PING_ISOLATION_ENV_VAR: &str = "AA_HEADUNIT_PING_ISOLATION";
/// `AA_HEADUNIT_SURVIVE_PROACTIVE_WRITE_TIMEOUT` (any value, same
/// `env::var_os` convention as `PING_ISOLATION_ENV_VAR` above) — an opt-in,
/// real-hardware experiment testing suggested next step 2 in
/// `docs/protocol/error-2-investigation.md`: does the phone's endpoint
/// *permanently* stop answering proactive writes once it starts, or does it
/// eventually recover? Every prior real-hardware trial ended the whole
/// probe on the first proactive-write timeout (`service_ping`'s `?`
/// propagates it out of `run()`), so no trial has ever been able to observe
/// a second attempt after a first one failed. With this set,
/// `service_ping` logs and swallows a proactive-write timeout instead of
/// returning it, and the LIVI-derived local watchdog (`PING_WATCHDOG_TIMEOUT`)
/// is also suppressed — enforcing it here would close the session the
/// moment the first attempt goes unanswered, defeating the point of this
/// experiment. `run()` also switches to the longer
/// `PROACTIVE_WRITE_SURVIVAL_TIMEOUT` budget instead of `PROBE_TIMEOUT` so
/// there's room for several spaced-out attempts. Off by default — normal
/// probe runs (including the ping-isolation experiment above) are
/// unaffected. Real-hardware-confirmed: the phone's endpoint never
/// recovered across 11 consecutive attempts spanning ~110s (see
/// `docs/protocol/error-2-investigation.md`, "Proactive-write survival
/// experiment") — the remaining open question is *why*, not *whether it
/// ever recovers on this timescale*.
const PROACTIVE_WRITE_RESILIENCE_ENV_VAR: &str = "AA_HEADUNIT_SURVIVE_PROACTIVE_WRITE_TIMEOUT";
/// `AA_HEADUNIT_REACTIVE_TRIGGERED_PROACTIVE_SEND` (same `env::var_os`
/// convention) — an opt-in, real-hardware experiment testing the other half
/// of suggested next step 2: is the write-timeout really about a write
/// being *proactive* (head-unit-initiated, no phone activity prompting it),
/// or is it actually about elapsed idle time on the OUT endpoint,
/// regardless of who initiates? Every proactive send in every prior trial
/// fired on a bare periodic schedule (`service_ping` runs on essentially
/// every ~500ms loop tick — `LibUsbBulkTransport::read`'s `rusb::Error::
/// Timeout` maps to `Ok(0)`, not `Err(TransportError::TimedOut)`, so the
/// `receive()` timeout branch in `run()`'s loop is effectively dead for the
/// real USB transport and `service_ping` runs whether or not anything
/// actually arrived that tick), never deliberately coupled to genuine
/// phone-initiated traffic. With this set, a due proactive send is held
/// back until the same loop iteration where a real message from the phone
/// was just decoded and dispatched (see `drain_and_dispatch_frames`'s
/// `processed_any` return value) — a real reactive round trip, not just a
/// nonzero byte count or a routine idle tick. Implies
/// `survive_proactive_write_timeout` (see that constant's doc comment)
/// regardless of whether the other flag is also set, since this experiment
/// is only informative if the session survives long enough to compare
/// multiple reactive-triggered attempts. Off by default.
const REACTIVE_TRIGGERED_PROACTIVE_SEND_ENV_VAR: &str =
    "AA_HEADUNIT_REACTIVE_TRIGGERED_PROACTIVE_SEND";
/// `AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS` (a positive integer number of
/// seconds) — overrides `PROBE_TIMEOUT`/`PROACTIVE_WRITE_SURVIVAL_TIMEOUT`
/// with an explicit operator-chosen observation window. Added for
/// `MILESTONES.md`'s 30-minute interactive drive-bench exit gate ("a
/// 30-minute interactive drive-bench scenario passes on the Pi 5 without
/// crash, unbounded memory growth, or private payload logging") — every
/// window length this probe has used until now was a temporary, hand-edited
/// constant for a specific one-off experiment (see `PROBE_TIMEOUT`'s own
/// doc-comment history above), which doesn't fit a scenario meant to be
/// re-run for future soak/regression testing without editing source each
/// time. Off by default (falls back to the existing `PROBE_TIMEOUT`/
/// `PROACTIVE_WRITE_SURVIVAL_TIMEOUT` selection unchanged). An unparseable
/// or zero value is a usage error, not silently ignored — an operator who
/// set this explicitly should learn immediately if it didn't take effect,
/// rather than unknowingly running the default short window instead.
const OBSERVATION_WINDOW_SECONDS_ENV_VAR: &str = "AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS";
/// Opt-in, off-by-default touch rotation override (`0`/`90`/`180`/`270`,
/// `wl_output.transform` convention — see `platform_linux::touch::Rotation`).
/// `MILESTONE_CHECKLIST.md` M3's "verify touch rotation ... in every
/// supported screen orientation" needs a way to exercise rotations other
/// than the reference rig's actual physical mounting (`Rotate0`); this env
/// var is that way, matching `OBSERVATION_WINDOW_SECONDS_ENV_VAR`'s
/// established pattern for an opt-in real-hardware experiment knob. Unset
/// defaults to `Rotate0` (current behavior, unchanged); an unparseable
/// value is a hard usage error, not a silent fallback, for the same reason
/// `read_observation_window_override` treats one as an error.
const TOUCH_ROTATION_ENV_VAR: &str = "AA_HEADUNIT_TOUCH_ROTATION";
/// Advertised in `ServiceDiscoveryResponse.connection_configuration.ping_configuration`
/// (`build_service_capabilities`). All four `PingConfiguration` sub-fields
/// are now populated with LIVI's own confirmed values
/// (`docs/protocol/livi-adoption.md`, "Adopted scope" items 4-5) — no
/// longer just `interval_ms` alone, now that the other three are backed by
/// an adopted, cited source rather than being unresearched guesses.
const ADVERTISED_PING_TIMEOUT_MS: u32 = 5000;
const ADVERTISED_PING_INTERVAL_MS: u32 = 1500;
const ADVERTISED_PING_HIGH_LATENCY_THRESHOLD_MS: u32 = 500;
const ADVERTISED_PING_TRACKED_COUNT: u32 = 5;
const MAX_ACCUMULATED_BYTES: usize = 64 * 1024;
/// Head-unit-assigned channel ids advertised in `ServiceDiscoveryResponse`.
/// These are this probe's own choice, not AASDK's internal `ChannelId`
/// numbering (which that fork's own source flags as a simplification, not
/// protocol-mandated).
const VIDEO_CHANNEL_ID: u8 = 1;
const INPUT_CHANNEL_ID: u8 = 2;
const MEDIA_AUDIO_CHANNEL_ID: u8 = 3;
const SYSTEM_AUDIO_CHANNEL_ID: u8 = 4;
const SPEECH_AUDIO_CHANNEL_ID: u8 = 5;
const SENSORS_CHANNEL_ID: u8 = 6;
const BLUETOOTH_CHANNEL_ID: u8 = 7;
const MICROPHONE_CHANNEL_ID: u8 = 8;
/// Advertises `RadioService` (`docs/protocol/aasdk-adoption.md`) as a
/// capability only — no runtime tuning/scanning/preset messages are
/// implemented (deliberately out of scope, no real tuner hardware
/// exists). Real-hardware-confirmed, 2026-08-16: before this existed, the
/// phone rejected `KeyCode::Radio` with "AA was not available"; once
/// advertised, swipe-right instead navigates to Android Auto's own
/// native radio screen (empty, since no tuning backend exists behind it,
/// but the routing itself is correct — radio is a first-class native AA
/// UI category, not a third-party-app switch like media/navigation/
/// phone). If the phone ever does send more than the open handshake on
/// this channel, `simple_channels` drives the same generic,
/// already-proven open-then-reject-further-messages path every other
/// unimplemented-beyond-open channel does (`handle_simple_channel_message`),
/// never a hang or panic.
const RADIO_CHANNEL_ID: u8 = 9;
/// The coordinate space touch reports must be sent in: the negotiated
/// video resolution (`VideoCodecResolution::Video1280x720`,
/// `build_service_capabilities` below), **not** the DSI touchscreen's own
/// native panel resolution (800x480). Confirmed from the pinned `OpenAuto`
/// source: `InputDevice::handleTouchEvent`
/// (`src/autoapp/Projection/InputDevice.cpp`) explicitly rescales raw
/// touchscreen pixels into `displayGeometry_` — the video output
/// resolution — before an event ever reaches `InputService::onTouchEvent`,
/// which then sends `event.x`/`event.y` unchanged; the phone maps touch
/// against the frame buffer it is actually rendering, not the physical
/// digitizer's own pixel count. A real-hardware trial that instead
/// advertised/scaled touch to the panel's native 800x480 (while video
/// stayed 1280x720) sent well-formed reports with zero protocol errors but
/// produced no visible reaction on the phone — consistent with, though not
/// yet proof of, this coordinate-space mismatch; this constant is the
/// single-variable fix under test.
const TOUCH_COORDINATE_SPACE_WIDTH: i32 = 1280;
const TOUCH_COORDINATE_SPACE_HEIGHT: i32 = 720;
/// Same dimensions as [`TOUCH_COORDINATE_SPACE_WIDTH`]/
/// [`TOUCH_COORDINATE_SPACE_HEIGHT`], as `u32` — what raw evdev touch
/// coordinates are actually scaled into (`open_touch_source`).
const TOUCH_COORDINATE_SPACE_WIDTH_PIXELS: u32 = TOUCH_COORDINATE_SPACE_WIDTH.unsigned_abs();
const TOUCH_COORDINATE_SPACE_HEIGHT_PIXELS: u32 = TOUCH_COORDINATE_SPACE_HEIGHT.unsigned_abs();
/// The four-finger arming swipe (`platform_api::ArmedGestureDetector`)
/// must travel at least this far (straight-line displacement, not
/// specifically "downward" — see that crate's own doc comment for why)
/// before lifting to count — short enough to be an easy deliberate
/// gesture, long enough that ordinary incidental finger movement during
/// real AA use (which never involves four simultaneous fingers anyway)
/// can't trigger it by accident. A quarter of the negotiated video
/// height's worth of physical panel travel.
const SETTINGS_GESTURE_SWIPE_THRESHOLD_PIXELS: u32 = TOUCH_COORDINATE_SPACE_HEIGHT_PIXELS / 4;
/// `OpenAuto`'s `ServiceFactory` defaults (`MediaAudioService`: 2ch/16-bit/48kHz;
/// `SpeechAudioService`/`SystemAudioService`/`AudioInputService`: 1ch/16-bit/16kHz),
/// not invented values.
const MEDIA_AUDIO_SAMPLING_RATE: u32 = 48_000;
const VOICE_AUDIO_SAMPLING_RATE: u32 = 16_000;
const VOICE_AUDIO_BITS: u32 = 16;
const VOICE_AUDIO_CHANNELS: u32 = 1;
/// Playback pipeline formats matching the `AudioConfiguration` values
/// actually advertised above for each channel (`MediaAudio`: stereo;
/// `SystemAudio`/`SpeechAudio`: mono voice) — kept alongside them so a
/// future change to one can't silently desync from the other.
const MEDIA_AUDIO_PLAYBACK_FORMAT: AudioFormat = AudioFormat {
    sampling_rate: MEDIA_AUDIO_SAMPLING_RATE,
    channels: 2,
};
const VOICE_AUDIO_PLAYBACK_FORMAT: AudioFormat = AudioFormat {
    sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
    channels: VOICE_AUDIO_CHANNELS,
};

/// Per-channel progress for the video channel, driven once
/// `ServiceDiscoveryResponse` has been sent. `Open` covers the entire
/// post-open lifecycle (`Setup`→`Config`→`Start`→ongoing media
/// observation) — `VideoSetupStateMachine` already tracks `Ready`
/// internally, so there's no separate app-level "ready" marker; the
/// machine is kept (not discarded) once `Ready`, so further `InboundMedia`
/// events keep flowing to it instead of being rejected.
enum VideoChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    Open(VideoSetupStateMachine),
}

/// Lifecycle of the real decode/render pipeline for the video channel,
/// independent of `VideoChannel`'s own protocol state — a pipeline failure
/// (no compositor reachable, decode error, etc.) must never affect
/// protocol-level correctness (acking keeps happening regardless). Built
/// lazily on `VideoSetupAction::Ready`, since only then is the negotiated
/// codec known — this probe advertises both a 1280x720@60 H.264 and an
/// identical H.265 configuration (`build_service_capabilities`), and
/// starts the pipeline typed for whichever one the phone actually
/// selected; see `handle_video_start_received`.
enum VideoRenderState {
    /// `GstreamerBackend::new()` itself failed (`GStreamer` unusable on this
    /// host at all) — never attempted again this run. The error is
    /// printed at the point of failure, not retained here.
    Unavailable,
    /// Backend is ready; pipeline not yet built (waiting for `Ready`).
    NotStarted(GstreamerBackend),
    /// Pipeline built and playing.
    Running(VideoRenderPipeline),
    /// Construction, start, or a later push/bus error occurred — no
    /// further attempts are made this run, but the probe (and acking)
    /// keeps going. The error is printed at the point of failure, not
    /// retained here.
    Failed,
}

/// Where a session's decoded video actually gets displayed. `Wayland` is
/// today's only behavior, used unmodified by all three existing callers of
/// [`run`] (`usb_auth_discovery_probe`/`developer_auth_discovery_probe` in
/// `main.rs`, `session_supervisor::SupervisedSession::attempt`) via
/// `VideoRenderTarget::Wayland`. `Gtk4Window` is new: pipeline
/// construction is handed off to whichever thread owns the GTK main loop
/// (see [`Gtk4WindowHandoff`]) — this type never touches `gtk4`/`gdk4`
/// itself, keeping GTK confined to `gtk_dev_ui.rs`.
pub(crate) enum VideoRenderTarget {
    Wayland,
    Gtk4Window(Gtk4WindowHandoff, TouchSettingsHandoff),
}

/// One-shot request/response channel pair, used at most once per session
/// (video render construction is already lazy/single-shot — see
/// `start_video_render_pipeline`'s `NotStarted` guard). `request` sends the
/// negotiated [`DecoderCapability`] and blocks, bounded by
/// [`GTK_PIPELINE_HANDOFF_TIMEOUT`], for the GTK-owning thread to build,
/// wire into its `gtk::Picture`, and start a `RenderSink::Gtk4Paintable`
/// pipeline — mirroring the exact build → retrieve-paintable →
/// set-on-`Picture` → present → start ordering the real-hardware-confirmed
/// GTK4 spike (`crates/media-gstreamer/examples/gtk_fullscreen_spike.rs`)
/// proved correct, just executed from a poll callback instead of directly
/// inside `connect_activate`.
pub(crate) struct Gtk4WindowHandoff {
    pub(crate) capability_sender: mpsc::Sender<DecoderCapability>,
    pub(crate) pipeline_receiver: mpsc::Receiver<Result<VideoRenderPipeline, GstreamerError>>,
}

/// Bridges the background protocol thread's touch handling to the GTK
/// thread's settings panel (`gtk_dev_ui.rs`). `rotation_sender` fires at
/// most once, as soon as the touchscreen (if any) is opened, carrying a
/// live-adjustable [`SharedRotation`] handle — `None` if no touchscreen
/// was found, so the GTK side can disable rotation controls rather than
/// hang waiting for a handle that will never arrive. `arm_window_sender`
/// fires once the same way, carrying the live-adjustable
/// [`platform_api::SharedArmWindow`] handle so the settings panel's
/// timeout control has something to change. `gesture_sender` fires every
/// time `ArmedGestureDetector` reports an armed/disarmed/completed event;
/// the GTK thread owns `gesture_settings::GestureSettings` and decides
/// what a completed gesture actually does, since that mapping is
/// head-unit/UI policy, not something the protocol/touch layer should
/// know about — it only needs the armed/disarmed events to show/hide its
/// own "listening" indicator.
#[allow(clippy::struct_field_names)]
pub(crate) struct TouchSettingsHandoff {
    pub(crate) rotation_sender: mpsc::Sender<Option<SharedRotation>>,
    pub(crate) arm_window_sender: mpsc::Sender<platform_api::SharedArmWindow>,
    pub(crate) gesture_sender: mpsc::Sender<GestureEvent>,
}

/// Well under `PING_WATCHDOG_TIMEOUT` (5s) — deliberately, not just
/// generously. `ping_state` is armed before video `Start` ever arrives,
/// and `service_ping` kills the whole session if `last_pong.elapsed() >=
/// PING_WATCHDOG_TIMEOUT` the next time it runs, which happens in the same
/// loop iteration right after this blocking handoff returns. A timeout
/// anywhere close to 5s would let a slow-but-successful handoff get its
/// pipeline built and then have the session killed immediately after, for
/// a reason unrelated to real link health. Real GTK pipeline construction
/// is sub-second in the spike's own real-hardware timing, so 3s stays
/// generous while staying safely clear of the watchdog.
const GTK_PIPELINE_HANDOFF_TIMEOUT: Duration = Duration::from_secs(3);

impl Gtk4WindowHandoff {
    fn request(
        &self,
        capability: DecoderCapability,
    ) -> Result<VideoRenderPipeline, GstreamerError> {
        self.capability_sender
            .send(capability)
            .map_err(|_| GstreamerError::Initialization("GTK window thread is gone".into()))?;
        self.pipeline_receiver
            .recv_timeout(GTK_PIPELINE_HANDOFF_TIMEOUT)
            .map_err(|_| {
                GstreamerError::Initialization(
                    "timed out waiting for the GTK window thread to build the render pipeline"
                        .into(),
                )
            })?
    }
}

/// Lifecycle of the real PCM playback pipeline for one audio channel
/// (`MediaAudio`/`SystemAudio`/`SpeechAudio` each get their own instance),
/// independent of that channel's own protocol state — same shape and same
/// reasoning as `VideoRenderState` (a pipeline failure must never affect
/// protocol-level correctness). Built lazily on `AudioSetupAction::Ready`.
enum AudioPlaybackState {
    /// `GstreamerBackend::new()` itself failed for this channel's backend —
    /// never attempted again this run. The error is printed at the point
    /// of failure, not retained here.
    Unavailable,
    /// Backend is ready; pipeline not yet built (waiting for `Ready`).
    NotStarted(GstreamerBackend),
    /// Pipeline built and playing.
    Running(RunningAudioPipeline),
    /// Construction, start, or a later push/bus error occurred — no
    /// further attempts are made this run, but the probe (and acking)
    /// keeps going. The error is printed at the point of failure, not
    /// retained here.
    Failed,
}

/// A running audio pipeline plus the bookkeeping needed to measure this
/// channel's real start latency exactly once: `started_at` is recorded the
/// instant the pipeline reaches `Playing`
/// (`start_audio_playback_pipeline`), and `first_frame_latency_logged`
/// latches after the first real `MediaDataReceived` push
/// (`apply_to_running_audio_pipeline`) so the metric is reported once per
/// channel, not once per frame. This is `MILESTONE_CHECKLIST.md` M3's
/// "measure ... audio ... latency against provisional targets" — the PRD's
/// "audio start latency below 150 ms" target — measuring software dispatch
/// latency (`Start` handled/pipeline started to first real frame pushed),
/// not glass-to-glass audible time, which no in-process timestamp can see.
struct RunningAudioPipeline {
    pipeline: AudioPlaybackPipeline,
    started_at: std::time::Instant,
    first_frame_latency_logged: bool,
}

/// Bundles every real media pipeline's lifecycle state together — purely
/// to keep `run()`/`drain_and_dispatch_frames`/`handle_message`'s argument
/// lists and line counts manageable as this grows; each field is otherwise
/// independent (see `VideoRenderState`/`AudioPlaybackState`'s own doc
/// comments), not a shared state machine.
struct MediaPipelines {
    video_render: VideoRenderState,
    media_audio_playback: AudioPlaybackState,
    system_audio_playback: AudioPlaybackState,
    speech_audio_playback: AudioPlaybackState,
}

impl MediaPipelines {
    fn new() -> Self {
        Self {
            video_render: new_video_render_state(),
            media_audio_playback: new_audio_playback_state("media_audio"),
            system_audio_playback: new_audio_playback_state("system_audio"),
            speech_audio_playback: new_audio_playback_state("speech_audio"),
        }
    }
}

/// Per-channel progress for the `MediaAudio` channel, driven once
/// `ServiceDiscoveryResponse` has been sent. Same shape as `VideoChannel`
/// (see `protocol_aap::audio_setup` for why this is a separate type rather
/// than a shared one) — `Open` covers the entire post-open lifecycle, same
/// reasoning as `VideoChannel::Open`.
enum MediaAudioChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    Open(AudioSetupStateMachine),
}

/// Per-channel progress for the `SystemAudio` channel, driven once
/// `ServiceDiscoveryResponse` has been sent. Same shape as
/// `MediaAudioChannel` — `SystemAudioChannel` is a thin `AudioMediaSinkService`
/// subclass in AASDK too, and this project advertises a single uncompressed
/// PCM `AudioConfiguration` for it just like `MediaAudio`, so the same
/// `AudioSetupStateMachine` is reused unmodified.
enum SystemAudioChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    Open(AudioSetupStateMachine),
}

/// Per-channel progress for the `SpeechAudio` channel, driven once
/// `ServiceDiscoveryResponse` has been sent. Same shape as
/// `SystemAudioChannel`/`MediaAudioChannel` — AASDK's `GuidanceAudioChannel`
/// (this project's `SpeechAudio`, matching `AudioStreamType::Guidance`) is a
/// third thin `AudioMediaSinkService` subclass, and this project advertises
/// a single uncompressed PCM `AudioConfiguration` for it too, so the same
/// `AudioSetupStateMachine` is reused unmodified.
enum SpeechAudioChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    Open(AudioSetupStateMachine),
}

/// Per-channel progress for the `Sensors` channel, driven once
/// `ServiceDiscoveryResponse` has been sent. Simpler than the audio/video
/// channels — `SensorRequest`/`SensorResponse`/`SensorBatch` is a flat
/// request/response exchange with no further state transition, so unlike
/// `VideoChannel`/`MediaAudioChannel`/etc. there's no dedicated inner
/// state machine, just `AwaitingOpen` then `Open`.
enum SensorsChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    Open,
}

/// What happened while routing one assembled message, as returned by
/// `handle_message`. Distinguishes the probe's original success condition
/// (video `Start` received, input opened) from the phone's own explicit
/// `ByeByeRequest` session-end signal — both are clean, non-error stops,
/// but warrant different `run()` reporting.
enum ProbeOutcome {
    Continue,
    ChannelSetupComplete,
    PhoneEndedSession(ByeByeReason),
}

/// Ping/pong timing, armed only once `ServiceDiscoveryResponse` is sent —
/// matching LIVI's `Session.ts` (`docs/protocol/livi-adoption.md`, "Adopted
/// scope" item 5), not the earlier OpenAuto-derived "armed at session
/// start, before even `VersionRequest`" model. `last_pong` doubles as the
/// watchdog baseline: if it goes stale past `PING_WATCHDOG_TIMEOUT`, this
/// probe closes the session itself, matching LIVI's own local watchdog
/// behavior.
struct PingState {
    last_sent: Instant,
    last_pong: Instant,
    /// How many scheduled proactive sends have fired since arming (the
    /// first real `PingRequest`, sent immediately on arm, counts as send
    /// 1). Used only by the `AA_HEADUNIT_PING_ISOLATION` experiment (see
    /// `service_ping`) to identify the *second* scheduled send — the one
    /// that has hit the USB write timeout in every real-hardware trial so
    /// far — without touching the first, already-proven-to-succeed send.
    sends_since_arm: u32,
}

/// Reads and reports both opt-in, off-by-default real-hardware experiment
/// flags (see `PING_ISOLATION_ENV_VAR`/`PROACTIVE_WRITE_RESILIENCE_ENV_VAR`'s
/// doc comments) as `(ping_isolation, survive_proactive_write_timeout)`.
/// Extracted from `run()` to keep it under the project's line-count lint.
struct ExperimentFlags {
    ping_isolation: bool,
    survive_proactive_write_timeout: bool,
    reactive_triggered_proactive_send: bool,
    observation_window_override: Option<Duration>,
    touch_rotation: Rotation,
}

fn read_experiment_flags() -> Result<ExperimentFlags, CliError> {
    let ping_isolation = std::env::var_os(PING_ISOLATION_ENV_VAR).is_some();
    if ping_isolation {
        println!("probe_state=ping_isolation_experiment_enabled");
    }
    let reactive_triggered_proactive_send =
        std::env::var_os(REACTIVE_TRIGGERED_PROACTIVE_SEND_ENV_VAR).is_some();
    if reactive_triggered_proactive_send {
        println!("probe_state=reactive_triggered_proactive_send_experiment_enabled");
    }
    // See REACTIVE_TRIGGERED_PROACTIVE_SEND_ENV_VAR's doc comment: this
    // experiment implies resilience, since it's only informative if the
    // session survives long enough to compare multiple attempts.
    let survive_proactive_write_timeout = reactive_triggered_proactive_send
        || std::env::var_os(PROACTIVE_WRITE_RESILIENCE_ENV_VAR).is_some();
    if survive_proactive_write_timeout {
        println!("probe_state=proactive_write_resilience_experiment_enabled");
    }
    let observation_window_override = read_observation_window_override()?;
    let touch_rotation = read_touch_rotation()?;
    Ok(ExperimentFlags {
        ping_isolation,
        survive_proactive_write_timeout,
        reactive_triggered_proactive_send,
        observation_window_override,
        touch_rotation,
    })
}

/// Parses `TOUCH_ROTATION_ENV_VAR` — see that constant's doc comment.
fn read_touch_rotation() -> Result<Rotation, CliError> {
    let Some(value) = std::env::var_os(TOUCH_ROTATION_ENV_VAR) else {
        return Ok(Rotation::Rotate0);
    };
    let text = value.to_str().unwrap_or("");
    let rotation = match text {
        "0" => Rotation::Rotate0,
        "90" => Rotation::Rotate90,
        "180" => Rotation::Rotate180,
        "270" => Rotation::Rotate270,
        _ => {
            return Err(CliError::Usage(format!(
                "{TOUCH_ROTATION_ENV_VAR} must be one of 0, 90, 180, 270"
            )));
        }
    };
    println!("probe_state=touch_rotation_override_enabled");
    println!("touch_rotation_override_degrees={text}");
    Ok(rotation)
}

/// Parses `OBSERVATION_WINDOW_SECONDS_ENV_VAR` — see that constant's doc
/// comment for why an invalid value is a hard usage error rather than a
/// silently-ignored fallback.
pub(crate) fn read_observation_window_override() -> Result<Option<Duration>, CliError> {
    let Some(value) = std::env::var_os(OBSERVATION_WINDOW_SECONDS_ENV_VAR) else {
        return Ok(None);
    };
    let seconds = value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "{OBSERVATION_WINDOW_SECONDS_ENV_VAR} must be a positive integer number of seconds"
            ))
        })?;
    println!("probe_state=observation_window_override_enabled");
    println!("observation_window_override_seconds={seconds}");
    Ok(Some(Duration::from_secs(seconds)))
}

// `video_render_target` is only ever borrowed inside this function (passed
// down as `&video_render_target`), never moved-from — an intentional API
// choice, not an oversight clippy should flag: `run` must own it for the
// whole session so the caller can't keep using `Gtk4WindowHandoff`'s
// channels after handing off, and `VideoRenderTarget` isn't `Clone` (it
// holds a non-`Clone` `mpsc::Receiver`), so a `&VideoRenderTarget`
// parameter here would just push the same ownership decision onto every
// caller instead.
#[allow(clippy::needless_pass_by_value)]
pub fn run<T: SessionTransport>(
    transport: &mut T,
    tls12_compatibility: bool,
    credentials: CredentialMaterial,
    video_render_target: VideoRenderTarget,
    cancel: &crate::cancellation::CancellationFlag,
) -> Result<(), CliError> {
    println!("probe_scope=version_tls_auth_and_service_discovery_summary");
    println!("probe_credentials=user_supplied_runtime");
    println!(
        "probe_tls_policy={}",
        if tls12_compatibility {
            "tls12_compat"
        } else {
            "system_default"
        }
    );
    println!("probe_payload_logging=disabled");

    let mut tls = OpenSslTlsClient::from_pem_with_policy(
        credentials.certificate_pem(),
        credentials.private_key_pem(),
        64 * 1024,
        if tls12_compatibility {
            TlsVersionPolicy::Tls12Only
        } else {
            TlsVersionPolicy::SystemDefault
        },
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    drop(credentials);

    let limits = ProtocolLimits::default();
    let experiment_flags = read_experiment_flags()?;
    let mut handshake = HandshakeStateMachine::default();
    let mut actions: VecDeque<_> = handshake
        .advance(HandshakeEvent::Start)
        .map_err(|error| CliError::Protocol(error.to_string()))?
        .into();
    process_actions(&mut actions, &mut handshake, &mut tls, transport, limits)?;
    println!("probe_state=version_request_sent");

    let deadline = Instant::now() + observation_window(&experiment_flags);
    // Not armed at session start (see `PingState`'s doc comment) — this
    // probe never sends a `PingRequest` until `ServiceDiscoveryResponse`
    // has actually been sent.
    let mut ping_state: Option<PingState> = None;
    let mut received = Vec::new();
    let mut read_buffer = vec![0_u8; AASDK_MAX_FRAME_PAYLOAD_SIZE + 8];
    // Control channel (0) + video channel + input channel + MediaAudio
    // channel + SystemAudio channel + SpeechAudio channel + Sensors channel
    // can each independently be mid-fragmentation once channel setup starts.
    let mut assembler =
        MessageAssembler::new(7).map_err(|error| CliError::Protocol(error.to_string()))?;

    // Set once ChannelSetupComplete is first reached; no longer stops the
    // probe (see report_probe_outcome) — the loop keeps running to observe
    // whatever the phone sends after video Start, until PROBE_TIMEOUT.
    let mut channel_setup_complete = false;
    let mut video_channel: Option<VideoChannel> = None;
    let mut media_audio_channel: Option<MediaAudioChannel> = None;
    let mut system_audio_channel: Option<SystemAudioChannel> = None;
    let mut speech_audio_channel: Option<SpeechAudioChannel> = None;
    let mut media_pipelines = MediaPipelines::new();
    let mut sensors_channel: Option<SensorsChannel> = None;
    // Every channel that only ever needs to reach ChannelOpenState::Open —
    // input/touch plus two of the six non-video channels this experiment
    // adds (MediaAudio, SystemAudio, SpeechAudio, and Sensors now have their
    // own dedicated state machines above, like video). None until
    // ServiceDiscoveryResponse is sent, then populated with one entry per
    // advertised channel_id.
    let mut simple_channels: HashMap<u8, ChannelOpenStateMachine> = HashMap::new();
    let touch_source = open_touch_source(experiment_flags.touch_rotation);
    let touch_settings = touch_settings_handoff(&video_render_target);
    let mut gesture_detector = setup_settings_gesture(touch_source.as_ref(), touch_settings);

    while Instant::now() < deadline && !cancel.is_set() {
        let size = match transport.receive(&mut read_buffer) {
            Ok(size) => size,
            Err(TransportError::TimedOut) => continue,
            Err(error) => return Err(CliError::Transport(error)),
        };
        if received.len() + size > MAX_ACCUMULATED_BYTES {
            return Err(CliError::Protocol(
                "incoming frame buffer exceeded the probe limit".into(),
            ));
        }
        received.extend_from_slice(&read_buffer[..size]);

        let (stop, reactive_frame_processed) = drain_and_dispatch_frames(
            &mut received,
            &mut assembler,
            &mut handshake,
            &mut video_channel,
            &mut media_audio_channel,
            &mut system_audio_channel,
            &mut speech_audio_channel,
            &mut media_pipelines,
            &video_render_target,
            &mut sensors_channel,
            &mut simple_channels,
            &mut ping_state,
            &mut channel_setup_complete,
            &mut tls,
            transport,
            limits,
        )?;
        if stop {
            return Ok(());
        }
        service_proactive_sends(
            &mut ping_state,
            &experiment_flags,
            reactive_frame_processed,
            touch_source.as_ref(),
            &mut gesture_detector,
            touch_settings,
            &simple_channels,
            transport,
            &mut tls,
            limits,
        )?;
    }

    finish_probe_after_loop(channel_setup_complete, cancel.is_set(), &tls)
}

/// `run()`'s outcome once its loop exits without an earlier explicit
/// stop — either the deadline was reached or the operator cancelled
/// (`Ctrl-C`) — split out purely to keep `run()` itself under
/// `clippy::too_many_lines`.
fn finish_probe_after_loop(
    channel_setup_complete: bool,
    cancelled: bool,
    tls: &OpenSslTlsClient,
) -> Result<(), CliError> {
    if cancelled {
        println!("probe_state=cancelled_by_operator");
        println!("probe_result=cancelled");
        return Err(CliError::Cancelled);
    }
    if channel_setup_complete {
        println!("probe_result=observation_window_complete");
        return Ok(());
    }
    println!("probe_tls_state={}", tls.handshake_state());
    Err(CliError::Protocol(
        "auth/service-discovery/channel-setup probe timed out before completion".into(),
    ))
}

/// Prints diagnostic lines for a [`ProbeOutcome`] and reports whether
/// `run()` should stop immediately. `ChannelSetupComplete` no longer stops
/// the probe — video `Start` is now a milestone to keep observing past
/// (does the phone send real media data?), not a termination signal —
/// `channel_setup_complete` is set once so `run()` can tell a clean
/// end-of-observation-window timeout apart from a genuine failure.
fn report_probe_outcome(outcome: &ProbeOutcome, channel_setup_complete: &mut bool) -> bool {
    match outcome {
        ProbeOutcome::Continue => false,
        ProbeOutcome::ChannelSetupComplete => {
            if !*channel_setup_complete {
                println!("probe_state=channel_setup_complete");
                println!("probe_state=observing_for_post_start_media_traffic");
                crate::connection_state::report(
                    crate::connection_state::ConnectionState::Connected,
                );
                *channel_setup_complete = true;
            }
            false
        }
        ProbeOutcome::PhoneEndedSession(reason) => {
            println!("probe_result=phone_ended_session");
            println!("byebye_reason={reason:?}");
            println!("probe_stop=byebye_request_received");
            true
        }
    }
}

/// Routes one assembled message by channel. See [`ProbeOutcome`].
/// Decodes and dispatches every fully-assembled message currently sitting
/// in `received` (there may be more than one per read), routing each
/// through [`handle_message`]. Returns `(stop, processed_any)`: `stop` is
/// `true` if `run()` should stop immediately (see [`report_probe_outcome`]);
/// `processed_any` is `true` if at least one message was actually decoded
/// and dispatched this call — a genuine reactive round trip, not just a
/// nonzero byte count — used by `service_ping` when
/// `REACTIVE_TRIGGERED_PROACTIVE_SEND_ENV_VAR` is set.
#[allow(clippy::too_many_arguments)]
fn drain_and_dispatch_frames<T: SessionTransport>(
    received: &mut Vec<u8>,
    assembler: &mut MessageAssembler,
    handshake: &mut HandshakeStateMachine,
    video_channel: &mut Option<VideoChannel>,
    media_audio_channel: &mut Option<MediaAudioChannel>,
    system_audio_channel: &mut Option<SystemAudioChannel>,
    speech_audio_channel: &mut Option<SpeechAudioChannel>,
    media_pipelines: &mut MediaPipelines,
    video_render_target: &VideoRenderTarget,
    sensors_channel: &mut Option<SensorsChannel>,
    simple_channels: &mut HashMap<u8, ChannelOpenStateMachine>,
    ping_state: &mut Option<PingState>,
    channel_setup_complete: &mut bool,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(bool, bool), CliError> {
    let mut processed_any = false;
    loop {
        let frame = match decode_frame(received, limits) {
            Ok(frame) => frame,
            Err(FrameError::Incomplete { .. }) => return Ok((false, processed_any)),
            Err(error) => return Err(CliError::Protocol(error.to_string())),
        };
        let consumed = frame.consumed;
        let message = push_decoded_frame(frame, assembler, tls, handshake.state())?;
        received.drain(..consumed);
        let Some(message) = message else {
            continue;
        };
        processed_any = true;

        let outcome = handle_message(
            &message,
            handshake,
            video_channel,
            media_audio_channel,
            system_audio_channel,
            speech_audio_channel,
            media_pipelines,
            video_render_target,
            sensors_channel,
            simple_channels,
            ping_state,
            tls,
            transport,
            limits,
        )?;
        if report_probe_outcome(&outcome, channel_setup_complete) {
            return Ok((true, processed_any));
        }
    }
}

/// Arms every channel's state machine the instant `ServiceDiscoveryResponse`
/// has been sent — extracted out of `handle_message` purely to stay under
/// clippy's line-count limit; no behavior change from when this was inline.
#[allow(clippy::too_many_arguments)]
fn arm_channels_after_service_discovery(
    video_channel: &mut Option<VideoChannel>,
    media_audio_channel: &mut Option<MediaAudioChannel>,
    system_audio_channel: &mut Option<SystemAudioChannel>,
    speech_audio_channel: &mut Option<SpeechAudioChannel>,
    sensors_channel: &mut Option<SensorsChannel>,
    simple_channels: &mut HashMap<u8, ChannelOpenStateMachine>,
) {
    *video_channel = Some(VideoChannel::AwaitingOpen(ChannelOpenStateMachine::new(
        VIDEO_CHANNEL_ID,
    )));
    *media_audio_channel = Some(MediaAudioChannel::AwaitingOpen(
        ChannelOpenStateMachine::new(MEDIA_AUDIO_CHANNEL_ID),
    ));
    *system_audio_channel = Some(SystemAudioChannel::AwaitingOpen(
        ChannelOpenStateMachine::new(SYSTEM_AUDIO_CHANNEL_ID),
    ));
    *speech_audio_channel = Some(SpeechAudioChannel::AwaitingOpen(
        ChannelOpenStateMachine::new(SPEECH_AUDIO_CHANNEL_ID),
    ));
    *sensors_channel = Some(SensorsChannel::AwaitingOpen(ChannelOpenStateMachine::new(
        SENSORS_CHANNEL_ID,
    )));
    for channel_id in [
        INPUT_CHANNEL_ID,
        BLUETOOTH_CHANNEL_ID,
        MICROPHONE_CHANNEL_ID,
        RADIO_CHANNEL_ID,
    ] {
        simple_channels.insert(channel_id, ChannelOpenStateMachine::new(channel_id));
    }
}

fn simple_channel_is_open(
    simple_channels: &HashMap<u8, ChannelOpenStateMachine>,
    channel_id: u8,
) -> bool {
    simple_channels
        .get(&channel_id)
        .is_some_and(|machine| machine.state() == ChannelOpenState::Open)
}

#[allow(clippy::too_many_arguments)]
fn handle_message<T: SessionTransport>(
    message: &Message,
    handshake: &mut HandshakeStateMachine,
    video_channel: &mut Option<VideoChannel>,
    media_audio_channel: &mut Option<MediaAudioChannel>,
    system_audio_channel: &mut Option<SystemAudioChannel>,
    speech_audio_channel: &mut Option<SpeechAudioChannel>,
    media_pipelines: &mut MediaPipelines,
    video_render_target: &VideoRenderTarget,
    sensors_channel: &mut Option<SensorsChannel>,
    simple_channels: &mut HashMap<u8, ChannelOpenStateMachine>,
    ping_state: &mut Option<PingState>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<ProbeOutcome, CliError> {
    if message.channel_id == 0 {
        if handshake.state() == HandshakeState::ServiceDiscoveryReceived {
            if let Some(reason) =
                handle_post_discovery_control_message(message, ping_state, tls, transport, limits)?
            {
                return Ok(ProbeOutcome::PhoneEndedSession(reason));
            }
        } else if let Some(summary) =
            handle_assembled_message(message, handshake, tls, transport, limits)?
        {
            print_summary(&summary);
            println!("probe_result=service_discovery_summary_received");
            send_service_discovery_response(tls, transport, limits)?;
            // Arm ping/pong here, matching LIVI's own timing exactly (see
            // `PingState`'s doc comment) — not before, not after channel
            // setup starts.
            send_ping_request(transport, tls, limits)?;
            let now = Instant::now();
            *ping_state = Some(PingState {
                last_sent: now,
                last_pong: now,
                sends_since_arm: 1,
            });
            println!("probe_state=ping_armed");
            arm_channels_after_service_discovery(
                video_channel,
                media_audio_channel,
                system_audio_channel,
                speech_audio_channel,
                sensors_channel,
                simple_channels,
            );
        }
        return Ok(ProbeOutcome::Continue);
    }

    if message.channel_id == VIDEO_CHANNEL_ID {
        handle_video_channel_message(
            message,
            video_channel,
            &mut media_pipelines.video_render,
            video_render_target,
            tls,
            transport,
            limits,
        )?;
    } else if message.channel_id == MEDIA_AUDIO_CHANNEL_ID {
        handle_media_audio_channel_message(
            message,
            media_audio_channel,
            &mut media_pipelines.media_audio_playback,
            tls,
            transport,
            limits,
        )?;
    } else if message.channel_id == SYSTEM_AUDIO_CHANNEL_ID {
        handle_system_audio_channel_message(
            message,
            system_audio_channel,
            &mut media_pipelines.system_audio_playback,
            tls,
            transport,
            limits,
        )?;
    } else if message.channel_id == SPEECH_AUDIO_CHANNEL_ID {
        handle_speech_audio_channel_message(
            message,
            speech_audio_channel,
            &mut media_pipelines.speech_audio_playback,
            tls,
            transport,
            limits,
        )?;
    } else if message.channel_id == SENSORS_CHANNEL_ID {
        handle_sensors_channel_message(message, sensors_channel, tls, transport, limits)?;
    } else if message.channel_id == INPUT_CHANNEL_ID
        && simple_channel_is_open(simple_channels, INPUT_CHANNEL_ID)
    {
        handle_input_channel_message(message, tls, transport, limits)?;
    } else if message.channel_id == BLUETOOTH_CHANNEL_ID
        && simple_channel_is_open(simple_channels, BLUETOOTH_CHANNEL_ID)
    {
        handle_bluetooth_channel_message(message, tls, transport, limits)?;
    } else {
        handle_simple_channel_message(
            message.channel_id,
            message,
            simple_channels,
            tls,
            transport,
            limits,
        )?;
    }

    let input_open = simple_channel_is_open(simple_channels, INPUT_CHANNEL_ID);
    let video_ready = matches!(
        video_channel,
        Some(VideoChannel::Open(machine)) if machine.state() == VideoSetupState::Ready
    );
    Ok(if video_ready && input_open {
        ProbeOutcome::ChannelSetupComplete
    } else {
        ProbeOutcome::Continue
    })
}

/// Handles control-channel traffic that arrives after `HandshakeStateMachine`
/// has already reached `ServiceDiscoveryReceived` (which has nothing further
/// to do — see `docs/protocol/error-2-investigation.md`). `AudioFocusRequest`,
/// `PingResponse`, `PingRequest`, `NavFocusRequest`, `VoiceSessionNotification`,
/// and `ByeByeRequest` are handled; anything else fails closed with a clear,
/// distinct error naming the unexpected message, so if the phone sends
/// something new next, that's immediately visible rather than silently
/// swallowed. Returns `Some(reason)` only for `ByeByeRequest` — the
/// protocol's own explicit session-end signal, which `run()` treats as a
/// clean stop, not an error.
///
/// `VoiceSessionNotification` (wire id 17, `START`/`END`) was discovered as
/// a real, previously-unmapped gap by a real phone: it arrived, unprompted,
/// when a `WhatsApp` message notification came in during a trial, and crashed
/// the whole probe, since any `ControlMessageId::Unknown` here was a hard
/// error. Decoded and
/// logged only, no reply sent — `f-io/LIVI`'s `ControlChannel.ts`
/// (`docs/protocol/livi-adoption.md`) documents this as "no response
/// expected (matches aasdk + openauto behaviour)" for the ordinary
/// phone-initiated case; see `crates/protocol-aap/src/voice_session.rs`'s
/// module doc comment for the full citation, including the separate
/// head-unit-initiated push-to-talk path this project has no working
/// microphone hardware to exercise.
///
/// `PingRequest` arriving from the phone (not just the `PingResponse` this
/// probe expects after its own proactive `PingRequest`, see
/// `PING_INTERVAL`) was found the same way, in the first successful
/// wireless-bootstrap trial: the session had already reached
/// `connection_state=connected` with video rendering, then the phone sent
/// its own `PingRequest` and this probe treated it as an unmapped control
/// message and aborted the whole session over it — a real, previously-
/// unseen defect, not specific to wireless. AASDK's `ControlServiceChannel`
/// handles `PingRequest` from either side; replying with a `PingResponse`
/// echoing the phone's own timestamp (`crates/protocol-aap/src/ping.rs`)
/// mirrors exactly what this probe already does when it's the initiator.
fn handle_post_discovery_control_message<T: SessionTransport>(
    message: &Message,
    ping_state: &mut Option<PingState>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<Option<ByeByeReason>, CliError> {
    if message.message_type != MessageType::Specific {
        return Err(CliError::Protocol(
            "unexpected control message type after service discovery".into(),
        ));
    }
    let control_message = ControlMessage::decode(&message.payload, DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    match control_message.id {
        ControlMessageId::AudioFocusRequest => {
            let requested = decode_audio_focus_request(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=audio_focus_requested");
            println!("audio_focus_request_type={requested:?}");
            let granted = grant_audio_focus(requested);
            let response = encode_audio_focus_notification(granted);
            let payload = response
                .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
            println!("probe_state=audio_focus_notification_sent");
            Ok(None)
        }
        ControlMessageId::PingResponse => {
            let echoed_timestamp = decode_ping_response(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=ping_response_received");
            println!("ping_response_echoed_timestamp={echoed_timestamp}");
            if let Some(state) = ping_state.as_mut() {
                state.last_pong = Instant::now();
            }
            Ok(None)
        }
        ControlMessageId::PingRequest => {
            let requested_timestamp = decode_ping_request(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=phone_ping_request_received");
            println!("phone_ping_requested_timestamp={requested_timestamp}");
            let response = encode_ping_response(requested_timestamp);
            let payload = response
                .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
            println!("probe_state=phone_ping_response_sent");
            Ok(None)
        }
        ControlMessageId::NavFocusRequest => {
            let requested_type = decode_nav_focus_request(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=nav_focus_requested");
            println!("nav_focus_requested_type={requested_type:?}");
            let response = encode_nav_focus_notification(NavFocusType::Projected);
            let payload = response
                .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
            println!("probe_state=nav_focus_notification_sent");
            Ok(None)
        }
        ControlMessageId::VoiceSessionNotification => {
            let status = decode_voice_session_notification(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=voice_session_notification_received");
            println!("voice_session_status={status:?}");
            Ok(None)
        }
        ControlMessageId::ByeByeRequest => {
            let reason = decode_byebye_request(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=byebye_requested");
            println!("byebye_reason={reason:?}");
            let response = encode_byebye_response();
            let payload = response
                .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
            println!("probe_state=byebye_response_sent");
            Ok(Some(reason))
        }
        other => Err(CliError::Protocol(format!(
            "unexpected control message {other:?} after service discovery"
        ))),
    }
}

/// Checks `ping_state` once per call: closes the session via the
/// LIVI-derived watchdog if `PingResponse` has gone stale past
/// `PING_WATCHDOG_TIMEOUT` (unless suppressed, see
/// `PROACTIVE_WRITE_RESILIENCE_ENV_VAR`'s doc comment), otherwise sends a
/// new proactive message once `PING_INTERVAL` has elapsed since the last
/// one. A no-op before `ping_state` is armed (see `PingState`'s doc
/// comment). Called from `run()`'s loop after `drain_and_dispatch_frames`
/// rather than before it, so `reactive_frame_processed` reflects whether a
/// real message was just decoded and dispatched this iteration (see
/// `REACTIVE_TRIGGERED_PROACTIVE_SEND_ENV_VAR`'s doc comment) — when that
/// experiment is off, this parameter is ignored and a due send fires
/// unconditionally, matching every prior trial's behavior.
///
/// `flags.ping_isolation` selects the `AA_HEADUNIT_PING_ISOLATION`
/// experiment (see its doc comment below): when set, every scheduled send
/// *after* the first (which always stays a real `PingRequest` —
/// real-hardware-confirmed to succeed and get a real `PingResponse`, see
/// `docs/protocol/error-2-investigation.md`, "LIVI formally adopted; real
/// ping-timing trial") substitutes a harmless, already-proven-safe
/// unsolicited message instead of a second `PingRequest`, to distinguish
/// "a second `PingRequest` specifically fails" from "any second proactive,
/// timer-fired write at that point in the session fails."
fn service_ping<T: SessionTransport>(
    ping_state: &mut Option<PingState>,
    flags: &ExperimentFlags,
    reactive_frame_processed: bool,
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let Some(state) = ping_state else {
        return Ok(());
    };
    if !flags.survive_proactive_write_timeout && state.last_pong.elapsed() >= PING_WATCHDOG_TIMEOUT
    {
        println!("probe_result=ping_watchdog_timeout");
        return Err(CliError::Protocol(format!(
            "no PingResponse within {}ms of the last one — closing session \
             (LIVI-derived watchdog, docs/protocol/livi-adoption.md)",
            PING_WATCHDOG_TIMEOUT.as_millis()
        )));
    }
    let due = state.last_sent.elapsed() >= PING_INTERVAL;
    let gated_by_reactive_trigger =
        flags.reactive_triggered_proactive_send && !reactive_frame_processed;
    if due && !gated_by_reactive_trigger {
        let attempt = state.sends_since_arm + 1;
        let send_result = if flags.ping_isolation && state.sends_since_arm >= 1 {
            println!("probe_state=ping_isolation_control_frame_send_attempt");
            send_control_probe_frame(transport, tls, limits)
        } else {
            send_ping_request(transport, tls, limits)
        };
        match send_result {
            Ok(()) => {
                if flags.ping_isolation && state.sends_since_arm >= 1 {
                    println!("probe_state=ping_isolation_control_frame_sent");
                }
            }
            Err(error) if flags.survive_proactive_write_timeout => {
                println!("probe_state=proactive_write_timed_out attempt={attempt} error={error}");
            }
            Err(error) => return Err(error),
        }
        state.last_sent = Instant::now();
        state.sends_since_arm += 1;
    }
    Ok(())
}

/// Every proactive (not-a-reply) send `run()`'s loop attempts once per
/// iteration, grouped into one call purely to keep `run()` itself under
/// `clippy::too_many_lines`.
#[allow(clippy::too_many_arguments)]
fn service_proactive_sends<T: SessionTransport>(
    ping_state: &mut Option<PingState>,
    flags: &ExperimentFlags,
    reactive_frame_processed: bool,
    touch_source: Option<&EvdevTouchSource>,
    gesture_detector: &mut ArmedGestureDetector,
    touch_settings: Option<&TouchSettingsHandoff>,
    simple_channels: &HashMap<u8, ChannelOpenStateMachine>,
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    service_ping(
        ping_state,
        flags,
        reactive_frame_processed,
        transport,
        tls,
        limits,
    )?;
    let input_open = simple_channels
        .get(&INPUT_CHANNEL_ID)
        .is_some_and(|machine| machine.state() == ChannelOpenState::Open);
    service_touch_input(
        touch_source,
        gesture_detector,
        touch_settings,
        input_open,
        transport,
        tls,
        limits,
    )
}

/// `Some` only when running under `VideoRenderTarget::Gtk4Window` — the
/// plain `Wayland` path has no window/desktop concept for a settings panel
/// to belong to, so the settings-gesture machinery is simply not driven at
/// all there (see `service_touch_input`).
fn touch_settings_handoff(target: &VideoRenderTarget) -> Option<&TouchSettingsHandoff> {
    match target {
        VideoRenderTarget::Wayland => None,
        VideoRenderTarget::Gtk4Window(_, touch_settings) => Some(touch_settings),
    }
}

/// Resolves how long the receive loop's deadline should be from `run()`'s
/// own start, given the operator's chosen experiment flags. Extracted
/// purely to keep `run()` itself under `clippy::too_many_lines`.
fn observation_window(experiment_flags: &ExperimentFlags) -> Duration {
    experiment_flags.observation_window_override.unwrap_or(
        if experiment_flags.survive_proactive_write_timeout {
            PROACTIVE_WRITE_SURVIVAL_TIMEOUT
        } else {
            PROBE_TIMEOUT
        },
    )
}

/// Sends the touchscreen's live-adjustable rotation handle (if any
/// touchscreen and any `TouchSettingsHandoff` exist) exactly once, builds
/// the `ArmedGestureDetector` `run()`'s loop feeds every touch frame into
/// (its arm-window duration seeded from the persisted
/// `gesture_settings::GestureSettings`, so a previous session's
/// customized timeout survives a restart), and sends its live-adjustable
/// arm-window handle too. Extracted purely to keep `run()` itself under
/// `clippy::too_many_lines`.
fn setup_settings_gesture(
    touch_source: Option<&EvdevTouchSource>,
    touch_settings: Option<&TouchSettingsHandoff>,
) -> ArmedGestureDetector {
    if let Some(touch_settings) = touch_settings {
        let rotation_handle = touch_source.map(EvdevTouchSource::rotation_handle);
        let _ = touch_settings.rotation_sender.send(rotation_handle);
    }
    let arm_window_seconds = crate::gesture_settings::GestureSettings::load(std::path::Path::new(
        crate::gesture_settings::DEFAULT_SETTINGS_PATH,
    ))
    .arm_window_seconds();
    let detector = ArmedGestureDetector::new(
        SETTINGS_GESTURE_SWIPE_THRESHOLD_PIXELS,
        u64::from(arm_window_seconds) * 1_000_000,
    );
    if let Some(touch_settings) = touch_settings {
        let _ = touch_settings
            .arm_window_sender
            .send(detector.arm_window_handle());
    }
    detector
}

/// Best-effort touchscreen discovery, run once before the receive loop
/// starts. Mirrors `video_render`/`media_pipelines`' posture: a missing or
/// unopenable touchscreen is logged and never fails the probe — the head
/// unit may simply not be attached to a display yet (e.g. an SSH-only
/// session), and touch input has no bearing on protocol correctness.
fn open_touch_source(rotation: Rotation) -> Option<EvdevTouchSource> {
    let path = match discover_touchscreen() {
        Ok(Some(path)) => path,
        Ok(None) => {
            println!("probe_state=touch_input_unavailable reason=no_touchscreen_found");
            return None;
        }
        Err(error) => {
            println!("probe_state=touch_input_unavailable reason=discovery_failed error={error}");
            return None;
        }
    };
    match EvdevTouchSource::open(
        &path,
        TOUCH_COORDINATE_SPACE_WIDTH_PIXELS,
        TOUCH_COORDINATE_SPACE_HEIGHT_PIXELS,
        rotation,
    ) {
        Ok(source) => {
            println!(
                "probe_state=touch_input_source_opened path={}",
                path.display()
            );
            Some(source)
        }
        Err(error) => {
            println!("probe_state=touch_input_unavailable reason=open_failed error={error}");
            None
        }
    }
}

/// Maps `platform_api::TouchPhase` onto the wire `PointerAction` it always
/// corresponds to (both are literally Android's own `MotionEvent.ACTION_*`
/// contract — see `protocol_aap::PointerAction`'s doc comment).
const fn touch_wire_action(phase: TouchPhase) -> PointerAction {
    match phase {
        TouchPhase::Down => PointerAction::Down,
        TouchPhase::Up => PointerAction::Up,
        TouchPhase::Moved => PointerAction::Moved,
        TouchPhase::PointerDown => PointerAction::PointerDown,
        TouchPhase::PointerUp => PointerAction::PointerUp,
    }
}

/// Logs and forwards one `ArmedGestureDetector` event to the GTK thread.
fn report_settings_gesture_event(event: GestureEvent, touch_settings: &TouchSettingsHandoff) {
    match event {
        GestureEvent::Armed => println!("probe_state=settings_gesture_armed"),
        GestureEvent::Disarmed => println!("probe_state=settings_gesture_disarmed"),
        GestureEvent::Completed(gesture) => {
            println!("probe_state=settings_gesture_detected gesture={gesture:?}");
        }
    }
    let _ = touch_settings.gesture_sender.send(event);
}

/// If `gesture`'s currently-assigned action is one of the four
/// `SwitchTo*` category switches, sends the matching real-key-press pair
/// (`down=true` then `down=false`, per `encode_key_event`'s doc comment)
/// on the already-open input channel. A no-op for every other action —
/// those are dispatched locally by the GTK thread instead (see
/// `gtk_dev_ui.rs::dispatch_action`'s doc comment for why the split), and
/// this function has no window/rotation state to act on anyway. Runs on
/// this background thread specifically because it's the one thread with
/// transport/TLS access.
fn dispatch_gesture_key_action<T: SessionTransport>(
    gesture: platform_api::GestureId,
    timestamp_micros: u64,
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let action = crate::gesture_settings::GestureSettings::load(std::path::Path::new(
        crate::gesture_settings::DEFAULT_SETTINGS_PATH,
    ))
    .action_for(gesture);
    let Some(keycode) = action.key_code() else {
        return Ok(());
    };
    for down in [true, false] {
        let message = encode_key_event(timestamp_micros, keycode, down);
        let payload = message
            .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
            .map_err(|error| CliError::Protocol(error.to_string()))?;
        send_encrypted(
            transport,
            tls,
            INPUT_CHANNEL_ID,
            MessageType::Specific,
            &payload,
            limits,
        )?;
    }
    println!("probe_state=settings_gesture_key_sent gesture={gesture:?} key_code={keycode:?}");
    Ok(())
}

/// Drains every touch frame queued since the last call and sends each as an
/// `InputReport` — proactive, not a reply to anything the phone sent, so
/// this only runs once the input channel has actually reached `Open`
/// (sending on an unopened channel would be a protocol violation the phone
/// has no reason to expect).
fn service_touch_input<T: SessionTransport>(
    touch_source: Option<&EvdevTouchSource>,
    gesture_detector: &mut ArmedGestureDetector,
    touch_settings: Option<&TouchSettingsHandoff>,
    input_open: bool,
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    if !input_open {
        return Ok(());
    }
    let Some(touch_source) = touch_source else {
        return Ok(());
    };
    if let Some(touch_settings) = touch_settings {
        // Independent of whether any frame arrives this call — an armed
        // window with no further touch input at all still needs to
        // disarm eventually (see `ArmedGestureDetector::tick`'s doc
        // comment).
        let now_micros = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros(),
        )
        .unwrap_or(u64::MAX);
        if let Some(event) = gesture_detector.tick(now_micros) {
            report_settings_gesture_event(event, touch_settings);
        }
    }
    while let Some(frame) = touch_source.try_recv() {
        if let Some(touch_settings) = touch_settings {
            if let Some(event) = gesture_detector.push(&frame) {
                if let GestureEvent::Completed(gesture) = event {
                    dispatch_gesture_key_action(
                        gesture,
                        frame.timestamp_micros,
                        transport,
                        tls,
                        limits,
                    )?;
                }
                report_settings_gesture_event(event, touch_settings);
            }
        }
        let pointers: Vec<TouchPointer> = frame
            .points
            .iter()
            .map(|point| TouchPointer {
                x: point.x,
                y: point.y,
                pointer_id: point.pointer_id,
            })
            .collect();
        let message = encode_touch_report(
            frame.timestamp_micros,
            &pointers,
            frame.action_index,
            touch_wire_action(frame.phase),
        );
        let payload = message
            .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
            .map_err(|error| CliError::Protocol(error.to_string()))?;
        send_encrypted(
            transport,
            tls,
            INPUT_CHANNEL_ID,
            MessageType::Specific,
            &payload,
            limits,
        )?;
        // Coordinates are never logged, matching the no-raw-payload-logging
        // rule applied to media `Data`/`CodecConfig` elsewhere in this file.
        println!(
            "probe_state=touch_report_sent phase={:?} pointer_count={}",
            frame.phase,
            frame.points.len()
        );
    }
    Ok(())
}

/// The `AA_HEADUNIT_PING_ISOLATION` experiment's substitute message: a
/// duplicate, unsolicited `VideoFocusNotification(Projected)` on the video
/// channel — the same message/value this probe already sends exactly once,
/// unconditionally, right after video `Config`
/// (`protocol_aap::video_setup::encode_video_focus_notification`,
/// real-hardware-confirmed safe and load-bearing, see
/// `docs/protocol/error-2-investigation.md`, "`VideoFocusNotification`
/// breakthrough"). Re-sending it a second time, unsolicited, is the
/// harmless-non-ping control frame an external analysis suggested: if the
/// phone tolerates a redundant focus grant the same way it tolerated the
/// first one, this write should succeed exactly like every other write in
/// this probe — if it hangs the same way a second `PingRequest` does, the
/// failure is about proactive-write *position* in the session, not `Ping`
/// content specifically.
fn send_control_probe_frame<T: SessionTransport>(
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let message = encode_video_focus_notification(VideoFocusMode::Projected);
    let payload = message
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    send_encrypted(
        transport,
        tls,
        VIDEO_CHANNEL_ID,
        MessageType::Specific,
        &payload,
        limits,
    )
}

/// Sends `PingRequest` on the control channel. See `PING_INTERVAL`'s doc
/// comment for why this is sent proactively at all, and for why it's sent
/// TLS-encrypted here — a deliberate deviation from AASDK's own
/// `sendPingRequest` (`EncryptionType::PLAIN`), being tested against the
/// real phone's undocumented protocol `1.7`. `timestamp` is epoch
/// milliseconds — a reasonable but unconfirmed assumption about the
/// field's units (`PingRequest.proto` only declares `required int64
/// timestamp = 1`, no unit); not load-bearing for this experiment, since
/// the phone only needs *a* consistent value it can echo back in
/// `PingResponse`.
fn send_ping_request<T: SessionTransport>(
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let timestamp_millis = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX);
    let payload = encode_ping_request(timestamp_millis)
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    println!("probe_state=ping_request_send_attempt");
    send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
    println!("probe_state=ping_request_sent");
    Ok(())
}

/// Placeholder audio-focus policy: grant exactly what's asked. This
/// project has no real audio hardware/focus-arbitration pipeline yet (M3
/// still open) — this is the simplest thing that answers honestly and
/// keeps the session alive, not a claim about real Android Auto behavior
/// (none is publicly documented — see the module doc comment).
const fn grant_audio_focus(requested: AudioFocusRequestType) -> AudioFocusStateType {
    match requested {
        AudioFocusRequestType::Gain => AudioFocusStateType::Gain,
        AudioFocusRequestType::GainTransient | AudioFocusRequestType::GainTransientMayDuck => {
            AudioFocusStateType::GainTransient
        }
        AudioFocusRequestType::Release => AudioFocusStateType::Loss,
    }
}

/// Builds and sends `ServiceDiscoveryResponse`, advertising all eight
/// `ServiceKind`s (the full canonical set `OpenAuto`'s `ServiceFactory`
/// unconditionally constructs — see the module doc comment) with
/// head-unit-chosen capability data — not phone-derived, so safe to
/// construct without any privacy concern (unlike
/// `ServiceDiscoveryRequestSummary`).
fn send_service_discovery_response<T: SessionTransport>(
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let catalogue = build_service_catalogue()?;
    let capabilities = build_service_capabilities();
    let response = encode_service_discovery_response(&catalogue, &capabilities)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let payload = response
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
    println!("probe_state=service_discovery_response_sent");
    Ok(())
}

/// The canonical eight-service set (`OpenAuto`'s `ServiceFactory`
/// finding, see the module doc comment) plus `Radio`, added 2026-08-16
/// and real-hardware-confirmed necessary for `KeyCode::Radio` to route
/// anywhere at all (see `RADIO_CHANNEL_ID`'s doc comment).
fn build_service_catalogue() -> Result<ServiceCatalogue, CliError> {
    ServiceCatalogue::build(
        &[
            ServiceCandidate {
                channel_id: VIDEO_CHANNEL_ID,
                kind: ServiceKind::Video,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: INPUT_CHANNEL_ID,
                kind: ServiceKind::Input,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: MEDIA_AUDIO_CHANNEL_ID,
                kind: ServiceKind::MediaAudio,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: SYSTEM_AUDIO_CHANNEL_ID,
                kind: ServiceKind::SystemAudio,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: SPEECH_AUDIO_CHANNEL_ID,
                kind: ServiceKind::SpeechAudio,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: SENSORS_CHANNEL_ID,
                kind: ServiceKind::Sensors,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: BLUETOOTH_CHANNEL_ID,
                kind: ServiceKind::Bluetooth,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: MICROPHONE_CHANNEL_ID,
                kind: ServiceKind::Microphone,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: RADIO_CHANNEL_ID,
                kind: ServiceKind::Radio,
                availability: ServiceAvailability::Ready,
            },
        ],
        DEFAULT_MAX_SERVICE_CANDIDATES,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))
}

/// Head-unit-chosen capability data for every advertised service — not
/// phone-derived, so safe to construct and log without any privacy
/// concern (unlike `ServiceDiscoveryRequestSummary`).
fn build_service_capabilities() -> ServiceCapabilities {
    ServiceCapabilities {
        // Both entries share the same resolution/frame-rate tier and
        // differ only in codec — position in this list is load-bearing:
        // `video_setup.rs`'s `ADVERTISED_H264_CONFIGURATION_INDEX`/
        // `ADVERTISED_H265_CONFIGURATION_INDEX` assume H.264 is index 0
        // and H.265 is index 1. H.265 is advertised on real-hardware
        // evidence that the same phone actively selects it over H.264 when
        // offered — see `docs/protocol/error-2-investigation.md`, "LIVI
        // known-good capture" — but this probe's own render pipeline only
        // decodes H.264 so far (see `start_video_render_pipeline`).
        //
        // Resolution tier: `VideoCodecResolution::Video1280x720`, not the
        // real Pi 5 reference display's actual native 800x480 panel
        // resolution. `TouchCapability` below now advertises this same
        // 1280x720 tier rather than the panel's native resolution — see
        // `TOUCH_COORDINATE_SPACE_WIDTH`'s doc comment. This is a deliberate,
        // single-variable, reversible real-hardware experiment — LIVI
        // advertised 1280x720, not 800x480, in the same known-good capture
        // that first surfaced H.265 (`docs/protocol/error-2-investigation.md`,
        // "H.265 advertisement implemented and tested"): H.265 alone at
        // 800x480 didn't change the phone's codec choice, so resolution
        // tier is now the variable under test, isolated from touch/display
        // capability, which stays truthful to the real target hardware.
        // frame_rate=Fps60 and density=180 both come from a real,
        // TLS-decrypted `f-io/LIVI` session capture (session-keylog +
        // usbmon, not source-code reuse — see
        // `docs/protocol/error-2-investigation.md`, "TLS-decrypted LIVI
        // session capture"): the exact wire bytes LIVI sends for its own
        // 1280x720 tier. This project had only ever advertised Fps30 and
        // never populated `density` at all.
        video: Some(vec![
            VideoCapability {
                resolution: VideoCodecResolution::Video1280x720,
                frame_rate: VideoFrameRate::Fps60,
                codec: VideoCodecType::H264,
                density: Some(180),
                // 10000 = 1:1 (square-pixel) ratio, matching LIVI's own
                // observed value (`PAR e4=10000`) — the only
                // `VideoConfiguration` field difference left after both
                // codec and resolution-tier advertisement were tried and
                // refuted as sufficient alone. See
                // `docs/protocol/error-2-investigation.md`, "1280×720
                // resolution tested".
                pixel_aspect_ratio_e4: Some(10000),
                // All-zero: matches LIVI's own default when no custom
                // display geometry is configured (see UiConfig's doc
                // comment, `service_discovery_response.rs`).
                ui_config: Some(UiConfig::default()),
            },
            VideoCapability {
                resolution: VideoCodecResolution::Video1280x720,
                frame_rate: VideoFrameRate::Fps60,
                codec: VideoCodecType::Hevc,
                density: Some(180),
                pixel_aspect_ratio_e4: Some(10000),
                ui_config: Some(UiConfig::default()),
            },
        ]),
        touch: Some(TouchCapability {
            width: TOUCH_COORDINATE_SPACE_WIDTH,
            height: TOUCH_COORDINATE_SPACE_HEIGHT,
            touch_type: TouchScreenType::Capacitive,
            // The four car-specific category-switch codes this project
            // can actually send (`send_category_switch_key_event`) —
            // real-hardware-untested until M3's swipe-direction gestures
            // are tried against a real phone (`MILESTONE_CHECKLIST.md`).
            keycodes_supported: vec![
                KeyCode::Media,
                KeyCode::Navigation,
                KeyCode::Radio,
                KeyCode::Tel,
            ],
        }),
        media_audio: Some(AudioCapability {
            sampling_rate: MEDIA_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: 2,
            stream_type: AudioStreamType::Media,
        }),
        system_audio: Some(AudioCapability {
            sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: VOICE_AUDIO_CHANNELS,
            stream_type: AudioStreamType::SystemAudio,
        }),
        speech_audio: Some(AudioCapability {
            sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: VOICE_AUDIO_CHANNELS,
            stream_type: AudioStreamType::Guidance,
        }),
        bluetooth: Some(BluetoothCapability {
            car_address: "02:00:00:00:00:01".into(),
        }),
        microphone: Some(MicrophoneCapability {
            sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: VOICE_AUDIO_CHANNELS,
        }),
        sensors: Some(SensorCapability {
            sensor_types: vec![SensorType::DrivingStatusData, SensorType::NightMode],
        }),
        // See `RADIO_CHANNEL_ID`'s doc comment — real-hardware-confirmed
        // to correctly route to Android Auto's native radio screen.
        // `radio_id`/`channel_spacing` are placeholder values; no real
        // tuner hardware exists for them to describe.
        radio: Some(RadioCapability {
            radio_id: 0,
            radio_type: RadioType::FmRadio,
            channel_spacing: 100,
        }),
        head_unit_info: Some(HeadUnitInfo {
            make: "pi-auto-headunit".into(),
            model: "aa-headunit-diagnostics".into(),
            year: "2026".into(),
            vehicle_id: "dev-probe".into(),
            head_unit_make: "pi-auto-headunit".into(),
            head_unit_model: "aa-headunit-diagnostics".into(),
            head_unit_software_build: env!("CARGO_PKG_VERSION").into(),
            head_unit_software_version: env!("CARGO_PKG_VERSION").into(),
        }),
        ping_configuration: Some(PingConfiguration {
            timeout_ms: ADVERTISED_PING_TIMEOUT_MS,
            interval_ms: ADVERTISED_PING_INTERVAL_MS,
            high_latency_threshold_ms: ADVERTISED_PING_HIGH_LATENCY_THRESHOLD_MS,
            tracked_ping_count: ADVERTISED_PING_TRACKED_COUNT,
        }),
    }
}

/// Drives the video channel's `ChannelOpenStateMachine` then
/// `VideoSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data.
fn handle_video_channel_message<T: SessionTransport>(
    message: &Message,
    video_channel: &mut Option<VideoChannel>,
    video_render: &mut VideoRenderState,
    video_render_target: &VideoRenderTarget,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = video_channel.as_mut().ok_or_else(|| {
        CliError::Protocol("video channel message before ServiceDiscoveryResponse was sent".into())
    })?;
    match state {
        VideoChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on video channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    VIDEO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=video_channel_open");
            *state = VideoChannel::Open(VideoSetupStateMachine::new());
            Ok(())
        }
        VideoChannel::Open(machine) => handle_video_channel_open_message(
            message,
            machine,
            video_render,
            video_render_target,
            tls,
            transport,
            limits,
        ),
    }
}

/// The `VideoChannel::Open` arm of `handle_video_channel_message`, split
/// out purely to keep that function under `clippy::too_many_lines` — no
/// behavior change from when this was inline.
#[allow(clippy::too_many_arguments)]
fn handle_video_channel_open_message<T: SessionTransport>(
    message: &Message,
    machine: &mut VideoSetupStateMachine,
    video_render: &mut VideoRenderState,
    video_render_target: &VideoRenderTarget,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    if message.message_type != MessageType::Specific {
        return Err(CliError::Protocol(
            "expected a video-channel media message".into(),
        ));
    }
    let actions = machine
        .advance(VideoSetupEvent::InboundMedia(&message.payload))
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    for action in actions {
        match action {
            VideoSetupAction::SetupRequested { codec_type } => {
                println!("probe_state=video_setup_requested");
                println!("video_setup_codec_type={codec_type}");
            }
            VideoSetupAction::SendMedia(response) => {
                let payload = response
                    .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                let response_id = response.id;
                send_encrypted(
                    transport,
                    tls,
                    VIDEO_CHANNEL_ID,
                    MessageType::Specific,
                    &payload,
                    limits,
                )?;
                match response_id {
                    MediaMessageId::VideoFocusNotification => {
                        println!("probe_state=video_channel_video_focus_notification_sent");
                    }
                    MediaMessageId::Ack => {
                        println!("probe_state=video_channel_ack_sent");
                    }
                    _ => {
                        println!("probe_state=video_channel_setup_config_sent");
                    }
                }
            }
            VideoSetupAction::Ready {
                session_id,
                configuration_index,
            } => handle_video_start_received(
                video_render,
                session_id,
                configuration_index,
                video_render_target,
            ),
            VideoSetupAction::MediaDataReceived { timestamp, payload } => {
                println!("probe_state=video_media_data_received");
                println!("video_media_data_timestamp={timestamp}");
                println!("video_media_data_bytes={}", payload.len());
                apply_to_running_pipeline(video_render, |pipeline| {
                    pipeline.push_frame(&payload, timestamp)
                });
            }
            VideoSetupAction::CodecConfigReceived { payload } => {
                println!("probe_state=video_media_codec_config_received");
                println!("video_media_codec_config_bytes={}", payload.len());
                apply_to_running_pipeline(video_render, |pipeline| {
                    pipeline.push_codec_config(&payload)
                });
            }
            VideoSetupAction::VideoFocusRequested { body_len } => {
                log_video_focus_requested(body_len);
            }
            VideoSetupAction::StopReceived => log_video_stop_received(),
        }
    }
    Ok(())
}

/// Runs `push` against the pipeline only if `video_render` is currently
/// `Running`; a no-op otherwise (e.g. no compositor was ever reachable this
/// run). A push error or a bus error observed right after demotes
/// `video_render` to `Failed` — never returns an error, since rendering is
/// independent of protocol correctness (see `VideoRenderState`'s doc
/// comment).
fn apply_to_running_pipeline(
    video_render: &mut VideoRenderState,
    push: impl FnOnce(&VideoRenderPipeline) -> Result<(), media_gstreamer::GstreamerError>,
) {
    let failure = match video_render {
        VideoRenderState::Running(pipeline) => match push(pipeline) {
            Err(error) => Some(("video_render_push_failed", error)),
            Ok(()) => pipeline
                .poll_bus_error()
                .map(|error| ("video_render_pipeline_error", error)),
        },
        _ => None,
    };
    if let Some((state, error)) = failure {
        println!("probe_state={state}");
        println!("video_render_error={error}");
        *video_render = VideoRenderState::Failed;
    }
}

/// Initializes `GStreamer` once at probe startup, independent of session
/// progress (the pipeline itself is still built lazily, on `Start` — see
/// `start_video_render_pipeline`). Failure here (e.g. `GStreamer` unusable
/// on this host at all) is logged and never retried this run.
fn new_video_render_state() -> VideoRenderState {
    match GstreamerBackend::new() {
        Ok(backend) => VideoRenderState::NotStarted(backend),
        Err(error) => {
            println!("probe_state=video_render_backend_unavailable");
            println!("video_render_error={error}");
            VideoRenderState::Unavailable
        }
    }
}

/// Logs `VideoSetupAction::VideoFocusRequested` — the phone asking for
/// video focus back while `Ready`, real-hardware-confirmed to arrive
/// shortly after `Start` (`docs/protocol/error-2-investigation.md`,
/// "`frame_rate`/`density` breakthrough"). The reply itself is a
/// `VideoSetupAction::SendMedia` handled by the match arm above this one;
/// this only logs receipt.
fn log_video_focus_requested(body_len: usize) {
    println!("probe_state=video_focus_requested");
    println!("video_focus_request_bytes={body_len}");
}

/// Logs `VideoSetupAction::StopReceived` — see `MediaMessageId::Stop`'s doc
/// comment; no reply is sent and the channel stays `Ready`.
fn log_video_stop_received() {
    println!("probe_state=video_channel_stop_received");
}

/// Handles `VideoSetupAction::Ready` (video `Start` accepted). Index 0 is
/// H.264, index 1 is H.265 (`ADVERTISED_H264_CONFIGURATION_INDEX`/
/// `ADVERTISED_H265_CONFIGURATION_INDEX`, `video_setup.rs`) — both now have
/// a real decode path (`crates/media-gstreamer`'s `pipeline_elements`
/// already selected the matching parser/decoder/caps per codec; only this
/// probe's own pipeline construction was hardcoded to H.264 before).
fn handle_video_start_received(
    video_render: &mut VideoRenderState,
    session_id: i32,
    configuration_index: u32,
    target: &VideoRenderTarget,
) {
    println!("probe_state=video_channel_start_received");
    println!("video_channel_session_id={session_id}");
    println!("video_channel_configuration_index={configuration_index}");
    let codec = match configuration_index {
        0 => DecoderVideoCodec::H264,
        _ => DecoderVideoCodec::Hevc,
    };
    start_video_render_pipeline(video_render, codec, target);
}

/// Lazily builds and starts the real decode/render pipeline once (on the
/// first `Start`, for whichever codec the phone actually selected — see the
/// caller in `handle_video_channel_message`) for the advertised 1280x720
/// configuration. Construction/start failure (most commonly: no reachable
/// Wayland compositor) demotes `video_render` to `Failed` and is logged; it
/// never returns an error or aborts the probe, since rendering is
/// independent of protocol correctness (see `VideoRenderState`'s doc
/// comment).
fn start_video_render_pipeline(
    video_render: &mut VideoRenderState,
    codec: DecoderVideoCodec,
    target: &VideoRenderTarget,
) {
    if !matches!(video_render, VideoRenderState::NotStarted(_)) {
        return;
    }
    let VideoRenderState::NotStarted(backend) =
        std::mem::replace(video_render, VideoRenderState::Failed)
    else {
        unreachable!("just matched NotStarted above");
    };
    let id = match codec {
        DecoderVideoCodec::H264 => "gstreamer:avdec_h264",
        DecoderVideoCodec::Hevc => "gstreamer:avdec_h265",
    };
    let capability = DecoderCapability {
        id: id.into(),
        codec,
        kind: DecoderKind::Software,
        // Descriptive only — the pipeline's own caps carry no explicit
        // width/height (see `render.rs`'s module doc comment), so these
        // fields aren't functionally enforced. Matches the resolution
        // actually advertised in `build_service_capabilities`.
        maximum_width: 1280,
        maximum_height: 720,
        maximum_frames_per_second: 60,
    };
    match target {
        VideoRenderTarget::Wayland => {
            match backend.build_video_render_pipeline(&capability, RenderSink::Wayland) {
                Ok(pipeline) => match pipeline.start() {
                    Ok(()) => {
                        println!("probe_state=video_render_pipeline_started");
                        *video_render = VideoRenderState::Running(pipeline);
                    }
                    Err(error) => {
                        println!("probe_state=video_render_pipeline_start_failed");
                        println!("video_render_error={error}");
                        *video_render = VideoRenderState::Failed;
                    }
                },
                Err(error) => {
                    println!("probe_state=video_render_pipeline_build_failed");
                    println!("video_render_error={error}");
                    *video_render = VideoRenderState::Failed;
                }
            }
        }
        VideoRenderTarget::Gtk4Window(handoff, _) => match handoff.request(capability) {
            Ok(pipeline) => {
                println!("probe_state=video_render_pipeline_started");
                *video_render = VideoRenderState::Running(pipeline);
            }
            Err(error) => {
                println!("probe_state=video_render_pipeline_start_failed");
                println!("video_render_error={error}");
                *video_render = VideoRenderState::Failed;
            }
        },
    }
}

/// Initializes `GStreamer` once at probe startup for one audio channel's
/// playback pipeline, independent of session progress (the pipeline itself
/// is still built lazily, on that channel's own `Start` — see
/// `start_audio_playback_pipeline`). Mirrors `new_video_render_state`; each
/// audio channel gets its own `GstreamerBackend` (cheap — `gst::init()` is
/// idempotent) so one channel's failure is independently diagnosable from
/// the others.
fn new_audio_playback_state(label: &str) -> AudioPlaybackState {
    match GstreamerBackend::new() {
        Ok(backend) => AudioPlaybackState::NotStarted(backend),
        Err(error) => {
            println!("probe_state={label}_playback_backend_unavailable");
            println!("{label}_playback_error={error}");
            AudioPlaybackState::Unavailable
        }
    }
}

/// Logs `AudioSetupAction::Ready` for any of the three audio channels —
/// shared since the print shape is identical across `MediaAudio`/
/// `SystemAudio`/`SpeechAudio`, only the `label` prefix differs.
fn log_audio_channel_start_received(label: &str, session_id: i32, configuration_index: u32) {
    println!("probe_state={label}_channel_start_received");
    println!("{label}_channel_session_id={session_id}");
    println!("{label}_channel_configuration_index={configuration_index}");
}

/// Logs `AudioSetupAction::MediaDataReceived` for any of the three audio
/// channels — shared for the same reason as `log_audio_channel_start_received`.
fn log_audio_media_data_received(label: &str, timestamp: u64, byte_len: usize) {
    println!("probe_state={label}_media_data_received");
    println!("{label}_media_data_timestamp={timestamp}");
    println!("{label}_media_data_bytes={byte_len}");
}

/// Logs `AudioSetupAction::StopReceived` for any of the three audio
/// channels — see `MediaMessageId::Stop`'s doc comment; no reply is sent
/// and the channel stays `Ready`.
fn log_audio_stop_received(label: &str) {
    println!("probe_state={label}_channel_stop_received");
}

/// Lazily builds and starts one audio channel's real PCM playback pipeline
/// on that channel's own `Start`. Mirrors `start_video_render_pipeline`;
/// construction/start failure (most commonly: no reachable
/// PipeWire/PulseAudio session, e.g. an unprivileged `sudo` session without
/// `-E`) demotes `audio_playback` to `Failed` and is logged — never returns
/// an error or aborts the probe, since playback is independent of protocol
/// correctness (see `AudioPlaybackState`'s doc comment).
fn start_audio_playback_pipeline(
    audio_playback: &mut AudioPlaybackState,
    format: AudioFormat,
    label: &str,
) {
    if !matches!(audio_playback, AudioPlaybackState::NotStarted(_)) {
        return;
    }
    let AudioPlaybackState::NotStarted(backend) =
        std::mem::replace(audio_playback, AudioPlaybackState::Failed)
    else {
        unreachable!("just matched NotStarted above");
    };
    match backend.build_audio_playback_pipeline(format, AudioSink::Pulse) {
        Ok(pipeline) => match pipeline.start() {
            Ok(()) => {
                println!("probe_state={label}_playback_pipeline_started");
                *audio_playback = AudioPlaybackState::Running(RunningAudioPipeline {
                    pipeline,
                    started_at: std::time::Instant::now(),
                    first_frame_latency_logged: false,
                });
            }
            Err(error) => {
                println!("probe_state={label}_playback_pipeline_start_failed");
                println!("{label}_playback_error={error}");
                *audio_playback = AudioPlaybackState::Failed;
            }
        },
        Err(error) => {
            println!("probe_state={label}_playback_pipeline_build_failed");
            println!("{label}_playback_error={error}");
            *audio_playback = AudioPlaybackState::Failed;
        }
    }
}

/// Pushes into an already-`Running` pipeline and, on the first successful
/// push only, logs this channel's start latency (see
/// `RunningAudioPipeline`'s doc comment). Returns the same
/// `Some((state, error))`/`None` shape `apply_to_running_audio_pipeline`
/// reports upward.
fn push_and_check_running_audio_pipeline(
    running: &mut RunningAudioPipeline,
    label: &str,
    push: impl FnOnce(&AudioPlaybackPipeline) -> Result<(), media_gstreamer::GstreamerError>,
) -> Option<(&'static str, media_gstreamer::GstreamerError)> {
    if let Err(error) = push(&running.pipeline) {
        return Some(("playback_push_failed", error));
    }
    if !running.first_frame_latency_logged {
        running.first_frame_latency_logged = true;
        println!(
            "probe_metric={label}_audio_start_latency_ms={}",
            running.started_at.elapsed().as_millis()
        );
    }
    running
        .pipeline
        .poll_bus_error()
        .map(|error| ("playback_pipeline_error", error))
}

/// Runs `push` against the pipeline only if `audio_playback` is currently
/// `Running`; a no-op otherwise. Mirrors `apply_to_running_pipeline`; a
/// push error or a bus error observed right after demotes `audio_playback`
/// to `Failed` — never returns an error, since playback is independent of
/// protocol correctness (see `AudioPlaybackState`'s doc comment).
fn apply_to_running_audio_pipeline(
    audio_playback: &mut AudioPlaybackState,
    label: &str,
    push: impl FnOnce(&AudioPlaybackPipeline) -> Result<(), media_gstreamer::GstreamerError>,
) {
    let failure = match audio_playback {
        AudioPlaybackState::Running(running) => {
            push_and_check_running_audio_pipeline(running, label, push)
        }
        _ => None,
    };
    if let Some((state, error)) = failure {
        println!("probe_state={label}_{state}");
        println!("{label}_playback_error={error}");
        *audio_playback = AudioPlaybackState::Failed;
    }
}

/// Drives the `MediaAudio` channel's `ChannelOpenStateMachine` then
/// `AudioSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data. Mirrors `handle_video_channel_message`
/// exactly — same message shape, different channel id and accepted codec
/// (see `protocol_aap::audio_setup`).
fn handle_media_audio_channel_message<T: SessionTransport>(
    message: &Message,
    media_audio_channel: &mut Option<MediaAudioChannel>,
    audio_playback: &mut AudioPlaybackState,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = media_audio_channel.as_mut().ok_or_else(|| {
        CliError::Protocol(
            "media-audio channel message before ServiceDiscoveryResponse was sent".into(),
        )
    })?;
    match state {
        MediaAudioChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on media-audio channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    MEDIA_AUDIO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=media_audio_channel_open");
            *state = MediaAudioChannel::Open(AudioSetupStateMachine::new());
            Ok(())
        }
        MediaAudioChannel::Open(machine) => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "expected a media-audio-channel media message".into(),
                ));
            }
            let actions = machine
                .advance(AudioSetupEvent::InboundMedia(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                match action {
                    AudioSetupAction::SendMedia(response) => {
                        let payload = response
                            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        let response_id = response.id;
                        send_encrypted(
                            transport,
                            tls,
                            MEDIA_AUDIO_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        match response_id {
                            MediaMessageId::Ack => {
                                println!("probe_state=media_audio_channel_ack_sent");
                            }
                            _ => {
                                println!("probe_state=media_audio_channel_setup_config_sent");
                            }
                        }
                    }
                    AudioSetupAction::Ready {
                        session_id,
                        configuration_index,
                    } => {
                        log_audio_channel_start_received(
                            "media_audio",
                            session_id,
                            configuration_index,
                        );
                        start_audio_playback_pipeline(
                            audio_playback,
                            MEDIA_AUDIO_PLAYBACK_FORMAT,
                            "media_audio",
                        );
                    }
                    AudioSetupAction::MediaDataReceived { timestamp, payload } => {
                        log_audio_media_data_received("media_audio", timestamp, payload.len());
                        apply_to_running_audio_pipeline(
                            audio_playback,
                            "media_audio",
                            |pipeline| pipeline.push_frame(&payload, timestamp),
                        );
                    }
                    AudioSetupAction::CodecConfigReceived { payload } => {
                        println!("probe_state=media_audio_media_codec_config_received");
                        println!("media_audio_media_codec_config_bytes={}", payload.len());
                    }
                    AudioSetupAction::StopReceived => log_audio_stop_received("media_audio"),
                }
            }
            Ok(())
        }
    }
}

/// Drives the `SystemAudio` channel's `ChannelOpenStateMachine` then
/// `AudioSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data. Mirrors
/// `handle_media_audio_channel_message` exactly — same underlying
/// `AudioSetupStateMachine` (this project advertises a single uncompressed
/// PCM `AudioConfiguration` for `SystemAudio` too, so the same accepted
/// codec applies), just a different channel id.
fn handle_system_audio_channel_message<T: SessionTransport>(
    message: &Message,
    system_audio_channel: &mut Option<SystemAudioChannel>,
    audio_playback: &mut AudioPlaybackState,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = system_audio_channel.as_mut().ok_or_else(|| {
        CliError::Protocol(
            "system-audio channel message before ServiceDiscoveryResponse was sent".into(),
        )
    })?;
    match state {
        SystemAudioChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on system-audio channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    SYSTEM_AUDIO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=system_audio_channel_open");
            *state = SystemAudioChannel::Open(AudioSetupStateMachine::new());
            Ok(())
        }
        SystemAudioChannel::Open(machine) => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "expected a system-audio-channel media message".into(),
                ));
            }
            let actions = machine
                .advance(AudioSetupEvent::InboundMedia(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                match action {
                    AudioSetupAction::SendMedia(response) => {
                        let payload = response
                            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        let response_id = response.id;
                        send_encrypted(
                            transport,
                            tls,
                            SYSTEM_AUDIO_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        match response_id {
                            MediaMessageId::Ack => {
                                println!("probe_state=system_audio_channel_ack_sent");
                            }
                            _ => {
                                println!("probe_state=system_audio_channel_setup_config_sent");
                            }
                        }
                    }
                    AudioSetupAction::Ready {
                        session_id,
                        configuration_index,
                    } => {
                        log_audio_channel_start_received(
                            "system_audio",
                            session_id,
                            configuration_index,
                        );
                        start_audio_playback_pipeline(
                            audio_playback,
                            VOICE_AUDIO_PLAYBACK_FORMAT,
                            "system_audio",
                        );
                    }
                    AudioSetupAction::MediaDataReceived { timestamp, payload } => {
                        log_audio_media_data_received("system_audio", timestamp, payload.len());
                        apply_to_running_audio_pipeline(
                            audio_playback,
                            "system_audio",
                            |pipeline| pipeline.push_frame(&payload, timestamp),
                        );
                    }
                    AudioSetupAction::CodecConfigReceived { payload } => {
                        println!("probe_state=system_audio_media_codec_config_received");
                        println!("system_audio_media_codec_config_bytes={}", payload.len());
                    }
                    AudioSetupAction::StopReceived => log_audio_stop_received("system_audio"),
                }
            }
            Ok(())
        }
    }
}

/// Drives the `SpeechAudio` channel's `ChannelOpenStateMachine` then
/// `AudioSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data. Mirrors
/// `handle_system_audio_channel_message`/`handle_media_audio_channel_message`
/// exactly — same underlying `AudioSetupStateMachine` (this project
/// advertises a single uncompressed PCM `AudioConfiguration` for
/// `SpeechAudio` too, so the same accepted codec applies), just a different
/// channel id.
fn handle_speech_audio_channel_message<T: SessionTransport>(
    message: &Message,
    speech_audio_channel: &mut Option<SpeechAudioChannel>,
    audio_playback: &mut AudioPlaybackState,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = speech_audio_channel.as_mut().ok_or_else(|| {
        CliError::Protocol(
            "speech-audio channel message before ServiceDiscoveryResponse was sent".into(),
        )
    })?;
    match state {
        SpeechAudioChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on speech-audio channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    SPEECH_AUDIO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=speech_audio_channel_open");
            *state = SpeechAudioChannel::Open(AudioSetupStateMachine::new());
            Ok(())
        }
        SpeechAudioChannel::Open(machine) => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "expected a speech-audio-channel media message".into(),
                ));
            }
            let actions = machine
                .advance(AudioSetupEvent::InboundMedia(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                match action {
                    AudioSetupAction::SendMedia(response) => {
                        let payload = response
                            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        let response_id = response.id;
                        send_encrypted(
                            transport,
                            tls,
                            SPEECH_AUDIO_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        match response_id {
                            MediaMessageId::Ack => {
                                println!("probe_state=speech_audio_channel_ack_sent");
                            }
                            _ => {
                                println!("probe_state=speech_audio_channel_setup_config_sent");
                            }
                        }
                    }
                    AudioSetupAction::Ready {
                        session_id,
                        configuration_index,
                    } => {
                        log_audio_channel_start_received(
                            "speech_audio",
                            session_id,
                            configuration_index,
                        );
                        start_audio_playback_pipeline(
                            audio_playback,
                            VOICE_AUDIO_PLAYBACK_FORMAT,
                            "speech_audio",
                        );
                    }
                    AudioSetupAction::MediaDataReceived { timestamp, payload } => {
                        log_audio_media_data_received("speech_audio", timestamp, payload.len());
                        apply_to_running_audio_pipeline(
                            audio_playback,
                            "speech_audio",
                            |pipeline| pipeline.push_frame(&payload, timestamp),
                        );
                    }
                    AudioSetupAction::CodecConfigReceived { payload } => {
                        println!("probe_state=speech_audio_media_codec_config_received");
                        println!("speech_audio_media_codec_config_bytes={}", payload.len());
                    }
                    AudioSetupAction::StopReceived => log_audio_stop_received("speech_audio"),
                }
            }
            Ok(())
        }
    }
}

/// Drives the `Sensors` channel's `ChannelOpenStateMachine`, then handles
/// `SensorRequest`/responds `SensorResponse`+`SensorBatch` directly (no
/// further state transition — a second `SensorRequest` for the other
/// advertised sensor type is expected and handled the same way). Matches
/// `OpenAuto`'s `SensorService::onSensorStartRequest`: always responds `OK`
/// regardless of `sensor_type`, and only sends a follow-up `SensorBatch` for
/// the two types this project actually advertises/models
/// (`DrivingStatusData`, `NightMode`) — any other/`Unknown` type gets only
/// the response, matching `OpenAuto`'s own no-op-for-unhandled-type
/// behavior rather than introducing a new rejection path.
fn handle_sensors_channel_message<T: SessionTransport>(
    message: &Message,
    sensors_channel: &mut Option<SensorsChannel>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = sensors_channel.as_mut().ok_or_else(|| {
        CliError::Protocol(
            "sensors channel message before ServiceDiscoveryResponse was sent".into(),
        )
    })?;
    match state {
        SensorsChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on sensors channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    SENSORS_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=sensors_channel_open");
            *state = SensorsChannel::Open;
            Ok(())
        }
        SensorsChannel::Open => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "unexpected message type on sensors channel after open".into(),
                ));
            }
            let sensor_message =
                SensorMessage::decode(&message.payload, DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
            match sensor_message.id {
                SensorMessageId::SensorRequest => {
                    let sensor_type = decode_sensor_request(&sensor_message.body)
                        .map_err(|error| CliError::Protocol(error.to_string()))?;
                    println!("probe_state=sensor_request_received");
                    println!("sensor_request_type={sensor_type:?}");
                    let response = encode_sensor_response();
                    let payload = response
                        .encode(DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE)
                        .map_err(|error| CliError::Protocol(error.to_string()))?;
                    send_encrypted(
                        transport,
                        tls,
                        SENSORS_CHANNEL_ID,
                        MessageType::Specific,
                        &payload,
                        limits,
                    )?;
                    println!("probe_state=sensor_response_sent");
                    let batch = match sensor_type {
                        SensorType::DrivingStatusData => {
                            Some(encode_driving_status_unrestricted_batch())
                        }
                        SensorType::NightMode => Some(encode_night_mode_batch(false)),
                        SensorType::Unknown(_) => None,
                    };
                    if let Some(batch) = batch {
                        let payload = batch
                            .encode(DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        send_encrypted(
                            transport,
                            tls,
                            SENSORS_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        println!("probe_state=sensor_batch_sent");
                    }
                    Ok(())
                }
                other => Err(CliError::Protocol(format!(
                    "unexpected sensor message {other:?} after open"
                ))),
            }
        }
    }
}

/// Drives one "advertise → open → nothing further" channel's
/// `ChannelOpenStateMachine` — now covers only `Input` (before it opens),
/// `Bluetooth`, and `Microphone`; every other advertised channel
/// (`Video`/`MediaAudio`/`SystemAudio`/`SpeechAudio`/`Sensors`) has
/// graduated to its own dedicated state machine above as this project
/// learned what each one needs beyond `ChannelOpenState::Open`.
fn handle_simple_channel_message<T: SessionTransport>(
    channel_id: u8,
    message: &Message,
    simple_channels: &mut HashMap<u8, ChannelOpenStateMachine>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let machine = simple_channels.get_mut(&channel_id).ok_or_else(|| {
        CliError::Protocol(format!("message on unadvertised channel {channel_id}"))
    })?;
    if machine.state() != ChannelOpenState::AwaitingOpenRequest {
        return Err(CliError::Protocol(format!(
            "unexpected message on channel {channel_id} after open"
        )));
    }
    if message.message_type != MessageType::Control {
        return Err(CliError::Protocol(format!(
            "expected ChannelOpenRequest on channel {channel_id}"
        )));
    }
    let actions = machine
        .advance(ChannelOpenEvent::InboundControl(&message.payload))
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    for action in actions {
        let ChannelOpenAction::SendControl(response) = action;
        let payload = response
            .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
            .map_err(|error| CliError::Protocol(error.to_string()))?;
        send_encrypted(
            transport,
            tls,
            channel_id,
            MessageType::Control,
            &payload,
            limits,
        )?;
    }
    println!("probe_state=simple_channel_open channel_id={channel_id}");
    Ok(())
}

/// Handles Input-channel traffic that arrives once the channel has already
/// reached `ChannelOpenState::Open`. Only `KeyBindingRequest` is handled;
/// anything else fails closed with a clear, distinct error naming the
/// unexpected message, matching `handle_post_discovery_control_message`'s
/// posture.
fn handle_input_channel_message<T: SessionTransport>(
    message: &Message,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    if message.message_type != MessageType::Specific {
        return Err(CliError::Protocol(
            "unexpected message type on input channel after open".into(),
        ));
    }
    let input_message = InputMessage::decode(&message.payload, DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    match input_message.id {
        InputMessageId::KeyBindingRequest => {
            let keycodes = decode_key_binding_request(&input_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=key_binding_requested");
            println!("key_binding_requested_count={}", keycodes.len());
            let status = evaluate_key_binding_request();
            let response = encode_key_binding_response(status);
            let payload = response
                .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(
                transport,
                tls,
                INPUT_CHANNEL_ID,
                MessageType::Specific,
                &payload,
                limits,
            )?;
            println!("probe_state=key_binding_response_sent");
            println!("key_binding_response_status={status:?}");
            Ok(())
        }
        other => Err(CliError::Protocol(format!(
            "unexpected input message {other:?} after open"
        ))),
    }
}

/// Handles traffic on the Bluetooth channel after it opens. Until a real
/// wireless-bootstrap trial (first successful run, real hardware) this
/// channel had never been observed carrying anything beyond
/// `ChannelOpenRequest` — the earlier research pass in
/// `docs/protocol/wireless-source-assessment.md` concluded (from reading
/// only `.proto`/README sources, not real traffic) that it was likely only
/// used by an already-connected session bootstrapping a *future* reconnect,
/// not something a phone would use mid-session. That assumption was wrong:
/// a real phone sent a genuine `BluetoothPairingRequest` shortly after
/// video frames started flowing. This probe has no real classic-Bluetooth
/// audio pairing implemented, so it declines gracefully — see
/// `encode_bluetooth_pairing_response`'s doc comment — rather than
/// attempting or faking a pairing exchange.
fn handle_bluetooth_channel_message<T: SessionTransport>(
    message: &Message,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    if message.message_type != MessageType::Specific {
        return Err(CliError::Protocol(
            "unexpected message type on bluetooth channel after open".into(),
        ));
    }
    let bluetooth_message =
        BluetoothServiceMessage::decode(&message.payload, DEFAULT_MAX_BLUETOOTH_MESSAGE_BODY_SIZE)
            .map_err(|error| CliError::Protocol(error.to_string()))?;
    match bluetooth_message.id {
        BluetoothMessageId::PairingRequest => {
            let pairing_method = decode_bluetooth_pairing_request(&bluetooth_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=bluetooth_pairing_requested");
            println!("bluetooth_pairing_requested_method={pairing_method:?}");
            let response = encode_bluetooth_pairing_response();
            let payload = response
                .encode(DEFAULT_MAX_BLUETOOTH_MESSAGE_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(
                transport,
                tls,
                BLUETOOTH_CHANNEL_ID,
                MessageType::Specific,
                &payload,
                limits,
            )?;
            println!("probe_state=bluetooth_pairing_response_sent");
            Ok(())
        }
        other => Err(CliError::Protocol(format!(
            "unexpected bluetooth message {other:?} after open"
        ))),
    }
}

/// Always answers `Success`, matching `f-io/LIVI`'s own `KeyBindingResponse`
/// handling (`Session.ts`: unconditional `[0x08,0x00]`/`STATUS_OK`,
/// regardless of which keycodes were requested). This project previously
/// validated the request against its own advertised (empty) keycode
/// capability list, replying `KeycodeNotBound` for any non-empty request —
/// matching `OpenAuto`'s older, stricter `InputService::onBindingRequest`
/// behavior. Every real request observed so far has been empty, so this
/// hadn't mattered in practice, but LIVI (a confirmed-working modern
/// client) never does this validation, so the stricter behavior is now a
/// known-wrong deviation, corrected here.
const fn evaluate_key_binding_request() -> KeyBindingStatus {
    KeyBindingStatus::Success
}

/// Encrypts `plaintext_payload` and sends it framed on `channel_id`.
/// `ServiceDiscoveryResponse`, `ChannelOpenResponse`, and `Config` are all
/// sent this way — verified directly against the pinned AASDK C++ source,
/// not assumed (see the module doc comment).
fn send_encrypted<T: SessionTransport>(
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    channel_id: u8,
    message_type: MessageType,
    plaintext_payload: &[u8],
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let ciphertext = tls
        .encrypt_application_data(plaintext_payload)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let frame = encode_frame(
        FrameHeader {
            channel_id,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type,
        },
        None,
        &ciphertext,
        limits,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    transport.send_all(&frame).map_err(CliError::Transport)
}

/// Pushes one decoded wire frame into `assembler`, decrypting it first if
/// `Encrypted`. Encrypted frames arriving before TLS completes are a
/// protocol violation, since decryption isn't yet possible.
fn push_decoded_frame(
    frame: DecodedFrame<'_>,
    assembler: &mut MessageAssembler,
    tls: &mut OpenSslTlsClient,
    handshake_state: HandshakeState,
) -> Result<Option<Message>, CliError> {
    match frame.header.encryption {
        Encryption::Plain => assembler
            .push(frame)
            .map_err(|error| CliError::Protocol(error.to_string())),
        Encryption::Encrypted => {
            if !matches!(
                handshake_state,
                HandshakeState::AwaitingServiceDiscovery | HandshakeState::ServiceDiscoveryReceived
            ) {
                return Err(CliError::Protocol(
                    "encrypted frame received before TLS handshake completed".into(),
                ));
            }
            println!("probe_state=encrypted_frame_received");
            let plaintext = tls
                .decrypt_application_data(frame.payload)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            let decrypted_frame = DecodedFrame {
                header: frame.header,
                total_message_size: frame.total_message_size,
                payload: &plaintext,
                consumed: frame.consumed,
            };
            assembler
                .push(decrypted_frame)
                .map_err(|error| CliError::Protocol(error.to_string()))
        }
    }
}

/// Validates an assembled message's metadata, then advances the handshake
/// state machine with its (now-plaintext) payload.
fn handle_assembled_message<T: SessionTransport>(
    message: &Message,
    handshake: &mut HandshakeStateMachine,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<Option<ServiceDiscoveryRequestSummary>, CliError> {
    if message.channel_id != 0 || message.message_type != MessageType::Specific {
        println!("unexpected_message_channel_id={}", message.channel_id);
        println!("unexpected_message_encryption={:?}", message.encryption);
        println!("unexpected_message_type={:?}", message.message_type);
        println!("unexpected_message_payload_bytes={}", message.payload.len());
        return Err(CliError::Protocol(
            "unexpected message metadata during auth/service-discovery probe".into(),
        ));
    }

    let mut actions: VecDeque<_> = handshake
        .advance(HandshakeEvent::InboundControl(&message.payload))
        .map_err(|error| CliError::Protocol(error.to_string()))?
        .into();
    process_actions(&mut actions, handshake, tls, transport, limits)
}

fn print_summary(summary: &ServiceDiscoveryRequestSummary) {
    fn bytes(field: Option<usize>) -> String {
        field.map_or_else(|| "absent".to_string(), |size| size.to_string())
    }
    println!(
        "service_discovery_small_icon_bytes={}",
        bytes(summary.small_icon_bytes)
    );
    println!(
        "service_discovery_medium_icon_bytes={}",
        bytes(summary.medium_icon_bytes)
    );
    println!(
        "service_discovery_large_icon_bytes={}",
        bytes(summary.large_icon_bytes)
    );
    println!(
        "service_discovery_label_text_bytes={}",
        bytes(summary.label_text_bytes)
    );
    println!(
        "service_discovery_device_name_bytes={}",
        bytes(summary.device_name_bytes)
    );
    println!(
        "service_discovery_phone_info_bytes={}",
        bytes(summary.phone_info_bytes)
    );
    println!(
        "service_discovery_unknown_fields={}",
        summary.unknown_fields
    );
}

/// Drains queued handshake actions, sending control messages and driving TLS
/// as needed. Unlike `live_probe`'s equivalent, TLS completion is fed back
/// into `HandshakeStateMachine::advance` rather than short-circuited, so
/// `AuthComplete` is sent and the machine can reach `ServiceDiscoveryRequest`.
/// Returns the bounded summary the instant one is produced; nothing further
/// is read or sent afterward.
fn process_actions<T: SessionTransport>(
    actions: &mut VecDeque<HandshakeAction>,
    handshake: &mut HandshakeStateMachine,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<Option<ServiceDiscoveryRequestSummary>, CliError> {
    while let Some(action) = actions.pop_front() {
        match action {
            HandshakeAction::SendControl(message) => {
                send_control(transport, &message, limits)?;
            }
            HandshakeAction::StartTlsClient => {
                println!("probe_state=version_accepted");
                if let Some(version) = handshake.negotiated_version() {
                    println!(
                        "probe_negotiated_version={}.{}",
                        version.major, version.minor
                    );
                }
                let progress = tls
                    .start()
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                queue_tls_progress(&progress, actions, handshake)?;
            }
            HandshakeAction::FeedTls(inbound) => {
                println!("probe_state=tls_peer_data_received");
                let progress = tls
                    .feed(&inbound)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                queue_tls_progress(&progress, actions, handshake)?;
            }
            HandshakeAction::ServiceDiscoveryRequest(summary) => {
                return Ok(Some(summary));
            }
        }
    }
    Ok(None)
}

fn queue_tls_progress(
    progress: &TlsProgress,
    actions: &mut VecDeque<HandshakeAction>,
    handshake: &mut HandshakeStateMachine,
) -> Result<(), CliError> {
    if progress.complete {
        println!("probe_state=tls_handshake_complete");
    }
    actions.extend(
        handshake
            .advance(HandshakeEvent::TlsProgress {
                outbound: &progress.outbound,
                complete: progress.complete,
            })
            .map_err(|error| CliError::Protocol(error.to_string()))?,
    );
    Ok(())
}

fn send_control<T: SessionTransport>(
    transport: &mut T,
    message: &ControlMessage,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let payload = message
        .encode(protocol_aap::DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let frame = encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Plain,
            message_type: MessageType::Specific,
        },
        None,
        &payload,
        limits,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    transport.send_all(&frame).map_err(CliError::Transport)
}
