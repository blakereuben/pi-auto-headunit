# Real-phone investigation: Android Auto "Error 2" after `ServiceDiscoveryResponse`

## Status: open, unresolved, but the failure boundary keeps moving further into the session with each increment — and has now changed character. Three early hypotheses tested and refuted; advertising the full 8-service canonical set, then handling `AudioFocusRequest`, then `KeyBindingRequest` on the Input channel, then `Setup`/`Config`/`Start` on all three `AudioMediaSinkService`-derived channels (`MediaAudio`/`SystemAudio`/`SpeechAudio`), each produced real forward progress — the phone now opens every channel, drives video and all three audio channels through `Setup`→`Config`, and completes a full key-binding round trip. With `SpeechAudio` handled, the probe no longer hits an explicit "unexpected message" rejection at all: it now runs every implemented handshake step successfully and then **times out** waiting for anything further from the phone. Error 2 still appears on screen either way (confirmed on three separate real-phone runs). A fourth hypothesis — that the phone just needed more time before sending whatever comes next, so the 10s `PROBE_TIMEOUT` was the limiting factor — was tested and refuted: raising it to 30s produced no behavioral difference.

## Current symptom (most recent real-phone runs)

With `SpeechAudio`'s `Setup`/`Config`/`Start` handling added, two consecutive real-phone runs (fresh MTP replug before each) no longer hit an explicit "unexpected message" rejection — every implemented handshake step now completes successfully, but the phone stops sending anything further and the probe eventually times out. The two runs varied in how much they completed before the phone went quiet, which is itself worth recording:

**Run 1** (`usb auth-discovery-probe --device <bus:address> --allow-live-aap`):

```
probe_negotiated_version=1.7
probe_state=tls_handshake_complete
probe_result=service_discovery_summary_received
probe_state=service_discovery_response_sent
probe_state=video_channel_open
probe_state=simple_channel_open channel_id=2   # Input
probe_state=audio_focus_requested
audio_focus_request_type=Release
probe_state=audio_focus_notification_sent
probe_state=simple_channel_open channel_id=6   # Sensors
probe_state=simple_channel_open channel_id=7   # Bluetooth
probe_state=simple_channel_open channel_id=8   # Microphone
probe_state=media_audio_channel_open
probe_state=system_audio_channel_open
probe_state=speech_audio_channel_open
probe_state=video_channel_setup_config_sent
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

Here, all channels opened and video reached `Config`, but none of the three audio channels' `Setup` ever arrived before the timeout — the phone simply stopped sending anything after the key-binding exchange.

**Run 2** (immediately after a fresh replug, same command):

```
...(identical up through all channels opening)...
probe_state=media_audio_channel_setup_config_sent
probe_state=system_audio_channel_setup_config_sent
probe_state=speech_audio_channel_setup_config_sent
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_state=encrypted_frame_received
probe_state=video_channel_setup_config_sent
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

Here, all three audio channels' `Setup` **did** arrive and were answered with `Config` — each accepted without complaint, using the same shared `AudioSetupStateMachine` unmodified for all three — and video's `Config` was also sent. This is the furthest the probe has ever gotten: every currently-implemented handshake step completed successfully. It still timed out waiting for whatever comes next (most plausibly a `Start` on one of the audio channels, or something on `Sensors`/`Bluetooth`/`Microphone`, none of which have any post-open handling yet).

**Run 3** (diagnostic: `PROBE_TIMEOUT` temporarily raised from 10s to 30s, fresh replug, same command): reached the same point as Run 2 — all four `Config` responses sent (video, `MediaAudio`, `SystemAudio`, `SpeechAudio`) plus the key-binding exchange — and still timed out waiting for anything further, despite three times as long to wait. `PROBE_TIMEOUT` was reverted back to 10s immediately afterward, since the longer value produced no behavioral difference (see "Hypotheses tested against real hardware and refuted" below).

**Confirmed on the phone screen for all three runs: still Error 2** ("Communication error 2 — the phone and the car are running incompatible software"). The run-to-run variation in exactly which messages arrive before the phone goes quiet (not a fixed, reproducible message sequence) is itself a new data point — see "What this rules out, and what it doesn't" below.

## What changed and why it mattered

The prior three hypotheses (below) each produced **zero change** in behavior — the phone rejected `ServiceDiscoveryResponse` identically every time, which was itself a strong hint that the problem was structural rather than a single missing field. Externally-sourced research (AASDK/OpenAuto/LIVI all construct a complete, fixed 8-service `ServiceDiscoveryResponse` unconditionally, not a curated subset) was checked directly against this project's already-approved `OpenAuto` primary source (`ServiceFactory.cpp`) and confirmed: `ServiceFactory::create()` unconditionally constructs 7 of 8 canonical services, with the 8th config-gated but on by default.

Implementing the full canonical set (`Video`, `Input`, `MediaAudio`, `SystemAudio`, `SpeechAudio`, `Sensors`, `Bluetooth`, `Microphone` — `crates/protocol-aap/src/service_discovery_response.rs`, generic `ChannelOpenStateMachine` reused per channel in `auth_discovery_probe.rs`) was the first change in this entire investigation that altered real-phone behavior: the phone stopped rejecting `ServiceDiscoveryResponse` and instead proceeded into the session, first sending a previously-unseen encrypted control message (`AudioFocusRequest`, control ID 18). Implementing and answering that (`crates/protocol-aap/src/audio_focus.rs`, `AudioFocusNotification` granting exactly what's asked — see that file's doc comments for full wire provenance against the pinned AASDK source) unblocked the phone further still: it proceeded to open all 8 channels and drive the video channel through `Setup`→`Config`, then send `KeyBindingRequest` on the Input channel.

Implementing and answering that too (`crates/protocol-aap/src/input_message.rs`, `KeyBindingRequest`/`KeyBindingResponse` — see that file's doc comments for full wire provenance; policy in `handle_input_channel_message`/`evaluate_key_binding_request` in `auth_discovery_probe.rs`, mirroring `OpenAuto`'s `InputService::onBindingRequest` validation against the head unit's own advertised keycode list) unblocked the phone yet again: the empty request was answered `Success` and accepted, and the session proceeded to a new message — `Setup` — on the `MediaAudio` channel.

Implementing that too (`crates/protocol-aap/src/audio_setup.rs`, a deliberate duplicate of `video_setup.rs` rather than a shared generic state machine, to avoid touching the real-hardware-proven video code path — see that file's doc comments for full wire provenance) unblocked the phone yet again: the `Setup` (requesting `MEDIA_CODEC_AUDIO_PCM`) was answered with `Config` and accepted, and the session proceeded to a new message on the `SystemAudio` channel.

`SystemAudio` also turned out to be a thin `AudioMediaSinkService` subclass in AASDK, advertising the same single-PCM-configuration shape this project already advertises for it — so no new crate-level code was needed at all; `AudioSetupStateMachine` was reused unmodified, with only a parallel `SystemAudioChannel` enum/`handle_system_audio_channel_message` dispatch added in `auth_discovery_probe.rs` (mirroring the `MediaAudio` wiring exactly, purely additive — the just-proven `MediaAudio` code was never touched). This unblocked the phone yet again: `SystemAudio`'s `Setup` was answered with `Config` and accepted, and the session proceeded to a new message on the `SpeechAudio` channel.

`SpeechAudio` (AASDK's `GuidanceAudioChannel`, matching `AudioStreamType::Guidance`) turned out to be the third and final thin `AudioMediaSinkService` subclass, advertising the same single-PCM-configuration shape again — so once more, no new crate-level code was needed; `AudioSetupStateMachine` was reused unmodified for a third channel, with only a third parallel `SpeechAudioChannel` enum/`handle_speech_audio_channel_message` dispatch added, purely additive. This is where the pattern changed: with all three audio channels' `Setup`/`Config`/`Start` handling in place, the probe no longer hits an explicit "unexpected message" rejection at all on a subsequent real-phone run — it runs every implemented handshake step to completion and then times out. Error 2 still appears on screen regardless (confirmed on two separate runs), but the failure signature is now "the phone goes quiet" rather than "the phone sends something new we reject."

## Original symptom (earlier in this investigation, now superseded)

Before the full-service-catalogue change, the probe reached only as far as:

```
probe_result=service_discovery_summary_received
probe_state=service_discovery_response_sent
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

with Error 2 on screen immediately after `ServiceDiscoveryResponse` was sent, and no `ChannelOpenRequest` ever arriving. The three hypotheses below were tested against *this* earlier boundary.

## Confirmed facts (from our own real-phone runs, not speculation)

- The phone negotiates and accepts **protocol version 1.7** (`HandshakeStateMachine::negotiated_version()`, surfaced via `probe_negotiated_version=<major>.<minor>` in `auth_discovery_probe.rs`). Version negotiation itself succeeds regardless of what we offer (tested offering both `1.6` and `1.7` — see below).
- **No publicly known AASDK fork documents or implements protocol 1.7.** Checked directly (`Version.hpp`'s `AASDK_MAJOR`/`AASDK_MINOR` constants): our pinned `opencardev/aasdk` @ `9bf6adf933665dee26532201719fac14a047ccf1` (`1.6`); `opencardev/aasdk`'s own `newdev` branch HEAD as of this investigation, pushed as recently as 2026-06-11 (still `1.6` — the most recently active fork checked, and it hasn't caught up either); `openDsh/aasdk` `develop` (`1.1`); `f1xpl/aasdk` (no `Version.hpp` found at the checked path); `n8ohu/aasdk`, `Spooky998/aasdk`, `arash-rasouli/aasdk` (all stale since 2018–2023, not independently re-checked for this constant beyond confirming they're inactive). This looks like a genuine, currently-undocumented-in-open-source gap between what current real Android Auto expects and what any community reverse-engineering effort has reached.

## Hypotheses tested against real hardware and refuted

Each was a deliberately minimal, independently reversible change, verified with the full `cargo fmt`/`check`/`clippy`/`test` sweep before each real-phone run. The first three still ship in the current code (see "What's still in place" below) since each represents a more complete, independently-defensible `ServiceDiscoveryResponse` regardless of whether it fixed Error 2; the version-number and probe-timeout experiments were both reverted since neither changed anything and there was no independent reason to keep them.

1. **Missing audio service.** Added a `ServiceKind::MediaAudio` channel (`protocol_aap::service_discovery_response::AudioCapability`/`AudioStreamType`, `MediaSinkService.audio_type`/`audio_configs`) alongside the existing video/touch channels. **No change in outcome.**
2. **Stale protocol version number.** Temporarily changed `AASDK_PROTOCOL_VERSION` from the pinned source's `1.6` to `1.7` (matching what the phone reports it wants), to test whether a bare version-number mismatch was the trigger. **No change in outcome** — reverted back to `1.6` (the pinned, documented value) once refuted, since Android Auto's version negotiation is designed to be backward-compatible and a real phone accepting-but-rejecting on version number alone was never a well-motivated theory to begin with; the experiment confirmed that reasoning rather than contradicting it.
3. **Missing head-unit identity.** Populated `ServiceDiscoveryResponse.headunit_info` (field 17, `HeadUnitInfo` — `protocol_aap::service_discovery_response::HeadUnitInfo`) with fixed, non-sensitive project-identifying strings (project name, `CARGO_PKG_VERSION`, etc. — never real vehicle/user data). Theory: Android Auto's app-level validation might reject a head unit that never identifies itself, distinct from the wire schema (which marks every `HeadUnitInfo` field `optional`). **No change in outcome.**
4. **Probe timeout too short.** Once every currently-implemented handshake step started succeeding (after the `SpeechAudio` increment), the probe began timing out instead of being rejected on a new message. Temporarily raised `PROBE_TIMEOUT` (`auth_discovery_probe.rs`) from 10s to 30s, to test whether the phone just needed more time before sending whatever comes next. **No change in outcome** — the same handshake steps completed and it still timed out waiting for more, just with three times as long to wait. Reverted back to 10s once refuted.

## What this rules out, and what it doesn't

Ruled out: a simple missing-channel gap; a simple version-number-field mismatch; a simple missing-identity gap; a too-short probe timeout (30s produced no change over 10s, so the phone isn't simply slow to continue — it appears to genuinely stop sending anything once it decides to fail).

**Not ruled out**, in roughly descending order of how well-motivated they are:

- **A genuine 1.7 schema change** in `Service`/`ServiceDiscoveryResponse`/`MediaSinkService`/etc. that this project has no source for (new required fields, restructured messages, or fields that must be *absent* now where they weren't before). This remains the leading theory by elimination, but there is no public reference to confirm or investigate it against — see "Research leads" below.
- **A missing control-message step** between `AuthComplete` and `ServiceDiscoveryRequest`, or between `ServiceDiscoveryRequest` and our response, that a real 1.7-speaking phone expects and the pinned 1.6-era AASDK source never modeled at all (not just a field-level gap, but a whole message this project doesn't send or handle).
- **A framing/encryption detail specific to a real phone's TLS stack** that the project's own real-TLS integration tests (`encrypted_service_discovery.rs`, `full_channel_setup.rs`) can't catch, since those exercise the same Rust encrypt/decrypt code on both sides of a controlled fake-phone harness rather than a genuinely independent TLS implementation.
- Something else entirely not yet considered.

Given three independent, content-varying experiments produced *identical* behavior, the consistency itself is a data point: this doesn't look like "almost right, one field off" — it looks structural.

**New observation, not yet well-motivated as a hypothesis:** the three most recent real-phone runs (all after every currently-implemented handshake step succeeds) did *not* reproduce the same message sequence — Run 1's phone stopped sending anything right after the key-binding exchange (no audio `Setup` ever arrived), while Runs 2 and 3 sent all three audio channels' `Setup` and video's `Config` before going quiet. All three ended in Error 2 regardless, and Run 3 confirms the phone isn't simply pausing before continuing (30s produced the same outcome as 10s). This run-to-run variability could mean Android Auto itself is retrying/backing off internally before declaring incompatibility (rather than following one fixed message sequence), or it could be ordinary session jitter unrelated to the eventual failure. Not enough data yet to tell which.

## Research leads already checked (see conversation history for exact queries/results)

- `opencardev/aasdk` PR #2 ("Fix compatibility with latest Android Auto", merged 2019-08-07) — already included in our pinned revision; fixed `VideoFocusRequest`/`touch_event.action_index` parsing issues, not anything we've hit.
- `opencardev/crankshaft` issue #5 ("Error 2 with several phones") — no technical root cause recorded, just "goes away with official OpenAuto on X11."
- `rsjudka/intelligent-auto` issue #40 ("Android Auto won't connect with latest AA") — reports the same class of failure across every AASDK-based implementation checked (crankshaft, intelligent-auto), while the one implementation that doesn't hit it (OpenAuto Pro) is the more complete, commercial one. No technical diagnosis in the issue thread itself.
- One additional source of uncertain provenance was consulted during troubleshooting, for understanding only, per explicit project-owner instruction not to name, cite, quote, or otherwise reflect it in this repository. It did not resolve the question it was consulted for and contributed nothing citable to this record.

## What's still in place in the code (not reverted)

- All 8 canonical services (`Video`, `Input`, `MediaAudio`, `SystemAudio`, `SpeechAudio`, `Sensors`, `Bluetooth`, `Microphone`) advertised in `ServiceDiscoveryResponse` (`crates/protocol-aap/src/service_discovery_response.rs`), each driven to `ChannelOpenState::Open` via `ChannelOpenStateMachine` in `auth_discovery_probe.rs`. `Video`, `MediaAudio`, `SystemAudio`, and `SpeechAudio` go further still (`Setup`→`Config`→`Start` via `VideoSetupStateMachine`/`AudioSetupStateMachine`), and `Input` handles one post-open message (`KeyBindingRequest`) — `Sensors`/`Bluetooth`/`Microphone` are the only channels left with no post-open handshake.
- `AudioFocusRequest`/`AudioFocusNotification` handling (`crates/protocol-aap/src/audio_focus.rs`, `handle_post_discovery_control_message`/`grant_audio_focus` in `auth_discovery_probe.rs`) — grants exactly what's requested (placeholder policy, no real audio-focus arbitration exists yet).
- `KeyBindingRequest`/`KeyBindingResponse` handling on the Input channel (`crates/protocol-aap/src/input_message.rs`, `handle_input_channel_message`/`evaluate_key_binding_request` in `auth_discovery_probe.rs`) — responds `Success` only for an empty request (matches the zero supported keycodes this project currently advertises), `KeycodeNotBound` otherwise. Real-hardware-confirmed: the phone's request was empty, the `Success` response was accepted, and the session proceeded past this boundary.
- `Setup`/`Config`/`Start` handling on the `MediaAudio`, `SystemAudio`, and `SpeechAudio` channels (`crates/protocol-aap/src/audio_setup.rs`'s single `AudioSetupStateMachine`, reused unmodified for all three; `handle_media_audio_channel_message`/`handle_system_audio_channel_message`/`handle_speech_audio_channel_message` in `auth_discovery_probe.rs`) — accepts only `MEDIA_CODEC_AUDIO_PCM`, matching the single uncompressed `AudioConfiguration` this project advertises for each. Real-hardware-confirmed on all three channels (across two runs — see "Current symptom" above): each `Setup` requested PCM, each `Config` response was accepted.
- `HeadUnitInfo` (`service_discovery_response.rs`), populated with fixed project-identifying strings in `auth_discovery_probe.rs`.
- `probe_negotiated_version=<major>.<minor>` diagnostic print in `process_actions` (`auth_discovery_probe.rs`) — cheap, permanent, useful visibility into what the phone actually negotiates.
- `AASDK_PROTOCOL_VERSION` is back to the pinned source's `1.6` (the `1.7` experiment was reverted).

## Suggested next steps for whoever picks this up

1. **Immediate next question:** with every currently-implemented handshake step now succeeding (all 8 channels open, video/`MediaAudio`/`SystemAudio`/`SpeechAudio` all through `Setup`→`Config`, `KeyBindingRequest` answered) and the phone still going quiet before Error 2 — confirmed not a timeout-length issue (see hypothesis 4 above) — the "one more unhandled message" pattern that drove every prior increment no longer applies directly — there's nothing left in the current log output to react to. Leading candidate: drive `Video`/`MediaAudio`/`SystemAudio`/`SpeechAudio` further past `Config` — none has ever reached `Start` on a real run yet, so the phone may be waiting for `Start` on one or more of them before it will send anything else (video did once reach `video_channel_start_received` in an earlier session before this increment's changes — re-check whether that's still true, or whether the newly-added audio channels are now competing for whatever the phone sends first). This would need each `AudioSetupStateMachine`/`VideoSetupStateMachine` instance's `Start` handling to actually be exercised in the probe rather than just implemented — confirm what, if anything, currently blocks that from happening on a real run.
2. Given the run-to-run variability newly observed (see "What this rules out, and what it doesn't"), don't over-index on a single run's exact message sequence — repeat any experiment at least twice before drawing conclusions, as this session did for the `SpeechAudio` real-hardware test.
3. Look for a newer/different open-source Android Auto receiver project (not necessarily AASDK-derived) that has demonstrably worked with a *current* phone, and diff its full post-service-discovery message sequence against ours.
4. Consider whether a packet capture against a **known-working** current head unit (if one becomes available) could reveal the complete expected message sequence, rather than continuing to discover it one real-phone run at a time.
5. Re-check whether any AASDK fork has been updated since this investigation (2026-08-12) — the ecosystem may catch up.
6. If continuing to experiment against real hardware, keep changes as minimal and independently reversible as every increment so far, and keep running the full verification sweep before each real-phone cycle.
