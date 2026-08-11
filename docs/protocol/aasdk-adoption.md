# AASDK Adoption Record

## Decision

The project owner approved AASDK as a GPLv3-compatible source for Android Auto Protocol behaviour on 4 August 2026. OpenAuto was separately approved under a stricter adoption record on 5 August 2026. Protocol work uses Rust by default and preserves attribution wherever behaviour or definitions are derived.

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

### Encrypted-message framing: total-size domain and the `frameLength - 29` heuristic

Re-reading `src/Messenger/MessageOutStream.cpp` and `src/Messenger/MessageInStream.cpp` at the pinned revision for the post-handshake TLS application-data work confirms the first frame's total-size field (`FrameSize`'s extended `totalSize_`) is always `message_->getPayload().size()` — the full **plaintext** message size, computed once from the original unencrypted payload and passed unchanged to every frame's `setFrameSize` call (`MessageOutStream.cpp:118`), never a running ciphertext total. Each individual frame's own (always-present) short size field is `payloadSize`, the byte count actually written to the wire for *that* frame — the ciphertext length for that one frame when encrypted, taken from `cryptor_->encrypt()`'s return value (`MessageOutStream.cpp:111-112`, `122-127`). `MessageInStream.cpp` never reads or enforces the parsed total-size field at all: completion is detected purely from the frame-type flag (`BULK`/`LAST`), confirming the note above that the Rust `MessageAssembler`'s declared-size bound checking is a Rust-only addition beyond upstream, not a mirrored upstream check.

Consequence for the Rust implementation: for an encrypted message, `MessageAssembler`'s `declared_size` (sourced from `DecodedFrame::total_message_size`) is in the plaintext domain. Each encrypted frame's payload must be TLS-decrypted before being handed to `MessageAssembler::push`, so accumulated bytes stay in the same (plaintext) domain as the declared total; pushing raw ciphertext would spuriously exceed the declared size before all frames arrive, since ciphertext-per-frame is always somewhat larger than the plaintext it encodes.

`src/Messenger/Cryptor.cpp:149-151` is also where the pinned revision computes `int overhead = 29; int length = frameLength - overhead;` inside `Cryptor::decrypt`. Reading the rest of that function (`Cryptor.cpp:152-188`) confirms this exists only as an initial buffer-size hint for looping `sslRead` calls, not as the source of truth for the actual decrypted length — that is `totalReadSize`, whatever `sslRead` actually returns across the loop, independently re-checked each iteration via `getAvailableBytes`. It is a cipher-suite-specific (AES-GCM record overhead) estimate and is not safe to reuse generally; any Rust decrypt path must derive plaintext length only from what the TLS library actually returns, never from this constant.

**Consequence found while implementing the above** (2026-08-11): the Rust frame codec's existing `TotalSmallerThanFrame` check (`crates/protocol-aap/src/lib.rs`, both `decode_frame` and `encode_frame`) compared the declared total directly against the current frame's on-wire length, unconditionally. That invariant only holds when both values are in the same domain, which is true for plain frames (the frame payload is a literal subset of the plaintext total) but not for encrypted frames — per the finding above, the wire frame length is ciphertext while the declared total is plaintext, and TLS per-record overhead (the same ~29-byte figure `Cryptor.cpp` uses as a heuristic) means a small trailing chunk's ciphertext can exceed the declared plaintext total. This was previously untested because no existing test drove `encode_frame`/`decode_frame` with real encrypted, multi-frame-shaped byte counts; it surfaced when adding an integration test (`crates/protocol-aap/tests/encrypted_service_discovery.rs`) that fragments a small `ServiceDiscoveryRequest` across two encrypted frames. The check is now skipped for `Encryption::Encrypted` frames (both encode and decode paths) and remains fully enforced, unchanged, for `Encryption::Plain` frames; see `tests::allows_ciphertext_larger_than_declared_plaintext_total_only_when_encrypted` in `lib.rs` for the regression coverage on both branches.

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

Derived facts cover protocol version 1.6, two-byte big-endian control-message identifiers, the version request/response layout, TLS records encapsulated as control message 3, the successful proto2 authentication response, transition to a received service-discovery request, and the six optional length-delimited fields of `ServiceDiscoveryRequest.proto`. The negotiated version returned by the phone is retained instead of assuming it matches the offered version.

The control-state test uses fake TLS bytes. The separate `security-openssl` crate reproduces AASDK's OpenSSL client/memory-buffer boundary with injected credentials and bounded input/output, but it does not copy AASDK's embedded certificate/private-key material. Bounded live probes with runtime-generated credentials reached version/TLS exchange and were rejected with Android Auto error 7; live generated-identity commands are now permanently locked out.

The Rust service-discovery parser uses the AASDK schema only for field numbers and wire types. It adds strict per-field and total bounds, validates text as UTF-8, does not decode the nested phone-info message, and discards all field content after recording byte counts. The accompanying event transition is attributed separately to OpenAuto.

`protobuf/aap_protobuf/service/control/message/ServiceDiscoveryResponse.proto` and `protobuf/aap_protobuf/service/Service.proto` were reviewed for the next response slice but are not yet adopted into Rust wire encoding. Their service representation differs materially from the older AASDK schema used by pinned OpenAuto. Response encoding remains gated until every advertised service's current nested schema, required fields, identifiers, limits, and source notices are recorded here. The field-by-field mapping for the service kinds the current catalogue models is now recorded in "Adopted `Service` response schema mapping" below; the remaining nested service types and their leaf enum/config messages are explicitly listed there as not yet mapped.

The shared AASDK certificate identifies organisations named Google Automotive Link and JVC Kenwood and is paired with a publicly distributed private key. GPL compatibility does not authorise this independent project to present that identity. The repository does not contain it, and the credential is permanently excluded by `tls-credential-policy.md` and the OpenAuto adoption decision.

## Adopted `Service` response schema mapping

This is a schema-mapping and provenance record only. No `Service` or `ServiceDiscoveryResponse` Rust wire encoder is added by this record; response encoding remains gated per the paragraph above and the M2 checklist.

### Pinned source and scope

Field numbers, labels, and types below are read directly from the same pinned AASDK revision (`9bf6adf933665dee26532201719fac14a047ccf1`, repository https://github.com/opencardev/aasdk, branch `newdev`) used elsewhere in this document:

- `protobuf/aap_protobuf/service/Service.proto`
- `protobuf/aap_protobuf/service/control/message/ServiceDiscoveryResponse.proto`
- `protobuf/aap_protobuf/service/sensorsource/SensorSourceService.proto`
- `protobuf/aap_protobuf/service/media/sink/MediaSinkService.proto`
- `protobuf/aap_protobuf/service/inputsource/InputSourceService.proto` (including its nested `TouchScreen`/`TouchPad` messages)
- `protobuf/aap_protobuf/service/media/source/MediaSourceService.proto`
- `protobuf/aap_protobuf/service/bluetooth/BluetoothService.proto`
- `protobuf/aap_protobuf/service/radio/RadioService.proto` (including its referenced `RadioProperties.proto`)
- `protobuf/aap_protobuf/service/navigationstatus/NavigationStatusService.proto`
- `protobuf/aap_protobuf/service/mediaplayback/MediaPlaybackStatusService.proto`
- `protobuf/aap_protobuf/service/phonestatus/PhoneStatusService.proto`
- `protobuf/aap_protobuf/service/mediabrowser/MediaBrowserService.proto`
- `protobuf/aap_protobuf/service/vendorextension/VendorExtensionService.proto`
- `protobuf/aap_protobuf/service/genericnotification/GenericNotificationService.proto`
- `protobuf/aap_protobuf/service/wifiprojection/WifiProjectionService.proto`
- `protobuf/aap_protobuf/service/control/message/DriverPosition.proto`
- `protobuf/aap_protobuf/service/control/message/ConnectionConfiguration.proto`
- `protobuf/aap_protobuf/service/control/message/HeadUnitInfo.proto`
- `protobuf/aap_protobuf/service/control/message/PingConfiguration.proto`
- `protobuf/aap_protobuf/service/control/message/WirelessTcpConfiguration.proto`

The first five nested per-kind messages were selected because they are the only ones with a corresponding `ServiceKind` in `crates/protocol-aap/src/service_catalogue.rs` today (`Sensors`, `Video`/`MediaAudio`/`SpeechAudio`/`SystemAudio`, `Input`, `Microphone`, `Bluetooth`). The remaining eight — `radio_service`, `navigation_status_service`, `media_playback_service`, `phone_status_service`, `media_browser_service`, `vendor_extension_service`, `generic_notification_service`, `wifi_projection_service` — have no corresponding `ServiceKind` yet; they are mapped below purely as a provenance record, ahead of any catalogue or encoder work that would consume them. None of these proto files carry a per-file licence/copyright header at the pinned revision — the same posture already recorded above for `ServiceDiscoveryRequest.proto` and the other already-adopted proto files in this document, which rely on the repository-wide GPL-3.0-or-later notices found in the adopted `.hpp`/`.cpp` files rather than a per-proto notice.

**Not yet mapped**: `UiConfig` (referenced by `VideoConfiguration`, itself referencing `UiTheme` and `Insets`), deliberately left unmapped as the next recursion boundary — the same one-hop-of-context policy already applied elsewhere in this section (e.g. mapping `SensorType` because it was `Sensor`'s sole field, or `PingConfiguration`/`WirelessTcpConfiguration` below because they are `ConnectionConfiguration`'s only two fields, without chasing what those in turn reference). These must each be mapped and recorded here before any encoder reads or writes them.

### `Service` (`aap_protobuf.service.Service`, proto2)

| # | Name | Label | Type | Notes |
|---|---|---|---|---|
| 1 | `id` | required | `int32` | channel identifier |
| 2 | `sensor_source_service` | optional | `sensorsource.SensorSourceService` | mapped below |
| 3 | `media_sink_service` | optional | `media.sink.MediaSinkService` | mapped below |
| 4 | `input_source_service` | optional | `inputsource.InputSourceService` | mapped below |
| 5 | `media_source_service` | optional | `media.source.MediaSourceService` | mapped below |
| 6 | `bluetooth_service` | optional | `bluetooth.BluetoothService` | mapped below |
| 7 | `radio_service` | optional | `radio.RadioService` | mapped below |
| 8 | `navigation_status_service` | optional | `navigationstatus.NavigationStatusService` | mapped below |
| 9 | `media_playback_service` | optional | `mediaplayback.MediaPlaybackStatusService` | mapped below |
| 10 | `phone_status_service` | optional | `phonestatus.PhoneStatusService` | mapped below |
| 11 | `media_browser_service` | optional | `mediabrowser.MediaBrowserService` | mapped below |
| 12 | `vendor_extension_service` | optional | `vendorextension.VendorExtensionService` | mapped below |
| 13 | `generic_notification_service` | optional | `genericnotification.GenericNotificationService` | mapped below |
| 14 | `wifi_projection_service` | optional | `wifiprojection.WifiProjectionService` | mapped below |

Every field after `id` is wire type 2 (length-delimited/embedded message); `id` is wire type 0 (varint). At most one service-type field is expected populated per `Service` instance in observed upstream usage, but the schema does not itself enforce that as a `oneof`.

### `ServiceDiscoveryResponse` (`aap_protobuf.service.control.message.ServiceDiscoveryResponse`, proto2)

| # | Name | Label | Type | Notes |
|---|---|---|---|---|
| 1 | `channels` | repeated | `service.Service` | the mapped catalogue, above |
| 2 | `make` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 3 | `model` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 4 | `year` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 5 | `vehicle_id` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 6 | `driver_position` | optional | `DriverPosition` | mapped below |
| 7 | `head_unit_make` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 8 | `head_unit_model` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 9 | `head_unit_software_build` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 10 | `head_unit_software_version` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 11 | `can_play_native_media_during_vr` | optional | `bool` | `[deprecated = true]` upstream; excluded |
| 13 | `session_configuration` | optional | `int32` | not yet mapped; field 12 does not exist upstream |
| 14 | `display_name` | optional | `string` | not yet mapped |
| 15 | `probe_for_support` | optional | `bool` | not yet mapped |
| 16 | `connection_configuration` | optional | `ConnectionConfiguration` | mapped below |
| 17 | `headunit_info` | optional | `HeadUnitInfo` | mapped below |

Field 12 is genuinely absent upstream (the sequence skips from 11 to 13); this is not an omission in this record.

### `SensorSourceService` (`aap_protobuf.service.sensorsource.SensorSourceService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `sensors` | repeated | `message.Sensor` (mapped below) |
| 2 | `location_characterization` | optional | `uint32` |
| 3 | `supported_fuel_types` | repeated | `message.FuelType` enum (mapped below) |
| 4 | `supported_ev_connector_types` | repeated | `message.EvConnectorType` enum (mapped below) |

### `MediaSinkService` (`aap_protobuf.service.media.sink.MediaSinkService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `available_type` | optional | `shared.message.MediaCodecType` enum, default `MEDIA_CODEC_AUDIO_PCM` (mapped below) |
| 2 | `audio_type` | optional | `message.AudioStreamType` enum (mapped below) |
| 3 | `audio_configs` | repeated | `shared.message.AudioConfiguration` (mapped below) |
| 4 | `video_configs` | repeated | `message.VideoConfiguration` (mapped below) |
| 5 | `available_while_in_call` | optional | `bool` |
| 6 | `display_id` | optional | `uint32` |
| 7 | `display_type` | optional | `message.DisplayType` enum (mapped below) |
| 8 | `initial_content_keycode` | optional | `message.KeyCode` enum (mapped below) |

### `InputSourceService` (`aap_protobuf.service.inputsource.InputSourceService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `keycodes_supported` | repeated, packed | `int32` |
| 2 | `touchscreen` | repeated | nested `TouchScreen` (below) |
| 3 | `touchpad` | repeated | nested `TouchPad` (below) |
| 4 | `feedback_events_supported` | repeated | `message.FeedbackEvent` enum (mapped below) |
| 5 | `display_id` | optional | `uint32` |

Nested `InputSourceService.TouchScreen`:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `width` | required | `int32` |
| 2 | `height` | required | `int32` |
| 3 | `type` | optional | `message.TouchScreenType` enum (mapped below) |
| 4 | `is_secondary` | optional | `bool` |

Nested `InputSourceService.TouchPad`:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `width` | required | `int32` |
| 2 | `height` | required | `int32` |
| 3 | `ui_navigation` | optional | `bool` |
| 4 | `physical_width` | optional | `int32` |
| 5 | `physical_height` | optional | `int32` |
| 6 | `ui_absolute` | optional | `bool` |
| 7 | `tap_as_select` | optional | `bool` |
| 8 | `sensitivity` | optional | `int32` |

### `MediaSourceService` (`aap_protobuf.service.media.source.MediaSourceService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `available_type` | optional | `media.shared.message.MediaCodecType` enum, default `MEDIA_CODEC_AUDIO_PCM` (mapped below) |
| 2 | `audio_config` | optional | `media.shared.message.AudioConfiguration` (mapped below) |
| 3 | `available_while_in_call` | optional | `bool` |

### `BluetoothService` (`aap_protobuf.service.bluetooth.BluetoothService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `car_address` | required | `string` |
| 2 | `supported_pairing_methods` | repeated, packed | `message.BluetoothPairingMethod` enum (mapped below) |

### `RadioService` (`aap_protobuf.service.radio.RadioService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `radio_properties` | repeated | `message.RadioProperties` (mapped below) |

Nested `RadioProperties` (`aap_protobuf.service.radio.message.RadioProperties`, proto2, its own file — `protobuf/aap_protobuf/service/radio/message/RadioProperties.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `radio_id` | required | `int32` |
| 2 | `type` | required | `message.RadioType` enum (mapped below) |
| 3 | `channel_range` | repeated | `message.Range` (mapped below) |
| 4 | `channel_spacings` | repeated | `int32` |
| 5 | `channel_spacing` | required | `int32` |
| 6 | `background_tuner` | optional | `bool` |
| 7 | `region` | optional | `message.ItuRegion` enum (mapped below) |
| 8 | `rds` | optional | `message.RdsType` enum (mapped below) |
| 9 | `af_switch` | optional | `bool` |
| 10 | `ta` | optional | `bool` |
| 11 | `traffic_service` | optional | `message.TrafficServiceType` enum (mapped below) |
| 12 | `audio_loopback` | optional | `bool` |
| 13 | `mute_capability` | optional | `bool` |
| 14 | `station_presets_access` | optional | `int32` |

`RadioService` itself carries no per-request/response messages (tuning, scanning, presets, HD radio data, traffic incidents) in this mapping pass — only the discovery-time capability advertisement (`RadioProperties`). The `radio` package has ~25 further message files (`TuneToStationRequest`, `ScanStationsRequest`, HD radio data, etc.) that are runtime request/response traffic, not part of `Service`/`ServiceDiscoveryResponse`, and are out of scope for this response-schema mapping.

### `NavigationStatusService` (`aap_protobuf.service.navigationstatus.NavigationStatusService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `minimum_interval_ms` | required | `int32` |
| 2 | `type` | required | nested `InstrumentClusterType` enum: `IMAGE = 1`, `ENUM = 2` |
| 3 | `image_options` | optional | nested `ImageOptions` message (below) |

Nested `NavigationStatusService.ImageOptions`:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `height` | required | `int32` |
| 2 | `width` | required | `int32` |
| 3 | `colour_depth_bits` | required | `int32` |

Both `InstrumentClusterType` and `ImageOptions` are declared inline inside `NavigationStatusService.proto` itself (no separate file), unlike every other nested enum/message referenced in this document, which each have their own proto file.

### `MediaPlaybackStatusService` (`aap_protobuf.service.mediaplayback.MediaPlaybackStatusService`, proto2)

Empty message — no fields. Declared as a bare marker type (`message MediaPlaybackStatusService {}`) used purely to advertise the service's presence in `Service`; the actual playback status/metadata payloads (`MediaPlaybackStatus`, `MediaPlaybackMetadata`) are separate runtime messages under `service/mediaplayback/message/`, not part of this discovery-time type.

### `PhoneStatusService` (`aap_protobuf.service.phonestatus.PhoneStatusService`, proto2)

Empty message — no fields, same bare-marker shape as `MediaPlaybackStatusService` above. Runtime payloads (`PhoneStatus`, `PhoneStatusInput`) live under `service/phonestatus/message/`, outside this discovery-time type.

### `MediaBrowserService` (`aap_protobuf.service.mediabrowser.MediaBrowserService`, proto2)

Empty message — no fields, same bare-marker shape. Runtime payloads (`MediaList`, `MediaSong`, `MediaSource`, etc.) live under `service/mediabrowser/message/`, outside this discovery-time type.

### `VendorExtensionService` (`aap_protobuf.service.vendorextension.VendorExtensionService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `name` | required | `string` |
| 2 | `package_white_list` | repeated | `string` |
| 3 | `data` | optional | `bytes` |

### `GenericNotificationService` (`aap_protobuf.service.genericnotification.GenericNotificationService`, proto2)

Empty message — no fields, same bare-marker shape. Runtime payloads (`GenericNotificationMessage`, `GenericNotificationAck`, subscribe/unsubscribe) live under `service/genericnotification/message/`, outside this discovery-time type.

### `WifiProjectionService` (`aap_protobuf.service.wifiprojection.WifiProjectionService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `car_wifi_bssid` | optional | `string` |

### Leaf enum and config messages

Every leaf type referenced as "not yet mapped" above is mapped here, read from its own proto file at the same pinned revision. None carry a per-file licence/copyright header, consistent with every other proto file cited in this section.

`MediaCodecType` (`aap_protobuf.service.media.shared.message.MediaCodecType`, enum, `service/media/shared/message/MediaCodecType.proto`):

| Value | Name |
|---|---|
| 1 | `MEDIA_CODEC_AUDIO_PCM` |
| 2 | `MEDIA_CODEC_AUDIO_AAC_LC` |
| 3 | `MEDIA_CODEC_VIDEO_H264_BP` |
| 4 | `MEDIA_CODEC_AUDIO_AAC_LC_ADTS` |
| 5 | `MEDIA_CODEC_VIDEO_VP9` |
| 6 | `MEDIA_CODEC_VIDEO_AV1` |
| 7 | `MEDIA_CODEC_VIDEO_H265` |

`AudioStreamType` (`aap_protobuf.service.media.sink.message.AudioStreamType`, enum, `service/media/sink/message/AudioStreamType.proto`):

| Value | Name |
|---|---|
| 1 | `AUDIO_STREAM_GUIDANCE` |
| 2 | `AUDIO_STREAM_SYSTEM_AUDIO` |
| 3 | `AUDIO_STREAM_MEDIA` |
| 4 | `AUDIO_STREAM_TELEPHONY` |

`AudioConfiguration` (`aap_protobuf.service.media.shared.message.AudioConfiguration`, proto2, `service/media/shared/message/AudioConfiguration.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `sampling_rate` | required | `uint32` |
| 2 | `number_of_bits` | required | `uint32` |
| 3 | `number_of_channels` | required | `uint32` |

`VideoConfiguration` (`aap_protobuf.service.media.sink.message.VideoConfiguration`, proto2, `service/media/sink/message/VideoConfiguration.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `codec_resolution` | optional | `VideoCodecResolutionType` enum (mapped below) |
| 2 | `frame_rate` | optional | `VideoFrameRateType` enum (mapped below) |
| 3 | `width_margin` | optional | `uint32` |
| 4 | `height_margin` | optional | `uint32` |
| 5 | `density` | optional | `uint32` |
| 6 | `decoder_additional_depth` | optional | `uint32` |
| 7 | `viewing_distance` | optional | `uint32` |
| 8 | `pixel_aspect_ratio_e4` | optional | `uint32` |
| 9 | `real_density` | optional | `uint32` |
| 10 | `video_codec_type` | optional | `shared.message.MediaCodecType` enum (mapped above) |
| 11 | `ui_config` | optional | `shared.message.UiConfig` (not yet mapped) |

`VideoCodecResolutionType` (`aap_protobuf.service.media.sink.message.VideoCodecResolutionType`, enum, `service/media/sink/message/VideoCodecResolutionType.proto`):

| Value | Name |
|---|---|
| 1 | `VIDEO_800x480` |
| 2 | `VIDEO_1280x720` |
| 3 | `VIDEO_1920x1080` |
| 4 | `VIDEO_2560x1440` |
| 5 | `VIDEO_3840x2160` |
| 6 | `VIDEO_720x1280` |
| 7 | `VIDEO_1080x1920` |
| 8 | `VIDEO_1440x2560` |
| 9 | `VIDEO_2160x3840` |

`VideoFrameRateType` (`aap_protobuf.service.media.sink.message.VideoFrameRateType`, enum, `service/media/sink/message/VideoFrameRateType.proto`):

| Value | Name |
|---|---|
| 1 | `VIDEO_FPS_60` |
| 2 | `VIDEO_FPS_30` |

`DisplayType` (`aap_protobuf.service.media.sink.message.DisplayType`, enum, `service/media/sink/message/DisplayType.proto`):

| Value | Name |
|---|---|
| 0 | `DISPLAY_TYPE_MAIN` |
| 1 | `DISPLAY_TYPE_CLUSTER` |
| 2 | `DISPLAY_TYPE_AUXILIARY` |

`KeyCode` (`aap_protobuf.service.media.sink.message.KeyCode`, enum, `service/media/sink/message/KeyCode.proto`): a large enum (278 named values total) mirroring Android's `KeyEvent` key codes. The main contiguous block runs `KEYCODE_UNKNOWN = 0` through `KEYCODE_DPAD_DOWN_RIGHT = 271` (268 values), with values 264–267 genuinely absent upstream — the sequence skips from `KEYCODE_NAVIGATE_OUT = 263` straight to `KEYCODE_DPAD_UP_LEFT = 268` — not an omission in this record. A separate, non-contiguous block of ten car-specific/sentinel values sits at 65535–65544: `KEYCODE_SENTINEL`, `KEYCODE_ROTARY_CONTROLLER`, `KEYCODE_MEDIA`, `KEYCODE_NAVIGATION`, `KEYCODE_RADIO`, `KEYCODE_TEL`, `KEYCODE_PRIMARY_BUTTON`, `KEYCODE_SECONDARY_BUTTON`, `KEYCODE_TERTIARY_BUTTON`, `KEYCODE_TURN_CARD`. The full enumeration is not transcribed here; consult the pinned proto file directly for any value not named above.

`Sensor` (`aap_protobuf.service.sensorsource.message.Sensor`, proto2, `service/sensorsource/message/Sensor.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `sensor_type` | required | `SensorType` enum (mapped below) |

`SensorType` (`aap_protobuf.service.sensorsource.message.SensorType`, enum, `service/sensorsource/message/SensorType.proto`):

| Value | Name |
|---|---|
| 1 | `SENSOR_LOCATION` |
| 2 | `SENSOR_COMPASS` |
| 3 | `SENSOR_SPEED` |
| 4 | `SENSOR_RPM` |
| 5 | `SENSOR_ODOMETER` |
| 6 | `SENSOR_FUEL` |
| 7 | `SENSOR_PARKING_BRAKE` |
| 8 | `SENSOR_GEAR` |
| 9 | `SENSOR_OBDII_DIAGNOSTIC_CODE` |
| 10 | `SENSOR_NIGHT_MODE` |
| 11 | `SENSOR_ENVIRONMENT_DATA` |
| 12 | `SENSOR_HVAC_DATA` |
| 13 | `SENSOR_DRIVING_STATUS_DATA` |
| 14 | `SENSOR_DEAD_RECKONING_DATA` |
| 15 | `SENSOR_PASSENGER_DATA` |
| 16 | `SENSOR_DOOR_DATA` |
| 17 | `SENSOR_LIGHT_DATA` |
| 18 | `SENSOR_TIRE_PRESSURE_DATA` |
| 19 | `SENSOR_ACCELEROMETER_DATA` |
| 20 | `SENSOR_GYROSCOPE_DATA` |
| 21 | `SENSOR_GPS_SATELLITE_DATA` |
| 22 | `SENSOR_TOLL_CARD` |

`FuelType` (`aap_protobuf.service.sensorsource.message.FuelType`, enum, `service/sensorsource/message/FuelType.proto`):

| Value | Name |
|---|---|
| 0 | `FUEL_TYPE_UNKNOWN` |
| 1 | `FUEL_TYPE_UNLEADED` |
| 2 | `FUEL_TYPE_LEADED` |
| 3 | `FUEL_TYPE_DIESEL_1` |
| 4 | `FUEL_TYPE_DIESEL_2` |
| 5 | `FUEL_TYPE_BIODIESEL` |
| 6 | `FUEL_TYPE_E85` |
| 7 | `FUEL_TYPE_LPG` |
| 8 | `FUEL_TYPE_CNG` |
| 9 | `FUEL_TYPE_LNG` |
| 10 | `FUEL_TYPE_ELECTRIC` |
| 11 | `FUEL_TYPE_HYDROGEN` |
| 12 | `FUEL_TYPE_OTHER` |

`EvConnectorType` (`aap_protobuf.service.sensorsource.message.EvConnectorType`, enum, `service/sensorsource/message/EvConnectorType.proto`):

| Value | Name | Notes |
|---|---|---|
| 0 | `EV_CONNECTOR_TYPE_UNKNOWN` | |
| 1 | `EV_CONNECTOR_TYPE_J1772` | |
| 2 | `EV_CONNECTOR_TYPE_MENNEKES` | |
| 3 | `EV_CONNECTOR_TYPE_CHADEMO` | |
| 4 | `EV_CONNECTOR_TYPE_COMBO_1` | |
| 5 | `EV_CONNECTOR_TYPE_COMBO_2` | |
| 6 | `EV_CONNECTOR_TYPE_TESLA_ROADSTER` | `[deprecated = true]` upstream |
| 7 | `EV_CONNECTOR_TYPE_TESLA_HPWC` | `[deprecated = true]` upstream |
| 8 | `EV_CONNECTOR_TYPE_TESLA_SUPERCHARGER` | |
| 9 | `EV_CONNECTOR_TYPE_GBT` | |
| 101 | `EV_CONNECTOR_TYPE_OTHER` | non-contiguous jump from 9, genuinely upstream |

`BluetoothPairingMethod` (`aap_protobuf.service.bluetooth.message.BluetoothPairingMethod`, enum, `service/bluetooth/message/BluetoothPairingMethod.proto`):

| Value | Name |
|---|---|
| -1 | `BLUETOOTH_PAIRING_UNAVAILABLE` |
| 1 | `BLUETOOTH_PAIRING_OOB` |
| 2 | `BLUETOOTH_PAIRING_NUMERIC_COMPARISON` |
| 3 | `BLUETOOTH_PAIRING_PASSKEY_ENTRY` |
| 4 | `BLUETOOTH_PAIRING_PIN` |

The only negative enum value seen anywhere in this document's mappings so far — proto2 permits this since the wire encoding is a plain varint (zigzag encoding does not apply to plain `enum` fields), but it means a naive unsigned-width Rust representation would be wrong for this one type specifically.

`TouchScreenType` (`aap_protobuf.service.inputsource.message.TouchScreenType`, enum, `service/inputsource/message/TouchScreenType.proto`):

| Value | Name |
|---|---|
| 1 | `CAPACITIVE` |
| 2 | `RESISTIVE` |
| 3 | `INFRARED` |

`FeedbackEvent` (`aap_protobuf.service.inputsource.message.FeedbackEvent`, enum, `service/inputsource/message/FeedbackEvent.proto`):

| Value | Name |
|---|---|
| 1 | `FEEDBACK_SELECT` |
| 2 | `FEEDBACK_FOCUS_CHANGE` |
| 3 | `FEEDBACK_DRAG_SELECT` |
| 4 | `FEEDBACK_DRAG_START` |
| 5 | `FEEDBACK_DRAG_END` |

`RadioType` (`aap_protobuf.service.radio.message.RadioType`, enum, `service/radio/message/RadioType.proto`):

| Value | Name |
|---|---|
| 0 | `AM_RADIO` |
| 1 | `FM_RADIO` |
| 2 | `AM_HD_RADIO` |
| 3 | `FM_HD_RADIO` |
| 4 | `DAB_RADIO` |
| 5 | `XM_RADIO` |

`Range` (`aap_protobuf.service.radio.message.Range`, proto2, `service/radio/message/Range.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `min` | required | `int32` |
| 2 | `max` | required | `int32` |

`RdsType` (`aap_protobuf.service.radio.message.RdsType`, enum, `service/radio/message/RdsType.proto`):

| Value | Name |
|---|---|
| 0 | `NO_RDS` |
| 1 | `RDS` |
| 2 | `RBDS` |

`TrafficServiceType` (`aap_protobuf.service.radio.message.TrafficServiceType`, enum, `service/radio/message/TrafficServiceType.proto`):

| Value | Name |
|---|---|
| 0 | `NO_TRAFFIC_SERVICE` |
| 1 | `TMC_TRAFFIC_SERVICE` |

`ItuRegion` (`aap_protobuf.service.radio.ItuRegion`, enum, `service/radio/message/ItuRegion.proto`):

| Value | Name |
|---|---|
| 0 | `RADIO_REGION_NONE` |
| 1 | `RADIO_REGION_ITU_1` |
| 2 | `RADIO_REGION_ITU_2` |
| 3 | `RADIO_REGION_OIRT` |
| 4 | `RADIO_REGION_JAPAN` |
| 5 | `RADIO_REGION_KOREA` |

Note the package deviation: `ItuRegion.proto` declares `package aap_protobuf.service.radio;` (one level shallower than every sibling file in the same `service/radio/message/` directory, which all declare `aap_protobuf.service.radio.message`) — its own file location and every other file's package agree with each other, only this one file's package statement is shallower than its directory would suggest. `RadioProperties.proto` refers to it as unqualified `ItuRegion`, which resolves correctly under proto2 scoping rules regardless of the package mismatch, but a Rust codegen path that derives module paths from directory structure rather than each file's own `package` statement would place this type incorrectly.

### `ServiceDiscoveryResponse`'s remaining non-deprecated messages

`DriverPosition` (`aap_protobuf.service.control.message.DriverPosition`, enum, `service/control/message/DriverPosition.proto`):

| Value | Name |
|---|---|
| 0 | `DRIVER_POSITION_LEFT` |
| 1 | `DRIVER_POSITION_RIGHT` |
| 2 | `DRIVER_POSITION_CENTER` |
| 3 | `DRIVER_POSITION_UNKNOWN` |

`ConnectionConfiguration` (`aap_protobuf.service.control.message.ConnectionConfiguration`, proto2, `service/control/message/ConnectionConfiguration.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `ping_configuration` | optional | `PingConfiguration` (mapped below) |
| 2 | `wireless_tcp_configuration` | optional | `WirelessTcpConfiguration` (mapped below) |

`PingConfiguration` (`aap_protobuf.service.control.message.PingConfiguration`, proto2, `service/control/message/PingConfiguration.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `timeout_ms` | optional | `uint32` |
| 2 | `interval_ms` | optional | `uint32` |
| 3 | `high_latency_threshold_ms` | optional | `uint32` |
| 4 | `tracked_ping_count` | optional | `uint32` |

`WirelessTcpConfiguration` (`aap_protobuf.service.control.message.WirelessTcpConfiguration`, proto2, `service/control/message/WirelessTcpConfiguration.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `socket_receive_buffer_size_kb` | optional | `uint32` |
| 2 | `socket_send_buffer_size_kb` | optional | `uint32` |
| 3 | `socket_read_timeout_ms` | optional | `uint32` |

`HeadUnitInfo` (`aap_protobuf.service.control.message.HeadUnitInfo`, proto2, `service/control/message/HeadUnitInfo.proto`):

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `make` | optional | `string` |
| 2 | `model` | optional | `string` |
| 3 | `year` | optional | `string` |
| 4 | `vehicle_id` | optional | `string` |
| 5 | `head_unit_make` | optional | `string` |
| 6 | `head_unit_model` | optional | `string` |
| 7 | `head_unit_software_build` | optional | `string` |
| 8 | `head_unit_software_version` | optional | `string` |

`HeadUnitInfo` is a direct non-deprecated replacement for `ServiceDiscoveryResponse`'s own deprecated flat fields 2–5 and 7–10 (`make`, `model`, `year`, `vehicle_id`, `head_unit_make`, `head_unit_model`, `head_unit_software_build`, `head_unit_software_version`) — same field names and types, moved into a nested message rather than left flat on the response. This is a real upstream migration, not a naming coincidence introduced by this record.

Every field in this section is `proto2`, and none of the five files carries a per-file licence/copyright header, consistent with every other proto file cited in this document.

With this section, all three of `ServiceDiscoveryResponse`'s previously "not yet mapped" fields (`driver_position`, `connection_configuration`, `headunit_info`) are now mapped; `UiConfig` (and its own further references `UiTheme`/`Insets`) remains the only open leaf in the "not yet mapped" list above.

### Contrast against OpenAuto's older schema

`openauto-adoption.md` already records that the approved OpenAuto revision uses an older AASDK `ChannelDescriptor`/`ServiceDiscoveryResponseMessage` schema. Fetching that schema from the original upstream (`https://github.com/f1xpl/aasdk`, `aasdk_proto/ChannelDescriptorData.proto` and `aasdk_proto/ServiceDiscoveryResponseMessage.proto`, GPL-3.0-or-later, Copyright (C) 2018 f1x.studio) for direct comparison confirms it is a materially different wire shape from the pinned `newdev` schema mapped above, not merely a renamed one:

- proto3 (`ChannelDescriptorData.proto`) versus proto2 (`Service.proto`): no `required`/`optional` distinction, different default-value and unknown-field semantics.
- `ChannelDescriptor` has 7 typed channel fields (`sensor_channel`=2, `av_channel`=3, `input_channel`=4, `av_input_channel`=5, `bluetooth_channel`=6, `navigation_channel`=8, `vendor_extension_channel`=12, with 7/9-11/13+ unused) covering a single combined audio/video channel; `Service` has 13 typed service fields covering split media sink/source, radio, media playback status, phone status, media browser, and generic notification roles the old schema does not express at all.
- `ServiceDiscoveryResponseMessage` (old) carries flat `string`/`bool` head-unit and vehicle-identity fields (`head_unit_name`, `car_model`, `car_year`, `car_serial`, `headunit_manufacturer`, `headunit_model`, `sw_build`, `sw_version`, `hide_clock`) with no analogue to the new schema's `DriverPosition`, `ConnectionConfiguration`, or `HeadUnitInfo` sub-messages; the new schema instead deprecates its equivalent flat identity fields outright.
- Field numbers are not stable across the two schemas even where names coincide by accident (e.g. `bluetooth_channel`/`bluetooth_service` both land on field 6, but `av_channel`=3 in the old schema has no single-field equivalent in the new one, which splits sink/source across fields 3 and 5).

No behaviour, field mapping, channel number, or value from the old `ChannelDescriptor` schema is reused for the new `Service` schema above; each was read and recorded independently from its own pinned/upstream source.

### Corrected finding: `ServiceDiscoveryRequest` field 4/5 labelling

While re-reading the pinned `newdev` `ServiceDiscoveryRequest.proto` for this mapping, its actual field names were found to be `4 = label_text` and `5 = device_name` (both `optional string`) — not `device_name`/`device_brand` as `crates/protocol-aap/src/service_discovery.rs`'s `ServiceDiscoveryRequestSummary` previously named them (`device_name_bytes` for field 4, `device_brand_bytes` for field 5, sourced in that file's comment from a secondary reference, `f-io/LIVI` commit `b7435e8`, instead of the pinned primary AASDK source). There is no `device_brand` field in the pinned schema at all. This was a naming-only defect — both fields are `optional string`, wire type 2, and the summarizer only ever recorded byte length and validated UTF-8, never the field's semantic name — but the Rust struct field names and doc comment did not match this project's own pinned primary source. The struct has been corrected to `label_text_bytes` (field 4) and `device_name_bytes` (field 5), matching the pinned source and this document.

## Adopted USB interoperability-probe scope

The completed bench probe's Android Auto accessory identification and claimed bulk-transfer behaviour were derived from:

- `include/aasdk/USB/AccessoryModeQueryFactory.hpp`
- `src/USB/AccessoryModeQueryFactory.cpp`
- `include/aasdk/Transport/USBTransport.hpp`
- `src/Transport/USBTransport.cpp`

The historical probe used AASDK's exact six accessory strings, including its third-party URI and serial value, with explicit operator opt-in. It sent the adopted version and encapsulated-TLS messages over a claimed bulk interface using temporary project-generated credentials, stopped before authentication completion or service discovery, and logged no payloads. After the recorded error-7 rejection, all live generated-identity paths were permanently disabled.

## Expansion rule

Before another AASDK behaviour, schema, identifier, or non-excluded asset is used, add its exact upstream path and purpose here, verify its file-level licence/copyright notice, and update third-party notices. OpenAuto-derived behaviour follows the separate `openauto-adoption.md` procedure; neither record may be used to introduce credentials, identities, security bypasses, trademarks, or bundled assets.
