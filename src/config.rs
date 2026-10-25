//! Configuration management and CLI parsing.

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::device::parse_hex_u16;
pub use crate::mapping::MappingConfig;
pub use crate::pedal::PedalConfig;
pub use crate::relative::RelativeConfig;

/// Pen lift behavior when pen leaves tablet proximity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LiftAction {
    /// Center steering immediately when pen is lifted (default).
    #[default]
    Center,
    /// Hold the last steering angle when pen is lifted.
    Hold,
}

impl fmt::Display for LiftAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Center => write!(f, "center"),
            Self::Hold => write!(f, "hold"),
        }
    }
}

/// Steering mode: absolute (direct mapping) or relative (rate-based with auto-centering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum SteeringMode {
    /// Hardware stylus position directly sets steering angle (default).
    #[default]
    Absolute,
    /// Stylus deflection commands turn rate, auto-centering when returned to resting position.
    Relative,
}

impl fmt::Display for SteeringMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute => write!(f, "absolute"),
            Self::Relative => write!(f, "relative"),
        }
    }
}

/// Pedal mode for mapping stylus Y-axis to throttle and brake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum PedalMode {
    /// Stylus Y-axis is disabled / ignored (default).
    #[default]
    Off,
    /// Split triggers: forward = throttle (ABS_RZ), backward = brake (ABS_Z).
    Split,
    /// Combined stick axis: ABS_Y (forward = positive/throttle, backward = negative/brake).
    Combined,
}

impl fmt::Display for PedalMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::Split => write!(f, "split"),
            Self::Combined => write!(f, "combined"),
        }
    }
}

/// Logging level for the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trace => write!(f, "trace"),
            Self::Debug => write!(f, "debug"),
            Self::Info => write!(f, "info"),
            Self::Warn => write!(f, "warn"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Command-line arguments for wacom-steer.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "wacom-steer",
    author = "tanmay",
    version,
    about = "Wacom pen steering bridge for TrackMania Nations Forever"
)]
pub struct CliArgs {
    /// Path to TOML configuration file
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Path to input event device (e.g. /dev/input/event7)
    #[arg(long, value_name = "PATH")]
    pub device: Option<PathBuf>,

    /// Substring to match against device name
    #[arg(long, value_name = "TEXT")]
    pub device_name: Option<String>,

    /// Device USB vendor ID in hex (e.g. 056a)
    #[arg(long, value_name = "HEX")]
    pub vendor: Option<String>,

    /// Device USB product ID in hex (e.g. 0375)
    #[arg(long, value_name = "HEX")]
    pub product: Option<String>,

    /// Normalized tablet center position [0.0..1.0] (default: 0.5)
    #[arg(long, value_name = "F")]
    pub center: Option<f64>,

    /// Normalized tablet half-span [> 0.0..1.0] (default: 0.5)
    #[arg(long, value_name = "F")]
    pub half_span: Option<f64>,

    /// Deadzone around center [0.0..1.0) (default: 0.0)
    #[arg(long, value_name = "F")]
    pub deadzone: Option<f64>,

    /// Exponential curve factor (> 0.0, 1.0 = linear) (default: 1.0)
    #[arg(long, value_name = "F")]
    pub curve: Option<f64>,

    /// Invert steering direction
    #[arg(long)]
    pub invert: bool,

    /// Steering mode: absolute (default) or relative
    #[arg(long, value_name = "absolute|relative")]
    pub steering_mode: Option<SteeringMode>,

    /// Full lock sweeps per second at full deflection in relative mode (default: 2.0)
    #[arg(long, value_name = "F")]
    pub relative_turn_rate: Option<f64>,

    /// Auto-centering rate in relative mode (default: 3.0)
    #[arg(long, value_name = "F")]
    pub relative_return_rate: Option<f64>,

    /// Resting deadzone around center in relative mode (default: 0.05)
    #[arg(long, value_name = "F")]
    pub relative_deadzone: Option<f64>,

    /// Recenter button: none (default), btn_stylus, btn_stylus2, btn_0..btn_9, or hex code
    #[arg(long, value_name = "NAME|HEX")]
    pub recenter_button: Option<String>,

    /// Pedal mode for stylus Y-axis: off (default), split, combined
    #[arg(long, value_name = "off|split|combined")]
    pub pedal_mode: Option<PedalMode>,

    /// Normalized tablet center for pedal Y-axis [0.0..1.0] (default: 0.5)
    #[arg(long, value_name = "F")]
    pub pedal_center: Option<f64>,

    /// Normalized tablet half-span for pedal Y-axis [> 0.0..1.0] (default: 0.5)
    #[arg(long, value_name = "F")]
    pub pedal_half_span: Option<f64>,

    /// Neutral coasting deadzone for pedals [0.0..1.0) (default: 0.05)
    #[arg(long, value_name = "F")]
    pub pedal_deadzone: Option<f64>,

    /// Exponential curve factor for pedals (> 0.0, 1.0 = linear) (default: 1.0)
    #[arg(long, value_name = "F")]
    pub pedal_curve: Option<f64>,

    /// Invert forward/backward pedal direction
    #[arg(long)]
    pub pedal_invert: bool,

    /// Pen lift behavior: center or hold (default: center)
    #[arg(long, value_name = "center|hold")]
    pub lift: Option<LiftAction>,

    /// Log level: trace, debug, info, warn, error (default: info)
    #[arg(long, value_name = "trace|debug|info|warn|error")]
    pub log_level: Option<LogLevel>,

    /// Dry run / probe: inspect devices and diagnostics, do not create virtual gamepad
    #[arg(long, alias = "probe")]
    pub dry_run: bool,

    /// Print effective configuration as TOML and exit
    #[arg(long)]
    pub print_config: bool,
}

/// Helper deserializer for u16 that accepts hex strings or numbers.
fn deserialize_hex_or_int<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let opt = Option::<toml::Value>::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(toml::Value::Integer(i)) => {
            if (0..=0xffff).contains(&i) {
                Ok(Some(i as u16))
            } else {
                Err(D::Error::custom(format!("integer {i} out of u16 range")))
            }
        }
        Some(toml::Value::String(s)) => parse_hex_u16(&s)
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("invalid hex string: '{s}'"))),
        Some(other) => Err(D::Error::custom(format!(
            "expected hex string or integer, got {other:?}"
        ))),
    }
}

/// Helper deserializer for required u16 that accepts hex strings or numbers.
fn deserialize_hex_or_int_required<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let val = toml::Value::deserialize(deserializer)?;
    match val {
        toml::Value::Integer(i) => {
            if (0..=0xffff).contains(&i) {
                Ok(i as u16)
            } else {
                Err(D::Error::custom(format!("integer {i} out of u16 range")))
            }
        }
        toml::Value::String(s) => {
            parse_hex_u16(&s).ok_or_else(|| D::Error::custom(format!("invalid hex string: '{s}'")))
        }
        other => Err(D::Error::custom(format!(
            "expected hex string or integer, got {other:?}"
        ))),
    }
}

fn serialize_hex_u16<S>(val: &u16, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("{val:04x}"))
}

/// Configuration as deserialized from file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileConfig {
    pub device: Option<PathBuf>,
    pub device_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_hex_or_int")]
    pub vendor: Option<u16>,
    #[serde(default, deserialize_with = "deserialize_hex_or_int")]
    pub product: Option<u16>,
    pub center: Option<f64>,
    pub half_span: Option<f64>,
    pub deadzone: Option<f64>,
    pub curve: Option<f64>,
    pub invert: Option<bool>,
    pub steering_mode: Option<SteeringMode>,
    pub relative_turn_rate: Option<f64>,
    pub relative_return_rate: Option<f64>,
    pub relative_deadzone: Option<f64>,
    pub recenter_button: Option<String>,
    pub pedal_mode: Option<PedalMode>,
    pub pedal_center: Option<f64>,
    pub pedal_half_span: Option<f64>,
    pub pedal_deadzone: Option<f64>,
    pub pedal_curve: Option<f64>,
    pub pedal_invert: Option<bool>,
    pub lift: Option<LiftAction>,
    pub log_level: Option<LogLevel>,
    pub dry_run: Option<bool>,
}

/// Fully resolved application configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<PathBuf>,
    pub device_name: String,
    #[serde(
        serialize_with = "serialize_hex_u16",
        deserialize_with = "deserialize_hex_or_int_required"
    )]
    pub vendor: u16,
    #[serde(
        serialize_with = "serialize_hex_u16",
        deserialize_with = "deserialize_hex_or_int_required"
    )]
    pub product: u16,
    pub center: f64,
    pub half_span: f64,
    pub deadzone: f64,
    pub curve: f64,
    pub invert: bool,
    pub steering_mode: SteeringMode,
    pub relative_turn_rate: f64,
    pub relative_return_rate: f64,
    pub relative_deadzone: f64,
    pub recenter_button: String,
    pub pedal_mode: PedalMode,
    pub pedal_center: f64,
    pub pedal_half_span: f64,
    pub pedal_deadzone: f64,
    pub pedal_curve: f64,
    pub pedal_invert: bool,
    pub lift: LiftAction,
    pub log_level: LogLevel,
    pub dry_run: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: None,
            device_name: "Wacom".to_string(),
            vendor: 0x056a,
            product: 0x0375,
            center: 0.5,
            half_span: 0.5,
            deadzone: 0.0,
            curve: 1.0,
            invert: false,
            steering_mode: SteeringMode::Absolute,
            relative_turn_rate: 2.0,
            relative_return_rate: 3.0,
            relative_deadzone: 0.05,
            recenter_button: "none".to_string(),
            pedal_mode: PedalMode::Off,
            pedal_center: 0.5,
            pedal_half_span: 0.5,
            pedal_deadzone: 0.05,
            pedal_curve: 1.0,
            pedal_invert: false,
            lift: LiftAction::Center,
            log_level: LogLevel::Info,
            dry_run: false,
        }
    }
}

impl Config {
    /// Extracts mapping-specific configuration.
    pub fn mapping_config(&self) -> MappingConfig {
        MappingConfig {
            center: self.center,
            half_span: self.half_span,
            deadzone: self.deadzone,
            curve: self.curve,
            invert: self.invert,
        }
    }

    /// Extracts pedal-specific configuration.
    pub fn pedal_config(&self) -> PedalConfig {
        PedalConfig {
            center: self.pedal_center,
            half_span: self.pedal_half_span,
            deadzone: self.pedal_deadzone,
            curve: self.pedal_curve,
            invert: self.pedal_invert,
            mode: self.pedal_mode,
        }
    }

    /// Extracts relative steering configuration.
    pub fn relative_config(&self) -> RelativeConfig {
        RelativeConfig {
            turn_rate: self.relative_turn_rate,
            return_rate: self.relative_return_rate,
            deadzone: self.relative_deadzone,
        }
    }

    /// Validates configuration parameters.
    pub fn validate(&self) -> Result<(), String> {
        self.mapping_config().validate()?;
        self.relative_config().validate()?;

        if self.pedal_mode != PedalMode::Off {
            self.pedal_config()
                .validate()
                .map_err(|e| format!("pedal_{e}"))?;
        }
        Ok(())
    }

    /// Returns the standard default user configuration file path.
    pub fn default_config_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join(".config")
                .join("wacom-steer")
                .join("config.toml")
        })
    }

    /// Resolves final configuration by merging defaults, config file, and CLI arguments.
    pub fn resolve(cli: &CliArgs) -> Result<Self, String> {
        let file_config = match &cli.config {
            Some(path) => {
                let content = fs::read_to_string(path)
                    .map_err(|e| format!("Failed to read config file {}: {e}", path.display()))?;
                toml::from_str::<FileConfig>(&content)
                    .map_err(|e| format!("Failed to parse config file {}: {e}", path.display()))?
            }
            None => {
                if let Some(path) = Self::default_config_path() {
                    if path.exists() {
                        let content = fs::read_to_string(&path).map_err(|e| {
                            format!("Failed to read default config {}: {e}", path.display())
                        })?;
                        toml::from_str::<FileConfig>(&content).map_err(|e| {
                            format!("Failed to parse default config {}: {e}", path.display())
                        })?
                    } else {
                        FileConfig::default()
                    }
                } else {
                    FileConfig::default()
                }
            }
        };

        let defaults = Config::default();

        let vendor = if let Some(ref v_str) = cli.vendor {
            parse_hex_u16(v_str).ok_or_else(|| format!("Invalid hex for --vendor: '{v_str}'"))?
        } else {
            file_config.vendor.unwrap_or(defaults.vendor)
        };

        let product = if let Some(ref p_str) = cli.product {
            parse_hex_u16(p_str).ok_or_else(|| format!("Invalid hex for --product: '{p_str}'"))?
        } else {
            file_config.product.unwrap_or(defaults.product)
        };

        let invert = if cli.invert {
            true
        } else {
            file_config.invert.unwrap_or(defaults.invert)
        };

        let pedal_invert = if cli.pedal_invert {
            true
        } else {
            file_config.pedal_invert.unwrap_or(defaults.pedal_invert)
        };

        let dry_run = if cli.dry_run {
            true
        } else {
            file_config.dry_run.unwrap_or(defaults.dry_run)
        };

        let config = Config {
            device: cli.device.clone().or(file_config.device),
            device_name: cli
                .device_name
                .clone()
                .or(file_config.device_name)
                .unwrap_or(defaults.device_name),
            vendor,
            product,
            center: cli.center.or(file_config.center).unwrap_or(defaults.center),
            half_span: cli
                .half_span
                .or(file_config.half_span)
                .unwrap_or(defaults.half_span),
            deadzone: cli
                .deadzone
                .or(file_config.deadzone)
                .unwrap_or(defaults.deadzone),
            curve: cli.curve.or(file_config.curve).unwrap_or(defaults.curve),
            invert,
            steering_mode: cli
                .steering_mode
                .or(file_config.steering_mode)
                .unwrap_or(defaults.steering_mode),
            relative_turn_rate: cli
                .relative_turn_rate
                .or(file_config.relative_turn_rate)
                .unwrap_or(defaults.relative_turn_rate),
            relative_return_rate: cli
                .relative_return_rate
                .or(file_config.relative_return_rate)
                .unwrap_or(defaults.relative_return_rate),
            relative_deadzone: cli
                .relative_deadzone
                .or(file_config.relative_deadzone)
                .unwrap_or(defaults.relative_deadzone),
            recenter_button: cli
                .recenter_button
                .clone()
                .or(file_config.recenter_button)
                .unwrap_or(defaults.recenter_button),
            pedal_mode: cli
                .pedal_mode
                .or(file_config.pedal_mode)
                .unwrap_or(defaults.pedal_mode),
            pedal_center: cli
                .pedal_center
                .or(file_config.pedal_center)
                .unwrap_or(defaults.pedal_center),
            pedal_half_span: cli
                .pedal_half_span
                .or(file_config.pedal_half_span)
                .unwrap_or(defaults.pedal_half_span),
            pedal_deadzone: cli
                .pedal_deadzone
                .or(file_config.pedal_deadzone)
                .unwrap_or(defaults.pedal_deadzone),
            pedal_curve: cli
                .pedal_curve
                .or(file_config.pedal_curve)
                .unwrap_or(defaults.pedal_curve),
            pedal_invert,
            lift: cli.lift.or(file_config.lift).unwrap_or(defaults.lift),
            log_level: cli
                .log_level
                .or(file_config.log_level)
                .unwrap_or(defaults.log_level),
            dry_run,
        };

        config.validate()?;
        Ok(config)
    }

    /// Serializes configuration to TOML string.
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

impl From<&Config> for PedalConfig {
    fn from(config: &Config) -> Self {
        config.pedal_config()
    }
}

impl From<Config> for PedalConfig {
    fn from(config: Config) -> Self {
        (&config).into()
    }
}

impl From<&Config> for RelativeConfig {
    fn from(config: &Config) -> Self {
        config.relative_config()
    }
}

impl From<Config> for RelativeConfig {
    fn from(config: Config) -> Self {
        (&config).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let default_cfg = Config::default();
        assert_eq!(default_cfg.vendor, 0x056a);
        assert_eq!(default_cfg.product, 0x0375);
        assert_eq!(default_cfg.center, 0.5);
        assert_eq!(default_cfg.half_span, 0.5);
        assert_eq!(default_cfg.deadzone, 0.0);
        assert_eq!(default_cfg.curve, 1.0);
        assert!(!default_cfg.invert);
        assert_eq!(default_cfg.lift, LiftAction::Center);
        assert_eq!(default_cfg.log_level, LogLevel::Info);
        assert!(!default_cfg.dry_run);
        assert!(default_cfg.validate().is_ok());
    }

    #[test]
    fn test_cli_overrides_file_config() {
        let toml_str = r#"
            device_name = "CustomTablet"
            vendor = "056a"
            product = "0375"
            center = 0.6
            half_span = 0.4
            deadzone = 0.05
            curve = 1.5
            invert = true
            lift = "hold"
            log_level = "debug"
            dry_run = false
        "#;
        let file_cfg: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(file_cfg.device_name.as_deref(), Some("CustomTablet"));
        assert_eq!(file_cfg.center, Some(0.6));
        assert_eq!(file_cfg.lift, Some(LiftAction::Hold));

        let cli = CliArgs {
            config: None,
            device: None,
            device_name: Some("OverrideName".to_string()),
            vendor: Some("1234".to_string()),
            product: None,
            center: Some(0.55),
            half_span: None,
            deadzone: None,
            curve: None,
            invert: false,
            steering_mode: Some(SteeringMode::Relative),
            relative_turn_rate: Some(2.5),
            relative_return_rate: Some(4.0),
            relative_deadzone: Some(0.08),
            recenter_button: Some("btn_stylus".to_string()),
            pedal_mode: Some(PedalMode::Split),
            pedal_center: Some(0.5),
            pedal_half_span: Some(0.4),
            pedal_deadzone: Some(0.06),
            pedal_curve: Some(1.2),
            pedal_invert: true,
            lift: Some(LiftAction::Center),
            log_level: None,
            dry_run: true,
            print_config: false,
        };

        let resolved = Config::resolve(&cli).unwrap();
        assert_eq!(resolved.device_name, "OverrideName");
        assert_eq!(resolved.vendor, 0x1234);
        assert_eq!(resolved.product, 0x0375);
        assert_eq!(resolved.center, 0.55);
        assert_eq!(resolved.steering_mode, SteeringMode::Relative);
        assert_eq!(resolved.relative_turn_rate, 2.5);
        assert_eq!(resolved.relative_return_rate, 4.0);
        assert_eq!(resolved.relative_deadzone, 0.08);
        assert_eq!(resolved.recenter_button, "btn_stylus");
        assert_eq!(resolved.pedal_mode, PedalMode::Split);
        assert_eq!(resolved.pedal_center, 0.5);
        assert_eq!(resolved.pedal_half_span, 0.4);
        assert_eq!(resolved.pedal_deadzone, 0.06);
        assert_eq!(resolved.pedal_curve, 1.2);
        assert!(resolved.pedal_invert);
        assert_eq!(resolved.lift, LiftAction::Center);
        assert!(resolved.dry_run);
    }

    #[test]
    fn test_print_config_toml_roundtrip() {
        let original = Config::default();
        let toml_str = original.to_toml_string().unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_pedal_config_extraction() {
        let mut config = Config::default();
        config.pedal_mode = PedalMode::Split;
        config.pedal_center = 0.45;
        config.pedal_half_span = 0.35;
        config.pedal_deadzone = 0.08;
        config.pedal_curve = 1.4;
        config.pedal_invert = true;

        let pedal_cfg = config.pedal_config();
        assert_eq!(pedal_cfg.mode, PedalMode::Split);
        assert_eq!(pedal_cfg.center, 0.45);
        assert_eq!(pedal_cfg.half_span, 0.35);
        assert_eq!(pedal_cfg.deadzone, 0.08);
        assert_eq!(pedal_cfg.curve, 1.4);
        assert!(pedal_cfg.invert);
        assert!(pedal_cfg.validate().is_ok());

        let from_ref: PedalConfig = (&config).into();
        assert_eq!(from_ref, pedal_cfg);
    }

    #[test]
    fn test_relative_config_extraction() {
        let mut config = Config::default();
        config.relative_turn_rate = 2.5;
        config.relative_return_rate = 4.0;
        config.relative_deadzone = 0.08;

        let rel_cfg = config.relative_config();
        assert_eq!(rel_cfg.turn_rate, 2.5);
        assert_eq!(rel_cfg.return_rate, 4.0);
        assert_eq!(rel_cfg.deadzone, 0.08);
        assert!(rel_cfg.validate().is_ok());

        let from_ref: RelativeConfig = (&config).into();
        assert_eq!(from_ref, rel_cfg);

        let from_val: RelativeConfig = config.clone().into();
        assert_eq!(from_val, rel_cfg);
    }
}
