//! Gated authentication/service-discovery/channel-setup probe.
//!
//! Reuses the same frame codec, message assembler, `HandshakeStateMachine`,
//! and `OpenSslTlsClient` wiring as the frozen `credential-probe`
//! (`live_probe.rs`, unmodified). Beyond that, this probe lets
//! `HandshakeStateMachine::advance` run through to
//! `ServiceDiscoveryRequest`, then goes further still: it builds and sends
//! `ServiceDiscoveryResponse` (advertising video, touch/input, and one
//! experimental media-audio channel — see
//! `protocol_aap::service_discovery_response`), then drives each channel's
//! `ChannelOpenRequest`/`ChannelOpenResponse` handshake
//! (`protocol_aap::channel_open`), then the video channel's
//! `Setup`→`Config`→`Start` handshake (`protocol_aap::video_setup`). It
//! stops the instant the video channel receives `Start` and the input
//! channel has opened — no `MEDIA_MESSAGE_DATA` byte is ever parsed, no
//! video decode/render/UI work happens here, and no other service kind is
//! advertised. See the channel-setup design record for the full scope
//! boundary and provenance trail.
//!
//! The media-audio channel (`AUDIO_CHANNEL_ID`) and the populated
//! `HeadUnitInfo` are both experiments toward the same real-phone finding:
//! Android Auto's "phone and car are running incompatible software"
//! (Error 2) appears immediately after the phone receives
//! `ServiceDiscoveryResponse`, before any `ChannelOpenRequest` arrives.
//! Adding an audio channel didn't change the outcome, and neither did
//! offering the phone's own reported protocol version (`1.7`, versus the
//! pinned source's `1.6`) instead of the pinned value — ruling out a
//! simple missing-service or version-number-mismatch cause (Android
//! Auto's version negotiation is designed to be backward-compatible).
//! `HeadUnitInfo` tests a different theory: the response never identifies
//! the head unit at all, which may fail an app-level check distinct from
//! the wire schema itself. The audio channel is driven only to
//! `ChannelOpenState::Open` — no audio `Setup`/`Config`/`Start` handshake
//! exists yet; that is separate follow-on scope once a hypothesis is
//! confirmed.
//!
//! Once TLS completes, a real phone sends `AuthComplete`/
//! `ServiceDiscoveryRequest` as TLS-encrypted application data at the AAP
//! frame level (the `Encrypted` flag), not as more `EncapsulatedTls`
//! control messages. Each encrypted frame's payload is decrypted with
//! `TlsClient::decrypt_application_data` before it reaches bounded message
//! reassembly, matching AASDK's proven per-frame decrypt-before-dispatch
//! behaviour (`docs/protocol/aasdk-adoption.md`); an encrypted frame
//! arriving before TLS completes is rejected outright, since decryption
//! isn't yet possible. Outbound, `ServiceDiscoveryResponse`,
//! `ChannelOpenResponse`, and `Config` are all sent TLS-encrypted — verified
//! directly against the pinned AASDK C++ source
//! (`ControlServiceChannel.cpp`, `VideoMediaSinkService.cpp`) rather than
//! assumed, since `VersionRequest`/`EncapsulatedTls`/`AuthComplete` are sent
//! *plain* by that same source despite also happening post-handshake for
//! `AuthComplete` — encryption is message-specific, not simply
//! before/after TLS completion.

use credential_store::CredentialMaterial;
use protocol_aap::{
    AASDK_MAX_FRAME_PAYLOAD_SIZE, AudioCapability, AudioStreamType, ChannelOpenAction,
    ChannelOpenEvent, ChannelOpenState, ChannelOpenStateMachine, ControlMessage,
    DEFAULT_MAX_CONTROL_BODY_SIZE, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE,
    DEFAULT_MAX_SERVICE_CANDIDATES, DecodedFrame, Encryption, FrameError, FrameHeader, FrameType,
    HandshakeAction, HandshakeEvent, HandshakeState, HandshakeStateMachine, HeadUnitInfo, Message,
    MessageAssembler, MessageType, ProtocolLimits, ServiceAvailability, ServiceCandidate,
    ServiceCapabilities, ServiceCatalogue, ServiceDiscoveryRequestSummary, ServiceKind, TlsClient,
    TlsProgress, TouchCapability, TouchScreenType, VideoCapability, VideoCodecResolution,
    VideoFrameRate, VideoSetupAction, VideoSetupEvent, VideoSetupStateMachine, decode_frame,
    encode_frame, encode_service_discovery_response,
};
use security_openssl::{OpenSslTlsClient, TlsVersionPolicy};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use transport_api::{SessionTransport, TransportError};

use crate::CliError;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ACCUMULATED_BYTES: usize = 64 * 1024;
/// Head-unit-assigned channel ids advertised in `ServiceDiscoveryResponse`.
/// These are this probe's own choice, not AASDK's internal `ChannelId`
/// numbering (which that fork's own source flags as a simplification, not
/// protocol-mandated).
const VIDEO_CHANNEL_ID: u8 = 1;
const INPUT_CHANNEL_ID: u8 = 2;
/// Experimental: advertised to test whether a real phone's "phone and car
/// are running incompatible software" (Error 2) rejection is caused by
/// advertising no audio service at all. Driven only to `ChannelOpenState::Open`
/// this pass — no audio Setup/Config/Start handshake exists yet.
const AUDIO_CHANNEL_ID: u8 = 3;
/// The Pi 5 reference display: the official 7-inch DSI touchscreen,
/// matching the 800x480/30fps baseline already selected in
/// `ARCHITECTURE.md`/M3.
const REFERENCE_DISPLAY_WIDTH: i32 = 800;
const REFERENCE_DISPLAY_HEIGHT: i32 = 480;

/// Per-channel progress for the video channel, driven once
/// `ServiceDiscoveryResponse` has been sent.
enum VideoChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    AwaitingSetup(VideoSetupStateMachine),
    Ready,
}

/// Per-channel progress for the input/touch channel. No setup handshake is
/// needed beyond channel-open — confirmed from `InputSourceService.cpp`:
/// touch/key events are head-unit→phone only.
enum InputChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    Open,
}

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
    // Control channel (0) + video channel + input channel can each
    // independently be mid-fragmentation once channel setup starts.
    let mut assembler =
        MessageAssembler::new(3).map_err(|error| CliError::Protocol(error.to_string()))?;

    let mut video_channel: Option<VideoChannel> = None;
    let mut input_channel: Option<InputChannel> = None;
    let mut audio_channel: Option<ChannelOpenStateMachine> = None;

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
            let message = push_decoded_frame(frame, &mut assembler, &mut tls, handshake.state())?;
            received.drain(..consumed);
            let Some(message) = message else {
                continue;
            };

            let done = handle_message(
                &message,
                &mut handshake,
                &mut video_channel,
                &mut input_channel,
                &mut audio_channel,
                &mut tls,
                transport,
                limits,
            )?;
            if done {
                println!("probe_result=video_channel_start_received");
                println!("probe_stop=video_channel_start_received_ready_for_media_data");
                return Ok(());
            }
        }
    }

    println!("probe_tls_state={}", tls.handshake_state());
    Err(CliError::Protocol(
        "auth/service-discovery/channel-setup probe timed out before completion".into(),
    ))
}

/// Routes one assembled message by channel. Returns `true` once the video
/// channel has received `Start` and the input channel has opened — the
/// probe's success condition.
#[allow(clippy::too_many_arguments)]
fn handle_message<T: SessionTransport>(
    message: &Message,
    handshake: &mut HandshakeStateMachine,
    video_channel: &mut Option<VideoChannel>,
    input_channel: &mut Option<InputChannel>,
    audio_channel: &mut Option<ChannelOpenStateMachine>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<bool, CliError> {
    if message.channel_id == 0 {
        if let Some(summary) = handle_assembled_message(message, handshake, tls, transport, limits)?
        {
            print_summary(&summary);
            println!("probe_result=service_discovery_summary_received");
            send_service_discovery_response(tls, transport, limits)?;
            *video_channel = Some(VideoChannel::AwaitingOpen(ChannelOpenStateMachine::new(
                VIDEO_CHANNEL_ID,
            )));
            *input_channel = Some(InputChannel::AwaitingOpen(ChannelOpenStateMachine::new(
                INPUT_CHANNEL_ID,
            )));
            *audio_channel = Some(ChannelOpenStateMachine::new(AUDIO_CHANNEL_ID));
        }
        return Ok(false);
    }

    if message.channel_id == VIDEO_CHANNEL_ID {
        handle_video_channel_message(message, video_channel, tls, transport, limits)?;
    } else if message.channel_id == INPUT_CHANNEL_ID {
        handle_input_channel_message(message, input_channel, tls, transport, limits)?;
    } else if message.channel_id == AUDIO_CHANNEL_ID {
        handle_audio_channel_message(message, audio_channel, tls, transport, limits)?;
    } else {
        return Err(CliError::Protocol(format!(
            "message on unadvertised channel {}",
            message.channel_id
        )));
    }

    Ok(matches!(video_channel, Some(VideoChannel::Ready))
        && matches!(input_channel, Some(InputChannel::Open)))
}

/// Builds and sends `ServiceDiscoveryResponse`, advertising video,
/// input/touch, and (experimentally — see `AUDIO_CHANNEL_ID`) one
/// media-audio channel, with head-unit-chosen capability data — not
/// phone-derived, so safe to construct without any privacy concern (unlike
/// `ServiceDiscoveryRequestSummary`).
fn send_service_discovery_response<T: SessionTransport>(
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
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
            ServiceCandidate {
                channel_id: AUDIO_CHANNEL_ID,
                kind: ServiceKind::MediaAudio,
                availability: ServiceAvailability::Ready,
            },
        ],
        DEFAULT_MAX_SERVICE_CANDIDATES,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    let capabilities = ServiceCapabilities {
        video: Some(VideoCapability {
            resolution: VideoCodecResolution::Video800x480,
            frame_rate: VideoFrameRate::Fps30,
        }),
        touch: Some(TouchCapability {
            width: REFERENCE_DISPLAY_WIDTH,
            height: REFERENCE_DISPLAY_HEIGHT,
            touch_type: TouchScreenType::Capacitive,
        }),
        media_audio: Some(AudioCapability {
            sampling_rate: 48_000,
            number_of_bits: 16,
            number_of_channels: 2,
            stream_type: AudioStreamType::Media,
        }),
        head_unit_info: Some(HeadUnitInfo {
            make: "pi-auto-headunit".into(),
            model: "aa-headunit-diagnostics".into(),
            year: "2026".into(),
            vehicle_id: "dev-probe".into(),
            head_unit_make: "pi-auto-headunit".into(),
            head_unit_model: "aa-headunit-diagnostics".into(),
            head_unit_software_build: env!("CARGO_PKG_VERSION").into(),
            head_unit_software_version: env!("CARGO_PKG_VERSION").into(),
        }),
    };
    let response = encode_service_discovery_response(&catalogue, &capabilities)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let payload = response
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
    println!("probe_state=service_discovery_response_sent");
    Ok(())
}

/// Drives the video channel's `ChannelOpenStateMachine` then
/// `VideoSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data.
fn handle_video_channel_message<T: SessionTransport>(
    message: &Message,
    video_channel: &mut Option<VideoChannel>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = video_channel.as_mut().ok_or_else(|| {
        CliError::Protocol("video channel message before ServiceDiscoveryResponse was sent".into())
    })?;
    match state {
        VideoChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on video channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    VIDEO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=video_channel_open");
            *state = VideoChannel::AwaitingSetup(VideoSetupStateMachine::new());
            Ok(())
        }
        VideoChannel::AwaitingSetup(machine) => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "expected Setup/Start on video channel".into(),
                ));
            }
            let actions = machine
                .advance(VideoSetupEvent::InboundMedia(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                match action {
                    VideoSetupAction::SendMedia(response) => {
                        let payload = response
                            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        send_encrypted(
                            transport,
                            tls,
                            VIDEO_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        println!("probe_state=video_channel_setup_config_sent");
                    }
                    VideoSetupAction::Ready {
                        session_id,
                        configuration_index,
                    } => {
                        println!("probe_state=video_channel_start_received");
                        println!("video_channel_session_id={session_id}");
                        println!("video_channel_configuration_index={configuration_index}");
                        *state = VideoChannel::Ready;
                    }
                }
            }
            Ok(())
        }
        VideoChannel::Ready => Err(CliError::Protocol(
            "unexpected message on video channel after Start".into(),
        )),
    }
}

/// Drives the input/touch channel's `ChannelOpenStateMachine`. No further
/// setup is needed once it opens — see the module doc comment.
fn handle_input_channel_message<T: SessionTransport>(
    message: &Message,
    input_channel: &mut Option<InputChannel>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = input_channel.as_mut().ok_or_else(|| {
        CliError::Protocol("input channel message before ServiceDiscoveryResponse was sent".into())
    })?;
    match state {
        InputChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on input channel".into(),
                ));
            }
            let actions = machine
                .advance(ChannelOpenEvent::InboundControl(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                let ChannelOpenAction::SendControl(response) = action;
                let payload = response
                    .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                    .map_err(|error| CliError::Protocol(error.to_string()))?;
                send_encrypted(
                    transport,
                    tls,
                    INPUT_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=input_channel_open");
            *state = InputChannel::Open;
            Ok(())
        }
        InputChannel::Open => Err(CliError::Protocol(
            "unexpected message on input channel after open".into(),
        )),
    }
}

/// Drives the experimental audio channel's `ChannelOpenStateMachine` only —
/// no wrapper enum, since (unlike video/input) there is no further state to
/// track this pass: no audio Setup/Config/Start handshake exists yet. See
/// `AUDIO_CHANNEL_ID`.
fn handle_audio_channel_message<T: SessionTransport>(
    message: &Message,
    audio_channel: &mut Option<ChannelOpenStateMachine>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let machine = audio_channel.as_mut().ok_or_else(|| {
        CliError::Protocol("audio channel message before ServiceDiscoveryResponse was sent".into())
    })?;
    if machine.state() != ChannelOpenState::AwaitingOpenRequest {
        return Err(CliError::Protocol(
            "unexpected message on audio channel after open".into(),
        ));
    }
    if message.message_type != MessageType::Control {
        return Err(CliError::Protocol(
            "expected ChannelOpenRequest on audio channel".into(),
        ));
    }
    let actions = machine
        .advance(ChannelOpenEvent::InboundControl(&message.payload))
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    for action in actions {
        let ChannelOpenAction::SendControl(response) = action;
        let payload = response
            .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
            .map_err(|error| CliError::Protocol(error.to_string()))?;
        send_encrypted(
            transport,
            tls,
            AUDIO_CHANNEL_ID,
            MessageType::Control,
            &payload,
            limits,
        )?;
    }
    println!("probe_state=audio_channel_open");
    Ok(())
}

/// Encrypts `plaintext_payload` and sends it framed on `channel_id`.
/// `ServiceDiscoveryResponse`, `ChannelOpenResponse`, and `Config` are all
/// sent this way — verified directly against the pinned AASDK C++ source,
/// not assumed (see the module doc comment).
fn send_encrypted<T: SessionTransport>(
    transport: &mut T,
    tls: &mut OpenSslTlsClient,
    channel_id: u8,
    message_type: MessageType,
    plaintext_payload: &[u8],
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let ciphertext = tls
        .encrypt_application_data(plaintext_payload)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let frame = encode_frame(
        FrameHeader {
            channel_id,
            frame_type: FrameType::Bulk,
            encryption: Encryption::Encrypted,
            message_type,
        },
        None,
        &ciphertext,
        limits,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))?;
    transport.send_all(&frame).map_err(CliError::Transport)
}

/// Pushes one decoded wire frame into `assembler`, decrypting it first if
/// `Encrypted`. Encrypted frames arriving before TLS completes are a
/// protocol violation, since decryption isn't yet possible.
fn push_decoded_frame(
    frame: DecodedFrame<'_>,
    assembler: &mut MessageAssembler,
    tls: &mut OpenSslTlsClient,
    handshake_state: HandshakeState,
) -> Result<Option<Message>, CliError> {
    match frame.header.encryption {
        Encryption::Plain => assembler
            .push(frame)
            .map_err(|error| CliError::Protocol(error.to_string())),
        Encryption::Encrypted => {
            if !matches!(
                handshake_state,
                HandshakeState::AwaitingServiceDiscovery | HandshakeState::ServiceDiscoveryReceived
            ) {
                return Err(CliError::Protocol(
                    "encrypted frame received before TLS handshake completed".into(),
                ));
            }
            println!("probe_state=encrypted_frame_received");
            let plaintext = tls
                .decrypt_application_data(frame.payload)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            let decrypted_frame = DecodedFrame {
                header: frame.header,
                total_message_size: frame.total_message_size,
                payload: &plaintext,
                consumed: frame.consumed,
            };
            assembler
                .push(decrypted_frame)
                .map_err(|error| CliError::Protocol(error.to_string()))
        }
    }
}

/// Validates an assembled message's metadata, then advances the handshake
/// state machine with its (now-plaintext) payload.
fn handle_assembled_message<T: SessionTransport>(
    message: &Message,
    handshake: &mut HandshakeStateMachine,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<Option<ServiceDiscoveryRequestSummary>, CliError> {
    if message.channel_id != 0 || message.message_type != MessageType::Specific {
        println!("unexpected_message_channel_id={}", message.channel_id);
        println!("unexpected_message_encryption={:?}", message.encryption);
        println!("unexpected_message_type={:?}", message.message_type);
        println!("unexpected_message_payload_bytes={}", message.payload.len());
        return Err(CliError::Protocol(
            "unexpected message metadata during auth/service-discovery probe".into(),
        ));
    }

    let mut actions: VecDeque<_> = handshake
        .advance(HandshakeEvent::InboundControl(&message.payload))
        .map_err(|error| CliError::Protocol(error.to_string()))?
        .into();
    process_actions(&mut actions, handshake, tls, transport, limits)
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
        "service_discovery_label_text_bytes={}",
        bytes(summary.label_text_bytes)
    );
    println!(
        "service_discovery_device_name_bytes={}",
        bytes(summary.device_name_bytes)
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
                if let Some(version) = handshake.negotiated_version() {
                    println!(
                        "probe_negotiated_version={}.{}",
                        version.major, version.minor
                    );
                }
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
