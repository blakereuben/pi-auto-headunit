use std::fmt;

use crate::control::{ControlMessage, ControlMessageId};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's NavFocusRequestNotification/NavFocusNotification
// protobuf schema (protobuf/aap_protobuf/service/control/message/), and
// ControlServiceChannel's/AndroidAutoEntity's onNavigationFocusRequest
// dispatch (always replying with a hardcoded focus_type, not echoing the
// request), at the pinned project revision (9bf6adf933665dee26532201719fac14a047ccf1).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.control.message.NavFocusType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavFocusType {
    Native,
    Projected,
    Unknown(i32),
}

impl NavFocusType {
    const fn wire_value(self) -> i32 {
        match self {
            Self::Native => 1,
            Self::Projected => 2,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: i32) -> Self {
        match value {
            1 => Self::Native,
            2 => Self::Projected,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavFocusError {
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for NavFocusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "nav-focus field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated nav-focus protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid nav-focus protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("nav-focus protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("nav-focus field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for NavFocusError {}

impl ProtobufDecodeError for NavFocusError {
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

/// Decodes `NavFocusRequestNotification.focus_type` (field 1, **optional**
/// enum — unlike every other request this crate decodes, an absent field
/// here is a valid request, not an error).
pub fn decode_nav_focus_request(body: &[u8]) -> Result<Option<NavFocusType>, NavFocusError> {
    let mut cursor = 0;
    let mut focus_type = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<NavFocusError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(NavFocusError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<NavFocusError>(body, &mut cursor)?;
                focus_type = Some(NavFocusType::from_wire(varint_to_i32(raw)));
            }
            _ => {
                protobuf::skip_unknown_field::<NavFocusError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    Ok(focus_type)
}

/// Encodes `NavFocusNotification` as a control message. This project has no
/// native in-car navigation competing for focus, so it always answers
/// `Projected` (the phone keeps focus) regardless of what was requested —
/// matching `OpenAuto`'s own hardcoded response, not echoing the request.
#[must_use]
pub fn encode_nav_focus_notification(focus_type: NavFocusType) -> ControlMessage {
    let mut body = Vec::new();
    // NavFocusNotification.focus_type (field 1, required enum).
    protobuf::write_int32_field(&mut body, 1, focus_type.wire_value());
    ControlMessage {
        id: ControlMessageId::NavFocusNotification,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_body(focus_type: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, focus_type);
        body
    }

    #[test]
    fn decodes_known_focus_types() {
        assert_eq!(
            decode_nav_focus_request(&request_body(1)),
            Ok(Some(NavFocusType::Native))
        );
        assert_eq!(
            decode_nav_focus_request(&request_body(2)),
            Ok(Some(NavFocusType::Projected))
        );
    }

    #[test]
    fn unknown_focus_type_survives_round_trip() {
        assert_eq!(
            decode_nav_focus_request(&request_body(99)),
            Ok(Some(NavFocusType::Unknown(99)))
        );
    }

    #[test]
    fn absent_focus_type_is_a_valid_request() {
        assert_eq!(decode_nav_focus_request(&[]), Ok(None));
    }

    #[test]
    fn skips_unknown_fields() {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 7, 42);
        protobuf::write_int32_field(&mut body, 1, 2);
        assert_eq!(
            decode_nav_focus_request(&body),
            Ok(Some(NavFocusType::Projected))
        );
    }

    #[test]
    fn encodes_notification_with_exact_bytes() {
        let message = encode_nav_focus_notification(NavFocusType::Projected);
        assert_eq!(message.id, ControlMessageId::NavFocusNotification);
        assert_eq!(message.body, vec![0x08, 0x02]);
    }
}
