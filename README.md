# wacom-steer

`wacom-steer` is a lightweight, high-performance userspace input bridge written in Rust that converts Wacom stylus movements into an analog gamepad controller via Linux `/dev/uinput`. It enables precise analog steering, throttle, and braking for racing, flight, and simulation games running natively or under Wine / Proton.

---

## Features

- **Non-Exclusive Reading**: Does **not** grab (`EVIOCGRAB`) the device; the pen retains full desktop cursor control.
- **Multiple Steering Modes**:
  - **Absolute Mode** *(default)*: Physical stylus position directly determines the steering angle.
  - **Relative / Steering Wheel Mode**: Pen deflection applies an incremental turn rate with automatic return-to-center spring torque when returned to the resting zone.
- **Dynamic On-The-Fly Recentering**:
  - Press a stylus side-switch (`BTN_STYLUS`, `BTN_STYLUS2`) or tablet ExpressKey (`BTN_0`..`BTN_9`) to instantly set the current pen position as neutral center.
  - Pressing the button while the pen is lifted restores the default configured center (`0.5`).
- **Acceleration & Braking (Pedal/Trigger Axes)**:
  - Maps stylus Y-axis to throttle (pushing forward) and brake/reverse (pulling backward).
  - **Split Mode**: Maps to standard Xbox 360 analog triggers (`ABS_RZ` for throttle, `ABS_Z` for brake, `0..32767`), simultaneously emitting Gamepad Button 1 (A) and Button 2 (B) for seamless compatibility with arcade racers and TrackMania.
  - **Combined Mode**: Maps to analog stick vertical axis (`ABS_Y`, `-32767..32767`).
  - Configurable central coasting deadzone, half-span travel, and sensitivity curve.
- **Multi-Device Companion Polling**: Automatically discovers and passively polls both the primary Pen node and paired ExpressKey Pad node.
- **Wine / Proton / SDL2 Ready**: Emulates an Xbox 360 controller (`045e:028e`) with standard buttons and axes recognized out-of-the-box by Wine, Proton, Steam, and SDL2.
- **Safe Lifecycle**: Automatically centers steering and drops pedals to zero on pen lift (`--lift center`), source device disconnect, and shutdown signals (`SIGINT`/`SIGTERM`).
- **Ease of Use & Shell Completions**: Built-in tab completion generator (`--completions zsh`) and pre-generated zsh completions.
- **Probe / Diagnostics**: Built-in `--dry-run` to inspect connected devices, axis resolutions, and effective mapping parameters without creating a virtual device.

---

## Installation & Requirements

### Prerequisites

1. Linux kernel with `uinput` enabled (`/dev/uinput`).
2. Read permissions for `/dev/input/event*` and read/write permissions for `/dev/uinput`:
   - On most distributions (Arch Linux, Ubuntu, Fedora), add your user to the `input` group:
     ```sh
     sudo usermod -aG input $USER
     ```
   - Ensure a udev rule exists for `/dev/uinput`:
     ```sh
     echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/99-uinput.rules
     sudo udevadm control --reload-rules && sudo udevadm trigger
     ```

### Building

Build the optimized release binary using Cargo:

```sh
cargo build --release
```

The compiled binary will be located at `target/release/wacom-steer`.

---

## Tab Completion (Zsh, Bash, Fish)

`wacom-steer` includes built-in shell argument completion generation.

### Zsh Setup

#### Option A: Instant Activation in `.zshrc`
Add this line to your `~/.zshrc`:
```sh
source <(wacom-steer --completions zsh)
```

#### Option B: Install Completion File
Copy the pre-generated completion file from this repository to your zsh functions directory:
```sh
mkdir -p ~/.zfunc
cp completions/_wacom-steer ~/.zfunc/
```
Ensure `~/.zfunc` is in your `fpath` in `~/.zshrc` before `compinit`:
```zsh
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

For system-wide installation (e.g. packages / AUR):
```sh
sudo install -Dm644 completions/_wacom-steer /usr/share/zsh/site-functions/_wacom-steer
```

---

## Manual Pages

`wacom-steer` includes standard roff manual pages for both command usage and configuration file format:
- `man/wacom-steer.1` \- Command line options, modes of operation, and examples.
- `man/wacom-steer.5` \- File format reference for `~/.config/wacom-steer/config.toml`.

### Viewing Without Installation

View the built-in manual page directly using the `--man` flag:
```sh
wacom-steer --man | man -l -
```

Or view local repo files directly with `man`:
```sh
man -l man/wacom-steer.1
man -l man/wacom-steer.5
```

### System Installation

To install man pages system-wide (or in packaging scripts):
```sh
sudo install -Dm644 man/wacom-steer.1 /usr/local/share/man/man1/wacom-steer.1
sudo install -Dm644 man/wacom-steer.5 /usr/local/share/man/man5/wacom-steer.5
sudo mandb
```

---

## Usage

### Quick Start

Run with automatic device discovery:
```sh
wacom-steer
```

### Diagnostics & Probe Mode

Inspect detected input hardware, axes, companion pad devices, and active parameters without creating a virtual gamepad:

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
+> /dev/input/event8    "Wacom Intuos M Pad"             [056a:0375] Pen: no

Selected Source Device (Pen):
  Path:        /dev/input/event7
  Sysfs:       /sys/class/input/event7
  Name:        Wacom Intuos M Pen
  Vendor ID:   0x056a
  Product ID:  0x0375
  Pen Tool:    Supported (BTN_TOOL_PEN)
  X Axis:      min=0, max=21600, fuzz=4, flat=0, res=100, current=0
  Y Axis:      min=0, max=13500, fuzz=4, flat=0, res=100, current=0

Companion Pad Device (ExpressKeys):
  Path:        /dev/input/event8
  Name:        Wacom Intuos M Pad

Active Steering Configuration:
  Mode:        absolute
  Center:      0.500 (normalized)
  Half-Span:   0.500 (normalized)
  Deadzone:    0.000
  Curve:       1.000 (exponent)
  Invert:      false
  Lift Action: center
  Recenter Btn:none

Active Pedal Configuration (Y-Axis):
  Mode:        off

Target Virtual Gamepad Output:
  Name:        Wacom Steer Virtual Gamepad
  Steering:    ABS_X (range -32767..32767)
  Subsystem:   Linux uinput (userspace virtual gamepad)
  Dry-run:     Active (virtual device creation was skipped)
============================================================
```

### Common Tuning Examples

- **Enable Acceleration & Braking on Stylus Y-Axis (Triggers)**:
  ```sh
  wacom-steer --pedal-mode split --pedal-deadzone 0.08
  ```
- **Enable Relative / Steering Wheel Rate Mode**:
  ```sh
  wacom-steer --steering-mode relative --relative-turn-rate 2.5 --relative-return-rate 3.5
  ```
- **Enable Dynamic Recentering via Stylus Side-Switch**:
  ```sh
  wacom-steer --recenter-button btn_stylus
  ```
- **Faster Steering Lock (Narrow Active Tablet Region)**:
  ```sh
  wacom-steer --half-span 0.35
  ```
- **Non-Linear Response Curve (Finer Control Near Center)**:
  ```sh
  wacom-steer --curve 1.5
  ```
- **Hold Steering When Pen Lifts**:
  ```sh
  wacom-steer --lift hold
  ```

---

## Configuration File

`wacom-steer` reads `~/.config/wacom-steer/config.toml` if present. A template is provided in `packaging/default.toml`.

Example `config.toml`:

```toml
# Device matching parameters
device_name = "Wacom"
vendor = "056a"
product = "0375"

# Steering configuration
steering_mode = "absolute"     # "absolute" or "relative"
center = 0.5                   # Neutral center [0.0..1.0]
half_span = 0.5                # Active width from center [> 0.0..1.0]
deadzone = 0.0                 # Center deadzone [0.0..1.0)
curve = 1.0                    # Exponential response curve (> 0.0)
invert = false                 # Invert steering direction

# Relative mode tuning
relative_turn_rate = 2.0       # Sweeps per second at full lock
relative_return_rate = 3.0     # Auto-centering return speed
relative_deadzone = 0.05       # Resting deadzone around neutral

# Dynamic recentering button ("none", "btn_stylus", "btn_stylus2", "btn_0", etc.)
recenter_button = "btn_stylus"

# Pedal configuration (Stylus Y-Axis)
pedal_mode = "split"           # "off", "split" (LT/RT), or "combined" (ABS_Y)
pedal_center = 0.5             # Neutral resting band [0.0..1.0]
pedal_half_span = 0.5          # Travel required for 100% pedal
pedal_deadzone = 0.05          # Coasting deadzone around center
pedal_curve = 1.0              # Pedal response curve
pedal_invert = false           # Invert forward/backward

# Pen lift behavior: "center" (default) or "hold"
lift = "center"

# Logging verbosity: "trace", "debug", "info", "warn", "error"
log_level = "info"

# Dry run / probe mode
dry_run = false
```

Print effective merged configuration:
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
      --steering-mode <absolute|relative>      Steering mode: absolute (default) or relative
      --relative-turn-rate <F>                 Full lock sweeps per second in relative mode (default: 2.0)
      --relative-return-rate <F>               Auto-centering rate in relative mode (default: 3.0)
      --relative-deadzone <F>                  Resting deadzone around center in relative mode (default: 0.05)
      --recenter-button <NAME|HEX>             Recenter button: none (default), btn_stylus, btn_0..btn_9, etc.
      --pedal-mode <off|split|combined>        Pedal mode for stylus Y-axis: off (default), split, combined
      --pedal-center <F>                       Normalized tablet center for pedal Y-axis (default: 0.5)
      --pedal-half-span <F>                    Normalized tablet half-span for pedal Y-axis (default: 0.5)
      --pedal-deadzone <F>                     Neutral coasting deadzone for pedals (default: 0.05)
      --pedal-curve <F>                        Exponential curve factor for pedals (default: 1.0)
      --pedal-invert                           Invert forward/backward pedal direction
      --lift <center|hold>                     Pen lift behavior: center or hold (default: center)
      --log-level <trace|debug|info|warn|error> Log level (default: info)
      --completions <SHELL>                    Generate shell completion script and exit [bash, zsh, fish, ...]
      --man                                    Display or export the roff man page and exit
      --dry-run                                Dry run / probe: inspect devices and diagnostics without uinput
      --print-config                           Print effective configuration as TOML and exit
  -h, --help                                   Print help
  -V, --version                                Print version
```

---

## Game & Wine / Proton Setup

1. Launch `wacom-steer` in a terminal or as a systemd user service.
2. Launch your game natively or through Steam / Proton / Wine.
3. In the game's controller or input settings:
   - The virtual device is recognized as **Wacom Steer Virtual Gamepad** (standard Xbox 360 controller layout).
   - Under Steering axis, assign the Left Stick horizontal axis by moving your stylus left and right across the tablet.
   - If using pedals (`--pedal-mode split`), assign Throttle to the Right Trigger (`ABS_RZ`) and Brake to the Left Trigger (`ABS_Z`) by moving the stylus forward or backward.
4. Enjoy smooth analog control with your stylus!
