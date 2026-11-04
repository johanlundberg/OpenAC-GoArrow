//! The portal table (`portals/ace-portals.json`): reading it, finding a live
//! portal in it, and reading it from beside the plugin at start.

mod navsim;

use goarrow::ace_portals::{AcePortals, PortalSource, RESOURCE};
use navsim::{Harness, portal, pos};
use openac_plugin::LogLevel;
use openac_plugin::calls;
use openac_plugin::world::Position;

/// Four portals: a dungeon's exit, an admin-only floor trap and a
/// generator's spawn in landblock 0x02AA, and a portal in the Town Network.
pub const FIXTURE: &str = r#"{
"format":1,
"source":"test",
"version":{"base":"v0.8.8","patch":"v0.9.296","modified":"2026-10-08 05:42:11"},
"extracted":"2026-11-03",
"license":"AGPL-3.0",
"keys":"...",
"counts":{},
"portals":[
{"g":1879052292,"w":28783,"n":"Surface Portal","c":44695809,"p":[10,-20,0],"h":270,"d":2627338273,"dp":[100,50,10],"pb":1},
{"g":1879052293,"w":72119,"n":"Portal Floor Walk","c":44695810,"p":[30,-20,0],"h":0,"d":44696016,"dp":[5,5,0],"pb":49,"walk":1,"inv":1},
{"w":5000,"n":"Gauntlet Stage 1","c":44695811,"p":[50,-20,0],"h":90,"src":"g","gw":123,"pr":0.5},
{"g":1879076904,"w":42820,"n":"Portal to Holtburg","c":459044,"p":[53.91,-39.95,-0.06],"h":0,"d":2847146009,"dp":[84,7.1,94],"lmin":10,"lmax":40,"pb":1}
]}"#;

#[test]
fn the_table_reads_its_portals_by_landblock() {
    let table = AcePortals::parse(FIXTURE).expect("the fixture");
    assert_eq!(table.len(), 4);
    assert_eq!(
        table.source(),
        "ACE world v0.8.8 v0.9.296, extracted 2026-11-03"
    );
    let dungeon = table.in_landblock(0x02AA_0105);
    assert_eq!(dungeon.len(), 3);
    let surface = &dungeon[0];
    assert_eq!(surface.name, "Surface Portal");
    assert_eq!(surface.guid, Some(1_879_052_292));
    assert_eq!(surface.cell_id, 0x02AA_0101);
    assert_eq!(surface.local, [10.0, -20.0, 0.0]);
    assert_eq!(surface.heading_degrees, 270.0);
    assert_eq!(
        surface.destination,
        Some((0x9C9A_0021, [100.0, 50.0, 10.0]))
    );
    assert_eq!(surface.restrictions, Some(1));
    assert!(surface.leads_outdoors() && surface.is_usable());
    assert_eq!(surface.source, PortalSource::Placed);
    // The label an appraisal gives: the name and where it leads.
    let arrival = Position::from_cell(0x9C9A_0021, [100.0, 50.0, 10.0], 0.0);
    assert_eq!(
        surface.destination_label().as_deref(),
        Some(
            format!(
                "Surface Portal ({}).",
                goarrow::coords::Coordinates::new(arrival.north_south, arrival.east_west)
                    .to_text_with_decimals(2, false)
            )
            .as_str()
        )
    );

    let trap = &dungeon[1];
    assert!(trap.walk_on && trap.admin_only && !trap.is_usable());
    assert!(!trap.leads_outdoors(), "it leads to another room");
    assert_eq!(
        trap.destination_label().as_deref(),
        Some("Portal Floor Walk")
    );

    let stage = &dungeon[2];
    assert_eq!(stage.guid, None);
    assert_eq!(stage.source, PortalSource::Generator);
    assert_eq!(stage.destination, None);
    assert_eq!(stage.destination_label(), None);

    let exits: Vec<&str> = table
        .exits_outdoors(0x02AA_0200)
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(exits, ["Surface Portal"]);

    let holtburg = &table.in_landblock(0x0007_0124)[0];
    assert_eq!(
        (holtburg.min_level, holtburg.max_level),
        (Some(10), Some(40))
    );
    assert!(table.in_landblock(0x0101_0001).is_empty());
}

#[test]
fn a_table_of_another_format_or_malformed_is_refused() {
    assert!(
        AcePortals::parse(r#"{"format":2,"portals":[]}"#)
            .unwrap_err()
            .contains("format 2")
    );
    assert!(AcePortals::parse("{").is_err());
}

#[test]
fn a_live_portal_is_found_by_its_name_and_place() {
    let table = AcePortals::parse(FIXTURE).expect("the fixture");
    let at = Position::from_cell(0x02AA_0101, [10.4, -20.3, 0.2], 0.0);
    let live = portal(7, "Surface Portal", at.clone());
    let entry = table
        .find_object(&live)
        .expect("the table's Surface Portal");
    assert_eq!(entry.guid, Some(1_879_052_292));
    // Another name, or more than a metre away, is another portal.
    assert!(table.find_object(&portal(7, "Portal", at)).is_none());
    let far = Position::from_cell(0x02AA_0101, [11.2, -20.0, 0.0], 0.0);
    assert!(
        table
            .find_object(&portal(7, "Surface Portal", far))
            .is_none()
    );
    // The destination it fills in is the appraisal's.
    let mut objects = vec![
        portal(
            7,
            "Surface Portal",
            Position::from_cell(0x02AA_0101, [10.0, -20.0, 0.0], 0.0),
        ),
        portal(8, "Unknown Portal", pos(0x02AA_0101, 0.0, 0.0, 0.0, false)),
    ];
    assert!(table.fill_destinations(&mut objects));
    assert_eq!(objects[0].portal_destination, entry.destination_label());
    assert_eq!(objects[1].portal_destination, None);
}

fn info_and_warnings(h: &Harness) -> (Vec<String>, Vec<String>) {
    let lines = |level: LogLevel| {
        h.sim
            .fake
            .logs
            .iter()
            .filter(|(l, m)| *l == level && m.contains("portal table"))
            .map(|(_, m)| m.clone())
            .collect::<Vec<_>>()
    };
    (lines(LogLevel::Info), lines(LogLevel::Warn))
}

#[test]
fn the_navigator_reads_the_table_beside_the_plugin_at_start() {
    let mut h = Harness::empty();
    h.sim
        .fake
        .set_reply::<calls::StorageRead>(&Ok(Some(FIXTURE.as_bytes().to_vec())));
    h.enable();
    assert_eq!(h.nav.ace_portals().len(), 4);
    let reads = h.sim.fake.requests::<calls::StorageRead>();
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].key, RESOURCE);
    assert_eq!(
        reads[0].place.root,
        openac_plugin::storage::StorageRoot::PluginFiles
    );
    let (info, warnings) = info_and_warnings(&h);
    assert_eq!(
        info,
        ["GoArrow: portal table: 4 portals (ACE world v0.8.8 v0.9.296, extracted 2026-11-03)."]
    );
    assert!(warnings.is_empty());
}

#[test]
fn without_the_table_the_navigator_says_so_once() {
    let mut h = Harness::empty();
    h.enable();
    assert!(h.nav.ace_portals().is_empty());
    let (info, warnings) = info_and_warnings(&h);
    assert_eq!(
        info,
        [
            "GoArrow: no portal table (portals/ace-portals.json); dungeon exits are found by exploring."
        ]
    );
    assert!(warnings.is_empty());
}

#[test]
fn a_table_that_does_not_read_is_one_warning_and_an_empty_table() {
    let mut h = Harness::empty();
    h.sim
        .fake
        .set_reply::<calls::StorageRead>(&Ok(Some(b"{ not json".to_vec())));
    h.enable();
    assert!(h.nav.ace_portals().is_empty());
    let (info, warnings) = info_and_warnings(&h);
    assert!(info.is_empty());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("could not be read"), "{warnings:?}");
}

/// The shipped table: run with `cargo test -- --ignored`.
#[test]
#[ignore = "reads the shipped portals/ace-portals.json"]
fn the_shipped_table_reads_and_holds_known_portals() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../portals/ace-portals.json"
    );
    let json = std::fs::read_to_string(path).expect("portals/ace-portals.json");
    let table = AcePortals::parse(&json).expect("the shipped table");
    assert_eq!(table.len(), 4964);
    // A Town Network portal to Holtburg, and where it leads.
    let holtburg = table
        .in_landblock(0x0007_0124)
        .iter()
        .find(|p| p.guid == Some(0x7000_7028))
        .expect("the Town Network's portal to Holtburg");
    assert_eq!(holtburg.name, "Portal to Holtburg");
    assert_eq!(holtburg.cell_id, 0x0007_0124);
    assert_eq!(holtburg.destination.map(|d| d.0), Some(0xA9B4_0019));
    assert!(holtburg.leads_outdoors());
    let arrival = holtburg.arrival().expect("outdoors");
    // Holtburg is at about 42.1N, 33.6E.
    assert!(
        (arrival.ns - 42.1).abs() < 0.3 && (arrival.ew - 33.6).abs() < 0.3,
        "{arrival}"
    );
    // Most dungeon exits are named "Surface".
    let surfaces = (0..=0xFFFFu32)
        .flat_map(|lb| table.in_landblock(lb << 16))
        .filter(|p| p.name == "Surface")
        .count();
    assert!(surfaces > 700, "{surfaces}");
}
