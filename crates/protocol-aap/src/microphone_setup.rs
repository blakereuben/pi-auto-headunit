use std::fmt;

use crate::media_message::{
    DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, MediaMessage, MediaMessageError, MediaMessageId,
    encode_media_data,
};
use crate::protobuf::{self, ProtobufDecodeError};

// Portions derived from AASDK's Setup/Config protobuf schema
// (protobuf/aap_protobuf/service/media/shared/message/{Setup,Config,Start,
// Stop}.proto, shared with every other media channel in this crate) and
// the microphone-specific `MicrophoneRequest`/`MicrophoneResponse`/`Ack`
// schemas (protobuf/aap_protobuf/service/media/source/message/
// {MicrophoneRequest,MicrophoneResponse,Ack}.proto), at the pinned project
// revision (9bf6adf933665dee26532201719fac14a047ccf1):
//   MicrophoneRequest { required bool open = 1; optional bool anc_enabled = 2;
//     optional bool ec_enabled = 3; optional int32 max_unacked = 4; }
//   MicrophoneResponse { required int32 status = 1; optional int32 session_id = 2; }
//   Ack (source variant, distinct schema from the sink-side Ack this crate
//     already encodes in `media_message::encode_media_ack`) {
//     required int32 session_id = 1; optional uint32 ack = 2;
//     repeated uint64 receive_timestamp_ns = 3; }
// `anc_enabled`/`ec_enabled` are decoded (skipped as unknown fields) but
// not acted on — this project has no active-noise-cancellation or
// echo-cancellation hardware path to honour them.
//
// AASDK's own C++ for this exact channel (`MediaSourceService.cpp`) has
// two confirmed defects: `sendChannelSetupResponse` tags its `Config`
// reply with the `Setup` message id instead of `Config`, and
// `sendMicrophoneOpenResponse` tags its `MicrophoneResponse` with the
// `MicrophoneRequest` id instead of the dedicated `MicrophoneResponse` id
// (32774, otherwise unreferenced anywhere in that repository). Neither
// defect is reproduced here. `Start`/`Stop` handling for this channel is
// missing from AASDK's C++ entirely (`IMediaSourceService` has no
// send-Start method at all). For all of the above, `f-io/LIVI` (a
// separate, independently implemented, real working GPL-3.0-or-later
// Android Auto client, formally adopted per
// `docs/protocol/livi-adoption.md`) is the behaviourally-correct
// reference for this specific channel, not AASDK —
// `src/main/services/projection/driver/aa/stack/channels/MicChannel.ts`
// is the source for: the correct message ids on `Config`/
// `MicrophoneResponse`; that `Start`/`Data` are head-unit-initiated
// (reversed from every sink channel elsewhere in this crate); that the
// head unit originates `session_id` rather than echoing the phone's; and
// the `Ack`-driven flow-control shape. No LIVI code is reproduced, only
// the wire shape and behavioural facts, cross-checked against the pinned
// AASDK schema above.
// Copyright (C) 2018 f1x.studio (Michal Szwaj)
// Copyright (C) 2024 CubeOne (Simon Dean)
// Copyright (C) 2024-2026 Open Android Auto contributors (LIVI)
// SPDX-License-Identifier: GPL-3.0-or-later

/// `aap_protobuf.service.media.shared.message.MediaCodecType.MEDIA_CODEC_AUDIO_PCM`
/// — the only codec this project's `ServiceDiscoveryResponse` ever
/// advertises for the `Microphone` service (a single, uncompressed
/// `AudioConfiguration`), matching `audio_setup.rs`'s identical constant.
const MEDIA_CODEC_AUDIO_PCM: i32 = 1;

/// `Config`'s nested `Status` enum, `STATUS_READY = 2` — same constant as
/// `audio_setup.rs`'s `CONFIG_STATUS_READY`.
const CONFIG_STATUS_READY: i32 = 2;

/// A deliberately conservative placeholder for `Config.max_unacked`,
/// matching `audio_setup.rs`'s identical `DEFAULT_AUDIO_MAX_UNACKED`. This
/// is a *different* concept from `DEFAULT_MICROPHONE_MAX_UNACKED` below:
/// `Config.max_unacked` shapes the phone's own generic send capacity for
/// this channel's `Setup`/`Config` handshake, sent before the microphone-
/// specific `MicrophoneRequest.max_unacked` (which bounds the reversed,
/// head-unit-to-phone `Data` flow) is even known.
const CONFIG_MAX_UNACKED_PLACEHOLDER: i32 = 1;

/// The only `AudioConfiguration` entry this project's `ServiceDiscoveryResponse`
/// ever advertises for `Microphone`, so it's the only configuration index
/// `Start` (sent by this head unit) may ever reference.
const ADVERTISED_CONFIGURATION_INDEX: u32 = 0;

/// `MicrophoneResponse.status` has no named enum anywhere in either the
/// pinned AASDK source or LIVI — `0` ("OK") is the only value either
/// reference ever produces or consumes. Genuinely unconfirmed whether a
/// real phone expects other values; flagged in
/// `docs/protocol/error-2-investigation.md`-style honesty as an open
/// question pending real-hardware evidence.
const MICROPHONE_RESPONSE_STATUS_OK: i32 = 0;

/// Used only when `MicrophoneRequest.max_unacked` is absent or non-positive.
/// Deliberately smaller than LIVI's own 64-frame backlog design (this
/// project drops the newest buffer on exhaustion rather than queuing —
/// see `MicrophoneSendOutcome::CreditExhausted`'s doc comment) and
/// deliberately larger than `audio_setup.rs`'s `DEFAULT_AUDIO_MAX_UNACKED
/// = 1`, which would force a synchronous send-then-wait-for-ack round
/// trip per PCM chunk and likely stall real-time audio. Provisional —
/// the first constant to revisit if a real-hardware trial shows choppy or
/// laggy assistant behaviour.
const DEFAULT_MICROPHONE_MAX_UNACKED: u32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicrophoneSetupState {
    /// Accepts either `Setup` (replied to with `Config`, an optional
    /// acknowledgment that doesn't gate anything further — see
    /// `handle_awaiting_request`'s doc comment) or `MicrophoneRequest`
    /// directly.
    AwaitingMicrophoneRequest,
    Streaming,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicrophoneSetupEvent<'a> {
    /// Payload of a `MessageType::Specific`-flagged frame on the
    /// `Microphone` channel.
    InboundMedia(&'a [u8]),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MicrophoneSetupAction {
    SendMedia(MediaMessage),
    /// Entered (or re-entered, on a repeated `MicrophoneRequest{open:
    /// true}` while already `Streaming` — see `apply_microphone_request`'s
    /// doc comment) the `Streaming` state. Carries the fresh `session_id`
    /// this head unit just issued. Callers should (re)start real capture.
    Streaming {
        session_id: i32,
    },
    /// Left `Streaming` — via `Stop` or `MicrophoneRequest{open: false}`.
    /// Callers should stop real capture.
    StreamingStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MicrophoneSetupError {
    UnexpectedEvent {
        state: MicrophoneSetupState,
    },
    UnexpectedMediaMessage {
        state: MicrophoneSetupState,
        id: MediaMessageId,
    },
    UnsupportedCodec {
        requested: i32,
    },
    MissingCodecType,
    MissingOpenFlag,
    MissingSessionId,
    UnexpectedWireType {
        field: u32,
        wire_type: u8,
    },
    Envelope(MediaMessageError),
    Truncated,
    InvalidVarint,
    InvalidFieldNumber,
    LengthNotRepresentable,
    UnsupportedWireType(u8),
}

impl fmt::Display for MicrophoneSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEvent { state } => write!(
                formatter,
                "unexpected microphone-setup event in state {state:?}"
            ),
            Self::UnexpectedMediaMessage { state, id } => write!(
                formatter,
                "unexpected media message {id:?} in state {state:?}"
            ),
            Self::UnsupportedCodec { requested } => write!(
                formatter,
                "phone requested unsupported codec type {requested}"
            ),
            Self::MissingCodecType => {
                formatter.write_str("Setup is missing its required type field")
            }
            Self::MissingOpenFlag => {
                formatter.write_str("MicrophoneRequest is missing its required open field")
            }
            Self::MissingSessionId => {
                formatter.write_str("Ack is missing its required session_id field")
            }
            Self::UnexpectedWireType { field, wire_type } => write!(
                formatter,
                "microphone-setup field {field} has unexpected wire type {wire_type}"
            ),
            Self::Envelope(error) => write!(formatter, "{error}"),
            Self::Truncated => formatter.write_str("truncated microphone-setup protobuf field"),
            Self::InvalidVarint => formatter.write_str("invalid microphone-setup protobuf varint"),
            Self::InvalidFieldNumber => {
                formatter.write_str("microphone-setup protobuf field number cannot be zero")
            }
            Self::LengthNotRepresentable => {
                formatter.write_str("microphone-setup field length cannot be represented")
            }
            Self::UnsupportedWireType(wire_type) => {
                write!(formatter, "unsupported protobuf wire type {wire_type}")
            }
        }
    }
}

impl std::error::Error for MicrophoneSetupError {}

impl ProtobufDecodeError for MicrophoneSetupError {
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

/// What [`MicrophoneSetupStateMachine::send_data`] did with a captured PCM
/// buffer the caller wanted to send.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MicrophoneSendOutcome {
    /// Within credit — the caller should `send_encrypted` this message.
    Sent(MediaMessage),
    /// The channel isn't `Streaming` right now — the caller should discard
    /// the buffer (there is nowhere to send it).
    NotStreaming,
    /// `max_unacked` outstanding `Data` frames are already unacknowledged.
    /// The caller should drop the newest buffer and count it, never queue
    /// it — a voice assistant needs live audio, and a stale backlog is
    /// nearly as useless as a gap. Deliberately simpler than LIVI's fuller
    /// bounded-backlog-with-refcount design; see `DEFAULT_MICROPHONE_MAX_UNACKED`'s
    /// doc comment.
    CreditExhausted,
}

/// The `Microphone` channel's (optional `Setup`→`Config`→)
/// `MicrophoneRequest`/`MicrophoneResponse`→`Start`→`Data`/`Ack` handshake,
/// driven only after that channel's [`crate::ChannelOpenStateMachine`] has
/// reached `Open`. Unlike every sink channel in this crate
/// (`VideoSetupStateMachine`, `AudioSetupStateMachine`), this channel is a
/// `MediaSourceService`: the head unit — not the phone — sends
/// `Start`/`Data`, and originates `session_id` itself rather than echoing
/// one from the phone. See this module's header comment for full
/// provenance.
///
/// **`Setup` is optional, not a required gate — real-hardware-confirmed
/// 2026-08-17.** The assumed exchange order (from LIVI, see the header
/// comment) had `Setup`→`Config` always precede `MicrophoneRequest`,
/// mirroring the sink channels. A real phone instead sent
/// `MicrophoneRequest` directly, with no `Setup` ever sent on this
/// channel, crashing the whole probe (`UnexpectedMediaMessage` in
/// `AwaitingSetup`) partway through an otherwise-healthy session. This
/// makes sense in hindsight: unlike the sink channels, where `Setup`
/// negotiates *which* of several advertised codecs/configs to use, this
/// channel's `ServiceDiscoveryResponse` only ever advertises one
/// `AudioConfiguration` (`MicrophoneCapability`) — there is nothing to
/// negotiate, so a real phone may skip asking. `AwaitingMicrophoneRequest`
/// now accepts `Setup` optionally (replied to with `Config`, but not
/// required before `MicrophoneRequest`) instead of gating on it via a
/// separate `AwaitingSetup` state.
#[derive(Debug)]
pub struct MicrophoneSetupStateMachine {
    state: MicrophoneSetupState,
    /// Set on every accepted `open: true` request; cleared on close.
    session_id: Option<i32>,
    /// Monotonically incrementing — this head unit's own session-id
    /// source, since (unlike every sink channel) there is no phone-
    /// provided value to echo.
    next_session_id: i32,
    /// From the most recent `MicrophoneRequest.max_unacked`, or
    /// [`DEFAULT_MICROPHONE_MAX_UNACKED`] if absent/non-positive.
    max_unacked: u32,
    /// `Data` frames sent since the last credit reset, minus what `Ack`
    /// has replenished. Reset to 0 on every fresh `Streaming` entry.
    outstanding: u32,
}

impl Default for MicrophoneSetupStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl MicrophoneSetupStateMachine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: MicrophoneSetupState::AwaitingMicrophoneRequest,
            session_id: None,
            next_session_id: 1,
            max_unacked: DEFAULT_MICROPHONE_MAX_UNACKED,
            outstanding: 0,
        }
    }

    #[must_use]
    pub const fn state(&self) -> MicrophoneSetupState {
        self.state
    }

    pub fn advance(
        &mut self,
        event: MicrophoneSetupEvent<'_>,
    ) -> Result<Vec<MicrophoneSetupAction>, MicrophoneSetupError> {
        let result = self.advance_inner(event);
        if result.is_err() {
            self.state = MicrophoneSetupState::Failed;
        }
        result
    }

    fn advance_inner(
        &mut self,
        event: MicrophoneSetupEvent<'_>,
    ) -> Result<Vec<MicrophoneSetupAction>, MicrophoneSetupError> {
        let MicrophoneSetupEvent::InboundMedia(payload) = event;
        let message = MediaMessage::decode(payload, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
            .map_err(MicrophoneSetupError::Envelope)?;
        match self.state {
            MicrophoneSetupState::AwaitingMicrophoneRequest => {
                self.handle_awaiting_request(&message)
            }
            MicrophoneSetupState::Streaming => self.handle_streaming(&message),
            MicrophoneSetupState::Failed => {
                Err(MicrophoneSetupError::UnexpectedEvent { state: self.state })
            }
        }
    }

    /// Handles traffic before `Streaming`: an optional `Setup` (replied to
    /// with `Config`, but not required — see [`MicrophoneSetupStateMachine`]'s
    /// doc comment for the real-hardware finding behind this) or a
    /// `MicrophoneRequest` arriving directly. Fails closed on anything
    /// else.
    fn handle_awaiting_request(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<MicrophoneSetupAction>, MicrophoneSetupError> {
        match message.id {
            MediaMessageId::Setup => {
                let codec_type = decode_setup(&message.body)?;
                if codec_type != MEDIA_CODEC_AUDIO_PCM {
                    return Err(MicrophoneSetupError::UnsupportedCodec {
                        requested: codec_type,
                    });
                }
                let mut body = Vec::new();
                // Config.status (field 1, required nested Status enum).
                protobuf::write_int32_field(&mut body, 1, CONFIG_STATUS_READY);
                // Config.max_unacked (field 2, optional uint32).
                protobuf::write_int32_field(&mut body, 2, CONFIG_MAX_UNACKED_PLACEHOLDER);
                // Config.configuration_indices (field 3, repeated uint32, non-packed).
                #[allow(clippy::cast_possible_wrap)]
                let advertised_index = ADVERTISED_CONFIGURATION_INDEX as i32;
                protobuf::write_int32_field(&mut body, 3, advertised_index);
                Ok(vec![MicrophoneSetupAction::SendMedia(MediaMessage {
                    id: MediaMessageId::Config,
                    body,
                })])
            }
            MediaMessageId::MicrophoneRequest => self.apply_microphone_request(message),
            // A trailing `Ack`/`Stop` for the just-closed `Streaming`
            // session, racing against our own transition back to this
            // state — real-hardware-confirmed 2026-08-17: a phone sent
            // `Ack` immediately after this project's own
            // `StreamingStopped` transition (acknowledging the last
            // `Data` frames sent before it closed the channel). Silently
            // ignored, not an error — there is no session/credit state to
            // update here, mirroring how `handle_streaming`'s `Ack` arm
            // already tolerates a stale/mismatched `session_id` the same
            // way.
            MediaMessageId::Ack | MediaMessageId::Stop => Ok(Vec::new()),
            other => Err(MicrophoneSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: other,
            }),
        }
    }

    /// Validates and applies a `MicrophoneRequest`, shared by the initial
    /// `AwaitingMicrophoneRequest` entry point and a repeated request
    /// while already `Streaming` (accepted as a reconfiguration, mirroring
    /// `crate::AudioSetupAction::Ready`'s documented repeated-`Start`
    /// handling). Every accepted `open: true` — first or repeat — issues a
    /// **fresh** `session_id` and resets the flow-control credit, treating
    /// "reopen while already open" uniformly as "start clean" rather than
    /// trying to preserve in-flight state across it.
    fn apply_microphone_request(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<MicrophoneSetupAction>, MicrophoneSetupError> {
        let (open, max_unacked) = decode_microphone_request(&message.body)?;
        let was_streaming = self.state == MicrophoneSetupState::Streaming;
        if open {
            let session_id = self.next_session_id;
            self.next_session_id = self.next_session_id.wrapping_add(1);
            self.session_id = Some(session_id);
            self.max_unacked = match max_unacked {
                #[allow(clippy::cast_sign_loss)]
                Some(value) if value > 0 => value as u32,
                _ => DEFAULT_MICROPHONE_MAX_UNACKED,
            };
            self.outstanding = 0;
            self.state = MicrophoneSetupState::Streaming;
            Ok(vec![
                MicrophoneSetupAction::SendMedia(encode_microphone_response(Some(session_id))),
                MicrophoneSetupAction::SendMedia(encode_start(
                    session_id,
                    ADVERTISED_CONFIGURATION_INDEX,
                )),
                MicrophoneSetupAction::Streaming { session_id },
            ])
        } else {
            self.session_id = None;
            self.outstanding = 0;
            self.state = MicrophoneSetupState::AwaitingMicrophoneRequest;
            let mut actions = vec![MicrophoneSetupAction::SendMedia(
                encode_microphone_response(None),
            )];
            if was_streaming {
                actions.push(MicrophoneSetupAction::StreamingStopped);
            }
            Ok(actions)
        }
    }

    /// Handles traffic once `Streaming`: a repeated `MicrophoneRequest`
    /// (open or close), the phone's flow-control `Ack`, or `Stop`. Fails
    /// closed on anything else, matching every other decoder in this
    /// crate.
    fn handle_streaming(
        &mut self,
        message: &MediaMessage,
    ) -> Result<Vec<MicrophoneSetupAction>, MicrophoneSetupError> {
        match message.id {
            MediaMessageId::MicrophoneRequest => self.apply_microphone_request(message),
            MediaMessageId::Ack => {
                let (session_id, ack) = decode_ack(&message.body)?;
                // A mismatched session_id is a stale/racing ack (e.g. from
                // a just-closed session) — ignored, not an error.
                if self.session_id == Some(session_id) {
                    self.outstanding = self.outstanding.saturating_sub(ack.unwrap_or(1));
                }
                Ok(Vec::new())
            }
            // Unconditional, unacknowledged notification — mirrors every
            // sink channel's identical `Stop` handling
            // (`crate::AudioSetupAction::StopReceived`'s doc comment).
            MediaMessageId::Stop => {
                self.session_id = None;
                self.outstanding = 0;
                self.state = MicrophoneSetupState::AwaitingMicrophoneRequest;
                Ok(vec![MicrophoneSetupAction::StreamingStopped])
            }
            other => Err(MicrophoneSetupError::UnexpectedMediaMessage {
                state: self.state,
                id: other,
            }),
        }
    }

    /// Asks whether a just-captured PCM buffer may be sent right now. Not
    /// part of [`Self::advance`] — this isn't decoding an inbound wire
    /// message, it's the caller (owning real capture hardware) asking
    /// permission before spending a send. See [`MicrophoneSendOutcome`]'s
    /// variants for what the caller should do with each outcome.
    pub fn send_data(&mut self, timestamp: u64, payload: &[u8]) -> MicrophoneSendOutcome {
        if self.state != MicrophoneSetupState::Streaming {
            return MicrophoneSendOutcome::NotStreaming;
        }
        if self.outstanding >= self.max_unacked {
            return MicrophoneSendOutcome::CreditExhausted;
        }
        self.outstanding += 1;
        MicrophoneSendOutcome::Sent(encode_media_data(timestamp, payload))
    }
}

/// Decodes `Setup` and returns its `type` (a `MediaCodecType` wire value).
/// Identical shape to `audio_setup.rs`'s `decode_setup`, duplicated rather
/// than shared since the two crates' error types differ and this is a
/// handful of lines.
fn decode_setup(body: &[u8]) -> Result<i32, MicrophoneSetupError> {
    let mut cursor = 0;
    let mut codec_type = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<MicrophoneSetupError>(body, &mut cursor)?;
        match field {
            1 if wire_type != 0 => {
                return Err(MicrophoneSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<MicrophoneSetupError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                codec_type = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<MicrophoneSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    codec_type.ok_or(MicrophoneSetupError::MissingCodecType)
}

/// Decodes `MicrophoneRequest` and returns `(open, max_unacked)`.
/// `anc_enabled`/`ec_enabled` (fields 2/3) are decoded implicitly via the
/// unknown-field skip path — this project has no ANC/EC hardware path to
/// act on them.
fn decode_microphone_request(body: &[u8]) -> Result<(bool, Option<i32>), MicrophoneSetupError> {
    let mut cursor = 0;
    let mut open = None;
    let mut max_unacked = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<MicrophoneSetupError>(body, &mut cursor)?;
        match field {
            1 | 4 if wire_type != 0 => {
                return Err(MicrophoneSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<MicrophoneSetupError>(body, &mut cursor)?;
                open = Some(raw != 0);
            }
            4 => {
                let raw = protobuf::read_varint::<MicrophoneSetupError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                max_unacked = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<MicrophoneSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    let open = open.ok_or(MicrophoneSetupError::MissingOpenFlag)?;
    Ok((open, max_unacked))
}

/// Decodes the microphone channel's `Ack` and returns `(session_id, ack)`.
/// `receive_timestamp_ns` (field 3, repeated) is decoded implicitly via
/// the unknown-field skip path — never needed by this project's own
/// flow-control logic.
fn decode_ack(body: &[u8]) -> Result<(i32, Option<u32>), MicrophoneSetupError> {
    let mut cursor = 0;
    let mut session_id = None;
    let mut ack = None;
    while cursor < body.len() {
        let (field, wire_type) = protobuf::read_tag::<MicrophoneSetupError>(body, &mut cursor)?;
        match field {
            1 | 2 if wire_type != 0 => {
                return Err(MicrophoneSetupError::UnexpectedWireType { field, wire_type });
            }
            1 => {
                let raw = protobuf::read_varint::<MicrophoneSetupError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
                let value = (raw as i64) as i32;
                session_id = Some(value);
            }
            2 => {
                let raw = protobuf::read_varint::<MicrophoneSetupError>(body, &mut cursor)?;
                #[allow(clippy::cast_possible_truncation)]
                let value = raw as u32;
                ack = Some(value);
            }
            _ => {
                protobuf::skip_unknown_field::<MicrophoneSetupError>(body, &mut cursor, wire_type)?;
            }
        }
    }
    let session_id = session_id.ok_or(MicrophoneSetupError::MissingSessionId)?;
    Ok((session_id, ack))
}

/// Encodes `MicrophoneResponse`. `session_id` (field 2, optional) is only
/// present when accepting an `open: true` request — omitted on the reply
/// to an `open: false` close, matching this channel's own doc comment on
/// `MicrophoneSetupAction::StreamingStopped`.
fn encode_microphone_response(session_id: Option<i32>) -> MediaMessage {
    let mut body = Vec::new();
    protobuf::write_int32_field(&mut body, 1, MICROPHONE_RESPONSE_STATUS_OK);
    if let Some(session_id) = session_id {
        protobuf::write_int32_field(&mut body, 2, session_id);
    }
    MediaMessage {
        id: MediaMessageId::MicrophoneResponse,
        body,
    }
}

/// Encodes `Start`, sent by this head unit on this channel (reversed from
/// every sink channel elsewhere in this crate, where the phone sends it).
fn encode_start(session_id: i32, configuration_index: u32) -> MediaMessage {
    let mut body = Vec::new();
    protobuf::write_int32_field(&mut body, 1, session_id);
    #[allow(clippy::cast_possible_wrap)]
    let index = configuration_index as i32;
    protobuf::write_int32_field(&mut body, 2, index);
    MediaMessage {
        id: MediaMessageId::Start,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media_message(id: MediaMessageId, body: Vec<u8>) -> Vec<u8> {
        MediaMessage { id, body }
            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
            .expect("encode envelope")
    }

    fn setup_payload(codec_type: i32) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, codec_type);
        media_message(MediaMessageId::Setup, body)
    }

    fn microphone_request_payload(open: bool, max_unacked: Option<i32>) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_bool_field(&mut body, 1, open);
        if let Some(max_unacked) = max_unacked {
            protobuf::write_int32_field(&mut body, 4, max_unacked);
        }
        media_message(MediaMessageId::MicrophoneRequest, body)
    }

    fn ack_payload(session_id: i32, ack: Option<u32>) -> Vec<u8> {
        let mut body = Vec::new();
        protobuf::write_int32_field(&mut body, 1, session_id);
        if let Some(ack) = ack {
            protobuf::write_uint32_field(&mut body, 2, ack);
        }
        media_message(MediaMessageId::Ack, body)
    }

    fn reach_streaming(machine: &mut MicrophoneSetupStateMachine, max_unacked: Option<i32>) -> i32 {
        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(true, max_unacked),
            ))
            .expect("open request");
        let MicrophoneSetupAction::Streaming { session_id } = actions[2] else {
            panic!("expected Streaming action");
        };
        session_id
    }

    #[test]
    fn full_handshake_reaches_streaming_with_exact_bytes() {
        let mut machine = MicrophoneSetupStateMachine::new();
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        assert_eq!(
            machine.state(),
            MicrophoneSetupState::AwaitingMicrophoneRequest
        );
        assert_eq!(actions.len(), 1);
        let MicrophoneSetupAction::SendMedia(config) = &actions[0] else {
            panic!("expected SendMedia");
        };
        assert_eq!(config.id, MediaMessageId::Config);
        assert_eq!(config.body, vec![0x08, 0x02, 0x10, 0x01, 0x18, 0x00]);

        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(true, None),
            ))
            .expect("open");
        assert_eq!(machine.state(), MicrophoneSetupState::Streaming);
        assert_eq!(actions.len(), 3);
        let MicrophoneSetupAction::SendMedia(response) = &actions[0] else {
            panic!("expected SendMedia");
        };
        assert_eq!(response.id, MediaMessageId::MicrophoneResponse);
        assert_eq!(response.body, vec![0x08, 0x00, 0x10, 0x01]);
        let MicrophoneSetupAction::SendMedia(start) = &actions[1] else {
            panic!("expected SendMedia");
        };
        assert_eq!(start.id, MediaMessageId::Start);
        assert_eq!(start.body, vec![0x08, 0x01, 0x10, 0x00]);
        assert_eq!(
            actions[2],
            MicrophoneSetupAction::Streaming { session_id: 1 }
        );
    }

    #[test]
    fn send_data_produces_an_8_byte_timestamp_prefixed_data_message() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, None);
        let outcome = machine.send_data(42, &[0xaa, 0xbb, 0xcc]);
        let MicrophoneSendOutcome::Sent(message) = outcome else {
            panic!("expected Sent");
        };
        assert_eq!(message.id, MediaMessageId::Data);
        let mut expected = 42_u64.to_be_bytes().to_vec();
        expected.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        assert_eq!(message.body, expected);
    }

    #[test]
    fn send_data_before_streaming_returns_not_streaming() {
        let mut machine = MicrophoneSetupStateMachine::new();
        assert_eq!(
            machine.send_data(1, &[0x00]),
            MicrophoneSendOutcome::NotStreaming
        );
    }

    #[test]
    fn send_data_exhausts_credit_and_recovers_after_ack() {
        let mut machine = MicrophoneSetupStateMachine::new();
        let session_id = reach_streaming(&mut machine, Some(2));
        assert!(matches!(
            machine.send_data(1, &[0x00]),
            MicrophoneSendOutcome::Sent(_)
        ));
        assert!(matches!(
            machine.send_data(2, &[0x00]),
            MicrophoneSendOutcome::Sent(_)
        ));
        assert_eq!(
            machine.send_data(3, &[0x00]),
            MicrophoneSendOutcome::CreditExhausted
        );

        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&ack_payload(
                session_id,
                Some(1),
            )))
            .expect("ack");
        assert!(matches!(
            machine.send_data(3, &[0x00]),
            MicrophoneSendOutcome::Sent(_)
        ));
    }

    #[test]
    fn ack_with_stale_session_id_is_ignored() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, Some(1));
        assert!(matches!(
            machine.send_data(1, &[0x00]),
            MicrophoneSendOutcome::Sent(_)
        ));
        assert_eq!(
            machine.send_data(2, &[0x00]),
            MicrophoneSendOutcome::CreditExhausted
        );
        // Ack for a session that isn't the current one — ignored, credit
        // stays exhausted.
        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&ack_payload(
                999,
                Some(1),
            )))
            .expect("stale ack");
        assert_eq!(
            machine.send_data(2, &[0x00]),
            MicrophoneSendOutcome::CreditExhausted
        );
    }

    #[test]
    fn default_max_unacked_applies_when_request_omits_it() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, None);
        for index in 0..DEFAULT_MICROPHONE_MAX_UNACKED {
            assert!(
                matches!(
                    machine.send_data(u64::from(index), &[0x00]),
                    MicrophoneSendOutcome::Sent(_)
                ),
                "frame {index} should have been sent"
            );
        }
        assert_eq!(
            machine.send_data(99, &[0x00]),
            MicrophoneSendOutcome::CreditExhausted
        );
    }

    #[test]
    fn accepts_repeated_open_request_as_reconfiguration_with_a_fresh_session_id() {
        let mut machine = MicrophoneSetupStateMachine::new();
        let first_session = reach_streaming(&mut machine, None);
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(true, None),
            ))
            .expect("repeated open is accepted");
        assert_eq!(machine.state(), MicrophoneSetupState::Streaming);
        let MicrophoneSetupAction::Streaming { session_id } = actions[2] else {
            panic!("expected Streaming action");
        };
        assert_ne!(session_id, first_session);
    }

    #[test]
    fn microphone_request_open_false_closes_then_can_reopen() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, None);

        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(false, None),
            ))
            .expect("close");
        assert_eq!(
            machine.state(),
            MicrophoneSetupState::AwaitingMicrophoneRequest
        );
        assert_eq!(actions.len(), 2);
        let MicrophoneSetupAction::SendMedia(response) = &actions[0] else {
            panic!("expected SendMedia");
        };
        assert_eq!(response.id, MediaMessageId::MicrophoneResponse);
        assert_eq!(response.body, vec![0x08, 0x00]);
        assert_eq!(actions[1], MicrophoneSetupAction::StreamingStopped);
        assert_eq!(
            machine.send_data(1, &[0x00]),
            MicrophoneSendOutcome::NotStreaming
        );

        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(true, None),
            ))
            .expect("reopen");
        assert_eq!(machine.state(), MicrophoneSetupState::Streaming);
        assert!(matches!(
            actions[2],
            MicrophoneSetupAction::Streaming { .. }
        ));
    }

    #[test]
    fn microphone_request_open_false_while_not_streaming_gets_no_streaming_stopped() {
        let mut machine = MicrophoneSetupStateMachine::new();
        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(false, None),
            ))
            .expect("close while never opened");
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], MicrophoneSetupAction::SendMedia(_)));
    }

    #[test]
    fn stop_while_streaming_closes_without_a_reply() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, None);
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Stop,
                Vec::new(),
            )))
            .expect("stop");
        assert_eq!(actions, vec![MicrophoneSetupAction::StreamingStopped]);
        assert_eq!(
            machine.state(),
            MicrophoneSetupState::AwaitingMicrophoneRequest
        );
    }

    /// Real-hardware-confirmed 2026-08-17: a phone sent a trailing `Ack`
    /// for the just-closed session's last `Data` frames, racing against
    /// this project's own transition back to `AwaitingMicrophoneRequest`
    /// — this crashed the probe before this fix.
    #[test]
    fn stray_ack_after_streaming_stopped_is_ignored() {
        let mut machine = MicrophoneSetupStateMachine::new();
        let session_id = reach_streaming(&mut machine, None);
        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Stop,
                Vec::new(),
            )))
            .expect("stop");
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&ack_payload(
                session_id,
                Some(1),
            ))),
            Ok(Vec::new())
        );
        assert_eq!(
            machine.state(),
            MicrophoneSetupState::AwaitingMicrophoneRequest
        );
    }

    /// Same reasoning as the stray-`Ack` case above, for a redundant
    /// `Stop` — plausible since either `Stop` or `MicrophoneRequest{open:
    /// false}` can close the channel (`apply_microphone_request`'s doc
    /// comment), so both could legitimately arrive in either order.
    #[test]
    fn stray_stop_after_streaming_stopped_is_ignored() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, None);
        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Stop,
                Vec::new(),
            )))
            .expect("stop");
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Stop,
                Vec::new(),
            ))),
            Ok(Vec::new())
        );
        assert_eq!(
            machine.state(),
            MicrophoneSetupState::AwaitingMicrophoneRequest
        );
    }

    #[test]
    fn rejects_unexpected_message_while_streaming() {
        let mut machine = MicrophoneSetupStateMachine::new();
        reach_streaming(&mut machine, None);
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM
            ))),
            Err(MicrophoneSetupError::UnexpectedMediaMessage {
                state: MicrophoneSetupState::Streaming,
                id: MediaMessageId::Setup,
            })
        );
        assert_eq!(machine.state(), MicrophoneSetupState::Failed);
    }

    #[test]
    fn rejects_unsupported_codec_and_fails_closed() {
        let mut machine = MicrophoneSetupStateMachine::new();
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(99))),
            Err(MicrophoneSetupError::UnsupportedCodec { requested: 99 })
        );
        assert_eq!(machine.state(), MicrophoneSetupState::Failed);
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM
            ))),
            Err(MicrophoneSetupError::UnexpectedEvent {
                state: MicrophoneSetupState::Failed
            })
        );
    }

    /// Real-hardware-confirmed 2026-08-17: a real phone sent
    /// `MicrophoneRequest` with no `Setup` ever sent on this channel at
    /// all — see [`MicrophoneSetupStateMachine`]'s doc comment. `Setup` is
    /// optional, not a required gate.
    #[test]
    fn accepts_microphone_request_with_no_prior_setup() {
        let mut machine = MicrophoneSetupStateMachine::new();
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(true, None),
            ))
            .expect("MicrophoneRequest with no prior Setup is accepted");
        assert_eq!(machine.state(), MicrophoneSetupState::Streaming);
        assert_eq!(actions.len(), 3);
    }

    /// `Setup` is still accepted and replied to with `Config` if a phone
    /// does send it — it just doesn't gate anything further.
    #[test]
    fn setup_is_optional_but_still_acknowledged_if_sent() {
        let mut machine = MicrophoneSetupStateMachine::new();
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        assert_eq!(
            machine.state(),
            MicrophoneSetupState::AwaitingMicrophoneRequest
        );
        let [MicrophoneSetupAction::SendMedia(config)] = actions.as_slice() else {
            panic!("expected exactly one SendMedia action, got {actions:?}");
        };
        assert_eq!(config.id, MediaMessageId::Config);
        // A MicrophoneRequest still works fine afterward.
        let actions = machine
            .advance(MicrophoneSetupEvent::InboundMedia(
                &microphone_request_payload(true, None),
            ))
            .expect("open after setup");
        assert_eq!(machine.state(), MicrophoneSetupState::Streaming);
        assert_eq!(actions.len(), 3);
    }

    #[test]
    fn rejects_missing_required_fields() {
        let mut machine = MicrophoneSetupStateMachine::new();
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&media_message(
                MediaMessageId::Setup,
                Vec::new(),
            ))),
            Err(MicrophoneSetupError::MissingCodecType)
        );

        let mut machine = MicrophoneSetupStateMachine::new();
        machine
            .advance(MicrophoneSetupEvent::InboundMedia(&setup_payload(
                MEDIA_CODEC_AUDIO_PCM,
            )))
            .expect("setup");
        assert_eq!(
            machine.advance(MicrophoneSetupEvent::InboundMedia(&media_message(
                MediaMessageId::MicrophoneRequest,
                Vec::new(),
            ))),
            Err(MicrophoneSetupError::MissingOpenFlag)
        );
    }
}
