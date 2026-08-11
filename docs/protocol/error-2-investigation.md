# Real-phone investigation: Android Auto "Error 2" after `ServiceDiscoveryResponse`

## Status: open, unresolved. Three specific hypotheses tested and refuted.

## Symptom

Running `usb auth-discovery-probe --device <bus:address> --allow-live-aap` against a real phone reaches, reliably and repeatably:

```
probe_state=version_accepted
probe_negotiated_version=1.7
probe_state=tls_peer_data_received
probe_state=tls_peer_data_received
probe_state=tls_handshake_complete
probe_state=encrypted_frame_received
service_discovery_small_icon_bytes=152
...
probe_result=service_discovery_summary_received
probe_state=service_discovery_response_sent
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

The probe builds, TLS-encrypts, and sends `ServiceDiscoveryResponse` with **no local error** — encoding, encryption, and framing all succeed. The phone's screen shows **"Communication error 2 — the phone and the car are running incompatible software"** immediately after. No `ChannelOpenRequest` ever arrives on any channel within the 10s `PROBE_TIMEOUT`. The probe always fails closed cleanly (exit 19, no hang) and the USB interface always releases cleanly (`usb list` responsive immediately after).

This is a **new** failure boundary — every prior real-phone milestone (version negotiation, TLS handshake, `ServiceDiscoveryRequest` receipt/summary) is unaffected and still works exactly as documented elsewhere in this repo. The failure is specifically: real phone rejects our `ServiceDiscoveryResponse`.

## Confirmed facts (from our own real-phone runs, not speculation)

- The phone negotiates and accepts **protocol version 1.7** (`HandshakeStateMachine::negotiated_version()`, surfaced via `probe_negotiated_version=<major>.<minor>` in `auth_discovery_probe.rs`). Version negotiation itself succeeds regardless of what we offer (tested offering both `1.6` and `1.7` — see below).
- **No publicly known AASDK fork documents or implements protocol 1.7.** Checked directly (`Version.hpp`'s `AASDK_MAJOR`/`AASDK_MINOR` constants): our pinned `opencardev/aasdk` @ `9bf6adf933665dee26532201719fac14a047ccf1` (`1.6`); `opencardev/aasdk`'s own `newdev` branch HEAD as of this investigation, pushed as recently as 2026-06-11 (still `1.6` — the most recently active fork checked, and it hasn't caught up either); `openDsh/aasdk` `develop` (`1.1`); `f1xpl/aasdk` (no `Version.hpp` found at the checked path); `n8ohu/aasdk`, `Spooky998/aasdk`, `arash-rasouli/aasdk` (all stale since 2018–2023, not independently re-checked for this constant beyond confirming they're inactive). This looks like a genuine, currently-undocumented-in-open-source gap between what current real Android Auto expects and what any community reverse-engineering effort has reached.

## Hypotheses tested against real hardware and refuted

Each was a deliberately minimal, independently reversible change, verified with the full `cargo fmt`/`check`/`clippy`/`test` sweep before each real-phone run. All three still ship in the current code (see "What's still in place" below) since each represents a more complete, independently-defensible `ServiceDiscoveryResponse` regardless of whether it fixed Error 2 — none was reverted except the version-number experiment.

1. **Missing audio service.** Added a `ServiceKind::MediaAudio` channel (`protocol_aap::service_discovery_response::AudioCapability`/`AudioStreamType`, `MediaSinkService.audio_type`/`audio_configs`) alongside the existing video/touch channels. **No change in outcome.**
2. **Stale protocol version number.** Temporarily changed `AASDK_PROTOCOL_VERSION` from the pinned source's `1.6` to `1.7` (matching what the phone reports it wants), to test whether a bare version-number mismatch was the trigger. **No change in outcome** — reverted back to `1.6` (the pinned, documented value) once refuted, since Android Auto's version negotiation is designed to be backward-compatible and a real phone accepting-but-rejecting on version number alone was never a well-motivated theory to begin with; the experiment confirmed that reasoning rather than contradicting it.
3. **Missing head-unit identity.** Populated `ServiceDiscoveryResponse.headunit_info` (field 17, `HeadUnitInfo` — `protocol_aap::service_discovery_response::HeadUnitInfo`) with fixed, non-sensitive project-identifying strings (project name, `CARGO_PKG_VERSION`, etc. — never real vehicle/user data). Theory: Android Auto's app-level validation might reject a head unit that never identifies itself, distinct from the wire schema (which marks every `HeadUnitInfo` field `optional`). **No change in outcome.**

## What this rules out, and what it doesn't

Ruled out: a simple missing-channel gap; a simple version-number-field mismatch; a simple missing-identity gap.

**Not ruled out**, in roughly descending order of how well-motivated they are:

- **A genuine 1.7 schema change** in `Service`/`ServiceDiscoveryResponse`/`MediaSinkService`/etc. that this project has no source for (new required fields, restructured messages, or fields that must be *absent* now where they weren't before). This remains the leading theory by elimination, but there is no public reference to confirm or investigate it against — see "Research leads" below.
- **A missing control-message step** between `AuthComplete` and `ServiceDiscoveryRequest`, or between `ServiceDiscoveryRequest` and our response, that a real 1.7-speaking phone expects and the pinned 1.6-era AASDK source never modeled at all (not just a field-level gap, but a whole message this project doesn't send or handle).
- **A framing/encryption detail specific to a real phone's TLS stack** that the project's own real-TLS integration tests (`encrypted_service_discovery.rs`, `full_channel_setup.rs`) can't catch, since those exercise the same Rust encrypt/decrypt code on both sides of a controlled fake-phone harness rather than a genuinely independent TLS implementation.
- Something else entirely not yet considered.

Given three independent, content-varying experiments produced *identical* behavior, the consistency itself is a data point: this doesn't look like "almost right, one field off" — it looks structural.

## Research leads already checked (see conversation history for exact queries/results)

- `opencardev/aasdk` PR #2 ("Fix compatibility with latest Android Auto", merged 2019-08-07) — already included in our pinned revision; fixed `VideoFocusRequest`/`touch_event.action_index` parsing issues, not anything we've hit.
- `opencardev/crankshaft` issue #5 ("Error 2 with several phones") — no technical root cause recorded, just "goes away with official OpenAuto on X11."
- `rsjudka/intelligent-auto` issue #40 ("Android Auto won't connect with latest AA") — reports the same class of failure across every AASDK-based implementation checked (crankshaft, intelligent-auto), while the one implementation that doesn't hit it (OpenAuto Pro) is the more complete, commercial one. No technical diagnosis in the issue thread itself.
- One additional source of uncertain provenance was consulted during troubleshooting, for understanding only, per explicit project-owner instruction not to name, cite, quote, or otherwise reflect it in this repository. It did not resolve the question it was consulted for and contributed nothing citable to this record.

## What's still in place in the code (not reverted)

- `ServiceKind::MediaAudio` channel, `AudioCapability`/`AudioStreamType` (`crates/protocol-aap/src/service_discovery_response.rs`), advertised via `AUDIO_CHANNEL_ID` in `auth_discovery_probe.rs`. Driven only to `ChannelOpenState::Open` — no audio `Setup`/`Config`/`Start` handshake exists.
- `HeadUnitInfo` (same file), populated with fixed project-identifying strings in `auth_discovery_probe.rs`.
- `probe_negotiated_version=<major>.<minor>` diagnostic print in `process_actions` (`auth_discovery_probe.rs`) — cheap, permanent, useful visibility into what the phone actually negotiates.
- `AASDK_PROTOCOL_VERSION` is back to the pinned source's `1.6` (the `1.7` experiment was reverted).

## Suggested next steps for whoever picks this up

1. Look for a newer/different open-source Android Auto receiver project (not necessarily AASDK-derived) that has demonstrably worked with a *current* phone, and diff its `ServiceDiscoveryResponse` construction against ours field-by-field.
2. Consider whether a packet capture against a **known-working** current head unit (if one becomes available) could reveal what a real 1.7 response looks like, rather than continuing to guess at content changes.
3. Re-check whether any AASDK fork has been updated since this investigation (2026-08-12) — the ecosystem may catch up.
4. If continuing to experiment against real hardware, keep changes as minimal and independently reversible as the three tried here, and keep running the full verification sweep before each real-phone cycle.
