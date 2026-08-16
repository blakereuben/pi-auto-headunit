use crate::is_accessory_id;
use rusb::{
    Context, Device, DeviceHandle, Direction, Error as RusbError, TransferType, UsbContext,
};
use std::thread;
use std::time::{Duration, Instant};
use transport_api::{
    AoaBackend, AoaError, AoaIdentification, BulkTransportInfo, SessionTransport, TransportError,
    UsbDeviceId,
};

const REQUEST_GET_PROTOCOL: u8 = 51;
const REQUEST_SEND_STRING: u8 = 52;
const REQUEST_START_ACCESSORY: u8 = 53;
const REQUEST_TYPE_IN_VENDOR: u8 = 0xc0;
const REQUEST_TYPE_OUT_VENDOR: u8 = 0x40;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
/// Matches AASDK's own reference bulk-OUT write timeout exactly
/// (`include/f1x/aasdk/Transport/USBTransport.hpp`'s
/// `cSendTimeoutMs = 10000`, pinned revision
/// `9bf6adf933665dee26532201719fac14a047ccf1` — used for every bulk-OUT
/// transfer in that implementation, no message-type special-casing). This
/// project's own bulk-OUT writes previously used a much tighter 2-second
/// timeout for the same operation; see
/// `docs/protocol/error-2-investigation.md` for why that mismatch is the
/// leading suspect for a real-hardware USB write timeout observed when
/// this crate's probe sends an unsolicited message (`PingRequest`).
const BULK_SEND_TIMEOUT: Duration = Duration::from_secs(10);

pub struct LibUsbAoaBackend {
    context: Context,
}

pub struct LibUsbBulkTransport {
    handle: DeviceHandle<Context>,
    info: BulkTransportInfo,
    /// Monotonically increasing per-transport counter, incremented once per
    /// `write_all` call. Printed alongside each write's timing/outcome
    /// below — diagnostic instrumentation added for the still-unresolved
    /// USB bulk-OUT write timeout (`docs/protocol/error-2-investigation.md`,
    /// "Ping cadence"/"LIVI ping-model" sections). This crate has exactly
    /// one `SessionTransport` consumer today (`aa-headunit-diagnostics`);
    /// `write_all`/`read` are only ever called from that single-threaded
    /// probe's own receive loop, one bulk transfer at a time — there is no
    /// concurrent/overlapping write in this crate's own code. If writes are
    /// nonetheless overlapping or blocked, the cause is below this layer
    /// (kernel USB stack, libusb's own internal state, or the phone's own
    /// USB peer), not a concurrency bug in this Rust code.
    write_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldResult {
    Unplugged,
    TimedOut,
}

/// Outcome of [`LibUsbAoaBackend::soft_reset`]. `libusb_reset_device`'s own
/// documentation (`libusb/core.c`, `libusb_reset_device`'s doc comment):
/// "If the reset fails, the descriptors change, or the previous state
/// cannot be restored, the device will appear to be disconnected and
/// reconnected... A return code of `LIBUSB_ERROR_NOT_FOUND` indicates when
/// this is the case." Real-hardware-observed (2026-08-16): this specific
/// error is the *common* outcome for this project's actual use (an Android
/// phone in AOA accessory mode), not a rare edge case — every real trial so
/// far reset successfully in the sense that mattered (the device came back
/// and was rediscoverable), but every one of them also hit this exact
/// libusb return code, which this project's code previously logged as a
/// plain failure. It isn't one: it's libusb's documented way of saying the
/// reset caused genuine re-enumeration, which is exactly what a caller
/// asking for a "replug without touching the cable" wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftResetOutcome {
    /// The reset completed and the original device handle is still valid.
    Completed,
    /// `LIBUSB_ERROR_NOT_FOUND` — the device re-enumerated as a result of
    /// the reset (almost certainly with a new address) and must be
    /// rediscovered; the original handle is no longer usable. Treated as
    /// success, not failure, since re-enumeration is exactly what a
    /// software-triggered "replug" is meant to cause.
    Reenumerated,
}

impl LibUsbAoaBackend {
    pub fn new() -> Result<Self, AoaError> {
        Context::new()
            .map(|context| Self { context })
            .map_err(map_usb_error)
    }

    pub fn list_devices(&self) -> Result<Vec<UsbDeviceId>, AoaError> {
        let devices = self.context.devices().map_err(map_usb_error)?;
        let mut result = Vec::new();
        for device in devices.iter() {
            let Ok(descriptor) = device.device_descriptor() else {
                continue;
            };
            result.push(to_id(&device, &descriptor));
        }
        result.sort_by_key(|device| (device.bus, device.address));
        Ok(result)
    }

    fn find_device(&self, id: &UsbDeviceId) -> Result<Device<Context>, AoaError> {
        self.context
            .devices()
            .map_err(map_usb_error)?
            .iter()
            .find(|device| device.bus_number() == id.bus && device.address() == id.address)
            .ok_or(AoaError::Unplugged)
    }

    fn open_device(&self, id: &UsbDeviceId) -> Result<DeviceHandle<Context>, AoaError> {
        self.find_device(id)?.open().map_err(map_usb_error)
    }

    fn open_claimed_bulk_transport(
        &self,
        device: &UsbDeviceId,
    ) -> Result<(DeviceHandle<Context>, BulkTransportInfo), AoaError> {
        let usb_device = self.find_device(device)?;
        let config = usb_device
            .active_config_descriptor()
            .map_err(map_usb_error)?;
        let handle = usb_device.open().map_err(map_usb_error)?;

        for interface in config.interfaces() {
            for descriptor in interface.descriptors() {
                let mut bulk_in = None;
                let mut bulk_out = None;
                for endpoint in descriptor.endpoint_descriptors() {
                    if endpoint.transfer_type() != TransferType::Bulk {
                        continue;
                    }
                    match endpoint.direction() {
                        Direction::In => bulk_in.get_or_insert(endpoint.address()),
                        Direction::Out => bulk_out.get_or_insert(endpoint.address()),
                    };
                }
                let (Some(bulk_in_endpoint), Some(bulk_out_endpoint)) = (bulk_in, bulk_out) else {
                    continue;
                };

                let interface_number = descriptor.interface_number();
                let _ = handle.set_auto_detach_kernel_driver(true);
                handle
                    .claim_interface(interface_number)
                    .map_err(map_usb_error)?;
                return Ok((
                    handle,
                    BulkTransportInfo {
                        device: device.clone(),
                        interface_number,
                        bulk_in_endpoint,
                        bulk_out_endpoint,
                    },
                ));
            }
        }
        Err(AoaError::Unsupported(
            "accessory device exposes no interface with bulk IN and OUT endpoints".into(),
        ))
    }

    pub fn hold_bulk_interface(
        &mut self,
        device: &UsbDeviceId,
        duration: Duration,
    ) -> Result<HoldResult, AoaError> {
        let (handle, transport) = self.open_claimed_bulk_transport(device)?;
        let started = Instant::now();
        while started.elapsed() < duration {
            let present = self.list_devices()?.iter().any(|candidate| {
                candidate.bus == device.bus
                    && candidate.address == device.address
                    && candidate.vendor_id == device.vendor_id
                    && candidate.product_id == device.product_id
            });
            if !present {
                return Ok(HoldResult::Unplugged);
            }
            thread::sleep(Duration::from_millis(100));
        }
        handle
            .release_interface(transport.interface_number)
            .map_err(map_usb_error)?;
        Ok(HoldResult::TimedOut)
    }

    pub fn open_claimed_session_transport(
        &self,
        device: &UsbDeviceId,
    ) -> Result<LibUsbBulkTransport, AoaError> {
        let (handle, info) = self.open_claimed_bulk_transport(device)?;
        Ok(LibUsbBulkTransport {
            handle,
            info,
            write_id: 0,
        })
    }

    /// Polls `list_devices()` every 100ms until `timeout`, returning the
    /// first device satisfying `matches`. Shared by `wait_for_accessory`
    /// (`AoaBackend`'s in-session MTP→accessory-mode reenumeration wait,
    /// which additionally requires an accessory vendor/product ID) and
    /// `wait_for_reconnect` (a full physical unplug/replug, where the phone
    /// comes back in its normal, non-accessory mode and has no such
    /// requirement).
    fn poll_for_match(
        &self,
        timeout: Duration,
        matches: impl Fn(&UsbDeviceId) -> bool,
    ) -> Result<UsbDeviceId, AoaError> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if let Some(candidate) = self.list_devices()?.into_iter().find(&matches) {
                return Ok(candidate);
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(AoaError::TimedOut(
            transport_api::AoaState::WaitingForReenumeration,
        ))
    }

    /// Polls for a device at the same physical USB port (`bus` +
    /// `port_path`) to reappear after a genuine physical unplug, in *any*
    /// USB mode — unlike `wait_for_accessory`, which is only correct for
    /// the in-session reenumeration AASDK/AOA itself triggers (MTP mode →
    /// accessory mode), a phone coming back from a real unplug
    /// re-enumerates into its normal mode and needs the *entire* AOA
    /// transition run again from scratch, not just a wait for its
    /// accessory-mode reappearance.
    pub fn wait_for_reconnect(
        &self,
        original: &UsbDeviceId,
        timeout: Duration,
    ) -> Result<UsbDeviceId, AoaError> {
        self.poll_for_match(timeout, |candidate| {
            !original.port_path.is_empty()
                && candidate.bus == original.bus
                && candidate.port_path == original.port_path
        })
    }

    /// Performs a software-only USB port reset (`libusb_reset_device`, via
    /// `rusb`'s `DeviceHandle::reset`) — no physical unplug involved. Added
    /// 2026-08-16 at Blake's explicit request: a phone left in a stale
    /// post-session protocol state (still expecting encrypted post-
    /// handshake traffic from a prior run) can make a fresh
    /// `auth-discovery-probe` cycle fail immediately (real-hardware-
    /// observed: `encrypted frame received before TLS handshake
    /// completed`) even though the device never physically disconnected.
    /// This is `usb session-supervisor`'s first, software-only recovery
    /// attempt before it ever asks the operator to physically replug —
    /// see `apps/aa-headunit-diagnostics/src/session_supervisor.rs`.
    ///
    /// Real-hardware-confirmed effective (2026-08-16, two separate
    /// trials): every reset against a real phone in AOA accessory mode
    /// returned [`SoftResetOutcome::Reenumerated`], and the device was
    /// reliably rediscoverable afterward — see that variant's doc comment
    /// for why this is libusb's documented success case, not a failure the
    /// earlier version of this function mistakenly reported as one
    /// (`outcome=failed reason=USB error: Entity not found` in
    /// `session_supervisor.rs`'s log, despite the recovery actually
    /// working). Whether the phone's own Android Auto app-level session
    /// state clears as a result (as opposed to just its USB-level
    /// connection) is still outside this project's control and not
    /// separately provable — callers must still treat a repeated failure
    /// as needing a real physical replug (`wait_for_physical_replug`), not
    /// retry this indefinitely.
    pub fn soft_reset(&self, device: &UsbDeviceId) -> Result<SoftResetOutcome, AoaError> {
        let handle = self.open_device(device)?;
        match handle.reset() {
            Ok(()) => Ok(SoftResetOutcome::Completed),
            Err(RusbError::NotFound) => Ok(SoftResetOutcome::Reenumerated),
            Err(error) => Err(map_usb_error(error)),
        }
    }

    /// Waits for `original` to become physically absent, then waits for a
    /// device to reappear at the same port. Unlike `wait_for_reconnect`,
    /// which only waits for *reappearance* and matches immediately if the
    /// device never actually left — the common case right after
    /// `soft_reset`, which doesn't necessarily change the device's
    /// enumerated bus/address — this confirms a genuine physical replug.
    /// Used only once `soft_reset` has already been tried for the same
    /// failure streak and the next cycle failed again, so a caller (the
    /// popup in `replug_prompt.rs`) can honestly tell the operator a real
    /// physical replug is what's actually needed, and know when it
    /// happened.
    pub fn wait_for_physical_replug(
        &self,
        original: &UsbDeviceId,
        timeout: Duration,
    ) -> Result<UsbDeviceId, AoaError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let present = self.list_devices()?.iter().any(|candidate| {
                candidate.bus == original.bus && candidate.address == original.address
            });
            if !present {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        self.wait_for_reconnect(original, remaining)
    }
}

impl LibUsbBulkTransport {
    #[must_use]
    pub const fn info(&self) -> &BulkTransportInfo {
        &self.info
    }

    pub fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), AoaError> {
        self.write_id += 1;
        let write_id = self.write_id;
        let total_len = bytes.len();
        let leading_byte = bytes.first().copied();
        let start = Instant::now();
        println!(
            "usb_write_start write_id={write_id} bytes={total_len} \
             channel_id={leading_byte:?} timeout_ms={}",
            timeout.as_millis()
        );
        let mut offset = 0;
        while offset < bytes.len() {
            let chunk_start = Instant::now();
            let written =
                match self
                    .handle
                    .write_bulk(self.info.bulk_out_endpoint, &bytes[offset..], timeout)
                {
                    Ok(written) => written,
                    Err(error) => {
                        println!(
                            "usb_write_error write_id={write_id} offset={offset} \
                         elapsed_ms={} error={error}",
                            start.elapsed().as_millis()
                        );
                        return Err(map_usb_error(error));
                    }
                };
            if written == 0 {
                println!(
                    "usb_write_error write_id={write_id} offset={offset} \
                     elapsed_ms={} error=zero_bytes_written",
                    start.elapsed().as_millis()
                );
                return Err(AoaError::Usb("bulk transfer wrote zero bytes".into()));
            }
            println!(
                "usb_write_chunk write_id={write_id} offset={offset} written={written} \
                 chunk_elapsed_ms={}",
                chunk_start.elapsed().as_millis()
            );
            offset += written;
        }
        println!(
            "usb_write_complete write_id={write_id} bytes={total_len} \
             elapsed_ms={}",
            start.elapsed().as_millis()
        );
        Ok(())
    }

    pub fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<usize, AoaError> {
        let start = Instant::now();
        match self
            .handle
            .read_bulk(self.info.bulk_in_endpoint, buffer, timeout)
        {
            Ok(size) => {
                if size > 0 {
                    println!(
                        "usb_read_complete bytes={size} elapsed_ms={}",
                        start.elapsed().as_millis()
                    );
                }
                Ok(size)
            }
            Err(rusb::Error::Timeout) => Ok(0),
            Err(error) => {
                println!(
                    "usb_read_error elapsed_ms={} error={error}",
                    start.elapsed().as_millis()
                );
                Err(map_usb_error(error))
            }
        }
    }
}

impl SessionTransport for LibUsbBulkTransport {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        self.read(buffer, Duration::from_millis(500))
            .map_err(map_transport_error)
    }

    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.write_all(bytes, BULK_SEND_TIMEOUT)
            .map_err(map_transport_error)
    }
}

fn map_transport_error(error: AoaError) -> TransportError {
    match error {
        AoaError::Unplugged => TransportError::Closed,
        AoaError::TimedOut(_) => TransportError::TimedOut,
        error => TransportError::Io(error.to_string()),
    }
}

impl Drop for LibUsbBulkTransport {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(self.info.interface_number);
    }
}

impl AoaBackend for LibUsbAoaBackend {
    fn query_protocol(&mut self, device: &UsbDeviceId) -> Result<u16, AoaError> {
        let handle = self.open_device(device)?;
        let mut response = [0_u8; 2];
        let read = handle
            .read_control(
                REQUEST_TYPE_IN_VENDOR,
                REQUEST_GET_PROTOCOL,
                0,
                0,
                &mut response,
                CONTROL_TIMEOUT,
            )
            .map_err(map_usb_error)?;
        if read != response.len() {
            return Err(AoaError::Unsupported(format!(
                "AOA Get Protocol returned {read} bytes; expected 2"
            )));
        }
        Ok(u16::from_le_bytes(response))
    }

    fn send_identification(
        &mut self,
        device: &UsbDeviceId,
        identification: &AoaIdentification,
    ) -> Result<(), AoaError> {
        let handle = self.open_device(device)?;
        let values = [
            &identification.manufacturer,
            &identification.model,
            &identification.description,
            &identification.version,
            &identification.uri,
            &identification.serial,
        ];
        for (index, value) in values.iter().enumerate() {
            let mut bytes = value.as_bytes().to_vec();
            bytes.push(0);
            let written = handle
                .write_control(
                    REQUEST_TYPE_OUT_VENDOR,
                    REQUEST_SEND_STRING,
                    0,
                    u16::try_from(index).expect("six string indexes fit in u16"),
                    &bytes,
                    CONTROL_TIMEOUT,
                )
                .map_err(map_usb_error)?;
            if written != bytes.len() {
                return Err(AoaError::Usb(format!(
                    "AOA string {index} wrote {written} of {} bytes",
                    bytes.len()
                )));
            }
        }
        Ok(())
    }

    fn request_accessory_mode(&mut self, device: &UsbDeviceId) -> Result<(), AoaError> {
        let handle = self.open_device(device)?;
        handle
            .write_control(
                REQUEST_TYPE_OUT_VENDOR,
                REQUEST_START_ACCESSORY,
                0,
                0,
                &[],
                CONTROL_TIMEOUT,
            )
            .map_err(map_usb_error)?;
        Ok(())
    }

    fn wait_for_accessory(
        &mut self,
        original: &UsbDeviceId,
        timeout: Duration,
    ) -> Result<UsbDeviceId, AoaError> {
        self.poll_for_match(timeout, |candidate| {
            !original.port_path.is_empty()
                && candidate.bus == original.bus
                && candidate.port_path == original.port_path
                && is_accessory_id(candidate.vendor_id, candidate.product_id)
        })
    }

    fn open_bulk_transport(&mut self, device: &UsbDeviceId) -> Result<BulkTransportInfo, AoaError> {
        let (handle, transport) = self.open_claimed_bulk_transport(device)?;
        handle
            .release_interface(transport.interface_number)
            .map_err(map_usb_error)?;
        Ok(transport)
    }

    fn is_accessory_mode(&self, device: &UsbDeviceId) -> bool {
        is_accessory_id(device.vendor_id, device.product_id)
    }
}

fn to_id<T: UsbContext>(device: &Device<T>, descriptor: &rusb::DeviceDescriptor) -> UsbDeviceId {
    UsbDeviceId {
        bus: device.bus_number(),
        address: device.address(),
        vendor_id: descriptor.vendor_id(),
        product_id: descriptor.product_id(),
        port_path: device.port_numbers().unwrap_or_default(),
    }
}

fn map_usb_error(error: rusb::Error) -> AoaError {
    match error {
        rusb::Error::Access => AoaError::PermissionDenied(
            "check the aa-headunit udev rule and reconnect the phone".into(),
        ),
        rusb::Error::NoDevice => AoaError::Unplugged,
        rusb::Error::Timeout => AoaError::Usb("USB transfer timed out".into()),
        other => AoaError::Usb(other.to_string()),
    }
}
