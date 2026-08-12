use std::fmt;

use crate::control::{ControlMessage, ControlMessageId};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's AudioFocusRequest/AudioFocusNotification
// protobuf schema (protobuf/aap_protobuf/service/control/message/) and
// ControlServiceChannel's sendAudioFocusResponse/messageHandler dispatch,
// at the pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.control.message.AudioFocusRequestType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioFocusRequestType {
    Gain,
    GainTransient,
    GainTransientMayDuck,
    Release,
}

impl AudioFocusRequestType {
    const fn from_wire(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::Gain),
            2 => Some(Self::GainTransient),
            3 => Some(Self::GainTransientMayDuck),
            4 => Some(Self::Release),
            _ => None,
        }
    }
}

/// `aap_protobuf.service.control.message.AudioFocusStateType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioFocusStateType {
    Invalid,
    Gain,
    GainTransient,
    Loss,
    LossTransientCanDuck,
    LossTransient,
    GainMediaOnly,
    GainTransientGuidanceOnly,
}

impl AudioFocusStateType {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Invalid => 0,
            Self::Gain => 1,
            Self::GainTransient => 2,
            Self::Loss => 3,
            Self::LossTransientCanDuck => 4,
            Self::LossTransient => 5,
            Self::GainMediaOnly => 6,
            Self::GainTransientGuidanceOnly => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioFocusError {
    MissingAudioFocusType,
    UnknownAudioFocusType(i32),
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for AudioFocusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAudioFocusType => formatter
                .write_str("AudioFocusRequest is missing its required audio_focus_type field"),
            Self::UnknownAudioFocusType(value) => {
                write!(formatter, "unknown AudioFocusRequestType value {value}")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "audio-focus field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated audio-focus protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid audio-focus protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("audio-focus protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("audio-focus field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for AudioFocusError {}

impl ProtobufDecodeError for AudioFocusError {
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

/// Decodes `AudioFocusRequest` and returns its `audio_focus_type`. Fails
/// closed on any value outside the four known ones, matching every other
/// decoder in this crate — a genuinely new value from a real phone is
/// useful information to surface via a clean error, not something to
/// silently paper over.
pub fn decode_audio_focus_request(body: &[u8]) -> Result<AudioFocusRequestType, AudioFocusError> {
    let mut cursor = 0;
    let mut audio_focus_type = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<AudioFocusError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(AudioFocusError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<AudioFocusError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                audio_focus_type = Some(
                    AudioFocusRequestType::from_wire(value)
                        .ok_or(AudioFocusError::UnknownAudioFocusType(value))?,
                );
            }
            _ => {
                protobuf::skip_unknown_field::<AudioFocusError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    audio_focus_type.ok_or(AudioFocusError::MissingAudioFocusType)
}

/// Encodes `AudioFocusNotification` as a control message. `unsolicited`
/// (field 2, optional bool) is deliberately never written: this is always
/// a solicited reply to a request, and proto2's default (`false`) for an
/// omitted optional field is already correct.
#[must_use]
pub fn encode_audio_focus_notification(focus_state: AudioFocusStateType) -> ControlMessage {
    let mut body = Vec::new();
    // AudioFocusNotification.focus_state (field 1, required enum).
    protobuf::write_int32_field(&mut body, 1, focus_state.wire_value());
    ControlMessage {
        id: ControlMessageId::AudioFocusNotification,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_body(audio_focus_type: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, audio_focus_type);
        body
    }

    #[test]
    fn decodes_every_known_request_type() {
        assert_eq!(
            decode_audio_focus_request(&request_body(1)),
            Ok(AudioFocusRequestType::Gain)
        );
        assert_eq!(
            decode_audio_focus_request(&request_body(2)),
            Ok(AudioFocusRequestType::GainTransient)
        );
        assert_eq!(
            decode_audio_focus_request(&request_body(3)),
            Ok(AudioFocusRequestType::GainTransientMayDuck)
        );
        assert_eq!(
            decode_audio_focus_request(&request_body(4)),
            Ok(AudioFocusRequestType::Release)
        );
    }

    #[test]
    fn rejects_unknown_request_type() {
        assert_eq!(
            decode_audio_focus_request(&request_body(99)),
            Err(AudioFocusError::UnknownAudioFocusType(99))
        );
    }

    #[test]
    fn rejects_missing_audio_focus_type() {
        assert_eq!(
            decode_audio_focus_request(&[]),
            Err(AudioFocusError::MissingAudioFocusType)
        );
    }

    #[test]
    fn encodes_notification_with_exact_bytes() {
        let message = encode_audio_focus_notification(AudioFocusStateType::Gain);
        assert_eq!(message.id, ControlMessageId::AudioFocusNotification);
        assert_eq!(message.body, vec![0x08, 0x01]);
    }

    #[test]
    fn encodes_loss_state_with_exact_bytes() {
        let message = encode_audio_focus_notification(AudioFocusStateType::Loss);
        assert_eq!(message.body, vec![0x08, 0x03]);
    }
}
