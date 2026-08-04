//! Bounded Android Auto Protocol frame encoding and decoding.
//!
//! Framing behaviour is derived from AASDK at revision
//! `9bf6adf933665dee26532201719fac14a047ccf1`, licensed GPL-3.0-or-later.
//! See `docs/protocol/aasdk-adoption.md` for exact provenance.

// Portions derived from AASDK framing behaviour.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;

mod assembly;
mod control;
mod tls;

pub use assembly::{AssemblyError, Message, MessageAssembler};
pub use control::{
    AASDK_PROTOCOL_VERSION, CONTROL_CHANNEL_ID, ControlError, ControlMessage, ControlMessageId,
    DEFAULT_MAX_CONTROL_BODY_SIZE, DEFAULT_MAX_TLS_CHUNK_SIZE, HandshakeAction, HandshakeEvent,
    HandshakeState, HandshakeStateMachine, ProtocolVersion,
};
pub use tls::{TlsClient, TlsProgress};

pub const AASDK_MAX_FRAME_PAYLOAD_SIZE: usize = 0x4000;
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;
const FRAME_HEADER_SIZE: usize = 2;
const SHORT_SIZE_HEADER: usize = 2;
const EXTENDED_SIZE_HEADER: usize = 6;
const KNOWN_FLAG_MASK: u8 = 0x0f;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameType {
    Middle,
    First,
    Last,
    Bulk,
}

impl FrameType {
    const fn bits(self) -> u8 {
        match self {
            Self::Middle => 0,
            Self::First => 1,
            Self::Last => 2,
            Self::Bulk => 3,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            0 => Self::Middle,
            1 => Self::First,
            2 => Self::Last,
            _ => Self::Bulk,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encryption {
    Plain,
    Encrypted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageType {
    Specific,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub channel_id: u8,
    pub frame_type: FrameType,
    pub encryption: Encryption,
    pub message_type: MessageType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolLimits {
    pub maximum_frame_payload_size: usize,
    pub maximum_message_size: usize,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            maximum_frame_payload_size: AASDK_MAX_FRAME_PAYLOAD_SIZE,
            maximum_message_size: DEFAULT_MAX_MESSAGE_SIZE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodedFrame<'a> {
    pub header: FrameHeader,
    pub total_message_size: Option<usize>,
    pub payload: &'a [u8],
    pub consumed: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    Incomplete { required: usize, available: usize },
    ReservedFlags(u8),
    InvalidLimits,
    FrameTooLarge { size: usize, maximum: usize },
    MessageTooLarge { size: usize, maximum: usize },
    TotalSmallerThanFrame { total: usize, frame: usize },
    MissingTotalSize,
    UnexpectedTotalSize,
    LengthNotRepresentable(usize),
}

impl fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete {
                required,
                available,
            } => write!(
                formatter,
                "incomplete frame: {required} bytes required, {available} available"
            ),
            Self::ReservedFlags(flags) => {
                write!(formatter, "frame uses reserved flag bits 0x{flags:02x}")
            }
            Self::InvalidLimits => formatter.write_str("protocol limits must be non-zero"),
            Self::FrameTooLarge { size, maximum } => {
                write!(formatter, "frame payload {size} exceeds limit {maximum}")
            }
            Self::MessageTooLarge { size, maximum } => {
                write!(formatter, "message {size} exceeds limit {maximum}")
            }
            Self::TotalSmallerThanFrame { total, frame } => write!(
                formatter,
                "total message size {total} is smaller than first frame {frame}"
            ),
            Self::MissingTotalSize => {
                formatter.write_str("a first frame requires the total message size")
            }
            Self::UnexpectedTotalSize => {
                formatter.write_str("only a first frame can carry the total message size")
            }
            Self::LengthNotRepresentable(size) => {
                write!(formatter, "length {size} cannot be represented on the wire")
            }
        }
    }
}

impl std::error::Error for FrameError {}

pub fn decode_frame(input: &[u8], limits: ProtocolLimits) -> Result<DecodedFrame<'_>, FrameError> {
    validate_limits(limits)?;
    require(input, FRAME_HEADER_SIZE)?;

    let flags = input[1];
    let reserved = flags & !KNOWN_FLAG_MASK;
    if reserved != 0 {
        return Err(FrameError::ReservedFlags(reserved));
    }

    let frame_type = FrameType::from_bits(flags & 0x03);
    let size_header_length = if frame_type == FrameType::First {
        EXTENDED_SIZE_HEADER
    } else {
        SHORT_SIZE_HEADER
    };
    let payload_offset = FRAME_HEADER_SIZE + size_header_length;
    require(input, payload_offset)?;

    let frame_size = usize::from(u16::from_be_bytes([input[2], input[3]]));
    if frame_size > limits.maximum_frame_payload_size {
        return Err(FrameError::FrameTooLarge {
            size: frame_size,
            maximum: limits.maximum_frame_payload_size,
        });
    }

    let total_message_size = if frame_type == FrameType::First {
        let total = u32::from_be_bytes([input[4], input[5], input[6], input[7]]) as usize;
        if total > limits.maximum_message_size {
            return Err(FrameError::MessageTooLarge {
                size: total,
                maximum: limits.maximum_message_size,
            });
        }
        if total < frame_size {
            return Err(FrameError::TotalSmallerThanFrame {
                total,
                frame: frame_size,
            });
        }
        Some(total)
    } else {
        None
    };

    let consumed = payload_offset
        .checked_add(frame_size)
        .ok_or(FrameError::LengthNotRepresentable(frame_size))?;
    require(input, consumed)?;

    Ok(DecodedFrame {
        header: FrameHeader {
            channel_id: input[0],
            frame_type,
            encryption: if flags & 0x08 == 0 {
                Encryption::Plain
            } else {
                Encryption::Encrypted
            },
            message_type: if flags & 0x04 == 0 {
                MessageType::Specific
            } else {
                MessageType::Control
            },
        },
        total_message_size,
        payload: &input[payload_offset..consumed],
        consumed,
    })
}

pub fn encode_frame(
    header: FrameHeader,
    total_message_size: Option<usize>,
    payload: &[u8],
    limits: ProtocolLimits,
) -> Result<Vec<u8>, FrameError> {
    validate_limits(limits)?;
    if payload.len() > limits.maximum_frame_payload_size {
        return Err(FrameError::FrameTooLarge {
            size: payload.len(),
            maximum: limits.maximum_frame_payload_size,
        });
    }
    let frame_size = u16::try_from(payload.len())
        .map_err(|_| FrameError::LengthNotRepresentable(payload.len()))?;

    let size_header_length = if header.frame_type == FrameType::First {
        EXTENDED_SIZE_HEADER
    } else {
        SHORT_SIZE_HEADER
    };
    let mut output = Vec::with_capacity(FRAME_HEADER_SIZE + size_header_length + payload.len());
    output.push(header.channel_id);
    output.push(
        header.frame_type.bits()
            | match header.encryption {
                Encryption::Plain => 0,
                Encryption::Encrypted => 0x08,
            }
            | match header.message_type {
                MessageType::Specific => 0,
                MessageType::Control => 0x04,
            },
    );
    output.extend_from_slice(&frame_size.to_be_bytes());

    if header.frame_type == FrameType::First {
        let total = total_message_size.ok_or(FrameError::MissingTotalSize)?;
        if total > limits.maximum_message_size {
            return Err(FrameError::MessageTooLarge {
                size: total,
                maximum: limits.maximum_message_size,
            });
        }
        if total < payload.len() {
            return Err(FrameError::TotalSmallerThanFrame {
                total,
                frame: payload.len(),
            });
        }
        let total = u32::try_from(total).map_err(|_| FrameError::LengthNotRepresentable(total))?;
        output.extend_from_slice(&total.to_be_bytes());
    } else if total_message_size.is_some() {
        return Err(FrameError::UnexpectedTotalSize);
    }
    output.extend_from_slice(payload);
    Ok(output)
}

const fn validate_limits(limits: ProtocolLimits) -> Result<(), FrameError> {
    if limits.maximum_frame_payload_size == 0 || limits.maximum_message_size == 0 {
        Err(FrameError::InvalidLimits)
    } else {
        Ok(())
    }
}

fn require(input: &[u8], required: usize) -> Result<(), FrameError> {
    if input.len() < required {
        Err(FrameError::Incomplete {
            required,
            available: input.len(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: ProtocolLimits = ProtocolLimits {
        maximum_frame_payload_size: AASDK_MAX_FRAME_PAYLOAD_SIZE,
        maximum_message_size: 1_000_000,
    };

    #[test]
    fn bulk_frame_round_trips() {
        let header = FrameHeader {
            channel_id: 3,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Plain,
            message_type: MessageType::Specific,
        };
        let encoded = encode_frame(header, None, b"hello", LIMITS).expect("encode");
        assert_eq!(encoded, [3, 3, 0, 5, b'h', b'e', b'l', b'l', b'o']);

        let decoded = decode_frame(&encoded, LIMITS).expect("decode");
        assert_eq!(decoded.header, header);
        assert_eq!(decoded.total_message_size, None);
        assert_eq!(decoded.payload, b"hello");
        assert_eq!(decoded.consumed, encoded.len());
    }

    #[test]
    fn first_encrypted_control_frame_uses_extended_size() {
        let header = FrameHeader {
            channel_id: 0,
            frame_type: FrameType::First,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Control,
        };
        let encoded = encode_frame(header, Some(70_000), &[0xaa, 0xbb], LIMITS).expect("encode");
        assert_eq!(encoded, [0, 0x0d, 0, 2, 0, 1, 0x11, 0x70, 0xaa, 0xbb]);

        let decoded = decode_frame(&encoded, LIMITS).expect("decode");
        assert_eq!(decoded.header, header);
        assert_eq!(decoded.total_message_size, Some(70_000));
        assert_eq!(decoded.payload, [0xaa, 0xbb]);
    }

    #[test]
    fn reports_each_incomplete_stage_without_reading_past_input() {
        assert_eq!(
            decode_frame(&[1], LIMITS),
            Err(FrameError::Incomplete {
                required: 2,
                available: 1
            })
        );
        assert_eq!(
            decode_frame(&[1, 3, 0], LIMITS),
            Err(FrameError::Incomplete {
                required: 4,
                available: 3
            })
        );
        assert_eq!(
            decode_frame(&[1, 3, 0, 2, 0], LIMITS),
            Err(FrameError::Incomplete {
                required: 6,
                available: 5
            })
        );
    }

    #[test]
    fn rejects_reserved_flags() {
        assert_eq!(
            decode_frame(&[1, 0x83, 0, 0], LIMITS),
            Err(FrameError::ReservedFlags(0x80))
        );
    }

    #[test]
    fn rejects_frame_and_message_limits() {
        let small_frame_limits = ProtocolLimits {
            maximum_frame_payload_size: 1,
            maximum_message_size: 100,
        };
        assert_eq!(
            decode_frame(&[1, 3, 0, 2, 0, 0], small_frame_limits),
            Err(FrameError::FrameTooLarge {
                size: 2,
                maximum: 1
            })
        );

        let small_message_limits = ProtocolLimits {
            maximum_frame_payload_size: 10,
            maximum_message_size: 3,
        };
        assert_eq!(
            decode_frame(&[1, 1, 0, 2, 0, 0, 0, 4, 0, 0], small_message_limits),
            Err(FrameError::MessageTooLarge {
                size: 4,
                maximum: 3
            })
        );
    }

    #[test]
    fn rejects_impossible_first_frame_total() {
        assert_eq!(
            decode_frame(&[1, 1, 0, 2, 0, 0, 0, 1, 0, 0], LIMITS),
            Err(FrameError::TotalSmallerThanFrame { total: 1, frame: 2 })
        );
    }

    #[test]
    fn encoder_rejects_total_size_on_the_wrong_frame_type() {
        let first = FrameHeader {
            channel_id: 1,
            frame_type: FrameType::First,
            encryption: Encryption::Plain,
            message_type: MessageType::Specific,
        };
        assert_eq!(
            encode_frame(first, None, &[1], LIMITS),
            Err(FrameError::MissingTotalSize)
        );

        let bulk = FrameHeader {
            frame_type: FrameType::Bulk,
            ..first
        };
        assert_eq!(
            encode_frame(bulk, Some(1), &[1], LIMITS),
            Err(FrameError::UnexpectedTotalSize)
        );
    }
}
