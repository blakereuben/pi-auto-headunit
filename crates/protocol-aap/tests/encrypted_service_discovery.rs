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
//!
//! The shared real-TLS harness lives in `common/mod.rs`, also used by
//! `full_channel_setup.rs`.

mod common;

use common::{establish_tls_session, limits, push_decoded_frame, read_and_decrypt_message};
use protocol_aap::{
    ControlMessage, ControlMessageId, DEFAULT_MAX_CONTROL_BODY_SIZE, DecodedFrame, Encryption,
    FrameHeader, FrameType, HandshakeAction, HandshakeEvent, HandshakeState, MessageAssembler,
    MessageType, ServiceDiscoveryRequestSummary, encode_frame,
};
use security_openssl::{OpenSslTlsClient, generate_ephemeral_credentials};

#[test]
fn reassembles_a_single_frame_encrypted_service_discovery_request() {
    let (mut transport, phone, mut handshake, mut client_tls, mut phone_tls, mut assembler) =
        establish_tls_session(1);

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
        establish_tls_session(1);

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
        establish_tls_session(1);

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
