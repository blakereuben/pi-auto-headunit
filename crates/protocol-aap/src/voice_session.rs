use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

// Field mapping (VoiceSessionNotification.status, field 1; VoiceSessionStatus
// enum: START=1, END=2) is confirmed directly against the pinned primary
// AASDK source (`protobuf/aap_protobuf/service/control/message/
// {VoiceSessionNotification,VoiceSessionStatus}.proto`, revision
// `9bf6adf933665dee26532201719fac14a047ccf1`,
// `docs/protocol/aasdk-adoption.md`), the same source `nav_focus.rs`/
// `audio_focus.rs` use for their own wire schemas. Wire id 17 discovered on
// a real phone, unprompted, when a WhatsApp message notification arrived
// during a `SystemAudio`/`SpeechAudio` verification trial — not something
// reproducible from fixtures alone. `ControlServiceChannel.hpp` shows AASDK
// models this
// bidirectionally (a `sendVoiceSessionFocusResponse` HU-to-phone path
// exists, presumably for a head-unit-initiated push-to-talk session this
// project doesn't have working microphone hardware to exercise); for the
// ordinary phone-initiated case this project actually observed, `f-io/LIVI`
// revision `9000f308eec423c5c56ac0a14491a7c95ce5762d`
// (`docs/protocol/livi-adoption.md`) — a real, working implementation — is
// the source for the *behavioural* fact that no reply is sent:
// `ControlChannel.ts`'s `_onVoiceSessionNotification` documents this as
// "Pure status info, no response expected (matches aasdk + openauto
// behaviour)".
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.control.message.VoiceSessionStatus`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceSessionStatus {
    Start,
    End,
    Unknown(i32),
}

impl VoiceSessionStatus {
    const fn from_wire(value: i32) -> Self {
        match value {
            1 => Self::Start,
            2 => Self::End,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceSessionError {
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for VoiceSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "voice-session field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated voice-session protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid voice-session protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("voice-session protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("voice-session field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for VoiceSessionError {}

impl ProtobufDecodeError for VoiceSessionError {
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

#[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
fn varint_to_i32(raw: u64) -> i32 {
    (raw as i64) as i32
}

/// Decodes `VoiceSessionNotification.status` (field 1, **optional** enum —
/// an absent field is a valid, if uninformative, notification).
pub fn decode_voice_session_notification(
    body: &[u8],
) -> Result<Option<VoiceSessionStatus>, VoiceSessionError> {
    let mut cursor = 0;
    let mut status = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<VoiceSessionError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(VoiceSessionError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<VoiceSessionError>(body, &mut cursor)?;
                status = Some(VoiceSessionStatus::from_wire(varint_to_i32(raw)));
            }
            _ => {
                protobuf::skip_unknown_field::<VoiceSessionError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification_body(status: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, status);
        body
    }

    #[test]
    fn decodes_known_statuses() {
        assert_eq!(
            decode_voice_session_notification(&notification_body(1)),
            Ok(Some(VoiceSessionStatus::Start))
        );
        assert_eq!(
            decode_voice_session_notification(&notification_body(2)),
            Ok(Some(VoiceSessionStatus::End))
        );
    }

    #[test]
    fn unknown_status_survives_round_trip() {
        assert_eq!(
            decode_voice_session_notification(&notification_body(99)),
            Ok(Some(VoiceSessionStatus::Unknown(99)))
        );
    }

    #[test]
    fn absent_status_is_a_valid_notification() {
        assert_eq!(decode_voice_session_notification(&[]), Ok(None));
    }

    #[test]
    fn skips_unknown_fields() {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 7, 42);
        protobuf::write_int32_field(&mut body, 1, 1);
        assert_eq!(
            decode_voice_session_notification(&body),
            Ok(Some(VoiceSessionStatus::Start))
        );
    }
}
