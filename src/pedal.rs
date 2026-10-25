//! Stylus Y-axis mapping to throttle and brake (Pedal/Trigger axes).

use serde::{Deserialize, Serialize};

use crate::config::PedalMode;
use crate::mapping::AxisRange;
use crate::output::{GAMEPAD_AXIS_MAX, GAMEPAD_AXIS_MIN, GAMEPAD_TRIGGER_MAX, GAMEPAD_TRIGGER_MIN};

/// Configuration parameters for pedal (throttle and brake) mapping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PedalConfig {
    /// Physical tablet center mapping to neutral coasting (0.0). Default is 0.5.
    pub center: f64,
    /// Distance from center to reach full throttle/brake lock in normalized coordinates. Default is 0.5.
    pub half_span: f64,
    /// Deadzone around center where neither throttle nor brake is active in [0.0, 1.0). Default is 0.05.
    pub deadzone: f64,
    /// Exponential curve factor (> 0.0, 1.0 = linear). Default is 1.0.
    pub curve: f64,
    /// Whether forward/backward pedal direction is inverted. Default is false.
    pub invert: bool,
    /// Pedal operating mode (Off, Split, or Combined). Default is Off.
    pub mode: PedalMode,
}

impl Default for PedalConfig {
    fn default() -> Self {
        Self {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.05,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Off,
        }
    }
}

impl PedalConfig {
    /// Validates the pedal configuration parameters.
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.center) {
            return Err(format!(
                "center must be between 0.0 and 1.0, got {}",
                self.center
            ));
        }
        if self.half_span <= 0.0 || self.half_span > 1.0 {
            return Err(format!(
                "half_span must be in (0.0, 1.0], got {}",
                self.half_span
            ));
        }
        if !(0.0..1.0).contains(&self.deadzone) {
            return Err(format!(
                "deadzone must be in [0.0, 1.0), got {}",
                self.deadzone
            ));
        }
        if self.curve <= 0.0 {
            return Err(format!(
                "curve must be greater than 0.0, got {}",
                self.curve
            ));
        }
        Ok(())
    }
}

/// Pedal output representation according to configured mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PedalOutput {
    /// Stylus Y-axis pedal mapping is disabled.
    Off,
    /// Split triggers: throttle on ABS_RZ [0, 255] and brake on ABS_Z [0, 255].
    Split { throttle: i32, brake: i32 },
    /// Combined stick axis on ABS_Y [-32767, 32767] (positive = throttle, negative = brake).
    Combined(i32),
}

impl PedalOutput {
    /// Returns true if pedal output is disabled.
    pub fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }
}

/// Mapper that transforms raw hardware Y coordinates to pedal outputs.
#[derive(Debug, Clone)]
pub struct PedalMapper {
    pub input_range: AxisRange,
    pub config: PedalConfig,
}

impl PedalMapper {
    /// Creates a new mapper for the specified input axis range and pedal configuration.
    pub fn new(input_range: AxisRange, config: PedalConfig) -> Self {
        Self {
            input_range,
            config,
        }
    }

    /// Creates a mapper with default configuration for the specified input axis range.
    pub fn with_default_config(input_range: AxisRange) -> Self {
        Self::new(input_range, PedalConfig::default())
    }

    /// Maps normalized tablet Y position [0.0, 1.0] to normalized (throttle, brake) in [0.0, 1.0].
    ///
    /// Wacom tablet physical top is Y_min (normalized 0.0), physical bottom is Y_max (normalized 1.0).
    /// Forward movement (pen pushed toward top of tablet, normalized_y < center) commands throttle.
    /// Backward movement (pen pulled toward bottom of tablet, normalized_y > center) commands brake.
    pub fn map_normalized(&self, normalized_y: f64) -> (f64, f64) {
        let u = normalized_y.clamp(0.0, 1.0);

        let half_span = if self.config.half_span.abs() < 1e-9 {
            1e-9
        } else {
            self.config.half_span
        };

        // Forward movement: normalized_y < config.center gives positive offset.
        // Backward movement: normalized_y > config.center gives negative offset.
        let mut offset = (self.config.center - u) / half_span;

        if self.config.invert {
            offset = -offset;
        }

        let s = offset.clamp(-1.0, 1.0);
        let deadzone = self.config.deadzone.clamp(0.0, 0.999999);

        let (mut throttle, mut brake) = if s.abs() <= deadzone {
            (0.0, 0.0)
        } else if s > deadzone {
            let mag = (s - deadzone) / (1.0 - deadzone);
            (mag.clamp(0.0, 1.0).powf(self.config.curve), 0.0)
        } else {
            let mag = (-s - deadzone) / (1.0 - deadzone);
            (0.0, mag.clamp(0.0, 1.0).powf(self.config.curve))
        };

        throttle = throttle.clamp(0.0, 1.0);
        brake = brake.clamp(0.0, 1.0);

        (throttle, brake)
    }

    /// Maps raw hardware Y coordinate directly to PedalOutput.
    pub fn map_raw(&self, raw_y: i32) -> PedalOutput {
        if self.config.mode == PedalMode::Off {
            return PedalOutput::Off;
        }

        let span = self.input_range.span();
        let u = if span > 0.0 {
            (raw_y - self.input_range.min) as f64 / span
        } else {
            0.5
        };

        let (throttle_f, brake_f) = self.map_normalized(u);

        match self.config.mode {
            PedalMode::Off => PedalOutput::Off,
            PedalMode::Split => {
                let throttle = (throttle_f * GAMEPAD_TRIGGER_MAX as f64).round() as i32;
                let brake = (brake_f * GAMEPAD_TRIGGER_MAX as f64).round() as i32;
                PedalOutput::Split {
                    throttle: throttle.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX),
                    brake: brake.clamp(GAMEPAD_TRIGGER_MIN, GAMEPAD_TRIGGER_MAX),
                }
            }
            PedalMode::Combined => {
                let combined = if throttle_f > 0.0 {
                    (throttle_f * GAMEPAD_AXIS_MAX as f64).round() as i32
                } else if brake_f > 0.0 {
                    (-brake_f * GAMEPAD_AXIS_MAX as f64).round() as i32
                } else {
                    0
                };
                PedalOutput::Combined(combined.clamp(GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    const EPSILON: f64 = 1e-6;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_default_pedal_config() {
        let config = PedalConfig::default();
        assert!(approx_eq(config.center, 0.5));
        assert!(approx_eq(config.half_span, 0.5));
        assert!(approx_eq(config.deadzone, 0.05));
        assert!(approx_eq(config.curve, 1.0));
        assert!(!config.invert);
        assert_eq!(config.mode, PedalMode::Off);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_pedal_config_validation() {
        let valid = PedalConfig::default();
        assert!(valid.validate().is_ok());

        // Center out of [0.0, 1.0]
        let invalid_center_low = PedalConfig {
            center: -0.1,
            ..valid
        };
        assert!(invalid_center_low.validate().is_err());
        let invalid_center_high = PedalConfig {
            center: 1.1,
            ..valid
        };
        assert!(invalid_center_high.validate().is_err());

        // Half span in (0.0, 1.0]
        let invalid_span_zero = PedalConfig {
            half_span: 0.0,
            ..valid
        };
        assert!(invalid_span_zero.validate().is_err());
        let invalid_span_neg = PedalConfig {
            half_span: -0.5,
            ..valid
        };
        assert!(invalid_span_neg.validate().is_err());
        let invalid_span_high = PedalConfig {
            half_span: 1.01,
            ..valid
        };
        assert!(invalid_span_high.validate().is_err());

        // Deadzone in [0.0, 1.0)
        let invalid_deadzone_neg = PedalConfig {
            deadzone: -0.01,
            ..valid
        };
        assert!(invalid_deadzone_neg.validate().is_err());
        let invalid_deadzone_one = PedalConfig {
            deadzone: 1.0,
            ..valid
        };
        assert!(invalid_deadzone_one.validate().is_err());
        let invalid_deadzone_high = PedalConfig {
            deadzone: 1.5,
            ..valid
        };
        assert!(invalid_deadzone_high.validate().is_err());

        // Curve > 0.0
        let invalid_curve_zero = PedalConfig {
            curve: 0.0,
            ..valid
        };
        assert!(invalid_curve_zero.validate().is_err());
        let invalid_curve_neg = PedalConfig {
            curve: -1.0,
            ..valid
        };
        assert!(invalid_curve_neg.validate().is_err());
    }

    #[test]
    fn test_neutral_coasting_inside_deadzone() {
        let config = PedalConfig {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.05,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Split,
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        // Center position: Y = 5000 (u = 0.5)
        let (throttle_f, brake_f) = mapper.map_normalized(0.5);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(5000),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );

        // Slightly forward inside deadzone: Y = 4800 (u = 0.48, offset = 0.04 <= 0.05)
        let (throttle_f, brake_f) = mapper.map_normalized(0.48);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(4800),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );

        // Slightly backward inside deadzone: Y = 5200 (u = 0.52, offset = -0.04)
        let (throttle_f, brake_f) = mapper.map_normalized(0.52);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(5200),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );

        // Exact deadzone threshold forward: Y = 4750 (u = 0.475, offset = 0.05)
        let (throttle_f, brake_f) = mapper.map_normalized(0.475);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(4750),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );

        // Exact deadzone threshold backward: Y = 5250 (u = 0.525, offset = -0.05)
        let (throttle_f, brake_f) = mapper.map_normalized(0.525);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(5250),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );

        // Combined mode also yields 0 neutral
        let mut combined_mapper = mapper.clone();
        combined_mapper.config.mode = PedalMode::Combined;
        assert_eq!(combined_mapper.map_raw(5000), PedalOutput::Combined(0));
        assert_eq!(combined_mapper.map_raw(4800), PedalOutput::Combined(0));
        assert_eq!(combined_mapper.map_raw(5200), PedalOutput::Combined(0));
    }

    #[test]
    fn test_full_forward_throttle_lock() {
        let config = PedalConfig {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.05,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Split,
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        // Top of tablet: Y = 0 (u = 0.0) -> full throttle
        let (throttle_f, brake_f) = mapper.map_normalized(0.0);
        assert!(approx_eq(throttle_f, 1.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(0),
            PedalOutput::Split {
                throttle: 255,
                brake: 0
            }
        );

        // Beyond tablet physical top (clamping): Y = -500
        assert_eq!(
            mapper.map_raw(-500),
            PedalOutput::Split {
                throttle: 255,
                brake: 0
            }
        );
    }

    #[test]
    fn test_full_backward_brake_lock() {
        let config = PedalConfig {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.05,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Split,
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        // Bottom of tablet: Y = 10000 (u = 1.0) -> full brake
        let (throttle_f, brake_f) = mapper.map_normalized(1.0);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 1.0));
        assert_eq!(
            mapper.map_raw(10000),
            PedalOutput::Split {
                throttle: 0,
                brake: 255
            }
        );

        // Beyond tablet physical bottom (clamping): Y = 15000
        assert_eq!(
            mapper.map_raw(15000),
            PedalOutput::Split {
                throttle: 0,
                brake: 255
            }
        );
    }

    #[test]
    fn test_curve_exponential_response() {
        // Linear curve (1.0) with zero deadzone for direct comparison
        let linear_config = PedalConfig {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.0,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Split,
        };
        let linear_mapper = PedalMapper::new(AxisRange::new(0, 10000), linear_config);

        // Mid forward position: u = 0.25 (offset = +0.5)
        let (t_lin, b_lin) = linear_mapper.map_normalized(0.25);
        assert!(approx_eq(t_lin, 0.5));
        assert!(approx_eq(b_lin, 0.0));
        assert_eq!(
            linear_mapper.map_raw(2500),
            PedalOutput::Split {
                throttle: 128,
                brake: 0
            }
        );

        // Quadratic curve (2.0): progressive response
        let quad_config = PedalConfig {
            curve: 2.0,
            ..linear_config
        };
        let quad_mapper = PedalMapper::new(AxisRange::new(0, 10000), quad_config);

        let (t_quad, b_quad) = quad_mapper.map_normalized(0.25);
        assert!(approx_eq(t_quad, 0.25)); // 0.5^2 = 0.25
        assert!(approx_eq(b_quad, 0.0));
        assert_eq!(
            quad_mapper.map_raw(2500),
            PedalOutput::Split {
                throttle: 64, // round(0.25 * 255) = 64
                brake: 0
            }
        );

        // Quadratic curve on brake side: u = 0.75 (offset = -0.5)
        let (t_quad_b, b_quad_b) = quad_mapper.map_normalized(0.75);
        assert!(approx_eq(t_quad_b, 0.0));
        assert!(approx_eq(b_quad_b, 0.25)); // 0.5^2 = 0.25
        assert_eq!(
            quad_mapper.map_raw(7500),
            PedalOutput::Split {
                throttle: 0,
                brake: 64
            }
        );

        // Square root curve (0.5): aggressive initial response
        let sqrt_config = PedalConfig {
            curve: 0.5,
            ..linear_config
        };
        let sqrt_mapper = PedalMapper::new(AxisRange::new(0, 10000), sqrt_config);

        let (t_sqrt, _) = sqrt_mapper.map_normalized(0.25);
        assert!(approx_eq(t_sqrt, 0.5_f64.sqrt())); // ~0.7071
        assert_eq!(
            sqrt_mapper.map_raw(2500),
            PedalOutput::Split {
                throttle: (0.5_f64.sqrt() * 255.0).round() as i32,
                brake: 0
            }
        );
    }

    #[test]
    fn test_invert_flag() {
        let config = PedalConfig {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.05,
            curve: 1.0,
            invert: true,
            mode: PedalMode::Split,
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        // Forward physical movement (Y = 0, u = 0.0) should now act as full brake
        let (throttle_f, brake_f) = mapper.map_normalized(0.0);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 1.0));
        assert_eq!(
            mapper.map_raw(0),
            PedalOutput::Split {
                throttle: 0,
                brake: 255
            }
        );

        // Backward physical movement (Y = 10000, u = 1.0) should now act as full throttle
        let (throttle_f, brake_f) = mapper.map_normalized(1.0);
        assert!(approx_eq(throttle_f, 1.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(10000),
            PedalOutput::Split {
                throttle: 255,
                brake: 0
            }
        );

        // In combined mode with invert:
        let mut combined_mapper = mapper;
        combined_mapper.config.mode = PedalMode::Combined;
        assert_eq!(
            combined_mapper.map_raw(0),
            PedalOutput::Combined(-32767) // Forward commands negative (brake)
        );
        assert_eq!(
            combined_mapper.map_raw(10000),
            PedalOutput::Combined(32767) // Backward commands positive (throttle)
        );
    }

    #[test]
    fn test_half_span_active_zone() {
        let config = PedalConfig {
            center: 0.5,
            half_span: 0.25,
            deadzone: 0.0,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Split,
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        // Reach full throttle lock early at u = 0.25 (Y = 2500)
        let (throttle_f, brake_f) = mapper.map_normalized(0.25);
        assert!(approx_eq(throttle_f, 1.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(2500),
            PedalOutput::Split {
                throttle: 255,
                brake: 0
            }
        );

        // Beyond half span: u = 0.10 (Y = 1000) clamps to full lock
        let (throttle_f, brake_f) = mapper.map_normalized(0.10);
        assert!(approx_eq(throttle_f, 1.0));
        assert!(approx_eq(brake_f, 0.0));
        assert_eq!(
            mapper.map_raw(1000),
            PedalOutput::Split {
                throttle: 255,
                brake: 0
            }
        );

        // Reach full brake lock early at u = 0.75 (Y = 7500)
        let (throttle_f, brake_f) = mapper.map_normalized(0.75);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 1.0));
        assert_eq!(
            mapper.map_raw(7500),
            PedalOutput::Split {
                throttle: 0,
                brake: 255
            }
        );

        // Beyond half span: u = 0.90 (Y = 9000) clamps to full brake lock
        let (throttle_f, brake_f) = mapper.map_normalized(0.90);
        assert!(approx_eq(throttle_f, 0.0));
        assert!(approx_eq(brake_f, 1.0));
        assert_eq!(
            mapper.map_raw(9000),
            PedalOutput::Split {
                throttle: 0,
                brake: 255
            }
        );

        // Midpoint of throttle active zone: u = 0.375 (Y = 3750)
        // offset = (0.5 - 0.375) / 0.25 = 0.5 -> throttle = 0.5 -> 128
        let (throttle_f, _) = mapper.map_normalized(0.375);
        assert!(approx_eq(throttle_f, 0.5));
        assert_eq!(
            mapper.map_raw(3750),
            PedalOutput::Split {
                throttle: 128,
                brake: 0
            }
        );
    }

    #[test]
    fn test_combined_mode_positive_negative() {
        let config = PedalConfig {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.0,
            curve: 1.0,
            invert: false,
            mode: PedalMode::Combined,
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        // Full forward: positive 32767
        assert_eq!(mapper.map_raw(0), PedalOutput::Combined(32767));

        // Half forward: positive 16384
        assert_eq!(mapper.map_raw(2500), PedalOutput::Combined(16384));

        // Neutral center: 0
        assert_eq!(mapper.map_raw(5000), PedalOutput::Combined(0));

        // Half backward: negative -16384
        assert_eq!(mapper.map_raw(7500), PedalOutput::Combined(-16384));

        // Full backward: negative -32767
        assert_eq!(mapper.map_raw(10000), PedalOutput::Combined(-32767));
    }

    #[test]
    fn test_pedal_mode_off() {
        let config = PedalConfig {
            mode: PedalMode::Off,
            ..PedalConfig::default()
        };
        let mapper = PedalMapper::new(AxisRange::new(0, 10000), config);

        assert_eq!(mapper.map_raw(0), PedalOutput::Off);
        assert_eq!(mapper.map_raw(2500), PedalOutput::Off);
        assert_eq!(mapper.map_raw(5000), PedalOutput::Off);
        assert_eq!(mapper.map_raw(7500), PedalOutput::Off);
        assert_eq!(mapper.map_raw(10000), PedalOutput::Off);
        assert!(PedalOutput::Off.is_off());
    }

    #[test]
    fn test_from_config_and_helper() {
        let config = Config::default();
        let pedal_cfg = config.pedal_config();
        assert_eq!(pedal_cfg.center, config.pedal_center);
        assert_eq!(pedal_cfg.half_span, config.pedal_half_span);
        assert_eq!(pedal_cfg.deadzone, config.pedal_deadzone);
        assert_eq!(pedal_cfg.curve, config.pedal_curve);
        assert_eq!(pedal_cfg.invert, config.pedal_invert);
        assert_eq!(pedal_cfg.mode, config.pedal_mode);

        let from_pedal_cfg = PedalConfig::from(&config);
        assert_eq!(from_pedal_cfg, pedal_cfg);

        let from_owned_pedal_cfg = PedalConfig::from(config);
        assert_eq!(from_owned_pedal_cfg, pedal_cfg);
    }

    #[test]
    fn test_zero_span_input_range() {
        let config = PedalConfig {
            mode: PedalMode::Split,
            ..PedalConfig::default()
        };
        let mapper = PedalMapper::new(AxisRange::new(5000, 5000), config);
        // Span is 0, should default to normalized 0.5 (neutral) without crashing
        assert_eq!(
            mapper.map_raw(5000),
            PedalOutput::Split {
                throttle: 0,
                brake: 0
            }
        );
    }
}
