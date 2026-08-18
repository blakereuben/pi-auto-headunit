use std::fmt;

use crate::control::{ControlMessage, ControlMessageId};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's ByeByeRequest/ByeByeResponse/ByeByeReason
// protobuf schema (protobuf/aap_protobuf/service/control/message/), and
// ControlServiceChannel's/AndroidAutoEntity's onShutdownRequest dispatch
// (always replies, then tears the session down once the response is
// confirmed sent), at the pinned project revision
// (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.control.message.ByeByeReason`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByeByeReason {
    UserSelection,
    DeviceSwitch,
    NotSupported,
    NotCurrentlySupported,
    ProbeSupported,
    Unknown(i32),
}

impl ByeByeReason {
    const fn from_wire(value: i32) -> Self {
        match value {
            1 => Self::UserSelection,
            2 => Self::DeviceSwitch,
            3 => Self::NotSupported,
            4 => Self::NotCurrentlySupported,
            5 => Self::ProbeSupported,
            value => Self::Unknown(value),
        }
    }

    const fn to_wire(self) -> i32 {
        match self {
            Self::UserSelection => 1,
            Self::DeviceSwitch => 2,
            Self::NotSupported => 3,
            Self::NotCurrentlySupported => 4,
            Self::ProbeSupported => 5,
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByeByeError {
    MissingReason,
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for ByeByeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingReason => {
                formatter.write_str("ByeByeRequest is missing its required reason field")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "byebye field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated byebye protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid byebye protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("byebye protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("byebye field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for ByeByeError {}

impl ProtobufDecodeError for ByeByeError {
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

/// Decodes `ByeByeRequest.reason` (field 1, required enum). An unrecognized
/// enum *value* survives as `ByeByeReason::Unknown` rather than failing —
/// still informative, matching every other enum-value decoder in this
/// crate; only a *missing* field fails closed.
pub fn decode_byebye_request(body: &[u8]) -> Result<ByeByeReason, ByeByeError> {
    let mut cursor = 0;
    let mut reason = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<ByeByeError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(ByeByeError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<ByeByeError>(body, &mut cursor)?;
                reason = Some(ByeByeReason::from_wire(varint_to_i32(raw)));
            }
            _ => {
                protobuf::skip_unknown_field::<ByeByeError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    reason.ok_or(ByeByeError::MissingReason)
}

/// Encodes `ByeByeResponse` — the message has no fields at all, so the
/// encoded body is always empty.
#[must_use]
pub fn encode_byebye_response() -> ControlMessage {
    ControlMessage {
        id: ControlMessageId::ByeByeResponse,
        body: Vec::new(),
    }
}

/// Encodes a `ByeByeRequest` (field 1, `reason`) for the head unit to
/// *send*, not just receive. `ControlServiceChannel::sendShutdownRequest`
/// in the pinned AASDK source confirms this message is symmetric — either
/// side may initiate it — not phone-only as this module's original
/// receive-only shape implied. Added 2026-08-18 after a real gap: ending a
/// probe session by simply dropping the transport (no wire notice at all)
/// left the phone believing the session was still live, so the next
/// `session-supervisor` cycle's fresh TLS handshake collided with the
/// phone still sending encrypted application data for the "old" session
/// (`encrypted frame received before TLS handshake completed`, recovered
/// only by a full soft-reset). Sending this first gives the phone a clean,
/// explicit signal to tear its own session state down.
#[must_use]
pub fn encode_byebye_request(reason: ByeByeReason) -> ControlMessage {
    let mut body = Vec::new();
    protobuf::write_int32_field(&mut body, 1, reason.to_wire());
    ControlMessage {
        id: ControlMessageId::ByeByeRequest,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_body(reason: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, reason);
        body
    }

    #[test]
    fn decodes_every_known_reason() {
        assert_eq!(
            decode_byebye_request(&request_body(1)),
            Ok(ByeByeReason::UserSelection)
        );
        assert_eq!(
            decode_byebye_request(&request_body(2)),
            Ok(ByeByeReason::DeviceSwitch)
        );
        assert_eq!(
            decode_byebye_request(&request_body(3)),
            Ok(ByeByeReason::NotSupported)
        );
        assert_eq!(
            decode_byebye_request(&request_body(4)),
            Ok(ByeByeReason::NotCurrentlySupported)
        );
        assert_eq!(
            decode_byebye_request(&request_body(5)),
            Ok(ByeByeReason::ProbeSupported)
        );
    }

    #[test]
    fn unknown_reason_survives_round_trip() {
        assert_eq!(
            decode_byebye_request(&request_body(99)),
            Ok(ByeByeReason::Unknown(99))
        );
    }

    #[test]
    fn rejects_missing_reason() {
        assert_eq!(decode_byebye_request(&[]), Err(ByeByeError::MissingReason));
    }

    #[test]
    fn skips_unknown_fields() {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 7, 42);
        protobuf::write_int32_field(&mut body, 1, 3);
        assert_eq!(decode_byebye_request(&body), Ok(ByeByeReason::NotSupported));
    }

    #[test]
    fn encodes_response_with_an_empty_body() {
        let message = encode_byebye_response();
        assert_eq!(message.id, ControlMessageId::ByeByeResponse);
        assert_eq!(message.body, Vec::<u8>::new());
    }

    #[test]
    fn encodes_and_decodes_every_known_reason_round_trip() {
        for reason in [
            ByeByeReason::UserSelection,
            ByeByeReason::DeviceSwitch,
            ByeByeReason::NotSupported,
            ByeByeReason::NotCurrentlySupported,
            ByeByeReason::ProbeSupported,
        ] {
            let message = encode_byebye_request(reason);
            assert_eq!(message.id, ControlMessageId::ByeByeRequest);
            assert_eq!(decode_byebye_request(&message.body), Ok(reason));
        }
    }

    #[test]
    fn encodes_unknown_reason_round_trip() {
        let message = encode_byebye_request(ByeByeReason::Unknown(42));
        assert_eq!(
            decode_byebye_request(&message.body),
            Ok(ByeByeReason::Unknown(42))
        );
    }
}
