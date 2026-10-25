//! Relative / rate-based steering mode with auto-centering.

use serde::{Deserialize, Serialize};

use crate::output::{GAMEPAD_AXIS_MAX, GAMEPAD_AXIS_MIN};

/// Configuration parameters for relative / rate-based steering mode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RelativeConfig {
    /// Full lock sweeps per second at full deflection (> 0.0). Default is 2.0.
    pub turn_rate: f64,
    /// Auto-centering rate towards neutral per second (> 0.0). Default is 3.0.
    pub return_rate: f64,
    /// Resting deadzone around center in [0.0, 1.0). Default is 0.05.
    pub deadzone: f64,
}

impl Default for RelativeConfig {
    fn default() -> Self {
        Self {
            turn_rate: 2.0,
            return_rate: 3.0,
            deadzone: 0.05,
        }
    }
}

impl RelativeConfig {
    /// Validates the relative steering configuration parameters.
    pub fn validate(&self) -> Result<(), String> {
        if self.turn_rate <= 0.0 {
            return Err(format!("turn_rate must be > 0.0, got {}", self.turn_rate));
        }
        if self.return_rate <= 0.0 {
            return Err(format!(
                "return_rate must be > 0.0, got {}",
                self.return_rate
            ));
        }
        if !(0.0..1.0).contains(&self.deadzone) {
            return Err(format!(
                "deadzone must be in [0.0, 1.0), got {}",
                self.deadzone
            ));
        }
        Ok(())
    }
}

/// Relative steering state tracker accumulating turn rates over time steps.
#[derive(Debug, Clone)]
pub struct RelativeSteering {
    /// Accumulated steering angle in [-1.0, 1.0].
    pub current_steering: f64,
    /// Relative steering configuration parameters.
    pub config: RelativeConfig,
}

impl Default for RelativeSteering {
    fn default() -> Self {
        Self::new(RelativeConfig::default())
    }
}

impl RelativeSteering {
    /// Creates a new relative steering state with steering angle initialized to 0.0.
    pub fn new(config: RelativeConfig) -> Self {
        Self {
            current_steering: 0.0,
            config,
        }
    }

    /// Updates accumulated steering angle based on pen deflection and delta time.
    ///
    /// - `deflection`: current pen deflection in `[-1.0, 1.0]` (from center).
    /// - `dt_seconds`: elapsed time in seconds since previous update.
    pub fn update(&mut self, deflection: f64, dt_seconds: f64) -> f64 {
        let deflection = deflection.clamp(-1.0, 1.0);
        let dt_seconds = dt_seconds.max(0.0);

        if deflection.abs() > self.config.deadzone {
            let deadzone = self.config.deadzone.clamp(0.0, 0.999999);
            let mag = (deflection.abs() - deadzone) / (1.0 - deadzone);
            let dir = deflection.signum();
            let delta = dir * mag * self.config.turn_rate * dt_seconds;
            self.current_steering = (self.current_steering + delta).clamp(-1.0, 1.0);
        } else {
            let step = self.config.return_rate * dt_seconds;
            if self.current_steering > 0.0 {
                self.current_steering = (self.current_steering - step).max(0.0);
            } else if self.current_steering < 0.0 {
                self.current_steering = (self.current_steering + step).min(0.0);
            }
        }

        self.current_steering.clamp(-1.0, 1.0)
    }

    /// Converts `self.current_steering` in `[-1.0, 1.0]` to integer in `[-32767, 32767]`.
    pub fn to_output_value(&self) -> i32 {
        let s = self.current_steering.clamp(-1.0, 1.0);
        if s == 0.0 {
            0
        } else if s > 0.0 {
            let val = (s * GAMEPAD_AXIS_MAX as f64).round() as i32;
            val.clamp(0, GAMEPAD_AXIS_MAX)
        } else {
            let val = (s * GAMEPAD_AXIS_MIN.abs() as f64).round() as i32;
            val.clamp(GAMEPAD_AXIS_MIN, 0)
        }
    }

    /// Clears accumulated steering to 0.0 (neutral).
    pub fn reset(&mut self) {
        self.current_steering = 0.0;
    }

    /// Explicitly sets the accumulated steering angle clamped to `[-1.0, 1.0]`.
    pub fn set_steering(&mut self, val: f64) {
        self.current_steering = val.clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-6;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_validation() {
        let valid = RelativeConfig::default();
        assert!(valid.validate().is_ok());

        let invalid_turn = RelativeConfig {
            turn_rate: 0.0,
            ..RelativeConfig::default()
        };
        assert!(invalid_turn.validate().is_err());

        let negative_turn = RelativeConfig {
            turn_rate: -1.0,
            ..RelativeConfig::default()
        };
        assert!(negative_turn.validate().is_err());

        let invalid_return = RelativeConfig {
            return_rate: 0.0,
            ..RelativeConfig::default()
        };
        assert!(invalid_return.validate().is_err());

        let negative_return = RelativeConfig {
            return_rate: -0.5,
            ..RelativeConfig::default()
        };
        assert!(negative_return.validate().is_err());

        let negative_deadzone = RelativeConfig {
            deadzone: -0.01,
            ..RelativeConfig::default()
        };
        assert!(negative_deadzone.validate().is_err());

        let deadzone_one = RelativeConfig {
            deadzone: 1.0,
            ..RelativeConfig::default()
        };
        assert!(deadzone_one.validate().is_err());

        let deadzone_over_one = RelativeConfig {
            deadzone: 1.5,
            ..RelativeConfig::default()
        };
        assert!(deadzone_over_one.validate().is_err());

        let zero_deadzone = RelativeConfig {
            deadzone: 0.0,
            ..RelativeConfig::default()
        };
        assert!(zero_deadzone.validate().is_ok());
    }

    #[test]
    fn test_sustained_deflection_increases_steering() {
        let config = RelativeConfig {
            turn_rate: 2.0,
            return_rate: 3.0,
            deadzone: 0.05,
        };
        let mut rel = RelativeSteering::new(config);
        assert!(approx_eq(rel.current_steering, 0.0));

        // Deflect right with deflection = 0.525.
        // Normalized magnitude: (0.525 - 0.05) / (1.0 - 0.05) = 0.475 / 0.95 = 0.5.
        // Rate delta per 0.1s: 1.0 * 0.5 * 2.0 * 0.1 = 0.1.
        let out1 = rel.update(0.525, 0.1);
        assert!(approx_eq(out1, 0.1));
        assert!(approx_eq(rel.current_steering, 0.1));

        // Another step of 0.1s at same deflection -> increases steering to 0.2
        let out2 = rel.update(0.525, 0.1);
        assert!(approx_eq(out2, 0.2));
        assert!(approx_eq(rel.current_steering, 0.2));

        // Sustained deflection to left (-0.525) for 0.4s
        // Delta = -1.0 * 0.5 * 2.0 * 0.4 = -0.4 -> steering moves 0.2 - 0.4 = -0.2
        let out3 = rel.update(-0.525, 0.4);
        assert!(approx_eq(out3, -0.2));
        assert!(approx_eq(rel.current_steering, -0.2));
    }

    #[test]
    fn test_clamping_at_full_lock() {
        let config = RelativeConfig {
            turn_rate: 2.0,
            return_rate: 3.0,
            deadzone: 0.0,
        };
        let mut rel = RelativeSteering::new(config);

        // Deflect fully right for 1.0s (rate 2.0 * 1.0 = +2.0, clamped to 1.0)
        let s = rel.update(1.0, 1.0);
        assert!(approx_eq(s, 1.0));
        assert!(approx_eq(rel.current_steering, 1.0));

        // Further updates stay at full lock
        let s2 = rel.update(1.0, 0.5);
        assert!(approx_eq(s2, 1.0));

        // Turn fully left for 2.0s (-1.0 * 2.0 * 2.0 = -4.0, from 1.0 reaches -1.0 lock)
        let s3 = rel.update(-1.0, 2.0);
        assert!(approx_eq(s3, -1.0));
        assert!(approx_eq(rel.current_steering, -1.0));

        // Further updates stay at -1.0 lock
        let s4 = rel.update(-1.0, 0.5);
        assert!(approx_eq(s4, -1.0));

        // set_steering clamps values beyond bounds
        rel.set_steering(2.5);
        assert!(approx_eq(rel.current_steering, 1.0));
        rel.set_steering(-3.0);
        assert!(approx_eq(rel.current_steering, -1.0));
    }

    #[test]
    fn test_resting_deadzone_auto_centers() {
        let config = RelativeConfig {
            turn_rate: 2.0,
            return_rate: 3.0,
            deadzone: 0.05,
        };
        let mut rel = RelativeSteering::new(config);

        // Start deflected at positive steering 0.9
        rel.set_steering(0.9);

        // Inside deadzone (deflection = 0.02 < 0.05) with dt = 0.1s
        // Return step = 3.0 * 0.1 = 0.3. New steering: 0.9 - 0.3 = 0.6.
        let s1 = rel.update(0.02, 0.1);
        assert!(approx_eq(s1, 0.6));

        // Second step with neutral deflection 0.0
        let s2 = rel.update(0.0, 0.1);
        assert!(approx_eq(s2, 0.3));

        // Third step brings it to exactly 0.0
        let s3 = rel.update(0.0, 0.1);
        assert!(approx_eq(s3, 0.0));

        // Further steps do not overshoot into negative
        let s4 = rel.update(0.0, 0.1);
        assert!(approx_eq(s4, 0.0));

        // Test auto-centering from negative steering (-0.7)
        rel.set_steering(-0.7);
        // Step 0.2s: step = 3.0 * 0.2 = 0.6. Steering: -0.7 + 0.6 = -0.1.
        let s5 = rel.update(-0.04, 0.2); // -0.04 is inside deadzone
        assert!(approx_eq(s5, -0.1));

        // Step 0.1s: step = 0.3. Clamps at max 0.0, doesn't overshoot into positive.
        let s6 = rel.update(0.0, 0.1);
        assert!(approx_eq(s6, 0.0));
    }

    #[test]
    fn test_reset_clears_steering() {
        let mut rel = RelativeSteering::default();
        rel.set_steering(0.85);
        assert!(approx_eq(rel.current_steering, 0.85));

        rel.reset();
        assert!(approx_eq(rel.current_steering, 0.0));

        rel.set_steering(-0.4);
        assert!(approx_eq(rel.current_steering, -0.4));

        rel.reset();
        assert!(approx_eq(rel.current_steering, 0.0));
    }

    #[test]
    fn test_to_output_value_matches_axis_range() {
        let mut rel = RelativeSteering::default();

        // Neutral steering produces 0
        rel.set_steering(0.0);
        assert_eq!(rel.to_output_value(), 0);

        // Full right produces GAMEPAD_AXIS_MAX (32767)
        rel.set_steering(1.0);
        assert_eq!(rel.to_output_value(), GAMEPAD_AXIS_MAX);

        // Full left produces GAMEPAD_AXIS_MIN (-32767)
        rel.set_steering(-1.0);
        assert_eq!(rel.to_output_value(), GAMEPAD_AXIS_MIN);

        // Half steering
        rel.set_steering(0.5);
        assert_eq!(rel.to_output_value(), (0.5f64 * 32767.0).round() as i32);

        rel.set_steering(-0.5);
        assert_eq!(rel.to_output_value(), (-0.5f64 * 32767.0).round() as i32);
    }
}
