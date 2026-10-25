use evdev::{AbsoluteAxisCode, EventType, InputEvent, KeyCode};
use wacom_steer::launch::handle_minecraft_event;
use wacom_steer::mapping::AxisRange;
use wacom_steer::minecraft::{MinecraftConfig, MinecraftController, MinecraftStyle};
use wacom_steer::output::{MockMouse, MouseOutput};

#[test]
fn test_integration_minecraft_delta_mode_natural_aiming() {
    let mut mouse = MockMouse::new();
    let config = MinecraftConfig {
        enabled: true,
        style: MinecraftStyle::Delta,
        sensitivity: 1.0,
        deadzone: 0.05,
        invert_y: false,
        rate_speed: 1200.0,
    };

    let mut controller =
        MinecraftController::new(AxisRange::new(0, 21600), AxisRange::new(0, 13500), config);

    // 1. Pen touches down in center area
    let pen_down = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1);
    handle_minecraft_event(&pen_down, &mut controller, &mut mouse).unwrap();

    let x1 = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 10000);
    let y1 = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 6000);
    handle_minecraft_event(&x1, &mut controller, &mut mouse).unwrap();
    handle_minecraft_event(&y1, &mut controller, &mut mouse).unwrap();
    assert_eq!(mouse.total_dx(), 0);
    assert_eq!(mouse.total_dy(), 0);

    // 2. Swipe right by +2000 units (dx = 2000 * 0.04 = 80)
    let x2 = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 12000);
    handle_minecraft_event(&x2, &mut controller, &mut mouse).unwrap();
    assert_eq!(mouse.total_dx(), 80);

    // 3. Swipe up by -1000 units (dy = -1000 * 0.04 = -40, looking up)
    let y2 = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 5000);
    handle_minecraft_event(&y2, &mut controller, &mut mouse).unwrap();
    assert_eq!(mouse.total_dy(), -40);

    // 4. Lift pen from tablet
    let pen_up = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
    handle_minecraft_event(&pen_up, &mut controller, &mut mouse).unwrap();

    // 5. Move across air to top-left (2000, 2000) - must NOT emit any deltas
    let x_air = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 2000);
    let y_air = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 2000);
    handle_minecraft_event(&x_air, &mut controller, &mut mouse).unwrap();
    handle_minecraft_event(&y_air, &mut controller, &mut mouse).unwrap();
    assert_eq!(mouse.total_dx(), 80);
    assert_eq!(mouse.total_dy(), -40);

    // 6. Pen re-enters proximity at top-left (2000, 2000)
    handle_minecraft_event(&pen_down, &mut controller, &mut mouse).unwrap();
    handle_minecraft_event(&x_air, &mut controller, &mut mouse).unwrap();
    handle_minecraft_event(&y_air, &mut controller, &mut mouse).unwrap();
    assert_eq!(mouse.total_dx(), 80);
    assert_eq!(mouse.total_dy(), -40);

    // 7. Swipe down by +1000 units (dy = +1000 * 0.04 = 40, looking down)
    let y3 = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 3000);
    handle_minecraft_event(&y3, &mut controller, &mut mouse).unwrap();
    assert_eq!(mouse.total_dy(), 0); // -40 + 40 = 0
}

#[test]
fn test_integration_minecraft_rate_mode_loop() {
    let mut mouse = MockMouse::new();
    let config = MinecraftConfig {
        enabled: true,
        style: MinecraftStyle::Rate,
        sensitivity: 1.0,
        deadzone: 0.05,
        invert_y: false,
        rate_speed: 1000.0,
    };

    let mut controller =
        MinecraftController::new(AxisRange::new(0, 10000), AxisRange::new(0, 10000), config);

    // Pen in proximity
    let pen_down = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1);
    handle_minecraft_event(&pen_down, &mut controller, &mut mouse).unwrap();

    // Deflection to right edge (10000) and vertical center (5000)
    let x_full = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 10000);
    let y_mid = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 5000);
    handle_minecraft_event(&x_full, &mut controller, &mut mouse).unwrap();
    handle_minecraft_event(&y_mid, &mut controller, &mut mouse).unwrap();

    // In Rate mode, handle_minecraft_event does not emit deltas directly:
    assert_eq!(mouse.total_dx(), 0);

    // Tick at 50ms (0.05s * 1000 counts/sec = 50 counts)
    let (dx, dy) = controller.tick_rate(0.05);
    mouse.emit_relative(dx, dy).unwrap();
    assert_eq!(mouse.total_dx(), 50);
    assert_eq!(mouse.total_dy(), 0);

    // Another tick at 50ms
    let (dx, dy) = controller.tick_rate(0.05);
    mouse.emit_relative(dx, dy).unwrap();
    assert_eq!(mouse.total_dx(), 100);

    // Move to center -> deflection is 0
    let x_mid = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 5000);
    handle_minecraft_event(&x_mid, &mut controller, &mut mouse).unwrap();
    let (dx, dy) = controller.tick_rate(0.05);
    mouse.emit_relative(dx, dy).unwrap();
    assert_eq!(mouse.total_dx(), 100); // no change
}
