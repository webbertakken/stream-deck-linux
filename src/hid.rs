//! Minimal pure-Rust hidraw backend.
//!
//! We talk to `/dev/hidraw*` directly via `read`/`write` and a few ioctls,
//! avoiding any libudev/hidapi system dependency. Only the pieces a Stream
//! Deck needs are implemented: device info, output reports (images), input
//! reports (buttons) and feature reports (brightness/reset/info).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

// ioctl direction bits (asm-generic, used by x86_64 and aarch64 Linux).
const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;
const HID_IOC_MAGIC: u64 = b'H' as u64;

/// Encode a Linux ioctl request number (asm-generic layout).
const fn ioc(dir: u64, ty: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (ty << 8) | nr
}

/// `HIDIOCGRAWINFO` - read bus/vendor/product into `struct hidraw_devinfo`.
const fn hidioc_grawinfo() -> u64 {
    ioc(
        IOC_READ,
        HID_IOC_MAGIC,
        0x03,
        core::mem::size_of::<HidrawDevinfo>() as u64,
    )
}

/// `HIDIOCSFEATURE(len)` - send a feature report of `len` bytes.
const fn hidioc_sfeature(len: u64) -> u64 {
    ioc(IOC_READ | IOC_WRITE, HID_IOC_MAGIC, 0x06, len)
}

/// `HIDIOCGFEATURE(len)` - fetch a feature report of `len` bytes.
const fn hidioc_gfeature(len: u64) -> u64 {
    ioc(IOC_READ | IOC_WRITE, HID_IOC_MAGIC, 0x07, len)
}

#[repr(C)]
#[derive(Default)]
struct HidrawDevinfo {
    bustype: u32,
    vendor: i16,
    product: i16,
}

/// Identity of a hidraw device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInfo {
    pub bus_type: u32,
    pub vendor_id: u16,
    pub product_id: u16,
}

/// An opened hidraw character device.
pub struct RawHidDevice {
    file: File,
    fd: RawFd,
    path: PathBuf,
}

impl RawHidDevice {
    /// Open a hidraw node for reading and writing.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let fd = file.as_raw_fd();
        Ok(Self { file, fd, path })
    }

    /// Filesystem path of this device node.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Query bus/vendor/product via `HIDIOCGRAWINFO`.
    pub fn info(&self) -> io::Result<DeviceInfo> {
        let mut raw = HidrawDevinfo::default();
        let rc = unsafe { libc::ioctl(self.fd, hidioc_grawinfo(), &mut raw as *mut _) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(DeviceInfo {
            bus_type: raw.bustype,
            vendor_id: raw.vendor as u16,
            product_id: raw.product as u16,
        })
    }

    /// Write a single output report (first byte is the report id).
    pub fn write_output(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)
    }

    /// Send a feature report via `HIDIOCSFEATURE` (first byte is the report id).
    pub fn send_feature(&self, data: &[u8]) -> io::Result<()> {
        let rc = unsafe { libc::ioctl(self.fd, hidioc_sfeature(data.len() as u64), data.as_ptr()) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Fetch a feature report via `HIDIOCGFEATURE` for `report_id`.
    pub fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        buf[0] = report_id;
        let rc = unsafe { libc::ioctl(self.fd, hidioc_gfeature(len as u64), buf.as_mut_ptr()) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(buf)
    }

    /// Read an input report. With a timeout, returns `Ok(0)` if none arrived.
    ///
    /// A signal interrupting the wait (`EINTR`) is reported as `Ok(0)` rather
    /// than an error, so a poll loop can re-check a shutdown flag and exit
    /// cleanly on Ctrl-C.
    pub fn read_timeout(&mut self, buf: &mut [u8], timeout: Option<Duration>) -> io::Result<usize> {
        if let Some(timeout) = timeout {
            let mut pfd = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
            let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
            if rc < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    return Ok(0);
                }
                return Err(err);
            }
            if rc == 0 {
                return Ok(0);
            }
        }
        match self.file.read(buf) {
            Ok(n) => Ok(n),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => Ok(0),
            Err(err) => Err(err),
        }
    }
}

/// Enumerate every accessible `/dev/hidraw*` device and its identity.
///
/// Nodes we cannot open (permissions) or query are silently skipped so a
/// single locked-down device never breaks discovery.
pub fn enumerate() -> io::Result<Vec<(PathBuf, DeviceInfo)>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/dev")? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with("hidraw") {
            continue;
        }
        let path = entry.path();
        if let Ok(dev) = RawHidDevice::open(&path) {
            if let Ok(info) = dev.info() {
                found.push((path, info));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ground-truth values are the well-known Linux hidraw ioctl constants.
    #[test]
    fn ioctl_numbers_match_linux_constants() {
        assert_eq!(hidioc_grawinfo(), 0x8008_4803);
        assert_eq!(hidioc_sfeature(32), 0xC020_4806);
        assert_eq!(hidioc_gfeature(32), 0xC020_4807);
    }

    #[test]
    fn feature_size_is_encoded_into_request() {
        // The 14-bit size field sits at bit 16.
        let len = 0x1f;
        assert_eq!((hidioc_sfeature(len) >> 16) & 0x3fff, len);
        assert_eq!((hidioc_gfeature(len) >> 16) & 0x3fff, len);
    }

    #[test]
    fn devinfo_struct_is_eight_bytes() {
        assert_eq!(core::mem::size_of::<HidrawDevinfo>(), 8);
    }

    #[test]
    fn enumerate_does_not_error_on_this_host() {
        // We have real hidraw nodes here; enumeration must succeed and not
        // panic, regardless of which devices are attached.
        let devices = enumerate().expect("enumerate /dev/hidraw*");
        for (path, info) in &devices {
            assert!(path.to_string_lossy().contains("hidraw"));
            let _ = info.vendor_id;
        }
    }
}
