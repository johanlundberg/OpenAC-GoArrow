//! The map: a screen overlay with two views. Dereth (`GoArrowMap.cs`
//! drew on the client's map; this draws its own) looks and works as the
//! original GoArrow's map ([`crate::map_view`] has its rules): the world
//! map under a coordinate grid with labelled gutters, the places as the
//! original's icons with their names, the route (walks as pale lines over
//! a dark one, portals and recalls dashed), the destination and the
//! player's arrow. It zooms in steps of √2 (the wheel about the pointer),
//! pans by dragging, follows the player if asked; a place under the
//! pointer is outlined and named in a tooltip; a click on a place points
//! the arrow there (Shift: the route's start, Ctrl: its end), a click
//! elsewhere sets a map-point destination, a double click centres there,
//! and a right click opens a menu drawn on the map. The dungeon view (`GoArrowDungeonMap.cs` showed downloaded pictures; this
//! draws the game's own floorplan) shows the floors and walls of the
//! dungeon the player is in, the player's floor bright and the others
//! dim, the way to go as a dashed line, and the portals: those in view,
//! and dimmed, those the portal table knows that are not in view yet.
//!
//! The header drags the map; its buttons pick the view, zoom, follow and
//! close. The game's world map is placed with the client map panel's own
//! calibration ([`map_pixel`], [`map_coords`]). It sits behind
//! [`DerethBackground`]: while a window is attached and the map shows, the
//! plugin draws a sharp map of its own from the landscape's terrain
//! ([`crate::terrain_map`]) a little every tick, and the game's map stands
//! in until it is ready.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use openac_plugin::Event;
use openac_plugin::Host;
use openac_plugin::ui::{
    Anchor, Canvas, CanvasSpec, Font, FontWeight, Image, Painter, PointerEvent, rgba,
};
use openac_plugin::world::{Floorplan, ObjectClass, WorldObject};

use crate::ace_portals::AcePortals;
use crate::coords::{Coordinates, METRES_PER_MAP_UNIT};
use crate::distance::format_travel_distance;
use crate::hud::Rgba;
use crate::location::Location;
use crate::map_icons::{MapIcon, MapIcons};
use crate::map_view::{
    self, COORD_FONT_PIXELS, COORD_GUTTER, Hotspot, IconRules, Label, MENU_PAD, MENU_ROW, MapMenu,
    MenuAction, MenuTarget, Rect, TOOLTIP_FONT_PIXELS, TOOLTIP_HEIGHT, TOOLTIP_MS, TOOLTIP_OFFSET,
    TOOLTIP_TEXT_MAX, Zoom,
};
use crate::plugin::{GoArrow, Here};
use crate::route::{Route, RouteStepKind};
use crate::settings::{MAP_ZOOM_RANGE, Settings};
use crate::sprites::{PIN_BOX, PIN_TIP, Sprite, Sprites};
use crate::terrain_map::{TerrainBuild, TerrainMap};

/// The canvas's id.
pub const CANVAS_ID: &str = "goarrow.map";
/// Its size, pixels.
pub const SIZE: (i32, i32) = (586, 560);
/// The header's height.
pub const HEADER: f32 = 36.0;
/// The footer's height.
pub const FOOTER: f32 = 24.0;
/// The map area: x, y, width, height.
pub const BODY: (f32, f32, f32, f32) = (0.0, HEADER, 586.0, 560.0 - HEADER - FOOTER);

/// The game's world map (257 x 267 pixels).
pub const WORLD_MAP: u32 = 0x0600_127D;
/// Its size, pixels.
pub const WORLD_MAP_SIZE: (f32, f32) = (257.0, 267.0);
/// Where markers fall on it: x0, x1, y0, y1 (the client map panel's
/// marker area).
pub const MARKER_AREA: (f64, f64, f64, f64) = (6.0, 247.0, 8.0, 258.0);

/// A walk's colour in the dungeon view's legend of old; the dungeon
/// view's portal marks use [`PORTAL_COLOR`].
pub const WALK_COLOR: Rgba = Rgba(240, 180, 60, 255);
/// A portal's colour in the dungeon view.
pub const PORTAL_COLOR: Rgba = Rgba(180, 140, 255, 255);
/// The route's walks on the Dereth view: the original's pale green line.
pub const ROUTE_COLOR: Rgba = Rgba(204, 255, 204, 255);
/// Its width, pixels.
pub const ROUTE_WIDTH: f32 = 1.75;
/// The dark line under a walk.
pub const ROUTE_BACKGROUND: Rgba = Rgba(0, 0, 0, 192);
/// Its width, pixels.
pub const ROUTE_BACKGROUND_WIDTH: f32 = 4.0;
/// A portal hop or recall: the original's pink dashed line, no dark line
/// under it.
pub const ROUTE_PORTAL_COLOR: Rgba = Rgba(255, 102, 255, 255);
/// Its width, pixels.
pub const ROUTE_PORTAL_WIDTH: f32 = 1.25;
/// Its dashes: drawn and left out, in line widths (a GDI+ dash).
pub const ROUTE_DASH: (f32, f32) = (3.0, 1.0);
/// The arrowhead at the end of each walk on the route (a portal hop has
/// none): its width and length, in line widths.
pub const ROUTE_ARROW: (f32, f32) = (4.0, 5.0);
/// The shortest route line drawn, pixels.
pub const ROUTE_MIN_SEGMENT: f32 = 5.0;
/// The coordinate grid's lines.
pub const GRID_COLOR: Rgba = Rgba(0, 0, 0, 96);
/// The coordinate gutters' shade.
pub const GUTTER_COLOR: Rgba = Rgba(0, 0, 0, 96);
/// The coordinate labels and the place names.
pub const LABEL_COLOR: Rgba = Rgba(255, 255, 255, 255);
/// The glow under a place's name: the name drawn a pixel left, up, right
/// and down in this.
pub const LABEL_GLOW: Rgba = Rgba(0, 0, 0, 192);
/// The outline round the place under the pointer.
pub const HOVER_COLOR: Rgba = Rgba(255, 215, 0, 255);
/// The tooltip's face.
pub const TOOLTIP_BACKGROUND: Rgba = Rgba(0, 0, 0, 160);
/// The tooltip's edge.
pub const TOOLTIP_BORDER: Rgba = Rgba(239, 165, 16, 255);
/// A portal the portal table knows that is not in view yet.
pub const UNSEEN_PORTAL_COLOR: Rgba = Rgba(180, 140, 255, 110);
/// The player's colour.
pub const PLAYER_COLOR: Rgba = Rgba(94, 214, 232, 255);
/// The destination pin's colour.
pub const PIN_COLOR: Rgba = Rgba(232, 88, 74, 255);
/// The marks' dark edge.
pub const EDGE_COLOR: Rgba = Rgba(10, 14, 19, 255);
/// The hole in the pin's head.
pub const PIN_HOLE_COLOR: Rgba = Rgba(255, 255, 255, 235);

/// Where map coordinates fall on the world map, pixels (the client map
/// panel's `marker_position`).
#[must_use]
pub fn map_pixel(ns: f64, ew: f64) -> (f64, f64) {
    let (x0, x1, y0, y1) = MARKER_AREA;
    (
        x0 + (x1 - x0 + 1.0) * (ew * 10.0 + 1024.0) / 2048.0,
        y0 + (y1 - y0 + 1.0) * (2047.0 - (ns * 10.0 + 1024.0)) / 2048.0,
    )
}

/// The map coordinates (north-south, east-west) under a world map pixel:
/// the inverse of [`map_pixel`].
#[must_use]
pub fn map_coords(x: f64, y: f64) -> (f64, f64) {
    let (x0, x1, y0, y1) = MARKER_AREA;
    let ew = ((x - x0) * 2048.0 / (x1 - x0 + 1.0) - 1024.0) / 10.0;
    let ns = (2047.0 - (y - y0) * 2048.0 / (y1 - y0 + 1.0) - 1024.0) / 10.0;
    (ns, ew)
}

/// What the Dereth view draws under the route: an image of the world in
/// its own pixels, and how map coordinates fall on it.
pub trait DerethBackground: std::fmt::Debug {
    /// Its size, pixels.
    fn size(&self) -> (f32, f32);
    /// Map coordinates to its pixels.
    fn to_pixel(&self, ns: f64, ew: f64) -> (f64, f64);
    /// Its pixels to map coordinates (north-south, east-west).
    fn to_coords(&self, x: f64, y: f64) -> (f64, f64);
    /// Draws it with its top-left at `origin`, `scale` canvas pixels to
    /// each of its own.
    fn draw(&self, painter: &mut Painter, origin: (f32, f32), scale: f32);
    /// Its pixels per map unit east-west.
    fn pixels_per_map_unit(&self) -> f64 {
        self.to_pixel(0.0, 1.0).0 - self.to_pixel(0.0, 0.0).0
    }
}

/// The game's own world map.
#[derive(Debug, Clone, Copy)]
pub struct GameWorldMap {
    /// The client's art, none when it has none.
    pub image: Image,
}

impl DerethBackground for GameWorldMap {
    fn size(&self) -> (f32, f32) {
        WORLD_MAP_SIZE
    }

    fn to_pixel(&self, ns: f64, ew: f64) -> (f64, f64) {
        map_pixel(ns, ew)
    }

    fn to_coords(&self, x: f64, y: f64) -> (f64, f64) {
        map_coords(x, y)
    }

    fn draw(&self, painter: &mut Painter, origin: (f32, f32), scale: f32) {
        let (w, h) = self.size();
        if self.image.is_valid() {
            painter.image(
                self.image,
                origin.0,
                origin.1,
                w * scale,
                h * scale,
                rgba(255, 255, 255, 255),
            );
        } else {
            painter.fill_rect(
                origin.0,
                origin.1,
                w * scale,
                h * scale,
                rgba(24, 30, 36, 255),
            );
            painter.stroke_rect(
                origin.0,
                origin.1,
                w * scale,
                h * scale,
                rgba(80, 90, 100, 255),
                1.0,
            );
        }
    }
}

/// How the Dereth view lies on the canvas: the background pixel at the
/// body's middle, and canvas pixels per background pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DerethView {
    /// The background pixel shown at the body's middle.
    pub center: (f64, f64),
    /// Canvas pixels per background pixel.
    pub scale: f64,
}

impl DerethView {
    /// The view for a zoom ([`Zoom::multiplier`]: canvas pixels per pixel
    /// of the original's 10-per-map-unit map, whatever the background's
    /// own pixels) centred on map coordinates, moved so the map's edges
    /// stay outside the body (or centred on the map where it is the
    /// smaller), as the original kept them.
    #[must_use]
    pub fn new(background: &dyn DerethBackground, zoom: f64, ns: f64, ew: f64) -> Self {
        let scale = zoom * map_view::PIXELS_PER_MAP_UNIT / background.pixels_per_map_unit();
        let (w, h) = background.size();
        let center = background.to_pixel(ns, ew);
        let keep = |at: f64, side: f32, body: f32| {
            let (side, half) = (f64::from(side), f64::from(body) / 2.0 / scale);
            if side * scale <= f64::from(body) {
                side / 2.0
            } else {
                at.clamp(half, side - half)
            }
        };
        Self {
            center: (keep(center.0, w, BODY.2), keep(center.1, h, BODY.3)),
            scale,
        }
    }

    /// A background pixel to the canvas.
    #[must_use]
    pub fn to_canvas(&self, pixel: (f64, f64)) -> (f32, f32) {
        let (mx, my) = body_middle();
        (
            (f64::from(mx) + (pixel.0 - self.center.0) * self.scale) as f32,
            (f64::from(my) + (pixel.1 - self.center.1) * self.scale) as f32,
        )
    }

    /// A canvas point to a background pixel.
    #[must_use]
    pub fn to_pixel(&self, point: (f32, f32)) -> (f64, f64) {
        let (mx, my) = body_middle();
        (
            self.center.0 + f64::from(point.0 - mx) / self.scale,
            self.center.1 + f64::from(point.1 - my) / self.scale,
        )
    }
}

fn body_middle() -> (f32, f32) {
    (BODY.0 + BODY.2 / 2.0, BODY.1 + BODY.3 / 2.0)
}

fn body_contains(at: (f32, f32)) -> bool {
    Rect::new(BODY.0, BODY.1, BODY.2, BODY.3).contains(at)
}

/// A pointer event's Shift bit.
pub const MODIFIER_SHIFT: u8 = 1;
/// Its Ctrl bit.
pub const MODIFIER_CONTROL: u8 = 2;

/// The header's buttons: x, width.
pub const DERETH_TAB: (f32, f32) = (8.0, 62.0);
/// The Dungeon view's tab.
pub const DUNGEON_TAB: (f32, f32) = (72.0, 70.0);
/// Zoom out.
pub const ZOOM_OUT: (f32, f32) = (402.0, 24.0);
/// Zoom in.
pub const ZOOM_IN: (f32, f32) = (456.0, 24.0);
/// Follow me.
pub const FOLLOW: (f32, f32) = (486.0, 66.0);
/// Close.
pub const CLOSE: (f32, f32) = (556.0, 24.0);

/// A header button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapButton {
    /// The Dereth view.
    Dereth,
    /// The dungeon view.
    Dungeon,
    /// Zoom out.
    ZoomOut,
    /// Zoom in.
    ZoomIn,
    /// Follow the player.
    Follow,
    /// Hide the map.
    Close,
}

/// The header button under a point, if any.
#[must_use]
pub fn header_button(x: f32, y: f32) -> Option<MapButton> {
    if !(6.0..HEADER - 6.0).contains(&y) {
        return None;
    }
    [
        (DERETH_TAB, MapButton::Dereth),
        (DUNGEON_TAB, MapButton::Dungeon),
        (ZOOM_OUT, MapButton::ZoomOut),
        (ZOOM_IN, MapButton::ZoomIn),
        (FOLLOW, MapButton::Follow),
        (CLOSE, MapButton::Close),
    ]
    .into_iter()
    .find(|((left, width), _)| x >= *left && x < left + width)
    .map(|(_, button)| button)
}

/// Which view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapViewKind {
    /// Dereth.
    #[default]
    Dereth,
    /// The dungeon the player is in.
    Dungeon,
}

/// The map's colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapPalette {
    /// The face.
    pub background: Rgba,
    /// The map area.
    pub field: Rgba,
    /// Edges.
    pub border: Rgba,
    /// Text.
    pub text: Rgba,
    /// Captions.
    pub muted: Rgba,
    /// Highlights.
    pub accent: Rgba,
    /// A selected tab.
    pub selected: Rgba,
}

impl MapPalette {
    /// The brass palette, for Classic.
    pub const CLASSIC: Self = Self {
        background: Rgba(34, 31, 27, 240),
        field: Rgba(24, 22, 18, 255),
        border: Rgba(76, 67, 53, 255),
        text: Rgba(233, 226, 213, 255),
        muted: Rgba(178, 165, 142, 255),
        accent: Rgba(201, 166, 101, 255),
        selected: Rgba(81, 68, 45, 255),
    };

    /// A theme's colours.
    #[must_use]
    pub fn of(theme: &ac_plugin_abi::ui::ThemeInfo) -> Self {
        theme.palette.as_ref().map_or(Self::CLASSIC, |p| Self {
            background: Rgba::of(&p.background, 240),
            field: Rgba::of(&p.field, 255),
            border: Rgba::of(&p.border, 255),
            text: Rgba::of(&p.text, 255),
            muted: Rgba::of(&p.muted, 255),
            accent: Rgba::of(&p.accent, 255),
            selected: Rgba::of(&p.selected, 255),
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum Press {
    Header {
        grab: (f32, f32),
        moved: bool,
        button: Option<MapButton>,
    },
    Body {
        grab: (f32, f32),
        last: (f32, f32),
        moved: bool,
        /// 1 left (a click acts), 3 middle (it only drags).
        button: u8,
    },
    /// The right button, down on the body: the menu opens where it is let
    /// go.
    Right,
    /// Down on a menu item.
    Menu(usize),
}

#[derive(Debug, Clone, PartialEq)]
struct Key {
    view: MapViewKind,
    player: (i64, i64, i32),
    route: u64,
    zoom: i64,
    center: (i64, i64),
    dungeon: (u32, i64, (i64, i64)),
    follow: bool,
    pointer: Option<(i32, i32)>,
    palette: MapPalette,
    target: String,
    terrain: Option<u32>,
    sprites: Sprites,
    icons: MapIcons,
    places: (usize, usize, bool),
    from: Option<String>,
    hover: Option<(String, i32, i32)>,
    tooltip: bool,
    menu: Option<(String, Option<usize>, (i32, i32))>,
}

impl Press {
    /// The button it was made with.
    fn button(self) -> u8 {
        match self {
            Self::Header { .. } | Self::Menu(_) => 1,
            Self::Body { button, .. } => button,
            Self::Right => 2,
        }
    }
}

/// The place under the pointer: its name, where it answers the pointer,
/// and where the tooltip went when it came under it.
#[derive(Debug, Clone, PartialEq)]
struct Active {
    location: Rc<Location>,
    hit: Rect,
    tooltip_at: (f32, f32),
    since_ms: u64,
}

/// The fonts the Dereth view writes in, and the sizes of the texts it has
/// measured (by font; 0 is the interface font, which stands in for a font
/// the client refused).
#[derive(Debug, Default)]
struct MapFonts {
    fonts: Vec<((u32, bool), Font)>,
    sizes: HashMap<u32, HashMap<String, (f32, f32)>>,
    measured: usize,
}

/// What the places' layout depends on: the view's middle and scale, the
/// zoom factor, the places (where their list is and how long), the
/// favourites and "Show locations at every zoom".
#[derive(Debug, Clone, PartialEq)]
struct LayoutKey {
    center: (u64, u64),
    scale: u64,
    factor: u64,
    places: (usize, usize),
    favourites: Vec<String>,
    every_zoom: bool,
}

/// The most text sizes kept before they are measured afresh.
const MAX_MEASURES: usize = 8192;

impl MapFonts {
    /// The client's bundled font at a whole pixel size, regular or the
    /// semi-bold the client has; asked for once.
    fn font(&mut self, host: &mut Host<'_>, pixels: f32, bold: bool) -> Font {
        let key = (pixels.round() as u32, bold);
        if let Some((_, font)) = self.fonts.iter().find(|(k, _)| *k == key) {
            return *font;
        }
        let weight = if bold {
            FontWeight::SemiBold
        } else {
            FontWeight::Regular
        };
        let font = host.font_bundled(key.0 as f32, weight);
        self.fonts.push((key, font));
        font
    }

    /// A text's width and line height in a font.
    fn measure(&mut self, host: &mut Host<'_>, font: Font, text: &str) -> (f32, f32) {
        if let Some(size) = self.sizes.get(&font.handle).and_then(|s| s.get(text)) {
            return *size;
        }
        if self.measured >= MAX_MEASURES {
            self.sizes.clear();
            self.measured = 0;
        }
        let size = if font.is_valid() {
            host.font_measure(font, text)
        } else {
            host.measure_text(text)
        };
        self.sizes
            .entry(font.handle)
            .or_default()
            .insert(text.to_owned(), size);
        self.measured += 1;
        size
    }
}

/// A character of a stacked coordinate label, and its width and line
/// height.
type StackedChar = (char, f32, f32);

/// What the Dereth view draws this paint, laid out before it is painted
/// (the texts need the client to measure them).
#[derive(Debug, Default)]
struct DerethFrame {
    hotspots: Rc<Vec<Hotspot>>,
    labels: Vec<(Label, Font)>,
    north_south: Vec<(f32, Vec<StackedChar>)>,
    east_west: Vec<(f32, String, f32, f32)>,
    coord_font: Font,
    tooltip: Option<(Rect, String, Font)>,
}

/// Where the sharp map stands.
#[derive(Debug)]
enum Terrain {
    /// Not started.
    Waiting,
    /// Being drawn.
    Building(Box<TerrainBuild>),
    /// Drawn and handed to the client.
    Ready(Image),
    /// Not drawn (no game files, or the client refused the image): the
    /// game's map stays.
    Failed,
}

/// The dungeon view's own zoom and pan, kept per landblock.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DungeonView {
    landblock: u32,
    zoom: f64,
    center: Option<(f64, f64)>,
}

/// The map overlay.
#[derive(Debug)]
pub struct MapOverlay {
    canvas: Canvas,
    background: Box<dyn DerethBackground>,
    window_attached: bool,
    terrain: Terrain,
    offset: (f32, f32),
    palette: MapPalette,
    visible: bool,
    view: MapViewKind,
    in_dungeon: bool,
    follow: bool,
    zoom: Zoom,
    center: (f64, f64),
    dungeon: DungeonView,
    player_dungeon: Option<u32>,
    dungeon_chosen: bool,
    floorplan: Option<Floorplan>,
    press: Option<Press>,
    pointer_at: Option<(f32, f32)>,
    hover_at: Option<(f32, f32)>,
    active: Option<Active>,
    last_click: Option<((f32, f32), u64)>,
    menu: Option<MapMenu>,
    menu_hover: Option<usize>,
    fonts: MapFonts,
    layout: RefCell<Option<(LayoutKey, Rc<Vec<Hotspot>>)>>,
    player: Option<(f64, f64)>,
    portals: Vec<MapPortal>,
    portals_read: Option<(u32, u64)>,
    shown: Option<Key>,
    last_paint: u64,
    ticks: u64,
}

/// The fewest ticks between two paints (about 8 a second).
pub const PAINT_TICKS: u64 = 8;
/// The fewest ticks between two reads of the portals nearby (a second).
pub const PORTAL_TICKS: u64 = 67;

impl MapOverlay {
    /// Registers the map at its saved place.
    ///
    /// # Errors
    /// The client refused the canvas.
    pub fn open(host: &mut Host<'_>, settings: &Settings) -> Result<Self, String> {
        let offset = (
            settings.dungeon_offset_x as f32,
            settings.dungeon_offset_y as f32,
        );
        let canvas = host.register_canvas(&CanvasSpec {
            canvas_id: CANVAS_ID.to_owned(),
            width: SIZE.0,
            height: SIZE.1,
            anchor: Anchor::TopLeft,
            offset,
            start_visible: settings.map_visible,
            accepts_pointer_input: !settings.hud_click_through,
        })?;
        canvas.listen_pointer(host, !settings.hud_click_through);
        let image = host.image_client_art(WORLD_MAP);
        Ok(Self {
            canvas,
            background: Box::new(GameWorldMap { image }),
            window_attached: host.window_attached(),
            terrain: Terrain::Waiting,
            offset,
            palette: MapPalette::of(&host.theme()),
            visible: settings.map_visible,
            view: MapViewKind::Dereth,
            in_dungeon: false,
            follow: false,
            zoom: Zoom::new(
                settings
                    .map_zoom_factor
                    .unwrap_or_else(|| map_view::factor_from_legacy_zoom(settings.map_zoom)),
                (BODY.2, BODY.3),
            ),
            center: (
                settings.map_center_north_south,
                settings.map_center_east_west,
            ),
            dungeon: DungeonView {
                landblock: 0,
                zoom: 1.0,
                center: None,
            },
            player_dungeon: None,
            dungeon_chosen: false,
            floorplan: None,
            press: None,
            pointer_at: None,
            hover_at: None,
            active: None,
            last_click: None,
            menu: None,
            menu_hover: None,
            fonts: MapFonts::default(),
            layout: RefCell::new(None),
            player: None,
            portals: Vec::new(),
            portals_read: None,
            shown: None,
            last_paint: 0,
            ticks: 0,
        })
    }

    /// The canvas's number.
    #[must_use]
    pub fn canvas_id(&self) -> u32 {
        self.canvas.id()
    }

    /// Where it is, pixels from the screen's top-left corner.
    #[must_use]
    pub fn offset(&self) -> (f32, f32) {
        self.offset
    }

    /// The view shown.
    #[must_use]
    pub fn view(&self) -> MapViewKind {
        self.view
    }

    /// The Dereth view's zoom: canvas pixels per pixel of the original's
    /// 10-per-map-unit map.
    #[must_use]
    pub fn zoom(&self) -> f64 {
        self.zoom.multiplier()
    }

    /// The Dereth view's zoom factor (the zoom is √2 to its power).
    #[must_use]
    pub fn zoom_factor(&self) -> f64 {
        self.zoom.factor()
    }

    /// The right-click menu shown, if any.
    #[must_use]
    pub fn menu(&self) -> Option<&MapMenu> {
        self.menu.as_ref()
    }

    /// The place under the pointer, if any.
    #[must_use]
    pub fn hovered(&self) -> Option<&Rc<Location>> {
        self.active.as_ref().map(|a| &a.location)
    }

    /// The landblock the dungeon view shows (0 before any).
    #[must_use]
    pub fn shown_dungeon(&self) -> u32 {
        self.dungeon.landblock
    }

    /// The Dereth view's middle, map coordinates (north-south, east-west).
    #[must_use]
    pub fn center(&self) -> (f64, f64) {
        self.center
    }

    /// Whether it follows the player.
    #[must_use]
    pub fn follows(&self) -> bool {
        self.follow
    }

    /// The Dereth view as drawn now.
    #[must_use]
    pub fn dereth_view(&self) -> DerethView {
        DerethView::new(
            self.background.as_ref(),
            self.zoom.multiplier(),
            self.center.0,
            self.center.1,
        )
    }

    /// The sharp map's image, once the client has it.
    #[must_use]
    pub fn terrain_image(&self) -> Option<Image> {
        match self.terrain {
            Terrain::Ready(image) => Some(image),
            _ => None,
        }
    }

    /// How far the sharp map is drawn, 0 to 1, while it is being drawn.
    #[must_use]
    pub fn terrain_progress(&self) -> Option<f64> {
        match &self.terrain {
            Terrain::Building(build) => Some(build.progress()),
            _ => None,
        }
    }

    /// Replaces the Dereth background (a sharper map, say).
    pub fn set_background(&mut self, background: Box<dyn DerethBackground>) {
        self.background = background;
        self.shown = None;
    }

    /// Shows or hides it (a menu open closes).
    pub fn set_visible(&mut self, host: &mut Host<'_>, visible: bool) {
        self.visible = visible;
        self.canvas.set_visible(host, visible);
        self.close_menu();
        self.active = None;
        self.hover_at = None;
        self.shown = None;
    }

    /// The theme changed, or a window attached or left.
    pub fn on_event(&mut self, event: &Event) {
        match event {
            Event::ThemeChanged(theme) => {
                self.palette = MapPalette::of(theme);
                self.shown = None;
            }
            Event::WindowAttached(attached) => {
                self.window_attached = *attached;
                // The client keeps a plugin's images across windows; a map
                // it did not take is tried again with the new window.
                if *attached
                    && (matches!(self.terrain, Terrain::Failed)
                        || matches!(self.terrain, Terrain::Ready(image) if !image.is_valid()))
                {
                    self.terrain = Terrain::Waiting;
                }
            }
            _ => {}
        }
    }

    /// The sharp map is drawn only for a window, and only while the map
    /// shows: a headless session never needs it.
    fn may_build(&self) -> bool {
        self.visible && self.window_attached
    }

    /// This tick's share of drawing the sharp map: what is left of the
    /// tick's work budget after the plugin's other jobs.
    fn step_terrain(&mut self, app: &GoArrow, host: &mut Host<'_>) {
        if !self.may_build() {
            return;
        }
        if matches!(self.terrain, Terrain::Waiting) {
            self.terrain = Terrain::Building(Box::new(TerrainBuild::new(None)));
        }
        let Terrain::Building(build) = &mut self.terrain else {
            return;
        };
        if let Some(image) = build.step(host, app.tick_started_ms()) {
            self.terrain = Terrain::Ready(image);
            self.set_background(Box::new(TerrainMap { image }));
        } else if build.failed() {
            self.terrain = Terrain::Failed;
            self.shown = None;
        }
    }

    fn dungeon_landblock(here: &Here) -> Option<u32> {
        let cell = here.position.cell_id;
        (here.available
            && !here.portal_space
            && !here.position.is_outdoor
            && cell & 0xFFFF >= 0x100)
            .then_some(cell >> 16)
    }

    fn update_dungeon(&mut self, app: &GoArrow, host: &mut Host<'_>, here: &Here) {
        let landblock = Self::dungeon_landblock(here);
        let now_in = landblock.is_some();
        // The player's dungeon is loaded when the player comes into it; a
        // dungeon chosen from the Dereth view's menu stays until then.
        if landblock != self.player_dungeon {
            self.player_dungeon = landblock;
            if let Some(landblock) = landblock
                && self.dungeon.landblock != landblock
            {
                self.load_dungeon(host, landblock);
            }
        }
        if now_in != self.in_dungeon {
            self.in_dungeon = now_in;
            self.dungeon_chosen = false;
            // Entering or leaving a dungeon picks the view; a choice made
            // in the header holds until the next.
            self.set_view(
                if now_in && app.settings().dungeon_map_visible && self.floorplan.is_some() {
                    MapViewKind::Dungeon
                } else {
                    MapViewKind::Dereth
                },
            );
        }
        if self.view == MapViewKind::Dungeon
            && !app.settings().dungeon_map_visible
            && !now_in
            && !self.dungeon_chosen
        {
            self.set_view(MapViewKind::Dereth);
        }
    }

    /// Every change of view goes here: a menu open closes, and the place
    /// under the pointer is let go (neither belongs to the other view).
    fn set_view(&mut self, view: MapViewKind) {
        if self.view != view {
            self.view = view;
            self.close_menu();
            self.active = None;
            self.shown = None;
        }
    }

    /// Reads a dungeon's floorplan for the dungeon view; false when the
    /// client has none.
    fn load_dungeon(&mut self, host: &mut Host<'_>, landblock: u32) -> bool {
        self.dungeon = DungeonView {
            landblock,
            zoom: 1.0,
            center: None,
        };
        // The client takes the landblock in a cell id's high 16 bits.
        self.floorplan = host
            .dungeon_floorplan(landblock << 16)
            .filter(|f| !f.layers.is_empty());
        self.portals_read = None;
        self.floorplan.is_some()
    }

    fn key(&self, app: &GoArrow, here: &Here, now_ms: u64) -> Key {
        let p = here.position;
        let round = |v: f64, by: f64| (v * by).round() as i64;
        Key {
            view: self.view,
            player: (
                round(p.north_south, 240.0),
                round(p.east_west, 240.0),
                (p.heading_degrees / 5.0).round() as i32,
            ),
            route: crate::ground::route_signature(app),
            zoom: round(self.zoom.factor(), 100.0),
            center: (round(self.center.0, 1000.0), round(self.center.1, 1000.0)),
            dungeon: (
                self.dungeon.landblock,
                round(self.dungeon.zoom, 100.0),
                self.dungeon
                    .center
                    .map_or((0, 0), |c| (round(c.0, 10.0), round(c.1, 10.0))),
            ),
            follow: self.follow,
            pointer: self.pointer_at.map(|(x, y)| (x as i32, y as i32)),
            palette: self.palette,
            target: app.destination().target_name().to_owned(),
            terrain: self.terrain_progress().map(|p| (p * 100.0).floor() as u32),
            sprites: *app.sprites(),
            icons: *app.map_icons(),
            places: (
                app.database().all_locations().len(),
                app.settings().favorite_destinations.len(),
                app.settings().show_locations_all_zooms,
            ),
            from: app.route_from().map(|l| l.name.clone()),
            hover: self
                .active
                .as_ref()
                .map(|a| (a.location.name.clone(), a.hit.x as i32, a.hit.y as i32)),
            tooltip: self.tooltip_shows(now_ms),
            menu: self.menu.as_ref().map(|m| {
                (
                    m.title.clone(),
                    self.menu_hover,
                    (m.rect.x as i32, m.rect.y as i32),
                )
            }),
        }
    }

    /// Whether the hovered place's tooltip shows: for [`TOOLTIP_MS`] after
    /// the place came under the pointer, as the original's did.
    fn tooltip_shows(&self, now_ms: u64) -> bool {
        self.view == MapViewKind::Dereth
            && self.menu.is_none()
            && self
                .active
                .as_ref()
                .is_some_and(|a| now_ms.saturating_sub(a.since_ms) < TOOLTIP_MS)
    }

    /// One tick: picks the view and paints when something it shows
    /// changed, at most every [`PAINT_TICKS`].
    pub fn tick(&mut self, app: &mut GoArrow, host: &mut Host<'_>, here: &Here) {
        self.ticks += 1;
        self.update_dungeon(app, host, here);
        if !self.visible {
            return;
        }
        self.step_terrain(app, host);
        self.player = here
            .outdoors()
            .then_some((here.position.north_south, here.position.east_west));
        if self.follow
            && let Some(player) = self.player
            && self.center != player
        {
            self.center = player;
            // The map moved under the pointer: what is under it now, as the
            // original checked at every paint.
            self.rehover(app, host.clock().monotonic_ms);
        }
        if self.shown.is_some() && self.ticks - self.last_paint < PAINT_TICKS {
            return;
        }
        let key = self.key(app, here, host.clock().monotonic_ms);
        if self.shown.as_ref() == Some(&key) {
            return;
        }
        // A paint the client refused is tried again next tick.
        if self.paint(app, host, here) {
            self.shown = Some(key);
            self.last_paint = self.ticks;
        }
    }

    /// Pointer input: the header drags the map and has its buttons; the
    /// body pans (left or middle button), zooms with the wheel, and a
    /// click acts on the place under it (or sets a map-point destination);
    /// a double click centres; the right button opens a menu. Hover and
    /// leave (API 1.6) outline the place under the pointer and light the
    /// menu's items.
    pub fn pointer(&mut self, app: &mut GoArrow, host: &mut Host<'_>, event: &PointerEvent) {
        let at = (event.x, event.y);
        let now = host.clock().monotonic_ms;
        match event.kind {
            PointerEvent::DOWN => {
                self.hover_at = Some(at);
                self.press(app, host, event, now);
            }
            // A release ends only the press of its own button; another
            // button's goes by.
            PointerEvent::UP if self.press.is_some_and(|p| p.button() != event.button) => {}
            PointerEvent::UP | PointerEvent::MOVE => {
                self.drag_or_release(app, host, event);
                self.hover(app, Some(at), now);
            }
            // The wheel zooms over the map only, not the header or footer.
            PointerEvent::WHEEL if body_contains(at) => {
                self.hover_at = Some(at);
                // One zoom factor a notch, as the original's wheel.
                let steps = f64::from(event.wheel_delta);
                // Ctrl or Shift zooms about the middle, as the original did.
                let about =
                    (event.modifiers & (MODIFIER_SHIFT | MODIFIER_CONTROL) == 0).then_some(at);
                self.zoom_by(app, host, steps, about);
            }
            PointerEvent::CANCELLED => {
                match self.press {
                    Some(Press::Header { moved: true, .. }) => {
                        let (x, y) = self.offset;
                        app.update_settings(host, |s| {
                            s.dungeon_offset_x = f64::from(x);
                            s.dungeon_offset_y = f64::from(y);
                        });
                    }
                    // A pan cut short is kept, as one let go.
                    Some(Press::Body { moved: true, .. }) => self.save_view(app, host),
                    _ => {}
                }
                self.press = None;
                self.pointer_at = None;
                self.shown = None;
            }
            PointerEvent::HOVER => self.hover(app, Some(at), now),
            PointerEvent::LEAVE => self.hover(app, None, now),
            _ => {}
        }
    }

    /// A button went down.
    fn press(&mut self, app: &mut GoArrow, host: &mut Host<'_>, event: &PointerEvent, now: u64) {
        let at = (event.x, event.y);
        // A second button pressed while one is held is ignored.
        if self.press.is_some() {
            return;
        }
        self.shown = None;
        if let Some(menu) = &self.menu {
            if event.button == 1
                && let Some(item) = menu.item_at(at)
            {
                self.press = Some(Press::Menu(item));
                return;
            }
            // A press off the menu closes it, and does nothing else on the
            // map.
            self.close_menu();
            if event.y >= HEADER {
                self.press = None;
                return;
            }
        }
        self.press = match event.button {
            1 if event.y < HEADER => Some(Press::Header {
                grab: at,
                moved: false,
                button: header_button(event.x, event.y),
            }),
            1 => {
                let double = map_view::is_double_click(self.last_click, at, now);
                self.last_click = (!double).then_some((at, now));
                if double && self.view == MapViewKind::Dereth {
                    // The second press of a double click centres there and
                    // is done: it neither drags nor clicks.
                    self.center_on(at);
                    self.save_view(app, host);
                    self.rehover(app, now);
                    None
                } else {
                    self.pointer_at = Some(at);
                    Some(Press::Body {
                        grab: at,
                        last: at,
                        moved: false,
                        button: 1,
                    })
                }
            }
            3 if event.y >= HEADER => Some(Press::Body {
                grab: at,
                last: at,
                moved: false,
                button: 3,
            }),
            2 if event.y >= HEADER => Some(Press::Right),
            _ => None,
        };
    }

    /// The pointer moved with a button down, or the button came up.
    fn drag_or_release(&mut self, app: &mut GoArrow, host: &mut Host<'_>, event: &PointerEvent) {
        let at = (event.x, event.y);
        match self.press {
            Some(Press::Header {
                grab,
                mut moved,
                button,
            }) => {
                let (dx, dy) = (at.0 - grab.0, at.1 - grab.1);
                if !moved && dx.abs().max(dy.abs()) >= crate::hud::CLICK_SLOP {
                    moved = true;
                }
                if moved && (dx != 0.0 || dy != 0.0) {
                    self.offset = (self.offset.0 + dx, self.offset.1 + dy);
                    self.canvas.set_offset(host, self.offset.0, self.offset.1);
                }
                self.press = Some(Press::Header {
                    grab,
                    moved,
                    button,
                });
                if event.kind == PointerEvent::UP {
                    self.press = None;
                    if moved {
                        let (x, y) = self.offset;
                        app.update_settings(host, |s| {
                            s.dungeon_offset_x = f64::from(x);
                            s.dungeon_offset_y = f64::from(y);
                        });
                    } else if let Some(button) = button {
                        self.press_button(app, host, button);
                    }
                }
            }
            Some(Press::Body {
                grab,
                last,
                mut moved,
                button,
            }) => {
                if !moved
                    && (at.0 - grab.0).abs().max((at.1 - grab.1).abs()) >= crate::hud::CLICK_SLOP
                {
                    moved = true;
                }
                if moved {
                    self.pan(at.0 - last.0, at.1 - last.1);
                }
                self.pointer_at = (button == 1).then_some(at);
                self.press = Some(Press::Body {
                    grab,
                    last: if moved { at } else { last },
                    moved,
                    button,
                });
                if event.kind == PointerEvent::UP {
                    self.press = None;
                    self.pointer_at = None;
                    if moved {
                        self.save_view(app, host);
                    } else if button == 1 {
                        self.click(app, host, grab, event.modifiers);
                    }
                }
            }
            Some(Press::Right) if event.kind == PointerEvent::UP => {
                self.press = None;
                if (BODY.1..BODY.1 + BODY.3).contains(&at.1) {
                    self.open_menu(app, host, at);
                }
            }
            Some(Press::Menu(item)) if event.kind == PointerEvent::UP => {
                self.press = None;
                let chosen = self
                    .menu
                    .as_ref()
                    .filter(|m| m.item_at(at) == Some(item))
                    .map(|m| (m.target.clone(), m.items[item].1.clone()));
                self.close_menu();
                if let Some((target, action)) = chosen {
                    self.run_menu(app, host, &target, &action);
                }
            }
            Some(Press::Right | Press::Menu(_)) | None => {}
        }
        self.shown = None;
    }

    /// The pointer is over the canvas at `at` (none: it left): the place
    /// under it, or the menu item.
    fn hover(&mut self, app: &GoArrow, at: Option<(f32, f32)>, now: u64) {
        self.hover_at = at;
        let menu_hover = match (&self.menu, at) {
            (Some(menu), Some(at)) => menu.item_at(at),
            _ => None,
        };
        if menu_hover != self.menu_hover {
            self.menu_hover = menu_hover;
            self.shown = None;
        }
        let over_menu = matches!((&self.menu, at), (Some(m), Some(at)) if m.rect.contains(at));
        let found = at
            .filter(|at| self.view == MapViewKind::Dereth && !over_menu && body_contains(*at))
            .and_then(|at| {
                let hotspots = self.hotspots(app);
                map_view::hotspot_at(&hotspots, at).map(|h| (Rc::clone(&h.location), h.hit, at))
            });
        let same = match (&self.active, &found) {
            (Some(a), Some((location, hit, _))) => {
                Rc::ptr_eq(&a.location, location) && a.hit == *hit
            }
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.active = found.map(|(location, hit, at)| Active {
                location,
                hit,
                tooltip_at: (at.0 + TOOLTIP_OFFSET.0, at.1 + TOOLTIP_OFFSET.1),
                since_ms: now,
            });
            self.shown = None;
        }
    }

    /// What the pointer was over when the map moved under it: found again
    /// where the pointer last was.
    fn rehover(&mut self, app: &GoArrow, now: u64) {
        if let Some(at) = self.hover_at {
            self.hover(app, Some(at), now);
        }
    }

    /// The places drawn in the Dereth view now, last on top: laid out again
    /// only when the view, the places or the settings they follow changed
    /// (the original kept the last paint's hotspots so).
    fn hotspots(&self, app: &GoArrow) -> Rc<Vec<Hotspot>> {
        let view = self.dereth_view();
        let settings = app.settings();
        let places = app.database().all_locations();
        let key = LayoutKey {
            center: (view.center.0.to_bits(), view.center.1.to_bits()),
            scale: view.scale.to_bits(),
            factor: self.zoom.factor().to_bits(),
            places: (places.as_ptr() as usize, places.len()),
            favourites: settings.favorite_destinations.clone(),
            every_zoom: settings.show_locations_all_zooms,
        };
        if let Some((held, hotspots)) = self.layout.borrow().as_ref()
            && *held == key
        {
            return Rc::clone(hotspots);
        }
        let bg = self.background.as_ref();
        let corner = |at: (f32, f32)| {
            let pixel = view.to_pixel(at);
            let (ns, ew) = bg.to_coords(pixel.0, pixel.1);
            Coordinates::new(ns, ew)
        };
        let hotspots = Rc::new(map_view::icon_layout(
            app.database().all_locations(),
            IconRules {
                zoom: self.zoom,
                every_zoom: settings.show_locations_all_zooms,
            },
            corner((BODY.0, BODY.1)),
            corner((BODY.0 + BODY.2, BODY.1 + BODY.3)),
            &|c| view.to_canvas(bg.to_pixel(c.ns, c.ew)),
            &|l| l.is_favorite || settings.is_favorite(&l.name),
        ));
        *self.layout.borrow_mut() = Some((key, Rc::clone(&hotspots)));
        hotspots
    }

    /// Whether the Dereth view is centred on the player (within 5 pixels
    /// of the middle, as the original judged it); never indoors.
    fn centred_on_player(&self) -> bool {
        self.player.is_some_and(|(ns, ew)| {
            let at = self.canvas_at(ns, ew);
            let (mx, my) = body_middle();
            (at.0 - mx).abs() < 5.0 && (at.1 - my).abs() < 5.0
        })
    }

    fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.shown = None;
        }
        self.menu_hover = None;
    }

    /// Opens the menu for what is under a point: the place, or the point.
    fn open_menu(&mut self, app: &GoArrow, host: &mut Host<'_>, at: (f32, f32)) {
        self.close_menu();
        if self.view != MapViewKind::Dereth {
            return;
        }
        let hotspots = self.hotspots(app);
        let mut menu = match map_view::hotspot_at(&hotspots, at) {
            Some(hotspot) => {
                let location = Rc::clone(&hotspot.location);
                // A dungeon's id is its landblock; the menu offers its map
                // when the client has a floorplan for it.
                let dungeon = u32::try_from(location.dungeon_id)
                    .ok()
                    .map(|id| id & 0xFFFF)
                    .filter(|id| *id != 0)
                    .filter(|id| {
                        host.dungeon_floorplan(id << 16)
                            .is_some_and(|f| !f.layers.is_empty())
                    });
                MapMenu::for_location(location, dungeon)
            }
            None => {
                let (ns, ew) = self.coords_at(at);
                MapMenu::for_point(Coordinates::new(ns, ew))
            }
        };
        let width = std::iter::once(menu.title.as_str())
            .chain(menu.items.iter().map(|(text, _)| text.as_str()))
            .map(|text| host.measure_text(text).0)
            .fold(0.0, f32::max);
        menu.place(at, width, Rect::new(BODY.0, BODY.1, BODY.2, BODY.3));
        self.menu = Some(menu);
        self.active = None;
        self.shown = None;
    }

    /// Does what a menu item says, as the original's menu did.
    fn run_menu(
        &mut self,
        app: &mut GoArrow,
        host: &mut Host<'_>,
        target: &MenuTarget,
        action: &MenuAction,
    ) {
        match (action, target) {
            (MenuAction::ShowDetails, MenuTarget::Location(location)) => {
                app.show_place_details(host, location);
            }
            (MenuAction::PointArrow, MenuTarget::Location(location)) => {
                app.point_arrow_at(host, Rc::clone(location));
            }
            (MenuAction::PointArrow, MenuTarget::Point(c)) => {
                app.point_arrow_at_coordinates(host, *c);
            }
            (MenuAction::StartRoute, MenuTarget::Location(location)) => {
                app.set_route_start(host, Rc::clone(location));
            }
            (MenuAction::StartRoute, MenuTarget::Point(c)) => {
                app.set_route_start_at(host, *c);
            }
            (MenuAction::EndRoute, MenuTarget::Location(location)) => {
                app.set_route_end(host, Rc::clone(location));
            }
            (MenuAction::EndRoute, MenuTarget::Point(c)) => {
                app.set_route_end_at(host, *c);
            }
            (MenuAction::ShowDungeonMap(landblock), _) => {
                if self.load_dungeon(host, *landblock) {
                    self.set_view(MapViewKind::Dungeon);
                    self.dungeon_chosen = true;
                }
            }
            (MenuAction::JumpToExit(exit), _) => {
                self.follow = false;
                self.center = (exit.ns, exit.ew);
                self.center = self.clamped_center();
                self.save_view(app, host);
            }
            (MenuAction::ShowDetails, MenuTarget::Point(_)) => {}
        }
        self.shown = None;
    }

    /// The Dereth view's middle where it is drawn: the saved middle, moved
    /// in from the map's edges.
    fn clamped_center(&self) -> (f64, f64) {
        let view = self.dereth_view();
        self.background.to_coords(view.center.0, view.center.1)
    }

    /// Centres the Dereth view on a canvas point (by whole pixels, as the
    /// original moved it) and stops following.
    fn center_on(&mut self, at: (f32, f32)) {
        let (mx, my) = body_middle();
        let view = self.dereth_view();
        let pixel = (
            view.center.0 - f64::from((mx - at.0).round()) / view.scale,
            view.center.1 - f64::from((my - at.1).round()) / view.scale,
        );
        self.center = self.background.to_coords(pixel.0, pixel.1);
        self.center = self.clamped_center();
        self.follow = false;
        self.shown = None;
    }

    fn pan(&mut self, dx: f32, dy: f32) {
        self.follow = false;
        match self.view {
            MapViewKind::Dereth => {
                let view = self.dereth_view();
                let pixel = (
                    view.center.0 - f64::from(dx) / view.scale,
                    view.center.1 - f64::from(dy) / view.scale,
                );
                self.center = self.background.to_coords(pixel.0, pixel.1);
                self.center = self.clamped_center();
            }
            MapViewKind::Dungeon => {
                if let Some(fit) = self.dungeon_fit() {
                    let scale = fit.scale * self.dungeon.zoom;
                    let c = self.dungeon.center.unwrap_or(fit.center);
                    self.dungeon.center =
                        Some((c.0 - f64::from(dx) / scale, c.1 + f64::from(dy) / scale));
                }
            }
        }
    }

    fn save_view(&self, app: &mut GoArrow, host: &mut Host<'_>) {
        let (ns, ew) = self.center;
        let factor = self.zoom.factor();
        app.update_settings(host, |s| {
            s.map_center_north_south = ns;
            s.map_center_east_west = ew;
            s.map_zoom_factor = Some(factor);
        });
    }

    /// Zooms `steps` factors in (out when negative): the Dereth view keeps
    /// the point under `about` where it is (none: the middle; following,
    /// the player stays in the middle); the dungeon view zooms by one.
    fn zoom_by(
        &mut self,
        app: &mut GoArrow,
        host: &mut Host<'_>,
        steps: f64,
        about: Option<(f32, f32)>,
    ) {
        match self.view {
            MapViewKind::Dereth => {
                // Following and centred on the player, the player stays in
                // the middle; otherwise the point under `about` stays.
                let anchor = about
                    .filter(|_| !(self.follow && self.centred_on_player()))
                    .map(|at| (at, self.coords_at(at)));
                self.zoom = self.zoom.stepped(steps, (BODY.2, BODY.3));
                if let Some((at, (ns, ew))) = anchor {
                    let scale = self.dereth_view().scale;
                    let (mx, my) = body_middle();
                    let pixel = self.background.to_pixel(ns, ew);
                    let middle = (
                        pixel.0 - f64::from(at.0 - mx) / scale,
                        pixel.1 - f64::from(at.1 - my) / scale,
                    );
                    self.center = self.background.to_coords(middle.0, middle.1);
                }
                self.center = self.clamped_center();
                self.save_view(app, host);
                self.rehover(app, host.clock().monotonic_ms);
            }
            MapViewKind::Dungeon => {
                self.dungeon.zoom =
                    (self.dungeon.zoom + steps.signum()).clamp(MAP_ZOOM_RANGE.0, MAP_ZOOM_RANGE.1);
            }
        }
        self.shown = None;
    }

    fn press_button(&mut self, app: &mut GoArrow, host: &mut Host<'_>, button: MapButton) {
        match button {
            MapButton::Dereth => {
                self.set_view(MapViewKind::Dereth);
                self.dungeon_chosen = false;
            }
            MapButton::Dungeon => {
                if self.floorplan.is_some() {
                    self.set_view(MapViewKind::Dungeon);
                }
            }
            MapButton::ZoomOut => self.zoom_by(app, host, -1.0, None),
            MapButton::ZoomIn => self.zoom_by(app, host, 1.0, None),
            MapButton::Follow => self.follow = !self.follow,
            // The app cannot reach this map while it handles the pointer
            // (it is lifted out for the call), so it hides itself here and
            // the app saves the setting.
            MapButton::Close => {
                self.set_visible(host, false);
                app.set_map_visible(host, false);
            }
        }
        self.close_menu();
        self.shown = None;
    }

    /// A left click on the Dereth view: on a place, points the arrow there
    /// (Shift: the route's start; Ctrl: its end), as the original's map
    /// did; elsewhere sets a map-point destination there.
    fn click(&mut self, app: &mut GoArrow, host: &mut Host<'_>, at: (f32, f32), modifiers: u8) {
        if self.view != MapViewKind::Dereth {
            return;
        }
        let hotspots = self.hotspots(app);
        if let Some(hotspot) = map_view::hotspot_at(&hotspots, at) {
            let location = Rc::clone(&hotspot.location);
            if modifiers & MODIFIER_CONTROL != 0 {
                app.set_route_end(host, location);
            } else if modifiers & MODIFIER_SHIFT != 0 {
                app.set_route_start(host, location);
            } else {
                app.point_arrow_at(host, location);
            }
            return;
        }
        let (ns, ew) = self.coords_at(at);
        let ns = (ns * 10.0).round() / 10.0;
        let ew = (ew * 10.0).round() / 10.0;
        app.set_coordinate_destination(host, ns, ew, None);
    }

    /// The map coordinates under a canvas point in the Dereth view.
    #[must_use]
    pub fn coords_at(&self, at: (f32, f32)) -> (f64, f64) {
        let pixel = self.dereth_view().to_pixel(at);
        self.background.to_coords(pixel.0, pixel.1)
    }

    /// Where map coordinates fall on the canvas in the Dereth view.
    #[must_use]
    pub fn canvas_at(&self, ns: f64, ew: f64) -> (f32, f32) {
        self.dereth_view()
            .to_canvas(self.background.to_pixel(ns, ew))
    }

    fn dungeon_fit(&self) -> Option<DungeonFit> {
        let plan = self.floorplan.as_ref()?;
        let (min, max) = (plan.bounds_min, plan.bounds_max);
        let (w, h) = (
            f64::from(max.0 - min.0).max(1.0),
            f64::from(max.1 - min.1).max(1.0),
        );
        let scale = (f64::from(BODY.2) / w).min(f64::from(BODY.3) / h) * 0.92;
        Some(DungeonFit {
            center: (
                f64::from(min.0 + max.0) / 2.0,
                f64::from(min.1 + max.1) / 2.0,
            ),
            scale,
        })
    }

    /// Paints; false when the client refused the paint.
    fn paint(&mut self, app: &GoArrow, host: &mut Host<'_>, here: &Here) -> bool {
        if self.view == MapViewKind::Dungeon
            && self.portals_read.is_none_or(|(landblock, at)| {
                landblock != self.dungeon.landblock || self.ticks - at >= PORTAL_TICKS
            })
        {
            self.portals =
                dungeon_portals(host, self.dungeon.landblock, app.navigator().ace_portals());
            self.portals_read = Some((self.dungeon.landblock, self.ticks));
        }
        let portals = if self.view == MapViewKind::Dungeon {
            self.portals.clone()
        } else {
            Vec::new()
        };
        let footer = self.footer(app, here);
        let frame = if self.view == MapViewKind::Dereth {
            let now = host.clock().monotonic_ms;
            self.dereth_frame(app, host, now)
        } else {
            DerethFrame::default()
        };
        self.canvas
            .paint(host, |p| {
                let palette = self.palette;
                p.clear(rgba(0, 0, 0, 0));
                p.fill_rect(
                    0.0,
                    0.0,
                    SIZE.0 as f32,
                    SIZE.1 as f32,
                    palette.background.c(),
                );
                p.fill_rect(BODY.0, BODY.1, BODY.2, BODY.3, palette.field.c());
                p.push_clip(BODY.0, BODY.1, BODY.2, BODY.3);
                match self.view {
                    MapViewKind::Dereth => self.paint_dereth(p, app, here, &frame),
                    MapViewKind::Dungeon => self.paint_dungeon(p, app, here, &portals),
                }
                p.pop_clip();
                self.paint_header(p, app);
                p.text(
                    &footer,
                    (8.0, SIZE.1 as f32 - FOOTER + 4.0),
                    palette.muted.c(),
                    false,
                );
                legend(p, &palette);
                p.stroke_rect(
                    0.0,
                    0.0,
                    SIZE.0 as f32,
                    SIZE.1 as f32,
                    palette.border.c(),
                    1.0,
                );
            })
            .is_ok()
    }

    fn footer(&self, app: &GoArrow, here: &Here) -> String {
        if let (Some(at), MapViewKind::Dereth) = (self.pointer_at, self.view) {
            let (ns, ew) = self.coords_at(at);
            let point = Coordinates::new(ns, ew).round(1);
            let away = app.origin_coordinates(here).map_or_else(String::new, |o| {
                format!(" · {} away", format_travel_distance(o.distance_to(point)))
            });
            return format!("Cursor {point}{away} · release to go there");
        }
        let drawing = self
            .terrain_progress()
            .filter(|_| self.view == MapViewKind::Dereth)
            .map(|p| format!("Drawing the map... {:.0}%", (p * 100.0).floor()));
        if !here.available {
            return drawing.map_or_else(
                || "Not in the world".to_owned(),
                |d| format!("{d} · Not in the world"),
            );
        }
        match self.view {
            MapViewKind::Dereth => match drawing {
                // The heading gives way to the progress, so the line ends
                // before the legend.
                Some(d) => format!("{d} · You {}", here.position.coordinates().round(1)),
                None => format!(
                    "You {} · heading {:.0}°",
                    here.position.coordinates().round(1),
                    here.position.heading_degrees
                ),
            },
            MapViewKind::Dungeon => {
                let local = openac_plugin::world::Position {
                    cell_id: here.position.cell_id,
                    east_west: here.position.east_west,
                    north_south: here.position.north_south,
                    elevation: here.position.elevation,
                    heading_degrees: here.position.heading_degrees,
                    is_outdoor: here.position.is_outdoor,
                }
                .landblock_local();
                format!(
                    "Dungeon 0x{:04X} · floor {:.0} m · wheel: zoom · drag: pan",
                    self.dungeon.landblock, local[2]
                )
            }
        }
    }

    fn paint_header(&self, p: &mut Painter, app: &GoArrow) {
        let palette = self.palette;
        let tab = |p: &mut Painter, (x, w): (f32, f32), label: &str, on: bool| {
            if on {
                p.fill_rect(x, 6.0, w, HEADER - 12.0, palette.selected.c());
            }
            p.stroke_rect(x, 6.0, w, HEADER - 12.0, palette.border.c(), 1.0);
            p.text(
                label,
                (x + 8.0, 10.0),
                if on {
                    palette.text.c()
                } else {
                    palette.muted.c()
                },
                false,
            );
        };
        tab(p, DERETH_TAB, "Dereth", self.view == MapViewKind::Dereth);
        tab(p, DUNGEON_TAB, "Dungeon", self.view == MapViewKind::Dungeon);
        let title = match self.view {
            MapViewKind::Dereth if app.destination().has_destination() => {
                format!("To {}", app.destination().target_name())
            }
            MapViewKind::Dereth => "Dereth".to_owned(),
            MapViewKind::Dungeon => format!("Dungeon 0x{:04X}", self.dungeon.landblock),
        };
        let title: String = title.chars().take(30).collect();
        p.text(&title, (150.0, 10.0), palette.text.c(), false);
        let zoom = match self.view {
            MapViewKind::Dereth => self.zoom.multiplier(),
            MapViewKind::Dungeon => self.dungeon.zoom,
        };
        tab(p, ZOOM_OUT, "-", false);
        p.text(
            &format!("{}x", zoom_text(zoom)),
            (ZOOM_OUT.0 + 30.0, 10.0),
            palette.text.c(),
            false,
        );
        tab(p, ZOOM_IN, "+", false);
        tab(p, FOLLOW, "Follow me", self.follow);
        tab(p, CLOSE, "x", false);
    }

    /// The view's corners, map coordinates: top-left and bottom-right.
    fn corners(&self) -> (Coordinates, Coordinates) {
        let corner = |at: (f32, f32)| {
            let (ns, ew) = self.coords_at(at);
            Coordinates::new(ns, ew)
        };
        (
            corner((BODY.0, BODY.1)),
            corner((BODY.0 + BODY.2, BODY.1 + BODY.3)),
        )
    }

    /// Lays out what the Dereth view writes: the places' names, the
    /// coordinate labels and the tooltip, measured by the client.
    fn dereth_frame(&mut self, app: &GoArrow, host: &mut Host<'_>, now: u64) -> DerethFrame {
        let hotspots = self.hotspots(app);
        let multiplier = self.zoom.multiplier();
        let (regular_px, bold_px) = map_view::label_font_pixels(multiplier);
        let regular = self.fonts.font(host, regular_px, false);
        let bold = self.fonts.font(host, bold_px, true);
        let coord_font = self.fonts.font(host, COORD_FONT_PIXELS, false);
        let tooltip_font = self
            .tooltip_shows(now)
            .then(|| self.fonts.font(host, TOOLTIP_FONT_PIXELS, false));
        let delta = map_view::coord_tick_delta(multiplier);
        let (top_left, bottom_right) = self.corners();
        let view = self.dereth_view();
        let bg = self.background.as_ref();
        let to = |ns: f64, ew: f64| view.to_canvas(bg.to_pixel(ns, ew));
        let fonts = &mut self.fonts;
        let labels = map_view::place_labels(&hotspots, self.zoom.factor(), &mut |text, is_bold| {
            fonts.measure(host, if is_bold { bold } else { regular }, text)
        })
        .into_iter()
        .map(|label| {
            let font = if label.bold { bold } else { regular };
            (label, font)
        })
        .collect();
        let (north_south, east_west) = map_view::grid_ticks(top_left, bottom_right, delta);
        let north_south = north_south
            .into_iter()
            .map(|v| {
                let label = map_view::tick_label(v, delta, ('N', 'S'));
                let chars = label
                    .chars()
                    .map(|c| {
                        let (w, h) = fonts.measure(host, coord_font, &c.to_string());
                        (c, w, h)
                    })
                    .collect();
                (to(f64::from(v), top_left.ew).1, chars)
            })
            .collect();
        let east_west = east_west
            .into_iter()
            .map(|v| {
                let label = map_view::tick_label(v, delta, ('E', 'W'));
                let (w, h) = fonts.measure(host, coord_font, &label);
                (to(top_left.ns, f64::from(v)).0, label, w, h)
            })
            .collect();
        let tooltip = match (tooltip_font, &self.active) {
            (Some(font), Some(active)) => {
                let (text, width) = fit_text(&active.location.name, TOOLTIP_TEXT_MAX, |t| {
                    fonts.measure(host, font, t).0
                });
                // Inside the map area, which the body's clip keeps it to.
                let w = width.ceil() + 2.0;
                let x = active
                    .tooltip_at
                    .0
                    .min(BODY.0 + BODY.2 - w - 1.0)
                    .max(BODY.0);
                let y = active
                    .tooltip_at
                    .1
                    .min(BODY.1 + BODY.3 - TOOLTIP_HEIGHT - 1.0)
                    .max(BODY.1);
                Some((Rect::new(x, y, w, TOOLTIP_HEIGHT), text, font))
            }
            _ => None,
        };
        DerethFrame {
            hotspots,
            labels,
            north_south,
            east_west,
            coord_font,
            tooltip,
        }
    }

    fn paint_dereth(&self, p: &mut Painter, app: &GoArrow, here: &Here, frame: &DerethFrame) {
        let view = self.dereth_view();
        let bg = self.background.as_ref();
        let origin = view.to_canvas((0.0, 0.0));
        bg.draw(p, origin, view.scale as f32);
        let to = |c: Coordinates| view.to_canvas(bg.to_pixel(c.ns, c.ew));
        let icons = app.map_icons();
        let scale = map_view::icon_scale(self.zoom.multiplier());
        let body = Rect::new(BODY.0, BODY.1, BODY.2, BODY.3);
        let g = COORD_GUTTER;
        let inner = Rect::new(body.x + g, body.y + g, body.w - 2.0 * g, body.h - 2.0 * g);
        // The gutters' shade, then the grid inside them.
        for strip in gutters(body) {
            p.fill_rect(strip.x, strip.y, strip.w, strip.h, GUTTER_COLOR.c());
        }
        for (y, _) in &frame.north_south {
            if *y > inner.y && *y < inner.bottom() {
                p.line(
                    (inner.x, *y),
                    (inner.right() - 1.0, *y),
                    GRID_COLOR.c(),
                    1.0,
                );
            }
        }
        for (x, ..) in &frame.east_west {
            if *x > inner.x && *x < inner.right() {
                p.line(
                    (*x, inner.y),
                    (*x, inner.bottom() - 1.0),
                    GRID_COLOR.c(),
                    1.0,
                );
            }
        }
        // The places, then their names over a dark glow.
        for hotspot in frame.hotspots.iter() {
            let r = hotspot.rect;
            icons.draw(p, hotspot.icon, (r.x, r.y, r.w, r.h));
        }
        for (label, font) in &frame.labels {
            let (x, y) = (label.rect.x, label.rect.y);
            for (dx, dy) in [(-1.0, 0.0), (0.0, -1.0), (1.0, 0.0), (0.0, 1.0)] {
                write(p, *font, &label.text, (x + dx, y + dy), LABEL_GLOW);
            }
            write(p, *font, &label.text, (x, y), LABEL_COLOR);
        }
        // The route: lines only, as the original's map draws it.
        if let Some(route) = app.destination().current_route() {
            let player = here.outdoors().then(|| here.position.coordinates());
            for line in &route_lines(route, player) {
                draw_route_line(p, to(line.from), to(line.to), line.hop);
            }
        }
        if let Some(target) = app.destination().target()
            && target.has_coordinates()
        {
            let at = to(target.coords);
            if body.contains(at) && !draw_icon(p, icons, MapIcon::ArrowDest2, at, scale) {
                pin(p, app.sprites(), at);
            }
        }
        self.paint_coordinates(p, frame, body);
        if let Some(active) = &self.active {
            outline(p, active.hit, HOVER_COLOR);
        }
        if here.available {
            marker(
                p,
                app.sprites(),
                to(here.position.coordinates()),
                here.position.heading_degrees,
            );
        }
        if let Some((rect, text, font)) = &frame.tooltip {
            p.fill_rect(rect.x, rect.y, rect.w, rect.h, TOOLTIP_BACKGROUND.c());
            p.stroke_rect(rect.x, rect.y, rect.w, rect.h, TOOLTIP_BORDER.c(), 1.0);
            write(p, *font, text, (rect.x + 2.0, rect.y + 2.0), LABEL_COLOR);
        }
        if let Some(menu) = &self.menu {
            self.paint_menu(p, menu);
        }
    }

    /// The gutters' coordinate labels: east-west across the top and the
    /// bottom, north-south down the left and the right, a character under
    /// another (the original turned them; the painter cannot turn text).
    fn paint_coordinates(&self, p: &mut Painter, frame: &DerethFrame, body: Rect) {
        let g = COORD_GUTTER;
        let font = frame.coord_font;
        for strip in [
            Rect::new(body.x, body.y, body.w, g),
            Rect::new(body.x, body.bottom() - g, body.w, g),
        ] {
            p.push_clip(strip.x, strip.y, strip.w, strip.h);
            let top = strip.y == body.y;
            for (x, label, w, h) in &frame.east_west {
                let y = if top { body.y } else { body.bottom() - h };
                write(p, font, label, (x - w / 2.0, y), LABEL_COLOR);
            }
            p.pop_clip();
        }
        for left in [body.x, body.right() - g] {
            p.push_clip(left, body.y + g, g, body.h - 2.0 * g);
            for (y, chars) in &frame.north_south {
                let total: f32 = chars.iter().map(|(c, _, h)| stacked_step(*c, *h)).sum();
                let mut at = y - total / 2.0;
                for (c, w, h) in chars {
                    let step = stacked_step(*c, *h);
                    write(
                        p,
                        font,
                        &c.to_string(),
                        (left + (g - w) / 2.0, at - (h - step) / 2.0),
                        LABEL_COLOR,
                    );
                    at += step;
                }
            }
            p.pop_clip();
        }
    }

    /// The right-click menu: the title row, then the items, the one under
    /// the pointer lit.
    fn paint_menu(&self, p: &mut Painter, menu: &MapMenu) {
        let palette = self.palette;
        let r = menu.rect;
        p.fill_rect(r.x, r.y, r.w, r.h, palette.background.c());
        p.text(
            &menu.title,
            (r.x + MENU_PAD, r.y + 2.0),
            palette.accent.c(),
            false,
        );
        p.line(
            (r.x, r.y + MENU_ROW),
            (r.right(), r.y + MENU_ROW),
            palette.border.c(),
            1.0,
        );
        for (i, (text, _)) in menu.items.iter().enumerate() {
            let row = menu.row(i);
            if self.menu_hover == Some(i) {
                p.fill_rect(row.x, row.y, row.w, row.h, palette.selected.c());
            }
            p.text(
                text,
                (row.x + MENU_PAD, row.y + 2.0),
                palette.text.c(),
                false,
            );
        }
        p.stroke_rect(r.x, r.y, r.w, r.h, palette.border.c(), 1.0);
    }

    fn paint_dungeon(&self, p: &mut Painter, app: &GoArrow, here: &Here, portals: &[MapPortal]) {
        let palette = self.palette;
        let (Some(plan), Some(fit)) = (self.floorplan.as_ref(), self.dungeon_fit()) else {
            p.text(
                "No dungeon floorplan here.",
                (BODY.0 + 16.0, BODY.1 + 16.0),
                palette.muted.c(),
                false,
            );
            return;
        };
        let scale = fit.scale * self.dungeon.zoom;
        // A dungeon chosen from the Dereth view's menu may not be the
        // player's: then the player, the way and following are not drawn.
        let player_in = Self::dungeon_landblock(here) == Some(self.dungeon.landblock);
        let follow_center = (self.follow && player_in).then(|| {
            let local = local_of(here);
            (f64::from(local[0]), f64::from(local[1]))
        });
        let center = follow_center.or(self.dungeon.center).unwrap_or(fit.center);
        let (mx, my) = body_middle();
        let to = |x: f32, y: f32| -> (f32, f32) {
            (
                (f64::from(mx) + (f64::from(x) - center.0) * scale) as f32,
                (f64::from(my) - (f64::from(y) - center.1) * scale) as f32,
            )
        };
        let player = local_of(here);
        let floor = plan
            .layers
            .iter()
            .enumerate()
            .min_by(|a, b| {
                (a.1.z - player[2])
                    .abs()
                    .total_cmp(&(b.1.z - player[2]).abs())
            })
            .map_or(0, |(i, _)| i);
        let dim = Rgba(palette.border.0, palette.border.1, palette.border.2, 110);
        for (i, layer) in plan.layers.iter().enumerate() {
            if i == floor {
                continue;
            }
            for wall in &layer.walls {
                p.line(
                    to(wall.start.0, wall.start.1),
                    to(wall.end.0, wall.end.1),
                    dim.c(),
                    1.0,
                );
            }
        }
        let layer = &plan.layers[floor];
        for polygon in &layer.floors {
            for pair in polygon.windows(2) {
                p.line(
                    to(pair[0].0, pair[0].1),
                    to(pair[1].0, pair[1].1),
                    palette.border.c(),
                    1.0,
                );
            }
            if let (Some(first), Some(last)) = (polygon.first(), polygon.last()) {
                p.line(
                    to(last.0, last.1),
                    to(first.0, first.1),
                    palette.border.c(),
                    1.0,
                );
            }
        }
        for wall in &layer.walls {
            p.line(
                to(wall.start.0, wall.start.1),
                to(wall.end.0, wall.end.1),
                palette.text.c(),
                2.0,
            );
        }
        let view = app.navigator().view();
        let mut path: Vec<(f32, f32)> = vec![to(player[0], player[1])];
        let points: Vec<[f32; 3]> = match &view.indoor_preview {
            Some(preview) if !preview.points.is_empty() => {
                preview.points.iter().map(|(_, l)| *l).collect()
            }
            _ => view.dungeon_waypoints.iter().map(|w| w.local).collect(),
        };
        if player_in && !points.is_empty() {
            path.extend(points.iter().map(|l| to(l[0], l[1])));
            for pair in path.windows(2) {
                dashed_line(p, pair[0], pair[1], palette.accent, 3.0, 7.0, 6.0);
            }
        }
        // The table's portals not in view first, dim; those in view over
        // them.
        for portal in portals
            .iter()
            .filter(|p| !p.seen)
            .chain(portals.iter().filter(|p| p.seen))
        {
            let at = to(portal.x, portal.y);
            let (fill, text) = if portal.seen {
                (PORTAL_COLOR, palette.text)
            } else {
                (UNSEEN_PORTAL_COLOR, palette.muted)
            };
            p.fill_rect(at.0 - 4.0, at.1 - 4.0, 8.0, 8.0, fill.c());
            p.text(&portal.name, (at.0 + 7.0, at.1 - 8.0), text.c(), true);
        }
        if player_in {
            marker(
                p,
                app.sprites(),
                to(player[0], player[1]),
                here.position.heading_degrees,
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DungeonFit {
    center: (f64, f64),
    scale: f64,
}

fn local_of(here: &Here) -> [f32; 3] {
    openac_plugin::world::Position {
        cell_id: here.position.cell_id,
        east_west: here.position.east_west,
        north_south: here.position.north_south,
        elevation: here.position.elevation,
        heading_degrees: here.position.heading_degrees,
        is_outdoor: here.position.is_outdoor,
    }
    .landblock_local()
}

/// A portal on the dungeon map.
#[derive(Debug, Clone, PartialEq)]
pub struct MapPortal {
    /// Landblock-local metres east.
    pub x: f32,
    /// Landblock-local metres north.
    pub y: f32,
    /// Its name.
    pub name: String,
    /// The client sees it; otherwise only the portal table knows it.
    pub seen: bool,
}

/// The most portals in view the dungeon map draws.
pub const MAX_SEEN_PORTALS: usize = 32;
/// The most portals the dungeon map draws in all.
pub const MAX_MAP_PORTALS: usize = 64;

/// The portals in a landblock: those the client knows, then those the
/// portal table has that are not among them (admin-only ones left out:
/// no player sees them).
fn dungeon_portals(host: &mut Host<'_>, landblock: u32, table: &AcePortals) -> Vec<MapPortal> {
    let live: Vec<WorldObject> = host
        .objects()
        .into_iter()
        .filter(|o| {
            o.object_class == ObjectClass::Portal
                && o.position
                    .as_ref()
                    .is_some_and(|p| p.cell_id >> 16 == landblock)
        })
        .take(MAX_SEEN_PORTALS)
        .collect();
    let mut portals: Vec<MapPortal> = live
        .iter()
        .filter_map(|o| {
            let local = o.position.as_ref()?.landblock_local();
            Some(MapPortal {
                x: local[0],
                y: local[1],
                name: o.name.clone(),
                seen: true,
            })
        })
        .collect();
    for known in table.in_landblock(landblock << 16) {
        if portals.len() >= MAX_MAP_PORTALS {
            break;
        }
        if known.admin_only || live.iter().any(|o| known.is_object(o)) {
            continue;
        }
        portals.push(MapPortal {
            x: known.local[0],
            y: known.local[1],
            name: known.name.clone(),
            seen: false,
        });
    }
    portals
}

/// A dashed straight line: `dash` pixels drawn, `gap` left out.
pub fn dashed_line(
    p: &mut Painter,
    from: (f32, f32),
    to: (f32, f32),
    color: Rgba,
    width: f32,
    dash: f32,
    gap: f32,
) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 0.5 {
        return;
    }
    let (ux, uy) = (dx / length, dy / length);
    let mut at = 0.0;
    while at < length {
        let end = (at + dash).min(length);
        p.line(
            (from.0 + ux * at, from.1 + uy * at),
            (from.0 + ux * end, from.1 + uy * end),
            color.c(),
            width,
        );
        at = end + gap;
    }
}

/// A portal hop: a dashed arc bowing to one side of the straight line.
pub fn dashed_arc(p: &mut Painter, from: (f32, f32), to: (f32, f32), color: Rgba, width: f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let control = (
        (from.0 + to.0) / 2.0 - dy * 0.25,
        (from.1 + to.1) / 2.0 + dx * 0.25,
    );
    let mut points = Vec::with_capacity(17);
    for i in 0..=16 {
        let t = i as f32 / 16.0;
        let u = 1.0 - t;
        points.push((
            u * u * from.0 + 2.0 * u * t * control.0 + t * t * to.0,
            u * u * from.1 + 2.0 * u * t * control.1 + t * t * to.1,
        ));
    }
    for (i, pair) in points.windows(2).enumerate() {
        if i % 2 == 0 {
            p.line(pair[0], pair[1], color.c(), width);
        }
    }
}

/// The destination: a pin whose point is on it, over its dark edge.
fn pin(p: &mut Painter, sprites: &Sprites, at: (f32, f32)) {
    // The box's middle, from its point.
    let middle = (at.0, at.1 - PIN_TIP + PIN_BOX.1 / 2.0);
    sprites.draw(p, Sprite::PinEdge, middle, EDGE_COLOR.c());
    sprites.draw(p, Sprite::Pin, middle, PIN_COLOR.c());
    sprites.draw(p, Sprite::PinHole, middle, PIN_HOLE_COLOR.c());
}

/// The player: the arrow's shape, small, turned to the heading, over its
/// dark edge.
fn marker(p: &mut Painter, sprites: &Sprites, at: (f32, f32), heading_degrees: f32) {
    let rotation = heading_degrees.to_radians();
    sprites.draw_turned(p, Sprite::MarkerEdge, at, rotation, 1.0, EDGE_COLOR.c());
    sprites.draw_turned(p, Sprite::Marker, at, rotation, 1.0, PLAYER_COLOR.c());
}

fn legend(p: &mut Painter, palette: &MapPalette) {
    let y = SIZE.1 as f32 - FOOTER / 2.0;
    let x = SIZE.0 as f32 - 210.0;
    p.line(
        (x, y),
        (x + 18.0, y),
        ROUTE_BACKGROUND.c(),
        ROUTE_BACKGROUND_WIDTH,
    );
    p.line((x, y), (x + 18.0, y), ROUTE_COLOR.c(), ROUTE_WIDTH);
    p.text("walk", (x + 22.0, y - 8.0), palette.muted.c(), false);
    let (on, off) = ROUTE_DASH;
    dashed_line(
        p,
        (x + 64.0, y),
        (x + 84.0, y),
        ROUTE_PORTAL_COLOR,
        ROUTE_PORTAL_WIDTH,
        on * ROUTE_PORTAL_WIDTH,
        off * ROUTE_PORTAL_WIDTH,
    );
    p.text(
        "portal or recall",
        (x + 88.0, y - 8.0),
        palette.muted.c(),
        false,
    );
}

/// One line of the route as the Dereth view draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteLine {
    /// Where it starts.
    pub from: Coordinates,
    /// Where it ends.
    pub to: Coordinates,
    /// A portal hop or recall (dashed), not a walk.
    pub hop: bool,
}

/// The route's lines: each walk from where it starts (the first from the
/// player, `player`, when outdoors) to where it ends, and each portal or
/// recall from where it is used to where it lets out. A walk to or from a
/// hub's made-up point is left out and the hops through the hub are one,
/// from where the route went in to where its exit lets out (GA-60).
#[must_use]
pub fn route_lines(route: &Route, player: Option<Coordinates>) -> Vec<RouteLine> {
    let mut lines = Vec::new();
    let mut first = true;
    // Where the route went into a hub: a hub's portals are listed at a
    // made-up point, so the way through it is one hop from there to where
    // its exit lets out.
    let mut hub_entrance = None;
    for step in route.steps() {
        let is_first = std::mem::replace(&mut first, false);
        match step.kind {
            RouteStepKind::Travel => {
                let from = match player {
                    Some(player) if is_first => player,
                    _ => step.from.coords,
                };
                if from.is_none()
                    || step.to.coords.is_none()
                    || route.hub_at(from).is_some()
                    || route.hub_at(step.to.coords).is_some()
                {
                    continue;
                }
                lines.push(RouteLine {
                    from,
                    to: step.to.coords,
                    hop: false,
                });
            }
            RouteStepKind::Portal | RouteStepKind::Recall => {
                let mut start = step.from.coords;
                let end = if step.from.has_exit_coords() {
                    step.from.exit_coords()
                } else {
                    step.to.coords
                };
                if route.hub_at(start).is_some() {
                    start = hub_entrance.take().unwrap_or(Coordinates::NONE);
                }
                if route.hub_at(end).is_some() {
                    hub_entrance = Some(start);
                    continue;
                }
                if start.is_none() || end.is_none() {
                    continue;
                }
                lines.push(RouteLine {
                    from: start,
                    to: end,
                    hop: true,
                });
            }
        }
    }
    lines
}

/// Draws one route line as the original did: a walk as the pale line over
/// a dark one ending in an arrowhead, a hop dashed with neither; a line
/// shorter than [`ROUTE_MIN_SEGMENT`] is not drawn.
pub fn draw_route_line(p: &mut Painter, from: (f32, f32), to: (f32, f32), hop: bool) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    if length < ROUTE_MIN_SEGMENT {
        return;
    }
    // A portal hop is a plain dashed line: only a walk ends in an arrowhead.
    if hop {
        let width = ROUTE_PORTAL_WIDTH;
        let (on, off) = ROUTE_DASH;
        dashed_line(
            p,
            from,
            to,
            ROUTE_PORTAL_COLOR,
            width,
            on * width,
            off * width,
        );
        return;
    }
    p.line(from, to, ROUTE_BACKGROUND.c(), ROUTE_BACKGROUND_WIDTH);
    let (ux, uy) = (dx / length, dy / length);
    let head = (ROUTE_ARROW.1 * ROUTE_WIDTH).min(length);
    let base = (to.0 - ux * head, to.1 - uy * head);
    p.line(from, base, ROUTE_COLOR.c(), ROUTE_WIDTH);
    arrowhead(
        p,
        to,
        (ux, uy),
        head,
        ROUTE_ARROW.0 * ROUTE_WIDTH,
        ROUTE_COLOR,
    );
}

/// A filled arrowhead, its point at `tip` facing `dir` (a unit vector):
/// thin lines across it from its base to its point.
fn arrowhead(
    p: &mut Painter,
    tip: (f32, f32),
    dir: (f32, f32),
    length: f32,
    base: f32,
    color: Rgba,
) {
    let slices = length.ceil().max(1.0);
    let step = length / slices;
    for i in 0..slices as usize {
        let back = (i as f32 + 0.5) * step;
        let half = base / 2.0 * back / length;
        let middle = (tip.0 - dir.0 * back, tip.1 - dir.1 * back);
        let across = (-dir.1 * half, dir.0 * half);
        p.line(
            (middle.0 - across.0, middle.1 - across.1),
            (middle.0 + across.0, middle.1 + across.1),
            color.c(),
            step + 0.25,
        );
    }
}

/// Draws a map icon at the icon scale, its middle at `at`; false when the
/// client does not hold it.
fn draw_icon(p: &mut Painter, icons: &MapIcons, icon: MapIcon, at: (f32, f32), scale: f32) -> bool {
    let (w, h) = icon.size();
    let (w, h) = (w * scale, h * scale);
    icons.draw(p, icon, (at.0 - w / 2.0, at.1 - h / 2.0, w, h))
}

/// Text in one of the plugin's fonts, or the interface font where the
/// client refused it.
fn write(p: &mut Painter, font: Font, text: &str, at: (f32, f32), color: Rgba) {
    if font.is_valid() {
        p.text_in(font, text, at, color.c(), false);
    } else {
        p.text(text, at, color.c(), false);
    }
}

/// A one-pixel frame just outside a rectangle (the hovered place).
fn outline(p: &mut Painter, r: Rect, color: Rgba) {
    p.fill_rect(r.x - 1.0, r.y - 1.0, r.w + 2.0, 1.0, color.c());
    p.fill_rect(r.x - 1.0, r.bottom(), r.w + 2.0, 1.0, color.c());
    p.fill_rect(r.x - 1.0, r.y, 1.0, r.h, color.c());
    p.fill_rect(r.right(), r.y, 1.0, r.h, color.c());
}

/// The four coordinate gutters inside the body: top and bottom across it,
/// left and right between them.
#[must_use]
pub fn gutters(body: Rect) -> [Rect; 4] {
    let g = COORD_GUTTER;
    [
        Rect::new(body.x, body.y, body.w, g),
        Rect::new(body.x, body.bottom() - g, body.w, g),
        Rect::new(body.x, body.y + g, g, body.h - 2.0 * g),
        Rect::new(body.right() - g, body.y + g, g, body.h - 2.0 * g),
    ]
}

/// How far down the next character of a stacked label goes: most of a
/// line, less for a point.
fn stacked_step(c: char, line_height: f32) -> f32 {
    if c == '.' {
        line_height * 0.35
    } else {
        line_height * 0.7
    }
}

/// A text cut to fit a width, ending in "..." when cut (the original's
/// tooltip trimmed so), and its width.
fn fit_text(text: &str, width: f32, mut measure: impl FnMut(&str) -> f32) -> (String, f32) {
    let full = measure(text);
    if full <= width {
        return (text.to_owned(), full);
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>() + "...";
        let w = measure(&candidate);
        if w <= width {
            return (candidate, w);
        }
    }
    ("...".to_owned(), measure("..."))
}

/// A zoom as the header shows it: to two decimals, without trailing zeros.
fn zoom_text(zoom: f64) -> String {
    let text = format!("{zoom:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Metres between two map coordinates.
#[must_use]
pub fn metres_between(a: Coordinates, b: Coordinates) -> f64 {
    a.distance_to(b) * METRES_PER_MAP_UNIT
}
