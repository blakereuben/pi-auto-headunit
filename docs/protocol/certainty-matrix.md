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
| Android Auto accessory identification values | PX | Not publicly specified in the sources reviewed | No |
| Two-byte AAP frame header, flags, short/extended big-endian lengths, and `0x4000` frame limit | P1 | Owner-approved GPL-3.0-or-later AASDK revision `9bf6adf` | Yes; bounded Rust codec and tests |
| Android Auto security negotiation, services, or channels beyond the adopted framing scope | PX | AASDK paths not yet individually reviewed/adopted | No |
| Wireless Android Auto bootstrap/session | PX | Not publicly specified in the sources reviewed | No |

Public sources:

- https://source.android.com/docs/core/interaction/accessories/aoa
- https://developer.android.com/training/cars/testing/dhu

The generic Milestone 1 identity proves only documented AOA transport behavior. It is not represented as an Android Auto production identity or session.

See [ADR-0002](../architecture/decisions/0002-android-auto-protocol-source-gate.md) for the decision not to infer or copy the missing session protocol.
The [AASDK adoption record](aasdk-adoption.md) records the owner's approval, pinned revision, exact derived files, attribution, and expansion rules.
