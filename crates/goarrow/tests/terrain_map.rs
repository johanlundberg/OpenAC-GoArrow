//! The sharp map of Dereth the plugin draws from the landscape's terrain:
//! where each vertex falls, its colour, how the map is read over ticks and
//! handed to the client, and when the map uses it.

mod common;

use std::collections::BTreeSet;

use ac_plugin_abi::world::{
    LANDSCAPE_TERRAIN_MAX, LandblockTerrain, Position, ROAD_TERRAIN_TYPE, TerrainColor,
    TerrainVertex,
};
use ac_plugin_ui::PaintCmd;
use common::Harness;
use goarrow::map::{BODY, DerethBackground, DerethView};
use goarrow::map_view::Zoom;
use goarrow::terrain_map::{
    BRIGHTNESS, CALLS_PER_TICK, IMAGE_NAME, PAGES, SIDE, TerrainColors, TerrainMap, WATER_TYPES,
    pixel_coordinates, relief, shaded, vertex_pixel,
};
use openac_plugin::Event;
use openac_plugin::calls;
use openac_plugin::ui::Image;

const MAP: &str = "goarrow.map";
const HOLTBURG: u16 = 0xA9B4;
/// The road's texture colour.
const ROAD: u32 = 0x707469;

fn rgb(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// The game files' palette: each type's texture mean, its map colour and
/// its name.
const PALETTE: [(u32, u32, &str); 32] = [
    (0x665949, 0x649664, "BarrenRock"),
    (0x5A5E30, 0x64DC64, "Grassland"),
    (0xAFB3B3, 0x969696, "Ice"),
    (0x605E24, 0x46FA46, "LushGrass"),
    (0x442F15, 0x46C8C8, "MarshSparseSwamp"),
    (0x382815, 0xC89696, "MudRichDirt"),
    (0x17111C, 0x282864, "ObsidianPlain"),
    (0x705433, 0xB48264, "PackedDirt"),
    (0x62503A, 0x967846, "PatchyDirt"),
    (0x464825, 0x96C896, "PatchyGrassland"),
    (0xD79B68, 0xC8C846, "sand-yellow"),
    (0x94816C, 0x969646, "sand-grey"),
    (0xB8906D, 0x969664, "sand-rockStrewn"),
    (0x987958, 0x467846, "SedimentaryRock"),
    (0x595338, 0x649664, "SemiBarrenRock"),
    (0xC0C4CA, 0xFAFAFA, "Snow"),
    (0x234D6F, 0x6464C8, "WaterRunning"),
    (0x164551, 0x6496DC, "WaterStandingFresh"),
    (0x242A44, 0x2896C8, "WaterShallowSea"),
    (0x1F3F3A, 0x0A7896, "WaterShallowStillSea"),
    (0x1F243F, 0x0A4664, "WaterDeepSea"),
    (0x5A5F2A, 0x009600, "forestfloor"),
    (0x234D6F, 0x6464C8, "FauxWaterRunning"),
    (0x465A42, 0x649664, "SeaSlime"),
    (0x665949, 0x649664, "Argila"),
    (0x1C1417, 0x649664, "Volcano1"),
    (0x676767, 0x649664, "Volcano2"),
    (0x7FA5A4, 0xC3E1F0, "BlueIce"),
    (0x464825, 0xC3E1F0, "Moss"),
    (0x413717, 0xC3E1F0, "DarkMoss"),
    (0x463A38, 0xC3E1F0, "olthoi"),
    (0x665949, 0x46C8C8, "DesolateLands"),
];

/// The 32 types, then the road (whose map colour repeats its texture's).
fn palette() -> Vec<TerrainColor> {
    let mut palette: Vec<TerrainColor> = PALETTE
        .iter()
        .enumerate()
        .map(|(t, (texture, map, name))| TerrainColor {
            terrain_type: t as u8,
            rgb: *texture,
            map_rgb: *map,
            name: Some((*name).to_owned()),
        })
        .collect();
    palette.push(TerrainColor {
        terrain_type: ROAD_TERRAIN_TYPE,
        rgb: ROAD,
        map_rgb: ROAD,
        name: Some("Road".to_owned()),
    });
    palette
}

/// A land type's colour on the map: its texture's, lifted.
fn type_colour(t: u8) -> [u8; 3] {
    shaded(rgb(PALETTE[usize::from(t)].0), BRIGHTNESS)
}

/// The deep sea's colour on the map (type 20).
fn deep_sea() -> [u8; 3] {
    TerrainColors::from_palette(&palette()).unwrap().base(20, 0)
}

fn distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(y)).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Holtburg's landblock: type 1, rising 10 m a vertex to the east, a road
/// over vertex (4, 4).
fn holtburg() -> LandblockTerrain {
    LandblockTerrain {
        landblock: HOLTBURG,
        vertices: (0..81)
            .map(|i| {
                let (x, y) = (i / 9, i % 9);
                TerrainVertex {
                    terrain_type: 1,
                    road: u8::from(x == 4 && y == 4),
                    height_meters: 10.0 * x as f32,
                }
            })
            .collect(),
    }
}

/// The plugin with a window attached, the landscape answering Holtburg
/// to every page, and the map shown or not.
fn with_window(shown: bool) -> Harness {
    Harness::new(|sim| {
        if shown {
            sim.show_map();
        }
        sim.fake.window_attached = true;
        sim.fake.ui.set_window_attached(true);
        sim.fake
            .set_reply::<calls::LandscapeTerrainPalette>(&palette());
        sim.fake
            .set_reply::<calls::LandscapeTerrain>(&vec![holtburg()]);
        sim.stand(0.0, 0.0, 0.0);
    })
}

fn tick_until_drawn(h: &mut Harness) -> usize {
    for ticks in 0..2000 {
        if h.plugin.map().unwrap().terrain_image().is_some() {
            return ticks;
        }
        h.tick();
    }
    panic!("the map was never drawn");
}

fn map_image(h: &Harness) -> (u32, f32, f32) {
    h.paint(MAP)
        .into_iter()
        .find_map(|c| match c {
            PaintCmd::Image { image, rect, .. } => Some((image, rect.w, rect.h)),
            _ => None,
        })
        .expect("a map image")
}

/// The map images handed to the client (the arrow's shapes are handed over
/// too, once a window is attached).
fn map_uploads(h: &Harness) -> Vec<ac_plugin_abi::ui::RgbaImage> {
    h.sim
        .fake
        .requests::<calls::UiImageRgba>()
        .into_iter()
        .filter(|i| i.name == IMAGE_NAME)
        .collect()
}

fn pixel(rgba: &[u8], at: (usize, usize)) -> [u8; 4] {
    let i = (at.1 * SIDE + at.0) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
}

#[test]
fn each_vertex_has_its_pixel_where_the_game_puts_its_coordinates() {
    let map = TerrainMap {
        image: Image::default(),
    };
    assert_eq!(map.size(), (2041.0, 2041.0));
    // South-west corner at the bottom left, north-east at the top right.
    assert_eq!(vertex_pixel(0x0000, 0, 0), Some((0, 2040)));
    assert_eq!(vertex_pixel(0xFEFE, 8, 8), Some((2040, 0)));
    assert_eq!(
        vertex_pixel(HOLTBURG, 3, 5),
        Some((0xA9 * 8 + 3, 2040 - (0xB4 * 8 + 5)))
    );
    // A landblock's east edge is its neighbour's west edge.
    assert_eq!(vertex_pixel(0x1020, 8, 2), vertex_pixel(0x1120, 0, 2));
    assert_eq!(vertex_pixel(0xFF00, 0, 0), None, "past the map");
    assert_eq!(vertex_pixel(HOLTBURG, 9, 0), None, "past the grid");
    assert!(close(pixel_coordinates(0, 2040), (-101.95, -101.95)));
    assert!(close(pixel_coordinates(2040, 0), (102.05, 102.05)));
    // Each vertex sits where the SDK's landblock conversion puts it, at
    // its pixel's middle, both ways.
    for (landblock, x, y) in [
        (HOLTBURG, 0, 0),
        (HOLTBURG, 3, 5),
        (0x0000, 0, 0),
        (0xFEFE, 8, 8),
        (0x7F7F, 4, 4),
        (0x1A2B, 7, 1),
    ] {
        let p = Position::from_cell(
            u32::from(landblock) << 16 | 1,
            [24.0 * x as f32, 24.0 * y as f32, 0.0],
            0.0,
        );
        let (px, py) = vertex_pixel(landblock, x, y).unwrap();
        assert!(
            close(pixel_coordinates(px, py), (p.north_south, p.east_west)),
            "{landblock:04X} ({x}, {y})"
        );
        assert!(close(
            map.to_pixel(p.north_south, p.east_west),
            (px as f64 + 0.5, py as f64 + 0.5)
        ));
        assert!(close(
            map.to_coords(px as f64 + 0.5, py as f64 + 0.5),
            (p.north_south, p.east_west)
        ));
    }
    for (ns, ew) in [(42.1, 33.6), (-12.3, -4.0), (0.0, 0.0), (99.9, -99.9)] {
        let (x, y) = map.to_pixel(ns, ew);
        assert!(close(map.to_coords(x, y), (ns, ew)));
    }
    // North is up, east is right, ten pixels to a map unit.
    let (x0, y0) = map.to_pixel(0.0, 0.0);
    let (x1, y1) = map.to_pixel(1.0, 1.0);
    assert!(((x1 - x0) - 10.0).abs() < 1e-9 && ((y0 - y1) - 10.0).abs() < 1e-9);
}

#[test]
fn the_terrain_map_covers_the_view_at_the_least_zoom_and_shows_a_vertex_32_pixels_wide_at_the_most()
{
    let map = TerrainMap {
        image: Image::default(),
    };
    // The least zoom just covers the view: the map's width is the body's.
    let least = Zoom::new(f64::NEG_INFINITY, (BODY.2, BODY.3));
    let middle = pixel_coordinates(1020, 1020);
    let whole = DerethView::new(&map, least.multiplier(), middle.0, middle.1);
    assert!((whole.scale - f64::from(BODY.2) / 2041.0).abs() < 1e-12);
    let (left, top) = whole.to_canvas((0.0, 0.0));
    let (right, bottom) = whole.to_canvas((2041.0, 2041.0));
    assert!(
        (left - BODY.0).abs() < 0.01 && (right - (BODY.0 + BODY.2)).abs() < 0.01,
        "{left} {right}"
    );
    assert!(top <= BODY.1 && bottom >= BODY.1 + BODY.3, "{top} {bottom}");
    // At the most zoom (factor 10, a zoom of 32) a vertex (24 m) is 32
    // canvas pixels wide: the terrain map has the original's 10 pixels to
    // a map unit.
    let most = Zoom::new(100.0, (BODY.2, BODY.3));
    assert_eq!(most.multiplier(), 32.0);
    let (ns, ew) = pixel_coordinates(0xA9 * 8 + 4, 2040 - (0xB4 * 8 + 4));
    let close_up = DerethView::new(&map, most.multiplier(), ns, ew);
    let here = close_up.to_canvas(map.to_pixel(ns, ew));
    assert_eq!(here, (BODY.0 + BODY.2 / 2.0, BODY.1 + BODY.3 / 2.0));
    let east = close_up.to_canvas(map.to_pixel(ns, ew + 0.1));
    let step = f64::from(east.0 - here.0);
    assert!((step - 32.0).abs() < 1e-3, "{step}");
}

#[test]
fn a_vertex_takes_its_textures_colour_lifted_the_roads_and_a_gentle_relief() {
    let colours = TerrainColors::from_palette(&palette()).expect("a palette");
    // The texture's mean, lifted by 1.3: Grassland 5A5E30.
    assert_eq!(BRIGHTNESS, 1.3);
    assert_eq!(colours.base(1, 0), [117, 122, 62]);
    assert_eq!(colours.base(7, 0), type_colour(7));
    assert_eq!(
        colours.base(1, 2),
        shaded(rgb(ROAD), BRIGHTNESS),
        "a road over the vertex: the road's texture"
    );
    // Not the map colour codes: rock is not green; marsh, moss, olthoi
    // land and the desolate lands are not pale blue.
    let [r, g, _] = colours.base(0, 0);
    assert!(r > g, "BarrenRock {:?}", colours.base(0, 0));
    for t in [4, 28, 29, 30, 31] {
        let [r, g, b] = colours.base(t, 0);
        assert!(b < r && b < g, "type {t}: {:?}", colours.base(t, 0));
    }
    // Brighter, never past white.
    assert_eq!(colours.base(15, 0), [250, 255, 255], "snow, clamped");
    // A road whose texture gave no colour leaves the ground's.
    let mut no_road = palette();
    no_road[32].rgb = 0;
    no_road[32].map_rgb = 0;
    let colours_without = TerrainColors::from_palette(&no_road).unwrap();
    assert_eq!(colours_without.base(1, 1), type_colour(1));
    // A type whose texture gave no colour takes the missing colour.
    let mut no_grass = palette();
    no_grass[1].rgb = 0;
    let colours_without = TerrainColors::from_palette(&no_grass).unwrap();
    assert_eq!(colours_without.base(1, 0), colours_without.missing());
    assert!(TerrainColors::from_palette(&[]).is_none(), "no game files");
    // Flat ground keeps its colour; light comes from the north-west.
    assert_eq!(relief(0.0, 0.0), 1.0);
    assert_eq!(shaded([100, 200, 250], 1.0), [100, 200, 250]);
    assert!(relief(0.5, 0.0) > 1.0, "rising east faces west: lit");
    assert!(relief(0.0, -0.5) > 1.0, "falling north faces north: lit");
    assert!(
        relief(-0.5, 0.0) < 1.0 && relief(0.0, 0.5) < 1.0,
        "in shade"
    );
    assert!(relief(0.5, -0.5) > relief(0.5, 0.0), "facing north-west");
    // Gently: a 1 in 4 slope changes the colour by under 15 %, and even a
    // cliff keeps the colour recognisable.
    let gentle = relief(0.25, 0.0);
    assert!(gentle > 1.0 && gentle < 1.15, "{gentle}");
    for (east, north) in [(4.0, -4.0), (-4.0, 4.0), (4.0, 4.0), (-4.0, -4.0)] {
        let r = relief(east, north);
        assert!((0.65..=1.3).contains(&r), "{east} {north}: {r}");
    }
    assert_eq!(shaded([100, 200, 250], 1.2), [120, 240, 255]);
    assert_eq!(shaded([100, 200, 250], 0.5), [50, 100, 125]);
}

#[test]
fn water_reads_as_water_blue_and_apart_from_the_land() {
    let colours = TerrainColors::from_palette(&palette()).unwrap();
    assert_eq!(WATER_TYPES, 16..=20);
    // Faux running water (22) has running water's texture: it is meant to
    // look like water.
    let land: Vec<u8> = (0..32u8)
        .filter(|t| !WATER_TYPES.contains(t) && *t != 22)
        .collect();
    for water in WATER_TYPES {
        let c = colours.base(water, 0);
        let [r, g, b] = c;
        assert!(b > g && b > r, "type {water} is blue: {c:?}");
        for &t in &land {
            let other = colours.base(t, 0);
            assert!(
                distance(c, other) > 40.0,
                "type {water} {c:?} against type {t} {other:?}"
            );
        }
    }
    // Deeper water is darker.
    let luminance =
        |[r, g, b]: [u8; 3]| 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b);
    assert!(luminance(colours.base(20, 0)) < luminance(colours.base(18, 0)));
    assert!(luminance(colours.base(18, 0)) < luminance(colours.base(16, 0)));
}

#[test]
fn a_landblock_without_terrain_takes_the_deep_seas_colour() {
    let colours = TerrainColors::from_palette(&palette()).unwrap();
    assert_eq!(colours.missing(), colours.base(20, 0));
    // Found by its number, whatever its name.
    let mut renamed = palette();
    renamed[20].name = None;
    assert_eq!(
        TerrainColors::from_palette(&renamed).unwrap().missing(),
        colours.base(20, 0)
    );
    // Without a deep-sea texture: the darkest colour (Obsidian Plain).
    let mut no_sea = palette();
    no_sea[20].rgb = 0;
    assert_eq!(
        TerrainColors::from_palette(&no_sea).unwrap().missing(),
        type_colour(6)
    );
    let mut black: Vec<TerrainColor> = palette();
    for c in &mut black {
        c.rgb = 0;
    }
    assert_eq!(
        TerrainColors::from_palette(&black).unwrap().missing(),
        [0, 0, 0]
    );
}

#[test]
fn the_map_is_read_a_page_of_landblocks_at_a_time_over_ticks() {
    let mut h = with_window(false);
    assert!(
        h.sim.fake.requests::<calls::LandscapeTerrain>().is_empty(),
        "nothing is read while the map is hidden"
    );
    assert!(h.plugin.map().unwrap().terrain_progress().is_none());
    h.with_host(|app, host| app.set_map_visible(host, true));
    let mut ticks = 0;
    let mut seen_progress = Vec::new();
    let mut footer_said = false;
    while h.plugin.map().unwrap().terrain_image().is_none() {
        let before = h.sim.fake.requests::<calls::LandscapeTerrain>().len();
        h.tick();
        ticks += 1;
        assert!(ticks < 2000, "never drawn");
        let pages = h.sim.fake.requests::<calls::LandscapeTerrain>();
        assert!(
            pages.len() - before <= CALLS_PER_TICK,
            "{} calls in one tick",
            pages.len() - before
        );
        if let Some(p) = h.plugin.map().unwrap().terrain_progress() {
            seen_progress.push(p);
        }
        footer_said |= h
            .painted_texts(MAP)
            .iter()
            .any(|t| t.starts_with("Drawing the map... "));
    }
    let pages = h.sim.fake.requests::<calls::LandscapeTerrain>();
    assert_eq!(pages.len(), PAGES);
    assert!(pages.iter().all(|p| p.len() <= LANDSCAPE_TERRAIN_MAX));
    let asked: BTreeSet<u16> = pages.iter().flatten().copied().collect();
    assert_eq!(asked.len(), 255 * 255, "every landblock, once");
    assert_eq!(pages.iter().map(Vec::len).sum::<usize>(), 255 * 255);
    assert!(asked.iter().all(|lb| lb >> 8 <= 0xFE && lb & 0xFF <= 0xFE));
    assert!(ticks >= PAGES / CALLS_PER_TICK, "{ticks} ticks");
    assert_eq!(
        h.sim
            .fake
            .requests::<calls::LandscapeTerrainPalette>()
            .len(),
        1,
        "the palette is asked once"
    );
    assert!(seen_progress.windows(2).all(|w| w[0] <= w[1]));
    assert!(seen_progress.first().is_some_and(|p| *p < 0.1));
    assert!(footer_said, "the footer says the map is being drawn");
}

#[test]
fn the_drawn_map_is_handed_to_the_client_once() {
    let mut h = with_window(true);
    tick_until_drawn(&mut h);
    let images = map_uploads(&h);
    assert_eq!(images.len(), 1);
    let image = &images[0];
    assert_eq!(image.name, IMAGE_NAME);
    assert_eq!((image.width, image.height), (2041, 2041));
    assert_eq!(image.rgba.len(), 2041 * 2041 * 4);
    let held = h.plugin.map().unwrap().terrain_image().unwrap();
    assert!(held.is_valid());
    assert_eq!((held.width, held.height), (2041, 2041));
    // The colours: the road and the ground shaded by the slope east
    // (10 m in 24), a landblock the files lack in the deep sea's colour.
    let slope = relief(10.0 / 24.0, 0.0);
    let road = vertex_pixel(HOLTBURG, 4, 4).unwrap();
    let ground = vertex_pixel(HOLTBURG, 2, 3).unwrap();
    let [r, g, b] = shaded(shaded(rgb(ROAD), BRIGHTNESS), slope);
    assert_eq!(pixel(&image.rgba, road), [r, g, b, 255]);
    let [r, g, b] = shaded(type_colour(1), slope);
    assert_eq!(pixel(&image.rgba, ground), [r, g, b, 255]);
    let [r, g, b] = deep_sea();
    assert_eq!(
        pixel(&image.rgba, vertex_pixel(0x0000, 4, 4).unwrap()),
        [r, g, b, 255]
    );
    // Nothing more is read or handed over, even after a window comes back.
    h.sim.fake.ui.set_window_attached(false);
    h.send(Event::WindowAttached(false));
    h.sim.fake.ui.set_window_attached(true);
    h.send(Event::WindowAttached(true));
    for _ in 0..100 {
        h.tick();
    }
    assert_eq!(map_uploads(&h).len(), 1);
    assert_eq!(
        h.sim.fake.requests::<calls::LandscapeTerrain>().len(),
        PAGES
    );
    assert_eq!(h.plugin.map().unwrap().terrain_image(), Some(held));
}

#[test]
fn the_map_shows_the_games_map_until_its_own_is_drawn() {
    let mut h = with_window(false);
    h.with_host(|app, host| app.set_map_visible(host, true));
    for _ in 0..10 {
        h.tick();
    }
    assert!(h.plugin.map().unwrap().terrain_image().is_none());
    let (_, w, hgt) = map_image(&h);
    assert!(
        (w / hgt - 257.0 / 267.0).abs() < 1e-3,
        "the game's world map, {w} x {hgt}"
    );
    let texts = h.painted_texts(MAP);
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("Drawing the map... ") && t.contains("% · You 0.0N, 0.0E")),
        "{texts:?}"
    );
    tick_until_drawn(&mut h);
    for _ in 0..goarrow::map::PAINT_TICKS {
        h.tick();
    }
    let drawn = h.plugin.map().unwrap().terrain_image().unwrap();
    let (image, w, hgt) = map_image(&h);
    assert_eq!(image, drawn.handle);
    assert!((w - hgt).abs() < 1e-3, "{w} x {hgt}");
    let texts = h.painted_texts(MAP);
    assert!(
        !texts.iter().any(|t| t.starts_with("Drawing the map")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t.starts_with("You 0.0N, 0.0E")));
}

#[test]
fn a_headless_session_draws_no_map_until_a_window_attaches() {
    let mut h = Harness::new(|sim| {
        sim.show_map();
        sim.fake
            .set_reply::<calls::LandscapeTerrainPalette>(&palette());
        sim.fake
            .set_reply::<calls::LandscapeTerrain>(&vec![holtburg()]);
        sim.stand(0.0, 0.0, 0.0);
    });
    for _ in 0..200 {
        h.tick();
    }
    assert!(
        h.sim
            .fake
            .requests::<calls::LandscapeTerrainPalette>()
            .is_empty()
    );
    assert!(h.sim.fake.requests::<calls::LandscapeTerrain>().is_empty());
    assert!(h.sim.fake.requests::<calls::UiImageRgba>().is_empty());
    assert!(h.plugin.map().unwrap().terrain_progress().is_none());
    h.sim.fake.window_attached = true;
    h.sim.fake.ui.set_window_attached(true);
    h.send(Event::WindowAttached(true));
    tick_until_drawn(&mut h);
    assert_eq!(map_uploads(&h).len(), 1);
}

#[test]
fn without_the_game_files_the_games_map_stays() {
    // The palette answers empty, as on a client without the game files.
    let mut h = Harness::new(|sim| {
        sim.show_map();
        sim.fake.window_attached = true;
        sim.fake.ui.set_window_attached(true);
        sim.fake
            .set_reply::<calls::LandscapeTerrain>(&vec![holtburg()]);
        sim.stand(0.0, 0.0, 0.0);
    });
    for _ in 0..200 {
        h.tick();
    }
    assert_eq!(
        h.sim
            .fake
            .requests::<calls::LandscapeTerrainPalette>()
            .len(),
        1,
        "asked once"
    );
    assert!(h.sim.fake.requests::<calls::LandscapeTerrain>().is_empty());
    assert!(h.plugin.map().unwrap().terrain_image().is_none());
    assert!(h.plugin.map().unwrap().terrain_progress().is_none());
    let (_, w, hgt) = map_image(&h);
    assert!((w / hgt - 257.0 / 267.0).abs() < 1e-3);
    assert!(
        !h.painted_texts(MAP)
            .iter()
            .any(|t| t.starts_with("Drawing the map"))
    );
}
