# LIVI Adoption Record

## Decision

On 14 August 2026, the project owner approved `f-io/LIVI` commit `9000f308eec423c5c56ac0a14491a7c95ce5762d` as a GPL-3.0-or-later source for Android Auto session/channel behaviour, following the same gate `docs/protocol/openauto-adoption.md` and `docs/protocol/aasdk-adoption.md` already went through (explicit project-owner decision, compatible GPL review, exact file-level provenance, preserved notices — `docs/architecture/decisions/0002-android-auto-protocol-source-gate.md`).

This supersedes the reference-only status LIVI held throughout the rest of this investigation (`docs/protocol/error-2-investigation.md`, "Comprehensive LIVI audit" and later sections), where it was cited for facts/wire-shapes/timing only, with explicit "no LIVI code reproduced" language. From this record onward, behaviour derived from LIVI is tracked with the same file-level rigor as OpenAuto/AASDK; earlier LIVI-derived fixes made before this record existed are retroactively attributed below.

This does not approve LIVI's branding, name, icons/assets, build/packaging tooling, or any credential/certificate material — see "Permanent exclusions" below.

## Pinned upstream

- Repository: https://github.com/f-io/LIVI
- Revision: `9000f308eec423c5c56ac0a14491a7c95ce5762d`
- Revision date: 14 August 2026 (`HEAD` at time of cloning for research)
- Upstream description: "LIVI — Linux In-Vehicle Infotainment" (`package.json`), an Electron/TypeScript Android Auto/CarPlay head unit
- `package.json` licence declaration: `GPL-3.0-or-later`
- `README.md` licence declaration: "LIVI is free software, licensed under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`). See LICENSE for the full text."
- Top-level `LICENSE`: full GPL-3.0 text present
- Per-file notices (confirmed present on reviewed protocol/session files, e.g. `src/main/services/projection/driver/aa/protos/oaa/sensor/SensorChannelData.proto`): `SPDX-License-Identifier: GPL-3.0-or-later`, `Copyright (C) 2024-2026 Open Android Auto contributors`, `Based on works by: f1x.studio (aasdk), opencardev (crankshaft), aa-proxy (aa-proxy-rs)`
- Author/contributors (`package.json`): Lasse Heitgres (author), Anton Ashyn, Lasse Heitgres (contributors)

Compatible with this project's own `GPL-3.0-or-later` licensing (`LICENSE`). Cloned locally only into the session scratchpad (outside this repository's working tree) for research and file-level review; the upstream tree itself is never vendored or committed here, matching how AASDK/OpenAuto are handled (see `THIRD_PARTY_NOTICES.md`, "Approved source references").

## Architectural finding

LIVI's session state machine (`src/main/services/projection/driver/aa/stack/session/Session.ts`) differs from the OpenAuto-derived `Pinger` model this project's `PING_INTERVAL` design was based on in two respects: LIVI arms its ping timer immediately after sending `ServiceDiscoveryResponse` — before any channel is opened — rather than before `VersionRequest` (OpenAuto's `AndroidAutoEntity::start()` timing); and LIVI runs a local watchdog (a private constant, 5000ms) that closes the session itself if no `PING_RESPONSE` arrives within that window of the last one, describing the ping loop's dual role as both a liveness signal and a keepalive watchdog. Ping/pong cadence is 1500ms in both directions, matching the `ping_configuration.interval_ms` value this project already advertises (added this session, prior to this formal adoption).

LIVI's video/audio channel handlers (`VideoChannel.ts`, `AudioChannel.ts`) send an unconditional ack for every received AV frame for flow control, and its session layer answers `KeyBindingRequest` unconditionally rather than validating against an advertised keycode capability list — both already independently reimplemented in this project (see "Adopted scope" below) before this formal record existed, based on informal LIVI reference during the "Comprehensive LIVI audit" pass.

## Approved candidate scope

The following paths may be reviewed for Rust behaviour on a file-by-file basis, matching the `docs/protocol/openauto-adoption.md` convention (listing a path approves it as a candidate; a behaviour is adopted only when added below):

- `src/main/services/projection/driver/aa/stack/session/Session.ts`: session state machine, ping/pong timing and watchdog, channel-open/setup sequencing, video-focus timing
- `src/main/services/projection/driver/aa/stack/session/ServiceDiscoveryBuilder.ts`: `ServiceDiscoveryResponse` field population, including `ping_configuration`, `ui_config`
- `src/main/services/projection/driver/aa/stack/channels/VideoChannel.ts`: video channel ack/flow-control behaviour
- `src/main/services/projection/driver/aa/stack/channels/AudioChannel.ts`: audio channel ack/flow-control behaviour (all three sink roles)
- `src/main/services/projection/driver/aa/protos/oaa/**/*.proto`: wire schema cross-reference only (field names/numbers), not code

## Adopted scope

1. **Unsolicited `VideoFocusNotification` timing.** Derived from `VideoChannel.ts`'s documented constraint that video focus must be granted only after the video channel's own `Config`, not immediately after `ServiceDiscoveryResponse` (an earlier ordering was found to trigger `AudioFocus RELEASE`). Rust destination: `crates/protocol-aap/src/video_setup.rs`'s `encode_video_focus_notification`. Implemented and real-hardware-confirmed (`docs/protocol/error-2-investigation.md`, "`VideoFocusNotification` breakthrough") prior to this formal record; retroactively attributed here.
2. **Unconditional per-frame `Ack`.** Derived from `VideoChannel.ts`/`AudioChannel.ts`'s ack-every-frame flow-control behaviour. Rust destination: `crates/protocol-aap/src/media_message.rs`'s `encode_media_ack`. Implemented prior to this formal record ("Comprehensive LIVI audit"); retroactively attributed here.
3. **Unconditional `KeyBindingResponse` success.** Derived from `Session.ts`'s unconditional-success key-binding reply, contrasted with this project's earlier stricter validation. Rust destination: `apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs`'s `evaluate_key_binding_request`. Implemented prior to this formal record; retroactively attributed here.
4. **`ping_configuration.interval_ms = 1500` advertisement.** Derived from `Session.ts`'s 1500ms ping/pong cadence. Rust destination: `crates/protocol-aap/src/service_discovery_response.rs`'s `PingConfiguration`. Implemented this session, prior to this formal record; retroactively attributed here.
5. **Ping arm-timing and watchdog model.** Derived from `Session.ts`: the ping timer is armed immediately after `ServiceDiscoveryResponse` is sent (not gated on any later handshake/channel-setup progress), sends `PingRequest` every 1500ms continuously for the session's duration, and a local watchdog closes the session if no `PING_RESPONSE` is seen within 5000ms of the last one. Rust destination: `apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs` (`PingState`, `PING_INTERVAL`, `PING_WATCHDOG_TIMEOUT`). Implemented and real-hardware-tested (`docs/protocol/error-2-investigation.md`, "LIVI formally adopted; real ping-timing trial") prior to this line being added; retroactively attributed here.
6. **`VideoConfiguration.ui_config` populated with all-zero insets.** Derived from `ServiceDiscoveryBuilder.ts`: LIVI populates `UiConfig.margins`/`content_insets`/`stable_content_insets` on every video config, computing non-zero values only when its own configured display size diverges in aspect ratio from the negotiated video tier, and always setting `stable_content_insets` equal to `content_insets`. This project advertises a single fixed 800x480 tier with no display-specific customization implemented yet, so LIVI's own default (all-zero) case applies directly. Rust destination: `crates/protocol-aap/src/service_discovery_response.rs`'s `UiConfig`/`Insets`/`encode_ui_config`/`encode_insets`. `UiConfig`'s fourth field, `ui_theme`, is not adopted (unresearched this pass — see `docs/protocol/aasdk-adoption.md`'s "not yet mapped" list). Field numbers/types for `UiConfig`/`Insets` themselves come from this project's own pinned AASDK source (already field-mapped in `docs/protocol/aasdk-adoption.md`), not from LIVI — LIVI is the source for the *behavioural decision* to populate this field at all and for what value to send by default, not for the wire schema.

Each adopted behaviour above is reimplemented independently in Rust, matching this project's own architecture and error handling — not translated line-by-line from LIVI's TypeScript. What's adopted is the *behaviour* (timing, sequencing, response content), cited to its exact LIVI source file, not LIVI's code text itself.

## Permanent exclusions

- `native/crypto/**` and any TLS/certificate/private-key material anywhere in the LIVI tree — never read for content, never referenced, never adopted. This project uses only its own operator-authorised credentials, loaded exclusively from `/etc/aa-headunit/credentials` at runtime (`credential_store::load_credentials`); nothing credential-shaped from LIVI is ever committed to this repository or distributed in any form, per explicit, standing project-owner instruction (also `CLAUDE.md`).
- LIVI's name, branding, `desktopName` (`dev.f-io.livi.desktop`), icons, and `assets/**`
- LIVI's build/packaging tooling (`electron-builder.yml`, `native/livi-compositor`, `native/gst-video`, CI workflows) — this project has its own ARM64 `.deb` packaging
- Channels/services LIVI implements that this project does not model or intend to (Navigation-status, Media-playback, Phone-status, WiFi-projection, Cluster video/input) — out of scope for this adoption
- Any LIVI test fixtures, sample data, or synthetic phone-identity strings that could be mistaken for real device/user data

## Adoption procedure

Same as `docs/protocol/openauto-adoption.md`:

1. pin the exact upstream path and purpose in this document;
2. verify its file-level `SPDX-License-Identifier`/copyright notice;
3. name the Rust crate/module receiving the behaviour;
4. record material differences, bounds, privacy controls, and tests;
5. update `THIRD_PARTY_NOTICES.md` and `docs/protocol/certainty-matrix.md`;
6. keep credential/asset/branding exclusions enforced by review;
7. test first with synthetic fixtures or a deterministic fake peer before any real-phone trial.

The project remains GPL-3.0-or-later and must preserve applicable LIVI notices (SPDX identifier, copyright, "Based on works by" chain) when distributing derived work.
