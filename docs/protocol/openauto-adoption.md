# OpenAuto Adoption Record

## Decision

On 5 August 2026, the project owner approved OpenAuto commit `aa90412bf93b5a5078495ea85ac9270c6297d369` as a GPLv3 source for code and protocol behaviour, while explicitly excluding certificates, private keys, authentication material, security bypasses, trademarks, and bundled assets.

This decision supersedes the earlier blanket exclusion of OpenAuto code. It does not approve OpenAuto Pro, proprietary binaries, Google Desktop Head Unit code, private captures, or any material whose provenance is unclear.

## Pinned upstream

- Repository: https://github.com/f1xpl/openauto
- Branch reviewed: `development`
- Revision: `aa90412bf93b5a5078495ea85ac9270c6297d369`
- Revision date: 12 December 2024
- Upstream description: Android Auto head-unit emulator built on AASDK and Qt
- Repository README licence declaration: GNU GPLv3
- Relevant C++ source/header notices: GNU GPL version 3 or later
- Copyright notice in reviewed source: 2018 f1x.studio (Michal Szwaj)

The pinned tree does not contain a top-level `LICENSE` file. Any adopted behaviour therefore requires verification of the exact source file's GPL-3.0-or-later header, preservation of its copyright/licence notice, and inclusion in this record before implementation.

## Architectural finding

OpenAuto owns application/session orchestration and service implementations, but delegates transport framing and TLS authentication to AASDK. In particular, `AndroidAutoEntityFactory.cpp` constructs AASDK USB or TCP transport, `SSLWrapper`, `Cryptor`, message streams, and the messenger. The credential-bearing implementation remains in AASDK and is not approved for adoption.

OpenAuto's advertised Wi-Fi mode connects to the phone's hidden developer head-unit server. It is not evidence of the normal consumer wireless Android Auto bootstrap, pairing, or authentication flow.

## Approved candidate scope

The following paths may be reviewed for Rust behaviour on a file-by-file basis. Listing a path here approves it as a candidate source; a behaviour is adopted only when its exact purpose and Rust destination are added below.

- `src/autoapp/Service/AndroidAutoEntity.cpp` and matching header: session orchestration after transport creation, version response handling, handshake event flow, service discovery, focus, ping, and shutdown
- `src/autoapp/Service/AndroidAutoEntityFactory.cpp` and matching header: separation of USB/TCP transport from the common session composition
- `src/autoapp/Service/ServiceFactory.cpp` and matching headers: service composition and capability boundaries
- `src/autoapp/Service/VideoService.cpp` and matching header: video service lifecycle and media acknowledgement behaviour
- `src/autoapp/Service/AudioService.cpp`, `MediaAudioService.cpp`, `SystemAudioService.cpp`, `SpeechAudioService.cpp`, and matching headers: separate audio roles and channel lifecycle
- `src/autoapp/Service/AudioInputService.cpp` and matching header: microphone service lifecycle
- `src/autoapp/Service/InputService.cpp` and matching header: touch/button input service lifecycle
- `src/autoapp/Service/SensorService.cpp` and matching header: declared sensor capabilities and event lifecycle
- `src/autoapp/Service/BluetoothService.cpp` and matching header: Bluetooth service boundary and pairing requests
- `src/autoapp/Service/Pinger.cpp` and matching header: liveness behaviour
- `src/autoapp/Projection/*.cpp` and matching interfaces, except `OMXVideoOutput.cpp`: adapter boundaries for video, audio, microphone, input, buffering, and Bluetooth

## Adopted service-discovery event scope

The first OpenAuto-derived behaviour is the transition from successful authentication to handling the phone's service-discovery request. It is derived from:

- `src/autoapp/Service/AndroidAutoEntity.cpp`
- `include/f1x/openauto/autoapp/Service/AndroidAutoEntity.hpp`

Both reviewed files carry the 2018 f1x.studio (Michal Szwaj) copyright notice and GPL-3.0-or-later terms. The Rust destinations are `crates/protocol-aap/src/control.rs` and `crates/protocol-aap/src/service_discovery.rs`. The protobuf field definitions are separately attributed to the pinned AASDK `ServiceDiscoveryRequest.proto` in the AASDK adoption record.

Unlike OpenAuto, the Rust boundary does not log or retain the phone's label, device name, icons, or nested phone information. It validates the request with explicit total, icon, text, and nested-message limits and exposes only field presence and byte counts. Unknown non-group protobuf fields are skipped safely; malformed, oversized, invalid-wire-type, and invalid-UTF-8 inputs fail closed. Four deterministic unit tests use synthetic data only. No service-discovery response, upstream identity string, phone connection, certificate, key, or authentication behaviour is included in this scope.

## Adopted service-catalogue scope

The bounded internal catalogue and conditional service-composition behaviour are derived from:

- `src/autoapp/Service/ServiceFactory.cpp`
- `include/f1x/openauto/autoapp/Service/ServiceFactory.hpp`
- `include/f1x/openauto/autoapp/Service/IService.hpp`

Each file carries the 2018 f1x.studio (Michal Szwaj) copyright notice and GPL-3.0-or-later terms. The Rust destination is `crates/protocol-aap/src/service_catalogue.rs`.

The Rust model separates microphone, media audio, speech audio, system audio, sensors, video, Bluetooth, and input roles. It advertises only locally ready services, reserves channel zero for control, rejects duplicate channel identifiers and roles, and bounds the candidate count. Four deterministic tests cover filtering, lookup, conflicts, duplicate roles, and limits. Hardware selection remains outside the protocol crate and will supply availability through an interface.

The approved OpenAuto revision uses an older AASDK `ChannelDescriptor` service-discovery schema, while approved AASDK revision `9bf6adf` uses a newer repeated `Service` schema. No wire response encoder, old-to-new field mapping, OpenAuto channel number, deprecated vehicle identity, Bluetooth address, display capability, or other upstream value is adopted in this slice. Each newer service schema must be mapped and attributed separately before response bytes are generated.

## Permanent exclusions

- All certificates, private keys, authentication material, shared identities, and security-bypass behaviour
- AASDK `cert/headunit.crt`, `cert/headunit.key`, credential constants, and equivalent material reached through OpenAuto's `Cryptor`
- OpenAuto's hard-coded head-unit name, manufacturer, model, serial number, build identity, or any identity that could misrepresent this project
- `assets/**`, including Android Auto logos/icons and other bundled artwork
- Google and third-party trademarks, trade dress, proprietary binaries, and Desktop Head Unit code
- OpenAuto Pro or any closed/commercial source not separately licensed and explicitly approved
- Pi 3 Broadcom/OpenMAX implementation in `OMXVideoOutput.cpp`; modern Pi targets retain the GStreamer/Wayland capability architecture
- Logging of phone addresses, device names, brands, navigation data, user content, or raw protocol payloads

## Adoption procedure

Before implementing an OpenAuto-derived behaviour:

1. pin the exact upstream path and purpose in this document;
2. verify its file-level copyright and GPL-3.0-or-later notice;
3. name the Rust crate/module receiving the behaviour;
4. record material differences, bounds, privacy controls, and tests;
5. update `THIRD_PARTY_NOTICES.md` and the protocol certainty matrix;
6. keep authentication and asset exclusions enforced by review and automated checks;
7. test first with synthetic fixtures or a deterministic fake peer.

The project remains GPL-3.0-or-later and must preserve applicable OpenAuto notices when distributing derived work.
