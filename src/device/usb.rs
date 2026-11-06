//! libusb transport to an Audient iD interface.
//!
//! snd-usb-audio owns the AudioControl interface, and detaching it would kill
//! the sound card. The iD firmware accepts its mixer requests on any of its
//! interfaces, so we claim the spare DFU/vendor interface instead, which no
//! kernel driver uses. Interface 0 is never touched.

use std::time::Duration;

use rusb::{Context, DeviceHandle, UsbContext};

use super::profile::{AUDIENT_VENDOR_ID, Profile, find_profile};
use super::protocol::{
    ENTITY_MIXER, ENTITY_MONITOR, METER_BLOCK_LEN, MONITOR_VOLUME, REQ_CUR, REQ_MEM, REQ_TYPE_GET,
    REQ_TYPE_SET, Request,
};

const WRITE_TIMEOUT: Duration = Duration::from_millis(250);
const READ_TIMEOUT: Duration = Duration::from_millis(100);
const METER_TIMEOUT: Duration = Duration::from_millis(50);

const CLASS_APPLICATION: u8 = 0xFE;
const CLASS_VENDOR: u8 = 0xFF;

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("no Audient iD interface connected")]
    NotFound,
    #[error("USB access denied: install the udev rule")]
    PermissionDenied,
    #[error("unknown Audient model (product id {0:#06x})")]
    Unsupported(u16),
    #[error(
        "no free control interface: refusing to detach snd-usb-audio (the sound card would disappear)"
    )]
    NoSpareInterface,
    #[error("control interface busy: another program (MixiD, BiD…?) is using it")]
    Busy,
    #[error("interface unplugged")]
    Disconnected,
    #[error("short reply from the interface ({got} bytes instead of {expected})")]
    ShortRead { got: usize, expected: usize },
    #[error("USB error: {0}")]
    Usb(rusb::Error),
}

impl From<rusb::Error> for DeviceError {
    fn from(e: rusb::Error) -> Self {
        match e {
            rusb::Error::Access => Self::PermissionDenied,
            rusb::Error::NoDevice => Self::Disconnected,
            rusb::Error::Busy => Self::Busy,
            other => Self::Usb(other),
        }
    }
}

impl DeviceError {
    /// The device is gone and the handle must be dropped.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Disconnected | Self::NotFound)
    }
}

pub struct IdDevice {
    handle: DeviceHandle<Context>,
    interface: u8,
    profile: &'static Profile,
}

impl IdDevice {
    /// Opens the first Audient iD found on the bus.
    pub fn open() -> Result<Self, DeviceError> {
        let context = Context::new()?;
        let mut unsupported = None;
        for device in context.devices()?.iter() {
            let Ok(desc) = device.device_descriptor() else {
                continue;
            };
            if desc.vendor_id() != AUDIENT_VENDOR_ID {
                continue;
            }
            let Some(profile) = find_profile(desc.product_id()) else {
                unsupported = Some(desc.product_id());
                continue;
            };
            let interface = spare_interface(&device).ok_or(DeviceError::NoSpareInterface)?;
            let handle = device.open()?;
            // Never auto-detach: the interface we claim has no kernel driver.
            handle.claim_interface(interface)?;
            log::info!(
                "{} opened, control through interface {interface}",
                profile.name
            );
            return Ok(Self {
                handle,
                interface,
                profile,
            });
        }
        Err(unsupported.map_or(DeviceError::NotFound, DeviceError::Unsupported))
    }

    pub fn profile(&self) -> &'static Profile {
        self.profile
    }

    pub fn interface(&self) -> u8 {
        self.interface
    }

    fn index(&self, entity: u8) -> u16 {
        (u16::from(entity) << 8) | u16::from(self.interface)
    }

    pub fn send(&self, request: &Request) -> Result<(), DeviceError> {
        self.handle.write_control(
            REQ_TYPE_SET,
            REQ_CUR,
            request.value,
            self.index(request.entity),
            &request.data,
            WRITE_TIMEOUT,
        )?;
        Ok(())
    }

    fn get(&self, entity: u8, value: u16, buf: &mut [u8]) -> Result<(), DeviceError> {
        let got = self.handle.read_control(
            REQ_TYPE_GET,
            REQ_CUR,
            value,
            self.index(entity),
            buf,
            READ_TIMEOUT,
        )?;
        if got != buf.len() {
            return Err(DeviceError::ShortRead {
                got,
                expected: buf.len(),
            });
        }
        Ok(())
    }

    /// Monitor level as raw int16 (follows the front-panel knob).
    pub fn monitor_volume(&self) -> Result<i16, DeviceError> {
        let mut b = [0u8; 2];
        self.get(ENTITY_MONITOR, MONITOR_VOLUME, &mut b)?;
        Ok(i16::from_le_bytes(b))
    }

    pub fn monitor_flag(&self, selector: u16) -> Result<bool, DeviceError> {
        let mut b = [0u8; 1];
        self.get(ENTITY_MONITOR, selector, &mut b)?;
        Ok(b[0] != 0)
    }

    /// Raw meter block (two bytes per mixer node).
    pub fn meter_block(&self) -> Result<[u8; METER_BLOCK_LEN], DeviceError> {
        let mut buf = [0u8; METER_BLOCK_LEN];
        let got = self.handle.read_control(
            REQ_TYPE_GET,
            REQ_MEM,
            0,
            self.index(ENTITY_MIXER),
            &mut buf,
            METER_TIMEOUT,
        )?;
        if got != METER_BLOCK_LEN {
            return Err(DeviceError::ShortRead {
                got,
                expected: METER_BLOCK_LEN,
            });
        }
        Ok(buf)
    }
}

impl Drop for IdDevice {
    fn drop(&mut self) {
        if let Err(e) = self.handle.release_interface(self.interface) {
            log::debug!("release_interface: {e}");
        }
    }
}

/// First DFU (application-specific) or vendor interface of the active config.
fn spare_interface(device: &rusb::Device<Context>) -> Option<u8> {
    let config = device.active_config_descriptor().ok()?;
    config
        .interfaces()
        .flat_map(|i| i.descriptors())
        .find(|d| matches!(d.class_code(), CLASS_APPLICATION | CLASS_VENDOR))
        .map(|d| d.interface_number())
}
