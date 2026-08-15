# Wireless Android Auto Protocol Source Assessment — 15 August 2026

This assessment concerns source provenance and project policy for wireless
Android Auto, matching [the 4 August 2026 wired
assessment](source-assessment-2026-08-04.md)'s scope and rigor. It is not
legal advice. No LIVI or AASDK code is reproduced below — only field names,
message shapes, and cited facts (file paths, commit/revision identifiers,
quoted documentation text), matching this project's existing adoption-record
convention (`docs/protocol/aasdk-adoption.md`, `docs/protocol/livi-adoption.md`).

## Result

`certainty-matrix.md`'s existing "Wireless Android Auto bootstrap/session"
row (`PX`, "not publicly specified in the sources reviewed") is **partially
resolved**. AASDK's own pinned schema — already the project's primary
approved source, no new revision needed — includes a real `Bluetooth`
service and a `WifiProjection` service, matching the same channel/message-ID
shape this project already implements for Video/Input/MediaAudio. This
appears to be the mechanism an *already-connected* session (wired, in this
project's case) uses to bootstrap Bluetooth pairing and advertise the car's
Wi-Fi AP identity for future automatic wireless reconnect — not necessarily
the from-scratch discovery a phone that has never connected before would
use. That cold-start discovery question **remains genuinely open** — nothing
found in this assessment answers it.

## Sources assessed

### AASDK `Bluetooth`/`WifiProjection` services (approved, same pinned revision)

Confirmed present, verbatim, at this project's already-approved AASDK
revision (`opencardev/aasdk`, `9bf6adf933665dee26532201719fac14a047ccf1` —
the same revision `docs/protocol/aasdk-adoption.md` already pins; no re-pin
required):

`BluetoothService` (`aap_protobuf.service.bluetooth`, proto2,
`service/bluetooth/BluetoothService.proto`) — advertised in
`ServiceDiscoveryResponse`, matching every other service kind:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `car_address` | required | `string` |
| 2 | `supported_pairing_methods` | repeated, packed | `message.BluetoothPairingMethod` |

`BluetoothMessageId` (`aap_protobuf.service.bluetooth`, proto2,
`service/bluetooth/BluetoothMessageId.proto`) — same wire-value range
(`32768`+) this project's other channel message-ID enums already use
(e.g. `MediaMessageId`):

| Name | Wire value |
|---|---|
| `BLUETOOTH_MESSAGE_PAIRING_REQUEST` | 32769 |
| `BLUETOOTH_MESSAGE_PAIRING_RESPONSE` | 32770 |
| `BLUETOOTH_MESSAGE_AUTHENTICATION_DATA` | 32771 |
| `BLUETOOTH_MESSAGE_AUTHENTICATION_RESULT` | 32772 |

`BluetoothPairingMethod` (`aap_protobuf.service.bluetooth.message`, proto2,
`service/bluetooth/message/BluetoothPairingMethod.proto`) — standard
Bluetooth Secure Simple Pairing method enum:

| Name | Wire value |
|---|---|
| `BLUETOOTH_PAIRING_UNAVAILABLE` | -1 |
| `BLUETOOTH_PAIRING_OOB` | 1 |
| `BLUETOOTH_PAIRING_NUMERIC_COMPARISON` | 2 |
| `BLUETOOTH_PAIRING_PASSKEY_ENTRY` | 3 |
| `BLUETOOTH_PAIRING_PIN` | 4 |

`BluetoothPairingRequest` (`aap_protobuf.service.bluetooth.message`, proto2,
`service/bluetooth/message/BluetoothPairingRequest.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `phone_address` | required | `string` |
| 2 | `pairing_method` | required | `BluetoothPairingMethod` |

`BluetoothPairingResponse` (`aap_protobuf.service.bluetooth.message`,
proto2, `service/bluetooth/message/BluetoothPairingResponse.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `status` | required | `shared.MessageStatus` |
| 2 | `already_paired` | required | `bool` |

`BluetoothAuthenticationData` (`aap_protobuf.service.bluetooth.message`,
proto2, `service/bluetooth/message/BluetoothAuthenticationData.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `auth_data` | required | `string` |
| 2 | `pairing_method` | optional | `BluetoothPairingMethod` |

`BluetoothAuthenticationResult` (`aap_protobuf.service.bluetooth.message`,
proto2, `service/bluetooth/message/BluetoothAuthenticationResult.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `status` | required | `shared.MessageStatus` |

`WifiProjectionService` (`aap_protobuf.service.wifiprojection`, proto2,
`service/wifiprojection/WifiProjectionService.proto`) — advertised in
`ServiceDiscoveryResponse`, same as `BluetoothService`:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `car_wifi_bssid` | optional | `string` |

`WirelessTcpConfiguration` was already field-mapped in
`docs/protocol/aasdk-adoption.md` (a `ConnectionConfiguration` field,
alongside `PingConfiguration`) — three `uint32` socket-tuning fields
(`socket_receive_buffer_size_kb`, `socket_send_buffer_size_kb`,
`socket_read_timeout_ms`). On its own this only tunes an *already-open*
TCP socket; it says nothing about how that socket gets established
wirelessly in the first place, and remains the only wireless-adjacent
field this project had previously reviewed before this assessment.

None of the `bluetooth`/`wifiprojection` proto files carry a per-file
licence/copyright header at the pinned revision — the same posture already
recorded for `ServiceDiscoveryRequest.proto` and other already-adopted
proto files in `aasdk-adoption.md`, which rely on the repository-wide
GPL-3.0-or-later notices in the adopted `.hpp`/`.cpp` files rather than a
per-proto notice.

**Not resolved by this schema alone**: nothing in `service/bluetooth` or
`service/wifiprojection` describes *when* or *how* this Bluetooth channel
gets opened in the first place — whether it requires an existing AAP
session (wired or otherwise) already established, or can be reached
standalone. This project has only ever reached AAP channel-open behaviour
over an already-negotiated session (per `docs/protocol/aasdk-adoption.md`'s
channel-open work); nothing here changes that boundary.

### `f-io/LIVI` (already approved, GPL-3.0-or-later, revision `9000f308eec423c5c56ac0a14491a7c95ce5762d`)

LIVI implements working wireless Android Auto in production (confirmed via
its own README): "**Android Auto** (wireless) on Linux", and "Wireless
CarPlay and wireless Android Auto are enabled separately, so a head unit
can offer one, both, or neither." Runtime requirements listed: `bluez`,
`libspa-0.2-bluetooth`, `hostapd`, `dnsmasq-base`. One specific, citable
protocol fact from LIVI's own documentation: "PipeWire is what puts HFP
into the adapter's service record" and "Wireless Android Auto needs that
plugin because the phone will only start a session over an HFP
connection" — i.e. the phone's wireless-session trigger rides on the
head unit's Bluetooth adapter advertising a standard Bluetooth Hands-Free
Profile (HFP) service record, not an Android-Auto-specific Bluetooth
profile.

The actual Bluetooth pairing/session logic in LIVI's reviewed source
(`src/main/services/projection/bt/BluezDeviceClient.ts`,
`BtPairedRegistry.ts`) is a thin IPC client talking JSON over a Unix
socket (`/tmp/aa-bt.sock`) to an **external helper process** referenced in
the code as "aa-bluetooth.py" — not included in LIVI's own reviewed
TypeScript source tree, and not located by this assessment (not present
under LIVI's `docs/`, not named in `CREDITS.md`, GitHub code search for it
required authentication this assessment didn't have). **This means LIVI's
own reviewed source does not give wire-level provenance for the actual
Bluetooth RFCOMM/pairing sequence** — only the architectural facts above
(HFP-triggered, bluez/hostapd/dnsmasq-based). The wire-level provenance
for the pairing *message shapes* comes from AASDK's schema (above), not
from LIVI.

### `BertoldVdb/WACResearch` — ruled out, unrelated protocol

Cited in LIVI's `CREDITS.md` under "Android Auto / CarPlay Related" as
"Inspiration & Prior Art", but direct review found this repository
concerns **Apple's WAC (Wireless Accessory Configuration)** protocol —
Wi-Fi credential provisioning for MFi/AirPlay accessories — not Android
Auto. MIT-licensed, Go-language security research on Apple's protocol.
Almost certainly cited by LIVI for its *CarPlay* wireless support, not its
Android Auto support; CREDITS.md groups both platforms under one heading
without distinguishing which entry applies to which. Not a usable source
for this project's Android-Auto-only scope.

## What remains unresolved

- **Cold-start discovery**: how a phone that has never connected to this
  head unit before (no prior wired pairing, no existing AAP session) finds
  and initiates a wireless session from scratch. Nothing found in this
  assessment answers this — LIVI's own architecture appears to also depend
  on the HFP-advertising Bluetooth adapter already being paired/known
  somehow, which this assessment did not trace further.
- **The Bluetooth RFCOMM/pairing wire sequence's exact byte-level
  behaviour** beyond the protobuf message shapes above (ordering,
  timeouts, retry behaviour) — AASDK's schema gives message *shapes*, not
  session *choreography*; LIVI's own choreography lives in the
  unreviewed external helper noted above.
- Whether `WifiProjectionService.car_wifi_bssid` is sent proactively by
  the head unit or requested by the phone, and at what point in a session
  this channel is expected to open, remain unconfirmed — no real-hardware
  wireless trial has been attempted (this project has no wireless AA
  hardware/software setup yet; M7 is unstarted).

## Candidate route for M7

Given the above, the same adoption gate this project already uses for
wired protocol behaviour applies directly: `service/bluetooth` and
`service/wifiprojection` are viable **candidate paths** for a future
AASDK adoption-record expansion (matching `aasdk-adoption.md`'s existing
"Approved candidate scope, only adopted when added" convention) — no new
source-approval decision is needed to *list* them as candidates, since
they're already within the same pinned, owner-approved AASDK revision.
Actually *adopting* specific behaviour from them (building a Rust
`BluetoothChannel`, wiring it into the service catalogue) would be new M7
implementation work, not something this assessment authorises on its own.

## References

- AASDK (pinned, approved): https://github.com/opencardev/aasdk/tree/9bf6adf933665dee26532201719fac14a047ccf1/protobuf/aap_protobuf/service/bluetooth
- AASDK (pinned, approved): https://github.com/opencardev/aasdk/tree/9bf6adf933665dee26532201719fac14a047ccf1/protobuf/aap_protobuf/service/wifiprojection
- LIVI (pinned, approved): https://github.com/f-io/LIVI
- LIVI Bluetooth client (reviewed, not adopted): https://github.com/f-io/LIVI/tree/main/src/main/services/projection/bt
- `BertoldVdb/WACResearch` (reviewed, ruled out — unrelated Apple protocol): https://github.com/BertoldVdb/WACResearch
