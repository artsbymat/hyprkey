mod drag;
mod keyboard;
mod model;
mod render;

use drag::{position_limits, Drag, SmoothPosition};
use model::{Model, Settings};
use render::{Action, Hit, Renderer};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{channel, EventLoop},
        calloop_wayland_source::WaylandSource,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{
            CursorIcon, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer,
            BTN_LEFT, BTN_RIGHT,
        },
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::time::Instant;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use wayland_protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    if let Err(error) = run() {
        eprintln!("hyprkey: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result {
    let conn = Connection::connect_to_env().map_err(|e| format!("Connect to Wayland: {e}. Run hyprkey inside Hyprland or another layer-shell compositor."))?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let mut event_loop = EventLoop::<App>::try_new()?;
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle())?;
    let compositor = CompositorState::bind(&globals, &qh)?;
    let shell = LayerShell::bind(&globals, &qh)
        .map_err(|e| format!("wlr-layer-shell is required (supported by Hyprland): {e}"))?;
    let relative_manager = globals
        .bind::<ZwpRelativePointerManagerV1, _, _>(&qh, 1..=1, ())
        .map_err(|e| format!("Relative pointer protocol is required for stable dragging: {e}"))?;
    let shm = Shm::bind(&globals, &qh)?;
    let model = Model::new(Settings::load());
    let renderer = Renderer::new();
    let size = renderer.dimensions(&model);
    let surface = compositor.create_surface(&qh);
    let layer = shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("hyprkey"), None);
    // Anchor to top-left so that margins position the surface from (0,0).
    layer.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    // No reserved space and no displacement by panels' exclusive zones.
    layer.set_exclusive_zone(-1);
    // Set the HUD size directly — no full-screen transparent parent needed.
    layer.set_size(size.0, size.1);
    // Position via margins from the top-left corner.
    layer.set_margin(model.settings.y, 0, 0, model.settings.x);
    layer.commit();
    let pool = SlotPool::new((size.0 * size.1 * 4) as usize, &shm)?;
    let motion = SmoothPosition::new(model.settings.x, model.settings.y);
    let mut app = App {
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        pool,
        motion,
        frame_pending: false,
        last_frame: Instant::now(),
        shm,
        compositor,
        layer,
        model,
        renderer,
        hits: Vec::new(),
        themed_pointer: None,
        current_cursor: None,
        pointer_position: None,
        relative_manager,
        relative_pointer: None,
        position_dirty: false,
        current_output: None,
        drag: None,
        size,
        scale: 1,
        configured: false,
        dirty: true,
        exit: false,
    };
    let (tx, rx) = channel::sync_channel(256);
    event_loop
        .handle()
        .insert_source(rx, |event, _, app| {
            match event {
                channel::Event::Msg(message) => app.model.message(message),
                channel::Event::Closed => {
                    eprintln!("hyprkey: keyboard listener stopped");
                    app.model.keyboards = 0;
                }
            }
            app.dirty = true;
        })
        .map_err(|e| format!("Register keyboard events: {e}"))?;
    keyboard::start(tx);
    while !app.exit {
        if app.drag.is_none() && app.model.expire() {
            app.dirty = true;
        }
        app.present(&conn, &qh)?;
        conn.flush()?;
        let timeout = if app.drag.is_some() {
            if app.position_dirty {
                Some(std::time::Duration::from_millis(16))
            } else {
                Some(std::time::Duration::from_millis(50))
            }
        } else {
            app.model.timeout()
        };
        event_loop.dispatch(timeout, &mut app)?;
    }
    Ok(())
}

struct App {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    motion: SmoothPosition,
    frame_pending: bool,
    last_frame: Instant,
    model: Model,
    renderer: Renderer,
    hits: Vec<Hit>,
    themed_pointer: Option<ThemedPointer>,
    current_cursor: Option<CursorIcon>,
    pointer_position: Option<(f64, f64)>,
    relative_manager: ZwpRelativePointerManagerV1,
    relative_pointer: Option<ZwpRelativePointerV1>,
    drag: Option<Drag>,
    position_dirty: bool,
    current_output: Option<wl_output::WlOutput>,
    size: (u32, u32),
    scale: u32,
    configured: bool,
    dirty: bool,
    exit: bool,
}
impl App {
    fn current_output_size(&self) -> (i32, i32) {
        let output = self
            .current_output
            .clone()
            .or_else(|| self.outputs.outputs().next());
        if let Some(info) = output.as_ref().and_then(|o| self.outputs.info(o)) {
            if let Some(logical) = info.logical_size {
                if logical.0 > 0 && logical.1 > 0 {
                    return logical;
                }
            }
            if let Some(mode) = info.modes.iter().find(|m| m.current) {
                let scale = info.scale_factor.max(1);
                let w = mode.dimensions.0 / scale;
                let h = mode.dimensions.1 / scale;
                if w > 0 && h > 0 {
                    return (w, h);
                }
            }
        }
        (8192, 8192)
    }
    fn output_bounds(&self, size: (u32, u32)) -> Option<(i32, i32)> {
        self.configured
            .then(|| position_limits(self.current_output_size(), size))
    }
    fn keep_visible(&mut self, size: (u32, u32)) {
        if let Some(bounds) = self.output_bounds(size) {
            if let Some(drag) = &mut self.drag {
                drag.set_bounds(bounds);
            }
            let x = self.model.settings.x.clamp(0, bounds.0);
            let y = self.model.settings.y.clamp(0, bounds.1);
            if (x, y) != (self.model.settings.x, self.model.settings.y) {
                self.model.settings.x = x;
                self.model.settings.y = y;
                self.position_dirty = true;
                if self.drag.is_none() {
                    self.model.save();
                }
            }
        }
    }
    fn desired_cursor(&self, pos: (f64, f64)) -> CursorIcon {
        if self.drag.is_some() {
            CursorIcon::Grabbing
        } else if self
            .hits
            .iter()
            .any(|h| h.contains(pos.0 as f32, pos.1 as f32))
        {
            CursorIcon::Pointer
        } else if pos.1 < 94.0 * self.model.settings.hud_scale as f64 {
            CursorIcon::Grab
        } else {
            CursorIcon::Default
        }
    }
    fn update_cursor(&mut self, conn: &Connection) {
        if let (Some(themed_pointer), Some(pos)) = (&self.themed_pointer, self.pointer_position) {
            let cursor = self.desired_cursor(pos);
            if self.current_cursor != Some(cursor) {
                if let Err(err) = themed_pointer.set_cursor(conn, cursor) {
                    eprintln!("hyprkey: set cursor error: {err}");
                } else {
                    self.current_cursor = Some(cursor);
                }
            }
        }
    }
    fn present(&mut self, conn: &Connection, qh: &QueueHandle<Self>) -> Result {
        if !self.configured {
            return Ok(());
        }
        let desired = if self.drag.is_some() {
            self.size
        } else {
            self.renderer.dimensions(&self.model)
        };
        self.keep_visible(desired);
        let target = (self.model.settings.x, self.model.settings.y);
        let mut commit = false;
        // Drive the brief follow animation. Updating margins repositions the layer surface.
        let should_step = self.position_dirty
            && (!self.frame_pending || self.last_frame.elapsed().as_millis() > 30);
        if should_step {
            let elapsed = self.last_frame.elapsed().as_secs_f64();
            self.last_frame = Instant::now();
            let (position, settled) = self.motion.advance(target, elapsed);
            self.layer.set_margin(position.1, 0, 0, position.0);
            self.position_dirty = !settled;
            // Request one callback and damage buffer to ensure compositor presentation.
            self.layer
                .wl_surface()
                .frame(qh, self.layer.wl_surface().clone());
            self.layer.wl_surface().damage_buffer(0, 0, 1, 1);
            self.frame_pending = true;
            commit = true;
        }
        if desired != self.size {
            self.size = desired;
            self.dirty = true;
        }
        if self.dirty {
            let (width, height) = self.size;
            let (pw, ph) = (width * self.scale, height * self.scale);
            let (buffer, canvas) = self.pool.create_buffer(
                pw as i32,
                ph as i32,
                pw as i32 * 4,
                wl_shm::Format::Argb8888,
            )?;
            self.hits = self
                .renderer
                .draw(canvas, width, height, self.scale, &self.model);
            self.layer.wl_surface().set_buffer_scale(self.scale as i32);
            let region = Region::new(&self.compositor)?;
            if !self.model.hidden() {
                region.add(0, 0, width as i32, height as i32);
            }
            self.layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
            self.layer
                .wl_surface()
                .damage_buffer(0, 0, pw as i32, ph as i32);
            buffer.attach_to(self.layer.wl_surface())?;
            // Update layer surface size so the compositor knows the actual extent.
            self.layer.set_size(width, height);
            self.dirty = false;
            commit = true;
            self.update_cursor(conn);
        }
        if commit {
            self.layer.commit();
        }
        Ok(())
    }
    fn action(&mut self, action: Action) {
        match action {
            Action::Settings => self.model.show_settings = !self.model.show_settings,
            Action::Hide => self.model.settings.hide_ui = !self.model.settings.hide_ui,
            Action::Labels => self.model.settings.show_labels = !self.model.settings.show_labels,
            Action::Delay(delta) => {
                self.model.settings.clear_delay =
                    ((self.model.settings.clear_delay * 10.0).round() + delta as f32) / 10.0
            }
            Action::Count(delta) => {
                self.model.settings.max_keys =
                    (self.model.settings.max_keys as i32 + delta as i32).clamp(1, 8) as usize
            }
            Action::Font(delta) => {
                self.model.settings.font_size =
                    (self.model.settings.font_size as i32 + delta as i32 * 2).clamp(24, 48) as u16
            }
            Action::Scale(delta) => {
                self.model.settings.hud_scale =
                    ((self.model.settings.hud_scale * 10.0).round() + delta as f32) / 10.0;
            }
            Action::Quit => self.exit = true,
        }
        self.model.settings.normalize();
        self.model.trim();
        if !matches!(action, Action::Settings | Action::Quit) {
            self.model.save();
        }
        self.dirty = true;
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        self.scale = factor.clamp(1, 8) as u32;
        self.dirty = true;
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if surface == self.layer.wl_surface() {
            self.frame_pending = false;
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        self.current_output = Some(output.clone());
        self.position_dirty = true;
        self.dirty = true;
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.current_output.as_ref() == Some(&output) {
            self.position_dirty = true;
            self.dirty = true;
        }
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        _config: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.configured = true;
        self.position_dirty = true;
        self.dirty = true;
    }
}
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.themed_pointer.is_none() {
            let surface = self.compositor.create_surface(qh);
            match self.seats.get_pointer_with_theme(
                qh,
                &seat,
                self.shm.wl_shm(),
                surface,
                ThemeSpec::default(),
            ) {
                Ok(themed_pointer) => {
                    self.relative_pointer = Some(self.relative_manager.get_relative_pointer(
                        themed_pointer.pointer(),
                        qh,
                        (),
                    ));
                    self.themed_pointer = Some(themed_pointer);
                    self.update_cursor(conn);
                }
                Err(error) => eprintln!("hyprkey: themed pointer unavailable: {error}"),
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(relative) = self.relative_pointer.take() {
                relative.destroy();
            }
            if self.drag.take().is_some() {
                self.model.save();
                self.dirty = true;
            }
            if let Some(themed_pointer) = self.themed_pointer.take() {
                themed_pointer.pointer().release();
            }
            self.current_cursor = None;
            self.pointer_position = None;
            self.drag = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}
impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if event.surface != *self.layer.wl_surface() {
                continue;
            }
            let (x, y) = event.position;
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.pointer_position = Some((x, y));
                    self.current_cursor = None;
                    self.update_cursor(conn);
                }
                PointerEventKind::Leave { .. } => {
                    if self.drag.is_none() {
                        self.pointer_position = None;
                        self.current_cursor = None;
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_position = Some((x, y));
                    self.update_cursor(conn);
                }
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => {
                    self.pointer_position = Some((x, y));
                    if let Some(hit) = self
                        .hits
                        .iter()
                        .find(|h| h.contains(x as f32, y as f32))
                        .copied()
                    {
                        self.action(hit.action);
                    } else if y < 94.0 * self.model.settings.hud_scale as f64 {
                        let mut drag = Drag::new(self.model.settings.x, self.model.settings.y);
                        if let Some(bounds) = self.output_bounds(self.size) {
                            drag.set_bounds(bounds);
                        }
                        self.drag = Some(drag);
                        self.last_frame = Instant::now();
                    }
                    self.update_cursor(conn);
                }
                PointerEventKind::Press {
                    button: BTN_RIGHT, ..
                } => {
                    self.pointer_position = Some((x, y));
                    if self.drag.is_none() {
                        self.action(Action::Settings);
                    }
                    self.update_cursor(conn);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.pointer_position = Some((x, y));
                    if self.drag.take().is_some() {
                        self.frame_pending = false;
                        self.position_dirty = true;
                        self.model.save();
                        self.dirty = true;
                    }
                    self.update_cursor(conn);
                }
                _ => {}
            }
        }
    }
}
impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_pointer!(App);
delegate_layer!(App);
delegate_registry!(App);

impl wayland_client::Dispatch<ZwpRelativePointerV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event {
            if let Some(drag) = &mut app.drag {
                let (x, y) = drag.motion(dx, dy);
                if (x, y) != (app.model.settings.x, app.model.settings.y) {
                    app.model.settings.x = x;
                    app.model.settings.y = y;
                    app.position_dirty = true;
                }
            }
        }
    }
}
wayland_client::delegate_noop!(App: ignore ZwpRelativePointerManagerV1);
