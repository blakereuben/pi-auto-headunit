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

These five nested per-kind messages were selected because they are the only ones with a corresponding `ServiceKind` in `crates/protocol-aap/src/service_catalogue.rs` today (`Sensors`, `Video`/`MediaAudio`/`SpeechAudio`/`SystemAudio`, `Input`, `Microphone`, `Bluetooth`). None of these proto files carry a per-file licence/copyright header at the pinned revision — the same posture already recorded above for `ServiceDiscoveryRequest.proto` and the other already-adopted proto files in this document, which rely on the repository-wide GPL-3.0-or-later notices found in the adopted `.hpp`/`.cpp` files rather than a per-proto notice.

**Not yet mapped** (out of scope for this pass because the catalogue does not model them): the eight remaining `Service` nested types — `radio_service`, `navigation_status_service`, `media_playback_service`, `phone_status_service`, `media_browser_service`, `vendor_extension_service`, `generic_notification_service`, `wifi_projection_service` — and every leaf enum/config message referenced below (`MediaCodecType`, `AudioStreamType`, `AudioConfiguration`, `VideoConfiguration`, `DisplayType`, `KeyCode`, `Sensor`, `FuelType`, `EvConnectorType`, `BluetoothPairingMethod`, `TouchScreenType`, `FeedbackEvent`), plus `ServiceDiscoveryResponse`'s non-deprecated `DriverPosition`, `ConnectionConfiguration`, and `HeadUnitInfo` messages. These must each be mapped and recorded here before any encoder reads or writes them.

### `Service` (`aap_protobuf.service.Service`, proto2)

| # | Name | Label | Type | Notes |
|---|---|---|---|---|
| 1 | `id` | required | `int32` | channel identifier |
| 2 | `sensor_source_service` | optional | `sensorsource.SensorSourceService` | mapped below |
| 3 | `media_sink_service` | optional | `media.sink.MediaSinkService` | mapped below |
| 4 | `input_source_service` | optional | `inputsource.InputSourceService` | mapped below |
| 5 | `media_source_service` | optional | `media.source.MediaSourceService` | mapped below |
| 6 | `bluetooth_service` | optional | `bluetooth.BluetoothService` | mapped below |
| 7 | `radio_service` | optional | `radio.RadioService` | not yet mapped |
| 8 | `navigation_status_service` | optional | `navigationstatus.NavigationStatusService` | not yet mapped |
| 9 | `media_playback_service` | optional | `mediaplayback.MediaPlaybackStatusService` | not yet mapped |
| 10 | `phone_status_service` | optional | `phonestatus.PhoneStatusService` | not yet mapped |
| 11 | `media_browser_service` | optional | `mediabrowser.MediaBrowserService` | not yet mapped |
| 12 | `vendor_extension_service` | optional | `vendorextension.VendorExtensionService` | not yet mapped |
| 13 | `generic_notification_service` | optional | `genericnotification.GenericNotificationService` | not yet mapped |
| 14 | `wifi_projection_service` | optional | `wifiprojection.WifiProjectionService` | not yet mapped |

Every field after `id` is wire type 2 (length-delimited/embedded message); `id` is wire type 0 (varint). At most one service-type field is expected populated per `Service` instance in observed upstream usage, but the schema does not itself enforce that as a `oneof`.

### `ServiceDiscoveryResponse` (`aap_protobuf.service.control.message.ServiceDiscoveryResponse`, proto2)

| # | Name | Label | Type | Notes |
|---|---|---|---|---|
| 1 | `channels` | repeated | `service.Service` | the mapped catalogue, above |
| 2 | `make` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 3 | `model` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 4 | `year` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 5 | `vehicle_id` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 6 | `driver_position` | optional | `DriverPosition` | not yet mapped |
| 7 | `head_unit_make` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 8 | `head_unit_model` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 9 | `head_unit_software_build` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 10 | `head_unit_software_version` | optional | `string` | `[deprecated = true]` upstream; excluded |
| 11 | `can_play_native_media_during_vr` | optional | `bool` | `[deprecated = true]` upstream; excluded |
| 13 | `session_configuration` | optional | `int32` | not yet mapped; field 12 does not exist upstream |
| 14 | `display_name` | optional | `string` | not yet mapped |
| 15 | `probe_for_support` | optional | `bool` | not yet mapped |
| 16 | `connection_configuration` | optional | `ConnectionConfiguration` | not yet mapped |
| 17 | `headunit_info` | optional | `HeadUnitInfo` | not yet mapped |

Field 12 is genuinely absent upstream (the sequence skips from 11 to 13); this is not an omission in this record.

### `SensorSourceService` (`aap_protobuf.service.sensorsource.SensorSourceService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `sensors` | repeated | `message.Sensor` (not yet mapped) |
| 2 | `location_characterization` | optional | `uint32` |
| 3 | `supported_fuel_types` | repeated | `message.FuelType` enum (not yet mapped) |
| 4 | `supported_ev_connector_types` | repeated | `message.EvConnectorType` enum (not yet mapped) |

### `MediaSinkService` (`aap_protobuf.service.media.sink.MediaSinkService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `available_type` | optional | `shared.message.MediaCodecType` enum, default `MEDIA_CODEC_AUDIO_PCM` (not yet mapped) |
| 2 | `audio_type` | optional | `message.AudioStreamType` enum (not yet mapped) |
| 3 | `audio_configs` | repeated | `shared.message.AudioConfiguration` (not yet mapped) |
| 4 | `video_configs` | repeated | `message.VideoConfiguration` (not yet mapped) |
| 5 | `available_while_in_call` | optional | `bool` |
| 6 | `display_id` | optional | `uint32` |
| 7 | `display_type` | optional | `message.DisplayType` enum (not yet mapped) |
| 8 | `initial_content_keycode` | optional | `message.KeyCode` enum (not yet mapped) |

### `InputSourceService` (`aap_protobuf.service.inputsource.InputSourceService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `keycodes_supported` | repeated, packed | `int32` |
| 2 | `touchscreen` | repeated | nested `TouchScreen` (below) |
| 3 | `touchpad` | repeated | nested `TouchPad` (below) |
| 4 | `feedback_events_supported` | repeated | `message.FeedbackEvent` enum (not yet mapped) |
| 5 | `display_id` | optional | `uint32` |

Nested `InputSourceService.TouchScreen`:

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `width` | required | `int32` |
| 2 | `height` | required | `int32` |
| 3 | `type` | optional | `message.TouchScreenType` enum (not yet mapped) |
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
| 1 | `available_type` | optional | `media.shared.message.MediaCodecType` enum, default `MEDIA_CODEC_AUDIO_PCM` (not yet mapped) |
| 2 | `audio_config` | optional | `media.shared.message.AudioConfiguration` (not yet mapped) |
| 3 | `available_while_in_call` | optional | `bool` |

### `BluetoothService` (`aap_protobuf.service.bluetooth.BluetoothService`, proto2)

| # | Name | Label | Type |
|---|---|---|---|
| 1 | `car_address` | required | `string` |
| 2 | `supported_pairing_methods` | repeated, packed | `message.BluetoothPairingMethod` enum (not yet mapped) |

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
