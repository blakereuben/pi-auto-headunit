use std::fmt;

use crate::media_message::{
    DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, MediaMessage, MediaMessageError, MediaMessageId,
};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's Setup/Config/Start protobuf schema
// (protobuf/aap_protobuf/service/media/shared/message/) and the MediaAudio
// channel's setup dispatch behaviour in AudioMediaSinkService.cpp, at the
// pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1): the
// phone sends Setup, the head unit replies Config, the phone sends Start —
// confirmed against that source, not any public specification (none is
// known to exist for this wire protocol). This is the exact same Setup/
// Config/Start message shape already used by the video channel
// (video_setup.rs) — only the accepted codec type differs. See the
// channel-setup design record for the full provenance trail.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.media.shared.message.MediaCodecType.MEDIA_CODEC_AUDIO_PCM`
/// — the only codec `ServiceDiscoveryResponse` ever advertised for the
/// `MediaAudio` service (a single, uncompressed `AudioConfiguration`), so
/// it's the only one `Setup` may legitimately request.
const MEDIA_CODEC_AUDIO_PCM: i32 = 1;

/// `Config`'s nested `Status` enum, `STATUS_READY = 2`. `STATUS_WAIT` (1)
/// is never sent this increment — there is no reason yet to ask the phone
/// to wait.
const CONFIG_STATUS_READY: i32 = 2;

/// A deliberately conservative placeholder for `Config.max_unacked`. The
/// real ack/backpressure chain (`MEDIA_MESSAGE_ACK`) is out of scope this
/// increment; this only shapes what capacity is advertised.
const DEFAULT_AUDIO_MAX_UNACKED: i32 = 1;

/// The only `AudioConfiguration` entry `ServiceDiscoveryResponse` ever
/// advertised for `MediaAudio`, so it's the only configuration index
/// `Start` may reference.
const ADVERTISED_CONFIGURATION_INDEX: u32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioSetupState {
    AwaitingSetup,
    AwaitingStart,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioSetupEvent<'a> {
    /// Payload of a `MessageType::Specific`-flagged frame on the
    /// `MediaAudio` channel.
    InboundMedia(&'a [u8]),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioSetupAction {
    SendMedia(MediaMessage),
    /// Terminal: `Start` was received and accepted. This is where this
    /// increment stops — no `MEDIA_MESSAGE_DATA` byte is ever parsed.
    Ready {
        session_id: i32,
        configuration_index: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioSetupError {
    UnexpectedEvent {
        state: AudioSetupState,
    },
    UnexpectedMediaMessage {
        state: AudioSetupState,
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

impl fmt::Display for AudioSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEvent { state } => {
                write!(formatter, "unexpected audio-setup event in state {state:?}")
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
                "audio-setup field {field} has unexpected wire type {wire_type}"
            ),
            Self::Envelope(error) => write!(formatter, "{error}"),
            Self::Truncated => formatter.write_str("truncated audio-setup protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid audio-setup protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("audio-setup protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("audio-setup field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for AudioSetupError {}

impl ProtobufDecodeError for AudioSetupError {
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

/// The `MediaAudio` channel's `Setup`→`Config`→`Start` handshake, driven
/// only after that channel's [`crate::ChannelOpenStateMachine`] has reached
/// `Open`. Same message shape as [`crate::VideoSetupStateMachine`]; kept as
/// a separate type rather than a shared generic one so this never touches
/// the real-hardware-proven video code path.
#[derive(Debug)]
pub struct AudioSetupStateMachine {
    state: AudioSetupState,
}

impl Default for AudioSetupStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSetupStateMachine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: AudioSetupState::AwaitingSetup,
        }
    }

    #[must_use]
    pub const fn state(&self) -> AudioSetupState {
        self.state
    }

    pub fn advance(
        &mut self,
        event: AudioSetupEvent<'_>,
    ) -> Result<Vec<AudioSetupAction>, AudioSetupError> {
        let result = self.advance_inner(event);
        if result.is_err() {
            self.state = AudioSetupState::Failed;
        }
        result
    }

    fn advance_inner(
        &mut self,
        event: AudioSetupEvent<'_>,
    ) -> Result<Vec<AudioSetupAction>, AudioSetupError> {
        let AudioSetupEvent::InboundMedia(payload) = event;
        let message = MediaMessage::decode(payload, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
            .map_err(AudioSetupError::Envelope)?;
        match self.state {
            AudioSetupState::AwaitingSetup => self.handle_setup(&message),
            AudioSetupState::AwaitingStart => self.handle_start(&message),
            AudioSetupState::Ready | AudioSetupState::Failed => {
                Err(AudioSetupError::UnexpectedEvent { state: self.state })
            }
        }
    }

    fn handle_setup(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<AudioSetupAction>, AudioSetupError> {
        if message.id != MediaMessageId::Setup {
            return Err(AudioSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: message.id,
            });
        }
        let codec_type = decode_setup(&message.body)?;
        if codec_type != MEDIA_CODEC_AUDIO_PCM {
            return Err(AudioSetupError::UnsupportedCodec {
                requested: codec_type,
            });
        }
        self.state = AudioSetupState::AwaitingStart;
        let mut body = Vec::new();
        // Config.status (field 1, required nested Status enum).
        protobuf::write_int32_field(&mut body, 1, CONFIG_STATUS_READY);
        // Config.max_unacked (field 2, optional uint32).
        protobuf::write_int32_field(&mut body, 2, DEFAULT_AUDIO_MAX_UNACKED);
        // Config.configuration_indices (field 3, repeated uint32, non-packed).
        #[allow(clippy::cast_possible_wrap)]
        let advertised_index = ADVERTISED_CONFIGURATION_INDEX as i32;
        protobuf::write_int32_field(&mut body, 3, advertised_index);
        Ok(vec![AudioSetupAction::SendMedia(MediaMessage {
            id: MediaMessageId::Config,
            body,
        })])
    }

    fn handle_start(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<AudioSetupAction>, AudioSetupError> {
        if message.id != MediaMessageId::Start {
            return Err(AudioSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: message.id,
            });
        }
        let (session_id, configuration_index) = decode_start(&message.body)?;
        if configuration_index != ADVERTISED_CONFIGURATION_INDEX {
            return Err(AudioSetupError::UnknownConfigurationIndex {
                requested: configuration_index,
            });
        }
        self.state = AudioSetupState::Ready;
        Ok(vec![AudioSetupAction::Ready {
            session_id,
            configuration_index,
        }])
    }
}

/// Decodes `Setup` and returns its `type` (a `MediaCodecType` wire value).
fn decode_setup(body: &[u8]) -> Result<i32, AudioSetupError> {
    let mut cursor = 0;
    let mut codec_type = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<AudioSetupError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(AudioSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<AudioSetupError>(body, &mut cursor)?;
                // Canonical proto2 enum decode: sign-extend through i64,
                // then truncate to i32 (mirrors ChannelOpenRequest.service_id).
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                codec_type = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<AudioSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    codec_type.ok_or(AudioSetupError::MissingCodecType)
}

/// Decodes `Start` and returns `(session_id, configuration_index)`.
fn decode_start(body: &[u8]) -> Result<(i32, u32), AudioSetupError> {
    let mut cursor = 0;
    let mut session_id = None;
    let mut configuration_index = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<AudioSetupError>(body, &mut cursor)?;
        match field {
            1 | 2 if wire_type != 0 => {
                return Err(AudioSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<AudioSetupError>(body, &mut cursor)?;
                // session_id is a plain (signed) int32 — same decode shape
                // as ChannelOpenRequest.service_id.
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                session_id = Some(value);
            }
            2 => {
                let raw = protobuf::read_varint::<AudioSetupError>(body, &mut cursor)?;
                // configuration_index is genuinely unsigned (uint32): take
                // the low 32 bits, no sign-extension semantics apply.
                #[allow(clippy::cast_possible_truncation)]
                let value = raw as u32;
                configuration_index = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<AudioSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    let session_id = session_id.ok_or(AudioSetupError::MissingSessionId)?;
    let configuration_index =
        configuration_index.ok_or(AudioSetupError::MissingConfigurationIndex)?;
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
        let mut machine = AudioSetupStateMachine::new();
        let actions = machine
            .advance(AudioSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        assert_eq!(machine.state(), AudioSetupState::AwaitingStart);
        assert_eq!(actions.len(), 1);
        let AudioSetupAction::SendMedia(config) = &actions[0] else {
            panic!("expected SendMedia");
        };
        assert_eq!(config.id, MediaMessageId::Config);
        assert_eq!(config.body, vec![0x08, 0x02, 0x10, 0x01, 0x18, 0x00]);

        let actions = machine
            .advance(AudioSetupEvent::InboundMedia(&start_payload(7, 0)))
            .expect("start");
        assert_eq!(machine.state(), AudioSetupState::Ready);
        assert_eq!(
            actions,
            vec![AudioSetupAction::Ready {
                session_id: 7,
                configuration_index: 0,
            }]
        );
    }

    #[test]
    fn rejects_unsupported_codec_and_fails_closed() {
        let mut machine = AudioSetupStateMachine::new();
        assert_eq!(
            machine.advance(AudioSetupEvent::InboundMedia(&setup_payload(99))),
            Err(AudioSetupError::UnsupportedCodec { requested: 99 })
        );
        assert_eq!(machine.state(), AudioSetupState::Failed);
        assert_eq!(
            machine.advance(AudioSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM
            ))),
            Err(AudioSetupError::UnexpectedEvent {
                state: AudioSetupState::Failed
            })
        );
    }

    #[test]
    fn rejects_unknown_configuration_index() {
        let mut machine = AudioSetupStateMachine::new();
        machine
            .advance(AudioSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        assert_eq!(
            machine.advance(AudioSetupEvent::InboundMedia(&start_payload(1, 5))),
            Err(AudioSetupError::UnknownConfigurationIndex { requested: 5 })
        );
    }

    #[test]
    fn rejects_start_before_setup() {
        let mut machine = AudioSetupStateMachine::new();
        assert_eq!(
            machine.advance(AudioSetupEvent::InboundMedia(&start_payload(1, 0))),
            Err(AudioSetupError::UnexpectedMediaMessage {
                state: AudioSetupState::AwaitingSetup,
                id: MediaMessageId::Start,
            })
        );
    }

    #[test]
    fn rejects_missing_required_fields() {
        let mut machine = AudioSetupStateMachine::new();
        assert_eq!(
            machine.advance(AudioSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Setup,
                Vec::new(),
            ))),
            Err(AudioSetupError::MissingCodecType)
        );

        let mut machine = AudioSetupStateMachine::new();
        machine
            .advance(AudioSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 2, 0);
        assert_eq!(
            machine.advance(AudioSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Start,
                body,
            ))),
            Err(AudioSetupError::MissingSessionId)
        );
    }
}
