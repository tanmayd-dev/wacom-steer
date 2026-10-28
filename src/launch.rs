//! Orchestration: device discovery, virtual gamepad lifecycle, event loop, and signal handling.

use evdev::{AbsoluteAxisCode, Device, EventType, InputEvent, KeyCode};
use std::error::Error;
use std::io;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::config::{
    CliArgs, Config, LiftAction, LogLevel, OperatingMode, PedalMode, SteeringMode,
};
use crate::device::{
    find_companion_pad_device, open_passive_device, parse_button_code, scan_devices, select_device,
    DeviceFilter, WacomDeviceInfo,
};
use crate::mapping::{AxisRange, SteeringMapper};
use crate::mouse::{MouseController, MouseStyle};
use crate::output::{
    MouseOutput, SteeringOutput, UinputGamepad, UinputMouse, GAMEPAD_AXIS_MAX, GAMEPAD_AXIS_MIN,
    GAMEPAD_TRIGGER_MAX, VIRTUAL_GAMEPAD_NAME, VIRTUAL_MOUSE_NAME,
};
use crate::pedal::{PedalMapper, PedalOutput};
use crate::relative::RelativeSteering;

/// Tracks the active runtime state of the pen steering system.
#[derive(Debug, Clone)]
pub struct SteeringState {
    pub pen_in_proximity: bool,
    pub has_pen_tool: bool,
    pub current_steering: i32,
    pub current_throttle: i32,
    pub current_brake: i32,
    pub current_combined_pedal: i32,
    pub last_raw_x: Option<i32>,
    pub last_raw_y: Option<i32>,
    pub default_center: f64,
    pub recenter_key: Option<KeyCode>,
    pub is_pedal_only: bool,
}

impl SteeringState {
    pub fn new(
        has_pen_tool: bool,
        initial_proximity: bool,
        default_center: f64,
        recenter_key: Option<KeyCode>,
    ) -> Self {
        Self {
            pen_in_proximity: initial_proximity,
            has_pen_tool,
            current_steering: 0,
            current_throttle: 0,
            current_brake: 0,
            current_combined_pedal: 0,
            last_raw_x: None,
            last_raw_y: None,
            default_center,
            recenter_key,
            is_pedal_only: false,
        }
    }

    /// Sets whether this state is operating in dedicated pedal-only mode.
    pub fn with_pedal_only(mut self, pedal_only: bool) -> Self {
        self.is_pedal_only = pedal_only;
        self
    }
}

/// Initializes logger based on the configured log level.
fn init_logger(level: LogLevel) {
    let filter = match level {
        LogLevel::Trace => log::LevelFilter::Trace,
        LogLevel::Debug => log::LevelFilter::Debug,
        LogLevel::Info => log::LevelFilter::Info,
        LogLevel::Warn => log::LevelFilter::Warn,
        LogLevel::Error => log::LevelFilter::Error,
    };
    let _ = env_logger::Builder::from_default_env()
        .filter_level(filter)
        .try_init();
}

/// Handles a single incoming input event and updates steering and pedal output accordingly.
#[allow(clippy::too_many_arguments)]
pub fn handle_event(
    event: &InputEvent,
    state: &mut SteeringState,
    mapper: &mut SteeringMapper,
    pedal_mapper: &PedalMapper,
    relative_steering: &mut Option<RelativeSteering>,
    output: &mut impl SteeringOutput,
    lift: LiftAction,
    steering_mode: SteeringMode,
) -> io::Result<()> {
    match event.event_type() {
        EventType::KEY => {
            let key = KeyCode::new(event.code());
            if key == KeyCode::BTN_TOOL_PEN {
                let was_in_prox = state.pen_in_proximity;
                state.pen_in_proximity = event.value() != 0;

                if was_in_prox && !state.pen_in_proximity {
                    log::debug!("Pen lifted from tablet (proximity lost)");
                    if lift == LiftAction::Center {
                        if state.current_steering != 0
                            || state.current_throttle != 0
                            || state.current_brake != 0
                            || state.current_combined_pedal != 0
                        {
                            output.center()?;
                            state.current_steering = 0;
                            state.current_throttle = 0;
                            state.current_brake = 0;
                            state.current_combined_pedal = 0;
                        }
                        if let Some(rel) = relative_steering.as_mut() {
                            rel.reset();
                        }
                    }
                } else if !was_in_prox && state.pen_in_proximity {
                    log::debug!("Pen entered tablet proximity");
                }
            } else if Some(key) == state.recenter_key && event.value() == 1 {
                // Button pressed down
                if state.is_pedal_only {
                    // In pedal-only mode, horizontal recentering is disabled
                } else if state.pen_in_proximity {
                    if let Some(raw_x) = state.last_raw_x {
                        let span = mapper.input_range.span();
                        let u = if span > 0.0 {
                            ((raw_x - mapper.input_range.min) as f64 / span).clamp(0.0, 1.0)
                        } else {
                            0.5
                        };
                        mapper.set_center(u);
                        log::info!(
                            "Dynamic recenter: new center set to {:.4} (raw X = {})",
                            u,
                            raw_x
                        );
                        if steering_mode == SteeringMode::Absolute {
                            let mapped = mapper.map_raw(raw_x);
                            if mapped != state.current_steering {
                                output.set_steering(mapped)?;
                                state.current_steering = mapped;
                            }
                        }
                    }
                } else {
                    // Reset to configured default center when button pressed while pen is lifted
                    mapper.set_center(state.default_center);
                    log::info!(
                        "Dynamic recenter: reset to default center {:.4}",
                        state.default_center
                    );
                    if steering_mode == SteeringMode::Absolute {
                        output.set_steering(0)?;
                        state.current_steering = 0;
                    }
                }
            }
        }
        EventType::ABSOLUTE => {
            let axis = AbsoluteAxisCode(event.code());
            if axis == AbsoluteAxisCode::ABS_X && (state.pen_in_proximity || !state.has_pen_tool) {
                state.last_raw_x = Some(event.value());
                if !state.is_pedal_only && steering_mode == SteeringMode::Absolute {
                    let mapped = mapper.map_raw(event.value());
                    if mapped != state.current_steering {
                        output.set_steering(mapped)?;
                        state.current_steering = mapped;
                    }
                }
            } else if axis == AbsoluteAxisCode::ABS_Y
                && (state.pen_in_proximity || !state.has_pen_tool)
            {
                state.last_raw_y = Some(event.value());
                match pedal_mapper.map_raw(event.value()) {
                    PedalOutput::Off => {}
                    PedalOutput::Split { throttle, brake } => {
                        if throttle != state.current_throttle {
                            output.set_throttle(throttle)?;
                            state.current_throttle = throttle;
                        }
                        if brake != state.current_brake {
                            output.set_brake(brake)?;
                            state.current_brake = brake;
                        }
                    }
                    PedalOutput::Combined(combined) => {
                        if combined != state.current_combined_pedal {
                            output.set_combined_pedal(combined)?;
                            state.current_combined_pedal = combined;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Processes a single frame of events from a source device (bounded by SYN_REPORT).
///
/// Handles tool proximity transitions and suppresses bogus zero-coordinate packets
/// emitted by kernel tablet drivers (such as `wacom_wac`) on proximity-out.
#[allow(clippy::too_many_arguments)]
fn process_frame(
    frame: &[InputEvent],
    state: &mut SteeringState,
    mapper: &mut SteeringMapper,
    pedal_mapper: &PedalMapper,
    relative_steering: &mut Option<RelativeSteering>,
    output: &mut impl SteeringOutput,
    lift: LiftAction,
    steering_mode: SteeringMode,
) -> io::Result<()> {
    // Check if this frame signals proximity out (BTN_TOOL_PEN = 0)
    let pen_lift_in_frame = frame.iter().any(|ev| {
        ev.event_type() == EventType::KEY && ev.code() == KeyCode::BTN_TOOL_PEN.0 && ev.value() == 0
    });

    if pen_lift_in_frame {
        let was_in_prox = state.pen_in_proximity;
        state.pen_in_proximity = false;

        if was_in_prox {
            log::debug!(
                "Pen lifted from tablet (proximity lost). Lift action: {}, holding steering={}",
                lift,
                state.current_steering
            );
            if lift == LiftAction::Center {
                if state.current_steering != 0
                    || state.current_throttle != 0
                    || state.current_brake != 0
                    || state.current_combined_pedal != 0
                {
                    output.center()?;
                    state.current_steering = 0;
                    state.current_throttle = 0;
                    state.current_brake = 0;
                    state.current_combined_pedal = 0;
                }
                if let Some(rel) = relative_steering.as_mut() {
                    rel.reset();
                }
            }
        }

        // When proximity is lost, kernel drivers emit zeroed ABS_X/ABS_Y in the same frame.
        // Process non-coordinate events (e.g. key releases), but skip ABS_X and ABS_Y
        // to protect the held values from being overwritten with bogus zeros.
        for ev in frame {
            if ev.event_type() != EventType::ABSOLUTE {
                handle_event(
                    ev,
                    state,
                    mapper,
                    pedal_mapper,
                    relative_steering,
                    output,
                    lift,
                    steering_mode,
                )?;
            }
        }
        return Ok(());
    }

    // Normal frame: process all events
    for ev in frame {
        handle_event(
            ev,
            state,
            mapper,
            pedal_mapper,
            relative_steering,
            output,
            lift,
            steering_mode,
        )?;
    }
    Ok(())
}

/// Processes a batch of events, splitting into frames by SYN_REPORT.
#[allow(clippy::too_many_arguments)]
pub fn process_event_batch(
    events: &[InputEvent],
    state: &mut SteeringState,
    mapper: &mut SteeringMapper,
    pedal_mapper: &PedalMapper,
    relative_steering: &mut Option<RelativeSteering>,
    output: &mut impl SteeringOutput,
    lift: LiftAction,
    steering_mode: SteeringMode,
) -> io::Result<()> {
    let mut frame_start = 0;
    for (i, ev) in events.iter().enumerate() {
        if ev.event_type() == EventType::SYNCHRONIZATION {
            let frame = &events[frame_start..=i];
            process_frame(
                frame,
                state,
                mapper,
                pedal_mapper,
                relative_steering,
                output,
                lift,
                steering_mode,
            )?;
            frame_start = i + 1;
        }
    }
    if frame_start < events.len() {
        let frame = &events[frame_start..];
        process_frame(
            frame,
            state,
            mapper,
            pedal_mapper,
            relative_steering,
            output,
            lift,
            steering_mode,
        )?;
    }
    Ok(())
}

/// Main event polling loop forwarding Wacom events to the steering and pedal outputs.
#[allow(clippy::too_many_arguments)]
pub fn run_event_loop(
    pen_device: &mut Device,
    mut pad_device: Option<&mut Device>,
    output: &mut impl SteeringOutput,
    mapper: &mut SteeringMapper,
    pedal_mapper: &PedalMapper,
    mut relative_steering: Option<RelativeSteering>,
    config: &Config,
    has_pen_tool: bool,
    initial_proximity: bool,
    term_signal: Arc<AtomicBool>,
) -> io::Result<()> {
    let recenter_key = parse_button_code(&config.recenter_button);
    let mut state =
        SteeringState::new(has_pen_tool, initial_proximity, config.center, recenter_key)
            .with_pedal_only(config.mode == OperatingMode::Pedal);

    let pen_fd = pen_device.as_raw_fd();
    let pad_fd_opt = pad_device.as_ref().map(|d| d.as_raw_fd());

    let mut pfds = vec![libc::pollfd {
        fd: pen_fd,
        events: libc::POLLIN,
        revents: 0,
    }];
    if let Some(p_fd) = pad_fd_opt {
        pfds.push(libc::pollfd {
            fd: p_fd,
            events: libc::POLLIN,
            revents: 0,
        });
    }

    let mut last_tick = std::time::Instant::now();

    while !term_signal.load(Ordering::Relaxed) {
        // Poll input fds with 10ms timeout for smooth relative steering integration & prompt signal handling
        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, 10) };
        if term_signal.load(Ordering::Relaxed) {
            break;
        }

        let now = std::time::Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64();
        last_tick = now;

        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            log::error!("Poll error: {err}");
            break;
        }

        if pfds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            log::warn!("Primary input device disconnected or hung up");
            break;
        }

        // Process incoming events from pen device
        if pfds[0].revents & libc::POLLIN != 0 {
            match pen_device.fetch_events() {
                Ok(events) => {
                    let batch: Vec<InputEvent> = events.collect();
                    process_event_batch(
                        &batch,
                        &mut state,
                        mapper,
                        pedal_mapper,
                        &mut relative_steering,
                        output,
                        config.lift,
                        config.steering_mode,
                    )?;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    log::warn!("Pen device read error (source device likely disconnected): {e}");
                    break;
                }
            }
        }

        // Process incoming events from companion pad device (if present)
        if pfds.len() > 1 && (pfds[1].revents & libc::POLLIN != 0) {
            if let Some(ref mut pad) = pad_device {
                match pad.fetch_events() {
                    Ok(events) => {
                        let batch: Vec<InputEvent> = events.collect();
                        process_event_batch(
                            &batch,
                            &mut state,
                            mapper,
                            pedal_mapper,
                            &mut relative_steering,
                            output,
                            config.lift,
                            config.steering_mode,
                        )?;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        log::warn!("Pad device read error: {e}");
                    }
                }
            }
        }

        // Update relative steering mode rate integration and auto-centering on each tick
        if config.mode != OperatingMode::Pedal && config.steering_mode == SteeringMode::Relative {
            if let Some(rel) = relative_steering.as_mut() {
                let deflection = if state.pen_in_proximity || !state.has_pen_tool {
                    state
                        .last_raw_x
                        .map(|rx| mapper.calculate_deflection(rx))
                        .unwrap_or(0.0)
                } else {
                    // Pen is lifted; return-to-center engages if deflection is 0
                    0.0
                };
                rel.update(deflection, dt);
                let mapped = rel.to_output_value();
                if mapped != state.current_steering {
                    output.set_steering(mapped)?;
                    state.current_steering = mapped;
                }
            }
        }
    }

    // On shutdown or source disconnect, center the steering output and flush
    log::info!("Signal or source loss received. Centering virtual gamepad and exiting...");
    output.center()?;
    Ok(())
}

/// Handles a single incoming event in Mouse mode and emits relative mouse motion.
pub fn handle_mouse_event(
    event: &InputEvent,
    controller: &mut MouseController,
    output: &mut impl MouseOutput,
) -> io::Result<()> {
    match event.event_type() {
        EventType::KEY => {
            let key = KeyCode::new(event.code());
            if key == KeyCode::BTN_TOOL_PEN {
                let in_proximity = event.value() != 0;
                controller.set_proximity(in_proximity);
                if in_proximity {
                    log::debug!("Mouse mode: Pen entered proximity");
                } else {
                    log::debug!("Mouse mode: Pen lifted from tablet");
                }
            }
        }
        EventType::ABSOLUTE => {
            let axis = AbsoluteAxisCode(event.code());
            if axis == AbsoluteAxisCode::ABS_X {
                let dx = controller.handle_raw_x(event.value());
                if dx != 0 {
                    output.emit_relative(dx, 0)?;
                }
            } else if axis == AbsoluteAxisCode::ABS_Y {
                let dy = controller.handle_raw_y(event.value());
                if dy != 0 {
                    output.emit_relative(0, dy)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Backwards compatibility alias for [`handle_mouse_event`].
pub use handle_mouse_event as handle_minecraft_event;

/// Main event polling loop forwarding Wacom events to virtual mouse output in Mouse mode.
pub fn run_mouse_event_loop(
    pen_device: &mut Device,
    mut pad_device: Option<&mut Device>,
    output: &mut impl MouseOutput,
    mut controller: MouseController,
    term_signal: Arc<AtomicBool>,
) -> io::Result<()> {
    // Acquire exclusive grab so absolute tablet coordinates do not leak to dwl/XWayland
    pen_device.grab().map_err(|e| {
        io::Error::other(format!(
            "Failed to grab source pen device exclusively for Mouse mode: {e}"
        ))
    })?;
    log::info!("Acquired exclusive grab (EVIOCGRAB) on source pen device.");

    let pen_fd = pen_device.as_raw_fd();
    let pad_fd_opt = pad_device.as_ref().map(|d| d.as_raw_fd());

    let mut pfds = vec![libc::pollfd {
        fd: pen_fd,
        events: libc::POLLIN,
        revents: 0,
    }];
    if let Some(p_fd) = pad_fd_opt {
        pfds.push(libc::pollfd {
            fd: p_fd,
            events: libc::POLLIN,
            revents: 0,
        });
    }

    let mut last_tick = std::time::Instant::now();

    while !term_signal.load(Ordering::Relaxed) {
        let ret = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, 10) };

        if term_signal.load(Ordering::Relaxed) {
            break;
        }

        let now = std::time::Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64();
        last_tick = now;

        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            log::error!("Poll error: {err}");
            break;
        }

        if pfds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            log::warn!("Primary input device disconnected or hung up");
            break;
        }

        // Process incoming events from pen device
        if pfds[0].revents & libc::POLLIN != 0 {
            match pen_device.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        handle_mouse_event(&ev, &mut controller, output)?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    log::warn!("Pen device read error: {e}");
                    break;
                }
            }
        }

        // Process companion pad events if any
        if pfds.len() > 1 && (pfds[1].revents & libc::POLLIN != 0) {
            if let Some(ref mut pad) = pad_device {
                match pad.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            handle_mouse_event(&ev, &mut controller, output)?;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        log::warn!("Pad device read error: {e}");
                    }
                }
            }
        }

        // In Rate mode: emit periodic integrated deflection deltas on each tick
        if controller.config.style == MouseStyle::Rate {
            let (dx, dy) = controller.tick_rate(dt);
            if dx != 0 || dy != 0 {
                output.emit_relative(dx, dy)?;
            }
        }
    }

    log::info!("Releasing exclusive grab on source pen device...");
    if let Err(e) = pen_device.ungrab() {
        log::warn!("Failed to ungrab pen device on exit: {e}");
    }

    Ok(())
}

/// Backwards compatibility alias for [`run_mouse_event_loop`].
pub use run_mouse_event_loop as run_minecraft_event_loop;

/// Prints diagnostics and probe metadata for dry-run/probe mode.
pub fn run_probe(
    config: &Config,
    devices: &[WacomDeviceInfo],
    selected: &WacomDeviceInfo,
    pad: Option<&WacomDeviceInfo>,
) {
    println!("============================================================");
    println!("              Wacom Steer Diagnostics (Probe)");
    println!("============================================================");
    println!();
    println!("Detected Input Devices ({} found):", devices.len());
    for d in devices {
        let marker = if d.path == selected.path {
            "-> "
        } else if pad.map(|p| p.path == d.path).unwrap_or(false) {
            "+> "
        } else {
            "   "
        };
        let vendor_str = d
            .vendor
            .map(|v| format!("{v:04x}"))
            .unwrap_or_else(|| "----".to_string());
        let product_str = d
            .product
            .map(|p| format!("{p:04x}"))
            .unwrap_or_else(|| "----".to_string());
        println!(
            "{}{:<20} {:<32} [{}:{}] Pen: {}",
            marker,
            d.path.display(),
            format!("\"{}\"", d.name),
            vendor_str,
            product_str,
            if d.has_pen_tool { "yes" } else { "no" }
        );
    }
    println!();
    println!("Selected Source Device (Pen):");
    println!("  Path:        {}", selected.path.display());
    println!("  Sysfs:       {}", selected.sysfs_path.display());
    println!("  Name:        {}", selected.name);
    println!("  Vendor ID:   0x{:04x}", selected.vendor.unwrap_or(0));
    println!("  Product ID:  0x{:04x}", selected.product.unwrap_or(0));
    println!(
        "  Pen Tool:    {}",
        if selected.has_pen_tool {
            "Supported (BTN_TOOL_PEN)"
        } else {
            "Not detected"
        }
    );
    if let Some(ref x) = selected.x_axis {
        println!(
            "  X Axis:      min={}, max={}, fuzz={}, flat={}, res={}, current={}",
            x.min, x.max, x.fuzz, x.flat, x.resolution, x.value
        );
    } else {
        println!("  X Axis:      MISSING");
    }
    if let Some(ref y) = selected.y_axis {
        println!(
            "  Y Axis:      min={}, max={}, fuzz={}, flat={}, res={}, current={}",
            y.min, y.max, y.fuzz, y.flat, y.resolution, y.value
        );
    }

    if let Some(p) = pad {
        println!();
        println!("Companion Pad Device (ExpressKeys):");
        println!("  Path:        {}", p.path.display());
        println!("  Name:        {}", p.name);
    }

    println!();
    println!("Active Operating Mode: {}", config.mode);

    if config.mode == OperatingMode::Mouse {
        println!();
        println!("Active Mouse Mode Configuration:");
        println!("  Style:       {}", config.mouse.style);
        println!("  Sensitivity: {:.2}", config.mouse.sensitivity);
        println!("  Deadzone:    {:.3}", config.mouse.deadzone);
        println!("  Invert Y:    {}", config.mouse.invert_y);
        println!("  Rate Speed:  {:.1} counts/sec", config.mouse.rate_speed);
        println!();
        println!("Target Virtual Mouse Output:");
        println!("  Name:        {}", VIRTUAL_MOUSE_NAME);
        println!("  Axes:        REL_X, REL_Y (relative mouse aiming)");
        println!("  Grabbing:    EVIOCGRAB (exclusive tablet ownership)");
        println!("  Subsystem:   Linux uinput (userspace virtual mouse)");
        println!("  Dry-run:     Active (virtual device creation was skipped)");
        println!("============================================================");
        return;
    }

    if config.mode == OperatingMode::Pedal {
        println!();
        println!("Active Pedal Configuration (Y-Axis):");
        println!("  Mode:        {}", config.pedal_mode);
        println!("  Center:      {:.3}", config.pedal_center);
        println!("  Half-Span:   {:.3}", config.pedal_half_span);
        println!("  Deadzone:    {:.3}", config.pedal_deadzone);
        println!("  Curve:       {:.3}", config.pedal_curve);
        println!("  Invert:      {}", config.pedal_invert);
        println!();
        println!("Target Virtual Gamepad Output (Pedal Mode):");
        println!("  Name:        {}", VIRTUAL_GAMEPAD_NAME);
        println!("  Steering:    ABS_X (locked to neutral center 0)");
        if config.pedal_mode == PedalMode::Split {
            println!("  Throttle:    ABS_RZ (range 0..{})", GAMEPAD_TRIGGER_MAX);
            println!("  Brake:       ABS_Z (range 0..{})", GAMEPAD_TRIGGER_MAX);
        } else if config.pedal_mode == PedalMode::Combined {
            println!(
                "  Pedals:      ABS_Y (range {}..{})",
                GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX
            );
        }
        println!("  Subsystem:   Linux uinput (userspace virtual gamepad)");
        println!("  Dry-run:     Active (virtual device creation was skipped)");
        println!("============================================================");
        return;
    }

    println!();
    println!("Active Steering Configuration:");
    println!("  Mode:        {}", config.steering_mode);
    println!("  Center:      {:.3} (normalized)", config.center);
    println!("  Half-Span:   {:.3} (normalized)", config.half_span);
    println!("  Deadzone:    {:.3}", config.deadzone);
    println!("  Curve:       {:.3} (exponent)", config.curve);
    println!("  Invert:      {}", config.invert);
    println!("  Lift Action: {}", config.lift);
    if config.steering_mode == SteeringMode::Relative {
        println!(
            "  Turn Rate:   {:.2} (sweeps/sec)",
            config.relative_turn_rate
        );
        println!(
            "  Return Rate: {:.2} (sweeps/sec)",
            config.relative_return_rate
        );
        println!("  Rel Deadzone:{:.3}", config.relative_deadzone);
    }
    println!("  Recenter Btn:{}", config.recenter_button);

    println!();
    println!("Active Pedal Configuration (Y-Axis):");
    println!("  Mode:        {}", config.pedal_mode);
    if config.pedal_mode != PedalMode::Off {
        println!("  Center:      {:.3}", config.pedal_center);
        println!("  Half-Span:   {:.3}", config.pedal_half_span);
        println!("  Deadzone:    {:.3}", config.pedal_deadzone);
        println!("  Curve:       {:.3}", config.pedal_curve);
        println!("  Invert:      {}", config.pedal_invert);
    }

    println!();
    println!("Target Virtual Gamepad Output:");
    println!("  Name:        {}", VIRTUAL_GAMEPAD_NAME);
    println!(
        "  Steering:    ABS_X (range {}..{})",
        GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX
    );
    if config.pedal_mode == PedalMode::Split {
        println!("  Throttle:    ABS_RZ (range 0..{})", GAMEPAD_TRIGGER_MAX);
        println!("  Brake:       ABS_Z (range 0..{})", GAMEPAD_TRIGGER_MAX);
    } else if config.pedal_mode == PedalMode::Combined {
        println!(
            "  Pedals:      ABS_Y (range {}..{})",
            GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX
        );
    }
    println!("  Subsystem:   Linux uinput (userspace virtual gamepad)");
    println!("  Dry-run:     Active (virtual device creation was skipped)");
    println!("============================================================");
}

/// Primary orchestrator invoked from main.
pub fn run(cli: CliArgs) -> Result<(), Box<dyn Error>> {
    // Handle --man flag immediately
    if cli.man {
        print!("{}", include_str!("../man/wacom-steer.1"));
        return Ok(());
    }

    // Handle --completions flag immediately
    if let Some(shell) = cli.completions {
        CliArgs::print_completions(shell);
        return Ok(());
    }

    // Handle --print-config flag immediately
    if cli.print_config {
        let config = Config::resolve(&cli)?;
        println!("{}", config.to_toml_string()?);
        return Ok(());
    }

    let config = Config::resolve(&cli)?;
    init_logger(config.log_level);

    log::info!("Scanning for Wacom input devices...");
    let devices = scan_devices();

    let filter = DeviceFilter {
        path: config.device.clone(),
        device_name: Some(config.device_name.clone()),
        vendor: Some(config.vendor),
        product: Some(config.product),
    };

    let selected = select_device(&devices, &filter)?;
    let pad_companion = find_companion_pad_device(&devices, &selected).cloned();

    if config.dry_run {
        run_probe(&config, &devices, &selected, pad_companion.as_ref());
        return Ok(());
    }

    let x_axis = selected.x_axis.as_ref().ok_or_else(|| {
        io::Error::other(format!(
            "Selected device '{}' has no ABS_X axis",
            selected.name
        ))
    })?;

    if config.mode == OperatingMode::Mouse {
        let y_axis = selected.y_axis.as_ref().ok_or_else(|| {
            io::Error::other(format!(
                "Selected device '{}' has no ABS_Y axis",
                selected.name
            ))
        })?;
        let mut controller = MouseController::new(
            AxisRange::new(x_axis.min, x_axis.max),
            AxisRange::new(y_axis.min, y_axis.max),
            config.mouse.clone(),
        );

        let mut source_dev = open_passive_device(&selected.path)?;

        let initial_proximity = source_dev
            .get_key_state()
            .map(|k| k.contains(KeyCode::BTN_TOOL_PEN))
            .unwrap_or(false);
        controller.set_proximity(initial_proximity);

        let mut pad_dev = if let Some(ref pad) = pad_companion {
            match open_passive_device(&pad.path) {
                Ok(d) => {
                    log::info!("Opening companion Pad device: {}", pad.path.display());
                    Some(d)
                }
                Err(e) => {
                    log::warn!(
                        "Could not open companion Pad device '{}': {e}",
                        pad.path.display()
                    );
                    None
                }
            }
        } else {
            None
        };

        log::info!("Initializing virtual mouse: '{}'...", VIRTUAL_MOUSE_NAME);
        let mut mouse_output = UinputMouse::open()?;

        let term_signal = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term_signal))?;
        signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term_signal))?;

        log::info!(
            "wacom-steer active in Mouse mode. Bridging '{}' -> '{}'. Ready for relative mouse control.",
            selected.name,
            mouse_output.device_name()
        );

        run_mouse_event_loop(
            &mut source_dev,
            pad_dev.as_mut(),
            &mut mouse_output,
            controller,
            term_signal,
        )?;

        log::info!("Shutdown complete.");
        return Ok(());
    }

    let input_range = AxisRange::new(x_axis.min, x_axis.max);
    let mut mapper = SteeringMapper::with_default_output(input_range, config.mapping_config());

    let y_range = selected
        .y_axis
        .as_ref()
        .map(|y| AxisRange::new(y.min, y.max))
        .unwrap_or_else(|| AxisRange::new(0, 1));
    let pedal_mapper = PedalMapper::new(y_range, config.pedal_config());

    let relative_steering =
        if config.mode != OperatingMode::Pedal && config.steering_mode == SteeringMode::Relative {
            Some(RelativeSteering::new(config.relative_config()))
        } else {
            None
        };

    log::info!(
        "Opening source device passively: {}",
        selected.path.display()
    );
    let mut source_dev = open_passive_device(&selected.path)?;

    let mut pad_dev = if let Some(ref pad) = pad_companion {
        match open_passive_device(&pad.path) {
            Ok(d) => {
                log::info!(
                    "Opening companion Pad device passively: {}",
                    pad.path.display()
                );
                Some(d)
            }
            Err(e) => {
                log::warn!(
                    "Could not open companion Pad device '{}': {e}",
                    pad.path.display()
                );
                None
            }
        }
    } else {
        None
    };

    let initial_proximity = source_dev
        .get_key_state()
        .map(|k| k.contains(KeyCode::BTN_TOOL_PEN))
        .unwrap_or(false);

    log::info!(
        "Initializing virtual gamepad: '{}'...",
        VIRTUAL_GAMEPAD_NAME
    );
    let mut output = UinputGamepad::open()?;

    let term_signal = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term_signal))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term_signal))?;

    if config.mode == OperatingMode::Pedal {
        log::info!(
            "wacom-steer active in Pedal mode. Bridging '{}' -> '{}'. Ready for pedal inputs.",
            selected.name,
            output.device_name()
        );
    } else {
        log::info!(
            "wacom-steer active in Joystick mode. Bridging '{}' -> '{}'. Ready for gamepad inputs / Wine / Proton.",
            selected.name,
            output.device_name()
        );
    }

    run_event_loop(
        &mut source_dev,
        pad_dev.as_mut(),
        &mut output,
        &mut mapper,
        &pedal_mapper,
        relative_steering,
        &config,
        selected.has_pen_tool,
        initial_proximity,
        term_signal,
    )?;

    log::info!("Shutdown complete.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::MockGamepad;
    use crate::pedal::PedalConfig;

    #[test]
    fn test_handle_event_proximity_and_steering() {
        let mut mock = MockGamepad::new();
        let mut mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), PedalConfig::default());
        let mut rel_opt = None;
        let mut state = SteeringState::new(true, false, 0.5, None);

        // Movement while pen is not in proximity should be ignored
        let abs_x_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 15000);
        handle_event(
            &abs_x_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert_eq!(mock.current_steering(), 0);

        // Pen enters proximity (BTN_TOOL_PEN = 1)
        let pen_down_ev = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1);
        handle_event(
            &pen_down_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert!(state.pen_in_proximity);

        // Now movement should be mapped
        handle_event(
            &abs_x_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        // 15000 on [0, 20000] with center=0.5 -> offset = (0.75 - 0.5) / 0.5 = 0.5 -> 32767 * 0.5 = 16384
        assert_eq!(mock.current_steering(), 16384);

        // Pen lifts (BTN_TOOL_PEN = 0) with LiftAction::Center -> resets to 0
        let pen_up_ev = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
        handle_event(
            &pen_up_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert!(!state.pen_in_proximity);
        assert_eq!(mock.current_steering(), 0);
    }

    #[test]
    fn test_handle_event_lift_hold() {
        let mut mock = MockGamepad::new();
        let mut mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), PedalConfig::default());
        let mut rel_opt = None;
        let mut state = SteeringState::new(true, true, 0.5, None);

        // Steer to full right
        let abs_right = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 20000);
        handle_event(
            &abs_right,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Hold,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert_eq!(mock.current_steering(), 32767);

        // Lift pen with LiftAction::Hold -> should retain full right
        let pen_up_ev = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
        handle_event(
            &pen_up_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Hold,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert!(!state.pen_in_proximity);
        assert_eq!(mock.current_steering(), 32767);
    }

    #[test]
    fn test_proximity_out_frame_with_bogus_zero_coordinates_lift_hold() {
        let mut mock = MockGamepad::new();
        let mut mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), PedalConfig::default());
        let mut rel_opt = None;
        let mut state = SteeringState::new(true, true, 0.5, None);

        // Pen at center X (10000 -> 0 steering) and top Y (0 -> full forward throttle)
        let center_x = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 10000);
        let top_y = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0);
        let syn = InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0);

        process_event_batch(
            &[center_x, top_y, syn],
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Hold,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert_eq!(mock.current_steering(), 0);
        assert_eq!(mock.current_throttle(), GAMEPAD_TRIGGER_MAX);

        // Hardware lifts: Wacom driver emits ABS_X=0, ABS_Y=0, BTN_TOOL_PEN=0 in the proximity-out frame!
        let bogus_x = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 0);
        let bogus_y = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 0);
        let pen_up = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);

        process_event_batch(
            &[bogus_x, bogus_y, pen_up, syn],
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Hold,
            SteeringMode::Absolute,
        )
        .unwrap();

        // Steering must NOT be corrupted to -32767 (sharp left)! It must hold at 0 (center)!
        assert_eq!(mock.current_steering(), 0);
        // Throttle must hold at full forward!
        assert_eq!(mock.current_throttle(), GAMEPAD_TRIGGER_MAX);
    }

    #[test]
    fn test_handle_event_pedals_split() {
        let mut mock = MockGamepad::new();
        let mut mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let mut p_cfg = PedalConfig::default();
        p_cfg.mode = PedalMode::Split;
        let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), p_cfg);
        let mut rel_opt = None;
        let mut state = SteeringState::new(true, true, 0.5, None);

        // Y=0 is full forward (throttle)
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

        // Y=10000 is full backward (brake)
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
        assert_eq!(mock.current_throttle(), 0);
        assert_eq!(mock.current_brake(), GAMEPAD_TRIGGER_MAX);

        // Y=5000 is neutral coasting
        let center_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 5000);
        handle_event(
            &center_ev,
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

        // Pen lift resets throttle/brake to 0
        let _ = handle_event(
            &forward_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        );
        assert_eq!(mock.current_throttle(), GAMEPAD_TRIGGER_MAX);

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
    }

    #[test]
    fn test_handle_event_dynamic_recenter() {
        let mut mock = MockGamepad::new();
        let mut mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), PedalConfig::default());
        let mut rel_opt = None;
        let recenter_btn = KeyCode::BTN_STYLUS;
        let mut state = SteeringState::new(true, true, 0.5, Some(recenter_btn));

        // Place pen at raw X = 15000 (normalized 0.75)
        let pen_pos = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 15000);
        handle_event(
            &pen_pos,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert_eq!(mock.current_steering(), 16384);

        // Press BTN_STYLUS (recenter)
        let btn_down = InputEvent::new(EventType::KEY.0, recenter_btn.0, 1);
        handle_event(
            &btn_down,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();

        // New center should now be 0.75, so current position 15000 outputs 0!
        assert_eq!(mapper.current_center(), 0.75);
        assert_eq!(mock.current_steering(), 0);

        // Now lift pen and press recenter button -> resets to default center 0.5
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

        handle_event(
            &btn_down,
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

    #[test]
    fn test_handle_mouse_event_delta() {
        use crate::mouse::MouseConfig;
        use crate::output::MockMouse;

        let mut mouse = MockMouse::new();
        let mut controller = MouseController::new(
            AxisRange::new(0, 20000),
            AxisRange::new(0, 10000),
            MouseConfig::default(),
        );

        // Pen enters proximity
        let pen_down = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1);
        handle_mouse_event(&pen_down, &mut controller, &mut mouse).unwrap();
        assert!(controller.pen_in_proximity);

        // Initial coordinates (seeds baseline)
        let x_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 5000);
        let y_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 5000);
        handle_mouse_event(&x_ev, &mut controller, &mut mouse).unwrap();
        handle_mouse_event(&y_ev, &mut controller, &mut mouse).unwrap();
        assert_eq!(mouse.total_dx(), 0);
        assert_eq!(mouse.total_dy(), 0);

        // Movement right (+1000 raw counts * 0.04 = +40)
        let x_move = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 6000);
        handle_mouse_event(&x_move, &mut controller, &mut mouse).unwrap();
        assert_eq!(mouse.total_dx(), 40);

        // Movement down (+500 raw counts * 0.04 = +20)
        let y_move = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 5500);
        handle_mouse_event(&y_move, &mut controller, &mut mouse).unwrap();
        assert_eq!(mouse.total_dy(), 20);

        // Pen lifts away
        let pen_up = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
        handle_mouse_event(&pen_up, &mut controller, &mut mouse).unwrap();
        assert!(!controller.pen_in_proximity);

        // Movement while lifted should produce zero delta
        let x_lifted = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 12000);
        handle_mouse_event(&x_lifted, &mut controller, &mut mouse).unwrap();
        assert_eq!(mouse.total_dx(), 40);
    }

    #[test]
    fn test_handle_event_pedal_only_mode() {
        let mut mock = MockGamepad::new();
        let mut mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let pedal_mapper = PedalMapper::new(AxisRange::new(0, 10000), PedalConfig::default());
        let mut rel_opt = None;
        let mut state = SteeringState::new(true, true, 0.5, None).with_pedal_only(true);

        // In pedal mode, X movements must NOT affect steering output (remains 0)
        let x_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 18000);
        handle_event(
            &x_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert_eq!(mock.current_steering(), 0);

        // Y movement pushes forward into throttle
        let y_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 2000);
        handle_event(
            &y_ev,
            &mut state,
            &mut mapper,
            &pedal_mapper,
            &mut rel_opt,
            &mut mock,
            LiftAction::Center,
            SteeringMode::Absolute,
        )
        .unwrap();
        assert!(mock.current_throttle() > 0);
        assert_eq!(mock.current_steering(), 0);

        // Pen lift centers pedal outputs
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
        assert_eq!(mock.current_steering(), 0);
    }
}
