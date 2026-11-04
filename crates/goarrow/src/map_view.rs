//! The Dereth view's rules, taken from the original GoArrow's map (VVS
//! Edition `MapHud`, MIT; NOTICE.md): the zoom in steps of √2, the
//! coordinate grid's spacing and its labelled gutters, which places show
//! at which zoom and with which icon, where their names go, what lies
//! under the pointer, the right-click menus and the double click. Pure:
//! [`crate::map`] paints and wires them.
//!
//! The original's map image was 2041 pixels a side, 10 to a map unit
//! ([`MAP_SIZE`], [`PIXELS_PER_MAP_UNIT`]); its zoom is canvas pixels per
//! pixel of that image, whatever image the view draws under it.

use std::f64::consts::SQRT_2;
use std::rc::Rc;

use crate::coords::Coordinates;
use crate::location::{Location, LocationType};
use crate::map_icons::MapIcon;
use crate::text;

/// One zoom step: the zoom is this to the power of the zoom factor.
pub const ZOOM_BASE: f64 = SQRT_2;
/// The largest zoom factor (a zoom of 32).
pub const MAX_ZOOM_FACTOR: f64 = 10.0;
/// The original's map image, pixels a side.
pub const MAP_SIZE: f64 = 2041.0;
/// Its pixels per map unit.
pub const PIXELS_PER_MAP_UNIT: f64 = MAP_SIZE / 204.1;
/// The coordinate gutters' width, pixels, on every side.
pub const COORD_GUTTER: f32 = 15.0;
/// The grid spacings the coordinate grid chooses from, map units.
pub const POSSIBLE_COORD_DELTAS: [f32; 12] = [
    0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0,
];
/// The grid spacing the choice aims at, canvas pixels.
pub const COORD_TICK_PIXELS: f32 = 40.0;

/// The longest gap between the two presses of a double click,
/// milliseconds: the original took the system's double-click time, whose
/// Windows default this is.
pub const DOUBLE_CLICK_MS: u64 = 500;
/// How far apart the two presses may be, pixels either way: the system's
/// double-click size, Windows' default.
pub const DOUBLE_CLICK_SLOP: f32 = 4.0;

/// How long the hover's tooltip stays, milliseconds.
pub const TOOLTIP_MS: u64 = 4000;
/// The tooltip's place from the pointer, pixels right and down.
pub const TOOLTIP_OFFSET: (f32, f32) = (16.0, 16.0);
/// The tooltip text's widest, pixels (longer names end in "...").
pub const TOOLTIP_TEXT_MAX: f32 = 250.0;
/// The tooltip's height, pixels.
pub const TOOLTIP_HEIGHT: f32 = 20.0;

/// The portal kinds that share the portal icon: every portal but a hub
/// (the original's `AnyPortal`, which a hub is not part of).
pub const MAP_PORTALS: LocationType = LocationType(
    LocationType::PORTAL.0
        | LocationType::SETTLEMENT_PORTAL.0
        | LocationType::TOWN_PORTAL.0
        | LocationType::UNDERGROUND_PORTAL.0
        | LocationType::WILDERNESS_PORTAL.0,
);

/// A rectangle: x, y, width, height, canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left.
    pub x: f32,
    /// Top.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Its right edge.
    #[must_use]
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Its bottom edge.
    #[must_use]
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Whether a point is in it: the left and top edges are, the right and
    /// bottom ones are not.
    #[must_use]
    pub fn contains(&self, (x, y): (f32, f32)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Whether two rectangles overlap (touching edges do not).
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        other.x < self.right()
            && self.x < other.right()
            && other.y < self.bottom()
            && self.y < other.bottom()
    }

    /// The whole pixels it touches: its corner truncated, its size rounded
    /// up (a hotspot's rectangle).
    #[must_use]
    pub fn whole_pixels(&self) -> Self {
        Self::new(self.x.trunc(), self.y.trunc(), self.w.ceil(), self.h.ceil())
    }
}

/// The Dereth view's zoom: the zoom factor and the zoom it gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zoom {
    factor: f64,
    multiplier: f64,
}

impl Zoom {
    /// The zoom for a factor in a view of `body` pixels: √2 to the power of
    /// the factor rounded down, at most [`MAX_ZOOM_FACTOR`]; a zoom at which
    /// the map would not cover the view in both directions gives way to the
    /// one that just covers it (and no more than 1), its factor the
    /// logarithm of that.
    #[must_use]
    pub fn new(factor: f64, body: (f32, f32)) -> Self {
        let mut factor = if factor.is_nan() {
            f64::NEG_INFINITY
        } else {
            factor.min(MAX_ZOOM_FACTOR)
        };
        let mut multiplier = zoom_power(factor.floor());
        let (w, h) = (f64::from(body.0), f64::from(body.1));
        let side = MAP_SIZE * multiplier;
        if side < w || side < h {
            let fit = (w / MAP_SIZE).max(h / MAP_SIZE).min(1.0);
            factor = fit.log(ZOOM_BASE).min(MAX_ZOOM_FACTOR);
            multiplier = fit;
        }
        Self { factor, multiplier }
    }

    /// The zoom factor.
    #[must_use]
    pub fn factor(&self) -> f64 {
        self.factor
    }

    /// Canvas pixels per pixel of the original's map (10 per map unit).
    #[must_use]
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// Canvas pixels per map unit.
    #[must_use]
    pub fn pixels_per_map_unit(&self) -> f64 {
        self.multiplier * PIXELS_PER_MAP_UNIT
    }

    /// The zoom `steps` factors in (out when negative).
    #[must_use]
    pub fn stepped(&self, steps: f64, body: (f32, f32)) -> Self {
        Self::new(self.factor + steps, body)
    }
}

/// √2 to a whole power, exactly where it is a power of two.
fn zoom_power(steps: f64) -> f64 {
    if !steps.is_finite() {
        return ZOOM_BASE.powf(steps);
    }
    let n = steps as i32;
    let odd = if n.rem_euclid(2) == 1 { ZOOM_BASE } else { 1.0 };
    2f64.powi(n.div_euclid(2)) * odd
}

/// The zoom factor a saved zoom of the earlier map gives: it zoomed 1 to
/// 16 times the zoom that fitted the whole 2041-pixel map in its 586 x 500
/// view; the nearest factor to the same scale.
#[must_use]
pub fn factor_from_legacy_zoom(zoom: f64) -> f64 {
    (500.0 / MAP_SIZE * zoom.max(1.0)).log(ZOOM_BASE).round()
}

/// The coordinate grid's spacing at a zoom, map units: the original's
/// choice, which walks [`POSSIBLE_COORD_DELTAS`] up while they come closer
/// to [`COORD_TICK_PIXELS`] apart and takes the first that does not (so
/// the one after the closest), or the largest.
#[must_use]
pub fn coord_tick_delta(multiplier: f64) -> f32 {
    let want = COORD_TICK_PIXELS / (PIXELS_PER_MAP_UNIT as f32 * multiplier as f32);
    let mut best = f32::INFINITY;
    for (i, delta) in POSSIBLE_COORD_DELTAS.iter().enumerate() {
        let off = (delta - want).abs();
        if off < best || i == 0 {
            best = off;
            continue;
        }
        return *delta;
    }
    POSSIBLE_COORD_DELTAS[POSSIBLE_COORD_DELTAS.len() - 1]
}

/// A grid line's label: the coordinate without its sign, to as many
/// decimals as the spacing needs (none from 1, one from 0.1, else two),
/// and its side (`N`/`S` or `E`/`W`; zero is `N` and `E`).
#[must_use]
pub fn tick_label(value: f32, delta: f32, sides: (char, char)) -> String {
    let decimals = if delta >= 1.0 {
        0
    } else if f64::from(delta) >= 0.1 {
        1
    } else {
        2
    };
    format!(
        "{}{}",
        text::format_fixed(f64::from(value.abs()), decimals, decimals),
        if value >= 0.0 { sides.0 } else { sides.1 }
    )
}

/// The grid's lines in a view (its top-left and bottom-right corners), as
/// the original's one loop made them: north-south from the bottom edge
/// rounded up to a multiple of `delta`, east-west from the left edge
/// rounded down (so a label cut by the edge still shows), each a step at a
/// time until north-south has passed the top edge rounded down and
/// east-west the right edge rounded up; both lists are as long.
#[must_use]
pub fn grid_ticks(
    top_left: Coordinates,
    bottom_right: Coordinates,
    delta: f32,
) -> (Vec<f32>, Vec<f32>) {
    let d = f64::from(delta);
    let ns = ((bottom_right.ns / d).ceil(), (top_left.ns / d).floor());
    let ew = ((top_left.ew / d).floor(), (bottom_right.ew / d).ceil());
    let steps = (ns.1 - ns.0).max(ew.1 - ew.0);
    if !(0.0..=4096.0).contains(&steps) {
        return (Vec::new(), Vec::new());
    }
    let line = |first: f64, i: i64| ((first + i as f64) * d) as f32;
    (0..=steps as i64)
        .map(|i| (line(ns.0, i), line(ew.0, i)))
        .unzip()
}

/// The icons' scale at a zoom: (zoom / √2) to the power ¼.
#[must_use]
pub fn icon_scale(multiplier: f64) -> f32 {
    (multiplier / SQRT_2).powf(0.25) as f32
}

/// The labels' font scale at a zoom: (zoom / √2) to the power 0.1.
#[must_use]
pub fn label_scale(multiplier: f64) -> f32 {
    (multiplier / SQRT_2).powf(0.1) as f32
}

/// Points to pixels (96 per inch), for the original's font sizes.
pub const PIXELS_PER_POINT: f32 = 96.0 / 72.0;

/// The label fonts' sizes at a zoom, whole pixels: the other places' (8
/// points by the label scale, at most 10) and the towns' (9 points by it,
/// at most 12, bold).
#[must_use]
pub fn label_font_pixels(multiplier: f64) -> (f32, f32) {
    let scale = label_scale(multiplier);
    (
        ((8.0 * scale).min(10.0) * PIXELS_PER_POINT).round(),
        ((9.0 * scale).min(12.0) * PIXELS_PER_POINT).round(),
    )
}

/// The coordinate labels' font size (8 points), pixels.
pub const COORD_FONT_PIXELS: f32 = 11.0;
/// The tooltip's font size (10 points), pixels.
pub const TOOLTIP_FONT_PIXELS: f32 = 13.0;

/// One kind of place on the map: the kinds (a place's portal kinds count
/// as [`MAP_PORTALS`]), its icon, and the least zoom factor it shows from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconGroup {
    /// The kinds.
    pub kinds: LocationType,
    /// Its icon.
    pub icon: MapIcon,
    /// The least zoom factor it shows from.
    pub from_factor: f64,
}

const fn group(kinds: LocationType, icon: MapIcon, from_factor: f64) -> IconGroup {
    IconGroup {
        kinds,
        icon,
        from_factor,
    }
}

/// The kinds the map draws, in the order the original drew them (later
/// ones over earlier ones), with their icons and least zoom factors. The
/// original's `Custom` kind (from −1, the landmark icon) has no flag in
/// this port's kinds.
pub const ICON_GROUPS: [IconGroup; 12] = [
    group(LocationType::NPC, MapIcon::Npc, 5.0),
    group(LocationType::VENDOR, MapIcon::Store, 5.0),
    group(LocationType::VILLAGE, MapIcon::Settlement, 2.0),
    group(LocationType::LANDMARK, MapIcon::PointOfInterest, 2.0),
    group(LocationType::ALLEGIANCE_HALL, MapIcon::Dungeon, 2.0),
    group(LocationType::BINDSTONE, MapIcon::Bindstone, 2.0),
    group(LocationType::LIFESTONE, MapIcon::Lifestone, -1.0),
    group(LocationType::DUNGEON, MapIcon::Dungeon, -1.0),
    group(MAP_PORTALS, MapIcon::Portal, -1.0),
    group(LocationType::PORTAL_HUB, MapIcon::PortalHub, -1.0),
    group(LocationType::OUTPOST, MapIcon::TownSmall, -1.0),
    group(LocationType::TOWN, MapIcon::Town, f64::NEG_INFINITY),
];

/// Which group a place falls in: any portal kind but a hub is a portal;
/// otherwise its kind exactly (a place of two other kinds is drawn by
/// none, as the original drew it).
#[must_use]
pub fn group_of(kind: LocationType) -> Option<&'static IconGroup> {
    group_index(kind).map(|i| &ICON_GROUPS[i])
}

/// [`group_of`]'s place in [`ICON_GROUPS`].
fn group_index(kind: LocationType) -> Option<usize> {
    let bucket = if kind.intersects(MAP_PORTALS) {
        MAP_PORTALS
    } else {
        kind
    };
    ICON_GROUPS.iter().position(|g| g.kinds == bucket)
}

/// Whether a group shows at a zoom factor (always, with "Show locations at
/// every zoom").
#[must_use]
pub fn group_shows(group: &IconGroup, factor: f64, every_zoom: bool) -> bool {
    every_zoom || factor >= group.from_factor
}

/// The kinds labelled from a zoom factor: towns (in the larger bold font)
/// from −4; outposts, hubs, portals, dungeons and lifestones from 1;
/// bindstones, allegiance halls, landmarks and villages from 4. People and
/// vendors are never labelled. In the original's order.
pub const LABEL_GROUPS: [(LocationType, f64, bool); 10] = [
    (LocationType::TOWN, -4.0, true),
    (LocationType::OUTPOST, 1.0, false),
    (LocationType::PORTAL_HUB, 1.0, false),
    (MAP_PORTALS, 1.0, false),
    (LocationType::DUNGEON, 1.0, false),
    (LocationType::LIFESTONE, 1.0, false),
    (LocationType::BINDSTONE, 4.0, false),
    (LocationType::ALLEGIANCE_HALL, 4.0, false),
    (LocationType::LANDMARK, 4.0, false),
    (LocationType::VILLAGE, 4.0, false),
];

/// A place drawn on the map: where its icon is, the whole pixels it
/// answers the pointer in, and the place.
#[derive(Debug, Clone)]
pub struct Hotspot {
    /// The icon drawn.
    pub icon: MapIcon,
    /// The icon's rectangle.
    pub rect: Rect,
    /// The pixels that answer the pointer ([`Rect::whole_pixels`]).
    pub hit: Rect,
    /// The place.
    pub location: Rc<Location>,
}

/// What the map shows of the places in view.
#[derive(Debug, Clone, Copy)]
pub struct IconRules {
    /// The zoom.
    pub zoom: Zoom,
    /// "Show locations at every zoom".
    pub every_zoom: bool,
}

/// The places' icons in drawing order, last on top: those in view
/// (`top_left` and `bottom_right` the view's corners, map coordinates)
/// whose group shows, each centred on its place at its icon's size times
/// [`icon_scale`], a favourite (`is_favorite`) with the favourite's icon.
/// A portal whose icon would cover a hub's point is left out: the hub
/// stands for it.
#[must_use]
pub fn icon_layout(
    locations: &[Rc<Location>],
    rules: IconRules,
    top_left: Coordinates,
    bottom_right: Coordinates,
    to_canvas: &dyn Fn(Coordinates) -> (f32, f32),
    is_favorite: &dyn Fn(&Location) -> bool,
) -> Vec<Hotspot> {
    let scale = icon_scale(rules.zoom.multiplier());
    // Each place in view into its group's list, once.
    let mut groups: [Vec<&Rc<Location>>; ICON_GROUPS.len()] = Default::default();
    for location in locations
        .iter()
        .filter(|l| !l.is_retired && l.has_coordinates())
        .filter(|l| {
            l.coords.ns <= top_left.ns
                && l.coords.ns >= bottom_right.ns
                && l.coords.ew >= top_left.ew
                && l.coords.ew <= bottom_right.ew
        })
    {
        if let Some(i) = group_index(location.kind) {
            groups[i].push(location);
        }
    }
    let hubs: Vec<(f32, f32)> = group_index(LocationType::PORTAL_HUB)
        .map(|i| groups[i].iter().map(|l| to_canvas(l.coords)).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for (group, members) in ICON_GROUPS.iter().zip(&groups) {
        if !group_shows(group, rules.zoom.factor(), rules.every_zoom) {
            continue;
        }
        let (w, h) = group.icon.size();
        let (w, h) = (w * scale, h * scale);
        for location in members {
            let at = to_canvas(location.coords);
            let rect = Rect::new(at.0 - w / 2.0, at.1 - h / 2.0, w, h);
            if group.kinds == MAP_PORTALS && hubs.iter().any(|hub| rect.contains(*hub)) {
                continue;
            }
            out.push(Hotspot {
                icon: if is_favorite(location) {
                    MapIcon::Favorite
                } else {
                    group.icon
                },
                rect,
                hit: rect.whole_pixels(),
                location: Rc::clone(location),
            });
        }
    }
    out
}

/// The place under a point: the topmost hotspot containing it, the point
/// taken to its whole pixel.
#[must_use]
pub fn hotspot_at(hotspots: &[Hotspot], at: (f32, f32)) -> Option<&Hotspot> {
    let at = (at.0.floor(), at.1.floor());
    hotspots.iter().rev().find(|h| h.hit.contains(at))
}

/// Where a label of `size` may go beside a hotspot's pixels, in the order
/// tried: below, above, over its lower half, right, left, then the four
/// corners (up-right, down-right, up-left, down-left), each overlapping
/// the icon by a pixel.
#[must_use]
pub fn label_candidates(hit: Rect, size: (f32, f32)) -> [Rect; 9] {
    let (w, h) = size;
    let (left, top, right, bottom) = (hit.x, hit.y, hit.right(), hit.bottom());
    let cx = f32::midpoint(left, right);
    let cy = f32::midpoint(top, bottom);
    let lower = (top + 3.0 * bottom) / 4.0;
    let at = |x: f32, y: f32| Rect::new(x, y, w, h);
    [
        at(cx - w / 2.0, bottom - 1.0),
        at(cx - w / 2.0, top - h + 1.0),
        at(cx - w / 2.0, lower - h / 2.0),
        at(right - 1.0, cy - h / 2.0),
        at(left - w + 1.0, cy - h / 2.0),
        at(right - 1.0, top - h + 1.0),
        at(right - 1.0, bottom - 1.0),
        at(left - w + 1.0, top - h + 1.0),
        at(left - w + 1.0, bottom - 1.0),
    ]
}

/// A place's label as placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// The text.
    pub text: String,
    /// Its rectangle.
    pub rect: Rect,
    /// In the towns' bold font.
    pub bold: bool,
}

/// The labels at a zoom factor: for each [`LABEL_GROUPS`] entry shown at
/// that factor, each of its hotspots, topmost first, takes the first of
/// its [`label_candidates`] that overlaps no label placed before it, or
/// goes without. `measure` gives a text's size in the regular or the bold
/// font.
#[must_use]
pub fn place_labels(
    hotspots: &[Hotspot],
    factor: f64,
    measure: &mut dyn FnMut(&str, bool) -> (f32, f32),
) -> Vec<Label> {
    let mut placed: Vec<Label> = Vec::new();
    for (kinds, from, bold) in LABEL_GROUPS {
        if factor < from {
            continue;
        }
        for hotspot in hotspots
            .iter()
            .rev()
            .filter(|h| h.location.kind.intersects(kinds))
        {
            let size = measure(&hotspot.location.name, bold);
            if let Some(rect) = label_candidates(hotspot.hit, size)
                .into_iter()
                .find(|r| !placed.iter().any(|p| p.rect.intersects(r)))
            {
                placed.push(Label {
                    text: hotspot.location.name.clone(),
                    rect,
                    bold,
                });
            }
        }
    }
    placed
}

/// What a menu item does.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    /// Shows the place in the window.
    ShowDetails,
    /// Points the arrow at it.
    PointArrow,
    /// Makes it the route finder's start.
    StartRoute,
    /// Makes it the route finder's end.
    EndRoute,
    /// Shows a dungeon's floorplan (its landblock).
    ShowDungeonMap(u32),
    /// Centres the map on where a portal lets out.
    JumpToExit(Coordinates),
}

/// What a menu is for.
#[derive(Debug, Clone)]
pub enum MenuTarget {
    /// A place.
    Location(Rc<Location>),
    /// A point of the map.
    Point(Coordinates),
}

/// A right-click menu drawn on the map: a title row, then its items.
#[derive(Debug, Clone)]
pub struct MapMenu {
    /// What it is for.
    pub target: MenuTarget,
    /// The title row.
    pub title: String,
    /// The items, in order.
    pub items: Vec<(String, MenuAction)>,
    /// Its rectangle, canvas pixels.
    pub rect: Rect,
}

/// A menu row's height, pixels.
pub const MENU_ROW: f32 = 20.0;
/// The space either side of a menu row's text, pixels.
pub const MENU_PAD: f32 = 8.0;

impl MapMenu {
    /// The menu for a place: Show Details, Point Arrow Here, Start Route
    /// Here and End Route Here, then Show Dungeon Map when its dungeon's
    /// floorplan can be shown (`dungeon`, the landblock) and Jump to Exit
    /// when it is a portal with an exit.
    #[must_use]
    pub fn for_location(location: Rc<Location>, dungeon: Option<u32>) -> Self {
        let mut items = vec![
            ("Show Details".to_owned(), MenuAction::ShowDetails),
            ("Point Arrow Here".to_owned(), MenuAction::PointArrow),
            ("Start Route Here".to_owned(), MenuAction::StartRoute),
            ("End Route Here".to_owned(), MenuAction::EndRoute),
        ];
        if let Some(landblock) = dungeon {
            items.push((
                "Show Dungeon Map".to_owned(),
                MenuAction::ShowDungeonMap(landblock),
            ));
        }
        if location.has_exit_coords() {
            let exit = location.exit_coords();
            items.push((
                format!("Jump to Exit ({exit})"),
                MenuAction::JumpToExit(exit),
            ));
        }
        Self {
            title: location.name.clone(),
            target: MenuTarget::Location(location),
            items,
            rect: Rect::default(),
        }
    }

    /// The menu for a point of the map: its coordinates as the title, then
    /// Point Arrow Here, Start Route Here and End Route Here.
    #[must_use]
    pub fn for_point(coords: Coordinates) -> Self {
        Self {
            title: coords.to_text_with_decimals(1, false),
            target: MenuTarget::Point(coords),
            items: vec![
                ("Point Arrow Here".to_owned(), MenuAction::PointArrow),
                ("Start Route Here".to_owned(), MenuAction::StartRoute),
                ("End Route Here".to_owned(), MenuAction::EndRoute),
            ],
            rect: Rect::default(),
        }
    }

    /// Places it with its top-left at `at`, as wide as its widest text
    /// (`text_width`) and padding, moved in to stay inside `bounds`.
    pub fn place(&mut self, at: (f32, f32), text_width: f32, bounds: Rect) {
        let w = text_width + 2.0 * MENU_PAD;
        let h = MENU_ROW * (self.items.len() + 1) as f32;
        let x = at.0.min(bounds.right() - w).max(bounds.x);
        let y = at.1.min(bounds.bottom() - h).max(bounds.y);
        self.rect = Rect::new(x, y, w, h);
    }

    /// The item under a point, by its index (none on the title or off the
    /// menu).
    #[must_use]
    pub fn item_at(&self, at: (f32, f32)) -> Option<usize> {
        if !self.rect.contains(at) {
            return None;
        }
        let row = ((at.1 - self.rect.y) / MENU_ROW).floor() as usize;
        row.checked_sub(1).filter(|i| *i < self.items.len())
    }

    /// An item's row.
    #[must_use]
    pub fn row(&self, index: usize) -> Rect {
        Rect::new(
            self.rect.x,
            self.rect.y + MENU_ROW * (index + 1) as f32,
            self.rect.w,
            MENU_ROW,
        )
    }
}

/// Whether a press makes a double click with the click before it: within
/// [`DOUBLE_CLICK_MS`] and [`DOUBLE_CLICK_SLOP`] pixels either way.
#[must_use]
pub fn is_double_click(last: Option<((f32, f32), u64)>, at: (f32, f32), now_ms: u64) -> bool {
    last.is_some_and(|(was, when)| {
        now_ms.saturating_sub(when) <= DOUBLE_CLICK_MS
            && (at.0 - was.0).abs() <= DOUBLE_CLICK_SLOP
            && (at.1 - was.1).abs() <= DOUBLE_CLICK_SLOP
    })
}
