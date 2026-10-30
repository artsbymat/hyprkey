/// Accumulate compositor-accelerated relative motion in logical coordinates.
/// Surface-local wl_pointer coordinates must not feed back into a moving layer.
pub struct Drag {
    x: f64,
    y: f64,
    bounds: (i32, i32),
}
impl Drag {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            x: x as f64,
            y: y as f64,
            bounds: (8192, 8192),
        }
    }
    pub fn set_bounds(&mut self, bounds: (i32, i32)) {
        self.bounds = bounds;
        self.x = self.x.clamp(0.0, bounds.0 as f64);
        self.y = self.y.clamp(0.0, bounds.1 as f64);
    }
    pub fn motion(&mut self, dx: f64, dy: f64) -> (i32, i32) {
        self.x = (self.x + dx).clamp(0.0, self.bounds.0 as f64);
        self.y = (self.y + dy).clamp(0.0, self.bounds.1 as f64);
        (self.x.round() as i32, self.y.round() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batched_motion_does_not_reapply_previous_displacement() {
        let mut drag = Drag::new(100, 100);
        assert_eq!(drag.motion(10.0, 4.0), (110, 104));
        assert_eq!(drag.motion(10.0, 4.0), (120, 108));
        // A surface reposition or stationary pointer adds no displacement.
        assert_eq!(drag.motion(0.0, 0.0), (120, 108));
        assert_eq!(drag.motion(-20.0, -8.0), (100, 100));
    }
    #[test]
    fn fractional_motion_is_not_lost_between_events() {
        let mut drag = Drag::new(100, 100);
        for _ in 0..7 {
            drag.motion(0.125, -0.125);
        }
        assert_eq!(drag.motion(0.125, -0.125), (101, 99));
    }
    #[test]
    fn reversing_at_boundary_moves_immediately() {
        let mut drag = Drag::new(1, 8191);
        assert_eq!(drag.motion(-10.0, 10.0), (0, 8192));
        assert_eq!(drag.motion(1.0, -1.0), (1, 8191));
    }
}

/// Margins are output-local logical coordinates, not global desktop pixels.
pub fn position_limits(output: (i32, i32), hud: (u32, u32)) -> (i32, i32) {
    (
        (output.0 - hud.0 as i32).max(0),
        (output.1 - hud.1 as i32).max(0),
    )
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    #[test]
    fn saved_offscreen_position_is_recovered_and_drag_stops_at_screen_edge() {
        let mut drag = Drag::new(2408, 302);
        drag.set_bounds(position_limits((1920, 1080), (360, 142)));
        assert_eq!(drag.motion(0.0, 0.0), (1560, 302));
        assert_eq!(drag.motion(1000.0, 1000.0), (1560, 938));
        assert_eq!(drag.motion(-1.0, -1.0), (1559, 937));
    }
    #[test]
    fn panel_growth_and_logical_hidpi_size_reduce_bounds() {
        assert_eq!(position_limits((1280, 720), (360, 410)), (920, 310));
        assert_eq!(position_limits((320, 240), (360, 410)), (0, 0));
    }
}

/// A short, time-based follow animation. Limit visual lag to eight logical
/// pixels so easing never turns into the long compositor-layer chase.
pub struct SmoothPosition {
    x: f64,
    y: f64,
}
impl SmoothPosition {
    pub fn new(x: i32, y: i32) -> Self {
        Self {
            x: x as f64,
            y: y as f64,
        }
    }
    pub fn advance(&mut self, target: (i32, i32), seconds: f64) -> ((i32, i32), bool) {
        let target = (target.0 as f64, target.1 as f64);
        let alpha = 1.0 - (-seconds.clamp(0.0, 0.05) / 0.012).exp();
        self.x += (target.0 - self.x) * alpha;
        self.y += (target.1 - self.y) * alpha;
        let dx = target.0 - self.x;
        let dy = target.1 - self.y;
        let distance = dx.hypot(dy);
        if distance > 8.0 {
            self.x = target.0 - dx / distance * 8.0;
            self.y = target.1 - dy / distance * 8.0;
        }
        let settled = (target.0 - self.x).hypot(target.1 - self.y) < 0.25;
        if settled {
            self.x = target.0;
            self.y = target.1;
        }
        ((self.x.round() as i32, self.y.round() as i32), settled)
    }
}

#[cfg(test)]
mod smoothing_tests {
    use super::*;
    #[test]
    fn animation_is_bounded_monotonic_and_settles() {
        let mut position = SmoothPosition::new(100, 100);
        let mut previous = 100;
        let mut settled = false;
        for _ in 0..12 {
            let (point, done) = position.advance((200, 100), 1.0 / 60.0);
            assert!((previous..=200).contains(&point.0));
            assert!(200 - point.0 <= 8);
            assert_eq!(point.1, 100);
            previous = point.0;
            settled = done;
        }
        assert!(settled);
        assert_eq!(previous, 200);
    }
    #[test]
    fn easing_uses_elapsed_time_not_frame_count() {
        let mut slow = SmoothPosition::new(0, 0);
        let mut fast = SmoothPosition::new(0, 0);
        for _ in 0..2 {
            slow.advance((5, 0), 1.0 / 60.0);
        }
        for _ in 0..4 {
            fast.advance((5, 0), 1.0 / 120.0);
        }
        assert!((slow.x - fast.x).abs() < 0.00001);
    }
}
