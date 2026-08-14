//! Proves `ServiceDiscoveryResponse` → `ChannelOpenRequest`/`Response`
//! (video + input) → `Setup` → `Config` → `Start` end to end, using real
//! TLS 1.2 crypto (not scripted bytes) and real frame reassembly across
//! three concurrently-fragmenting channels (control, video, input).
//! Mirrors, but does not modify or depend on,
//! `apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs`'s dispatch
//! logic — see the channel-setup design record for the scope this proves
//! and what stays deliberately out of scope (`MEDIA_MESSAGE_DATA`, video
//! decode/render, every other service kind).
//!
//! The video and input channels are deliberately interleaved (input opens
//! *between* the video channel's open and its `Setup`/`Config` exchange)
//! to prove the routing this integration test exercises doesn't assume a
//! fixed global ordering across channels.

mod common;

use common::{establish_tls_session, limits, read_and_decrypt_message};
use protocol_aap::{
    ChannelOpenAction, ChannelOpenEvent, ChannelOpenStateMachine, ControlMessage, ControlMessageId,
    DEFAULT_MAX_CONTROL_BODY_SIZE, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE,
    DEFAULT_MAX_SERVICE_CANDIDATES, Encryption, FrameHeader, FrameType, HandshakeAction,
    HandshakeEvent, HandshakeState, MediaMessage, MediaMessageId, MessageAssembler, MessageType,
    ServiceAvailability, ServiceCandidate, ServiceCapabilities, ServiceCatalogue, ServiceKind,
    TlsClient, TouchCapability, TouchScreenType, VideoCapability, VideoCodecResolution,
    VideoCodecType, VideoFrameRate, VideoSetupAction, VideoSetupEvent, VideoSetupStateMachine,
    decode_frame, encode_frame, encode_service_discovery_response,
};
use security_openssl::{OpenSslTlsClient, TestServerTls};
use transport_api::{SessionTransport, fake};

const VIDEO_CHANNEL_ID: u8 = 1;
const INPUT_CHANNEL_ID: u8 = 2;

fn channel_open_request(service_id: u8) -> ControlMessage {
    ControlMessage {
        id: ControlMessageId::ChannelOpenRequest,
        // priority = 0 (zigzag), service_id — both single-byte varints here.
        body: vec![0x08, 0x00, 0x10, service_id],
    }
}

fn setup_message() -> MediaMessage {
    MediaMessage {
        id: MediaMessageId::Setup,
        body: vec![0x08, 0x03], // type = MEDIA_CODEC_VIDEO_H264_BP (3)
    }
}

fn start_message(session_id: u8) -> MediaMessage {
    MediaMessage {
        id: MediaMessageId::Start,
        body: vec![0x08, session_id, 0x10, 0x00], // session_id, configuration_index = 0
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

/// Drives one channel's `ChannelOpenRequest`→`ChannelOpenResponse` exchange
/// and asserts the phone received exactly `STATUS_SUCCESS` back. Shared by
/// the video and input channels, whose open sequences are otherwise
/// byte-for-byte identical.
#[allow(clippy::too_many_arguments)]
fn open_channel(
    transport: &mut fake::Transport,
    phone: &fake::Peer,
    phone_tls: &mut TestServerTls,
    assembler: &mut MessageAssembler,
    client_tls: &mut OpenSslTlsClient,
    handshake_state: HandshakeState,
    channel_id: u8,
    machine: &mut ChannelOpenStateMachine,
) {
    let open_payload = channel_open_request(channel_id)
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode open request");
    phone_sends_encrypted(
        phone,
        phone_tls,
        channel_id,
        MessageType::Control,
        &open_payload,
    );

    let message = read_and_decrypt_message(transport, assembler, client_tls, handshake_state)
        .expect("read channel open request");
    assert_eq!(message.channel_id, channel_id);
    assert_eq!(message.message_type, MessageType::Control);

    let actions = machine
        .advance(ChannelOpenEvent::InboundControl(&message.payload))
        .expect("advance channel open");
    assert_eq!(actions.len(), 1);
    let ChannelOpenAction::SendControl(response) = &actions[0];
    let payload = response
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode open response");
    let ciphertext = client_tls
        .encrypt_application_data(&payload)
        .expect("client encrypt open response");
    let frame = encode_frame(
        FrameHeader {
            channel_id,
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

    let (received_channel_id, message_type, plaintext) = phone_receives_encrypted(phone, phone_tls);
    assert_eq!(received_channel_id, channel_id);
    assert_eq!(message_type, MessageType::Control);
    let decoded =
        ControlMessage::decode(&plaintext, DEFAULT_MAX_CONTROL_BODY_SIZE).expect("decode");
    assert_eq!(decoded.id, ControlMessageId::ChannelOpenResponse);
    assert_eq!(decoded.body, vec![0x08, 0x00]); // STATUS_SUCCESS
}

// One continuous linear trace through all five phases (service-discovery
// response, video open, input open, video setup, video start) is the point
// of this test — splitting it further would thread 6+ mutable references
// through helper functions without adding clarity. Matches the existing
// `#[allow(clippy::too_many_lines)]` precedent in `main.rs` for the same
// kind of inherently sequential code.
#[allow(clippy::too_many_lines)]
#[test]
fn drives_service_discovery_response_and_both_channels_to_start() {
    let (mut transport, phone, mut handshake, mut client_tls, mut phone_tls, mut assembler) =
        establish_tls_session(3);

    // Phone sends ServiceDiscoveryRequest as real TLS-encrypted application
    // data, exactly like encrypted_service_discovery.rs.
    let request = ControlMessage {
        id: ControlMessageId::ServiceDiscoveryRequest,
        body: vec![0x0a, 0x00],
    };
    let plaintext = request
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode request");
    phone_sends_encrypted(&phone, &mut phone_tls, 0, MessageType::Specific, &plaintext);

    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read service discovery request");
    let actions = handshake
        .advance(HandshakeEvent::InboundControl(&message.payload))
        .expect("advance service discovery request");
    assert!(matches!(
        actions.as_slice(),
        [HandshakeAction::ServiceDiscoveryRequest(_)]
    ));

    // SUT builds and sends ServiceDiscoveryResponse — mirrors
    // auth_discovery_probe.rs::send_service_discovery_response.
    let catalogue = ServiceCatalogue::build(
        &[
            ServiceCandidate {
                channel_id: VIDEO_CHANNEL_ID,
                kind: ServiceKind::Video,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: INPUT_CHANNEL_ID,
                kind: ServiceKind::Input,
                availability: ServiceAvailability::Ready,
            },
        ],
        DEFAULT_MAX_SERVICE_CANDIDATES,
    )
    .expect("catalogue");
    let capabilities = ServiceCapabilities {
        video: Some(vec![VideoCapability {
            resolution: VideoCodecResolution::Video800x480,
            frame_rate: VideoFrameRate::Fps30,
            codec: VideoCodecType::H264,
            pixel_aspect_ratio_e4: None,
            ui_config: None,
        }]),
        touch: Some(TouchCapability {
            width: 800,
            height: 480,
            touch_type: TouchScreenType::Capacitive,
        }),
        media_audio: None,
        system_audio: None,
        speech_audio: None,
        bluetooth: None,
        microphone: None,
        sensors: None,
        head_unit_info: None,
        ping_configuration: None,
    };
    let response =
        encode_service_discovery_response(&catalogue, &capabilities).expect("encode response");
    let response_payload = response
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode response envelope");
    let ciphertext = client_tls
        .encrypt_application_data(&response_payload)
        .expect("client encrypt response");
    let frame = encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode response frame");
    transport.send_all(&frame).expect("send response");

    let (channel_id, message_type, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    assert_eq!(channel_id, 0);
    assert_eq!(message_type, MessageType::Specific);
    let decoded_response =
        ControlMessage::decode(&plaintext, DEFAULT_MAX_CONTROL_BODY_SIZE).expect("decode response");
    assert_eq!(
        decoded_response.id,
        ControlMessageId::ServiceDiscoveryResponse
    );

    // --- Video channel opens ---
    let mut video_open = ChannelOpenStateMachine::new(VIDEO_CHANNEL_ID);
    open_channel(
        &mut transport,
        &phone,
        &mut phone_tls,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
        VIDEO_CHANNEL_ID,
        &mut video_open,
    );

    // --- Input channel opens, interleaved before video's Setup/Config ---
    let mut input_open = ChannelOpenStateMachine::new(INPUT_CHANNEL_ID);
    open_channel(
        &mut transport,
        &phone,
        &mut phone_tls,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
        INPUT_CHANNEL_ID,
        &mut input_open,
    );

    // --- Video channel: Setup -> Config ---
    let mut video_setup = VideoSetupStateMachine::new();
    let setup_payload = setup_message()
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode setup");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        VIDEO_CHANNEL_ID,
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
    assert_eq!(message.message_type, MessageType::Specific);
    let actions = video_setup
        .advance(VideoSetupEvent::InboundMedia(&message.payload))
        .expect("advance setup");
    assert_eq!(actions.len(), 3);
    let VideoSetupAction::SetupRequested { .. } = &actions[0] else {
        panic!("expected SetupRequested action, got {actions:?}");
    };
    let VideoSetupAction::SendMedia(config) = &actions[1] else {
        panic!("expected SendMedia action, got {actions:?}");
    };
    assert_eq!(config.id, MediaMessageId::Config);
    let config_payload = config
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode config");
    let ciphertext = client_tls
        .encrypt_application_data(&config_payload)
        .expect("client encrypt config");
    let frame = encode_frame(
        FrameHeader {
            channel_id: VIDEO_CHANNEL_ID,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode config frame");
    transport.send_all(&frame).expect("send config");

    let (channel_id, message_type, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    assert_eq!(channel_id, VIDEO_CHANNEL_ID);
    assert_eq!(message_type, MessageType::Specific);
    let decoded_config = MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("decode config");
    assert_eq!(decoded_config.id, MediaMessageId::Config);
    assert_eq!(
        decoded_config.body,
        vec![0x08, 0x02, 0x10, 0x01, 0x18, 0x00]
    );

    // --- Video channel: unsolicited VideoFocusNotification, sent right after Config ---
    let VideoSetupAction::SendMedia(video_focus) = &actions[2] else {
        panic!("expected SendMedia action, got {actions:?}");
    };
    assert_eq!(video_focus.id, MediaMessageId::VideoFocusNotification);
    let video_focus_payload = video_focus
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode video focus notification");
    let ciphertext = client_tls
        .encrypt_application_data(&video_focus_payload)
        .expect("client encrypt video focus notification");
    let frame = encode_frame(
        FrameHeader {
            channel_id: VIDEO_CHANNEL_ID,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode video focus notification frame");
    transport
        .send_all(&frame)
        .expect("send video focus notification");

    let (channel_id, message_type, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    assert_eq!(channel_id, VIDEO_CHANNEL_ID);
    assert_eq!(message_type, MessageType::Specific);
    let decoded_video_focus = MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("decode video focus notification");
    assert_eq!(
        decoded_video_focus.id,
        MediaMessageId::VideoFocusNotification
    );
    assert_eq!(decoded_video_focus.body, vec![0x08, 0x01]);

    // --- Video channel: Start -> Ready (this is where the whole increment stops) ---
    let start_payload = start_message(7)
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode start");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        VIDEO_CHANNEL_ID,
        MessageType::Specific,
        &start_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read start");
    let actions = video_setup
        .advance(VideoSetupEvent::InboundMedia(&message.payload))
        .expect("advance start");
    assert_eq!(
        actions,
        vec![VideoSetupAction::Ready {
            session_id: 7,
            configuration_index: 0,
        }]
    );

    // --- Video channel: Ready stays Ready and decodes real media data ---
    let mut data_body = 99_u64.to_be_bytes().to_vec();
    data_body.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let data_payload = MediaMessage {
        id: MediaMessageId::Data,
        body: data_body,
    }
    .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
    .expect("encode data");
    phone_sends_encrypted(
        &phone,
        &mut phone_tls,
        VIDEO_CHANNEL_ID,
        MessageType::Specific,
        &data_payload,
    );
    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read data");
    let actions = video_setup
        .advance(VideoSetupEvent::InboundMedia(&message.payload))
        .expect("advance data");
    assert_eq!(actions.len(), 2);
    assert_eq!(
        actions[0],
        VideoSetupAction::MediaDataReceived {
            timestamp: 99,
            payload: vec![0xde, 0xad, 0xbe, 0xef],
        }
    );
    let VideoSetupAction::SendMedia(ack) = &actions[1] else {
        panic!("expected SendMedia");
    };
    assert_eq!(ack.id, MediaMessageId::Ack);
    assert_eq!(ack.body, vec![0x08, 0x07, 0x10, 0x01]);

    // --- Send the Ack and confirm the phone receives it, proving the flow-control reply end to end ---
    let ack_payload = ack
        .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
        .expect("encode ack");
    let ciphertext = client_tls
        .encrypt_application_data(&ack_payload)
        .expect("client encrypt ack");
    let frame = encode_frame(
        FrameHeader {
            channel_id: VIDEO_CHANNEL_ID,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &ciphertext,
        limits(),
    )
    .expect("encode ack frame");
    transport.send_all(&frame).expect("send ack");

    let (channel_id, message_type, plaintext) = phone_receives_encrypted(&phone, &mut phone_tls);
    assert_eq!(channel_id, VIDEO_CHANNEL_ID);
    assert_eq!(message_type, MessageType::Specific);
    let decoded_ack =
        MediaMessage::decode(&plaintext, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE).expect("decode ack");
    assert_eq!(decoded_ack.id, MediaMessageId::Ack);
    assert_eq!(decoded_ack.body, vec![0x08, 0x07, 0x10, 0x01]);
}
