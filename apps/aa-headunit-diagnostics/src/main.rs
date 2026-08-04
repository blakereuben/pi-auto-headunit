use platform_api::{
    CapabilityState, ProviderPreference, RadioKind, RadioProvider, choose_provider,
};
use std::env;

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

fn run(args: &[String]) -> Result<(), CliError> {
    match args {
        [command] if command == "preflight" => preflight(),
        [command] if command == "wireless" => wireless(&[]),
        [command, rest @ ..] if command == "wireless" => wireless(rest),
        [group, command] if group == "usb" && command == "list" => usb_list(),
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
           usb list\n\
           usb aoa --device BUS:ADDRESS\n\
           usb soak --device BUS:ADDRESS --cycles COUNT\n\
         \n\
         The AOA command sends documented USB vendor requests only to the explicitly selected device.",
        env!("CARGO_PKG_VERSION")
    );
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
fn open_fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd").map_or(0, Iterator::count)
}

#[cfg(target_os = "linux")]
fn resident_memory_kib() -> u64 {
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
    Aoa(transport_api::AoaError),
}

impl CliError {
    const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::UnsupportedPlatform => 10,
            Self::Io(_) => 12,
            Self::Aoa(transport_api::AoaError::PermissionDenied(_)) => 13,
            Self::Aoa(transport_api::AoaError::Unsupported(_)) => 14,
            Self::Aoa(transport_api::AoaError::TimedOut(_)) => 15,
            Self::Aoa(transport_api::AoaError::Unplugged) => 16,
            Self::Aoa(_) => 17,
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
            Self::Aoa(error) => error.fmt(f),
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
}
