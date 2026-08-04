# ADR-0002: Android Auto Protocol Source Gate

- Status: accepted for the current source set; session implementation blocked pending an approved source
- Date: 4 August 2026

## Context

The project must implement only behaviour with traceable provenance and compatible distribution rights. It must not copy Google Desktop Head Unit binaries, OpenAuto code, private captures, or protocol definitions with unclear origin. Android Open Accessory support alone does not specify an Android Auto session.

The official sources reviewed establish that:

- AOSP publicly specifies the generic Android Open Accessory control requests, accessory re-enumeration, and bulk endpoints.
- Google's Desktop Head Unit documentation confirms that DHU 2.x can connect to Android Auto over AOA USB.
- The same documentation exposes a development-only alternative in which ADB forwards TCP port 5277 to the phone's head-unit server.
- DHU configuration documentation names supported head-unit capabilities such as 800x480, 1280x720, and 1920x1080 video modes, touch input, microphone input, and sensors.

Those sources do not specify Android Auto accessory identification values, stream framing, cryptographic negotiation, service discovery, channel messages, media framing, touch messages, or wireless bootstrap/session behaviour. The downloadable DHU executable is a tool, not a public protocol specification or source-code grant.

## Decision

1. Keep the implemented generic AOA transition and bulk-endpoint discovery at certainty level P0.
2. Treat official DHU transport and capability statements as product evidence only; do not infer a wire format from them.
3. Keep all post-AOA Android Auto session and wireless behaviour at PX until an approved source and licence/provenance review exists.
4. Do not inspect, disassemble, redistribute, or translate the proprietary DHU binary into project code.
5. Do not copy OpenAuto/AASDK protocol code or definitions into the repository without an explicit project-owner decision, compatible GPL review, and any required legal advice.
6. Continue transport-independent safety work and synthetic media, audio, touch, and UI validation while this gate is unresolved.

## Consequences

- Milestone 2 cannot truthfully claim an Android Auto session from the currently approved sources.
- The project can still validate bounded I/O, cancellation, media performance, display/touch/audio integration, packaging, and appliance behaviour without protocol constants.
- A future proposal must list every source, its licence, the exact behaviours derived from it, and the separation or attribution required before session code begins.

The subsequent public/open-source candidate survey is recorded in [the 4 August 2026 source assessment](../../protocol/source-assessment-2026-08-04.md). It found that AASDK declares GPLv3 and is separate from OpenAuto, but no reviewed public material establishes the provenance of its undocumented protocol definitions. AASDK therefore remains unapproved pending written provenance and legal/licence review.

## Official sources reviewed

- AOSP Android Open Accessory: https://source.android.com/docs/core/interaction/accessories/aoa
- Google Android Auto Desktop Head Unit testing: https://developer.android.com/training/cars/testing/dhu
