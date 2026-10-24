//! Configuration management and CLI parsing.

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::device::parse_hex_u16;
use crate::mapping::MappingConfig;

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

    /// Validates configuration parameters.
    pub fn validate(&self) -> Result<(), String> {
        self.mapping_config().validate()?;
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
}
