# M2 Session Boundary: Approved Sources, Bounds, Timeouts, Cancellation, and Logging

## Scope

Narrowly scoped to the current M2 boundary: transport connection through receiving and parsing the phone's `ServiceDiscoveryRequest` into a bounded summary. It stops there deliberately, matching `crates/protocol-aap/tests/fake_phone_transport.rs`, `apps/aa-headunit-diagnostics/src/live_probe.rs` (frozen, stops before authentication), and `apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs` (stops after the service-discovery summary, before any response or media setup). `ServiceDiscoveryResponse` encoding, media channel setup, and wireless are out of scope here and remain separately gated per `docs/protocol/aasdk-adoption.md`.

This is a consolidation record, not new protocol research: it cross-references `certainty-matrix.md`, `aasdk-adoption.md`, and `openauto-adoption.md` for provenance rather than repeating them, and catalogues bounds/timeouts/cancellation/logging that already exist in code but were not previously recorded in one place.

## 1. Approved-source coverage through this boundary

`docs/protocol/certainty-matrix.md` already records, per protocol operation, its certainty level, approved source, and implementation status. Cross-checking every behaviour actually exercised on this path against that matrix:

| Behaviour on this path | Rust location | Certainty-matrix row |
|---|---|---|
| Frame header/flags/short-extended sizes/`0x4000` payload limit | `crates/protocol-aap/src/lib.rs` | "Two-byte AAP frame header, flags, short/extended big-endian lengths, and `0x4000` frame limit" |
| Per-channel first/middle/last/bulk reassembly | `crates/protocol-aap/src/assembly.rs` | "Per-channel first/middle/last/bulk message reassembly" |
| Control envelope, version 1.6 negotiation, encapsulated TLS, successful auth response, transition to service discovery | `crates/protocol-aap/src/control.rs` | "Control envelope, version 1.6 negotiation, encapsulated TLS flow, successful authentication response, and transition to service discovery" |
| Replaceable OpenSSL TLS client, bounded memory transport, injected credentials | `crates/security-openssl/src/linux.rs` | "Replaceable OpenSSL client using bounded memory transport and injected credentials" |
| Service-discovery request field parsing (bounded summary only) | `crates/protocol-aap/src/service_discovery.rs` | "Service-discovery request fields" |
| Internal service catalogue construction and readiness filtering | `crates/protocol-aap/src/service_catalogue.rs` | "Internal service catalogue and readiness filtering" |
| USB AOA transition used to reach the transport | `crates/transport-usb/src/linux.rs` | AOA `Get`/`Send`/`Start` request rows |
| Development ADB-forwarded TCP 5277 transport | `crates/transport-tcp/src/lib.rs` | "Development DHU can reach the phone head-unit server through ADB-forwarded TCP port 5277" / "Official developer-mode ADB-forwarded TCP 5277 transport" |

Every behaviour exercised through this boundary already has a recorded approved source and a "Yes"/implemented status in the certainty matrix — **no gap was found**. Two supporting pieces on this path are original project code with no external adoption claim, so no "approved source" applies: `credential-store`'s offline credential validation/loading, and the diagnostics CLI's `println!`-based state logging (catalogued in §5 below on its own terms).

Behaviour beyond this boundary — `ServiceDiscoveryResponse` encoding, the eight unmapped nested service schemas, media, wireless — remains explicitly PX/gated per the certainty matrix and `aasdk-adoption.md`, and is intentionally not addressed here.

## 2. Bounded message sizes

Every bound below is enforced by a returning `Result::Err`, not a panic or silent truncation (see `crates/protocol-aap/tests/property_fuzz.rs` for adversarial-input coverage). Values are the `Default` impl unless noted.

| Bound | Value | Defined at | Enforced by |
|---|---|---|---|
| Frame payload | 16 KiB (`0x4000`) | `lib.rs::AASDK_MAX_FRAME_PAYLOAD_SIZE` | `decode_frame`/`encode_frame` |
| Reassembled message | 8 MiB | `lib.rs::DEFAULT_MAX_MESSAGE_SIZE` — an independent project-chosen bound, not cited to AASDK (AASDK's own source specifies only the per-frame limit above) | `decode_frame`'s first-frame total-size check |
| Concurrent reassembly channels | 1 | `MessageAssembler::new(1)` at every M2-boundary call site (`live_probe.rs`, `auth_discovery_probe.rs`, `main.rs`, `fake_phone_transport.rs`) | `MessageAssembler::push` |
| Control message body | 1 MiB | `control.rs::DEFAULT_MAX_CONTROL_BODY_SIZE` | `ControlMessage::decode`/`encode` |
| TLS chunk (per encapsulated-TLS control message) | 64 KiB | `control.rs::DEFAULT_MAX_TLS_CHUNK_SIZE` | `HandshakeStateMachine` |
| Service-discovery message total | 1 MiB | `service_discovery.rs::DEFAULT_MAX_SERVICE_DISCOVERY_SIZE` | `summarize_service_discovery_request` |
| Each icon field (small/medium/large) | 256 KiB | `service_discovery.rs::DEFAULT_MAX_DISCOVERY_ICON_SIZE` | same |
| Each text field (`label_text`, `device_name`) | 4 KiB | `service_discovery.rs::DEFAULT_MAX_DISCOVERY_TEXT_SIZE` | same |
| Nested `phone_info` field | 64 KiB | `service_discovery.rs::DEFAULT_MAX_PHONE_INFO_SIZE` | same |
| Probe's own accumulated-but-undecoded receive buffer | 64 KiB | `MAX_ACCUMULATED_BYTES` in `live_probe.rs`/`auth_discovery_probe.rs` | probe read loop; independent of, and tighter than, the protocol-level message bound above |
| Service catalogue candidate count (local construction, not phone input) | 32 | `service_catalogue.rs::DEFAULT_MAX_SERVICE_CANDIDATES` | `ServiceCatalogue::build` |

## 3. Timeouts

| Timeout | Value | Where | Behaviour on expiry |
|---|---|---|---|
| Overall probe wall-clock deadline | 10 s | `PROBE_TIMEOUT` in `live_probe.rs` and `auth_discovery_probe.rs` (and the same-named USB AOA-transition constant in `main.rs`) | Loop exits, returns `CliError::Protocol("... timed out ...")`; no partial state left running |
| TCP connect | 2 s | `developer_auth_discovery_probe`/`developer_credential_probe` in `main.rs` | `TcpStream::connect_timeout` fails closed |
| TCP per-read/write I/O | 500 ms | same call sites, `DeveloperTcpTransport::connect` | `receive` returns `TransportError::TimedOut`, mapped by the probe loop to `continue`, re-checking the overall deadline |
| USB control transfer (AOA `Get`/`Send`/`Start` requests) | 2 s | `CONTROL_TIMEOUT` in `transport-usb/src/linux.rs` | `AoaError`, surfaced to the caller |
| USB bulk read (`SessionTransport::receive`) | 500 ms | `LibUsbBulkTransport::receive`, `transport-usb/src/linux.rs` | Returns `Ok(0)`, **not** `TransportError::TimedOut` — see note below |
| USB bulk write (`SessionTransport::send_all`) | 2 s | same file | `AoaError` on failure, surfaced to the caller |

**Observed asymmetry, relevant to reading probe output over USB:** the TCP transport's `receive` maps an I/O timeout to `TransportError::TimedOut`, which `auth_discovery_probe::run`'s loop explicitly matches and turns into `continue`. The USB transport's `receive` instead maps a `rusb::Error::Timeout` to `Ok(0)` (`transport-usb/src/linux.rs:174`). The probe loop still reaches the same effective outcome — a 0-byte read leaves `received` unchanged, `decode_frame` reports `Incomplete`, the inner loop `break`s, and the outer `while Instant::now() < deadline` loop tries again — but through the `Ok(size) => size` arm rather than the explicit `Err(TransportError::TimedOut) => continue` arm. This is existing, already-tested behaviour (`transport-usb`'s own unit tests cover this timeout mapping); it is not changed here, just recorded as a bound worth knowing when reading probe output against the real USB-connected phone.

## 4. Cancellation

There is no cooperative cancellation token anywhere on this path today. `ARCHITECTURE.md` §6's "one cancellation token tree" describes the future `app` orchestration layer, which does not exist yet — it should not be read as describing current CLI-probe behaviour. What's actually implemented, proven by the code:

- **A wall-clock deadline, not an external cancel signal.** Both probes loop on `Instant::now() < deadline` with short-timeout blocking reads (500 ms), checking back roughly twice a second. There is no way to cancel a run early from outside the process — no signal handler, no `ctrlc`-equivalent dependency anywhere in the workspace.
- **Ctrl-C/SIGINT falls through to default Rust/OS process termination**; there is no custom handler, so cleanup on that path is whatever `Drop` impls provide, not an explicit cancellation-aware shutdown.
- **USB**: `LibUsbBulkTransport` has an explicit `Drop` impl (`transport-usb/src/linux.rs:200`) that releases the claimed USB interface on both a normal return and unwind-driven drop.
- **TCP**: `DeveloperTcpTransport` relies on `std::net::TcpStream`'s default `Drop`, which closes the socket; no custom cleanup.
- **TLS**: `OpenSslTlsClient` is dropped with the probe's stack frame; no explicit close-notify is sent mid-probe (the probe's only exit paths today are: summary received, deadline reached, or a protocol/transport error — none send a TLS shutdown).

For this boundary — short-lived, operator-invoked diagnostic probes, not the long-running appliance session — a hard deadline plus `Drop`-based resource release is what's implemented and tested. It is not equivalent to the cooperative, tree-wide cancellation `ARCHITECTURE.md` specifies for the eventual `app` layer, and that gap is recorded here as open rather than presented as closed.

## 5. Privacy-safe logging

`protocol-aap` itself performs no logging (zero `println!`/`log::` call sites in `crates/protocol-aap/src`). All logging on this path happens at the CLI boundary in `apps/aa-headunit-diagnostics`, as one named `key=value` state marker per line, never structured phone content:

- `live_probe.rs` and `auth_discovery_probe.rs` both open with `probe_scope=...`, `probe_credentials=user_supplied_runtime`, `probe_tls_policy=...`, and `probe_payload_logging=disabled` — an explicit, printed declaration that payload logging is off, not just an absence of it.
- State transitions are logged by name only: `probe_state=version_request_sent`, `probe_state=version_accepted`, `probe_state=tls_peer_data_received`, `probe_state=tls_handshake_complete` — never the bytes that caused the transition.
- `auth_discovery_probe.rs`'s result prints only what `ServiceDiscoveryRequestSummary` exposes: `service_discovery_{small_icon,medium_icon,large_icon,label_text,device_name,phone_info}_bytes` (byte counts, `"absent"` if unset) and `service_discovery_unknown_fields` (a count). No field content is ever read into a loggable string — `service_discovery.rs`'s parser discards field bytes immediately after recording `.len()`.
- An unexpected-message rejection logs `unexpected_message_channel_id`, `_encryption`, `_type`, and `_payload_bytes` — structural metadata and a length, never payload bytes.
- `probe_stop=before_service_discovery_response_and_media_setup` / `probe_stop=before_authentication_and_service_discovery` are printed at each probe's respective stopping point, making the boundary itself part of the logged, reviewable output.

This matches `openauto-adoption.md`'s permanent exclusion "Logging of phone addresses, device names, brands, navigation data, user content, or raw protocol payloads" and the project's instruction to log no raw payloads.
