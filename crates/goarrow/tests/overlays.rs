//! The overlays: the arrow (`GoArrowHud.cs`; GoArrow's `HudCanvasTests`),
//! the map with its projection, and the route on the ground.

mod common;

use ac_plugin_abi::Wire;
use ac_plugin_abi::ui::{Color, ThemeInfo, ThemeKind, ThemePalette};
use ac_plugin_abi::world::{DungeonLayer, DungeonWall, Floorplan};
use ac_plugin_ui::{PaintCmd, PointerKind};
use common::{ATLAS_XML, Harness};
use goarrow::hud::{self, DIAL_CENTER, GO_BUTTON, HudPalette, MAP_BUTTON, pointer_angle, readout};
use goarrow::map::{self, BODY, DerethView, GameWorldMap, MapViewKind, map_coords, map_pixel};
use goarrow::map_icons::MapIcon;
use goarrow::sprites::Sprite;
use openac_plugin::ui::Image;
use openac_plugin::{Event, Host, Import, Plugin, Transport, calls};

const ARROW: &str = "goarrow.arrow";
const MAP: &str = "goarrow.map";

fn attach_window(sim: &mut common::Sim) {
    sim.fake.window_attached = true;
    sim.fake.ui.set_window_attached(true);
}

fn csv_route(heading: f32) -> Harness {
    csv_route_with(heading, false)
}

/// GoArrow's test route, with a window attached or not.
fn csv_route_with(heading: f32, window: bool) -> Harness {
    // GoArrow's test data: Start 0;0, First 0;10, Finish 10;10.
    let mut h = Harness::new(|sim| {
        sim.show_map();
        if window {
            attach_window(sim);
        }
        sim.storage.insert(
            "GoArrow/legs.xml".to_owned(),
            "<locations><loc name='First' NS='0' EW='10'/><loc name='Finish' NS='10' EW='10'/></locations>"
                .to_owned(),
        );
        sim.stand(0.0, 0.0, heading);
    });
    h.go("file legs.xml");
    h.settle();
    h.go("to Finish");
    h.settle();
    h
}

/// The turned images a canvas painted: image, rect (x, y, w, h), tint
/// and rotation, in paint order.
type Rect = (f32, f32, f32, f32);
type Tint = (u8, u8, u8, u8);
type Turned = (u32, Rect, Tint, f32);

fn turned(h: &Harness, canvas: &str) -> Vec<Turned> {
    h.paint(canvas)
        .into_iter()
        .filter_map(|c| match c {
            PaintCmd::ImageTransformed {
                image,
                rect,
                tint,
                rotation,
                ..
            } => Some((
                image,
                (rect.x, rect.y, rect.w, rect.h),
                (tint.r, tint.g, tint.b, tint.a),
                rotation,
            )),
            _ => None,
        })
        .collect()
}

/// Where a sprite was painted turned: its index among the turned images,
/// and the image.
fn layer(h: &Harness, canvas: &str, sprite: Sprite) -> (usize, Turned) {
    let image = h.plugin.sprites().image(sprite).handle;
    assert_ne!(image, 0, "{sprite:?} was handed to the client");
    turned(h, canvas)
        .into_iter()
        .enumerate()
        .find(|(_, t)| t.0 == image)
        .unwrap_or_else(|| panic!("{sprite:?} painted turned on {canvas}"))
}

/// Where a sprite was painted upright: its rect and tint.
fn upright(h: &Harness, canvas: &str, sprite: Sprite) -> (Rect, Tint) {
    let image = h.plugin.sprites().image(sprite).handle;
    assert_ne!(image, 0, "{sprite:?} was handed to the client");
    h.paint(canvas)
        .into_iter()
        .find_map(|c| match c {
            PaintCmd::Image {
                image: i,
                rect,
                tint,
            } if i == image => Some((
                (rect.x, rect.y, rect.w, rect.h),
                (tint.r, tint.g, tint.b, tint.a),
            )),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{sprite:?} painted on {canvas}"))
}

fn middle(rect: (f32, f32, f32, f32)) -> (f32, f32) {
    (rect.0 + rect.2 / 2.0, rect.1 + rect.3 / 2.0)
}

fn near(a: (f32, f32), b: (f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
}

fn colour(c: hud::Rgba) -> (u8, u8, u8, u8) {
    (c.0, c.1, c.2, c.3)
}

#[test]
fn the_map_projection_follows_the_client_map_panel_and_inverts() {
    let (x, y) = map_pixel(0.0, 0.0);
    assert!((x - 127.0).abs() < 1e-9, "{x}");
    assert!((y - (8.0 + 251.0 * 1023.0 / 2048.0)).abs() < 1e-9, "{y}");
    // The corners of the marker area.
    assert_eq!(map_pixel(102.3, -102.4), (6.0, 8.0));
    let (x, y) = map_pixel(-102.5, 102.4);
    assert!(
        (x - 248.0).abs() < 1e-9 && (y - 259.0).abs() < 1e-9,
        "{x} {y}"
    );
    for (ns, ew) in [(42.1, 33.6), (-12.3, -4.0), (0.0, 0.0), (99.9, -99.9)] {
        let (x, y) = map_pixel(ns, ew);
        let (back_ns, back_ew) = map_coords(x, y);
        assert!((back_ns - ns).abs() < 1e-9 && (back_ew - ew).abs() < 1e-9);
    }
    // North is up, east is right.
    assert!(map_pixel(10.0, 0.0).1 < map_pixel(0.0, 0.0).1);
    assert!(map_pixel(0.0, 10.0).0 > map_pixel(0.0, 0.0).0);
}

#[test]
fn the_dereth_view_scales_the_games_map_to_the_original_maps_zoom_and_maps_back() {
    let background = GameWorldMap {
        image: Image::default(),
    };
    // A zoom of 1 is the original map's 10 canvas pixels to a map unit,
    // whatever the background's own pixels.
    let view = DerethView::new(&background, 1.0, 0.0, 0.0);
    let (a, b) = (map_pixel(0.0, 0.0), map_pixel(0.0, 1.0));
    let east = view.to_canvas(b).0 - view.to_canvas(a).0;
    assert!((east - 10.0).abs() < 1e-3, "{east}");
    let zoomed = DerethView::new(&background, 4.0, 42.1, 33.6);
    assert!((zoomed.scale - 4.0 * view.scale).abs() < 1e-12);
    let middle = (BODY.0 + BODY.2 / 2.0, BODY.1 + BODY.3 / 2.0);
    assert_eq!(zoomed.to_canvas(map_pixel(42.1, 33.6)), middle);
    let pixel = zoomed.to_pixel((100.0, 200.0));
    let back = zoomed.to_canvas(pixel);
    assert!((back.0 - 100.0).abs() < 1e-3 && (back.1 - 200.0).abs() < 1e-3);
    // Centred past the map's edge, the view moves in so the edge stays at
    // the body's.
    let edge = DerethView::new(&background, 4.0, 0.0, -102.4);
    let left = edge.to_canvas((0.0, 0.0)).0;
    assert!((left - BODY.0).abs() < 1e-3, "{left}");
}

#[test]
fn the_pointer_turns_by_bearing_less_heading() {
    assert_eq!(pointer_angle(90.0, 0.0), Some(std::f64::consts::FRAC_PI_2));
    assert_eq!(
        pointer_angle(90.0, 135.0),
        Some(-std::f64::consts::FRAC_PI_4)
    );
    assert_eq!(pointer_angle(f64::NAN, 0.0), None);
    assert_eq!(pointer_angle(0.0, f64::INFINITY), None);
}

// HudCanvasTests.DisplayTogglesControlArrowReadout
#[test]
fn the_readout_follows_the_distance_and_bearing_switches() {
    assert_eq!(readout(true, true, 184.0 / 240.0, 52.0), "184 m · 52°");
    assert_eq!(readout(false, true, 1.0, 52.4), "52°");
    assert_eq!(readout(true, false, 10.0, 52.0), "2.4 km");
    assert_eq!(readout(false, false, 1.0, 52.0), "");
    assert_eq!(readout(false, true, 1.0, f64::NAN), "--°");
    let mut h = csv_route(0.0);
    for (distance, bearing) in [(true, true), (true, false), (false, true), (false, false)] {
        h.with_host(|app, host| {
            app.update_settings(host, |s| {
                s.show_distance = distance;
                s.show_bearing = bearing;
            });
        });
        h.tick();
        let texts = h.painted_texts(ARROW);
        let shown = texts
            .iter()
            .find(|t| t.contains('°') || t.ends_with(" m") || t.ends_with(" km"));
        assert_eq!(shown.is_some_and(|t| t.contains('°')), bearing);
        assert_eq!(
            shown.is_some_and(|t| t.contains(" m") || t.contains(" km")),
            distance
        );
    }
}

// HudCanvasTests.VisibleArrowUsesCanvasAndTurnsTowardFirstWaypoint: the
// pointer, now a smooth image turned about the dial's middle.
#[test]
fn the_arrow_turns_toward_the_first_waypoint() {
    let mut h = csv_route_with(0.0, true);
    assert_eq!(h.sim.fake.ui.canvases().count(), 2);
    let texts = h.painted_texts(ARROW);
    assert_eq!(texts[0], "Next · step 1 of 2");
    assert_eq!(texts[1], "First");
    // First is due east; facing north, the arrow turns a quarter clockwise.
    let (at, (_, rect, _, rotation)) = layer(&h, ARROW, Sprite::Pointer);
    assert!(
        (rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
        "{rotation}"
    );
    assert!(near(middle(rect), DIAL_CENTER), "{rect:?}");
    // Its dark edge is drawn under it, turned alike, a pixel lower.
    let (edge_at, (_, edge, _, edge_rotation)) = layer(&h, ARROW, Sprite::PointerEdge);
    assert!(edge_at < at);
    assert_eq!(edge_rotation, rotation);
    assert!(near(middle(edge), (DIAL_CENTER.0, DIAL_CENTER.1 + 1.0)));
    let (lit_at, (_, _, _, lit_rotation)) = layer(&h, ARROW, Sprite::PointerLit);
    assert!(
        lit_at > at && lit_rotation == rotation,
        "its lit half over it"
    );
    h.sim.stand(0.0, 0.0, 90.0);
    h.tick();
    let (_, (_, _, _, rotation)) = layer(&h, ARROW, Sprite::Pointer);
    assert!(rotation.abs() < 1e-3, "facing east, east is straight ahead");
    h.sim.stand(0.0, 0.0, 180.0);
    h.tick();
    let (_, (_, _, _, rotation)) = layer(&h, ARROW, Sprite::Pointer);
    assert!(
        (rotation + std::f32::consts::FRAC_PI_2).abs() < 1e-3,
        "facing south, east is to the left: {rotation}"
    );
}

#[test]
fn the_arrow_is_dim_until_walking_and_bright_while_walking() {
    let mut h = csv_route_with(0.0, true);
    let classic = HudPalette::CLASSIC;
    let (_, (_, _, idle, _)) = layer(&h, ARROW, Sprite::Pointer);
    assert_eq!(idle, colour(classic.pointer_dim));
    let (x, y) = middle(GO_BUTTON);
    h.pointer(ARROW, PointerKind::Down, x, y, 0);
    h.pointer(ARROW, PointerKind::Up, x, y, 0);
    assert!(h.plugin.is_navigating());
    h.tick();
    let (_, (_, _, walking, _)) = layer(&h, ARROW, Sprite::Pointer);
    assert_eq!(walking, colour(classic.pointer));
    let sum = |c: (u8, u8, u8, u8)| u32::from(c.0) + u32::from(c.1) + u32::from(c.2);
    assert!(sum(walking) > sum(idle) + 60, "{walking:?} {idle:?}");
    // Under a theme, its accent.
    let c = |r, g, b| Color { r, g, b, a: 255 };
    h.send(Event::ThemeChanged(ThemeInfo {
        kind: ThemeKind::Moss,
        display_name: "Charcoal + moss".to_owned(),
        palette: Some(ThemePalette {
            background: c(23, 29, 27),
            field: c(16, 22, 19),
            border: c(53, 68, 59),
            text: c(224, 232, 226),
            muted: c(154, 169, 158),
            accent: c(151, 190, 129),
            selected: c(52, 75, 55),
        }),
    }));
    h.tick();
    let (_, (_, _, walking, _)) = layer(&h, ARROW, Sprite::Pointer);
    assert_eq!(walking, (151, 190, 129, 255));
}

#[test]
fn the_dial_and_the_buttons_are_smooth_images_not_line_segments() {
    let h = csv_route_with(0.0, true);
    let paint = h.paint(ARROW);
    assert!(
        !paint.iter().any(|c| matches!(c, PaintCmd::Line { .. })),
        "{paint:?}"
    );
    let classic = HudPalette::CLASSIC;
    let (ring, tint) = upright(&h, ARROW, Sprite::Ring);
    assert!(near(middle(ring), DIAL_CENTER));
    assert_eq!(tint, colour(classic.ring));
    let (ticks, _) = upright(&h, ARROW, Sprite::Ticks);
    assert!(near(middle(ticks), DIAL_CENTER));
    let (go, _) = upright(&h, ARROW, Sprite::Go);
    assert!(near(middle(go), middle(GO_BUTTON)));
    let (map_icon, _) = upright(&h, ARROW, Sprite::MapIcon);
    assert!(near(middle(map_icon), middle(MAP_BUTTON)));
}

#[test]
fn the_shapes_are_handed_to_the_client_once_not_every_paint() {
    let mut h = csv_route_with(0.0, true);
    for step in 0..40 {
        h.sim.stand(0.0, 0.0, step as f32 * 9.0);
        h.tick();
    }
    h.send(Event::WindowAttached(true));
    h.settle();
    let uploads = h.sim.fake.requests::<calls::UiImageRgba>();
    for sprite in Sprite::ALL {
        let mine: Vec<_> = uploads.iter().filter(|u| u.name == sprite.name()).collect();
        assert_eq!(mine.len(), 1, "{sprite:?}");
        let mask = sprite.mask();
        assert_eq!((mine[0].width, mine[0].height), (mask.width, mask.height));
        assert_eq!(mine[0].rgba, mask.rgba());
    }
    assert!(h.plugin.sprites().ready());
    let (_, (_, _, _, rotation)) = layer(&h, ARROW, Sprite::Pointer);
    assert!(rotation.is_finite(), "still drawn");
    // A session without a window hands over none.
    let mut headless = csv_route_with(0.0, false);
    headless.settle();
    assert!(
        headless
            .sim
            .fake
            .requests::<calls::UiImageRgba>()
            .is_empty()
    );
    assert!(!headless.plugin.sprites().ready());
}

#[test]
fn indoors_the_arrow_says_the_route_waits() {
    let mut h = csv_route(0.0);
    h.sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    h.tick();
    let texts = h.painted_texts(ARROW);
    assert_eq!(texts[0], "OUTDOOR ROUTE PAUSED");
    assert_eq!(texts[1], "Resumes outdoors");
}

// HudCanvasTests: a drag moves the arrow and the drop is saved.
#[test]
fn dragging_the_arrow_moves_it_and_saves_where_it_was_dropped() {
    let mut h = csv_route(0.0);
    h.pointer(ARROW, PointerKind::Down, 40.0, 40.0, 0);
    h.pointer(ARROW, PointerKind::Move, 70.0, 60.0, 0);
    assert_eq!(h.plugin.hud().unwrap().offset(), (-40.0, 140.0));
    // The client measures from where the arrow is now: the same spot.
    h.pointer(ARROW, PointerKind::Move, 40.0, 40.0, 0);
    assert_eq!(h.plugin.hud().unwrap().offset(), (-40.0, 140.0));
    h.pointer(ARROW, PointerKind::Up, 40.0, 40.0, 0);
    assert_eq!(h.saved().arrow_offset_x, -40.0);
    assert_eq!(h.saved().arrow_offset_y, 140.0);
    let arrow = h.canvas(ARROW);
    let view = h
        .sim
        .fake
        .ui
        .canvases()
        .find(|c| c.id == arrow)
        .unwrap()
        .offset;
    assert_eq!(view, (-40.0, 140.0));
}

#[test]
fn the_arrows_buttons_go_and_stop_and_show_the_map() {
    let mut h = csv_route(0.0);
    let middle = |r: (f32, f32, f32, f32)| (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    let (x, y) = middle(GO_BUTTON);
    h.pointer(ARROW, PointerKind::Down, x, y, 0);
    h.pointer(ARROW, PointerKind::Up, x + 1.0, y, 0);
    assert!(h.plugin.is_navigating(), "Go: the route is being computed");
    h.pointer(ARROW, PointerKind::Down, x, y, 0);
    h.pointer(ARROW, PointerKind::Up, x, y, 0);
    assert!(!h.plugin.is_navigating(), "Stop");
    let (x, y) = middle(MAP_BUTTON);
    h.pointer(ARROW, PointerKind::Down, x, y, 0);
    h.pointer(ARROW, PointerKind::Up, x, y, 0);
    assert!(!h.saved().map_visible);
    h.pointer(ARROW, PointerKind::Down, x, y, 0);
    h.pointer(ARROW, PointerKind::Up, x, y, 0);
    assert!(h.saved().map_visible);
    assert_eq!(hud::button_at(10.0, 10.0), None);
}

#[test]
fn the_overlays_paint_in_the_theme_and_again_when_it_changes() {
    let mut h = csv_route(0.0);
    let face = |h: &Harness, name: &str| {
        h.paint(name)
            .into_iter()
            .find_map(|c| match c {
                PaintCmd::FillRect { color, .. } => Some((color.r, color.g, color.b)),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(face(&h, ARROW), (12, 20, 29), "GoArrow's own under Classic");
    let c = |r, g, b| Color { r, g, b, a: 255 };
    h.send(Event::ThemeChanged(ThemeInfo {
        kind: ThemeKind::Moss,
        display_name: "Charcoal + moss".to_owned(),
        palette: Some(ThemePalette {
            background: c(23, 29, 27),
            field: c(16, 22, 19),
            border: c(53, 68, 59),
            text: c(224, 232, 226),
            muted: c(154, 169, 158),
            accent: c(151, 190, 129),
            selected: c(52, 75, 55),
        }),
    }));
    h.settle();
    assert_eq!(face(&h, ARROW), (23, 29, 27));
    assert_eq!(face(&h, MAP), (23, 29, 27));
}

#[test]
fn the_map_draws_the_route_the_player_and_the_destination() {
    let mut h = Harness::new(|sim| {
        sim.show_map();
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 0.0);
    });
    h.go("Rithwic");
    h.settle();
    // Zoomed in enough that the first walk (one map unit) is longer than
    // the shortest line drawn.
    for _ in 0..4 {
        h.pointer(MAP, PointerKind::Wheel, 293.0, 286.0, 1);
    }
    h.settle();
    let paint = h.paint(MAP);
    let pale = paint.iter().filter(|c| matches!(c, PaintCmd::Line { color, .. } if (color.r, color.g, color.b) == (204, 255, 204))).count();
    let pink = paint.iter().filter(|c| matches!(c, PaintCmd::Line { color, .. } if (color.r, color.g, color.b) == (255, 102, 255))).count();
    assert!(pale >= 2, "the two walks, {pale}");
    assert!(pink >= 8, "a dashed hop, {pink}");
    assert!(
        paint.iter().any(|c| matches!(c, PaintCmd::Image { .. })),
        "the world map"
    );
    let texts = h.painted_texts(MAP);
    assert!(texts.contains(&"To Rithwic".to_owned()), "{texts:?}");
    assert!(
        texts.iter().any(|t| t.starts_with("You 0.0N, 0.0E")),
        "{texts:?}"
    );
}

#[test]
fn the_map_shows_the_player_as_the_arrow_and_the_destination_as_the_originals_icon() {
    let mut h = Harness::new(|sim| {
        sim.show_map();
        attach_window(sim);
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 45.0);
    });
    h.go("Rithwic");
    h.settle();
    let view = h.plugin.map().unwrap().dereth_view();
    let (at, (_, rect, tint, rotation)) = layer(&h, MAP, Sprite::Marker);
    assert!(
        (rotation - std::f32::consts::FRAC_PI_4).abs() < 1e-3,
        "turned to the heading: {rotation}"
    );
    assert!(near(middle(rect), view.to_canvas(map_pixel(0.0, 0.0))));
    assert_eq!(tint, colour(map::PLAYER_COLOR));
    let (edge_at, (_, _, edge_tint, edge_rotation)) = layer(&h, MAP, Sprite::MarkerEdge);
    assert!(edge_at < at && edge_rotation == rotation);
    assert_eq!(edge_tint, colour(map::EDGE_COLOR));
    // The original's destination icon, centred on Rithwic.
    let rithwic = h.plugin.destination().target().unwrap().coords;
    let icon = h.plugin.map_icons().image(MapIcon::ArrowDest2).handle;
    assert_ne!(icon, 0, "the icons were handed to the client");
    let rect = h
        .paint(MAP)
        .into_iter()
        .find_map(|c| match c {
            PaintCmd::Image { image, rect, .. } if image == icon => {
                Some((rect.x, rect.y, rect.w, rect.h))
            }
            _ => None,
        })
        .expect("the destination drawn");
    let place = view.to_canvas(map_pixel(rithwic.ns, rithwic.ew));
    assert!(near(middle(rect), place), "{rect:?} {place:?}");
}

/// The client refuses every canvas paint; everything else goes to the sim.
struct RefusingPaints<'a>(&'a mut common::Sim);

impl Transport for RefusingPaints<'_> {
    fn call(&mut self, import: Import, bytes: &[u8]) -> Vec<u8> {
        if import == Import::UiCanvasPaint {
            return Err::<(), String>("refused".to_owned()).to_bytes();
        }
        self.0.call(import, bytes)
    }
}

// A paint the client refused is tried again (the arrow did so already).
#[test]
fn a_refused_map_paint_is_tried_again() {
    let mut h = Harness::new(|sim| {
        sim.show_map();
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 0.0);
    });
    h.go("Rithwic");
    for _ in 0..map::PAINT_TICKS * 2 {
        h.plugin
            .tick(&mut Host::with_transport(&mut RefusingPaints(&mut h.sim)));
    }
    assert!(!h.painted_texts(MAP).contains(&"To Rithwic".to_owned()));
    h.tick();
    h.tick();
    let texts = h.painted_texts(MAP);
    assert!(texts.contains(&"To Rithwic".to_owned()), "{texts:?}");
}

#[test]
fn the_map_zooms_pans_and_sets_a_destination_where_clicked() {
    let mut h = Harness::new(|sim| {
        sim.show_map();
        sim.stand(0.0, 0.0, 0.0);
    });
    // A first start takes the earlier map's zoom of 1: the least zoom.
    let least = h.plugin.map().unwrap().zoom_factor();
    assert!((least - (586.0f64 / 2041.0).log(2f64.sqrt())).abs() < 1e-9);
    h.pointer(MAP, PointerKind::Wheel, 300.0, 300.0, 1);
    assert_eq!(h.plugin.map().unwrap().zoom_factor(), least + 1.0);
    assert_eq!(h.saved().map_zoom_factor, Some(least + 1.0));
    for _ in 0..20 {
        h.pointer(MAP, PointerKind::Wheel, 300.0, 300.0, 1);
    }
    assert_eq!(h.plugin.map().unwrap().zoom_factor(), 10.0);
    assert_eq!(h.plugin.map().unwrap().zoom(), 32.0);
    h.pointer(MAP, PointerKind::Down, 410.0, 18.0, 0);
    h.pointer(MAP, PointerKind::Up, 410.0, 18.0, 0);
    assert_eq!(
        h.plugin.map().unwrap().zoom_factor(),
        9.0,
        "the zoom-out button"
    );
    // A drag on the map pans it.
    let before = h.plugin.map().unwrap().center();
    h.pointer(MAP, PointerKind::Down, 300.0, 300.0, 0);
    h.pointer(MAP, PointerKind::Move, 340.0, 300.0, 0);
    h.pointer(MAP, PointerKind::Up, 340.0, 300.0, 0);
    let after = h.plugin.map().unwrap().center();
    assert!(
        after.1 < before.1,
        "dragged right, the map shows further west"
    );
    assert_eq!(h.saved().map_center_east_west, after.1);
    // A click sets a destination there, to a tenth.
    let at = (200.0, 250.0);
    let (ns, ew) = h.plugin.map().unwrap().coords_at(at);
    h.pointer(MAP, PointerKind::Down, at.0, at.1, 0);
    h.tick();
    assert!(
        h.painted_texts(MAP)
            .iter()
            .any(|t| t.starts_with("Cursor ")),
        "the footer names the point under the press"
    );
    h.pointer(MAP, PointerKind::Up, at.0, at.1, 0);
    let target = h.plugin.destination().target().unwrap().clone();
    assert!((target.coords.ns - (ns * 10.0).round() / 10.0).abs() < 1e-9);
    assert!((target.coords.ew - (ew * 10.0).round() / 10.0).abs() < 1e-9);
}

#[test]
fn the_maps_header_drags_it_follows_and_closes() {
    let mut h = Harness::new(|sim| {
        sim.show_map();
        sim.stand(3.0, 4.0, 0.0);
    });
    h.pointer(MAP, PointerKind::Down, 300.0, 10.0, 0);
    h.pointer(MAP, PointerKind::Move, 320.0, 30.0, 0);
    h.pointer(MAP, PointerKind::Up, 300.0, 10.0, 0);
    assert_eq!(h.plugin.map().unwrap().offset(), (45.0, 115.0));
    assert_eq!(h.saved().dungeon_offset_x, 45.0);
    h.pointer(MAP, PointerKind::Down, 500.0, 18.0, 0);
    h.pointer(MAP, PointerKind::Up, 500.0, 18.0, 0);
    assert!(h.plugin.map().unwrap().follows());
    h.tick();
    assert_eq!(h.plugin.map().unwrap().center(), (3.0, 4.0));
    h.pointer(MAP, PointerKind::Down, 566.0, 18.0, 0);
    h.pointer(MAP, PointerKind::Up, 566.0, 18.0, 0);
    assert!(!h.saved().map_visible);
    let shown = |h: &Harness, name: &str| {
        h.sim
            .fake
            .ui
            .canvases()
            .find(|c| c.descriptor.canvas_id == name)
            .is_some_and(|c| c.visible)
    };
    assert!(!shown(&h, MAP), "the X hides the map, not only its setting");
    assert_eq!(map::header_button(20.0, 18.0), Some(map::MapButton::Dereth));
}

#[test]
fn indoors_the_map_turns_to_the_dungeon_floorplan() {
    let h = Harness::new(|sim| {
        sim.show_map();
        sim.fake
            .set_reply::<calls::DungeonFloorplan>(&Some(Floorplan {
                landblock_id: 0x01AB,
                layers: vec![
                    DungeonLayer {
                        z: 0.0,
                        floors: vec![vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]],
                        walls: vec![DungeonWall {
                            start: (0.0, 0.0),
                            end: (20.0, 0.0),
                        }],
                    },
                    DungeonLayer {
                        z: -6.0,
                        floors: vec![],
                        walls: vec![DungeonWall {
                            start: (0.0, 5.0),
                            end: (20.0, 5.0),
                        }],
                    },
                ],
                cells: vec![],
                bounds_min: (0.0, 0.0, -6.0),
                bounds_max: (20.0, 20.0, 0.0),
            }));
        attach_window(sim);
        sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    });
    assert_eq!(h.plugin.map().unwrap().view(), MapViewKind::Dungeon);
    // The client wants the landblock in the high 16 bits (a cell id's form).
    assert_eq!(
        h.sim.fake.requests::<calls::DungeonFloorplan>(),
        [0x01AB_0000]
    );
    let texts = h.painted_texts(MAP);
    assert!(
        texts.iter().any(|t| t.starts_with("Dungeon 0x01AB")),
        "{texts:?}"
    );
    let walls = h
        .paint(MAP)
        .iter()
        .filter(|c| matches!(c, PaintCmd::Line { thickness, .. } if *thickness == 2.0))
        .count();
    assert!(walls >= 1);
    // The player: the same small arrow as on Dereth.
    let (_, (_, _, tint, rotation)) = layer(&h, MAP, Sprite::Marker);
    assert_eq!((tint, rotation), (colour(map::PLAYER_COLOR), 0.0));
    // Off, the map stays on Dereth indoors.
    let mut off = Harness::new(|sim| {
        sim.storage.insert(
            goarrow::settings::SETTINGS_KEY.to_owned(),
            r#"{ "DungeonMapVisible": false, "OverlayPositionVersion": 1, "VisibilitySettingsVersion": 1 }"#
                .to_owned(),
        );
        sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    });
    off.settle();
    assert_eq!(off.plugin.map().unwrap().view(), MapViewKind::Dereth);
}

#[test]
fn the_route_is_drawn_on_the_ground_and_taken_down_indoors() {
    let mut h = csv_route(0.0);
    let lines = h.sim.fake.ui.merged_lines();
    assert_eq!(lines.len(), 2, "the two walks");
    assert!(
        lines
            .iter()
            .all(|l| l.follow_terrain && l.color_rgb == 0xF0_B4_3C)
    );
    assert_eq!(
        (lines[0].start.east_west, lines[0].start.north_south),
        (0.0, 0.0)
    );
    assert_eq!(h.sim.fake.ui.merged_shapes().len(), 1, "the ring");
    h.sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    h.tick();
    assert!(h.sim.fake.ui.merged_lines().is_empty());
    assert!(h.sim.fake.ui.merged_shapes().is_empty());
    h.sim.stand(0.0, 0.0, 0.0);
    h.settle();
    assert_eq!(h.sim.fake.ui.merged_lines().len(), 2);
    h.with_host(|app, host| app.set_route_line_visible(host, false));
    h.tick();
    assert!(h.sim.fake.ui.merged_lines().is_empty());
}

#[test]
fn the_dungeon_map_draws_the_portal_tables_portals_dim_until_seen() {
    // In dungeon 0x01AB: "Surface" out of view, "Exit Portal" in view and
    // an admin-only trap no player sees.
    let table = r#"{"format":1,"portals":[
{"g":1879048193,"w":1,"n":"Surface","c":27984133,"p":[10,10,0],"d":4135518226,"dp":[36,84,0]},
{"g":1879048194,"w":2,"n":"Exit Portal","c":27984134,"p":[15,5,0],"d":4135518226,"dp":[36,84,0]},
{"g":1879048195,"w":3,"n":"Portal Floor Walk","c":27984134,"p":[5,5,0],"d":27984134,"dp":[1,1,0],"walk":1,"inv":1}
]}"#;
    let h = Harness::new(|sim| {
        sim.show_map();
        sim.fake
            .set_reply::<calls::StorageRead>(&Ok(Some(table.as_bytes().to_vec())));
        sim.fake
            .set_reply::<calls::WorldObjects>(&vec![ac_plugin_abi::world::WorldObject {
                object_id: 9,
                name: "Exit Portal".to_owned(),
                object_class: ac_plugin_abi::world::ObjectClass::Portal,
                position: Some(ac_plugin_abi::world::Position::from_cell(
                    0x01AB_0106,
                    [15.2, 5.0, 0.0],
                    0.0,
                )),
                ..Default::default()
            }]);
        sim.fake
            .set_reply::<calls::DungeonFloorplan>(&Some(Floorplan {
                landblock_id: 0x01AB,
                layers: vec![DungeonLayer {
                    z: 0.0,
                    floors: vec![vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)]],
                    walls: vec![],
                }],
                cells: vec![],
                bounds_min: (0.0, 0.0, 0.0),
                bounds_max: (20.0, 20.0, 0.0),
            }));
        attach_window(sim);
        sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    });
    assert_eq!(h.plugin.map().unwrap().view(), MapViewKind::Dungeon);
    let paint = h.paint(MAP);
    let text_colour = |name: &str| {
        let colours: Vec<_> = paint
            .iter()
            .filter_map(|c| match c {
                PaintCmd::Text { text, color, .. } if text == name => {
                    Some((color.r, color.g, color.b, color.a))
                }
                _ => None,
            })
            .collect();
        assert_eq!(colours.len(), 1, "{name} drawn once: {paint:?}");
        colours[0]
    };
    // Named, the one not in view dimmer than the one in view.
    assert_ne!(text_colour("Surface"), text_colour("Exit Portal"));
    assert!(
        !h.painted_texts(MAP)
            .iter()
            .any(|t| t == "Portal Floor Walk"),
        "admin-only portals are not drawn"
    );
    let marks = |c: hud::Rgba| {
        paint
            .iter()
            .filter(|p| {
                matches!(p, PaintCmd::FillRect { color, rect }
                    if (color.r, color.g, color.b, color.a) == colour(c) && rect.w == 8.0)
            })
            .count()
    };
    assert_eq!(marks(map::UNSEEN_PORTAL_COLOR), 1, "Surface, dim");
    assert_eq!(marks(map::PORTAL_COLOR), 1, "Exit Portal, in view");
}
