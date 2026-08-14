//! Proves the video channel's media-payload plumbing carries exact bytes
//! from the wire through to `VideoSetupAction`, independent of `GStreamer`
//! entirely — this is the protocol-level half of the render-pipeline work;
//! `crates/media-gstreamer/src/render.rs` proves the `GStreamer` half
//! separately with its own self-generated synthetic fixture. The bytes
//! used here are deliberately arbitrary and non-video-shaped (not valid
//! H.264), never derived from any real phone content — see `CLAUDE.md`'s
//! user-content rule.

use protocol_aap::{
    DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, MediaMessage, MediaMessageId, VideoSetupAction,
    VideoSetupEvent, VideoSetupStateMachine,
};

const MEDIA_CODEC_VIDEO_H264_BP: i32 = 3;
const ADVERTISED_CONFIGURATION_INDEX: i32 = 0;

fn write_int32_field(body: &mut Vec<u8>, field: u32, value: i32) {
    let tag = field << 3;
    let mut tag_buffer = tag;
    loop {
        let mut byte = (tag_buffer & 0x7f) as u8;
        tag_buffer >>= 7;
        if tag_buffer != 0 {
            byte |= 0x80;
        }
        body.push(byte);
        if tag_buffer == 0 {
            break;
        }
    }
    #[allow(clippy::cast_sign_loss)]
    let mut varint = value as u64;
    loop {
        let mut byte = (varint & 0x7f) as u8;
        varint >>= 7;
        if varint != 0 {
            byte |= 0x80;
        }
        body.push(byte);
        if varint == 0 {
            break;
        }
    }
}

fn media_message(id: MediaMessageId, body: Vec<u8>) -> Vec<u8> {
    MediaMessage { id, body }
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode envelope")
}

fn setup_payload(codec_type: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_int32_field(&mut body, 1, codec_type);
    media_message(MediaMessageId::Setup, body)
}

fn start_payload(session_id: i32, configuration_index: i32) -> Vec<u8> {
    let mut body = Vec::new();
    write_int32_field(&mut body, 1, session_id);
    write_int32_field(&mut body, 2, configuration_index);
    media_message(MediaMessageId::Start, body)
}

fn ready_machine() -> VideoSetupStateMachine {
    let mut machine = VideoSetupStateMachine::new();
    machine
        .advance(VideoSetupEvent::InboundMedia(&setup_payload(
            MEDIA_CODEC_VIDEO_H264_BP,
        )))
        .expect("setup");
    machine
        .advance(VideoSetupEvent::InboundMedia(&start_payload(
            7,
            ADVERTISED_CONFIGURATION_INDEX,
        )))
        .expect("start");
    machine
}

#[test]
fn media_data_action_carries_the_exact_synthetic_payload_bytes() {
    let mut machine = ready_machine();
    let synthetic_payload: Vec<u8> = (0..64).collect();
    let mut body = 123_456_789_u64.to_be_bytes().to_vec();
    body.extend_from_slice(&synthetic_payload);
    let frame = media_message(MediaMessageId::Data, body);

    let actions = machine
        .advance(VideoSetupEvent::InboundMedia(&frame))
        .expect("advance data");
    let VideoSetupAction::MediaDataReceived { timestamp, payload } = &actions[0] else {
        panic!("expected MediaDataReceived");
    };
    assert_eq!(*timestamp, 123_456_789);
    assert_eq!(payload, &synthetic_payload);
}

#[test]
fn codec_config_action_carries_the_exact_synthetic_payload_bytes() {
    let mut machine = ready_machine();
    let synthetic_payload: Vec<u8> = (200..255).collect();
    let frame = media_message(MediaMessageId::CodecConfig, synthetic_payload.clone());

    let actions = machine
        .advance(VideoSetupEvent::InboundMedia(&frame))
        .expect("advance codec config");
    let VideoSetupAction::CodecConfigReceived { payload } = &actions[0] else {
        panic!("expected CodecConfigReceived");
    };
    assert_eq!(payload, &synthetic_payload);
}
