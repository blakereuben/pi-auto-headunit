use crate::is_accessory_id;
use rusb::{Context, Device, DeviceHandle, Direction, TransferType, UsbContext};
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

pub struct LibUsbAoaBackend {
    context: Context,
}

pub struct LibUsbBulkTransport {
    handle: DeviceHandle<Context>,
    info: BulkTransportInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldResult {
    Unplugged,
    TimedOut,
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
        Ok(LibUsbBulkTransport { handle, info })
    }
}

impl LibUsbBulkTransport {
    #[must_use]
    pub const fn info(&self) -> &BulkTransportInfo {
        &self.info
    }

    pub fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), AoaError> {
        let mut offset = 0;
        while offset < bytes.len() {
            let written = self
                .handle
                .write_bulk(self.info.bulk_out_endpoint, &bytes[offset..], timeout)
                .map_err(map_usb_error)?;
            if written == 0 {
                return Err(AoaError::Usb("bulk transfer wrote zero bytes".into()));
            }
            offset += written;
        }
        Ok(())
    }

    pub fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> Result<usize, AoaError> {
        match self
            .handle
            .read_bulk(self.info.bulk_in_endpoint, buffer, timeout)
        {
            Ok(size) => Ok(size),
            Err(rusb::Error::Timeout) => Ok(0),
            Err(error) => Err(map_usb_error(error)),
        }
    }
}

impl SessionTransport for LibUsbBulkTransport {
    fn receive(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        self.read(buffer, Duration::from_millis(500))
            .map_err(map_transport_error)
    }

    fn send_all(&mut self, bytes: &[u8]) -> Result<(), TransportError> {
        self.write_all(bytes, Duration::from_secs(2))
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
        let started = Instant::now();
        while started.elapsed() < timeout {
            for candidate in self.list_devices()? {
                let same_port = !original.port_path.is_empty()
                    && candidate.bus == original.bus
                    && candidate.port_path == original.port_path;
                if same_port && is_accessory_id(candidate.vendor_id, candidate.product_id) {
                    return Ok(candidate);
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(AoaError::TimedOut(
            transport_api::AoaState::WaitingForReenumeration,
        ))
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
        rusb::Error::Timeout => AoaError::Usb("control transfer timed out".into()),
        other => AoaError::Usb(other.to_string()),
    }
}
