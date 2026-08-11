//! Proves per-frame decrypt-before-reassembly against a **real** TLS 1.2
//! session (not opaque scripted bytes): the SUT's `OpenSslTlsClient`
//! completes a genuine client-role handshake against a server-role
//! `security_openssl::TestServerTls` standing in for the phone, then the
//! phone sends a `ServiceDiscoveryRequest` as TLS-encrypted, possibly
//! fragmented AAP application data.
//!
//! Complements `fake_phone_transport.rs`, which proves the same control-
//! channel/reassembly logic against opaque scripted TLS bytes with no real
//! crypto and stops before any encrypted application data exists. This file
//! is the missing piece: it exercises the actual decrypt integration
//! described in `docs/protocol/aasdk-adoption.md`'s "Encrypted-message
//! framing" note (decrypt each frame's ciphertext before it reaches bounded
//! reassembly; the declared total is plaintext-domain). It mirrors, but
//! does not modify or depend on, the gated
//! `apps/aa-headunit-diagnostics/src/auth_discovery_probe.rs` probe; the
//! frozen `credential-probe` (`live_probe.rs`) is untouched.

use protocol_aap::{
    AASDK_MAX_FRAME_PAYLOAD_SIZE, ControlMessage, ControlMessageId, DEFAULT_MAX_CONTROL_BODY_SIZE,
    DecodedFrame, Encryption, FrameError, FrameHeader, FrameType, HandshakeAction, HandshakeEvent,
    HandshakeState, HandshakeStateMachine, Message, MessageAssembler, MessageType, ProtocolLimits,
    ServiceDiscoveryRequestSummary, TlsClient, decode_frame, encode_frame,
};
use security_openssl::{
    OpenSslTlsClient, TestServerTls, TlsVersionPolicy, generate_ephemeral_credentials,
};
use transport_api::{SessionTransport, TransportError, fake};

fn limits() -> ProtocolLimits {
    ProtocolLimits::default()
}

fn as_frame(message: &ControlMessage) -> Vec<u8> {
    let payload = message
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode control message");
    encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Plain,
            message_type: MessageType::Specific,
        },
        None,
        &payload,
        limits(),
    )
    .expect("encode frame")
}

fn send_control(transport: &mut impl SessionTransport, message: &ControlMessage) {
    transport.send_all(&as_frame(message)).expect("send frame");
}

fn phone_sends_control(peer: &fake::Peer, message: &ControlMessage) {
    peer.push_inbound(&as_frame(message)).expect("push inbound");
}

fn phone_receives_control(peer: &fake::Peer) -> ControlMessage {
    let bytes = peer.drain_outbound();
    let decoded = decode_frame(&bytes, limits()).expect("decode frame");
    assert_eq!(
        decoded.consumed,
        bytes.len(),
        "expected exactly one frame in the outbound buffer"
    );
    ControlMessage::decode(decoded.payload, DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("decode control message")
}

/// Sends every `SendControl` action and returns what the phone observed for
/// each, in order.
fn send_and_capture(
    transport: &mut impl SessionTransport,
    peer: &fake::Peer,
    actions: &[HandshakeAction],
) -> Vec<ControlMessage> {
    let mut observed = Vec::new();
    for action in actions {
        if let HandshakeAction::SendControl(message) = action {
            send_control(transport, message);
            observed.push(phone_receives_control(peer));
        }
    }
    observed
}

fn read_plain_message(
    transport: &mut impl SessionTransport,
    assembler: &mut MessageAssembler,
) -> Message {
    let mut buffer = vec![0_u8; AASDK_MAX_FRAME_PAYLOAD_SIZE + 8];
    let mut received = Vec::new();
    loop {
        match transport.receive(&mut buffer) {
            Ok(size) => received.extend_from_slice(&buffer[..size]),
            Err(TransportError::TimedOut) => continue,
            Err(error) => panic!("unexpected transport error: {error}"),
        }
        loop {
            let frame = match decode_frame(&received, limits()) {
                Ok(frame) => frame,
                Err(FrameError::Incomplete { .. }) => break,
                Err(error) => panic!("frame decode error: {error}"),
            };
            let consumed = frame.consumed;
            let pushed = assembler.push(frame).expect("assemble frame");
            received.drain(..consumed);
            if let Some(message) = pushed {
                return message;
            }
        }
    }
}

/// Mirrors `auth_discovery_probe.rs`'s `push_decoded_frame`: decrypts an
/// `Encrypted` frame's payload before handing it to the assembler, and
/// rejects encrypted frames arriving before TLS has completed.
fn push_decoded_frame(
    frame: DecodedFrame<'_>,
    assembler: &mut MessageAssembler,
    tls: &mut OpenSslTlsClient,
    handshake_state: HandshakeState,
) -> Result<Option<Message>, String> {
    match frame.header.encryption {
        Encryption::Plain => assembler.push(frame).map_err(|error| error.to_string()),
        Encryption::Encrypted => {
            if !matches!(
                handshake_state,
                HandshakeState::AwaitingServiceDiscovery | HandshakeState::ServiceDiscoveryReceived
            ) {
                return Err("encrypted frame received before TLS handshake completed".into());
            }
            let plaintext = tls
                .decrypt_application_data(frame.payload)
                .map_err(|error| error.to_string())?;
            let decrypted_frame = DecodedFrame {
                header: frame.header,
                total_message_size: frame.total_message_size,
                payload: &plaintext,
                consumed: frame.consumed,
            };
            assembler
                .push(decrypted_frame)
                .map_err(|error| error.to_string())
        }
    }
}

fn read_and_decrypt_message(
    transport: &mut impl SessionTransport,
    assembler: &mut MessageAssembler,
    tls: &mut OpenSslTlsClient,
    handshake_state: HandshakeState,
) -> Result<Message, String> {
    let mut buffer = vec![0_u8; AASDK_MAX_FRAME_PAYLOAD_SIZE + 8];
    let mut received = Vec::new();
    loop {
        match transport.receive(&mut buffer) {
            Ok(size) => received.extend_from_slice(&buffer[..size]),
            Err(TransportError::TimedOut) => continue,
            Err(error) => panic!("unexpected transport error: {error}"),
        }
        loop {
            let frame = match decode_frame(&received, limits()) {
                Ok(frame) => frame,
                Err(FrameError::Incomplete { .. }) => break,
                Err(error) => panic!("frame decode error: {error}"),
            };
            let consumed = frame.consumed;
            let pushed = push_decoded_frame(frame, assembler, tls, handshake_state)?;
            received.drain(..consumed);
            if let Some(message) = pushed {
                return Ok(message);
            }
        }
    }
}

/// Runs version negotiation and a real TLS 1.2 handshake (client-role
/// `OpenSslTlsClient` vs. server-role `TestServerTls`) over the fake
/// transport, exactly as the real handshake state machine would drive it.
/// TLS 1.2 (rather than the system default) sidesteps TLS 1.3's
/// post-handshake session-ticket flight, which is immaterial to what this
/// test proves and would otherwise need its own draining logic.
fn establish_tls_session() -> (
    fake::Transport,
    fake::Peer,
    HandshakeStateMachine,
    OpenSslTlsClient,
    TestServerTls,
    MessageAssembler,
) {
    let (mut transport, phone) = fake::pair(64 * 1024);
    let mut handshake = HandshakeStateMachine::default();
    let mut assembler = MessageAssembler::new(1).expect("assembler");

    let client_credentials = generate_ephemeral_credentials().expect("client credentials");
    let server_credentials = generate_ephemeral_credentials().expect("server credentials");
    let mut client_tls = OpenSslTlsClient::from_pem_with_policy(
        &client_credentials.certificate_pem,
        &client_credentials.private_key_pem,
        64 * 1024,
        TlsVersionPolicy::Tls12Only,
    )
    .expect("client tls");
    let mut phone_tls = TestServerTls::from_pem(
        &server_credentials.certificate_pem,
        &server_credentials.private_key_pem,
        &client_credentials.certificate_pem,
        64 * 1024,
    )
    .expect("phone tls");

    // 1. Version request/response (plain), matching fake_phone_transport.rs.
    let actions = handshake.advance(HandshakeEvent::Start).expect("start");
    let observed = send_and_capture(&mut transport, &phone, &actions);
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].id, ControlMessageId::VersionRequest);

    phone_sends_control(
        &phone,
        &ControlMessage {
            id: ControlMessageId::VersionResponse,
            body: {
                let mut body = Vec::new();
                body.extend_from_slice(&1_u16.to_be_bytes());
                body.extend_from_slice(&6_u16.to_be_bytes());
                body.extend_from_slice(&0_i16.to_be_bytes());
                body
            },
        },
    );
    let received = read_plain_message(&mut transport, &mut assembler);
    let actions = handshake
        .advance(HandshakeEvent::InboundControl(&received.payload))
        .expect("version accepted");
    assert_eq!(actions, vec![HandshakeAction::StartTlsClient]);
    assert_eq!(handshake.state(), HandshakeState::TlsHandshake);

    // 2. Real TLS 1.2 handshake, driven over the AAP control channel.
    let mut client_progress = client_tls.start().expect("client hello");
    for _ in 0..16 {
        let actions = handshake
            .advance(HandshakeEvent::TlsProgress {
                outbound: &client_progress.outbound,
                complete: client_progress.complete,
            })
            .expect("advance tls progress");
        let observed = send_and_capture(&mut transport, &phone, &actions);

        let mut phone_replied = false;
        for message in &observed {
            if message.id == ControlMessageId::EncapsulatedTls {
                let phone_progress = phone_tls.accept(&message.body).expect("phone tls progress");
                if !phone_progress.outbound.is_empty() {
                    phone_sends_control(
                        &phone,
                        &ControlMessage::encapsulated_tls(&phone_progress.outbound),
                    );
                    phone_replied = true;
                }
            }
        }

        if client_progress.complete && phone_tls.is_complete() {
            break;
        }

        if phone_replied {
            let received = read_plain_message(&mut transport, &mut assembler);
            let feed_actions = handshake
                .advance(HandshakeEvent::InboundControl(&received.payload))
                .expect("advance inbound control");
            assert_eq!(feed_actions.len(), 1, "expected exactly one FeedTls action");
            let HandshakeAction::FeedTls(inbound) = &feed_actions[0] else {
                panic!("expected FeedTls action, got {feed_actions:?}");
            };
            client_progress = client_tls.feed(inbound).expect("client tls progress");
        }
    }

    assert!(
        client_progress.complete,
        "client TLS handshake did not complete"
    );
    assert!(
        phone_tls.is_complete(),
        "phone TLS handshake did not complete"
    );
    assert_eq!(handshake.state(), HandshakeState::AwaitingServiceDiscovery);

    (
        transport, phone, handshake, client_tls, phone_tls, assembler,
    )
}

#[test]
fn reassembles_a_single_frame_encrypted_service_discovery_request() {
    let (mut transport, phone, mut handshake, mut client_tls, mut phone_tls, mut assembler) =
        establish_tls_session();

    let request = ControlMessage {
        id: ControlMessageId::ServiceDiscoveryRequest,
        body: vec![0x0a, 0x00],
    };
    let plaintext = request
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode control message");
    let ciphertext = phone_tls
        .encrypt_application_data(&plaintext)
        .expect("phone encrypt");
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
    .expect("encode encrypted frame");
    phone.push_inbound(&frame).expect("push encrypted frame");

    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read and decrypt message");
    assert_eq!(message.channel_id, 0);
    assert_eq!(message.message_type, MessageType::Specific);

    let actions = handshake
        .advance(HandshakeEvent::InboundControl(&message.payload))
        .expect("advance service discovery request");
    assert_eq!(
        actions,
        vec![HandshakeAction::ServiceDiscoveryRequest(
            ServiceDiscoveryRequestSummary {
                small_icon_bytes: Some(0),
                ..ServiceDiscoveryRequestSummary::default()
            }
        )]
    );
    assert_eq!(handshake.state(), HandshakeState::ServiceDiscoveryReceived);
}

#[test]
fn reassembles_a_fragmented_encrypted_service_discovery_request() {
    let (mut transport, phone, handshake, mut client_tls, mut phone_tls, mut assembler) =
        establish_tls_session();

    let request = ControlMessage {
        id: ControlMessageId::ServiceDiscoveryRequest,
        body: vec![0x0a, 0x00],
    };
    let plaintext = request
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .expect("encode control message");
    assert!(plaintext.len() > 2, "test needs a splittable payload");
    let total_plaintext_len = plaintext.len();
    let (first_half, second_half) = plaintext.split_at(plaintext.len() / 2);

    let first_ciphertext = phone_tls
        .encrypt_application_data(first_half)
        .expect("phone encrypt first");
    let first_frame = encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::First,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        Some(total_plaintext_len),
        &first_ciphertext,
        limits(),
    )
    .expect("encode first frame");
    phone.push_inbound(&first_frame).expect("push first frame");

    let second_ciphertext = phone_tls
        .encrypt_application_data(second_half)
        .expect("phone encrypt second");
    let last_frame = encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Last,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &second_ciphertext,
        limits(),
    )
    .expect("encode last frame");
    phone.push_inbound(&last_frame).expect("push last frame");

    let message = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect("read and decrypt fragmented message");
    assert_eq!(message.payload, plaintext);
}

#[test]
fn rejects_encrypted_frame_before_tls_handshake_completes() {
    let credentials = generate_ephemeral_credentials().expect("credentials");
    let mut client_tls = OpenSslTlsClient::from_pem(
        &credentials.certificate_pem,
        &credentials.private_key_pem,
        64 * 1024,
    )
    .expect("client tls");
    let mut assembler = MessageAssembler::new(1).expect("assembler");

    let frame = DecodedFrame {
        header: FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        total_message_size: None,
        payload: &[0xAA; 16],
        consumed: 18,
    };

    let result = push_decoded_frame(
        frame,
        &mut assembler,
        &mut client_tls,
        HandshakeState::TlsHandshake,
    );
    assert!(
        result.is_err(),
        "an encrypted frame before TLS completion must be rejected, not buffered"
    );
}

#[test]
fn rejects_invalid_ciphertext_in_an_encrypted_frame() {
    let (mut transport, phone, handshake, mut client_tls, _phone_tls, mut assembler) =
        establish_tls_session();

    let garbage = vec![0xAA_u8; 32];
    let frame = encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type: MessageType::Specific,
        },
        None,
        &garbage,
        limits(),
    )
    .expect("encode frame");
    phone.push_inbound(&frame).expect("push garbage frame");

    let error = read_and_decrypt_message(
        &mut transport,
        &mut assembler,
        &mut client_tls,
        handshake.state(),
    )
    .expect_err("garbage ciphertext must be rejected");
    assert!(!error.is_empty());
}
