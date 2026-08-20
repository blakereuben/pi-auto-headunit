//! Property/fuzz tests for the parsers that see untrusted phone input:
//! frame decoding, control-message decoding, and service-discovery
//! summarization (all pre-authentication), plus the post-TLS-handshake
//! per-channel decoders (`ChannelOpenStateMachine`, `VideoSetupStateMachine`,
//! `MediaMessage::decode`) — still untrusted phone-originated bytes even
//! though TLS has completed by the time they run (M6's "complete parser
//! fuzz/property testing" widened this file's original pre-auth-only
//! scope). Every parser here must only ever return `Ok` or a well-typed
//! error for arbitrary bytes — never panic, index out of bounds, or
//! allocate unboundedly.

use proptest::prelude::*;
use protocol_aap::{
    ChannelOpenEvent, ChannelOpenStateMachine, ControlMessage, DEFAULT_MAX_CONTROL_BODY_SIZE,
    DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, Encryption, FrameHeader, FrameType, MediaMessage,
    MediaMessageId, MessageType, ProtocolLimits, ServiceDiscoveryLimits, VideoSetupEvent,
    VideoSetupStateMachine, decode_frame, encode_frame, summarize_service_discovery_request,
};

fn push_varint(body: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        body.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn push_length_delimited_field(body: &mut Vec<u8>, field: u32, value: &[u8]) {
    push_varint(body, (u64::from(field) << 3) | 2);
    push_varint(body, value.len() as u64);
    body.extend_from_slice(value);
}

/// Proto2 `int32`/enum field encoding (varint, sign-extended through
/// `i64`) — mirrors `protobuf::write_int32_field`, which is `pub(crate)`
/// and so not reachable from this external test crate.
fn push_int32_field(body: &mut Vec<u8>, field: u32, value: i32) {
    push_varint(body, u64::from(field) << 3);
    #[allow(clippy::cast_sign_loss)]
    push_varint(body, i64::from(value) as u64);
}

/// A real `Setup` (H264 baseline profile — `MEDIA_CODEC_VIDEO_H264_BP`,
/// the private `video_setup.rs` constant `3`, cited directly since it's
/// not exported) then `Start` (echoing the only configuration index this
/// project ever offers for that codec, `0` —
/// `ADVERTISED_H264_CONFIGURATION_INDEX`, same reachability note), driving
/// a fresh state machine to `Ready` — the state fuzzed arbitrary media
/// traffic below actually needs to exercise `handle_media` rather than
/// only ever hitting the earlier `handle_setup`/`handle_start` reject
/// paths a purely random first message would almost always hit.
fn drive_video_setup_to_ready() -> VideoSetupStateMachine {
    let mut machine = VideoSetupStateMachine::new();
    let mut setup_body = Vec::new();
    push_int32_field(&mut setup_body, 1, 3);
    let setup_payload = MediaMessage {
        id: MediaMessageId::Setup,
        body: setup_body,
    }
    .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
    .expect("encode setup");
    machine
        .advance(VideoSetupEvent::InboundMedia(&setup_payload))
        .expect("setup");

    let mut start_body = Vec::new();
    push_int32_field(&mut start_body, 1, 7);
    push_int32_field(&mut start_body, 2, 0);
    let start_payload = MediaMessage {
        id: MediaMessageId::Start,
        body: start_body,
    }
    .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
    .expect("encode start");
    machine
        .advance(VideoSetupEvent::InboundMedia(&start_payload))
        .expect("start");

    machine
}

proptest! {
    /// Arbitrary bytes, of any length and content a phone (or an attacker
    /// impersonating one) could send, must never panic the frame decoder.
    #[test]
    fn decode_frame_never_panics_on_arbitrary_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let _ = decode_frame(&bytes, ProtocolLimits::default());
    }

    /// Same, for the reassembled control-message decoder.
    #[test]
    fn control_message_decode_never_panics_on_arbitrary_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let _ = ControlMessage::decode(&bytes, DEFAULT_MAX_CONTROL_BODY_SIZE);
    }

    /// Same, for the bounded service-discovery summarizer, which is the
    /// first parser to see raw phone-supplied protobuf bytes.
    #[test]
    fn service_discovery_summary_never_panics_on_arbitrary_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let _ = summarize_service_discovery_request(&bytes, ServiceDiscoveryLimits::default());
    }

    /// Any payload that fits the default limits round-trips through
    /// encode/decode unchanged, for arbitrary channel IDs and content.
    #[test]
    fn frame_round_trips_for_arbitrary_valid_payloads(
        channel_id in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 0..protocol_aap::AASDK_MAX_FRAME_PAYLOAD_SIZE)
    ) {
        let header = FrameHeader {
            channel_id,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Plain,
            message_type: MessageType::Specific,
        };
        let limits = ProtocolLimits::default();
        let encoded = encode_frame(header, None, &payload, limits).expect("encode");
        let decoded = decode_frame(&encoded, limits).expect("decode");
        prop_assert_eq!(decoded.header, header);
        prop_assert_eq!(decoded.payload, payload.as_slice());
        prop_assert_eq!(decoded.consumed, encoded.len());
        prop_assert_eq!(decoded.total_message_size, None);
    }

    /// The summary never retains (and its Debug output never leaks) the
    /// actual label text/device name a phone supplied, for arbitrary
    /// generated values — generalizing the fixed-input unit test in
    /// `service_discovery.rs`.
    #[test]
    fn service_discovery_summary_never_leaks_device_text(
        // Letters only, minimum 8 characters: long and varied enough that
        // it cannot coincidentally collide with the short fixed vocabulary
        // (field names, "None"/"Some") or the 1-2 digit byte counts that
        // legitimately appear in the summary's own Debug output.
        label_text in "[a-zA-Z]{8,64}",
        device_name in "[a-zA-Z]{8,64}",
    ) {
        let mut body = Vec::new();
        push_length_delimited_field(&mut body, 4, label_text.as_bytes());
        push_length_delimited_field(&mut body, 5, device_name.as_bytes());

        let summary = summarize_service_discovery_request(&body, ServiceDiscoveryLimits::default())
            .expect("summarize");
        prop_assert_eq!(summary.label_text_bytes, Some(label_text.len()));
        prop_assert_eq!(summary.device_name_bytes, Some(device_name.len()));

        let debug = format!("{summary:?}");
        prop_assert!(!debug.contains(&label_text));
        prop_assert!(!debug.contains(&device_name));
    }

    /// `MediaMessage::decode` is the shared envelope every media/control
    /// channel's phone-supplied bytes pass through first (video, audio,
    /// microphone, input) — must never panic regardless of the caller's
    /// configured body-size limit.
    #[test]
    fn media_message_decode_never_panics_on_arbitrary_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
        maximum_body_size in 0_usize..8192,
    ) {
        let _ = MediaMessage::decode(&bytes, maximum_body_size);
    }

    /// `ChannelOpenStateMachine` decodes `ChannelOpenRequest` on all three
    /// wired channels (video, input, media audio) — the first phone-
    /// supplied bytes each channel ever sees, post-TLS-handshake but still
    /// untrusted.
    #[test]
    fn channel_open_state_machine_never_panics_on_arbitrary_bytes(
        channel_id in any::<u8>(),
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
    ) {
        let mut machine = ChannelOpenStateMachine::new(channel_id);
        let _ = machine.advance(ChannelOpenEvent::InboundControl(&bytes));
    }

    /// Same, for the video channel's own setup handshake
    /// (`VideoSetupStateMachine`) from its very first message
    /// (`AwaitingSetup`) — almost always rejects arbitrary bytes, but must
    /// never panic doing so.
    #[test]
    fn video_setup_state_machine_never_panics_on_arbitrary_bytes_from_awaiting_setup(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let mut machine = VideoSetupStateMachine::new();
        let _ = machine.advance(VideoSetupEvent::InboundMedia(&bytes));
    }

    /// Same, but from `Ready` (`handle_media` — the `Data`/`CodecConfig`/
    /// `VideoFocusRequest`/`Stop` parsing path) via
    /// `drive_video_setup_to_ready`, since a purely random first message
    /// almost never reaches that state on its own — this is the actual
    /// steady-state attack surface for the rest of a real session, not
    /// just its opening handshake.
    #[test]
    fn video_setup_state_machine_never_panics_on_arbitrary_bytes_once_ready(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let mut machine = drive_video_setup_to_ready();
        let _ = machine.advance(VideoSetupEvent::InboundMedia(&bytes));
    }
}
