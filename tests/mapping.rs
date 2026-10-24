use wacom_steer::mapping::{AxisRange, MappingConfig, SteeringMapper};

#[test]
fn test_integration_mapping_suite() {
    let tablet_range = AxisRange::new(0, 21600);
    let config = MappingConfig::default();
    let mapper = SteeringMapper::with_default_output(tablet_range, config);

    // Identity tests
    assert_eq!(mapper.map_raw(10800), 0);
    assert_eq!(mapper.map_raw(0), -32767);
    assert_eq!(mapper.map_raw(21600), 32767);
}

#[test]
fn test_integration_inversion() {
    let tablet_range = AxisRange::new(0, 21600);
    let mut config = MappingConfig::default();
    config.invert = true;
    let mapper = SteeringMapper::with_default_output(tablet_range, config);

    assert_eq!(mapper.map_raw(10800), 0);
    assert_eq!(mapper.map_raw(0), 32767);
    assert_eq!(mapper.map_raw(21600), -32767);
}

#[test]
fn test_integration_half_span_active_region() {
    let tablet_range = AxisRange::new(0, 20000);
    let mut config = MappingConfig::default();
    config.half_span = 0.25; // 25% of tablet on each side of center (5000 units)
    let mapper = SteeringMapper::with_default_output(tablet_range, config);

    // Center is 10000
    assert_eq!(mapper.map_raw(10000), 0);
    // Full right is at 10000 + 5000 = 15000
    assert_eq!(mapper.map_raw(15000), 32767);
    // Beyond full right is clamped
    assert_eq!(mapper.map_raw(18000), 32767);
    assert_eq!(mapper.map_raw(20000), 32767);

    // Full left is at 10000 - 5000 = 5000
    assert_eq!(mapper.map_raw(5000), -32767);
    // Beyond full left is clamped
    assert_eq!(mapper.map_raw(2000), -32767);
    assert_eq!(mapper.map_raw(0), -32767);
}

#[test]
fn test_integration_deadzone_and_curve() {
    let tablet_range = AxisRange::new(0, 10000);
    let mut config = MappingConfig::default();
    config.deadzone = 0.1;
    config.curve = 2.0;
    let mapper = SteeringMapper::with_default_output(tablet_range, config);

    // Center is 5000
    assert_eq!(mapper.map_raw(5000), 0);
    // Within 10% deadzone (span=10000, half_span=0.5 -> active span = 5000. 10% of 5000 is 500)
    assert_eq!(mapper.map_raw(5200), 0);
    assert_eq!(mapper.map_raw(4800), 0);

    // Max endpoints still reach full lock
    assert_eq!(mapper.map_raw(10000), 32767);
    assert_eq!(mapper.map_raw(0), -32767);
}
