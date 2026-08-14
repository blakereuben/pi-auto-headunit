use std::fmt;

use crate::protobuf;

// Portions derived from AASDK's MediaMessageId protobuf schema
// (protobuf/aap_protobuf/service/media/sink/MediaMessageId.proto),
// message-id wire framing (src/Messenger/MessageId.cpp), and the
// media.source.message.Ack protobuf schema
// (protobuf/aap_protobuf/service/media/source/message/Ack.proto), at the
// pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1).
// `encode_media_ack`'s send-unconditionally-after-every-frame policy is
// derived from a separate, independently implemented, GPL-3.0-or-later
// Android Auto client (`f-io/LIVI` revision
// 9000f308eec423c5c56ac0a14491a7c95ce5762d,
// `src/main/services/projection/driver/aa/stack/channels/{Video,Audio}Channel.ts`,
// not AASDK-derived), formally adopted per `docs/protocol/livi-adoption.md`
// ("Adopted scope" item 2). No LIVI code is reproduced, only the wire
// shape, itself confirmed against this project's own pinned AASDK schema
// above, and the ack-every-frame behavioural rule.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// Copyright (C) 2024-2026 Open Android Auto contributors (LIVI)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE: usize = 1024 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

/// `aap_protobuf.service.media.sink.MediaMessageId`. `Setup`/`Start`/`Config`
/// are the video-channel setup handshake; `VideoFocusRequest`/
/// `VideoFocusNotification` are the head-unit-initiated video-focus grant
/// (`protocol_aap::video_setup`); `Data`/`CodecConfig` are the phone-to-
/// head-unit media stream itself (also `protocol_aap::video_setup`/
/// `protocol_aap::audio_setup` — this project is a `MediaSinkService`, so
/// the phone sends these, not us); `Ack` is the head-unit's flow-control
/// reply to each (`encode_media_ack`, below). The rest (`_STOP`,
/// microphone, UI config, audio underflow) are out of scope until
/// something decodes or sends them, matching `Unknown` surviving
/// round-trip the same way `ControlMessageId::Unknown` already does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaMessageId {
    Data,
    CodecConfig,
    Setup,
    Start,
    Config,
    Ack,
    VideoFocusRequest,
    VideoFocusNotification,
    Unknown(u16),
}

impl MediaMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::Data => 0,
            Self::CodecConfig => 1,
            Self::Setup => 32768,
            Self::Start => 32769,
            Self::Config => 32771,
            Self::Ack => 32772,
            Self::VideoFocusRequest => 32775,
            Self::VideoFocusNotification => 32776,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            0 => Self::Data,
            1 => Self::CodecConfig,
            32768 => Self::Setup,
            32769 => Self::Start,
            32771 => Self::Config,
            32772 => Self::Ack,
            32775 => Self::VideoFocusRequest,
            32776 => Self::VideoFocusNotification,
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

/// Decodes `Data`'s framing — shared by video and every audio sink
/// channel, since the wire shape is identical regardless of media type: an
/// 8-byte big-endian timestamp prefix followed by the raw encoded payload.
/// Returns `None` if the body is shorter than the prefix; callers map that
/// to their own channel-specific `TruncatedMediaData` error variant.
pub(crate) fn decode_media_data(body: &[u8]) -> Option<(u64, usize)> {
    const TIMESTAMP_SIZE: usize = 8;
    if body.len() < TIMESTAMP_SIZE {
        return None;
    }
    let timestamp_bytes: [u8; TIMESTAMP_SIZE] = body[..TIMESTAMP_SIZE]
        .try_into()
        .expect("length checked above");
    Some((
        u64::from_be_bytes(timestamp_bytes),
        body.len() - TIMESTAMP_SIZE,
    ))
}

/// Encodes `Ack` (`aap_protobuf.service.media.source.message.Ack`) — sent
/// unconditionally after every `Data`/`CodecConfig` received, on every AV
/// sink channel (video and all three audio channels). `session_id` (field
/// 1, required) is the value echoed from that channel's `Start`; `ack`
/// (field 2, optional) is always the literal value `1`;
/// `receive_timestamp_ns` (field 3, repeated) is never populated — matches
/// `f-io/LIVI`'s own `_sendAck()` exactly (see this module's header
/// comment for provenance).
#[must_use]
pub fn encode_media_ack(session_id: i32) -> MediaMessage {
    let mut body = Vec::new();
    protobuf::write_int32_field(&mut body, 1, session_id);
    protobuf::write_uint32_field(&mut body, 2, 1);
    MediaMessage {
        id: MediaMessageId::Ack,
        body,
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
    fn decodes_media_data_timestamp_and_length() {
        let mut body = 42_u64.to_be_bytes().to_vec();
        body.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        assert_eq!(decode_media_data(&body), Some((42, 3)));
    }

    #[test]
    fn rejects_truncated_media_data() {
        assert_eq!(decode_media_data(&[0x00, 0x00, 0x00]), None);
    }

    #[test]
    fn encodes_media_ack_with_exact_bytes() {
        let message = encode_media_ack(7);
        assert_eq!(message.id, MediaMessageId::Ack);
        assert_eq!(message.body, vec![0x08, 0x07, 0x10, 0x01]);
    }

    #[test]
    fn known_ids_round_trip_through_their_wire_values() {
        for id in [
            MediaMessageId::Data,
            MediaMessageId::CodecConfig,
            MediaMessageId::Setup,
            MediaMessageId::Start,
            MediaMessageId::Config,
            MediaMessageId::Ack,
            MediaMessageId::VideoFocusRequest,
            MediaMessageId::VideoFocusNotification,
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
