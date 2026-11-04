//! Atlas portal arrivals taken from the portal table: an Atlas portal the
//! table has (by its entrance outdoors, by the place it leads to in an
//! indoor hub) arrives where the table says, unless a correction says
//! otherwise.

mod navsim;

use std::collections::HashMap;

use goarrow::ace_portals::{AcePortals, AtlasMatch};
use goarrow::coords::Coordinates;
use goarrow::database::LocationDatabase;
use goarrow::finder::{RouteCostProfile, RouteFinder};
use goarrow::location::Location;
use goarrow::route::RouteStepKind;
use navsim::{Harness, pos, snapshot};
use openac_plugin::calls;
use openac_plugin::world::Position;

/// The way from Sawato to the Seaside Lair, as the Atlas lists it: into
/// the Town Network at Sawato, out by its Eastham portal, whose arrival
/// it puts at 16.9N 65.3E (161 m from the lair).
const SAWATO_TO_SEASIDE: &str = "<atlas>
  <location><id>50</id><latitude>28.9</latitude><longitude>59.3</longitude>
    <name>Sawato</name><type>Town</type></location>
  <location><id>9298</id><latitude>28.7</latitude><longitude>59.4</longitude>
    <name>Town Network Portal(Sawato)</name><type>Wilderness Portal</type>
    <arrival_latitude>99.1</arrival_latitude><arrival_longitude>8.7</arrival_longitude></location>
  <location><id>9323</id><latitude>99.1</latitude><longitude>8.7</longitude>
    <name>Town Network (N R 3) to Eastham</name><type>Wilderness Portal</type>
    <arrival_latitude>-16.9</arrival_latitude><arrival_longitude>65.3</arrival_longitude></location>
  <location><id>9273</id><latitude>-17.7</latitude><longitude>63.4</longitude>
    <name>Town Network Portal(Eastham)</name><type>Wilderness Portal</type>
    <arrival_latitude>99.1</arrival_latitude><arrival_longitude>8.7</arrival_longitude></location>
  <location><id>74</id><latitude>-17.5</latitude><longitude>63.4</longitude>
    <name>Eastham</name><type>Town</type></location>
  <location><id>276</id><latitude>-17.2</latitude><longitude>65.9</longitude>
    <name>Seaside Lair</name><type>Dungeon</type></location>
</atlas>";

/// The Town Network's Eastham portal (landblock 0x0007) as the ACE world
/// database places it: it leads to cell 0xCE940035 (151.05, 112.61).
const EASTHAM_PORTAL: &str = r#"{"g":1879076987,"w":42815,"n":"Eastham Portal","c":459105,"p":[86.49,-20.03,-0.06],"h":180,"d":3465805877,"dp":[151.05,112.61,17.42],"pb":1}"#;

fn table(portals: &[&str]) -> String {
    format!(
        r#"{{"format":1,"version":{{"base":"v0.8.8","patch":"v0.9.296"}},"extracted":"2026-11-03","portals":[{}]}}"#,
        portals.join(",")
    )
}

fn parsed_table(portals: &[&str]) -> AcePortals {
    AcePortals::parse(&table(portals)).expect("a table")
}

/// The map coordinates of a cell and landblock-local metres.
fn map_of(cell_id: u32, local: [f32; 3]) -> Coordinates {
    let p = Position::from_cell(cell_id, local, 0.0);
    Coordinates::new(p.north_south, p.east_west)
}

/// Where the Eastham portal really lets out: 16.92N 63.48E.
fn true_eastham_arrival() -> Coordinates {
    map_of(0xCE94_0035, [151.05, 112.61, 17.42])
}

/// A cell and landblock-local metres for map coordinates outdoors.
fn cell_at(ns: f64, ew: f64) -> (u32, [f32; 2]) {
    let x = ew * 240.0 + 84.0;
    let y = ns * 240.0 + 84.0;
    let (bx, by) = ((x / 192.0).floor(), (y / 192.0).floor());
    let cell = ((((bx + 127.0) as u32) << 24) | (((by + 127.0) as u32) << 16)) | 1;
    (cell, [(x - bx * 192.0) as f32, (y - by * 192.0) as f32])
}

/// A table portal standing at map coordinates outdoors and leading to
/// others; `extra` adds keys (`,"walk":1`).
fn ace_at(name: &str, at: (f64, f64), to: (f64, f64), extra: &str) -> String {
    let (cell, local) = cell_at(at.0, at.1);
    let (to_cell, to_local) = cell_at(to.0, to.1);
    format!(
        r#"{{"g":1,"w":1,"n":"{name}","c":{cell},"p":[{},{},0],"h":0,"d":{to_cell},"dp":[{},{},0]{extra}}}"#,
        local[0], local[1], to_local[0], to_local[1]
    )
}

fn db_with(xml: &str) -> LocationDatabase {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(xml).expect("Atlas XML");
    db
}

fn exit_of(db: &LocationDatabase, name: &str) -> Coordinates {
    db.find_location(name).expect(name).exit_coords()
}

fn close(a: Coordinates, b: Coordinates) -> bool {
    a.distance_to(b) < 0.001
}

#[test]
fn the_eastham_portal_arrives_where_the_table_says() {
    let mut db = db_with(SAWATO_TO_SEASIDE);
    let listed = exit_of(&db, "Town Network (N R 3) to Eastham");
    assert_eq!(listed, Coordinates::new(16.9, 65.3));

    assert_eq!(db.use_portal_table(&parsed_table(&[EASTHAM_PORTAL])), 1);

    let arrival = exit_of(&db, "Town Network (N R 3) to Eastham");
    assert!(close(arrival, true_eastham_arrival()), "{arrival}");
    // 16.92N 63.48E: 1.8 map units (440 m) west of the Atlas listing.
    assert!((arrival.ns - 16.919).abs() < 0.001 && (arrival.ew - 63.479).abs() < 0.001);
    assert_eq!(db.listed_arrival(9323), Some(listed));
    assert_eq!(db.table_arrival_count(), 1);
    // The ways into the hub keep the hub's own point.
    assert_eq!(
        exit_of(&db, "Town Network Portal(Sawato)"),
        Coordinates::new(-99.1, 8.7)
    );
}

#[test]
fn the_route_to_the_seaside_lair_walks_from_the_true_arrival() {
    let mut db = db_with(SAWATO_TO_SEASIDE);
    db.use_portal_table(&parsed_table(&[EASTHAM_PORTAL]));
    let sawato = db.find_location("Sawato").unwrap();
    let lair = db.find_location("Seaside Lair").unwrap();
    let route = RouteFinder::new(db).find_route(&sawato, &lair, RouteCostProfile::ShortestWalk);
    let steps = route.steps();
    let portals: Vec<&str> = steps
        .iter()
        .filter(|s| s.kind == RouteStepKind::Portal)
        .map(|s| s.via.as_str())
        .collect();
    assert_eq!(
        portals,
        [
            "Town Network Portal(Sawato)",
            "Town Network (N R 3) to Eastham"
        ],
        "{steps:?}"
    );
    let last = steps.last().unwrap();
    assert_eq!(last.kind, RouteStepKind::Travel);
    assert!(close(last.from.coords, true_eastham_arrival()), "{last:?}");
}

/// The character standing at the true Eastham arrival, going to the
/// Seaside Lair, with the table read and used.
fn at_the_true_eastham_arrival(portals: &[&str]) -> Harness {
    let mut h = Harness::new(SAWATO_TO_SEASIDE);
    h.sim
        .fake
        .set_reply::<calls::StorageRead>(&Ok(Some(table(portals).into_bytes())));
    let here = true_eastham_arrival();
    let (cell, _) = cell_at(here.ns, here.ew);
    h.sim.snapshot = snapshot(pos(cell, here.ew, here.ns, 0.0, true));
    h.go_to_named("Seaside Lair");
    h.enable();
    let table = h.nav.ace_portals().clone();
    h.finder.database_mut().use_portal_table(&table);
    h
}

#[test]
fn from_the_true_arrival_the_route_does_not_go_back_into_the_town_network() {
    let mut h = at_the_true_eastham_arrival(&[EASTHAM_PORTAL]);
    assert!(h.plan(), "{}", h.failure());
    assert!(
        h.destination
            .current_route()
            .unwrap()
            .steps()
            .iter()
            .all(|s| s.kind == RouteStepKind::Travel),
        "{:?}",
        h.vias()
    );
}

#[test]
fn an_outdoor_portal_is_the_table_portal_within_48_metres_of_its_entrance() {
    let xml = "<atlas>
      <location><id>1</id><latitude>0</latitude><longitude>1</longitude>
        <name>Portal to Rithwic</name><type>Town Portal</type>
        <arrival_latitude>-50</arrival_latitude><arrival_longitude>50</arrival_longitude></location>
    </atlas>";
    // 30 m east of the Atlas entrance, leading to 50.2N 50.1E.
    let near = ace_at("Portal to Rithwic", (0.0, 1.125), (50.2, 50.1), "");
    let mut db = db_with(xml);
    assert_eq!(db.use_portal_table(&parsed_table(&[&near])), 1);
    assert!(close(
        exit_of(&db, "Portal to Rithwic"),
        Coordinates::new(50.2, 50.1)
    ));

    // 60 m away it is another portal.
    let far = ace_at("Portal to Rithwic", (0.0, 1.25), (50.2, 50.1), "");
    let mut db = db_with(xml);
    assert_eq!(db.use_portal_table(&parsed_table(&[&far])), 0);
    assert_eq!(
        exit_of(&db, "Portal to Rithwic"),
        Coordinates::new(50.0, 50.0)
    );
}

#[test]
fn near_an_entrance_the_table_portal_named_for_the_place_is_it() {
    // A settlement hub: one Atlas point for every portal of the ring.
    let xml = "<atlas>
      <location><id>1</id><latitude>66.3</latitude><longitude>14.3</longitude>
        <name>Crater to East Esper Valley Portal</name><type>Settlement Portal</type>
        <arrival_latitude>72.5</arrival_latitude><arrival_longitude>28</arrival_longitude></location>
      <location><id>2</id><latitude>66.3</latitude><longitude>14.3</longitude>
        <name>Crater to Neydisa Village Portal</name><type>Settlement Portal</type>
        <arrival_latitude>70.9</arrival_latitude><arrival_longitude>23</arrival_longitude></location>
    </atlas>";
    let esper = ace_at(
        "East Esper Valley  Portal",
        (-66.3, 14.33),
        (-72.6, 28.1),
        "",
    );
    let neydisa = ace_at("Neydisa Village Portal", (-66.27, 14.3), (-70.8, 23.1), "");
    let mut db = db_with(xml);
    assert_eq!(db.use_portal_table(&parsed_table(&[&esper, &neydisa])), 2);
    assert!(close(
        exit_of(&db, "Crater to East Esper Valley Portal"),
        Coordinates::new(-72.6, 28.1)
    ));
    assert!(close(
        exit_of(&db, "Crater to Neydisa Village Portal"),
        Coordinates::new(-70.8, 23.1)
    ));

    // None near it named for its place, even one alone: another portal
    // (where the Atlas's is gone, the table has only its walk-in remains).
    let other = ace_at(
        "Bandit Road Villas Portal",
        (-66.3, 14.33),
        (-60.0, 20.0),
        "",
    );
    let remains = ace_at(
        "Destroyed East Esper Valley Portal",
        (-66.3, 14.3),
        (-72.6, 28.1),
        r#","walk":1"#,
    );
    let esper_atlas = Location::with_details(
        1,
        "Crater to East Esper Valley Portal",
        goarrow::location::LocationType::SETTLEMENT_PORTAL,
        Coordinates::new(-66.3, 14.3),
        "",
        0,
        Coordinates::new(-72.5, 28.0),
    );
    for portals in [vec![&other, &neydisa], vec![&other, &remains]] {
        let table = parsed_table(&portals.iter().map(|p| p.as_str()).collect::<Vec<_>>());
        assert_eq!(table.match_atlas_portal(&esper_atlas), AtlasMatch::None);
    }
    let mut db = db_with(xml);
    assert_eq!(db.use_portal_table(&parsed_table(&[&other])), 0);
}

#[test]
fn in_the_hub_same_named_portals_leading_apart_are_no_match() {
    // A second "Eastham Portal" leading elsewhere: which one the Atlas
    // means is not known.
    let elsewhere = EASTHAM_PORTAL
        .replace("1879076987", "1879076988")
        .replace("3465805877", "3465805878");
    let mut db = db_with(SAWATO_TO_SEASIDE);
    assert_eq!(
        db.use_portal_table(&parsed_table(&[EASTHAM_PORTAL, &elsewhere])),
        0
    );
    assert_eq!(
        exit_of(&db, "Town Network (N R 3) to Eastham"),
        Coordinates::new(16.9, 65.3)
    );

    // Leading to the same cell they agree, whatever their names.
    let same = EASTHAM_PORTAL
        .replace("1879076987", "1879076989")
        .replace("Eastham Portal", "Portal to Eastham")
        .replace("151.05", "150");
    let mut db = db_with(SAWATO_TO_SEASIDE);
    assert_eq!(
        db.use_portal_table(&parsed_table(&[EASTHAM_PORTAL, &same])),
        1
    );
    assert!(close(
        exit_of(&db, "Town Network (N R 3) to Eastham"),
        true_eastham_arrival()
    ));
}

#[test]
fn walk_in_admin_only_and_destinationless_table_entries_are_not_candidates() {
    let walk = EASTHAM_PORTAL
        .replace("3465805877", "3465805878")
        .replace(r#""pb":1"#, r#""pb":1,"walk":1"#);
    let admin = EASTHAM_PORTAL
        .replace("3465805877", "3465805879")
        .replace(r#""pb":1"#, r#""pb":1,"inv":1"#);
    let nowhere = r#"{"g":5,"w":5,"n":"Eastham Portal","c":459105,"p":[80,-20,0],"h":0}"#;
    let mut db = db_with(SAWATO_TO_SEASIDE);
    assert_eq!(
        db.use_portal_table(&parsed_table(&[&walk, &admin, nowhere, EASTHAM_PORTAL])),
        1
    );
    assert!(close(
        exit_of(&db, "Town Network (N R 3) to Eastham"),
        true_eastham_arrival()
    ));
}

#[test]
fn the_corrections_file_wins_over_the_table_and_the_table_over_the_atlas() {
    let mut db = db_with(SAWATO_TO_SEASIDE);
    let corrected = Coordinates::new(17.0, 63.5);
    db.set_corrections(HashMap::from([(9323, corrected)]), Vec::new());
    db.use_portal_table(&parsed_table(&[EASTHAM_PORTAL]));
    assert_eq!(exit_of(&db, "Town Network (N R 3) to Eastham"), corrected);

    // Without the correction the table's arrival stands.
    db.set_corrections(HashMap::new(), Vec::new());
    assert!(close(
        exit_of(&db, "Town Network (N R 3) to Eastham"),
        true_eastham_arrival()
    ));

    // A reload of the Atlas data keeps the table's arrival once it is
    // used again, and a correction set later wins at once.
    db.load_locations_xml(SAWATO_TO_SEASIDE).unwrap();
    assert_eq!(
        exit_of(&db, "Town Network (N R 3) to Eastham"),
        Coordinates::new(16.9, 65.3)
    );
    db.use_portal_table(&parsed_table(&[EASTHAM_PORTAL]));
    assert!(close(
        exit_of(&db, "Town Network (N R 3) to Eastham"),
        true_eastham_arrival()
    ));
    db.set_corrections(HashMap::from([(9323, corrected)]), Vec::new());
    assert_eq!(exit_of(&db, "Town Network (N R 3) to Eastham"), corrected);
}
