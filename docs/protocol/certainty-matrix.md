# Protocol Certainty Matrix

| Operation | Level | Source | Implemented |
|---|---|---|---|
| USB host enumerates an attached device | P0 | USB/libusb platform contract | Yes |
| AOA Get Protocol request 51 | P0 | AOSP AOA 1.0 | Yes |
| AOA Send String request 52 | P0 | AOSP AOA 1.0 | Yes, with generic project identity |
| AOA Start Accessory request 53 | P0 | AOSP AOA 1.0 | Yes |
| Wait for Google accessory VID/PID and locate bulk endpoints | P0 | AOSP AOA 1.0 | Yes |
| DHU 2.x supports Android Auto over AOA USB | P0 product fact | Official Google DHU documentation | Evidence only |
| Development DHU can reach the phone head-unit server through ADB-forwarded TCP port 5277 | P0 product fact | Official Google DHU documentation | Yes; loopback-only connection adapter/probe, development use only |
| DHU advertises 800x480, 1280x720, and 1920x1080 configurations plus touch/microphone/sensor capabilities | P0 product fact | Official Google DHU documentation | Evidence only; no wire format inferred |
| Android Auto accessory identification values used by the opt-in probe | P1 | Owner-approved GPL-3.0-or-later AASDK `AccessoryModeQueryFactory.cpp` | Yes; exact strings, live result pending |
| Two-byte AAP frame header, flags, short/extended big-endian lengths, and `0x4000` frame limit | P1 | Owner-approved GPL-3.0-or-later AASDK revision `9bf6adf` | Yes; bounded Rust codec and tests |
| Per-channel first/middle/last/bulk message reassembly | P1 | Same approved AASDK revision and recorded `MessageInStream` source | Yes; bounded Rust assembler and tests |
| Control envelope, version 1.6 negotiation, encapsulated TLS flow, successful authentication response, and transition to service discovery | P1 | Owner-approved GPL-3.0-or-later AASDK revision `9bf6adf`; exact control/cryptor/schema paths recorded | Yes; bounded state machine with fake TLS only |
| Replaceable OpenSSL client using bounded memory transport and injected credentials | P1 | Approved AASDK cryptor/SSL wrapper paths plus maintained Rust OpenSSL bindings | Yes; native tests include complete mutual TLS against a synthetic verifier; live Android Auto still rejects the independent identity |
| Independently generated TLS identity | P1 negative result | Pi 5 live probes over USB/AOA and the official ADB tunnel: AAP 1.6 accepted, TLS peer data received, phone displayed Android Auto error 7 | Rejected on both transports; do not repeat |
| AASDK/OpenAuto shared compatibility credentials or security bypasses | Excluded | Project-owner decision following error-7 research and identity review | No; must not be added or tested |
| Official developer-mode ADB-forwarded TCP 5277 transport | P0 product fact plus live negative result | Google DHU documentation and sanitized Pi 5 test | Implemented; version 1.6 accepted and TLS peer data received, but generated identity rejected with error 7 |
| OpenAuto session and service behaviour | P1 partially adopted | GPL-3.0-or-later OpenAuto revision `aa90412`; exact file scope in adoption record | First bounded service-discovery event transition implemented with synthetic tests; remaining services not implemented |
| Service-discovery request fields | P1 | Approved AASDK `ServiceDiscoveryRequest.proto` plus the attributed OpenAuto event boundary | Yes; bounded, privacy-preserving summary only, with no field content retained |
| Internal service catalogue and readiness filtering | P1 | Approved OpenAuto `ServiceFactory.cpp`, `ServiceFactory.hpp`, and `IService.hpp` | Yes; bounded synthetic model, not wire encoded |
| Service-discovery response wire format | PX pending mapping | Pinned OpenAuto uses an older `ChannelDescriptor` schema; pinned maintained AASDK uses newer repeated `Service` messages | No; field-by-field current-schema adoption required before encoding |
| Wireless Android Auto bootstrap/session | PX, partially resolved | AASDK's pinned schema (`service/bluetooth`, `service/wifiprojection`) confirmed present, same revision `9bf6adf`; `docs/protocol/wireless-source-assessment.md` | No; schema-only candidate, not implemented. Cold-start discovery (no prior pairing) remains unspecified in sources reviewed |
| LIVI-derived video-focus timing, per-frame media ack, unconditional key-binding response, and ping arm-timing/cadence/watchdog | P1 partially adopted | GPL-3.0-or-later LIVI revision `9000f30`; exact file scope in `docs/protocol/livi-adoption.md` | Implemented; real-hardware result mixed — see `docs/protocol/error-2-investigation.md`, "LIVI formally adopted; real ping-timing trial" |
| LIVI-derived small/recycled touch pointer-id allocation (kernel `ABS_MT_SLOT` used as `pointer_id`, not the raw driver `ABS_MT_TRACKING_ID`) | P1 adopted | GPL-3.0-or-later LIVI revision `9000f30`; exact file scope in `docs/protocol/livi-adoption.md` | Implemented; real-hardware-confirmed — continuous drag and pinch, previously non-functional across four prior trials, now register on a real phone; see `docs/protocol/touch-input-investigation.md`, "Trial 5" |
| Media message envelope: `MediaMessageId` wire values, `Data`/`CodecConfig`/`Ack`/`Stop` framing | P1 | Owner-approved GPL-3.0-or-later AASDK revision `9bf6adf` (`MediaMessageId.proto`, `MessageId.cpp`, `media.source.message.Ack.proto`) plus LIVI's per-frame ack-every-frame policy, `docs/protocol/livi-adoption.md` | Yes; `crates/protocol-aap/src/media_message.rs` |
| Video codec payload format: Annex-B H.264/H.265 byte-stream framing, in-band SPS/PPS extraction, AAP timestamp as PTS microseconds | Public ITU-T H.264/H.265 standard, not Android-Auto-specific — no AASDK/OpenAuto/LIVI source needed | Least-assumption default when first written | Yes; real-hardware-confirmed — 1,462 real H.265 `Data` frames decoded and rendered with zero pipeline errors, real video confirmed on the head unit's own physical display; `MILESTONE_CHECKLIST.md` M4 |
| Audio codec payload format: signed 16-bit little-endian PCM (`S16LE`), AAP timestamp as PTS microseconds | Platform-standard raw layout, not Android-Auto-specific — no AASDK/OpenAuto/LIVI source needed | Least-assumption default when first written | Yes; real-hardware-confirmed by ear — 611 real `MediaAudio` PCM frames, correct audible playback confirmed by the operator; `MILESTONE_CHECKLIST.md` M4 (playback *reliability* remains separately open — an intermittent root-vs-PipeWire environment conflict, not a format-correctness issue) |

Public sources:

- https://source.android.com/docs/core/interaction/accessories/aoa
- https://developer.android.com/training/cars/testing/dhu

The generic Milestone 1 identity proves only documented AOA transport behavior. It is not represented as an Android Auto production identity or session.

See [ADR-0002](../architecture/decisions/0002-android-auto-protocol-source-gate.md) for the decision not to infer or copy the missing session protocol.
The [AASDK adoption record](aasdk-adoption.md) records the owner's approval, pinned revision, exact derived files, attribution, and expansion rules.
