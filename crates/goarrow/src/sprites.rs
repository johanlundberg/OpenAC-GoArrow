//! The smooth shapes ([`crate::shapes`]) the arrow and the map draw, held
//! by the client as white images and tinted when drawn.
//!
//! They are drawn and handed over once, at the first tick a window is
//! attached (a headless session never needs them), and kept: the client
//! keeps a plugin's images across windows. One it refused is tried again
//! when a window attaches. Each is drawn at [`OVERSAMPLE`] times the size
//! it is shown at, so it stays smooth turned (the client samples a
//! plugin's images linearly).

use openac_plugin::ui::{Color, Image, Painter};
use openac_plugin::{Event, Host};

use crate::shapes::{self, Mask};

/// Image pixels per canvas pixel.
pub const OVERSAMPLE: f32 = 2.0;

/// The arrow's square, canvas pixels.
pub const POINTER_BOX: f32 = 56.0;
/// The arrow from its middle to its tip, canvas pixels.
pub const POINTER_HALF: f32 = 20.0;
/// How far its dark edge reaches past it, canvas pixels.
pub const POINTER_EDGE: f32 = 1.75;
/// The player's marker on the map: its square, canvas pixels.
pub const MARKER_BOX: f32 = 26.0;
/// The marker from its middle to its tip.
pub const MARKER_HALF: f32 = 9.0;
/// How far its dark edge reaches past it.
pub const MARKER_EDGE: f32 = 1.5;
/// The dial's square, canvas pixels.
pub const DIAL_BOX: f32 = 76.0;
/// The dial's ring: radius to the middle of its band, and its width.
pub const RING: (f32, f32) = (29.0, 1.5);
/// The east, south and west ticks: from, to (pixels out) and width.
pub const TICK: (f32, f32, f32) = (31.0, 34.0, 2.0);
/// The north tick, a little stronger.
pub const NORTH_TICK: (f32, f32, f32) = (30.5, 36.0, 3.0);
/// The dial's middle dot: its square and radius.
pub const HUB: (f32, f32) = (8.0, 2.5);
/// The destination pin's box, canvas pixels.
pub const PIN_BOX: (f32, f32) = (18.0, 26.0);
/// The pin's head: its middle down from the box's top, and its radius.
pub const PIN_HEAD: (f32, f32) = (8.0, 6.0);
/// The pin's point, down from the box's top.
pub const PIN_TIP: f32 = 23.5;
/// How far the pin's dark edge reaches past it.
pub const PIN_EDGE: f32 = 1.5;
/// The hole in the pin's head.
pub const PIN_HOLE: f32 = 2.2;
/// The Go button's triangle: its square and middle-to-point.
pub const GO: (f32, f32) = (18.0, 7.0);
/// The map button's folded map: its box.
pub const MAP_ICON: (f32, f32) = (16.0, 14.0);

/// One of the shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sprite {
    /// The arrow.
    Pointer,
    /// The arrow's dark edge.
    PointerEdge,
    /// The arrow's lit left half.
    PointerLit,
    /// The player on the map.
    Marker,
    /// Its dark edge.
    MarkerEdge,
    /// The dial's ring.
    Ring,
    /// The dial's ticks.
    Ticks,
    /// The dial's middle dot.
    Hub,
    /// The destination pin.
    Pin,
    /// Its dark edge.
    PinEdge,
    /// The hole in its head.
    PinHole,
    /// The Go button's triangle.
    Go,
    /// The map button's folded map.
    MapIcon,
}

impl Sprite {
    /// Every shape.
    pub const ALL: [Self; 13] = [
        Self::Pointer,
        Self::PointerEdge,
        Self::PointerLit,
        Self::Marker,
        Self::MarkerEdge,
        Self::Ring,
        Self::Ticks,
        Self::Hub,
        Self::Pin,
        Self::PinEdge,
        Self::PinHole,
        Self::Go,
        Self::MapIcon,
    ];

    /// The plugin's name for its image.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Pointer => "goarrow.pointer",
            Self::PointerEdge => "goarrow.pointer-edge",
            Self::PointerLit => "goarrow.pointer-lit",
            Self::Marker => "goarrow.marker",
            Self::MarkerEdge => "goarrow.marker-edge",
            Self::Ring => "goarrow.dial-ring",
            Self::Ticks => "goarrow.dial-ticks",
            Self::Hub => "goarrow.dial-hub",
            Self::Pin => "goarrow.pin",
            Self::PinEdge => "goarrow.pin-edge",
            Self::PinHole => "goarrow.pin-hole",
            Self::Go => "goarrow.go",
            Self::MapIcon => "goarrow.map-icon",
        }
    }

    /// The size it is shown at, canvas pixels.
    #[must_use]
    pub fn size(self) -> (f32, f32) {
        match self {
            Self::Pointer | Self::PointerEdge | Self::PointerLit => (POINTER_BOX, POINTER_BOX),
            Self::Marker | Self::MarkerEdge => (MARKER_BOX, MARKER_BOX),
            Self::Ring | Self::Ticks => (DIAL_BOX, DIAL_BOX),
            Self::Hub => (HUB.0, HUB.0),
            Self::Pin | Self::PinEdge | Self::PinHole => PIN_BOX,
            Self::Go => (GO.0, GO.0),
            Self::MapIcon => MAP_ICON,
        }
    }

    /// Its coverage, [`OVERSAMPLE`] times its size.
    #[must_use]
    pub fn mask(self) -> Mask {
        let k = OVERSAMPLE;
        let px = |v: f32| (v * k).round() as u32;
        let (w, h) = self.size();
        match self {
            Self::Pointer => shapes::arrow(px(w), POINTER_HALF * k, 0.0),
            Self::PointerEdge => shapes::arrow(px(w), POINTER_HALF * k, POINTER_EDGE * k),
            Self::PointerLit => shapes::arrow_left_half(px(w), POINTER_HALF * k),
            Self::Marker => shapes::arrow(px(w), MARKER_HALF * k, 0.0),
            Self::MarkerEdge => shapes::arrow(px(w), MARKER_HALF * k, MARKER_EDGE * k),
            Self::Ring => shapes::ring(px(w), RING.0 * k, RING.1 * k),
            Self::Ticks => shapes::ticks(
                px(w),
                TICK.0 * k,
                TICK.1 * k,
                TICK.2 * k,
                (NORTH_TICK.0 * k, NORTH_TICK.1 * k, NORTH_TICK.2 * k),
            ),
            Self::Hub => shapes::disk(px(w), HUB.1 * k),
            Self::Pin | Self::PinEdge => shapes::pin(
                px(w),
                px(h),
                PIN_HEAD.0 * k,
                PIN_HEAD.1 * k,
                PIN_TIP * k,
                if self == Self::PinEdge {
                    PIN_EDGE * k
                } else {
                    0.0
                },
            ),
            Self::PinHole => {
                let c = (w * k / 2.0, PIN_HEAD.0 * k);
                Mask::rasterise(px(w), px(h), |x, y| {
                    (x - c.0).hypot(y - c.1) <= PIN_HOLE * k
                })
            }
            Self::Go => shapes::play(px(w), GO.1 * k),
            Self::MapIcon => {
                // A map folded in three: the outline zig-zags along the top
                // and bottom, with the two folds.
                let (left, top, right, bottom) = (1.5 * k, 2.0 * k, (w - 1.5) * k, (h - 2.0) * k);
                let third = (right - left) / 3.0;
                let lift = 1.5 * k;
                let outline = [
                    (left, top + lift),
                    (left + third, top),
                    (left + 2.0 * third, top + lift),
                    (right, top),
                    (right, bottom - lift),
                    (left + 2.0 * third, bottom),
                    (left + third, bottom - lift),
                    (left, bottom),
                    (left, top + lift),
                ];
                let mut segments: Vec<_> = outline.windows(2).map(|p| (p[0], p[1])).collect();
                segments.push(((left + third, top), (left + third, bottom - lift)));
                segments.push((
                    (left + 2.0 * third, top + lift),
                    (left + 2.0 * third, bottom),
                ));
                shapes::strokes(px(w), px(h), &segments, 1.4 * k)
            }
        }
    }
}

/// The shapes the client holds for the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sprites {
    images: [Image; Sprite::ALL.len()],
    tried: bool,
}

impl Sprites {
    /// A shape's image (none until handed over).
    #[must_use]
    pub fn image(&self, sprite: Sprite) -> Image {
        self.images[sprite as usize]
    }

    /// Whether the client holds them all.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.images.iter().all(|i| i.is_valid())
    }

    /// Draws and hands over the shapes the client does not hold yet, once
    /// a window is attached; tried once, and again when a window attaches.
    pub fn ensure(&mut self, host: &mut Host<'_>) {
        if self.tried {
            return;
        }
        self.tried = true;
        if !host.window_attached() {
            return;
        }
        for sprite in Sprite::ALL {
            let slot = &mut self.images[sprite as usize];
            if slot.is_valid() {
                continue;
            }
            let mask = sprite.mask();
            *slot = host.image_rgba(sprite.name(), mask.width, mask.height, &mask.rgba());
        }
    }

    /// A window attached: hand over what is missing at the next tick.
    pub fn on_event(&mut self, event: &Event) {
        if matches!(event, Event::WindowAttached(true)) && !self.ready() {
            self.tried = false;
        }
    }

    /// Draws a shape at its size, its box's middle at `middle`, tinted.
    pub fn draw(&self, p: &mut Painter, sprite: Sprite, middle: (f32, f32), tint: Color) {
        let (w, h) = sprite.size();
        p.image(
            self.image(sprite),
            middle.0 - w / 2.0,
            middle.1 - h / 2.0,
            w,
            h,
            tint,
        );
    }

    /// Draws a shape turned `rotation` radians clockwise and scaled by
    /// `scale` about its box's middle, at `middle`, tinted.
    pub fn draw_turned(
        &self,
        p: &mut Painter,
        sprite: Sprite,
        middle: (f32, f32),
        rotation: f32,
        scale: f32,
        tint: Color,
    ) {
        let (w, h) = sprite.size();
        p.image_transformed(
            self.image(sprite),
            (middle.0 - w / 2.0, middle.1 - h / 2.0, w, h),
            tint,
            rotation,
            (w / 2.0, h / 2.0),
            (scale, scale),
        );
    }
}
