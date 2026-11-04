//! The search for a portal inside a dungeon, from OpenAC-GoArrow's
//! `DungeonPortalSearchTests` (each test names the C# test it ports). A
//! clearance preview the C# fake answered at once arrives here as the
//! client's plan, handed to the search before it is asked again.

mod navsim;

use goarrow::dungeon_search::DungeonPortalSearch;
use goarrow::traversals::TraversalStore;
use navsim::{Sim, cell, dungeon_position, layer, search_next, snapshot};
use openac_plugin::Host;
use openac_plugin::navigation::{NavigationPlan, NavigationPlanStatus};
use openac_plugin::world::{
    DungeonCell, Floorplan, IndoorCell, ObjectClass, Position, WorldObject,
};

fn at(x: f64, y: f64, z: f64, cell_id: u32) -> Position {
    let base = dungeon_position(0.0);
    let mut p = dungeon_position(x as f32);
    p.north_south = base.north_south + y / 240.0;
    p.elevation = z / 240.0;
    p.cell_id = cell_id;
    p
}

fn sim_at(position: Position) -> Sim {
    let mut sim = Sim::new();
    sim.snapshot = snapshot(position);
    sim
}

fn arrive(sim: &mut Sim, search: &mut DungeonPortalSearch, target: &Position) {
    search.target_accepted(target.cell_id);
    sim.move_to(target.clone());
    search.complete_target(target, true);
}

fn square_floor(min: (f32, f32), max: (f32, f32)) -> Vec<(f32, f32)> {
    vec![min, (max.0, min.1), max, (min.0, max.1)]
}

// RecordedWalkShortcutsClearFloorButKeepsWallPortalAndStoreyTurns
#[test]
fn recorded_walk_shortcuts_clear_floor_but_keeps_wall_portal_and_storey_turns() {
    let a = |x: f64, y: f64| at(x, y, 0.0, 0x02AA_022F);
    let floor = square_floor((-5.0, -10.0), (50.0, 10.0));
    let plan = |walls: Vec<((f32, f32), (f32, f32))>| Floorplan {
        landblock_id: 0x02AA_0000,
        layers: vec![
            layer(0.0, vec![floor.clone()], walls),
            layer(-6.0, vec![floor.clone()], vec![]),
        ],
        cells: vec![
            cell(0x02AA_022F, (0.0, 0.0, 0.0), 0.0),
            cell(0x02AA_0230, (0.0, 0.0, 0.0), -6.0),
            cell(0x02AA_0231, (0.0, 0.0, 0.0), 0.0),
        ],
        bounds_min: (0.0, 0.0, 0.0),
        bounds_max: (1.0, 1.0, 1.0),
    };
    let wandering = [
        a(0.0, 0.0),
        a(6.0, 4.0),
        a(12.0, 4.0),
        a(18.0, 4.0),
        a(24.0, 0.0),
    ];
    let direct = DungeonPortalSearch::simplify_recorded_path(&wandering, &plan(vec![]), &[]);
    assert_eq!(direct, vec![a(0.0, 0.0), a(24.0, 0.0)]);
    let walled = DungeonPortalSearch::simplify_recorded_path(
        &wandering,
        &plan(vec![((12.0, -1.0), (12.0, 1.0))]),
        &[],
    );
    assert!(walled.len() > 2, "{walled:?}");
    let portal_path = [a(0.0, 0.0), a(6.0, 8.0), a(12.0, 8.0), a(24.0, 0.0)];
    let around_portal =
        DungeonPortalSearch::simplify_recorded_path(&portal_path, &plan(vec![]), &[a(12.0, -3.0)]);
    assert!(around_portal.len() > 2, "{around_portal:?}");
    assert_eq!(around_portal.last(), Some(&a(24.0, 0.0)));
    let stairs = [
        a(0.0, 0.0),
        at(6.0, 0.0, -6.0, 0x02AA_0230),
        at(12.0, 0.0, 0.0, 0x02AA_0231),
    ];
    assert_eq!(
        DungeonPortalSearch::simplify_recorded_path(&stairs, &plan(vec![]), &[]),
        stairs.to_vec()
    );
}

// DownstairsTargetKeepsItsFloorHeightEvenWhenGeometryExtendsUpstairs
fn downstairs_target_keeps_its_floor_height(floor: f32) {
    let mut sim = sim_at(dungeon_position(0.0));
    // Same horizontal place, a lower floor; its geometry's middle is
    // upstairs, so it cannot be the walking height.
    sim.cells = vec![cell(0x02AA_0101, (0.0, 0.0, 0.0), floor)];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let downstairs = search_next(&mut sim, &mut search, &store).expect("a target");
    assert_eq!(downstairs.cell_id, 0x02AA_0101);
    assert_eq!(downstairs.elevation, f64::from(floor) / 240.0);
    arrive(&mut sim, &mut search, &downstairs);
    let upstairs = search_next(&mut sim, &mut search, &store).expect("a way back");
    assert!(
        search.reason().contains("Backtracking"),
        "{}",
        search.reason()
    );
    assert_eq!(upstairs.elevation, 0.0);
}

#[test]
fn downstairs_target_keeps_its_floor_height_even_when_geometry_extends_upstairs() {
    downstairs_target_keeps_its_floor_height(-6.0);
}

#[test]
fn two_storeys_down_target_keeps_its_floor_height() {
    downstairs_target_keeps_its_floor_height(-12.0);
}

// SearchFollowsDistantForwardTargetBeforeBacktrackingToNearbySideBranch
#[test]
fn search_follows_distant_forward_target_before_backtracking_to_nearby_side_branch() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![
        cell(0x02AA_0101, (20.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (40.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0103, (30.0, 30.0, 0.0), 0.0),
        cell(0x02AA_0104, (100.0, 0.0, 0.0), 0.0),
    ];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let mut visit = |sim: &mut Sim, expected: u32, backtracking: bool| {
        let target = search_next(sim, &mut search, &store).expect("a target");
        assert_eq!(target.cell_id, expected, "{}", search.reason());
        assert_eq!(
            search.reason().starts_with("Backtracking"),
            backtracking,
            "{}",
            search.reason()
        );
        let mut arrived = target;
        arrived.elevation = 0.0;
        arrive(sim, &mut search, &arrived);
    };
    visit(&mut sim, 0x02AA_0101, false);
    visit(&mut sim, 0x02AA_0102, false);
    visit(&mut sim, 0x02AA_0104, false);
    visit(&mut sim, 0x02AA_0102, true);
    visit(&mut sim, 0x02AA_0101, true);
    visit(&mut sim, 0x02AA_0103, false);
    visit(&mut sim, 0x02AA_0101, true);
    visit(&mut sim, 0x02AA_022F, true);
    assert!(search_next(&mut sim, &mut search, &store).is_none());
    assert!(sim.previews.is_empty());
}

// SearchTakesTheWayDownEvenWhenItLeadsBackPastTheLastCheckpoint
#[test]
fn search_takes_the_way_down_even_when_it_leads_back_past_the_last_checkpoint() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![
        cell(0x02AA_0101, (15.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (30.0, 0.0, -6.0), -6.0),
    ];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let mut visit = |sim: &mut Sim| {
        let target = search_next(sim, &mut search, &store).expect("a target");
        arrive(sim, &mut search, &target);
        (target.cell_id, search.reason().to_owned())
    };
    // The level cell is nearer, but the way down comes first.
    assert_eq!(visit(&mut sim).0, 0x02AA_0102);
    // The next flight switches back past the start; it is still deeper, so
    // it continues this branch rather than waiting for a backtrack.
    sim.cells.push(cell(0x02AA_0103, (-5.0, 0.0, -12.0), -12.0));
    let (next, reason) = visit(&mut sim);
    assert_eq!(next, 0x02AA_0103);
    assert!(!reason.contains("Backtracking"), "{reason}");
}

// SearchFollowsBentCorridorToItsEndBeforeTheSideBranch
#[test]
fn search_follows_bent_corridor_to_its_end_before_the_side_branch() {
    // A long hall turns back on itself around a dividing wall; its far end
    // lies right behind the start, so the straight distance would send the
    // search there through the wall and then sideways.
    let mut start = dungeon_position(0.0);
    start.north_south += 5.0 / 240.0;
    let mut sim = sim_at(start);
    let c = |id: u32, x: f32, y: f32| cell(0x02AA_0100 + id, (x, y, 0.0), 0.0);
    sim.cells = vec![
        c(1, 20.0, 5.0),
        c(2, 40.0, 5.0),
        c(3, 55.0, 5.0),
        c(4, 78.0, 18.0),
        c(5, 55.0, 25.0),
        c(6, 30.0, 25.0),
        c(7, 5.0, 25.0),
        c(8, -30.0, 5.0),
        c(9, -60.0, 5.0),
    ];
    sim.layers = vec![layer(
        0.0,
        vec![square_floor((-70.0, -5.0), (90.0, 35.0))],
        vec![((-70.0, 12.0), (65.0, 12.0))],
    )];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let mut explored = Vec::new();
    for _ in 0..20 {
        let Some(target) = search_next(&mut sim, &mut search, &store) else {
            break;
        };
        if !search.reason().starts_with("Backtracking") {
            explored.push(target.cell_id);
        }
        arrive(&mut sim, &mut search, &target);
    }
    assert_eq!(
        explored,
        vec![0x02AA_0103, 0x02AA_0105, 0x02AA_0107, 0x02AA_0109]
    );
}

// TargetStandsOnTheCellFloorNotTheLayerBand
fn target_stands_on_the_cell_floor(origin: Option<f32>, expected: f64) {
    // A ruin floor at 43.5 m drawn in the 42 m layer: outside a sealed
    // dungeon the client cannot route to a goal 1.5 m under that floor.
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![cell(0x02AA_0101, (20.0, 0.0, 44.0), 42.0)];
    if let Some(origin) = origin {
        sim.indoor_cells = vec![IndoorCell {
            cell_id: 0x02AA_0101,
            origin: (20.0, 0.0, origin),
            orientation: (1.0, 0.0, 0.0, 0.0),
            sees_outside: true,
            ..IndoorCell::default()
        }];
    }
    let mut search = DungeonPortalSearch::new();
    let target = search_next(&mut sim, &mut search, &TraversalStore::new()).expect("a target");
    assert!(
        (target.elevation * 240.0 - expected).abs() < 0.001,
        "{}",
        target.elevation * 240.0
    );
}

#[test]
fn target_stands_on_the_cell_floor_not_the_layer_band() {
    target_stands_on_the_cell_floor(Some(43.5), 43.5);
}

#[test]
fn target_ignores_a_cell_origin_far_from_its_layer() {
    target_stands_on_the_cell_floor(Some(80.0), 42.0);
}

#[test]
fn target_without_a_cell_origin_stands_on_its_layer() {
    target_stands_on_the_cell_floor(None, 42.0);
}

// FailedTargetDoesNotBecomeABacktrackingCheckpoint
#[test]
fn failed_target_does_not_become_a_backtracking_checkpoint() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![
        cell(0x02AA_0101, (20.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (100.0, 0.0, 0.0), 0.0),
    ];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let failed = search_next(&mut sim, &mut search, &store).expect("a target");
    search.target_accepted(failed.cell_id);
    search.complete_target(&sim.snapshot.position.clone(), false);
    let next = search_next(&mut sim, &mut search, &store).expect("another target");
    assert_eq!(next.cell_id, 0x02AA_0102);
    assert!(!search.reason().contains("Backtracking"));
}

// TallCellBesideSurfacePortalIsExcludedAndCrossingPathIsRejected
#[test]
fn tall_cell_beside_surface_portal_is_excluded_and_crossing_path_is_rejected() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.objects = vec![WorldObject {
        object_id: 43,
        name: "Surface Portal".to_owned(),
        object_class: ObjectClass::Portal,
        position: Some(dungeon_position(20.0)),
        ..WorldObject::default()
    }];
    sim.cells = vec![
        cell(0x02AA_0230, (20.0, 0.0, 30.0), 0.0),
        cell(0x02AA_0232, (50.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0233, (50.0, 30.0, 0.0), 0.0),
    ];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    assert!(search_next(&mut sim, &mut search, &store).is_none());
    assert_eq!(sim.previews.len(), 1);
    let checked = sim.previews[0].place.clone().expect("a place preview");
    assert_eq!(checked.cell_id, 0x02AA_0232);
    // The client plans straight past the portal.
    assert!(search.plan_arrived(&NavigationPlan {
        handle: 1,
        status: NavigationPlanStatus::Routed,
        path: vec![dungeon_position(0.0), checked],
        ..NavigationPlan::default()
    }));
    let safe = search_next(&mut sim, &mut search, &store).expect("a safe target");
    assert_eq!(safe.cell_id, 0x02AA_0233);
    assert!(
        sim.previews
            .iter()
            .all(|p| p.place.as_ref().is_some_and(|p| p.cell_id != 0x02AA_0230))
    );
    assert!(sim.go_to_place_calls.is_empty());
    sim.move_to(dungeon_position(13.0));
    let mut host = Host::with_transport(&mut sim);
    let objects = host.objects();
    assert!(!search.safe_to_continue(&mut host, &objects, false));
    assert!(
        search.reason().contains("Surface Portal"),
        "{}",
        search.reason()
    );
}

// LeavingPortalIsAllowedButRunningAcrossItToFarEndpointIsNot
#[test]
fn leaving_portal_is_allowed_but_running_across_it_to_far_endpoint_is_not() {
    let p = dungeon_position;
    assert!(DungeonPortalSearch::crosses_portal(
        &[p(0.0), p(30.0)],
        &[p(5.0)]
    ));
    assert!(!DungeonPortalSearch::crosses_portal(
        &[p(0.0), p(-20.0)],
        &[p(5.0)]
    ));
    let high = |x: f32| {
        let mut q = p(x);
        q.elevation = 12.0 / 240.0;
        q
    };
    assert!(!DungeonPortalSearch::crosses_portal(
        &[high(0.0), high(30.0)],
        &[p(5.0)]
    ));
}

fn room_floor() -> Vec<(f32, f32)> {
    square_floor((-5.0, -10.0), (40.0, 10.0))
}

// VisibleRoomCellsAreSkippedButCellsBehindWallsAndOnOtherFloorsRemain
#[test]
fn visible_room_cells_are_skipped_but_cells_behind_walls_and_on_other_floors_remain() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![
        cell(0x02AA_0101, (12.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (16.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0103, (30.0, 5.0, 0.0), 0.0),
        cell(0x02AA_0104, (50.0, 0.0, 0.0), 0.0), // outside every floor polygon
        cell(0x02AA_0105, (30.0, 0.0, 12.0), 12.0),
    ];
    sim.layers = vec![
        layer(0.0, vec![room_floor()], vec![((20.0, 2.0), (20.0, 10.0))]),
        layer(12.0, vec![room_floor()], vec![]),
    ];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let first = search_next(&mut sim, &mut search, &store).expect("a target");
    assert_eq!(first.cell_id, 0x02AA_0103);
    search.target_accepted(first.cell_id);
    let second = search_next(&mut sim, &mut search, &store).expect("a target");
    assert_eq!(second.cell_id, 0x02AA_0105);
    search.target_accepted(second.cell_id);
    assert!(sim.previews.is_empty());
    assert!(search_next(&mut sim, &mut search, &store).is_none());
}

// SearchVisitsNextRoomOnceInsteadOfWalkingThroughItsCells
#[test]
fn search_visits_next_room_once_instead_of_walking_through_its_cells() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![
        cell(0x02AA_0101, (12.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (16.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0103, (30.0, 5.0, 0.0), 0.0),
        cell(0x02AA_0104, (32.0, 7.0, 0.0), 0.0),
    ];
    sim.layers = vec![layer(
        0.0,
        vec![room_floor()],
        vec![((20.0, -10.0), (20.0, 10.0))],
    )];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();

    let next_room = search_next(&mut sim, &mut search, &store).expect("a target");
    assert_eq!(next_room.cell_id, 0x02AA_0103);
    arrive(&mut sim, &mut search, &next_room);

    let back = search_next(&mut sim, &mut search, &store).expect("a way back");
    assert_eq!(back.cell_id, dungeon_position(0.0).cell_id);
    assert!(
        search.reason().contains("Backtracking"),
        "{}",
        search.reason()
    );
    arrive(&mut sim, &mut search, &dungeon_position(0.0));
    assert!(search_next(&mut sim, &mut search, &store).is_none());
}

// SearchSkipsShallowDeadEndButKeepsLongPassageAndPortals
fn search_skips_shallow_dead_end(
    hallway_cells: u32,
    portal_in_hallway: bool,
    should_explore: bool,
) {
    let mut start = dungeon_position(0.0);
    start.north_south += 10.0 / 240.0;
    let mut sim = sim_at(start);
    let mut cells: Vec<DungeonCell> = vec![
        cell(0x02AA_0101, (15.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (26.0, 0.0, 0.0), 0.0),
    ];
    for i in 1..hallway_cells {
        cells.push(cell(
            0x02AA_0102 + i,
            (26.0 + i as f32 * 12.0, 0.0, 0.0),
            0.0,
        ));
    }
    sim.cells = cells;
    if portal_in_hallway {
        sim.objects = vec![WorldObject {
            object_id: 43,
            name: "Dungeon Portal".to_owned(),
            object_class: ObjectClass::Portal,
            position: Some(dungeon_position(50.0)),
            ..WorldObject::default()
        }];
    }
    let end = 26.0 + (hallway_cells as f32 - 1.0) * 12.0 + 7.0;
    sim.layers = vec![layer(
        0.0,
        vec![square_floor((-5.0, -12.0), (80.0, 15.0))],
        vec![
            ((20.0, -12.0), (20.0, -2.0)),
            ((20.0, 2.0), (20.0, 15.0)),
            ((20.0, -5.0), (end, -5.0)),
            ((20.0, 5.0), (end, 5.0)),
            ((end, -5.0), (end, 5.0)),
        ],
    )];
    let mut search = DungeonPortalSearch::new();
    let target = search_next(&mut sim, &mut search, &TraversalStore::new());
    assert_eq!(target.is_some(), should_explore, "{}", search.reason());
    if should_explore {
        assert_eq!(target.unwrap().cell_id, 0x02AA_0102);
    }
}

#[test]
fn search_skips_a_one_cell_dead_end() {
    search_skips_shallow_dead_end(1, false, false);
}

#[test]
fn search_skips_a_two_cell_dead_end() {
    search_skips_shallow_dead_end(2, false, false);
}

#[test]
fn search_keeps_a_long_passage() {
    search_skips_shallow_dead_end(4, false, true);
}

#[test]
fn search_keeps_a_dead_end_with_a_portal() {
    search_skips_shallow_dead_end(2, true, true);
}

// SearchUsesWalkPlannerWithoutPreviewAndDoesNotConsumeRefusedTargets
#[test]
fn search_uses_walk_planner_without_preview_and_does_not_consume_refused_targets() {
    let mut sim = sim_at(dungeon_position(0.0));
    sim.cells = vec![
        cell(0x02AA_0101, (20.0, 0.0, 0.0), 0.0),
        cell(0x02AA_0102, (50.0, 0.0, 0.0), 0.0),
    ];
    let store = TraversalStore::new();
    let mut search = DungeonPortalSearch::new();
    let first = search_next(&mut sim, &mut search, &store).expect("a target");
    let refused = search_next(&mut sim, &mut search, &store).expect("the same target");
    assert_eq!(first.cell_id, refused.cell_id);
    search.target_accepted(first.cell_id);
    search.record_failure(Some("No path to the first cell"));
    let second = search_next(&mut sim, &mut search, &store).expect("a second target");
    assert_eq!(second.cell_id, 0x02AA_0102);
    assert_eq!(second.elevation, 0.0);
    search.target_accepted(second.cell_id);
    assert!(search_next(&mut sim, &mut search, &store).is_none());
    assert!(
        search.reason().contains("No path to the first cell"),
        "{}",
        search.reason()
    );
    assert!(
        search.reason().contains("2 attempted"),
        "{}",
        search.reason()
    );
    search.reset();
    let restarted = search_next(&mut sim, &mut search, &store).expect("a fresh target");
    assert_eq!(restarted.cell_id, first.cell_id);
    assert!(sim.previews.is_empty());
}
