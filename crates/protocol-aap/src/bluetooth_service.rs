use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's BluetoothMessageId/BluetoothPairingRequest/
// BluetoothPairingResponse/BluetoothPairingMethod protobuf schema
// (protobuf/aap_protobuf/service/bluetooth/), at the pinned project
// revision (9bf6adf933665dee26532201719fac14a047ccf1) — field mapping
// recorded in `docs/protocol/wireless-source-assessment.md`. This channel
// was first thought (from reading only the `.proto`/README sources) to be
// unused by AASDK outside an already-connected reconnect scenario; a real
// phone sending a genuine `BluetoothPairingRequest` mid-session, on a real
// wireless-bootstrap trial, proved that assumption wrong — see
// `apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs`'s
// `handle_bluetooth_channel_message` doc comment for the full finding.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_BLUETOOTH_MESSAGE_BODY_SIZE: usize = 64 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

/// `aap_protobuf.shared.MessageStatus.STATUS_SUCCESS` — the only status
/// this encoder ever sends, matching `channel_open.rs`'s/`sensor.rs`'s own
/// local redefinition of the same constant rather than sharing it
/// cross-module.
const BLUETOOTH_STATUS_SUCCESS: i32 = 0;

/// `aap_protobuf.service.bluetooth.BluetoothMessageId`. Only
/// `PairingRequest`/`PairingResponse` are named — this probe never
/// initiates or decodes `AuthenticationData`/`AuthenticationResult`
/// (a real classic-Bluetooth pairing handshake over the OS Bluetooth
/// stack, out of scope for this project's current milestone).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BluetoothMessageId {
    PairingRequest,
    PairingResponse,
    Unknown(u16),
}

impl BluetoothMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::PairingRequest => 32769,
            Self::PairingResponse => 32770,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            32769 => Self::PairingRequest,
            32770 => Self::PairingResponse,
            value => Self::Unknown(value),
        }
    }
}

/// `aap_protobuf.service.bluetooth.message.BluetoothPairingMethod`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BluetoothPairingMethod {
    Unavailable,
    Oob,
    NumericComparison,
    PasskeyEntry,
    Pin,
    Unknown(i32),
}

impl BluetoothPairingMethod {
    const fn from_wire(value: i32) -> Self {
        match value {
            -1 => Self::Unavailable,
            1 => Self::Oob,
            2 => Self::NumericComparison,
            3 => Self::PasskeyEntry,
            4 => Self::Pin,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BluetoothServiceError {
    InvalidLimit,
    TruncatedMessageId { available: usize },
    BodyTooLarge { size: usize, maximum: usize },
    MissingPairingMethod,
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for BluetoothServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => {
                formatter.write_str("bluetooth service message limits must be non-zero")
            }
            Self::TruncatedMessageId { available } => write!(
                formatter,
                "bluetooth service message id requires 2 bytes, {available} available"
            ),
            Self::BodyTooLarge { size, maximum } => write!(
                formatter,
                "bluetooth service message body {size} exceeds limit {maximum}"
            ),
            Self::MissingPairingMethod => formatter
                .write_str("BluetoothPairingRequest is missing its required pairing_method field"),
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "bluetooth service field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated bluetooth service protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid bluetooth service protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("bluetooth service protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("bluetooth service field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for BluetoothServiceError {}

impl ProtobufDecodeError for BluetoothServiceError {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BluetoothServiceMessage {
    pub id: BluetoothMessageId,
    pub body: Vec<u8>,
}

impl BluetoothServiceMessage {
    pub fn decode(payload: &[u8], maximum_body_size: usize) -> Result<Self, BluetoothServiceError> {
        if maximum_body_size == 0 {
            return Err(BluetoothServiceError::InvalidLimit);
        }
        if payload.len() < MESSAGE_ID_SIZE {
            return Err(BluetoothServiceError::TruncatedMessageId {
                available: payload.len(),
            });
        }
        let body_size = payload.len() - MESSAGE_ID_SIZE;
        if body_size > maximum_body_size {
            return Err(BluetoothServiceError::BodyTooLarge {
                size: body_size,
                maximum: maximum_body_size,
            });
        }
        Ok(Self {
            id: BluetoothMessageId::from_wire(u16::from_be_bytes([payload[0], payload[1]])),
            body: payload[MESSAGE_ID_SIZE..].to_vec(),
        })
    }

    pub fn encode(&self, maximum_body_size: usize) -> Result<Vec<u8>, BluetoothServiceError> {
        if maximum_body_size == 0 {
            return Err(BluetoothServiceError::InvalidLimit);
        }
        if self.body.len() > maximum_body_size {
            return Err(BluetoothServiceError::BodyTooLarge {
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

/// Decodes `BluetoothPairingRequest` and returns only its `pairing_method`
/// (field 2). `phone_address` (field 1, required string) is the phone's own
/// Bluetooth MAC address — a phone identifier this project's own rules
/// (`CLAUDE.md`) forbid ever storing or logging — so it is walked past
/// structurally (to reach field 2) and never materialized into a `String`,
/// exactly like an unrecognized field would be.
pub fn decode_bluetooth_pairing_request(
    body: &[u8],
) -> Result<BluetoothPairingMethod, BluetoothServiceError> {
    let mut cursor = 0;
    let mut pairing_method = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<BluetoothServiceError>(body, &mut cursor)?;
        match field {
            2 if wire_type != 0 => {
                return Err(BluetoothServiceError::UnexpectedWireType { field, wire_type });
            }
            2 => {
                let raw = protobuf::read_varint::<BluetoothServiceError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = raw as i32;
                pairing_method = Some(BluetoothPairingMethod::from_wire(value));
            }
            _ => {
                protobuf::skip_unknown_field::<BluetoothServiceError>(
                    body,
                    &mut cursor,
                    wire_type,
                )?;
            }
        }
    }
    pairing_method.ok_or(BluetoothServiceError::MissingPairingMethod)
}

/// Encodes `BluetoothPairingResponse{status: STATUS_SUCCESS,
/// already_paired: true}`. This probe has no real classic-Bluetooth audio
/// pairing implemented (out of scope for this milestone — no HFP/A2DP
/// service on this project's side beyond what the OS's own desktop
/// Bluetooth/audio stack already provides), so it declares "already
/// paired, no further pairing action needed" rather than attempting or
/// simulating a real pairing exchange (`AuthenticationData`/
/// `AuthenticationResult`) it can't actually complete.
#[must_use]
pub fn encode_bluetooth_pairing_response() -> BluetoothServiceMessage {
    let mut body = Vec::new();
    protobuf::write_int32_field(&mut body, 1, BLUETOOTH_STATUS_SUCCESS);
    protobuf::write_uint32_field(&mut body, 2, 1);
    BluetoothServiceMessage {
        id: BluetoothMessageId::PairingResponse,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_message_ids_are_big_endian_and_unknown_values_survive() {
        let decoded = BluetoothServiceMessage::decode(&[0x80, 0x01, 0xaa], 16).expect("decode");
        assert_eq!(decoded.id, BluetoothMessageId::PairingRequest);
        assert_eq!(decoded.body, vec![0xaa]);

        let decoded = BluetoothServiceMessage::decode(&[0x12, 0x34], 16).expect("decode");
        assert_eq!(decoded.id, BluetoothMessageId::Unknown(0x1234));
    }

    #[test]
    fn decodes_pairing_method_and_discards_phone_address() {
        let mut body = Vec::new();
        protobuf::write_length_delimited_field(&mut body, 1, b"AA:BB:CC:DD:EE:FF");
        protobuf::write_int32_field(&mut body, 2, 3);
        assert_eq!(
            decode_bluetooth_pairing_request(&body),
            Ok(BluetoothPairingMethod::PasskeyEntry)
        );
    }

    #[test]
    fn rejects_missing_pairing_method() {
        let mut body = Vec::new();
        protobuf::write_length_delimited_field(&mut body, 1, b"AA:BB:CC:DD:EE:FF");
        assert_eq!(
            decode_bluetooth_pairing_request(&body),
            Err(BluetoothServiceError::MissingPairingMethod)
        );
    }

    #[test]
    fn encodes_response_with_exact_bytes() {
        let message = encode_bluetooth_pairing_response();
        assert_eq!(message.id, BluetoothMessageId::PairingResponse);
        let mut expected = Vec::new();
        protobuf::write_int32_field(&mut expected, 1, BLUETOOTH_STATUS_SUCCESS);
        protobuf::write_uint32_field(&mut expected, 2, 1);
        assert_eq!(message.body, expected);
    }

    #[test]
    fn round_trips_through_encode_and_decode() {
        let message = encode_bluetooth_pairing_response();
        let payload = message
            .encode(DEFAULT_MAX_BLUETOOTH_MESSAGE_BODY_SIZE)
            .expect("encode");
        let decoded =
            BluetoothServiceMessage::decode(&payload, DEFAULT_MAX_BLUETOOTH_MESSAGE_BODY_SIZE)
                .expect("decode");
        assert_eq!(decoded, message);
    }
}
