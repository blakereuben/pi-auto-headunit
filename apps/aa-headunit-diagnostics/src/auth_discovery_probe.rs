//! Gated authentication/service-discovery/channel-setup probe.
//!
//! Reuses the same frame codec, message assembler, `HandshakeStateMachine`,
//! and `OpenSslTlsClient` wiring as the frozen `credential-probe`
//! (`live_probe.rs`, unmodified). Beyond that, this probe lets
//! `HandshakeStateMachine::advance` run through to
//! `ServiceDiscoveryRequest`, then goes further still: it builds and sends
//! `ServiceDiscoveryResponse` advertising the full canonical eight-service
//! set (video, touch/input, media/system/speech audio, sensors, Bluetooth,
//! microphone — see `protocol_aap::service_discovery_response`), then
//! handles `AudioFocusRequest`/`AudioFocusNotification` on the control
//! channel (`protocol_aap::audio_focus`), then drives each channel's
//! `ChannelOpenRequest`/`ChannelOpenResponse` handshake
//! (`protocol_aap::channel_open`), then the video channel's
//! `Setup`→`Config`→`Start` handshake (`protocol_aap::video_setup`), the
//! input channel's `KeyBindingRequest`/`KeyBindingResponse` exchange
//! (`protocol_aap::input_message`), and the `MediaAudio`/`SystemAudio`
//! channels' own `Setup`→`Config`→`Start` handshakes
//! (`protocol_aap::audio_setup` — same message shape as video's, accepting
//! `MEDIA_CODEC_AUDIO_PCM` instead of H.264; the same `AudioSetupStateMachine`
//! is reused unmodified for both audio channels, since both advertise a
//! single uncompressed PCM `AudioConfiguration`, just at different sample
//! rates). It stops the instant the video channel receives `Start` and the
//! input channel has opened — no `MEDIA_MESSAGE_DATA` byte is ever parsed,
//! no video decode/render/UI work happens here, and none of the other four
//! channels are driven past open. See the channel-setup design record for
//! the full scope boundary and provenance trail.
//!
//! Every non-video channel, the populated `HeadUnitInfo`, `AudioFocusRequest`
//! handling, `KeyBindingRequest` handling, and the `MediaAudio`/`SystemAudio`
//! channels' `Setup`/`Config`/`Start` handling are all experiments toward the
//! same real-phone finding: Android Auto's "phone and car are running
//! incompatible software" (Error 2). Advertising one audio channel, offering
//! the phone's own reported protocol version (`1.7`, versus the pinned
//! source's `1.6`), and populating `HeadUnitInfo` were each tried
//! independently against the earliest form of this failure (appearing
//! immediately after `ServiceDiscoveryResponse`, before any
//! `ChannelOpenRequest` arrived) and each made no difference — ruling out a
//! simple missing-service, version-number-mismatch, or missing-identity
//! cause. Advertising the full canonical set instead — motivated directly by
//! this project's own already-approved `OpenAuto` source
//! (`ServiceFactory::create()`, revision `aa90412bf93b5a5078495ea85ac9270c6297d369`):
//! it unconditionally constructs seven of these eight services (an eighth,
//! `SpeechAudio`, is config-gated but on by default), not a curated subset —
//! was the first change that altered real-phone behavior: the phone stopped
//! rejecting `ServiceDiscoveryResponse` and progressed into the session,
//! first requesting audio focus, then opening every channel and driving
//! video through `Setup`→`Config`, then requesting key bindings on the input
//! channel, then sending `Setup` on the `MediaAudio` channel, then sending
//! `Setup` on the `SystemAudio` channel. Error 2 still appears at each new
//! point reached (confirmed on the phone screen, not inferred from probe
//! output), but the failure boundary keeps moving further into the session
//! as each new message is handled — see
//! `docs/protocol/error-2-investigation.md` for the full, still-open
//! history. Every non-video, non-audio-setup channel besides input is
//! driven only to `ChannelOpenState::Open` — no further handshake (sensor
//! data, Bluetooth pairing, microphone capture, `SpeechAudio` playback)
//! exists yet; that is separate follow-on scope once a hypothesis is
//! confirmed. The input channel's `KeyBindingRequest` is answered
//! `KeycodeNotBound` for any non-empty request, matching this project's own
//! `ServiceDiscoveryResponse` exactly (it advertises zero supported
//! keycodes — no button hardware exists yet), mirroring `OpenAuto`'s
//! `InputService::onBindingRequest` validation-against-declared-capability
//! behavior rather than fabricating success.
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
    AASDK_MAX_FRAME_PAYLOAD_SIZE, AudioCapability, AudioFocusRequestType, AudioFocusStateType,
    AudioSetupAction, AudioSetupEvent, AudioSetupStateMachine, AudioStreamType,
    BluetoothCapability, ChannelOpenAction, ChannelOpenEvent, ChannelOpenState,
    ChannelOpenStateMachine, ControlMessage, ControlMessageId, DEFAULT_MAX_CONTROL_BODY_SIZE,
    DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE, DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE,
    DEFAULT_MAX_SERVICE_CANDIDATES, DecodedFrame, Encryption, FrameError, FrameHeader, FrameType,
    HandshakeAction, HandshakeEvent, HandshakeState, HandshakeStateMachine, HeadUnitInfo,
    InputMessage, InputMessageId, KeyBindingStatus, Message, MessageAssembler, MessageType,
    MicrophoneCapability, ProtocolLimits, ServiceAvailability, ServiceCandidate,
    ServiceCapabilities, ServiceCatalogue, ServiceDiscoveryRequestSummary, ServiceKind, TlsClient,
    TlsProgress, TouchCapability, TouchScreenType, VideoCapability, VideoCodecResolution,
    VideoFrameRate, VideoSetupAction, VideoSetupEvent, VideoSetupStateMachine,
    decode_audio_focus_request, decode_frame, decode_key_binding_request,
    encode_audio_focus_notification, encode_frame, encode_key_binding_response,
    encode_service_discovery_response,
};
use security_openssl::{OpenSslTlsClient, TlsVersionPolicy};
use std::collections::{HashMap, VecDeque};
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
const MEDIA_AUDIO_CHANNEL_ID: u8 = 3;
const SYSTEM_AUDIO_CHANNEL_ID: u8 = 4;
const SPEECH_AUDIO_CHANNEL_ID: u8 = 5;
const SENSORS_CHANNEL_ID: u8 = 6;
const BLUETOOTH_CHANNEL_ID: u8 = 7;
const MICROPHONE_CHANNEL_ID: u8 = 8;
/// The Pi 5 reference display: the official 7-inch DSI touchscreen,
/// matching the 800x480/30fps baseline already selected in
/// `ARCHITECTURE.md`/M3.
const REFERENCE_DISPLAY_WIDTH: i32 = 800;
const REFERENCE_DISPLAY_HEIGHT: i32 = 480;
/// `OpenAuto`'s `ServiceFactory` defaults (`MediaAudioService`: 2ch/16-bit/48kHz;
/// `SpeechAudioService`/`SystemAudioService`/`AudioInputService`: 1ch/16-bit/16kHz),
/// not invented values.
const MEDIA_AUDIO_SAMPLING_RATE: u32 = 48_000;
const VOICE_AUDIO_SAMPLING_RATE: u32 = 16_000;
const VOICE_AUDIO_BITS: u32 = 16;
const VOICE_AUDIO_CHANNELS: u32 = 1;

/// Per-channel progress for the video channel, driven once
/// `ServiceDiscoveryResponse` has been sent.
enum VideoChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    AwaitingSetup(VideoSetupStateMachine),
    Ready,
}

/// Per-channel progress for the `MediaAudio` channel, driven once
/// `ServiceDiscoveryResponse` has been sent. Same shape as `VideoChannel`
/// (see `protocol_aap::audio_setup` for why this is a separate type rather
/// than a shared one).
enum MediaAudioChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    AwaitingSetup(AudioSetupStateMachine),
    Ready,
}

/// Per-channel progress for the `SystemAudio` channel, driven once
/// `ServiceDiscoveryResponse` has been sent. Same shape as
/// `MediaAudioChannel` — `SystemAudioChannel` is a thin `AudioMediaSinkService`
/// subclass in AASDK too, and this project advertises a single uncompressed
/// PCM `AudioConfiguration` for it just like `MediaAudio`, so the same
/// `AudioSetupStateMachine` is reused unmodified.
enum SystemAudioChannel {
    AwaitingOpen(ChannelOpenStateMachine),
    AwaitingSetup(AudioSetupStateMachine),
    Ready,
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
    // Control channel (0) + video channel + input channel + MediaAudio
    // channel + SystemAudio channel can each independently be
    // mid-fragmentation once channel setup starts.
    let mut assembler =
        MessageAssembler::new(5).map_err(|error| CliError::Protocol(error.to_string()))?;

    let mut video_channel: Option<VideoChannel> = None;
    let mut media_audio_channel: Option<MediaAudioChannel> = None;
    let mut system_audio_channel: Option<SystemAudioChannel> = None;
    // Every channel that only ever needs to reach ChannelOpenState::Open —
    // input/touch plus four of the six non-video channels this experiment
    // adds (MediaAudio and SystemAudio now have their own dedicated
    // Setup/Config/Start state machines above, like video). None until
    // ServiceDiscoveryResponse is sent, then populated with one entry per
    // advertised channel_id.
    let mut simple_channels: HashMap<u8, ChannelOpenStateMachine> = HashMap::new();

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
                &mut media_audio_channel,
                &mut system_audio_channel,
                &mut simple_channels,
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
    media_audio_channel: &mut Option<MediaAudioChannel>,
    system_audio_channel: &mut Option<SystemAudioChannel>,
    simple_channels: &mut HashMap<u8, ChannelOpenStateMachine>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<bool, CliError> {
    if message.channel_id == 0 {
        if handshake.state() == HandshakeState::ServiceDiscoveryReceived {
            handle_post_discovery_control_message(message, tls, transport, limits)?;
        } else if let Some(summary) =
            handle_assembled_message(message, handshake, tls, transport, limits)?
        {
            print_summary(&summary);
            println!("probe_result=service_discovery_summary_received");
            send_service_discovery_response(tls, transport, limits)?;
            *video_channel = Some(VideoChannel::AwaitingOpen(ChannelOpenStateMachine::new(
                VIDEO_CHANNEL_ID,
            )));
            *media_audio_channel = Some(MediaAudioChannel::AwaitingOpen(
                ChannelOpenStateMachine::new(MEDIA_AUDIO_CHANNEL_ID),
            ));
            *system_audio_channel = Some(SystemAudioChannel::AwaitingOpen(
                ChannelOpenStateMachine::new(SYSTEM_AUDIO_CHANNEL_ID),
            ));
            for channel_id in [
                INPUT_CHANNEL_ID,
                SPEECH_AUDIO_CHANNEL_ID,
                SENSORS_CHANNEL_ID,
                BLUETOOTH_CHANNEL_ID,
                MICROPHONE_CHANNEL_ID,
            ] {
                simple_channels.insert(channel_id, ChannelOpenStateMachine::new(channel_id));
            }
        }
        return Ok(false);
    }

    if message.channel_id == VIDEO_CHANNEL_ID {
        handle_video_channel_message(message, video_channel, tls, transport, limits)?;
    } else if message.channel_id == MEDIA_AUDIO_CHANNEL_ID {
        handle_media_audio_channel_message(message, media_audio_channel, tls, transport, limits)?;
    } else if message.channel_id == SYSTEM_AUDIO_CHANNEL_ID {
        handle_system_audio_channel_message(message, system_audio_channel, tls, transport, limits)?;
    } else if message.channel_id == INPUT_CHANNEL_ID
        && simple_channels
            .get(&INPUT_CHANNEL_ID)
            .is_some_and(|machine| machine.state() == ChannelOpenState::Open)
    {
        handle_input_channel_message(message, tls, transport, limits)?;
    } else {
        handle_simple_channel_message(
            message.channel_id,
            message,
            simple_channels,
            tls,
            transport,
            limits,
        )?;
    }

    let input_open = simple_channels
        .get(&INPUT_CHANNEL_ID)
        .is_some_and(|machine| machine.state() == ChannelOpenState::Open);
    Ok(matches!(video_channel, Some(VideoChannel::Ready)) && input_open)
}

/// Handles control-channel traffic that arrives after `HandshakeStateMachine`
/// has already reached `ServiceDiscoveryReceived` (which has nothing further
/// to do — see `docs/protocol/error-2-investigation.md`). Only
/// `AudioFocusRequest` is handled; anything else fails closed with a clear,
/// distinct error naming the unexpected message, so if the phone sends
/// something new next, that's immediately visible rather than silently
/// swallowed.
fn handle_post_discovery_control_message<T: SessionTransport>(
    message: &Message,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    if message.message_type != MessageType::Specific {
        return Err(CliError::Protocol(
            "unexpected control message type after service discovery".into(),
        ));
    }
    let control_message = ControlMessage::decode(&message.payload, DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    match control_message.id {
        ControlMessageId::AudioFocusRequest => {
            let requested = decode_audio_focus_request(&control_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=audio_focus_requested");
            println!("audio_focus_request_type={requested:?}");
            let granted = grant_audio_focus(requested);
            let response = encode_audio_focus_notification(granted);
            let payload = response
                .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
            println!("probe_state=audio_focus_notification_sent");
            Ok(())
        }
        other => Err(CliError::Protocol(format!(
            "unexpected control message {other:?} after service discovery"
        ))),
    }
}

/// Placeholder audio-focus policy: grant exactly what's asked. This
/// project has no real audio hardware/focus-arbitration pipeline yet (M3
/// still open) — this is the simplest thing that answers honestly and
/// keeps the session alive, not a claim about real Android Auto behavior
/// (none is publicly documented — see the module doc comment).
const fn grant_audio_focus(requested: AudioFocusRequestType) -> AudioFocusStateType {
    match requested {
        AudioFocusRequestType::Gain => AudioFocusStateType::Gain,
        AudioFocusRequestType::GainTransient | AudioFocusRequestType::GainTransientMayDuck => {
            AudioFocusStateType::GainTransient
        }
        AudioFocusRequestType::Release => AudioFocusStateType::Loss,
    }
}

/// Builds and sends `ServiceDiscoveryResponse`, advertising all eight
/// `ServiceKind`s (the full canonical set `OpenAuto`'s `ServiceFactory`
/// unconditionally constructs — see the module doc comment) with
/// head-unit-chosen capability data — not phone-derived, so safe to
/// construct without any privacy concern (unlike
/// `ServiceDiscoveryRequestSummary`).
fn send_service_discovery_response<T: SessionTransport>(
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let catalogue = build_service_catalogue()?;
    let capabilities = build_service_capabilities();
    let response = encode_service_discovery_response(&catalogue, &capabilities)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    let payload = response
        .encode(DEFAULT_MAX_CONTROL_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    send_encrypted(transport, tls, 0, MessageType::Specific, &payload, limits)?;
    println!("probe_state=service_discovery_response_sent");
    Ok(())
}

/// The full canonical eight-service set — see the module doc comment for
/// why (`OpenAuto`'s `ServiceFactory` finding).
fn build_service_catalogue() -> Result<ServiceCatalogue, CliError> {
    ServiceCatalogue::build(
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
                channel_id: MEDIA_AUDIO_CHANNEL_ID,
                kind: ServiceKind::MediaAudio,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: SYSTEM_AUDIO_CHANNEL_ID,
                kind: ServiceKind::SystemAudio,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: SPEECH_AUDIO_CHANNEL_ID,
                kind: ServiceKind::SpeechAudio,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: SENSORS_CHANNEL_ID,
                kind: ServiceKind::Sensors,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: BLUETOOTH_CHANNEL_ID,
                kind: ServiceKind::Bluetooth,
                availability: ServiceAvailability::Ready,
            },
            ServiceCandidate {
                channel_id: MICROPHONE_CHANNEL_ID,
                kind: ServiceKind::Microphone,
                availability: ServiceAvailability::Ready,
            },
        ],
        DEFAULT_MAX_SERVICE_CANDIDATES,
    )
    .map_err(|error| CliError::Protocol(error.to_string()))
}

/// Head-unit-chosen capability data for every advertised service — not
/// phone-derived, so safe to construct and log without any privacy
/// concern (unlike `ServiceDiscoveryRequestSummary`).
fn build_service_capabilities() -> ServiceCapabilities {
    ServiceCapabilities {
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
            sampling_rate: MEDIA_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: 2,
            stream_type: AudioStreamType::Media,
        }),
        system_audio: Some(AudioCapability {
            sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: VOICE_AUDIO_CHANNELS,
            stream_type: AudioStreamType::SystemAudio,
        }),
        speech_audio: Some(AudioCapability {
            sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: VOICE_AUDIO_CHANNELS,
            stream_type: AudioStreamType::Guidance,
        }),
        bluetooth: Some(BluetoothCapability {
            car_address: "02:00:00:00:00:01".into(),
        }),
        microphone: Some(MicrophoneCapability {
            sampling_rate: VOICE_AUDIO_SAMPLING_RATE,
            number_of_bits: VOICE_AUDIO_BITS,
            number_of_channels: VOICE_AUDIO_CHANNELS,
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
    }
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

/// Drives the `MediaAudio` channel's `ChannelOpenStateMachine` then
/// `AudioSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data. Mirrors `handle_video_channel_message`
/// exactly — same message shape, different channel id and accepted codec
/// (see `protocol_aap::audio_setup`).
fn handle_media_audio_channel_message<T: SessionTransport>(
    message: &Message,
    media_audio_channel: &mut Option<MediaAudioChannel>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = media_audio_channel.as_mut().ok_or_else(|| {
        CliError::Protocol(
            "media-audio channel message before ServiceDiscoveryResponse was sent".into(),
        )
    })?;
    match state {
        MediaAudioChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on media-audio channel".into(),
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
                    MEDIA_AUDIO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=media_audio_channel_open");
            *state = MediaAudioChannel::AwaitingSetup(AudioSetupStateMachine::new());
            Ok(())
        }
        MediaAudioChannel::AwaitingSetup(machine) => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "expected Setup/Start on media-audio channel".into(),
                ));
            }
            let actions = machine
                .advance(AudioSetupEvent::InboundMedia(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                match action {
                    AudioSetupAction::SendMedia(response) => {
                        let payload = response
                            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        send_encrypted(
                            transport,
                            tls,
                            MEDIA_AUDIO_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        println!("probe_state=media_audio_channel_setup_config_sent");
                    }
                    AudioSetupAction::Ready {
                        session_id,
                        configuration_index,
                    } => {
                        println!("probe_state=media_audio_channel_start_received");
                        println!("media_audio_channel_session_id={session_id}");
                        println!("media_audio_channel_configuration_index={configuration_index}");
                        *state = MediaAudioChannel::Ready;
                    }
                }
            }
            Ok(())
        }
        MediaAudioChannel::Ready => Err(CliError::Protocol(
            "unexpected message on media-audio channel after Start".into(),
        )),
    }
}

/// Drives the `SystemAudio` channel's `ChannelOpenStateMachine` then
/// `AudioSetupStateMachine`, sending each state machine's response actions
/// as TLS-encrypted application data. Mirrors
/// `handle_media_audio_channel_message` exactly — same underlying
/// `AudioSetupStateMachine` (this project advertises a single uncompressed
/// PCM `AudioConfiguration` for `SystemAudio` too, so the same accepted
/// codec applies), just a different channel id.
fn handle_system_audio_channel_message<T: SessionTransport>(
    message: &Message,
    system_audio_channel: &mut Option<SystemAudioChannel>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let state = system_audio_channel.as_mut().ok_or_else(|| {
        CliError::Protocol(
            "system-audio channel message before ServiceDiscoveryResponse was sent".into(),
        )
    })?;
    match state {
        SystemAudioChannel::AwaitingOpen(machine) => {
            if message.message_type != MessageType::Control {
                return Err(CliError::Protocol(
                    "expected ChannelOpenRequest on system-audio channel".into(),
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
                    SYSTEM_AUDIO_CHANNEL_ID,
                    MessageType::Control,
                    &payload,
                    limits,
                )?;
            }
            println!("probe_state=system_audio_channel_open");
            *state = SystemAudioChannel::AwaitingSetup(AudioSetupStateMachine::new());
            Ok(())
        }
        SystemAudioChannel::AwaitingSetup(machine) => {
            if message.message_type != MessageType::Specific {
                return Err(CliError::Protocol(
                    "expected Setup/Start on system-audio channel".into(),
                ));
            }
            let actions = machine
                .advance(AudioSetupEvent::InboundMedia(&message.payload))
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            for action in actions {
                match action {
                    AudioSetupAction::SendMedia(response) => {
                        let payload = response
                            .encode(DEFAULT_MAX_MEDIA_MESSAGE_BODY_SIZE)
                            .map_err(|error| CliError::Protocol(error.to_string()))?;
                        send_encrypted(
                            transport,
                            tls,
                            SYSTEM_AUDIO_CHANNEL_ID,
                            MessageType::Specific,
                            &payload,
                            limits,
                        )?;
                        println!("probe_state=system_audio_channel_setup_config_sent");
                    }
                    AudioSetupAction::Ready {
                        session_id,
                        configuration_index,
                    } => {
                        println!("probe_state=system_audio_channel_start_received");
                        println!("system_audio_channel_session_id={session_id}");
                        println!("system_audio_channel_configuration_index={configuration_index}");
                        *state = SystemAudioChannel::Ready;
                    }
                }
            }
            Ok(())
        }
        SystemAudioChannel::Ready => Err(CliError::Protocol(
            "unexpected message on system-audio channel after Start".into(),
        )),
    }
}

/// Drives one "advertise → open → nothing further" channel's
/// `ChannelOpenStateMachine` — covers `Input` and four other non-video
/// channels this experiment adds (`MediaAudio` and `SystemAudio` now have
/// their own dedicated Setup/Config/Start state machines above, like
/// video). Generalizes what used to be two near-identical per-channel
/// functions, since this shape now repeats five times;
/// `VideoChannel`/`MediaAudioChannel`/`SystemAudioChannel`'s
/// `Setup`/`Config`/`Start` follow-through is genuinely different and stays
/// separate.
fn handle_simple_channel_message<T: SessionTransport>(
    channel_id: u8,
    message: &Message,
    simple_channels: &mut HashMap<u8, ChannelOpenStateMachine>,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    let machine = simple_channels.get_mut(&channel_id).ok_or_else(|| {
        CliError::Protocol(format!("message on unadvertised channel {channel_id}"))
    })?;
    if machine.state() != ChannelOpenState::AwaitingOpenRequest {
        return Err(CliError::Protocol(format!(
            "unexpected message on channel {channel_id} after open"
        )));
    }
    if message.message_type != MessageType::Control {
        return Err(CliError::Protocol(format!(
            "expected ChannelOpenRequest on channel {channel_id}"
        )));
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
            channel_id,
            MessageType::Control,
            &payload,
            limits,
        )?;
    }
    println!("probe_state=simple_channel_open channel_id={channel_id}");
    Ok(())
}

/// Handles Input-channel traffic that arrives once the channel has already
/// reached `ChannelOpenState::Open`. Only `KeyBindingRequest` is handled;
/// anything else fails closed with a clear, distinct error naming the
/// unexpected message, matching `handle_post_discovery_control_message`'s
/// posture.
fn handle_input_channel_message<T: SessionTransport>(
    message: &Message,
    tls: &mut OpenSslTlsClient,
    transport: &mut T,
    limits: ProtocolLimits,
) -> Result<(), CliError> {
    if message.message_type != MessageType::Specific {
        return Err(CliError::Protocol(
            "unexpected message type on input channel after open".into(),
        ));
    }
    let input_message = InputMessage::decode(&message.payload, DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    match input_message.id {
        InputMessageId::KeyBindingRequest => {
            let keycodes = decode_key_binding_request(&input_message.body)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            println!("probe_state=key_binding_requested");
            println!("key_binding_requested_count={}", keycodes.len());
            let status = evaluate_key_binding_request(&keycodes);
            let response = encode_key_binding_response(status);
            let payload = response
                .encode(DEFAULT_MAX_INPUT_MESSAGE_BODY_SIZE)
                .map_err(|error| CliError::Protocol(error.to_string()))?;
            send_encrypted(
                transport,
                tls,
                INPUT_CHANNEL_ID,
                MessageType::Specific,
                &payload,
                limits,
            )?;
            println!("probe_state=key_binding_response_sent");
            println!("key_binding_response_status={status:?}");
            Ok(())
        }
        other => Err(CliError::Protocol(format!(
            "unexpected input message {other:?} after open"
        ))),
    }
}

/// This project's `ServiceDiscoveryResponse` advertises zero supported
/// keycodes today (no button hardware wired up yet), so the only honest
/// response is `KeycodeNotBound` for any non-empty request — matching that
/// declared capability exactly, not a guess (mirrors `OpenAuto`'s
/// `InputService::onBindingRequest` validation against its own advertised
/// list). An empty request trivially has nothing unsupported in it.
const fn evaluate_key_binding_request(keycodes: &[i32]) -> KeyBindingStatus {
    if keycodes.is_empty() {
        KeyBindingStatus::Success
    } else {
        KeyBindingStatus::KeycodeNotBound
    }
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
