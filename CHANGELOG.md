# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-11-05

### Added
- **Full Custom Hex Colors**: Support custom color palette in `~/.config/hyprkey/settings.json` (`background`, `text`, `muted`, `active`, `inactive`, `accent`, `border`) supporting 6-digit `#RRGGBB` and 8-digit `#RRGGBBAA` (transparency) hex strings with aliases (`bg`, `fg`, `separator`).
- **Context-Aware Wayland Cursor**: Integrated `ThemedPointer` with dynamic cursor shapes (`Grab`, `Grabbing`, `Pointer`, `Default`) based on mouse state and hover targets, resolving cursor shape retention from previously focused windows.

### Changed
- **More Compact HUD**: Removed `'...'` button; settings menu is now toggled via right-click on the HUD.
- **Stable Layout Sizing**: Stabilized HUD width calculation (`capacity_width`) to eliminate jitter and fluctuating dimensions when displaying up to 8 history keys.

### Performance
- **Zero-Allocation Keystrokes**: Converted keystroke labels and symbols to borrowed `&'static str` references.
- **Direct Hyprland Socket IPC**: Replaced `hyprctl` process spawning with direct Unix domain socket communication (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`).
- **Optimized Software Rasterizer**: Added opaque pixel fast paths and bounding-box corner distance checks in `Canvas::rect`.

## [1.0.0] - 2026-10-30

### Added
- Native Wayland keystroke HUD for Hyprland using `wlr-layer-shell`.
- Lightweight CPU software rasterizer with bundled JetBrains Mono font subset.
- Global low-latency input tracking via Linux `evdev` without window focus requirements.
- Smart Caps Lock state synchronization with Hyprland (`hyprctl devices`) respecting custom XKB remaps.
- Consecutive keystroke counter (`×2`, `×3`, …) supporting hardware press and OS auto-repeat.
- Smooth mouse dragging to reposition using `zwp_relative_pointer_v1`.
- Built-in interactive settings menu:
  - HUD scaling (50% – 200%).
  - Configurable font size and history length.
  - Idle auto-hide and custom timeout.
  - Optional key descriptive labels.
- Persistent configuration stored in `~/.config/hyprkey/settings.json`.
- Non-intrusive click-through behavior outside the HUD area.
