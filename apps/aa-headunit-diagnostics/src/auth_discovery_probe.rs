//! Gated authentication/service-discovery probe.
//!
//! Reuses the same frame codec, message assembler, `HandshakeStateMachine`,
//! and `OpenSslTlsClient` wiring as the frozen `credential-probe`
//! (`live_probe.rs`, unmodified). The only behavioural difference is that
//! this probe lets `HandshakeStateMachine::advance` run one step further:
//! where `credential-probe` short-circuits before ever asking the state
//! machine to send `AuthComplete`, this probe feeds the TLS-complete event
//! through normally, which reaches the phone's `ServiceDiscoveryRequest` and
//! yields exactly one bounded, privacy-preserving summary
//! (`ServiceDiscoveryRequestSummary`, byte counts only — see
//! `protocol_aap::service_discovery`). It stops immediately after that: no
//! `ServiceDiscoveryResponse` is built or sent, and no media setup is
//! attempted, matching `crates/protocol-aap/tests/fake_phone_transport.rs`.

use credential_store::CredentialMaterial;
use protocol_aap::{
    AASDK_MAX_FRAME_PAYLOAD_SIZE, ControlMessage, Encryption, FrameError, FrameHeader, FrameType,
    HandshakeAction, HandshakeEvent, HandshakeStateMachine, MessageAssembler, MessageType,
    ProtocolLimits, ServiceDiscoveryRequestSummary, TlsClient, TlsProgress, decode_frame,
    encode_frame,
};
use security_openssl::{OpenSslTlsClient, TlsVersionPolicy};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use transport_api::{SessionTransport, TransportError};

use crate::CliError;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ACCUMULATED_BYTES: usize = 64 * 1024;

pub fn run<T: SessionTransport>(
    transport: &mut T,
    tls12_compatibility: bool,
    credentials: CredentialMaterial,
) -> Result<(), CliError> {
    println!("probe_scope=version_tls_auth_and_service_discovery_summary");
    println!("probe_credentials=user_supplied_runtime");
    println!(
        "probe_tls_policy={}",
        if tls12_compatibility {
            "tls12_compat"
        } else {
            "system_default"
        }
    );
    println!("probe_payload_logging=disabled");

    let mut tls = OpenSslTlsClient::from_pem_with_policy(
        credentials.certificate_pem(),
        credentials.private_key_pem(),
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
                    "unexpected message metadata during auth/service-discovery probe".into(),
                ));
            }

            let mut actions: VecDeque<_> = handshake
                .advance(HandshakeEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?
                .into();
            if let Some(summary) =
                process_actions(&mut actions, &mut handshake, &mut tls, transport, limits)?
            {
                print_summary(&summary);
                println!("probe_result=service_discovery_summary_received");
                println!("probe_stop=before_service_discovery_response_and_media_setup");
                return Ok(());
            }
        }
    }

    println!("probe_tls_state={}", tls.handshake_state());
    Err(CliError::Protocol(
        "auth/service-discovery probe timed out before a service-discovery summary".into(),
    ))
}

fn print_summary(summary: &ServiceDiscoveryRequestSummary) {
    fn bytes(field: Option<usize>) -> String {
        field.map_or_else(|| "absent".to_string(), |size| size.to_string())
    }
    println!(
        "service_discovery_small_icon_bytes={}",
        bytes(summary.small_icon_bytes)
    );
    println!(
        "service_discovery_medium_icon_bytes={}",
        bytes(summary.medium_icon_bytes)
    );
    println!(
        "service_discovery_large_icon_bytes={}",
        bytes(summary.large_icon_bytes)
    );
    println!(
        "service_discovery_device_name_bytes={}",
        bytes(summary.device_name_bytes)
    );
    println!(
        "service_discovery_device_brand_bytes={}",
        bytes(summary.device_brand_bytes)
    );
    println!(
        "service_discovery_phone_info_bytes={}",
        bytes(summary.phone_info_bytes)
    );
    println!(
        "service_discovery_unknown_fields={}",
        summary.unknown_fields
    );
}

/// Drains queued handshake actions, sending control messages and driving TLS
/// as needed. Unlike `live_probe`'s equivalent, TLS completion is fed back
/// into `HandshakeStateMachine::advance` rather than short-circuited, so
/// `AuthComplete` is sent and the machine can reach `ServiceDiscoveryRequest`.
/// Returns the bounded summary the instant one is produced; nothing further
/// is read or sent afterward.
fn process_actions<T: SessionTransport>(
    actions: &mut VecDeque<HandshakeAction>,
    handshake: &mut HandshakeStateMachine,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<Option<ServiceDiscoveryRequestSummary>, CliError> {
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
                queue_tls_progress(&progress, actions, handshake)?;
            }
            HandshakeAction::FeedTls(inbound) => {
                println!("probe_state=tls_peer_data_received");
                let progress = tls
                    .feed(&inbound)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                queue_tls_progress(&progress, actions, handshake)?;
            }
            HandshakeAction::ServiceDiscoveryRequest(summary) => {
                return Ok(Some(summary));
            }
        }
    }
    Ok(None)
}

fn queue_tls_progress(
    progress: &TlsProgress,
    actions: &mut VecDeque<HandshakeAction>,
    handshake: &mut HandshakeStateMachine,
) -> Result<(), CliError> {
    if progress.complete {
        println!("probe_state=tls_handshake_complete");
    }
    actions.extend(
        handshake
            .advance(HandshakeEvent::TlsProgress {
                outbound: &progress.outbound,
                complete: progress.complete,
            })
            .map_err(|error| CliError::Protocol(error.to_string()))?,
    );
    Ok(())
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
