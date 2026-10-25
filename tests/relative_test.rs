use wacom_steer::config::RelativeConfig;
use wacom_steer::relative::RelativeSteering;

#[test]
fn test_integration_relative_steering_full_cycle() {
    let mut config = RelativeConfig::default();
    config.turn_rate = 2.0;
    config.return_rate = 4.0;
    config.deadzone = 0.05;

    let mut rel = RelativeSteering::new(config);
    assert_eq!(rel.current_steering, 0.0);
    assert_eq!(rel.to_output_value(), 0);

    // Deflect right by 50% for 0.5s:
    // mag = (0.5 - 0.05) / 0.95 ≈ 0.47368
    // delta = 1.0 * 0.47368 * 2.0 * 0.5 ≈ 0.47368
    rel.update(0.5, 0.5);
    let st1 = rel.current_steering;
    assert!(st1 > 0.45 && st1 < 0.5);
    assert!(rel.to_output_value() > 14000);

    // Deflect full right for 2.0s -> reaches lock 1.0
    rel.update(1.0, 2.0);
    assert_eq!(rel.current_steering, 1.0);
    assert_eq!(rel.to_output_value(), 32767);

    // Return to resting position (deflection = 0.0) for 0.1s
    // Decay: 1.0 - 4.0 * 0.1 = 0.6
    rel.update(0.0, 0.1);
    assert!((rel.current_steering - 0.6).abs() < 1e-4);

    // Resting for another 0.2s: 0.6 - 4.0 * 0.2 = -0.2 -> clamps to 0.0
    rel.update(0.0, 0.2);
    assert_eq!(rel.current_steering, 0.0);
    assert_eq!(rel.to_output_value(), 0);
}
