use std::fmt;

use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's SensorMessageId/SensorRequest/SensorResponse/
// SensorBatch protobuf schema (protobuf/aap_protobuf/service/sensorsource/),
// the shared MessageStatus enum (protobuf/aap_protobuf/shared/MessageStatus.proto),
// and SensorService's messageHandler/onSensorStartRequest dispatch
// (src/autoapp/Service/SensorService.cpp, src/Channel/Sensor/SensorServiceChannel.cpp),
// at the pinned AASDK project revision (9bf6adf933665dee26532201719fac14a047ccf1)
// and the approved OpenAuto revision (aa90412bf93b5a5078495ea85ac9270c6297d369,
// docs/protocol/openauto-adoption.md).
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE: usize = 1024 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

/// `aap_protobuf.shared.MessageStatus.STATUS_SUCCESS` — the only status this
/// encoder ever sends, matching `channel_open.rs`'s own local redefinition
/// of the same constant rather than sharing it cross-module.
const SENSOR_STATUS_SUCCESS: i32 = 0;

/// `aap_protobuf.service.sensorsource.SensorMessageId`. Only the three
/// values this increment's request/response/batch exchange needs are
/// named; `SENSOR_MESSAGE_ERROR` (32772) is out of scope until something
/// decodes or sends it, matching `InputMessageId::Unknown`'s precedent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SensorMessageId {
    SensorRequest,
    SensorResponse,
    SensorBatch,
    Unknown(u16),
}

impl SensorMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::SensorRequest => 32769,
            Self::SensorResponse => 32770,
            Self::SensorBatch => 32771,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            32769 => Self::SensorRequest,
            32770 => Self::SensorResponse,
            32771 => Self::SensorBatch,
            value => Self::Unknown(value),
        }
    }
}

/// `aap_protobuf.service.sensorsource.message.SensorType`. Only the two
/// values this project advertises/handles (matching `OpenAuto`'s
/// `SensorService::fillFeatures()`) are named; every other value survives
/// round-trip as `Unknown`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SensorType {
    DrivingStatusData,
    NightMode,
    Unknown(i32),
}

impl SensorType {
    pub(crate) const fn wire_value(self) -> i32 {
        match self {
            Self::DrivingStatusData => 13,
            Self::NightMode => 10,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: i32) -> Self {
        match value {
            13 => Self::DrivingStatusData,
            10 => Self::NightMode,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SensorMessageError {
    InvalidLimit,
    TruncatedMessageId { available: usize },
    BodyTooLarge { size: usize, maximum: usize },
}

impl fmt::Display for SensorMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("sensor message limits must be non-zero"),
            Self::TruncatedMessageId { available } => write!(
                formatter,
                "sensor message id requires 2 bytes, {available} available"
            ),
            Self::BodyTooLarge { size, maximum } => {
                write!(
                    formatter,
                    "sensor message body {size} exceeds limit {maximum}"
                )
            }
        }
    }
}

impl std::error::Error for SensorMessageError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SensorMessage {
    pub id: SensorMessageId,
    pub body: Vec<u8>,
}

impl SensorMessage {
    pub fn decode(payload: &[u8], maximum_body_size: usize) -> Result<Self, SensorMessageError> {
        if maximum_body_size == 0 {
            return Err(SensorMessageError::InvalidLimit);
        }
        if payload.len() < MESSAGE_ID_SIZE {
            return Err(SensorMessageError::TruncatedMessageId {
                available: payload.len(),
            });
        }
        let body_size = payload.len() - MESSAGE_ID_SIZE;
        if body_size > maximum_body_size {
            return Err(SensorMessageError::BodyTooLarge {
                size: body_size,
                maximum: maximum_body_size,
            });
        }
        Ok(Self {
            id: SensorMessageId::from_wire(u16::from_be_bytes([payload[0], payload[1]])),
            body: payload[MESSAGE_ID_SIZE..].to_vec(),
        })
    }

    pub fn encode(&self, maximum_body_size: usize) -> Result<Vec<u8>, SensorMessageError> {
        if maximum_body_size == 0 {
            return Err(SensorMessageError::InvalidLimit);
        }
        if self.body.len() > maximum_body_size {
            return Err(SensorMessageError::BodyTooLarge {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SensorError {
    MissingSensorType,
    UnexpectedWireType { field: u32, wire_type: u8 },
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for SensorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSensorType => {
                formatter.write_str("SensorRequest is missing its required type field")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "sensor field {field} has unexpected wire type {wire_type}"
            ),
            Self::Truncated => formatter.write_str("truncated sensor protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid sensor protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("sensor protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("sensor field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for SensorError {}

impl ProtobufDecodeError for SensorError {
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

/// Decodes `SensorRequest.type` (field 1, required enum). `min_update_period`
/// (field 2, required int64) is read by nobody in `OpenAuto`'s own reference
/// implementation either — not decoded here, matching
/// `decode_audio_focus_request`'s precedent of only decoding the field
/// actually used; it falls through the unknown-field-skip wildcard arm.
pub fn decode_sensor_request(body: &[u8]) -> Result<SensorType, SensorError> {
    let mut cursor = 0;
    let mut sensor_type = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<SensorError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(SensorError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<SensorError>(body, &mut cursor)?;
                sensor_type = Some(SensorType::from_wire(varint_to_i32(raw)));
            }
            _ => {
                protobuf::skip_unknown_field::<SensorError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    sensor_type.ok_or(SensorError::MissingSensorType)
}

/// Encodes `SensorResponse` as a sensor-channel message. Always
/// `STATUS_SUCCESS`, matching `OpenAuto`'s `onSensorStartRequest` which
/// always responds `OK` regardless of `sensor_type` — no `FAIL` path is
/// modeled since nothing in this project's scope ever needs to send one.
#[must_use]
pub fn encode_sensor_response() -> SensorMessage {
    let mut body = Vec::new();
    // SensorResponse.status (field 1, required MessageStatus enum).
    protobuf::write_int32_field(&mut body, 1, SENSOR_STATUS_SUCCESS);
    SensorMessage {
        id: SensorMessageId::SensorResponse,
        body,
    }
}

/// Encodes a `SensorBatch` reporting `DRIVE_STATUS_UNRESTRICTED`, matching
/// `OpenAuto`'s `sendDrivingStatusUnrestricted()` — the only driving-status
/// value this project (no real driving-restriction pipeline) ever reports.
#[must_use]
pub fn encode_driving_status_unrestricted_batch() -> SensorMessage {
    let mut driving_status_data = Vec::new();
    // DrivingStatusData.status (field 1, required int32; DRIVE_STATUS_UNRESTRICTED = 0).
    protobuf::write_int32_field(&mut driving_status_data, 1, 0);
    let mut body = Vec::new();
    // SensorBatch.driving_status_data (field 13, repeated DrivingStatusData).
    protobuf::write_length_delimited_field(&mut body, 13, &driving_status_data);
    SensorMessage {
        id: SensorMessageId::SensorBatch,
        body,
    }
}

/// Encodes a `SensorBatch` reporting night mode, matching `OpenAuto`'s
/// `sendNightData()`. This project has no real day/night sensor yet — the
/// caller always passes `false` (matches `grant_audio_focus`'s own honesty
/// precedent: no real hardware pipeline exists yet).
#[must_use]
pub fn encode_night_mode_batch(is_night: bool) -> SensorMessage {
    let mut night_mode_data = Vec::new();
    // NightModeData.night_mode (field 1, optional bool).
    protobuf::write_int32_field(&mut night_mode_data, 1, i32::from(is_night));
    let mut body = Vec::new();
    // SensorBatch.night_mode_data (field 10, repeated NightModeData).
    protobuf::write_length_delimited_field(&mut body, 10, &night_mode_data);
    SensorMessage {
        id: SensorMessageId::SensorBatch,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_message_ids_are_big_endian_and_unknown_values_survive() {
        let decoded = SensorMessage::decode(&[0x12, 0x34, 0xaa], 4).expect("decode");
        assert_eq!(decoded.id, SensorMessageId::Unknown(0x1234));
        assert_eq!(decoded.body, [0xaa]);
        assert_eq!(decoded.encode(4).expect("encode"), [0x12, 0x34, 0xaa]);
    }

    #[test]
    fn known_ids_round_trip_through_their_wire_values() {
        for id in [
            SensorMessageId::SensorRequest,
            SensorMessageId::SensorResponse,
            SensorMessageId::SensorBatch,
        ] {
            let message = SensorMessage {
                id,
                body: Vec::new(),
            };
            let payload = message
                .encode(DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE)
                .expect("encode");
            let decoded = SensorMessage::decode(&payload, DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE)
                .expect("decode");
            assert_eq!(decoded.id, id);
        }
    }

    #[test]
    fn rejects_truncated_message_id() {
        assert_eq!(
            SensorMessage::decode(&[0x80], DEFAULT_MAX_SENSOR_MESSAGE_BODY_SIZE),
            Err(SensorMessageError::TruncatedMessageId { available: 1 })
        );
    }

    #[test]
    fn rejects_oversized_body_and_invalid_limit() {
        let message = SensorMessage {
            id: SensorMessageId::SensorRequest,
            body: vec![0; 3],
        };
        assert_eq!(
            message.encode(2),
            Err(SensorMessageError::BodyTooLarge {
                size: 3,
                maximum: 2
            })
        );
        assert_eq!(message.encode(0), Err(SensorMessageError::InvalidLimit));
        assert_eq!(
            SensorMessage::decode(&[0x80, 0x00], 0),
            Err(SensorMessageError::InvalidLimit)
        );
    }

    fn request_body(sensor_type: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, sensor_type);
        body
    }

    #[test]
    fn decodes_driving_status_and_night_mode_requests() {
        assert_eq!(
            decode_sensor_request(&request_body(13)),
            Ok(SensorType::DrivingStatusData)
        );
        assert_eq!(
            decode_sensor_request(&request_body(10)),
            Ok(SensorType::NightMode)
        );
    }

    #[test]
    fn unknown_sensor_type_survives_round_trip() {
        assert_eq!(
            decode_sensor_request(&request_body(99)),
            Ok(SensorType::Unknown(99))
        );
    }

    #[test]
    fn rejects_missing_sensor_type() {
        assert_eq!(
            decode_sensor_request(&[]),
            Err(SensorError::MissingSensorType)
        );
    }

    #[test]
    fn skips_unknown_fields_before_finding_sensor_type() {
        let mut body = Vec::new();
        protobuf::write_int64_field(&mut body, 2, 1000);
        protobuf::write_int32_field(&mut body, 1, 13);
        assert_eq!(
            decode_sensor_request(&body),
            Ok(SensorType::DrivingStatusData)
        );
    }

    #[test]
    fn encodes_sensor_response_with_exact_bytes() {
        let message = encode_sensor_response();
        assert_eq!(message.id, SensorMessageId::SensorResponse);
        assert_eq!(message.body, vec![0x08, 0x00]);
    }

    #[test]
    fn encodes_driving_status_batch_with_exact_bytes() {
        let message = encode_driving_status_unrestricted_batch();
        assert_eq!(message.id, SensorMessageId::SensorBatch);
        assert_eq!(message.body, vec![0x6a, 0x02, 0x08, 0x00]);
    }

    #[test]
    fn encodes_night_mode_batch_with_exact_bytes() {
        let message = encode_night_mode_batch(false);
        assert_eq!(message.id, SensorMessageId::SensorBatch);
        assert_eq!(message.body, vec![0x52, 0x02, 0x08, 0x00]);
    }

    #[test]
    fn encodes_night_mode_true_with_exact_bytes() {
        let message = encode_night_mode_batch(true);
        assert_eq!(message.body, vec![0x52, 0x02, 0x08, 0x01]);
    }
}
