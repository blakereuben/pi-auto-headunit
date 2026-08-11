//! Property/fuzz tests for the parsers that see untrusted phone input
//! before any authentication has happened: frame decoding, control-message
//! decoding, and service-discovery summarization. Every parser here must
//! only ever return `Ok` or a well-typed error for arbitrary bytes — never
//! panic, index out of bounds, or allocate unboundedly.

use proptest::prelude::*;
use protocol_aap::{
    ControlMessage, DEFAULT_MAX_CONTROL_BODY_SIZE, Encryption, FrameHeader, FrameType, MessageType,
    ProtocolLimits, ServiceDiscoveryLimits, decode_frame, encode_frame,
    summarize_service_discovery_request,
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
}
