//! The destination and its guidance, from OpenAC-GoArrow's
//! `DestinationTests` and the pure parts of `RoutePlanningIntegrationTests`
//! and `RouteStepNotesTests`.

use std::rc::Rc;

use goarrow::coords::{Coordinates, NavPosition};
use goarrow::database::LocationDatabase;
use goarrow::destination::{CyclePlan, Destination, DestinationKind, coordinate_destination_name};
use goarrow::finder::RouteFinder;
use goarrow::location::Location;
use goarrow::route::RouteStepKind;

fn at(name: &str, ns: f64, ew: f64) -> Rc<Location> {
    Rc::new(Location::new(name, ns, ew))
}

fn finder_of_xml(xml: &str) -> RouteFinder {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(xml).unwrap();
    RouteFinder::new(db)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// DestinationTests.CoordinateDestinationRetainsKindAndDisplayText
#[test]
fn coordinate_destination_retains_kind_and_display_text() {
    let mut destination = Destination::new();
    destination.set_coordinate(42.1, 33.6, Some("42.1N 33.6E"));
    assert_eq!(destination.kind(), DestinationKind::Coordinates);
    assert_eq!(destination.coordinate_text(), "42.1N 33.6E");
    assert_eq!(
        destination.target().unwrap().coords,
        Coordinates::new(42.1, 33.6)
    );
    assert_eq!(destination.saved_name(), "42.1N 33.6E");
    destination.set_coordinate(1.0, 2.0, Some("  padded  "));
    assert_eq!(destination.target_name(), "padded");
}

// DIVERGENCES.md, GA-2: a point named from numbers reads as the map shows
// it, not `-12.3N -4E`.
#[test]
fn a_point_without_text_gets_the_standard_name() {
    assert_eq!(coordinate_destination_name(-12.3, -4.0), "12.3S, 4.0W");
    assert_eq!(coordinate_destination_name(42.125, 33.6), "42.13N, 33.60E");
    let mut destination = Destination::new();
    destination.set_coordinate(-12.3, -4.0, None);
    assert_eq!(destination.target_name(), "12.3S, 4.0W");
    assert_eq!(destination.coordinate_text(), "12.3S, 4.0W");
    destination.set_coordinate(-12.3, -4.0, Some("   "));
    assert_eq!(destination.target_name(), "12.3S, 4.0W");
}

// DestinationTests.ObjectDestinationBecomesUnavailableWhenSelectionIsLost
#[test]
fn object_destination_becomes_unavailable_when_selection_is_lost() {
    let mut destination = Destination::new();
    let position = NavPosition {
        cell_id: 1,
        east_west: 4.0,
        north_south: 5.0,
        is_outdoor: true,
        ..NavPosition::default()
    };
    assert!(destination.set_object(12, "Portal", Some(position)));
    destination.mark_object_unavailable();
    assert_eq!(destination.kind(), DestinationKind::Object);
    assert!(destination.target_unavailable());
    assert_eq!(destination.target_object_id(), Some(12));
    assert_eq!(
        destination.target().unwrap().coords,
        Coordinates::new(5.0, 4.0)
    );
}

// GoArrowDestination.SetObject / UpdateObject
#[test]
fn object_destination_rules() {
    let mut destination = Destination::new();
    let position = NavPosition {
        north_south: 1.0,
        east_west: 1.0,
        ..NavPosition::default()
    };
    assert!(!destination.set_object(0, "x", Some(position)));
    assert!(!destination.set_object(5, "x", None));
    assert!(destination.set_object(12, " ", Some(position)));
    assert_eq!(destination.target_name(), "Object 0x0000000C");
    let moved = NavPosition {
        north_south: 2.0,
        ..position
    };
    destination.update_object(99, "Other", Some(moved));
    assert_eq!(destination.target().unwrap().coords.ns, 1.0);
    destination.update_object(12, "", Some(moved));
    assert_eq!(destination.target_name(), "Object 0x0000000C");
    assert_eq!(destination.target().unwrap().coords.ns, 2.0);
    assert_eq!(destination.target_object_position(), Some(moved));
    destination.update_object(12, "Named", None);
    assert!(destination.target_unavailable());
    destination.update_object(12, "Named", Some(moved));
    assert!(!destination.target_unavailable());
    assert_eq!(destination.target_name(), "Named");
}

// DestinationTests.RouteGuidancePointsToFirstGraphWaypoint
#[test]
fn route_guidance_points_to_first_graph_waypoint() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Start;0;0", "First;0;10", "Finish;10;10"])
        .unwrap();
    let mut finder = RouteFinder::new(db);
    let mut destination = Destination::new();
    assert!(destination.set_destination_by_name(finder.database(), "Finish"));
    destination.calculate_route(&at("Current", 0.0, 0.0), &mut finder);
    assert_eq!(destination.immediate_target().unwrap().name, "First");
    assert!(close(destination.bearing_degrees(), 90.0));
    assert!(close(destination.guidance_distance(), 10.0));
    assert!(close(destination.estimated_distance(), 200f64.sqrt()));
}

// DestinationTests.BearingUsesClockwiseDegreesFromNorth
#[test]
fn bearing_uses_clockwise_degrees_from_north() {
    for (ns, ew, expected) in [
        (0.0, 1.0, 90.0),
        (-1.0, 0.0, 180.0),
        (0.0, -1.0, 270.0),
        (1.0, -1.0, 315.0),
    ] {
        let mut destination = Destination::new();
        destination.set_coordinate(ns, ew, Some("Target"));
        destination.update_guidance(&Location::new("Current", 0.0, 0.0));
        assert!(close(destination.bearing_degrees(), expected), "{ns},{ew}");
    }
}

// DestinationTests.CoordinateDestinationRoutesThroughAPortalInsteadOfWalking
#[test]
fn coordinate_destination_routes_through_a_portal_instead_of_walking() {
    let mut finder = finder_of_xml(
        r#"<locations><loc name="Far Portal" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="95" /></locations>"#,
    );
    let mut destination = Destination::new();
    destination.set_coordinate(0.0, 97.0, Some("0N 97E"));
    destination.calculate_route(&at("Current", 0.0, 0.0), &mut finder);
    let route = destination.current_route().unwrap();
    assert_eq!(route.portal_count(), 1);
    assert_eq!(destination.immediate_target().unwrap().name, "Far Portal");
    assert_eq!(route.steps().last().unwrap().to.name, "0N 97E");
}

// GoArrowDestination: no destination means no guidance; clearing resets.
#[test]
fn clear_destination_resets_everything() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["A;0;0", "B;0;5"]).unwrap();
    let mut finder = RouteFinder::new(db);
    let mut destination = Destination::new();
    assert!(!destination.has_destination());
    assert!(destination.estimated_distance().is_nan());
    assert!(!destination.set_destination_by_name(finder.database(), "Nowhere"));
    assert!(destination.set_destination_by_name(finder.database(), "b"));
    assert_eq!(destination.saved_name(), "B");
    destination.calculate_route(&at("Here", 0.0, 0.0), &mut finder);
    assert!(destination.current_route().is_some());
    destination.clear_destination();
    assert!(!destination.has_destination());
    assert!(destination.current_route().is_none());
    assert!(destination.bearing_degrees().is_nan());
    assert!(destination.guidance_distance().is_nan());
    assert_eq!(destination.saved_name(), "");
    assert_eq!(destination.target_name(), "");
}

// GoArrowDestination.AdvanceStep and GetImmediateTarget
#[test]
fn advance_step_drops_the_first_step() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc id="2" name="Far Portal" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="95" />
          <loc name="End" type="Town" NS="0" EW="96" />
        </locations>"#,
    );
    let mut destination = Destination::new();
    assert!(destination.set_destination_by_name(finder.database(), "End"));
    destination.calculate_route(&at("Here", 0.0, 0.0), &mut finder);
    let texts: Vec<String> = destination
        .current_route()
        .unwrap()
        .steps()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        texts,
        [
            "Walk: Far Portal (480 m)",
            "Portal: Far Portal",
            "Walk: End (240 m)"
        ]
    );
    assert_eq!(destination.immediate_target().unwrap().name, "Far Portal");
    destination.advance_step();
    // A portal step points at where it is used.
    assert_eq!(
        destination.current_route().unwrap().steps()[0].kind,
        RouteStepKind::Portal
    );
    assert_eq!(destination.immediate_target().unwrap().name, "Far Portal");
    destination.advance_step();
    assert_eq!(destination.immediate_target().unwrap().name, "End");
    destination.advance_step();
    assert_eq!(destination.current_route().unwrap().step_count(), 0);
    assert_eq!(destination.estimated_distance(), 0.0);
    assert_eq!(destination.immediate_target().unwrap().name, "End");
}

// GoArrowDestination.CycleAlternative and CanKeepSelectedAlternative
#[test]
fn alternatives_cycle_and_reset_when_the_origin_moves() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc name="Portal A" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="99" />
          <loc name="Portal B" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="98" />
          <loc name="Portal C" type="WildernessPortal" NS="0" EW="3" exitNS="0" exitEW="97" />
          <loc name="End" type="Town" NS="0" EW="100" />
        </locations>"#,
    );
    let origin = at("Here", 0.0, 0.0);
    let mut destination = Destination::new();
    assert!(!destination.cycle_alternative(1, &origin, &mut finder));
    assert!(destination.set_destination_by_name(finder.database(), "End"));
    destination.calculate_route(&origin, &mut finder);
    assert!(!destination.cycle_alternative(2, &origin, &mut finder));
    assert!(!destination.cycle_alternative(-1, &origin, &mut finder));

    let via = |d: &Destination| d.current_route().unwrap().steps()[1].via.clone();
    assert_eq!(via(&destination), "Portal A");
    assert!(destination.cycle_alternative(1, &origin, &mut finder));
    assert_eq!(destination.alternative_index(), 1);
    assert_eq!(destination.alternative_count(), 2);
    assert!(destination.alternative_has_more());
    assert_eq!(via(&destination), "Portal B");
    assert!(destination.cycle_alternative(1, &origin, &mut finder));
    assert_eq!(via(&destination), "Portal C");
    assert_eq!(destination.alternative_count(), 3);
    // Asking for four finds three: no more.
    assert!(!destination.cycle_alternative(1, &origin, &mut finder));
    assert!(!destination.alternative_has_more());
    assert!(destination.cycle_alternative(-1, &origin, &mut finder));
    assert_eq!(via(&destination), "Portal B");

    let near = NavPosition {
        east_west: 10.0 / 240.0,
        is_outdoor: true,
        ..NavPosition::default()
    };
    assert!(destination.can_keep_selected_alternative(&near));
    assert!(!destination.can_keep_selected_alternative(&NavPosition {
        is_outdoor: false,
        ..near
    }));
    assert!(!destination.can_keep_selected_alternative(&NavPosition {
        east_west: 0.1,
        ..near
    }));

    // From more than 20 m away the alternatives start again.
    let moved = at("Moved", 0.0, 0.1);
    assert_eq!(destination.prepare_cycle(1, &moved), CyclePlan::Fetch(2));
    assert_eq!(destination.alternative_index(), 0);
    assert!(destination.finish_cycle(
        1,
        &moved,
        Some((
            finder.find_route_alternatives(
                &moved,
                destination.target().unwrap(),
                destination.profile,
                2
            ),
            2
        ))
    ));
    assert_eq!(destination.alternative_index(), 1);
    assert_eq!(destination.prepare_cycle(-1, &moved), CyclePlan::Move);
    assert!(destination.finish_cycle(-1, &moved, None));
    assert_eq!(via(&destination), "Portal A");
}

// GoArrowDestination.TryPlanDungeonExit
#[test]
fn dungeon_exit_takes_the_cheapest_onward_walk() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc id="10" name="Cave" type="Dungeon" NS="0" EW="0" dungeonId="1234" />
          <loc id="11" name="Cave to Far" type="WildernessPortal" NS="0" EW="0" exitNS="0" exitEW="50" />
          <loc id="12" name="Cave to Near" type="WildernessPortal" NS="0" EW="0" exitNS="0" exitEW="58" />
          <loc id="13" name="Caveman to End" type="WildernessPortal" NS="0" EW="0" exitNS="0" exitEW="59.9" />
          <loc name="End" type="Town" NS="0" EW="60" />
        </locations>"#,
    );
    let mut destination = Destination::new();
    assert!(!destination.try_plan_dungeon_exit(0x1234_0105, &mut finder));
    assert!(destination.set_destination_by_name(finder.database(), "End"));
    assert!(!destination.try_plan_dungeon_exit(0x9999_0105, &mut finder));
    assert!(destination.try_plan_dungeon_exit(0x1234_0105, &mut finder));
    let route = destination.current_route().unwrap();
    assert_eq!(route.step_count(), 1);
    let step = &route.steps()[0];
    assert_eq!(step.from.name, "Cave to Near arrival (12)");
    assert_eq!(step.via, "Walk after dungeon exit");
    assert_eq!(step.to.name, "End");
}

// GoArrowDestination.TryPlanFromPortalArrival
#[test]
fn plan_from_a_portal_arrival() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc id="5" name="Hub Portal" type="PortalHub" NS="0" EW="0" exitNS="0" exitEW="50" />
          <loc name="End" type="Town" NS="0" EW="52" />
        </locations>"#,
    );
    let mut destination = Destination::new();
    assert!(destination.set_destination_by_name(finder.database(), "End"));
    assert!(!destination.try_plan_from_portal_arrival("Nowhere", &mut finder));
    assert!(destination.try_plan_from_portal_arrival("hub portal", &mut finder));
    let route = destination.current_route().unwrap();
    assert_eq!(route.steps()[0].from.name, "Hub Portal arrival (5)");
    assert_eq!(route.steps().last().unwrap().to.name, "End");
}

// GoArrowDestination.AvoidStepForSession
#[test]
fn avoided_step_is_left_out_until_reload() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc id="2" name="Far Portal" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="95" />
          <loc name="End" type="Town" NS="0" EW="96" />
        </locations>"#,
    );
    let origin = at("Here", 0.0, 0.0);
    let mut destination = Destination::new();
    assert!(destination.set_destination_by_name(finder.database(), "End"));
    destination.calculate_route(&origin, &mut finder);
    let portal = destination.current_route().unwrap().steps()[1].clone();
    destination.avoid_step_for_session(&portal, "Observed arrival", &mut finder);
    destination.calculate_route(&origin, &mut finder);
    assert_eq!(destination.current_route().unwrap().portal_count(), 0);
    finder.database_mut().remove_session_blocked_steps(2);
    destination.calculate_route(&origin, &mut finder);
    assert_eq!(destination.current_route().unwrap().portal_count(), 1);
}

// RoutePlanningIntegrationTests.StartupLoadsCachedAtlasPortalRoutes: the
// route and coordinate texts the panel shows.
#[test]
fn atlas_portal_route_texts() {
    let mut finder = finder_of_xml(
        "<atlas>
          <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
          <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><description>Take the portal near the town gate.</description><retired>N</retired></location>
          <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
        </atlas>",
    );
    let mut destination = Destination::new();
    assert!(destination.set_destination_by_name(finder.database(), "End"));
    destination.calculate_route(&at("Current Position", 0.0, 1.0), &mut finder);
    let route = destination.current_route().unwrap();
    let texts: Vec<String> = route.steps().iter().map(ToString::to_string).collect();
    assert_eq!(
        texts,
        [
            "Walk: Far Portal (240 m)",
            "Portal: Far Portal",
            "Walk: End (240 m)"
        ]
    );
    let portal = route
        .steps()
        .iter()
        .find(|s| s.kind == RouteStepKind::Portal)
        .unwrap();
    assert_eq!(portal.from.coords.to_string(), "0.0N, 2.0E");
    assert_eq!(portal.from.exit_coords().to_string(), "0.0N, 95.0E");
    assert!(portal.from.notes.contains("town gate"));
}

// RoutePlanningIntegrationTests.SearchFieldsPlanFromNamedPlaceToCurrentLocationWithoutMoving:
// a current-location destination is the player's point, named by its
// coordinates.
#[test]
fn current_location_destination_text() {
    assert_eq!(
        coordinate_destination_name(9.0, 9.0),
        Coordinates::new(9.0, 9.0).to_string()
    );
}

// RouteStepNotesTests.RouteListMarksStepsWhoseDetailsHaveNotes: the route
// to a place keeps the place itself, notes and all.
#[test]
fn route_steps_keep_the_place() {
    let mut finder = RouteFinder::new(LocationDatabase::new());
    let target = Rc::new(Location::with_details(
        1,
        "Library",
        Default::default(),
        Coordinates::new(0.0, 1.0),
        "Read the plaque by the entrance.",
        0,
        Coordinates::NONE,
    ));
    let mut destination = Destination::new();
    destination.set_destination(target.clone());
    destination.calculate_route(&at("Start", 0.0, 0.0), &mut finder);
    let route = destination.current_route().unwrap();
    assert_eq!(route.step_count(), 1);
    assert!(Rc::ptr_eq(&route.steps()[0].to, &target));
    assert!(route.steps()[0].to.notes.contains("plaque"));
}
