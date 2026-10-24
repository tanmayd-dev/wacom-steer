use std::fs;
use std::path::Path;
use wacom_steer::config::{CliArgs, Config, FileConfig};

#[test]
fn test_packaging_default_toml_matches_defaults() {
    let path = Path::new("packaging/default.toml");
    let content = fs::read_to_string(path).expect("Failed to read packaging/default.toml");
    let _file_cfg: FileConfig =
        toml::from_str(&content).expect("Failed to parse packaging/default.toml");

    let cli = CliArgs {
        config: Some(path.to_path_buf()),
        device: None,
        device_name: None,
        vendor: None,
        product: None,
        center: None,
        half_span: None,
        deadzone: None,
        curve: None,
        invert: false,
        lift: None,
        log_level: None,
        dry_run: false,
        print_config: false,
    };

    let resolved =
        Config::resolve(&cli).expect("Failed to resolve config with packaging/default.toml");
    let expected = Config::default();

    assert_eq!(resolved.device, expected.device);
    assert_eq!(resolved.device_name, expected.device_name);
    assert_eq!(resolved.vendor, expected.vendor);
    assert_eq!(resolved.product, expected.product);
    assert_eq!(resolved.center, expected.center);
    assert_eq!(resolved.half_span, expected.half_span);
    assert_eq!(resolved.deadzone, expected.deadzone);
    assert_eq!(resolved.curve, expected.curve);
    assert_eq!(resolved.invert, expected.invert);
    assert_eq!(resolved.lift, expected.lift);
    assert_eq!(resolved.log_level, expected.log_level);
    assert_eq!(resolved.dry_run, expected.dry_run);
}
