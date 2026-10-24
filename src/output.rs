//! Virtual gamepad output abstraction using Linux uinput.

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    UinputAbsSetup,
};
use std::io;

/// Virtual gamepad device name seen by the host and Wine/Proton.
pub const VIRTUAL_GAMEPAD_NAME: &str = "Wacom Steer Virtual Gamepad";

/// Minimum and maximum range for virtual gamepad axes.
pub const GAMEPAD_AXIS_MIN: i32 = -32767;
pub const GAMEPAD_AXIS_MAX: i32 = 32767;

/// Button codes required for standard gamepad recognition.
pub const GAMEPAD_BUTTON_CODES: &[KeyCode] = &[
    KeyCode::BTN_SOUTH,  // BTN_GAMEPAD / BTN_A (0x130)
    KeyCode::BTN_EAST,   // BTN_B (0x131)
    KeyCode::BTN_NORTH,  // BTN_X (0x133)
    KeyCode::BTN_WEST,   // BTN_Y (0x134)
    KeyCode::BTN_SELECT, // BTN_SELECT (0x13a)
    KeyCode::BTN_START,  // BTN_START (0x13b)
    KeyCode::BTN_MODE,   // BTN_MODE (0x13c)
];

/// Absolute axes required for standard gamepad recognition.
pub const GAMEPAD_AXIS_CODES: &[AbsoluteAxisCode] = &[
    AbsoluteAxisCode::ABS_X,
    AbsoluteAxisCode::ABS_Y,
    AbsoluteAxisCode::ABS_RX,
    AbsoluteAxisCode::ABS_RY,
];

/// Trait abstracting the steering output device.
pub trait SteeringOutput: Send {
    /// Emits a steering value for ABS_X (clamped to [-32767, 32767]).
    fn set_steering(&mut self, value: i32) -> io::Result<()>;

    /// Emits neutral steering (0.0 / 0) and synchronizes state.
    fn center(&mut self) -> io::Result<()> {
        self.set_steering(0)
    }

    /// Name of the output device.
    fn device_name(&self) -> &str;

    /// Current steering axis value.
    fn current_steering(&self) -> i32;
}

/// Real uinput virtual gamepad device.
pub struct UinputGamepad {
    device: VirtualDevice,
    current_steering: i32,
}

impl UinputGamepad {
    /// Creates and configures the virtual gamepad via `/dev/uinput`.
    pub fn open() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for &k in GAMEPAD_BUTTON_CODES {
            keys.insert(k);
        }

        let abs_info = AbsInfo::new(0, GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX, 0, 0, 0);

        let mut builder = VirtualDevice::builder()
            .map_err(|e| io::Error::other(format!("Failed to initialize uinput: {e}")))?
            .name(VIRTUAL_GAMEPAD_NAME)
            // Use standard USB vendor/product (e.g. Xbox 360 controller compatible ID) for maximum Wine/Proton compatibility
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110))
            .with_keys(&keys)
            .map_err(|e| io::Error::other(format!("Failed to set uinput keys: {e}")))?;

        for &axis in GAMEPAD_AXIS_CODES {
            let setup = UinputAbsSetup::new(axis, abs_info);
            builder = builder.with_absolute_axis(&setup).map_err(|e| {
                io::Error::other(format!("Failed to set uinput axis {axis:?}: {e}"))
            })?;
        }

        let mut device = builder
            .build()
            .map_err(|e| io::Error::other(format!("Failed to build uinput virtual device: {e}")))?;

        // Emit initial neutral state for all axes
        let initial_events = [
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RX.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RY.0, 0),
        ];
        device.emit(&initial_events)?;

        Ok(Self {
            device,
            current_steering: 0,
        })
    }
}

impl SteeringOutput for UinputGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, clamped);
        self.device.emit(&[event])?;
        self.current_steering = clamped;
        Ok(())
    }

    fn device_name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }

    fn current_steering(&self) -> i32 {
        self.current_steering
    }
}

/// In-memory mock gamepad output for testing without accessing `/dev/uinput`.
#[derive(Debug, Clone)]
pub struct MockGamepad {
    pub name: String,
    pub current_steering: i32,
    pub history: Vec<i32>,
}

impl Default for MockGamepad {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGamepad {
    pub fn new() -> Self {
        Self {
            name: VIRTUAL_GAMEPAD_NAME.to_string(),
            current_steering: 0,
            history: vec![0], // initial neutral state
        }
    }

    pub fn events(&self) -> &[i32] {
        &self.history
    }

    pub fn last_steering(&self) -> i32 {
        self.current_steering
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.current_steering = 0;
    }
}

impl SteeringOutput for MockGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        self.current_steering = clamped;
        self.history.push(clamped);
        Ok(())
    }

    fn device_name(&self) -> &str {
        &self.name
    }

    fn current_steering(&self) -> i32 {
        self.current_steering
    }
}

/// Dry-run output that records steering and logs without real uinput access.
#[derive(Debug, Default)]
pub struct DryRunGamepad {
    current_steering: i32,
}

impl SteeringOutput for DryRunGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        self.current_steering = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        log::debug!(
            "[DryRun] Virtual gamepad ABS_X set to {}",
            self.current_steering
        );
        Ok(())
    }

    fn device_name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }

    fn current_steering(&self) -> i32 {
        self.current_steering
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_gamepad_operations() {
        let mut mock = MockGamepad::new();
        assert_eq!(mock.current_steering(), 0);
        assert_eq!(mock.events(), &[0]);

        mock.set_steering(12345).unwrap();
        assert_eq!(mock.current_steering(), 12345);

        mock.set_steering(-30000).unwrap();
        assert_eq!(mock.current_steering(), -30000);

        // Clamping check
        mock.set_steering(50000).unwrap();
        assert_eq!(mock.current_steering(), GAMEPAD_AXIS_MAX);

        mock.set_steering(-50000).unwrap();
        assert_eq!(mock.current_steering(), GAMEPAD_AXIS_MIN);

        // Centering check
        mock.center().unwrap();
        assert_eq!(mock.current_steering(), 0);

        assert_eq!(mock.device_name(), VIRTUAL_GAMEPAD_NAME);
    }
}
