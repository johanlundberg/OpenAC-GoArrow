//! The arrow (`GoArrowHud.cs`): a screen overlay with a compass dial whose
//! pointer turns toward the next waypoint (its bearing less the
//! character's heading, so straight up is straight ahead), the step it
//! points to, the distance and bearing, and two small buttons: Go/Stop and
//! show or hide the map. GoArrow's separate Stop/Resume toolbar is these
//! buttons now (DIVERGENCES.md, GA-14). Drag it anywhere to move it; where
//! it is dropped is saved.
//!
//! The dial, the pointer and the buttons' icons are smooth images drawn by
//! the plugin ([`crate::sprites`]), tinted: the pointer is a navigation
//! arrow with a notched tail over a dark edge a pixel lower, turned about
//! the dial's middle, dimmer while not walking and bright while walking
//! (DIVERGENCES.md, GA-14).
//!
//! It paints with the plugin theme's colours, and again when the theme
//! changes; under Classic it keeps GoArrow's own.

use openac_plugin::Event;
use openac_plugin::Host;
use openac_plugin::ui::{Anchor, Canvas, CanvasSpec, Color, Painter, PointerEvent, rgba};

use crate::distance::format_travel_distance;
use crate::plugin::{GoArrow, Here};
use crate::settings::Settings;
use crate::sprites::{Sprite, Sprites};

/// The canvas's id.
pub const CANVAS_ID: &str = "goarrow.arrow";
/// Its size, pixels.
pub const SIZE: (i32, i32) = (330, 92);
/// The dial's centre.
pub const DIAL_CENTER: (f32, f32) = (40.0, 46.0);
/// The dial's ring radius.
pub const RING_RADIUS: f32 = crate::sprites::RING.0;
/// The divider between the dial and the text.
pub const DIVIDER_X: f32 = 80.0;
/// Where the text starts.
pub const TEXT_X: f32 = 90.0;
/// The Go/Stop button: x, y, width, height.
pub const GO_BUTTON: (f32, f32, f32, f32) = (292.0, 10.0, 30.0, 32.0);
/// The map button.
pub const MAP_BUTTON: (f32, f32, f32, f32) = (292.0, 50.0, 30.0, 32.0);
/// The widest the text may be.
pub const TEXT_WIDTH: f32 = GO_BUTTON.0 - TEXT_X - 8.0;
/// How far a press may move and still be a click, pixels.
pub const CLICK_SLOP: f32 = 4.0;
/// How far below the pointer its dark edge is drawn, pixels.
pub const SHADOW_OFFSET: f32 = 1.0;

/// A colour as bytes red, green, blue, alpha (the SDK's colour, copyable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    /// The SDK's colour.
    #[must_use]
    pub const fn c(self) -> Color {
        rgba(self.0, self.1, self.2, self.3)
    }

    /// A theme colour with another alpha.
    #[must_use]
    pub fn of(c: &Color, a: u8) -> Self {
        Self(c.r, c.g, c.b, a)
    }
}

/// The colours the arrow paints with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudPalette {
    /// The face.
    pub background: Rgba,
    /// The edge.
    pub border: Rgba,
    /// The dial's ring.
    pub ring: Rgba,
    /// The dial's ticks.
    pub ticks: Rgba,
    /// The centre dot.
    pub dot: Rgba,
    /// The line above the name.
    pub muted: Rgba,
    /// The name.
    pub text: Rgba,
    /// The readout.
    pub accent: Rgba,
    /// The arrow while walking.
    pub pointer: Rgba,
    /// The arrow while not walking: dimmer, the same hue.
    pub pointer_dim: Rgba,
    /// A button's face.
    pub field: Rgba,
    /// A button's face while it is on.
    pub selected: Rgba,
}

impl HudPalette {
    /// GoArrow's own colours, for Classic.
    pub const CLASSIC: Self = Self {
        background: Rgba(12, 20, 29, 226),
        border: Rgba(111, 128, 138, 215),
        ring: Rgba(107, 129, 139, 170),
        ticks: Rgba(190, 206, 211, 210),
        dot: Rgba(255, 224, 145, 255),
        muted: Rgba(156, 177, 186, 255),
        text: Rgba(255, 255, 255, 255),
        accent: Rgba(255, 215, 113, 255),
        pointer: Rgba(245, 175, 52, 255),
        pointer_dim: dim(Rgba(245, 175, 52, 255), Rgba(12, 20, 29, 226)),
        field: Rgba(4, 8, 12, 235),
        selected: Rgba(52, 66, 76, 235),
    };

    /// The colours of a theme (Classic: GoArrow's).
    #[must_use]
    pub fn of(theme: &ac_plugin_abi::ui::ThemeInfo) -> Self {
        theme.palette.as_ref().map_or(Self::CLASSIC, |p| {
            let with = |c: &Color, a: u8| Rgba::of(c, a);
            Self {
                background: with(&p.background, 232),
                border: with(&p.border, 255),
                ring: with(&p.border, 230),
                ticks: with(&p.muted, 255),
                dot: with(&p.text, 255),
                muted: with(&p.muted, 255),
                text: with(&p.text, 255),
                accent: with(&p.accent, 255),
                pointer: with(&p.accent, 255),
                pointer_dim: dim(with(&p.accent, 255), with(&p.background, 255)),
                field: with(&p.field, 255),
                selected: with(&p.selected, 255),
            }
        })
    }
}

/// How much of the background a dim arrow takes on, percent.
pub const DIM_PERCENT: u32 = 35;

/// `c` dimmed: [`DIM_PERCENT`] of the way to `background`, opaque.
#[must_use]
pub const fn dim(c: Rgba, background: Rgba) -> Rgba {
    const fn mix(a: u8, b: u8) -> u8 {
        ((a as u32 * (100 - DIM_PERCENT) + b as u32 * DIM_PERCENT + 50) / 100) as u8
    }
    Rgba(
        mix(c.0, background.0),
        mix(c.1, background.1),
        mix(c.2, background.2),
        255,
    )
}

/// The pointer's dark edge (GoArrow's outline colour, under every theme).
pub const POINTER_OUTLINE: Color = rgba(30, 22, 11, 240);
/// The light over the pointer's left half: walking, and not.
pub const POINTER_LIGHT: (Color, Color) = (rgba(255, 239, 170, 96), rgba(255, 239, 170, 48));

/// The turn the pointer shows, radians: bearing less heading.
#[must_use]
pub fn pointer_angle(bearing_degrees: f64, heading_degrees: f64) -> Option<f64> {
    (bearing_degrees.is_finite() && heading_degrees.is_finite())
        .then(|| (bearing_degrees - heading_degrees).to_radians())
}

/// The readout: `184 m · 52°`, either half per the settings, `--°` for a
/// bearing not known.
#[must_use]
pub fn readout(show_distance: bool, show_bearing: bool, distance: f64, bearing: f64) -> String {
    let bearing = if bearing.is_finite() {
        format!("{}°", crate::text::format_fixed(bearing, 0, 0))
    } else {
        "--°".to_owned()
    };
    let distance = format_travel_distance(distance);
    match (show_distance, show_bearing) {
        (true, true) => format!("{distance} · {bearing}"),
        (true, false) => distance,
        (false, true) => bearing,
        (false, false) => String::new(),
    }
}

fn inside(rect: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= rect.0 && x < rect.0 + rect.2 && y >= rect.1 && y < rect.1 + rect.3
}

/// What a click on the arrow does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudButton {
    /// Go, or Stop while walking.
    GoStop,
    /// Shows or hides the map.
    Map,
}

/// The button under a point, if any.
#[must_use]
pub fn button_at(x: f32, y: f32) -> Option<HudButton> {
    if inside(GO_BUTTON, x, y) {
        Some(HudButton::GoStop)
    } else if inside(MAP_BUTTON, x, y) {
        Some(HudButton::Map)
    } else {
        None
    }
}

/// What the arrow shows, compared to skip painting the same again.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    paused: bool,
    eyebrow: String,
    name: String,
    readout: String,
    angle: Option<i32>,
    navigating: bool,
    map_visible: bool,
    palette: HudPalette,
    sprites: Sprites,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    grab: (f32, f32),
    moved: bool,
}

/// The arrow overlay.
#[derive(Debug)]
pub struct Hud {
    canvas: Canvas,
    offset: (f32, f32),
    palette: HudPalette,
    drag: Option<Drag>,
    shown: Option<Frame>,
    fitted: Option<(String, String)>,
    visible: bool,
}

impl Hud {
    /// Registers the arrow at its saved place.
    ///
    /// # Errors
    /// The client refused the canvas.
    pub fn open(host: &mut Host<'_>, settings: &Settings) -> Result<Self, String> {
        let offset = (
            settings.arrow_offset_x as f32,
            settings.arrow_offset_y as f32,
        );
        let canvas = host.register_canvas(&CanvasSpec {
            canvas_id: CANVAS_ID.to_owned(),
            width: SIZE.0,
            height: SIZE.1,
            anchor: Anchor::TopRight,
            offset,
            start_visible: settings.hud_visible,
            accepts_pointer_input: !settings.hud_click_through,
        })?;
        canvas.listen_pointer(host, !settings.hud_click_through);
        let palette = HudPalette::of(&host.theme());
        Ok(Self {
            canvas,
            offset,
            palette,
            drag: None,
            shown: None,
            fitted: None,
            visible: settings.hud_visible,
        })
    }

    /// The canvas's number (its pointer events' `canvas`).
    #[must_use]
    pub fn canvas_id(&self) -> u32 {
        self.canvas.id()
    }

    /// Where it is, pixels from the screen's top-right corner.
    #[must_use]
    pub fn offset(&self) -> (f32, f32) {
        self.offset
    }

    /// Shows or hides it.
    pub fn set_visible(&mut self, host: &mut Host<'_>, visible: bool) {
        self.visible = visible;
        self.canvas.set_visible(host, visible);
        self.shown = None;
    }

    /// The theme changed: paint again in its colours.
    pub fn on_event(&mut self, event: &Event) {
        if let Event::ThemeChanged(theme) = event {
            self.palette = HudPalette::of(theme);
            self.shown = None;
        }
    }

    fn fit(&mut self, host: &mut Host<'_>, name: &str) -> String {
        if let Some((full, fitted)) = &self.fitted
            && full == name
        {
            return fitted.clone();
        }
        let fitted = fit_text(host, name, TEXT_WIDTH);
        self.fitted = Some((name.to_owned(), fitted.clone()));
        fitted
    }

    fn frame(&mut self, app: &GoArrow, host: &mut Host<'_>, here: &Here) -> Frame {
        let destination = app.destination();
        let view = app.navigator().view();
        let navigating = app.is_navigating();
        let map_visible = app.settings().map_visible;
        if here.paused() {
            let (eyebrow, name) = if view.waiting_for_indoor_portal {
                (
                    "FINDING INDOOR PORTAL",
                    "Waiting for portal object".to_owned(),
                )
            } else if view.has_indoor_target {
                let target = destination.target_name().to_owned();
                ("INDOOR TARGET", self.fit(host, &target))
            } else {
                ("OUTDOOR ROUTE PAUSED", "Resumes outdoors".to_owned())
            };
            return Frame {
                paused: true,
                eyebrow: eyebrow.to_owned(),
                name,
                readout: String::new(),
                angle: None,
                navigating,
                map_visible,
                palette: self.palette,
                sprites: *app.sprites(),
            };
        }
        let remaining = destination.current_route().map_or(0, |r| r.step_count());
        let eyebrow = if !destination.has_destination() {
            "No destination".to_owned()
        } else if remaining == 0 {
            if view.is_navigating {
                "Walking".to_owned()
            } else {
                "Next waypoint".to_owned()
            }
        } else {
            let step = view.leg_index + 1;
            let total = view.leg_index + remaining;
            let lead = if view.is_navigating {
                "Walking"
            } else {
                "Next"
            };
            format!("{lead} · step {step} of {total}")
        };
        let target = destination
            .immediate_target()
            .map_or_else(|| "No destination".to_owned(), |t| t.name.clone());
        let name = self.fit(host, &target);
        let settings = app.settings();
        let readout = if destination.has_destination() {
            readout(
                settings.show_distance,
                settings.show_bearing,
                destination.guidance_distance(),
                destination.bearing_degrees(),
            )
        } else {
            String::new()
        };
        let angle = pointer_angle(
            destination.bearing_degrees(),
            f64::from(here.position.heading_degrees),
        )
        .map(|a| (a.to_degrees() * 2.0).round() as i32);
        Frame {
            paused: false,
            eyebrow,
            name,
            readout,
            angle,
            navigating,
            map_visible,
            palette: self.palette,
            sprites: *app.sprites(),
        }
    }

    /// One tick: paints when what it shows changed.
    pub fn tick(&mut self, app: &mut GoArrow, host: &mut Host<'_>, here: &Here) {
        if !self.visible {
            return;
        }
        let frame = self.frame(app, host, here);
        if self.shown.as_ref() == Some(&frame) {
            return;
        }
        let paint = frame.clone();
        if self.canvas.paint(host, |p| paint_frame(p, &paint)).is_ok() {
            self.shown = Some(frame);
        }
    }

    /// Pointer input: a drag moves it (saved when dropped); a click on a
    /// button presses it.
    pub fn pointer(&mut self, app: &mut GoArrow, host: &mut Host<'_>, event: &PointerEvent) {
        match event.kind {
            PointerEvent::DOWN if event.button == 1 => {
                self.drag = Some(Drag {
                    grab: (event.x, event.y),
                    moved: false,
                });
            }
            PointerEvent::UP | PointerEvent::MOVE => {
                let Some(mut drag) = self.drag else {
                    return;
                };
                let dx = event.x - drag.grab.0;
                let dy = event.y - drag.grab.1;
                if !drag.moved && dx.abs().max(dy.abs()) >= CLICK_SLOP {
                    drag.moved = true;
                }
                if drag.moved && (dx != 0.0 || dy != 0.0) {
                    self.offset = (self.offset.0 + dx, self.offset.1 + dy);
                    self.canvas.set_offset(host, self.offset.0, self.offset.1);
                }
                if event.kind == PointerEvent::UP {
                    self.drag = None;
                    if drag.moved {
                        self.save(app, host);
                    } else if let Some(button) = button_at(drag.grab.0, drag.grab.1) {
                        Self::press(app, host, button);
                        self.shown = None;
                    }
                } else {
                    self.drag = Some(drag);
                }
            }
            PointerEvent::CANCELLED if self.drag.take().is_some_and(|d| d.moved) => {
                self.save(app, host);
            }
            _ => {}
        }
    }

    fn save(&self, app: &mut GoArrow, host: &mut Host<'_>) {
        let (x, y) = self.offset;
        app.update_settings(host, |s| {
            s.arrow_offset_x = f64::from(x);
            s.arrow_offset_y = f64::from(y);
        });
    }

    fn press(app: &mut GoArrow, host: &mut Host<'_>, button: HudButton) {
        match button {
            HudButton::GoStop => {
                if app.is_navigating() {
                    app.stop_navigation(host);
                } else if app.destination().has_destination() {
                    app.go_pressed(host);
                }
            }
            HudButton::Map => {
                let visible = !app.settings().map_visible;
                app.set_map_visible(host, visible);
            }
        }
    }
}

/// `text`, cut with `…` to fit `width` pixels in the interface font.
pub fn fit_text(host: &mut Host<'_>, text: &str, width: f32) -> String {
    if host.measure_text(text).0 <= width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if host.measure_text(&candidate).0 <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    chars[..low].iter().collect::<String>() + "…"
}

fn paint_frame(p: &mut Painter, frame: &Frame) {
    let palette = frame.palette;
    let sprites = &frame.sprites;
    let (w, h) = (p.width() as f32, p.height() as f32);
    p.clear(rgba(0, 0, 0, 0));
    p.fill_rect(0.0, 0.0, w, h, palette.background.c());
    p.stroke_rect(0.0, 0.0, w, h, palette.border.c(), 1.0);
    p.fill_rect(DIVIDER_X, 10.0, 1.0, h - 20.0, palette.ring.c());
    sprites.draw(p, Sprite::Ring, DIAL_CENTER, palette.ring.c());
    sprites.draw(p, Sprite::Ticks, DIAL_CENTER, palette.ticks.c());
    match frame.angle {
        Some(half_degrees) => {
            let rotation = (f64::from(half_degrees) / 2.0).to_radians() as f32;
            paint_pointer(p, sprites, rotation, frame.navigating, &palette);
        }
        None => sprites.draw(p, Sprite::Hub, DIAL_CENTER, palette.dot.c()),
    }
    if frame.paused {
        p.text(&frame.eyebrow, (TEXT_X, 22.0), palette.accent.c(), true);
        p.text(&frame.name, (TEXT_X, 50.0), palette.text.c(), true);
    } else {
        p.text(&frame.eyebrow, (TEXT_X, 8.0), palette.muted.c(), true);
        p.text(&frame.name, (TEXT_X, 30.0), palette.text.c(), true);
        if !frame.readout.is_empty() {
            p.text(&frame.readout, (TEXT_X, 56.0), palette.accent.c(), true);
        }
    }
    paint_buttons(p, frame, &palette);
}

/// The pointer turned `rotation` radians clockwise from straight up: its
/// dark edge a pixel lower, the arrow, the light on its left half.
fn paint_pointer(
    p: &mut Painter,
    sprites: &Sprites,
    rotation: f32,
    walking: bool,
    palette: &HudPalette,
) {
    let (cx, cy) = DIAL_CENTER;
    let (tint, light) = if walking {
        (palette.pointer, POINTER_LIGHT.0)
    } else {
        (palette.pointer_dim, POINTER_LIGHT.1)
    };
    sprites.draw_turned(
        p,
        Sprite::PointerEdge,
        (cx, cy + SHADOW_OFFSET),
        rotation,
        1.0,
        POINTER_OUTLINE,
    );
    sprites.draw_turned(p, Sprite::Pointer, DIAL_CENTER, rotation, 1.0, tint.c());
    sprites.draw_turned(p, Sprite::PointerLit, DIAL_CENTER, rotation, 1.0, light);
}

/// The buttons as the window draws its own: a dark face (lit while on)
/// in a thin border, the icon in the text colour.
fn paint_buttons(p: &mut Painter, frame: &Frame, palette: &HudPalette) {
    let sprites = &frame.sprites;
    let middle = |r: (f32, f32, f32, f32)| (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    for (rect, on) in [
        (GO_BUTTON, frame.navigating),
        (MAP_BUTTON, frame.map_visible),
    ] {
        let face = if on { palette.selected } else { palette.field };
        p.fill_rect(rect.0, rect.1, rect.2, rect.3, face.c());
        p.stroke_rect(rect.0, rect.1, rect.2, rect.3, palette.border.c(), 1.0);
    }
    let (cx, cy) = middle(GO_BUTTON);
    if frame.navigating {
        // Stop: two bars, on whole pixels.
        p.fill_rect(cx - 6.0, cy - 7.0, 4.0, 14.0, palette.text.c());
        p.fill_rect(cx + 2.0, cy - 7.0, 4.0, 14.0, palette.text.c());
    } else {
        sprites.draw(p, Sprite::Go, (cx, cy), palette.text.c());
    }
    sprites.draw(p, Sprite::MapIcon, middle(MAP_BUTTON), palette.text.c());
}
