# Protocol Certainty Matrix

| Operation | Level | Source | Implemented |
|---|---|---|---|
| USB host enumerates an attached device | P0 | USB/libusb platform contract | Yes |
| AOA Get Protocol request 51 | P0 | AOSP AOA 1.0 | Yes |
| AOA Send String request 52 | P0 | AOSP AOA 1.0 | Yes, with generic project identity |
| AOA Start Accessory request 53 | P0 | AOSP AOA 1.0 | Yes |
| Wait for Google accessory VID/PID and locate bulk endpoints | P0 | AOSP AOA 1.0 | Yes |
| DHU 2.x supports Android Auto over AOA USB | P0 product fact | Official Google DHU documentation | Evidence only |
| Development DHU can reach the phone head-unit server through ADB-forwarded TCP port 5277 | P0 product fact | Official Google DHU documentation | No; development transport only |
| DHU advertises 800x480, 1280x720, and 1920x1080 configurations plus touch/microphone/sensor capabilities | P0 product fact | Official Google DHU documentation | Evidence only; no wire format inferred |
| Android Auto accessory identification values used by the opt-in probe | P1 | Owner-approved GPL-3.0-or-later AASDK `AccessoryModeQueryFactory.cpp` | Yes; exact strings, live result pending |
| Two-byte AAP frame header, flags, short/extended big-endian lengths, and `0x4000` frame limit | P1 | Owner-approved GPL-3.0-or-later AASDK revision `9bf6adf` | Yes; bounded Rust codec and tests |
| Per-channel first/middle/last/bulk message reassembly | P1 | Same approved AASDK revision and recorded `MessageInStream` source | Yes; bounded Rust assembler and tests |
| Control envelope, version 1.6 negotiation, encapsulated TLS flow, successful authentication response, and transition to service discovery | P1 | Owner-approved GPL-3.0-or-later AASDK revision `9bf6adf`; exact control/cryptor/schema paths recorded | Yes; bounded state machine with fake TLS only |
| Replaceable OpenSSL client using bounded memory transport and injected credentials | P1 | Approved AASDK cryptor/SSL wrapper paths plus maintained Rust OpenSSL bindings | Yes; native tests pass; first live probe accepted version 1.6 then TLS timed out |
| Compatibility certificate/key policy, service-discovery parsing, and subsequent channels | PX | Credential identity/distribution decision and further AASDK paths are not yet approved | No |
| Wireless Android Auto bootstrap/session | PX | Not publicly specified in the sources reviewed | No |

Public sources:

- https://source.android.com/docs/core/interaction/accessories/aoa
- https://developer.android.com/training/cars/testing/dhu

The generic Milestone 1 identity proves only documented AOA transport behavior. It is not represented as an Android Auto production identity or session.

See [ADR-0002](../architecture/decisions/0002-android-auto-protocol-source-gate.md) for the decision not to infer or copy the missing session protocol.
The [AASDK adoption record](aasdk-adoption.md) records the owner's approval, pinned revision, exact derived files, attribution, and expansion rules.
