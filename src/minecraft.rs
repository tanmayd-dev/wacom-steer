//! Minecraft camera control mode mapping tablet input to virtual relative mouse movement.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::mapping::AxisRange;

/// Control scheme for translation of tablet coordinates to mouse movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum MinecraftStyle {
    /// Trackpad / swipe style: stylus movement generates relative mouse deltas (default).
    #[default]
    Delta,
    /// Rate / joystick style: stylus deflection away from center commands continuous rotation speed.
    Rate,
}

impl std::fmt::Display for MinecraftStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Delta => write!(f, "delta"),
            Self::Rate => write!(f, "rate"),
        }
    }
}

/// Configuration parameters for Minecraft mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinecraftConfig {
    /// Whether Minecraft mode is active.
    pub enabled: bool,
    /// Control style (Delta or Rate).
    pub style: MinecraftStyle,
    /// Aiming sensitivity multiplier (> 0.0). Default is 1.0.
    pub sensitivity: f64,
    /// Resting deadzone around tablet center for Rate mode in [0.0, 1.0). Default is 0.05.
    pub deadzone: f64,
    /// Whether pitch (vertical Y-axis) is inverted. Default is false (standard up = look up).
    pub invert_y: bool,
    /// Base full-lock camera speed in mouse counts per second for Rate mode. Default is 1200.0.
    pub rate_speed: f64,
}

impl Default for MinecraftConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            style: MinecraftStyle::Delta,
            sensitivity: 1.0,
            deadzone: 0.05,
            invert_y: false,
            rate_speed: 1200.0,
        }
    }
}

impl MinecraftConfig {
    /// Validates the configuration parameters.
    pub fn validate(&self) -> Result<(), String> {
        if self.sensitivity <= 0.0 {
            return Err(format!(
                "minecraft sensitivity must be > 0.0, got {}",
                self.sensitivity
            ));
        }
        if !(0.0..1.0).contains(&self.deadzone) {
            return Err(format!(
                "minecraft deadzone must be in [0.0, 1.0), got {}",
                self.deadzone
            ));
        }
        if self.rate_speed <= 0.0 {
            return Err(format!(
                "minecraft rate_speed must be > 0.0, got {}",
                self.rate_speed
            ));
        }
        Ok(())
    }
}

/// Base scaling factor converting raw high-resolution tablet counts to standard mouse counts in Delta mode.
/// Tablet resolutions are typically 100 counts/mm (2540 LPI), whereas standard mice are ~30-60 counts/mm.
/// 0.04 maps a full stroke on an Intuos M (21600 units) to ~864 mouse counts (~360-degree turn in Minecraft at 100% sens).
pub const DEFAULT_DELTA_SCALE: f64 = 0.04;

/// Controller managing tablet-to-mouse translation for Minecraft camera aiming.
#[derive(Debug, Clone)]
pub struct MinecraftController {
    pub x_range: AxisRange,
    pub y_range: AxisRange,
    pub config: MinecraftConfig,

    pub pen_in_proximity: bool,
    pub last_raw_x: Option<i32>,
    pub last_raw_y: Option<i32>,

    accum_x: f64,
    accum_y: f64,
}

impl MinecraftController {
    /// Creates a new controller for the specified tablet ranges and configuration.
    pub fn new(x_range: AxisRange, y_range: AxisRange, config: MinecraftConfig) -> Self {
        Self {
            x_range,
            y_range,
            config,
            pen_in_proximity: false,
            last_raw_x: None,
            last_raw_y: None,
            accum_x: 0.0,
            accum_y: 0.0,
        }
    }

    /// Updates proximity state. When proximity is lost, tracking baseline is cleared
    /// so that placing the pen back down elsewhere on the tablet does not produce a delta jump.
    pub fn set_proximity(&mut self, in_proximity: bool) {
        self.pen_in_proximity = in_proximity;
        if !in_proximity {
            self.last_raw_x = None;
            self.last_raw_y = None;
            self.accum_x = 0.0;
            self.accum_y = 0.0;
        }
    }

    /// Handles a new raw X coordinate from the tablet.
    /// In Delta mode: returns relative mouse delta `dx` if tracking is active.
    pub fn handle_raw_x(&mut self, raw_x: i32) -> i32 {
        if !self.pen_in_proximity {
            self.last_raw_x = None;
            return 0;
        }

        let prev = match self.last_raw_x {
            Some(p) => p,
            None => {
                self.last_raw_x = Some(raw_x);
                return 0;
            }
        };

        self.last_raw_x = Some(raw_x);

        if self.config.style == MinecraftStyle::Delta {
            let raw_dx = (raw_x - prev) as f64;
            self.accum_x += raw_dx * DEFAULT_DELTA_SCALE * self.config.sensitivity;
            let emit_dx = self.accum_x.trunc() as i32;
            self.accum_x -= emit_dx as f64;
            emit_dx
        } else {
            0
        }
    }

    /// Handles a new raw Y coordinate from the tablet.
    /// In Delta mode: returns relative mouse delta `dy` if tracking is active.
    pub fn handle_raw_y(&mut self, raw_y: i32) -> i32 {
        if !self.pen_in_proximity {
            self.last_raw_y = None;
            return 0;
        }

        let prev = match self.last_raw_y {
            Some(p) => p,
            None => {
                self.last_raw_y = Some(raw_y);
                return 0;
            }
        };

        self.last_raw_y = Some(raw_y);

        if self.config.style == MinecraftStyle::Delta {
            // Note: Tablet Y=0 is top, max is bottom.
            // Moving pen up decreases raw_y (raw_dy < 0).
            // In standard mouse coords, negative REL_Y pitches view up in Minecraft.
            // Thus, raw_dy directly matches mouse REL_Y.
            let raw_dy = (raw_y - prev) as f64;
            let oriented_dy = if self.config.invert_y {
                -raw_dy
            } else {
                raw_dy
            };

            self.accum_y += oriented_dy * DEFAULT_DELTA_SCALE * self.config.sensitivity;
            let emit_dy = self.accum_y.trunc() as i32;
            self.accum_y -= emit_dy as f64;
            emit_dy
        } else {
            0
        }
    }

    /// Computes relative mouse motion on each periodic timer tick for Rate mode.
    /// Returns `(dx, dy)` to be emitted this tick.
    pub fn tick_rate(&mut self, dt_seconds: f64) -> (i32, i32) {
        if self.config.style != MinecraftStyle::Rate || !self.pen_in_proximity {
            return (0, 0);
        }

        let (rx, ry) = match (self.last_raw_x, self.last_raw_y) {
            (Some(x), Some(y)) => (x, y),
            _ => return (0, 0),
        };

        let x_span = self.x_range.span();
        let y_span = self.y_range.span();
        if x_span <= 0.0 || y_span <= 0.0 {
            return (0, 0);
        }

        let norm_x = ((rx - self.x_range.min) as f64 / x_span).clamp(0.0, 1.0);
        let norm_y = ((ry - self.y_range.min) as f64 / y_span).clamp(0.0, 1.0);

        // Center is at 0.5, deflection in [-1.0, 1.0]
        let def_x = (norm_x - 0.5) * 2.0;
        // norm_y = 0 is top (def_y = -1.0), norm_y = 1 is bottom (def_y = 1.0)
        let def_y = (norm_y - 0.5) * 2.0;

        let deadzone = self.config.deadzone.clamp(0.0, 0.9999);

        let calc_rate = |deflection: f64| -> f64 {
            let abs_d = deflection.abs();
            if abs_d > deadzone {
                let mag = (abs_d - deadzone) / (1.0 - deadzone);
                deflection.signum() * mag
            } else {
                0.0
            }
        };

        let rate_x = calc_rate(def_x);
        let rate_y = calc_rate(def_y);

        let oriented_rate_y = if self.config.invert_y {
            -rate_y
        } else {
            rate_y
        };

        let speed = self.config.rate_speed * self.config.sensitivity * dt_seconds.max(0.0);
        self.accum_x += rate_x * speed;
        self.accum_y += oriented_rate_y * speed;

        let emit_dx = self.accum_x.trunc() as i32;
        let emit_dy = self.accum_y.trunc() as i32;
        self.accum_x -= emit_dx as f64;
        self.accum_y -= emit_dy as f64;

        (emit_dx, emit_dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_validation() {
        let cfg = MinecraftConfig::default();
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.style, MinecraftStyle::Delta);
        assert!(!cfg.enabled);
    }

    #[test]
    fn test_config_validation_failures() {
        let mut cfg = MinecraftConfig::default();
        cfg.sensitivity = 0.0;
        assert!(cfg.validate().is_err());

        let mut cfg = MinecraftConfig::default();
        cfg.deadzone = 1.0;
        assert!(cfg.validate().is_err());

        let mut cfg = MinecraftConfig::default();
        cfg.rate_speed = -10.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_delta_mode_continuous_tracking() {
        let mut controller = MinecraftController::new(
            AxisRange::new(0, 20000),
            AxisRange::new(0, 10000),
            MinecraftConfig::default(),
        );

        controller.set_proximity(true);

        // Initial coordinates seed baseline; no delta emitted yet
        assert_eq!(controller.handle_raw_x(5000), 0);
        assert_eq!(controller.handle_raw_y(5000), 0);

        // Movement right: +1000 raw counts * 0.04 = +40 counts
        let dx = controller.handle_raw_x(6000);
        assert_eq!(dx, 40);

        // Movement down: +500 raw counts * 0.04 = +20 counts
        let dy = controller.handle_raw_y(5500);
        assert_eq!(dy, 20);

        // Movement up: -250 raw counts * 0.04 = -10 counts
        let dy_up = controller.handle_raw_y(5250);
        assert_eq!(dy_up, -10);
    }

    #[test]
    fn test_delta_mode_proximity_lift_resets_baseline() {
        let mut controller = MinecraftController::new(
            AxisRange::new(0, 20000),
            AxisRange::new(0, 10000),
            MinecraftConfig::default(),
        );

        controller.set_proximity(true);
        controller.handle_raw_x(2000);
        controller.handle_raw_y(2000);

        // Move to 3000
        assert_eq!(controller.handle_raw_x(3000), 40);

        // Pen lifts away from tablet
        controller.set_proximity(false);
        assert_eq!(controller.handle_raw_x(8000), 0);

        // Pen re-enters far away at 15000
        controller.set_proximity(true);
        // First coordinate after re-entry must seed baseline without jumping
        assert_eq!(controller.handle_raw_x(15000), 0);
        assert_eq!(controller.handle_raw_y(8000), 0);

        // Subsequent movement calculates delta from new baseline
        assert_eq!(controller.handle_raw_x(15100), 4);
    }

    #[test]
    fn test_delta_mode_invert_y() {
        let mut cfg = MinecraftConfig::default();
        cfg.invert_y = true;

        let mut controller =
            MinecraftController::new(AxisRange::new(0, 20000), AxisRange::new(0, 10000), cfg);

        controller.set_proximity(true);
        controller.handle_raw_y(5000);

        // Moving pen down (+500 raw) with invert_y should produce negative dy (-20)
        let dy = controller.handle_raw_y(5500);
        assert_eq!(dy, -20);
    }

    #[test]
    fn test_rate_mode_deflection_and_deadzone() {
        let mut cfg = MinecraftConfig::default();
        cfg.style = MinecraftStyle::Rate;
        cfg.deadzone = 0.1;
        cfg.rate_speed = 1000.0;
        cfg.sensitivity = 1.0;

        let mut controller =
            MinecraftController::new(AxisRange::new(0, 10000), AxisRange::new(0, 10000), cfg);

        controller.set_proximity(true);

        // At center (5000, 5000)
        controller.handle_raw_x(5000);
        controller.handle_raw_y(5000);

        let (dx, dy) = controller.tick_rate(0.1);
        assert_eq!(dx, 0);
        assert_eq!(dy, 0);

        // Inside deadzone (5200 is norm 0.52 -> def 0.04 < 0.1)
        controller.handle_raw_x(5200);
        let (dx, dy) = controller.tick_rate(0.1);
        assert_eq!(dx, 0);
        assert_eq!(dy, 0);

        // Full right deflection (10000 -> def 1.0)
        controller.handle_raw_x(10000);
        // dt = 0.1s, rate_speed = 1000 -> 100 counts
        let (dx, _) = controller.tick_rate(0.1);
        assert_eq!(dx, 100);

        // Lift pen stops rate ticks immediately
        controller.set_proximity(false);
        let (dx, dy) = controller.tick_rate(0.1);
        assert_eq!(dx, 0);
        assert_eq!(dy, 0);
    }
}
