//! The route finder, from OpenAC-GoArrow's `RouteFinderTests`.

use std::rc::Rc;

use goarrow::corrections::BlockedStep;
use goarrow::database::LocationDatabase;
use goarrow::finder::{RouteCostProfile, RouteFinder};
use goarrow::location::Location;
use goarrow::route::{Route, RouteStepKind};

fn finder_of_xml(xml: &str) -> RouteFinder {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(xml).unwrap();
    RouteFinder::new(db)
}

fn at(name: &str, ns: f64, ew: f64) -> Rc<Location> {
    Rc::new(Location::new(name, ns, ew))
}

fn portal_via(route: &Route) -> String {
    route
        .steps()
        .iter()
        .find(|s| s.kind == RouteStepKind::Portal)
        .unwrap()
        .via
        .clone()
}

// RouteFinderTests.AlternativeRoutesUseDistinctPortalsInShortestWalkOrder
#[test]
fn alternatives_use_distinct_portals_in_shortest_walk_order() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc name="Portal A" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="99" />
          <loc name="Portal B" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="98" />
          <loc name="Portal C" type="WildernessPortal" NS="0" EW="3" exitNS="0" exitEW="97" />
          <loc name="End" type="Town" NS="0" EW="100" />
        </locations>"#,
    );
    let end = finder.database().find_location("End").unwrap();
    let routes = finder.find_route_alternatives(
        &at("Player", 0.0, 0.0),
        &end,
        RouteCostProfile::ShortestWalk,
        8,
    );
    let vias: Vec<String> = routes.iter().take(3).map(portal_via).collect();
    assert_eq!(vias, ["Portal A", "Portal B", "Portal C"]);
    assert_eq!(routes.len(), 3);
    assert!(routes[0].total_distance() < routes[1].total_distance());
    assert!(routes[1].total_distance() < routes[2].total_distance());
}

// RouteFinderTests.DirectWalkCanBeFollowedByLongerPortalAlternative
#[test]
fn direct_walk_can_be_followed_by_longer_portal_alternative() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc name="Long Portal" type="WildernessPortal" NS="0" EW="9" exitNS="0" exitEW="9.5" />
          <loc name="End" type="Town" NS="0" EW="8" />
        </locations>"#,
    );
    let end = finder.database().find_location("End").unwrap();
    let routes = finder.find_route_alternatives(
        &at("Player", 0.0, 0.0),
        &end,
        RouteCostProfile::ShortestWalk,
        8,
    );
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].portal_count(), 0);
    assert_eq!(portal_via(&routes[1]), "Long Portal");
    // At most `maximum`.
    let one = finder.find_route_alternatives(
        &at("Player", 0.0, 0.0),
        &end,
        RouteCostProfile::ShortestWalk,
        1,
    );
    assert_eq!(one.len(), 1);
    assert!(
        finder
            .find_route_alternatives(
                &at("Player", 0.0, 0.0),
                &end,
                RouteCostProfile::ShortestWalk,
                0
            )
            .is_empty()
    );
}

// RouteFinderTests.CoordinateDestinationUsesPortalAndWalksTheLastStretch
#[test]
fn coordinate_destination_uses_portal_and_walks_the_last_stretch() {
    let mut finder = finder_of_xml(
        r#"<locations><loc name="Far Portal" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="95" /></locations>"#,
    );
    let target = at("40N 97E", 0.0, 97.0);
    let route = finder.find_route(
        &at("Player", 0.0, 0.0),
        &target,
        RouteCostProfile::ShortestWalk,
    );
    let kinds: Vec<RouteStepKind> = route.steps().iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [
            RouteStepKind::Travel,
            RouteStepKind::Portal,
            RouteStepKind::Travel
        ]
    );
    assert_eq!(route.steps()[1].via, "Far Portal");
    let last = route.steps().last().unwrap();
    assert!(Rc::ptr_eq(&last.to, &target));
    assert!((last.distance - 2.0).abs() < 1e-6);
    assert!(route.description.is_empty());
}

// RouteFinderTests.NearbyCoordinateDestinationIsAPlainWalk
#[test]
fn nearby_coordinate_destination_is_a_plain_walk() {
    let mut finder = finder_of_xml(
        r#"<locations><loc name="Far Portal" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="95" /></locations>"#,
    );
    let target = at("0N 3E", 0.0, 3.0);
    let route = finder.find_route(
        &at("Player", 0.0, 0.0),
        &target,
        RouteCostProfile::ShortestWalk,
    );
    assert_eq!(route.step_count(), 1);
    let step = &route.steps()[0];
    assert_eq!(step.kind, RouteStepKind::Travel);
    assert!(Rc::ptr_eq(&step.to, &target));
    assert!((step.distance - 3.0).abs() < 1e-6);
}

// RouteFinderTests.CoordinateDestinationAlternativesOfferWalkAndPortal
#[test]
fn coordinate_destination_alternatives_offer_walk_and_portal() {
    let mut finder = finder_of_xml(
        r#"<locations><loc name="Long Portal" type="WildernessPortal" NS="0" EW="9" exitNS="0" exitEW="9.5" /></locations>"#,
    );
    let target = at("0N 8E", 0.0, 8.0);
    let routes = finder.find_route_alternatives(
        &at("Player", 0.0, 0.0),
        &target,
        RouteCostProfile::ShortestWalk,
        8,
    );
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].portal_count(), 0);
    assert_eq!(portal_via(&routes[1]), "Long Portal");
    for route in &routes {
        assert!(Rc::ptr_eq(&route.steps().last().unwrap().to, &target));
    }
}

// RouteFinderTests.CoordinateDestinationDoesNotWalkOutOfADungeonEntrance
#[test]
fn coordinate_destination_does_not_walk_out_of_a_dungeon_entrance() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc name="Cave" type="Dungeon" NS="0" EW="90" />
          <loc name="Cave Portal" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="90" />
        </locations>"#,
    );
    let target = at("0N 92E", 0.0, 92.0);
    let route = finder.find_route(
        &at("Player", 0.0, 0.0),
        &target,
        RouteCostProfile::ShortestWalk,
    );
    assert!(
        route
            .steps()
            .iter()
            .any(|s| s.kind == RouteStepKind::Portal)
    );
    assert!(!route.steps().iter().any(|s| s.from.name == "Cave"));
    assert!(Rc::ptr_eq(&route.steps().last().unwrap().to, &target));
}

fn test_finder() -> RouteFinder {
    let mut db = LocationDatabase::new();
    db.load_locations_csv([
        "TownA;0;0",
        "TownB;10;0",
        "TownC;10;10",
        "TownD;0;10",
        "PortalDest;5;5",
    ])
    .unwrap();
    db.load_portal_devices_csv(["PortalDest;Magic Portal;Dereth"]);
    db.load_route_starts_csv(["TownB;TownA;Walk"]);
    RouteFinder::new(db)
}

// RouteFinderTests.RouteFinder_FindRouteToKnownDestination_ReturnsRoute
#[test]
fn route_to_known_destination() {
    let route = test_finder().find_route_to_name(&at("Current", 0.0, 0.0), "TownB");
    assert_eq!(route.destination, "TownB");
}

// RouteFinderTests.RouteFinder_FindRouteToUnknownDestination_ReturnsEmptyRoute
#[test]
fn route_to_unknown_destination() {
    let route = test_finder().find_route_to_name(&at("Current", 0.0, 0.0), "Nowhere");
    assert!(route.description.to_lowercase().contains("not found"));
    assert_eq!(route.description, "Destination 'Nowhere' not found.");
    assert_eq!(route.step_count(), 0);
}

// RouteFinder.FindRoute: a destination without coordinates.
#[test]
fn destination_without_coordinates() {
    let route = test_finder().find_route(
        &at("Current", 0.0, 0.0),
        &Rc::new(Location::named("Lost")),
        RouteCostProfile::ShortestWalk,
    );
    assert_eq!(route.description, "Destination 'Lost' has no coordinates.");
    assert_eq!(route.step_count(), 0);
}

// RouteFinderTests.RouteFinder_FindNearestNamedLocation_ReturnsClosest
#[test]
fn nearest_named_location() {
    let nearest = test_finder()
        .find_nearest_named_location(&Location::new("Near A", 0.5, 0.5))
        .unwrap();
    assert_eq!(nearest.name, "TownA");
    assert!(
        test_finder()
            .find_nearest_named_location(&Location::new("Far", 90.0, 90.0))
            .is_none()
    );
}

// RouteFinderTests.RouteFinder_SearchLocations_ReturnsMatching
#[test]
fn search_locations() {
    assert_eq!(test_finder().search_locations("Town").len(), 4);
}

// RouteFinderTests.RouteFinder_GetAllDestinationNames_ReturnsDistinct
#[test]
fn all_destination_names() {
    let names = test_finder().all_destination_names();
    assert_eq!(names.len(), 5);
    assert!(names.contains(&"TownA".to_owned()));
    assert!(names.contains(&"TownC".to_owned()));
    assert_eq!(names[0], "PortalDest");
}

// RouteFinderTests.RouteFinder_GraphBasedRoute_UsesWalkEdges
#[test]
fn graph_route_uses_walk_edges() {
    let route = test_finder().find_route_to_name(&at("At Town A", 0.0, 0.0), "TownC");
    assert_eq!(route.destination, "TownC");
    assert!(route.step_count() > 0);
    assert!(route.total_distance() > 0.0);
    assert_eq!(route.portal_count(), 0);
}

// RouteFinderTests.RouteStartsAtExactCurrentCoordinatesEvenForTinyOffset
#[test]
fn route_starts_at_exact_current_coordinates_even_for_tiny_offset() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Start;0;0", "Middle;0;8", "End;0;16"])
        .unwrap();
    let current = at("Current Position", 0.003, 0.004);
    let route = RouteFinder::new(db).find_route_to_name(&current, "End");
    assert!(route.step_count() > 0);
    assert!(Rc::ptr_eq(&route.steps()[0].from, &current));
    assert_eq!(route.steps()[0].via, "Walk to start");
}

// RouteFinderTests.RouteKeepsExactOriginWhenStandingOnAGraphNode
#[test]
fn route_keeps_exact_origin_when_standing_on_a_graph_node() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Start;0;0", "Middle;0;8", "End;0;16"])
        .unwrap();
    let current = at("Current Position", 0.0, 0.0);
    let route = RouteFinder::new(db).find_route_to_name(&current, "End");
    assert!(Rc::ptr_eq(&route.steps()[0].from, &current));
    // Entering at Middle (8 + 8) ties with Start, Middle (0 + 8 + 8); the
    // seed found first keeps the node, so the route walks to Middle.
    assert_eq!(route.steps()[0].via, "Walk to start");
    assert_eq!(route.steps()[0].to.name, "Middle");
    assert_eq!(route.step_count(), 2);
}

// RouteFinder.RouteFromPath: an origin on the entry node starts the first
// graph step there, which keeps the node as the place it leaves from.
#[test]
fn an_origin_on_the_entry_node_starts_the_first_graph_step() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc id="7" name="Gate" type="WildernessPortal" NS="0" EW="0" exitNS="0" exitEW="50" />
          <loc name="End" type="Town" NS="0" EW="51" />
        </locations>"#,
    );
    let current = at("Current Position", 0.0, 0.0);
    let route = finder.find_route_to_name(&current, "End");
    let texts: Vec<String> = route.steps().iter().map(ToString::to_string).collect();
    assert_eq!(texts, ["Portal: Gate", "Walk: End (240 m)"]);
    assert!(Rc::ptr_eq(&route.steps()[0].from, &current));
    assert_eq!(route.steps()[0].graph_from.name, "Gate");
    assert_eq!(route.steps()[0].to.name, "Gate arrival (7)");
}

// RouteFinderTests.RouteCanEnterGraphAwayFromNearestNamedLocation
#[test]
fn route_can_enter_graph_away_from_nearest_named_location() {
    let mut finder = finder_of_xml(
        "<atlas>
          <location><id>1</id><name>Nearest Town</name><type>Town</type><latitude>0</latitude><longitude>-1</longitude><retired>N</retired></location>
          <location><id>2</id><name>Useful Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>5</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
          <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
        </atlas>",
    );
    let current = at("Current Position", 0.0, 0.0);
    let route = finder.find_route_to_name(&current, "End");
    assert_eq!(route.steps()[0].to.name, "Useful Portal");
    assert!(Rc::ptr_eq(&route.steps()[0].from, &current));
    assert_eq!(route.portal_count(), 1);
}

// RouteFinderTests.RouteFinder_FindRouteFindsDirectRouteStart
#[test]
fn route_finds_direct_route_start() {
    let route = test_finder().find_route_to_name(&at("At Town A", 0.0, 0.0), "TownB");
    assert_eq!(route.destination, "TownB");
    assert!(route.step_count() > 0);
    assert!(route.steps()[0].via.contains("Walk"));
}

// RouteFinderTests.RouteFinder_UsesAtlasPortalEntranceAndArrivalCoordinates
#[test]
fn uses_atlas_portal_entrance_and_arrival_coordinates() {
    let mut finder = finder_of_xml(
        "<atlas>
          <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
          <location><id>2</id><name>Shared Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>20</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>50</arrival_longitude><retired>N</retired></location>
          <location><id>3</id><name>Shared Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
          <location><id>4</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
        </atlas>",
    );
    let route = finder.find_route_to_name(&at("Here", 0.0, 1.0), "End");
    assert_eq!(route.portal_count(), 1);
    assert_eq!(route.step_count(), 3);
    let s = route.steps();
    assert_eq!(s[0].kind, RouteStepKind::Travel);
    assert_eq!(s[0].to.coords.ew, 2.0);
    assert_eq!(s[1].kind, RouteStepKind::Portal);
    assert_eq!(s[1].via, "Shared Portal");
    assert_eq!(s[1].from.coords.ew, 2.0);
    assert_eq!(s[1].to.coords.ew, 95.0);
    assert_eq!(s[2].to.coords.ew, 96.0);
}

// RouteFinderTests.RouteFinder_IgnoresRetiredAndUnmappedAtlasPortals
#[test]
fn ignores_retired_and_unmapped_atlas_portals() {
    let mut finder = finder_of_xml(
        "<atlas>
          <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
          <location><id>2</id><name>Retired Portal</name><type>Town Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>Y</retired></location>
          <location><id>3</id><name>Unknown Exit</name><type>Town Portal</type><latitude>0</latitude><longitude>3</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>0</arrival_longitude><retired>N</retired></location>
          <location><id>4</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
        </atlas>",
    );
    let route = finder.find_route_to_name(&at("Here", 0.0, 1.0), "End");
    assert_eq!(route.portal_count(), 0);
    assert!(route.description.contains("Direct walk"));
    assert_eq!(route.description, "Direct walk (no graph route found)");
}

// RouteFinder.CostSelector: the profiles change which route wins.
#[test]
fn cost_profiles_weigh_interactions() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc name="Short Portal" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="7.5" />
          <loc name="End" type="Town" NS="0" EW="8" />
        </locations>"#,
    );
    let end = finder.database().find_location("End").unwrap();
    let origin = at("Player", 0.0, 0.0);
    // 2 + 0.1 + 0.5 beats walking 8 straight, unless the portal costs a
    // thousand times more or every walk costs 1 (one walk beats two).
    assert_eq!(
        finder
            .find_route(&origin, &end, RouteCostProfile::ShortestWalk)
            .portal_count(),
        1
    );
    assert_eq!(
        finder
            .find_route(&origin, &end, RouteCostProfile::PreferRecall)
            .portal_count(),
        1
    );
    assert_eq!(
        finder
            .find_route(&origin, &end, RouteCostProfile::AvoidInteractions)
            .portal_count(),
        0
    );
    assert_eq!(
        finder
            .find_route(&origin, &end, RouteCostProfile::FewestInteractions)
            .portal_count(),
        0
    );
}

// RouteFinder.InvalidateGraph: changing the data rebuilds the graph.
#[test]
fn changing_the_database_rebuilds_the_graph() {
    let mut finder = test_finder();
    let origin = at("Here", 0.0, 0.0);
    assert!(
        finder
            .find_route_to_name(&origin, "TownC")
            .description
            .is_empty()
    );
    assert!(finder.is_graph_built());
    finder
        .database_mut()
        .load_locations_csv(["Far;90;90"])
        .unwrap();
    assert!(!finder.is_graph_built());
    let route = finder.find_route_to_name(&origin, "Far");
    assert_eq!(route.description, "Direct walk (no graph route found)");
}

// A job run in small steps gives the blocking result.
#[test]
fn jobs_in_steps_equal_the_blocking_calls() {
    let mut finder = finder_of_xml(
        r#"<locations>
          <loc name="Portal A" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="99" />
          <loc name="Portal B" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="98" />
          <loc name="Portal C" type="WildernessPortal" NS="0" EW="3" exitNS="0" exitEW="97" />
          <loc name="End" type="Town" NS="0" EW="100" />
        </locations>"#,
    );
    let end = finder.database().find_location("End").unwrap();
    let origin = at("Player", 0.0, 0.0);
    let texts = |routes: &[Route]| -> Vec<Vec<String>> {
        routes
            .iter()
            .map(|r| r.steps().iter().map(ToString::to_string).collect())
            .collect()
    };
    let blocking = finder.find_route_alternatives(&origin, &end, RouteCostProfile::ShortestWalk, 8);
    let mut job = finder.alternatives_job(&origin, &end, RouteCostProfile::ShortestWalk, 8);
    let mut steps = 0;
    let stepped = loop {
        steps += 1;
        if let Some(routes) = job.step(1) {
            break routes;
        }
    };
    assert!(steps > 3);
    assert_eq!(texts(&stepped), texts(&blocking));

    let blocking = finder.find_route(&origin, &end, RouteCostProfile::ShortestWalk);
    let mut job = finder.route_job(&origin, &end, RouteCostProfile::ShortestWalk);
    let stepped = loop {
        if let Some(route) = job.step(1) {
            break route;
        }
    };
    assert_eq!(texts(&[stepped]), texts(&[blocking]));
}

// The graph is built a little per call; a change to the data starts the
// build again.
#[test]
fn the_graph_builds_over_several_calls() {
    let mut finder = test_finder();
    assert!(!finder.step_graph(1));
    assert!(!finder.is_graph_built());
    finder
        .database_mut()
        .load_locations_csv(["A;0;0", "B;1;0", "C;2;0"])
        .unwrap();
    let mut calls = 1;
    while !finder.step_graph(1) {
        calls += 1;
    }
    // From the start again: the nodes, three nodes' walks, the links.
    assert_eq!(calls, 5);
    assert!(finder.is_graph_built());
    assert_eq!(finder.graph().node_count(), 3);
}

// RouteFinder.HasGraphStep looks only at the edges leaving the step's
// start (and, for a walk, its end): the same answer as every edge.
#[test]
fn has_graph_step_agrees_with_scanning_every_edge() {
    let mut finder = finder_of_xml(
        "<l><loc id='11' name='Gate' type='WildernessPortal' NS='0' EW='1' exitNS='0' exitEW='40'/>\
         <loc id='12' name='Gate' type='WildernessPortal' NS='0' EW='3' exitNS='0' exitEW='60'/>\
         <loc id='1' name='Home' type='Town' NS='0' EW='0'/>\
         <loc id='2' name='Mill' type='Village' NS='1' EW='1'/>\
         <loc id='4' name='Far' type='Town' NS='0' EW='62'/></l>",
    );
    finder
        .database_mut()
        .load_route_starts_csv(["Far;Home;Recall", "Far;Mill;Walk"]);
    let graph = finder.graph();
    let names: Vec<(String, i32)> = (0..graph.node_count())
        .map(|i| (graph.location(i).name.clone(), graph.location(i).id))
        .chain([("Nowhere".to_owned(), 0), ("".to_owned(), 99)])
        .collect();
    let mut checked = 0;
    for kind in ["Travel", "PORTAL", "recall", "Walk"] {
        for (from, from_id) in &names {
            for (to, to_id) in &names {
                for (from_id, to_id) in [(0, 0), (*from_id, *to_id), (*from_id, 0)] {
                    let step = BlockedStep {
                        kind: kind.to_owned(),
                        from: from.to_uppercase(),
                        from_id,
                        to: to.clone(),
                        to_id,
                        note: String::new(),
                    };
                    let every_edge = (0..graph.node_count()).any(|node| {
                        graph.edges(node).any(|e| {
                            step.blocks(graph.location(e.from), graph.location(e.to), e.kind)
                        })
                    });
                    assert_eq!(finder.has_graph_step(&step), every_edge, "{step:?}");
                    checked += usize::from(every_edge);
                }
            }
        }
    }
    assert!(checked > 20, "{checked}");
}
