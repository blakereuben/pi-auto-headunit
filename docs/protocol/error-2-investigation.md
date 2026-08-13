# Real-phone investigation: Android Auto "Error 2" after `ServiceDiscoveryResponse`

## Status: open, unresolved, but the failure boundary keeps moving further into the session with each increment — and has now changed character twice. Three early hypotheses tested and refuted; advertising the full 8-service canonical set, then handling `AudioFocusRequest`, then `KeyBindingRequest` on the Input channel, then `Setup`/`Config`/`Start` on all three `AudioMediaSinkService`-derived channels (`MediaAudio`/`SystemAudio`/`SpeechAudio`), each produced real forward progress — the phone now opens every channel, drives video and all three audio channels through `Setup`→`Config`, and completes a full key-binding round trip. With `SpeechAudio` handled, the probe no longer hit an explicit "unexpected message" rejection at all: it ran every implemented handshake step successfully and then **timed out** waiting for anything further from the phone. A fourth hypothesis — that the phone just needed more time before sending whatever comes next, so the 10s `PROBE_TIMEOUT` was the limiting factor — was tested and refuted: raising it to 30s produced no behavioral difference. A fifth hypothesis — that the phone expects the head unit to proactively send `PingRequest` every 5 seconds as a session-liveness signal (matching `OpenAuto`'s `AndroidAutoEntity`/`Pinger`) — was implemented and tested, but the probe itself hit a new, reproducible local failure (a USB bulk-OUT write timeout) right around when the first ping would be sent, before the hypothesis could be fairly observed; still unresolved. A systematic diff against `OpenAuto`'s full post-`ServiceDiscoveryResponse` session lifecycle (rather than one hypothesis at a time) then surfaced a sixth, better-evidenced hypothesis — this project's `ServiceDiscoveryResponse` had never advertised any sensor capability at all (`Sensors` was encoded as an empty message), so the phone had no way to know it could request `DRIVING_STATUS`/`NIGHT_DATA`, described in the research as eager/near-mandatory session-bring-up behavior. This was implemented and tested twice more: first with `PingRequest` neutralized (clean timeout, `SensorRequest` never arrived), then — after implementing a seventh increment (`NavFocusRequest`/`ByeByeRequest`, below) — with `PingRequest` active at 5s, where a real run reached `SensorRequest`/`SensorResponse`/`SensorBatch` for the **first time ever** before hitting the write timeout again. Neutralizing `PingRequest` a second time reproduced the identical sensor exchange with a clean timeout instead, confirming the write timeout is specifically tied to the ping write rather than incidental USB flakiness. Neither `NavFocusRequest` nor `ByeByeRequest` ever arrived in either clean run, even after the full sensor exchange completed — refuted, or at minimum not yet observed. Diagnosing the ping write-timeout confound directly (at the user's request) refuted two more candidate causes — switching `PingRequest` to TLS-encrypted framing, and raising this project's bulk-OUT write timeout to match AASDK's own reference value (10s) — neither stopped the write from timing out, meaning the phone had already stopped servicing the connection by that point regardless of what was sent next. A direct, targeted comparison against a second, independent, working Android Auto implementation (`f-io/LIVI`, GPL-3.0-or-later, not AASDK-derived) at the user's request then found the actual gap: LIVI sends an unsolicited `VideoFocusNotification` granting video focus immediately after `Config`, something this project had never sent at all despite the message pair already existing in this project's own pinned AASDK schema. **Implementing this produced the first real breakthrough in this entire investigation: the phone sent `Start` on the video channel for the first time ever, and the probe reached its own internal success condition (`probe_result=video_channel_start_received`).** Error 2 still appeared on the phone screen even then — but since this probe deliberately never sends any real video/audio media data after `Start` (out of scope; see the module doc comment), the leading theory now is that Error 2 at this point reflects the complete absence of a following media stream, not a remaining wire-protocol defect — a fundamentally different, more optimistic conclusion than everything tested before it, though not yet confirmed. Error 2 has appeared on screen in every run so far regardless (confirmed on eleven separate real-phone runs total across all increments) — but the failure boundary has now moved past every wire-protocol step this probe implements. A comprehensive, whole-session LIVI audit (at the user's explicit request, replacing the narrow per-message comparisons above) then found two more concrete gaps — no `MEDIA_MESSAGE_ACK` was ever sent despite advertising `Config.max_unacked = 1` on every AV channel, and `KeyBindingResponse` was more strict than LIVI's own unconditional-`OK` behavior. Both implemented and real-hardware tested (thirteenth run): the probe now reaches `Start`, keeps observing for real media data, and completes its entire observation window with **no local error of any kind** — but `Data`/`CodecConfig` still never arrived on the video channel, and Error 2 still appeared. That run's short window (~1-2 real seconds after `Start`) left open whether it was simply too short to be conclusive. A fourteenth run directly tested that: `PROBE_TIMEOUT` raised 10s→30s, same clean-completion result, but this time with the full 30-second window elapsing — still no `Data`/`CodecConfig`, still Error 2. **This refutes the "window too short" hypothesis.** The leading theory is now that the phone simply does not intend to send media data on this session at all, regardless of how long the head unit waits — plausibly because a real head unit needs to already be behaving like a genuine media sink (decoding/rendering, or some other liveness signal) before the phone commits to streaming, not just correctly complete the wire handshake. See "30-second observation window" below.

## Current symptom (most recent real-phone runs)

With `SpeechAudio`'s `Setup`/`Config`/`Start` handling added, two consecutive real-phone runs (fresh MTP replug before each) no longer hit an explicit "unexpected message" rejection — every implemented handshake step now completes successfully, but the phone stops sending anything further and the probe eventually times out. The two runs varied in how much they completed before the phone went quiet, which is itself worth recording:

**Run 1** (`usb auth-discovery-probe --device <bus:address> --allow-live-aap`):

```
probe_negotiated_version=1.7
probe_state=tls_handshake_complete
probe_result=service_discovery_summary_received
probe_state=service_discovery_response_sent
probe_state=video_channel_open
probe_state=simple_channel_open channel_id=2   # Input
probe_state=audio_focus_requested
audio_focus_request_type=Release
probe_state=audio_focus_notification_sent
probe_state=simple_channel_open channel_id=6   # Sensors
probe_state=simple_channel_open channel_id=7   # Bluetooth
probe_state=simple_channel_open channel_id=8   # Microphone
probe_state=media_audio_channel_open
probe_state=system_audio_channel_open
probe_state=speech_audio_channel_open
probe_state=video_channel_setup_config_sent
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

Here, all channels opened and video reached `Config`, but none of the three audio channels' `Setup` ever arrived before the timeout — the phone simply stopped sending anything after the key-binding exchange.

**Run 2** (immediately after a fresh replug, same command):

```
...(identical up through all channels opening)...
probe_state=media_audio_channel_setup_config_sent
probe_state=system_audio_channel_setup_config_sent
probe_state=speech_audio_channel_setup_config_sent
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_state=encrypted_frame_received
probe_state=video_channel_setup_config_sent
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

Here, all three audio channels' `Setup` **did** arrive and were answered with `Config` — each accepted without complaint, using the same shared `AudioSetupStateMachine` unmodified for all three — and video's `Config` was also sent. This is the furthest the probe has ever gotten: every currently-implemented handshake step completed successfully. It still timed out waiting for whatever comes next (most plausibly a `Start` on one of the audio channels, or something on `Sensors`/`Bluetooth`/`Microphone`, none of which have any post-open handling yet).

**Run 3** (diagnostic: `PROBE_TIMEOUT` temporarily raised from 10s to 30s, fresh replug, same command): reached the same point as Run 2 — all four `Config` responses sent (video, `MediaAudio`, `SystemAudio`, `SpeechAudio`) plus the key-binding exchange — and still timed out waiting for anything further, despite three times as long to wait. `PROBE_TIMEOUT` was reverted back to 10s immediately afterward, since the longer value produced no behavioral difference (see "Hypotheses tested against real hardware and refuted" below).

**Confirmed on the phone screen for all three runs: still Error 2** ("Communication error 2 — the phone and the car are running incompatible software"). The run-to-run variation in exactly which messages arrive before the phone goes quiet (not a fixed, reproducible message sequence) is itself a new data point — see "What this rules out, and what it doesn't" below.

## Ping/liveness experiment (most recent, real hardware, inconclusive)

`OpenAuto`'s `AndroidAutoEntity::start()` (`AndroidAutoEntity.cpp`) arms a `Pinger` immediately at session start — before even `VersionRequest` — that proactively sends `PingRequest` every 5 seconds for the life of the session (`AndroidAutoEntityFactory.cpp`'s hard-coded `std::make_shared<Pinger>(ioService_, 5000)`), completely decoupled from handshake/channel-setup progress. This project had never implemented this at all. Given the post-`SpeechAudio` symptom is "the phone goes quiet," a plausible theory is that a real phone treats ping receipt as its own liveness signal, independent of whatever else is happening in the session — which would explain both the timeout and the previously-observed run-to-run variability.

Implemented: `write_int64_field` (`protobuf.rs`, needed since epoch-millisecond timestamps exceed `i32::MAX`), `ControlMessageId::PingRequest`/`PingResponse` (wire values 11/12, confirmed via `ControlMessageType.proto`), a new `crates/protocol-aap/src/ping.rs` (`encode_ping_request`/`decode_ping_response`, matching `OpenAuto`'s own scope exactly — it never sends `PingResponse` or decodes an incoming `PingRequest`), and `PING_INTERVAL`/`send_ping_request`/a `PingResponse` arm in `auth_discovery_probe.rs`. Confirmed directly from the pinned AASDK source (`ControlServiceChannel.cpp`): both ping messages are sent `EncryptionType::PLAIN`, `MessageType::SPECIFIC` — unencrypted, unlike virtually everything else post-handshake — so `send_ping_request` uses the same plain-frame `send_control` helper already used for `VersionRequest`/`EncapsulatedTls`/`AuthComplete`.

Two consecutive real-phone runs (fresh MTP replug before each) both produced the same new, reproducible result — not the previous clean `auth/service-discovery/channel-setup probe timed out before completion`, but a local I/O failure:

```
...(video/MediaAudio/SystemAudio/SpeechAudio all opened, AudioFocusRequest handled, all four channels' Setup/Config exchanged, key binding answered)...
probe_state=speech_audio_channel_setup_config_sent   # Run 1's last successful line
error=transport I/O: USB error: USB transfer timed out
```

```
...(same shape, different config-send ordering)...
probe_state=video_channel_setup_config_sent           # Run 2's last successful line
error=transport I/O: USB error: USB transfer timed out
```

In both runs, the failure landed immediately after the last of the four `Setup`/`Config` exchanges completed — i.e., right around the ~5-second mark, exactly when `PING_INTERVAL` first fires. This is a **new class of failure**: a `TransportError` surfacing from `LibUsbBulkTransport::write_all`'s bulk-OUT write (`CONTROL_TIMEOUT`-style 2-second timeout in `write_all`, `transport-usb/src/linux.rs`), not a clean protocol-level timeout — never observed in any prior run, across four earlier increments, before ping code was added. **Confirmed on the phone screen for both runs: still Error 2.**

This result is **inconclusive for the ping hypothesis itself**: the probe crashes locally before it can be observed whether continued pinging would have changed the phone's behavior. It's not yet established whether the ping write is the literal cause of the USB stall (as opposed to the *next* write after it, or ordinary real-hardware USB flakiness that happens to coincide with this point in the deterministic handshake timeline) — that needs its own targeted investigation before this hypothesis can be fairly tested. The code is left in place (not reverted) since it's a reasonable, `OpenAuto`-matched implementation regardless of the outcome, and the write-timeout finding is itself useful information for whoever picks this up next.

## Systematic `OpenAuto` diff and the Sensors capability-advertisement experiment (most recent, real hardware, refuted)

Rather than continue testing one hypothesis at a time, three parallel research passes audited (1) everything this project's probe currently implements/rejects, (2) `AndroidAutoEntity`'s complete control-channel message lifecycle, and (3) `OpenAuto`'s `SensorService`/`BluetoothService`/`AudioInputService` — the three channels this project opens but has never handled beyond `ChannelOpenState::Open`.

The strongest finding: `service_discovery_response.rs`'s `ServiceKind::Sensors` arm was encoding `Service.sensor_source_service` as an **empty message**, never populating `SensorSourceService.sensors` (the repeated list of supported `SensorType`s). A real phone has no way to request a sensor it was never told exists. `OpenAuto`'s `SensorService::fillFeatures()` (`SensorService.cpp`, pre-approved path per `docs/protocol/openauto-adoption.md`) advertises exactly two: `DRIVING_STATUS` and `NIGHT_DATA`; the research characterized driving-status as eager/near-mandatory — it gates whether the phone shows a full or driving-restricted UI, normally requested unconditionally at session bring-up rather than on user action — a stronger candidate than Bluetooth pairing or microphone capture (both more clearly user/voice-triggered, and also researched this pass). Field numbers were confirmed directly against this project's actually-pinned AASDK fork (`opencardev/aasdk` @ `9bf6adf933665dee26532201719fac14a047ccf1`) rather than assumed from the older fork `OpenAuto`'s C++ behavior was researched against — the two forks have genuinely renamed this exact message set (`SensorStartRequestMessage`→`SensorRequest`, `SensorEventIndicationMessage`→`SensorBatch`).

Implemented: `crates/protocol-aap/src/sensor.rs` (`SensorMessageId`/`SensorType`, `decode_sensor_request`, `encode_sensor_response`, `encode_driving_status_unrestricted_batch`, `encode_night_mode_batch`), a new `SensorCapability` in `service_discovery_response.rs` actually populating `SensorSourceService.sensors` with both types, and a dedicated `SensorsChannel` state machine in `auth_discovery_probe.rs` (graduated out of the generic `simple_channels` pool, matching the precedent already used for `MediaAudio`/`SystemAudio`/`SpeechAudio`) that answers any `SensorRequest` with `SensorResponse{OK}` plus a matching one-shot `SensorBatch`, mirroring `OpenAuto`'s `onSensorStartRequest` exactly.

To isolate this experiment's variable from the still-unresolved Ping write-timeout confound, `PING_INTERVAL` was temporarily raised past `PROBE_TIMEOUT` (so it couldn't fire during this trial) and reverted back to 5s immediately after, matching the `PROBE_TIMEOUT` 10s→30s→10s precedent. One real-phone run (fresh MTP replug):

```
...(video/MediaAudio/SystemAudio/SpeechAudio all opened, AudioFocusRequest handled)...
probe_state=sensors_channel_open
probe_state=encrypted_frame_received
probe_state=simple_channel_open channel_id=7   # Bluetooth
probe_state=simple_channel_open channel_id=8   # Microphone
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_state=encrypted_frame_received
probe_state=video_channel_setup_config_sent
probe_state=media_audio_channel_setup_config_sent
probe_state=system_audio_channel_setup_config_sent
probe_state=speech_audio_channel_setup_config_sent
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

No USB write-timeout this run — confirms neutralizing `PING_INTERVAL` successfully isolated that confound. But the Sensors channel opened cleanly and **no `SensorRequest` ever arrived** — `probe_state=sensor_request_received` never printed. Every other step matched the pre-Sensors baseline exactly (all four `Setup`/`Config` exchanges, key binding, clean timeout). **Confirmed on the phone screen: still Error 2.** This refutes the hypothesis: the phone's silence isn't explained by a missing sensor-capability advertisement — even now that it's told the head unit supports `DRIVING_STATUS`/`NIGHT_DATA`, it never asks for either. The `SensorsChannel`/`sensor.rs` code is left in place (a more complete, independently-correct `ServiceDiscoveryResponse` regardless of outcome, matching the disposition of every previously-refuted-but-kept increment).

## `NavFocusRequest`/`ByeByeRequest` experiment (most recent, real hardware) — and the sensor exchange finally happening

The same systematic `OpenAuto` diff (above) named two more control-channel gaps, using the older `f1xpl/aasdk` fork's naming from the C++ behavior research: `NavigationFocusRequest`/`Response` and `ShutdownRequest`/`Response`. Fetching this project's actually-pinned AASDK fork directly (`opencardev/aasdk` @ `9bf6adf933665dee26532201719fac14a047ccf1`, `ControlMessageType.proto`) found the pinned fork uses genuinely different names for the same wire values: `MESSAGE_NAV_FOCUS_REQUEST = 13`, `MESSAGE_NAV_FOCUS_NOTIFICATION = 14`, `MESSAGE_BYEBYE_REQUEST = 15`, `MESSAGE_BYEBYE_RESPONSE = 16` — `NAV_FOCUS` not `NAVIGATION_FOCUS`, `BYEBYE` not `SHUTDOWN`. Implemented `crates/protocol-aap/src/nav_focus.rs` (`NavFocusRequestNotification` decode, always answering a hardcoded `Projected` via `NavFocusNotification`, matching `OpenAuto`'s own hardcoded reply since this project has no native navigation either) and `crates/protocol-aap/src/byebye.rs` (`ByeByeRequest.reason` decode, answering an empty `ByeByeResponse`). `ByeByeRequest`'s `reason` enum (`USER_SELECTION`/`DEVICE_SWITCH`/`NOT_SUPPORTED`/`NOT_CURRENTLY_SUPPORTED`/`PROBE_SUPPORTED`) was the motivating lead — the protocol's own explicit session-end signal, potentially diagnostic about *why* the phone considers this head unit incompatible if it's ever sent. Receiving `ByeByeRequest` is now treated as a clean, non-error probe stop (`ProbeOutcome::PhoneEndedSession`), not a timeout or a protocol error.

First real-phone run (`PING_INTERVAL` active at 5s, as normal) produced new behavior never seen before:

```
...(all channels open, AudioFocusRequest handled, key binding, all four Setup/Config sent)...
probe_state=sensor_request_received
sensor_request_type=DrivingStatusData
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_state=encrypted_frame_received
probe_state=sensor_request_received
sensor_request_type=NightMode
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
error=transport I/O: USB error: USB transfer timed out
```

The phone finally sent `SensorRequest` for both advertised sensor types — the Sensors experiment's own real-hardware trial (above) never observed this. Both were answered correctly. The run then hit the same USB bulk-OUT write timeout the original Ping trial produced, before it could be observed whether `NavFocusRequest`/`ByeByeRequest` would follow. **Confirmed on the phone screen: still Error 2.**

To get a clean read, `PING_INTERVAL` was neutralized again (same technique as the Sensors trial) and the probe rebuilt/retested on a fresh replug:

```
...(identical up through both SensorRequest/SensorResponse/SensorBatch exchanges)...
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

Clean timeout, no write-timeout — reproducing the sensor exchange exactly, but this time neither `NavFocusRequest` nor `ByeByeRequest` ever arrived (`probe_state=nav_focus_requested`/`probe_state=byebye_requested` never printed) before the 10s deadline. **Confirmed on the phone screen: still Error 2.**

Two things follow from these two runs together. First, **the write-timeout confound is now strongly implicated as caused by the ping write itself**, not incidental USB flakiness that happens to coincide with session timing — reproduced present-with-ping/absent-without-ping twice now (once for Sensors, once here), the cleanest signal yet. Second, `NavFocusRequest`/`ByeByeRequest` are refuted, or at minimum not yet observed — the phone still goes quiet after completing the sensor exchange, one step further into the session than any prior run reached, with the same "nothing left to react to" symptom shape. `nav_focus.rs`/`byebye.rs` are kept in the code regardless (independently correct protocol coverage).

## Ping write-timeout diagnosis (real hardware, both attempts refuted)

At the user's explicit request, the ping write-timeout was diagnosed directly rather than continuing to work around it. A bracketing diagnostic print (`probe_state=ping_request_send_attempt`, immediately before the send call) was added first, to remove any doubt about which write was stalling.

**Attempt 1: switch `PingRequest` from plain to TLS-encrypted framing.** This project's `send_ping_request` had always sent `PingRequest` unencrypted, matching AASDK's own `sendPingRequest` (`EncryptionType::PLAIN`) — the only post-handshake message this probe ever sent that way. Theory: a real `1.7`-speaking client might expect an unbroken all-encrypted bytestream and desync on a stray plain frame badly enough to stop draining its own USB OUT buffer. Tested with `PingRequest` active as normal: the write **still** timed out, and `probe_state=ping_request_send_attempt` was again the last printed line — confirming definitively (not just by timing correlation) that it's the ping write itself that stalls, but refuting the encryption-domain theory. **Confirmed on the phone screen: still Error 2.**

**Attempt 2: raise the bulk-OUT write timeout to match AASDK's own reference value.** Deep research into `OpenAuto`/AASDK's actual send-path mechanics (`Pinger.cpp`, `AndroidAutoEntity::sendPing`/`schedulePing`, and the full `ServiceChannel`→`Messenger`→`Transport`→`USBTransport`→`USBEndpoint` send chain) found the ping is not prioritized, delayed, or specially scheduled anywhere in that reference implementation — it enters the exact same generic FIFO send queue as every other control message. But it also found `include/f1x/aasdk/Transport/USBTransport.hpp`'s `cSendTimeoutMs = 10000` — a **10-second** per-transfer libusb write timeout, used for literally every bulk-OUT transfer, versus this project's own **2-second** timeout for the same operation (`crates/transport-usb/src/linux.rs`). Raised to match (`BULK_SEND_TIMEOUT = 10s`) and tested with `PingRequest` active: the write **still** timed out (again confirmed via the bracketing print, still the last line). Since even 5x more time didn't help, this rules out "the phone was just briefly slow" — the write genuinely never completes at all. **Confirmed on the phone screen: still Error 2.**

A separate research pass into `f-io/LIVI` (a second, independent, GPL-3.0-or-later Android Auto implementation, not AASDK-derived, done at the user's request specifically to look at how a real working implementation handles this) found its own bulk-OUT writes are serialized through a single promise chain (`UsbAoapBridge.ts`'s `_outChain`) so no two writes are ever issued concurrently — but this doesn't apply to this project's own architecture, since `run()`'s loop is already single-threaded and fully synchronous (every `send_all()` call already blocks to completion before the next is issued). No adoptable fix came from this angle.

Both refutations, taken together with what came next (see below), now support a different reading: the write never completing regardless of content or timeout length is consistent with the **phone having already stopped servicing the USB connection by that point**, independent of what this probe tries to send next — not a bug in how the ping is sent at all.

## `VideoFocusNotification` breakthrough (real hardware — first time this probe has ever reached `Start`)

Every real-hardware run through this point in the investigation — regardless of which increment was under test — stalled at the identical spot: `Config` sent on every channel, `Start` never received on any of them. At the user's explicit direction ("just use livi/openauto to solve this"), this pass did a focused, deep comparison against `f-io/LIVI` specifically targeting the Setup→Config→Start sequence, since that's exactly where every run gets stuck.

`Config`'s own field values turned out to be **identical** to what this project already sends (`status`=`OK`/`READY`=2, `max_unacked`=1, `configuration_indices`=`[0]`) — not the gap. But LIVI's `Session.ts` sends an extra, unsolicited message on the video channel that this project had never sent in any form: immediately after `Config`, before ever expecting `Start`, it proactively sends `VideoFocusNotification { focus: VIDEO_FOCUS_PROJECTED }` — granting the phone video focus without being asked. LIVI's own source comment calls this a "keyframe-request" that prompts the phone to actually begin encoding/streaming. Confirmed directly against this project's own pinned AASDK source (not just LIVI's independent reimplementation): `protobuf/aap_protobuf/service/media/sink/MediaMessageId.proto` already defines `MEDIA_MESSAGE_VIDEO_FOCUS_REQUEST = 32775`/`MEDIA_MESSAGE_VIDEO_FOCUS_NOTIFICATION = 32776` in the same per-video-channel message-ID space this project's `Setup`/`Start`/`Config` already use — a real, pre-existing part of the protocol this project simply never implemented, not something invented from LIVI alone.

Implemented (`crates/protocol-aap/src/video_setup.rs`'s `encode_video_focus_notification`, sent as a second `SendMedia` action right after `Config` in the same `handle_setup` response) and tested on real hardware:

```
...(all channels open, AudioFocusRequest handled)...
probe_state=video_channel_setup_config_sent
probe_state=video_channel_video_focus_notification_sent
probe_state=encrypted_frame_received
probe_state=media_audio_channel_setup_config_sent
probe_state=encrypted_frame_received
probe_state=system_audio_channel_setup_config_sent
probe_state=encrypted_frame_received
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_state=encrypted_frame_received
probe_state=speech_audio_channel_setup_config_sent
probe_state=encrypted_frame_received
probe_state=sensor_request_received
sensor_request_type=DrivingStatusData
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_state=encrypted_frame_received
probe_state=sensor_request_received
sensor_request_type=NightMode
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_state=encrypted_frame_received
probe_state=video_channel_start_received
video_channel_session_id=0
video_channel_configuration_index=0
probe_result=video_channel_start_received
probe_stop=video_channel_start_received_ready_for_media_data
```

**The phone sent `Start` on the video channel for the first time in this entire investigation.** The probe exited cleanly (exit code 0) at its own internal success condition, no error, no timeout. This is the deepest into the session this probe has ever reached, past every wire-protocol step it implements.

**Confirmed on the phone screen: still Error 2.** Reaching `Start` did not resolve it. But this changes what Error 2 most plausibly means at this point: this probe deliberately never sends any real video or audio media data after `Start` — no `MEDIA_MESSAGE_DATA`, no actual H.264 stream, no GStreamer/render pipeline (explicit, deliberate scope boundary since this probe's inception; the actual encode/render work is separate M3 scope, not yet built). A real phone that receives `Start` almost certainly expects an actual video stream to begin arriving shortly afterward — its own internal timeout/expectation for that, with nothing ever following, is now the leading (but unconfirmed) explanation for why Error 2 still appears even here. This would mean the wire-protocol handshake this probe tests is now fully correct through `Start`, and the remaining gap is real media delivery, not a further undiscovered handshake step. This is a materially different, more optimistic situation than everything tested earlier in this document, where the failure was somewhere in the handshake itself.

## Comprehensive LIVI audit, `MEDIA_MESSAGE_ACK`, and `KeyBindingResponse` (real hardware — clean run, still no `Data`)

At the user's explicit direction — after repeated real-hardware runs kept finding one gap at a time — this pass replaced narrow, single-message comparisons with one comprehensive audit of `f-io/LIVI`'s **entire** session lifecycle: every channel, every message, from connection through active streaming (not just the video Setup/Config/Start sequence the earlier `VideoFocusNotification` comparison targeted). Full findings recorded in conversation history; two concrete, byte-level-confirmed discrepancies came out of it:

1. **No `MEDIA_MESSAGE_ACK` was ever sent.** LIVI acks *every single* `Data`/`CodecConfig` frame, unconditionally, on *every* AV channel — video and all three audio sinks, byte-identical code in `VideoChannel.ts`/`AudioChannel.ts`. Its own comment: *"ACK every frame to avoid phone triggering `CAR_NOT_RESPONDING` (>400 unacked)."* This project advertises `Config.max_unacked = 1` on every channel and had never implemented `Ack` at all — with that combination, the phone could send at most one frame before flow control blocked it. `MEDIA_MESSAGE_ACK = 32772` was already confirmed present in this project's own pinned AASDK schema before this pass (same `MediaMessageId` space as `Setup`/`Start`/`Config`), and the `Ack` message body (`session_id`/`ack`/`receive_timestamp_ns`) was already found under `service/media/source/message/Ack.proto` — a real, pre-existing part of the protocol, not a LIVI invention. Implemented in `crates/protocol-aap/src/media_message.rs` (`encode_media_ack`/`decode_media_data`, shared by video and all three audio channels — the only two truly channel-agnostic helpers in this codebase's otherwise deliberately-duplicated-per-channel state machines) and wired into `video_setup.rs`/`audio_setup.rs`'s `Ready` state: every `Data`/`CodecConfig` now gets an unconditional `Ack` echoing the channel's own `session_id`.
2. **`KeyBindingResponse` was over-strict.** LIVI replies `status=OK` unconditionally, regardless of which keycodes were requested. This project previously validated against its own advertised (empty) keycode capability list, replying `KeycodeNotBound` for any non-empty request — matching `OpenAuto`'s older, stricter behavior. Every real request observed so far has been empty, so this had never mattered in practice, but it's now a confirmed-wrong deviation from a working reference. `evaluate_key_binding_request` now always answers `Success`.

Everything else the audit covered — video-focus timing (already correct), audio channels' lack of any proactive send beyond the ack, sensors/Bluetooth/microphone/navigation (all purely reactive, matching what LIVI itself does) — was confirmed to already match, not gaps. Ping cadence (LIVI: 1500ms, advertised in `ServiceDiscoveryResponse.connectionConfiguration.pingConfiguration`, which this project has never populated) was a lower-confidence finding, deliberately left out of this batch.

Real-hardware result, `PING_INTERVAL` neutralized (same technique as prior trials, to keep this batch's result uncontaminated by the still-unresolved ping write-timeout confound):

```
...(all channels open, AudioFocusRequest handled, video Config/VideoFocusNotification sent)...
probe_state=key_binding_requested
key_binding_requested_count=0
probe_state=key_binding_response_sent
key_binding_response_status=Success
probe_state=encrypted_frame_received
probe_state=media_audio_channel_setup_config_sent
probe_state=encrypted_frame_received
probe_state=system_audio_channel_setup_config_sent
probe_state=encrypted_frame_received
probe_state=sensor_request_received
sensor_request_type=DrivingStatusData
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_state=encrypted_frame_received
probe_state=speech_audio_channel_setup_config_sent
probe_state=encrypted_frame_received
probe_state=video_channel_start_received
video_channel_session_id=0
video_channel_configuration_index=0
probe_state=channel_setup_complete
probe_state=observing_for_post_start_media_traffic
probe_state=encrypted_frame_received
probe_state=sensor_request_received
sensor_request_type=NightMode
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_result=observation_window_complete
```

This is the **cleanest run in this entire investigation** — no local error of any kind, exit code 0. `Start` was reached, the observation window opened, a second `SensorRequest` (`NightMode`) arrived and was handled correctly *during* that window, proving the window itself was live and working — but no `video_media_data_received`/`video_media_codec_config_received` ever printed, and the window closed via the natural `PROBE_TIMEOUT` deadline. **Confirmed on the phone screen: still Error 2.**

This result doesn't refute the ack/`Data` hypothesis outright — the observation window here was short (`Start` happened late in an already-`PROBE_TIMEOUT`-constrained 10-second run; only enough real time remained for one more sensor round trip before the deadline). It's equally consistent with "the phone would have sent `Data` given a longer window" and "the phone never intends to send `Data` at all while everything else about this session looks the way it does to the phone." Distinguishing those needs a longer observation window — a minimal, reversible experiment (raising `PROBE_TIMEOUT`) directly analogous to the one already proven-safe earlier in this investigation, not yet run for this specific question.

## 30-second observation window (real hardware — refutes "window too short")

Direct follow-up to the previous section's open question. `PROBE_TIMEOUT` raised from 10s to 30s (a minimal, reversible change directly analogous to the earlier, already-proven-safe 10s→30s handshake-timeout experiment); `PING_INTERVAL` neutralized again for this trial, since the write-timeout confound recurred on the very first attempt at the new timeout (the phone had re-entered AOA accessory mode from the previous run and needed a fresh replug regardless).

Real-hardware result:

```
...(all channels open, AudioFocusRequest handled, video Config/VideoFocusNotification sent, KeyBindingResponse=Success)...
probe_state=sensor_request_received
sensor_request_type=DrivingStatusData
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_state=encrypted_frame_received
probe_state=video_channel_start_received
video_channel_session_id=0
video_channel_configuration_index=0
probe_state=channel_setup_complete
probe_state=observing_for_post_start_media_traffic
probe_state=encrypted_frame_received
probe_state=sensor_request_received
sensor_request_type=NightMode
probe_state=sensor_response_sent
probe_state=sensor_batch_sent
probe_result=observation_window_complete
```

Exit code 0, no local error. The full 30-second window elapsed (not just the ~1-2s available in the 10s-budget run above); only one more `SensorRequest` (`NightMode`) arrived during it, nothing else — no `video_media_data_received`/`video_media_codec_config_received` ever printed. **Confirmed on the phone screen: still Error 2**, on this, the **fourteenth** real-phone run.

This refutes the "observation window was simply too short" hypothesis (item 10 in "Hypotheses tested against real hardware and refuted", below) — a 30-second window, mostly spent doing nothing after the second sensor exchange, is not a plausibly-too-short amount of time for a phone that intends to start streaming video to have done so. The leading theory is now the other branch from the previous section's either/or: **the phone does not intend to send media data on this session at all**, independent of how long the head unit waits — plausibly because Error 2 (or whatever internal state produces it) is decided before or independent of the video stream actually starting, and a real head unit would need to already be behaving like a genuine sink (e.g. actually decoding/rendering, or maintaining some other liveness the phone checks for) before the phone commits further. This is unconfirmed and is now the single best-motivated next research question — see "Suggested next steps" below.

## What changed and why it mattered

The prior three hypotheses (below) each produced **zero change** in behavior — the phone rejected `ServiceDiscoveryResponse` identically every time, which was itself a strong hint that the problem was structural rather than a single missing field. Externally-sourced research (AASDK/OpenAuto/LIVI all construct a complete, fixed 8-service `ServiceDiscoveryResponse` unconditionally, not a curated subset) was checked directly against this project's already-approved `OpenAuto` primary source (`ServiceFactory.cpp`) and confirmed: `ServiceFactory::create()` unconditionally constructs 7 of 8 canonical services, with the 8th config-gated but on by default.

Implementing the full canonical set (`Video`, `Input`, `MediaAudio`, `SystemAudio`, `SpeechAudio`, `Sensors`, `Bluetooth`, `Microphone` — `crates/protocol-aap/src/service_discovery_response.rs`, generic `ChannelOpenStateMachine` reused per channel in `auth_discovery_probe.rs`) was the first change in this entire investigation that altered real-phone behavior: the phone stopped rejecting `ServiceDiscoveryResponse` and instead proceeded into the session, first sending a previously-unseen encrypted control message (`AudioFocusRequest`, control ID 18). Implementing and answering that (`crates/protocol-aap/src/audio_focus.rs`, `AudioFocusNotification` granting exactly what's asked — see that file's doc comments for full wire provenance against the pinned AASDK source) unblocked the phone further still: it proceeded to open all 8 channels and drive the video channel through `Setup`→`Config`, then send `KeyBindingRequest` on the Input channel.

Implementing and answering that too (`crates/protocol-aap/src/input_message.rs`, `KeyBindingRequest`/`KeyBindingResponse` — see that file's doc comments for full wire provenance; policy in `handle_input_channel_message`/`evaluate_key_binding_request` in `auth_discovery_probe.rs`, mirroring `OpenAuto`'s `InputService::onBindingRequest` validation against the head unit's own advertised keycode list) unblocked the phone yet again: the empty request was answered `Success` and accepted, and the session proceeded to a new message — `Setup` — on the `MediaAudio` channel.

Implementing that too (`crates/protocol-aap/src/audio_setup.rs`, a deliberate duplicate of `video_setup.rs` rather than a shared generic state machine, to avoid touching the real-hardware-proven video code path — see that file's doc comments for full wire provenance) unblocked the phone yet again: the `Setup` (requesting `MEDIA_CODEC_AUDIO_PCM`) was answered with `Config` and accepted, and the session proceeded to a new message on the `SystemAudio` channel.

`SystemAudio` also turned out to be a thin `AudioMediaSinkService` subclass in AASDK, advertising the same single-PCM-configuration shape this project already advertises for it — so no new crate-level code was needed at all; `AudioSetupStateMachine` was reused unmodified, with only a parallel `SystemAudioChannel` enum/`handle_system_audio_channel_message` dispatch added in `auth_discovery_probe.rs` (mirroring the `MediaAudio` wiring exactly, purely additive — the just-proven `MediaAudio` code was never touched). This unblocked the phone yet again: `SystemAudio`'s `Setup` was answered with `Config` and accepted, and the session proceeded to a new message on the `SpeechAudio` channel.

`SpeechAudio` (AASDK's `GuidanceAudioChannel`, matching `AudioStreamType::Guidance`) turned out to be the third and final thin `AudioMediaSinkService` subclass, advertising the same single-PCM-configuration shape again — so once more, no new crate-level code was needed; `AudioSetupStateMachine` was reused unmodified for a third channel, with only a third parallel `SpeechAudioChannel` enum/`handle_speech_audio_channel_message` dispatch added, purely additive. This is where the pattern changed: with all three audio channels' `Setup`/`Config`/`Start` handling in place, the probe no longer hits an explicit "unexpected message" rejection at all on a subsequent real-phone run — it runs every implemented handshake step to completion and then times out. Error 2 still appears on screen regardless (confirmed on two separate runs), but the failure signature is now "the phone goes quiet" rather than "the phone sends something new we reject."

## Original symptom (earlier in this investigation, now superseded)

Before the full-service-catalogue change, the probe reached only as far as:

```
probe_result=service_discovery_summary_received
probe_state=service_discovery_response_sent
probe_tls_state=SSL negotiation finished successfully
error=protocol probe: auth/service-discovery/channel-setup probe timed out before completion
```

with Error 2 on screen immediately after `ServiceDiscoveryResponse` was sent, and no `ChannelOpenRequest` ever arriving. The three hypotheses below were tested against *this* earlier boundary.

## Confirmed facts (from our own real-phone runs, not speculation)

- The phone negotiates and accepts **protocol version 1.7** (`HandshakeStateMachine::negotiated_version()`, surfaced via `probe_negotiated_version=<major>.<minor>` in `auth_discovery_probe.rs`). Version negotiation itself succeeds regardless of what we offer (tested offering both `1.6` and `1.7` — see below).
- **No publicly known AASDK fork documents or implements protocol 1.7.** Checked directly (`Version.hpp`'s `AASDK_MAJOR`/`AASDK_MINOR` constants): our pinned `opencardev/aasdk` @ `9bf6adf933665dee26532201719fac14a047ccf1` (`1.6`); `opencardev/aasdk`'s own `newdev` branch HEAD as of this investigation, pushed as recently as 2026-06-11 (still `1.6` — the most recently active fork checked, and it hasn't caught up either); `openDsh/aasdk` `develop` (`1.1`); `f1xpl/aasdk` (no `Version.hpp` found at the checked path); `n8ohu/aasdk`, `Spooky998/aasdk`, `arash-rasouli/aasdk` (all stale since 2018–2023, not independently re-checked for this constant beyond confirming they're inactive). This looks like a genuine, currently-undocumented-in-open-source gap between what current real Android Auto expects and what any community reverse-engineering effort has reached.

## Hypotheses tested against real hardware and refuted

Each was a deliberately minimal, independently reversible change, verified with the full `cargo fmt`/`check`/`clippy`/`test` sweep before each real-phone run. The first three still ship in the current code (see "What's still in place" below) since each represents a more complete, independently-defensible `ServiceDiscoveryResponse` regardless of whether it fixed Error 2; the version-number and probe-timeout experiments were both reverted since neither changed anything and there was no independent reason to keep them.

1. **Missing audio service.** Added a `ServiceKind::MediaAudio` channel (`protocol_aap::service_discovery_response::AudioCapability`/`AudioStreamType`, `MediaSinkService.audio_type`/`audio_configs`) alongside the existing video/touch channels. **No change in outcome.**
2. **Stale protocol version number.** Temporarily changed `AASDK_PROTOCOL_VERSION` from the pinned source's `1.6` to `1.7` (matching what the phone reports it wants), to test whether a bare version-number mismatch was the trigger. **No change in outcome** — reverted back to `1.6` (the pinned, documented value) once refuted, since Android Auto's version negotiation is designed to be backward-compatible and a real phone accepting-but-rejecting on version number alone was never a well-motivated theory to begin with; the experiment confirmed that reasoning rather than contradicting it.
3. **Missing head-unit identity.** Populated `ServiceDiscoveryResponse.headunit_info` (field 17, `HeadUnitInfo` — `protocol_aap::service_discovery_response::HeadUnitInfo`) with fixed, non-sensitive project-identifying strings (project name, `CARGO_PKG_VERSION`, etc. — never real vehicle/user data). Theory: Android Auto's app-level validation might reject a head unit that never identifies itself, distinct from the wire schema (which marks every `HeadUnitInfo` field `optional`). **No change in outcome.**
4. **Probe timeout too short.** Once every currently-implemented handshake step started succeeding (after the `SpeechAudio` increment), the probe began timing out instead of being rejected on a new message. Temporarily raised `PROBE_TIMEOUT` (`auth_discovery_probe.rs`) from 10s to 30s, to test whether the phone just needed more time before sending whatever comes next. **No change in outcome** — the same handshake steps completed and it still timed out waiting for more, just with three times as long to wait. Reverted back to 10s once refuted.
5. **Missing sensor-capability advertisement.** `ServiceDiscoveryResponse.Sensors` had never advertised any supported `SensorType`, so a real phone had no way to know it could request `DRIVING_STATUS`/`NIGHT_DATA` — described in research as eager/near-mandatory session-bring-up behavior in `OpenAuto`'s reference implementation (see "Systematic `OpenAuto` diff" above). Implemented `SensorCapability`/`sensor.rs`/`SensorsChannel`, advertising and handling both types. **No change in outcome** on the first (neutralized-ping) trial — the phone opened the Sensors channel but never sent a `SensorRequest`. A later trial (with ping active, then again with ping neutralized — see item 6 below and the `NavFocusRequest`/`ByeByeRequest` section above) *did* observe the phone sending `SensorRequest` for both types, correctly answered — still **no change in Error 2 outcome**. Kept in the code (a more complete, independently-correct `ServiceDiscoveryResponse` regardless).
6. **Missing `NavFocusRequest`/`ByeByeRequest` handling.** Two more control-channel gaps named by the same systematic `OpenAuto` diff. Implemented `nav_focus.rs`/`byebye.rs`, answering `NavFocusRequest` with a hardcoded `Projected` and `ByeByeRequest` with an empty response (treating receipt as a clean probe stop, not an error). **No change in outcome** — neither message ever arrived across two real-phone runs (one with the write-timeout confound, one clean), even after the phone completed the full sensor exchange for the first time. Kept in the code regardless.
7. **Ping write-timeout: plain vs. encrypted framing.** Switched `PingRequest` from `Encryption::Plain` (matching AASDK's own `sendPingRequest`) to TLS-encrypted, on the theory a real `1.7` client might desync on a stray plain frame mid-session. **No change in outcome** — the write still timed out, confirmed (via a bracketing diagnostic print) to still be the ping write itself. Left encrypted (independently reasonable, matches every other post-handshake message) even though it didn't fix the timeout.
8. **Ping write-timeout: transport timeout too short.** Raised `crates/transport-usb/src/linux.rs`'s bulk-OUT write timeout from 2s to 10s, matching AASDK's own reference value (`USBTransport.hpp`'s `cSendTimeoutMs = 10000`) exactly. **No change in outcome** — the write still timed out even given 5x more time, ruling out "the phone was just briefly slow." Left at 10s (matches the reference implementation regardless).
9. **Missing unsolicited `VideoFocusNotification` after video `Config`.** Found via direct comparison against a second, independent, working Android Auto implementation (`f-io/LIVI`) rather than the AASDK/`OpenAuto` sources everything else in this document is sourced from. Implemented `video_setup.rs`'s `encode_video_focus_notification`, sent as a second action right after `Config`. **This is not "no change in outcome" like every item above** — the phone sent `Start` on the video channel for the first time in this entire investigation, and the probe reached its own internal success condition. Error 2 still appeared on the phone screen regardless — see the `VideoFocusNotification` section above for why that's now understood differently (likely missing real media data, not a further handshake gap). Kept in the code — this is the most load-bearing single change in this document's history.
10. **Missing `MEDIA_MESSAGE_ACK`.** Found via a comprehensive, whole-session `f-io/LIVI` audit — this project advertised `Config.max_unacked = 1` on every AV channel but never sent an `Ack`, so the phone could send at most one unacknowledged frame. Implemented `encode_media_ack`/`decode_media_data` (shared, `media_message.rs`), wired into every AV channel's `Open` state. Kept in the code (independently correct regardless of the outcome below).
11. **Over-strict `KeyBindingResponse`.** Same audit — LIVI answers unconditionally `Success`; this project validated against its own (empty) advertised keycode list. Fixed. No real request has ever been non-empty, so this has not yet been observed to change anything, but it's a confirmed correction.
12. **Observation window too short to see `Data`.** After (10)/(11), a real-hardware run reached `Start`, ran its full observation window, and completed with no local error at all — but only ~1-2 real seconds elapsed post-`Start` before the (then-10s) `PROBE_TIMEOUT` fired. Raising `PROBE_TIMEOUT` to 30s (a minimal, reversible change directly analogous to the earlier 10s→30s handshake-timeout experiment) and re-running produced the same clean completion, this time with the full 30-second window elapsing — still no `Data`/`CodecConfig` ever arrived. **Refuted**: whatever is stopping the phone from streaming, it isn't a head unit that gives up too soon. See "30-second observation window" above.

The fifth hypothesis (proactive `PingRequest`, "Ping/liveness experiment" above) remains **inconclusive, not refuted** — the probe crashed locally (a USB bulk-OUT write timeout) before it could be fairly tested in its own right, and two direct attempts to fix that write timeout (items 7 and 8 above) both failed to resolve it. That local failure needed to be worked around (temporarily neutralizing `PING_INTERVAL`) to test the Sensors, `NavFocusRequest`/`ByeByeRequest`, and `VideoFocusNotification` hypotheses cleanly (the `VideoFocusNotification` breakthrough run above notably had `PingRequest` active and completed without hitting the write timeout at all, since the session ended — via `Start`'s success condition — before the 5s ping interval fired). The write timeout's root *cause* is still unresolved; it now looks most consistent with the phone itself having already stopped servicing the USB connection at that point in earlier runs, for reasons unrelated to ping's own content or timing.

## What this rules out, and what it doesn't

Ruled out: a simple missing-channel gap; a simple version-number-field mismatch; a simple missing-identity gap; a too-short probe timeout (30s produced no change over 10s at the handshake level, and separately, a 30s post-`Start` observation window produced no `Data` either — the phone appears to genuinely stop sending anything relevant once it decides to fail, not just pause); a missing sensor-capability advertisement; missing `NavFocusRequest`/`ByeByeRequest` handling; the ping write-timeout being caused by plain-vs-encrypted framing or too-short a transport timeout; a missing `MEDIA_MESSAGE_ACK`/over-strict `KeyBindingResponse` being what's blocking media data (both fixed, no change in outcome); a too-short post-`Start` observation window (30s produced the same result as ~1-2s); and, most significantly, **a missing wire-protocol handshake step through `Start`** — every step through video `Start` (the deepest point Android Auto's wire protocol requires before real media begins) is now real-hardware-confirmed correct, including the previously-undiscovered unsolicited `VideoFocusNotification`.

**Not ruled out**, in roughly descending order of how well-motivated they are:

- **The phone never intends to send real media data on this session at all** — now the leading theory. This project has never sent a single byte of actual video/audio stream data, deliberately out of scope since this probe's inception (separate M3 encode/render pipeline work that doesn't exist yet). With the window-length and flow-control (ack) explanations both ruled out, a real phone plausibly needs to see the head unit *also* behaving like a real sink (decoding/rendering, or at least accepting and doing something with a sustained stream) before it will commit to the session, not just correctly ack the protocol handshake. Testing this needs the head unit to actually send/accept real (or synthetic-but-decodable) media data — see "Suggested next steps".
- **A genuine 1.7 schema change** elsewhere in the protocol that this project has no source for. Weakened as a leading theory now that `Start` has been reached correctly and the ack/keybinding gaps are closed, but not eliminated for whatever comes after `Data` starts flowing (if it ever does).
- **A framing/encryption detail specific to a real phone's TLS stack** that the project's own real-TLS integration tests (`encrypted_service_discovery.rs`, `full_channel_setup.rs`) can't catch, since those exercise the same Rust encrypt/decrypt code on both sides of a controlled fake-phone harness rather than a genuinely independent TLS implementation.
- Something else entirely not yet considered.

**New observation, not yet well-motivated as a hypothesis:** the three most recent real-phone runs (all after every currently-implemented handshake step succeeds) did *not* reproduce the same message sequence — Run 1's phone stopped sending anything right after the key-binding exchange (no audio `Setup` ever arrived), while Runs 2 and 3 sent all three audio channels' `Setup` and video's `Config` before going quiet. All three ended in Error 2 regardless, and Run 3 confirms the phone isn't simply pausing before continuing (30s produced the same outcome as 10s). This run-to-run variability could mean Android Auto itself is retrying/backing off internally before declaring incompatibility (rather than following one fixed message sequence), or it could be ordinary session jitter unrelated to the eventual failure. Not enough data yet to tell which.

## Research leads already checked (see conversation history for exact queries/results)

- `opencardev/aasdk` PR #2 ("Fix compatibility with latest Android Auto", merged 2019-08-07) — already included in our pinned revision; fixed `VideoFocusRequest`/`touch_event.action_index` parsing issues, not anything we've hit.
- `opencardev/crankshaft` issue #5 ("Error 2 with several phones") — no technical root cause recorded, just "goes away with official OpenAuto on X11."
- `rsjudka/intelligent-auto` issue #40 ("Android Auto won't connect with latest AA") — reports the same class of failure across every AASDK-based implementation checked (crankshaft, intelligent-auto), while the one implementation that doesn't hit it (OpenAuto Pro) is the more complete, commercial one. No technical diagnosis in the issue thread itself.
- One additional source of uncertain provenance was consulted during troubleshooting, for understanding only, per explicit project-owner instruction not to name, cite, quote, or otherwise reflect it in this repository. It did not resolve the question it was consulted for and contributed nothing citable to this record.

## What's still in place in the code (not reverted)

- All 8 canonical services (`Video`, `Input`, `MediaAudio`, `SystemAudio`, `SpeechAudio`, `Sensors`, `Bluetooth`, `Microphone`) advertised in `ServiceDiscoveryResponse` (`crates/protocol-aap/src/service_discovery_response.rs`), each driven to `ChannelOpenState::Open` via `ChannelOpenStateMachine` in `auth_discovery_probe.rs`. `Video`, `MediaAudio`, `SystemAudio`, and `SpeechAudio` go further still (`Setup`→`Config`→`Start` via `VideoSetupStateMachine`/`AudioSetupStateMachine`), `Input` handles one post-open message (`KeyBindingRequest`), and `Sensors` advertises `DRIVING_STATUS`/`NIGHT_DATA` and handles `SensorRequest` (`crates/protocol-aap/src/sensor.rs`, `SensorCapability`, `SensorsChannel`/`handle_sensors_channel_message` in `auth_discovery_probe.rs`) — real-hardware-confirmed working (both types requested and answered) once the `PingRequest` write-timeout confound was isolated, see the `NavFocusRequest`/`ByeByeRequest` section above. `Bluetooth`/`Microphone` are the only channels left with no post-open handshake (researched and ranked lower-eagerness than Sensors — see "Systematic `OpenAuto` diff" above).
- Unsolicited `VideoFocusNotification` sent on the video channel immediately after `Config` (`crates/protocol-aap/src/video_setup.rs`'s `encode_video_focus_notification`, sent as a second `SendMedia` action from `VideoSetupStateMachine::handle_setup`) — real-hardware-confirmed to be what finally got the phone to send `Start` on the video channel, the deepest point this probe has ever reached. See the `VideoFocusNotification` breakthrough section above. This is the most load-bearing single change in this document's history.
- `NavFocusRequest`/`NavFocusNotification` and `ByeByeRequest`/`ByeByeResponse` handling on the control channel (`crates/protocol-aap/src/nav_focus.rs`, `crates/protocol-aap/src/byebye.rs`; the `ControlMessageId::NavFocusRequest`/`ByeByeRequest` arms in `handle_post_discovery_control_message`, `auth_discovery_probe.rs`) — `NavFocusRequest` always answered `Projected` (no native navigation to contest focus with); `ByeByeRequest` answered with an empty response and treated as a clean probe stop (`ProbeOutcome::PhoneEndedSession`), not an error. Neither has arrived in any real-phone run yet.
- `AudioFocusRequest`/`AudioFocusNotification` handling (`crates/protocol-aap/src/audio_focus.rs`, `handle_post_discovery_control_message`/`grant_audio_focus` in `auth_discovery_probe.rs`) — grants exactly what's requested (placeholder policy, no real audio-focus arbitration exists yet).
- `KeyBindingRequest`/`KeyBindingResponse` handling on the Input channel (`crates/protocol-aap/src/input_message.rs`, `handle_input_channel_message`/`evaluate_key_binding_request` in `auth_discovery_probe.rs`) — `evaluate_key_binding_request` now unconditionally returns `Success`, regardless of requested keycodes, matching `f-io/LIVI`'s own unconditional-`OK` behavior (found via the comprehensive LIVI audit; previously validated against this project's own empty advertised keycode list, replying `KeycodeNotBound` for any non-empty request — a stricter, `OpenAuto`-matching policy confirmed wrong relative to a working modern client). Real-hardware-confirmed only for the empty-request case observed so far (the policy change itself has not yet been observed to alter behavior, since no real request has been non-empty).
- `Ack` on every AV sink channel (video, `MediaAudio`, `SystemAudio`, `SpeechAudio`) — `MediaMessageId::Ack` (wire `32772`) plus the shared `decode_media_data`/`encode_media_ack` helpers (`crates/protocol-aap/src/media_message.rs`), wired into `VideoSetupStateMachine`/`AudioSetupStateMachine`'s `Ready`-state `handle_media` (`video_setup.rs`/`audio_setup.rs`): every `Data`/`CodecConfig` received now gets an unconditional `Ack` reply (`session_id` echoed from that channel's `Start`, `ack` always `1`), matching `f-io/LIVI`'s `_sendAck()` and closing the gap against this project's own advertised `Config.max_unacked = 1`. `VideoChannel`/`MediaAudioChannel`/`SystemAudioChannel`/`SpeechAudioChannel` (`auth_discovery_probe.rs`) now keep their state machine alive in an `Open` variant once `Ready` (previously a bare `Ready` variant discarded it), so post-`Start` `Data`/`CodecConfig`/`Ack` traffic keeps flowing to it. Real-hardware-tested twice (10s and 30s observation windows), no local error either time; not yet observed to actually receive a `Data`/`CodecConfig` frame from the phone (see "What this rules out, and what it doesn't" above).
- `PROBE_TIMEOUT = 30s` (`auth_discovery_probe.rs`, raised from 10s) — gives a longer post-`Start` media-observation window; real-hardware-confirmed the full 30s elapses cleanly with no local error, and that this rules out "window too short" as an explanation for the missing `Data` (see "30-second observation window" above). Kept at 30s going forward.
- `Setup`/`Config`/`Start` handling on the `MediaAudio`, `SystemAudio`, and `SpeechAudio` channels (`crates/protocol-aap/src/audio_setup.rs`'s single `AudioSetupStateMachine`, reused unmodified for all three; `handle_media_audio_channel_message`/`handle_system_audio_channel_message`/`handle_speech_audio_channel_message` in `auth_discovery_probe.rs`) — accepts only `MEDIA_CODEC_AUDIO_PCM`, matching the single uncompressed `AudioConfiguration` this project advertises for each. Real-hardware-confirmed on all three channels (across two runs — see "Current symptom" above): each `Setup` requested PCM, each `Config` response was accepted.
- `HeadUnitInfo` (`service_discovery_response.rs`), populated with fixed project-identifying strings in `auth_discovery_probe.rs`.
- `probe_negotiated_version=<major>.<minor>` diagnostic print in `process_actions` (`auth_discovery_probe.rs`) — cheap, permanent, useful visibility into what the phone actually negotiates.
- `AASDK_PROTOCOL_VERSION` is back to the pinned source's `1.6` (the `1.7` experiment was reverted).
- Proactive `PingRequest` every `PING_INTERVAL` and `PingResponse` handling (`crates/protocol-aap/src/ping.rs`, `PING_INTERVAL`/`send_ping_request`/the `ControlMessageId::PingResponse` arm in `auth_discovery_probe.rs`) — matches `OpenAuto`'s own scope (never handles an incoming `PingRequest`), now sent TLS-encrypted (deviating from AASDK's own plain-framing behavior, an experiment that didn't resolve the write timeout but is independently reasonable). **`PING_INTERVAL` is currently neutralized (`3600s`, effectively disabled) rather than its normal `5s`** — on a fresh, cleanly-replugged session, the write timeout recurred yet again right as the first ping would be sent (the 30s-`PROBE_TIMEOUT` trial's first attempt), confirming it's still unresolved and not something that "stopped happening" just because a prior session had completed successfully. Needs reverting to `5s` (and re-diagnosing) before ping-liveness behavior itself can be considered proven in a session that also reaches `Start`/observes media.
- `BULK_SEND_TIMEOUT = 10s` for bulk-OUT USB writes (`crates/transport-usb/src/linux.rs`), raised from 2s to match AASDK's own reference value exactly (`USBTransport.hpp`'s `cSendTimeoutMs = 10000`). Didn't resolve the ping write timeout, but matches the reference implementation and is kept regardless.

## Suggested next steps for whoever picks this up

1. **Immediate next question: does sending real (or even minimal/synthetic) media data after `Start` change anything?** With window-length and flow-control (ack)/key-binding explanations all now ruled out (see "What this rules out, and what it doesn't" above), the leading theory is that the phone needs the head unit to actually behave like a media sink — send/accept a real stream — before it will commit to the session. This is a materially larger scope step than any prior increment: actually sending `MEDIA_MESSAGE_DATA` on the video channel (at minimum a single valid H.264 frame — doesn't need to be a real camera/screen feed to test the hypothesis, just *something* decodable), now that `Ack` handling exists to support it. This is real M3 scope (encode/render pipeline), not a small protocol tweak like every prior increment — plan it as such.
2. If (1) doesn't resolve Error 2 either, the same LIVI-comparison approach that found `VideoFocusNotification`, `MEDIA_MESSAGE_ACK`, and `KeyBindingResponse` should be repeated for whatever comes after `Start`/first media frame in LIVI's own `Session.ts`/related files — that file is now a proven, higher-signal reference than the 2018-era AASDK/`OpenAuto` sources for finding gaps a real modern phone actually cares about.
3. Diagnose the still-unresolved USB bulk-OUT write timeout (see "Ping write-timeout diagnosis" above) before re-enabling `PingRequest` (currently neutralized at `PING_INTERVAL = 3600s`, see "What's still in place" above) — it has now recurred on a fresh, cleanly-replugged session that never got the chance to complete anything first, ruling out "it only happens on already-broken sessions" as an explanation. Two direct fix attempts (encrypted framing, matching AASDK's 10s timeout) have both failed to resolve it; needs a genuinely new hypothesis, not a repeat of either.
4. Given the run-to-run variability observed across this investigation (see "What this rules out, and what it doesn't"), don't over-index on a single run's exact message sequence — repeat any experiment at least twice before drawing conclusions, as this session did for the `SpeechAudio`, Ping, Sensors, `NavFocusRequest`/`ByeByeRequest`, and observation-window real-hardware tests.
5. Consider whether a packet capture against a **known-working** current head unit (if one becomes available) could reveal the complete expected message sequence for media data/acks specifically, rather than continuing to discover it one real-phone run at a time.
6. Re-check whether any AASDK fork has been updated since this investigation (2026-08-14) — the ecosystem may catch up.
7. If continuing to experiment against real hardware, keep changes as minimal and independently reversible as every increment so far, and keep running the full verification sweep before each real-phone cycle.
