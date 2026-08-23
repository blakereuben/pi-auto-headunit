//! `usb wireless-bootstrap-probe` — a minimal proof of concept that
//! wireless Android Auto's cold-start bootstrap (no prior wired pairing)
//! can actually work, not just that a source for it exists. See
//! `docs/protocol/wireless-source-assessment.md` for the full protocol
//! research and citation trail this implements, and the approved plan
//! history for why this is deliberately the smallest possible slice, not
//! the full M7 milestone (no persistence, no reconnect loop, no soak
//! testing, no multi-radio provider selection).
//!
//! Sequence ([`bootstrap_wireless_transport`]): bring up a Wi-Fi access
//! point on `wlan0` (`hostapd` + `dnsmasq`, matching `f-io/LIVI`'s own
//! documented runtime dependencies) → become Bluetooth-discoverable and
//! accept one RFCOMM connection (`transport_bluetooth`) → drive the `aaw`
//! bootstrap exchange (`protocol_aap::{encode_aaw_message,
//! decode_aaw_message, ...}`) → once the phone reports success, accept
//! its incoming Wi-Fi connection (`transport_tcp::WirelessTcpTransport`)
//! → hand that transport to the exact same, completely unmodified
//! `auth_discovery_probe::run` every other command already calls. [`run`]
//! (`usb wireless-bootstrap-probe`) is this sequence alone, under
//! `VideoRenderTarget::Wayland`; `usb wireless-kiosk`
//! (`gtk_dev_ui::discover_and_run_kiosk_session`'s `Wireless` branch)
//! reuses [`bootstrap_wireless_transport`] directly, under
//! `VideoRenderTarget::Gtk4Window` instead, so head-unit gestures
//! (arm-swipe → settings/quick-controls/screen-off) actually work over a
//! wireless session — `Wayland` never wires a `TouchSettingsHandoff` (see
//! `auth_discovery_probe::touch_settings_handoff`'s own doc comment),
//! which is why they didn't the first time this was tried (2026-08-23).
//!
//! **Honesty up front**: unlike the wired protocol, this is reverse-
//! engineered, non-official behaviour with real but not first-party-
//! confirmed precedent. A real failure at the Bluetooth-discovery step —
//! the phone never recognising this device as wireless-Android-Auto-
//! capable — is a genuine possible outcome here, not just a formality.
//! Treat this the way `docs/protocol/error-2-investigation.md` treated
//! the wired protocol: attempt, observe real phone behaviour, adjust.

use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures_util::TryStreamExt;
use protocol_aap::{
    AawMessageId, AawStatus, AccessPointType, WifiInfoResponse, WifiSecurityMode, WifiStartRequest,
    decode_aaw_message, decode_wifi_connection_status, decode_wifi_start_response,
    encode_aaw_message, encode_wifi_info_response, encode_wifi_start_request,
};
use transport_api::SessionTransport;
use transport_bluetooth::accept_wireless_bootstrap_connection;
use transport_tcp::WirelessTcpTransport;

use crate::CliError;
use crate::cancellation;
use crate::connection_state::{self, ConnectionState};

const WLAN_INTERFACE: &str = "wlan0";
const AP_IP: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);
const AP_PREFIX_LEN: u8 = 24;
const AP_DHCP_RANGE_START: &str = "192.168.4.10";
const AP_DHCP_RANGE_END: &str = "192.168.4.50";
const AP_PORT: u16 = 5288;

// Raised from an initial 60s: real-hardware trials found plain OS-level
// Bluetooth pairing alone doesn't trigger the RFCOMM connection on the
// AA-Wireless UUID — only the Android Auto app itself does, by noticing
// an already-paired device advertising it, which needs enough real time
// for the operator to pair at the OS level, then separately open the
// Android Auto app and let it notice.
const BLUETOOTH_ACCEPT_TIMEOUT: Duration = Duration::from_secs(180);
/// Raised from an initial 10s — real-hardware finding, 2026-08-23: a
/// single `transport.receive()` call bounds the *entire* wait for each
/// `aaw` message (`receive_aaw_message` gives up on its very first
/// timeout, it doesn't loop-and-retry for an overall longer budget — see
/// that function's own doc comment). Most `aaw` reads really are
/// near-instant protocol acks, but the last one in the exchange,
/// `WifiConnectionStatus`, only arrives after the phone has *actually*
/// associated to the new Wi-Fi network and obtained a DHCP lease — a
/// genuine multi-second real-world operation, not a protocol
/// round-trip, especially the first time a phone sees a brand-new SSID.
/// 10s was intermittently too tight for that specific wait — it timed
/// out even with the operator actively watching their phone, nothing to
/// do with attention. Matches [`WIFI_ACCEPT_TIMEOUT`]'s own existing 30s
/// precedent for "waiting on a real phone-side Wi-Fi operation."
const BLUETOOTH_IO_TIMEOUT: Duration = Duration::from_secs(30);
const WIFI_ACCEPT_TIMEOUT: Duration = Duration::from_secs(30);
const WIFI_IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Hostapd/dnsmasq need a moment to actually start listening after being
/// spawned; a fixed pause is simpler and sufficient for a one-shot proof
/// of concept than parsing their log output for a readiness line.
const AP_STARTUP_SETTLE: Duration = Duration::from_millis(1500);

pub(crate) fn run(tls12_compatibility: bool) -> Result<(), CliError> {
    connection_state::report(ConnectionState::Ready);
    let cancel = cancellation::install_ctrlc_handler()?;
    println!("probe_authorization=operator_confirmed");
    println!("probe_payload_logging=disabled");

    let paths = credential_store::CredentialPaths::from(
        credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
            .map_err(|error| CliError::Credentials(error.to_string()))?,
    );
    let credentials = credential_store::load_credentials(&paths, true)
        .map_err(|error| CliError::Credentials(error.to_string()))?;

    let (mut transport, _ap_guard) = bootstrap_wireless_transport()?;

    let result = crate::auth_discovery_probe::run(
        &mut transport,
        tls12_compatibility,
        credentials.material,
        crate::auth_discovery_probe::VideoRenderTarget::Wayland,
        &cancel,
    );
    if result.is_err() {
        connection_state::report(ConnectionState::Error);
    }
    result
}

/// The transport-acquisition sequence shared by [`run`] (the standalone
/// `usb wireless-bootstrap-probe` diagnostic, which hands the result to
/// `auth_discovery_probe::run` under `VideoRenderTarget::Wayland`) and
/// `usb wireless-kiosk`
/// (`gtk_dev_ui::discover_and_run_kiosk_session`'s `Wireless` branch,
/// which uses `VideoRenderTarget::Gtk4Window` instead so gestures work):
/// bring up this device's own Wi-Fi access point, wait for a phone to
/// pair over Bluetooth and complete the `aaw` credential handoff, then
/// accept that phone's resulting Wi-Fi connection.
///
/// Returns the [`WifiAccessPoint`] guard alongside the transport — the
/// caller must keep it alive for the whole `AAP` session that follows,
/// not just this handshake, since it's the actual network the phone
/// stays connected to for streaming. Dropping it tears down
/// `hostapd`/`dnsmasq`/the interface.
pub(crate) fn bootstrap_wireless_transport()
-> Result<(WirelessTcpTransport, WifiAccessPoint), CliError> {
    let ssid = format!("aa-headunit-{}", random_hex(2)?);
    let password = random_hex(8)?;
    let bssid = wlan_mac_address()?;

    println!("wireless_bootstrap_state=bringing_up_access_point");
    let ap_guard = WifiAccessPoint::start(&ssid, &password)?;
    println!("wireless_bootstrap_state=access_point_ready ssid={ssid}");

    connection_state::report(ConnectionState::Connecting);
    println!("wireless_bootstrap_state=bluetooth_discoverable_waiting_for_phone");
    let mut bluetooth =
        accept_wireless_bootstrap_connection(BLUETOOTH_ACCEPT_TIMEOUT, BLUETOOTH_IO_TIMEOUT)
            .map_err(|error| CliError::Protocol(error.to_string()))?;
    println!("wireless_bootstrap_state=bluetooth_connected");

    run_aaw_bootstrap(&mut bluetooth, &ssid, &password, &bssid)?;
    drop(bluetooth);

    println!("wireless_bootstrap_state=waiting_for_wifi_connection");
    let transport = WirelessTcpTransport::listen(
        SocketAddr::new(IpAddr::V4(AP_IP), AP_PORT),
        WIFI_ACCEPT_TIMEOUT,
        WIFI_IO_TIMEOUT,
    )
    .map_err(CliError::Transport)?;
    println!(
        "wireless_bootstrap_state=wifi_connected peer={}",
        transport.peer()
    );

    Ok((transport, ap_guard))
}

/// Drives the `aaw` exchange over an already-connected Bluetooth RFCOMM
/// transport: `WifiStartRequest` → (wait for) `WifiInfoRequest` →
/// `WifiInfoResponse` → (wait for) `WifiStartResponse` → (wait for)
/// `WifiConnectionStatus`. Fails closed on any unexpected message or a
/// non-success status — this is the one part of the whole plan with
/// real, acknowledged uncertainty about whether it works at all against
/// a real phone (see module doc comment).
fn run_aaw_bootstrap(
    transport: &mut impl SessionTransport,
    ssid: &str,
    password: &str,
    bssid: &str,
) -> Result<(), CliError> {
    let start_request = WifiStartRequest {
        ip_address: AP_IP.to_string(),
        port: u32::from(AP_PORT),
    };
    send_aaw_message(
        transport,
        AawMessageId::WifiStartRequest,
        &encode_wifi_start_request(&start_request),
    )?;
    println!("wireless_bootstrap_state=wifi_start_request_sent");

    let (message_id, _body) = receive_aaw_message(transport)?;
    if message_id.wire_value() != AawMessageId::WifiInfoRequest.wire_value() {
        return Err(CliError::Protocol(format!(
            "expected WifiInfoRequest, got aaw message id {}",
            message_id.wire_value()
        )));
    }
    println!("wireless_bootstrap_state=wifi_info_request_received");

    let info_response = WifiInfoResponse {
        ssid: ssid.to_string(),
        password: password.to_string(),
        bssid: bssid.to_string(),
        security_mode: WifiSecurityMode::Wpa2Personal,
        access_point_type: Some(AccessPointType::Static),
    };
    send_aaw_message(
        transport,
        AawMessageId::WifiInfoResponse,
        &encode_wifi_info_response(&info_response),
    )?;
    println!("wireless_bootstrap_state=wifi_info_response_sent");

    let (message_id, body) = receive_aaw_message(transport)?;
    if message_id.wire_value() != AawMessageId::WifiStartResponse.wire_value() {
        return Err(CliError::Protocol(format!(
            "expected WifiStartResponse, got aaw message id {}",
            message_id.wire_value()
        )));
    }
    let start_response =
        decode_wifi_start_response(&body).map_err(|error| CliError::Protocol(error.to_string()))?;
    println!(
        "wireless_bootstrap_state=wifi_start_response_received status={:?}",
        start_response.status
    );
    if start_response.status != AawStatus::Success {
        return Err(CliError::Protocol(format!(
            "phone reported WifiStartResponse status {:?}",
            start_response.status
        )));
    }

    let (message_id, body) = receive_aaw_message(transport)?;
    if message_id.wire_value() != AawMessageId::WifiConnectionStatus.wire_value() {
        return Err(CliError::Protocol(format!(
            "expected WifiConnectionStatus, got aaw message id {}",
            message_id.wire_value()
        )));
    }
    let connection_status = decode_wifi_connection_status(&body)
        .map_err(|error| CliError::Protocol(error.to_string()))?;
    println!(
        "wireless_bootstrap_state=wifi_connection_status_received status={:?}",
        connection_status.status
    );
    if connection_status.status != AawStatus::Success {
        return Err(CliError::Protocol(format!(
            "phone reported WifiConnectionStatus {:?}: {}",
            connection_status.status,
            connection_status.error_message.as_deref().unwrap_or("")
        )));
    }
    Ok(())
}

fn send_aaw_message(
    transport: &mut impl SessionTransport,
    message_id: AawMessageId,
    body: &[u8],
) -> Result<(), CliError> {
    let encoded = encode_aaw_message(message_id, body)
        .map_err(|error| CliError::Protocol(format!("failed to encode aaw message: {error}")))?;
    transport.send_all(&encoded).map_err(CliError::Transport)
}

/// Reads and accumulates bytes from `transport` until one complete `aaw`
/// message is available, mirroring how `auth_discovery_probe.rs` already
/// accumulates AAP frames — this transport gives no delivery-boundary
/// guarantee either.
fn receive_aaw_message(
    transport: &mut impl SessionTransport,
) -> Result<(AawMessageId, Vec<u8>), CliError> {
    let mut received = Vec::new();
    let mut read_buffer = vec![0_u8; 1024];
    loop {
        match decode_aaw_message(&received) {
            Ok(decoded) => {
                let message_id = decoded.message_id;
                let body = decoded.body.to_vec();
                received.drain(..decoded.consumed);
                return Ok((message_id, body));
            }
            Err(protocol_aap::AawError::Incomplete { .. }) => {}
            Err(error) => return Err(CliError::Protocol(error.to_string())),
        }
        let size = transport
            .receive(&mut read_buffer)
            .map_err(CliError::Transport)?;
        received.extend_from_slice(&read_buffer[..size]);
    }
}

fn wlan_mac_address() -> Result<String, CliError> {
    std::fs::read_to_string(format!("/sys/class/net/{WLAN_INTERFACE}/address"))
        .map(|contents| contents.trim().to_string())
        .map_err(CliError::Io)
}

fn random_hex(byte_count: usize) -> Result<String, CliError> {
    use std::io::Read;
    let mut buffer = vec![0_u8; byte_count];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut buffer))
        .map_err(CliError::Io)?;
    Ok(buffer.iter().fold(String::new(), |mut hex, byte| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

fn run_command(description: &str, mut command: Command) -> Result<(), CliError> {
    let status = command
        .status()
        .map_err(|error| CliError::Protocol(format!("{description}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Protocol(format!(
            "{description}: exited with {status}"
        )))
    }
}

/// Configures `WLAN_INTERFACE`'s IPv4 address and admin (up/down) state
/// directly over netlink, instead of shelling out to `ip` like every
/// other command here does.
///
/// **Why this one command is different, real-hardware finding
/// 2026-08-22**: after granting this binary `CAP_NET_ADMIN`/`CAP_NET_RAW`
/// (`packaging/debian/aa-headunit-diagnostics.postinst`) specifically so
/// the AP bring-up never needs `sudo`, `ip addr add`/`ip link set up`
/// still failed with `Operation not permitted` — confirmed via `strace`
/// that Debian's own `ip` binary unconditionally calls `capset` to drop
/// *all* of its capabilities to zero the instant it starts, if its real
/// uid isn't 0. That's deliberate upstream hardening against exactly the
/// setcap-instead-of-setuid-root pattern this project needed, and no
/// amount of file-capability tuning on `ip` itself gets around code that
/// throws its own privileges away on purpose. Talking to the kernel
/// directly from *this* process sidesteps it entirely: `aa-headunit-diagnostics`
/// carries the capability itself, and has no such self-dropping logic of
/// its own to fight. `rfkill unblock wifi`/`nmcli device set ... managed no`
/// are unaffected by any of this and stay as plain subprocess calls —
/// real-hardware-confirmed to already work unprivileged for an active
/// local desktop session (`nmcli` via ordinary `NetworkManager` polkit
/// rules; `rfkill` never even needed a capability grant), so widening
/// either would be an unnecessary privilege increase, not a fix for
/// anything actually broken.
async fn resolve_wlan_index(handle: &rtnetlink::Handle) -> Result<u32, CliError> {
    let mut links = handle
        .link()
        .get()
        .match_name(WLAN_INTERFACE.to_string())
        .execute();
    let link = links
        .try_next()
        .await
        .map_err(|error| CliError::Protocol(format!("netlink: resolve {WLAN_INTERFACE}: {error}")))?
        .ok_or_else(|| CliError::Protocol(format!("netlink: {WLAN_INTERFACE} not found")))?;
    Ok(link.header.index)
}

async fn flush_wlan_addresses(handle: &rtnetlink::Handle, index: u32) -> Result<(), CliError> {
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(index)
        .execute();
    while let Some(address) = addresses
        .try_next()
        .await
        .map_err(|error| CliError::Protocol(format!("netlink: list addresses: {error}")))?
    {
        handle
            .address()
            .del(address)
            .execute()
            .await
            .map_err(|error| CliError::Protocol(format!("netlink: flush address: {error}")))?;
    }
    Ok(())
}

async fn configure_wlan_interface_async() -> Result<(), CliError> {
    let (connection, handle, _) = rtnetlink::new_connection().map_err(CliError::Io)?;
    tokio::spawn(connection);

    let index = resolve_wlan_index(&handle).await?;
    flush_wlan_addresses(&handle, index).await?;

    handle
        .address()
        .add(index, IpAddr::V4(AP_IP), AP_PREFIX_LEN)
        .execute()
        .await
        .map_err(|error| CliError::Protocol(format!("netlink: add AP address: {error}")))?;

    handle
        .link()
        .set(rtnetlink::LinkUnspec::new_with_index(index).up().build())
        .execute()
        .await
        .map_err(|error| {
            CliError::Protocol(format!("netlink: set {WLAN_INTERFACE} up: {error}"))
        })?;

    Ok(())
}

/// Synchronous wrapper for [`configure_wlan_interface_async`] — every
/// other caller in this file (`run`, `WifiAccessPoint::start`/`Drop`) is
/// plain synchronous code; this is the only place that needs a Tokio
/// runtime, kept as small and local as possible rather than making the
/// whole module async.
fn configure_wlan_interface() -> Result<(), CliError> {
    tokio::runtime::Runtime::new()
        .map_err(CliError::Io)?
        .block_on(configure_wlan_interface_async())
}

async fn flush_wlan_interface_async() -> Result<(), CliError> {
    let (connection, handle, _) = rtnetlink::new_connection().map_err(CliError::Io)?;
    tokio::spawn(connection);
    let index = resolve_wlan_index(&handle).await?;
    flush_wlan_addresses(&handle, index).await
}

/// Best-effort cleanup counterpart to [`configure_wlan_interface`], used
/// only from [`WifiAccessPoint`]'s `Drop` — mirrors that impl's existing
/// "errors during cleanup are not fatal" posture for every other
/// teardown step there.
fn flush_wlan_interface() -> Result<(), CliError> {
    tokio::runtime::Runtime::new()
        .map_err(CliError::Io)?
        .block_on(flush_wlan_interface_async())
}

/// A running `hostapd` + `dnsmasq` pair advertising this device's own
/// Wi-Fi access point on `wlan0`, and the interface state changes needed
/// to support it. `Drop` tears everything down: kills both processes and
/// restores `wlan0` to `NetworkManager`'s management, best-effort (errors
/// during cleanup are not fatal — this is a diagnostic tool, not a
/// long-running service).
pub(crate) struct WifiAccessPoint {
    hostapd: Child,
    dnsmasq: Child,
    hostapd_config_path: PathBuf,
    dnsmasq_config_path: PathBuf,
    dnsmasq_lease_path: PathBuf,
}

impl WifiAccessPoint {
    fn start(ssid: &str, password: &str) -> Result<Self, CliError> {
        run_command(
            "rfkill unblock wifi",
            command_with_args("rfkill", ["unblock", "wifi"]),
        )?;
        // Let NetworkManager stop managing wlan0 before we configure it by
        // hand — otherwise it fights hostapd for the interface.
        run_command(
            "nmcli device set wlan0 managed no",
            command_with_args("nmcli", ["device", "set", WLAN_INTERFACE, "managed", "no"]),
        )?;
        configure_wlan_interface()?;

        let dnsmasq_lease_path =
            std::env::temp_dir().join(format!("aa-headunit-dnsmasq-{}.leases", std::process::id()));
        let hostapd_config_path = write_temp_config("hostapd", &hostapd_config(ssid, password))?;
        let dnsmasq_config_path =
            write_temp_config("dnsmasq", &dnsmasq_config(&dnsmasq_lease_path))?;

        let hostapd = Command::new("hostapd")
            .arg(&hostapd_config_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CliError::Io)?;
        let dnsmasq = Command::new("dnsmasq")
            .arg("--keep-in-foreground")
            .arg(format!("--conf-file={}", dnsmasq_config_path.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CliError::Io)?;

        std::thread::sleep(AP_STARTUP_SETTLE);

        Ok(Self {
            hostapd,
            dnsmasq,
            hostapd_config_path,
            dnsmasq_config_path,
            dnsmasq_lease_path,
        })
    }
}

impl Drop for WifiAccessPoint {
    fn drop(&mut self) {
        let _ = self.hostapd.kill();
        let _ = self.hostapd.wait();
        let _ = self.dnsmasq.kill();
        let _ = self.dnsmasq.wait();
        let _ = std::fs::remove_file(&self.hostapd_config_path);
        let _ = std::fs::remove_file(&self.dnsmasq_config_path);
        let _ = std::fs::remove_file(&self.dnsmasq_lease_path);
        let _ = flush_wlan_interface();
        let _ = Command::new("nmcli")
            .args(["device", "set", WLAN_INTERFACE, "managed", "yes"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn command_with_args<const N: usize>(program: &str, args: [&str; N]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    command
}

/// `hw_mode=a`/`channel=36` (5GHz), not the original `g`/`6` (2.4GHz) —
/// real-hardware finding, 2026-08-23: the very first fully-working
/// wireless Android Auto session (real video/audio confirmed on the head
/// unit's own screen) still "felt very laggy" to the operator. 2.4GHz is
/// far more congestion-prone and lower-bandwidth than 5GHz, and this AP
/// had never had any latency/throughput tuning before that first session
/// — this project's own onboard radio (`iw list`, confirmed directly on
/// this hardware) supports 5GHz AP mode. Channel 36 is deliberately a
/// non-DFS channel (no `(radar detection)` flag in `iw list`'s output)
/// so beaconing starts immediately, with no mandatory Channel
/// Availability Check delay a DFS channel would add. `country_code=GB`
/// matches this device's own already-configured regulatory domain
/// (`iw reg get`, confirmed directly) — required for hostapd to use the
/// correct 5GHz power/channel rules rather than falling back to the
/// more restrictive "00" world regulatory domain.
fn hostapd_config(ssid: &str, password: &str) -> String {
    format!(
        "interface={WLAN_INTERFACE}\n\
         driver=nl80211\n\
         ssid={ssid}\n\
         country_code=GB\n\
         ieee80211d=1\n\
         hw_mode=a\n\
         channel=36\n\
         wpa=2\n\
         wpa_passphrase={password}\n\
         wpa_key_mgmt=WPA-PSK\n\
         rsn_pairwise=CCMP\n"
    )
}

/// `lease_path` is required, not optional — real-hardware finding,
/// 2026-08-23: dnsmasq's own default lease file
/// (`/var/lib/misc/dnsmasq.leases`) is root-owned, so it silently exits
/// with "Permission denied" the instant it starts as this project's
/// unprivileged appliance user, well before it ever answers a DHCP
/// request. The phone still receives real Wi-Fi credentials over
/// Bluetooth and reports back a success status for that exchange (which
/// is *not* a real connection — see `run_aaw_bootstrap`'s own doc
/// comment: it only covers the `aaw` credential handoff), then genuinely
/// fails to associate, because there is no DHCP server left alive to
/// hand it an IP address. `write_temp_config`'s own temp-file convention
/// is reused here for the lease path too — a real lease file, just one
/// this unprivileged user can actually write.
fn dnsmasq_config(lease_path: &Path) -> String {
    format!(
        "interface={WLAN_INTERFACE}\n\
         bind-interfaces\n\
         dhcp-range={AP_DHCP_RANGE_START},{AP_DHCP_RANGE_END},255.255.255.0,12h\n\
         dhcp-leasefile={}\n\
         port=0\n",
        lease_path.display()
    )
}

fn write_temp_config(label: &str, contents: &str) -> Result<PathBuf, CliError> {
    let path =
        std::env::temp_dir().join(format!("aa-headunit-{label}-{}.conf", std::process::id()));
    let mut file = std::fs::File::create(&path).map_err(CliError::Io)?;
    file.write_all(contents.as_bytes()).map_err(CliError::Io)?;
    Ok(path)
}
