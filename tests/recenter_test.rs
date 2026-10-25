use wacom_steer::config::{LiftAction, SteeringMode};
use wacom_steer::launch::{handle_event, SteeringState};
use wacom_steer::mapping::{AxisRange, MappingConfig, SteeringMapper};
use wacom_steer::output::{MockGamepad, SteeringOutput};
use wacom_steer::pedal::{PedalConfig, PedalMapper};

use evdev::{AbsoluteAxisCode, EventType, InputEvent, KeyCode};

#[test]
fn test_integration_dynamic_recenter_and_restore() {
    let mut mock = MockGamepad::new();
    let mut mapper =
        SteeringMapper::with_default_output(AxisRange::new(0, 10000), MappingConfig::default());
    let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), PedalConfig::default());
    let mut rel_opt = None;

    let recenter_key = KeyCode::BTN_STYLUS;
    let mut state = SteeringState::new(true, true, 0.5, Some(recenter_key));

    // Initially center is 5000 (0.5). Coordinate 8000 maps to offset 0.6 -> positive steering
    let ev_8000 = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 8000);
    handle_event(
        &ev_8000,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();
    let initial_steer = mock.current_steering();
    assert!(initial_steer > 0);

    // Press recenter button at position 8000
    let btn_press = InputEvent::new(EventType::KEY.0, recenter_key.0, 1);
    handle_event(
        &btn_press,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();

    // Now center is 0.8, and 8000 is neutral (0)
    assert_eq!(mapper.current_center(), 0.8);
    assert_eq!(mock.current_steering(), 0);

    // Lift pen and press recenter again -> restores default 0.5
    let pen_lift = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
    handle_event(
        &pen_lift,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();

    handle_event(
        &btn_press,
        &mut state,
        &mut mapper,
        &pedal_mapper,
        &mut rel_opt,
        &mut mock,
        LiftAction::Center,
        SteeringMode::Absolute,
    )
    .unwrap();

    assert_eq!(mapper.current_center(), 0.5);
}
