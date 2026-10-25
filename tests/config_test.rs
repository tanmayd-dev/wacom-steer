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
        steering_mode: None,
        relative_turn_rate: None,
        relative_return_rate: None,
        relative_deadzone: None,
        recenter_button: None,
        pedal_mode: None,
        pedal_center: None,
        pedal_half_span: None,
        pedal_deadzone: None,
        pedal_curve: None,
        pedal_invert: false,
        lift: None,
        minecraft: false,
        minecraft_style: None,
        minecraft_sensitivity: None,
        minecraft_deadzone: None,
        minecraft_invert_y: false,
        log_level: None,
        dry_run: false,
        print_config: false,
        man: false,
        completions: None,
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
    assert_eq!(resolved.steering_mode, expected.steering_mode);
    assert_eq!(resolved.relative_turn_rate, expected.relative_turn_rate);
    assert_eq!(resolved.relative_return_rate, expected.relative_return_rate);
    assert_eq!(resolved.relative_deadzone, expected.relative_deadzone);
    assert_eq!(resolved.recenter_button, expected.recenter_button);
    assert_eq!(resolved.pedal_mode, expected.pedal_mode);
    assert_eq!(resolved.pedal_center, expected.pedal_center);
    assert_eq!(resolved.pedal_half_span, expected.pedal_half_span);
    assert_eq!(resolved.pedal_deadzone, expected.pedal_deadzone);
    assert_eq!(resolved.pedal_curve, expected.pedal_curve);
    assert_eq!(resolved.pedal_invert, expected.pedal_invert);
    assert_eq!(resolved.lift, expected.lift);
    assert_eq!(resolved.log_level, expected.log_level);
    assert_eq!(resolved.dry_run, expected.dry_run);
    assert_eq!(resolved.minecraft, expected.minecraft);
}
