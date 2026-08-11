use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's ServiceDiscoveryRequest protobuf schema and
// OpenAuto's service-discovery handling at the pinned project revisions.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_SERVICE_DISCOVERY_SIZE: usize = 1024 * 1024;
pub const DEFAULT_MAX_DISCOVERY_ICON_SIZE: usize = 256 * 1024;
pub const DEFAULT_MAX_DISCOVERY_TEXT_SIZE: usize = 4 * 1024;
pub const DEFAULT_MAX_PHONE_INFO_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceDiscoveryLimits {
    pub maximum_total_size: usize,
    pub maximum_icon_size: usize,
    pub maximum_text_size: usize,
    pub maximum_phone_info_size: usize,
}

impl Default for ServiceDiscoveryLimits {
    fn default() -> Self {
        Self {
            maximum_total_size: DEFAULT_MAX_SERVICE_DISCOVERY_SIZE,
            maximum_icon_size: DEFAULT_MAX_DISCOVERY_ICON_SIZE,
            maximum_text_size: DEFAULT_MAX_DISCOVERY_TEXT_SIZE,
            maximum_phone_info_size: DEFAULT_MAX_PHONE_INFO_SIZE,
        }
    }
}

/// Privacy-preserving information about a service-discovery request.
///
/// The actual icons, labels, device name, and nested phone information are
/// deliberately not retained.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServiceDiscoveryRequestSummary {
    pub small_icon_bytes: Option<usize>,
    pub medium_icon_bytes: Option<usize>,
    pub large_icon_bytes: Option<usize>,
    pub label_text_bytes: Option<usize>,
    pub device_name_bytes: Option<usize>,
    pub phone_info_bytes: Option<usize>,
    pub unknown_fields: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceDiscoveryError {
    InvalidLimits,
    MessageTooLarge {
        size: usize,
        maximum: usize,
    },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    UnsupportedWireType(u8),
    UnexpectedWireType {
        field: u32,
        wire_type: u8,
    },
    FieldTooLarge {
        field: u32,
        size: usize,
        maximum: usize,
    },
    InvalidUtf8 {
        field: u32,
    },
    LengthNotRepresentable,
}

impl fmt::Display for ServiceDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => formatter.write_str("service-discovery limits must be non-zero"),
            Self::MessageTooLarge { size, maximum } => {
                write!(
                    formatter,
                    "service-discovery message {size} exceeds limit {maximum}"
                )
            }
            Self::Truncated => formatter.write_str("truncated service-discovery protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid service-discovery protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("service-discovery protobuf field number cannot be zero")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "service-discovery field {field} has unexpected wire type {wire_type}"
            ),
            Self::FieldTooLarge {
                field,
                size,
                maximum,
            } => write!(
                formatter,
                "service-discovery field {field} size {size} exceeds limit {maximum}"
            ),
            Self::InvalidUtf8 { field } => {
                write!(
                    formatter,
                    "service-discovery text field {field} is not UTF-8"
                )
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("service-discovery field length cannot be represented")
            }
        }
    }
}

impl std::error::Error for ServiceDiscoveryError {}

impl ProtobufDecodeError for ServiceDiscoveryError {
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

pub fn summarize_service_discovery_request(
    input: &[u8],
    limits: ServiceDiscoveryLimits,
) -> Result<ServiceDiscoveryRequestSummary, ServiceDiscoveryError> {
    validate_limits(limits)?;
    if input.len() > limits.maximum_total_size {
        return Err(ServiceDiscoveryError::MessageTooLarge {
            size: input.len(),
            maximum: limits.maximum_total_size,
        });
    }

    let mut cursor = 0;
    let mut summary = ServiceDiscoveryRequestSummary::default();
    while cursor < input.len() {
        let (field, wire_type) = protobuf::read_tag(input, &mut cursor)?;

        if (1..=6).contains(&field) {
            if wire_type != 2 {
                return Err(ServiceDiscoveryError::UnexpectedWireType { field, wire_type });
            }
            let value = protobuf::read_length_delimited(input, &mut cursor)?;
            let maximum = match field {
                1..=3 => limits.maximum_icon_size,
                4 | 5 => limits.maximum_text_size,
                6 => limits.maximum_phone_info_size,
                _ => unreachable!(),
            };
            if value.len() > maximum {
                return Err(ServiceDiscoveryError::FieldTooLarge {
                    field,
                    size: value.len(),
                    maximum,
                });
            }
            if matches!(field, 4 | 5) && std::str::from_utf8(value).is_err() {
                return Err(ServiceDiscoveryError::InvalidUtf8 { field });
            }
            match field {
                1 => summary.small_icon_bytes = Some(value.len()),
                2 => summary.medium_icon_bytes = Some(value.len()),
                3 => summary.large_icon_bytes = Some(value.len()),
                // Field 4 is label_text and field 5 is device_name per the
                // pinned aap_protobuf ServiceDiscoveryRequest.proto schema
                // (see docs/protocol/aasdk-adoption.md); there is no
                // device_brand field.
                4 => summary.label_text_bytes = Some(value.len()),
                5 => summary.device_name_bytes = Some(value.len()),
                6 => summary.phone_info_bytes = Some(value.len()),
                _ => unreachable!(),
            }
        } else {
            protobuf::skip_unknown_field(input, &mut cursor, wire_type)?;
            summary.unknown_fields = summary.unknown_fields.saturating_add(1);
        }
    }
    Ok(summary)
}

const fn validate_limits(limits: ServiceDiscoveryLimits) -> Result<(), ServiceDiscoveryError> {
    if limits.maximum_total_size == 0
        || limits.maximum_icon_size == 0
        || limits.maximum_text_size == 0
        || limits.maximum_phone_info_size == 0
    {
        Err(ServiceDiscoveryError::InvalidLimits)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_known_fields_without_retaining_private_content() {
        let request = [
            0x0a, 0x02, 0xaa, 0xbb, // small icon
            0x22, 0x0c, b's', b'e', b'c', b'r', b'e', b't', b'-', b'l', b'a', b'b', b'e', b'l',
            0x2a, 0x0c, b'p', b'r', b'i', b'v', b'a', b't', b'e', b'-', b'n', b'a', b'm', b'e',
            0x32, 0x02, 0x08, 0x01, // nested phone info
        ];
        let summary =
            summarize_service_discovery_request(&request, ServiceDiscoveryLimits::default())
                .expect("summarize");

        assert_eq!(summary.small_icon_bytes, Some(2));
        assert_eq!(summary.label_text_bytes, Some(12));
        assert_eq!(summary.device_name_bytes, Some(12));
        assert_eq!(summary.phone_info_bytes, Some(2));
        let debug = format!("{summary:?}");
        assert!(!debug.contains("secret-label"));
        assert!(!debug.contains("private-name"));
    }

    #[test]
    fn skips_supported_unknown_wire_types() {
        let request = [
            0x38, 0x96, 0x01, // field 7, varint
            0x41, 1, 2, 3, 4, 5, 6, 7, 8, // field 8, fixed64
            0x4a, 0x02, 9, 10, // field 9, bytes
            0x55, 11, 12, 13, 14, // field 10, fixed32
        ];
        let summary =
            summarize_service_discovery_request(&request, ServiceDiscoveryLimits::default())
                .expect("skip unknown fields");
        assert_eq!(summary.unknown_fields, 4);
    }

    #[test]
    fn rejects_malformed_protobuf() {
        let limits = ServiceDiscoveryLimits::default();
        assert_eq!(
            summarize_service_discovery_request(&[0x0a, 0x02, 0x01], limits),
            Err(ServiceDiscoveryError::Truncated)
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x00], limits),
            Err(ServiceDiscoveryError::InvalidFieldNumber)
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x0b], limits),
            Err(ServiceDiscoveryError::UnexpectedWireType {
                field: 1,
                wire_type: 3
            })
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x3b], limits),
            Err(ServiceDiscoveryError::UnsupportedWireType(3))
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x80; 10], limits),
            Err(ServiceDiscoveryError::InvalidVarint)
        );
    }

    #[test]
    fn enforces_all_limits_and_utf8() {
        let limits = ServiceDiscoveryLimits {
            maximum_total_size: 6,
            maximum_icon_size: 1,
            maximum_text_size: 2,
            maximum_phone_info_size: 1,
        };
        assert_eq!(
            summarize_service_discovery_request(&[0; 7], limits),
            Err(ServiceDiscoveryError::MessageTooLarge {
                size: 7,
                maximum: 6
            })
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x0a, 0x02, 1, 2], limits),
            Err(ServiceDiscoveryError::FieldTooLarge {
                field: 1,
                size: 2,
                maximum: 1
            })
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x22, 0x01, 0xff], limits),
            Err(ServiceDiscoveryError::InvalidUtf8 { field: 4 })
        );
        assert_eq!(
            summarize_service_discovery_request(&[0x32, 0x02, 1, 2], limits),
            Err(ServiceDiscoveryError::FieldTooLarge {
                field: 6,
                size: 2,
                maximum: 1
            })
        );
        assert_eq!(
            summarize_service_discovery_request(
                &[],
                ServiceDiscoveryLimits {
                    maximum_total_size: 0,
                    ..limits
                }
            ),
            Err(ServiceDiscoveryError::InvalidLimits)
        );
    }
}
