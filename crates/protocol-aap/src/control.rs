use std::fmt;

use crate::{
    ServiceDiscoveryError, ServiceDiscoveryLimits, ServiceDiscoveryRequestSummary,
    summarize_service_discovery_request,
};

// Portions derived from AASDK control-channel behaviour and protobuf schemas.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

pub const CONTROL_CHANNEL_ID: u8 = 0;
/// Matches the pinned AASDK source's own `Version.hpp`
/// (`AASDK_MAJOR`/`AASDK_MINOR`), confirmed unchanged even on that fork's
/// current HEAD. A real phone accepted this and separately reported it is
/// running protocol `1.7` (`probe_negotiated_version=1.7`,
/// `HandshakeStateMachine::negotiated_version`); temporarily offering `1.7`
/// here to test whether the version number itself was the cause of a
/// real-phone "phone and car are running incompatible software" rejection
/// made no observable difference, so that deviation was reverted — Android
/// Auto's version negotiation is designed to be backward-compatible, and
/// the evidence now points at a gap in this project's `ServiceDiscoveryResponse`
/// content instead of the offered version number.
pub const AASDK_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 6 };
pub const DEFAULT_MAX_CONTROL_BODY_SIZE: usize = 1024 * 1024;
pub const DEFAULT_MAX_TLS_CHUNK_SIZE: usize = 64 * 1024;
const MESSAGE_ID_SIZE: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlMessageId {
    VersionRequest,
    VersionResponse,
    EncapsulatedTls,
    AuthComplete,
    ServiceDiscoveryRequest,
    ServiceDiscoveryResponse,
    ChannelOpenRequest,
    ChannelOpenResponse,
    Unknown(u16),
}

impl ControlMessageId {
    #[must_use]
    pub const fn wire_value(self) -> u16 {
        match self {
            Self::VersionRequest => 1,
            Self::VersionResponse => 2,
            Self::EncapsulatedTls => 3,
            Self::AuthComplete => 4,
            Self::ServiceDiscoveryRequest => 5,
            Self::ServiceDiscoveryResponse => 6,
            Self::ChannelOpenRequest => 7,
            Self::ChannelOpenResponse => 8,
            Self::Unknown(value) => value,
        }
    }

    const fn from_wire(value: u16) -> Self {
        match value {
            1 => Self::VersionRequest,
            2 => Self::VersionResponse,
            3 => Self::EncapsulatedTls,
            4 => Self::AuthComplete,
            5 => Self::ServiceDiscoveryRequest,
            6 => Self::ServiceDiscoveryResponse,
            7 => Self::ChannelOpenRequest,
            8 => Self::ChannelOpenResponse,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlMessage {
    pub id: ControlMessageId,
    pub body: Vec<u8>,
}

impl ControlMessage {
    pub fn decode(payload: &[u8], maximum_body_size: usize) -> Result<Self, ControlError> {
        if maximum_body_size == 0 {
            return Err(ControlError::InvalidLimit);
        }
        if payload.len() < MESSAGE_ID_SIZE {
            return Err(ControlError::TruncatedMessageId {
                available: payload.len(),
            });
        }
        let body_size = payload.len() - MESSAGE_ID_SIZE;
        if body_size > maximum_body_size {
            return Err(ControlError::BodyTooLarge {
                size: body_size,
                maximum: maximum_body_size,
            });
        }
        Ok(Self {
            id: ControlMessageId::from_wire(u16::from_be_bytes([payload[0], payload[1]])),
            body: payload[MESSAGE_ID_SIZE..].to_vec(),
        })
    }

    pub fn encode(&self, maximum_body_size: usize) -> Result<Vec<u8>, ControlError> {
        if maximum_body_size == 0 {
            return Err(ControlError::InvalidLimit);
        }
        if self.body.len() > maximum_body_size {
            return Err(ControlError::BodyTooLarge {
                size: self.body.len(),
                maximum: maximum_body_size,
            });
        }
        let mut payload = Vec::with_capacity(MESSAGE_ID_SIZE + self.body.len());
        payload.extend_from_slice(&self.id.wire_value().to_be_bytes());
        payload.extend_from_slice(&self.body);
        Ok(payload)
    }

    #[must_use]
    pub fn version_request(version: ProtocolVersion) -> Self {
        let mut body = Vec::with_capacity(4);
        body.extend_from_slice(&version.major.to_be_bytes());
        body.extend_from_slice(&version.minor.to_be_bytes());
        Self {
            id: ControlMessageId::VersionRequest,
            body,
        }
    }

    #[must_use]
    pub fn encapsulated_tls(bytes: &[u8]) -> Self {
        Self {
            id: ControlMessageId::EncapsulatedTls,
            body: bytes.to_vec(),
        }
    }

    #[must_use]
    pub fn auth_success() -> Self {
        Self {
            id: ControlMessageId::AuthComplete,
            body: vec![0x08, 0x00],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeState {
    Idle,
    AwaitingVersionResponse,
    TlsHandshake,
    AwaitingServiceDiscovery,
    ServiceDiscoveryReceived,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandshakeEvent<'a> {
    Start,
    InboundControl(&'a [u8]),
    TlsProgress { outbound: &'a [u8], complete: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HandshakeAction {
    SendControl(ControlMessage),
    StartTlsClient,
    FeedTls(Vec<u8>),
    ServiceDiscoveryRequest(ServiceDiscoveryRequestSummary),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlError {
    InvalidLimit,
    TruncatedMessageId {
        available: usize,
    },
    BodyTooLarge {
        size: usize,
        maximum: usize,
    },
    UnexpectedEvent {
        state: HandshakeState,
    },
    UnexpectedMessage {
        state: HandshakeState,
        id: ControlMessageId,
    },
    InvalidVersionResponseSize(usize),
    VersionRejected(i16),
    TlsChunkTooLarge {
        size: usize,
        maximum: usize,
    },
    EmptyTlsProgress,
    InvalidServiceDiscovery(ServiceDiscoveryError),
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => formatter.write_str("control limits must be non-zero"),
            Self::TruncatedMessageId { available } => write!(
                formatter,
                "control message id requires 2 bytes, {available} available"
            ),
            Self::BodyTooLarge { size, maximum } => {
                write!(formatter, "control body {size} exceeds limit {maximum}")
            }
            Self::UnexpectedEvent { state } => {
                write!(formatter, "unexpected handshake event in state {state:?}")
            }
            Self::UnexpectedMessage { state, id } => {
                write!(
                    formatter,
                    "unexpected control message {id:?} in state {state:?}"
                )
            }
            Self::InvalidVersionResponseSize(size) => {
                write!(
                    formatter,
                    "version response body must be 6 bytes, got {size}"
                )
            }
            Self::VersionRejected(status) => {
                write!(
                    formatter,
                    "phone rejected protocol version with status {status}"
                )
            }
            Self::TlsChunkTooLarge { size, maximum } => {
                write!(formatter, "TLS chunk {size} exceeds limit {maximum}")
            }
            Self::EmptyTlsProgress => {
                formatter.write_str("TLS progress contained no output and was not complete")
            }
            Self::InvalidServiceDiscovery(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ControlError {}

#[derive(Debug)]
pub struct HandshakeStateMachine {
    state: HandshakeState,
    version: ProtocolVersion,
    negotiated_version: Option<ProtocolVersion>,
    maximum_control_body_size: usize,
    maximum_tls_chunk_size: usize,
}

impl HandshakeStateMachine {
    pub fn new(
        version: ProtocolVersion,
        maximum_control_body_size: usize,
        maximum_tls_chunk_size: usize,
    ) -> Result<Self, ControlError> {
        if maximum_control_body_size == 0 || maximum_tls_chunk_size == 0 {
            return Err(ControlError::InvalidLimit);
        }
        Ok(Self {
            state: HandshakeState::Idle,
            version,
            negotiated_version: None,
            maximum_control_body_size,
            maximum_tls_chunk_size,
        })
    }

    #[must_use]
    pub const fn state(&self) -> HandshakeState {
        self.state
    }

    #[must_use]
    pub const fn negotiated_version(&self) -> Option<ProtocolVersion> {
        self.negotiated_version
    }

    pub fn advance(
        &mut self,
        event: HandshakeEvent<'_>,
    ) -> Result<Vec<HandshakeAction>, ControlError> {
        let result = self.advance_inner(event);
        if result.is_err() {
            self.state = HandshakeState::Failed;
        }
        result
    }

    fn advance_inner(
        &mut self,
        event: HandshakeEvent<'_>,
    ) -> Result<Vec<HandshakeAction>, ControlError> {
        match event {
            HandshakeEvent::Start if self.state == HandshakeState::Idle => {
                self.state = HandshakeState::AwaitingVersionResponse;
                Ok(vec![HandshakeAction::SendControl(
                    ControlMessage::version_request(self.version),
                )])
            }
            HandshakeEvent::InboundControl(payload) => self.inbound(payload),
            HandshakeEvent::TlsProgress { outbound, complete }
                if self.state == HandshakeState::TlsHandshake =>
            {
                self.tls_progress(outbound, complete)
            }
            _ => Err(ControlError::UnexpectedEvent { state: self.state }),
        }
    }

    fn inbound(&mut self, payload: &[u8]) -> Result<Vec<HandshakeAction>, ControlError> {
        let message = ControlMessage::decode(payload, self.maximum_control_body_size)?;
        match (self.state, message.id) {
            (HandshakeState::AwaitingVersionResponse, ControlMessageId::VersionResponse) => {
                if message.body.len() != 6 {
                    return Err(ControlError::InvalidVersionResponseSize(message.body.len()));
                }
                let negotiated_version = ProtocolVersion {
                    major: u16::from_be_bytes([message.body[0], message.body[1]]),
                    minor: u16::from_be_bytes([message.body[2], message.body[3]]),
                };
                let status = i16::from_be_bytes([message.body[4], message.body[5]]);
                if status != 0 {
                    return Err(ControlError::VersionRejected(status));
                }
                self.negotiated_version = Some(negotiated_version);
                self.state = HandshakeState::TlsHandshake;
                Ok(vec![HandshakeAction::StartTlsClient])
            }
            (HandshakeState::TlsHandshake, ControlMessageId::EncapsulatedTls) => {
                self.validate_tls_size(message.body.len())?;
                Ok(vec![HandshakeAction::FeedTls(message.body)])
            }
            (
                HandshakeState::AwaitingServiceDiscovery,
                ControlMessageId::ServiceDiscoveryRequest,
            ) => {
                let summary = summarize_service_discovery_request(
                    &message.body,
                    ServiceDiscoveryLimits::default(),
                )?;
                self.state = HandshakeState::ServiceDiscoveryReceived;
                Ok(vec![HandshakeAction::ServiceDiscoveryRequest(summary)])
            }
            (state, id) => Err(ControlError::UnexpectedMessage { state, id }),
        }
    }

    fn tls_progress(
        &mut self,
        outbound: &[u8],
        complete: bool,
    ) -> Result<Vec<HandshakeAction>, ControlError> {
        self.validate_tls_size(outbound.len())?;
        if outbound.is_empty() && !complete {
            return Err(ControlError::EmptyTlsProgress);
        }

        let mut actions = Vec::with_capacity(2);
        if !outbound.is_empty() {
            actions.push(HandshakeAction::SendControl(
                ControlMessage::encapsulated_tls(outbound),
            ));
        }
        if complete {
            actions.push(HandshakeAction::SendControl(ControlMessage::auth_success()));
            self.state = HandshakeState::AwaitingServiceDiscovery;
        }
        Ok(actions)
    }

    const fn validate_tls_size(&self, size: usize) -> Result<(), ControlError> {
        if size > self.maximum_tls_chunk_size {
            Err(ControlError::TlsChunkTooLarge {
                size,
                maximum: self.maximum_tls_chunk_size,
            })
        } else {
            Ok(())
        }
    }
}

impl From<ServiceDiscoveryError> for ControlError {
    fn from(error: ServiceDiscoveryError) -> Self {
        Self::InvalidServiceDiscovery(error)
    }
}

impl Default for HandshakeStateMachine {
    fn default() -> Self {
        Self::new(
            AASDK_PROTOCOL_VERSION,
            DEFAULT_MAX_CONTROL_BODY_SIZE,
            DEFAULT_MAX_TLS_CHUNK_SIZE,
        )
        .expect("default handshake limits are non-zero")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(message: &ControlMessage) -> Vec<u8> {
        message
            .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
            .expect("encode")
    }

    #[test]
    fn control_message_ids_are_big_endian_and_unknown_values_survive() {
        let decoded = ControlMessage::decode(&[0x12, 0x34, 0xaa], 4).expect("decode");
        assert_eq!(decoded.id, ControlMessageId::Unknown(0x1234));
        assert_eq!(decoded.body, [0xaa]);
        assert_eq!(decoded.encode(4).expect("encode"), [0x12, 0x34, 0xaa]);
    }

    #[test]
    fn version_request_matches_aasdk_1_6_wire_shape() {
        assert_eq!(
            payload(&ControlMessage::version_request(AASDK_PROTOCOL_VERSION)),
            [0, 1, 0, 1, 0, 6]
        );
    }

    #[test]
    fn auth_success_is_required_proto2_int32_zero() {
        assert_eq!(payload(&ControlMessage::auth_success()), [0, 4, 0x08, 0]);
    }

    #[test]
    fn reaches_service_discovery_with_fake_tls() {
        let mut machine = HandshakeStateMachine::default();
        assert_eq!(
            machine.advance(HandshakeEvent::Start).expect("start"),
            vec![HandshakeAction::SendControl(
                ControlMessage::version_request(AASDK_PROTOCOL_VERSION)
            )]
        );

        let version_response = [0, 2, 0, 1, 0, 6, 0, 0];
        assert_eq!(
            machine
                .advance(HandshakeEvent::InboundControl(&version_response))
                .expect("version"),
            vec![HandshakeAction::StartTlsClient]
        );
        assert_eq!(machine.state(), HandshakeState::TlsHandshake);
        assert_eq!(machine.negotiated_version(), Some(AASDK_PROTOCOL_VERSION));

        assert_eq!(
            machine
                .advance(HandshakeEvent::TlsProgress {
                    outbound: b"client hello",
                    complete: false,
                })
                .expect("client hello"),
            vec![HandshakeAction::SendControl(
                ControlMessage::encapsulated_tls(b"client hello")
            )]
        );

        let server_tls = payload(&ControlMessage::encapsulated_tls(b"server hello"));
        assert_eq!(
            machine
                .advance(HandshakeEvent::InboundControl(&server_tls))
                .expect("server hello"),
            vec![HandshakeAction::FeedTls(b"server hello".to_vec())]
        );

        assert_eq!(
            machine
                .advance(HandshakeEvent::TlsProgress {
                    outbound: b"client finished",
                    complete: true,
                })
                .expect("complete"),
            vec![
                HandshakeAction::SendControl(ControlMessage::encapsulated_tls(b"client finished")),
                HandshakeAction::SendControl(ControlMessage::auth_success()),
            ]
        );
        assert_eq!(machine.state(), HandshakeState::AwaitingServiceDiscovery);

        let discovery = payload(&ControlMessage {
            id: ControlMessageId::ServiceDiscoveryRequest,
            body: vec![0x0a, 0x00],
        });
        assert_eq!(
            machine
                .advance(HandshakeEvent::InboundControl(&discovery))
                .expect("discovery"),
            vec![HandshakeAction::ServiceDiscoveryRequest(
                ServiceDiscoveryRequestSummary {
                    small_icon_bytes: Some(0),
                    ..ServiceDiscoveryRequestSummary::default()
                }
            )]
        );
        assert_eq!(machine.state(), HandshakeState::ServiceDiscoveryReceived);
    }

    #[test]
    fn version_rejection_fails_closed() {
        let mut machine = HandshakeStateMachine::default();
        machine.advance(HandshakeEvent::Start).expect("start");
        assert_eq!(
            machine.advance(HandshakeEvent::InboundControl(&[
                0, 2, 0, 1, 0, 6, 0xff, 0xff,
            ])),
            Err(ControlError::VersionRejected(-1))
        );
        assert_eq!(machine.state(), HandshakeState::Failed);
        assert_eq!(
            machine.advance(HandshakeEvent::Start),
            Err(ControlError::UnexpectedEvent {
                state: HandshakeState::Failed
            })
        );
    }

    #[test]
    fn rejects_unexpected_messages_and_bad_version_lengths() {
        let mut machine = HandshakeStateMachine::default();
        machine.advance(HandshakeEvent::Start).expect("start");
        assert_eq!(
            machine.advance(HandshakeEvent::InboundControl(&[0, 2, 0, 1])),
            Err(ControlError::InvalidVersionResponseSize(2))
        );

        let mut machine = HandshakeStateMachine::default();
        machine.advance(HandshakeEvent::Start).expect("start");
        assert_eq!(
            machine.advance(HandshakeEvent::InboundControl(&[0, 3])),
            Err(ControlError::UnexpectedMessage {
                state: HandshakeState::AwaitingVersionResponse,
                id: ControlMessageId::EncapsulatedTls
            })
        );
    }

    #[test]
    fn bounds_control_and_tls_inputs() {
        assert!(matches!(
            HandshakeStateMachine::new(AASDK_PROTOCOL_VERSION, 0, 1),
            Err(ControlError::InvalidLimit)
        ));
        assert_eq!(
            ControlMessage::decode(&[0], 10),
            Err(ControlError::TruncatedMessageId { available: 1 })
        );
        assert_eq!(
            ControlMessage::decode(&[0, 3, 1, 2], 1),
            Err(ControlError::BodyTooLarge {
                size: 2,
                maximum: 1
            })
        );

        let mut machine =
            HandshakeStateMachine::new(AASDK_PROTOCOL_VERSION, 10, 2).expect("valid limits");
        machine.advance(HandshakeEvent::Start).expect("start");
        machine
            .advance(HandshakeEvent::InboundControl(&[0, 2, 0, 1, 0, 6, 0, 0]))
            .expect("version");
        assert_eq!(
            machine.advance(HandshakeEvent::TlsProgress {
                outbound: &[1, 2, 3],
                complete: false,
            }),
            Err(ControlError::TlsChunkTooLarge {
                size: 3,
                maximum: 2
            })
        );
    }

    #[test]
    fn empty_incomplete_tls_progress_fails_closed() {
        let mut machine = HandshakeStateMachine::default();
        machine.advance(HandshakeEvent::Start).expect("start");
        machine
            .advance(HandshakeEvent::InboundControl(&[0, 2, 0, 1, 0, 6, 0, 0]))
            .expect("version");

        assert_eq!(
            machine.advance(HandshakeEvent::TlsProgress {
                outbound: &[],
                complete: false,
            }),
            Err(ControlError::EmptyTlsProgress)
        );
        assert_eq!(machine.state(), HandshakeState::Failed);
    }
}
