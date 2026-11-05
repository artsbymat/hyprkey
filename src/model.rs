use crate::keyboard::{Message, Modifiers};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

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
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            hide_ui: false,
            clear_delay: 2.2,
            max_keys: 4,
            show_labels: true,
            font_size: 34,
            hud_scale: 1.0,
            x: 100,
            y: 100,
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
