use std::fmt;

// Portions derived from AASDK's MediaMessageId protobuf schema
// (protobuf/aap_protobuf/service/media/sink/MediaMessageId.proto) and
// message-id wire framing (src/Messenger/MessageId.cpp) at the pinned
// project revision (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE: usize = 1024 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

/// `aap_protobuf.service.media.sink.MediaMessageId`. Only the three values
/// this increment's video-channel setup handshake needs are named; the
/// rest (`MEDIA_MESSAGE_DATA`, `_STOP`, `_ACK`, video focus, microphone,
/// UI config, audio underflow) are out of scope until something decodes or
/// sends them, matching `Unknown` surviving round-trip the same way
/// `ControlMessageId::Unknown` already does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaMessageId {
    Setup,
    Start,
    Config,
    Unknown(u16),
}

impl MediaMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::Setup => 32768,
            Self::Start => 32769,
            Self::Config => 32771,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            32768 => Self::Setup,
            32769 => Self::Start,
            32771 => Self::Config,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaMessageError {
    InvalidLimit,
    TruncatedMessageId { available: usize },
    BodyTooLarge { size: usize, maximum: usize },
}

impl fmt::Display for MediaMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("media message limits must be non-zero"),
            Self::TruncatedMessageId { available } => write!(
                formatter,
                "media message id requires 2 bytes, {available} available"
            ),
            Self::BodyTooLarge { size, maximum } => {
                write!(
                    formatter,
                    "media message body {size} exceeds limit {maximum}"
                )
            }
        }
    }
}

impl std::error::Error for MediaMessageError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaMessage {
    pub id: MediaMessageId,
    pub body: Vec<u8>,
}

impl MediaMessage {
    pub fn decode(payload: &[u8], maximum_body_size: usize) -> Result<Self, MediaMessageError> {
        if maximum_body_size == 0 {
            return Err(MediaMessageError::InvalidLimit);
        }
        if payload.len() < MESSAGE_ID_SIZE {
            return Err(MediaMessageError::TruncatedMessageId {
                available: payload.len(),
            });
        }
        let body_size = payload.len() - MESSAGE_ID_SIZE;
        if body_size > maximum_body_size {
            return Err(MediaMessageError::BodyTooLarge {
                size: body_size,
                maximum: maximum_body_size,
            });
        }
        Ok(Self {
            id: MediaMessageId::from_wire(u16::from_be_bytes([payload[0], payload[1]])),
            body: payload[MESSAGE_ID_SIZE..].to_vec(),
        })
    }

    pub fn encode(&self, maximum_body_size: usize) -> Result<Vec<u8>, MediaMessageError> {
        if maximum_body_size == 0 {
            return Err(MediaMessageError::InvalidLimit);
        }
        if self.body.len() > maximum_body_size {
            return Err(MediaMessageError::BodyTooLarge {
                size: self.body.len(),
                maximum: maximum_body_size,
            });
        }
        let mut payload = Vec::with_capacity(MESSAGE_ID_SIZE + self.body.len());
        payload.extend_from_slice(&self.id.wire_value().to_be_bytes());
        payload.extend_from_slice(&self.body);
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_message_ids_are_big_endian_and_unknown_values_survive() {
        let decoded = MediaMessage::decode(&[0x12, 0x34, 0xaa], 4).expect("decode");
        assert_eq!(decoded.id, MediaMessageId::Unknown(0x1234));
        assert_eq!(decoded.body, [0xaa]);
        assert_eq!(decoded.encode(4).expect("encode"), [0x12, 0x34, 0xaa]);
    }

    #[test]
    fn known_ids_round_trip_through_their_wire_values() {
        for id in [
            MediaMessageId::Setup,
            MediaMessageId::Start,
            MediaMessageId::Config,
        ] {
            let message = MediaMessage {
                id,
                body: Vec::new(),
            };
            let payload = message
                .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                .expect("encode");
            let decoded = MediaMessage::decode(&payload, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                .expect("decode");
            assert_eq!(decoded.id, id);
        }
    }

    #[test]
    fn rejects_truncated_message_id() {
        assert_eq!(
            MediaMessage::decode(&[0x80], DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE),
            Err(MediaMessageError::TruncatedMessageId { available: 1 })
        );
    }

    #[test]
    fn rejects_oversized_body_and_invalid_limit() {
        let message = MediaMessage {
            id: MediaMessageId::Setup,
            body: vec![0; 3],
        };
        assert_eq!(
            message.encode(2),
            Err(MediaMessageError::BodyTooLarge {
                size: 3,
                maximum: 2
            })
        );
        assert_eq!(message.encode(0), Err(MediaMessageError::InvalidLimit));
        assert_eq!(
            MediaMessage::decode(&[0x80, 0x00], 0),
            Err(MediaMessageError::InvalidLimit)
        );
    }
}
