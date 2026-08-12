use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's InputMessageId protobuf schema
// (protobuf/aap_protobuf/service/inputsource/InputMessageId.proto),
// KeyBindingRequest/KeyBindingResponse protobuf schema
// (protobuf/aap_protobuf/service/media/sink/message/KeyBindingRequest.proto,
// KeyBindingResponse.proto), the shared MessageStatus enum
// (protobuf/aap_protobuf/shared/MessageStatus.proto), and
// InputSourceService's messageHandler/sendKeyBindingResponse dispatch
// (src/Channel/InputSource/InputSourceService.cpp), at the pinned project
// revision (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE: usize = 1024 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

/// `aap_protobuf.service.inputsource.InputMessageId`. Only the two values
/// this increment's key-binding exchange needs are named; the rest
/// (`INPUT_MESSAGE_INPUT_REPORT`, `_INPUT_FEEDBACK`) are out of scope until
/// something decodes or sends them, matching `MediaMessageId::Unknown`
/// surviving round-trip the same way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMessageId {
    KeyBindingRequest,
    KeyBindingResponse,
    Unknown(u16),
}

impl InputMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::KeyBindingRequest => 32770,
            Self::KeyBindingResponse => 32771,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            32770 => Self::KeyBindingRequest,
            32771 => Self::KeyBindingResponse,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMessageError {
    InvalidLimit,
    TruncatedMessageId { available: usize },
    BodyTooLarge { size: usize, maximum: usize },
}

impl fmt::Display for InputMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("input message limits must be non-zero"),
            Self::TruncatedMessageId { available } => write!(
                formatter,
                "input message id requires 2 bytes, {available} available"
            ),
            Self::BodyTooLarge { size, maximum } => {
                write!(
                    formatter,
                    "input message body {size} exceeds limit {maximum}"
                )
            }
        }
    }
}

impl std::error::Error for InputMessageError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputMessage {
    pub id: InputMessageId,
    pub body: Vec<u8>,
}

impl InputMessage {
    pub fn decode(payload: &[u8], maximum_body_size: usize) -> Result<Self, InputMessageError> {
        if maximum_body_size == 0 {
            return Err(InputMessageError::InvalidLimit);
        }
        if payload.len() < MESSAGE_ID_SIZE {
            return Err(InputMessageError::TruncatedMessageId {
                available: payload.len(),
            });
        }
        let body_size = payload.len() - MESSAGE_ID_SIZE;
        if body_size > maximum_body_size {
            return Err(InputMessageError::BodyTooLarge {
                size: body_size,
                maximum: maximum_body_size,
            });
        }
        Ok(Self {
            id: InputMessageId::from_wire(u16::from_be_bytes([payload[0], payload[1]])),
            body: payload[MESSAGE_ID_SIZE..].to_vec(),
        })
    }

    pub fn encode(&self, maximum_body_size: usize) -> Result<Vec<u8>, InputMessageError> {
        if maximum_body_size == 0 {
            return Err(InputMessageError::InvalidLimit);
        }
        if self.body.len() > maximum_body_size {
            return Err(InputMessageError::BodyTooLarge {
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

/// `aap_protobuf.shared.MessageStatus`, `KeyBindingResponse.status` values
/// only. Named local constants (matching `channel_open.rs`'s
/// `MESSAGE_STATUS_SUCCESS` precedent) rather than a full `MessageStatus`
/// model — nothing else in this crate needs the other ~28 values yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyBindingStatus {
    Success,
    KeycodeNotBound,
}

impl KeyBindingStatus {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::KeycodeNotBound => -18,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyBindingError {
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for KeyBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "key-binding field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated key-binding protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid key-binding protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("key-binding protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("key-binding field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for KeyBindingError {}

impl ProtobufDecodeError for KeyBindingError {
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

/// Decodes `KeyBindingRequest.keycodes` (field 1). Accepts both the proto's
/// declared packed encoding (wire type 2, a single length-delimited blob of
/// concatenated varints) and the unpacked form (wire type 0, one varint per
/// occurrence) — proto2 packed-repeated fields are a decoder-must-accept-
/// both wire-compatibility rule, not something a real phone's exact encoder
/// behavior can be assumed to always use.
pub fn decode_key_binding_request(body: &[u8]) -> Result<Vec<i32>, KeyBindingError> {
    let mut cursor = 0;
    let mut keycodes = Vec::new();
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<KeyBindingError>(body, &mut cursor)?;
        match (field, wire_type) {
            (1, 2) => {
                let packed = protobuf::read_length_delimited::<KeyBindingError>(body, &mut cursor)?;
                let mut packed_cursor = 0;
                while packed_cursor < packed.len() {
                    let raw = protobuf::read_varint::<KeyBindingError>(packed, &mut packed_cursor)?;
                    keycodes.push(varint_to_i32(raw));
                }
            }
            (1, 0) => {
                let raw = protobuf::read_varint::<KeyBindingError>(body, &mut cursor)?;
                keycodes.push(varint_to_i32(raw));
            }
            (1, wire_type) => {
                return Err(KeyBindingError::UnexpectedWireType { field, wire_type });
            }
            (_, wire_type) => {
                protobuf::skip_unknown_field::<KeyBindingError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    Ok(keycodes)
}

/// Encodes `KeyBindingResponse` as an input-channel message.
#[must_use]
pub fn encode_key_binding_response(status: KeyBindingStatus) -> InputMessage {
    let mut body = Vec::new();
    // KeyBindingResponse.status (field 1, required int32/MessageStatus enum).
    protobuf::write_int32_field(&mut body, 1, status.wire_value());
    InputMessage {
        id: InputMessageId::KeyBindingResponse,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_message_ids_are_big_endian_and_unknown_values_survive() {
        let decoded = InputMessage::decode(&[0x12, 0x34, 0xaa], 4).expect("decode");
        assert_eq!(decoded.id, InputMessageId::Unknown(0x1234));
        assert_eq!(decoded.body, [0xaa]);
        assert_eq!(decoded.encode(4).expect("encode"), [0x12, 0x34, 0xaa]);
    }

    #[test]
    fn known_ids_round_trip_through_their_wire_values() {
        for id in [
            InputMessageId::KeyBindingRequest,
            InputMessageId::KeyBindingResponse,
        ] {
            let message = InputMessage {
                id,
                body: Vec::new(),
            };
            let payload = message
                .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
                .expect("encode");
            let decoded = InputMessage::decode(&payload, DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
                .expect("decode");
            assert_eq!(decoded.id, id);
        }
    }

    #[test]
    fn rejects_truncated_message_id() {
        assert_eq!(
            InputMessage::decode(&[0x80], DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE),
            Err(InputMessageError::TruncatedMessageId { available: 1 })
        );
    }

    #[test]
    fn rejects_oversized_body_and_invalid_limit() {
        let message = InputMessage {
            id: InputMessageId::KeyBindingRequest,
            body: vec![0; 3],
        };
        assert_eq!(
            message.encode(2),
            Err(InputMessageError::BodyTooLarge {
                size: 3,
                maximum: 2
            })
        );
        assert_eq!(message.encode(0), Err(InputMessageError::InvalidLimit));
        assert_eq!(
            InputMessage::decode(&[0x80, 0x00], 0),
            Err(InputMessageError::InvalidLimit)
        );
    }

    fn packed_request_body(keycodes: &[i32]) -> Vec<u8> {
        let mut packed = Vec::new();
        for &keycode in keycodes {
            #[allow(clippy::cast_sign_loss)]
            let mut remaining = i64::from(keycode) as u64;
            loop {
                let byte = (remaining & 0x7f) as u8;
                remaining >>= 7;
                if remaining == 0 {
                    packed.push(byte);
                    break;
                }
                packed.push(byte | 0x80);
            }
        }
        let mut body = Vec::new();
        protobuf::write_length_delimited_field(&mut body, 1, &packed);
        body
    }

    #[test]
    fn decodes_a_packed_keycode_list() {
        assert_eq!(
            decode_key_binding_request(&packed_request_body(&[1, 2, 200])),
            Ok(vec![1, 2, 200])
        );
    }

    #[test]
    fn decodes_an_unpacked_keycode_list() {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, 4);
        protobuf::write_int32_field(&mut body, 1, 5);
        assert_eq!(decode_key_binding_request(&body), Ok(vec![4, 5]));
    }

    #[test]
    fn decodes_an_empty_request_as_an_empty_list() {
        assert_eq!(decode_key_binding_request(&[]), Ok(Vec::new()));
    }

    #[test]
    fn skips_unknown_fields() {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 7, 42);
        protobuf::write_length_delimited_field(&mut body, 1, &[0x2a]);
        assert_eq!(decode_key_binding_request(&body), Ok(vec![42]));
    }

    #[test]
    fn encodes_success_with_exact_bytes() {
        let message = encode_key_binding_response(KeyBindingStatus::Success);
        assert_eq!(message.id, InputMessageId::KeyBindingResponse);
        assert_eq!(message.body, vec![0x08, 0x00]);
    }

    #[test]
    fn encodes_keycode_not_bound_with_exact_bytes() {
        let message = encode_key_binding_response(KeyBindingStatus::KeycodeNotBound);
        assert_eq!(message.id, InputMessageId::KeyBindingResponse);
        // -18 sign-extended through i64 as a varint (matches write_int32_field's
        // existing sign-extension behavior, exercised elsewhere in protobuf.rs).
        assert_eq!(
            message.body,
            vec![
                0x08, 0xee, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01
            ]
        );
    }
}
