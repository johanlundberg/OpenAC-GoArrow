//! The route graph and its searches, from OpenAC-GoArrow's
//! `RouteGraphTests`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use goarrow::corrections::BlockedStep;
use goarrow::database::LocationDatabase;
use goarrow::finder::RouteFinder;
use goarrow::graph::{
    CostSelector, EdgeView, GraphBuild, NodeRef, PathSearch, RouteEdgeKind, RouteGraph,
    SearchTarget,
};
use goarrow::location::Location;
use goarrow::route::RouteStepKind;

fn graph_of(lines: &[&str]) -> (LocationDatabase, RouteGraph) {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(lines).unwrap();
    let graph = RouteGraph::build(&db, 10.0);
    (db, graph)
}

fn node(graph: &RouteGraph, name: &str) -> usize {
    graph
        .node_index(name)
        .unwrap_or_else(|| panic!("no node {name}"))
}

fn graph_with_portal_and_starts() -> RouteGraph {
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
    RouteGraph::build(&db, 10.0)
}

// RouteGraphTests.PortalHubWithArrivalCoordinatesCreatesPortalRoute
#[test]
fn portal_hub_with_arrival_coordinates_creates_portal_route() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(
        r#"<locations>
          <loc name="Town Network Portal(Sanamar)" type="PortalHub" NS="72" EW="-61.3" exitNS="-96.6" exitEW="-101.6" />
          <loc name="Town Network to Destination" type="PortalHub" NS="-96.6" EW="-101.6" exitNS="-65.4" exitEW="-44" />
          <loc name="Destination" type="Vendor" NS="-65.4" EW="-44" />
        </locations>"#,
    )
    .unwrap();
    let mut finder = RouteFinder::new(db);
    let route = finder.find_route_to_name(
        &Rc::new(Location::new("Player", 73.6, -59.2)),
        "Destination",
    );
    assert_eq!(route.portal_count(), 2);
    assert_eq!(route.steps()[0].to.name, "Town Network Portal(Sanamar)");
    assert_eq!(route.steps()[1].kind, RouteStepKind::Portal);
}

// RouteGraphTests.DungeonEntranceCannotBridgeOutdoorWalksButRemainsADestination
#[test]
fn dungeon_entrance_cannot_bridge_outdoor_walks_but_remains_a_destination() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(
        r#"<locations>
          <loc name="Start" type="Town" NS="0" EW="0" />
          <loc name="Humid Hovel" type="Dungeon" NS="0" EW="9" />
          <loc name="End" type="Town" NS="0" EW="18" />
        </locations>"#,
    )
    .unwrap();
    let graph = RouteGraph::build(&db, 10.0);
    assert!(graph.find_shortest_path_by_name("Start", "End").is_none());
    assert!(
        graph
            .find_shortest_path_by_name("Start", "Humid Hovel")
            .is_some()
    );
    let player = Location::new("Player", 0.0, 0.0);
    let base = CostSelector::Base;
    assert!(
        graph
            .find_shortest_path_from_position(&player, node(&graph, "End"), 10.0, &base, None)
            .is_none()
    );
    assert!(
        graph
            .find_shortest_path_from_position(
                &player,
                node(&graph, "Humid Hovel"),
                10.0,
                &base,
                None
            )
            .is_some()
    );
    // A search that starts on the dungeon node itself may walk out of it.
    assert!(
        graph
            .find_shortest_path_by_name("Humid Hovel", "End")
            .is_some()
    );
}

// RouteGraphTests.Build_CreatesNodesForEligibleLocations
#[test]
fn build_creates_nodes_for_eligible_locations() {
    let (_, graph) = graph_of(&["Alpha;0;0", "Beta;10;5", "Gamma;20;10"]);
    assert_eq!(graph.node_count(), 3);
    for name in ["Alpha", "Beta", "Gamma"] {
        assert!(graph.node_index(name).is_some());
    }
}

// RouteGraphTests.Build_ExcludesRetiredLocations (the C# test marks the
// loaded record retired; here the file says so).
#[test]
fn build_excludes_retired_locations() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(
        "<l><loc name='Active' NS='0' EW='0'/><loc name='Retired' NS='10' EW='10' retired='true' use='false'/>\
         <loc name='Unused' NS='1' EW='1' use='false'/></l>",
    )
    .unwrap();
    let graph = RouteGraph::build(&db, 10.0);
    assert_eq!(graph.node_count(), 1);
    assert!(graph.node_index("Active").is_some());
    assert!(graph.node_index("Retired").is_none());
}

// RouteGraphTests.Build_ExcludesLocationsWithoutCoordinates
#[test]
fn build_excludes_locations_without_coordinates() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml("<Locations><Location name='NoCoords'/></Locations>")
        .unwrap();
    db.load_locations_csv(["HasCoords;10;20"]).unwrap();
    let graph = RouteGraph::build(&db, 10.0);
    assert_eq!(graph.node_count(), 1);
    assert!(graph.node_index("NoCoords").is_none());
}

// RouteGraphTests.Build_CreatesWalkEdgesBetweenNearbyLocations
#[test]
fn build_creates_walk_edges_between_nearby_locations() {
    let (_, graph) = graph_of(&["Alpha;0;0", "Beta;5;0", "Gamma;15;0"]);
    let (alpha, beta, gamma) = (
        node(&graph, "Alpha"),
        node(&graph, "Beta"),
        node(&graph, "Gamma"),
    );
    assert!(
        graph
            .edges(alpha)
            .any(|e| e.to == beta && e.kind == RouteEdgeKind::Walk)
    );
    assert!(
        graph
            .edges(beta)
            .any(|e| e.to == alpha && e.kind == RouteEdgeKind::Walk)
    );
    assert!(!graph.edges(alpha).any(|e| e.to == gamma));
}

// RouteGraphTests.Build_IncludesExplicitPortalEdges
#[test]
fn build_includes_explicit_portal_edges() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Home;0;0", "FarTown;50;0"]).unwrap();
    db.load_portal_devices_csv(["FarTown;Far Portal;Dereth;Home;FarTown"]);
    let graph = RouteGraph::build(&db, 10.0);
    let path = graph
        .find_shortest_path_by_name("Home", "FarTown")
        .expect("path");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].kind, RouteEdgeKind::Portal);
    assert_eq!(&*path[0].via, "Far Portal");
}

// RouteGraph.Build: a device without an entrance makes no edge.
#[test]
fn a_device_without_entrance_makes_no_edge() {
    let graph = graph_with_portal_and_starts();
    let portal_dest = node(&graph, "PortalDest");
    for n in 0..graph.node_count() {
        assert!(
            !graph
                .edges(n)
                .any(|e| e.to == portal_dest && e.kind == RouteEdgeKind::Portal)
        );
    }
}

// RouteGraphTests.Build_IncludesRouteStartEdges
#[test]
fn build_includes_route_start_edges() {
    let graph = graph_with_portal_and_starts();
    let (a, b) = (node(&graph, "TownA"), node(&graph, "TownB"));
    assert!(
        graph
            .edges(a)
            .any(|e| e.to == b && e.kind == RouteEdgeKind::Walk)
    );
}

// RouteGraphTests.Build_WalkEdgesAreUndirected
#[test]
fn build_walk_edges_are_undirected() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0"]);
    let (a, b) = (node(&graph, "A"), node(&graph, "B"));
    assert!(graph.edges(a).any(|e| e.to == b));
    assert!(graph.edges(b).any(|e| e.to == a));
}

// RouteGraphTests.Build_DeduplicatesByName
#[test]
fn build_deduplicates_by_name() {
    let (_, graph) = graph_of(&["Town;0;0", "Town;1;1", "Town;2;2"]);
    assert_eq!(graph.node_count(), 1);
    let idx = node(&graph, "Town");
    assert_eq!(graph.location(idx).coords.ns, 0.0);
}

// RouteGraph.Build: Atlas portals that share a name stay separate, each
// with its own arrival node.
#[test]
fn atlas_portals_sharing_a_name_stay_separate() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(
        "<l><loc id='2' name='Shared' type='WildernessPortal' NS='0' EW='20' exitNS='0' exitEW='50'/>\
         <loc id='1' name='Shared' type='WildernessPortal' NS='0' EW='2' exitNS='0' exitEW='95'/></l>",
    )
    .unwrap();
    let graph = RouteGraph::build(&db, 10.0);
    assert_eq!(graph.node_count(), 4);
    // Sorted by name, then id.
    assert_eq!(graph.location(0).id, 1);
    assert_eq!(graph.location(2).name, "Shared arrival (1)");
    assert_eq!(graph.location(3).name, "Shared arrival (2)");
    assert!(graph.node_index("Shared arrival (1)").is_none());
    let second = db.find_location("Shared").unwrap();
    assert_eq!(second.id, 2);
    assert_eq!(graph.node_index_of(&second), Some(1));
    assert_eq!(
        graph.node_index_of(&Location::new("Shared", 0.0, 0.0)),
        Some(0)
    );
    let portal = graph
        .edges(1)
        .find(|e| e.kind == RouteEdgeKind::Portal)
        .unwrap();
    assert_eq!((portal.to, portal.cost, portal.via), (3, 0.1, "Shared"));
}

// RouteGraphTests.FindShortestPath_DirectWalk_ReturnsSingleEdge
#[test]
fn direct_walk_is_one_edge() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0"]);
    let path = graph.find_shortest_path_by_name("A", "B").unwrap();
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].kind, RouteEdgeKind::Walk);
}

// RouteGraphTests.FindShortestPath_MultiHop_ReturnsFullPath
#[test]
fn multi_hop_path() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0", "C;11;0"]);
    let path = graph.find_shortest_path_by_name("A", "C").unwrap();
    assert_eq!(path.len(), 2);
    assert!(path.iter().all(|e| e.kind == RouteEdgeKind::Walk));
    assert_eq!(path[0].to, NodeRef::Node(node(&graph, "B")));
    assert_eq!(path[1].to, NodeRef::Node(node(&graph, "C")));
}

// RouteGraphTests.FindShortestPath_SameStartAndEnd_ReturnsEmptyPath
#[test]
fn same_start_and_end_is_empty() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0"]);
    assert_eq!(graph.find_shortest_path_by_name("A", "A"), Some(Vec::new()));
}

// RouteGraphTests.FindShortestPath_UnknownNode_ReturnsNull
#[test]
fn unknown_node_is_none() {
    let (_, graph) = graph_of(&["A;0;0"]);
    assert!(graph.find_shortest_path_by_name("A", "Unknown").is_none());
    assert!(graph.find_shortest_path_by_name("Unknown", "A").is_none());
}

// RouteGraphTests.FindShortestPath_Unreachable_ReturnsNull
#[test]
fn unreachable_is_none() {
    let (_, graph) = graph_of(&["A;0;0", "B;50;0"]);
    assert!(graph.find_shortest_path_by_name("A", "B").is_none());
}

// RouteGraphTests.FindShortestPath_UsesRouteStartEdges
#[test]
fn uses_route_start_edges() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Home;0;0", "Town;50;0"]).unwrap();
    db.load_route_starts_csv(["Town;Home;Portal Recall"]);
    let graph = RouteGraph::build(&db, 10.0);
    let path = graph.find_shortest_path_by_name("Home", "Town").unwrap();
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].kind, RouteEdgeKind::Recall);
    assert_eq!(&*path[0].via, "Portal Recall");
    assert_eq!(path[0].cost, 0.05);
}

// RouteGraph.InferEdgeKind and the route-start costs.
#[test]
fn route_start_kinds_come_from_via() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Home;0;0", "A;50;0", "B;60;0", "C;70;0", "D;80;0", "E;90;0"])
        .unwrap();
    db.load_route_starts_csv([
        "A;Home;Lifestone Recall",
        "B;Home;Life Stone",
        "C;Home;Portal Device",
        "D;Home;Gem device",
        "E;Home;Run",
    ]);
    let graph = RouteGraph::build(&db, 10.0);
    let home = node(&graph, "Home");
    let kinds: Vec<(RouteEdgeKind, f64)> = graph.edges(home).map(|e| (e.kind, e.cost)).collect();
    assert_eq!(
        kinds,
        [
            (RouteEdgeKind::Recall, 0.05),
            (RouteEdgeKind::Lifestone, 0.05),
            (RouteEdgeKind::Portal, 0.1),
            (RouteEdgeKind::Portal, 0.1),
            (RouteEdgeKind::Walk, 90.0),
        ]
    );
}

// RouteGraphTests.FindShortestPath_PrefersShorterRoute
#[test]
fn prefers_shorter_route() {
    let (_, graph) = graph_of(&["A;0;0", "B;3;0", "C;1;5"]);
    let path = graph.find_shortest_path_by_name("A", "B").unwrap();
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].to, NodeRef::Node(node(&graph, "B")));
}

// RouteGraphTests.FindShortestPath_CustomCostSelectorUsesDeterministicWeightedPath
#[test]
fn custom_cost_selector() {
    let (_, graph) = graph_of(&["A;0;0", "B;2;0", "C;1;1", "D;3;0"]);
    let selector = CostSelector::Custom(Rc::new(|e: &EdgeView<'_>| {
        if e.kind == RouteEdgeKind::Walk && e.via.to_lowercase().contains("walk") {
            e.cost
        } else {
            e.cost * 100.0
        }
    }));
    let path = graph
        .find_shortest_path(node(&graph, "A"), node(&graph, "D"), &selector)
        .unwrap();
    assert_eq!(path.last().unwrap().to, NodeRef::Node(node(&graph, "D")));
}

// RouteGraphTests.FindShortestPath_MultiHopWithRouteStarts
#[test]
fn multi_hop_with_route_starts() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Start;0;0", "Middle;5;0", "Far;50;0"])
        .unwrap();
    db.load_route_starts_csv(["Far;Middle;Recall"]);
    let graph = RouteGraph::build(&db, 10.0);
    let path = graph.find_shortest_path_by_name("Start", "Far").unwrap();
    assert_eq!(path.len(), 2);
    assert_eq!(path[0].kind, RouteEdgeKind::Walk);
    assert_eq!(path[1].kind, RouteEdgeKind::Recall);
}

// RouteGraphTests.ToRoute_ConvertsWalkPath
#[test]
fn to_route_walk() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0"]);
    let path = graph.find_shortest_path_by_name("A", "B").unwrap();
    let route = graph.to_route(&path, "B");
    assert_eq!(route.destination, "B");
    assert_eq!(route.step_count(), 1);
    assert_eq!(route.steps()[0].kind, RouteStepKind::Travel);
    assert_eq!(route.steps()[0].via, "Walk");
}

// RouteGraphTests.ToRoute_ConvertsMultiHopPath
#[test]
fn to_route_multi_hop() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0", "C;11;0"]);
    let path = graph.find_shortest_path_by_name("A", "C").unwrap();
    let route = graph.to_route(&path, "C");
    assert_eq!(route.step_count(), 2);
    assert!(Rc::ptr_eq(&route.steps()[0].to, &route.steps()[1].from));
}

// RouteGraphTests.ToRoute_ConvertsRouteStartEdges
#[test]
fn to_route_route_start() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Home;0;0", "Town;50;0"]).unwrap();
    db.load_route_starts_csv(["Town;Home;Portal Recall"]);
    let graph = RouteGraph::build(&db, 10.0);
    let path = graph.find_shortest_path_by_name("Home", "Town").unwrap();
    let route = graph.to_route(&path, "Town");
    assert_eq!(route.step_count(), 1);
    assert_eq!(route.steps()[0].kind, RouteStepKind::Recall);
    assert_eq!(route.steps()[0].via, "Portal Recall");
}

// RouteGraphTests.ToRoute_EmptyPath_ReturnsSingleStepRoute
#[test]
fn to_route_empty_path() {
    let (_, graph) = graph_of(&["A;0;0"]);
    let path = graph.find_shortest_path_by_name("A", "A").unwrap();
    let route = graph.to_route(&path, "A");
    assert_eq!(route.destination, "A");
    assert_eq!(route.step_count(), 1);
    assert_eq!(route.steps()[0].via, "Arrived");
}

// RouteGraphTests.Build_EmptyDatabase_ProducesEmptyGraph
#[test]
fn empty_database_empty_graph() {
    assert_eq!(
        RouteGraph::build(&LocationDatabase::new(), 10.0).node_count(),
        0
    );
}

// RouteGraphTests.Build_NoEdges_SingleNodeHasNoConnections
#[test]
fn single_node_has_no_edges() {
    let (_, graph) = graph_of(&["Alone;0;0"]);
    assert_eq!(graph.edges(node(&graph, "Alone")).count(), 0);
}

// RouteGraphTests.FindShortestPath_NotBuilt_Throws and
// GetNodeIndex_NotBuilt_DoesNotThrow: a graph is built when it exists, so
// "not built" is the empty graph, which has no node and no path.
#[test]
fn empty_graph_has_no_node_or_path() {
    let graph = RouteGraph::default();
    assert_eq!(graph.node_index("Anything"), None);
    assert_eq!(graph.find_shortest_path(0, 0, &CostSelector::Base), None);
}

// RouteGraphTests.Build_RespectsCustomWalkDistance
#[test]
fn custom_walk_distance() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["A;0;0", "B;20;0", "C;30;0"])
        .unwrap();
    let graph = RouteGraph::build(&db, 25.0);
    let (a, b, c) = (node(&graph, "A"), node(&graph, "B"), node(&graph, "C"));
    assert!(graph.edges(a).any(|e| e.to == b));
    assert!(graph.edges(b).any(|e| e.to == c));
    assert!(!graph.edges(a).any(|e| e.to == c));
}

// RouteGraph.Build compares every pair; the grid must give the same walk
// edges in the same order (each node's walks by target index).
#[test]
fn grid_walk_edges_equal_every_pair() {
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut lines = Vec::new();
    for i in 0..600 {
        // Clusters and exact grid-boundary points.
        let (ns, ew) = if i % 10 == 0 {
            (f64::from(i / 10) * 10.0 - 50.0, 20.0)
        } else {
            (next() * 160.0 - 80.0, next() * 160.0 - 80.0)
        };
        lines.push(format!("P{i};{ns};{ew}"));
    }
    let mut db = LocationDatabase::new();
    db.load_locations_csv(&lines).unwrap();
    for reach in [10.0, 3.5, 0.0, f64::INFINITY] {
        let graph = RouteGraph::build(&db, reach);
        let n = graph.node_count();
        for i in 0..n {
            let walks: Vec<(usize, f64)> = graph
                .edges(i)
                .filter(|e| e.kind == RouteEdgeKind::Walk)
                .map(|e| (e.to, e.cost))
                .collect();
            let mut expected = Vec::new();
            for j in 0..n {
                if i == j {
                    continue;
                }
                let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                let d = graph.location(lo).distance_to(graph.location(hi));
                if d <= reach {
                    expected.push((j, d));
                }
            }
            assert_eq!(walks, expected, "node {i} reach {reach}");
        }
    }
}

// A search run in small steps gives the run-to-end result.
#[test]
fn stepped_search_equals_run() {
    let (_, graph) = graph_of(&["A;0;0", "B;5;0", "C;11;0", "D;16;0", "E;21;3"]);
    let origin = Location::new("Here", 0.1, 0.1);
    let target = SearchTarget::Node(node(&graph, "E"));
    let whole = PathSearch::from_position(
        &graph,
        &origin,
        target,
        10.0,
        CostSelector::Base,
        HashSet::new(),
    )
    .run();
    let mut stepped = PathSearch::from_position(
        &graph,
        &origin,
        target,
        10.0,
        CostSelector::Base,
        HashSet::new(),
    );
    let mut steps = 0;
    let result = loop {
        steps += 1;
        if let Some(result) = stepped.step(1) {
            break result;
        }
    };
    assert!(steps > 2);
    assert_eq!(result, whole);
    assert_eq!(whole.unwrap().len(), 4);
}

/// Portals (two sharing a name), a device, route starts, and walks among
/// them, with corrections that block some of each kind.
fn blocked_db(blocked: bool) -> LocationDatabase {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(
        "<l><loc id='11' name='Gate' type='WildernessPortal' NS='0' EW='1' exitNS='0' exitEW='40'/>\
         <loc id='12' name='Gate' type='WildernessPortal' NS='0' EW='3' exitNS='0' exitEW='60'/>\
         <loc id='13' name='Ferry' type='TownPortal' NS='2' EW='0' exitNS='0' exitEW='61'/>\
         <loc id='1' name='Home' type='Town' NS='0' EW='0'/>\
         <loc id='2' name='Mill' type='Village' NS='1' EW='1'/>\
         <loc id='3' name='Shrine' type='Landmark' NS='-1' EW='2'/>\
         <loc id='4' name='Far' type='Town' NS='0' EW='62'/>\
         <loc id='5' name='Dock' type='Outpost' NS='0' EW='41'/></l>",
    )
    .unwrap();
    db.load_portal_devices_csv(["Far;Ferry Stone;Dereth;Mill;Far"]);
    db.load_route_starts_csv(["Dock;Home;Recall", "Far;Shrine;Lifestone Recall"]);
    if blocked {
        let step = |kind: &str, from: &str, from_id: i32, to: &str, to_id: i32| BlockedStep {
            kind: kind.to_owned(),
            from: from.to_owned(),
            from_id,
            to: to.to_owned(),
            to_id,
            note: String::new(),
        };
        db.set_corrections(
            HashMap::new(),
            vec![
                // A walk by names in another case: both ways.
                step("travel", "MILL", 0, "home", 0),
                // One of the two Gates, by id, to its arrival by name.
                step("Portal", "", 12, "Gate arrival (12)", 0),
                // The device and a recall route start.
                step("Portal", "Mill", 0, "Far", 0),
                step("Recall", "Shrine", 0, "Far", 0),
                // A kind that names no edge kind blocks nothing.
                step("Walk", "Home", 0, "Shrine", 0),
                // An end that names no node.
                step("Travel", "Nowhere", 0, "Home", 0),
            ],
        );
    }
    db
}

type EdgeRow = (usize, usize, RouteEdgeKind, u64, String);

fn edge_rows(graph: &RouteGraph) -> Vec<Vec<EdgeRow>> {
    (0..graph.node_count())
        .map(|node| {
            graph
                .edges(node)
                .map(|e| (e.from, e.to, e.kind, e.cost.to_bits(), e.via.to_owned()))
                .collect()
        })
        .collect()
}

// RouteGraph.Build skips every edge some blocked step's Blocks names; the
// blocked pairs are looked up, with the same answers as asking each step.
#[test]
fn blocked_steps_leave_out_exactly_the_edges_they_block() {
    let open = RouteGraph::build(&blocked_db(false), 10.0);
    let db = blocked_db(true);
    let steps: Vec<BlockedStep> = db.blocked_steps().cloned().collect();
    let graph = RouteGraph::build(&db, 10.0);
    assert_eq!(graph.node_count(), open.node_count());
    let expected: Vec<Vec<EdgeRow>> = edge_rows(&open)
        .into_iter()
        .map(|edges| {
            edges
                .into_iter()
                .filter(|(from, to, kind, ..)| {
                    !steps
                        .iter()
                        .any(|s| s.blocks(open.location(*from), open.location(*to), *kind))
                })
                .collect()
        })
        .collect();
    assert_eq!(edge_rows(&graph), expected);
    let removed = open.edge_count() - graph.edge_count();
    // Mill-Home both ways, one Gate, the device, the recall start.
    assert_eq!(removed, 5);
}

// A graph built a unit at a time is the graph built in one call.
#[test]
fn the_graph_built_in_steps_equals_the_one_call_build() {
    let db = blocked_db(true);
    let whole = RouteGraph::build(&db, 10.0);
    let mut build = GraphBuild::new(10.0);
    let mut steps = 0;
    let stepped = loop {
        steps += 1;
        let mut budget = 1;
        if let Some(graph) = build.step(&db, &mut budget) {
            break graph;
        }
    };
    // The nodes, each node's walks, the links.
    assert_eq!(steps, whole.node_count() + 2);
    assert_eq!(edge_rows(&stepped), edge_rows(&whole));
    let names = |g: &RouteGraph| -> Vec<String> {
        (0..g.node_count())
            .map(|i| g.location(i).name.clone())
            .collect()
    };
    assert_eq!(names(&stepped), names(&whole));
}
