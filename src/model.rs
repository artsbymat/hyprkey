use crate::keyboard::{Message, Modifiers};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ColorSettings {
    #[serde(alias = "bg")]
    pub background: String,
    #[serde(alias = "fg", alias = "foreground")]
    pub text: String,
    pub muted: String,
    pub active: String,
    pub inactive: String,
    pub accent: String,
    #[serde(alias = "separator")]
    pub border: String,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            background: "#101217f5".to_string(),
            text: "#eef2fa".to_string(),
            muted: "#9ba4b7".to_string(),
            active: "#364c5c".to_string(),
            inactive: "#202229".to_string(),
            accent: "#b8e1ff".to_string(),
            border: "#30333d".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedColors {
    pub background: [u8; 4],
    pub text: [u8; 4],
    pub muted: [u8; 4],
    pub active: [u8; 4],
    pub inactive: [u8; 4],
    pub accent: [u8; 4],
    pub border: [u8; 4],
}

impl Default for ResolvedColors {
    fn default() -> Self {
        Self {
            background: [16, 18, 23, 245],
            text: [238, 242, 250, 255],
            muted: [155, 164, 183, 255],
            active: [54, 76, 92, 255],
            inactive: [32, 34, 41, 255],
            accent: [184, 225, 255, 255],
            border: [48, 51, 61, 255],
        }
    }
}

pub fn parse_hex_color(s: &str, default: [u8; 4]) -> [u8; 4] {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    match s.len() {
        6 => {
            if let Ok(v) = u32::from_str_radix(s, 16) {
                [
                    ((v >> 16) & 0xff) as u8,
                    ((v >> 8) & 0xff) as u8,
                    (v & 0xff) as u8,
                    255,
                ]
            } else {
                default
            }
        }
        8 => {
            if let Ok(v) = u32::from_str_radix(s, 16) {
                [
                    ((v >> 24) & 0xff) as u8,
                    ((v >> 16) & 0xff) as u8,
                    ((v >> 8) & 0xff) as u8,
                    (v & 0xff) as u8,
                ]
            } else {
                default
            }
        }
        3 => {
            let mut buf = [0u8; 6];
            for (i, b) in s.bytes().enumerate() {
                buf[i * 2] = b;
                buf[i * 2 + 1] = b;
            }
            if let Ok(s6) = std::str::from_utf8(&buf) {
                if let Ok(v) = u32::from_str_radix(s6, 16) {
                    return [
                        ((v >> 16) & 0xff) as u8,
                        ((v >> 8) & 0xff) as u8,
                        (v & 0xff) as u8,
                        255,
                    ];
                }
            }
            default
        }
        4 => {
            let mut buf = [0u8; 8];
            for (i, b) in s.bytes().enumerate() {
                buf[i * 2] = b;
                buf[i * 2 + 1] = b;
            }
            if let Ok(s8) = std::str::from_utf8(&buf) {
                if let Ok(v) = u32::from_str_radix(s8, 16) {
                    return [
                        ((v >> 24) & 0xff) as u8,
                        ((v >> 16) & 0xff) as u8,
                        ((v >> 8) & 0xff) as u8,
                        (v & 0xff) as u8,
                    ];
                }
            }
            default
        }
        _ => default,
    }
}

impl ColorSettings {
    pub fn resolve(&self) -> ResolvedColors {
        let def = ResolvedColors::default();
        ResolvedColors {
            background: parse_hex_color(&self.background, def.background),
            text: parse_hex_color(&self.text, def.text),
            muted: parse_hex_color(&self.muted, def.muted),
            active: parse_hex_color(&self.active, def.active),
            inactive: parse_hex_color(&self.inactive, def.inactive),
            accent: parse_hex_color(&self.accent, def.accent),
            border: parse_hex_color(&self.border, def.border),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hide_ui: bool,
    pub clear_delay: f32,
    pub max_keys: usize,
    pub show_labels: bool,
    pub font_size: u16,
    pub hud_scale: f32,
    pub x: i32,
    pub y: i32,
    pub colors: ColorSettings,
    #[serde(skip)]
    pub resolved_colors: ResolvedColors,
}
impl Default for Settings {
    fn default() -> Self {
        let colors = ColorSettings::default();
        let resolved_colors = colors.resolve();
        Self {
            hide_ui: false,
            clear_delay: 2.2,
            max_keys: 4,
            show_labels: true,
            font_size: 34,
            hud_scale: 1.0,
            x: 100,
            y: 100,
            colors,
            resolved_colors,
        }
    }
}
impl Settings {
    pub fn normalize(&mut self) {
        self.max_keys = self.max_keys.clamp(1, 8);
        self.clear_delay = if self.clear_delay.is_finite() {
            self.clear_delay.clamp(0.5, 6.0)
        } else {
            2.2
        };
        self.font_size = self.font_size.clamp(24, 48);
        self.hud_scale = if self.hud_scale.is_finite() {
            self.hud_scale.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self.x = self.x.clamp(0, 8192);
        self.y = self.y.clamp(0, 8192);
        self.resolved_colors = self.colors.resolve();
    }
    fn path() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("hyprkey/settings.json"))
    }
    pub fn load() -> Self {
        let mut settings = Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|s| serde_json::from_slice::<Self>(&s).ok())
            .unwrap_or_default();
        settings.normalize();
        settings
    }
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::path().ok_or("Neither XDG_CONFIG_HOME nor HOME is set")?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct HistoryEntry {
    pub symbol: &'static str,
    pub label: &'static str,
    pub count: u32,
    pub modifiers: Modifiers,
}
impl HistoryEntry {
    pub fn new(symbol: &'static str, label: &'static str, modifiers: Modifiers) -> Self {
        Self {
            symbol,
            label,
            count: 1,
            modifiers,
        }
    }
    pub fn caption(&self, show_label: bool) -> Cow<'static, str> {
        let label = if show_label && self.symbol != self.label {
            self.label
        } else {
            ""
        };
        if self.count > 1 {
            if label.is_empty() {
                format!("×{}", self.count).into()
            } else {
                format!("{label}  ×{}", self.count).into()
            }
        } else {
            Cow::Borrowed(label)
        }
    }
}

pub struct Model {
    pub settings: Settings,
    pub history: VecDeque<HistoryEntry>,
    pub modifiers: Modifiers,
    pub last_press: Instant,
    pub idle: bool,
    pub show_settings: bool,
    pub keyboards: usize,
    pub save_error: bool,
}
impl Model {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            history: VecDeque::new(),
            modifiers: Modifiers::default(),
            last_press: Instant::now(),
            idle: false,
            show_settings: false,
            keyboards: 0,
            save_error: false,
        }
    }
    pub fn message(&mut self, message: Message) {
        match message {
            Message::Status(n) => self.keyboards = n,
            Message::Key(event) => {
                self.modifiers = event.modifiers;
                if event.pressed {
                    self.last_press = Instant::now();
                    self.idle = false;
                    if !event.is_modifier {
                        if let Some(last) = self.history.back_mut().filter(|last| {
                            last.symbol == event.symbol
                                && last.label == event.key
                                && last.modifiers == self.modifiers
                        }) {
                            last.count = last.count.saturating_add(1);
                        } else {
                            self.history.push_back(HistoryEntry::new(
                                event.symbol,
                                event.key,
                                self.modifiers.clone(),
                            ));
                        }
                    }
                }
            }
        }
        self.trim();
    }
    pub fn trim(&mut self) {
        while self.history.len() > self.settings.max_keys {
            self.history.pop_front();
        }
    }
    pub fn timeout(&self) -> Option<Duration> {
        (!self.idle).then(|| {
            Duration::from_secs_f32(self.settings.clear_delay)
                .saturating_sub(self.last_press.elapsed())
        })
    }
    pub fn expire(&mut self) -> bool {
        if self.timeout() == Some(Duration::ZERO) {
            self.idle = true;
            self.history.clear();
            return true;
        }
        false
    }
    pub fn hidden(&self) -> bool {
        self.idle && self.settings.hide_ui && !self.show_settings && self.keyboards > 0
    }
    pub fn save(&mut self) {
        self.save_error = match self.settings.save() {
            Ok(()) => false,
            Err(error) => {
                eprintln!("hyprkey: cannot save settings: {error}");
                true
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_settings_are_bounded() {
        let mut s = Settings {
            max_keys: 0,
            clear_delay: f32::NAN,
            font_size: 0,
            x: -4,
            ..Settings::default()
        };
        s.normalize();
        assert_eq!(
            (s.max_keys, s.clear_delay, s.font_size, s.x),
            (1, 2.2, 24, 0)
        );
    }
    #[test]
    fn idle_clears_history_and_settings_stay_reachable() {
        let mut m = Model::new(Settings {
            hide_ui: true,
            ..Settings::default()
        });
        m.keyboards = 1;
        m.history
            .push_back(HistoryEntry::new("a", "a", Modifiers::default()));
        m.last_press -= Duration::from_secs(7);
        assert!(m.expire());
        assert!(m.history.is_empty());
        assert!(m.hidden());
        assert_eq!(m.timeout(), None);
        m.show_settings = true;
        assert!(!m.hidden());
        m.show_settings = false;
        m.keyboards = 0;
        assert!(!m.hidden());
    }
}

#[cfg(test)]
mod repeat_tests {
    use super::*;
    use crate::keyboard::KeyEvent;
    fn key(symbol: &'static str, pressed: bool, ctrl: bool) -> Message {
        Message::Key(KeyEvent {
            key: symbol,
            symbol,
            pressed,
            is_modifier: false,
            modifiers: Modifiers {
                ctrl,
                ..Modifiers::default()
            },
        })
    }
    #[test]
    fn consecutive_presses_count_without_filling_history() {
        let mut m = Model::new(Settings::default());
        m.message(key("a", true, false));
        m.message(key("a", false, false));
        m.last_press -= Duration::from_secs(1);
        m.message(key("a", true, false));
        assert_eq!(m.history.len(), 1);
        assert_eq!(m.history[0].count, 2);
        assert_eq!(m.history[0].caption(false), "×2");
        assert!(m.last_press.elapsed() < Duration::from_secs(1));
        m.message(key("b", true, false));
        m.message(key("a", true, false));
        assert_eq!(m.history.len(), 3);
        assert_eq!(m.history[2].count, 1);
    }
    #[test]
    fn modifier_changes_and_idle_start_a_new_counter() {
        let mut m = Model::new(Settings::default());
        m.message(key("c", true, true));
        m.message(key("c", true, false));
        assert_eq!(m.history.len(), 2);
        m.last_press -= Duration::from_secs(7);
        assert!(m.expire());
        m.message(key("c", true, false));
        assert_eq!(m.history.len(), 1);
        assert_eq!(m.history[0].count, 1);
        assert!(!m.idle);
    }
    #[test]
    fn old_settings_default_to_unscaled_and_invalid_scale_is_sanitized() {
        let s: Settings = serde_json::from_str(r#"{"font_size":48}"#).unwrap();
        assert_eq!(s.hud_scale, 1.0);
        for (value, expected) in [(f32::NAN, 1.0), (0.0, 0.5), (10.0, 2.0)] {
            let mut s = Settings {
                hud_scale: value,
                ..Settings::default()
            };
            s.normalize();
            assert_eq!(s.hud_scale, expected);
        }
    }
}

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn color_settings_defaults_match_expected_theme() {
        let colors = ColorSettings::default().resolve();
        assert_eq!(colors.background, [16, 18, 23, 245]);
        assert_eq!(colors.text, [238, 242, 250, 255]);
        assert_eq!(colors.muted, [155, 164, 183, 255]);
        assert_eq!(colors.active, [54, 76, 92, 255]);
        assert_eq!(colors.inactive, [32, 34, 41, 255]);
        assert_eq!(colors.accent, [184, 225, 255, 255]);
        assert_eq!(colors.border, [48, 51, 61, 255]);
    }

    #[test]
    fn parse_hex_color_supports_rgb_rgba_and_lenient_input() {
        let fallback = [1, 2, 3, 4];
        // 6 hex with and without #
        assert_eq!(
            parse_hex_color("#3cc3b6", fallback),
            [0x3c, 0xc3, 0xb6, 255]
        );
        assert_eq!(parse_hex_color("3cc3b6", fallback), [0x3c, 0xc3, 0xb6, 255]);
        assert_eq!(
            parse_hex_color("  #3CC3B6  ", fallback),
            [0x3c, 0xc3, 0xb6, 255]
        );

        // 8 hex with and without #
        assert_eq!(parse_hex_color("#101217f5", fallback), [16, 18, 23, 245]);
        assert_eq!(parse_hex_color("101217f5", fallback), [16, 18, 23, 245]);

        // 3 hex #RGB
        assert_eq!(parse_hex_color("#f0a", fallback), [0xff, 0x00, 0xaa, 255]);

        // 4 hex #RGBA
        assert_eq!(parse_hex_color("#f0a8", fallback), [0xff, 0x00, 0xaa, 0x88]);

        // Invalid hex falls back to default
        assert_eq!(parse_hex_color("invalid", fallback), fallback);
        assert_eq!(parse_hex_color("#12345", fallback), fallback);
        assert_eq!(parse_hex_color("", fallback), fallback);
    }

    #[test]
    fn settings_deserializes_without_colors_and_with_custom_colors() {
        // Backward compatibility: old settings.json without colors
        let old_json = r#"{"font_size": 36, "hud_scale": 1.0}"#;
        let s: Settings = serde_json::from_str(old_json).unwrap();
        assert_eq!(s.colors, ColorSettings::default());
        assert_eq!(s.resolved_colors, ResolvedColors::default());

        // Custom colors with aliases
        let custom_json = r##"{
            "colors": {
                "bg": "#081b19f0",
                "fg": "#ebf9f8",
                "muted": "#309c91",
                "active": "#24756d",
                "inactive": "#0c2724",
                "accent": "#3cc3b6",
                "separator": "#184e49"
            }
        }"##;
        let mut s: Settings = serde_json::from_str(custom_json).unwrap();
        s.normalize();
        assert_eq!(s.resolved_colors.background, [0x08, 0x1b, 0x19, 0xf0]);
        assert_eq!(s.resolved_colors.text, [0xeb, 0xf9, 0xf8, 255]);
        assert_eq!(s.resolved_colors.muted, [0x30, 0x9c, 0x91, 255]);
        assert_eq!(s.resolved_colors.active, [0x24, 0x75, 0x6d, 255]);
        assert_eq!(s.resolved_colors.inactive, [0x0c, 0x27, 0x24, 255]);
        assert_eq!(s.resolved_colors.accent, [0x3c, 0xc3, 0xb6, 255]);
        assert_eq!(s.resolved_colors.border, [0x18, 0x4e, 0x49, 255]);
    }
}
