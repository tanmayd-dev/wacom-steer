# Developer & Architecture Guide: `wacom-steer`

`wacom-steer` is a userspace Linux input bridge written in Rust that reads absolute X-axis coordinates from a Wacom stylus and maps them to analog steering on a virtual gamepad created via `/dev/uinput`.

## Architecture Overview

The crate is organized into clean, single-responsibility modules:

- **`src/mapping.rs`**: Pure mathematical mapping logic with zero system or OS dependencies.
  - Normalizes raw input coordinates $x \in [x_{\min}, x_{\max}]$ to tablet normalized range $u \in [0.0, 1.0]$.
  - Scales by configured `center` (default $0.5$) and `half_span` (default $0.5$).
  - Applies optional `deadzone` with smooth re-scaling past the threshold.
  - Applies non-linear power `curve` ($y = \text{sign}(s) \cdot |s|^{\text{curve}}$).
  - Handles `invert` flag and output clamping to $[-1.0, 1.0]$.
  - Maps to virtual gamepad axis range $[-32767, 32767]$.
  - Fully covered by unit and integration tests.

- **`src/mouse.rs`**: Relative mouse control module for 3D camera aiming and pointer navigation.
  - Translates tablet coordinates into relative mouse motion (`REL_X`, `REL_Y`).
  - Supports `Delta` mode (trackpad swipe with sub-pixel accumulation and proximity-lift baseline reset).
  - Supports `Rate` mode (joystick deflection rate with resting deadzone and periodic timer ticks).
  - Handles vertical pitch inversion and sensitivity scaling.

- **`src/output.rs`**: Output abstraction layer (`SteeringOutput` and `MouseOutput` traits).
  - `UinputGamepad`: Linux `uinput` virtual device (`Wacom Steer Virtual Gamepad`). Uses standard Microsoft USB vendor/product IDs (`045e:028e`), standard buttons (`BTN_GAMEPAD`, `BTN_A`, `BTN_B`, `BTN_X`, `BTN_Y`, `BTN_SELECT`, `BTN_START`, `BTN_MODE`), and axes (`ABS_X`, `ABS_Y`, `ABS_RX`, `ABS_RY`).
  - `UinputMouse`: Linux `uinput` virtual device (`Wacom Steer Virtual Mouse`) emitting relative deltas (`REL_X`, `REL_Y`).
  - `MockGamepad` / `MockMouse`: In-memory implementations used for testing without `/dev/uinput` access.
  - `DryRunGamepad` / `DryRunMouse`: Output sinks for diagnostics that log without device creation.

- **`src/device.rs`**: Wacom device discovery and passive reading.
  - Scans `/sys/class/input/event*` reading sysfs attributes (`device/name`, `device/id/vendor`, `device/id/product`).
  - Scores candidates to differentiate between Pen and Pad interfaces (e.g., event7 Pen vs event8 Pad on Wacom Intuos M).
  - Opens `/dev/input/event*` passively **without** `EVIOCGRAB` so the pen retains cursor tracking.

- **`src/config.rs`**: CLI (`clap`) and TOML file configuration.
  - Supports CLI overrides, default config path `~/.config/wacom-steer/config.toml`, and `packaging/default.toml`.
  - Supports hex vendor/product strings (e.g. `"056a"`) as well as integer representations.
  - Emits effective configuration as TOML on `--print-config`.

- **`src/launch.rs`**: Orchestration and lifecycle.
  - Coordinates device detection, passive source opening, uinput creation, signal handling (`signal-hook`), and main event loop.
  - Uses `libc::poll` non-blocking polling with timeout to promptly respond to `SIGINT`/`SIGTERM` and device disconnection.
  - Emits neutral steering ($0$) on startup, pen proximity lift (`BTN_TOOL_PEN=0` when `--lift center`), source device loss, and process termination.
  - Implements diagnostic probe mode (`--dry-run`).

- **`src/main.rs`**: Entrypoint parsing CLI arguments and executing `launch::run`.

---

## Critical Invariants & Constraints

1. **Passive Reading by Default (`No EVIOCGRAB` for Gamepad Steering)**:
   In standard joystick / steering and pedal modes, under no circumstances should `EVIOCGRAB` be issued on the source Wacom device; the pen must continue to control the host desktop cursor normally. In `--mode mouse` (or `--mouse`) mode, `EVIOCGRAB` is intentionally acquired so that absolute tablet coordinates do not leak to the compositor / XWayland and cause camera spinning. Grab is cleanly released on exit.

2. **No Real `/dev/uinput` in Ordinary Tests**:
   Unit and integration tests must never touch `/dev/uinput`. All testing of output behaviour must utilize `MockGamepad` or pure function interfaces.

3. **Neutral Steering Safety**:
   The output must always center ($0$):
   - At startup.
   - Immediately when the pen leaves tablet proximity (`BTN_TOOL_PEN=0`), unless `--lift hold` is explicitly requested.
   - Immediately on source device disconnection or EOF.
   - Immediately on shutdown signals (`SIGINT`, `SIGTERM`).

4. **Wine/Proton Gamepad Recognition**:
   Wine's `winebus.sys` and SDL2 require recognized button codes (`BTN_GAMEPAD` / `BTN_SOUTH`, `BTN_EAST`, `BTN_NORTH`, `BTN_WEST`, `BTN_SELECT`, `BTN_START`, `BTN_MODE`) and absolute axes (`ABS_X`, `ABS_Y`, `ABS_RX`, `ABS_RY`) to bind the virtual device as an analog joystick/gamepad.

5. **No `unwrap()` in Non-Test Code**:
   Production paths must cleanly propagate or handle errors using `Result` and `io::Error`.

6. **Single Worktree Boundary**:
   All changes, files, and scratch scripts must remain strictly within the repository worktree.

---

## Verification Commands

Ensure all checks remain clean before committing:

```sh
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo build --release
```
