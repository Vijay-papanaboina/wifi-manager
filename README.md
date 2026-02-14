# wifi-manager

A lightweight, native WiFi manager for Wayland compositors. Built with Rust, GTK4, and layer-shell — designed as a proper alternative to `nmtui`, `nm-applet`, and rofi-based scripts.

## Why

There is no standalone GUI WiFi manager that works well on Wayland window managers:

| Existing tool          | Problem                                                          |
| ---------------------- | ---------------------------------------------------------------- |
| `nm-applet`            | Tray-based, scan/connect dropdown broken on Wayland              |
| `nm-connection-editor` | Only edits saved connections, no scanning                        |
| `nmtui`                | Terminal TUI, not a GUI                                          |
| `iwgtk`                | Requires iwd, most distros use NetworkManager                    |
| Rofi/wofi scripts      | No real UI — no signal bars, no live updates, no visual feedback |

**wifi-manager** fills this gap: a floating panel that scans, lists, and connects to WiFi networks with a proper GUI, live state updates, and full theming support.

## Features

- **Scan and list** available WiFi networks with signal strength, frequency band, and security info
- **Connect** to open, WPA2, and WPA3 networks with inline password entry
- **Saved network detection** — reconnects to known networks without re-entering passwords
- **Live updates** — UI reflects WiFi state changes in real time (D-Bus signal subscriptions)
- **Scan-on-show** — automatically rescans when the panel is toggled visible
- **WiFi toggle** — enable/disable the wireless radio directly from the panel
- **Daemon mode** — runs as a background process, toggled via CLI flag or D-Bus
- **Layer-shell overlay** — floating panel with no window decorations, positioned via config
- **Configurable position** — 9 anchor positions with per-edge margin offsets
- **Custom CSS theming** — override the default dark theme with your own styles

## Requirements

- Linux with Wayland (Hyprland, Sway, or any wlroots-based compositor)
- [NetworkManager](https://networkmanager.dev/) as the system network service
- GTK4 and gtk4-layer-shell libraries
- Rust toolchain (1.70+)

### System Dependencies

**Arch Linux:**

```sh
sudo pacman -S gtk4 gtk4-layer-shell networkmanager
```

**Fedora:**

```sh
sudo dnf install gtk4-devel gtk4-layer-shell-devel NetworkManager
```

**Ubuntu/Debian:**

```sh
sudo apt install libgtk-4-dev libgtk4-layer-shell-dev network-manager
```

## Build

```sh
git clone https://github.com/Vijay-papanaboina/wifi-manager.git
cd wifi-manager
cargo build --release
```

The binary will be at `./target/release/wifi-manager`.

## Usage

```sh
# Launch the daemon (panel starts hidden, then shown on first load)
wifi-manager

# Toggle panel visibility
wifi-manager --toggle
```

### Hyprland Keybind

Add to your Hyprland config:

```ini
exec-once = wifi-manager
bind = $mainMod, W, exec, wifi-manager --toggle
```

## Configuration

Configuration is loaded from `~/.config/wifi-manager/config.toml`. All fields are optional and fall back to defaults.

```toml
# Window position on screen.
# Options: center, top-right, top-center, top-left,
#          bottom-right, bottom-center, bottom-left,
#          center-right, center-left
position = "center"

# Margin offsets in pixels (only effective on anchored edges).
margin_top = 10
margin_right = 10
margin_bottom = 10
margin_left = 10
```

> **Note:** Margins only apply to edges the window is anchored to. For example, with `top-left`, only `margin_top` and `margin_left` have an effect. With `center`, no margins apply.

## Theming

wifi-manager ships with a dark default theme. To customize, create:

```
~/.config/wifi-manager/style.css
```

Your CSS overrides the default theme. Available selectors:

| Selector                 | Element                        |
| ------------------------ | ------------------------------ |
| `.wifi-panel`            | Main window container          |
| `.header`                | Top bar (toggle, status, scan) |
| `.network-list`          | Scrollable network list        |
| `.network-row`           | Individual network entry       |
| `.network-row.connected` | Connected network              |
| `.network-row.saved`     | Known/saved network            |
| `.ssid-label`            | Network name                   |
| `.signal-icon`           | Signal strength indicator      |
| `.security-icon`         | Lock/open icon                 |
| `.password-entry`        | Password input field           |
| `.connect-button`        | Connect action button          |
| `.error-label`           | Error messages                 |

## Architecture

```
src/
├── main.rs                # Entry point, CLI parsing, GTK application setup
├── app.rs                 # Application controller (UI <-> D-Bus bridge, live updates)
├── config.rs              # Configuration loader (TOML)
├── daemon.rs              # D-Bus daemon service (Toggle/Show/Hide)
├── dbus/
│   ├── proxies.rs         # D-Bus proxy trait definitions (zbus)
│   ├── network_manager.rs # High-level WiFi operations (scan, connect, disconnect)
│   ├── access_point.rs    # Data model (Network, SecurityType, Band)
│   └── connection.rs      # NM connection settings builders
└── ui/
    ├── window.rs          # Layer-shell window setup and positioning
    ├── header.rs          # Header bar (WiFi toggle, status, scan button)
    ├── network_list.rs    # Scrollable network list
    ├── network_row.rs     # Individual network row widget
    └── password_dialog.rs # Inline password entry
```

## Tech Stack

| Component           | Library                            |
| ------------------- | ---------------------------------- |
| Language            | Rust                               |
| UI framework        | GTK4                               |
| Wayland integration | gtk4-layer-shell                   |
| D-Bus client        | zbus (pure Rust, async-io backend) |
| Configuration       | serde + toml                       |
| CLI                 | clap                               |

## License

MIT
