use evdev::{Device, EventType, Key};
use smithay_client_toolkit::reexports::calloop::channel::SyncSender;
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
    pub caps_lock: bool,
}

#[derive(Debug)]
pub struct KeyEvent {
    pub key: &'static str,
    pub symbol: &'static str,
    pub modifiers: Modifiers,
    pub pressed: bool,
    pub is_modifier: bool,
}

#[derive(Debug)]
pub enum Message {
    Key(KeyEvent),
    Status(usize),
}

#[derive(Default)]
struct KeyboardState {
    held: Vec<(usize, u16)>,
    caps_lock: bool,
    // evdev only reports the physical Caps Lock code; whether a press actually
    // enters a lock depends on the compositor's keymap. Hyprland exposes the
    // result (capsLock in `hyprctl devices`), so hyprkey treats that as the
    // truth and only uses evdev for latency. None means Hyprland was not
    // reachable, in which case a press is assumed to be a real lock toggle.
    caps_remapped: bool,
    // Caps Lock presses whose lock effect is not yet confirmed. Hyprland writes
    // capsLock before the release event, so a press is settled on its release.
    unconfirmed_caps_press: bool,
}

impl KeyboardState {
    fn event(&mut self, device: usize, key: Key, value: i32) -> Option<KeyEvent> {
        let code = key.code();
        if value == 0 {
            if let Some(pos) = self
                .held
                .iter()
                .position(|&(d, c)| d == device && c == code)
            {
                self.held.swap_remove(pos);
            }
        } else if value == 1 {
            if !self.held.iter().any(|&(d, c)| d == device && c == code) {
                self.held.push((device, code));
            }
        } else if value != 2 {
            return None;
        }
        let (name, symbol, is_modifier) = key_to_info(key)?;
        // Repeat is a key-down without another lock toggle or modifier change.
        if value == 2 && (is_modifier || key == Key::KEY_CAPSLOCK) {
            return None;
        }
        if key == Key::KEY_CAPSLOCK {
            if value == 1 {
                if !self.caps_remapped {
                    // Real lock key: reflect the toggle immediately so the HUD
                    // does not wait for the next Hyprland poll.
                    self.caps_lock = !self.caps_lock;
                }
                self.unconfirmed_caps_press = true;
            } else {
                self.unconfirmed_caps_press = false;
            }
        }
        let down = |a: Key, b: Key| {
            let ac = a.code();
            let bc = b.code();
            self.held.iter().any(|&(_, k)| k == ac || k == bc)
        };
        let modifiers = Modifiers {
            shift: down(Key::KEY_LEFTSHIFT, Key::KEY_RIGHTSHIFT),
            ctrl: down(Key::KEY_LEFTCTRL, Key::KEY_RIGHTCTRL),
            alt: down(Key::KEY_LEFTALT, Key::KEY_RIGHTALT),
            meta: down(Key::KEY_LEFTMETA, Key::KEY_RIGHTMETA),
            caps_lock: self.caps_lock,
        };
        let symbol: &'static str = if name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase() {
            if modifiers.shift ^ modifiers.caps_lock {
                const UPPER: [&str; 26] = [
                    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
                    "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
                ];
                UPPER[(name.as_bytes()[0] - b'a') as usize]
            } else {
                symbol
            }
        } else if modifiers.shift {
            match name {
                "1" => "!",
                "2" => "@",
                "3" => "#",
                "4" => "$",
                "5" => "%",
                "6" => "^",
                "7" => "&",
                "8" => "*",
                "9" => "(",
                "0" => ")",
                "-" => "_",
                "=" => "+",
                "[" => "{",
                "]" => "}",
                ";" => ":",
                "'" => "\"",
                "," => "<",
                "." => ">",
                "/" => "?",
                "`" => "~",
                "\\" => "|",
                _ => symbol,
            }
        } else {
            symbol
        };
        Some(KeyEvent {
            key: name,
            symbol,
            modifiers,
            pressed: value != 0,
            is_modifier,
        })
    }

    // Adopt Hyprland's own view of Caps Lock. This is the only reliable source:
    // a remapped key can still leave the lock on (set by another client or a
    // virtual keyboard), so hyprkey must never assume the lock is off.
    fn sync_with_hyprland(&mut self, report: &HyprlandKeyboardReport) {
        self.caps_lock = report.caps_lock;
        self.caps_remapped = report.caps_remapped;
        self.unconfirmed_caps_press = false;
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct HyprlandKeyboardReport {
    caps_lock: bool,
    caps_remapped: bool,
}

#[derive(serde::Deserialize)]
struct DevicesOutput<'a> {
    #[serde(borrow)]
    keyboards: Vec<KeyboardNode<'a>>,
}

#[derive(serde::Deserialize)]
struct KeyboardNode<'a> {
    #[serde(default)]
    main: bool,
    #[serde(rename = "capsLock", default)]
    caps_lock: bool,
    #[serde(borrow, default)]
    options: &'a str,
}

// Ask Hyprland for the effective lock state and keymap options. Only the main
// keyboard carries meaningful lock state; the auxiliary nodes Hyprland reports
// (consumer-control, power-button, ...) must not be consulted.
fn query_hyprland() -> Option<HyprlandKeyboardReport> {
    // Fast path: query via Hyprland's UNIX domain socket (zero subprocess forks).
    if let (Ok(runtime), Ok(instance)) = (
        std::env::var("XDG_RUNTIME_DIR"),
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE"),
    ) {
        use std::io::{Read, Write};
        let socket_path = format!("{runtime}/hypr/{instance}/.socket.sock");
        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket_path) {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
            if stream.write_all(b"j/devices").is_ok() {
                let mut buf = Vec::with_capacity(4096);
                if stream.read_to_end(&mut buf).is_ok() {
                    if let Some(report) = parse_devices(&buf) {
                        return Some(report);
                    }
                }
            }
        }
    }
    // Fallback: spawn hyprctl if socket path is unavailable
    let output = std::process::Command::new("hyprctl")
        .args(["-j", "devices"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_devices(&output.stdout)
}

fn parse_devices(json: &[u8]) -> Option<HyprlandKeyboardReport> {
    let output: DevicesOutput = serde_json::from_slice(json).ok()?;
    let main = output.keyboards.into_iter().find(|k| k.main)?;
    Some(HyprlandKeyboardReport {
        caps_lock: main.caps_lock,
        caps_remapped: has_caps_remap(main.options),
    })
}

// Every caps:* XKB option except the identity mapping takes Caps Lock away from
// the lock key, so a press may not latch it.
fn has_caps_remap(options: &str) -> bool {
    options
        .split(',')
        .map(str::trim)
        .any(|option| option.starts_with("caps:") && option != "caps:capslock")
}

// One polling worker owns all devices and modifier state. No grabs: other apps
// continue receiving input. Rescan periodically for hotplug and permission changes.
pub fn start(tx: SyncSender<Message>) {
    std::thread::spawn(move || {
        use std::os::fd::AsRawFd;
        let mut devices: Vec<(std::path::PathBuf, Device, usize)> = Vec::new();
        let mut state = KeyboardState::default();
        let mut next_id = 0;
        let mut scan = std::time::Instant::now() - Duration::from_secs(2);
        let mut last_count = None;
        if let Some(report) = query_hyprland() {
            state.sync_with_hyprland(&report);
        }
        let fast_poll = Duration::from_millis(50);
        let normal_poll = Duration::from_millis(1500);
        let mut last_query = Some(std::time::Instant::now());
        loop {
            let poll_tick = if state.unconfirmed_caps_press {
                fast_poll
            } else {
                normal_poll
            };
            if last_query.is_none_or(|t| t.elapsed() >= poll_tick) {
                if let Some(report) = query_hyprland() {
                    state.sync_with_hyprland(&report);
                }
                last_query = Some(std::time::Instant::now());
            }
            if scan.elapsed() >= Duration::from_secs(2) {
                for (path, device) in evdev::enumerate() {
                    if devices.iter().any(|(p, _, _)| *p == path) {
                        continue;
                    }
                    if !device
                        .supported_keys()
                        .is_some_and(|k| k.contains(Key::KEY_A) && k.contains(Key::KEY_SPACE))
                    {
                        continue;
                    }
                    // O_NONBLOCK via the safe nix API used by evdev's generation.
                    if nix::fcntl::fcntl(
                        device.as_raw_fd(),
                        nix::fcntl::FcntlArg::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK),
                    )
                    .is_err()
                    {
                        continue;
                    }
                    if let Ok(keys) = device.get_key_state() {
                        for key in keys.iter() {
                            state.held.push((next_id, key.code()));
                        }
                    }
                    if devices.is_empty() {
                        // Seed the lock state from the LED. Hyprland owns the
                        // LED, so this reflects the real compositor state.
                        if let Ok(leds) = device.get_led_state() {
                            state.caps_lock = leds.contains(evdev::LedType::LED_CAPSL);
                        }
                    }
                    devices.push((path, device, next_id));
                    next_id += 1;
                }
                scan = std::time::Instant::now();
            }
            if last_count != Some(devices.len()) {
                if tx.send(Message::Status(devices.len())).is_err() {
                    return;
                }
                last_count = Some(devices.len());
            }
            devices.retain_mut(|(_, device, id)| match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        if event.event_type() == EventType::KEY {
                            if let Some(event) =
                                state.event(*id, Key::new(event.code()), event.value())
                            {
                                if tx.send(Message::Key(event)).is_err() {
                                    return false;
                                }
                            }
                        }
                    }
                    true
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => true,
                Err(_) => {
                    state.held.retain(|(device, _)| *device != *id);
                    if let Some(event) = state.event(*id, Key::KEY_LEFTSHIFT, 0) {
                        let _ = tx.send(Message::Key(event));
                    }
                    false
                }
            });
            // Sleep in poll until input arrives, the Hyprland lock-state check
            // is due, or the hotplug rescan is due.
            let rescan = Duration::from_secs(2).saturating_sub(scan.elapsed());
            let query =
                last_query.map_or(Duration::ZERO, |t| poll_tick.saturating_sub(t.elapsed()));
            let timeout = rescan.min(query);
            let mut fds: Vec<_> = devices
                .iter()
                .map(|(_, device, _)| {
                    nix::poll::PollFd::new(device.as_raw_fd(), nix::poll::PollFlags::POLLIN)
                })
                .collect();
            if let Err(error) = nix::poll::poll(&mut fds, timeout.as_millis().min(2000) as i32) {
                if error != nix::errno::Errno::EINTR {
                    eprintln!("hyprkey: input poll failed: {error}");
                    return;
                }
            }
        }
    });
}

fn key_to_info(key: Key) -> Option<(&'static str, &'static str, bool)> {
    // (label, display symbol, is modifier)
    let info = match key {
        // Modifiers
        Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => ("Shift", "⇧", true),
        Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => ("Ctrl", "⌃", true),
        Key::KEY_LEFTALT | Key::KEY_RIGHTALT => ("Alt", "⌥", true),
        Key::KEY_LEFTMETA | Key::KEY_RIGHTMETA => ("Super", "⌘", true),

        // Letters; case is applied from Shift and Caps Lock.
        Key::KEY_A => ("a", "a", false),
        Key::KEY_B => ("b", "b", false),
        Key::KEY_C => ("c", "c", false),
        Key::KEY_D => ("d", "d", false),
        Key::KEY_E => ("e", "e", false),
        Key::KEY_F => ("f", "f", false),
        Key::KEY_G => ("g", "g", false),
        Key::KEY_H => ("h", "h", false),
        Key::KEY_I => ("i", "i", false),
        Key::KEY_J => ("j", "j", false),
        Key::KEY_K => ("k", "k", false),
        Key::KEY_L => ("l", "l", false),
        Key::KEY_M => ("m", "m", false),
        Key::KEY_N => ("n", "n", false),
        Key::KEY_O => ("o", "o", false),
        Key::KEY_P => ("p", "p", false),
        Key::KEY_Q => ("q", "q", false),
        Key::KEY_R => ("r", "r", false),
        Key::KEY_S => ("s", "s", false),
        Key::KEY_T => ("t", "t", false),
        Key::KEY_U => ("u", "u", false),
        Key::KEY_V => ("v", "v", false),
        Key::KEY_W => ("w", "w", false),
        Key::KEY_X => ("x", "x", false),
        Key::KEY_Y => ("y", "y", false),
        Key::KEY_Z => ("z", "z", false),

        // Numbers
        Key::KEY_0 => ("0", "0", false),
        Key::KEY_1 => ("1", "1", false),
        Key::KEY_2 => ("2", "2", false),
        Key::KEY_3 => ("3", "3", false),
        Key::KEY_4 => ("4", "4", false),
        Key::KEY_5 => ("5", "5", false),
        Key::KEY_6 => ("6", "6", false),
        Key::KEY_7 => ("7", "7", false),
        Key::KEY_8 => ("8", "8", false),
        Key::KEY_9 => ("9", "9", false),

        // Special keys
        Key::KEY_SPACE => ("Space", "␣", false),
        Key::KEY_ENTER => ("Enter", "⏎", false),
        Key::KEY_TAB => ("Tab", "⇥", false),
        Key::KEY_BACKSPACE => ("Backspace", "⌫", false),
        Key::KEY_ESC => ("Esc", "⎋", false),
        Key::KEY_DELETE => ("Delete", "⌦", false),
        Key::KEY_HOME => ("Home", "↖", false),
        Key::KEY_END => ("End", "↘", false),
        Key::KEY_PAGEUP => ("Page Up", "⇞", false),
        Key::KEY_PAGEDOWN => ("Page Down", "⇟", false),
        Key::KEY_UP => ("↑", "↑", false),
        Key::KEY_DOWN => ("↓", "↓", false),
        Key::KEY_LEFT => ("←", "←", false),
        Key::KEY_RIGHT => ("→", "→", false),

        // Function keys
        Key::KEY_F1 => ("F1", "F1", false),
        Key::KEY_F2 => ("F2", "F2", false),
        Key::KEY_F3 => ("F3", "F3", false),
        Key::KEY_F4 => ("F4", "F4", false),
        Key::KEY_F5 => ("F5", "F5", false),
        Key::KEY_F6 => ("F6", "F6", false),
        Key::KEY_F7 => ("F7", "F7", false),
        Key::KEY_F8 => ("F8", "F8", false),
        Key::KEY_F9 => ("F9", "F9", false),
        Key::KEY_F10 => ("F10", "F10", false),
        Key::KEY_F11 => ("F11", "F11", false),
        Key::KEY_F12 => ("F12", "F12", false),

        // Punctuation
        Key::KEY_MINUS => ("-", "−", false),
        Key::KEY_EQUAL => ("=", "=", false),
        Key::KEY_LEFTBRACE => ("[", "[", false),
        Key::KEY_RIGHTBRACE => ("]", "]", false),
        Key::KEY_BACKSLASH => ("\\", "\\", false),
        Key::KEY_SEMICOLON => (";", ";", false),
        Key::KEY_APOSTROPHE => ("'", "'", false),
        Key::KEY_GRAVE => ("`", "`", false),
        Key::KEY_COMMA => (",", ",", false),
        Key::KEY_DOT => (".", ".", false),
        Key::KEY_SLASH => ("/", "/", false),
        Key::KEY_CAPSLOCK => ("Caps Lock", "⇪", false),

        _ => return None,
    };
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn releasing_one_shift_preserves_other_shift() {
        let mut s = KeyboardState::default();
        s.event(0, Key::KEY_LEFTSHIFT, 1);
        s.event(0, Key::KEY_RIGHTSHIFT, 1);
        assert!(s.event(0, Key::KEY_LEFTSHIFT, 0).unwrap().modifiers.shift);
        assert!(!s.event(0, Key::KEY_RIGHTSHIFT, 0).unwrap().modifiers.shift);
    }
    #[test]
    fn modifiers_are_aggregated_across_keyboards() {
        let mut s = KeyboardState::default();
        s.event(0, Key::KEY_LEFTCTRL, 1);
        s.event(1, Key::KEY_LEFTCTRL, 1);
        assert!(s.event(0, Key::KEY_LEFTCTRL, 0).unwrap().modifiers.ctrl);
    }
    #[test]
    fn caps_shift_and_punctuation() {
        let mut s = KeyboardState::default();
        s.event(0, Key::KEY_CAPSLOCK, 1);
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, "A");
        s.event(0, Key::KEY_LEFTSHIFT, 1);
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, "a");
        assert_eq!(s.event(0, Key::KEY_MINUS, 1).unwrap().symbol, "_");
        assert_eq!(s.event(0, Key::KEY_1, 1).unwrap().symbol, "!");
        assert!(s.event(0, Key::KEY_CAPSLOCK, 2).is_none());
        assert!(s.caps_lock);
    }
    #[test]
    fn release_updates_state_without_a_new_keystroke() {
        let mut s = KeyboardState::default();
        s.event(0, Key::KEY_LEFTALT, 1);
        let event = s.event(0, Key::KEY_LEFTALT, 0).unwrap();
        assert!(!event.pressed);
        assert!(!event.modifiers.alt);
        assert!(event.is_modifier);
    }
}

#[cfg(test)]
mod repeat_tests {
    use super::*;
    #[test]
    fn evdev_hold_repeat_is_delivered_without_toggling_locks() {
        let mut state = KeyboardState::default();
        state.event(0, Key::KEY_CAPSLOCK, 1);
        state.event(0, Key::KEY_A, 1);
        let repeated = state.event(0, Key::KEY_A, 2).unwrap();
        assert!(repeated.pressed);
        assert_eq!(repeated.symbol, "A");
        assert!(state.event(0, Key::KEY_CAPSLOCK, 2).is_none());
        assert!(state.caps_lock);
        assert!(!state.event(0, Key::KEY_A, 0).unwrap().pressed);
    }
}

#[cfg(test)]
mod remap_tests {
    use super::*;

    fn remapped() -> KeyboardState {
        KeyboardState {
            caps_remapped: true,
            ..KeyboardState::default()
        }
    }

    // A real capture from `hyprctl -j devices` on a machine using
    // kb_options = caps:escape_shifted_capslock, trimmed to the fields used.
    fn devices_json(caps: bool, options: &str) -> Vec<u8> {
        format!(
            r#"{{"keyboards":[
                {{"name":"semico---consumer-control","options":"{options}","capsLock":false,"main":false}},
                {{"name":"semico---usb-gaming-keyboard-","options":"{options}","capsLock":{caps},"main":true}},
                {{"name":"power-button","options":"{options}","capsLock":false,"main":false}}
            ]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn caps_as_escape_remap_does_not_forge_uppercase() {
        let mut s = remapped();
        let esc = s.event(0, Key::KEY_CAPSLOCK, 1).unwrap();
        assert_eq!(esc.key, "Caps Lock");
        assert!(!esc.modifiers.caps_lock, "remap must not latch caps");
        let a = s.event(0, Key::KEY_A, 1).unwrap();
        assert_eq!(a.symbol, "a", "caps remap must not uppercase letters");
    }

    #[test]
    fn lock_set_elsewhere_is_still_honoured_under_a_remap() {
        // The reported regression: caps:escape_shifted_capslock is active but the
        // lock is genuinely on, so letters must be drawn uppercase.
        let mut s = remapped();
        let report = parse_devices(&devices_json(true, "caps:escape_shifted_capslock")).unwrap();
        s.sync_with_hyprland(&report);
        assert!(s.caps_remapped);
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, "A");
        assert!(s.event(0, Key::KEY_EQUAL, 1).unwrap().symbol == "=");
    }

    #[test]
    fn remap_follows_hyprland_when_configured() {
        let report = parse_devices(&devices_json(false, "caps:escape_shifted_capslock")).unwrap();
        let mut s = KeyboardState::default();
        s.sync_with_hyprland(&report);
        assert!(s.caps_remapped && !s.caps_lock);
        s.event(0, Key::KEY_CAPSLOCK, 1);
        s.event(0, Key::KEY_CAPSLOCK, 0);
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, "a");
        assert_eq!(s.event(0, Key::KEY_EQUAL, 1).unwrap().symbol, "=");
    }

    #[test]
    fn plain_caps_lock_still_toggles_and_uppercases() {
        let report = parse_devices(&devices_json(false, "")).unwrap();
        let mut s = KeyboardState::default();
        s.sync_with_hyprland(&report);
        assert!(!s.caps_remapped);
        s.event(0, Key::KEY_CAPSLOCK, 1);
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, "A");
        // Hyprland agrees once the lock is on.
        s.sync_with_hyprland(&parse_devices(&devices_json(true, "")).unwrap());
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, "A");
        // And a config reload that adds the remap is picked up.
        s.sync_with_hyprland(
            &parse_devices(&devices_json(true, "caps:escape_shifted_capslock")).unwrap(),
        );
        assert!(s.caps_remapped && s.caps_lock);
    }

    #[test]
    fn main_keyboard_wins_over_auxiliary_nodes() {
        // Only the main node reports the lock; the others always say no.
        let report = parse_devices(&devices_json(true, "caps:escape")).unwrap();
        assert!(report.caps_lock);
    }

    #[test]
    fn no_main_keyboard_reports_unknown_instead_of_guessing() {
        // Auxiliary-only lists must not be mistaken for a keyboard: their
        // capsLock is always false and would silently force lowercase.
        let json = br#"{"keyboards":[
            {"name":"consumer-control","options":"","capsLock":false,"main":false},
            {"name":"power-button","options":"","capsLock":false,"main":false}
        ]}"#;
        assert!(parse_devices(json).is_none());
    }

    #[test]
    fn unparsable_devices_output_keeps_previous_state() {
        let mut s = KeyboardState {
            caps_lock: true,
            ..KeyboardState::default()
        };
        let before = s.event(0, Key::KEY_A, 1).unwrap().symbol;
        assert!(parse_devices(b"not json").is_none());
        assert!(parse_devices(br#"{"keyboards":[]}"#).is_none());
        assert_eq!(s.event(0, Key::KEY_A, 1).unwrap().symbol, before);
    }

    #[test]
    fn kb_options_recognises_caps_remaps() {
        assert!(has_caps_remap("caps:escape_shifted_capslock"));
        assert!(has_caps_remap("ctrl:nocaps,caps:escape"));
        assert!(has_caps_remap("caps:ctrl_modifier"));
        assert!(!has_caps_remap(""));
        assert!(!has_caps_remap("grp:alt_shift_toggle"));
        assert!(!has_caps_remap("caps:capslock"));
        assert!(!has_caps_remap("compose:caps"));
    }
}
