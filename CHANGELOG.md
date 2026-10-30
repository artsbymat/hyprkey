# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
