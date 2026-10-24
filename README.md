# wacom-steer

`wacom-steer` is a lightweight, userspace bridge written in Rust that converts Wacom stylus absolute X-axis movements into an analog gamepad steering axis via Linux `uinput`. It is designed specifically for playing racing games like TrackMania Nations Forever (TMNF) under Wine/Proton.

## Features

- **Non-exclusive reading**: Does **not** grab (`EVIOCGRAB`) the device; the pen continues to control the desktop cursor normally.
- **Robust Auto-detection**: Automatically scans `/sys/class/input/event*` for Wacom Pen devices matching vendor/product IDs (default `056a:0375` for Wacom Intuos M) or Pen naming.
- **Configurable Steering Mapping**:
  - Full tablet width or customizable active width (`--half-span`).
  - Configurable physical center point (`--center`).
  - Deadzone filtering (`--deadzone`).
  - Non-linear sensitivity curves via power exponent (`--curve`).
  - Inverted steering direction (`--invert`).
- **Pen Proximity Handling**: Immediately centers steering when the pen lifts out of tablet proximity (`--lift center`, default) or maintains last angle (`--lift hold`).
- **Wine/Proton Ready**: Emulates standard gamepad button set and axes (`ABS_X`, `ABS_Y`, `ABS_RX`, `ABS_RY`) with Microsoft vendor ID compatibility so Wine and SDL2 detect it immediately.
- **Safe Lifecycle**: Centers steering on startup, on device disconnection, and on clean termination (`SIGINT`/`SIGTERM`).
- **Probe / Dry-Run**: Built-in `--dry-run` diagnostic mode to inspect devices, axis limits, and parameters without touching `uinput`.

---

## Installation & Requirements

### Prerequisites

1. Linux kernel with `uinput` enabled (`/dev/uinput`).
2. Read permissions for `/dev/input/event*` and read/write permissions for `/dev/uinput`.
   - On most modern Linux distributions (such as Arch Linux, Ubuntu, Fedora), add your user to the `input` group:
     ```sh
     sudo usermod -aG input $USER
     ```
   - If `/dev/uinput` requires a udev rule:
     ```sh
     echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/99-uinput.rules
     sudo udevadm control --reload-rules && sudo udevadm trigger
     ```

### Building

Build the release binary using Cargo:

```sh
cargo build --release
```

The compiled binary will be located at `target/release/wacom-steer`.

---

## Usage

### Quick Start

Run `wacom-steer` with automatic device detection:

```sh
wacom-steer
```

### Diagnostics & Probe Mode

To inspect connected Wacom hardware, detected axes, and effective mapping parameters without creating a virtual gamepad:

```sh
wacom-steer --dry-run
```

Sample output:
```text
============================================================
              Wacom Steer Diagnostics (Probe)
============================================================

Detected Input Devices (17 found):
-> /dev/input/event7    "Wacom Intuos M Pen"             [056a:0375] Pen: yes
   /dev/input/event8    "Wacom Intuos M Pad"             [056a:0375] Pen: no

Selected Source Device:
  Path:        /dev/input/event7
  Sysfs:       /sys/class/input/event7
  Name:        Wacom Intuos M Pen
  Vendor ID:   0x056a
  Product ID:  0x0375
  Pen Tool:    Supported (BTN_TOOL_PEN)
  X Axis:      min=0, max=21600, fuzz=4, flat=0, res=100, current=0
  Y Axis:      min=0, max=13500, fuzz=4, flat=0, res=100, current=0

Active Mapping Parameters:
  Center:      0.500 (normalized)
  Half-Span:   0.500 (normalized)
  Deadzone:    0.000
  Curve:       1.000 (exponent)
  Invert:      false
  Lift Action: center

Target Virtual Gamepad Output:
  Name:        Wacom Steer Virtual Gamepad
  Axis:        ABS_X (range -32767..32767)
  Subsystem:   Linux uinput (userspace virtual gamepad)
  Dry-run:     Active (virtual device creation was skipped)
============================================================
```

### Common Tuning Examples

- **Narrow active width for faster lock**:
  ```sh
  wacom-steer --half-span 0.35
  ```
- **Higher precision near center with sharper lock towards edges**:
  ```sh
  wacom-steer --curve 1.5
  ```
- **Add a 5% deadzone around center**:
  ```sh
  wacom-steer --deadzone 0.05
  ```
- **Target a specific device event node directly**:
  ```sh
  wacom-steer --device /dev/input/event7
  ```
- **Hold steering angle when pen leaves tablet**:
  ```sh
  wacom-steer --lift hold
  ```

---

## Configuration File

`wacom-steer` automatically looks for a configuration file at `~/.config/wacom-steer/config.toml`. A template is provided in `packaging/default.toml`.

Example `config.toml`:

```toml
# Device matching parameters
device_name = "Wacom"
vendor = "056a"
product = "0375"

# Steering mapping parameters
center = 0.5
half_span = 0.5
deadzone = 0.0
curve = 1.0
invert = false

# Pen lift behavior: "center" (default) or "hold"
lift = "center"

# Logging verbosity: "trace", "debug", "info", "warn", "error"
log_level = "info"

# Dry run / probe mode: when true, inspect device but do not create virtual gamepad
dry_run = false
```

You can view the effective merged configuration at any time by running:

```sh
wacom-steer --print-config
```

---

## CLI Reference

```text
Usage: wacom-steer [OPTIONS]

Options:
      --config <PATH>                          Path to TOML configuration file
      --device <PATH>                          Path to input event device (e.g. /dev/input/event7)
      --device-name <TEXT>                     Substring to match against device name
      --vendor <HEX>                           Device USB vendor ID in hex (e.g. 056a)
      --product <HEX>                          Device USB product ID in hex (e.g. 0375)
      --center <F>                             Normalized tablet center position [0.0..1.0] (default: 0.5)
      --half-span <F>                          Normalized tablet half-span [> 0.0..1.0] (default: 0.5)
      --deadzone <F>                           Deadzone around center [0.0..1.0) (default: 0.0)
      --curve <F>                              Exponential curve factor (> 0.0, 1.0 = linear) (default: 1.0)
      --invert                                 Invert steering direction
      --lift <center|hold>                     Pen lift behavior: center or hold (default: center)
      --log-level <trace|debug|info|warn|error> Log level (default: info)
      --dry-run                                Dry run / probe: inspect devices and diagnostics, do not create virtual gamepad
      --print-config                           Print effective configuration as TOML and exit
  -h, --help                                   Print help
  -V, --version                                Print version
```

---

## TrackMania Nations Forever (TMNF) / Proton Setup

1. Start `wacom-steer` in a terminal or background task.
2. Launch TrackMania Nations Forever through Steam / Proton.
3. In the TMNF launcher:
   - Go to **Configure** -> **Inputs**.
   - Select **Customize Controls**.
   - Under **Steer Left** and **Steer Right**, assign the analog steering axis by moving the pen to the left and right.
4. Enjoy analog steering with your Wacom pen!
