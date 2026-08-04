# AASDK Adoption Record

## Decision

The project owner approved AASDK as a GPLv3-compatible source for Android Auto Protocol behaviour on 4 August 2026. OpenAuto remains excluded as a code source. Protocol work will use Rust by default and preserve AASDK attribution wherever behaviour or definitions are derived.

## Pinned upstream

- Repository: https://github.com/opencardev/aasdk
- Branch reviewed: `newdev`
- Revision: `9bf6adf933665dee26532201719fac14a047ccf1`
- Upstream lineage: fork of https://github.com/f1xpl/aasdk
- Licence notices in the adopted framing files: GNU GPL version 3 or later
- Copyright notices in the adopted framing files: 2018 f1x.studio (Michal Szwaj); 2024 CubeOne (Simon Dean)

The maintained repository README links to a root `LICENSE` file that is absent at the pinned revision. Adoption therefore relies on the explicit GPL-3.0-or-later notices in each source file used, not the broken README link.

## Adopted framing scope

The Rust `protocol-aap` framing implementation is derived from these files:

- `include/aasdk/Messenger/FrameHeader.hpp`
- `include/aasdk/Messenger/FrameType.hpp`
- `include/aasdk/Messenger/EncryptionType.hpp`
- `include/aasdk/Messenger/MessageType.hpp`
- `include/aasdk/Messenger/FrameSize.hpp`
- `include/aasdk/Messenger/FrameSizeType.hpp`
- `include/aasdk/Messenger/MessageOutStream.hpp`
- `src/Messenger/FrameHeader.cpp`
- `src/Messenger/FrameSize.cpp`
- `src/Messenger/MessageInStream.cpp`
- `src/Messenger/MessageOutStream.cpp`

Derived facts currently cover the two-byte frame header, flag layout, big-endian short/extended sizes, first-frame total size, the `0x4000` frame payload limit, and per-channel fragment reassembly. The Rust implementation adds stricter reserved-bit validation, independent bounded total-message/concurrent-channel limits, and rejects restarted, incomplete, inconsistent, or metadata-changing fragment sequences.

## Adopted control-handshake scope

The bounded control envelope and fake-transport handshake state machine are derived from these files:

- `include/aasdk/Version.hpp`
- `include/aasdk/Messenger/Cryptor.hpp`
- `include/aasdk/Messenger/ICryptor.hpp`
- `src/Messenger/Cryptor.cpp`
- `include/aasdk/Transport/ISSLWrapper.hpp`
- `include/aasdk/Transport/SSLWrapper.hpp`
- `src/Transport/SSLWrapper.cpp`
- `include/aasdk/Channel/Control/ControlServiceChannel.hpp`
- `include/aasdk/Channel/Control/IControlServiceChannel.hpp`
- `include/aasdk/Channel/Control/IControlServiceChannelEventHandler.hpp`
- `src/Channel/Control/ControlServiceChannel.cpp`
- `protobuf/aap_protobuf/service/control/ControlMessageType.proto`
- `protobuf/aap_protobuf/service/control/message/AuthResponse.proto`
- `protobuf/aap_protobuf/service/control/message/ServiceDiscoveryRequest.proto`
- `protobuf/aap_protobuf/shared/MessageStatus.proto`

Derived facts cover protocol version 1.6, two-byte big-endian control-message identifiers, the version request/response layout, TLS records encapsulated as control message 3, the successful proto2 authentication response, and transition to a received service-discovery request. The negotiated version returned by the phone is retained instead of assuming it matches the offered version.

The control-state test still uses fake TLS bytes. The separate `security-openssl` crate now reproduces AASDK's OpenSSL client/memory-buffer boundary with injected credentials and bounded input/output, but it does not copy AASDK's embedded certificate/private-key material. It has only been tested with runtime-generated test credentials and has not contacted a phone. Service-discovery parsing and live session traffic remain outside this scope.

The shared AASDK certificate identifies organisations named Google Automotive Link and JVC Kenwood and is paired with a publicly distributed private key. GPL compatibility alone does not answer whether this project should present or redistribute that identity material. The repository therefore does not contain it; the compatibility, trademark/identity, and distribution decision is recorded separately in `tls-credential-policy.md` and remains open.

## Adopted USB interoperability-probe scope

The opt-in bench probe's Android Auto accessory identification and claimed bulk-transfer behaviour are derived from:

- `include/aasdk/USB/AccessoryModeQueryFactory.hpp`
- `src/USB/AccessoryModeQueryFactory.cpp`
- `include/aasdk/Transport/USBTransport.hpp`
- `src/Transport/USBTransport.cpp`

The probe uses AASDK's exact six accessory strings, including its third-party URI and serial value, only when the operator supplies `--allow-live-aap`. It then sends the adopted version and encapsulated-TLS messages over a claimed bulk interface using temporary project-generated credentials. It stops after the TLS engine completes, does not send authentication completion or a service-discovery response, logs no payloads, and rejects a phone already in accessory mode so the applied identity is unambiguous.

## Expansion rule

Before another AASDK behaviour, schema, identifier, certificate, or asset is used, add its exact upstream path and purpose here, verify its file-level licence/copyright notice, and update third-party notices. No OpenAuto file may be used to fill a gap.
