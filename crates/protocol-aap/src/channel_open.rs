use std::fmt;

use crate::control::{
    ControlError, ControlMessage, ControlMessageId, DEFAULT_MAX_CONTROL_BODY_SIZE,
};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's ChannelOpenRequest/ChannelOpenResponse
// protobuf schema (protobuf/aap_protobuf/service/control/message/) and the
// channel-open dispatch behaviour in VideoMediaSinkService.cpp/
// InputSourceService.cpp, at the pinned project revision
// (9bf6adf933665dee26532201719fac14a047ccf1). Both channel kinds send
// ChannelOpenResponse the same way (MessageType::CONTROL, on the channel's
// own channel_id, EncryptionType::ENCRYPTED), confirmed directly against
// that source — see the channel-setup design record.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.shared.MessageStatus.STATUS_SUCCESS` — the only status
/// this encoder ever sends. A service-id mismatch is treated as a hard
/// protocol failure by the caller instead of a negative-status response;
/// see [`ChannelOpenError::ServiceIdMismatch`].
const MESSAGE_STATUS_SUCCESS: i32 = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelOpenState {
    AwaitingOpenRequest,
    Open,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelOpenEvent<'a> {
    /// Payload of a `MessageType::Control`-flagged frame on this channel.
    InboundControl(&'a [u8]),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelOpenAction {
    SendControl(ControlMessage),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelOpenError {
    UnexpectedEvent {
        state: ChannelOpenState,
    },
    UnexpectedMessage {
        state: ChannelOpenState,
        id: ControlMessageId,
    },
    ServiceIdMismatch {
        channel_id: u8,
        service_id: i32,
    },
    MissingServiceId,
    UnexpectedWireType {
        field: u32,
        wire_type: u8,
    },
    Envelope(ControlError),
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for ChannelOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEvent { state } => {
                write!(
                    formatter,
                    "unexpected channel-open event in state {state:?}"
                )
            }
            Self::UnexpectedMessage { state, id } => write!(
                formatter,
                "unexpected control message {id:?} in state {state:?}"
            ),
            Self::ServiceIdMismatch {
                channel_id,
                service_id,
            } => write!(
                formatter,
                "channel {channel_id} received ChannelOpenRequest for service {service_id}"
            ),
            Self::MissingServiceId => {
                formatter.write_str("ChannelOpenRequest is missing its required service_id field")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "channel-open field {field} has unexpected wire type {wire_type}"
            ),
            Self::Envelope(error) => write!(formatter, "{error}"),
            Self::Truncated => formatter.write_str("truncated channel-open protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid channel-open protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("channel-open protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("channel-open field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for ChannelOpenError {}

impl ProtobufDecodeError for ChannelOpenError {
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

/// Generic channel-open handshake, reused as-is for both the video and
/// input/touch channels: the phone sends `ChannelOpenRequest` on the
/// channel's own `channel_id`, and this replies `ChannelOpenResponse`
/// (`STATUS_SUCCESS` only — no negative-status robustness handling yet).
#[derive(Debug)]
pub struct ChannelOpenStateMachine {
    channel_id: u8,
    state: ChannelOpenState,
}

impl ChannelOpenStateMachine {
    #[must_use]
    pub const fn new(channel_id: u8) -> Self {
        Self {
            channel_id,
            state: ChannelOpenState::AwaitingOpenRequest,
        }
    }

    #[must_use]
    pub const fn channel_id(&self) -> u8 {
        self.channel_id
    }

    #[must_use]
    pub const fn state(&self) -> ChannelOpenState {
        self.state
    }

    pub fn advance(
        &mut self,
        event: ChannelOpenEvent<'_>,
    ) -> Result<Vec<ChannelOpenAction>, ChannelOpenError> {
        let result = self.advance_inner(event);
        if result.is_err() {
            self.state = ChannelOpenState::Failed;
        }
        result
    }

    fn advance_inner(
        &mut self,
        event: ChannelOpenEvent<'_>,
    ) -> Result<Vec<ChannelOpenAction>, ChannelOpenError> {
        match event {
            ChannelOpenEvent::InboundControl(payload)
                if self.state == ChannelOpenState::AwaitingOpenRequest =>
            {
                self.inbound(payload)
            }
            ChannelOpenEvent::InboundControl(_) => {
                Err(ChannelOpenError::UnexpectedEvent { state: self.state })
            }
        }
    }

    fn inbound(&mut self, payload: &[u8]) -> Result<Vec<ChannelOpenAction>, ChannelOpenError> {
        let message = ControlMessage::decode(payload, DEFAULT_MAX_CONTROL_BODY_SIZE)
            .map_err(ChannelOpenError::Envelope)?;
        if message.id != ControlMessageId::ChannelOpenRequest {
            return Err(ChannelOpenError::UnexpectedMessage {
                state: self.state,
                id: message.id,
            });
        }
        let service_id = decode_channel_open_request(&message.body)?;
        if service_id != i32::from(self.channel_id) {
            return Err(ChannelOpenError::ServiceIdMismatch {
                channel_id: self.channel_id,
                service_id,
            });
        }
        self.state = ChannelOpenState::Open;
        let mut body = Vec::new();
        // ChannelOpenResponse.status (field 1, required MessageStatus enum).
        protobuf::write_int32_field(&mut body, 1, MESSAGE_STATUS_SUCCESS);
        Ok(vec![ChannelOpenAction::SendControl(ControlMessage {
            id: ControlMessageId::ChannelOpenResponse,
            body,
        })])
    }
}

/// Decodes `ChannelOpenRequest` and returns its `service_id`. `priority`
/// (field 1, required `sint32`, zigzag-encoded — a different varint
/// encoding than the plain `int32` `service_id`) is decoded and discarded:
/// proto2 requires it be present and well-formed, but this increment
/// implements no priority-based arbitration.
fn decode_channel_open_request(body: &[u8]) -> Result<i32, ChannelOpenError> {
    let mut cursor = 0;
    let mut service_id = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<ChannelOpenError>(body, &mut cursor)?;
        match field {
            1 | 2 if wire_type != 0 => {
                return Err(ChannelOpenError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                protobuf::read_zigzag_varint::<ChannelOpenError>(body, &mut cursor)?;
            }
            2 => {
                let raw = protobuf::read_varint::<ChannelOpenError>(body, &mut cursor)?;
                // Canonical proto2 plain-int32 decode: treat the wire
                // varint as i64 (sign-extended by the encoder for negative
                // values, exactly mirroring write_int32_field), then
                // truncate to i32 — a deliberate truncation, not a bug.
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                service_id = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<ChannelOpenError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    service_id.ok_or(ChannelOpenError::MissingServiceId)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_open_request(priority: i64, service_id: i32) -> Vec<u8> {
        let mut body = Vec::new();
        // Zigzag-encode by hand for the test fixture (no production writer
        // exists yet — nothing calls write_zigzag_varint this pass).
        #[allow(clippy::cast_sign_loss)]
        let zigzagged = ((priority << 1) ^ (priority >> 63)) as u64;
        // Field 1 (priority, sint32): tag then zigzag-encoded varint.
        body.extend_from_slice(&[0x08]);
        write_test_varint(&mut body, zigzagged);
        // Field 2 (service_id, int32): tag then plain sign-extended varint.
        protobuf::write_int32_field(&mut body, 2, service_id);
        ControlMessage {
            id: ControlMessageId::ChannelOpenRequest,
            body,
        }
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode envelope")
    }

    fn write_test_varint(out: &mut Vec<u8>, value: u64) {
        let mut remaining = value;
        loop {
            let byte = (remaining & 0x7f) as u8;
            remaining >>= 7;
            if remaining == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    #[test]
    fn opens_on_matching_service_id_with_exact_response_bytes() {
        let mut machine = ChannelOpenStateMachine::new(1);
        let actions = machine
            .advance(ChannelOpenEvent::InboundControl(&channel_open_request(
                0, 1,
            )))
            .expect("advance");
        assert_eq!(machine.state(), ChannelOpenState::Open);
        assert_eq!(machine.channel_id(), 1);
        assert_eq!(actions.len(), 1);
        let ChannelOpenAction::SendControl(message) = &actions[0];
        assert_eq!(message.id, ControlMessageId::ChannelOpenResponse);
        assert_eq!(message.body, vec![0x08, 0x00]);
    }

    #[test]
    fn discards_negative_priority_without_error() {
        let mut machine = ChannelOpenStateMachine::new(3);
        let actions = machine
            .advance(ChannelOpenEvent::InboundControl(&channel_open_request(
                -5, 3,
            )))
            .expect("advance");
        assert_eq!(machine.state(), ChannelOpenState::Open);
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn rejects_service_id_mismatch_and_fails_closed() {
        let mut machine = ChannelOpenStateMachine::new(1);
        assert_eq!(
            machine.advance(ChannelOpenEvent::InboundControl(&channel_open_request(
                0, 2,
            ))),
            Err(ChannelOpenError::ServiceIdMismatch {
                channel_id: 1,
                service_id: 2,
            })
        );
        assert_eq!(machine.state(), ChannelOpenState::Failed);
        assert_eq!(
            machine.advance(ChannelOpenEvent::InboundControl(&channel_open_request(
                0, 1,
            ))),
            Err(ChannelOpenError::UnexpectedEvent {
                state: ChannelOpenState::Failed
            })
        );
    }

    #[test]
    fn rejects_unexpected_message_in_open_state() {
        let mut machine = ChannelOpenStateMachine::new(1);
        machine
            .advance(ChannelOpenEvent::InboundControl(&channel_open_request(
                0, 1,
            )))
            .expect("first open succeeds");
        assert_eq!(
            machine.advance(ChannelOpenEvent::InboundControl(&channel_open_request(
                0, 1,
            ))),
            Err(ChannelOpenError::UnexpectedEvent {
                state: ChannelOpenState::Open
            })
        );
    }

    #[test]
    fn rejects_missing_service_id() {
        let payload = ControlMessage {
            id: ControlMessageId::ChannelOpenRequest,
            body: Vec::new(),
        }
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode");
        let mut machine = ChannelOpenStateMachine::new(1);
        assert_eq!(
            machine.advance(ChannelOpenEvent::InboundControl(&payload)),
            Err(ChannelOpenError::MissingServiceId)
        );
    }

    #[test]
    fn rejects_wrong_control_message_id() {
        let payload = ControlMessage {
            id: ControlMessageId::ServiceDiscoveryResponse,
            body: Vec::new(),
        }
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode");
        let mut machine = ChannelOpenStateMachine::new(1);
        assert_eq!(
            machine.advance(ChannelOpenEvent::InboundControl(&payload)),
            Err(ChannelOpenError::UnexpectedMessage {
                state: ChannelOpenState::AwaitingOpenRequest,
                id: ControlMessageId::ServiceDiscoveryResponse,
            })
        );
    }
}
