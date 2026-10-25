use wacom_steer::config::{LiftAction, PedalConfig, PedalMode, SteeringMode};
use wacom_steer::launch::{handle_event, SteeringState};
use wacom_steer::mapping::{AxisRange, MappingConfig, SteeringMapper};
use wacom_steer::output::{MockGamepad, SteeringOutput, GAMEPAD_TRIGGER_MAX};
use wacom_steer::pedal::PedalMapper;

use evdev::{AbsoluteAxisCode, EventType, InputEvent, KeyCode};

#[test]
fn test_integration_pedal_split_and_lift_reset() {
    let mut mock = MockGamepad::new();
    let mut mapper =
        SteeringMapper::with_default_output(AxisRange::new(0, 20000), MappingConfig::default());

    let mut p_cfg = PedalConfig::default();
    p_cfg.mode = PedalMode::Split;
    p_cfg.center = 0.5;
    p_cfg.half_span = 0.5;
    p_cfg.deadzone = 0.1; // 10% deadzone
    let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), p_cfg);

    let mut rel_opt = None;
    let mut state = SteeringState::new(true, true, 0.5, None);

    // Coasting in deadzone: Y=5000 is center
    let deadzone_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 5200);
    handle_event(
        &deadzone_ev,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();
    assert_eq!(mock.current_throttle(), 0);
    assert_eq!(mock.current_brake(), 0);
    assert!(!mock.is_btn_south_pressed());
    assert!(!mock.is_btn_east_pressed());

    // Forward throttle (Y=0 -> full forward)
    let forward_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0);
    handle_event(
        &forward_ev,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();
    assert_eq!(mock.current_throttle(), GAMEPAD_TRIGGER_MAX);
    assert_eq!(mock.current_brake(), 0);
    assert!(mock.is_btn_south_pressed());

    // Pen lift centers all outputs
    let pen_up = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
    handle_event(
        &pen_up,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();
    assert_eq!(mock.current_throttle(), 0);
    assert_eq!(mock.current_brake(), 0);
    assert!(!mock.is_btn_south_pressed());
    assert!(!mock.is_btn_east_pressed());
}

#[test]
fn test_integration_pedal_combined() {
    let mut mock = MockGamepad::new();
    let mut mapper =
        SteeringMapper::with_default_output(AxisRange::new(0, 20000), MappingConfig::default());

    let mut p_cfg = PedalConfig::default();
    p_cfg.mode = PedalMode::Combined;
    let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), p_cfg);

    let mut rel_opt = None;
    let mut state = SteeringState::new(true, true, 0.5, None);

    // Forward -> negative combined pedal (-32767, stick up)
    let forward_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0);
    handle_event(
        &forward_ev,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();
    assert_eq!(mock.current_combined_pedal(), -32767);
    assert!(mock.is_btn_south_pressed());

    // Backward -> positive combined pedal (32767, stick down)
    let backward_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 10000);
    handle_event(
        &backward_ev,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();
    assert_eq!(mock.current_combined_pedal(), 32767);
    assert!(mock.is_btn_east_pressed());
}
