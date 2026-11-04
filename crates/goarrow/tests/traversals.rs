//! Learned dungeon paths, from OpenAC-GoArrow's `DungeonTraversalStoreTests`
//! (each test names the C# test it ports), and the navigator's share
//! commands over them (`GoArrowPlugin.ShareDungeonTraversals`).

mod navsim;

use goarrow::dungeon_search::DungeonPortalSearch;
use goarrow::traversals::{TRAVERSALS_KEY, TraversalStore};
use navsim::{Harness, Sim, cell, dungeon_position, search_next};
use openac_plugin::Host;
use openac_plugin::navigation::NavigationSnapshot;
use openac_plugin::world::{ObjectClass, Position, WorldObject};

fn point(x: f64, y: f64, z: f64) -> Position {
    let base = dungeon_position(0.0);
    let mut p = dungeon_position(x as f32);
    p.north_south = base.north_south + y / 240.0;
    p.elevation = z / 240.0;
    p
}

fn snap(position: Position) -> NavigationSnapshot {
    NavigationSnapshot {
        is_available: true,
        local_object_id: 1,
        position,
        is_moving: true,
        ..NavigationSnapshot::default()
    }
}

fn exit_portal(name: &str) -> WorldObject {
    WorldObject {
        object_id: 44,
        name: name.to_owned(),
        object_class: ObjectClass::Portal,
        portal_destination: Some(name.to_owned()),
        ..WorldObject::default()
    }
}

fn remember(sim: &mut Sim, store: &mut TraversalStore, key: &str, name: &str, at: Position) {
    let mut host = Host::with_transport(sim);
    store.remember_exit(&mut host, key, &exit_portal(name), &snap(at));
}

// ShortestObservedRouteDropsLoopsAndKeepsMultipleExitsSeparateAfterReload
#[test]
fn shortest_observed_route_drops_loops_and_keeps_multiple_exits_separate_after_reload() {
    let mut sim = Sim::new();
    let mut store = TraversalStore::new();
    for p in [
        point(0.0, 0.0, 0.0),
        point(6.0, 0.0, 0.0),
        point(6.0, 6.0, 0.0),
        point(0.0, 6.0, 0.0),
        point(0.0, 0.0, 0.0),
        point(0.0, -6.0, 0.0),
    ] {
        store.observe(&snap(p));
    }
    remember(
        &mut sim,
        &mut store,
        "south",
        "South exit",
        point(0.0, -6.0, 0.0),
    );
    store.break_trace();
    store.observe(&snap(point(0.0, 0.0, 0.0)));
    store.observe(&snap(point(6.0, 0.0, 0.0)));
    remember(
        &mut sim,
        &mut store,
        "east",
        "East exit",
        point(6.0, 0.0, 0.0),
    );
    assert!(sim.storage.contains_key(TRAVERSALS_KEY));

    let mut reloaded = TraversalStore::new();
    reloaded.load(&mut Host::with_transport(&mut sim));
    assert_eq!(reloaded.error(), "");
    let south = reloaded.find_path(&point(0.0, 0.0, 0.0), "south");
    assert_eq!(south.len(), 2);
    assert_eq!(south.last(), Some(&point(0.0, -6.0, 0.0)));
    let east = reloaded.find_path(&point(0.0, 0.0, 0.0), "east");
    assert_eq!(east.len(), 2);
    assert_eq!(east.last(), Some(&point(6.0, 0.0, 0.0)));
    assert!(
        reloaded
            .find_path(&point(0.0, 0.0, 0.0), "unlearned")
            .is_empty()
    );
    assert!(
        reloaded
            .find_path(&point(0.0, 0.0, -12.0), "east")
            .is_empty()
    );
    let mut elsewhere = point(0.0, 0.0, 0.0);
    elsewhere.cell_id = 0x02AB_022F;
    assert!(reloaded.find_path(&elsewhere, "east").is_empty());
    // Directions are observed separately; a cliff drop is not a climb.
    assert!(
        reloaded
            .find_path(&point(0.0, -6.0, 0.0), "east")
            .is_empty()
    );
}

// SharingMergesRecordsIdempotentlyAndRejectsInvalidImportsWithoutMutation
#[test]
fn sharing_merges_records_idempotently_and_rejects_invalid_imports_without_mutation() {
    let mut sim = Sim::new();
    let mut source = TraversalStore::new();
    source.observe(&snap(point(0.0, 0.0, 0.0)));
    source.observe(&snap(point(6.0, 0.0, -6.0)));
    remember(
        &mut sim,
        &mut source,
        "downstairs",
        "Lower exit",
        point(6.0, 0.0, -6.0),
    );
    let mut target = TraversalStore::new();
    let mut host = Host::with_transport(&mut sim);
    target
        .import(&mut host, &source.export())
        .expect("a valid file");
    let once = target.export();
    target
        .import(&mut host, &source.export())
        .expect("a valid file");
    assert_eq!(target.export(), once);
    let path = target.find_path(&point(0.0, 0.0, 0.0), "downstairs");
    assert_eq!(path.last().map(|p| p.elevation), Some(-6.0 / 240.0));
    assert!(
        target
            .import(
                &mut host,
                r#"{"Version":1,"Points":[],"Edges":[{"From":0,"To":1}],"Exits":[]}"#
            )
            .is_err()
    );
    assert_eq!(target.export(), once);
    assert!(target.import(&mut host, r#"{"Version":2}"#).is_err());
    assert_eq!(target.export(), once);
}

// PortalSpaceAndLargeMovementGapsDoNotCreateWalkableConnections
#[test]
fn portal_space_and_large_movement_gaps_do_not_create_walkable_connections() {
    let mut sim = Sim::new();
    let mut store = TraversalStore::new();
    store.observe(&snap(point(0.0, 0.0, 0.0)));
    let mut in_portal_space = snap(point(0.0, 0.0, 0.0));
    in_portal_space.is_portal_space = true;
    store.observe(&in_portal_space);
    store.observe(&snap(point(6.0, 0.0, 0.0)));
    remember(
        &mut sim,
        &mut store,
        "teleport",
        "Exit",
        point(6.0, 0.0, 0.0),
    );
    assert!(
        store
            .find_path(&point(0.0, 0.0, 0.0), "teleport")
            .is_empty()
    );
    store.observe(&snap(point(40.0, 0.0, 0.0)));
    remember(&mut sim, &mut store, "gap", "Exit", point(40.0, 0.0, 0.0));
    assert!(store.find_path(&point(6.0, 0.0, 0.0), "gap").is_empty());
}

// SearchUsesRequestedRecordedExitAndFallsBackWhenItsPathFails
#[test]
fn search_uses_requested_recorded_exit_and_falls_back_when_its_path_fails() {
    let mut sim = Sim::new();
    let mut store = TraversalStore::new();
    store.observe(&snap(point(0.0, 0.0, 0.0)));
    store.observe(&snap(point(6.0, 0.0, 0.0)));
    remember(
        &mut sim,
        &mut store,
        "east",
        "East exit",
        point(6.0, 0.0, 0.0),
    );
    sim.snapshot = snap(point(0.0, 0.0, 0.0));
    sim.cells = vec![cell(0x02AA_0101, (20.0, 0.0, 0.0), 0.0)];
    let mut search = DungeonPortalSearch::new();
    search.use_recorded_exit("east");
    let saved = search_next(&mut sim, &mut search, &store).expect("the recorded point");
    assert_eq!(saved, point(6.0, 0.0, 0.0));
    assert!(
        search.reason().contains("Following recorded path"),
        "{}",
        search.reason()
    );
    search.target_accepted(saved.cell_id);
    search.complete_target(&point(0.0, 0.0, 0.0), false);
    let fallback = search_next(&mut sim, &mut search, &store).expect("an exploration target");
    assert_eq!(fallback.cell_id, 0x02AA_0101);
    assert!(!search.reason().contains("Following recorded path"));
}

// GoArrowPlugin.ShareDungeonTraversals: export and import by file name.
#[test]
fn learned_paths_are_exported_and_imported_by_file_name() {
    let mut h = Harness::empty();
    assert_eq!(
        h.host(|nav, host| nav.export_traversals(host, "../escape.json")),
        Err("Use a JSON filename in the plugin storage folder.".to_owned())
    );
    assert_eq!(
        h.host(|nav, host| nav.export_traversals(host, "share.txt")),
        Err("Use a JSON filename in the plugin storage folder.".to_owned())
    );
    assert_eq!(
        h.host(|nav, host| nav.export_traversals(host, "share.json")),
        Ok(
            "Exported dungeon traversals to 'share.json'. Storage folder: C:/plugin-data."
                .to_owned()
        )
    );
    assert!(h.sim.storage["share.json"].contains("\"Version\": 1"));
    assert_eq!(
        h.host(|nav, host| nav.import_traversals(host, "missing.json")),
        Err("Dungeon traversal import failed: File not found.".to_owned())
    );
    h.sim
        .storage
        .insert("bad.json".to_owned(), r#"{"Version":2}"#.to_owned());
    assert!(
        h.host(|nav, host| nav.import_traversals(host, "bad.json"))
            .is_err_and(|e| e.starts_with("Dungeon traversal import failed: "))
    );
    let mut source = TraversalStore::new();
    source.observe(&snap(point(0.0, 0.0, 0.0)));
    source.observe(&snap(point(6.0, 0.0, 0.0)));
    h.sim
        .storage
        .insert("good.json".to_owned(), source.export());
    assert_eq!(
        h.host(|nav, host| nav.import_traversals(host, "good.json")),
        Ok("Merged dungeon traversals from 'good.json'.".to_owned())
    );
    assert_eq!(h.nav.traversals().data().points.len(), 2);
    assert!(h.sim.storage.contains_key(TRAVERSALS_KEY));
}

// ---- limits ----

use goarrow::traversals::{MAX_EDGES, MAX_POINTS, TraversalData, TraversalEdge, TraversalPoint};

/// `count` points 2 m apart along one corridor of one cell.
fn corridor(count: usize) -> Vec<TraversalPoint> {
    (0..count)
        .map(|i| {
            let p = point(2.0 * i as f64, 0.0, 0.0);
            TraversalPoint {
                cell_id: p.cell_id,
                east_west: p.east_west,
                north_south: p.north_south,
                elevation: p.elevation,
            }
        })
        .collect()
}

fn saved(sim: &mut Sim, data: &TraversalData) {
    sim.storage.insert(
        TRAVERSALS_KEY.to_owned(),
        serde_json::to_string(data).expect("JSON"),
    );
}

fn loaded(sim: &mut Sim) -> TraversalStore {
    let mut store = TraversalStore::new();
    let mut host = Host::with_transport(sim);
    store.load(&mut host);
    store
}

fn warnings(sim: &Sim) -> Vec<String> {
    sim.fake
        .logs
        .iter()
        .map(|(_, line)| line.clone())
        .filter(|line| line.starts_with("GoArrow: "))
        .collect()
}

// A store at the most points a file may hold records no new point, so
// what it saves still loads.
#[test]
fn recording_stops_at_the_points_a_file_may_hold() {
    let mut sim = Sim::new();
    saved(
        &mut sim,
        &TraversalData {
            points: corridor(MAX_POINTS),
            ..TraversalData::default()
        },
    );
    let mut store = loaded(&mut sim);
    assert_eq!(store.error(), "");
    let mut far = point(0.0, 40.0, 0.0);
    far.cell_id += 1;
    store.observe(&snap(far.clone()));
    let mut further = far.clone();
    further.east_west += 6.0 / 240.0;
    store.observe(&snap(further));
    assert_eq!(store.data().points.len(), MAX_POINTS);
    let notices = store.take_notice();
    assert!(notices.is_some());
    assert_eq!(store.take_notice(), None);

    // A known point is still walked through.
    store.break_trace();
    store.observe(&snap(point(0.0, 0.0, 0.0)));
    store.observe(&snap(point(6.0, 0.0, 0.0)));
    assert_eq!(store.data().edges.len(), 1);

    let mut host = Host::with_transport(&mut sim);
    store.remember_exit(
        &mut host,
        "east",
        &WorldObject {
            object_id: 44,
            name: "East exit".to_owned(),
            portal_destination: Some("East exit".to_owned()),
            ..WorldObject::default()
        },
        &snap(point(6.0, 0.0, 0.0)),
    );
    let reloaded = loaded(&mut sim);
    assert_eq!(reloaded.error(), "");
    assert_eq!(reloaded.data().points.len(), MAX_POINTS);
}

// The same for the edges.
#[test]
fn recording_stops_at_the_edges_a_file_may_hold() {
    let count = 40_000;
    let mut edges: Vec<TraversalEdge> = Vec::new();
    for i in 0..count - 1 {
        edges.push(TraversalEdge { from: i, to: i + 1 });
        edges.push(TraversalEdge { from: i + 1, to: i });
    }
    let mut i = 0;
    while edges.len() < MAX_EDGES {
        edges.push(TraversalEdge { from: i, to: i + 2 });
        i += 1;
    }
    let mut sim = Sim::new();
    saved(
        &mut sim,
        &TraversalData {
            points: corridor(count),
            edges,
            ..TraversalData::default()
        },
    );
    let mut store = loaded(&mut sim);
    assert_eq!(store.error(), "");
    // From the last point back three points: an edge not known yet.
    let last = 2.0 * (count - 1) as f64;
    store.observe(&snap(point(last, 0.0, 0.0)));
    store.observe(&snap(point(last - 6.0, 0.0, 0.0)));
    // And a new point beside the corridor.
    store.observe(&snap(point(last - 6.0, 5.0, 0.0)));
    assert_eq!(store.data().edges.len(), MAX_EDGES);
    assert_eq!(store.data().points.len(), count);
    assert!(store.take_notice().is_some());
}

// A file that does not load is kept: nothing learned afterwards is saved
// over it.
#[test]
fn a_file_that_does_not_load_is_never_overwritten() {
    let mut sim = Sim::new();
    let original = r#"{"Version":2,"Points":[]}"#;
    sim.storage
        .insert(TRAVERSALS_KEY.to_owned(), original.to_owned());
    let mut store = loaded(&mut sim);
    assert!(
        store
            .error()
            .starts_with("Dungeon traversal data could not be loaded")
    );
    assert!(
        warnings(&sim)
            .iter()
            .any(|w| w.contains("could not be loaded")),
        "{:?}",
        warnings(&sim)
    );
    store.observe(&snap(point(0.0, 0.0, 0.0)));
    store.observe(&snap(point(6.0, 0.0, 0.0)));
    let mut host = Host::with_transport(&mut sim);
    assert!(!store.save(&mut host));
    assert_eq!(sim.storage[TRAVERSALS_KEY], original);
}

// An import that would take the store past the limits is refused and
// changes nothing.
#[test]
fn an_import_past_the_limits_is_refused() {
    let mut sim = Sim::new();
    saved(
        &mut sim,
        &TraversalData {
            points: corridor(MAX_POINTS),
            ..TraversalData::default()
        },
    );
    let mut store = loaded(&mut sim);
    let mut source = TraversalStore::new();
    let mut far = point(0.0, 40.0, 0.0);
    far.cell_id += 1;
    source.observe(&snap(far));
    let before = store.export();
    let mut host = Host::with_transport(&mut sim);
    assert!(store.import(&mut host, &source.export()).is_err());
    assert_eq!(store.export(), before);
}

// Import merges an incoming point into the first known point within 1.5 m
// in its cell, wherever the two lie, and adds no edge twice.
#[test]
fn import_merges_close_points_into_the_first_and_skips_known_edges() {
    let mut sim = Sim::new();
    let mut target = TraversalStore::new();
    for p in [
        point(0.0, 0.0, 0.0),
        point(3.1, 0.0, 0.0),
        point(10.0, 0.0, 0.0),
    ] {
        target.observe(&snap(p));
    }
    // A point of another trace, 1.55 m from the first and the second.
    target.break_trace();
    target.observe(&snap(point(1.55, 0.0, 0.0)));
    assert_eq!(target.data().points.len(), 4, "{:?}", target.data().points);
    assert_eq!(target.data().edges.len(), 2);

    let mut source = TraversalStore::new();
    for p in [
        // 1.44 m from the first point, on a diagonal.
        point(1.0, 1.04, 0.0),
        // 1.36 m from the second.
        point(4.4, 0.4, 0.0),
        // 1.43 m from the third, partly in height.
        point(8.6, 0.0, 0.3),
        point(16.0, 0.0, 0.0),
    ] {
        source.observe(&snap(p));
    }
    source.break_trace();
    // 0.8 m from both the first point and the other trace's.
    source.observe(&snap(point(0.8, 0.0, 0.0)));
    source.observe(&snap(point(0.8, 5.0, 0.0)));

    let mut host = Host::with_transport(&mut sim);
    target.import(&mut host, &source.export()).expect("valid");
    let data = target.data();
    assert_eq!(data.points.len(), 6, "{:?}", data.points);
    let mut edges: Vec<(usize, usize)> = data.edges.iter().map(|e| (e.from, e.to)).collect();
    edges.sort_unstable();
    assert_eq!(edges, vec![(0, 1), (0, 5), (1, 2), (2, 4)]);
}
