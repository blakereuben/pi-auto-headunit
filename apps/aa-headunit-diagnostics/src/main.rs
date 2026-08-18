use platform_api::{
    CapabilityState, ProviderPreference, RadioKind, RadioProvider, choose_provider,
};
use std::env;

#[cfg(target_os = "linux")]
mod live_probe;

#[cfg(target_os = "linux")]
mod auth_discovery_probe;

#[cfg(target_os = "linux")]
mod cancellation;

#[cfg(target_os = "linux")]
mod credentials;

#[cfg(target_os = "linux")]
mod session_supervisor;

#[cfg(target_os = "linux")]
mod replug_prompt;

#[cfg(target_os = "linux")]
mod connection_state;

#[cfg(target_os = "linux")]
mod gesture_settings;

#[cfg(target_os = "linux")]
mod mtp_suppression;

#[cfg(target_os = "linux")]
mod gtk_dev_ui;

#[cfg(target_os = "linux")]
mod wireless_bootstrap;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error={error}");
            error.exit_code()
        }
    };
    std::process::exit(code);
}

#[allow(clippy::too_many_lines)]
fn run(args: &[String]) -> Result<(), CliError> {
    match args {
        [command] if command == "preflight" => preflight(),
        [command] if command == "wireless" => wireless(&[]),
        [command, rest @ ..] if command == "wireless" => wireless(rest),
        [group, command] if group == "media" && command == "probe" => media_probe(),
        [group, command] if group == "media" && command == "mic-probe" => media_mic_probe(None),
        [group, command, seconds_flag, seconds]
            if group == "media" && command == "mic-probe" && seconds_flag == "--seconds" =>
        {
            media_mic_probe(Some(parse_mic_probe_seconds(seconds)?))
        }
        [group, command, rest @ ..] if group == "credentials" => credentials_command(command, rest),
        [group, command] if group == "usb" && command == "list" => usb_list(),
        [group, command] if group == "developer" && command == "tcp-probe" => developer_tcp_probe(),
        [group, command, allow]
            if group == "developer" && command == "tls-probe" && allow == "--allow-live-aap" =>
        {
            developer_tls_probe(false)
        }
        [group, command, allow, compatibility]
            if group == "developer"
                && command == "tls-probe"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            developer_tls_probe(true)
        }
        [group, command, allow]
            if group == "developer"
                && command == "credential-probe"
                && allow == "--allow-live-aap" =>
        {
            developer_credential_probe(false)
        }
        [group, command, allow, compatibility]
            if group == "developer"
                && command == "credential-probe"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            developer_credential_probe(true)
        }
        [group, command, allow]
            if group == "developer"
                && command == "auth-discovery-probe"
                && allow == "--allow-live-aap" =>
        {
            developer_auth_discovery_probe(false)
        }
        [group, command, allow, compatibility]
            if group == "developer"
                && command == "auth-discovery-probe"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            developer_auth_discovery_probe(true)
        }
        [group, command, flag, selector]
            if group == "usb" && command == "aoa" && flag == "--device" =>
        {
            usb_aoa(selector)
        }
        [group, command, device_flag, selector, cycles_flag, cycles]
            if group == "usb"
                && command == "soak"
                && device_flag == "--device"
                && cycles_flag == "--cycles" =>
        {
            usb_soak(selector, parse_cycles(cycles)? as usize)
        }
        [group, command, device_flag, selector, seconds_flag, seconds]
            if group == "usb"
                && command == "hold"
                && device_flag == "--device"
                && seconds_flag == "--seconds" =>
        {
            usb_hold(selector, parse_hold_seconds(seconds)?)
        }
        [group, command, device_flag, selector, allow]
            if group == "usb"
                && command == "tls-probe"
                && device_flag == "--device"
                && allow == "--allow-live-aap" =>
        {
            usb_tls_probe(selector, false)
        }
        [group, command, device_flag, selector, allow, compatibility]
            if group == "usb"
                && command == "tls-probe"
                && device_flag == "--device"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            usb_tls_probe(selector, true)
        }
        [group, command, device_flag, selector, allow]
            if group == "usb"
                && command == "credential-probe"
                && device_flag == "--device"
                && allow == "--allow-live-aap" =>
        {
            usb_credential_probe(selector, false)
        }
        [group, command, device_flag, selector, allow, compatibility]
            if group == "usb"
                && command == "credential-probe"
                && device_flag == "--device"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            usb_credential_probe(selector, true)
        }
        [group, command, device_flag, selector, allow]
            if group == "usb"
                && command == "auth-discovery-probe"
                && device_flag == "--device"
                && allow == "--allow-live-aap" =>
        {
            usb_auth_discovery_probe(selector, false)
        }
        [group, command, device_flag, selector, allow, compatibility]
            if group == "usb"
                && command == "auth-discovery-probe"
                && device_flag == "--device"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            usb_auth_discovery_probe(selector, true)
        }
        [group, command, device_flag, selector, allow]
            if group == "usb"
                && command == "session-supervisor"
                && device_flag == "--device"
                && allow == "--allow-live-aap" =>
        {
            usb_session_supervisor(selector, false, None, false)
        }
        [group, command, device_flag, selector, allow, compatibility]
            if group == "usb"
                && command == "session-supervisor"
                && device_flag == "--device"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            usb_session_supervisor(selector, true, None, false)
        }
        [
            group,
            command,
            device_flag,
            selector,
            allow,
            cycles_flag,
            cycles,
        ] if group == "usb"
            && command == "session-supervisor"
            && device_flag == "--device"
            && allow == "--allow-live-aap"
            && cycles_flag == "--max-cycles" =>
        {
            usb_session_supervisor(selector, false, Some(parse_cycles(cycles)?), false)
        }
        [
            group,
            command,
            device_flag,
            selector,
            allow,
            compatibility,
            cycles_flag,
            cycles,
        ] if group == "usb"
            && command == "session-supervisor"
            && device_flag == "--device"
            && allow == "--allow-live-aap"
            && compatibility == "--tls12-compat"
            && cycles_flag == "--max-cycles" =>
        {
            usb_session_supervisor(selector, true, Some(parse_cycles(cycles)?), false)
        }
        [
            group,
            command,
            device_flag,
            selector,
            allow,
            cycles_flag,
            cycles,
            force_flag,
        ] if group == "usb"
            && command == "session-supervisor"
            && device_flag == "--device"
            && allow == "--allow-live-aap"
            && cycles_flag == "--max-cycles"
            && force_flag == "--force-disconnect-each-cycle" =>
        {
            usb_session_supervisor(selector, false, Some(parse_cycles(cycles)?), true)
        }
        [
            group,
            command,
            device_flag,
            selector,
            allow,
            compatibility,
            cycles_flag,
            cycles,
            force_flag,
        ] if group == "usb"
            && command == "session-supervisor"
            && device_flag == "--device"
            && allow == "--allow-live-aap"
            && compatibility == "--tls12-compat"
            && cycles_flag == "--max-cycles"
            && force_flag == "--force-disconnect-each-cycle" =>
        {
            usb_session_supervisor(selector, true, Some(parse_cycles(cycles)?), true)
        }
        [group, command, device_flag, selector, allow]
            if group == "usb"
                && command == "gtk-dev-ui"
                && device_flag == "--device"
                && allow == "--allow-live-aap" =>
        {
            usb_gtk_dev_ui(selector, false)
        }
        [group, command, device_flag, selector, allow, compatibility]
            if group == "usb"
                && command == "gtk-dev-ui"
                && device_flag == "--device"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            usb_gtk_dev_ui(selector, true)
        }
        [group, command, allow]
            if group == "usb"
                && command == "wireless-bootstrap-probe"
                && allow == "--allow-live-aap" =>
        {
            usb_wireless_bootstrap_probe(false)
        }
        [group, command, allow, compatibility]
            if group == "usb"
                && command == "wireless-bootstrap-probe"
                && allow == "--allow-live-aap"
                && compatibility == "--tls12-compat" =>
        {
            usb_wireless_bootstrap_probe(true)
        }
        [] | [..] if args.iter().any(|arg| arg == "--help" || arg == "-h") => {
            print_help();
            Ok(())
        }
        _ => {
            print_help();
            Err(CliError::Usage("unknown or incomplete command".into()))
        }
    }
}

fn print_help() {
    println!(
        "aa-headunit-diagnostics {}\n\
         \n\
         Commands:\n\
           preflight\n\
           wireless [--wifi auto|onboard|STABLE_ID] [--bluetooth auto|onboard|STABLE_ID]\n\
           media probe\n\
           media mic-probe [--seconds N]\n\
           credentials check --certificate PATH --private-key PATH\n\
           credentials install --certificate PATH --private-key PATH\n\
           credentials status [--config PATH]\n\
           developer tcp-probe\n\
           developer tls-probe --allow-live-aap [--tls12-compat]\n\
           developer credential-probe --allow-live-aap [--tls12-compat]\n\
           developer auth-discovery-probe --allow-live-aap [--tls12-compat]\n\
           usb list\n\
           usb aoa --device BUS:ADDRESS\n\
           usb soak --device BUS:ADDRESS --cycles COUNT\n\
           usb hold --device BUS:ADDRESS --seconds COUNT\n\
           usb tls-probe --device BUS:ADDRESS --allow-live-aap\n\
           usb tls-probe --device BUS:ADDRESS --allow-live-aap --tls12-compat\n\
           usb credential-probe --device BUS:ADDRESS --allow-live-aap [--tls12-compat]\n\
           usb auth-discovery-probe --device BUS:ADDRESS --allow-live-aap [--tls12-compat]\n\
           usb session-supervisor --device BUS:ADDRESS --allow-live-aap [--tls12-compat] [--max-cycles COUNT [--force-disconnect-each-cycle]]\n\
           usb gtk-dev-ui --device BUS:ADDRESS --allow-live-aap [--tls12-compat]\n\
           usb wireless-bootstrap-probe --allow-live-aap [--tls12-compat]\n\
         \n\
         The AOA command sends documented USB vendor requests only to the explicitly selected device.",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(target_os = "linux")]
fn credentials_command(command: &str, args: &[String]) -> Result<(), CliError> {
    credentials::run(command, args).map_err(|error| CliError::Credentials(error.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn credentials_command(_: &str, _: &[String]) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

fn developer_tcp_probe() -> Result<(), CliError> {
    use std::time::Duration;

    let transport = transport_tcp::DeveloperTcpTransport::connect(
        transport_tcp::DEFAULT_DEVELOPER_ADDRESS,
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .map_err(CliError::Transport)?;
    transport
        .verify_peer_available()
        .map_err(CliError::Transport)?;
    println!("developer_transport=tcp");
    println!("developer_endpoint={}", transport.peer());
    println!("developer_connection=ready");
    Ok(())
}

#[cfg(target_os = "linux")]
fn developer_tls_probe(tls12_compatibility: bool) -> Result<(), CliError> {
    let _ = tls12_compatibility;
    reject_completed_generated_identity_probe()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn developer_credential_probe(tls12_compatibility: bool) -> Result<(), CliError> {
    use std::path::Path;
    use std::time::Duration;

    let paths = credential_store::CredentialPaths::from(
        credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
            .map_err(|error| CliError::Credentials(error.to_string()))?,
    );
    let credentials = credential_store::load_credentials(&paths, true)
        .map_err(|error| CliError::Credentials(error.to_string()))?;
    let mut transport = transport_tcp::DeveloperTcpTransport::connect(
        transport_tcp::DEFAULT_DEVELOPER_ADDRESS,
        Duration::from_secs(2),
        Duration::from_millis(500),
    )
    .map_err(CliError::Transport)?;
    println!("developer_transport=tcp");
    println!("developer_endpoint={}", transport.peer());
    live_probe::run(&mut transport, tls12_compatibility, credentials.material)
}

#[cfg(target_os = "linux")]
fn developer_auth_discovery_probe(tls12_compatibility: bool) -> Result<(), CliError> {
    use std::path::Path;
    use std::time::Duration;

    connection_state::report(connection_state::ConnectionState::Ready);
    let cancel = cancellation::install_ctrlc_handler()?;
    let result = (|| -> Result<(), CliError> {
        let paths = credential_store::CredentialPaths::from(
            credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
                .map_err(|error| CliError::Credentials(error.to_string()))?,
        );
        let credentials = credential_store::load_credentials(&paths, true)
            .map_err(|error| CliError::Credentials(error.to_string()))?;
        connection_state::report(connection_state::ConnectionState::Connecting);
        let mut transport = transport_tcp::DeveloperTcpTransport::connect(
            transport_tcp::DEFAULT_DEVELOPER_ADDRESS,
            Duration::from_secs(2),
            Duration::from_millis(500),
        )
        .map_err(CliError::Transport)?;
        println!("developer_transport=tcp");
        println!("developer_endpoint={}", transport.peer());
        auth_discovery_probe::run(
            &mut transport,
            tls12_compatibility,
            credentials.material,
            auth_discovery_probe::VideoRenderTarget::Wayland,
            &cancel,
        )
    })();
    if result.is_err() {
        connection_state::report(connection_state::ConnectionState::Error);
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn developer_tls_probe(_: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(not(target_os = "linux"))]
fn developer_credential_probe(_: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(not(target_os = "linux"))]
fn developer_auth_discovery_probe(_: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

fn preflight() -> Result<(), CliError> {
    let inventory = platform_linux::system_inventory();
    println!("model={}", inventory.model);
    println!("os={}", inventory.os_name);
    println!("os_version={}", inventory.os_version);
    println!("architecture={}", inventory.architecture);
    println!("kernel={}", inventory.kernel);
    println!("supported_baseline={}", inventory.supported_baseline);
    for note in &inventory.notes {
        println!("note={note}");
    }
    print_radios(&platform_linux::discover_radios().map_err(CliError::Io)?);
    if inventory.supported_baseline {
        Ok(())
    } else {
        Err(CliError::UnsupportedPlatform)
    }
}

fn wireless(args: &[String]) -> Result<(), CliError> {
    let (wifi_preference, bluetooth_preference) = parse_preferences(args)?;
    let providers = platform_linux::discover_radios().map_err(CliError::Io)?;
    print_radios(&providers);

    let wifi = choose_provider(RadioKind::Wifi, &providers, &wifi_preference);
    let bluetooth = choose_provider(RadioKind::Bluetooth, &providers, &bluetooth_preference);
    println!("wifi_selection={}", wifi.explanation);
    println!("bluetooth_selection={}", bluetooth.explanation);
    Ok(())
}

fn parse_preferences(
    args: &[String],
) -> Result<(ProviderPreference, ProviderPreference), CliError> {
    let mut wifi = ProviderPreference::Auto;
    let mut bluetooth = ProviderPreference::Auto;
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let value = args.get(index + 1).ok_or_else(|| {
            CliError::Usage(format!(
                "{flag} requires auto, onboard, or a stable adapter ID"
            ))
        })?;
        match flag.as_str() {
            "--wifi" => wifi = ProviderPreference::parse(value),
            "--bluetooth" => bluetooth = ProviderPreference::parse(value),
            _ => return Err(CliError::Usage(format!("unknown wireless option: {flag}"))),
        }
        index += 2;
    }
    Ok((wifi, bluetooth))
}

fn print_radios(providers: &[RadioProvider]) {
    for radio in [RadioKind::Wifi, RadioKind::Bluetooth] {
        let matches: Vec<_> = providers
            .iter()
            .filter(|provider| provider.radio == radio)
            .collect();
        if matches.is_empty() {
            println!("radio={radio} state={}", CapabilityState::Absent);
            continue;
        }
        for provider in matches {
            println!(
                "radio={} state={} provider={} interface={} stable_id={} driver={} usb_id={} reason={}",
                provider.radio,
                provider.state,
                provider.provider,
                provider.interface_name,
                provider.stable_id,
                provider.driver.as_deref().unwrap_or("unknown"),
                provider.usb_id.as_deref().unwrap_or("none"),
                provider.reason.as_deref().unwrap_or("none")
            );
        }
    }
}

#[cfg(target_os = "linux")]
fn media_probe() -> Result<(), CliError> {
    use media_api::{DecoderKind, DecoderPolicy, VideoCodec, VideoMode, VideoRequest};

    let request = VideoRequest {
        codec: VideoCodec::H264,
        mode: VideoMode {
            width: 800,
            height: 480,
            frames_per_second: 30,
        },
    };
    let backend = media_gstreamer::GstreamerBackend::new()
        .map_err(|error| CliError::Media(error.to_string()))?;
    let capabilities = backend.available_decoders(&request);
    for capability in &capabilities {
        let kind = match capability.kind {
            DecoderKind::Hardware => "hardware",
            DecoderKind::Software => "software",
        };
        println!(
            "media_decoder={} codec={} kind={kind} mode={}x{}@{}",
            capability.id,
            capability.codec,
            request.mode.width,
            request.mode.height,
            request.mode.frames_per_second
        );
    }
    let selected = media_api::select_decoder(
        &request,
        &capabilities,
        DecoderPolicy {
            allow_software: true,
        },
    )
    .map_err(|error| CliError::Media(error.to_string()))?;
    let elements = backend
        .verify_pipeline_elements(selected)
        .map_err(|error| CliError::Media(error.to_string()))?;
    println!("media_selection={}", selected.id);
    println!(
        "media_pipeline={}!{}!{}!{}",
        elements.parser, elements.decoder, elements.converter, elements.sink
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn media_probe() -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

const DEFAULT_MIC_PROBE_SECONDS: u64 = 8;

#[cfg(target_os = "linux")]
fn media_mic_probe(seconds: Option<u64>) -> Result<(), CliError> {
    use media_gstreamer::{AudioCaptureSource, AudioFormat};
    use std::time::{Duration, Instant};

    let seconds = seconds.unwrap_or(DEFAULT_MIC_PROBE_SECONDS);
    let format = AudioFormat {
        sampling_rate: 48_000,
        channels: 1,
    };
    let backend = media_gstreamer::GstreamerBackend::new()
        .map_err(|error| CliError::Media(error.to_string()))?;
    let pipeline = backend
        .build_audio_capture_pipeline(
            format,
            AudioCaptureSource::Pulse,
            Duration::from_millis(200),
        )
        .map_err(|error| CliError::Media(error.to_string()))?;
    pipeline
        .start()
        .map_err(|error| CliError::Media(error.to_string()))?;

    println!("mic_probe_source=pipewire_pulse_default_input");
    println!(
        "mic_probe_rate={} mic_probe_channels={}",
        format.sampling_rate, format.channels
    );
    println!("mic_probe_duration_seconds={seconds}");

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut max_peak_db = f64::NEG_INFINITY;
    let mut max_rms_db = f64::NEG_INFINITY;
    let mut messages = 0u32;
    while Instant::now() < deadline {
        match pipeline.next_level(Duration::from_secs(2)) {
            Ok(Some(level)) => {
                messages += 1;
                max_peak_db = level.peak_db.iter().copied().fold(max_peak_db, f64::max);
                max_rms_db = level.rms_db.iter().copied().fold(max_rms_db, f64::max);
            }
            Ok(None) => {}
            Err(error) => {
                let _ = pipeline.shutdown();
                return Err(CliError::Media(error.to_string()));
            }
        }
    }
    pipeline
        .shutdown()
        .map_err(|error| CliError::Media(error.to_string()))?;

    println!("mic_probe_level_messages={messages}");
    println!("mic_probe_peak_db={max_peak_db:.1}");
    println!("mic_probe_rms_db={max_rms_db:.1}");
    println!("mic_probe_result=complete");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn media_mic_probe(_seconds: Option<u64>) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

fn parse_mic_probe_seconds(value: &str) -> Result<u64, CliError> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| CliError::Usage(format!("invalid mic-probe duration: {value}")))?;
    if !(1..=60).contains(&seconds) {
        return Err(CliError::Usage(
            "mic-probe duration must be between 1 and 60 seconds".into(),
        ));
    }
    Ok(seconds)
}

fn usb_list() -> Result<(), CliError> {
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let devices = backend.list_devices().map_err(CliError::Aoa)?;
    if devices.is_empty() {
        println!("usb_devices=none");
    } else {
        for device in devices {
            println!("usb_device={device}");
        }
    }
    Ok(())
}

fn parse_cycles(value: &str) -> Result<u32, CliError> {
    let cycles = value
        .parse::<u32>()
        .map_err(|_| CliError::Usage(format!("invalid cycle count: {value}")))?;
    if !(1..=10_000).contains(&cycles) {
        return Err(CliError::Usage(
            "cycle count must be between 1 and 10000".into(),
        ));
    }
    Ok(cycles)
}

fn parse_hold_seconds(value: &str) -> Result<u64, CliError> {
    let seconds = value
        .parse::<u64>()
        .map_err(|_| CliError::Usage(format!("invalid hold duration: {value}")))?;
    if !(1..=300).contains(&seconds) {
        return Err(CliError::Usage(
            "hold duration must be between 1 and 300 seconds".into(),
        ));
    }
    Ok(seconds)
}

#[cfg(target_os = "linux")]
fn usb_aoa(selector: &str) -> Result<(), CliError> {
    use std::time::Duration;
    use transport_api::{AoaIdentification, AoaMachine};

    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let devices = backend.list_devices().map_err(CliError::Aoa)?;
    let candidate = devices
        .into_iter()
        .find(|device| device.bus == bus && device.address == address)
        .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;

    println!("selected_device={candidate}");
    println!(
        "warning=performing documented generic AOA transition; this is not an Android Auto session"
    );
    let mut machine = AoaMachine::new(backend, Duration::from_secs(10));
    let outcome = machine
        .run(candidate, &AoaIdentification::milestone_one())
        .map_err(CliError::Aoa)?;
    println!("aoa_protocol_version={}", outcome.protocol_version);
    for state in outcome.transitions {
        println!("aoa_state={state:?}");
    }
    println!("bulk_device={}", outcome.transport.device);
    println!("bulk_interface={}", outcome.transport.interface_number);
    println!(
        "bulk_endpoints=in:{:#04x},out:{:#04x}",
        outcome.transport.bulk_in_endpoint, outcome.transport.bulk_out_endpoint
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn usb_aoa(_: &str) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_soak(selector: &str, cycles: usize) -> Result<(), CliError> {
    use std::time::Duration;
    use transport_api::{AoaIdentification, AoaMachine};

    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let devices = backend.list_devices().map_err(CliError::Aoa)?;
    let candidate = devices
        .into_iter()
        .find(|device| device.bus == bus && device.address == address)
        .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;
    if !transport_usb::is_accessory_id(candidate.vendor_id, candidate.product_id) {
        return Err(CliError::Usage(
            "soak requires a device already in AOA accessory mode; run usb aoa first".into(),
        ));
    }

    let start_fds = open_fd_count();
    let start_rss = resident_memory_kib();
    let mut machine = AoaMachine::new(backend, Duration::from_secs(10));
    for cycle in 1..=cycles {
        machine
            .run(candidate.clone(), &AoaIdentification::milestone_one())
            .map_err(CliError::Aoa)?;
        if cycle == 1 || cycle == cycles || cycle % 10 == 0 {
            println!("soak_cycle={cycle}/{cycles}");
        }
    }
    println!(
        "open_fds_start={start_fds} open_fds_end={}",
        open_fd_count()
    );
    println!(
        "rss_kib_start={start_rss} rss_kib_end={}",
        resident_memory_kib()
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn usb_soak(_: &str, _: usize) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_hold(selector: &str, seconds: u64) -> Result<(), CliError> {
    use std::time::Duration;

    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    let mut backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let candidate = backend
        .list_devices()
        .map_err(CliError::Aoa)?
        .into_iter()
        .find(|device| device.bus == bus && device.address == address)
        .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;
    if !transport_usb::is_accessory_id(candidate.vendor_id, candidate.product_id) {
        return Err(CliError::Usage(
            "hold requires a device already in AOA accessory mode; run usb aoa first".into(),
        ));
    }

    println!("hold_device={candidate}");
    println!("hold_seconds={seconds}");
    println!("hold_state=interface_claimed unplug_phone_now=true");
    match backend
        .hold_bulk_interface(&candidate, Duration::from_secs(seconds))
        .map_err(CliError::Aoa)?
    {
        transport_usb::HoldResult::Unplugged => {
            println!("hold_result=unplug_detected");
        }
        transport_usb::HoldResult::TimedOut => {
            println!("hold_result=timeout_phone_still_present");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn usb_hold(_: &str, _: u64) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_lines, clippy::items_after_statements)]
fn usb_tls_probe(selector: &str, tls12_compatibility: bool) -> Result<(), CliError> {
    use protocol_aap::{
        AASDK_MAX_FRAME_PAYLOAD_SIZE, ControlMessage, Encryption, FrameError, FrameHeader,
        FrameType, HandshakeAction, HandshakeEvent, HandshakeStateMachine, MessageAssembler,
        MessageType, ProtocolLimits, TlsClient, decode_frame, encode_frame,
    };
    use security_openssl::{OpenSslTlsClient, TlsVersionPolicy, generate_ephemeral_credentials};
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};
    use transport_api::{AoaIdentification, AoaMachine};

    const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
    const IO_TIMEOUT: Duration = Duration::from_millis(500);
    const MAX_ACCUMULATED_BYTES: usize = 64 * 1024;

    reject_completed_generated_identity_probe()?;
    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let candidate = backend
        .list_devices()
        .map_err(CliError::Aoa)?
        .into_iter()
        .find(|device| device.bus == bus && device.address == address)
        .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;

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
    println!("probe_state=preparing_accessory_transport");

    let mut aoa = AoaMachine::new(backend, PROBE_TIMEOUT);
    let outcome = aoa
        .run(candidate, &AoaIdentification::aasdk_compatibility_probe())
        .map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let mut transport = backend
        .open_claimed_session_transport(&outcome.transport.device)
        .map_err(CliError::Aoa)?;

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
    process_probe_actions(
        &mut actions,
        &mut handshake,
        &mut tls,
        &mut transport,
        limits,
    )?;
    println!("probe_state=version_request_sent");

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut received = Vec::new();
    let mut read_buffer = vec![0_u8; AASDK_MAX_FRAME_PAYLOAD_SIZE + 8];
    let mut assembler =
        MessageAssembler::new(1).map_err(|error| CliError::Protocol(error.to_string()))?;

    while Instant::now() < deadline {
        let size = transport
            .read(&mut read_buffer, IO_TIMEOUT)
            .map_err(CliError::Aoa)?;
        if size == 0 {
            continue;
        }
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
            if process_probe_actions(
                &mut actions,
                &mut handshake,
                &mut tls,
                &mut transport,
                limits,
            )? {
                println!("probe_result=tls_handshake_complete");
                println!("probe_stop=before_authentication_and_service_discovery");
                return Ok(());
            }
        }
    }

    println!("probe_tls_state={}", tls.handshake_state());
    return Err(CliError::Protocol(
        "TLS probe timed out before handshake completion".into(),
    ));

    fn process_probe_actions(
        actions: &mut VecDeque<HandshakeAction>,
        handshake: &mut HandshakeStateMachine,
        tls: &mut OpenSslTlsClient,
        transport: &mut transport_usb::LibUsbBulkTransport,
        limits: ProtocolLimits,
    ) -> Result<bool, CliError> {
        while let Some(action) = actions.pop_front() {
            match action {
                HandshakeAction::SendControl(message) => {
                    send_probe_control(transport, &message, limits)?;
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

    fn finish_or_queue_tls(
        progress: &protocol_aap::TlsProgress,
        actions: &mut VecDeque<HandshakeAction>,
        handshake: &mut HandshakeStateMachine,
        transport: &mut transport_usb::LibUsbBulkTransport,
        limits: ProtocolLimits,
    ) -> Result<bool, CliError> {
        if progress.complete {
            if !progress.outbound.is_empty() {
                send_probe_control(
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

    fn send_probe_control(
        transport: &mut transport_usb::LibUsbBulkTransport,
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
        transport
            .write_all(&frame, Duration::from_secs(2))
            .map_err(CliError::Aoa)
    }
}

#[cfg(not(target_os = "linux"))]
fn usb_tls_probe(_: &str, _: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_credential_probe(selector: &str, tls12_compatibility: bool) -> Result<(), CliError> {
    use std::path::Path;
    use std::time::Duration;
    use transport_api::{AoaIdentification, AoaMachine};

    const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

    let paths = credential_store::CredentialPaths::from(
        credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
            .map_err(|error| CliError::Credentials(error.to_string()))?,
    );
    let credentials = credential_store::load_credentials(&paths, true)
        .map_err(|error| CliError::Credentials(error.to_string()))?;

    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let candidate = backend
        .list_devices()
        .map_err(CliError::Aoa)?
        .into_iter()
        .find(|device| device.bus == bus && device.address == address)
        .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;

    println!("probe_authorization=operator_confirmed");
    println!("probe_payload_logging=disabled");
    println!("probe_state=preparing_accessory_transport");
    let mut aoa = AoaMachine::new(backend, PROBE_TIMEOUT);
    let outcome = aoa
        .run(candidate, &AoaIdentification::receiver_probe())
        .map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let mut transport = backend
        .open_claimed_session_transport(&outcome.transport.device)
        .map_err(CliError::Aoa)?;
    live_probe::run(&mut transport, tls12_compatibility, credentials.material)
}

#[cfg(not(target_os = "linux"))]
fn usb_credential_probe(_: &str, _: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_auth_discovery_probe(selector: &str, tls12_compatibility: bool) -> Result<(), CliError> {
    use std::path::Path;
    use std::time::Duration;
    use transport_api::{AoaIdentification, AoaMachine};

    const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

    connection_state::report(connection_state::ConnectionState::Ready);
    let cancel = cancellation::install_ctrlc_handler()?;
    let result = (|| -> Result<(), CliError> {
        let paths = credential_store::CredentialPaths::from(
            credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
                .map_err(|error| CliError::Credentials(error.to_string()))?,
        );
        let credentials = credential_store::load_credentials(&paths, true)
            .map_err(|error| CliError::Credentials(error.to_string()))?;

        let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
        let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
        let candidate = backend
            .list_devices()
            .map_err(CliError::Aoa)?
            .into_iter()
            .find(|device| device.bus == bus && device.address == address)
            .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;

        println!("probe_authorization=operator_confirmed");
        println!("probe_payload_logging=disabled");
        println!("probe_state=preparing_accessory_transport");
        connection_state::report(connection_state::ConnectionState::Connecting);
        let mut aoa = AoaMachine::new(backend, PROBE_TIMEOUT);
        let outcome = aoa
            .run(candidate, &AoaIdentification::receiver_probe())
            .map_err(CliError::Aoa)?;
        let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
        let mut transport = backend
            .open_claimed_session_transport(&outcome.transport.device)
            .map_err(CliError::Aoa)?;
        auth_discovery_probe::run(
            &mut transport,
            tls12_compatibility,
            credentials.material,
            auth_discovery_probe::VideoRenderTarget::Wayland,
            &cancel,
        )
    })();
    if result.is_err() {
        connection_state::report(connection_state::ConnectionState::Error);
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn usb_auth_discovery_probe(_: &str, _: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_session_supervisor(
    selector: &str,
    tls12_compatibility: bool,
    max_cycles: Option<u32>,
    force_disconnect_each_cycle: bool,
) -> Result<(), CliError> {
    session_supervisor::run(
        selector,
        tls12_compatibility,
        max_cycles,
        force_disconnect_each_cycle,
    )
}

#[cfg(not(target_os = "linux"))]
fn usb_session_supervisor(_: &str, _: bool, _: Option<u32>, _: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_gtk_dev_ui(selector: &str, tls12_compatibility: bool) -> Result<(), CliError> {
    gtk_dev_ui::run(selector, tls12_compatibility)
}

#[cfg(not(target_os = "linux"))]
fn usb_gtk_dev_ui(_: &str, _: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

#[cfg(target_os = "linux")]
fn usb_wireless_bootstrap_probe(tls12_compatibility: bool) -> Result<(), CliError> {
    wireless_bootstrap::run(tls12_compatibility)
}

#[cfg(not(target_os = "linux"))]
fn usb_wireless_bootstrap_probe(_: bool) -> Result<(), CliError> {
    Err(CliError::UnsupportedPlatform)
}

fn reject_completed_generated_identity_probe() -> Result<(), CliError> {
    Err(CliError::Usage(
        "generated-identity phone probes are permanently disabled after the recorded Android Auto error-7 rejection"
            .into(),
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn open_fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd").map_or(0, Iterator::count)
}

#[cfg(target_os = "linux")]
pub(crate) fn resident_memory_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        })
        .unwrap_or(0)
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    UnsupportedPlatform,
    Io(std::io::Error),
    #[cfg(target_os = "linux")]
    Media(String),
    Aoa(transport_api::AoaError),
    Protocol(String),
    #[cfg(target_os = "linux")]
    Credentials(String),
    Transport(transport_api::TransportError),
    /// An operator-requested `SIGINT` (Ctrl-C) reached a probe's
    /// cooperative-cancellation check (`cancellation::CancellationFlag`).
    /// Distinct from every other variant here: it's not a failure, so it
    /// gets its own exit code rather than reusing `Protocol`'s or being
    /// folded into success — a caller (a human, or `session-supervisor`'s
    /// retry loop) needs to be able to tell "the operator asked to stop"
    /// apart from every other outcome.
    #[cfg(target_os = "linux")]
    Cancelled,
}

impl CliError {
    const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::UnsupportedPlatform => 10,
            Self::Io(_) => 12,
            #[cfg(target_os = "linux")]
            Self::Media(_) => 18,
            Self::Aoa(transport_api::AoaError::PermissionDenied(_)) => 13,
            Self::Aoa(transport_api::AoaError::Unsupported(_)) => 14,
            Self::Aoa(transport_api::AoaError::TimedOut(_)) => 15,
            Self::Aoa(transport_api::AoaError::Unplugged) => 16,
            Self::Aoa(_) => 17,
            Self::Protocol(_) => 19,
            #[cfg(target_os = "linux")]
            Self::Credentials(_) => 21,
            Self::Transport(_) => 20,
            #[cfg(target_os = "linux")]
            Self::Cancelled => 22,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "usage: {message}"),
            Self::UnsupportedPlatform => write!(
                f,
                "this command requires 64-bit Raspberry Pi OS Trixie on CM4, CM5, Pi 4, or Pi 5"
            ),
            Self::Io(error) => write!(f, "I/O: {error}"),
            #[cfg(target_os = "linux")]
            Self::Media(error) => write!(f, "media: {error}"),
            Self::Aoa(error) => error.fmt(f),
            Self::Protocol(error) => write!(f, "protocol probe: {error}"),
            #[cfg(target_os = "linux")]
            Self::Credentials(error) => write!(f, "credentials: {error}"),
            Self::Transport(error) => error.fmt(f),
            #[cfg(target_os = "linux")]
            Self::Cancelled => write!(f, "cancelled by operator (Ctrl-C)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_independent_provider_preferences() {
        let args = vec![
            "--wifi".into(),
            "onboard".into(),
            "--bluetooth".into(),
            "usb:1234:5678:1-2".into(),
        ];
        let (wifi, bluetooth) = parse_preferences(&args).expect("valid preferences");
        assert_eq!(wifi, ProviderPreference::Onboard);
        assert_eq!(
            bluetooth,
            ProviderPreference::StableId("usb:1234:5678:1-2".into())
        );
    }

    #[test]
    fn rejects_option_without_value() {
        assert!(parse_preferences(&["--wifi".into()]).is_err());
    }

    #[test]
    fn validates_soak_cycle_bounds() {
        assert_eq!(parse_cycles("100").expect("valid cycles"), 100);
        assert!(parse_cycles("0").is_err());
        assert!(parse_cycles("10001").is_err());
        assert!(parse_cycles("many").is_err());
    }

    #[test]
    fn validates_hold_duration_bounds() {
        assert_eq!(parse_hold_seconds("30").expect("valid duration"), 30);
        assert!(parse_hold_seconds("0").is_err());
        assert!(parse_hold_seconds("301").is_err());
        assert!(parse_hold_seconds("long").is_err());
    }

    #[test]
    fn live_tls_probe_requires_explicit_opt_in() {
        let args = vec![
            "usb".into(),
            "tls-probe".into(),
            "--device".into(),
            "1:2".into(),
        ];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));
    }

    #[test]
    fn credential_probe_requires_explicit_opt_in() {
        let args = vec![
            "usb".into(),
            "credential-probe".into(),
            "--device".into(),
            "1:2".into(),
        ];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));
    }

    #[test]
    fn auth_discovery_probe_requires_explicit_opt_in() {
        let args = vec![
            "usb".into(),
            "auth-discovery-probe".into(),
            "--device".into(),
            "1:2".into(),
        ];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));

        let args = vec!["developer".into(), "auth-discovery-probe".into()];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));
    }

    #[test]
    fn session_supervisor_requires_explicit_opt_in() {
        let args = vec![
            "usb".into(),
            "session-supervisor".into(),
            "--device".into(),
            "1:2".into(),
        ];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));
    }

    #[test]
    fn session_supervisor_rejects_invalid_max_cycles() {
        let args = vec![
            "usb".into(),
            "session-supervisor".into(),
            "--device".into(),
            "1:2".into(),
            "--allow-live-aap".into(),
            "--max-cycles".into(),
            "0".into(),
        ];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));
    }

    #[test]
    fn session_supervisor_force_disconnect_still_rejects_invalid_max_cycles() {
        let args = vec![
            "usb".into(),
            "session-supervisor".into(),
            "--device".into(),
            "1:2".into(),
            "--allow-live-aap".into(),
            "--max-cycles".into(),
            "0".into(),
            "--force-disconnect-each-cycle".into(),
        ];
        assert!(matches!(run(&args), Err(CliError::Usage(_))));
    }

    #[test]
    fn completed_generated_identity_probe_stays_disabled() {
        let args = vec![
            "developer".into(),
            "tls-probe".into(),
            "--allow-live-aap".into(),
        ];
        let error = run(&args).expect_err("completed experiment must stay disabled");
        assert!(error.to_string().contains("permanently disabled"));
    }
}
