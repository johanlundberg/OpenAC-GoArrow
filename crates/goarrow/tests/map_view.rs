//! The Dereth view as the original GoArrow's map (VVS Edition `MapHud`):
//! its zoom steps, grid, icons by zoom, labels, hotspots, hover and
//! tooltip, clicks with Shift and Ctrl, the double click and the
//! right-click menus.

mod common;

use std::rc::Rc;

use ac_plugin_abi::world::{DungeonLayer, DungeonWall, Floorplan};
use ac_plugin_ui::PaintCmd;
use common::Harness;
use goarrow::coords::Coordinates;
use goarrow::location::{Location, LocationType};
use goarrow::map::{BODY, MapViewKind};
use goarrow::map_icons::MapIcon;
use goarrow::map_view::{
    self, Hotspot, IconRules, MapMenu, MenuAction, Rect, TOOLTIP_MS, Zoom, coord_tick_delta,
    factor_from_legacy_zoom, hotspot_at, icon_layout, is_double_click, label_candidates,
    place_labels, tick_label,
};
use openac_plugin::calls;
use openac_plugin::ui::PointerEvent;

const MAP: &str = "goarrow.map";
const ARROW: &str = "goarrow.arrow";
const VIEW: (f32, f32) = (BODY.2, BODY.3);
const SQRT_2: f64 = std::f64::consts::SQRT_2;

// ---- Zoom. ----

#[test]
fn the_zoom_is_root_two_to_the_factor_rounded_down_at_most_ten_and_covers_the_view() {
    let z = Zoom::new(0.0, VIEW);
    assert_eq!((z.factor(), z.multiplier()), (0.0, 1.0));
    let z = Zoom::new(1.7, VIEW);
    assert_eq!(z.factor(), 1.7, "the factor keeps its fraction");
    assert!((z.multiplier() - SQRT_2).abs() < 1e-12, "the zoom does not");
    let z = Zoom::new(25.0, VIEW);
    assert_eq!((z.factor(), z.multiplier()), (10.0, 32.0));
    assert_eq!(z.pixels_per_map_unit(), 320.0);
    // Below the zoom that covers the view (586 / 2041 across), the zoom is
    // that, and the factor its logarithm.
    let cover = f64::from(BODY.2) / 2041.0;
    for factor in [-3.5, -4.0, -100.0, f64::NEG_INFINITY] {
        let z = Zoom::new(factor, VIEW);
        assert!((z.multiplier() - cover).abs() < 1e-12, "{factor}");
        assert!((z.factor() - cover.log(SQRT_2)).abs() < 1e-12, "{factor}");
    }
    // -3 is just over the cover (0.354 > 0.287); a step in from the cover.
    let least = Zoom::new(-100.0, VIEW);
    let next = least.stepped(1.0, VIEW);
    assert!((next.factor() - (least.factor() + 1.0)).abs() < 1e-12);
    assert!((next.multiplier() - SQRT_2.powi(-3)).abs() < 1e-12);
    assert_eq!(next.stepped(-1.0, VIEW).multiplier(), least.multiplier());
}

#[test]
fn an_earlier_saved_zoom_becomes_the_nearest_factor() {
    // The earlier map's zoom 1 fitted the 2041-pixel map in 500 pixels.
    assert_eq!(factor_from_legacy_zoom(1.0), -4.0);
    assert_eq!(factor_from_legacy_zoom(8.0), 2.0);
    assert_eq!(factor_from_legacy_zoom(16.0), 4.0);
    let h = Harness::new(|sim| {
        sim.storage.insert(
            goarrow::settings::SETTINGS_KEY.to_owned(),
            r#"{ "MapVisible": true, "MapZoom": 8, "OverlayPositionVersion": 1, "VisibilitySettingsVersion": 1 }"#
                .to_owned(),
        );
    });
    assert_eq!(h.plugin.map().unwrap().zoom_factor(), 2.0);
    assert_eq!(h.plugin.map().unwrap().zoom(), 2.0);
}

// ---- The grid. ----

#[test]
fn the_grid_spacing_is_the_one_after_the_closest_to_forty_pixels() {
    // 40 px is 4 map units at zoom 1 (10 px a unit): 5 is closest, and the
    // original took the next one.
    assert_eq!(coord_tick_delta(1.0), 10.0);
    assert_eq!(coord_tick_delta(4.0), 2.0);
    assert_eq!(coord_tick_delta(32.0), 0.2);
    assert_eq!(coord_tick_delta(f64::from(BODY.2) / 2041.0), 20.0);
    assert_eq!(coord_tick_delta(0.01), 50.0, "the largest");
}

#[test]
fn grid_labels_drop_the_sign_and_keep_the_decimals_the_spacing_needs() {
    assert_eq!(tick_label(12.0, 10.0, ('N', 'S')), "12N");
    assert_eq!(tick_label(-12.0, 2.0, ('N', 'S')), "12S");
    assert_eq!(tick_label(0.0, 1.0, ('E', 'W')), "0E");
    assert_eq!(tick_label(-0.5, 0.5, ('E', 'W')), "0.5W");
    assert_eq!(tick_label(0.25, 0.05, ('N', 'S')), "0.25N");
    // The original's one loop: north-south from the bottom edge rounded
    // up, east-west from the left edge rounded down, as many of each.
    let (ns, ew) = map_view::grid_ticks(
        Coordinates::new(4.2, -3.1),
        Coordinates::new(-3.1, 4.2),
        2.0,
    );
    assert_eq!(ew, [-4.0, -2.0, 0.0, 2.0, 4.0, 6.0]);
    assert_eq!(ns, [-2.0, 0.0, 2.0, 4.0, 6.0, 8.0]);
}

// ---- Icons. ----

fn place(name: &str, kind: LocationType, ns: f64, ew: f64) -> Rc<Location> {
    let mut l = Location::new(name, ns, ew);
    l.kind = kind;
    Rc::new(l)
}

fn every_kind() -> Vec<Rc<Location>> {
    [
        ("Town", LocationType::TOWN),
        ("Outpost", LocationType::OUTPOST),
        ("Village", LocationType::VILLAGE),
        ("Lifestone", LocationType::LIFESTONE),
        ("Bindstone", LocationType::BINDSTONE),
        ("Portal", LocationType::WILDERNESS_PORTAL),
        ("Hub", LocationType::PORTAL_HUB),
        ("Dungeon", LocationType::DUNGEON),
        ("Hall", LocationType::ALLEGIANCE_HALL),
        ("Landmark", LocationType::LANDMARK),
        ("Person", LocationType::NPC),
        ("Vendor", LocationType::VENDOR),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, kind))| place(name, kind, 0.0, i as f64 * 5.0))
    .collect()
}

fn layout(places: &[Rc<Location>], factor: f64, every: bool) -> Vec<Hotspot> {
    let zoom = Zoom::new(factor, VIEW);
    let per_unit = zoom.pixels_per_map_unit() as f32;
    icon_layout(
        places,
        IconRules {
            zoom,
            every_zoom: every,
        },
        Coordinates::new(50.0, -50.0),
        Coordinates::new(-50.0, 100.0),
        &|c| (c.ew as f32 * per_unit, -(c.ns as f32) * per_unit),
        &|l| l.name == "Favourite",
    )
}

fn names(hotspots: &[Hotspot]) -> Vec<&str> {
    hotspots.iter().map(|h| h.location.name.as_str()).collect()
}

#[test]
fn which_places_show_at_which_zoom() {
    let places = every_kind();
    assert_eq!(names(&layout(&places, -100.0, false)), ["Town"]);
    assert_eq!(
        names(&layout(&places, -1.0, false)),
        ["Lifestone", "Dungeon", "Portal", "Hub", "Outpost", "Town"],
        "in the original's drawing order, towns on top"
    );
    assert_eq!(
        names(&layout(&places, 2.0, false)),
        [
            "Village",
            "Landmark",
            "Hall",
            "Bindstone",
            "Lifestone",
            "Dungeon",
            "Portal",
            "Hub",
            "Outpost",
            "Town"
        ]
    );
    assert_eq!(layout(&places, 5.0, false).len(), 12);
    assert_eq!(
        layout(&places, -100.0, true).len(),
        12,
        "every zoom shows every kind"
    );
    let icons: Vec<MapIcon> = layout(&places, 5.0, false).iter().map(|h| h.icon).collect();
    assert_eq!(
        icons,
        [
            MapIcon::Npc,
            MapIcon::Store,
            MapIcon::Settlement,
            MapIcon::PointOfInterest,
            MapIcon::Dungeon,
            MapIcon::Bindstone,
            MapIcon::Lifestone,
            MapIcon::Dungeon,
            MapIcon::Portal,
            MapIcon::PortalHub,
            MapIcon::TownSmall,
            MapIcon::Town,
        ]
    );
}

#[test]
fn icons_scale_with_the_zoom_and_out_of_view_places_are_left_out() {
    // At a zoom of √2 an icon is its own size: (zoom / √2) ^ ¼ is 1.
    let town = [place("Town", LocationType::TOWN, 0.0, 0.0)];
    let at_one = layout(&town, 1.0, false);
    assert_eq!(at_one[0].rect, Rect::new(-6.5, -6.5, 13.0, 13.0));
    assert_eq!(MapIcon::Town.size(), (13.0, 13.0));
    let close = layout(&town, 9.0, false);
    let scale = 2f32.powf(0.25 * 4.0);
    assert!(
        (close[0].rect.w - 13.0 * scale).abs() < 1e-3,
        "{:?}",
        close[0].rect
    );
    let far = [place("Far", LocationType::TOWN, 80.0, 0.0)];
    assert!(layout(&far, 1.0, false).is_empty());
    let retired = {
        let mut l = Location::new("Old", 0.0, 0.0);
        l.kind = LocationType::TOWN;
        l.is_retired = true;
        [Rc::new(l)]
    };
    assert!(layout(&retired, 1.0, false).is_empty());
}

#[test]
fn a_favourite_shows_the_favourite_icon_and_a_portal_on_a_hub_is_left_out() {
    let places = [
        place("Favourite", LocationType::DUNGEON, 0.0, 0.0),
        place("Hub", LocationType::PORTAL_HUB, 0.0, 10.0),
        place("On the hub", LocationType::TOWN_PORTAL, 0.0, 10.01),
        place("Beside it", LocationType::TOWN_PORTAL, 0.0, 12.0),
    ];
    let hotspots = layout(&places, 4.0, false);
    assert_eq!(names(&hotspots), ["Favourite", "Beside it", "Hub"]);
    assert_eq!(hotspots[0].icon, MapIcon::Favorite);
}

#[test]
fn the_topmost_place_answers_the_pointer_in_whole_pixels() {
    let places = [
        place("Portal", LocationType::WILDERNESS_PORTAL, 0.0, 0.0),
        place("Town", LocationType::TOWN, 0.0, 0.01),
    ];
    let hotspots = layout(&places, 1.0, false);
    assert_eq!(
        hotspot_at(&hotspots, (0.0, 0.0)).unwrap().location.name,
        "Town"
    );
    // The portal's 9-pixel icon at (-4.5, -4.5) answers in whole pixels,
    // its corner truncated toward zero as the original's was.
    let portal = &hotspots[0];
    assert_eq!(portal.hit, Rect::new(-4.0, -4.0, 9.0, 9.0));
    assert!(hotspot_at(&hotspots, (40.0, 40.0)).is_none());
}

// ---- Labels. ----

fn measure(text: &str, bold: bool) -> (f32, f32) {
    (text.len() as f32 * if bold { 8.0 } else { 6.0 }, 12.0)
}

#[test]
fn a_label_tries_below_above_middle_right_left_then_the_corners() {
    let hit = Rect::new(10.0, 20.0, 10.0, 10.0);
    let c = label_candidates(hit, (30.0, 12.0));
    assert_eq!(c[0], Rect::new(0.0, 29.0, 30.0, 12.0), "below");
    assert_eq!(c[1], Rect::new(0.0, 9.0, 30.0, 12.0), "above");
    assert_eq!(
        c[2],
        Rect::new(0.0, 21.5, 30.0, 12.0),
        "over the lower half"
    );
    assert_eq!(c[3], Rect::new(19.0, 19.0, 30.0, 12.0), "right");
    assert_eq!(c[4], Rect::new(-19.0, 19.0, 30.0, 12.0), "left");
    assert_eq!(c[5], Rect::new(19.0, 9.0, 30.0, 12.0));
    assert_eq!(c[6], Rect::new(19.0, 29.0, 30.0, 12.0));
    assert_eq!(c[7], Rect::new(-19.0, 9.0, 30.0, 12.0));
    assert_eq!(c[8], Rect::new(-19.0, 29.0, 30.0, 12.0));
}

#[test]
fn labels_take_the_first_free_place_and_never_overlap() {
    // Five towns a few pixels apart crowd each other's labels.
    let places: Vec<_> = (0..5)
        .map(|i| {
            place(
                &format!("Town {i}"),
                LocationType::TOWN,
                0.0,
                f64::from(i) * 0.05,
            )
        })
        .collect();
    let hotspots = layout(&places, 1.0, false);
    let labels = place_labels(&hotspots, 1.0, &mut measure);
    assert!(labels.len() >= 2, "{labels:?}");
    for (i, a) in labels.iter().enumerate() {
        assert!(a.bold, "towns in the bold font");
        for b in &labels[i + 1..] {
            assert!(!a.rect.intersects(&b.rect), "{a:?} {b:?}");
        }
    }
    // The topmost (last drawn) town is labelled first, below its icon.
    let last = hotspots.last().unwrap();
    assert_eq!(labels[0].text, "Town 4");
    assert_eq!(
        labels[0].rect,
        label_candidates(last.hit, measure("Town 4", true))[0]
    );
}

#[test]
fn far_out_only_towns_are_named_and_people_never() {
    let places = every_kind();
    let named = |factor: f64| {
        let hotspots = layout(&places, factor, true);
        let mut names: Vec<String> = place_labels(&hotspots, factor, &mut measure)
            .into_iter()
            .map(|l| l.text)
            .collect();
        names.sort();
        names
    };
    assert_eq!(named(-4.0), ["Town"]);
    assert_eq!(
        named(1.0),
        ["Dungeon", "Hub", "Lifestone", "Outpost", "Portal", "Town"]
    );
    assert_eq!(named(4.0).len(), 10);
    assert!(!named(9.0).iter().any(|n| n == "Person" || n == "Vendor"));
}

// ---- Menus and the double click. ----

#[test]
fn the_menus_offer_the_originals_items() {
    let mut portal = Location::new("Portal", 1.0, 2.0);
    portal.kind = LocationType::WILDERNESS_PORTAL;
    portal.set_exit_coords(Coordinates::new(10.0, -20.5));
    let menu = MapMenu::for_location(Rc::new(portal), Some(0x01AB));
    assert_eq!(menu.title, "Portal");
    let items: Vec<&str> = menu.items.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        items,
        [
            "Show Details",
            "Point Arrow Here",
            "Start Route Here",
            "End Route Here",
            "Show Dungeon Map",
            "Jump to Exit (10.0N, 20.5W)"
        ]
    );
    assert_eq!(menu.items[4].1, MenuAction::ShowDungeonMap(0x01AB));
    let town = MapMenu::for_location(place("Town", LocationType::TOWN, 0.0, 0.0), None);
    assert_eq!(town.items.len(), 4, "no dungeon map, no exit");
    let point = MapMenu::for_point(Coordinates::new(12.34, -5.0));
    assert_eq!(point.title, "12.3N, 5.0W");
    let items: Vec<&str> = point.items.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        items,
        ["Point Arrow Here", "Start Route Here", "End Route Here"]
    );
}

#[test]
fn a_menu_stays_inside_the_map_and_finds_its_items() {
    let mut menu = MapMenu::for_point(Coordinates::new(0.0, 0.0));
    let bounds = Rect::new(0.0, 36.0, 586.0, 500.0);
    menu.place((580.0, 530.0), 100.0, bounds);
    assert_eq!(menu.rect, Rect::new(470.0, 456.0, 116.0, 80.0));
    assert_eq!(menu.item_at((500.0, 460.0)), None, "the title");
    assert_eq!(menu.item_at((500.0, 480.0)), Some(0));
    assert_eq!(menu.item_at((500.0, 535.0)), Some(2));
    assert_eq!(menu.item_at((400.0, 480.0)), None, "off the menu");
}

#[test]
fn two_presses_close_in_time_and_place_are_a_double_click() {
    let first = Some(((100.0, 100.0), 1000));
    assert!(is_double_click(first, (103.0, 96.0), 1500));
    assert!(!is_double_click(first, (105.0, 100.0), 1100), "too far");
    assert!(!is_double_click(first, (100.0, 100.0), 1501), "too late");
    assert!(!is_double_click(None, (100.0, 100.0), 1000));
}

// ---- On the map. ----

/// A few places round Holtburg, the map zoomed to factor 4 (40 pixels a
/// map unit) on 0, 0, a window attached or not.
fn holtburg(window: bool, floorplan: bool) -> Harness {
    holtburg_at(window, floorplan, (0.0, 0.0))
}

/// [`holtburg`], the map centred elsewhere.
fn holtburg_at(window: bool, floorplan: bool, center: (f64, f64)) -> Harness {
    let mut h = Harness::new(|sim| {
        sim.storage.insert(
            goarrow::settings::SETTINGS_KEY.to_owned(),
            format!(
                r#"{{ "MapVisible": true, "MapZoomFactor": 4, "MapCenterNorthSouth": {}, "MapCenterEastWest": {}, "OverlayPositionVersion": 1, "VisibilitySettingsVersion": 1 }}"#,
                center.0, center.1
            ),
        );
        sim.storage.insert(
            "GoArrow/holtburg.xml".to_owned(),
            r#"<locations>
  <loc name="Holtburg" type="Town" NS="0" EW="0" />
  <loc name="Holtburg Portal" type="WildernessPortal" NS="0" EW="1" exitNS="10" exitEW="10" />
  <loc name="Holtburg Lifestone" type="Lifestone" NS="-1" EW="0" />
  <loc name="Holtburg Healer" type="Vendor" NS="1" EW="0" />
  <loc name="Holtburg Crypt" type="Dungeon" NS="0" EW="-1" dungeonId="1AB" />
</locations>"#
                .to_owned(),
        );
        if floorplan {
            sim.fake
                .set_reply::<calls::DungeonFloorplan>(&Some(Floorplan {
                    landblock_id: 0x01AB,
                    layers: vec![DungeonLayer {
                        z: 0.0,
                        floors: vec![],
                        walls: vec![DungeonWall {
                            start: (0.0, 0.0),
                            end: (20.0, 0.0),
                        }],
                    }],
                    cells: vec![],
                    bounds_min: (0.0, 0.0, 0.0),
                    bounds_max: (20.0, 20.0, 0.0),
                }));
        }
        if window {
            sim.fake.window_attached = true;
            sim.fake.ui.set_window_attached(true);
        }
        sim.stand(5.0, 5.0, 0.0);
    });
    h.go("file holtburg.xml");
    h.settle();
    h
}

fn at(h: &Harness, ns: f64, ew: f64) -> (f32, f32) {
    h.plugin.map().unwrap().canvas_at(ns, ew)
}

fn fills(h: &Harness, rgba: (u8, u8, u8, u8)) -> Vec<(f32, f32, f32, f32)> {
    h.paint(MAP)
        .into_iter()
        .filter_map(|c| match c {
            PaintCmd::FillRect { rect, color } if (color.r, color.g, color.b, color.a) == rgba => {
                Some((rect.x, rect.y, rect.w, rect.h))
            }
            _ => None,
        })
        .collect()
}

const GOLD: (u8, u8, u8, u8) = (255, 215, 0, 255);
const TOOLTIP: (u8, u8, u8, u8) = (0, 0, 0, 160);

#[test]
fn the_map_draws_the_originals_icons_names_grid_and_gutters() {
    let h = holtburg(true, false);
    let town = h.plugin.map_icons().image(MapIcon::Town).handle;
    assert_ne!(town, 0, "the icons were handed to the client");
    let drawn = h
        .paint(MAP)
        .into_iter()
        .filter(|c| matches!(c, PaintCmd::Image { image, .. } if *image == town))
        .count();
    assert_eq!(drawn, 1, "Holtburg's town icon");
    let texts = h.painted_texts(MAP);
    // The town's name: four glow copies and the white one.
    assert_eq!(
        texts.iter().filter(|t| *t == "Holtburg").count(),
        5,
        "{texts:?}"
    );
    // Factor 4 labels lifestones but shows no vendors.
    assert!(texts.iter().any(|t| t == "Holtburg Lifestone"));
    assert!(!texts.iter().any(|t| t == "Holtburg Healer"));
    // Grid labels every 2 map units: east-west whole, north-south stacked.
    assert!(
        texts.iter().any(|t| t == "2E") && texts.iter().any(|t| t == "2W"),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t == "N") && texts.iter().any(|t| t == "S"));
    let gutters = fills(&h, (0, 0, 0, 96));
    assert_eq!(gutters.len(), 4, "{gutters:?}");
    assert!(gutters.contains(&(BODY.0, BODY.1, BODY.2, 15.0)));
}

#[test]
fn hovering_a_place_outlines_it_in_gold_and_names_it_in_a_tooltip_for_four_seconds() {
    let mut h = holtburg(true, false);
    assert!(fills(&h, GOLD).is_empty());
    let town = at(&h, 0.0, 0.0);
    h.sim.fake.clock.monotonic_ms = 10_000;
    h.hover(MAP, town);
    h.tick();
    assert_eq!(h.plugin.map().unwrap().hovered().unwrap().name, "Holtburg");
    assert_eq!(fills(&h, GOLD).len(), 4, "a one-pixel frame");
    let tip = fills(&h, TOOLTIP);
    assert_eq!(tip.len(), 1);
    assert_eq!((tip[0].0, tip[0].1), (town.0 + 16.0, town.1 + 16.0));
    assert!(
        h.painted_texts(MAP)
            .iter()
            .filter(|t| *t == "Holtburg")
            .count()
            >= 6
    );
    // The tooltip goes after four seconds; the outline stays.
    h.sim.fake.clock.monotonic_ms = 10_000 + TOOLTIP_MS;
    for _ in 0..goarrow::map::PAINT_TICKS {
        h.tick();
    }
    assert!(fills(&h, TOOLTIP).is_empty());
    assert_eq!(fills(&h, GOLD).len(), 4);
    // Off any place, and when the pointer leaves, neither.
    h.hover(MAP, at(&h, 3.0, 3.0));
    h.tick();
    assert!(h.plugin.map().unwrap().hovered().is_none());
    assert!(fills(&h, GOLD).is_empty());
    h.hover(MAP, town);
    h.leave(MAP);
    h.tick();
    assert!(h.plugin.map().unwrap().hovered().is_none());
    assert!(fills(&h, GOLD).is_empty() && fills(&h, TOOLTIP).is_empty());
}

#[test]
fn hover_and_leave_click_nothing_and_leave_a_drag_and_the_arrow_alone() {
    let mut h = holtburg(false, false);
    let town = at(&h, 0.0, 0.0);
    let before = h.plugin.map().unwrap().center();
    h.raw_pointer(MAP, PointerEvent::HOVER, town, 0, 0);
    h.raw_pointer(MAP, PointerEvent::LEAVE, town, 0, 0);
    assert!(!h.plugin.destination().has_destination());
    // A drag goes on through a hover.
    h.raw_pointer(MAP, PointerEvent::DOWN, (300.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::MOVE, (340.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::HOVER, (340.0, 300.0), 0, 0);
    h.raw_pointer(MAP, PointerEvent::UP, (340.0, 300.0), 1, 0);
    assert!(h.plugin.map().unwrap().center().1 < before.1, "panned west");
    assert_eq!(
        h.saved().map_center_east_west,
        h.plugin.map().unwrap().center().1,
        "the drag ended on its release, after the hover"
    );
    assert!(
        !h.plugin.destination().has_destination(),
        "a drag, not a click"
    );
    // The arrow hears them and does nothing.
    let offset = h.plugin.hud().unwrap().offset();
    h.raw_pointer(ARROW, PointerEvent::HOVER, (10.0, 10.0), 0, 0);
    h.raw_pointer(ARROW, PointerEvent::LEAVE, (10.0, 10.0), 0, 0);
    assert_eq!(h.plugin.hud().unwrap().offset(), offset);
}

#[test]
fn a_click_points_the_arrow_shift_starts_the_route_and_ctrl_ends_it() {
    let mut h = holtburg(false, false);
    h.with_host(|app, host| app.set_arrow_visible(host, false));
    h.raw_click(MAP, at(&h, 0.0, 0.0), 1, 0);
    assert_eq!(h.plugin.destination().target_name(), "Holtburg");
    assert!(
        h.saved().hud_visible,
        "the arrow shows, as the original's did"
    );
    h.click("ShowOptions");
    h.raw_click(MAP, at(&h, 0.0, 1.0), 1, map_view_shift());
    assert_eq!(h.plugin.route_from().unwrap().name, "Holtburg Portal");
    assert_eq!(h.text("FromText"), "Holtburg Portal");
    assert!(h.flag("RouteTab"), "the route finder shows");
    assert_eq!(h.plugin.destination().target_name(), "Holtburg");
    h.click("ShowOptions");
    h.raw_click(MAP, at(&h, -1.0, 0.0), 1, map_view_ctrl());
    assert_eq!(h.plugin.destination().target_name(), "Holtburg Lifestone");
    assert!(h.flag("RouteTab"));
    // Off the places, a map-point destination as before.
    h.raw_click(MAP, at(&h, 3.0, 3.0), 1, 0);
    let target = h.plugin.destination().target().unwrap().coords;
    assert!(
        target.distance_to(Coordinates::new(3.0, 3.0)) < 0.1,
        "{target}"
    );
}

fn map_view_shift() -> u8 {
    goarrow::map::MODIFIER_SHIFT
}

fn map_view_ctrl() -> u8 {
    goarrow::map::MODIFIER_CONTROL
}

#[test]
fn a_double_click_centres_there_and_a_slow_one_does_not() {
    let mut h = holtburg(false, false);
    let point = at(&h, 2.0, -3.0);
    h.sim.fake.clock.monotonic_ms = 1000;
    h.raw_click(MAP, point, 1, 0);
    h.sim.fake.clock.monotonic_ms = 1300;
    h.raw_click(MAP, (point.0 + 2.0, point.1 - 1.0), 1, 0);
    let (ns, ew) = h.plugin.map().unwrap().center();
    assert!(
        (ns - 2.0).abs() < 0.05 && (ew + 3.0).abs() < 0.05,
        "{ns} {ew}"
    );
    assert_eq!(h.saved().map_center_north_south, ns);
    let point = at(&h, 0.5, 2.5);
    h.sim.fake.clock.monotonic_ms = 5000;
    h.raw_click(MAP, point, 1, 0);
    h.sim.fake.clock.monotonic_ms = 5600;
    h.raw_click(MAP, point, 1, 0);
    assert_eq!(h.plugin.map().unwrap().center(), (ns, ew), "too slow");
}

#[test]
fn the_wheel_zooms_about_the_pointer() {
    let mut h = holtburg(false, false);
    let pointer = (150.0, 120.0);
    let under = h.plugin.map().unwrap().coords_at(pointer);
    h.pointer(
        MAP,
        ac_plugin_ui::PointerKind::Wheel,
        pointer.0,
        pointer.1,
        1,
    );
    assert_eq!(h.plugin.map().unwrap().zoom_factor(), 5.0);
    let after = h.plugin.map().unwrap().coords_at(pointer);
    assert!(
        (after.0 - under.0).abs() < 1e-6 && (after.1 - under.1).abs() < 1e-6,
        "{under:?} {after:?}"
    );
}

/// Right-clicks map coordinates and returns the menu's items.
fn right_click(h: &mut Harness, (ns, ew): (f64, f64)) -> Vec<String> {
    let point = at(h, ns, ew);
    h.raw_click(MAP, point, 2, 0);
    h.plugin
        .map()
        .unwrap()
        .menu()
        .map(|m| m.items.iter().map(|(t, _)| t.clone()).collect())
        .unwrap_or_default()
}

/// Clicks a menu item by its text.
fn choose(h: &mut Harness, item: &str) {
    let menu = h.plugin.map().unwrap().menu().unwrap();
    let i = menu.items.iter().position(|(t, _)| t == item).unwrap();
    let row = menu.row(i);
    h.raw_click(MAP, (row.x + 10.0, row.y + 10.0), 1, 0);
}

#[test]
fn a_right_click_on_a_place_opens_its_menu_and_the_items_act() {
    let mut h = holtburg(false, true);
    let items = right_click(&mut h, (0.0, 1.0));
    assert_eq!(
        items,
        [
            "Show Details",
            "Point Arrow Here",
            "Start Route Here",
            "End Route Here",
            "Jump to Exit (10.0N, 10.0E)"
        ]
    );
    h.tick();
    assert!(
        h.painted_texts(MAP)
            .contains(&"Start Route Here".to_owned())
    );
    choose(&mut h, "Start Route Here");
    assert!(
        h.plugin.map().unwrap().menu().is_none(),
        "an item closes it"
    );
    assert_eq!(h.plugin.route_from().unwrap().name, "Holtburg Portal");
    right_click(&mut h, (0.0, 1.0));
    choose(&mut h, "Jump to Exit (10.0N, 10.0E)");
    let center = h.plugin.map().unwrap().center();
    assert!((center.0 - 10.0).abs() < 1e-6 && (center.1 - 10.0).abs() < 1e-6);
    // The dungeon's menu shows its floorplan.
    let mut h = holtburg(false, true);
    let items = right_click(&mut h, (0.0, -1.0));
    assert!(items.contains(&"Show Dungeon Map".to_owned()), "{items:?}");
    choose(&mut h, "Show Dungeon Map");
    assert_eq!(h.plugin.map().unwrap().view(), MapViewKind::Dungeon);
    assert_eq!(h.plugin.map().unwrap().shown_dungeon(), 0x01AB);
    h.settle();
    assert_eq!(
        h.plugin.map().unwrap().view(),
        MapViewKind::Dungeon,
        "it stays while the player is outdoors"
    );
}

#[test]
fn show_details_opens_the_place_in_the_window_and_end_route_sets_the_destination() {
    let mut h = holtburg(false, false);
    right_click(&mut h, (0.0, 0.0));
    choose(&mut h, "Show Details");
    assert!(h.flag("PlacesTab"));
    assert_eq!(h.text("PlaceQuery"), "Holtburg");
    let selected = usize::try_from(h.int("PlaceSelected")).unwrap();
    assert!(
        h.texts("PlaceNames")[selected].starts_with("Holtburg · "),
        "{:?}",
        h.texts("PlaceNames")
    );
    right_click(&mut h, (-1.0, 0.0));
    choose(&mut h, "End Route Here");
    assert_eq!(h.plugin.destination().target_name(), "Holtburg Lifestone");
    assert!(h.flag("RouteTab"));
    // Without a floorplan the dungeon's menu has no map.
    let items = right_click(&mut h, (0.0, -1.0));
    assert!(!items.contains(&"Show Dungeon Map".to_owned()), "{items:?}");
}

#[test]
fn a_right_click_on_empty_map_offers_the_point_and_a_press_off_the_menu_closes_it() {
    let mut h = holtburg(false, false);
    let items = right_click(&mut h, (3.0, -2.0));
    assert_eq!(
        items,
        ["Point Arrow Here", "Start Route Here", "End Route Here"]
    );
    let title = h.plugin.map().unwrap().menu().unwrap().title.clone();
    assert_eq!(title, "3.0N, 2.0W");
    // A press off the menu closes it and sets no destination.
    h.raw_click(MAP, at(&h, -3.0, 3.0), 1, 0);
    assert!(h.plugin.map().unwrap().menu().is_none());
    assert!(!h.plugin.destination().has_destination());
    right_click(&mut h, (3.0, -2.0));
    choose(&mut h, "Point Arrow Here");
    let target = h.plugin.destination().target().unwrap().coords;
    assert_eq!((target.ns, target.ew), (3.0, -2.0));
    right_click(&mut h, (3.0, -2.0));
    choose(&mut h, "Start Route Here");
    assert_eq!(
        h.plugin.route_from().unwrap().coords,
        Coordinates::new(3.0, -2.0)
    );
    assert_eq!(h.text("FromText"), "3.0N, 2.0W");
}

#[test]
fn the_option_shows_every_place_at_every_zoom() {
    let mut h = holtburg(false, false);
    let healer = at(&h, 1.0, 0.0);
    h.raw_pointer(MAP, PointerEvent::HOVER, healer, 0, 0);
    assert!(
        h.plugin.map().unwrap().hovered().is_none(),
        "no vendors at factor 4"
    );
    h.click("ShowOptions");
    h.check("SetShowAllLocations", true);
    assert!(h.saved().show_locations_all_zooms);
    h.raw_pointer(MAP, PointerEvent::HOVER, (healer.0, healer.1 + 1.0), 0, 0);
    assert_eq!(
        h.plugin.map().unwrap().hovered().unwrap().name,
        "Holtburg Healer"
    );
}

#[test]
fn a_dungeon_chosen_from_the_menu_is_drawn_without_the_player_outdoors() {
    let mut h = holtburg(true, true);
    right_click(&mut h, (0.0, -1.0));
    choose(&mut h, "Show Dungeon Map");
    h.settle();
    assert_eq!(h.plugin.map().unwrap().view(), MapViewKind::Dungeon);
    let marker = h
        .plugin
        .sprites()
        .image(goarrow::sprites::Sprite::Marker)
        .handle;
    assert_ne!(marker, 0);
    let drawn = h
        .paint(MAP)
        .into_iter()
        .filter(|c| matches!(c, PaintCmd::ImageTransformed { image, .. } if *image == marker))
        .count();
    assert_eq!(drawn, 0, "the player is not in that dungeon");
}

// ---- Review fixes. ----

#[test]
fn a_tooltip_near_the_bottom_stays_inside_the_map() {
    // The lifestone 6 map units below the middle: 240 pixels, y 526.
    let mut h = holtburg_at(true, false, (5.0, 0.0));
    let lifestone = at(&h, -1.0, 0.0);
    assert!(lifestone.1 >= 520.0, "{lifestone:?}");
    h.raw_pointer(MAP, PointerEvent::HOVER, lifestone, 0, 0);
    h.tick();
    let tip = fills(&h, TOOLTIP);
    assert_eq!(tip.len(), 1);
    let (x, y, w, hgt) = tip[0];
    assert!(y + hgt <= BODY.1 + BODY.3, "{:?}", tip[0]);
    assert!(
        y >= BODY.1 && x >= BODY.0 && x + w <= BODY.0 + BODY.2,
        "{:?}",
        tip[0]
    );
}

#[test]
fn a_view_change_closes_the_menu_and_its_items_no_longer_act() {
    let mut h = holtburg(false, true);
    right_click(&mut h, (0.0, 0.0));
    let row = h.plugin.map().unwrap().menu().unwrap().row(1);
    h.sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    h.tick();
    assert_eq!(h.plugin.map().unwrap().view(), MapViewKind::Dungeon);
    assert!(h.plugin.map().unwrap().menu().is_none());
    assert!(h.plugin.map().unwrap().hovered().is_none());
    h.raw_click(MAP, (row.x + 10.0, row.y + 10.0), 1, 0);
    assert!(
        !h.plugin.destination().has_destination(),
        "Point Arrow Here did not run"
    );
}

#[test]
fn the_place_under_the_pointer_is_found_again_after_a_zoom() {
    let mut h = holtburg(false, false);
    let portal = at(&h, 0.0, 1.0);
    h.raw_pointer(MAP, PointerEvent::HOVER, portal, 0, 0);
    assert_eq!(
        h.plugin.map().unwrap().hovered().unwrap().name,
        "Holtburg Portal"
    );
    // Ctrl zooms about the middle: the portal moves out from under it.
    h.raw_wheel(MAP, portal, 1, goarrow::map::MODIFIER_CONTROL);
    assert_eq!(h.plugin.map().unwrap().zoom_factor(), 5.0);
    assert!(h.plugin.map().unwrap().hovered().is_none());
}

#[test]
fn the_place_under_the_pointer_is_found_again_after_a_double_click_centres() {
    let mut h = holtburg(false, false);
    let portal = at(&h, 0.0, 1.0);
    h.raw_pointer(MAP, PointerEvent::HOVER, portal, 0, 0);
    h.sim.fake.clock.monotonic_ms = 1000;
    h.raw_click(MAP, portal, 1, 0);
    h.sim.fake.clock.monotonic_ms = 1200;
    // The second press alone: the map centres on the portal, and the
    // pointer is no longer over it.
    h.raw_pointer(MAP, PointerEvent::DOWN, portal, 1, 0);
    assert!(h.plugin.map().unwrap().hovered().is_none());
}

#[test]
fn the_place_under_the_pointer_is_found_again_when_following_moves_the_map() {
    let mut h = holtburg(false, false);
    h.raw_click(MAP, (500.0, 18.0), 1, 0);
    assert!(h.plugin.map().unwrap().follows());
    let town = (BODY.0 + BODY.2 / 2.0, BODY.1 + BODY.3 / 2.0);
    h.raw_pointer(MAP, PointerEvent::HOVER, town, 0, 0);
    assert_eq!(h.plugin.map().unwrap().hovered().unwrap().name, "Holtburg");
    // The player stands at 5N 5E: following, the map moves there.
    h.tick();
    assert_eq!(h.plugin.map().unwrap().center(), (5.0, 5.0));
    assert!(h.plugin.map().unwrap().hovered().is_none());
}

#[test]
fn the_grid_starts_at_the_line_left_of_the_view_so_a_cut_label_shows() {
    // The view's left edge is 7.9W: the 8W line is 4 pixels left of it,
    // and half its label shows in the gutter.
    let h = holtburg_at(true, false, (0.0, -0.57));
    let left = h.plugin.map().unwrap().coords_at((BODY.0, BODY.1)).1;
    assert!(left > -8.0 && left < -7.8, "{left}");
    let texts = h.painted_texts(MAP);
    assert!(texts.iter().any(|t| t == "8W"), "{texts:?}");
}

#[test]
fn the_wheel_zooms_only_over_the_map() {
    let mut h = holtburg(false, false);
    h.raw_wheel(MAP, (300.0, 18.0), 1, 0);
    assert_eq!(
        h.plugin.map().unwrap().zoom_factor(),
        4.0,
        "not over the header"
    );
    h.raw_wheel(MAP, (300.0, 550.0), 1, 0);
    assert_eq!(
        h.plugin.map().unwrap().zoom_factor(),
        4.0,
        "not over the footer"
    );
    h.raw_wheel(MAP, (300.0, 300.0), 1, 0);
    assert_eq!(h.plugin.map().unwrap().zoom_factor(), 5.0);
}

#[test]
fn following_the_wheel_keeps_the_player_in_the_middle_only_while_centred_on_the_player() {
    let mut h = holtburg(false, false);
    h.raw_click(MAP, (500.0, 18.0), 1, 0);
    h.tick();
    assert_eq!(h.plugin.map().unwrap().center(), (5.0, 5.0));
    h.raw_wheel(MAP, (150.0, 120.0), 1, 0);
    assert_eq!(
        h.plugin.map().unwrap().center(),
        (5.0, 5.0),
        "centred: about the middle"
    );
    // Indoors the map is not centred on the player: it zooms about the
    // pointer.
    h.sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    h.tick();
    let pointer = (150.0, 120.0);
    let under = h.plugin.map().unwrap().coords_at(pointer);
    h.raw_wheel(MAP, pointer, 1, 0);
    let after = h.plugin.map().unwrap().coords_at(pointer);
    assert!(
        (after.0 - under.0).abs() < 1e-6 && (after.1 - under.1).abs() < 1e-6,
        "{under:?} {after:?}"
    );
}

#[test]
fn a_right_tap_while_the_left_is_held_opens_no_menu() {
    let mut h = holtburg(false, false);
    let empty = at(&h, 3.0, -3.0);
    h.raw_pointer(MAP, PointerEvent::DOWN, empty, 1, 0);
    h.raw_pointer(MAP, PointerEvent::DOWN, empty, 2, 0);
    h.raw_pointer(MAP, PointerEvent::UP, empty, 2, 0);
    assert!(h.plugin.map().unwrap().menu().is_none());
    h.raw_pointer(MAP, PointerEvent::UP, empty, 1, 0);
    assert!(h.plugin.map().unwrap().menu().is_none());
    assert!(
        h.plugin.destination().has_destination(),
        "the left click went on"
    );
}

#[test]
fn a_right_release_during_a_left_drag_leaves_the_drag_going() {
    let mut h = holtburg(false, false);
    let before = h.plugin.map().unwrap().center();
    h.raw_pointer(MAP, PointerEvent::DOWN, (300.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::MOVE, (340.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::DOWN, (340.0, 300.0), 2, 0);
    h.raw_pointer(MAP, PointerEvent::UP, (340.0, 300.0), 2, 0);
    h.raw_pointer(MAP, PointerEvent::MOVE, (380.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::UP, (380.0, 300.0), 1, 0);
    let after = h.plugin.map().unwrap().center();
    // 80 pixels at 40 a map unit: 2 map units west.
    assert!(
        (before.1 - after.1 - 2.0).abs() < 0.01,
        "{before:?} {after:?}"
    );
    assert_eq!(h.saved().map_center_east_west, after.1);
    assert!(h.plugin.map().unwrap().menu().is_none());
}

#[test]
fn a_pan_cut_short_by_a_cancel_is_saved() {
    let mut h = holtburg(false, false);
    h.raw_pointer(MAP, PointerEvent::DOWN, (300.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::MOVE, (340.0, 300.0), 1, 0);
    h.raw_pointer(MAP, PointerEvent::CANCELLED, (340.0, 300.0), 1, 0);
    let center = h.plugin.map().unwrap().center();
    assert!(center.1 < 0.0);
    assert_eq!(h.saved().map_center_east_west, center.1);
}
