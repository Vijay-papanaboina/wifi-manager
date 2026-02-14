# wifi-manager

A lightweight, floating WiFi manager for Wayland compositors like **Hyprland** and **Sway**. Built with Rust, GTK4, and layer-shell — designed as a proper native alternative to `nmtui` and rofi scripts.

> **⚠️ Status: Work in Progress** — The D-Bus backend is complete. GTK4 UI is next.

## Why

There is no standalone GUI WiFi manager that works well on Wayland window managers:

| Existing tool          | Problem                                                          |
| ---------------------- | ---------------------------------------------------------------- |
| `nm-applet`            | Tray-based, scan/connect dropdown broken on Wayland              |
| `nm-connection-editor` | Only edits saved connections, no scanning                        |
| `nmtui`                | Terminal TUI, not a GUI                                          |
| `iwgtk`                | Requires iwd, most distros use NetworkManager                    |
| Rofi/wofi scripts      | No real UI — no signal bars, no live updates, no visual feedback |

**wifi-manager** fills this gap: a floating panel that scans, lists, and connects to WiFi networks with a proper GUI.

## Features

- 📡 **Scan & list** available WiFi networks with signal strength and security info
- 🔐 **Connect** to open, WPA2, and WPA3 networks
- 💾 **Saved networks** detected and reconnected without re-entering passwords
- 📶 **Signal strength** and frequency band (2.4 GHz / 5 GHz) display
- 🔌 **Disconnect** and WiFi radio toggle
- 🪟 **Floating overlay** via layer-shell — no window decorations, anchored to screen edge
- 🎨 **Custom CSS theming** — user styles via `~/.config/wifi-manager/style.css`
- 🔁 **Daemon mode** — runs in background, toggles visibility on keybind

## Requirements

- Linux with Wayland (Hyprland, Sway, or any wlroots-based compositor)
- [NetworkManager](https://networkmanager.dev/) running as the system network service
- GTK4 and gtk4-layer-shell development libraries
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
git clone https://github.com/youruser/wifi-manager.git
cd wifi-manager
cargo build --release
```

The binary will be at `./target/release/wifi-manager`.

## Usage

```sh
# Start the daemon (window hidden)
wifi-manager

# Toggle panel visibility (from keybind or terminal)
wifi-manager --toggle
```

### Hyprland Keybind

Add to `~/.config/hypr/hyprland.conf`:

```ini
bind = $mainMod, W, exec, wifi-manager --toggle
```

## Theming

wifi-manager ships with a dark default theme. To customize, create:

```
~/.config/wifi-manager/style.css
```

Your CSS overrides the default theme via cascade. Available class names:

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
├── main.rs               # Entry point, CLI, GTK application setup
├── app.rs                # Application controller (UI ↔ D-Bus bridge)
├── dbus/
│   ├── proxies.rs        # D-Bus proxy trait definitions (zbus)
│   ├── network_manager.rs # High-level WifiManager (scan, connect, disconnect)
│   ├── access_point.rs   # Data model (Network, SecurityType, Band)
│   └── connection.rs     # NM connection settings builders
└── ui/
    ├── window.rs         # Layer-shell floating window setup
    ├── header.rs         # Header bar (WiFi toggle, status, scan button)
    ├── network_list.rs   # Scrollable network list
    ├── network_row.rs    # Individual network row widget
    └── password_dialog.rs # Inline password entry
```

## Roadmap

- [x] NetworkManager D-Bus backend (scan, list, connect, disconnect)
- [x] GTK4 UI (network list, password dialog, connection feedback)
- [x] CSS theming with user overrides (`~/.config/wifi-manager/style.css`)
- [x] Daemon mode with D-Bus toggle
- [ ] D-Bus signal subscriptions for live updates
- [ ] Packaging (AUR, Fedora COPR)

## Tech Stack

- **Rust** — systems language, no runtime overhead
- **GTK4** — UI framework
- **gtk4-layer-shell** — Wayland overlay/popup support
- **zbus** — pure-Rust D-Bus client (communicates with NetworkManager)
- **clap** — CLI argument parsing

## License

MIT
