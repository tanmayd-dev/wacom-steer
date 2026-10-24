//! Pure steering mapping logic from Wacom absolute X to gamepad steering.

use serde::{Deserialize, Serialize};

/// Represents the minimum and maximum boundaries of an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisRange {
    pub min: i32,
    pub max: i32,
}

impl AxisRange {
    pub const fn new(min: i32, max: i32) -> Self {
        Self { min, max }
    }

    pub fn span(&self) -> f64 {
        (self.max - self.min) as f64
    }
}

/// Configuration parameters for steering mapping.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MappingConfig {
    /// Physical tablet center mapping to neutral steering (0.0). Default is 0.5.
    pub center: f64,
    /// Distance from center to reach full lock in normalized coordinates. Default is 0.5.
    pub half_span: f64,
    /// Deadzone around neutral steering in [0.0, 1.0). Default is 0.0.
    pub deadzone: f64,
    /// Exponential curve factor (> 0.0, 1.0 = linear). Default is 1.0.
    pub curve: f64,
    /// Whether steering direction is inverted. Default is false.
    pub invert: bool,
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self {
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.0,
            curve: 1.0,
            invert: false,
        }
    }
}

impl MappingConfig {
    /// Validates the mapping configuration parameters.
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

/// Mapper that transforms raw hardware X inputs to gamepad steering values.
#[derive(Debug, Clone)]
pub struct SteeringMapper {
    pub input_range: AxisRange,
    pub output_range: AxisRange,
    pub config: MappingConfig,
}

impl SteeringMapper {
    /// Creates a new mapper for the specified input and output axis ranges and config.
    pub fn new(input_range: AxisRange, output_range: AxisRange, config: MappingConfig) -> Self {
        Self {
            input_range,
            output_range,
            config,
        }
    }

    /// Creates a mapper with standard virtual gamepad output range [-32767, 32767].
    pub fn with_default_output(input_range: AxisRange, config: MappingConfig) -> Self {
        Self::new(input_range, AxisRange::new(-32767, 32767), config)
    }

    /// Maps a normalized tablet position [0.0, 1.0] to normalized steering in [-1.0, 1.0].
    pub fn map_normalized(&self, normalized_x: f64) -> f64 {
        // Clamp raw normalized coordinate to tablet boundaries [0.0, 1.0]
        let u = normalized_x.clamp(0.0, 1.0);

        // Calculate offset from configured center, scaled by half_span
        let half_span = if self.config.half_span.abs() < 1e-9 {
            1e-9
        } else {
            self.config.half_span
        };
        let offset = (u - self.config.center) / half_span;

        // Clamp to [-1.0, 1.0] before applying deadzone and curve
        let mut s = offset.clamp(-1.0, 1.0);

        // Apply deadzone around center
        let deadzone = self.config.deadzone.clamp(0.0, 0.999999);
        if s.abs() <= deadzone {
            s = 0.0;
        } else {
            let mag = (s.abs() - deadzone) / (1.0 - deadzone);
            s = s.signum() * mag;
        }

        // Apply curve exponent
        if (self.config.curve - 1.0).abs() > 1e-9 && self.config.curve > 0.0 {
            s = s.signum() * s.abs().powf(self.config.curve);
        }

        // Apply inversion
        if self.config.invert {
            s = -s;
        }

        // Final clamp
        s.clamp(-1.0, 1.0)
    }

    /// Converts normalized steering in [-1.0, 1.0] to integer output range.
    pub fn normalized_to_output(&self, normalized_steering: f64) -> i32 {
        let s = normalized_steering.clamp(-1.0, 1.0);
        if s == 0.0 {
            0
        } else if s > 0.0 {
            let val = (s * self.output_range.max as f64).round() as i32;
            val.clamp(0, self.output_range.max)
        } else {
            let val = (s * self.output_range.min.abs() as f64).round() as i32;
            val.clamp(self.output_range.min, 0)
        }
    }

    /// Maps raw hardware X coordinate directly to integer output steering value.
    pub fn map_raw(&self, raw_x: i32) -> i32 {
        let span = self.input_range.span();
        let u = if span > 0.0 {
            (raw_x - self.input_range.min) as f64 / span
        } else {
            0.5
        };
        let normalized = self.map_normalized(u);
        self.normalized_to_output(normalized)
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
    fn test_identity_mapping() {
        let mapper =
            SteeringMapper::with_default_output(AxisRange::new(0, 20000), MappingConfig::default());

        // Center should map to 0.0 (neutral)
        assert!(approx_eq(mapper.map_normalized(0.5), 0.0));
        assert_eq!(mapper.map_raw(10000), 0);

        // Left edge should map to -1.0 (-32767)
        assert!(approx_eq(mapper.map_normalized(0.0), -1.0));
        assert_eq!(mapper.map_raw(0), -32767);

        // Right edge should map to 1.0 (32767)
        assert!(approx_eq(mapper.map_normalized(1.0), 1.0));
        assert_eq!(mapper.map_raw(20000), 32767);

        // Quarter points
        assert!(approx_eq(mapper.map_normalized(0.25), -0.5));
        assert!(approx_eq(mapper.map_normalized(0.75), 0.5));
    }

    #[test]
    fn test_inversion() {
        let mut config = MappingConfig::default();
        config.invert = true;
        let mapper = SteeringMapper::with_default_output(AxisRange::new(0, 20000), config);

        assert!(approx_eq(mapper.map_normalized(0.5), 0.0));
        assert!(approx_eq(mapper.map_normalized(0.0), 1.0));
        assert!(approx_eq(mapper.map_normalized(1.0), -1.0));
        assert_eq!(mapper.map_raw(0), 32767);
        assert_eq!(mapper.map_raw(20000), -32767);
    }

    #[test]
    fn test_deadzone_behavior() {
        let mut config = MappingConfig::default();
        config.deadzone = 0.2;
        let mapper = SteeringMapper::with_default_output(AxisRange::new(0, 20000), config);

        // Inside deadzone around center: center=0.5, half_span=0.5
        // Normalized offsets:
        // u = 0.5 -> offset = 0.0 -> inside deadzone -> 0.0
        // u = 0.55 -> offset = 0.1 -> inside deadzone (0.1 <= 0.2) -> 0.0
        // u = 0.45 -> offset = -0.1 -> inside deadzone (-0.1 <= 0.2) -> 0.0
        // u = 0.6 -> offset = 0.2 -> at boundary -> 0.0
        assert!(approx_eq(mapper.map_normalized(0.5), 0.0));
        assert!(approx_eq(mapper.map_normalized(0.55), 0.0));
        assert!(approx_eq(mapper.map_normalized(0.45), 0.0));
        assert!(approx_eq(mapper.map_normalized(0.6), 0.0));

        // Outside deadzone:
        // u = 0.8 -> offset = 0.6. mag = (0.6 - 0.2) / (1.0 - 0.2) = 0.4 / 0.8 = 0.5
        assert!(approx_eq(mapper.map_normalized(0.8), 0.5));
        // u = 1.0 -> offset = 1.0. mag = (1.0 - 0.2) / 0.8 = 1.0
        assert!(approx_eq(mapper.map_normalized(1.0), 1.0));
        // u = 0.2 -> offset = -0.6. mag = -0.5
        assert!(approx_eq(mapper.map_normalized(0.2), -0.5));
    }

    #[test]
    fn test_curve_behavior() {
        let mut config = MappingConfig::default();
        config.curve = 2.0; // quadratic sensitivity
        let mapper = SteeringMapper::with_default_output(AxisRange::new(0, 20000), config);

        assert!(approx_eq(mapper.map_normalized(0.5), 0.0));
        // At half displacement: 0.75 -> offset = 0.5. 0.5^2 = 0.25
        assert!(approx_eq(mapper.map_normalized(0.75), 0.25));
        // Negative displacement: 0.25 -> offset = -0.5 -> - (0.5^2) = -0.25
        assert!(approx_eq(mapper.map_normalized(0.25), -0.25));
        // At extremes: 1.0^2 = 1.0
        assert!(approx_eq(mapper.map_normalized(1.0), 1.0));
        assert!(approx_eq(mapper.map_normalized(0.0), -1.0));
    }

    #[test]
    fn test_center_and_half_span_behavior() {
        let mut config = MappingConfig::default();
        config.center = 0.6;
        config.half_span = 0.25;
        let mapper = SteeringMapper::with_default_output(AxisRange::new(0, 20000), config);

        // Neutral should be at center 0.6
        assert!(approx_eq(mapper.map_normalized(0.6), 0.0));

        // Full right should be reached at center + half_span = 0.6 + 0.25 = 0.85
        assert!(approx_eq(mapper.map_normalized(0.85), 1.0));

        // Full left should be reached at center - half_span = 0.6 - 0.25 = 0.35
        assert!(approx_eq(mapper.map_normalized(0.35), -1.0));

        // Clamping beyond active span
        assert!(approx_eq(mapper.map_normalized(0.95), 1.0));
        assert!(approx_eq(mapper.map_normalized(1.0), 1.0));
        assert!(approx_eq(mapper.map_normalized(0.2), -1.0));
        assert!(approx_eq(mapper.map_normalized(0.0), -1.0));
    }

    #[test]
    fn test_clamping() {
        let mapper =
            SteeringMapper::with_default_output(AxisRange::new(100, 200), MappingConfig::default());

        // Hardware value below min clamps to -32767
        assert_eq!(mapper.map_raw(50), -32767);
        // Hardware value above max clamps to 32767
        assert_eq!(mapper.map_raw(250), 32767);

        // Normalized out of range clamps
        assert!(approx_eq(mapper.map_normalized(-0.5), -1.0));
        assert!(approx_eq(mapper.map_normalized(1.5), 1.0));
    }

    #[test]
    fn test_config_validation() {
        let valid = MappingConfig::default();
        assert!(valid.validate().is_ok());

        let invalid_center = MappingConfig {
            center: -0.1,
            ..Default::default()
        };
        assert!(invalid_center.validate().is_err());

        let invalid_half_span = MappingConfig {
            half_span: 0.0,
            ..Default::default()
        };
        assert!(invalid_half_span.validate().is_err());

        let invalid_deadzone = MappingConfig {
            deadzone: 1.0,
            ..Default::default()
        };
        assert!(invalid_deadzone.validate().is_err());

        let invalid_curve = MappingConfig {
            curve: -1.0,
            ..Default::default()
        };
        assert!(invalid_curve.validate().is_err());
    }
}
