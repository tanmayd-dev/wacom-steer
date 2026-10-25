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

/// Minimum and maximum range for virtual gamepad triggers (ABS_Z and ABS_RZ).
pub const GAMEPAD_TRIGGER_MIN: i32 = 0;
pub const GAMEPAD_TRIGGER_MAX: i32 = 32767;

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
    AbsoluteAxisCode::ABS_Z,
    AbsoluteAxisCode::ABS_RX,
    AbsoluteAxisCode::ABS_RY,
    AbsoluteAxisCode::ABS_RZ,
];

/// Trait abstracting the steering and pedal output device.
pub trait SteeringOutput: Send {
    /// Emits a steering value for ABS_X (clamped to [-32767, 32767]).
    fn set_steering(&mut self, value: i32) -> io::Result<()>;

    /// Emits a throttle value for ABS_RZ (clamped to [0, 32767]).
    fn set_throttle(&mut self, value: i32) -> io::Result<()>;

    /// Emits a brake value for ABS_Z (clamped to [0, 32767]).
    fn set_brake(&mut self, value: i32) -> io::Result<()>;

    /// Emits combined throttle/brake for ABS_Y (clamped to [-32767, 32767]).
    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()>;

    /// Emits neutral steering (0.0 / 0) and releases all pedals.
    fn center(&mut self) -> io::Result<()> {
        self.set_steering(0)?;
        self.set_throttle(0)?;
        self.set_brake(0)?;
        self.set_combined_pedal(0)?;
        Ok(())
    }

    /// Name of the output device.
    fn device_name(&self) -> &str;

    /// Current steering axis value.
    fn current_steering(&self) -> i32;

    /// Current throttle trigger value.
    fn current_throttle(&self) -> i32;

    /// Current brake trigger value.
    fn current_brake(&self) -> i32;

    /// Current combined pedal axis value.
    fn current_combined_pedal(&self) -> i32;
}

/// Real uinput virtual gamepad device.
pub struct UinputGamepad {
    device: VirtualDevice,
    current_steering: i32,
    current_throttle: i32,
    current_brake: i32,
    current_combined_pedal: i32,
    current_btn_south: bool,
    current_btn_east: bool,
}

impl UinputGamepad {
    /// Creates and configures the virtual gamepad via `/dev/uinput`.
    pub fn open() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for &k in GAMEPAD_BUTTON_CODES {
            keys.insert(k);
        }

        let stick_abs_info = AbsInfo::new(0, GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX, 0, 0, 0);
        let trigger_abs_info = AbsInfo::new(0, GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX, 0, 0, 0);

        let mut builder = VirtualDevice::builder()
            .map_err(|e| io::Error::other(format!("Failed to initialize uinput: {e}")))?
            .name(VIRTUAL_GAMEPAD_NAME)
            // Use standard USB vendor/product (e.g. Xbox 360 controller compatible ID) for maximum Wine/Proton compatibility
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110))
            .with_keys(&keys)
            .map_err(|e| io::Error::other(format!("Failed to set uinput keys: {e}")))?;

        for &axis in GAMEPAD_AXIS_CODES {
            let setup = match axis {
                AbsoluteAxisCode::ABS_Z | AbsoluteAxisCode::ABS_RZ => {
                    UinputAbsSetup::new(axis, trigger_abs_info)
                }
                _ => UinputAbsSetup::new(axis, stick_abs_info),
            };
            builder = builder.with_absolute_axis(&setup).map_err(|e| {
                io::Error::other(format!("Failed to set uinput axis {axis:?}: {e}"))
            })?;
        }

        let mut device = builder
            .build()
            .map_err(|e| io::Error::other(format!("Failed to build uinput virtual device: {e}")))?;

        // Emit initial neutral state for all axes and pedal buttons
        let initial_events = [
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Z.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RX.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RY.0, 0),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_RZ.0, 0),
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 0),
            InputEvent::new(EventType::KEY.0, KeyCode::BTN_EAST.0, 0),
        ];
        device.emit(&initial_events)?;

        Ok(Self {
            device,
            current_steering: 0,
            current_throttle: 0,
            current_brake: 0,
            current_combined_pedal: 0,
            current_btn_south: false,
            current_btn_east: false,
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

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX);
        let btn_active = clamped > (GAMEPAD_TRIGGER_MAX / 10);
        let mut events = Vec::with_capacity(2);
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_RZ.0,
            clamped,
        ));
        if btn_active != self.current_btn_south {
            events.push(InputEvent::new(
                EventType::KEY.0,
                KeyCode::BTN_SOUTH.0,
                if btn_active { 1 } else { 0 },
            ));
            self.current_btn_south = btn_active;
        }
        self.device.emit(&events)?;
        self.current_throttle = clamped;
        Ok(())
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX);
        let btn_active = clamped > (GAMEPAD_TRIGGER_MAX / 10);
        let mut events = Vec::with_capacity(2);
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Z.0,
            clamped,
        ));
        if btn_active != self.current_btn_east {
            events.push(InputEvent::new(
                EventType::KEY.0,
                KeyCode::BTN_EAST.0,
                if btn_active { 1 } else { 0 },
            ));
            self.current_btn_east = btn_active;
        }
        self.device.emit(&events)?;
        self.current_brake = clamped;
        Ok(())
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        let btn_south = clamped < -(GAMEPAD_AXIS_MAX / 10);
        let btn_east = clamped > (GAMEPAD_AXIS_MAX / 10);
        let mut events = Vec::with_capacity(3);
        events.push(InputEvent::new(
            EventType::ABSOLUTE.0,
            AbsoluteAxisCode::ABS_Y.0,
            clamped,
        ));
        if btn_south != self.current_btn_south {
            events.push(InputEvent::new(
                EventType::KEY.0,
                KeyCode::BTN_SOUTH.0,
                if btn_south { 1 } else { 0 },
            ));
            self.current_btn_south = btn_south;
        }
        if btn_east != self.current_btn_east {
            events.push(InputEvent::new(
                EventType::KEY.0,
                KeyCode::BTN_EAST.0,
                if btn_east { 1 } else { 0 },
            ));
            self.current_btn_east = btn_east;
        }
        self.device.emit(&events)?;
        self.current_combined_pedal = clamped;
        Ok(())
    }

    fn center(&mut self) -> io::Result<()> {
        self.set_steering(0)?;
        self.set_throttle(0)?;
        self.set_brake(0)?;
        self.set_combined_pedal(0)?;
        let mut events = Vec::new();
        if self.current_btn_south {
            events.push(InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 0));
            self.current_btn_south = false;
        }
        if self.current_btn_east {
            events.push(InputEvent::new(EventType::KEY.0, KeyCode::BTN_EAST.0, 0));
            self.current_btn_east = false;
        }
        if !events.is_empty() {
            self.device.emit(&events)?;
        }
        Ok(())
    }

    fn device_name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }

    fn current_steering(&self) -> i32 {
        self.current_steering
    }

    fn current_throttle(&self) -> i32 {
        self.current_throttle
    }

    fn current_brake(&self) -> i32 {
        self.current_brake
    }

    fn current_combined_pedal(&self) -> i32 {
        self.current_combined_pedal
    }
}

/// In-memory mock gamepad output for testing without accessing `/dev/uinput`.
#[derive(Debug, Clone)]
pub struct MockGamepad {
    pub name: String,
    pub current_steering: i32,
    pub current_throttle: i32,
    pub current_brake: i32,
    pub current_combined_pedal: i32,
    pub current_btn_south: bool,
    pub current_btn_east: bool,
    pub history: Vec<i32>,
    pub throttle_history: Vec<i32>,
    pub brake_history: Vec<i32>,
    pub combined_history: Vec<i32>,
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
            current_throttle: 0,
            current_brake: 0,
            current_combined_pedal: 0,
            current_btn_south: false,
            current_btn_east: false,
            history: vec![0], // initial neutral state
            throttle_history: vec![0],
            brake_history: vec![0],
            combined_history: vec![0],
        }
    }

    pub fn events(&self) -> &[i32] {
        &self.history
    }

    pub fn last_steering(&self) -> i32 {
        self.current_steering
    }

    pub fn is_btn_south_pressed(&self) -> bool {
        self.current_btn_south
    }

    pub fn is_btn_east_pressed(&self) -> bool {
        self.current_btn_east
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.throttle_history.clear();
        self.brake_history.clear();
        self.combined_history.clear();
        self.current_steering = 0;
        self.current_throttle = 0;
        self.current_brake = 0;
        self.current_combined_pedal = 0;
        self.current_btn_south = false;
        self.current_btn_east = false;
    }
}

impl SteeringOutput for MockGamepad {
    fn set_steering(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        self.current_steering = clamped;
        self.history.push(clamped);
        Ok(())
    }

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX);
        self.current_throttle = clamped;
        self.current_btn_south = clamped > (GAMEPAD_TRIGGER_MAX / 10);
        self.throttle_history.push(clamped);
        Ok(())
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX);
        self.current_brake = clamped;
        self.current_btn_east = clamped > (GAMEPAD_TRIGGER_MAX / 10);
        self.brake_history.push(clamped);
        Ok(())
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        let clamped = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        self.current_combined_pedal = clamped;
        self.current_btn_south = clamped < -(GAMEPAD_AXIS_MAX / 10);
        self.current_btn_east = clamped > (GAMEPAD_AXIS_MAX / 10);
        self.combined_history.push(clamped);
        Ok(())
    }

    fn center(&mut self) -> io::Result<()> {
        self.set_steering(0)?;
        self.set_throttle(0)?;
        self.set_brake(0)?;
        self.set_combined_pedal(0)?;
        self.current_btn_south = false;
        self.current_btn_east = false;
        Ok(())
    }

    fn device_name(&self) -> &str {
        &self.name
    }

    fn current_steering(&self) -> i32 {
        self.current_steering
    }

    fn current_throttle(&self) -> i32 {
        self.current_throttle
    }

    fn current_brake(&self) -> i32 {
        self.current_brake
    }

    fn current_combined_pedal(&self) -> i32 {
        self.current_combined_pedal
    }
}

/// Dry-run output that records steering and logs without real uinput access.
#[derive(Debug, Default)]
pub struct DryRunGamepad {
    current_steering: i32,
    current_throttle: i32,
    current_brake: i32,
    current_combined_pedal: i32,
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

    fn set_throttle(&mut self, value: i32) -> io::Result<()> {
        self.current_throttle = value.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX);
        log::debug!(
            "[DryRun] Virtual gamepad ABS_RZ (throttle) set to {}",
            self.current_throttle
        );
        Ok(())
    }

    fn set_brake(&mut self, value: i32) -> io::Result<()> {
        self.current_brake = value.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX);
        log::debug!(
            "[DryRun] Virtual gamepad ABS_Z (brake) set to {}",
            self.current_brake
        );
        Ok(())
    }

    fn set_combined_pedal(&mut self, value: i32) -> io::Result<()> {
        self.current_combined_pedal = value.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX);
        log::debug!(
            "[DryRun] Virtual gamepad ABS_Y (combined pedal) set to {}",
            self.current_combined_pedal
        );
        Ok(())
    }

    fn device_name(&self) -> &str {
        VIRTUAL_GAMEPAD_NAME
    }

    fn current_steering(&self) -> i32 {
        self.current_steering
    }

    fn current_throttle(&self) -> i32 {
        self.current_throttle
    }

    fn current_brake(&self) -> i32 {
        self.current_brake
    }

    fn current_combined_pedal(&self) -> i32 {
        self.current_combined_pedal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_gamepad_operations() {
        let mut mock = MockGamepad::new();
        assert_eq!(mock.current_steering(), 0);
        assert_eq!(mock.current_throttle(), 0);
        assert_eq!(mock.current_brake(), 0);
        assert_eq!(mock.current_combined_pedal(), 0);
        assert_eq!(mock.events(), &[0]);

        mock.set_steering(12345).unwrap();
        assert_eq!(mock.current_steering(), 12345);

        mock.set_steering(-30000).unwrap();
        assert_eq!(mock.current_steering(), -30000);

        // Clamping check for steering
        mock.set_steering(50000).unwrap();
        assert_eq!(mock.current_steering(), GAMEPAD_AXIS_MAX);

        mock.set_steering(-50000).unwrap();
        assert_eq!(mock.current_steering(), GAMEPAD_AXIS_MIN);

        // Pedals check
        mock.set_throttle(20000).unwrap();
        assert_eq!(mock.current_throttle(), 20000);
        assert!(mock.is_btn_south_pressed());

        mock.set_throttle(40000).unwrap();
        assert_eq!(mock.current_throttle(), GAMEPAD_TRIGGER_MAX);
        assert!(mock.is_btn_south_pressed());

        mock.set_brake(15000).unwrap();
        assert_eq!(mock.current_brake(), 15000);
        assert!(mock.is_btn_east_pressed());

        mock.set_brake(50000).unwrap();
        assert_eq!(mock.current_brake(), GAMEPAD_TRIGGER_MAX);
        assert!(mock.is_btn_east_pressed());

        mock.set_combined_pedal(25000).unwrap();
        assert_eq!(mock.current_combined_pedal(), 25000);
        assert!(mock.is_btn_east_pressed());

        mock.set_combined_pedal(-25000).unwrap();
        assert_eq!(mock.current_combined_pedal(), -25000);
        assert!(mock.is_btn_south_pressed());

        // Centering check
        mock.center().unwrap();
        assert_eq!(mock.current_steering(), 0);
        assert_eq!(mock.current_throttle(), 0);
        assert_eq!(mock.current_brake(), 0);
        assert_eq!(mock.current_combined_pedal(), 0);
        assert!(!mock.is_btn_south_pressed());
        assert!(!mock.is_btn_east_pressed());

        assert_eq!(mock.device_name(), VIRTUAL_GAMEPAD_NAME);
    }
}
