//! Proves the `Microphone` channel's `ChannelOpenRequest`/`Response` →
//! `Setup` → `Config` → `MicrophoneRequest`/`MicrophoneResponse` → `Start`
//! → `Data` → `Ack` → `Stop` lifecycle end to end, using real TLS 1.2
//! crypto (not scripted bytes) and real frame reassembly, mirroring
//! `full_channel_setup.rs`'s rigor.
//!
//! Kept as its own file rather than a fourth channel folded into
//! `full_channel_setup.rs`: that test is specifically scoped to prove
//! routing across concurrently-fragmenting *reactive* channels, where the
//! phone always initiates. This channel's defining new behaviour —
//! proactive, head-unit-initiated `Start`/`Data` sends, and credit-gated
//! backpressure driven by the phone's own encrypted `Ack` frames — is a
//! different shape of thing to prove. Deliberately has no dependency on
//! `media-gstreamer`: this proves the wire protocol only, preserving
//! `protocol-aap`'s zero-media-pipeline-dependency boundary, exactly like
//! `full_channel_setup.rs` does for the sink channels.

mod common;

use common::{establish_tls_session, limits, read_and_decrypt_message};
use protocol_aap::{
    ChannelOpenAction, ChannelOpenEvent, ChannelOpenStateMachine, ControlMessage, ControlMessageId,
    DEFAULT_MAX_CONTROL_BODY_SIZE, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE, Encryption, FrameHeader,
    FrameType, MediaMessage, MediaMessageId, MessageType, MicrophoneSendOutcome,
    MicrophoneSetupAction, MicrophoneSetupEvent, MicrophoneSetupState, MicrophoneSetupStateMachine,
    TlsClient, decode_frame, encode_frame,
};
use security_openssl::{OpenSslTlsClient, TestServerTls};
use transport_api::{SessionTransport, fake};

const MICROPHONE_CHANNEL_ID: u8 = 8;

fn channel_open_request(service_id: u8) -> ControlMessage {
    ControlMessage {
        id: ControlMessageId::ChannelOpenRequest,
        body: vec![0x08, 0x00, 0x10, service_id],
    }
}

fn setup_message() -> MediaMessage {
    MediaMessage {
        id: MediaMessageId::Setup,
        body: vec![0x08, 0x01], // type = MEDIA_CODEC_AUDIO_PCM (1)
    }
}

/// `MicrophoneRequest{open, max_unacked}` — field 1 (`open`, bool), field 4
/// (`max_unacked`, optional int32).
fn microphone_request_message(open: bool, max_unacked: Option<i32>) -> MediaMessage {
    let mut body = vec![0x08, u8::from(open)];
    if let Some(max_unacked) = max_unacked {
        body.push(0x20);
        body.push(u8::try_from(max_unacked).expect("small test value"));
    }
    MediaMessage {
        id: MediaMessageId::MicrophoneRequest,
        body,
    }
}

/// The microphone channel's own `Ack` schema — field 1 (`session_id`,
/// int32), field 2 (`ack`, optional uint32) — distinct from the sink
/// channels' `Ack` this crate encodes via `encode_media_ack`.
fn ack_message(session_id: u8, ack: u8) -> MediaMessage {
    MediaMessage {
        id: MediaMessageId::Ack,
        body: vec![0x08, session_id, 0x10, ack],
    }
}

fn phone_sends_encrypted(
    phone: &fake::Peer,
    phone_tls: &mut TestServerTls,
    channel_id: u8,
    message_type: MessageType,
    plaintext: &[u8],
) {
    let ciphertext = phone_tls
        .encrypt_application_data(plaintext)
        .expect("phone encrypt");
    let frame = encode_frame(
        FrameHeader {
            channel_id,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode encrypted frame");
    phone.push_inbound(&frame).expect("push encrypted frame");
}

fn phone_receives_encrypted(
    phone: &fake::Peer,
    phone_tls: &mut TestServerTls,
) -> (u8, MessageType, Vec<u8>) {
    let outbound = phone.drain_outbound();
    let frame = decode_frame(&outbound, limits()).expect("decode outbound frame");
    assert_eq!(frame.header.encryption, Encryption::Encrypted);
    let plaintext = phone_tls
        .decrypt_application_data(frame.payload)
        .expect("phone decrypt");
    (
        frame.header.channel_id,
        frame.header.message_type,
        plaintext,
    )
}

fn send_media_from_sut(
    transport: &mut fake::Transport,
    client_tls: &mut OpenSslTlsClient,
    channel_id: u8,
    message: &MediaMessage,
) {
    let payload = message
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode media message");
    let ciphertext = client_tls
        .encrypt_application_data(&payload)
        .expect("client encrypt");
    let frame = encode_frame(
        FrameHeader {
            channel_id,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode media frame");
    transport.send_all(&frame).expect("send media message");
}

#[allow(clippy::too_many_lines)]
#[test]
fn drives_microphone_channel_through_a_full_streaming_and_flow_control_cycle() {
    let (mut transport, phone, handshake, mut client_tls, mut phone_tls, mut assembler) =
        establish_tls_session(2);

    // --- Channel opens ---
    let mut channel_open = ChannelOpenStateMachine::new(MICROPHONE_CHANNEL_ID);
    let open_payload = channel_open_request(MICROPHONE_CHANNEL_ID)
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode open request");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        MICROPHONE_CHANNEL_ID,
        MessageType::Control,
        &open_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read channel open request");
    assert_eq!(message.channel_id, MICROPHONE_CHANNEL_ID);
    let actions = channel_open
        .advance(ChannelOpenEvent::InboundControl(&message.payload))
        .expect("advance channel open");
    let ChannelOpenAction::SendControl(response) = &actions[0];
    let payload = response
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode open response");
    let ciphertext = client_tls
        .encrypt_application_data(&payload)
        .expect("client encrypt open response");
    let frame = encode_frame(
        FrameHeader {
            channel_id: MICROPHONE_CHANNEL_ID,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Control,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode open response frame");
    transport.send_all(&frame).expect("send open response");
    let (channel_id, message_type, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    assert_eq!(channel_id, MICROPHONE_CHANNEL_ID);
    assert_eq!(message_type, MessageType::Control);
    let decoded =
        ControlMessage::decode(&plaintext, DEFAULT_MAX_CONTROL_BODY_SIZE).expect("decode");
    assert_eq!(decoded.body, vec![0x08, 0x00]); // STATUS_SUCCESS

    let mut machine = MicrophoneSetupStateMachine::new();

    // --- Setup -> Config ---
    let setup_payload = setup_message()
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode setup");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        MICROPHONE_CHANNEL_ID,
        MessageType::Specific,
        &setup_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read setup");
    let actions = machine
        .advance(MicrophoneSetupEvent::InboundMedia(&message.payload))
        .expect("advance setup");
    assert_eq!(
        machine.state(),
        MicrophoneSetupState::AwaitingMicrophoneRequest
    );
    let [MicrophoneSetupAction::SendMedia(config)] = actions.as_slice() else {
        panic!("expected exactly one SendMedia action, got {actions:?}");
    };
    assert_eq!(config.id, MediaMessageId::Config);
    send_media_from_sut(
        &mut transport,
        &mut client_tls,
        MICROPHONE_CHANNEL_ID,
        config,
    );
    let (channel_id, message_type, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    assert_eq!(channel_id, MICROPHONE_CHANNEL_ID);
    assert_eq!(message_type, MessageType::Specific);
    let decoded_config = MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("decode config");
    assert_eq!(decoded_config.id, MediaMessageId::Config);

    // --- MicrophoneRequest{open: true, max_unacked: 2} -> MicrophoneResponse + Start ---
    let request_payload = microphone_request_message(true, Some(2))
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode microphone request");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        MICROPHONE_CHANNEL_ID,
        MessageType::Specific,
        &request_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read microphone request");
    let actions = machine
        .advance(MicrophoneSetupEvent::InboundMedia(&message.payload))
        .expect("advance microphone request");
    assert_eq!(machine.state(), MicrophoneSetupState::Streaming);
    assert_eq!(actions.len(), 3);
    let MicrophoneSetupAction::SendMedia(response) = &actions[0] else {
        panic!("expected SendMedia, got {:?}", actions[0]);
    };
    assert_eq!(response.id, MediaMessageId::MicrophoneResponse);
    send_media_from_sut(
        &mut transport,
        &mut client_tls,
        MICROPHONE_CHANNEL_ID,
        response,
    );
    let (_, _, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    let decoded_response = MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("decode response");
    assert_eq!(decoded_response.id, MediaMessageId::MicrophoneResponse);

    let MicrophoneSetupAction::SendMedia(start) = &actions[1] else {
        panic!("expected SendMedia, got {:?}", actions[1]);
    };
    assert_eq!(start.id, MediaMessageId::Start);
    send_media_from_sut(
        &mut transport,
        &mut client_tls,
        MICROPHONE_CHANNEL_ID,
        start,
    );
    let (_, _, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    let decoded_start = MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("decode start");
    assert_eq!(decoded_start.id, MediaMessageId::Start);

    let session_id = match actions[2] {
        MicrophoneSetupAction::Streaming { session_id } => session_id,
        ref other => panic!("expected Streaming, got {other:?}"),
    };

    // --- Data flows head-unit -> phone, gated by the max_unacked=2 credit window ---
    let frame_one = match machine.send_data(1, &[0xaa, 0xbb]) {
        MicrophoneSendOutcome::Sent(message) => message,
        other => panic!("expected Sent, got {other:?}"),
    };
    send_media_from_sut(
        &mut transport,
        &mut client_tls,
        MICROPHONE_CHANNEL_ID,
        &frame_one,
    );
    let (_, _, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    let decoded_data =
        MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE).expect("decode data");
    assert_eq!(decoded_data.id, MediaMessageId::Data);
    let mut expected_body = 1_u64.to_be_bytes().to_vec();
    expected_body.extend_from_slice(&[0xaa, 0xbb]);
    assert_eq!(decoded_data.body, expected_body);

    assert!(matches!(
        machine.send_data(2, &[0xcc]),
        MicrophoneSendOutcome::Sent(_)
    ));
    assert_eq!(
        machine.send_data(3, &[0xdd]),
        MicrophoneSendOutcome::CreditExhausted,
        "max_unacked=2 already spent by the two prior sends"
    );

    // --- The phone's real encrypted Ack replenishes credit ---
    let ack_payload = ack_message(u8::try_from(session_id).expect("small test session id"), 2)
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode ack");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        MICROPHONE_CHANNEL_ID,
        MessageType::Specific,
        &ack_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read ack");
    let actions = machine
        .advance(MicrophoneSetupEvent::InboundMedia(&message.payload))
        .expect("advance ack");
    assert_eq!(
        actions,
        Vec::new(),
        "Ack itself produces no outbound action"
    );
    assert!(
        matches!(
            machine.send_data(3, &[0xdd]),
            MicrophoneSendOutcome::Sent(_)
        ),
        "credit should be replenished after the Ack"
    );

    // --- Stop closes the channel with no reply ---
    let stop_payload = MediaMessage {
        id: MediaMessageId::Stop,
        body: Vec::new(),
    }
    .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
    .expect("encode stop");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        MICROPHONE_CHANNEL_ID,
        MessageType::Specific,
        &stop_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read stop");
    let actions = machine
        .advance(MicrophoneSetupEvent::InboundMedia(&message.payload))
        .expect("advance stop");
    assert_eq!(actions, vec![MicrophoneSetupAction::StreamingStopped]);
    assert_eq!(
        machine.state(),
        MicrophoneSetupState::AwaitingMicrophoneRequest
    );
    assert_eq!(
        machine.send_data(4, &[0xee]),
        MicrophoneSendOutcome::NotStreaming
    );
}
