use std::{collections::BTreeMap, fmt};

use crate::{DecodedFrame, Encryption, FrameType, MessageType};

// Portions derived from AASDK MessageInStream behaviour.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    pub channel_id: u8,
    pub encryption: Encryption,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PartialMessage {
    encryption: Encryption,
    message_type: MessageType,
    declared_size: usize,
    payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssemblyError {
    InvalidMaximumChannels,
    TooManyChannels { maximum: usize },
    ChannelAlreadyStarted(u8),
    ChannelNotStarted(u8),
    FragmentMetadataChanged(u8),
    MissingDeclaredSize(u8),
    DeclaredSizeExceeded { declared: usize, received: usize },
    LastFragmentSizeMismatch { declared: usize, received: usize },
    MessageCompletedBeforeLast { declared: usize },
}

impl fmt::Display for AssemblyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMaximumChannels => {
                formatter.write_str("maximum concurrent channels must be non-zero")
            }
            Self::TooManyChannels { maximum } => {
                write!(
                    formatter,
                    "concurrent partial-message limit {maximum} reached"
                )
            }
            Self::ChannelAlreadyStarted(channel) => {
                write!(formatter, "channel {channel} already has a partial message")
            }
            Self::ChannelNotStarted(channel) => {
                write!(formatter, "channel {channel} has no partial message")
            }
            Self::FragmentMetadataChanged(channel) => write!(
                formatter,
                "channel {channel} changed encryption or message type mid-message"
            ),
            Self::MissingDeclaredSize(channel) => {
                write!(
                    formatter,
                    "first frame on channel {channel} has no total size"
                )
            }
            Self::DeclaredSizeExceeded { declared, received } => write!(
                formatter,
                "received {received} message bytes but only {declared} were declared"
            ),
            Self::LastFragmentSizeMismatch { declared, received } => write!(
                formatter,
                "last fragment ended at {received} bytes instead of declared {declared}"
            ),
            Self::MessageCompletedBeforeLast { declared } => write!(
                formatter,
                "message reached declared size {declared} before a last fragment"
            ),
        }
    }
}

impl std::error::Error for AssemblyError {}

#[derive(Debug)]
pub struct MessageAssembler {
    maximum_concurrent_channels: usize,
    partial: BTreeMap<u8, PartialMessage>,
}

impl MessageAssembler {
    pub fn new(maximum_concurrent_channels: usize) -> Result<Self, AssemblyError> {
        if maximum_concurrent_channels == 0 {
            return Err(AssemblyError::InvalidMaximumChannels);
        }
        Ok(Self {
            maximum_concurrent_channels,
            partial: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn partial_channel_count(&self) -> usize {
        self.partial.len()
    }

    pub fn clear(&mut self) {
        self.partial.clear();
    }

    pub fn push(&mut self, frame: DecodedFrame<'_>) -> Result<Option<Message>, AssemblyError> {
        match frame.header.frame_type {
            FrameType::Bulk => self.bulk(frame).map(Some),
            FrameType::First => {
                self.first(frame)?;
                Ok(None)
            }
            FrameType::Middle => self.fragment(frame, false),
            FrameType::Last => self.fragment(frame, true),
        }
    }

    fn bulk(&self, frame: DecodedFrame<'_>) -> Result<Message, AssemblyError> {
        let channel = frame.header.channel_id;
        if self.partial.contains_key(&channel) {
            return Err(AssemblyError::ChannelAlreadyStarted(channel));
        }
        Ok(Message {
            channel_id: channel,
            encryption: frame.header.encryption,
            message_type: frame.header.message_type,
            payload: frame.payload.to_vec(),
        })
    }

    fn first(&mut self, frame: DecodedFrame<'_>) -> Result<(), AssemblyError> {
        let channel = frame.header.channel_id;
        if self.partial.contains_key(&channel) {
            return Err(AssemblyError::ChannelAlreadyStarted(channel));
        }
        if self.partial.len() >= self.maximum_concurrent_channels {
            return Err(AssemblyError::TooManyChannels {
                maximum: self.maximum_concurrent_channels,
            });
        }

        let declared_size = frame
            .total_message_size
            .ok_or(AssemblyError::MissingDeclaredSize(channel))?;
        if frame.payload.len() == declared_size {
            return Err(AssemblyError::MessageCompletedBeforeLast {
                declared: declared_size,
            });
        }
        self.partial.insert(
            channel,
            PartialMessage {
                encryption: frame.header.encryption,
                message_type: frame.header.message_type,
                declared_size,
                payload: frame.payload.to_vec(),
            },
        );
        Ok(())
    }

    fn fragment(
        &mut self,
        frame: DecodedFrame<'_>,
        is_last: bool,
    ) -> Result<Option<Message>, AssemblyError> {
        let channel = frame.header.channel_id;
        let partial = self
            .partial
            .get_mut(&channel)
            .ok_or(AssemblyError::ChannelNotStarted(channel))?;
        if partial.encryption != frame.header.encryption
            || partial.message_type != frame.header.message_type
        {
            return Err(AssemblyError::FragmentMetadataChanged(channel));
        }

        let received = partial.payload.len() + frame.payload.len();
        if received > partial.declared_size {
            return Err(AssemblyError::DeclaredSizeExceeded {
                declared: partial.declared_size,
                received,
            });
        }
        if is_last && received != partial.declared_size {
            return Err(AssemblyError::LastFragmentSizeMismatch {
                declared: partial.declared_size,
                received,
            });
        }
        if !is_last && received == partial.declared_size {
            return Err(AssemblyError::MessageCompletedBeforeLast {
                declared: partial.declared_size,
            });
        }

        partial.payload.extend_from_slice(frame.payload);
        if !is_last {
            return Ok(None);
        }

        let completed = self
            .partial
            .remove(&channel)
            .expect("partial message remains present until successful completion");
        Ok(Some(Message {
            channel_id: channel,
            encryption: completed.encryption,
            message_type: completed.message_type,
            payload: completed.payload,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameHeader, ProtocolLimits, decode_frame, encode_frame};

    const LIMITS: ProtocolLimits = ProtocolLimits {
        maximum_frame_payload_size: 16,
        maximum_message_size: 64,
    };

    fn decoded(
        channel_id: u8,
        frame_type: FrameType,
        encryption: Encryption,
        message_type: MessageType,
        total: Option<usize>,
        payload: &[u8],
    ) -> DecodedFrame<'static> {
        let encoded = encode_frame(
            FrameHeader {
                channel_id,
                frame_type,
                encryption,
                message_type,
            },
            total,
            payload,
            LIMITS,
        )
        .expect("encode test frame")
        .leak();
        decode_frame(encoded, LIMITS).expect("decode test frame")
    }

    #[test]
    fn bulk_completes_immediately() {
        let mut assembler = MessageAssembler::new(2).expect("assembler");
        let message = assembler
            .push(decoded(
                2,
                FrameType::Bulk,
                Encryption::Plain,
                MessageType::Control,
                None,
                b"ready",
            ))
            .expect("assemble")
            .expect("complete");
        assert_eq!(message.payload, b"ready");
        assert_eq!(assembler.partial_channel_count(), 0);
    }

    #[test]
    fn reassembles_interleaved_channels() {
        let mut assembler = MessageAssembler::new(2).expect("assembler");
        assert!(
            assembler
                .push(decoded(
                    1,
                    FrameType::First,
                    Encryption::Encrypted,
                    MessageType::Specific,
                    Some(4),
                    b"a",
                ))
                .expect("first")
                .is_none()
        );
        assert!(
            assembler
                .push(decoded(
                    2,
                    FrameType::First,
                    Encryption::Plain,
                    MessageType::Control,
                    Some(3),
                    b"x",
                ))
                .expect("first")
                .is_none()
        );
        assert!(
            assembler
                .push(decoded(
                    1,
                    FrameType::Middle,
                    Encryption::Encrypted,
                    MessageType::Specific,
                    None,
                    b"bc",
                ))
                .expect("middle")
                .is_none()
        );
        let first = assembler
            .push(decoded(
                1,
                FrameType::Last,
                Encryption::Encrypted,
                MessageType::Specific,
                None,
                b"d",
            ))
            .expect("last")
            .expect("complete");
        assert_eq!(first.payload, b"abcd");
        assert_eq!(assembler.partial_channel_count(), 1);
    }

    #[test]
    fn rejects_unstarted_and_restarted_channels() {
        let mut assembler = MessageAssembler::new(1).expect("assembler");
        assert_eq!(
            assembler.push(decoded(
                4,
                FrameType::Last,
                Encryption::Plain,
                MessageType::Specific,
                None,
                b"x",
            )),
            Err(AssemblyError::ChannelNotStarted(4))
        );
        assembler
            .push(decoded(
                4,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(2),
                b"x",
            ))
            .expect("first");
        assert_eq!(
            assembler.push(decoded(
                4,
                FrameType::Bulk,
                Encryption::Plain,
                MessageType::Specific,
                None,
                b"y",
            )),
            Err(AssemblyError::ChannelAlreadyStarted(4))
        );
    }

    #[test]
    fn bounds_concurrent_channels() {
        let mut assembler = MessageAssembler::new(1).expect("assembler");
        assembler
            .push(decoded(
                1,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(2),
                b"a",
            ))
            .expect("first");
        assert_eq!(
            assembler.push(decoded(
                2,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(2),
                b"b",
            )),
            Err(AssemblyError::TooManyChannels { maximum: 1 })
        );
    }

    #[test]
    fn rejects_metadata_changes() {
        let mut assembler = MessageAssembler::new(1).expect("assembler");
        assembler
            .push(decoded(
                1,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(2),
                b"a",
            ))
            .expect("first");
        assert_eq!(
            assembler.push(decoded(
                1,
                FrameType::Last,
                Encryption::Encrypted,
                MessageType::Specific,
                None,
                b"b",
            )),
            Err(AssemblyError::FragmentMetadataChanged(1))
        );
    }

    #[test]
    fn rejects_short_and_oversized_completion() {
        let mut assembler = MessageAssembler::new(1).expect("assembler");
        assembler
            .push(decoded(
                1,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(3),
                b"a",
            ))
            .expect("first");
        assert_eq!(
            assembler.push(decoded(
                1,
                FrameType::Last,
                Encryption::Plain,
                MessageType::Specific,
                None,
                b"b",
            )),
            Err(AssemblyError::LastFragmentSizeMismatch {
                declared: 3,
                received: 2
            })
        );

        assembler.clear();
        assembler
            .push(decoded(
                1,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(2),
                b"a",
            ))
            .expect("first");
        assert_eq!(
            assembler.push(decoded(
                1,
                FrameType::Last,
                Encryption::Plain,
                MessageType::Specific,
                None,
                b"bc",
            )),
            Err(AssemblyError::DeclaredSizeExceeded {
                declared: 2,
                received: 3
            })
        );
    }

    #[test]
    fn clear_drops_all_partial_state() {
        let mut assembler = MessageAssembler::new(2).expect("assembler");
        assembler
            .push(decoded(
                1,
                FrameType::First,
                Encryption::Plain,
                MessageType::Specific,
                Some(2),
                b"a",
            ))
            .expect("first");
        assembler.clear();
        assert_eq!(assembler.partial_channel_count(), 0);
    }

    #[test]
    fn rejects_zero_channel_capacity() {
        assert_eq!(
            MessageAssembler::new(0).expect_err("zero capacity must fail"),
            AssemblyError::InvalidMaximumChannels
        );
    }
}
