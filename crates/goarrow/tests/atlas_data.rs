//! Real Atlas data: the export GoArrow's README points at (about 5,400
//! places, a thousand portals with arrivals). It is someone else's
//! copyrighted data, so it is not in the repository; these tests are
//! ignored unless `GOARROW_ATLAS_XML` names the file:
//!
//! ```powershell
//! $env:GOARROW_ATLAS_XML = 'C:\path\to\data_cod.xml'
//! cargo test -p goarrow --release --test atlas_data -- --ignored --nocapture
//! ```
//!
//! They print the time each part takes; the plugin runs on the game's
//! session thread, so each must stay short or run in steps.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use goarrow::corrections::BlockedStep;
use goarrow::database::{LocationDatabase, parse_locations_xml, validate_atlas_xml};
use goarrow::finder::{RouteCostProfile, RouteFinder};
use goarrow::graph::{GraphBuild, RouteEdgeKind, RouteGraph};
use goarrow::loading::{AtlasJob, LocationsJob};
use goarrow::location::{Location, LocationType};
use goarrow::route::RouteStepKind;

fn atlas_xml() -> String {
    let path =
        std::env::var("GOARROW_ATLAS_XML").expect("set GOARROW_ATLAS_XML to the Atlas export");
    std::fs::read_to_string(path).expect("read the Atlas export")
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn loaded_finder(xml: &str) -> RouteFinder {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(xml).unwrap();
    RouteFinder::new(db)
}

#[test]
#[ignore = "needs GOARROW_ATLAS_XML"]
fn atlas_export_parses_validates_and_routes() {
    let xml = atlas_xml();

    let start = Instant::now();
    let count = validate_atlas_xml(&xml).unwrap();
    println!("validate: {:.1} ms ({count} records)", ms(start));

    let start = Instant::now();
    let parsed = parse_locations_xml(&xml).unwrap();
    println!(
        "parse: {:.1} ms ({} with coordinates)",
        ms(start),
        parsed.locations.len()
    );
    assert!(parsed.is_atlas);
    assert!(parsed.locations.len() > 5000);

    let mut db = LocationDatabase::new();
    db.replace_locations(parsed);
    let start = Instant::now();
    let graph = RouteGraph::build(&db, RouteFinder::DEFAULT_WALK_DISTANCE);
    println!(
        "graph build: {:.1} ms ({} nodes, {} edges)",
        ms(start),
        graph.node_count(),
        graph.edge_count()
    );
    drop(graph);

    let mut finder = RouteFinder::new(db);
    finder.graph();
    let holtburg = finder.database().find_location("Holtburg").unwrap();
    let sanamar = finder.database().find_location("Sanamar").unwrap();
    let origin = Rc::new(Location::at("Current Position", holtburg.coords));

    let start = Instant::now();
    let route = finder.find_route(&origin, &sanamar, RouteCostProfile::ShortestWalk);
    println!("find_route Holtburg -> Sanamar: {:.1} ms", ms(start));
    for step in route.steps() {
        println!("  {step}");
    }
    assert!(route.description.is_empty(), "{}", route.description);
    assert!(route.portal_count() > 0);

    let start = Instant::now();
    let routes =
        finder.find_route_alternatives(&origin, &sanamar, RouteCostProfile::ShortestWalk, 8);
    println!(
        "8 alternatives: {:.1} ms ({} found)",
        ms(start),
        routes.len()
    );
    assert!(!routes.is_empty() && routes.len() <= 8);
    for route in &routes {
        println!(
            "  {} / {}",
            route,
            route
                .steps()
                .iter()
                .filter(|s| s.kind != RouteStepKind::Travel)
                .count()
        );
    }

    let yaraq = finder.database().find_location("Yaraq").unwrap();
    let kryst = finder.database().find_location("Kryst").unwrap();
    let origin = Rc::new(Location::at("Current Position", yaraq.coords));
    let start = Instant::now();
    let route = finder.find_route(&origin, &kryst, RouteCostProfile::ShortestWalk);
    println!("find_route Yaraq -> Kryst: {:.1} ms ({route})", ms(start));

    // The worst case: a point no node is near, whose straight walk is long,
    // so the search settles nearly every node first.
    let far = Rc::new(Location::new("Far", -100.0, -100.0));
    let start = Instant::now();
    let route = finder.find_route(&origin, &far, RouteCostProfile::ShortestWalk);
    println!(
        "find_route Yaraq -> 100S 100W: {:.1} ms ({route})",
        ms(start)
    );
    let start = Instant::now();
    let routes = finder.find_route_alternatives(&origin, &far, RouteCostProfile::ShortestWalk, 8);
    println!(
        "8 alternatives Yaraq -> 100S 100W: {:.1} ms ({} found)",
        ms(start),
        routes.len()
    );
}

#[test]
#[ignore = "needs GOARROW_ATLAS_XML"]
fn atlas_job_in_steps_equals_validate_and_parse() {
    let xml = atlas_xml();
    let count = validate_atlas_xml(&xml).unwrap();
    let whole = parse_locations_xml(&xml).unwrap();
    let start = Instant::now();
    let mut job = AtlasJob::new(xml.clone());
    let mut steps = 0;
    let mut longest = 0.0f64;
    let (parsed, job_count) = loop {
        steps += 1;
        let step_start = Instant::now();
        let done = job.step(500);
        longest = longest.max(ms(step_start));
        if let Some(result) = done {
            break result.unwrap();
        }
    };
    println!(
        "atlas job (check + read): {:.1} ms in {steps} steps of 500 records, longest step {longest:.1} ms",
        ms(start)
    );
    assert_eq!(job_count, count);
    assert_eq!(parsed.locations.len(), whole.locations.len());
    for (a, b) in parsed.locations.iter().zip(&whole.locations) {
        assert_eq!(a.to_xml(), b.to_xml());
    }
    let start = Instant::now();
    let locations = LocationsJob::new(xml).run().unwrap();
    println!("locations job, run to the end: {:.1} ms", ms(start));
    assert_eq!(locations.locations.len(), whole.locations.len());
}

#[test]
#[ignore = "needs GOARROW_ATLAS_XML"]
fn atlas_jobs_in_steps_equal_the_blocking_calls() {
    let xml = atlas_xml();
    let mut finder = loaded_finder(&xml);
    let holtburg = finder.database().find_location("Holtburg").unwrap();
    let sanamar = finder.database().find_location("Sanamar").unwrap();
    let origin = Rc::new(Location::at("Current Position", holtburg.coords));
    let texts = |routes: &[goarrow::route::Route]| -> Vec<Vec<String>> {
        routes
            .iter()
            .map(|r| r.steps().iter().map(ToString::to_string).collect())
            .collect()
    };
    let blocking =
        finder.find_route_alternatives(&origin, &sanamar, RouteCostProfile::ShortestWalk, 8);
    let mut job = finder.alternatives_job(&origin, &sanamar, RouteCostProfile::ShortestWalk, 8);
    let mut steps = 0;
    let mut longest = 0.0f64;
    let stepped = loop {
        steps += 1;
        let start = Instant::now();
        let done = job.step(2000);
        longest = longest.max(ms(start));
        if let Some(routes) = done {
            break routes;
        }
    };
    println!("alternatives in {steps} steps of 2000 expansions, longest step {longest:.1} ms");
    assert_eq!(texts(&stepped), texts(&blocking));
}

/// Ten walks between towns, blocked by name (no ids): every graph pair is
/// checked against them while the graph is built.
fn ten_town_walks_blocked(db: &LocationDatabase) -> Vec<BlockedStep> {
    let graph = RouteGraph::build(db, RouteFinder::DEFAULT_WALK_DISTANCE);
    let is_town = |i: usize| graph.location(i).kind.intersects(LocationType::TOWN);
    let mut steps = Vec::new();
    for node in (0..graph.node_count()).filter(|&i| is_town(i)) {
        if let Some(edge) = graph
            .edges(node)
            .find(|e| e.kind == RouteEdgeKind::Walk && is_town(e.to))
        {
            steps.push(BlockedStep {
                kind: "Travel".to_owned(),
                from: graph.location(edge.from).name.clone(),
                from_id: 0,
                to: graph.location(edge.to).name.clone(),
                to_id: 0,
                note: String::new(),
            });
        }
        if steps.len() == 10 {
            break;
        }
    }
    steps
}

#[test]
#[ignore = "needs GOARROW_ATLAS_XML"]
fn atlas_graph_build_timings() {
    let xml = atlas_xml();
    let mut db = LocationDatabase::new();
    db.load_locations_xml(&xml).unwrap();
    let best = |db: &LocationDatabase| {
        (0..5)
            .map(|_| {
                let start = Instant::now();
                let graph = RouteGraph::build(db, RouteFinder::DEFAULT_WALK_DISTANCE);
                (ms(start), graph.node_count(), graph.edge_count())
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap()
    };
    let (plain, nodes, edges) = best(&db);
    println!("graph build: {plain:.1} ms ({nodes} nodes, {edges} edges)");
    let mut build = GraphBuild::new(RouteFinder::DEFAULT_WALK_DISTANCE);
    let (mut steps, mut longest) = (0, 0.0f64);
    loop {
        steps += 1;
        let start = Instant::now();
        let mut budget = goarrow::plugin::GRAPH_CHUNK;
        let done = build.step(&db, &mut budget).is_some();
        longest = longest.max(ms(start));
        if done {
            break;
        }
    }
    println!(
        "graph build in steps of {}: {steps} steps, longest {longest:.2} ms",
        goarrow::plugin::GRAPH_CHUNK
    );
    let blocked = ten_town_walks_blocked(&db);
    let mut finder = RouteFinder::new(db);
    finder.graph();
    let probe = |finder: &mut RouteFinder, expected: bool| {
        let start = Instant::now();
        let found = finder.has_graph_step(&blocked[0]);
        println!("has_graph_step: {:.2} ms ({found})", ms(start));
        assert_eq!(found, expected);
    };
    probe(&mut finder, true);
    finder
        .database_mut()
        .set_corrections(HashMap::new(), blocked.clone());
    let (with_blocks, _, edges) = best(finder.database());
    println!("graph build, 10 name-matched blocked walks: {with_blocks:.1} ms ({edges} edges)");
    finder.graph();
    probe(&mut finder, false);
}

#[test]
#[ignore = "needs GOARROW_ATLAS_XML"]
fn atlas_grid_walk_edges_equal_every_pair() {
    let xml = atlas_xml();
    let mut db = LocationDatabase::new();
    db.load_locations_xml(&xml).unwrap();
    let graph = RouteGraph::build(&db, RouteFinder::DEFAULT_WALK_DISTANCE);
    let n = graph.node_count();
    for i in 0..n {
        let walks: Vec<(usize, f64)> = graph
            .edges(i)
            .filter(|e| e.kind == RouteEdgeKind::Walk && e.via == "Walk")
            .map(|e| (e.to, e.cost))
            .take_while(|_| true)
            .collect();
        let mut expected = Vec::new();
        for j in 0..n {
            if i == j {
                continue;
            }
            let (lo, hi) = if i < j { (i, j) } else { (j, i) };
            let d = graph.location(lo).distance_to(graph.location(hi));
            if d <= RouteFinder::DEFAULT_WALK_DISTANCE {
                expected.push((j, d));
            }
        }
        // Route-start walks (none in the Atlas) would follow the pairs.
        assert_eq!(walks, expected, "node {i}");
    }
}

/// The Atlas portals' arrivals the shipped portal table gives: how many it
/// has, how many leading indoors, and how far its arrivals move the Atlas
/// listings.
#[test]
#[ignore = "needs GOARROW_ATLAS_XML"]
fn atlas_portal_arrivals_from_the_portal_table() {
    use goarrow::ace_portals::{AcePortals, AtlasMatch};
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../portals/ace-portals.json"
    );
    let table =
        AcePortals::parse(&std::fs::read_to_string(path).expect("portals/ace-portals.json"))
            .expect("the shipped table");
    let xml = atlas_xml();
    let mut db = LocationDatabase::new();
    db.load_locations_xml(&xml).unwrap();
    let portals: Vec<Rc<Location>> = db
        .all_locations()
        .iter()
        .filter(|l| l.id > 0 && l.has_exit_coords() && l.kind.intersects(LocationType::ANY_PORTAL))
        .cloned()
        .collect();
    let (mut arrivals, mut indoors, mut ambiguous, mut none) = (0, 0, 0, 0);
    let mut moved = [0usize; 3];
    let mut far = Vec::new();
    for portal in &portals {
        match table.match_atlas_portal(portal) {
            AtlasMatch::Arrival(arrival) => {
                arrivals += 1;
                let d = arrival.distance_to(portal.exit_coords());
                for (i, limit) in [0.1, 1.0, 5.0].iter().enumerate() {
                    if d > *limit {
                        moved[i] += 1;
                    }
                }
                if d > 5.0 {
                    far.push(format!(
                        "{} {}: Atlas {} table {arrival} ({d:.1})",
                        portal.id,
                        portal.name,
                        portal.exit_coords()
                    ));
                }
            }
            AtlasMatch::Indoors => indoors += 1,
            AtlasMatch::Ambiguous => ambiguous += 1,
            AtlasMatch::None => none += 1,
        }
    }
    let start = Instant::now();
    let count = db.use_portal_table(&table);
    println!("use_portal_table: {:.1} ms", ms(start));
    println!(
        "{} Atlas portals with arrivals: {arrivals} take the table's arrival, {indoors} lead indoors, {ambiguous} ambiguous, {none} not in the table",
        portals.len()
    );
    println!(
        "moved more than 0.1 / 1 / 5 map units: {} / {} / {}",
        moved[0], moved[1], moved[2]
    );
    for line in &far {
        println!("  {line}");
    }
    for (prefix, _) in goarrow::ace_portals::HUBS {
        let hub: Vec<&Rc<Location>> = portals
            .iter()
            .filter(|p| p.name.starts_with(prefix))
            .collect();
        let taken = hub
            .iter()
            .filter(|p| matches!(table.match_atlas_portal(p), AtlasMatch::Arrival(_)))
            .count();
        println!("{prefix}...: {taken} of {}", hub.len());
    }
    assert_eq!(count, arrivals);
    // The Town Network's Eastham portal lets out at 16.92N 63.48E.
    let eastham = db.find_location("Town Network (N R 3) to Eastham").unwrap();
    let arrival = eastham.exit_coords();
    assert!(
        (arrival.ns - 16.919).abs() < 0.001 && (arrival.ew - 63.479).abs() < 0.001,
        "{arrival}"
    );
}
