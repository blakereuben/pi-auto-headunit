use std::fmt;

use crate::media_message::{
    DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, MediaMessage, MediaMessageError, MediaMessageId,
    decode_media_data, encode_media_ack,
};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's Setup/Config/Start and
// VideoFocusNotification/VideoFocusMode protobuf schema
// (protobuf/aap_protobuf/service/media/shared/message/,
// protobuf/aap_protobuf/service/media/video/message/) and the video
// channel's setup dispatch behaviour in VideoMediaSinkService.cpp, at the
// pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1): the
// phone sends Setup, the head unit replies Config, the phone sends Start —
// confirmed against that source, not any public specification (none is
// known to exist for this wire protocol). See the channel-setup design
// record for the full provenance trail.
//
// The proactive, unsolicited VideoFocusNotification send after Config is
// derived from a separate, independently implemented, GPL-3.0-or-later
// Android Auto client (`f-io/LIVI` revision
// 9000f308eec423c5c56ac0a14491a7c95ce5762d,
// `src/main/services/projection/driver/aa/stack/session/Session.ts`, not
// AASDK-derived), formally adopted per
// `docs/protocol/livi-adoption.md` ("Adopted scope" item 1). No LIVI code
// is reproduced here; only the wire message/field shape, itself confirmed
// byte-for-byte against this project's own pinned AASDK schema above, and
// the behavioural rule of sending it unconditionally after Config.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// Copyright (C) 2024-2026 Open Android Auto contributors (LIVI)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.media.shared.message.MediaCodecType.MEDIA_CODEC_VIDEO_H264_BP`.
/// Matches `ADVERTISED_H264_CONFIGURATION_INDEX`'s position in
/// `ServiceDiscoveryResponse.video_configs` — see
/// `crates/protocol-aap/src/service_discovery_response.rs`'s
/// `VideoCodecType`.
const MEDIA_CODEC_VIDEO_H264_BP: i32 = 3;
/// `aap_protobuf.service.media.shared.message.MediaCodecType.MEDIA_CODEC_VIDEO_H265`.
/// Advertised alongside H.264 following real-hardware evidence that the
/// same phone actively selects it when offered — see
/// `docs/protocol/error-2-investigation.md`, "LIVI known-good capture".
/// Matches `ADVERTISED_H265_CONFIGURATION_INDEX`'s position.
const MEDIA_CODEC_VIDEO_H265: i32 = 7;

/// `Config`'s nested `Status` enum, `STATUS_READY = 2`. `STATUS_WAIT` (1)
/// is never sent this increment — there is no reason yet to ask the phone
/// to wait.
const CONFIG_STATUS_READY: i32 = 2;

/// A deliberately conservative placeholder for `Config.max_unacked`. The
/// real ack/backpressure chain (`MEDIA_MESSAGE_ACK`) is out of scope this
/// increment; this only shapes what capacity is advertised.
const DEFAULT_VIDEO_MAX_UNACKED: i32 = 1;

/// Position of the H.264 `VideoConfiguration` entry in
/// `ServiceDiscoveryResponse.video_configs` — this project's own advertised
/// list order, not a protocol constant. Must stay in sync with how
/// `build_service_capabilities()` (`auth_discovery_probe.rs`) orders its
/// `Vec<VideoCapability>`.
const ADVERTISED_H264_CONFIGURATION_INDEX: u32 = 0;
/// Position of the H.265 `VideoConfiguration` entry — see
/// `ADVERTISED_H264_CONFIGURATION_INDEX`'s doc comment.
const ADVERTISED_H265_CONFIGURATION_INDEX: u32 = 1;

/// `aap_protobuf.service.media.video.message.VideoFocusMode`. Only
/// `Projected` is ever sent this increment (the head unit proactively
/// grants the phone video focus, matching `f-io/LIVI`'s
/// `Session.ts`/`VideoFocusIndication` behavior — confirmed against this
/// project's own pinned AASDK source too,
/// `service/media/video/message/VideoFocusMode.proto`), but the enum is
/// modeled fully since it's small and nothing here decodes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoFocusMode {
    Projected,
    Native,
    NativeTransient,
    ProjectedNoInputFocus,
    Unknown(i32),
}

impl VideoFocusMode {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Projected => 1,
            Self::Native => 2,
            Self::NativeTransient => 3,
            Self::ProjectedNoInputFocus => 4,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoSetupState {
    AwaitingSetup,
    AwaitingStart,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoSetupEvent<'a> {
    /// Payload of a `MessageType::Specific`-flagged frame on the video
    /// channel.
    InboundMedia(&'a [u8]),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoSetupAction {
    SendMedia(MediaMessage),
    /// `Start` was received and accepted — the channel is now `Ready` and
    /// stays `Ready`; further `InboundMedia` events keep flowing to
    /// `handle_media` rather than being rejected.
    Ready {
        session_id: i32,
        configuration_index: u32,
    },
    /// A `Data` message arrived while `Ready` — the phone (a
    /// `MediaSinkService`'s source) sending an actual encoded video frame
    /// to the head unit. `payload` is the encoded-frame payload with the
    /// 8-byte timestamp prefix already stripped; owned so it can outlive
    /// the message that carried it (e.g. to hand it to a decode/render
    /// pipeline). Callers must never log it whole — only its length.
    MediaDataReceived {
        timestamp: u64,
        payload: Vec<u8>,
    },
    /// A `CodecConfig` message arrived while `Ready` — out-of-band codec
    /// initialization data (e.g. SPS/PPS), no timestamp prefix. `payload`
    /// is the raw body, owned for the same reason as `MediaDataReceived`.
    /// Callers must never log it whole — only its length.
    CodecConfigReceived {
        payload: Vec<u8>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoSetupError {
    UnexpectedEvent {
        state: VideoSetupState,
    },
    UnexpectedMediaMessage {
        state: VideoSetupState,
        id: MediaMessageId,
    },
    UnsupportedCodec {
        requested: i32,
    },
    UnknownConfigurationIndex {
        requested: u32,
    },
    MissingCodecType,
    MissingSessionId,
    MissingConfigurationIndex,
    TruncatedMediaData {
        available: usize,
    },
    UnexpectedWireType {
        field: u32,
        wire_type: u8,
    },
    Envelope(MediaMessageError),
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for VideoSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEvent { state } => {
                write!(formatter, "unexpected video-setup event in state {state:?}")
            }
            Self::UnexpectedMediaMessage { state, id } => write!(
                formatter,
                "unexpected media message {id:?} in state {state:?}"
            ),
            Self::UnsupportedCodec { requested } => write!(
                formatter,
                "phone requested unsupported codec type {requested}"
            ),
            Self::UnknownConfigurationIndex { requested } => write!(
                formatter,
                "phone started unknown configuration index {requested}"
            ),
            Self::MissingCodecType => {
                formatter.write_str("Setup is missing its required type field")
            }
            Self::MissingSessionId => {
                formatter.write_str("Start is missing its required session_id field")
            }
            Self::MissingConfigurationIndex => {
                formatter.write_str("Start is missing its required configuration_index field")
            }
            Self::TruncatedMediaData { available } => write!(
                formatter,
                "Data requires an 8-byte timestamp prefix, {available} available"
            ),
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "video-setup field {field} has unexpected wire type {wire_type}"
            ),
            Self::Envelope(error) => write!(formatter, "{error}"),
            Self::Truncated => formatter.write_str("truncated video-setup protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid video-setup protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("video-setup protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("video-setup field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for VideoSetupError {}

impl ProtobufDecodeError for VideoSetupError {
    fn truncated() -> Self {
        Self::Truncated
    }
    fn invalid_varint() -> Self {
        Self::InvalidVarint
    }
    fn invalid_field_number() -> Self {
        Self::InvalidFieldNumber
    }
    fn length_not_representable() -> Self {
        Self::LengthNotRepresentable
    }
    fn unsupported_wire_type(wire_type: u8) -> Self {
        Self::UnsupportedWireType(wire_type)
    }
}

/// The video channel's `Setup`→`Config`→`Start` handshake, driven only
/// after that channel's [`crate::ChannelOpenStateMachine`] has reached
/// `Open`. Input/touch needs no equivalent — confirmed from
/// `InputSourceService.cpp`: touch events are head-unit→phone only, so
/// that channel is ready as soon as it opens.
#[derive(Debug)]
pub struct VideoSetupStateMachine {
    state: VideoSetupState,
    /// Captured from `Start`; needed to echo back in `Ack` once `Ready`.
    /// Always `Some` once `state` is `Ready` — the only path into `Ready`
    /// is `handle_start`, which sets both in the same step.
    session_id: Option<i32>,
    /// The single configuration index `Config` offered back in response to
    /// `Setup`'s requested codec — set by `handle_setup`, checked by
    /// `handle_start` so a `Start` can only reference the exact index this
    /// session was actually offered, not just any globally-known one.
    offered_configuration_index: Option<u32>,
}

impl Default for VideoSetupStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoSetupStateMachine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: VideoSetupState::AwaitingSetup,
            session_id: None,
            offered_configuration_index: None,
        }
    }

    #[must_use]
    pub const fn state(&self) -> VideoSetupState {
        self.state
    }

    pub fn advance(
        &mut self,
        event: VideoSetupEvent<'_>,
    ) -> Result<Vec<VideoSetupAction>, VideoSetupError> {
        let result = self.advance_inner(event);
        if result.is_err() {
            self.state = VideoSetupState::Failed;
        }
        result
    }

    fn advance_inner(
        &mut self,
        event: VideoSetupEvent<'_>,
    ) -> Result<Vec<VideoSetupAction>, VideoSetupError> {
        let VideoSetupEvent::InboundMedia(payload) = event;
        let message = MediaMessage::decode(payload, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
            .map_err(VideoSetupError::Envelope)?;
        match self.state {
            VideoSetupState::AwaitingSetup => self.handle_setup(&message),
            VideoSetupState::AwaitingStart => self.handle_start(&message),
            VideoSetupState::Ready => self.handle_media(&message),
            VideoSetupState::Failed => Err(VideoSetupError::UnexpectedEvent { state: self.state }),
        }
    }

    fn handle_setup(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<VideoSetupAction>, VideoSetupError> {
        if message.id != MediaMessageId::Setup {
            return Err(VideoSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: message.id,
            });
        }
        let codec_type = decode_setup(&message.body)?;
        let offered_index = match codec_type {
            MEDIA_CODEC_VIDEO_H264_BP => ADVERTISED_H264_CONFIGURATION_INDEX,
            MEDIA_CODEC_VIDEO_H265 => ADVERTISED_H265_CONFIGURATION_INDEX,
            _ => {
                return Err(VideoSetupError::UnsupportedCodec {
                    requested: codec_type,
                });
            }
        };
        self.offered_configuration_index = Some(offered_index);
        self.state = VideoSetupState::AwaitingStart;
        let mut body = Vec::new();
        // Config.status (field 1, required nested Status enum).
        protobuf::write_int32_field(&mut body, 1, CONFIG_STATUS_READY);
        // Config.max_unacked (field 2, optional uint32).
        protobuf::write_int32_field(&mut body, 2, DEFAULT_VIDEO_MAX_UNACKED);
        // Config.configuration_indices (field 3, repeated uint32, non-packed)
        // — only the single index matching the requested codec, not every
        // advertised index.
        #[allow(clippy::cast_possible_wrap)]
        let advertised_index = offered_index as i32;
        protobuf::write_int32_field(&mut body, 3, advertised_index);
        Ok(vec![
            VideoSetupAction::SendMedia(MediaMessage {
                id: MediaMessageId::Config,
                body,
            }),
            // Proactive, unsolicited video-focus grant — not a reply to
            // anything the phone sent. See this file's module doc comment
            // and `encode_video_focus_notification` for provenance.
            VideoSetupAction::SendMedia(encode_video_focus_notification(VideoFocusMode::Projected)),
        ])
    }

    fn handle_start(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<VideoSetupAction>, VideoSetupError> {
        if message.id != MediaMessageId::Start {
            return Err(VideoSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: message.id,
            });
        }
        let (session_id, configuration_index) = decode_start(&message.body)?;
        if Some(configuration_index) != self.offered_configuration_index {
            return Err(VideoSetupError::UnknownConfigurationIndex {
                requested: configuration_index,
            });
        }
        self.state = VideoSetupState::Ready;
        self.session_id = Some(session_id);
        Ok(vec![VideoSetupAction::Ready {
            session_id,
            configuration_index,
        }])
    }

    /// Handles traffic once `Ready` — the phone (as `MediaSinkService`
    /// source) streaming actual encoded video to the head unit. Not a
    /// state transition: stays `Ready` either way. Every `Data`/
    /// `CodecConfig` gets an unconditional `Ack` right back (flow control —
    /// see `encode_media_ack`'s doc comment for why). Fails closed on
    /// anything else, matching every other decoder in this crate.
    fn handle_media(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<VideoSetupAction>, VideoSetupError> {
        let session_id = self
            .session_id
            .expect("session_id is set once Ready, the only state handle_media runs in");
        match message.id {
            MediaMessageId::Data => {
                let Some((timestamp, payload)) = decode_media_data(&message.body) else {
                    return Err(VideoSetupError::TruncatedMediaData {
                        available: message.body.len(),
                    });
                };
                Ok(vec![
                    VideoSetupAction::MediaDataReceived {
                        timestamp,
                        payload: payload.to_vec(),
                    },
                    VideoSetupAction::SendMedia(encode_media_ack(session_id)),
                ])
            }
            MediaMessageId::CodecConfig => Ok(vec![
                VideoSetupAction::CodecConfigReceived {
                    payload: message.body.clone(),
                },
                VideoSetupAction::SendMedia(encode_media_ack(session_id)),
            ]),
            other => Err(VideoSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: other,
            }),
        }
    }
}

/// Encodes `VideoFocusNotification`, proactively granting the phone video
/// focus. `focus` (field 1, optional enum) is the only field ever written —
/// `unsolicited` (field 2, optional bool) is left unset, matching `f-io/LIVI`'s
/// own exact wire bytes for this send (`[0x08, 0x01]` for `Projected`), not
/// a reply to any `VideoFocusRequest` from the phone (this project has
/// never received one — see this crate's module boundary notes).
#[must_use]
pub fn encode_video_focus_notification(focus: VideoFocusMode) -> MediaMessage {
    let mut body = Vec::new();
    protobuf::write_int32_field(&mut body, 1, focus.wire_value());
    MediaMessage {
        id: MediaMessageId::VideoFocusNotification,
        body,
    }
}

/// Decodes `Setup` and returns its `type` (a `MediaCodecType` wire value).
fn decode_setup(body: &[u8]) -> Result<i32, VideoSetupError> {
    let mut cursor = 0;
    let mut codec_type = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<VideoSetupError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(VideoSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<VideoSetupError>(body, &mut cursor)?;
                // Canonical proto2 enum decode: sign-extend through i64,
                // then truncate to i32 (mirrors ChannelOpenRequest.service_id).
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                codec_type = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<VideoSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    codec_type.ok_or(VideoSetupError::MissingCodecType)
}

/// Decodes `Start` and returns `(session_id, configuration_index)`.
fn decode_start(body: &[u8]) -> Result<(i32, u32), VideoSetupError> {
    let mut cursor = 0;
    let mut session_id = None;
    let mut configuration_index = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<VideoSetupError>(body, &mut cursor)?;
        match field {
            1 | 2 if wire_type != 0 => {
                return Err(VideoSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<VideoSetupError>(body, &mut cursor)?;
                // session_id is a plain (signed) int32 — same decode shape
                // as ChannelOpenRequest.service_id.
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                session_id = Some(value);
            }
            2 => {
                let raw = protobuf::read_varint::<VideoSetupError>(body, &mut cursor)?;
                // configuration_index is genuinely unsigned (uint32): take
                // the low 32 bits, no sign-extension semantics apply.
                #[allow(clippy::cast_possible_truncation)]
                let value = raw as u32;
                configuration_index = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<VideoSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    let session_id = session_id.ok_or(VideoSetupError::MissingSessionId)?;
    let configuration_index =
        configuration_index.ok_or(VideoSetupError::MissingConfigurationIndex)?;
    Ok((session_id, configuration_index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media_message(id: MediaMessageId, body: Vec<u8>) -> Vec<u8> {
        MediaMessage { id, body }
            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
            .expect("encode envelope")
    }

    fn setup_payload(codec_type: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, codec_type);
        media_message(MediaMessageId::Setup, body)
    }

    fn start_payload(session_id: i32, configuration_index: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, session_id);
        protobuf::write_int32_field(&mut body, 2, configuration_index);
        media_message(MediaMessageId::Start, body)
    }

    #[test]
    fn full_handshake_reaches_ready_with_exact_config_bytes() {
        let mut machine = VideoSetupStateMachine::new();
        let actions = machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP,
            )))
            .expect("setup");
        assert_eq!(machine.state(), VideoSetupState::AwaitingStart);
        assert_eq!(actions.len(), 2);
        let VideoSetupAction::SendMedia(config) = &actions[0] else {
            panic!("expected SendMedia");
        };
        assert_eq!(config.id, MediaMessageId::Config);
        assert_eq!(config.body, vec![0x08, 0x02, 0x10, 0x01, 0x18, 0x00]);
        let VideoSetupAction::SendMedia(video_focus) = &actions[1] else {
            panic!("expected SendMedia");
        };
        assert_eq!(video_focus.id, MediaMessageId::VideoFocusNotification);
        assert_eq!(video_focus.body, vec![0x08, 0x01]);

        let actions = machine
            .advance(VideoSetupEvent::InboundMedia(&start_payload(7, 0)))
            .expect("start");
        assert_eq!(machine.state(), VideoSetupState::Ready);
        assert_eq!(
            actions,
            vec![VideoSetupAction::Ready {
                session_id: 7,
                configuration_index: 0,
            }]
        );

        // --- Ready stays Ready and keeps decoding media traffic ---
        let mut data_body = 42_u64.to_be_bytes().to_vec();
        data_body.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let actions = machine
            .advance(VideoSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Data,
                data_body,
            )))
            .expect("data");
        assert_eq!(machine.state(), VideoSetupState::Ready);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            VideoSetupAction::MediaDataReceived {
                timestamp: 42,
                payload: vec![0xaa, 0xbb, 0xcc],
            }
        );
        let VideoSetupAction::SendMedia(ack) = &actions[1] else {
            panic!("expected SendMedia");
        };
        assert_eq!(ack.id, MediaMessageId::Ack);
        assert_eq!(ack.body, vec![0x08, 0x07, 0x10, 0x01]);

        let actions = machine
            .advance(VideoSetupEvent::InboundMedia(&media_message(
                MediaMessageId::CodecConfig,
                vec![0x01, 0x02, 0x03, 0x04],
            )))
            .expect("codec config");
        assert_eq!(machine.state(), VideoSetupState::Ready);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            VideoSetupAction::CodecConfigReceived {
                payload: vec![0x01, 0x02, 0x03, 0x04],
            }
        );
        let VideoSetupAction::SendMedia(ack) = &actions[1] else {
            panic!("expected SendMedia");
        };
        assert_eq!(ack.id, MediaMessageId::Ack);
        assert_eq!(ack.body, vec![0x08, 0x07, 0x10, 0x01]);
    }

    #[test]
    fn accepts_h265_setup_and_offers_the_h265_configuration_index() {
        let mut machine = VideoSetupStateMachine::new();
        let actions = machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H265,
            )))
            .expect("setup");
        let VideoSetupAction::SendMedia(config) = &actions[0] else {
            panic!("expected SendMedia");
        };
        assert_eq!(config.id, MediaMessageId::Config);
        // status=READY(2), max_unacked=1, configuration_indices=[1] (H.265's
        // position, not H.264's) — same shape as the H.264 case except the
        // last byte.
        assert_eq!(config.body, vec![0x08, 0x02, 0x10, 0x01, 0x18, 0x01]);

        let actions = machine
            .advance(VideoSetupEvent::InboundMedia(&start_payload(7, 1)))
            .expect("start");
        assert_eq!(machine.state(), VideoSetupState::Ready);
        assert_eq!(
            actions,
            vec![VideoSetupAction::Ready {
                session_id: 7,
                configuration_index: 1,
            }]
        );
    }

    #[test]
    fn rejects_start_at_the_other_codecs_configuration_index() {
        // A phone that requested H.264 (offered index 0) must not be able
        // to Start against index 1 (H.265's position) instead — only the
        // exact index this session was actually offered is valid.
        let mut machine = VideoSetupStateMachine::new();
        machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP,
            )))
            .expect("setup");
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&start_payload(7, 1))),
            Err(VideoSetupError::UnknownConfigurationIndex { requested: 1 })
        );
    }

    #[test]
    fn rejects_truncated_media_data() {
        let mut machine = VideoSetupStateMachine::new();
        machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP,
            )))
            .expect("setup");
        machine
            .advance(VideoSetupEvent::InboundMedia(&start_payload(7, 0)))
            .expect("start");
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Data,
                vec![0x00, 0x00, 0x00],
            ))),
            Err(VideoSetupError::TruncatedMediaData { available: 3 })
        );
        assert_eq!(machine.state(), VideoSetupState::Failed);
    }

    #[test]
    fn rejects_unexpected_message_while_ready() {
        let mut machine = VideoSetupStateMachine::new();
        machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP,
            )))
            .expect("setup");
        machine
            .advance(VideoSetupEvent::InboundMedia(&start_payload(7, 0)))
            .expect("start");
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&start_payload(7, 0))),
            Err(VideoSetupError::UnexpectedMediaMessage {
                state: VideoSetupState::Ready,
                id: MediaMessageId::Start,
            })
        );
        assert_eq!(machine.state(), VideoSetupState::Failed);
    }

    #[test]
    fn encodes_video_focus_notification_with_exact_bytes() {
        let message = encode_video_focus_notification(VideoFocusMode::Projected);
        assert_eq!(message.id, MediaMessageId::VideoFocusNotification);
        assert_eq!(message.body, vec![0x08, 0x01]);
    }

    #[test]
    fn rejects_unsupported_codec_and_fails_closed() {
        let mut machine = VideoSetupStateMachine::new();
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&setup_payload(99))),
            Err(VideoSetupError::UnsupportedCodec { requested: 99 })
        );
        assert_eq!(machine.state(), VideoSetupState::Failed);
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP
            ))),
            Err(VideoSetupError::UnexpectedEvent {
                state: VideoSetupState::Failed
            })
        );
    }

    #[test]
    fn rejects_unknown_configuration_index() {
        let mut machine = VideoSetupStateMachine::new();
        machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP,
            )))
            .expect("setup");
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&start_payload(1, 5))),
            Err(VideoSetupError::UnknownConfigurationIndex { requested: 5 })
        );
    }

    #[test]
    fn rejects_start_before_setup() {
        let mut machine = VideoSetupStateMachine::new();
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&start_payload(1, 0))),
            Err(VideoSetupError::UnexpectedMediaMessage {
                state: VideoSetupState::AwaitingSetup,
                id: MediaMessageId::Start,
            })
        );
    }

    #[test]
    fn rejects_missing_required_fields() {
        let mut machine = VideoSetupStateMachine::new();
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Setup,
                Vec::new(),
            ))),
            Err(VideoSetupError::MissingCodecType)
        );

        let mut machine = VideoSetupStateMachine::new();
        machine
            .advance(VideoSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_VIDEO_H264_BP,
            )))
            .expect("setup");
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 2, 0);
        assert_eq!(
            machine.advance(VideoSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Start,
                body,
            ))),
            Err(VideoSetupError::MissingSessionId)
        );
    }
}
