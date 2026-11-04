//! The plugin's lifecycle and its route work (`GoArrowPlugin.OnTick`):
//! re-planning while idle (every second at most, after 20 m), Go waiting a
//! frame while a window shows, and the dates the cache keeps.

mod common;

use common::{ATLAS_XML, Harness};
use goarrow::plugin::{REPLAN_TICKS, WORK_DELAY_TICKS};
use goarrow::settings::SETTINGS_KEY;
use goarrow::timestamp::{iso8601_utc, parse_iso8601, short_date};

fn atlas_to_rithwic() -> Harness {
    let mut h = Harness::new(|sim| {
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 0.0);
    });
    h.go("Rithwic");
    h.settle();
    h
}

fn origin(h: &Harness) -> (f64, f64) {
    let route = h.plugin.destination().current_route().expect("a route");
    let from = route.steps()[0].from.coords;
    (from.ns, from.ew)
}

#[test]
fn idle_the_route_is_planned_again_after_twenty_metres() {
    let mut h = atlas_to_rithwic();
    assert_eq!(origin(&h), (0.0, 0.0));
    // 10 m: not yet.
    h.sim.stand(10.0 / 240.0, 0.0, 0.0);
    for _ in 0..=REPLAN_TICKS * 2 {
        h.tick();
    }
    assert_eq!(origin(&h), (0.0, 0.0));
    // 25 m: once a second has passed.
    h.sim.stand(25.0 / 240.0, 0.0, 0.0);
    h.tick();
    h.tick();
    assert_eq!(origin(&h), (25.0 / 240.0, 0.0));
    // Re-plan off: the route stays where it was planned.
    h.check("SetRecalculate", false);
    h.sim.stand(60.0 / 240.0, 0.0, 0.0);
    h.settle();
    assert_eq!(origin(&h), (25.0 / 240.0, 0.0));
}

#[test]
fn the_route_waits_indoors_and_the_guidance_follows_outdoors() {
    let mut h = atlas_to_rithwic();
    let distance = h.plugin.destination().guidance_distance();
    h.sim.stand(0.0, 0.5, 0.0);
    h.settle();
    assert!(h.plugin.destination().guidance_distance() < distance);
    h.sim.stand_indoors(0x01AB_0105, 0.0, 0.0);
    h.settle();
    assert_eq!(h.text("StatusText"), "Outdoor route paused indoors");
}

#[test]
fn go_waits_a_frame_while_a_window_shows() {
    let mut h = atlas_to_rithwic();
    h.sim.fake.window_attached = true;
    h.click("GoStop");
    assert!(h.plugin.is_computing_route());
    assert_eq!(h.text("StatusText"), "Computing route...");
    for _ in 0..WORK_DELAY_TICKS + 2 {
        h.tick();
    }
    h.settle();
    assert!(!h.plugin.is_computing_route());
    assert_eq!(h.text("StatusText"), "Route ready");
}

/// 3,000 places in a row 0.5 map units apart: a graph a tick cannot build.
fn many_places() -> Harness {
    places_in_a_row(3000)
}

/// Places in a row 0.5 map units apart, the player at the first.
fn places_in_a_row(count: u32) -> Harness {
    let mut xml = String::from("<locations>");
    for i in 0..count {
        xml.push_str(&format!(
            "<loc name='Place {i}' type='Town' NS='0' EW='{}'/>",
            f64::from(i) * 0.5
        ));
    }
    xml.push_str("</locations>");
    let mut h = Harness::new(|sim| {
        sim.storage.insert("GoArrow/many.xml".to_owned(), xml);
        sim.stand(0.0, 0.0, 0.0);
    });
    h.go("file many.xml");
    h.settle();
    assert_eq!(h.plugin.database().location_count(), count as usize);
    h
}

/// 9,000 places, the destination at the far end, ticked until the idle
/// search is under way: it takes more than one tick.
fn search_under_way() -> Harness {
    let mut h = places_in_a_row(9000);
    h.go("Place 8999");
    for _ in 0..200 {
        if h.plugin.finder().is_graph_built() {
            break;
        }
        h.tick();
    }
    assert!(h.plugin.finder().is_graph_built());
    assert!(h.plugin.is_computing_route(), "the search spans ticks");
    h
}

// GoArrow's searches ran in one call, so a corrections reload could not
// meet one under way; here the reload drops it (it searches the old graph)
// and the route is planned again with the new data.
#[test]
fn reloading_the_corrections_drops_a_search_under_way() {
    let mut h = search_under_way();
    h.go("corrections reload");
    assert!(!h.plugin.is_computing_route());
    h.settle();
    let route = h.plugin.destination().current_route().expect("a route");
    assert_eq!(route.steps().last().unwrap().to.name, "Place 8999");
}

#[test]
fn reloading_the_corrections_plans_the_shown_route_again() {
    let mut h = atlas_to_rithwic();
    assert!(h.plugin.destination().current_route().is_some());
    h.go("corrections reload");
    assert!(h.plugin.destination().current_route().is_none());
    h.settle();
    assert!(h.plugin.destination().current_route().is_some());
}

// Go with "Walk it" on walks the navigator's route; an idle search under
// way must not replace it when it finishes.
#[test]
fn go_drops_an_idle_search_under_way() {
    let mut h = search_under_way();
    h.check("SetAutoNavigate", true);
    h.with_host(|app, host| app.go_pressed(host));
    assert!(!h.plugin.is_computing_route());
}

// GoArrow built the graph inside the first search; here it is built over
// several ticks, and the search waits for it.
#[test]
fn the_graph_is_built_over_ticks_and_the_search_waits_for_it() {
    let mut h = many_places();
    assert!(!h.plugin.finder().is_graph_built(), "no destination yet");
    h.go("Place 40");
    h.tick();
    assert!(!h.plugin.finder().is_graph_built());
    assert!(h.plugin.is_computing_route());
    h.with_host(|app, host| app.refresh_window(host));
    assert_eq!(h.text("StatusText"), "Computing route...");
    h.settle();
    assert!(h.plugin.finder().is_graph_built());
    assert!(!h.plugin.is_computing_route());
    let route = h.plugin.destination().current_route().expect("a route");
    assert_eq!(route.steps().last().unwrap().to.name, "Place 40");
}

// Go without a window waits for the graph too, instead of building it in
// the call.
#[test]
fn go_waits_for_the_graph_without_a_window() {
    let mut h = many_places();
    h.check("SetAutoNavigate", true);
    h.go("Place 40");
    h.click("GoStop");
    assert!(!h.plugin.finder().is_graph_built());
    assert!(h.plugin.is_computing_route());
    h.settle();
    assert!(h.plugin.finder().is_graph_built());
}

#[test]
fn disable_saves_the_settings() {
    let mut h = atlas_to_rithwic();
    h.sim.storage.remove(SETTINGS_KEY);
    h.disable();
    assert_eq!(h.saved().destination_name, "Rithwic");
}

#[test]
fn the_caches_dates_read_and_write_as_dotnet_writes_them() {
    assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00+00:00");
    assert_eq!(
        iso8601_utc(1_793_000_000_120),
        "2026-10-26T07:33:20.12+00:00"
    );
    for text in [
        "2026-10-26T07:33:20.12+00:00",
        "2026-10-26T09:33:20.1200000+02:00",
        "2026-10-26T07:33:20.12Z",
    ] {
        assert_eq!(parse_iso8601(text), Some(1_793_000_000_120), "{text}");
    }
    assert_eq!(parse_iso8601("yesterday"), None);
    assert_eq!(parse_iso8601("2026-13-01T00:00:00Z"), None);
    assert_eq!(short_date(1_793_000_000_000, 0), "26 Oct 2026");
    assert_eq!(short_date(1_793_000_000_000, -8 * 60), "25 Oct 2026");
}

#[test]
fn atlas_portal_arrivals_are_taken_from_the_portal_table_with_one_log_line() {
    // "Holtburg to Rithwic" (0N 1E, listed arrival 50N 50E): the table's
    // portal 12 m east of it leads to 50.3N 50.2E.
    const TABLE: &str = r#"{"format":1,"portals":[
        {"g":1,"w":1,"n":"Portal to Rithwic","c":2155806721,"p":[144,84,0],"h":0,"d":3200122881,"dp":[36,60,0]}]}"#;
    let h = Harness::new(|sim| {
        sim.fake
            .set_reply::<openac_plugin::calls::StorageRead>(&Ok(Some(TABLE.as_bytes().to_vec())));
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 0.0);
    });
    let portal = h
        .plugin
        .database()
        .find_location("Holtburg to Rithwic")
        .unwrap();
    let arrival = portal.exit_coords();
    assert!(
        (arrival.ns - 50.3).abs() < 1e-6 && (arrival.ew - 50.2).abs() < 1e-6,
        "{arrival}"
    );
    let lines: Vec<&String> = h
        .sim
        .fake
        .logs
        .iter()
        .map(|(_, line)| line)
        .filter(|line| line.contains("taken from the portal table"))
        .collect();
    assert_eq!(
        lines,
        ["GoArrow: 1 Atlas portal arrivals taken from the portal table."]
    );
}
