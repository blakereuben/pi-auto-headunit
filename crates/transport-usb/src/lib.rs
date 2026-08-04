//! Linux/libusb adapter for the publicly documented AOA control sequence.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{HoldResult, LibUsbAoaBackend};

use transport_api::AoaError;
#[cfg(not(target_os = "linux"))]
use transport_api::UsbDeviceId;

pub const GOOGLE_VENDOR_ID: u16 = 0x18d1;
pub const ACCESSORY_PRODUCT_IDS: [u16; 4] = [0x2d00, 0x2d01, 0x2d04, 0x2d05];

#[must_use]
pub fn is_accessory_id(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == GOOGLE_VENDOR_ID && ACCESSORY_PRODUCT_IDS.contains(&product_id)
}

pub fn parse_bus_address(value: &str) -> Result<(u8, u8), AoaError> {
    let (bus, address) = value.split_once(':').ok_or_else(|| {
        AoaError::Internal("device selector must use BUS:ADDRESS, for example 1:4".into())
    })?;
    let bus = bus
        .parse::<u8>()
        .map_err(|_| AoaError::Internal(format!("invalid USB bus: {bus}")))?;
    let address = address
        .parse::<u8>()
        .map_err(|_| AoaError::Internal(format!("invalid USB address: {address}")))?;
    Ok((bus, address))
}

#[cfg(not(target_os = "linux"))]
#[derive(Default)]
pub struct LibUsbAoaBackend;

#[cfg(not(target_os = "linux"))]
impl LibUsbAoaBackend {
    pub fn new() -> Result<Self, AoaError> {
        Err(AoaError::Unsupported(
            "real USB AOA diagnostics are supported on Linux targets only".into(),
        ))
    }

    pub fn list_devices(&self) -> Result<Vec<UsbDeviceId>, AoaError> {
        Err(AoaError::Unsupported(
            "real USB enumeration is supported on Linux targets only".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_selector() {
        assert_eq!(parse_bus_address("1:42").expect("valid selector"), (1, 42));
        assert!(parse_bus_address("any").is_err());
    }

    #[test]
    fn recognizes_documented_accessory_ids() {
        assert!(is_accessory_id(0x18d1, 0x2d00));
        assert!(is_accessory_id(0x18d1, 0x2d01));
        assert!(!is_accessory_id(0x1234, 0x2d00));
    }
}
