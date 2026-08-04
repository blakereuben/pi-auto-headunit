use protocol_aap::{
    AASDK_MAX_FRAME_PAYLOAD_SIZE, ControlMessage, Encryption, FrameError, FrameHeader, FrameType,
    HandshakeAction, HandshakeEvent, HandshakeStateMachine, MessageAssembler, MessageType,
    ProtocolLimits, TlsClient, decode_frame, encode_frame,
};
use security_openssl::{OpenSslTlsClient, TlsVersionPolicy, generate_ephemeral_credentials};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use transport_api::{SessionTransport, TransportError};

use crate::CliError;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ACCUMULATED_BYTES: usize = 64 * 1024;

pub fn run<T: SessionTransport>(
    transport: &mut T,
    tls12_compatibility: bool,
) -> Result<(), CliError> {
    println!("probe_scope=version_and_tls_only");
    println!("probe_credentials=temporary_project_generated");
    println!(
        "probe_tls_policy={}",
        if tls12_compatibility {
            "tls12_compat"
        } else {
            "system_default"
        }
    );
    println!("probe_payload_logging=disabled");

    let credentials =
        generate_ephemeral_credentials().map_err(|error| CliError::Protocol(error.to_string()))?;
    let mut tls = OpenSslTlsClient::from_pem_with_policy(
        &credentials.certificate_pem,
        &credentials.private_key_pem,
        64 * 1024,
        if tls12_compatibility {
            TlsVersionPolicy::Tls12Only
        } else {
            TlsVersionPolicy::SystemDefault
        },
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    drop(credentials);

    let limits = ProtocolLimits::default();
    let mut handshake = HandshakeStateMachine::default();
    let mut actions: VecDeque<_> = handshake
        .advance(HandshakeEvent::Start)
        .map_err(|error| CliError::Protocol(error.to_string()))?
        .into();
    process_actions(&mut actions, &mut handshake, &mut tls, transport, limits)?;
    println!("probe_state=version_request_sent");

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut received = Vec::new();
    let mut read_buffer = vec![0_u8; AASDK_MAX_FRAME_PAYLOAD_SIZE + 8];
    let mut assembler =
        MessageAssembler::new(1).map_err(|error| CliError::Protocol(error.to_string()))?;

    while Instant::now() < deadline {
        let size = match transport.receive(&mut read_buffer) {
            Ok(size) => size,
            Err(TransportError::TimedOut) => continue,
            Err(error) => return Err(CliError::Transport(error)),
        };
        if received.len() + size > MAX_ACCUMULATED_BYTES {
            return Err(CliError::Protocol(
                "incoming frame buffer exceeded the probe limit".into(),
            ));
        }
        received.extend_from_slice(&read_buffer[..size]);

        loop {
            let frame = match decode_frame(&received, limits) {
                Ok(frame) => frame,
                Err(FrameError::Incomplete { .. }) => break,
                Err(error) => return Err(CliError::Protocol(error.to_string())),
            };
            let consumed = frame.consumed;
            let message = assembler
                .push(frame)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            received.drain(..consumed);
            let Some(message) = message else {
                continue;
            };
            if message.channel_id != 0
                || message.encryption != Encryption::Plain
                || message.message_type != MessageType::Specific
            {
                return Err(CliError::Protocol(
                    "unexpected message metadata during TLS probe".into(),
                ));
            }

            let mut actions: VecDeque<_> = handshake
                .advance(HandshakeEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?
                .into();
            if process_actions(&mut actions, &mut handshake, &mut tls, transport, limits)? {
                println!("probe_result=tls_handshake_complete");
                println!("probe_stop=before_authentication_and_service_discovery");
                return Ok(());
            }
        }
    }

    println!("probe_tls_state={}", tls.handshake_state());
    Err(CliError::Protocol(
        "TLS probe timed out before handshake completion".into(),
    ))
}

fn process_actions<T: SessionTransport>(
    actions: &mut VecDeque<HandshakeAction>,
    handshake: &mut HandshakeStateMachine,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<bool, CliError> {
    while let Some(action) = actions.pop_front() {
        match action {
            HandshakeAction::SendControl(message) => {
                send_control(transport, &message, limits)?;
            }
            HandshakeAction::StartTlsClient => {
                println!("probe_state=version_accepted");
                let progress = tls
                    .start()
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                if finish_or_queue_tls(&progress, actions, handshake, transport, limits)? {
                    return Ok(true);
                }
            }
            HandshakeAction::FeedTls(inbound) => {
                println!("probe_state=tls_peer_data_received");
                let progress = tls
                    .feed(&inbound)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                if finish_or_queue_tls(&progress, actions, handshake, transport, limits)? {
                    return Ok(true);
                }
            }
            HandshakeAction::ServiceDiscoveryRequest(_) => {
                return Err(CliError::Protocol(
                    "probe crossed its service-discovery stop boundary".into(),
                ));
            }
        }
    }
    Ok(false)
}

fn finish_or_queue_tls<T: SessionTransport>(
    progress: &protocol_aap::TlsProgress,
    actions: &mut VecDeque<HandshakeAction>,
    handshake: &mut HandshakeStateMachine,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<bool, CliError> {
    if progress.complete {
        if !progress.outbound.is_empty() {
            send_control(
                transport,
                &ControlMessage::encapsulated_tls(&progress.outbound),
                limits,
            )?;
        }
        return Ok(true);
    }
    actions.extend(
        handshake
            .advance(HandshakeEvent::TlsProgress {
                outbound: &progress.outbound,
                complete: false,
            })
            .map_err(|error| CliError::Protocol(error.to_string()))?,
    );
    Ok(false)
}

fn send_control<T: SessionTransport>(
    transport: &mut T,
    message: &ControlMessage,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let payload = message
        .encode(protocol_aap::DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let frame = encode_frame(
        FrameHeader {
            channel_id: 0,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Plain,
            message_type: MessageType::Specific,
        },
        None,
        &payload,
        limits,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    transport.send_all(&frame).map_err(CliError::Transport)
}
