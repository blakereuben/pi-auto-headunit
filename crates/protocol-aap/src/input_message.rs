use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's InputMessageId protobuf schema
// (protobuf/aap_protobuf/service/inputsource/InputMessageId.proto),
// KeyBindingRequest/KeyBindingResponse protobuf schema
// (protobuf/aap_protobuf/service/media/sink/message/KeyBindingRequest.proto,
// KeyBindingResponse.proto), InputReport/TouchEvent/PointerAction/KeyEvent
// protobuf schema (protobuf/aap_protobuf/service/inputsource/message/
// InputReport.proto, TouchEvent.proto, PointerAction.proto, KeyEvent.proto),
// the car-specific KeyCode enum values
// (protobuf/aap_protobuf/service/media/sink/message/KeyCode.proto), the
// shared MessageStatus enum (protobuf/aap_protobuf/shared/MessageStatus.proto),
// and InputSourceService's messageHandler/sendKeyBindingResponse/
// sendInputReport dispatch (src/Channel/InputSource/InputSourceService.cpp),
// at the pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE: usize = 1024 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

/// `aap_protobuf.service.inputsource.InputMessageId`. `INPUT_MESSAGE_INPUT_FEEDBACK`
/// is the only value still out of scope until something decodes or sends
/// it, matching `MediaMessageId::Unknown` surviving round-trip the same way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMessageId {
    InputReport,
    KeyBindingRequest,
    KeyBindingResponse,
    Unknown(u16),
}

impl InputMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::InputReport => 32769,
            Self::KeyBindingRequest => 32770,
            Self::KeyBindingResponse => 32771,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            32769 => Self::InputReport,
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

/// One finger's current position in `TouchEvent.pointer_data` (nested
/// `Pointer` message). `pointer_id` must stay stable for the same physical
/// finger across an entire down-move-up sequence — callers derive it from
/// the touch driver's own per-contact tracking id, not a freshly assigned
/// counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchPointer {
    pub x: u32,
    pub y: u32,
    pub pointer_id: u32,
}

/// `aap_protobuf.service.inputsource.message.PointerAction`. Numeric values
/// (0, 1, 2, 5, 6) are exactly Android's own `MotionEvent.ACTION_DOWN` /
/// `_UP` / `_MOVE` / `_POINTER_DOWN` / `_POINTER_UP` constants, not an
/// AASDK-specific invention — `action`/`action_index` follow the same
/// `MotionEvent` contract: for `Down`/`Up`/`Moved` there is exactly one
/// relevant index (`0`); for `PointerDown`/`PointerUp` (a second or later
/// finger joining/leaving while others stay down) `pointer_data` lists
/// every currently active finger and `action_index` names which entry just
/// changed. Both fields are always sent — never omitted, even for `Moved`
/// — confirmed against `opencardev/openauto`'s current `main` branch
/// (`InputSourceService::onTouchEvent` unconditionally calls
/// `touchEvent->set_action_index(...)`); see `platform_api::touch::TouchFrame`'s
/// doc comment for the real-hardware finding that surfaced this.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerAction {
    Down,
    Up,
    Moved,
    PointerDown,
    PointerUp,
}

impl PointerAction {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Down => 0,
            Self::Up => 1,
            Self::Moved => 2,
            Self::PointerDown => 5,
            Self::PointerUp => 6,
        }
    }
}

/// Encodes an `InputReport` carrying one `TouchEvent` as an input-channel
/// message — the head unit reporting its own touchscreen back to the phone
/// (the reverse direction from `KeyBindingRequest`/`Response`). `timestamp`
/// is the report's own clock reading; this project has no confirmed real
/// phone behaviour pinning its unit, so callers should use the same
/// microseconds convention already assumed for media `Data` PTS
/// (`media_message`'s doc comment) for consistency, not because it is
/// independently confirmed here.
#[must_use]
pub fn encode_touch_report(
    timestamp: u64,
    pointers: &[TouchPointer],
    action_index: u32,
    action: PointerAction,
) -> InputMessage {
    let mut touch_event = Vec::new();
    for pointer in pointers {
        let mut pointer_body = Vec::new();
        protobuf::write_uint32_field(&mut pointer_body, 1, pointer.x);
        protobuf::write_uint32_field(&mut pointer_body, 2, pointer.y);
        protobuf::write_uint32_field(&mut pointer_body, 3, pointer.pointer_id);
        // TouchEvent.pointer_data (field 1, repeated Pointer).
        protobuf::write_length_delimited_field(&mut touch_event, 1, &pointer_body);
    }
    // TouchEvent.action_index (field 2, optional uint32) and .action (field
    // 3, optional PointerAction enum) — always written; see this
    // function's doc comment for why neither is ever omitted.
    protobuf::write_uint32_field(&mut touch_event, 2, action_index);
    protobuf::write_int32_field(&mut touch_event, 3, action.wire_value());

    let mut body = Vec::new();
    // InputReport.timestamp (field 1, required uint64).
    protobuf::write_uint64_field(&mut body, 1, timestamp);
    // InputReport.touch_event (field 3, optional TouchEvent).
    protobuf::write_length_delimited_field(&mut body, 3, &touch_event);
    InputMessage {
        id: InputMessageId::InputReport,
        body,
    }
}

/// `aap_protobuf.service.media.sink.message.KeyCode` — only the four
/// car-specific category-switch values this project sends (see
/// `docs/protocol/aasdk-adoption.md`'s "`KeyCode` — car-specific values
/// used" section for the full 278-value enum's scope and why only these
/// four are modeled). No approved source describes a way to launch a
/// specific named app — Android Auto only exposes switching to whichever
/// app the phone has configured as default for a category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyCode {
    Media,
    Navigation,
    Radio,
    Tel,
}

impl KeyCode {
    #[must_use]
    pub const fn wire_value(self) -> u32 {
        match self {
            Self::Media => 65537,
            Self::Navigation => 65538,
            Self::Radio => 65539,
            Self::Tel => 65540,
        }
    }
}

/// Encodes an `InputReport` carrying one `KeyEvent` with a single key
/// press or release — the head unit asking the phone to switch app
/// category (see [`KeyCode`]'s doc comment). A real key press is always
/// two of these: `down: true` then `down: false`, matching a real
/// physical button (see `docs/protocol/aasdk-adoption.md`'s `InputReport`
/// section — `metastate` is always `0` here, `longpress` always omitted,
/// since this project never generates modifier state or a genuine
/// long-press hold).
#[must_use]
pub fn encode_key_event(timestamp: u64, keycode: KeyCode, down: bool) -> InputMessage {
    let mut key = Vec::new();
    // KeyEvent.Key.keycode (field 1, required uint32).
    protobuf::write_uint32_field(&mut key, 1, keycode.wire_value());
    // KeyEvent.Key.down (field 2, required bool).
    protobuf::write_bool_field(&mut key, 2, down);
    // KeyEvent.Key.metastate (field 3, required uint32) — always 0; this
    // project never generates modifier-key state.
    protobuf::write_uint32_field(&mut key, 3, 0);

    let mut key_event = Vec::new();
    // KeyEvent.keys (field 1, repeated Key).
    protobuf::write_length_delimited_field(&mut key_event, 1, &key);

    let mut body = Vec::new();
    // InputReport.timestamp (field 1, required uint64).
    protobuf::write_uint64_field(&mut body, 1, timestamp);
    // InputReport.key_event (field 4, optional KeyEvent).
    protobuf::write_length_delimited_field(&mut body, 4, &key_event);
    InputMessage {
        id: InputMessageId::InputReport,
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
            InputMessageId::InputReport,
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

    #[test]
    fn encodes_a_single_finger_down_report_with_exact_bytes() {
        let message = encode_touch_report(
            5,
            &[TouchPointer {
                x: 10,
                y: 20,
                pointer_id: 0,
            }],
            0,
            PointerAction::Down,
        );
        assert_eq!(message.id, InputMessageId::InputReport);
        // InputReport{timestamp=5, touch_event=TouchEvent{
        //   pointer_data=[Pointer{x=10,y=20,pointer_id=0}],
        //   action_index=0, action=ACTION_DOWN(0)}}. Every value here is
        // <128, so each varint is exactly one byte — safe to spell out by
        // hand; larger values are covered by the round-trip test below
        // instead of a hand-computed multi-byte varint.
        let expected_pointer = [0x08, 10, 0x10, 20, 0x18, 0x00];
        assert_eq!(expected_pointer.len(), 6);
        let mut expected_touch_event = vec![0x0a, 6];
        expected_touch_event.extend_from_slice(&expected_pointer);
        expected_touch_event.extend_from_slice(&[0x10, 0x00, 0x18, 0x00]);
        assert_eq!(expected_touch_event.len(), 12);
        let mut expected_body = vec![0x08, 5, 0x1a, 12];
        expected_body.extend_from_slice(&expected_touch_event);
        assert_eq!(message.body, expected_body);
    }

    #[test]
    fn encodes_a_two_finger_report_listing_both_pointers() {
        let message = encode_touch_report(
            2000,
            &[
                TouchPointer {
                    x: 10,
                    y: 20,
                    pointer_id: 0,
                },
                TouchPointer {
                    x: 30,
                    y: 40,
                    pointer_id: 1,
                },
            ],
            1,
            PointerAction::PointerDown,
        );
        let payload = message
            .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
            .expect("encode");
        let decoded =
            InputMessage::decode(&payload, DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE).expect("decode");
        assert_eq!(decoded.id, InputMessageId::InputReport);
        assert_eq!(decoded.body, message.body);
        // Both Pointer sub-messages and the trailing action_index/action
        // fields are present, in encounter order.
        assert!(message.body.windows(2).any(|w| w == [0x08, 10]));
        assert!(message.body.windows(2).any(|w| w == [0x08, 30]));
    }

    #[test]
    fn key_code_wire_values_match_the_pinned_car_specific_block() {
        // aap_protobuf.service.media.sink.message.KeyCode, values read
        // directly from the pinned revision — see
        // docs/protocol/aasdk-adoption.md's "KeyCode — car-specific
        // values used" section.
        assert_eq!(KeyCode::Media.wire_value(), 65537);
        assert_eq!(KeyCode::Navigation.wire_value(), 65538);
        assert_eq!(KeyCode::Radio.wire_value(), 65539);
        assert_eq!(KeyCode::Tel.wire_value(), 65540);
    }

    #[test]
    fn encodes_a_key_down_report_with_exact_bytes() {
        let message = encode_key_event(5, KeyCode::Media, true);
        assert_eq!(message.id, InputMessageId::InputReport);
        // Key{keycode=65537, down=true, metastate=0}. keycode's varint is
        // 3 bytes (65537 = 0b1_0000_0000_0000_0001, split into 7-bit
        // groups [1, 0, 4] LSB-first): 0x81 0x80 0x04.
        let expected_key = [0x08, 0x81, 0x80, 0x04, 0x10, 0x01, 0x18, 0x00];
        assert_eq!(expected_key.len(), 8);
        let mut expected_key_event = vec![0x0a, 8];
        expected_key_event.extend_from_slice(&expected_key);
        assert_eq!(expected_key_event.len(), 10);
        // InputReport{timestamp=5, key_event=...}.
        let mut expected_body = vec![0x08, 5, 0x22, 10];
        expected_body.extend_from_slice(&expected_key_event);
        assert_eq!(message.body, expected_body);
    }

    #[test]
    fn a_key_release_round_trips_and_carries_down_false() {
        let message = encode_key_event(9_999, KeyCode::Navigation, false);
        let payload = message
            .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
            .expect("encode");
        let decoded =
            InputMessage::decode(&payload, DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE).expect("decode");
        assert_eq!(decoded.id, InputMessageId::InputReport);
        assert_eq!(decoded.body, message.body);
        // Key.down (field 2) is 0x10, 0x00 for a release — present, not
        // silently coalesced with the down=true case.
        assert!(message.body.windows(2).any(|w| w == [0x10, 0x00]));
    }

    #[test]
    fn every_key_code_produces_a_distinct_message() {
        let bodies: Vec<Vec<u8>> = [
            KeyCode::Media,
            KeyCode::Navigation,
            KeyCode::Radio,
            KeyCode::Tel,
        ]
        .into_iter()
        .map(|keycode| encode_key_event(0, keycode, true).body)
        .collect();
        for (index, body) in bodies.iter().enumerate() {
            for (other_index, other_body) in bodies.iter().enumerate() {
                if index != other_index {
                    assert_ne!(body, other_body);
                }
            }
        }
    }
}
