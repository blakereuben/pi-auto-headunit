# Protocol Certainty Matrix

| Operation | Level | Source | Implemented |
|---|---|---|---|
| USB host enumerates an attached device | P0 | USB/libusb platform contract | Yes |
| AOA Get Protocol request 51 | P0 | AOSP AOA 1.0 | Yes |
| AOA Send String request 52 | P0 | AOSP AOA 1.0 | Yes, with generic project identity |
| AOA Start Accessory request 53 | P0 | AOSP AOA 1.0 | Yes |
| Wait for Google accessory VID/PID and locate bulk endpoints | P0 | AOSP AOA 1.0 | Yes |
| Android Auto accessory identification values | PX | Not publicly specified in the sources reviewed | No |
| Android Auto framing, security negotiation, services, or channels | PX | Not publicly specified in the sources reviewed | No |
| Wireless Android Auto bootstrap/session | PX | Not publicly specified in the sources reviewed | No |

Public source: https://source.android.com/docs/core/interaction/accessories/aoa

The generic Milestone 1 identity proves only documented AOA transport behavior. It is not represented as an Android Auto production identity or session.

