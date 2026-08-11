//! Shared real-TLS integration-test harness: a real client-role
//! `OpenSslTlsClient` completes a genuine handshake against a server-role
//! `security_openssl::TestServerTls` standing in for the phone, over
//! `transport_api::fake`. Extracted from `encrypted_service_discovery.rs` so
//! `full_channel_setup.rs` doesn't need a third copy of the same TLS/frame
//! plumbing.

use protocol_aap::{
    AASDK_MAX_FRAME_PAYLOAD_SIZE, ControlMessage, ControlMessageId, DEFAULT_MAX_CONTROL_BODY_SIZE,
    DecodedFrame, Encryption, FrameError, FrameHeader, FrameType, HandshakeAction, HandshakeEvent,
    HandshakeState, HandshakeStateMachine, Message, MessageAssembler, MessageType, ProtocolLimits,
    TlsClient, decode_frame, encode_frame,
};
use security_openssl::{
    OpenSslTlsClient, TestServerTls, TlsVersionPolicy, generate_ephemeral_credentials,
};
use transport_api::{SessionTransport, TransportError, fake};

pub fn limits() -> ProtocolLimits {
    ProtocolLimits::default()
}

pub fn as_frame(message: &ControlMessage) -> Vec<u8> {
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

pub fn send_control(transport: &mut impl SessionTransport, message: &ControlMessage) {
    transport.send_all(&as_frame(message)).expect("send frame");
}

pub fn phone_sends_control(peer: &fake::Peer, message: &ControlMessage) {
    peer.push_inbound(&as_frame(message)).expect("push inbound");
}

pub fn phone_receives_control(peer: &fake::Peer) -> ControlMessage {
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
pub fn send_and_capture(
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

pub fn read_plain_message(
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
pub fn push_decoded_frame(
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

pub fn read_and_decrypt_message(
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
/// post-handshake session-ticket flight, which is immaterial to what these
/// tests prove and would otherwise need its own draining logic.
///
/// `assembler_capacity` is the number of concurrently-fragmenting channels
/// the caller's `MessageAssembler` needs to support (1 for control-channel-
/// only tests, 3 once video/input channel traffic is also exercised).
pub fn establish_tls_session(
    assembler_capacity: usize,
) -> (
    fake::Transport,
    fake::Peer,
    HandshakeStateMachine,
    OpenSslTlsClient,
    TestServerTls,
    MessageAssembler,
) {
    let (mut transport, phone) = fake::pair(64 * 1024);
    let mut handshake = HandshakeStateMachine::default();
    let mut assembler = MessageAssembler::new(assembler_capacity).expect("assembler");

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
