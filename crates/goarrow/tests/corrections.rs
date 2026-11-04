//! Route corrections, from the pure parts of OpenAC-GoArrow's
//! `AtlasCorrectionsTests`.

use std::collections::HashMap;
use std::rc::Rc;

use goarrow::coords::Coordinates;
use goarrow::corrections::{AtlasCorrections, BlockedStep, SHIPPED};
use goarrow::database::LocationDatabase;
use goarrow::finder::{RouteCostProfile, RouteFinder};
use goarrow::graph::RouteEdgeKind;
use goarrow::location::Location;
use goarrow::route::{RouteStep, RouteStepKind};

const ATLAS: &str = "<atlas>
  <location><id>1</id><latitude>0</latitude><longitude>1</longitude>
    <name>Portal</name><type>Wilderness Portal</type>
    <arrival_latitude>-50</arrival_latitude><arrival_longitude>50</arrival_longitude></location>
  <location><id>2</id><latitude>-20</latitude><longitude>20</longitude>
    <name>End</name><type>Town</type></location>
</atlas>";

fn origin() -> Rc<Location> {
    Rc::new(Location::new("Player", 0.0, 0.0))
}

// AtlasCorrectionsTests.UserArrivalOverridesShippedAndAppliesToLoadedAtlas
#[test]
fn user_arrival_overrides_shipped_and_applies_to_loaded_atlas() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(ATLAS).unwrap();
    let shipped =
        AtlasCorrections::parse(r#"{ "Arrivals": [ { "Id": 1, "Arrival": "1N, 1E" } ] }"#).unwrap();
    let user = AtlasCorrections::parse(
        "// Comments and trailing commas are allowed in the user file.
        { \"arrivals\": [ { \"id\": 1, \"arrival\": \"19.9N, 20.0E\" }, ] }",
    )
    .unwrap();
    let mut errors = Vec::new();
    db.set_corrections(
        AtlasCorrections::arrivals_by_id(&[&shipped, &user], &mut errors),
        Vec::new(),
    );
    assert!(errors.is_empty());
    assert_eq!(
        db.find_location("Portal").unwrap().exit_coords(),
        Coordinates::new(19.9, 20.0)
    );
    let end = db.find_location("End").unwrap();
    let route = RouteFinder::new(db).find_route(&origin(), &end, RouteCostProfile::ShortestWalk);
    assert!(
        route
            .steps()
            .iter()
            .any(|s| s.kind == RouteStepKind::Portal)
    );
}

// AtlasCorrectionsTests.UnreadableArrivalIsReportedNotApplied
#[test]
fn unreadable_arrival_is_reported_not_applied() {
    let mut errors = Vec::new();
    let corrections =
        AtlasCorrections::parse(r#"{ "Arrivals": [ { "Id": 1, "Arrival": "nowhere" } ] }"#)
            .unwrap();
    assert!(AtlasCorrections::arrivals_by_id(&[&corrections], &mut errors).is_empty());
    assert_eq!(
        errors,
        ["Arrival correction 1 has unreadable arrival 'nowhere'."]
    );
    let corrections =
        AtlasCorrections::parse(r#"{ "Arrivals": [ { "Name": "X", "Arrival": "1N 1E" } ] }"#)
            .unwrap();
    errors.clear();
    assert!(AtlasCorrections::arrivals_by_id(&[&corrections], &mut errors).is_empty());
    assert_eq!(errors, ["Arrival correction 'X' has no Atlas id."]);
}

// AtlasCorrectionsTests.BlockedPortalStepIsNotUsedByLaterSearches
#[test]
fn blocked_portal_step_is_not_used_by_later_searches() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(ATLAS).unwrap();
    db.set_corrections(
        HashMap::from([(1, Coordinates::new(19.9, 20.0))]),
        Vec::new(),
    );
    let end = db.find_location("End").unwrap();
    let mut finder = RouteFinder::new(db);
    let route = finder.find_route(&origin(), &end, RouteCostProfile::ShortestWalk);
    let portal_step = route
        .steps()
        .iter()
        .find(|s| s.kind == RouteStepKind::Portal)
        .unwrap();
    let blocked = BlockedStep::from_route(portal_step, "Portal is gone");
    assert!(finder.has_graph_step(&blocked));

    finder.database_mut().set_corrections(
        HashMap::from([(1, Coordinates::new(19.9, 20.0))]),
        vec![blocked.clone()],
    );
    finder.invalidate_graph();
    assert!(!finder.has_graph_step(&blocked));
    let route = finder.find_route(&origin(), &end, RouteCostProfile::ShortestWalk);
    assert!(
        !route
            .steps()
            .iter()
            .any(|s| s.kind == RouteStepKind::Portal)
    );
}

// AtlasCorrectionsTests.BlockedWalkIsBlockedInBothDirections
#[test]
fn blocked_walk_is_blocked_in_both_directions() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["TownA;0;0", "TownB;0;5"]).unwrap();
    let a = db.find_location("TownA").unwrap();
    let b = db.find_location("TownB").unwrap();
    let mut finder = RouteFinder::new(db);
    let walk = RouteStep::new(RouteStepKind::Travel, a.clone(), b.clone(), 5.0, "Walk");
    let blocked = BlockedStep::from_route(&walk, "");
    let reverse = BlockedStep::from_route(
        &RouteStep::new(RouteStepKind::Travel, b, a, 5.0, "Walk"),
        "",
    );
    assert!(finder.has_graph_step(&reverse));
    finder
        .database_mut()
        .set_corrections(HashMap::new(), vec![blocked]);
    finder.invalidate_graph();
    assert!(!finder.has_graph_step(&reverse));
}

// AtlasCorrections.BlockedStep.Blocks: ids when positive, else names; the
// kind must match; only walks block both ways.
#[test]
fn blocks_matches_by_id_then_name() {
    let from = Location::with_details(
        5,
        "Gate",
        Default::default(),
        Coordinates::new(0.0, 0.0),
        "",
        0,
        Coordinates::NONE,
    );
    let to = Location::new("Gate arrival (5)", 1.0, 1.0);
    let step = BlockedStep {
        kind: "portal".into(),
        from: "Some other name".into(),
        from_id: 5,
        to: "GATE ARRIVAL (5)".into(),
        ..BlockedStep::default()
    };
    assert!(step.blocks(&from, &to, RouteEdgeKind::Portal));
    assert!(!step.blocks(&to, &from, RouteEdgeKind::Portal));
    assert!(!step.blocks(&from, &to, RouteEdgeKind::Walk));
    let recall = BlockedStep {
        kind: "Recall".into(),
        from: "Gate".into(),
        to: "Gate arrival (5)".into(),
        ..BlockedStep::default()
    };
    assert!(recall.blocks(&from, &to, RouteEdgeKind::Recall));
    assert!(recall.blocks(&from, &to, RouteEdgeKind::Lifestone));
}

// AtlasCorrectionsTests.BlockedStepRoundTripsThroughJson
#[test]
fn blocked_step_round_trips_through_json() {
    let mut corrections = AtlasCorrections::default();
    corrections.blocked_steps.push(BlockedStep {
        kind: "Portal".into(),
        from: "Portal".into(),
        from_id: 1,
        to: "Portal arrival (1)".into(),
        note: "gone".into(),
        ..BlockedStep::default()
    });
    let copy = AtlasCorrections::parse(&corrections.to_json()).unwrap();
    assert_eq!(copy.blocked_steps.len(), 1);
    let step = &copy.blocked_steps[0];
    assert_eq!(
        (
            step.kind.as_str(),
            step.from_id,
            step.to.as_str(),
            step.note.as_str()
        ),
        ("Portal", 1, "Portal arrival (1)", "gone")
    );
    assert_eq!(copy, corrections);
}

// AtlasCorrections.ToJson: indented, the C# member names in their order.
#[test]
fn json_layout() {
    let mut corrections = AtlasCorrections::default();
    corrections
        .arrivals
        .push(goarrow::corrections::ArrivalCorrection {
            id: 9341,
            name: "Town".into(),
            arrival: "52.2S, 82.5E".into(),
            note: "n".into(),
        });
    assert_eq!(
        corrections.to_json(),
        "{\n  \"Version\": 1,\n  \"Arrivals\": [\n    {\n      \"Id\": 9341,\n      \"Name\": \"Town\",\n      \
         \"Arrival\": \"52.2S, 82.5E\",\n      \"Note\": \"n\"\n    }\n  ],\n  \"BlockedSteps\": []\n}"
    );
}

// AtlasCorrections.Parse: what the C# reader accepts and refuses.
#[test]
fn parse_rules() {
    let shipped = AtlasCorrections::parse(SHIPPED).unwrap();
    assert_eq!(shipped.version, 1);
    assert_eq!(shipped.arrivals.len(), 1);
    assert_eq!(shipped.arrivals[0].id, 9341);
    assert!(shipped.blocked_steps.is_empty());
    assert_eq!(
        AtlasCorrections::parse("null").unwrap(),
        AtlasCorrections::default()
    );
    assert_eq!(
        AtlasCorrections::parse("{}").unwrap(),
        AtlasCorrections::default()
    );
    let commented = AtlasCorrections::parse(
        "/* block */ { \"VERSION\": 2, \"BlockedSteps\": [ { \"kind\": \"Travel\", \"From\": \"a // not a comment\", }, ], \
         \"Unknown\": [1,2], }",
    )
    .unwrap();
    assert_eq!(commented.version, 2);
    assert_eq!(commented.blocked_steps[0].from, "a // not a comment");
    assert!(AtlasCorrections::parse("{ not json").is_err());
    assert!(AtlasCorrections::parse("[]").is_err());
    assert!(AtlasCorrections::parse(r#"{ "Arrivals": [ { "Id": "1" } ] }"#).is_err());
    assert!(AtlasCorrections::parse(r#"{ "Arrivals": [ { "Id": 1.5 } ] }"#).is_err());
}

// DIVERGENCES.md, GA-9: a null list or entry makes the file unreadable.
#[test]
fn null_lists_are_unreadable() {
    assert!(AtlasCorrections::parse(r#"{ "Arrivals": null }"#).is_err());
    assert!(AtlasCorrections::parse(r#"{ "BlockedSteps": [ null ] }"#).is_err());
    assert!(AtlasCorrections::parse(r#"{ "Arrivals": [ { "Name": null } ] }"#).is_ok());
}

// AtlasCorrections.BlockedStep.FromRoute: the graph start, not the exact
// origin.
#[test]
fn from_route_uses_the_graph_start() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(ATLAS).unwrap();
    db.set_corrections(
        HashMap::from([(1, Coordinates::new(19.9, 20.0))]),
        Vec::new(),
    );
    let end = db.find_location("End").unwrap();
    let mut finder = RouteFinder::new(db);
    let at_gate = Rc::new(Location::new("Player", 0.0, 1.0));
    let route = finder.find_route(&at_gate, &end, RouteCostProfile::ShortestWalk);
    let first = &route.steps()[0];
    assert_eq!(first.kind, RouteStepKind::Portal);
    assert!(Rc::ptr_eq(&first.from, &at_gate));
    let blocked = BlockedStep::from_route(first, "x");
    assert_eq!(
        (
            blocked.kind.as_str(),
            blocked.from.as_str(),
            blocked.from_id
        ),
        ("Portal", "Portal", 1)
    );
    assert_eq!(
        (blocked.to.as_str(), blocked.to_id),
        ("Portal arrival (1)", 0)
    );
}

// The shipped file and the player's (a copy of it to start with) as one
// set: the player's arrival replaces the shipped one with its Atlas id,
// and a blocked step the player repeats (in another case, with a note)
// is listed once.
#[test]
fn the_shipped_and_the_players_corrections_merge_into_one_set() {
    let shipped = AtlasCorrections::parse(SHIPPED).unwrap();
    let copy = AtlasCorrections::parse(SHIPPED).unwrap();
    let merged = AtlasCorrections::merged(&[shipped.clone(), copy]);
    assert_eq!(merged.arrivals, shipped.arrivals);

    let step = |kind: &str, from: &str, from_id: i32, note: &str| BlockedStep {
        kind: kind.to_owned(),
        from: from.to_owned(),
        from_id,
        to: "Rithwic".to_owned(),
        to_id: 0,
        note: note.to_owned(),
    };
    let mut first = shipped.clone();
    first.blocked_steps = vec![step("Portal", "Gate", 0, ""), step("Travel", "Mill", 0, "")];
    let mut user = AtlasCorrections::parse(SHIPPED).unwrap();
    user.arrivals[0].arrival = "52.0S, 82.5E".to_owned();
    user.blocked_steps = vec![
        step("portal", "GATE", 0, "gone"),
        // Another end, by id: not the same step.
        step("Portal", "Gate", 7, ""),
    ];
    let merged = AtlasCorrections::merged(&[first, user]);
    assert_eq!(merged.arrivals.len(), 1);
    assert_eq!(merged.arrivals[0].arrival, "52.0S, 82.5E");
    assert_eq!(
        merged.blocked_steps,
        [
            step("portal", "GATE", 0, "gone"),
            step("Travel", "Mill", 0, ""),
            step("Portal", "Gate", 7, ""),
        ]
    );
}
