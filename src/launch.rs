//! Orchestration: device discovery, virtual gamepad lifecycle, event loop, and signal handling.

use evdev::{AbsoluteAxisCode, Device, EventType, InputEvent, KeyCode};
use std::error::Error;
use std::io;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::config::{CliArgs, Config, LiftAction, LogLevel};
use crate::device::{
    open_passive_device, scan_devices, select_device, DeviceFilter, WacomDeviceInfo,
};
use crate::mapping::{AxisRange, SteeringMapper};
use crate::output::{
    SteeringOutput, UinputGamepad, GAMEPAD_AXIS_MAX, GAMEPAD_AXIS_MIN, VIRTUAL_GAMEPAD_NAME,
};

/// Tracks the active runtime state of the pen steering system.
#[derive(Debug, Clone)]
pub struct SteeringState {
    pub pen_in_proximity: bool,
    pub has_pen_tool: bool,
    pub current_steering: i32,
}

impl SteeringState {
    pub fn new(has_pen_tool: bool, initial_proximity: bool) -> Self {
        Self {
            pen_in_proximity: initial_proximity,
            has_pen_tool,
            current_steering: 0,
        }
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

/// Handles a single incoming input event and updates steering output accordingly.
pub fn handle_event(
    event: &InputEvent,
    state: &mut SteeringState,
    mapper: &SteeringMapper,
    output: &mut impl SteeringOutput,
    lift: LiftAction,
) -> io::Result<()> {
    match event.event_type() {
        EventType::KEY if event.code() == KeyCode::BTN_TOOL_PEN.0 => {
            let was_in_prox = state.pen_in_proximity;
            state.pen_in_proximity = event.value() != 0;

            if was_in_prox && !state.pen_in_proximity {
                log::debug!("Pen lifted from tablet (proximity lost)");
                if lift == LiftAction::Center && state.current_steering != 0 {
                    output.center()?;
                    state.current_steering = 0;
                }
            } else if !was_in_prox && state.pen_in_proximity {
                log::debug!("Pen entered tablet proximity");
            }
        }
        EventType::ABSOLUTE
            if event.code() == AbsoluteAxisCode::ABS_X.0
                && (state.pen_in_proximity || !state.has_pen_tool) =>
        {
            let mapped = mapper.map_raw(event.value());
            if mapped != state.current_steering {
                output.set_steering(mapped)?;
                state.current_steering = mapped;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Main event polling loop forwarding Wacom events to the steering output.
pub fn run_event_loop(
    device: &mut Device,
    output: &mut impl SteeringOutput,
    mapper: &SteeringMapper,
    lift: LiftAction,
    has_pen_tool: bool,
    initial_proximity: bool,
    term_signal: Arc<AtomicBool>,
) -> io::Result<()> {
    let mut state = SteeringState::new(has_pen_tool, initial_proximity);

    let raw_fd = device.as_raw_fd();
    let mut pfd = libc::pollfd {
        fd: raw_fd,
        events: libc::POLLIN,
        revents: 0,
    };

    while !term_signal.load(Ordering::Relaxed) {
        // Poll input fd with 50ms timeout so we regularly check termination flag
        let ret = unsafe { libc::poll(&mut pfd, 1, 50) };
        if term_signal.load(Ordering::Relaxed) {
            break;
        }
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            log::error!("Poll error: {err}");
            break;
        }
        if ret == 0 {
            // Timeout expired without input events; loop to recheck flags
            continue;
        }

        if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            log::warn!("Input device disconnected or hung up");
            break;
        }

        if pfd.revents & libc::POLLIN != 0 {
            match device.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        handle_event(&ev, &mut state, mapper, output, lift)?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    log::warn!("Device read error (source device likely disconnected): {e}");
                    break;
                }
            }
        }
    }

    // On shutdown or source disconnect, center the steering output and flush
    log::info!("Signal or source loss received. Centering virtual gamepad and exiting...");
    output.center()?;
    Ok(())
}

/// Prints diagnostics and probe metadata for dry-run/probe mode.
pub fn run_probe(config: &Config, devices: &[WacomDeviceInfo], selected: &WacomDeviceInfo) {
    println!("============================================================");
    println!("              Wacom Steer Diagnostics (Probe)");
    println!("============================================================");
    println!();
    println!("Detected Input Devices ({} found):", devices.len());
    for d in devices {
        let marker = if d.path == selected.path {
            "-> "
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
    println!("Selected Source Device:");
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
    println!();
    println!("Active Mapping Parameters:");
    println!("  Center:      {:.3} (normalized)", config.center);
    println!("  Half-Span:   {:.3} (normalized)", config.half_span);
    println!("  Deadzone:    {:.3}", config.deadzone);
    println!("  Curve:       {:.3} (exponent)", config.curve);
    println!("  Invert:      {}", config.invert);
    println!("  Lift Action: {}", config.lift);
    println!();
    println!("Target Virtual Gamepad Output:");
    println!("  Name:        {}", VIRTUAL_GAMEPAD_NAME);
    println!(
        "  Axis:        ABS_X (range {}..{})",
        GAMEPAD_AXIS_MIN, GAMEPAD_AXIS_MAX
    );
    println!("  Subsystem:   Linux uinput (userspace virtual gamepad)");
    println!("  Dry-run:     Active (virtual device creation was skipped)");
    println!("============================================================");
}

/// Primary orchestrator invoked from main.
pub fn run(cli: CliArgs) -> Result<(), Box<dyn Error>> {
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

    if config.dry_run {
        run_probe(&config, &devices, &selected);
        return Ok(());
    }

    let x_axis = selected.x_axis.as_ref().ok_or_else(|| {
        io::Error::other(format!(
            "Selected device '{}' has no ABS_X axis",
            selected.name
        ))
    })?;

    let input_range = AxisRange::new(x_axis.min, x_axis.max);
    let mapper = SteeringMapper::with_default_output(input_range, config.mapping_config());

    log::info!(
        "Opening source device passively: {}",
        selected.path.display()
    );
    let mut source_dev = open_passive_device(&selected.path)?;

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

    log::info!(
        "wacom-steer active. Bridging '{}' -> '{}'. Ready for TrackMania / Proton.",
        selected.name,
        output.device_name()
    );

    run_event_loop(
        &mut source_dev,
        &mut output,
        &mapper,
        config.lift,
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

    #[test]
    fn test_handle_event_proximity_and_steering() {
        let mut mock = MockGamepad::new();
        let mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let mut state = SteeringState::new(true, false);

        // Movement while pen is not in proximity should be ignored
        let abs_x_ev = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 15000);
        handle_event(
            &abs_x_ev,
            &mut state,
            &mapper,
            &mut mock,
            LiftAction::Center,
        )
        .unwrap();
        assert_eq!(mock.current_steering(), 0);

        // Pen enters proximity (BTN_TOOL_PEN = 1)
        let pen_down_ev = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 1);
        handle_event(
            &pen_down_ev,
            &mut state,
            &mapper,
            &mut mock,
            LiftAction::Center,
        )
        .unwrap();
        assert!(state.pen_in_proximity);

        // Now movement should be mapped
        handle_event(
            &abs_x_ev,
            &mut state,
            &mapper,
            &mut mock,
            LiftAction::Center,
        )
        .unwrap();
        // 15000 on [0, 20000] with center=0.5 -> offset = (0.75 - 0.5) / 0.5 = 0.5 -> 32767 * 0.5 = 16384
        assert_eq!(mock.current_steering(), 16384);

        // Pen lifts (BTN_TOOL_PEN = 0) with LiftAction::Center -> resets to 0
        let pen_up_ev = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
        handle_event(
            &pen_up_ev,
            &mut state,
            &mapper,
            &mut mock,
            LiftAction::Center,
        )
        .unwrap();
        assert!(!state.pen_in_proximity);
        assert_eq!(mock.current_steering(), 0);
    }

    #[test]
    fn test_handle_event_lift_hold() {
        let mut mock = MockGamepad::new();
        let mapper = SteeringMapper::with_default_output(
            AxisRange::new(0, 20000),
            crate::mapping::MappingConfig::default(),
        );
        let mut state = SteeringState::new(true, true);

        // Steer to full right
        let abs_right = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 20000);
        handle_event(&abs_right, &mut state, &mapper, &mut mock, LiftAction::Hold).unwrap();
        assert_eq!(mock.current_steering(), 32767);

        // Lift pen with LiftAction::Hold -> should retain full right
        let pen_up_ev = InputEvent::new(EventType::KEY.0, KeyCode::BTN_TOOL_PEN.0, 0);
        handle_event(&pen_up_ev, &mut state, &mapper, &mut mock, LiftAction::Hold).unwrap();
        assert!(!state.pen_in_proximity);
        assert_eq!(mock.current_steering(), 32767);
    }
}
