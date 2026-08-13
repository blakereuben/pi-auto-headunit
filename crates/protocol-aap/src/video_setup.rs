use std::fmt;

use crate::media_message::{
    DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, MediaMessage, MediaMessageError, MediaMessageId,
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
// not derived from AASDK/OpenAuto (neither sends it) — it's motivated by
// independently observing that behaviour in a separate, independently
// implemented, GPL-3.0-or-later Android Auto client (`f-io/LIVI`,
// `src/main/services/projection/driver/aa/stack/session/Session.ts`, not
// AASDK-derived). No LIVI code is reproduced here; only the wire
// message/field shape, itself confirmed byte-for-byte against this
// project's own pinned AASDK schema above, and the idea of sending it
// unconditionally after Config.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.media.shared.message.MediaCodecType.MEDIA_CODEC_VIDEO_H264_BP`
/// — the only codec `ServiceDiscoveryResponse` ever advertised for this
/// increment, so it's the only one `Setup` may legitimately request.
const MEDIA_CODEC_VIDEO_H264_BP: i32 = 3;

/// `Config`'s nested `Status` enum, `STATUS_READY = 2`. `STATUS_WAIT` (1)
/// is never sent this increment — there is no reason yet to ask the phone
/// to wait.
const CONFIG_STATUS_READY: i32 = 2;

/// A deliberately conservative placeholder for `Config.max_unacked`. The
/// real ack/backpressure chain (`MEDIA_MESSAGE_ACK`) is out of scope this
/// increment; this only shapes what capacity is advertised.
const DEFAULT_VIDEO_MAX_UNACKED: i32 = 1;

/// The only `VideoConfiguration` entry `ServiceDiscoveryResponse` ever
/// advertised, so it's the only configuration index `Start` may reference.
const ADVERTISED_CONFIGURATION_INDEX: u32 = 0;

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
    /// Terminal: `Start` was received and accepted. This is where this
    /// increment stops — no `MEDIA_MESSAGE_DATA` byte is ever parsed.
    Ready {
        session_id: i32,
        configuration_index: u32,
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
            VideoSetupState::Ready | VideoSetupState::Failed => {
                Err(VideoSetupError::UnexpectedEvent { state: self.state })
            }
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
        if codec_type != MEDIA_CODEC_VIDEO_H264_BP {
            return Err(VideoSetupError::UnsupportedCodec {
                requested: codec_type,
            });
        }
        self.state = VideoSetupState::AwaitingStart;
        let mut body = Vec::new();
        // Config.status (field 1, required nested Status enum).
        protobuf::write_int32_field(&mut body, 1, CONFIG_STATUS_READY);
        // Config.max_unacked (field 2, optional uint32).
        protobuf::write_int32_field(&mut body, 2, DEFAULT_VIDEO_MAX_UNACKED);
        // Config.configuration_indices (field 3, repeated uint32, non-packed).
        #[allow(clippy::cast_possible_wrap)]
        let advertised_index = ADVERTISED_CONFIGURATION_INDEX as i32;
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
        if configuration_index != ADVERTISED_CONFIGURATION_INDEX {
            return Err(VideoSetupError::UnknownConfigurationIndex {
                requested: configuration_index,
            });
        }
        self.state = VideoSetupState::Ready;
        Ok(vec![VideoSetupAction::Ready {
            session_id,
            configuration_index,
        }])
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
