//! A route through the Town Network: the Atlas lists every portal inside
//! it at one made-up point in the sea south of Dereth (99.1S 8.7E). The
//! window says such a step arrives in the Town Network, the map draws one
//! portal hop from the entrance to where the hub's exit lets out, and no
//! line goes to the made-up point.

mod common;

use ac_plugin_ui::PaintCmd;
use common::Harness;
use goarrow::coords::{Coordinates, NavPosition};
use goarrow::map_icons::MapIcon;
use goarrow::{ground, map};

const MAP: &str = "goarrow.map";

/// Sawato's Town Network portal, the hub's Eastham exit, and the Seaside
/// Lair 2.4 map units east of where it lets out. Atlas latitudes are
/// positive south.
const ATLAS: &str = r#"<atlas>
  <location><id>1</id><latitude>28.0</latitude><longitude>59.0</longitude><name>Sawato</name><type>Town</type></location>
  <location><id>2</id><latitude>28.7</latitude><longitude>59.4</longitude><name>Town Network Portal(Sawato)</name>
    <type>Wilderness Portal</type><arrival_latitude>99.1</arrival_latitude><arrival_longitude>8.7</arrival_longitude></location>
  <location><id>3</id><latitude>99.1</latitude><longitude>8.7</longitude><name>Town Network (N R 3) to Eastham</name>
    <type>Underground Portal</type><arrival_latitude>-16.9</arrival_latitude><arrival_longitude>63.5</arrival_longitude></location>
  <location><id>4</id><latitude>-17.2</latitude><longitude>65.9</longitude><name>Seaside Lair</name><type>Dungeon</type></location>
</atlas>"#;

const HUB: Coordinates = Coordinates { ns: -99.1, ew: 8.7 };

fn to_the_lair(map: bool) -> Harness {
    let mut h = Harness::new(|sim| {
        if map {
            sim.show_map();
            // A window, so the map hands the client its icons.
            sim.fake.window_attached = true;
            sim.fake.ui.set_window_attached(true);
        }
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS.to_owned());
        sim.stand(-28.0, 59.0, 0.0);
    });
    h.go("Seaside Lair");
    h.settle();
    let texts = h.texts("StepTexts");
    let wanted = [
        "Walk: Town Network Portal(Sawato)",
        "Portal: Town Network Portal(Sawato)",
        "Walk: Town Network (N R 3) to Eastham",
        "Portal: Town Network (N R 3) to Eastham",
        "Walk: Seaside Lair",
    ];
    assert_eq!(texts.len(), wanted.len(), "{texts:?}");
    for (text, want) in texts.iter().zip(wanted) {
        assert!(text.starts_with(want), "{texts:?}");
    }
    h
}

#[test]
fn a_step_into_the_town_network_arrives_in_it_not_at_the_made_up_point() {
    let mut h = to_the_lair(false);
    assert_eq!(h.text("DetailArrival"), "Arrives in the Town Network");
    h.pick("SelectStep", 1);
    assert_eq!(h.text("DetailArrival"), "Arrives in the Town Network");
    h.pick("SelectStep", 2);
    assert_eq!(h.text("DetailWhere"), "Walk inside the Town Network");
    assert_eq!(h.text("DetailArrival"), "Arrives 16.9N, 63.5E");
    assert_eq!(h.texts("StepWalks")[2], "");
    h.pick("SelectStep", 3);
    assert_eq!(h.text("DetailWhere"), "Use in the Town Network");
}

#[test]
fn the_map_draws_one_portal_hop_through_the_town_network() {
    let h = to_the_lair(true);
    let route = h.plugin.destination().current_route().unwrap();
    let lines = map::route_lines(route, Some(Coordinates::new(-28.0, 59.0)));
    let hops: Vec<_> = lines.iter().filter(|l| l.hop).collect();
    assert_eq!(hops.len(), 1, "one portal hop: {lines:?}");
    assert!(
        hops[0].from.distance_to(Coordinates::new(-28.7, 59.4)) < 0.01
            && hops[0].to.distance_to(Coordinates::new(16.9, 63.5)) < 0.01,
        "from the entrance to where the exit lets out: {hops:?}"
    );
    for line in &lines {
        for end in [line.from, line.to] {
            assert!(end.distance_to(HUB) > 1.0, "a line ends at {end}");
        }
    }
    // And the map draws it as the original GoArrow's map does: one dashed
    // line with no arrowhead, besides the legend's four dashes.
    let paint = h.paint(MAP);
    let pink: Vec<f32> = paint
        .iter()
        .filter_map(|c| match c {
            PaintCmd::Line {
                color, thickness, ..
            } if (color.r, color.g, color.b) == (255, 102, 255) => Some(*thickness),
            _ => None,
        })
        .collect();
    let m = h.plugin.map().unwrap();
    let (from, to) = (
        m.canvas_at(hops[0].from.ns, hops[0].from.ew),
        m.canvas_at(hops[0].to.ns, hops[0].to.ew),
    );
    let width = map::ROUTE_PORTAL_WIDTH;
    let length = (to.0 - from.0).hypot(to.1 - from.1);
    let period = (map::ROUTE_DASH.0 + map::ROUTE_DASH.1) * width;
    let dashes = (length / period).ceil() as usize;
    let dashed = pink.iter().filter(|t| **t == width).count();
    assert_eq!(dashed - 4, dashes, "one hop's dashes: {pink:?}");
    assert_eq!(pink.len(), dashed, "no arrowhead on a portal hop: {pink:?}");
    // Nor does it mark where the route starts and ends.
    let icons = h.plugin.map_icons();
    for icon in [MapIcon::StartPoint, MapIcon::EndPoint] {
        let handle = icons.image(icon).handle;
        assert_ne!(handle, 0, "{icon:?} was handed to the client");
        assert!(
            !paint
                .iter()
                .any(|c| matches!(c, PaintCmd::Image { image, .. } if *image == handle)),
            "{icon:?} drawn"
        );
    }
}

#[test]
fn no_ground_line_goes_to_the_made_up_point() {
    let h = to_the_lair(false);
    let here = NavPosition {
        cell_id: 0xD07D_0001,
        east_west: 59.0,
        north_south: -28.0,
        elevation: 0.0,
        heading_degrees: 0.0,
        is_outdoor: true,
    };
    let lines = ground::route_lines(&h.plugin, &here);
    assert!(!lines.is_empty());
    for line in lines {
        for end in [line.start, line.end] {
            let at = Coordinates::new(end.north_south, end.east_west);
            assert!(at.distance_to(HUB) > 1.0, "a line ends at {at}");
        }
    }
}
