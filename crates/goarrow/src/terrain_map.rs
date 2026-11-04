//! A sharp map of Dereth, drawn by the plugin from the landscape's terrain
//! (API 1.5) rather than the game's small world map: one pixel per terrain
//! vertex (24 m apart), 2041 pixels a side (255 landblocks of 8 vertex
//! steps, and the last edge), north up.
//!
//! Each pixel takes the mean colour of its terrain type's texture (`rgb`),
//! or the road texture's where a road runs over the vertex, lifted by
//! [`BRIGHTNESS`] (texture means are darker than the ground looks in play)
//! and shaded gently by the slope there so hills show, lit from the
//! north-west. Water ([`WATER_TYPES`]) keeps its texture's brightness but
//! takes the hue of the type's map colour, which is blue for every water
//! type: some water textures average green-grey and would read as land.
//! The palette's map colours (`map_rgb`) are otherwise not used: they code
//! the types (marsh and moss pale blue, bare rock green) rather than show
//! the ground. A pixel of a landblock the game files have no terrain for
//! takes the deep sea's colour ([`DEEP_SEA`]), or the darkest colour when
//! the deep sea has none.
//!
//! The map is read a page of [`LANDSCAPE_TERRAIN_MAX`] landblocks at a
//! call, at most [`CALLS_PER_TICK`] calls a tick and only within what is
//! left of the tick's work budget, then coloured a band of rows at a time,
//! then handed to the client once as one image ([`IMAGE_NAME`]).
//!
//! Pixel `(px, py)` is vertex `(vx, vy)` of landblock `(bx, by)` where
//! `px = 8 bx + vx` and `py = 2040 - (8 by + vy)`. Its middle stands at map
//! coordinates `EW = px / 10 - 101.95`, `NS = (2040 - py) / 10 - 101.95`:
//! the landblock conversion of [`openac_plugin::world::Position::from_cell`]
//! for a vertex at landblock-local `(24 vx, 24 vy)` metres.

use openac_plugin::Host;
use openac_plugin::ui::{Image, Painter, rgba};
use openac_plugin::world::{
    LANDSCAPE_TERRAIN_MAX, LandblockTerrain, ROAD_TERRAIN_TYPE, TERRAIN_VERTEX_SPACING_METERS,
    TERRAIN_VERTICES_PER_SIDE, TerrainColor,
};

use crate::map::DerethBackground;
use crate::plugin::{WORK_BUDGET_MS, WORK_CHUNKS};

/// Landblocks along each side of the map (`0x00` to `0xFE`; the game files
/// have no terrain past them).
pub const LANDBLOCKS_PER_SIDE: usize = 255;
/// Vertex steps along a landblock's side.
pub const STEPS_PER_LANDBLOCK: usize = TERRAIN_VERTICES_PER_SIDE - 1;
/// Pixels along each side of the map: one per vertex.
pub const SIDE: usize = LANDBLOCKS_PER_SIDE * STEPS_PER_LANDBLOCK + 1;
/// Pixels per map unit (a vertex step, 24 m, is a tenth of a map unit).
pub const PIXELS_PER_MAP_UNIT: f64 = 10.0;
/// The map coordinate of the first vertex, both east-west and north-south:
/// the south-west corner of landblock `0x0000`.
pub const FIRST_VERTEX: f64 = -101.95;
/// Landblocks on the map.
pub const LANDBLOCKS: usize = LANDBLOCKS_PER_SIDE * LANDBLOCKS_PER_SIDE;
/// Calls it takes to read them all.
pub const PAGES: usize = LANDBLOCKS.div_ceil(LANDSCAPE_TERRAIN_MAX);
/// The most landscape calls in one tick.
pub const CALLS_PER_TICK: usize = 4;
/// Rows coloured between two looks at the clock.
pub const ROWS_PER_CHUNK: usize = 32;
/// The plugin's name for the image.
pub const IMAGE_NAME: &str = "goarrow.terrain";
/// How strongly the slope shades: 0 flat, 1 full hill shading.
pub const RELIEF_STRENGTH: f64 = 0.5;
/// The darkest and brightest a slope makes a colour.
pub const RELIEF_RANGE: (f64, f64) = (0.7, 1.25);
/// How much every colour is lifted before the relief, so the map reads
/// well on screen (clamped at white).
pub const BRIGHTNESS: f64 = 1.3;
/// The water types: running, standing fresh, shallow sea, shallow still
/// sea and deep sea, as the client's terrain marks water.
pub const WATER_TYPES: std::ops::RangeInclusive<u8> = 16..=20;
/// The deep sea's type: the colour where the files have no terrain.
pub const DEEP_SEA: u8 = 20;

/// No terrain here: a pixel's kind.
const NO_TERRAIN: u8 = u8::MAX;
/// A road runs over the vertex: added to a pixel's kind.
const ROAD_BIT: u8 = 0x80;
/// Terrain types a vertex can name.
const TERRAIN_TYPES: usize = 32;

/// The pixel of vertex `(x, y)` (east, north of the south-west corner, 0 to
/// 8) of `landblock` (`x << 8 | y`); none off the map.
#[must_use]
pub fn vertex_pixel(landblock: u16, x: usize, y: usize) -> Option<(usize, usize)> {
    let bx = usize::from(landblock >> 8);
    let by = usize::from(landblock & 0xFF);
    if bx >= LANDBLOCKS_PER_SIDE
        || by >= LANDBLOCKS_PER_SIDE
        || x >= TERRAIN_VERTICES_PER_SIDE
        || y >= TERRAIN_VERTICES_PER_SIDE
    {
        return None;
    }
    Some((
        bx * STEPS_PER_LANDBLOCK + x,
        SIDE - 1 - (by * STEPS_PER_LANDBLOCK + y),
    ))
}

/// The map coordinates (north-south, east-west) of a pixel's vertex.
#[must_use]
pub fn pixel_coordinates(px: usize, py: usize) -> (f64, f64) {
    (
        (SIDE - 1 - py) as f64 / PIXELS_PER_MAP_UNIT + FIRST_VERTEX,
        px as f64 / PIXELS_PER_MAP_UNIT + FIRST_VERTEX,
    )
}

fn rgb_of(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

fn luminance(c: [u8; 3]) -> f64 {
    0.299 * f64::from(c[0]) + 0.587 * f64::from(c[1]) + 0.114 * f64::from(c[2])
}

/// Water's colour: the hue of its map colour at its texture's brightness
/// (the texture's when the map colour is black).
#[must_use]
pub fn water_colour(texture: [u8; 3], map: [u8; 3]) -> [u8; 3] {
    let hue = luminance(map);
    if hue <= 0.0 {
        return texture;
    }
    shaded(map, luminance(texture) / hue)
}

/// The colours a map is drawn in, from the client's terrain palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainColors {
    types: [Option<[u8; 3]>; TERRAIN_TYPES],
    road: Option<[u8; 3]>,
    missing: [u8; 3],
}

impl TerrainColors {
    /// The colours of `palette`; none when it is empty (no game files).
    #[must_use]
    pub fn from_palette(palette: &[TerrainColor]) -> Option<Self> {
        if palette.is_empty() {
            return None;
        }
        let mut types = [None; TERRAIN_TYPES];
        let mut road = None;
        for c in palette {
            // A texture that gave no colour (0) gives the type none.
            if c.rgb == 0 {
                continue;
            }
            let texture = rgb_of(c.rgb);
            if c.terrain_type == ROAD_TERRAIN_TYPE {
                // Without one, a road leaves the ground's colour.
                road = Some(shaded(texture, BRIGHTNESS));
            } else if let Some(slot) = types.get_mut(usize::from(c.terrain_type)) {
                let colour = if WATER_TYPES.contains(&c.terrain_type) {
                    water_colour(texture, rgb_of(c.map_rgb))
                } else {
                    texture
                };
                *slot = Some(shaded(colour, BRIGHTNESS));
            }
        }
        let missing = types[usize::from(DEEP_SEA)]
            .or_else(|| {
                types
                    .iter()
                    .flatten()
                    .copied()
                    .min_by(|a, b| luminance(*a).total_cmp(&luminance(*b)))
            })
            .unwrap_or([0, 0, 0]);
        Some(Self {
            types,
            road,
            missing,
        })
    }

    /// A vertex's colour before the relief: its type's, or the road's
    /// where `road` is non-zero; the missing colour for a type without
    /// one.
    #[must_use]
    pub fn base(&self, terrain_type: u8, road: u8) -> [u8; 3] {
        if road != 0
            && let Some(road) = self.road
        {
            return road;
        }
        self.types
            .get(usize::from(terrain_type))
            .copied()
            .flatten()
            .unwrap_or(self.missing)
    }

    /// The colour where the files have no terrain: the deep sea's, else
    /// the darkest type's, black when no type has one.
    #[must_use]
    pub fn missing(&self) -> [u8; 3] {
        self.missing
    }

    fn of_kind(&self, kind: u8) -> [u8; 3] {
        if kind == NO_TERRAIN {
            self.missing
        } else {
            self.base(kind & !ROAD_BIT, kind & ROAD_BIT)
        }
    }
}

/// How bright a slope makes the ground, 1 on the flat: hill shading lit
/// from the north-west, 45 degrees up, softened by [`RELIEF_STRENGTH`] and
/// kept within [`RELIEF_RANGE`]. `east_rise` and `north_rise` are the
/// ground's rise going east and going north, metres per metre.
#[must_use]
pub fn relief(east_rise: f64, north_rise: f64) -> f64 {
    // The ground's normal is (-east, -north, 1); the light comes from
    // (-1, 1) on the ground, 45 degrees up. On the flat the light falls at
    // sin 45; the shade is relative to that.
    let lit = (1.0 + (east_rise - north_rise) / std::f64::consts::SQRT_2)
        / (1.0 + east_rise * east_rise + north_rise * north_rise).sqrt();
    (1.0 + RELIEF_STRENGTH * (lit - 1.0)).clamp(RELIEF_RANGE.0, RELIEF_RANGE.1)
}

/// A colour brightened or darkened by `factor`.
#[must_use]
pub fn shaded(rgb: [u8; 3], factor: f64) -> [u8; 3] {
    rgb.map(|c| (f64::from(c) * factor).round().clamp(0.0, 255.0) as u8)
}

/// The sharp map as the Dereth view's background.
#[derive(Debug, Clone, Copy)]
pub struct TerrainMap {
    /// The image the client holds.
    pub image: Image,
}

impl DerethBackground for TerrainMap {
    fn size(&self) -> (f32, f32) {
        (SIDE as f32, SIDE as f32)
    }

    fn to_pixel(&self, ns: f64, ew: f64) -> (f64, f64) {
        // A pixel's middle (+0.5) is its vertex.
        (
            (ew - FIRST_VERTEX) * PIXELS_PER_MAP_UNIT + 0.5,
            (SIDE - 1) as f64 - (ns - FIRST_VERTEX) * PIXELS_PER_MAP_UNIT + 0.5,
        )
    }

    fn to_coords(&self, x: f64, y: f64) -> (f64, f64) {
        (
            ((SIDE - 1) as f64 - (y - 0.5)) / PIXELS_PER_MAP_UNIT + FIRST_VERTEX,
            (x - 0.5) / PIXELS_PER_MAP_UNIT + FIRST_VERTEX,
        )
    }

    fn draw(&self, painter: &mut Painter, origin: (f32, f32), scale: f32) {
        let side = SIDE as f32 * scale;
        if self.image.is_valid() {
            painter.image(
                self.image,
                origin.0,
                origin.1,
                side,
                side,
                rgba(255, 255, 255, 255),
            );
        } else {
            painter.fill_rect(origin.0, origin.1, side, side, rgba(24, 30, 36, 255));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Palette,
    Reading(usize),
    Colouring(usize),
    HandOver,
    Done,
    Failed,
}

/// The sharp map being drawn: the palette, then the landscape a page at a
/// time, then the colours a band of rows at a time, then the hand-over.
pub struct TerrainBuild {
    stage: Stage,
    colors: Option<TerrainColors>,
    /// Per pixel: the terrain type, [`ROAD_BIT`] where a road runs, or
    /// [`NO_TERRAIN`].
    kinds: Vec<u8>,
    /// Per pixel: the ground's height, metres; NaN with no terrain.
    heights: Vec<f32>,
    rgba: Vec<u8>,
    previous: Option<Image>,
}

impl std::fmt::Debug for TerrainBuild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerrainBuild")
            .field("stage", &self.stage)
            .finish_non_exhaustive()
    }
}

impl TerrainBuild {
    /// A build; `previous` is an image of an earlier build, let go before
    /// the new one is handed over (the client would otherwise answer the
    /// same name with the old image).
    #[must_use]
    pub fn new(previous: Option<Image>) -> Self {
        Self {
            stage: Stage::Palette,
            colors: None,
            kinds: Vec::new(),
            heights: Vec::new(),
            rgba: Vec::new(),
            previous,
        }
    }

    /// How far along, 0 to 1: reading is most of it.
    #[must_use]
    pub fn progress(&self) -> f64 {
        match self.stage {
            Stage::Palette | Stage::Failed => 0.0,
            Stage::Reading(page) => 0.9 * page as f64 / PAGES as f64,
            Stage::Colouring(row) => 0.9 + 0.1 * row as f64 / SIDE as f64,
            Stage::HandOver | Stage::Done => 1.0,
        }
    }

    /// It stopped without a map: no game files, or the client refused the
    /// image.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.stage == Stage::Failed
    }

    /// This tick's work, until the tick that started at `started_ms` has
    /// spent [`WORK_BUDGET_MS`]; the image once the client holds it.
    pub fn step(&mut self, host: &mut Host<'_>, started_ms: u64) -> Option<Image> {
        let spent = |host: &mut Host<'_>| {
            host.clock().monotonic_ms.saturating_sub(started_ms) >= WORK_BUDGET_MS
        };
        match self.stage {
            Stage::Palette => {
                if spent(host) {
                    return None;
                }
                // About 20 ms the first time: a tick of its own.
                match TerrainColors::from_palette(&host.terrain_palette()) {
                    Some(colors) => {
                        self.colors = Some(colors);
                        self.kinds = vec![NO_TERRAIN; SIDE * SIDE];
                        self.heights = vec![f32::NAN; SIDE * SIDE];
                        self.stage = Stage::Reading(0);
                    }
                    None => {
                        host.info("GoArrow: no terrain palette; the map stays the game's");
                        self.stage = Stage::Failed;
                    }
                }
                None
            }
            Stage::Reading(mut page) => {
                for _ in 0..CALLS_PER_TICK {
                    if spent(host) {
                        break;
                    }
                    let ids = page_ids(page);
                    for terrain in host.landscape_terrain(&ids) {
                        self.place(&terrain);
                    }
                    page += 1;
                    if page == PAGES {
                        break;
                    }
                }
                self.stage = if page == PAGES {
                    self.rgba = vec![0; SIDE * SIDE * 4];
                    Stage::Colouring(0)
                } else {
                    Stage::Reading(page)
                };
                None
            }
            Stage::Colouring(mut row) => {
                for _ in 0..WORK_CHUNKS {
                    if spent(host) {
                        break;
                    }
                    let end = (row + ROWS_PER_CHUNK).min(SIDE);
                    for py in row..end {
                        self.colour_row(py);
                    }
                    row = end;
                    if row == SIDE {
                        break;
                    }
                }
                if row == SIDE {
                    self.kinds = Vec::new();
                    self.heights = Vec::new();
                    self.stage = Stage::HandOver;
                } else {
                    self.stage = Stage::Colouring(row);
                }
                None
            }
            Stage::HandOver => {
                if spent(host) {
                    return None;
                }
                if let Some(old) = self.previous.take()
                    && old.is_valid()
                {
                    host.image_release(old);
                }
                let side = SIDE as u32;
                let image = host.image_rgba(IMAGE_NAME, side, side, &self.rgba);
                self.rgba = Vec::new();
                if image.is_valid() {
                    self.stage = Stage::Done;
                    Some(image)
                } else {
                    host.warn(
                        "GoArrow: the client did not take the map image; the map stays the game's",
                    );
                    self.stage = Stage::Failed;
                    None
                }
            }
            Stage::Done | Stage::Failed => None,
        }
    }

    fn place(&mut self, terrain: &LandblockTerrain) {
        if terrain.vertices.len() != TERRAIN_VERTICES_PER_SIDE * TERRAIN_VERTICES_PER_SIDE {
            return;
        }
        for x in 0..TERRAIN_VERTICES_PER_SIDE {
            for y in 0..TERRAIN_VERTICES_PER_SIDE {
                let (Some((px, py)), Some(v)) =
                    (vertex_pixel(terrain.landblock, x, y), terrain.vertex(x, y))
                else {
                    continue;
                };
                let i = py * SIDE + px;
                let road = if v.road != 0 { ROAD_BIT } else { 0 };
                // A type past the 32 has no colour; it must not read as
                // [`NO_TERRAIN`] with the road bit.
                let terrain_type = if usize::from(v.terrain_type) < TERRAIN_TYPES {
                    v.terrain_type
                } else {
                    0x7E
                };
                self.kinds[i] = terrain_type | road;
                self.heights[i] = v.height_meters;
            }
        }
    }

    fn colour_row(&mut self, py: usize) {
        let Some(colors) = self.colors.as_ref() else {
            return;
        };
        let height = |px: usize, py: usize| -> Option<f64> {
            let h = self.heights[py * SIDE + px];
            h.is_finite().then_some(f64::from(h))
        };
        for px in 0..SIDE {
            let i = py * SIDE + px;
            let kind = self.kinds[i];
            let colour = match height(px, py) {
                Some(here) if kind != NO_TERRAIN => {
                    let west = px.checked_sub(1).and_then(|x| height(x, py));
                    let east = (px + 1 < SIDE).then(|| height(px + 1, py)).flatten();
                    let north = py.checked_sub(1).and_then(|y| height(px, y));
                    let south = (py + 1 < SIDE).then(|| height(px, py + 1)).flatten();
                    let factor = relief(rise(west, here, east), rise(south, here, north));
                    shaded(colors.of_kind(kind), factor)
                }
                _ => colors.missing(),
            };
            let [r, g, b] = colour;
            self.rgba[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

/// The ground's rise from `before` through `here` to `after`, a vertex step
/// either side, metres per metre: across both where both are known, else
/// across the one known, else flat.
fn rise(before: Option<f64>, here: f64, after: Option<f64>) -> f64 {
    let step = f64::from(TERRAIN_VERTEX_SPACING_METERS);
    match (before, after) {
        (Some(b), Some(a)) => (a - b) / (2.0 * step),
        (None, Some(a)) => (a - here) / step,
        (Some(b), None) => (here - b) / step,
        (None, None) => 0.0,
    }
}

/// The landblocks of page `page`: column by column, `x << 8 | y`.
fn page_ids(page: usize) -> Vec<u16> {
    let first = page * LANDSCAPE_TERRAIN_MAX;
    let last = (first + LANDSCAPE_TERRAIN_MAX).min(LANDBLOCKS);
    (first..last)
        .map(|n| {
            let (x, y) = (n / LANDBLOCKS_PER_SIDE, n % LANDBLOCKS_PER_SIDE);
            (x << 8 | y) as u16
        })
        .collect()
}
