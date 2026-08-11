use std::fmt;

use crate::control::{ControlMessage, ControlMessageId};
use crate::protobuf;
use crate::service_catalogue::{ServiceCatalogue, ServiceKind};

// Portions derived from AASDK's Service/ServiceDiscoveryResponse protobuf
// schema at the pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1);
// field numbers and enum values match the mapping recorded in
// docs/protocol/aasdk-adoption.md.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
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

/// `aap_protobuf.service.media.shared.message.MediaCodecType.MEDIA_CODEC_VIDEO_H264_BP`
/// — the only codec this encoder ever advertises. Not caller-configurable
/// yet; matches the Pi 5 software-H.264 fallback path already selected in
/// `ARCHITECTURE.md`/M3. Extending to other codecs later is a new field
/// addition to [`VideoCapability`], not a redesign.
const MEDIA_CODEC_VIDEO_H264_BP: i32 = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoCapability {
    pub resolution: VideoCodecResolution,
    pub frame_rate: VideoFrameRate,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchCapability {
    pub width: i32,
    pub height: i32,
    pub touch_type: TouchScreenType,
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

/// Capability data for the services [`encode_service_discovery_response`]
/// knows how to encode. Kept separate from [`ServiceCatalogue`], which stays
/// wire-neutral per `ARCHITECTURE.md` §4.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServiceCapabilities {
    pub video: Option<VideoCapability>,
    pub touch: Option<TouchCapability>,
    pub media_audio: Option<AudioCapability>,
    pub head_unit_info: Option<HeadUnitInfo>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceDiscoveryResponseError {
    UnsupportedServiceKind(ServiceKind),
    MissingCapability { channel_id: u8, kind: ServiceKind },
}

impl fmt::Display for ServiceDiscoveryResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedServiceKind(kind) => {
                write!(
                    formatter,
                    "service discovery response encoding does not support {kind:?} yet"
                )
            }
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
/// payload. Only [`ServiceKind::Video`], [`ServiceKind::Input`], and
/// [`ServiceKind::MediaAudio`] are supported; any other kind in the
/// catalogue fails closed with
/// [`ServiceDiscoveryResponseError::UnsupportedServiceKind`] rather than
/// being silently dropped from the response.
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
            let capability = capabilities
                .video
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let media_sink_service = encode_media_sink_service(capability);
            // Service.media_sink_service (field 3).
            protobuf::write_length_delimited_field(&mut out, 3, &media_sink_service);
        }
        ServiceKind::Input => {
            let capability = capabilities
                .touch
                .ok_or(ServiceDiscoveryResponseError::MissingCapability { channel_id, kind })?;
            let input_source_service = encode_input_source_service(capability);
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
        other => return Err(ServiceDiscoveryResponseError::UnsupportedServiceKind(other)),
    }
    Ok(out)
}

fn encode_media_sink_service(capability: VideoCapability) -> Vec<u8> {
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
    // VideoConfiguration.video_codec_type (field 10, optional enum).
    protobuf::write_int32_field(&mut video_configuration, 10, MEDIA_CODEC_VIDEO_H264_BP);

    let mut media_sink_service = Vec::new();
    // MediaSinkService.video_configs (field 4, repeated VideoConfiguration).
    protobuf::write_length_delimited_field(&mut media_sink_service, 4, &video_configuration);
    media_sink_service
}

fn encode_media_sink_audio(capability: AudioCapability) -> Vec<u8> {
    let mut audio_configuration = Vec::new();
    // AudioConfiguration.sampling_rate (field 1, required uint32).
    protobuf::write_uint32_field(&mut audio_configuration, 1, capability.sampling_rate);
    // AudioConfiguration.number_of_bits (field 2, required uint32).
    protobuf::write_uint32_field(&mut audio_configuration, 2, capability.number_of_bits);
    // AudioConfiguration.number_of_channels (field 3, required uint32).
    protobuf::write_uint32_field(&mut audio_configuration, 3, capability.number_of_channels);

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

fn encode_input_source_service(capability: TouchCapability) -> Vec<u8> {
    let mut touch_screen = Vec::new();
    // TouchScreen.width (field 1, required int32).
    protobuf::write_int32_field(&mut touch_screen, 1, capability.width);
    // TouchScreen.height (field 2, required int32).
    protobuf::write_int32_field(&mut touch_screen, 2, capability.height);
    // TouchScreen.type (field 3, optional enum).
    protobuf::write_int32_field(&mut touch_screen, 3, capability.touch_type.wire_value());

    let mut input_source_service = Vec::new();
    // InputSourceService.touchscreen (field 2, repeated TouchScreen).
    protobuf::write_length_delimited_field(&mut input_source_service, 2, &touch_screen);
    input_source_service
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
            video: Some(VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
            }),
            touch: None,
            media_audio: None,
            head_unit_info: None,
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
            }),
            media_audio: None,
            head_unit_info: None,
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
            head_unit_info: None,
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
            video: Some(VideoCapability {
                resolution: VideoCodecResolution::Video800x480,
                frame_rate: VideoFrameRate::Fps30,
            }),
            touch: Some(TouchCapability {
                width: 800,
                height: 480,
                touch_type: TouchScreenType::Capacitive,
            }),
            media_audio: None,
            head_unit_info: None,
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
            video: None,
            touch: None,
            media_audio: None,
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
    fn rejects_unsupported_service_kinds() {
        let catalogue = catalogue(&[ServiceCandidate {
            channel_id: 1,
            kind: ServiceKind::Sensors,
            availability: ServiceAvailability::Ready,
        }]);
        assert_eq!(
            encode_service_discovery_response(&catalogue, &ServiceCapabilities::default()),
            Err(ServiceDiscoveryResponseError::UnsupportedServiceKind(
                ServiceKind::Sensors
            ))
        );
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
