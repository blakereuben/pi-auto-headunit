use std::fmt;

use crate::control::{ControlMessage, ControlMessageId};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's PingRequest/PingResponse protobuf schema
// (protobuf/aap_protobuf/service/control/message/) and
// ControlServiceChannel's sendPingRequest/sendPingResponse behaviour (both
// EncryptionType::PLAIN, MessageType::SPECIFIC), at the pinned project
// revision (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PingError {
    MissingTimestamp,
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for PingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingTimestamp => {
                formatter.write_str("PingResponse is missing its required timestamp field")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "ping field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated ping protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid ping protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("ping protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("ping field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for PingError {}

impl ProtobufDecodeError for PingError {
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

/// Encodes `PingRequest` as a control message. `timestamp` (field 1,
/// required int64) is the only field this probe ever populates — AASDK's
/// `PingRequest.proto` has optional fields for bug-report/wifi-visible-
/// network scenarios this probe never triggers.
#[must_use]
pub fn encode_ping_request(timestamp_millis: i64) -> ControlMessage {
    let mut body = Vec::new();
    protobuf::write_int64_field(&mut body, 1, timestamp_millis);
    ControlMessage {
        id: ControlMessageId::PingRequest,
        body,
    }
}

/// Decodes `PingResponse` and returns its echoed `timestamp` (field 1,
/// required int64) — the exact value this probe sent in the matching
/// `PingRequest`, per AASDK's `sendPingResponse`.
pub fn decode_ping_response(body: &[u8]) -> Result<i64, PingError> {
    decode_ping_timestamp(body)
}

/// Decodes a phone-initiated `PingRequest` and returns its `timestamp`
/// (field 1, required int64). AASDK's `ControlServiceChannel` handles
/// `PingRequest` arriving from either side — this probe previously only
/// ever sent one and decoded the matching `PingResponse`; a real phone
/// was observed sending its own `PingRequest` mid-session (real-hardware
/// wireless trial, see `docs/protocol/error-2-investigation.md`-style
/// finding logged in project history), which this probe rejected as an
/// unexpected control message. Same wire shape as `PingResponse`, so this
/// shares the same decode logic.
pub fn decode_ping_request(body: &[u8]) -> Result<i64, PingError> {
    decode_ping_timestamp(body)
}

fn decode_ping_timestamp(body: &[u8]) -> Result<i64, PingError> {
    let mut cursor = 0;
    let mut timestamp = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<PingError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(PingError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<PingError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_wrap)]
                let value = raw as i64;
                timestamp = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<PingError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    timestamp.ok_or(PingError::MissingTimestamp)
}

/// Encodes `PingResponse` echoing back the `timestamp` (field 1) from a
/// phone-initiated `PingRequest`, per AASDK's `sendPingResponse`.
#[must_use]
pub fn encode_ping_response(timestamp_millis: i64) -> ControlMessage {
    let mut body = Vec::new();
    protobuf::write_int64_field(&mut body, 1, timestamp_millis);
    ControlMessage {
        id: ControlMessageId::PingResponse,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_request_with_exact_bytes() {
        let message = encode_ping_request(1_700_000_000_000);
        assert_eq!(message.id, ControlMessageId::PingRequest);
        let mut expected = Vec::new();
        protobuf::write_int64_field(&mut expected, 1, 1_700_000_000_000);
        assert_eq!(message.body, expected);
    }

    #[test]
    fn decode_round_trips_through_encode() {
        let mut body = Vec::new();
        protobuf::write_int64_field(&mut body, 1, 42);
        assert_eq!(decode_ping_response(&body), Ok(42));
    }

    #[test]
    fn rejects_missing_timestamp() {
        assert_eq!(decode_ping_response(&[]), Err(PingError::MissingTimestamp));
    }

    #[test]
    fn skips_unknown_fields_before_finding_timestamp() {
        let mut body = Vec::new();
        protobuf::write_int64_field(&mut body, 2, 99);
        protobuf::write_int64_field(&mut body, 1, 7);
        assert_eq!(decode_ping_response(&body), Ok(7));
    }

    #[test]
    fn decodes_phone_initiated_ping_request() {
        let mut body = Vec::new();
        protobuf::write_int64_field(&mut body, 1, 55);
        assert_eq!(decode_ping_request(&body), Ok(55));
    }

    #[test]
    fn encodes_response_echoing_exact_timestamp() {
        let message = encode_ping_response(1_700_000_000_123);
        assert_eq!(message.id, ControlMessageId::PingResponse);
        let mut expected = Vec::new();
        protobuf::write_int64_field(&mut expected, 1, 1_700_000_000_123);
        assert_eq!(message.body, expected);
    }
}
