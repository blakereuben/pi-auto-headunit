//! Read-only Linux/Raspberry Pi inventory. No shell commands are executed.

use platform_api::{CapabilityState, ProviderKind, RadioKind, RadioProvider, SystemInventory};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

pub mod touch;

#[must_use]
pub fn system_inventory() -> SystemInventory {
    let os_release = parse_key_value_file(Path::new("/etc/os-release")).unwrap_or_default();
    let model = read_trimmed(Path::new("/proc/device-tree/model"))
        .unwrap_or_else(|_| "unknown (not running on a Device Tree Raspberry Pi target)".into());
    let kernel =
        read_trimmed(Path::new("/proc/sys/kernel/osrelease")).unwrap_or_else(|_| "unknown".into());
    let os_name = os_release
        .get("PRETTY_NAME")
        .or_else(|| os_release.get("NAME"))
        .cloned()
        .unwrap_or_else(|| std::env::consts::OS.into());
    let os_version = os_release
        .get("VERSION_ID")
        .cloned()
        .unwrap_or_else(|| "unknown".into());
    let architecture = std::env::consts::ARCH.to_owned();
    let is_pi = model.to_ascii_lowercase().contains("raspberry pi")
        || model.to_ascii_lowercase().contains("compute module");
    let supported_board = [
        "raspberry pi 4",
        "raspberry pi 5",
        "compute module 4",
        "compute module 5",
    ]
    .iter()
    .any(|needle| model.to_ascii_lowercase().contains(needle));
    let supported_baseline = cfg!(target_os = "linux")
        && architecture == "aarch64"
        && os_version == "13"
        && is_pi
        && supported_board;

    let mut notes = Vec::new();
    if architecture != "aarch64" {
        notes.push(format!("expected aarch64, detected {architecture}"));
    }
    if os_version != "13" {
        notes.push(format!(
            "initial baseline is Raspberry Pi OS/Debian 13 Trixie; detected version {os_version}"
        ));
    }
    if !supported_board {
        notes.push(format!(
            "model is outside the initial support matrix: {model}"
        ));
    }

    SystemInventory {
        model,
        os_name,
        os_version,
        architecture,
        kernel,
        supported_baseline,
        notes,
    }
}

pub fn discover_radios() -> io::Result<Vec<RadioProvider>> {
    let mut providers = Vec::new();
    discover_wifi(Path::new("/sys/class/net"), &mut providers)?;
    discover_bluetooth(Path::new("/sys/class/bluetooth"), &mut providers)?;
    providers.sort_by(|left, right| {
        left.radio
            .to_string()
            .cmp(&right.radio.to_string())
            .then_with(|| left.stable_id.cmp(&right.stable_id))
    });
    Ok(providers)
}

fn discover_wifi(root: &Path, providers: &mut Vec<RadioProvider>) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let interface_name = entry.file_name().to_string_lossy().into_owned();
        let interface = entry.path();
        if !interface.join("wireless").exists() && !interface.join("phy80211").exists() {
            continue;
        }
        providers.push(provider_from_sysfs(
            RadioKind::Wifi,
            interface_name,
            &interface,
        ));
    }
    Ok(())
}

fn discover_bluetooth(root: &Path, providers: &mut Vec<RadioProvider>) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let interface_name = entry.file_name().to_string_lossy().into_owned();
        if !interface_name.starts_with("hci") {
            continue;
        }
        providers.push(provider_from_sysfs(
            RadioKind::Bluetooth,
            interface_name,
            &entry.path(),
        ));
    }
    Ok(())
}

fn provider_from_sysfs(
    radio: RadioKind,
    interface_name: String,
    interface: &Path,
) -> RadioProvider {
    let device_link = interface.join("device");
    let canonical = fs::canonicalize(&device_link).unwrap_or_else(|_| device_link.clone());
    let canonical_text = canonical.to_string_lossy().into_owned();
    let provider = if canonical_text.contains("/usb") {
        ProviderKind::Usb
    } else if canonical_text.contains("/platform/")
        || canonical_text.contains("/mmc")
        || canonical_text.contains("/soc/")
    {
        ProviderKind::Onboard
    } else {
        ProviderKind::Other
    };
    let driver = driver_name(&device_link);
    let (blocked, block_reason) = rfkill_state(interface, &canonical);
    let state = if blocked {
        CapabilityState::Disabled
    } else if driver.is_none() {
        CapabilityState::Degraded
    } else {
        CapabilityState::Ready
    };
    let usb_id = find_usb_id(&canonical);
    let stable_id = stable_id(provider, &canonical, usb_id.as_deref(), &interface_name);

    RadioProvider {
        stable_id,
        interface_name,
        radio,
        provider,
        state,
        driver,
        usb_id,
        reason: block_reason.or_else(|| {
            (state == CapabilityState::Degraded).then(|| "no bound kernel driver found".into())
        }),
    }
}

fn driver_name(device_link: &Path) -> Option<String> {
    let mut cursor = fs::canonicalize(device_link).ok()?;
    loop {
        let driver = cursor.join("driver");
        if let Ok(target) = fs::canonicalize(driver) {
            return target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
        }
        if !cursor.pop() {
            return None;
        }
    }
}

fn rfkill_state(interface: &Path, device: &Path) -> (bool, Option<String>) {
    for root in [interface.to_path_buf(), interface.join("phy80211")] {
        if let Some(state) = blocked_rfkill_child(&root) {
            return state;
        }
    }

    let mut cursor = Some(device);
    while let Some(path) = cursor {
        if let Some(state) = blocked_rfkill_child(path) {
            return state;
        }
        cursor = path.parent();
    }
    (false, None)
}

fn blocked_rfkill_child(root: &Path) -> Option<(bool, Option<String>)> {
    for entry in fs::read_dir(root).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("rfkill") {
            continue;
        }
        let base = entry.path();
        let soft = read_trimmed(&base.join("soft")).unwrap_or_default() == "1";
        let hard = read_trimmed(&base.join("hard")).unwrap_or_default() == "1";
        if soft || hard {
            let reason = match (soft, hard) {
                (true, true) => "soft- and hard-blocked by rfkill",
                (true, false) => "soft-blocked by rfkill",
                (false, true) => "hard-blocked by rfkill",
                (false, false) => unreachable!(),
            };
            return Some((true, Some(reason.into())));
        }
    }
    None
}

fn find_usb_id(device: &Path) -> Option<String> {
    let mut cursor = Some(device);
    while let Some(path) = cursor {
        let vendor = read_trimmed(&path.join("idVendor"));
        let product = read_trimmed(&path.join("idProduct"));
        if let (Ok(vendor), Ok(product)) = (vendor, product) {
            return Some(format!("{vendor}:{product}"));
        }
        cursor = path.parent();
    }
    None
}

fn stable_id(
    provider: ProviderKind,
    device: &Path,
    usb_id: Option<&str>,
    interface_name: &str,
) -> String {
    match provider {
        ProviderKind::Usb => {
            let path = device.file_name().map_or_else(
                || interface_name.into(),
                |name| name.to_string_lossy().into_owned(),
            );
            format!("usb:{}:{path}", usb_id.unwrap_or("unknown"))
        }
        ProviderKind::Onboard => format!("onboard:{interface_name}"),
        ProviderKind::Other => format!("other:{interface_name}"),
    }
}

fn read_trimmed(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes)
        .trim_matches(char::from(0))
        .trim()
        .to_owned())
}

fn parse_key_value_file(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.into(), raw_value.trim_matches('"').into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn preference_parser_is_case_insensitive() {
        assert_eq!(
            platform_api::ProviderPreference::parse("AUTO"),
            platform_api::ProviderPreference::Auto
        );
    }

    #[test]
    fn stable_usb_id_does_not_depend_on_interface_enumeration_name() {
        let path = std::path::PathBuf::from("/sys/devices/platform/soc/usb1/1-2/1-2.3");
        assert_eq!(
            stable_id(ProviderKind::Usb, &path, Some("1234:5678"), "wlan7"),
            "usb:1234:5678:1-2.3"
        );
    }

    #[test]
    fn detects_rfkill_beneath_wifi_phy() {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "aa-headunit-rfkill-{}-{sequence}",
            std::process::id()
        ));
        let interface = root.join("wlan1");
        let rfkill = interface.join("phy80211/rfkill3");
        fs::create_dir_all(&rfkill).expect("create rfkill fixture");
        fs::write(rfkill.join("soft"), "1\n").expect("write soft state");
        fs::write(rfkill.join("hard"), "0\n").expect("write hard state");

        assert_eq!(
            rfkill_state(&interface, &root.join("device")),
            (true, Some("soft-blocked by rfkill".into()))
        );

        fs::remove_dir_all(root).expect("remove rfkill fixture");
    }
}
