//! Android Auto Wireless (`aaw`) cold-start bootstrap protocol.
//!
//! A standalone, pre-handshake exchange run directly over a Bluetooth
//! RFCOMM stream, **not** this crate's `FrameHeader`/`Message`/`Channel`
//! envelope (`decode_frame`/`encode_frame` in `lib.rs`) — no channel
//! concept, no encryption, no relation to the `HandshakeStateMachine` used
//! for the eventual USB/TCP session. Framing is a bare 4-byte header:
//! `[u16 BE length][u16 BE message_id][protobuf payload]`.
//!
//! Field mapping confirmed directly against the pinned primary AASDK
//! source (`protobuf/aap_protobuf/aaw/{MessageId,WifiStartRequest,
//! WifiInfoRequest,WifiInfoResponse,WifiStartResponse,WifiConnectionStatus,
//! WifiVersionRequest,WifiVersionResponse,Status}.proto`, revision
//! `9bf6adf933665dee26532201719fac14a047ccf1`) — present in the pinned
//! tree but with no `.cpp` consumer there. Confirmed as the real,
//! actually-used wire format (not just a schema no implementation
//! touches) by reading `manio/aa-proxy-rs`'s actual Rust source directly
//! (`src/bluetooth.rs`, GPL-2.0-only — cited here only for the wire-shape
//! fact, no code reproduced or adopted; see
//! `docs/protocol/wireless-source-assessment.md` for the full citation
//! trail, including the AASDK-vs-`aa-proxy-rs` cross-check that corrected
//! an earlier, wrong first-pass guess). `WifiSecurityMode`/
//! `AccessPointType` reuse the enums already field-mapped from
//! `service.wifiprojection.message` in that same document — this file
//! adds its own copies since nothing in this crate referenced them
//! before now.
//! Copyright (C) 2018 f1x.studio (Michal Szwaj)
//! SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

/// `[u16 length][u16 message_id]` — see module doc comment.
pub const AAW_HEADER_SIZE: usize = 4;

/// `aap_protobuf.aaw.MessageId`. Wire values `1..=7` — deliberately *not*
/// the `32768+` range every ordinary AAP channel message-ID enum in this
/// crate uses (e.g. `MediaMessageId`), since this isn't an AAP channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AawMessageId {
    WifiStartRequest,
    WifiInfoRequest,
    WifiInfoResponse,
    WifiVersionRequest,
    WifiVersionResponse,
    WifiConnectionStatus,
    WifiStartResponse,
    Unknown(u16),
}

impl AawMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::WifiStartRequest => 1,
            Self::WifiInfoRequest => 2,
            Self::WifiInfoResponse => 3,
            Self::WifiVersionRequest => 4,
            Self::WifiVersionResponse => 5,
            Self::WifiConnectionStatus => 6,
            Self::WifiStartResponse => 7,
            Self::Unknown(value) => value,
        }
    }

    #[must_use]
    pub const fn from_wire(value: u16) -> Self {
        match value {
            1 => Self::WifiStartRequest,
            2 => Self::WifiInfoRequest,
            3 => Self::WifiInfoResponse,
            4 => Self::WifiVersionRequest,
            5 => Self::WifiVersionResponse,
            6 => Self::WifiConnectionStatus,
            7 => Self::WifiStartResponse,
            value => Self::Unknown(value),
        }
    }
}

/// `aap_protobuf.aaw.Status`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AawStatus {
    Success,
    UnsolicitedMessage,
    NoCompatibleVersion,
    WifiInaccessibleChannel,
    WifiIncorrectCredentials,
    ProjectionAlreadyStarted,
    WifiDisabled,
    WifiNotYetStarted,
    InvalidHost,
    NoSupportedWifiChannels,
    InstructUserToCheckThePhone,
    PhoneWifiDisabled,
    WifiNetworkUnavailable,
    Unknown(i32),
}

impl AawStatus {
    #[must_use]
    pub const fn wire_value(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::UnsolicitedMessage => 1,
            Self::NoCompatibleVersion => -1,
            Self::WifiInaccessibleChannel => -2,
            Self::WifiIncorrectCredentials => -3,
            Self::ProjectionAlreadyStarted => -4,
            Self::WifiDisabled => -5,
            Self::WifiNotYetStarted => -6,
            Self::InvalidHost => -7,
            Self::NoSupportedWifiChannels => -8,
            Self::InstructUserToCheckThePhone => -9,
            Self::PhoneWifiDisabled => -10,
            Self::WifiNetworkUnavailable => -11,
            Self::Unknown(value) => value,
        }
    }

    #[must_use]
    pub const fn from_wire(value: i32) -> Self {
        match value {
            0 => Self::Success,
            1 => Self::UnsolicitedMessage,
            -1 => Self::NoCompatibleVersion,
            -2 => Self::WifiInaccessibleChannel,
            -3 => Self::WifiIncorrectCredentials,
            -4 => Self::ProjectionAlreadyStarted,
            -5 => Self::WifiDisabled,
            -6 => Self::WifiNotYetStarted,
            -7 => Self::InvalidHost,
            -8 => Self::NoSupportedWifiChannels,
            -9 => Self::InstructUserToCheckThePhone,
            -10 => Self::PhoneWifiDisabled,
            -11 => Self::WifiNetworkUnavailable,
            value => Self::Unknown(value),
        }
    }
}

/// `aap_protobuf.service.wifiprojection.message.WifiSecurityMode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiSecurityMode {
    UnknownSecurityMode,
    Open,
    Wep64,
    Wep128,
    WpaPersonal,
    Wpa2Personal,
    WpaWpa2Personal,
    WpaEnterprise,
    Wpa2Enterprise,
    WpaWpa2Enterprise,
    Unknown(i32),
}

impl WifiSecurityMode {
    #[must_use]
    pub const fn wire_value(self) -> i32 {
        match self {
            Self::UnknownSecurityMode => 0,
            Self::Open => 1,
            Self::Wep64 => 2,
            Self::Wep128 => 3,
            Self::WpaPersonal => 4,
            Self::Wpa2Personal => 5,
            Self::WpaWpa2Personal => 6,
            Self::WpaEnterprise => 7,
            Self::Wpa2Enterprise => 8,
            Self::WpaWpa2Enterprise => 9,
            Self::Unknown(value) => value,
        }
    }

    #[must_use]
    pub const fn from_wire(value: i32) -> Self {
        match value {
            0 => Self::UnknownSecurityMode,
            1 => Self::Open,
            2 => Self::Wep64,
            3 => Self::Wep128,
            4 => Self::WpaPersonal,
            5 => Self::Wpa2Personal,
            6 => Self::WpaWpa2Personal,
            7 => Self::WpaEnterprise,
            8 => Self::Wpa2Enterprise,
            9 => Self::WpaWpa2Enterprise,
            value => Self::Unknown(value),
        }
    }
}

/// `aap_protobuf.service.wifiprojection.message.AccessPointType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessPointType {
    Static,
    Dynamic,
    Unknown(i32),
}

impl AccessPointType {
    #[must_use]
    pub const fn wire_value(self) -> i32 {
        match self {
            Self::Static => 0,
            Self::Dynamic => 1,
            Self::Unknown(value) => value,
        }
    }

    #[must_use]
    pub const fn from_wire(value: i32) -> Self {
        match value {
            0 => Self::Static,
            1 => Self::Dynamic,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AawError {
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
    UnexpectedWireType { field: u32, wire_type: u8 },
    IntegerOutOfRange { field: u32 },
    MissingRequiredField { field: u32 },
    Incomplete { required: usize, available: usize },
}

impl fmt::Display for AawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => formatter.write_str("truncated aaw protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid aaw protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("aaw protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("aaw field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported aaw protobuf wire type {wire_type}")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "aaw field {field} has unexpected wire type {wire_type}"
            ),
            Self::IntegerOutOfRange { field } => {
                write!(formatter, "aaw field {field} integer value out of range")
            }
            Self::MissingRequiredField { field } => {
                write!(formatter, "aaw required field {field} missing")
            }
            Self::Incomplete {
                required,
                available,
            } => write!(
                formatter,
                "incomplete aaw message: {required} bytes required, {available} available"
            ),
        }
    }
}

impl std::error::Error for AawError {}

impl ProtobufDecodeError for AawError {
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

fn varint_to_u32(field: u32, raw: u64) -> Result<u32, AawError> {
    u32::try_from(raw).map_err(|_| AawError::IntegerOutOfRange { field })
}

/// One decoded `aaw` message: `body` is the raw protobuf payload, still
/// undecoded — callers pick the matching `decode_*` function by
/// `message_id`. `consumed` is the total header-plus-body byte count, for
/// the caller's own accumulation-buffer bookkeeping (mirrors
/// `DecodedFrame::consumed` in `lib.rs`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedAawMessage<'a> {
    pub message_id: AawMessageId,
    pub body: &'a [u8],
    pub consumed: usize,
}

/// Decodes one `[u16 length][u16 message_id][payload]` frame from the
/// front of `input`. `Err(AawError::Incomplete { required, .. })` means
/// the caller should accumulate more bytes from the transport and retry
/// — the same pattern `decode_frame`/`FrameError::Incomplete` already
/// establishes in `lib.rs`, since this stream (a raw Bluetooth RFCOMM
/// socket) gives no delivery-boundary guarantee either.
pub fn decode_aaw_message(input: &[u8]) -> Result<DecodedAawMessage<'_>, AawError> {
    if input.len() < AAW_HEADER_SIZE {
        return Err(AawError::Incomplete {
            required: AAW_HEADER_SIZE,
            available: input.len(),
        });
    }
    let length = usize::from(u16::from_be_bytes([input[0], input[1]]));
    let message_id = AawMessageId::from_wire(u16::from_be_bytes([input[2], input[3]]));
    let total = AAW_HEADER_SIZE + length;
    if input.len() < total {
        return Err(AawError::Incomplete {
            required: total,
            available: input.len(),
        });
    }
    Ok(DecodedAawMessage {
        message_id,
        body: &input[AAW_HEADER_SIZE..total],
        consumed: total,
    })
}

/// Encodes one `aaw` message. `body.len()` must fit in `u16` (the wire
/// length field) — every message this bootstrap exchange ever sends
/// (`WifiStartRequest`/`WifiInfoResponse`) is a handful of short strings,
/// nowhere near that bound in practice.
pub fn encode_aaw_message(message_id: AawMessageId, body: &[u8]) -> Result<Vec<u8>, AawError> {
    let length = u16::try_from(body.len()).map_err(|_| AawError::LengthNotRepresentable)?;
    let mut out = Vec::with_capacity(AAW_HEADER_SIZE + body.len());
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&message_id.wire_value().to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// `aap_protobuf.aaw.WifiStartRequest` — sent by the head unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WifiStartRequest {
    pub ip_address: String,
    pub port: u32,
}

#[must_use]
pub fn encode_wifi_start_request(request: &WifiStartRequest) -> Vec<u8> {
    let mut body = Vec::new();
    protobuf::write_length_delimited_field(&mut body, 1, request.ip_address.as_bytes());
    protobuf::write_uint32_field(&mut body, 2, request.port);
    body
}

/// `aap_protobuf.aaw.WifiInfoResponse` — sent by the head unit, the
/// actual Wi-Fi-join payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WifiInfoResponse {
    pub ssid: String,
    pub password: String,
    pub bssid: String,
    pub security_mode: WifiSecurityMode,
    pub access_point_type: Option<AccessPointType>,
}

#[must_use]
pub fn encode_wifi_info_response(response: &WifiInfoResponse) -> Vec<u8> {
    let mut body = Vec::new();
    protobuf::write_length_delimited_field(&mut body, 1, response.ssid.as_bytes());
    protobuf::write_length_delimited_field(&mut body, 2, response.password.as_bytes());
    protobuf::write_length_delimited_field(&mut body, 3, response.bssid.as_bytes());
    protobuf::write_int32_field(&mut body, 4, response.security_mode.wire_value());
    if let Some(access_point_type) = response.access_point_type {
        protobuf::write_int32_field(&mut body, 5, access_point_type.wire_value());
    }
    body
}

/// `aap_protobuf.aaw.WifiStartResponse` — sent by the phone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WifiStartResponse {
    pub ip_address: Option<String>,
    pub port: Option<u32>,
    pub status: AawStatus,
}

pub fn decode_wifi_start_response(body: &[u8]) -> Result<WifiStartResponse, AawError> {
    let mut cursor = 0;
    let mut ip_address = None;
    let mut port = None;
    let mut status = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<AawError>(body, &mut cursor)?;
        match field {
            1 => {
                if wire_type != 2 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let bytes = protobuf::read_length_delimited::<AawError>(body, &mut cursor)?;
                ip_address = Some(String::from_utf8_lossy(bytes).into_owned());
            }
            2 => {
                if wire_type != 0 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let raw = protobuf::read_varint::<AawError>(body, &mut cursor)?;
                port = Some(varint_to_u32(field, raw)?);
            }
            3 => {
                if wire_type != 0 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let raw = protobuf::read_varint::<AawError>(body, &mut cursor)?;
                status = Some(AawStatus::from_wire(varint_to_i32(raw)));
            }
            _ => {
                protobuf::skip_unknown_field::<AawError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    Ok(WifiStartResponse {
        ip_address,
        port,
        status: status.ok_or(AawError::MissingRequiredField { field: 3 })?,
    })
}

/// `aap_protobuf.aaw.WifiConnectionStatus` — sent by the phone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WifiConnectionStatus {
    pub status: AawStatus,
    pub error_message: Option<String>,
}

pub fn decode_wifi_connection_status(body: &[u8]) -> Result<WifiConnectionStatus, AawError> {
    let mut cursor = 0;
    let mut status = None;
    let mut error_message = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<AawError>(body, &mut cursor)?;
        match field {
            1 => {
                if wire_type != 0 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let raw = protobuf::read_varint::<AawError>(body, &mut cursor)?;
                status = Some(AawStatus::from_wire(varint_to_i32(raw)));
            }
            2 => {
                if wire_type != 2 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let bytes = protobuf::read_length_delimited::<AawError>(body, &mut cursor)?;
                error_message = Some(String::from_utf8_lossy(bytes).into_owned());
            }
            _ => {
                protobuf::skip_unknown_field::<AawError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    Ok(WifiConnectionStatus {
        status: status.ok_or(AawError::MissingRequiredField { field: 1 })?,
        error_message,
    })
}

/// `aap_protobuf.aaw.WifiVersionResponse` — field names kept as
/// `unknown_value_a..d`, matching the pinned schema's own unexplained
/// naming (no semantic names given). Decoded for completeness/logging if
/// a phone ever sends it unprompted; not part of this slice's driven
/// sequence (`WifiVersionRequest` is never sent).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WifiVersionResponse {
    pub unknown_value_a: u32,
    pub unknown_value_b: u32,
    pub unknown_value_c: Option<String>,
    pub unknown_value_d: u32,
}

pub fn decode_wifi_version_response(body: &[u8]) -> Result<WifiVersionResponse, AawError> {
    let mut cursor = 0;
    let mut unknown_value_a = None;
    let mut unknown_value_b = None;
    let mut unknown_value_c = None;
    let mut unknown_value_d = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<AawError>(body, &mut cursor)?;
        match field {
            1 | 2 | 4 => {
                if wire_type != 0 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let raw = protobuf::read_varint::<AawError>(body, &mut cursor)?;
                let value = Some(varint_to_u32(field, raw)?);
                match field {
                    1 => unknown_value_a = value,
                    2 => unknown_value_b = value,
                    _ => unknown_value_d = value,
                }
            }
            3 => {
                if wire_type != 2 {
                    return Err(AawError::UnexpectedWireType { field, wire_type });
                }
                let bytes = protobuf::read_length_delimited::<AawError>(body, &mut cursor)?;
                unknown_value_c = Some(String::from_utf8_lossy(bytes).into_owned());
            }
            _ => {
                protobuf::skip_unknown_field::<AawError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    Ok(WifiVersionResponse {
        unknown_value_a: unknown_value_a.ok_or(AawError::MissingRequiredField { field: 1 })?,
        unknown_value_b: unknown_value_b.ok_or(AawError::MissingRequiredField { field: 2 })?,
        unknown_value_c,
        unknown_value_d: unknown_value_d.ok_or(AawError::MissingRequiredField { field: 4 })?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_round_trips_known_and_unknown_values() {
        for (id, wire) in [
            (AawMessageId::WifiStartRequest, 1),
            (AawMessageId::WifiInfoRequest, 2),
            (AawMessageId::WifiInfoResponse, 3),
            (AawMessageId::WifiVersionRequest, 4),
            (AawMessageId::WifiVersionResponse, 5),
            (AawMessageId::WifiConnectionStatus, 6),
            (AawMessageId::WifiStartResponse, 7),
        ] {
            assert_eq!(id.wire_value(), wire);
            assert_eq!(AawMessageId::from_wire(wire), id);
        }
        assert_eq!(AawMessageId::from_wire(99), AawMessageId::Unknown(99));
    }

    #[test]
    fn status_round_trips_every_documented_value() {
        for (status, wire) in [
            (AawStatus::Success, 0),
            (AawStatus::UnsolicitedMessage, 1),
            (AawStatus::NoCompatibleVersion, -1),
            (AawStatus::WifiInaccessibleChannel, -2),
            (AawStatus::WifiIncorrectCredentials, -3),
            (AawStatus::ProjectionAlreadyStarted, -4),
            (AawStatus::WifiDisabled, -5),
            (AawStatus::WifiNotYetStarted, -6),
            (AawStatus::InvalidHost, -7),
            (AawStatus::NoSupportedWifiChannels, -8),
            (AawStatus::InstructUserToCheckThePhone, -9),
            (AawStatus::PhoneWifiDisabled, -10),
            (AawStatus::WifiNetworkUnavailable, -11),
        ] {
            assert_eq!(status.wire_value(), wire);
            assert_eq!(AawStatus::from_wire(wire), status);
        }
        assert_eq!(AawStatus::from_wire(42), AawStatus::Unknown(42));
    }

    #[test]
    fn message_framing_round_trips_and_reports_incomplete() {
        let body = b"hello".to_vec();
        let encoded = encode_aaw_message(AawMessageId::WifiStartRequest, &body).expect("encode");
        assert_eq!(
            decode_aaw_message(&encoded[..3]),
            Err(AawError::Incomplete {
                required: AAW_HEADER_SIZE,
                available: 3
            })
        );
        assert_eq!(
            decode_aaw_message(&encoded[..AAW_HEADER_SIZE + 2]),
            Err(AawError::Incomplete {
                required: encoded.len(),
                available: AAW_HEADER_SIZE + 2
            })
        );
        let decoded = decode_aaw_message(&encoded).expect("decode");
        assert_eq!(decoded.message_id, AawMessageId::WifiStartRequest);
        assert_eq!(decoded.body, body.as_slice());
        assert_eq!(decoded.consumed, encoded.len());
    }

    #[test]
    fn wifi_start_request_encodes_expected_fields() {
        let request = WifiStartRequest {
            ip_address: "192.168.4.1".into(),
            port: 5288,
        };
        let body = encode_wifi_start_request(&request);
        // Field 1 (string, tag 0x0a) then field 2 (varint, tag 0x10).
        assert_eq!(body[0], 0x0a);
        assert!(body.ends_with(&[0x10, 0xa8, 0x29])); // 5288 as varint
    }

    #[test]
    fn wifi_info_response_round_trips_via_start_response_decoder_shape() {
        // No decoder exists for WifiInfoResponse (head-unit-authored, never
        // received) — this instead confirms the encoder produces a body
        // decode_wifi_start_response's own field-walking logic can parse
        // structurally (same tag/wire-type shapes), catching gross framing
        // mistakes without needing a dedicated decoder just for a test.
        let response = WifiInfoResponse {
            ssid: "pi-auto-headunit".into(),
            password: "synthetic-test-password".into(),
            bssid: "02:00:00:00:00:00".into(),
            security_mode: WifiSecurityMode::Wpa2Personal,
            access_point_type: Some(AccessPointType::Static),
        };
        let body = encode_wifi_info_response(&response);
        let mut cursor = 0;
        let (field, wire_type) = protobuf::read_tag::<AawError>(&body, &mut cursor).expect("tag");
        assert_eq!((field, wire_type), (1, 2));
        let ssid = protobuf::read_length_delimited::<AawError>(&body, &mut cursor).expect("ssid");
        assert_eq!(ssid, response.ssid.as_bytes());
    }

    #[test]
    fn wifi_start_response_decodes_success_with_optional_fields_present() {
        let mut body = Vec::new();
        protobuf::write_length_delimited_field(&mut body, 1, b"192.168.4.2");
        protobuf::write_uint32_field(&mut body, 2, 5288);
        protobuf::write_int32_field(&mut body, 3, AawStatus::Success.wire_value());
        let decoded = decode_wifi_start_response(&body).expect("decode");
        assert_eq!(decoded.ip_address.as_deref(), Some("192.168.4.2"));
        assert_eq!(decoded.port, Some(5288));
        assert_eq!(decoded.status, AawStatus::Success);
    }

    #[test]
    fn wifi_start_response_decodes_with_optional_fields_absent() {
        let mut body = Vec::new();
        protobuf::write_int32_field(
            &mut body,
            3,
            AawStatus::WifiIncorrectCredentials.wire_value(),
        );
        let decoded = decode_wifi_start_response(&body).expect("decode");
        assert_eq!(decoded.ip_address, None);
        assert_eq!(decoded.port, None);
        assert_eq!(decoded.status, AawStatus::WifiIncorrectCredentials);
    }

    #[test]
    fn wifi_start_response_rejects_missing_required_status() {
        assert_eq!(
            decode_wifi_start_response(&[]),
            Err(AawError::MissingRequiredField { field: 3 })
        );
    }

    #[test]
    fn wifi_connection_status_round_trips_with_and_without_error_message() {
        let mut success_body = Vec::new();
        protobuf::write_int32_field(&mut success_body, 1, AawStatus::Success.wire_value());
        let decoded = decode_wifi_connection_status(&success_body).expect("decode");
        assert_eq!(decoded.status, AawStatus::Success);
        assert_eq!(decoded.error_message, None);

        let mut failure_body = Vec::new();
        protobuf::write_int32_field(&mut failure_body, 1, AawStatus::WifiDisabled.wire_value());
        protobuf::write_length_delimited_field(&mut failure_body, 2, b"wifi is off");
        let decoded = decode_wifi_connection_status(&failure_body).expect("decode");
        assert_eq!(decoded.status, AawStatus::WifiDisabled);
        assert_eq!(decoded.error_message.as_deref(), Some("wifi is off"));
    }

    #[test]
    fn wifi_version_response_decodes_all_four_fields() {
        let mut body = Vec::new();
        protobuf::write_uint32_field(&mut body, 1, 1);
        protobuf::write_uint32_field(&mut body, 2, 2);
        protobuf::write_length_delimited_field(&mut body, 3, b"c");
        protobuf::write_uint32_field(&mut body, 4, 4);
        let decoded = decode_wifi_version_response(&body).expect("decode");
        assert_eq!(decoded.unknown_value_a, 1);
        assert_eq!(decoded.unknown_value_b, 2);
        assert_eq!(decoded.unknown_value_c.as_deref(), Some("c"));
        assert_eq!(decoded.unknown_value_d, 4);
    }

    #[test]
    fn security_mode_and_access_point_type_round_trip() {
        for (mode, wire) in [
            (WifiSecurityMode::Open, 1),
            (WifiSecurityMode::Wpa2Personal, 5),
            (WifiSecurityMode::WpaWpa2Enterprise, 9),
        ] {
            assert_eq!(mode.wire_value(), wire);
            assert_eq!(WifiSecurityMode::from_wire(wire), mode);
        }
        assert_eq!(AccessPointType::Static.wire_value(), 0);
        assert_eq!(AccessPointType::from_wire(1), AccessPointType::Dynamic);
    }
}
