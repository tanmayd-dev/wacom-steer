//! Detection and passive read path for Wacom pen devices.

use evdev::{AbsoluteAxisCode, Device, KeyCode};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Detailed metadata for an absolute axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxisDetails {
    pub min: i32,
    pub max: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
    pub value: i32,
}

impl AxisDetails {
    pub fn new(min: i32, max: i32, fuzz: i32, flat: i32, resolution: i32, value: i32) -> Self {
        Self {
            min,
            max,
            fuzz,
            flat,
            resolution,
            value,
        }
    }
}

/// Information about a detected input device.
#[derive(Debug, Clone)]
pub struct WacomDeviceInfo {
    pub path: PathBuf,
    pub sysfs_path: PathBuf,
    pub name: String,
    pub vendor: Option<u16>,
    pub product: Option<u16>,
    pub bustype: Option<u16>,
    pub version: Option<u16>,
    pub x_axis: Option<AxisDetails>,
    pub y_axis: Option<AxisDetails>,
    pub has_pen_tool: bool,
    pub is_pen_device: bool,
    pub score: i32,
}

/// Filter criteria for selecting a Wacom device.
#[derive(Debug, Clone, Default)]
pub struct DeviceFilter {
    pub path: Option<PathBuf>,
    pub device_name: Option<String>,
    pub vendor: Option<u16>,
    pub product: Option<u16>,
}

#[derive(Debug)]
pub enum DeviceError {
    NoDevicesFound(String),
    DeviceNotFound(PathBuf),
    PermissionDenied(PathBuf),
    IoError(io::Error),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevicesFound(msg) => write!(f, "No matching Wacom device found: {msg}"),
            Self::DeviceNotFound(path) => write!(f, "Specified device path not found: {}", path.display()),
            Self::PermissionDenied(path) => write!(
                f,
                "Permission denied accessing device {}. Ensure current user has read permissions (e.g. member of 'input' group).",
                path.display()
            ),
            Self::IoError(e) => write!(f, "I/O error accessing device: {e}"),
        }
    }
}

impl std::error::Error for DeviceError {}

impl From<io::Error> for DeviceError {
    fn from(e: io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Parse a hex string (e.g. "056a", "0x056a") into a u16.
pub fn parse_hex_u16(s: &str) -> Option<u16> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u16::from_str_radix(s, 16).ok()
}

/// Reads a single line from a sysfs file, trimmed.
fn read_sysfs_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Reads a hex integer from a sysfs file.
fn read_sysfs_hex(path: &Path) -> Option<u16> {
    read_sysfs_string(path).and_then(|s| parse_hex_u16(&s))
}

/// Scans sysfs and /dev/input for input devices.
pub fn scan_devices() -> Vec<WacomDeviceInfo> {
    scan_devices_in(Path::new("/sys/class/input"), Path::new("/dev/input"))
}

/// Configurable scanning for testing with custom directory roots.
pub fn scan_devices_in(sysfs_root: &Path, dev_input_root: &Path) -> Vec<WacomDeviceInfo> {
    let mut devices = Vec::new();

    let entries = match fs::read_dir(sysfs_root) {
        Ok(e) => e,
        Err(err) => {
            log::warn!(
                "Failed to read sysfs input directory {}: {}",
                sysfs_root.display(),
                err
            );
            return devices;
        }
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if !name_str.starts_with("event") {
            continue;
        }

        let sysfs_path = entry.path();
        let dev_path = dev_input_root.join(&file_name);

        let device_sub = sysfs_path.join("device");
        let name =
            read_sysfs_string(&device_sub.join("name")).unwrap_or_else(|| "Unknown".to_string());
        let vendor = read_sysfs_hex(&device_sub.join("id/vendor"));
        let product = read_sysfs_hex(&device_sub.join("id/product"));
        let bustype = read_sysfs_hex(&device_sub.join("id/bustype"));
        let version = read_sysfs_hex(&device_sub.join("id/version"));

        let mut x_axis = None;
        let mut y_axis = None;
        let mut has_pen_tool = false;

        // Try reading device caps via evdev if device node exists and is readable
        if dev_path.exists() {
            if let Ok(dev) = Device::open(&dev_path) {
                if let Some(keys) = dev.supported_keys() {
                    has_pen_tool = keys.contains(KeyCode::BTN_TOOL_PEN);
                }
                if let Ok(info_iter) = dev.get_absinfo() {
                    for (axis, info) in info_iter {
                        let details = AxisDetails::new(
                            info.minimum(),
                            info.maximum(),
                            info.fuzz(),
                            info.flat(),
                            info.resolution(),
                            info.value(),
                        );
                        if axis == AbsoluteAxisCode::ABS_X {
                            x_axis = Some(details);
                        } else if axis == AbsoluteAxisCode::ABS_Y {
                            y_axis = Some(details);
                        }
                    }
                }
            }
        }

        let is_pen_device =
            has_pen_tool || (name.to_lowercase().contains("pen") && x_axis.is_some());

        // Calculate relevance score
        let mut score = 0;
        if has_pen_tool {
            score += 100;
        }
        if x_axis.is_some() {
            score += 80;
        }
        if name.to_lowercase().contains("pen") {
            score += 50;
        }
        if name.to_lowercase().contains("wacom") {
            score += 30;
        }
        if vendor == Some(0x056a) && product == Some(0x0375) {
            score += 20;
        }

        devices.push(WacomDeviceInfo {
            path: dev_path,
            sysfs_path,
            name,
            vendor,
            product,
            bustype,
            version,
            x_axis,
            y_axis,
            has_pen_tool,
            is_pen_device,
            score,
        });
    }

    // Sort devices so that highest scoring devices appear first
    devices.sort_by_key(|a| std::cmp::Reverse(a.score));
    devices
}

/// Inspects a specific device path directly (e.g. passed via --device).
pub fn inspect_device_path(path: &Path) -> Result<WacomDeviceInfo, DeviceError> {
    if !path.exists() {
        return Err(DeviceError::DeviceNotFound(path.to_path_buf()));
    }

    let dev = Device::open(path).map_err(|e| {
        if e.kind() == io::ErrorKind::PermissionDenied {
            DeviceError::PermissionDenied(path.to_path_buf())
        } else {
            DeviceError::IoError(e)
        }
    })?;

    let name = dev.name().unwrap_or("Unknown").to_string();
    let input_id = dev.input_id();
    let vendor = Some(input_id.vendor());
    let product = Some(input_id.product());
    let bustype = Some(input_id.bus_type().0);
    let version = Some(input_id.version());

    let mut x_axis = None;
    let mut y_axis = None;
    let mut has_pen_tool = false;

    if let Some(keys) = dev.supported_keys() {
        has_pen_tool = keys.contains(KeyCode::BTN_TOOL_PEN);
    }

    if let Ok(info_iter) = dev.get_absinfo() {
        for (axis, info) in info_iter {
            let details = AxisDetails::new(
                info.minimum(),
                info.maximum(),
                info.fuzz(),
                info.flat(),
                info.resolution(),
                info.value(),
            );
            if axis == AbsoluteAxisCode::ABS_X {
                x_axis = Some(details);
            } else if axis == AbsoluteAxisCode::ABS_Y {
                y_axis = Some(details);
            }
        }
    }

    let is_pen_device = has_pen_tool || (name.to_lowercase().contains("pen") && x_axis.is_some());

    Ok(WacomDeviceInfo {
        path: path.to_path_buf(),
        sysfs_path: PathBuf::new(),
        name,
        vendor,
        product,
        bustype,
        version,
        x_axis,
        y_axis,
        has_pen_tool,
        is_pen_device,
        score: 100,
    })
}

/// Selects the best device matching the filter criteria.
pub fn select_device(
    devices: &[WacomDeviceInfo],
    filter: &DeviceFilter,
) -> Result<WacomDeviceInfo, DeviceError> {
    if let Some(ref path) = filter.path {
        // If an explicit path is given, check scanned list or inspect directly
        if let Some(d) = devices.iter().find(|d| d.path == *path) {
            return Ok(d.clone());
        }
        return inspect_device_path(path);
    }

    let candidates: Vec<&WacomDeviceInfo> = devices
        .iter()
        .filter(|d| {
            if let Some(ref req_name) = filter.device_name {
                if !d.name.to_lowercase().contains(&req_name.to_lowercase()) {
                    return false;
                }
            }
            if let Some(req_vendor) = filter.vendor {
                if d.vendor != Some(req_vendor) {
                    return false;
                }
            }
            if let Some(req_product) = filter.product {
                if d.product != Some(req_product) {
                    return false;
                }
            }
            true
        })
        .collect();

    if candidates.is_empty() {
        let mut msg = format!(
            "No devices matching name='{:?}', vendor={:?}, product={:?}.",
            filter.device_name, filter.vendor, filter.product
        );
        if !devices.is_empty() {
            msg.push_str("\nAvailable input devices:\n");
            for d in devices {
                msg.push_str(&format!(
                    "  - {}: '{}' [vendor={:04x?}, product={:04x?}]\n",
                    d.path.display(),
                    d.name,
                    d.vendor,
                    d.product
                ));
            }
        }
        return Err(DeviceError::NoDevicesFound(msg));
    }

    // Pick candidate with highest score
    let selected = candidates
        .into_iter()
        .max_by_key(|d| d.score)
        .cloned()
        .expect("candidates is non-empty");

    Ok(selected)
}

/// Open an evdev input device in non-exclusive mode.
///
/// NOTE: This function explicitly avoids calling `EVIOCGRAB`. The device remains
/// shared so that the pen continues to control the desktop cursor without interruption.
pub fn open_passive_device(path: &Path) -> io::Result<Device> {
    Device::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_hex_u16("056a"), Some(0x056a));
        assert_eq!(parse_hex_u16("0x056a"), Some(0x056a));
        assert_eq!(parse_hex_u16("0X0375"), Some(0x0375));
        assert_eq!(parse_hex_u16("  0375 \n"), Some(0x0375));
        assert_eq!(parse_hex_u16("invalid"), None);
    }

    #[test]
    fn test_select_device_scoring() {
        let pen = WacomDeviceInfo {
            path: PathBuf::from("/dev/input/event7"),
            sysfs_path: PathBuf::from("/sys/class/input/event7"),
            name: "Wacom Intuos M Pen".to_string(),
            vendor: Some(0x056a),
            product: Some(0x0375),
            bustype: Some(3),
            version: Some(1),
            x_axis: Some(AxisDetails::new(0, 21600, 4, 0, 100, 0)),
            y_axis: Some(AxisDetails::new(0, 13500, 4, 0, 100, 0)),
            has_pen_tool: true,
            is_pen_device: true,
            score: 280,
        };

        let pad = WacomDeviceInfo {
            path: PathBuf::from("/dev/input/event8"),
            sysfs_path: PathBuf::from("/sys/class/input/event8"),
            name: "Wacom Intuos M Pad".to_string(),
            vendor: Some(0x056a),
            product: Some(0x0375),
            bustype: Some(3),
            version: Some(1),
            x_axis: None,
            y_axis: None,
            has_pen_tool: false,
            is_pen_device: false,
            score: 50,
        };

        let devices = vec![pad.clone(), pen.clone()];
        let filter = DeviceFilter::default();

        let selected = select_device(&devices, &filter).unwrap();
        assert_eq!(selected.path, pen.path);
        assert_eq!(selected.name, "Wacom Intuos M Pen");
    }

    #[test]
    fn test_select_device_filter_by_name() {
        let pen = WacomDeviceInfo {
            path: PathBuf::from("/dev/input/event7"),
            sysfs_path: PathBuf::from("/sys/class/input/event7"),
            name: "Wacom Intuos M Pen".to_string(),
            vendor: Some(0x056a),
            product: Some(0x0375),
            bustype: Some(3),
            version: Some(1),
            x_axis: Some(AxisDetails::new(0, 21600, 4, 0, 100, 0)),
            y_axis: Some(AxisDetails::new(0, 13500, 4, 0, 100, 0)),
            has_pen_tool: true,
            is_pen_device: true,
            score: 280,
        };

        let devices = vec![pen];
        let filter = DeviceFilter {
            device_name: Some("Pad".to_string()),
            ..Default::default()
        };

        let result = select_device(&devices, &filter);
        assert!(result.is_err());
    }
}
