//! Proves the existing frame codec, message assembler, and handshake state
//! machine already reach `ServiceDiscoveryReceived` — with only a bounded
//! summary, no response built — when driven over a real `SessionTransport`
//! against a scripted, deterministic fake phone.
//!
//! No real TLS, USB, or network is involved: TLS bytes are opaque scripted
//! placeholders, matching the pattern already used by
//! `control::tests::reaches_service_discovery_with_fake_tls`. This test adds
//! the transport/frame layer underneath that existing state-machine-only
//! proof. It does not touch the frozen `credential-probe` path
//! (`apps/aa-headunit-diagnostics/src/live_probe.rs`), which stops before
//! authentication completion and must stay unchanged.

use protocol_aap::{
    ControlMessage, ControlMessageId, DEFAULT_MAX_CONTROL_BODY_SIZE, Encryption, FrameError,
    FrameHeader, FrameType, HandshakeAction, HandshakeEvent, HandshakeState, HandshakeStateMachine,
    Message, MessageAssembler, MessageType, ProtocolLimits, ServiceDiscoveryRequestSummary,
    decode_frame, encode_frame,
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

/// Send `message` on the SUT's transport handle, as the probe code would.
fn send_control(transport: &mut impl SessionTransport, message: &ControlMessage) {
    transport.send_all(&as_frame(message)).expect("send frame");
}

/// Queue `message` on the fake phone's side, as if the phone sent it to us.
fn phone_sends(peer: &fake::Peer, message: &ControlMessage) {
    peer.push_inbound(&as_frame(message)).expect("push inbound");
}

/// Decode exactly one control message the SUT has sent since the last drain.
fn phone_receives_one(peer: &fake::Peer) -> ControlMessage {
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

/// Every `SendControl` action, sent in order, each individually observed and
/// confirmed by the fake phone before the next one is sent.
fn send_and_observe(
    transport: &mut impl SessionTransport,
    peer: &fake::Peer,
    actions: &[HandshakeAction],
) {
    for action in actions {
        if let HandshakeAction::SendControl(message) = action {
            send_control(transport, message);
            let observed = phone_receives_one(peer);
            assert_eq!(observed.id, message.id);
        }
    }
}

/// Block (busy-poll; bounded by the fake transport already holding the
/// bytes) until one full protocol message has been assembled.
fn read_one_message(
    transport: &mut impl SessionTransport,
    assembler: &mut MessageAssembler,
) -> Message {
    let mut buffer = vec![0_u8; protocol_aap::AASDK_MAX_FRAME_PAYLOAD_SIZE + 8];
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

#[test]
fn reaches_service_discovery_summary_over_fake_transport_and_stops() {
    let (mut transport, phone) = fake::pair(64 * 1024);
    let mut handshake = HandshakeStateMachine::default();
    let mut assembler = MessageAssembler::new(1).expect("assembler");

    // 1. Version request/response.
    let actions = handshake.advance(HandshakeEvent::Start).expect("start");
    send_and_observe(&mut transport, &phone, &actions);

    phone_sends(
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
    let received = read_one_message(&mut transport, &mut assembler);
    let actions = handshake
        .advance(HandshakeEvent::InboundControl(&received.payload))
        .expect("version accepted");
    assert_eq!(actions, vec![HandshakeAction::StartTlsClient]);
    assert_eq!(handshake.state(), HandshakeState::TlsHandshake);

    // 2. Fake TLS handshake — opaque scripted bytes, no real crypto, matching
    // the existing control::tests::reaches_service_discovery_with_fake_tls
    // pattern.
    let actions = handshake
        .advance(HandshakeEvent::TlsProgress {
            outbound: b"client hello",
            complete: false,
        })
        .expect("client hello");
    send_and_observe(&mut transport, &phone, &actions);

    phone_sends(&phone, &ControlMessage::encapsulated_tls(b"server hello"));
    let received = read_one_message(&mut transport, &mut assembler);
    let actions = handshake
        .advance(HandshakeEvent::InboundControl(&received.payload))
        .expect("server hello");
    assert_eq!(
        actions,
        vec![HandshakeAction::FeedTls(b"server hello".to_vec())]
    );

    let actions = handshake
        .advance(HandshakeEvent::TlsProgress {
            outbound: b"client finished",
            complete: true,
        })
        .expect("tls complete");
    assert_eq!(
        actions.len(),
        2,
        "expect the final TLS chunk and AuthComplete"
    );
    send_and_observe(&mut transport, &phone, &actions);
    assert_eq!(handshake.state(), HandshakeState::AwaitingServiceDiscovery);

    // 3. Phone sends a synthetic (freshly generated, not-a-real-device)
    // ServiceDiscoveryRequest — same bytes already used by the existing
    // state-machine-only fake-TLS test.
    phone_sends(
        &phone,
        &ControlMessage {
            id: ControlMessageId::ServiceDiscoveryRequest,
            body: vec![0x0a, 0x00],
        },
    );
    let received = read_one_message(&mut transport, &mut assembler);
    let actions = handshake
        .advance(HandshakeEvent::InboundControl(&received.payload))
        .expect("service discovery request");

    // The probe boundary: a bounded summary only. No response is built or
    // sent — that stays gated until the response schema is separately
    // mapped and reviewed (see MILESTONE_CHECKLIST.md M2).
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
    assert!(
        phone.outbound_is_empty(),
        "no service-discovery response should have been sent"
    );
}
