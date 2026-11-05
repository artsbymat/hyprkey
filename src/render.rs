use crate::model::{HistoryEntry, Model};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use std::collections::HashMap;

pub const HEIGHT: u32 = 142;
pub const SETTINGS_HEIGHT: u32 = 442;
#[derive(Clone, Copy, Debug)]
pub enum Action {
    Settings,
    Hide,
    Labels,
    Delay(i8),
    Count(i8),
    Font(i8),
    Scale(i8),
    Quit,
}
#[derive(Clone, Copy)]
pub struct Hit {
    pub rect: [f32; 4],
    pub action: Action,
}
impl Hit {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let [left, top, w, h] = self.rect;
        x >= left && x < left + w && y >= top && y < top + h
    }
}
struct Glyph {
    x: i32,
    y: i32,
    width: usize,
    coverage: Vec<u8>,
}
pub struct Renderer {
    font: FontArc,
    glyphs: HashMap<(char, u16), Glyph>,
    raster_scale: f32,
}
impl Renderer {
    pub fn new() -> Self {
        Self {
            font: FontArc::try_from_slice(include_bytes!("../assets/fonts/HyprkeyMono.ttf"))
                .expect("bundled monospace font"),
            glyphs: HashMap::new(),
            raster_scale: 0.0,
        }
    }
    pub fn width(&self, text: &str, size: f32) -> f32 {
        let font = self.font.as_scaled(size);
        text.chars()
            .map(|ch| font.h_advance(font.glyph_id(ch)))
            .sum()
    }
    // ab_glyph's scale is line height, not em size. Convert CSS-like font pixels
    // to its scale so 34 px has the intended readable cap height.
    fn scale(&self, size: f32) -> f32 {
        self.font.pt_to_px_scale(size * 0.75).unwrap().y
    }
    fn text_width(&self, text: &str, size: f32) -> f32 {
        self.width(text, self.scale(size))
    }
    fn cell_width(&self, entry: &HistoryEntry, latest: bool, m: &Model) -> f32 {
        let size = if latest {
            m.settings.font_size as f32
        } else {
            m.settings.font_size as f32 * 0.72
        };
        let caption = entry.caption(latest && m.settings.show_labels);
        self.text_width(&entry.symbol, size)
            .max(self.text_width(&caption, 12.0))
            .max(24.0)
            + 18.0
    }
    pub fn dimensions(&self, m: &Model) -> (u32, u32) {
        let total: f32 = m
            .history
            .iter()
            .enumerate()
            .map(|(i, entry)| self.cell_width(entry, i + 1 == m.history.len(), m))
            .sum();
        let width = (total.ceil() + 32.0).max(280.0);
        let height = if m.show_settings {
            SETTINGS_HEIGHT
        } else {
            HEIGHT
        } as f32;
        (
            (width * m.settings.hud_scale).ceil() as u32,
            (height * m.settings.hud_scale).ceil() as u32,
        )
    }
    fn text(
        &mut self,
        canvas: &mut Canvas<'_>,
        text: &str,
        size: f32,
        x: f32,
        baseline: f32,
        color: [u8; 4],
    ) {
        let px = self.scale(size * canvas.scale);
        let mut x = x * canvas.scale;
        for ch in text.chars() {
            let id = self.font.glyph_id(ch);
            let advance = self.font.as_scaled(px).h_advance(id);
            let glyph = self
                .glyphs
                .entry((ch, (px * 16.0).round() as u16))
                .or_insert_with(|| {
                    let glyph = id.with_scale_and_position(PxScale::from(px), point(0.0, 0.0));
                    if let Some(outline) = self.font.outline_glyph(glyph) {
                        let bounds = outline.px_bounds();
                        let width = bounds.width() as usize;
                        let mut coverage = vec![0; width * bounds.height() as usize];
                        outline.draw(|x, y, alpha| {
                            coverage[y as usize * width + x as usize] = (alpha * 255.0) as u8
                        });
                        Glyph {
                            x: bounds.min.x as i32,
                            y: bounds.min.y as i32,
                            width,
                            coverage,
                        }
                    } else {
                        Glyph {
                            x: 0,
                            y: 0,
                            width: 0,
                            coverage: vec![],
                        }
                    }
                });
            for (index, alpha) in glyph.coverage.iter().enumerate() {
                let mut color = color;
                color[3] = (u16::from(color[3]) * u16::from(*alpha) / 255) as u8;
                canvas.pixel(
                    x.round() as i32 + glyph.x + (index % glyph.width) as i32,
                    (baseline * canvas.scale).round() as i32
                        + glyph.y
                        + (index / glyph.width) as i32,
                    color,
                );
            }
            x += advance;
        }
    }
    fn centered(
        &mut self,
        c: &mut Canvas<'_>,
        text: &str,
        size: f32,
        center: f32,
        baseline: f32,
        color: [u8; 4],
    ) {
        self.text(
            c,
            text,
            size,
            center - self.text_width(text, size) / 2.0,
            baseline,
            color,
        );
    }
    fn button(
        &mut self,
        c: &mut Canvas<'_>,
        hits: &mut Vec<Hit>,
        rect: [f32; 4],
        label: &str,
        active: bool,
        action: Action,
    ) {
        c.rect(
            rect,
            7.0,
            if active {
                [54, 76, 92, 255]
            } else {
                [32, 34, 41, 255]
            },
        );
        self.centered(
            c,
            label,
            13.0,
            rect[0] + rect[2] / 2.0,
            rect[1] + 20.0,
            if active {
                [184, 225, 255, 255]
            } else {
                [181, 186, 199, 255]
            },
        );
        hits.push(Hit { rect, action });
    }
    pub fn draw(
        &mut self,
        bytes: &mut [u8],
        width: u32,
        height: u32,
        scale: u32,
        m: &Model,
    ) -> Vec<Hit> {
        bytes.fill(0);
        let mut hits = Vec::new();
        if m.hidden() {
            return hits;
        }
        let ui_scale = m.settings.hud_scale;
        let raster_scale = scale as f32 * ui_scale;
        if self.raster_scale != raster_scale {
            self.glyphs.clear();
            self.raster_scale = raster_scale;
        }
        let mut c = Canvas {
            bytes,
            width: width * scale,
            height: height * scale,
            scale: raster_scale,
        };
        let width = width as f32 / ui_scale;
        c.rect(
            [0.0, 0.0, width, height as f32 / ui_scale],
            16.0,
            [16, 18, 23, 245],
        );
        let white = [238, 242, 250, 255];
        let muted = [155, 164, 183, 255];
        if m.keyboards == 0 {
            self.centered(&mut c, "No keyboard access", 16.0, width / 2.0, 43.0, white);
            self.centered(
                &mut c,
                "Right-click for settings",
                12.0,
                width / 2.0,
                69.0,
                muted,
            );
        } else if m.history.is_empty() {
            self.centered(&mut c, "hyprkey", 24.0, width / 2.0, 49.0, muted);
            self.centered(&mut c, "waiting for keys", 12.0, width / 2.0, 74.0, muted);
        } else {
            let widths: Vec<_> = m
                .history
                .iter()
                .enumerate()
                .map(|(i, entry)| self.cell_width(entry, i + 1 == m.history.len(), m))
                .collect();
            let mut x = (width - widths.iter().sum::<f32>()) / 2.0;
            for (i, (entry, cell)) in m.history.iter().zip(widths).enumerate() {
                let latest = i + 1 == m.history.len();
                let size = m.settings.font_size as f32 * if latest { 1.0 } else { 0.72 };
                self.centered(
                    &mut c,
                    &entry.symbol,
                    size,
                    x + cell / 2.0,
                    59.0,
                    if latest { white } else { muted },
                );
                let caption = entry.caption(latest && m.settings.show_labels);
                if !caption.is_empty() {
                    self.centered(
                        &mut c,
                        &caption,
                        12.0,
                        x + cell / 2.0,
                        82.0,
                        if entry.count > 1 {
                            [184, 225, 255, 255]
                        } else {
                            muted
                        },
                    );
                }
                x += cell;
            }
        }
        c.rect([12.0, 94.0, width - 24.0, 1.0], 0.0, [48, 51, 61, 255]);
        let cell = (width - 24.0 - 18.0) / 4.0;
        for (i, (label, active)) in [
            ("⇧", m.modifiers.shift),
            ("⌃", m.modifiers.ctrl),
            ("⌥", m.modifiers.alt),
            ("⌘", m.modifiers.meta),
        ]
        .iter()
        .enumerate()
        {
            let rect = [12.0 + i as f32 * (cell + 6.0), 103.0, cell, 28.0];
            c.rect(
                rect,
                7.0,
                if *active {
                    [54, 76, 92, 255]
                } else {
                    [32, 34, 41, 255]
                },
            );
            self.centered(
                &mut c,
                label,
                22.0,
                rect[0] + cell / 2.0,
                124.0,
                if *active { white } else { muted },
            );
        }
        if m.show_settings {
            c.rect([12.0, 144.0, width - 24.0, 1.0], 0.0, [48, 51, 61, 255]);
            for (row, label, active, action) in [
                (0, "Hide when idle", m.settings.hide_ui, Action::Hide),
                (1, "Show labels", m.settings.show_labels, Action::Labels),
            ] {
                let y = 153.0 + row as f32 * 32.0;
                self.text(&mut c, label, 14.0, 16.0, y + 20.0, white);
                self.button(
                    &mut c,
                    &mut hits,
                    [width - 76.0, y, 60.0, 27.0],
                    if active { "ON" } else { "OFF" },
                    active,
                    action,
                );
            }
            for (row, label, minus, plus) in [
                (
                    2,
                    format!("Clear after {:.1}s", m.settings.clear_delay),
                    Action::Delay(-1),
                    Action::Delay(1),
                ),
                (
                    3,
                    format!("History: {} keys", m.settings.max_keys),
                    Action::Count(-1),
                    Action::Count(1),
                ),
                (
                    4,
                    format!("Text size: {}px", m.settings.font_size),
                    Action::Font(-1),
                    Action::Font(1),
                ),
                (
                    5,
                    format!("HUD scale: {:.0}%", m.settings.hud_scale * 100.0),
                    Action::Scale(-1),
                    Action::Scale(1),
                ),
            ] {
                let y = 153.0 + row as f32 * 32.0;
                self.text(&mut c, &label, 14.0, 16.0, y + 20.0, white);
                self.button(
                    &mut c,
                    &mut hits,
                    [width - 82.0, y, 30.0, 27.0],
                    "-",
                    false,
                    minus,
                );
                self.button(
                    &mut c,
                    &mut hits,
                    [width - 46.0, y, 30.0, 27.0],
                    "+",
                    false,
                    plus,
                );
            }
            let status = if m.save_error {
                "Settings save failed; see terminal"
            } else if m.keyboards == 0 {
                "Grant /dev/input access (README)"
            } else {
                "JetBrains Mono / Wayland"
            };
            self.centered(&mut c, status, 12.0, width / 2.0, 364.0, muted);
            self.centered(
                &mut c,
                "Drag the key area to reposition",
                12.0,
                width / 2.0,
                384.0,
                muted,
            );
            self.button(
                &mut c,
                &mut hits,
                [16.0, 400.0, width - 32.0, 28.0],
                "Quit",
                false,
                Action::Quit,
            );
        }
        for hit in &mut hits {
            hit.rect = hit.rect.map(|v| v * ui_scale);
        }
        hits
    }
}

struct Canvas<'a> {
    bytes: &'a mut [u8],
    width: u32,
    height: u32,
    scale: f32,
}
impl Canvas<'_> {
    fn pixel(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let index = ((y as u32 * self.width + x as u32) * 4) as usize;
        let dst = u32::from_ne_bytes(self.bytes[index..index + 4].try_into().unwrap());
        let a = u32::from(color[3]);
        let blend = |src: u8, shift: u32| {
            u32::from(src) * a / 255 + ((dst >> shift) & 255) * (255 - a) / 255
        };
        let pixel = ((a + (dst >> 24) * (255 - a) / 255) << 24)
            | (blend(color[0], 16) << 16)
            | (blend(color[1], 8) << 8)
            | blend(color[2], 0);
        self.bytes[index..index + 4].copy_from_slice(&pixel.to_ne_bytes());
    }
    fn rect(&mut self, rect: [f32; 4], radius: f32, color: [u8; 4]) {
        let [x, y, w, h] = rect.map(|v| v * self.scale);
        let r = radius * self.scale;
        for py in y.floor() as i32..(y + h).ceil() as i32 {
            for px in x.floor() as i32..(x + w).ceil() as i32 {
                let dx = (x + r - (px as f32 + 0.5))
                    .max((px as f32 + 0.5) - (x + w - r))
                    .max(0.0);
                let dy = (y + r - (py as f32 + 0.5))
                    .max((py as f32 + 0.5) - (y + h - r))
                    .max(0.0);
                let coverage = if r == 0.0 {
                    1.0
                } else {
                    (r + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0)
                };
                let mut c = color;
                c[3] = (c[3] as f32 * coverage) as u8;
                self.pixel(px, py, c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Settings;

    #[test]
    fn bundled_font_covers_all_display_symbols() {
        let renderer = Renderer::new();
        for ch in
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789⇧⌃⌥⌘␣⏎⇥⌫⎋⌦↖↘⇞⇟↑↓←→−⇪"
                .chars()
        {
            assert_ne!(renderer.font.glyph_id(ch).0, 0, "Missing glyph: {ch}");
        }
        assert!((renderer.text_width("iii", 34.0) - renderer.text_width("WWW", 34.0)).abs() < 0.01);
    }
    #[test]
    fn eight_long_keys_fit_and_hidden_surface_is_empty() {
        let mut renderer = Renderer::new();
        let mut model = Model::new(Settings {
            max_keys: 8,
            font_size: 48,
            hide_ui: true,
            ..Settings::default()
        });
        model.keyboards = 1;
        for _ in 0..8 {
            model.history.push_back(HistoryEntry::new(
                "F12".into(),
                "F12".into(),
                Default::default(),
            ));
        }
        let (width, height) = renderer.dimensions(&model);
        assert!(width > 360);
        let used: f32 = model
            .history
            .iter()
            .enumerate()
            .map(|(i, entry)| renderer.cell_width(entry, i == 7, &model))
            .sum();
        assert!(used + 31.0 <= width as f32);
        let mut bytes = vec![0; (width * height * 4) as usize];
        renderer.draw(&mut bytes, width, height, 1, &model);
        assert!(bytes.iter().any(|v| *v != 0));
        model.show_settings = true;
        assert!(!renderer
            .draw(&mut bytes, width, height, 1, &model)
            .is_empty());
        model.show_settings = false;
        model.idle = true;
        assert!(renderer
            .draw(&mut bytes, width, height, 1, &model)
            .is_empty());
        assert!(bytes.iter().all(|v| *v == 0));
    }
    #[test]
    fn render_preview_at_normal_and_hidpi_scales() {
        let mut renderer = Renderer::new();
        let mut model = Model::new(Settings::default());
        model.keyboards = 1;
        model.modifiers.ctrl = true;
        for (s, l) in [("a", "a"), ("B", "b"), ("F12", "F12"), ("⌫", "Backspace")] {
            model
                .history
                .push_back(HistoryEntry::new(s.into(), l.into(), Default::default()));
        }
        model.history.back_mut().unwrap().count = 12;
        for (name, scale, settings, hud_scale) in [
            ("hud", 1, false, 1.0),
            ("settings", 1, true, 1.0),
            ("hidpi", 2, false, 1.0),
            ("scaled", 1, true, 1.5),
        ] {
            model.show_settings = settings;
            model.settings.hud_scale = hud_scale;
            let (width, height) = renderer.dimensions(&model);
            let mut bytes = vec![0; (width * height * scale * scale * 4) as usize];
            let hits = renderer.draw(&mut bytes, width, height, scale, &model);
            assert!(hits
                .iter()
                .all(|h| h.rect[0] >= 0.0 && h.rect[1] + h.rect[3] <= height as f32));
            for pixel in bytes.as_chunks::<4>().0 {
                let argb = u32::from_ne_bytes(*pixel);
                let a = argb >> 24;
                assert!((argb & 255) <= a && ((argb >> 8) & 255) <= a && ((argb >> 16) & 255) <= a);
            }
            if let Ok(dir) = std::env::var("HYPRKEY_PREVIEW_DIR") {
                use std::io::Write;
                std::fs::create_dir_all(&dir).unwrap();
                let mut file = std::fs::File::create(format!("{dir}/{name}.ppm")).unwrap();
                write!(file, "P6\n{} {}\n255\n", width * scale, height * scale).unwrap();
                for p in bytes.as_chunks::<4>().0 {
                    let argb = u32::from_ne_bytes(*p);
                    file.write_all(&[(argb >> 16) as u8, (argb >> 8) as u8, argb as u8])
                        .unwrap();
                }
            }
        }
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;
    use crate::model::Settings;
    #[test]
    fn hud_scale_scales_the_entire_surface_and_hit_regions() {
        let mut renderer = Renderer::new();
        let mut model = Model::new(Settings::default());
        model.keyboards = 1;
        model.show_settings = true;
        let (w, h) = renderer.dimensions(&model);
        let mut hidpi = vec![0; (w * h * 16) as usize];
        let original_hits = renderer.draw(&mut hidpi, w, h, 2, &model);
        model.settings.hud_scale = 2.0;
        assert_eq!(renderer.dimensions(&model), (w * 2, h * 2));
        let mut scaled = vec![0; hidpi.len()];
        let hits = renderer.draw(&mut scaled, w * 2, h * 2, 1, &model);
        // The scale percentage itself changes from 100% to 200%; every other
        // pixel must match a 2x compositor buffer, including the settings UI.
        let stride = (w * 2 * 4) as usize;
        assert!(
            hidpi[..626 * stride] == scaled[..626 * stride],
            "Scaled top content differs"
        );
        assert!(
            hidpi[680 * stride..] == scaled[680 * stride..],
            "Scaled bottom content differs"
        );
        for (old, new) in original_hits.iter().zip(&hits) {
            assert_eq!(new.rect, old.rect.map(|v| v * 2.0));
            assert!(new.contains(
                new.rect[0] + new.rect[2] / 2.0,
                new.rect[1] + new.rect[3] / 2.0
            ));
        }
        model.settings.hud_scale = 0.5;
        assert_eq!(renderer.dimensions(&model), (w / 2, h / 2));
    }
    #[test]
    fn repeat_label_is_visible_with_labels_disabled_and_fits_cell() {
        let renderer = Renderer::new();
        let model = Model::new(Settings {
            show_labels: false,
            ..Settings::default()
        });
        let mut entry = HistoryEntry::new("a".into(), "a".into(), Default::default());
        entry.count = 100_000;
        assert_eq!(entry.caption(false), "×100000");
        assert!(
            renderer.cell_width(&entry, true, &model)
                >= renderer.text_width("×100000", 12.0) + 18.0
        );
    }
}
