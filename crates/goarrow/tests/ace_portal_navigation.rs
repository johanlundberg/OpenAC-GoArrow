//! The navigator with the portal table: a dungeon's way out is walked to
//! where the table puts it, without exploring; a live portal the table
//! knows needs no identification; without the table's help the search
//! goes on as before.

mod navsim;

use navsim::{Harness, dungeon_position, portal, snapshot};
use openac_plugin::calls;
use openac_plugin::navigation::GoToState;
use openac_plugin::world::Position;

/// Desert March (dungeon 0x02AA) with its Atlas exit to Black Hill,
/// arriving at 0N 95E, and a vendor near it.
const DESERT_MARCH: &str = r#"<locations>
  <loc name="Desert March" type="PortalHub" NS="0" EW="1" dungeonId="02AA" />
  <loc name="Desert March to Black Hill Portal" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
  <loc name="End" type="Vendor" NS="0" EW="98" />
</locations>"#;

/// Only the vendor: the Atlas knows no exit of the dungeon.
const END_ONLY: &str = r#"<locations><loc name="End" type="Vendor" NS="0" EW="98" /></locations>"#;

/// The exit: "Surface" in cell 0x02AA0240, 60 m east, leading to
/// landblock 0xF67F at (36, 84): 0N 95E.
const EXIT: &str = r#"{"g":1879048193,"w":1,"n":"Surface","c":44696128,"p":[60,0,0],"h":0,"d":4135518226,"dp":[36,84,0]}"#;
/// A nearer portal leading elsewhere (0N 0E).
const DECOY: &str = r#"{"g":1879048194,"w":2,"n":"Surface","c":44696113,"p":[-10,0,0],"h":0,"d":2139029505,"dp":[84,84,0]}"#;
/// The Surface Portal 5 m west of the player, leading to 0N 95E.
const SURFACE_PORTAL: &str = r#"{"g":1879048195,"w":3,"n":"Surface Portal","c":44696111,"p":[-5,0,0],"h":0,"d":4135518226,"dp":[36,84,0]}"#;
/// A portal in another landblock (the Town Network) leading to 0N 95E too.
const ELSEWHERE: &str = r#"{"g":1879076904,"w":42820,"n":"Surface","c":459044,"p":[53.91,-39.95,-0.06],"h":0,"d":4135518226,"dp":[36,84,0]}"#;

fn table(portals: &[&str]) -> String {
    format!(
        r#"{{"format":1,"version":{{"base":"v0.8.8","patch":"v0.9.296"}},"extracted":"2026-11-03","portals":[{}]}}"#,
        portals.join(",")
    )
}

/// In Desert March 0 m east, going to End, with the table and floorplan
/// cells an exploration would visit.
fn in_desert_march(xml: &str, portals: &[&str]) -> Harness {
    let mut h = Harness::new(xml);
    h.sim
        .fake
        .set_reply::<calls::StorageRead>(&Ok(Some(table(portals).into_bytes())));
    h.sim.snapshot = snapshot(dungeon_position(0.0));
    h.sim.cells = vec![navsim::cell(0x02AA_0230, (20.0, 0.0, 0.0), 0.0)];
    h.go_to_named("End");
    h.enable();
    h
}

fn table_exit_position() -> Position {
    Position::from_cell(0x02AA_0240, [60.0, 0.0, 0.0], 0.0)
}

fn assert_walks_to_the_table_exit(h: &Harness) {
    assert_eq!(h.sim.go_to_place_calls.len(), 1, "{}", h.failure());
    let (target, arrival) = &h.sim.go_to_place_calls[0];
    assert_eq!(target.cell_id, 0x02AA_0240);
    let local = target.landblock_local();
    assert!(
        (local[0] - 60.0).abs() < 0.01 && local[1].abs() < 0.01,
        "{local:?}"
    );
    assert!(*arrival <= 0.25);
    assert!(h.is_navigating());
    assert!(!h.failure().contains("Exploring"), "{}", h.failure());
    assert!(h.sim.go_to_calls.is_empty());
    assert!(h.sim.identified.is_empty());
}

#[test]
fn a_dungeon_exit_walks_to_the_table_portal_without_exploring() {
    let mut h = in_desert_march(DESERT_MARCH, &[EXIT, DECOY]);
    // Showing the route plans the way out but does not walk.
    assert!(h.plan());
    assert!(h.sim.go_to_place_calls.is_empty());
    h.start();
    assert_walks_to_the_table_exit(&h);
    let waypoints = &h.view().dungeon_waypoints;
    assert_eq!(waypoints.len(), 1);
    assert!(
        waypoints[0].label.contains("'Surface' (portal table)"),
        "{}",
        waypoints[0].label
    );
    assert_eq!(waypoints[0].cell_id, 0x02AA_0240);
    h.tick(0.1);
    assert_eq!(h.sim.go_to_place_calls.len(), 1);

    // In view: the walk goes to the object, which is used on arrival.
    h.sim.objects.push(portal(
        44,
        "Surface",
        Position::from_cell(0x02AA_0240, [60.2, 0.1, 0.0], 0.0),
    ));
    h.tick(0.1);
    assert_eq!(h.go_to_objects(), vec![44]);
    assert!(h.sim.identified.is_empty());
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 44, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![44]);
}

#[test]
fn a_live_portal_the_table_knows_leads_where_the_table_says_without_identifying() {
    let mut h = in_desert_march(DESERT_MARCH, &[SURFACE_PORTAL]);
    h.sim
        .objects
        .push(portal(43, "Surface Portal", dungeon_position(-5.0)));
    h.start();
    // Without the table it is identified first (and the search waits).
    assert!(h.sim.identified.is_empty());
    assert_eq!(h.go_to_objects(), vec![43]);
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 43, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![43]);
    assert!(!h.failure().contains("identification"), "{}", h.failure());
}

#[test]
fn without_a_table_portal_in_this_landblock_the_dungeon_is_explored() {
    let mut h = in_desert_march(DESERT_MARCH, &[ELSEWHERE]);
    assert_eq!(h.nav.ace_portals().len(), 1);
    h.start();
    h.tick(0.6);
    h.tick(0.6);
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert_eq!(h.sim.go_to_place_calls[0].0.cell_id, 0x02AA_0230);
    assert!(h.failure().contains("Exploring dungeon"), "{}", h.failure());
}

#[test]
fn inside_a_dungeon_the_atlas_does_not_know_the_table_plans_the_way_out() {
    let mut h = in_desert_march(END_ONLY, &[EXIT, DECOY]);
    assert!(h.plan(), "{}", h.failure());
    assert_eq!(h.step_count(), 1);
    let route = h.destination.current_route().expect("a route");
    let from = &route.steps()[0].from;
    assert_eq!(from.name, "Surface arrival (portal table 70000001)");
    assert!((from.coords.ew - 95.0).abs() < 1e-6 && from.coords.ns.abs() < 1e-6);
    h.start();
    assert_walks_to_the_table_exit(&h);
}

#[test]
fn a_table_portal_that_is_not_there_falls_back_to_the_search() {
    let mut h = in_desert_march(DESERT_MARCH, &[EXIT]);
    h.start();
    assert_walks_to_the_table_exit(&h);
    h.sim.move_to(table_exit_position());
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 0, 0.0, None, 1);
    assert!(h.sim.activated.is_empty());
    assert!(
        h.failure()
            .contains("was not where the portal table puts it"),
        "{}",
        h.failure()
    );
    h.sim.move_to(dungeon_position(0.0));
    h.tick(0.6);
    h.tick(0.6);
    assert_eq!(h.sim.go_to_place_calls.len(), 2, "{}", h.failure());
    assert_eq!(h.sim.go_to_place_calls[1].0.cell_id, 0x02AA_0230);
}
