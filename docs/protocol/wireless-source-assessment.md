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
row (`PX`, "not publicly specified in the sources reviewed") is now
**substantially resolved for the end-to-end bootstrap chain**, after two
research passes — the second one, prompted by a Plan-agent validation
pass that caught a real error in the first (see the "Correction" note
under `manio/aa-proxy-rs` below), corrected which specific AASDK message
family is actually used. The real chain, with citations for every leg:
standard Bluetooth SDP/pairing (public Bluetooth SIG behaviour) → a
standalone, pre-TLS, lightweight protobuf-over-RFCOMM exchange using
AASDK's own pinned-revision `aaw` message family
(`WifiStartRequest`/`WifiInfoRequest`/`WifiInfoResponse`/
`WifiStartResponse`/`WifiConnectionStatus`) — proto-only in AASDK itself,
but confirmed as the real wire format via `manio/aa-proxy-rs`'s actual
working Rust source (GPL-2.0-only, cited as operational corroboration
only, not adoptable code) — carrying the actual SSID/password/security-
mode/AP-type fields needed for a phone to join the head unit's Wi-Fi AP →
standard Wi-Fi association (public Wi-Fi Alliance behaviour) → a fresh,
ordinary AAP session over TCP instead of USB bulk transport, where this
project's existing, transport-agnostic `protocol-aap`/`SessionTransport`
work genuinely does apply unmodified. The separate `service.bluetooth`/
`service.wifiprojection` AAP channels found on the first pass are real
and licensed but appear to be for an already-connected session
bootstrapping *future* reconnect, not cold-start discovery — see the
correction note for the full reasoning. What remains genuinely unresolved
is narrower than either pass alone found — see "What remains unresolved"
below.

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

### AASDK `WifiProjection` credential exchange (approved, same pinned revision)

Found on a second research pass, prompted by `manio/aa-proxy-rs`'s
independently-documented bootstrap sequence (below) naming a credential
request/response pair this project's first pass hadn't located. Confirmed
present, verbatim, at the same already-approved AASDK revision
(`9bf6adf933665dee26532201719fac14a047ccf1`):

`WifiProjectionMessageId` (`aap_protobuf.service.wifiprojection`, proto2,
`service/wifiprojection/WifiProjectionMessageId.proto`) — same wire-value
range as every other channel:

| Name | Wire value |
|---|---|
| `WIFI_MESSAGE_CREDENTIALS_REQUEST` | 32769 |
| `WIFI_MESSAGE_CREDENTIALS_RESPONSE` | 32770 |

`WifiCredentialsRequest` (`aap_protobuf.service.wifiprojection.message`,
proto2, `service/wifiprojection/message/WifiCredentialsRequest.proto`) —
empty body, a pure trigger message:

| # | Name | Label | Type |
|---|---|---|---|
| — | (no fields) | | |

`WifiCredentialsResponse` (`aap_protobuf.service.wifiprojection.message`,
proto2, `service/wifiprojection/message/WifiCredentialsResponse.proto`) —
the actual bootstrap payload:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `car_wifi_password` | optional | `string` |
| 2 | `car_wifi_security_mode` | optional | `WifiSecurityMode` |
| 3 | `car_wifi_ssid` | optional | `string` |
| 4 | `supported_wifi_channels` | repeated | `int32` |
| 5 | `access_point_type` | optional | `AccessPointType` |

`WifiSecurityMode` (`service/wifiprojection/message/WifiSecurityMode.proto`):
`UNKNOWN_SECURITY_MODE`(0), `OPEN`(1), `WEP_64`(2), `WEP_128`(3),
`WPA_PERSONAL`(4), `WPA2_PERSONAL`(5), `WPA_WPA2_PERSONAL`(6),
`WPA_ENTERPRISE`(7), `WPA2_ENTERPRISE`(8), `WPA_WPA2_ENTERPRISE`(9).

`AccessPointType` (`service/wifiprojection/message/AccessPointType.proto`):
`STATIC`(0), `DYNAMIC`(1) — presumably head unit AP vs. phone-hosted AP,
unconfirmed which is which from the schema alone.

This is the actual credential-exchange payload the earlier
`WifiProjectionService.car_wifi_bssid` field (a `ServiceDiscoveryResponse`
advertisement only) doesn't itself carry: once this channel is open, the
phone sends an empty `WifiCredentialsRequest` and the head unit answers
with everything needed to join its Wi-Fi network directly. Same
no-per-file-licence-header posture as every other proto file already
recorded above and elsewhere in `aasdk-adoption.md`.

### `manio/aa-proxy-rs` — independent operational corroboration, reference only, not adoptable code

`aa-proxy-rs`'s own README ("How it works (technical)") documents a
complete, real, operational bootstrap sequence: the proxy powers up
Bluetooth, becomes discoverable, and registers two profiles — one for
Android Auto, one for a "fake headset" (this is very likely the concrete
mechanism behind LIVI's own HFP-service-record finding above — a fake
HFP/headset Bluetooth profile is what makes the phone believe a wireless
Android Auto head unit is present). Once the phone connects via the
Android Auto Bluetooth profile, the proxy sends a `WifiStartRequest`
(TCP server IP/port) and receives a `WifiInfoResponse` (access-point
details). The phone then connects over Wi-Fi to the advertised TCP
server, and a normal AAP session proceeds.

**Correction (second research pass, Plan-agent-validated)**: the paragraph
above originally claimed `WifiStartRequest`/`WifiInfoResponse` were "the
same request/response pair as AASDK's `WifiCredentialsRequest`/
`WifiCredentialsResponse` above, just named differently" — **this was
wrong**, caught by reading `aa-proxy-rs`'s actual Rust source
(`src/bluetooth.rs`) rather than trusting the README's paraphrase. Two
separate, real findings:

1. AASDK's own C++ source (`include/aasdk/Channel/Bluetooth/
   BluetoothService.hpp`/`.cpp`, `include/aasdk/Channel/WifiProjection/
   WifiProjectionService.hpp`/`.cpp`, all at the pinned revision) constructs
   `service.bluetooth`/`service.wifiprojection` messages with
   `messenger::EncryptionType::ENCRYPTED` and sends them through the
   standard `Channel`/`Messenger` path — confirming they *are* ordinary
   post-TLS AAP channels, exactly like Video/Input/MediaAudio. But AASDK's
   own `include/aasdk/Transport/` only ever implements `USBTransport` and
   `TCPTransport` — **no Bluetooth/RFCOMM transport exists anywhere in
   AASDK**, and nothing in the pinned tree ever instantiates
   `BluetoothService`/`WifiProjectionService` over one. They're real,
   licensed, but apparently unused-by-AASDK-itself schema, reachable (if
   at all) only from inside an already-negotiated USB/TCP AAP session —
   this matches the original first-pass guess that this channel pair
   serves an *already-connected* session bootstrapping future reconnect,
   not cold-start discovery.
2. The same pinned AASDK tree separately contains a different,
   proto-only, no-`.cpp`-consumer message family under
   `protobuf/aap_protobuf/aaw/`: `MessageId.proto`,
   `WifiStartRequest.proto`, `WifiStartResponse.proto`,
   `WifiInfoRequest.proto`, `WifiInfoResponse.proto`,
   `WifiVersionRequest.proto`, `WifiVersionResponse.proto`,
   `WifiConnectionStatus.proto`. `aa-proxy-rs`'s actual Rust source uses
   exactly these names (`WifiStartRequest`/`WifiInfoRequest`/
   `WifiInfoResponse`/`WifiStartResponse`/`WifiConnectionStatus`/
   `WifiVersionRequest`/`WifiVersionResponse`), confirming this `aaw`
   family — not `service.bluetooth`/`service.wifiprojection` — is the real
   cold-start bootstrap protocol. Its wire framing
   (`aa-proxy-rs/src/bluetooth.rs`) is a bare `[u16 BE length][u16 BE
   message_id][protobuf payload]` header — **not** this project's
   `FrameHeader` (`crates/protocol-aap/src/lib.rs`, channel ID/frame
   type/encryption flags), and involves no TLS/`Messenger`/`Channel` at
   all: a standalone, lightweight, pre-handshake exchange, run directly
   over the raw Bluetooth RFCOMM stream. The real driver sequence
   (`aa-proxy-rs/src/bluetooth.rs`): HU sends `WifiStartRequest{ip_address,
   port}` → reads phone's `WifiInfoRequest` (empty) → HU sends
   `WifiInfoResponse{ssid, password, bssid, security_mode,
   access_point_type}` → reads phone's `WifiStartResponse` → reads phone's
   `WifiConnectionStatus`. Only *after* Wi-Fi association does a fresh,
   ordinary AAP handshake (version negotiation, TLS, `ServiceDiscovery`,
   `ChannelOpen`) begin over TCP — where this project's existing,
   transport-agnostic protocol stack genuinely does apply unmodified.

The `aaw` family is still a legitimate AASDK-pinned-revision source (same
revision, same licence posture as everything else in this document) — it
was simply uncatalogued by the first research pass, which only reviewed
`service/bluetooth` and `service/wifiprojection`. `aa-proxy-rs` remains
GPL-2.0-only reference-only, cited here for confirming which AASDK message
family real implementations actually use, and for the concrete Bluetooth
SDP profile-registration technique (below) — not as an adopted source.

**Licence note, important**: `aa-proxy-rs` declares **GPL-2.0-only** (not
"or later" — confirmed by reading its `LICENSE` file directly). GPL-2.0-only
and this project's GPL-3.0-or-later are not combinable the way
AASDK/OpenAuto/LIVI (all GPL-3.0-or-later) are — this project cannot adopt
`aa-proxy-rs` code or exact message shapes the way it adopts from its three
existing approved sources. It's cited here the same way `BertoldVdb/
WACResearch` and pre-adoption LIVI were once cited elsewhere in this
project's records: as an independently-observed operational fact
corroborating that the AASDK schema above is real and used in a working
implementation, not as an adopted source in its own right. Its own project
also documents no direct source-file references for the Bluetooth pairing
step specifically (README describes the sequence; exact source files
weren't identified by this assessment).

### AASDK `aaw` cold-start bootstrap family (approved, same pinned revision) — the real mechanism

Confirmed present, verbatim, at the same already-approved AASDK revision
(`9bf6adf933665dee26532201719fac14a047ccf1`), under
`protobuf/aap_protobuf/aaw/` (proto-only in AASDK itself — no `.cpp`
consumer in the pinned tree — but confirmed as the real wire format via
`manio/aa-proxy-rs`'s actual Rust source, `src/bluetooth.rs`, read
directly, not just its README). Wire framing is **not** this project's
AAP `FrameHeader` — a bare 4-byte header, big-endian, no encryption, no
channel concept: `[u16 length][u16 message_id][protobuf payload]`.

`MessageId` (`aap_protobuf.aaw`, proto2, `aaw/MessageId.proto`):

| Name | Wire value |
|---|---|
| `WIFI_START_REQUEST` | 1 |
| `WIFI_INFO_REQUEST` | 2 |
| `WIFI_INFO_RESPONSE` | 3 |
| `WIFI_VERSION_REQUEST` | 4 |
| `WIFI_VERSION_RESPONSE` | 5 |
| `WIFI_CONNECTION_STATUS` | 6 |
| `WIFI_START_RESPONSE` | 7 |

`WifiStartRequest` (`aaw/WifiStartRequest.proto`) — sent by the head unit:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `ip_address` | required | `string` |
| 2 | `port` | required | `uint32` |

`WifiInfoRequest` (`aaw/WifiInfoRequest.proto`) — empty, sent by the phone.

`WifiInfoResponse` (`aaw/WifiInfoResponse.proto`) — sent by the head unit,
the actual bootstrap payload:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `ssid` | required | `string` |
| 2 | `password` | required | `string` |
| 3 | `bssid` | required | `string` |
| 4 | `security_mode` | required | `service.wifiprojection.message.WifiSecurityMode` (already mapped above) |
| 5 | `access_point_type` | optional | `service.wifiprojection.message.AccessPointType` (already mapped above) |

`WifiStartResponse` (`aaw/WifiStartResponse.proto`) — sent by the phone:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `ip_address` | optional | `string` |
| 2 | `port` | optional | `uint32` |
| 3 | `status` | required | `Status` (below) |

`WifiConnectionStatus` (`aaw/WifiConnectionStatus.proto`) — sent by the phone:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `status` | required | `Status` (below) |
| 2 | `error_message` | optional | `string` |

`WifiVersionRequest` (`aaw/WifiVersionRequest.proto`) — empty.

`WifiVersionResponse` (`aaw/WifiVersionResponse.proto`) — field names
unconfirmed beyond wire order/type (no semantic names given in the
schema itself):

| # | Name (as declared) | Label | Type |
|---|---|---|---|
| 1 | `unknown_value_a` | required | `uint32` |
| 2 | `unknown_value_b` | required | `uint32` |
| 3 | `unknown_value_c` | optional | `string` |
| 4 | `unknown_value_d` | required | `uint32` |

`Status` (`aaw/Status.proto`):

| Name | Wire value |
|---|---|
| `STATUS_SUCCESS` | 0 |
| `STATUS_UNSOLICITED_MESSAGE` | 1 |
| `STATUS_NO_COMPATIBLE_VERSION` | -1 |
| `STATUS_WIFI_INACCESSIBLE_CHANNEL` | -2 |
| `STATUS_WIFI_INCORRECT_CREDENTIALS` | -3 |
| `STATUS_PROJECTION_ALREADY_STARTED` | -4 |
| `STATUS_WIFI_DISABLED` | -5 |
| `STATUS_WIFI_NOT_YET_STARTED` | -6 |
| `STATUS_INVALID_HOST` | -7 |
| `STATUS_NO_SUPPORTED_WIFI_CHANNELS` | -8 |
| `STATUS_INSTRUCT_USER_TO_CHECK_THE_PHONE` | -9 |
| `STATUS_PHONE_WIFI_DISABLED` | -10 |
| `STATUS_WIFI_NETWORK_UNAVAILABLE` | -11 |

**Bluetooth SDP identity** (`aa-proxy-rs/src/bluetooth.rs`, read directly —
GPL-2.0-only, cited as an operational fact, not adopted code): the
Android-Auto-Wireless Bluetooth profile UUID is
`4de17a00-52cb-11e6-bdf4-0800200c9a66`. Registering a BlueZ SDP service
under this UUID (plus, per LIVI's finding, an HFP/headset-profile service
record) is the real, working mechanism by which a phone recognises a
device as wireless-Android-Auto-capable. One nuance worth recording:
`aa-proxy-rs`'s own function naming (`read_hu_wifi_start_with_prebootstrap_
passthrough`, "hu" = head unit, "passthrough") suggests its typical role
is relaying an existing real head unit's own bootstrap messages to a
phone, not necessarily originating them from scratch the way a
standalone head unit would — this doesn't change the wire-format
citation's validity (a relay still has to correctly parse/construct these
exact messages) but is worth knowing before assuming aa-proxy-rs is a
"reference head-unit implementation" in the fullest sense.

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

- **The very first Bluetooth handshake**: standard OS-level Bluetooth
  pairing/SDP-service discovery to find the registered
  `4de17a00-52cb-11e6-bdf4-0800200c9a66` RFCOMM channel in the first
  place. Almost certainly ordinary, publicly-specified Bluetooth SIG
  behaviour (SDP, RFCOMM channel discovery, the "fake HFP/headset
  profile" trick both LIVI and `aa-proxy-rs` independently point at)
  rather than an Android-Auto-specific protocol needing its own approved
  source — matching how this project already treats Annex-B/S16LE as
  public container-format facts (`certainty-matrix.md`'s media rows).
- **Exact session choreography beyond the message ordering already cited**
  (timeouts, retry behaviour, how failure `Status` values should be
  handled) — `aa-proxy-rs`'s source gives one credible, real-world-
  operational ordering, but as GPL-2.0-only reference material, not an
  adoptable source for exact behaviour, and its passthrough/relay role
  (see the nuance above) means it may not need to handle every case a
  standalone head unit would originate itself.
- Whether the separate `service.bluetooth`/`service.wifiprojection` AAP
  channels (first pass) ever come into play for anything real (e.g.
  advertising `car_wifi_bssid` for reconnect scenarios) remains
  unconfirmed — AASDK itself never uses them, so this is genuinely
  unclear pending real-hardware observation.
- No real-hardware wireless trial of any of this has been attempted yet.

## Candidate route for M7

`aaw/*` (the real bootstrap mechanism) and `service/bluetooth`/
`service/wifiprojection` (real but apparently reconnect-only) are all
viable **candidate paths** for a future AASDK adoption-record expansion
(matching `aasdk-adoption.md`'s existing "Approved candidate scope, only
adopted when added" convention) — no new source-approval decision is
needed to *list* them as candidates, since they're already within the
same pinned, owner-approved AASDK revision. Actually *adopting* specific
behaviour from them (building Rust message types, a Bluetooth RFCOMM
transport, a Wi-Fi-AP-then-TCP bootstrap flow, and the underlying OS-level
Bluetooth SDP/pairing and Wi-Fi AP/association logic) is real
implementation work — see the separate implementation plan for the
minimal proof-of-concept version of this, approved 2026-08-16.
`aa-proxy-rs` cannot be a direct code/behaviour source at all
(GPL-2.0-only) but remains useful *reference* corroboration for that
work.

## References

- AASDK (pinned, approved): https://github.com/opencardev/aasdk/tree/9bf6adf933665dee26532201719fac14a047ccf1/protobuf/aap_protobuf/service/bluetooth
- AASDK (pinned, approved): https://github.com/opencardev/aasdk/tree/9bf6adf933665dee26532201719fac14a047ccf1/protobuf/aap_protobuf/service/wifiprojection
- LIVI (pinned, approved): https://github.com/f-io/LIVI
- LIVI Bluetooth client (reviewed, not adopted): https://github.com/f-io/LIVI/tree/main/src/main/services/projection/bt
- `BertoldVdb/WACResearch` (reviewed, ruled out — unrelated Apple protocol): https://github.com/BertoldVdb/WACResearch
- `manio/aa-proxy-rs` (reviewed, reference only — GPL-2.0-only, not adoptable): https://github.com/manio/aa-proxy-rs
- `manio/aa-proxy-rs` Bluetooth/wire-framing source (reviewed directly): https://github.com/manio/aa-proxy-rs/blob/main/src/bluetooth.rs
- AASDK `aaw` bootstrap family (pinned, approved): https://github.com/opencardev/aasdk/tree/9bf6adf933665dee26532201719fac14a047ccf1/protobuf/aap_protobuf/aaw
- `bluer` crate (BSD-2-Clause, Rust BlueZ bindings): https://docs.rs/bluer
