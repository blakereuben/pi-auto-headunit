use std::fmt;

use crate::control::{ControlMessage, ControlMessageId};
use crate::input_message::KeyCode;
use crate::protobuf;
use crate::sensor::SensorType;
use crate::service_catalogue::{ServiceCatalogue, ServiceKind};

// Portions derived from AASDK's Service/ServiceDiscoveryResponse protobuf
// schema at the pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1);
// field numbers and enum values match the mapping recorded in
// docs/protocol/aasdk-adoption.md.
//
// `PingConfiguration`'s populated values are derived from a separate,
// independently implemented, GPL-3.0-or-later Android Auto client
// (`f-io/LIVI` revision 9000f308eec423c5c56ac0a14491a7c95ce5762d,
// `src/main/services/projection/driver/aa/stack/session/ServiceDiscoveryBuilder.ts`
// and `Session.ts`, not AASDK-derived), formally adopted per
// `docs/protocol/livi-adoption.md` ("Adopted scope" items 4-5). No LIVI
// code is reproduced; only the four numeric field values themselves.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// Copyright (C) 2024-2026 Open Android Auto contributors (LIVI)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.media.sink.message.VideoCodecResolutionType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoCodecResolution {
    Video800x480,
    Video1280x720,
    Video1920x1080,
    Video2560x1440,
    Video3840x2160,
    Video720x1280,
    Video1080x1920,
    Video1440x2560,
    Video2160x3840,
}

impl VideoCodecResolution {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Video800x480 => 1,
            Self::Video1280x720 => 2,
            Self::Video1920x1080 => 3,
            Self::Video2560x1440 => 4,
            Self::Video3840x2160 => 5,
            Self::Video720x1280 => 6,
            Self::Video1080x1920 => 7,
            Self::Video1440x2560 => 8,
            Self::Video2160x3840 => 9,
        }
    }
}

/// `aap_protobuf.service.media.sink.message.VideoFrameRateType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoFrameRate {
    Fps60,
    Fps30,
}

impl VideoFrameRate {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Fps60 => 1,
            Self::Fps30 => 2,
        }
    }
}

/// `aap_protobuf.service.media.shared.message.MediaCodecType`, the subset
/// this project can advertise for video. Real-hardware evidence for
/// advertising both at once: a known-good reference client (`f-io/LIVI`)
/// advertises `h264, h265` together and the same real phone actively
/// selected H.265 over H.264 when given the choice
/// (`docs/protocol/error-2-investigation.md`, "LIVI known-good capture").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoCodecType {
    H264,
    Hevc,
}

impl VideoCodecType {
    const fn wire_value(self) -> i32 {
        match self {
            Self::H264 => 3, // MEDIA_CODEC_VIDEO_H264_BP
            Self::Hevc => 7, // MEDIA_CODEC_VIDEO_H265
        }
    }
}

/// `aap_protobuf.service.media.shared.message.Insets` — four `uint32`
/// pixel-offset fields (`docs/protocol/aasdk-adoption.md`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Insets {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

/// `aap_protobuf.service.media.shared.message.UiConfig` — `VideoConfiguration`
/// field 11 (`docs/protocol/aasdk-adoption.md`). `ui_theme` (`UiConfig`'s
/// fourth field) is left unmodeled rather than filled with an unresearched
/// value. Populating this at all, and specifically with all-zero insets
/// when no custom display geometry is configured, is derived from `f-io/LIVI`
/// (`docs/protocol/livi-adoption.md`, "Adopted scope" item 6): LIVI computes
/// non-zero `margins`/`content_insets` only when its own configured display
/// size diverges in aspect ratio from the negotiated video tier, and always
/// sets `stable_content_insets` equal to `content_insets`. This project
/// advertises a single fixed 800x480 tier with no display-specific
/// customization implemented yet, so LIVI's own default (all-zero) case
/// applies directly — not a guess at an unresearched value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UiConfig {
    pub margins: Insets,
    pub content_insets: Insets,
    pub stable_content_insets: Insets,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCapability {
    pub resolution: VideoCodecResolution,
    pub frame_rate: VideoFrameRate,
    pub codec: VideoCodecType,
    /// `VideoConfiguration.density` (field 5, optional `uint32`) — a
    /// display-density value in dpi. Never populated before this — see
    /// `docs/protocol/error-2-investigation.md`, "TLS-decrypted LIVI
    /// session capture": a real, decrypted `f-io/LIVI` session (TLS
    /// session-keylog + raw `usbmon` capture, not source-code reuse) shows
    /// it advertising `density = 180` for its 1280x720 tier, a field this
    /// project has never populated at all.
    pub density: Option<u32>,
    /// `VideoConfiguration.pixel_aspect_ratio_e4` (field 8, optional
    /// `uint32`) — a fixed-point pixel-aspect-ratio value scaled by 10000,
    /// e.g. `10000` for a 1:1 (square-pixel) ratio. Never populated before
    /// this — see `docs/protocol/error-2-investigation.md`, "1280×720
    /// resolution tested": `f-io/LIVI` populates it (`PAR e4=10000`) and
    /// this project never had, the only remaining `VideoConfiguration`
    /// field difference found against LIVI's known-good capture after both
    /// codec and resolution-tier advertisement were tried and refuted as
    /// sufficient alone.
    pub pixel_aspect_ratio_e4: Option<u32>,
    pub ui_config: Option<UiConfig>,
}

/// `aap_protobuf.service.inputsource.message.TouchScreenType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchScreenType {
    Capacitive,
    Resistive,
    Infrared,
}

impl TouchScreenType {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Capacitive => 1,
            Self::Resistive => 2,
            Self::Infrared => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TouchCapability {
    pub width: i32,
    pub height: i32,
    pub touch_type: TouchScreenType,
    /// `InputSourceService.keycodes_supported` (field 1, a sibling of
    /// `touchscreen` field 2 within the same service, not nested under
    /// it — modeled here anyway to keep one capability struct per
    /// `ServiceKind::Input`, matching this project's existing "one
    /// capability struct per advertised service" shape). See
    /// `docs/protocol/aasdk-adoption.md`'s `KeyCode` section: only the
    /// four car-specific category-switch codes this project can
    /// actually send (`input_message::encode_key_event`) are ever
    /// advertised here.
    pub keycodes_supported: Vec<KeyCode>,
}

/// `aap_protobuf.service.media.sink.message.AudioStreamType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioStreamType {
    Guidance,
    SystemAudio,
    Media,
    Telephony,
}

impl AudioStreamType {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Guidance => 1,
            Self::SystemAudio => 2,
            Self::Media => 3,
            Self::Telephony => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioCapability {
    pub sampling_rate: u32,
    pub number_of_bits: u32,
    pub number_of_channels: u32,
    pub stream_type: AudioStreamType,
}

/// `aap_protobuf.service.control.message.HeadUnitInfo` — a direct
/// non-deprecated replacement for `ServiceDiscoveryResponse`'s own
/// deprecated flat identity fields (`docs/protocol/aasdk-adoption.md`). All
/// fields are head-unit-chosen identifying strings, never phone-derived
/// data — safe to populate with fixed project identifiers, not real
/// vehicle data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadUnitInfo {
    pub make: String,
    pub model: String,
    pub year: String,
    pub vehicle_id: String,
    pub head_unit_make: String,
    pub head_unit_model: String,
    pub head_unit_software_build: String,
    pub head_unit_software_version: String,
}

/// `aap_protobuf.service.control.message.PingConfiguration`, the phone-
/// visible advertisement of the head unit's ping cadence
/// (`ConnectionConfiguration`'s only implemented field —
/// `wireless_tcp_configuration`, `ConnectionConfiguration`'s other field, is
/// wired-only scope and stays unmodeled). All four fields are populated
/// with values confirmed against a formally-adopted working reference
/// (`f-io/LIVI`, `docs/protocol/livi-adoption.md`, "Adopted scope" items
/// 4-5) rather than unresearched guesses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PingConfiguration {
    pub timeout_ms: u32,
    pub interval_ms: u32,
    pub high_latency_threshold_ms: u32,
    pub tracked_ping_count: u32,
}

/// `aap_protobuf.service.bluetooth.BluetoothService`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BluetoothCapability {
    pub car_address: String,
}

/// `aap_protobuf.service.radio.message.RadioType`
/// (`service/radio/message/RadioType.proto`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioType {
    AmRadio,
    FmRadio,
    AmHdRadio,
    FmHdRadio,
    DabRadio,
    XmRadio,
}

impl RadioType {
    const fn wire_value(self) -> i32 {
        match self {
            Self::AmRadio => 0,
            Self::FmRadio => 1,
            Self::AmHdRadio => 2,
            Self::FmHdRadio => 3,
            Self::DabRadio => 4,
            Self::XmRadio => 5,
        }
    }
}

/// `aap_protobuf.service.radio.RadioService`, advertising exactly one
/// `RadioProperties` entry (`radio_id`/`type`/`channel_spacing` — the only
/// three fields proto2 `required` marks, per
/// `docs/protocol/aasdk-adoption.md`'s `RadioProperties` mapping). Every
/// other `RadioProperties` field (channel range, RDS, traffic service,
/// presets, ...) and all ~25 runtime tuning/scanning/preset messages are
/// deliberately unmapped and unimplemented. Real-hardware-confirmed,
/// 2026-08-16: without this capability advertised, the phone rejected
/// `KeyCode::Radio` with "AA was not available"; with it, the phone
/// correctly navigates to its own native (currently empty, since no
/// tuning backend exists behind it) radio screen — this capability alone
/// was the missing precondition, not a working tuner, which would need
/// the unmapped runtime messages implemented against real hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioCapability {
    pub radio_id: i32,
    pub radio_type: RadioType,
    pub channel_spacing: i32,
}

/// `aap_protobuf.service.sensorsource.SensorSourceService`. Only the
/// `sensors` list (field 1) is modeled — `location_characterization`,
/// `supported_fuel_types`, and `supported_ev_connector_types` have no
/// corresponding hardware yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SensorCapability {
    pub sensor_types: Vec<SensorType>,
}

/// `aap_protobuf.service.media.source.MediaSourceService` (the microphone
/// role — a phone-to-head-unit audio source, distinct from the
/// head-unit-to-phone `MediaSinkService` audio roles).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MicrophoneCapability {
    pub sampling_rate: u32,
    pub number_of_bits: u32,
    pub number_of_channels: u32,
}

/// Capability data for the services [`encode_service_discovery_response`]
/// knows how to encode. Kept separate from [`ServiceCatalogue`], which stays
/// wire-neutral per `ARCHITECTURE.md` §4.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceCapabilities {
    /// One entry per advertised `VideoConfiguration` (`MediaSinkService.
    /// video_configs`, a repeated field) — `Start.configuration_index`
    /// (`video_setup.rs`) refers back into this list by position.
    pub video: Option<Vec<VideoCapability>>,
    pub touch: Option<TouchCapability>,
    pub media_audio: Option<AudioCapability>,
    pub system_audio: Option<AudioCapability>,
    pub speech_audio: Option<AudioCapability>,
    pub bluetooth: Option<BluetoothCapability>,
    pub microphone: Option<MicrophoneCapability>,
    pub sensors: Option<SensorCapability>,
    pub radio: Option<RadioCapability>,
    pub head_unit_info: Option<HeadUnitInfo>,
    pub ping_configuration: Option<PingConfiguration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceDiscoveryResponseError {
    MissingCapability { channel_id: u8, kind: ServiceKind },
}

impl fmt::Display for ServiceDiscoveryResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCapability { channel_id, kind } => write!(
                formatter,
                "channel {channel_id} advertises {kind:?} but no matching capability was supplied"
            ),
        }
    }
}

impl std::error::Error for ServiceDiscoveryResponseError {}

/// Encodes a `ServiceDiscoveryResponse` control message advertising every
/// `Ready` service in `catalogue`, using `capabilities` for the per-service
/// payload. All eight [`ServiceKind`] variants are supported; `ServiceKind`
/// growing a ninth variant becomes a compile error here (the match is
/// exhaustive over the enum, not a runtime fallback), which is deliberate.
pub fn encode_service_discovery_response(
    catalogue: &ServiceCatalogue,
    capabilities: &ServiceCapabilities,
) -> Result<ControlMessage, ServiceDiscoveryResponseError> {
    let mut body = Vec::new();
    for service in catalogue.services() {
        let service_bytes = encode_service(service.channel_id, service.kind, capabilities)?;
        // ServiceDiscoveryResponse.channels (field 1, repeated Service).
        protobuf::write_length_delimited_field(&mut body, 1, &service_bytes);
    }
    if let Some(ping_configuration) = capabilities.ping_configuration {
        let connection_configuration_bytes = encode_connection_configuration(ping_configuration);
        // ServiceDiscoveryResponse.connection_configuration (field 16).
        protobuf::write_length_delimited_field(&mut body, 16, &connection_configuration_bytes);
    }
    if let Some(head_unit_info) = &capabilities.head_unit_info {
        let head_unit_info_bytes = encode_head_unit_info(head_unit_info);
        // ServiceDiscoveryResponse.headunit_info (field 17).
        protobuf::write_length_delimited_field(&mut body, 17, &head_unit_info_bytes);
    }
    Ok(ControlMessage {
        id: ControlMessageId::ServiceDiscoveryResponse,
        body,
    })
}

fn encode_connection_configuration(ping_configuration: PingConfiguration) -> Vec<u8> {
    let mut ping_bytes = Vec::new();
    // PingConfiguration.timeout_ms (field 1, optional uint32).
    protobuf::write_uint32_field(&mut ping_bytes, 1, ping_configuration.timeout_ms);
    // PingConfiguration.interval_ms (field 2, optional uint32).
    protobuf::write_uint32_field(&mut ping_bytes, 2, ping_configuration.interval_ms);
    // PingConfiguration.high_latency_threshold_ms (field 3, optional uint32).
    protobuf::write_uint32_field(
        &mut ping_bytes,
        3,
        ping_configuration.high_latency_threshold_ms,
    );
    // PingConfiguration.tracked_ping_count (field 4, optional uint32).
    protobuf::write_uint32_field(&mut ping_bytes, 4, ping_configuration.tracked_ping_count);
    let mut out = Vec::new();
    // ConnectionConfiguration.ping_configuration (field 1, optional PingConfiguration).
    protobuf::write_length_delimited_field(&mut out, 1, &ping_bytes);
    out
}

fn encode_head_unit_info(info: &HeadUnitInfo) -> Vec<u8> {
    let mut out = Vec::new();
    // HeadUnitInfo.make (field 1, optional string).
    protobuf::write_length_delimited_field(&mut out, 1, info.make.as_bytes());
    // HeadUnitInfo.model (field 2, optional string).
    protobuf::write_length_delimited_field(&mut out, 2, info.model.as_bytes());
    // HeadUnitInfo.year (field 3, optional string).
    protobuf::write_length_delimited_field(&mut out, 3, info.year.as_bytes());
    // HeadUnitInfo.vehicle_id (field 4, optional string).
    protobuf::write_length_delimited_field(&mut out, 4, info.vehicle_id.as_bytes());
    // HeadUnitInfo.head_unit_make (field 5, optional string).
    protobuf::write_length_delimited_field(&mut out, 5, info.head_unit_make.as_bytes());
    // HeadUnitInfo.head_unit_model (field 6, optional string).
    protobuf::write_length_delimited_field(&mut out, 6, info.head_unit_model.as_bytes());
    // HeadUnitInfo.head_unit_software_build (field 7, optional string).
    protobuf::write_length_delimited_field(&mut out, 7, info.head_unit_software_build.as_bytes());
    // HeadUnitInfo.head_unit_software_version (field 8, optional string).
    protobuf::write_length_delimited_field(&mut out, 8, info.head_unit_software_version.as_bytes());
    out
}

fn encode_service(
    channel_id: u8,
    kind: ServiceKind,
    capabilities: &ServiceCapabilities,
) -> Result<Vec<u8>, ServiceDiscoveryResponseError> {
    let mut out = Vec::new();
    // Service.id (field 1, required int32).
    protobuf::write_int32_field(&mut out, 1, i32::from(channel_id));
    match kind {
        ServiceKind::Video => {
            let capabilities = capabilities
                .video
                .as_deref()
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let media_sink_service = encode_media_sink_service(capabilities);
            // Service.media_sink_service (field 3).
            protobuf::write_length_delimited_field(&mut out, 3, &media_sink_service);
        }
        ServiceKind::Input => {
            let capability = capabilities
                .touch
                .clone()
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let input_source_service = encode_input_source_service(&capability);
            // Service.input_source_service (field 4).
            protobuf::write_length_delimited_field(&mut out, 4, &input_source_service);
        }
        ServiceKind::MediaAudio => {
            let capability = capabilities
                .media_audio
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let media_sink_service = encode_media_sink_audio(capability);
            // Service.media_sink_service (field 3) — the same field video
            // uses; MediaSinkService models both roles, distinguished by
            // which of its own fields are populated.
            protobuf::write_length_delimited_field(&mut out, 3, &media_sink_service);
        }
        ServiceKind::SystemAudio => {
            let capability = capabilities
                .system_audio
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let media_sink_service = encode_media_sink_audio(capability);
            protobuf::write_length_delimited_field(&mut out, 3, &media_sink_service);
        }
        ServiceKind::SpeechAudio => {
            let capability = capabilities
                .speech_audio
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let media_sink_service = encode_media_sink_audio(capability);
            protobuf::write_length_delimited_field(&mut out, 3, &media_sink_service);
        }
        ServiceKind::Sensors => {
            let capability = capabilities
                .sensors
                .as_ref()
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let sensor_source_service = encode_sensor_source_service(capability);
            // Service.sensor_source_service (field 2).
            protobuf::write_length_delimited_field(&mut out, 2, &sensor_source_service);
        }
        ServiceKind::Bluetooth => {
            let capability = capabilities
                .bluetooth
                .as_ref()
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let bluetooth_service = encode_bluetooth_service(capability);
            // Service.bluetooth_service (field 6).
            protobuf::write_length_delimited_field(&mut out, 6, &bluetooth_service);
        }
        ServiceKind::Microphone => {
            let capability = capabilities
                .microphone
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let media_source_service = encode_media_source_service(capability);
            // Service.media_source_service (field 5).
            protobuf::write_length_delimited_field(&mut out, 5, &media_source_service);
        }
        ServiceKind::Radio => {
            let capability = capabilities
                .radio
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let radio_service = encode_radio_service(capability);
            // Service.radio_service (field 7).
            protobuf::write_length_delimited_field(&mut out, 7, &radio_service);
        }
    }
    Ok(out)
}

fn encode_media_sink_service(capabilities: &[VideoCapability]) -> Vec<u8> {
    let mut media_sink_service = Vec::new();
    for capability in capabilities {
        let mut video_configuration = Vec::new();
        // VideoConfiguration.codec_resolution (field 1, optional enum).
        protobuf::write_int32_field(
            &mut video_configuration,
            1,
            capability.resolution.wire_value(),
        );
        // VideoConfiguration.frame_rate (field 2, optional enum).
        protobuf::write_int32_field(
            &mut video_configuration,
            2,
            capability.frame_rate.wire_value(),
        );
        if let Some(density) = capability.density {
            // VideoConfiguration.density (field 5, optional uint32).
            protobuf::write_uint32_field(&mut video_configuration, 5, density);
        }
        if let Some(pixel_aspect_ratio_e4) = capability.pixel_aspect_ratio_e4 {
            // VideoConfiguration.pixel_aspect_ratio_e4 (field 8, optional uint32).
            protobuf::write_uint32_field(&mut video_configuration, 8, pixel_aspect_ratio_e4);
        }
        // VideoConfiguration.video_codec_type (field 10, optional enum).
        protobuf::write_int32_field(&mut video_configuration, 10, capability.codec.wire_value());
        if let Some(ui_config) = capability.ui_config {
            let ui_config_bytes = encode_ui_config(ui_config);
            // VideoConfiguration.ui_config (field 11, optional UiConfig).
            protobuf::write_length_delimited_field(&mut video_configuration, 11, &ui_config_bytes);
        }
        // MediaSinkService.video_configs (field 4, repeated VideoConfiguration) —
        // one entry per advertised capability, in list order, since
        // `Start.configuration_index` (`video_setup.rs`) refers back into
        // this list by position.
        protobuf::write_length_delimited_field(&mut media_sink_service, 4, &video_configuration);
    }
    media_sink_service
}

fn encode_insets(insets: Insets) -> Vec<u8> {
    let mut out = Vec::new();
    // Insets.top (field 1, optional uint32).
    protobuf::write_uint32_field(&mut out, 1, insets.top);
    // Insets.bottom (field 2, optional uint32).
    protobuf::write_uint32_field(&mut out, 2, insets.bottom);
    // Insets.left (field 3, optional uint32).
    protobuf::write_uint32_field(&mut out, 3, insets.left);
    // Insets.right (field 4, optional uint32).
    protobuf::write_uint32_field(&mut out, 4, insets.right);
    out
}

fn encode_ui_config(ui_config: UiConfig) -> Vec<u8> {
    let mut out = Vec::new();
    let margins_bytes = encode_insets(ui_config.margins);
    // UiConfig.margins (field 1, optional Insets).
    protobuf::write_length_delimited_field(&mut out, 1, &margins_bytes);
    let content_insets_bytes = encode_insets(ui_config.content_insets);
    // UiConfig.content_insets (field 2, optional Insets).
    protobuf::write_length_delimited_field(&mut out, 2, &content_insets_bytes);
    let stable_content_insets_bytes = encode_insets(ui_config.stable_content_insets);
    // UiConfig.stable_content_insets (field 3, optional Insets).
    protobuf::write_length_delimited_field(&mut out, 3, &stable_content_insets_bytes);
    out
}

fn encode_audio_configuration(
    sampling_rate: u32,
    number_of_bits: u32,
    number_of_channels: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    // AudioConfiguration.sampling_rate (field 1, required uint32).
    protobuf::write_uint32_field(&mut out, 1, sampling_rate);
    // AudioConfiguration.number_of_bits (field 2, required uint32).
    protobuf::write_uint32_field(&mut out, 2, number_of_bits);
    // AudioConfiguration.number_of_channels (field 3, required uint32).
    protobuf::write_uint32_field(&mut out, 3, number_of_channels);
    out
}

fn encode_media_sink_audio(capability: AudioCapability) -> Vec<u8> {
    let audio_configuration = encode_audio_configuration(
        capability.sampling_rate,
        capability.number_of_bits,
        capability.number_of_channels,
    );

    let mut media_sink_service = Vec::new();
    // MediaSinkService.audio_type (field 2, optional enum).
    protobuf::write_int32_field(
        &mut media_sink_service,
        2,
        capability.stream_type.wire_value(),
    );
    // MediaSinkService.audio_configs (field 3, repeated AudioConfiguration).
    protobuf::write_length_delimited_field(&mut media_sink_service, 3, &audio_configuration);
    // MediaSinkService.available_type (field 1, MediaCodecType) is
    // deliberately not written: its documented proto2 default is
    // MEDIA_CODEC_AUDIO_PCM, and proto2 treats an omitted optional field as
    // its default — leaving it unset is correct, not an oversight.
    media_sink_service
}

fn encode_input_source_service(capability: &TouchCapability) -> Vec<u8> {
    let mut touch_screen = Vec::new();
    // TouchScreen.width (field 1, required int32).
    protobuf::write_int32_field(&mut touch_screen, 1, capability.width);
    // TouchScreen.height (field 2, required int32).
    protobuf::write_int32_field(&mut touch_screen, 2, capability.height);
    // TouchScreen.type (field 3, optional enum).
    protobuf::write_int32_field(&mut touch_screen, 3, capability.touch_type.wire_value());

    let mut input_source_service = Vec::new();
    // InputSourceService.keycodes_supported (field 1, repeated packed
    // int32) — written before touchscreen (field 2) to match field
    // declaration order, though wire order doesn't matter to a
    // spec-compliant decoder.
    let keycodes: Vec<u32> = capability
        .keycodes_supported
        .iter()
        .map(|keycode| keycode.wire_value())
        .collect();
    protobuf::write_packed_uint32_field(&mut input_source_service, 1, &keycodes);
    // InputSourceService.touchscreen (field 2, repeated TouchScreen).
    protobuf::write_length_delimited_field(&mut input_source_service, 2, &touch_screen);
    input_source_service
}

fn encode_sensor_source_service(capability: &SensorCapability) -> Vec<u8> {
    let mut out = Vec::new();
    for &sensor_type in &capability.sensor_types {
        let mut sensor = Vec::new();
        // Sensor.sensor_type (field 1, required enum).
        protobuf::write_int32_field(&mut sensor, 1, sensor_type.wire_value());
        // SensorSourceService.sensors (field 1, repeated Sensor).
        protobuf::write_length_delimited_field(&mut out, 1, &sensor);
    }
    out
}

fn encode_bluetooth_service(capability: &BluetoothCapability) -> Vec<u8> {
    let mut out = Vec::new();
    // BluetoothService.car_address (field 1, required string).
    protobuf::write_length_delimited_field(&mut out, 1, capability.car_address.as_bytes());
    // BluetoothService.supported_pairing_methods (field 2, repeated packed
    // enum) is deliberately left empty — nothing to advertise yet.
    out
}

fn encode_radio_service(capability: RadioCapability) -> Vec<u8> {
    let mut radio_properties = Vec::new();
    // RadioProperties.radio_id (field 1, required int32).
    protobuf::write_int32_field(&mut radio_properties, 1, capability.radio_id);
    // RadioProperties.type (field 2, required enum).
    protobuf::write_int32_field(&mut radio_properties, 2, capability.radio_type.wire_value());
    // RadioProperties.channel_spacing (field 5, required int32) — fields
    // 3/4 (channel_range/channel_spacings) are both `repeated` and left
    // empty; every optional field (6-14) is omitted.
    protobuf::write_int32_field(&mut radio_properties, 5, capability.channel_spacing);

    let mut out = Vec::new();
    // RadioService.radio_properties (field 1, repeated RadioProperties).
    protobuf::write_length_delimited_field(&mut out, 1, &radio_properties);
    out
}

fn encode_media_source_service(capability: MicrophoneCapability) -> Vec<u8> {
    let audio_config = encode_audio_configuration(
        capability.sampling_rate,
        capability.number_of_bits,
        capability.number_of_channels,
    );
    let mut out = Vec::new();
    // MediaSourceService.audio_config (field 2, optional AudioConfiguration).
    protobuf::write_length_delimited_field(&mut out, 2, &audio_config);
    // MediaSourceService.available_type (field 1, MediaCodecType) is left
    // unset for the same reason as MediaSinkService's: its documented
    // proto2 default is MEDIA_CODEC_AUDIO_PCM.
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protobuf::{ProtobufDecodeError, read_length_delimited, read_tag};
    use crate::service_catalogue::{ServiceAvailability, ServiceCandidate};

    #[derive(Debug, Eq, PartialEq)]
    struct TestDecodeError;

    impl ProtobufDecodeError for TestDecodeError {
        fn truncated() -> Self {
            Self
        }
        fn invalid_varint() -> Self {
            Self
        }
        fn invalid_field_number() -> Self {
            Self
        }
        fn length_not_representable() -> Self {
            Self
        }
        fn unsupported_wire_type(_wire_type: u8) -> Self {
            Self
        }
    }

    fn catalogue(candidates: &[ServiceCandidate]) -> ServiceCatalogue {
        ServiceCatalogue::build(candidates, 32).expect("catalogue")
    }

    #[test]
    fn encodes_a_single_video_service_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 1,
            kind: ServiceKind::Video,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            video: Some(vec![VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
                codec: VideoCodecType::H264,
                density: None,
                pixel_aspect_ratio_e4: None,
                ui_config: None,
            }]),
            touch: None,
            media_audio: None,
            system_audio: None,
            speech_audio: None,
            bluetooth: None,
            microphone: None,
            sensors: None,
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(message.id, ControlMessageId::ServiceDiscoveryResponse);
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0c, // channels (field 1), length 12
                0x08, 0x01, // Service.id = 1
                0x1a, 0x08, // Service.media_sink_service (field 3), length 8
                0x22, 0x06, // MediaSinkService.video_configs (field 4), length 6
                0x08, 0x01, // VideoConfiguration.codec_resolution = 1 (800x480)
                0x10, 0x02, // VideoConfiguration.frame_rate = 2 (30fps)
                0x50, 0x03, // VideoConfiguration.video_codec_type = 3 (H264 BP)
            ]
        );
    }

    #[test]
    fn encodes_pixel_aspect_ratio_e4_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 1,
            kind: ServiceKind::Video,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            video: Some(vec![VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
                codec: VideoCodecType::H264,
                density: None,
                pixel_aspect_ratio_e4: Some(10000),
                ui_config: None,
            }]),
            touch: None,
            media_audio: None,
            system_audio: None,
            speech_audio: None,
            bluetooth: None,
            microphone: None,
            sensors: None,
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0f, // channels (field 1), length 15
                0x08, 0x01, // Service.id = 1
                0x1a, 0x0b, // Service.media_sink_service (field 3), length 11
                0x22, 0x09, // MediaSinkService.video_configs (field 4), length 9
                0x08, 0x01, // VideoConfiguration.codec_resolution = 1 (800x480)
                0x10, 0x02, // VideoConfiguration.frame_rate = 2 (30fps)
                0x40, 0x90, 0x4e, // VideoConfiguration.pixel_aspect_ratio_e4 = 10000
                0x50, 0x03, // VideoConfiguration.video_codec_type = 3 (H264 BP)
            ]
        );
    }

    #[test]
    fn encodes_density_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 1,
            kind: ServiceKind::Video,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            video: Some(vec![VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
                codec: VideoCodecType::H264,
                density: Some(180),
                pixel_aspect_ratio_e4: None,
                ui_config: None,
            }]),
            touch: None,
            media_audio: None,
            system_audio: None,
            speech_audio: None,
            bluetooth: None,
            microphone: None,
            sensors: None,
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0f, // channels (field 1), length 15
                0x08, 0x01, // Service.id = 1
                0x1a, 0x0b, // Service.media_sink_service (field 3), length 11
                0x22, 0x09, // MediaSinkService.video_configs (field 4), length 9
                0x08, 0x01, // VideoConfiguration.codec_resolution = 1 (800x480)
                0x10, 0x02, // VideoConfiguration.frame_rate = 2 (30fps)
                0x28, 0xb4, 0x01, // VideoConfiguration.density = 180
                0x50, 0x03, // VideoConfiguration.video_codec_type = 3 (H264 BP)
            ]
        );
    }

    #[test]
    fn encodes_insets_with_exact_bytes() {
        let insets = Insets {
            top: 1,
            bottom: 2,
            left: 3,
            right: 4,
        };
        assert_eq!(
            encode_insets(insets),
            vec![
                0x08, 0x01, // top = 1
                0x10, 0x02, // bottom = 2
                0x18, 0x03, // left = 3
                0x20, 0x04, // right = 4
            ]
        );
    }

    #[test]
    fn encodes_ui_config_with_all_zero_insets() {
        let zero_insets_bytes = [0x08, 0x00, 0x10, 0x00, 0x18, 0x00, 0x20, 0x00];
        let expected: Vec<u8> = [
            &[0x0a, 0x08][..],      // margins (field 1), length 8
            &zero_insets_bytes[..], // Insets.{top,bottom,left,right} = 0
            &[0x12, 0x08][..],      // content_insets (field 2), length 8
            &zero_insets_bytes[..],
            &[0x1a, 0x08][..], // stable_content_insets (field 3), length 8
            &zero_insets_bytes[..],
        ]
        .concat();
        assert_eq!(encode_ui_config(UiConfig::default()), expected);
    }

    #[test]
    fn encodes_a_single_input_service_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 2,
            kind: ServiceKind::Input,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            video: None,
            touch: Some(TouchCapability {
                width: 2,
                height: 3,
                touch_type: TouchScreenType::Capacitive,
                keycodes_supported: Vec::new(),
            }),
            media_audio: None,
            system_audio: None,
            speech_audio: None,
            bluetooth: None,
            microphone: None,
            sensors: None,
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0c, // channels (field 1), length 12
                0x08, 0x02, // Service.id = 2
                0x22, 0x08, // Service.input_source_service (field 4), length 8
                0x12, 0x06, // InputSourceService.touchscreen (field 2), length 6
                0x08, 0x02, // TouchScreen.width = 2
                0x10, 0x03, // TouchScreen.height = 3
                0x18, 0x01, // TouchScreen.type = 1 (capacitive)
            ]
        );
    }

    #[test]
    fn encodes_a_single_media_audio_service_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 3,
            kind: ServiceKind::MediaAudio,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            video: None,
            touch: None,
            media_audio: Some(AudioCapability {
                sampling_rate: 48_000,
                number_of_bits: 16,
                number_of_channels: 2,
                stream_type: AudioStreamType::Media,
            }),
            system_audio: None,
            speech_audio: None,
            bluetooth: None,
            microphone: None,
            sensors: None,
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x10, // channels (field 1), length 16
                0x08, 0x03, // Service.id = 3
                0x1a, 0x0c, // Service.media_sink_service (field 3), length 12
                0x10, 0x03, // MediaSinkService.audio_type = 3 (media)
                0x1a, 0x08, // MediaSinkService.audio_configs (field 3), length 8
                0x08, 0x80, 0xf7, 0x02, // AudioConfiguration.sampling_rate = 48000
                0x10, 0x10, // AudioConfiguration.number_of_bits = 16
                0x18, 0x02, // AudioConfiguration.number_of_channels = 2
            ]
        );
    }

    #[test]
    fn encodes_realistic_video_and_touch_dimensions_with_well_formed_structure() {
        // Multi-byte-varint dimensions (an 800x480 real touchscreen) are
        // checked structurally by walking the tag/length framing with the
        // same read primitives the rest of this crate already trusts,
        // rather than by hand-expanding every varint byte.
        let catalogue = catalogue(&[
            ServiceCandidate {
                channel_id: 1,
                kind: ServiceKind::Video,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: 2,
                kind: ServiceKind::Input,
                availability: ServiceAvailability::Ready,
            },
        ]);
        let capabilities = ServiceCapabilities {
            video: Some(vec![VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
                codec: VideoCodecType::H264,
                density: None,
                pixel_aspect_ratio_e4: None,
                ui_config: None,
            }]),
            touch: Some(TouchCapability {
                width: 800,
                height: 480,
                touch_type: TouchScreenType::Capacitive,
                keycodes_supported: Vec::new(),
            }),
            media_audio: None,
            system_audio: None,
            speech_audio: None,
            bluetooth: None,
            microphone: None,
            sensors: None,
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");

        let mut cursor = 0;
        let mut channel_count = 0;
        while cursor < message.body.len() {
            let (field, wire_type) =
                read_tag::<TestDecodeError>(&message.body, &mut cursor).expect("tag");
            assert_eq!((field, wire_type), (1, 2));
            let channel = read_length_delimited::<TestDecodeError>(&message.body, &mut cursor)
                .expect("channel bytes");
            assert!(!channel.is_empty());
            channel_count += 1;
        }
        assert_eq!(channel_count, 2);
    }

    #[test]
    fn encodes_head_unit_info_when_present() {
        let catalogue = catalogue(&[]);
        let capabilities = ServiceCapabilities {
            head_unit_info: Some(HeadUnitInfo {
                make: "a".into(),
                model: "b".into(),
                year: "c".into(),
                vehicle_id: "d".into(),
                head_unit_make: "e".into(),
                head_unit_model: "f".into(),
                head_unit_software_build: "g".into(),
                head_unit_software_version: "h".into(),
            }),
            ..ServiceCapabilities::default()
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x8a, 0x01, 0x18, // headunit_info (field 17), length 24
                0x0a, 0x01, b'a', // make
                0x12, 0x01, b'b', // model
                0x1a, 0x01, b'c', // year
                0x22, 0x01, b'd', // vehicle_id
                0x2a, 0x01, b'e', // head_unit_make
                0x32, 0x01, b'f', // head_unit_model
                0x3a, 0x01, b'g', // head_unit_software_build
                0x42, 0x01, b'h', // head_unit_software_version
            ]
        );
    }

    #[test]
    fn encodes_ping_configuration_when_present() {
        let catalogue = catalogue(&[]);
        let capabilities = ServiceCapabilities {
            ping_configuration: Some(PingConfiguration {
                timeout_ms: 5000,
                interval_ms: 1500,
                high_latency_threshold_ms: 500,
                tracked_ping_count: 5,
            }),
            ..ServiceCapabilities::default()
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x82, 0x01, 0x0d, // connection_configuration (field 16), length 13
                0x0a, 0x0b, // ConnectionConfiguration.ping_configuration (field 1), length 11
                0x08, 0x88, 0x27, // PingConfiguration.timeout_ms (field 1) = 5000
                0x10, 0xdc, 0x0b, // PingConfiguration.interval_ms (field 2) = 1500
                0x18, 0xf4,
                0x03, // PingConfiguration.high_latency_threshold_ms (field 3) = 500
                0x20, 0x05, // PingConfiguration.tracked_ping_count (field 4) = 5
            ]
        );
    }

    #[test]
    fn encodes_sensors_with_driving_status_and_night_mode() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 6,
            kind: ServiceKind::Sensors,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            sensors: Some(SensorCapability {
                sensor_types: vec![SensorType::DrivingStatusData, SensorType::NightMode],
            }),
            ..ServiceCapabilities::default()
        };
        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0c, // channels (field 1), length 12
                0x08, 0x06, // Service.id = 6
                0x12, 0x08, // Service.sensor_source_service (field 2), length 8
                0x0a, 0x02, 0x08, 0x0d, // SensorSourceService.sensors[0] = {sensor_type: 13}
                0x0a, 0x02, 0x08, 0x0a, // SensorSourceService.sensors[1] = {sensor_type: 10}
            ]
        );
    }

    #[test]
    fn missing_sensor_capability_fails_closed() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 6,
            kind: ServiceKind::Sensors,
            availability: ServiceAvailability::Ready,
        }]);
        assert_eq!(
            encode_service_discovery_response(&catalogue, &ServiceCapabilities::default()),
            Err(ServiceDiscoveryResponseError::MissingCapability {
                channel_id: 6,
                kind: ServiceKind::Sensors,
            })
        );
    }

    #[test]
    fn encodes_bluetooth_service_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 7,
            kind: ServiceKind::Bluetooth,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            bluetooth: Some(BluetoothCapability {
                car_address: "ab".into(),
            }),
            ..ServiceCapabilities::default()
        };
        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x08, // channels (field 1), length 8
                0x08, 0x07, // Service.id = 7
                0x32, 0x04, // Service.bluetooth_service (field 6), length 4
                0x0a, 0x02, b'a', b'b', // BluetoothService.car_address = "ab"
            ]
        );
    }

    #[test]
    fn encodes_radio_service_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 9,
            kind: ServiceKind::Radio,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            radio: Some(RadioCapability {
                radio_id: 5,
                radio_type: RadioType::FmRadio,
                channel_spacing: 100,
            }),
            ..ServiceCapabilities::default()
        };
        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0c, // channels (field 1), length 12
                0x08, 0x09, // Service.id = 9
                0x3a, 0x08, // Service.radio_service (field 7), length 8
                0x0a, 0x06, // RadioService.radio_properties (field 1), length 6
                0x08, 0x05, // RadioProperties.radio_id = 5
                0x10, 0x01, // RadioProperties.type = FM_RADIO (1)
                0x28, 0x64, // RadioProperties.channel_spacing = 100
            ]
        );
    }

    #[test]
    fn missing_radio_capability_fails_closed() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 9,
            kind: ServiceKind::Radio,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities::default();
        let error = encode_service_discovery_response(&catalogue, &capabilities).unwrap_err();
        assert_eq!(
            error,
            ServiceDiscoveryResponseError::MissingCapability {
                channel_id: 9,
                kind: ServiceKind::Radio,
            }
        );
    }

    #[test]
    fn encodes_microphone_service_with_exact_bytes() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 8,
            kind: ServiceKind::Microphone,
            availability: ServiceAvailability::Ready,
        }]);
        let capabilities = ServiceCapabilities {
            microphone: Some(MicrophoneCapability {
                sampling_rate: 1,
                number_of_bits: 2,
                number_of_channels: 3,
            }),
            ..ServiceCapabilities::default()
        };
        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");
        assert_eq!(
            message.body,
            vec![
                0x0a, 0x0c, // channels (field 1), length 12
                0x08, 0x08, // Service.id = 8
                0x2a, 0x08, // Service.media_source_service (field 5), length 8
                0x12, 0x06, // MediaSourceService.audio_config (field 2), length 6
                0x08, 0x01, // AudioConfiguration.sampling_rate = 1
                0x10, 0x02, // AudioConfiguration.number_of_bits = 2
                0x18, 0x03, // AudioConfiguration.number_of_channels = 3
            ]
        );
    }

    #[test]
    fn encodes_all_eight_service_kinds_with_well_formed_structure() {
        // Mirrors encodes_realistic_video_and_touch_dimensions_with_well_formed_structure:
        // structural verification (tag/length framing) rather than a fully
        // hand-expanded byte vector, since eight services makes the latter
        // unreadable without adding verification value beyond what the
        // per-kind exact-byte tests above already prove.
        let kinds = [
            ServiceKind::Video,
            ServiceKind::Input,
            ServiceKind::MediaAudio,
            ServiceKind::SystemAudio,
            ServiceKind::SpeechAudio,
            ServiceKind::Sensors,
            ServiceKind::Bluetooth,
            ServiceKind::Microphone,
        ];
        let candidates: Vec<_> = kinds
            .iter()
            .enumerate()
            .map(|(index, &kind)| ServiceCandidate {
                #[allow(clippy::cast_possible_truncation)]
                channel_id: (index + 1) as u8,
                kind,
                availability: ServiceAvailability::Ready,
            })
            .collect();
        let catalogue = catalogue(&candidates);
        let capabilities = ServiceCapabilities {
            video: Some(vec![VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
                codec: VideoCodecType::H264,
                density: None,
                pixel_aspect_ratio_e4: None,
                ui_config: None,
            }]),
            touch: Some(TouchCapability {
                width: 800,
                height: 480,
                touch_type: TouchScreenType::Capacitive,
                keycodes_supported: Vec::new(),
            }),
            media_audio: Some(AudioCapability {
                sampling_rate: 48_000,
                number_of_bits: 16,
                number_of_channels: 2,
                stream_type: AudioStreamType::Media,
            }),
            system_audio: Some(AudioCapability {
                sampling_rate: 16_000,
                number_of_bits: 16,
                number_of_channels: 1,
                stream_type: AudioStreamType::SystemAudio,
            }),
            speech_audio: Some(AudioCapability {
                sampling_rate: 16_000,
                number_of_bits: 16,
                number_of_channels: 1,
                stream_type: AudioStreamType::Guidance,
            }),
            bluetooth: Some(BluetoothCapability {
                car_address: "02:00:00:00:00:01".into(),
            }),
            microphone: Some(MicrophoneCapability {
                sampling_rate: 16_000,
                number_of_bits: 16,
                number_of_channels: 1,
            }),
            sensors: Some(SensorCapability {
                sensor_types: vec![SensorType::DrivingStatusData, SensorType::NightMode],
            }),
            radio: None,
            head_unit_info: None,
            ping_configuration: None,
        };

        let message = encode_service_discovery_response(&catalogue, &capabilities).expect("encode");

        let mut cursor = 0;
        let mut channel_count = 0;
        while cursor < message.body.len() {
            let (field, wire_type) =
                read_tag::<TestDecodeError>(&message.body, &mut cursor).expect("tag");
            assert_eq!((field, wire_type), (1, 2));
            let channel = read_length_delimited::<TestDecodeError>(&message.body, &mut cursor)
                .expect("channel bytes");
            assert!(!channel.is_empty());
            channel_count += 1;
        }
        assert_eq!(channel_count, 8);
    }

    #[test]
    fn rejects_missing_capability_for_an_advertised_kind() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 1,
            kind: ServiceKind::Video,
            availability: ServiceAvailability::Ready,
        }]);
        assert_eq!(
            encode_service_discovery_response(&catalogue, &ServiceCapabilities::default()),
            Err(ServiceDiscoveryResponseError::MissingCapability {
                channel_id: 1,
                kind: ServiceKind::Video,
            })
        );
    }

    #[test]
    fn encodes_an_empty_catalogue_as_an_empty_body() {
        let catalogue = catalogue(&[]);
        let message =
            encode_service_discovery_response(&catalogue, &ServiceCapabilities::default())
                .expect("encode");
        assert!(message.body.is_empty());
    }
}
