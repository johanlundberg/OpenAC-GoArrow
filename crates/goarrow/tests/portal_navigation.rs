//! Walking routes through portals, from OpenAC-GoArrow's
//! `PortalNavigationTests` (each test names the C# test it ports). The
//! panel parts of the C# tests are the window's; here the navigator's view
//! stands in for them.

mod navsim;

use std::rc::Rc;

use goarrow::location::Location;
use goarrow::route::RouteStepKind;
use navsim::{Harness, dungeon_position, portal, pos, snapshot, with_destination};
use openac_plugin::items::ItemStatus;
use openac_plugin::navigation::GoToState;
use openac_plugin::world::{ActivationOutcome, ObjectClass, Position, WorldObject, capabilities};

const PORTAL: u8 = 2;
const LOGIN: u8 = 1;

fn outdoor(east_west: f64, north_south: f64) -> Position {
    pos(0, east_west, north_south, 0.0, true)
}

// RouteButtonsCycleChoicesAndGoKeepsTheSelectedPortal
#[test]
fn route_buttons_cycle_choices_and_go_keeps_the_selected_portal() {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Portal A" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="99" />
          <loc name="Portal B" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="98" />
          <loc name="Portal C" type="WildernessPortal" NS="0" EW="3" exitNS="0" exitEW="97" />
          <loc name="End" type="Town" NS="0" EW="100" />
        </locations>"#,
    );
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("End");
    let origin = Rc::new(Location::new("Current Position", 0.0, 0.0));
    h.destination.calculate_route(&origin, &mut h.finder);
    h.enable();
    assert!(h.destination.cycle_alternative(1, &origin, &mut h.finder));
    let portal_via = |h: &Harness| {
        h.destination
            .current_route()
            .unwrap()
            .steps()
            .iter()
            .find(|s| s.kind == RouteStepKind::Portal)
            .unwrap()
            .via
            .clone()
    };
    assert_eq!(portal_via(&h), "Portal B");
    assert!(h.destination.cycle_alternative(1, &origin, &mut h.finder));
    assert!(!h.destination.cycle_alternative(1, &origin, &mut h.finder));
    assert!(h.destination.cycle_alternative(-1, &origin, &mut h.finder));
    assert_eq!(h.destination.alternative_index(), 1);

    h.start();
    assert_eq!(portal_via(&h), "Portal B");
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert_eq!(h.sim.go_to_place_calls[0].0.east_west, 2.0);
}

// OutdoorNoProgressTriesSideDetoursThenReplansFromReachedPoint
#[test]
fn outdoor_no_progress_tries_side_detours_then_replans_from_reached_point() {
    let mut h = Harness::new(
        "<locations><loc name='Hurnmel the Smith' type='Vendor' NS='-65.4' EW='-44' /></locations>",
    );
    h.sim.snapshot = snapshot(outdoor(-52.15, -62.55));
    h.go_to_named("Hurnmel the Smith");
    h.enable();
    h.start();
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    let reason = Some("no clear path leads any nearer the goal");
    h.report(1, GoToState::NoRoute, 0, 2000.0, reason, 1);
    assert_eq!(h.sim.go_to_place_calls.len(), 2);
    let first = h.sim.go_to_place_calls[1].0.clone();
    let away = h.sim.snapshot.position.horizontal_distance_metres(&first);
    assert!((79.0..=81.0).contains(&away), "{away}");
    assert!(h.failure().contains("detour 1/12"), "{}", h.failure());
    h.report(2, GoToState::NoRoute, 0, 2000.0, reason, 2);
    assert_eq!(h.sim.go_to_place_calls.len(), 3);
    let second = h.sim.go_to_place_calls[2].0.clone();
    assert!(first.horizontal_distance_metres(&second) > 100.0);
    h.sim.move_to(second);
    h.report(3, GoToState::Arrived, 0, 0.0, None, 3);
    assert_eq!(h.sim.go_to_place_calls.len(), 4);
    assert_eq!(h.sim.go_to_place_calls[3].0.east_west, -44.0);
    assert_eq!(h.sim.go_to_place_calls[3].0.north_south, -65.4);
}

// UnrelatedOutdoorNoRouteDoesNotStartDetour
#[test]
fn unrelated_outdoor_no_route_does_not_start_detour() {
    let mut h =
        Harness::new("<locations><loc name='End' type='Town' NS='-65.4' EW='-44' /></locations>");
    h.sim.snapshot = snapshot(outdoor(-52.15, -62.55));
    h.go_to_named("End");
    h.enable();
    h.start();
    h.report(
        1,
        GoToState::NoRoute,
        0,
        2000.0,
        Some("the destination is unavailable"),
        1,
    );
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert!(!h.is_navigating());
}

// OutdoorDetoursStopAfterFiniteUnreachableCandidates
#[test]
fn outdoor_detours_stop_after_finite_unreachable_candidates() {
    let mut h =
        Harness::new("<locations><loc name='End' type='Town' NS='-65.4' EW='-44' /></locations>");
    h.sim.snapshot = snapshot(outdoor(-52.15, -62.55));
    h.go_to_named("End");
    h.enable();
    h.start();
    let mut sequence = 1;
    while sequence <= 13 && h.is_navigating() {
        h.report(
            sequence,
            GoToState::NoRoute,
            0,
            2000.0,
            Some("no clear path leads any nearer the goal"),
            sequence,
        );
        sequence += 1;
    }
    assert!(!h.is_navigating());
    assert!((2..=13).contains(&h.sim.go_to_place_calls.len()));
    assert!(
        h.failure().contains("outdoor detour attempts"),
        "{}",
        h.failure()
    );
}

// WalkingIntoTownNetworkBeforeArrivalCompletesApproachAndCrossing
fn walking_into_town_network(
    live_approach: bool,
    already_teleported: bool,
    resume_after_stop: bool,
) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network Portal(Sanamar)" type="PortalHub" NS="0" EW="2" exitNS="0" exitEW="95" />
          <loc name="Town Network (S L 1) to Sawato" type="PortalHub" NS="0" EW="95" exitNS="0" exitEW="200" />
          <loc name="End" type="Vendor" NS="0" EW="201" />
        </locations>"#,
    );
    h.sim.objects.push(portal(
        42,
        "Portal to Town Network",
        pos(0x1234_0001, 2.0, 0.0, 0.0, true),
    ));
    h.sim.objects.push(portal(
        43,
        "Sawato Portal",
        pos(0x5678_0101, 95.0, 0.0, 0.0, false),
    ));
    h.sim.snapshot = snapshot(pos(
        0x1234_0001,
        if live_approach { 1.9 } else { 0.0 },
        0.0,
        0.0,
        true,
    ));
    h.go_to_named("End");
    h.enable();
    h.start();
    let approach_sequence = h.sequence();
    let inside = pos(0x5678_0100, 95.0, 0.0, 0.0, false);
    h.sim.snapshot.is_portal_space = !already_teleported;
    if already_teleported {
        h.sim.move_to(inside.clone());
    }
    let lost = navsim::report(
        approach_sequence,
        GoToState::Lost,
        if live_approach { 42 } else { 0 },
        0.0,
        Some("the character entered portal space"),
        1,
    );
    if resume_after_stop {
        h.stop();
        h.sim.report = lost;
        h.resume();
    } else {
        h.event(openac_plugin::Event::NavigationChanged(lost));
    }
    if !already_teleported {
        assert!(!h.is_navigating());
        h.sim.snapshot.is_portal_space = false;
        h.sim.move_to(inside);
        h.tick(0.1);
    }
    assert!(h.is_navigating(), "{}", h.failure());
    assert_eq!(
        h.destination.current_route().unwrap().steps()[0].to.name,
        "Town Network (S L 1) to Sawato"
    );
    assert_eq!(h.sim.go_to_calls.last().unwrap().0, 43);
    assert!(h.sim.activated.is_empty());
    // A delayed report from the entrance cannot stop the outgoing walk.
    h.report(
        approach_sequence,
        GoToState::Lost,
        0,
        0.0,
        Some("the character entered portal space"),
        2,
    );
    assert!(h.is_navigating());
}

#[test]
fn walking_into_town_network_before_arrival_completes_approach_and_crossing() {
    walking_into_town_network(false, false, false);
}

#[test]
fn walking_into_town_network_live_approach() {
    walking_into_town_network(true, false, false);
}

#[test]
fn walking_into_town_network_already_teleported() {
    walking_into_town_network(false, true, false);
}

#[test]
fn walking_into_town_network_live_approach_already_teleported() {
    walking_into_town_network(true, true, false);
}

#[test]
fn walking_into_town_network_resumed_after_stop() {
    walking_into_town_network(true, true, true);
}

const DESERT_MARCH: &str = r#"<locations>
  <loc name="Desert March" type="PortalHub" NS="0" EW="1" dungeonId="02AA" />
  <loc name="Desert March to Black Hill Portal" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
  <loc name="End" type="Vendor" NS="0" EW="98" />
</locations>"#;

fn desert_march(cells: &[(u32, f32)]) -> Harness {
    let mut h = Harness::new(DESERT_MARCH);
    h.sim
        .objects
        .push(portal(43, "Surface Portal", dungeon_position(-5.0)));
    h.sim.snapshot = snapshot(dungeon_position(0.0));
    h.sim.cells = cells
        .iter()
        .map(|&(id, x)| navsim::cell(id, (x, 0.0, 0.0), 0.0))
        .collect();
    h.go_to_named("End");
    h
}

// RestartInsideDesertMarchExploresThenSwitchesToBlackHillWithoutUsingSurfacePortal
fn restart_inside_desert_march(reach_exploration_cell: bool, unreachable_cell: bool) {
    let mut h = desert_march(&[(0x02AA_0230, 20.0), (0x02AA_0232, 50.0)]);
    h.enable();
    h.start();
    assert_eq!(h.sim.identified, vec![43]);
    assert!(h.sim.activated.is_empty());
    h.tick(0.6);
    h.tick(0.6);
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert!(h.failure().contains("Exploring dungeon"), "{}", h.failure());
    assert_eq!(h.step_count(), 1);
    let waypoints = &h.view().dungeon_waypoints;
    assert_eq!(waypoints.len(), 1);
    assert!(
        waypoints[0].label.contains("Dungeon Searching cell"),
        "{}",
        waypoints[0].label
    );
    assert_eq!(waypoints[0].cell_id, 0x02AA_0230);
    if reach_exploration_cell || unreachable_cell {
        let sequence = h.sequence();
        if unreachable_cell {
            h.report(
                sequence,
                GoToState::NoRoute,
                0,
                0.0,
                Some("cell is unreachable"),
                1,
            );
        } else {
            h.report(sequence, GoToState::Arrived, 0, 0.0, None, 1);
        }
        assert_eq!(h.step_count(), 1);
        assert!(!h.view().has_arrived);
    }
    if unreachable_cell {
        h.tick(0.6);
        assert_eq!(h.sim.go_to_place_calls.len(), 2);
        assert_eq!(h.sim.go_to_place_calls[1].0.cell_id, 0x02AA_0232);
    }
    h.sim.objects.push(with_destination(
        portal(44, "Black Hill", dungeon_position(60.0)),
        "Black Hill",
    ));
    h.tick(0.6);
    assert_eq!(h.go_to_objects(), vec![44]);
    assert!(h.sim.activated.is_empty());
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 44, 0.0, None, 2);
    assert_eq!(h.sim.activated, vec![44]);
}

#[test]
fn restart_inside_desert_march_explores_then_switches_to_black_hill_without_using_surface_portal() {
    restart_inside_desert_march(false, false);
}

#[test]
fn restart_inside_desert_march_after_reaching_the_exploration_cell() {
    restart_inside_desert_march(true, false);
}

#[test]
fn restart_inside_desert_march_after_an_unreachable_cell() {
    restart_inside_desert_march(false, true);
}

// PlanningInsideDungeonNeverWalksBeforeGo
#[test]
fn planning_inside_dungeon_never_walks_before_go() {
    let mut h = desert_march(&[(0x02AA_0230, 20.0)]);
    h.enable();
    // Showing the route waits on the dungeon exit and asks the nearby
    // portal where it leads; neither may move the character before Go.
    assert!(!h.plan());
    assert_eq!(h.sim.identified, vec![43]);
    h.sim.set_destination(43, "Black Hill");
    h.ident_received(43);
    h.tick(0.6);
    h.tick(0.6);
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(h.sim.go_to_calls.is_empty());
    assert!(!h.is_navigating());

    h.start();
    assert!(h.is_navigating());
}

// ExplorationWaitsByUnidentifiedPortalWithDifferentName
fn exploration_waits_by_unidentified_portal(identified_destination: &str, correct_exit: bool) {
    let mut h = desert_march(&[(0x02AA_0230, 20.0)]);
    h.enable();
    h.start();
    h.tick(0.6);
    h.tick(0.6);
    assert!(
        h.is_navigating(),
        "{}; route={}",
        h.failure(),
        h.step_count()
    );
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    h.sim.set_destination(43, "Outdoor exit");
    h.ident_received(43);

    let mut arch = portal(44, "Ancient Arch", dungeon_position(60.0));
    arch.has_appraisal_data = true;
    h.sim.objects.push(arch);
    h.tick(0.1);
    assert!(h.sim.identified.contains(&44));
    assert!(h.sim.go_to_calls.is_empty());
    let diagnostics = h.with(|nav, cx| nav.dungeon_portal_diagnostics(cx));
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("Ancient Arch") && line.contains("identify=1/3 Started")),
        "{diagnostics:#?}"
    );

    h.sim.move_to(dungeon_position(55.0));
    h.tick(0.1);
    assert!(!h.is_navigating());
    assert!(
        h.failure().contains("Identifying nearby dungeon portal"),
        "{}",
        h.failure()
    );
    h.tick(0.6);
    assert_eq!(h.sim.go_to_place_calls.len(), 1);

    h.sim.set_destination(44, identified_destination);
    h.ident_received(44);
    if correct_exit {
        assert_eq!(h.go_to_objects(), vec![44]);
    } else {
        assert!(h.sim.go_to_calls.is_empty());
        assert_eq!(h.sim.go_to_place_calls.len(), 2);
        assert_eq!(h.sim.go_to_place_calls[1].0.cell_id, 0x02AA_0230);
    }
}

#[test]
fn exploration_waits_by_unidentified_portal_that_is_the_exit() {
    exploration_waits_by_unidentified_portal("Black Hill", true);
}

#[test]
fn exploration_waits_by_unidentified_portal_with_different_name() {
    exploration_waits_by_unidentified_portal("Somewhere Else", false);
}

// DungeonExitIdentifiesNearbyPortalsUntilDestinationMatches
fn dungeon_exit_identifies_nearby_portals(
    first_identification_rejected: bool,
    identified_destination: &str,
) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Desert March to Bandit Castle Portal" type="UndergroundPortal"
               NS="-22.9" EW="-28.8" exitNS="65" exitEW="49.3" />
          <loc name="Bandit Castle Lifestone" type="Lifestone" NS="65.4" EW="49" />
        </locations>"#,
    );
    h.sim.objects.push(portal(
        42,
        "Desert March to Bandit Castle Portal",
        outdoor(-28.8, -22.9),
    ));
    h.sim.objects.push(portal(
        43,
        "Surface Portal",
        pos(0x02AA_0101, 49.3, 65.0, 0.0, false),
    ));
    let mut bandit = portal(
        44,
        "Bandit Castle Portal",
        pos(0x02AA_0124, 49.4, 65.0, 0.0, false),
    );
    bandit.has_appraisal_data = true; // Other properties are known, but not the destination.
    h.sim.objects.push(bandit);
    if first_identification_rejected {
        h.sim.identify_statuses.insert(43, ItemStatus::InvalidItem);
    }
    h.sim.snapshot = snapshot(outdoor(-28.8, -22.9));
    h.go_to_named("Bandit Castle Lifestone");
    let origin = Rc::new(Location::new("Current Position", -22.9, -28.8));
    h.destination.calculate_route(&origin, &mut h.finder);
    h.enable();
    h.start();
    h.sim.move_to(pos(0x02AA_0100, 49.3, 65.0, 0.0, false));
    h.transition(0, 1, 0x02AA_0100, PORTAL);

    let expected: &[u32] = if first_identification_rejected {
        &[43, 44]
    } else {
        &[43]
    };
    assert_eq!(h.sim.identified, expected, "reason {}", h.failure());
    assert!(h.sim.go_to_calls.is_empty());
    if !first_identification_rejected {
        h.sim.set_destination(43, "Outdoor exit");
        h.ident_received(43);
        assert_eq!(h.sim.identified, vec![43, 44]);
        assert!(h.sim.go_to_calls.is_empty());
    }
    h.sim.set_destination(44, identified_destination);
    h.ident_received(44);
    assert_eq!(h.go_to_objects(), vec![44]);
}

#[test]
fn dungeon_exit_identifies_nearby_portals_until_destination_matches() {
    dungeon_exit_identifies_nearby_portals(false, "Bandit Castle Portal (64.9N, 49.2E).");
}

#[test]
fn dungeon_exit_identifies_the_next_portal_when_the_first_is_refused() {
    dungeon_exit_identifies_nearby_portals(true, "Bandit Castle Portal (64.9N, 49.2E).");
}

#[test]
fn dungeon_exit_matches_a_differently_named_portal_by_its_arrival() {
    dungeon_exit_identifies_nearby_portals(false, "Different label (65.8N, 49.3E).");
}

// UniqueSurfacePortalMustBeIdentifiedBeforeNavigationOrActivation
#[test]
fn unique_surface_portal_must_be_identified_before_navigation_or_activation() {
    let mut h =
        Harness::new("<locations><loc name='End' type='Town' NS='0' EW='98' /></locations>");
    let mut surface = portal(
        43,
        "Surface Portal",
        pos(0x1234_0122, 95.5, 0.0, 0.0, false),
    );
    surface.has_appraisal_data = true;
    h.sim.objects.push(surface);
    h.sim.snapshot = snapshot(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    h.go_to_named("End");
    h.enable();
    h.start();
    assert_eq!(h.sim.identified, vec![43]);
    assert!(h.sim.go_to_calls.is_empty());
    assert!(h.sim.activated.is_empty());
    h.sim.set_destination(43, "Outdoor exit");
    h.ident_received(43);
    assert_eq!(h.go_to_objects(), vec![43]);
}

// BlackHillCrossingUsesNamedDistantExitInsteadOfSurfacePortal
fn black_hill_crossing(exit_appears_later: bool, unnamed_exit: bool) {
    let atlas_name = if unnamed_exit {
        "Uncharted Passage"
    } else {
        "Desert March to Black Hill Portal"
    };
    let mut h = Harness::new(&format!(
        r#"<locations>
          <loc name="{atlas_name}" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
          <loc name="End" type="Vendor" NS="0" EW="98" />
        </locations>"#
    ));
    h.sim
        .objects
        .push(portal(42, "Desert March", outdoor(1.0, 0.0)));
    let mut surface = portal(
        43,
        "Surface Portal",
        pos(0x1234_0101, 95.01, 0.0, 0.0, false),
    );
    if unnamed_exit {
        surface.portal_destination = Some("0.0N, 1.0E".to_owned());
    }
    h.sim.objects.push(surface);
    let mut exit = portal(
        44,
        if unnamed_exit {
            "Unmarked Portal"
        } else {
            "Black Hill"
        },
        pos(0x1234_01FF, 101.25, 0.0, 0.0, false),
    );
    if !unnamed_exit {
        exit.portal_destination = Some("Black Hill".to_owned());
    }
    if !exit_appears_later {
        h.sim.objects.push(exit.clone());
    }
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![42]);
    h.sim.move_to(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    h.transition(0, 1, 0x1234_0100, PORTAL);
    if exit_appears_later {
        assert!(h.sim.go_to_calls.is_empty());
        let expected = if unnamed_exit { "End" } else { "Black Hill" };
        assert!(h.failure().contains(expected), "{}", h.failure());
        assert!(!h.plan());
        assert!(h.sim.go_to_calls.is_empty());
        h.sim.objects.push(exit);
        h.tick(0.6);
    }
    if unnamed_exit {
        assert!(h.sim.identified.contains(&44));
        assert!(h.sim.go_to_calls.is_empty());
        h.sim.set_destination(44, "Somewhere (0.0N, 95.0E)");
        h.ident_received(44);
    }
    assert_eq!(h.go_to_objects(), vec![44]);
    h.stop();
    h.resume();
    assert_eq!(h.go_to_objects(), vec![44, 44]);
    h.stop();
    h.start();
    assert_eq!(h.go_to_objects(), vec![44, 44, 44]);
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 44, 0.0, None, 2);
    assert_eq!(h.sim.activated, vec![42, 44]);
}

#[test]
fn black_hill_crossing_uses_named_distant_exit_instead_of_surface_portal() {
    black_hill_crossing(false, false);
}

#[test]
fn black_hill_crossing_when_the_exit_appears_later() {
    black_hill_crossing(true, false);
}

#[test]
fn black_hill_crossing_through_an_unnamed_exit() {
    black_hill_crossing(false, true);
}

#[test]
fn black_hill_crossing_through_an_unnamed_exit_that_appears_later() {
    black_hill_crossing(true, true);
}

// LandscapeWalkThroughInteriorContinuesButPortalTransitDoesNot
fn landscape_walk_through_interior(portal_transit: bool, next_is_distant_walk: bool) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="First entrance" type="WildernessPortal" NS="0" EW="0.1" exitNS="0" exitEW="90" />
          <loc name="End" type="Town" NS="0" EW="100" />
        </locations>"#,
    );
    h.sim.snapshot = snapshot(pos(0x35DB_0010, 0.0, 0.0, 0.0, true));
    h.go_to_named("End");
    h.enable();
    h.start();
    if next_is_distant_walk {
        assert!(h.destination.remove_route_step(1));
    }
    if portal_transit {
        h.sim.snapshot.is_portal_space = true;
        h.tick(0.1);
    }
    h.sim.snapshot.is_portal_space = false;
    h.sim.move_to(pos(0x35DB_0101, 0.1, 0.0, 0.0, false));
    h.tick(0.1);
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 0, 0.0, None, 1);
    // The next leg is the entrance activation: inside at the entrance with
    // no live portal, both cases search for the portal indoors.
    assert_eq!(
        h.view().waiting_for_indoor_portal,
        portal_transit || !next_is_distant_walk
    );
    if next_is_distant_walk && !portal_transit {
        assert!(h.is_navigating());
        assert_eq!(h.sim.go_to_place_calls.last().unwrap().0.east_west, 100.0);
    }
}

#[test]
fn landscape_walk_through_interior_continues() {
    landscape_walk_through_interior(false, false);
}

#[test]
fn landscape_walk_after_portal_transit_does_not_continue() {
    landscape_walk_through_interior(true, false);
}

#[test]
fn landscape_walk_through_interior_continues_to_a_distant_walk() {
    landscape_walk_through_interior(false, true);
}

#[test]
fn landscape_walk_after_portal_transit_waits_before_a_distant_walk() {
    landscape_walk_through_interior(true, true);
}

// LivePortalIsUsedImmediatelyWhenNearOrOnceAfterCoordinateFailure
fn live_portal_used(retry_fails: bool, portal_near: bool, portal_in_surface_interior: bool) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network Portal(Bluespire)" type="WildernessPortal" NS="0" EW="2" exitNS="0" exitEW="95" />
          <loc name="End" type="Town" NS="0" EW="96" />
        </locations>"#,
    );
    h.sim.objects.push(portal(
        42,
        "Portal to Town Network",
        pos(
            if portal_in_surface_interior {
                0x1234_0110
            } else {
                0x1234_0010
            },
            2.05,
            0.0,
            0.1,
            !portal_in_surface_interior,
        ),
    ));
    h.sim.snapshot = snapshot(outdoor(if portal_near { 1.95 } else { 1.0 }, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();
    if portal_in_surface_interior {
        h.sim.move_to(pos(0x1234_0120, 1.95, 0.0, 0.0, false));
        h.tick(0.1);
    }
    if portal_near {
        assert!(h.sim.go_to_place_calls.is_empty());
    } else {
        h.report(
            1,
            GoToState::NoRoute,
            0,
            36.0,
            Some("No reachable spot can see the goal"),
            1,
        );
    }
    assert!(h.is_navigating());
    assert_eq!(h.go_to_objects(), vec![42]);
    let sequence = h.sequence();
    if retry_fails {
        h.report(
            sequence,
            GoToState::NoRoute,
            42,
            0.0,
            Some("Live portal is also unreachable"),
            2,
        );
    } else {
        h.report(sequence, GoToState::Arrived, 42, 0.0, None, 2);
    }
    assert_eq!(h.go_to_objects(), vec![42]);
    if retry_fails {
        assert!(!h.is_navigating());
        assert!(h.sim.activated.is_empty());
        assert_eq!(h.failure(), "Live portal is also unreachable");
    } else {
        assert_eq!(h.sim.activated, vec![42]);
        assert!(h.view().waiting_for_interaction);
        assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    }
}

#[test]
fn live_portal_is_used_once_after_coordinate_failure() {
    live_portal_used(false, false, false);
}

#[test]
fn live_portal_retry_after_coordinate_failure_can_fail() {
    live_portal_used(true, false, false);
}

#[test]
fn live_portal_is_used_immediately_when_near() {
    live_portal_used(false, true, false);
}

#[test]
fn live_portal_used_when_near_can_fail() {
    live_portal_used(true, true, false);
}

#[test]
fn live_portal_in_a_surface_interior_is_used_when_near() {
    live_portal_used(false, true, true);
}

#[test]
fn live_portal_in_a_surface_interior_is_used_after_coordinate_failure() {
    live_portal_used(false, false, true);
}

// OutdoorRouteApproachesEotensfangPortalDespiteAtlasNameAndPosition
#[test]
fn outdoor_route_approaches_eotensfang_portal_despite_atlas_name_and_position() {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Bandit Castle Lifestone" type="Lifestone" NS="0" EW="0" />
          <loc name="Portal to Eotensfang" type="WildernessPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
          <loc name="Hurnmel the Smith" type="Vendor" NS="0" EW="96" />
        </locations>"#,
    );
    h.sim.objects.push(portal(
        42,
        "Eotensfang Portal",
        pos(0x1234_0010, 1.35, 0.0, 0.0, true),
    ));
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("Hurnmel the Smith");
    h.enable();
    h.start();
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert_eq!(h.sim.go_to_place_calls[0].0.east_west, 1.0);

    h.sim.move_to(outdoor(1.0, 0.0));
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    assert_eq!(h.go_to_objects(), vec![42]);
    assert!(h.sim.activated.is_empty());

    h.sim.move_to(pos(0x1234_0010, 1.35, 0.0, 0.0, true));
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 42, 0.0, None, 2);
    assert_eq!(h.sim.activated, vec![42]);
    assert!(h.view().waiting_for_interaction);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
}

// IndoorPortalMatchesNameOrDestinationLabel
fn indoor_portal_matches(target_name: &str, object_name: &str, destination_label: &str) {
    let mut h = Harness::new(&format!(
        "<locations><loc name='{target_name}' type='Town' NS='80' EW='80' /></locations>"
    ));
    h.sim.objects.push(with_destination(
        portal(43, object_name, pos(0x1234_0122, 40.0, 40.0, 0.0, false)),
        destination_label,
    ));
    h.sim.snapshot = snapshot(pos(0x1234_0100, 39.0, 40.0, 0.0, false));
    h.settings.arrival_distance = 2.5;
    h.go_to_named(target_name);
    h.start();
    assert_eq!(h.sim.go_to_calls, vec![(43, 0.25)]);
}

#[test]
fn indoor_portal_matches_destination_label() {
    indoor_portal_matches("Sawato", "Unmarked Portal", "Sawato");
}

#[test]
fn indoor_portal_matches_name_with_portal_suffix() {
    indoor_portal_matches("Sawato", "Sawato Portal", "28.7S, 59.3E");
}

#[test]
fn indoor_portal_matches_name_with_portal_to_prefix() {
    indoor_portal_matches("Sawato", "Portal to Sawato", "Sawato (28.7S, 59.3E)");
}

// StartingInsideDungeonCanUseUniqueSurfacePortalForOutdoorDestination
fn starting_inside_dungeon_uses_surface_portal(exit_offset: f64) {
    let mut h =
        Harness::new("<locations><loc name='End' type='Town' NS='0' EW='98' /></locations>");
    h.sim.objects.push(with_destination(
        portal(
            43,
            "Surface Portal",
            pos(0x1234_0122, 95.0 + exit_offset, 0.0, 0.0, false),
        ),
        "Outdoor exit",
    ));
    h.sim.snapshot = snapshot(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    h.go_to_named("End");
    h.enable();
    h.start();
    assert_eq!(h.go_to_objects(), vec![43]);
    h.report(1, GoToState::Arrived, 43, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![43]);
    h.sim.move_to(outdoor(96.0, 0.0));
    h.tick(0.1);
    assert!(h.is_navigating());
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert_eq!(h.sim.go_to_place_calls[0].0.east_west, 98.0);
}

#[test]
fn starting_inside_dungeon_can_use_unique_surface_portal_for_outdoor_destination() {
    starting_inside_dungeon_uses_surface_portal(0.5);
}

#[test]
fn starting_inside_dungeon_can_use_a_distant_unique_surface_portal() {
    starting_inside_dungeon_uses_surface_portal(6.25);
}

// MultipleSurfacePortalsDoNotSelectAnExitAutomatically
#[test]
fn multiple_surface_portals_do_not_select_an_exit_automatically() {
    let mut h =
        Harness::new("<locations><loc name='End' type='Town' NS='0' EW='98' /></locations>");
    for id in [43u32, 44] {
        h.sim.objects.push(portal(
            id,
            "Surface Portal",
            pos(0x1234_0122, 95.0 + f64::from(id - 43), 0.0, 0.0, false),
        ));
    }
    h.sim.snapshot = snapshot(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    h.go_to_named("End");
    h.start();
    assert!(h.sim.go_to_calls.is_empty());
    assert!(h.sim.activated.is_empty());
    // The first is asked where it leads; neither is chosen.
    assert_eq!(h.sim.identified, vec![43]);
}

// AtlasRouteUsesSurfacePortalDestinationThenReplansOutdoors
fn atlas_route_uses_surface_portal_destination(destination_known: bool) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Start" type="Town" NS="0" EW="0" />
          <loc name="Black Hill Portal" type="UndergroundPortal" NS="0" EW="1" exitNS="0" exitEW="95" />
          <loc name="Portal to Direlands South Landbridge" type="WildernessPortal" NS="0" EW="98" exitNS="0" exitEW="200" />
          <loc name="End" type="Town" NS="0" EW="107" />
        </locations>"#,
    );
    h.sim.objects.push(with_destination(
        portal(42, "Black Hill Portal", outdoor(1.0, 0.0)),
        "Dungeon entrance",
    ));
    let mut first = portal(
        43,
        "Surface Portal",
        pos(0x1234_0122, 95.5, 0.0, 0.0, false),
    );
    if destination_known {
        first.portal_destination = Some("Direlands South Landbridge".to_owned());
        first.has_appraisal_data = true;
    }
    h.sim.objects.push(first);
    let mut second = with_destination(
        portal(
            44,
            "Surface Portal",
            pos(0x1234_0123, 95.7, 0.0, 0.0, false),
        ),
        "Somewhere Else",
    );
    second.has_appraisal_data = true;
    h.sim.objects.push(second);
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![42]);

    h.sim.move_to(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    h.transition(0, 1, 0x1234_0100, PORTAL);
    if !destination_known {
        assert_eq!(h.sim.identified, vec![43]);
        assert!(h.sim.go_to_calls.is_empty());
        h.sim.set_destination(43, "Direlands South Landbridge");
        h.sim.objects[1].has_appraisal_data = true;
        h.ident_received(43);
    }
    assert_eq!(h.go_to_objects(), vec![43]);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
    h.report(2, GoToState::Arrived, 43, 0.0, None, 2);
    assert_eq!(h.sim.activated, vec![42, 43]);
    assert!(h.view().waiting_for_interaction);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));

    h.sim.move_to(outdoor(98.0, 0.0));
    h.tick(0.1);
    assert!(!h.view().waiting_for_interaction);
    assert!(h.is_navigating());
    assert_eq!(h.sim.go_to_place_calls.last().unwrap().0.east_west, 107.0);
}

#[test]
fn atlas_route_uses_known_surface_portal_destination_then_replans_outdoors() {
    atlas_route_uses_surface_portal_destination(true);
}

#[test]
fn atlas_route_identifies_surface_portal_destination_then_replans_outdoors() {
    atlas_route_uses_surface_portal_destination(false);
}

// OutdoorRouteContinuesThroughTownNetworkIndoorPortal
#[derive(Debug, Clone, Copy, Default)]
struct TownNetworkCase {
    receives_transition_event: bool,
    exits_before_indoor_walk_arrives: bool,
    portal_event_precedes_indoor_position: bool,
    portal_in_next_landblock: bool,
    portal_space_seen_on_tick: bool,
    resume_during_indoor_walk: bool,
    resume_before_entry_step_completes: bool,
}

#[allow(clippy::too_many_lines)]
fn through_town_network(case: TownNetworkCase) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Start" type="Town" NS="0" EW="1" />
          <loc name="Town Network Portal(Shoushi)" type="TownPortal"
            NS="0" EW="2" exitNS="40" exitEW="40" />
          <loc name="Town Network (E R 5) to Sawato" type="UndergroundPortal"
            NS="40" EW="40.02" exitNS="80" exitEW="80" />
          <loc name="Sawato" type="Town" NS="80" EW="80.57" />
        </locations>"#,
    );
    h.sim
        .objects
        .push(portal(42, "Portal to Town Network", outdoor(2.0, 0.0)));
    let sawato = with_destination(
        portal(
            43,
            "Sawato Portal",
            pos(
                if case.portal_in_next_landblock {
                    0x1235_0122
                } else {
                    0x1234_0122
                },
                40.02,
                40.0,
                0.0,
                false,
            ),
        ),
        "Sawato (80N, 80E)",
    );
    h.sim.snapshot = snapshot(outdoor(1.0, 0.0));
    h.go_to_named("Sawato");
    h.enable();
    h.start();

    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert_eq!(h.sim.go_to_place_calls[0].0.east_west, 2.0);
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![42]);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));

    if case.portal_event_precedes_indoor_position {
        h.transition(0, 1, 0x1234_0100, PORTAL);
        assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
        h.sim.move_to(outdoor(80.0, 80.0));
        h.tick(0.1);
        let last = h.sim.go_to_place_calls.last().unwrap().0.east_west;
        assert!((last - 80.57).abs() < 0.001, "{last}");
        assert!(h.is_navigating());
        return;
    }

    if !case.receives_transition_event {
        h.sim.snapshot.is_portal_space = true;
        h.tick(0.1);
        assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    }

    h.sim.snapshot.is_portal_space = false;
    h.sim.move_to(pos(
        0x1234_0100,
        if case.resume_before_entry_step_completes {
            42.0
        } else {
            40.0
        },
        40.0,
        0.0,
        false,
    ));
    if case.resume_before_entry_step_completes {
        h.stop();
        assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
        h.resume();
    } else if case.receives_transition_event {
        h.transition(0, 1, 0x1234_0100, PORTAL);
    } else {
        h.tick(0.1);
    }

    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
    assert!(h.view().waiting_for_indoor_portal);
    assert!(h.sim.go_to_calls.is_empty());
    h.sim.objects.push(sawato);
    h.tick(0.6);
    assert_eq!(h.go_to_objects(), vec![43]);
    if case.resume_during_indoor_walk {
        h.stop();
        assert!(h.view().can_resume);
        h.resume();
        assert_eq!(h.go_to_objects(), vec![43, 43]);
        assert!(h.is_navigating());
    }
    if case.portal_in_next_landblock {
        h.sim.move_to(pos(0x1236_0100, 40.01, 40.0, 0.0, false));
        h.tick(0.1);
        assert!(h.is_navigating());
        assert_eq!(h.sim.stop_go_to_count, 0);
    }
    if case.exits_before_indoor_walk_arrives {
        h.sim.snapshot.is_portal_space = true;
        if case.portal_space_seen_on_tick {
            h.tick(0.1);
        } else {
            h.report(2, GoToState::Lost, 43, 0.0, Some("Portal space"), 2);
        }
        assert_eq!(h.failure(), "Waiting for portal transition.");
        assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
        h.sim.snapshot.is_portal_space = false;
        h.sim.move_to(outdoor(80.0, 80.0));
        h.tick(0.1);
        let walk = h.sim.go_to_place_calls[1].0.east_west;
        assert!((walk - 80.57).abs() < 0.001, "{walk}");
        assert!(h.is_navigating());
        assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
        return;
    }
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 43, 0.0, None, 2);
    assert_eq!(h.sim.activated, vec![42, 43]);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));

    h.sim.move_to(outdoor(80.0, 80.0));
    if case.receives_transition_event {
        h.transition(0, 2, 0, PORTAL);
    } else {
        h.tick(0.1);
    }
    let walk = h.sim.go_to_place_calls[1].0.east_west;
    assert!((walk - 80.57).abs() < 0.001, "{walk}");
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));

    h.tick(16.0);
    assert!(h.is_navigating());
    assert_eq!(h.sim.go_to_place_calls.len(), 2);
    let sequence = h.sequence();
    h.report(
        sequence,
        GoToState::ArrivedWithoutSight,
        0,
        400.0,
        Some("No accessible route"),
        3,
    );
    assert!(!h.view().has_arrived);
    assert!(!h.is_navigating());
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
    assert!(h.failure().contains("400 m remaining"), "{}", h.failure());
}

#[test]
fn outdoor_route_continues_through_town_network_indoor_portal_with_transition_events() {
    through_town_network(TownNetworkCase {
        receives_transition_event: true,
        ..TownNetworkCase::default()
    });
}

#[test]
fn outdoor_route_continues_through_town_network_from_live_positions() {
    through_town_network(TownNetworkCase::default());
}

#[test]
fn town_network_indoor_walk_can_exit_before_it_arrives() {
    through_town_network(TownNetworkCase {
        exits_before_indoor_walk_arrives: true,
        ..TownNetworkCase::default()
    });
}

#[test]
fn town_network_indoor_walk_exit_seen_as_portal_space_on_a_tick() {
    through_town_network(TownNetworkCase {
        exits_before_indoor_walk_arrives: true,
        portal_space_seen_on_tick: true,
        ..TownNetworkCase::default()
    });
}

#[test]
fn town_network_transition_before_the_indoor_position() {
    through_town_network(TownNetworkCase {
        receives_transition_event: true,
        portal_event_precedes_indoor_position: true,
        ..TownNetworkCase::default()
    });
}

#[test]
fn town_network_portal_in_the_next_landblock() {
    through_town_network(TownNetworkCase {
        portal_in_next_landblock: true,
        ..TownNetworkCase::default()
    });
}

#[test]
fn town_network_resume_during_the_indoor_walk() {
    through_town_network(TownNetworkCase {
        resume_during_indoor_walk: true,
        ..TownNetworkCase::default()
    });
}

#[test]
fn town_network_resume_before_the_entry_step_completes() {
    through_town_network(TownNetworkCase {
        resume_before_entry_step_completes: true,
        ..TownNetworkCase::default()
    });
}

// PortalRouteWaitsForTransitionThenContinuesWalking
fn portal_route_waits_for_transition(
    arrives_indoors: bool,
    manual_resume_with_auto_navigate_off: bool,
) {
    let mut h = Harness::new(
        r#"<atlas>
          <location><id>1</id><name>Start</name><type>Town</type><latitude>0</latitude><longitude>1</longitude><retired>N</retired></location>
          <location><id>2</id><name>Far Portal</name><type>Wilderness Portal</type><latitude>0</latitude><longitude>2</longitude><arrival_latitude>0</arrival_latitude><arrival_longitude>95</arrival_longitude><retired>N</retired></location>
          <location><id>3</id><name>End</name><type>Town</type><latitude>0</latitude><longitude>96</longitude><retired>N</retired></location>
        </atlas>"#,
    );
    h.sim.objects.push(WorldObject {
        object_id: 42,
        name: "Entrance to Somewhere".to_owned(),
        capabilities: capabilities::PORTAL | capabilities::INTERACTABLE,
        position: Some(outdoor(2.0, 0.0)),
        ..WorldObject::default()
    });
    h.sim.snapshot = snapshot(outdoor(1.0, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();

    assert_eq!(h.sim.go_to_place_calls[0].0.east_west, 2.0);
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![42]);
    assert!(h.view().waiting_for_interaction);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    assert_eq!(h.sim.go_to_place_calls.len(), 1);

    h.activation(1, 42, ActivationOutcome::Completed);
    assert!(h.view().waiting_for_interaction);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    assert_eq!(h.sim.go_to_place_calls.len(), 1);

    h.transition(1, 1, 0, LOGIN);
    assert!(h.view().waiting_for_interaction);
    if manual_resume_with_auto_navigate_off {
        h.settings.auto_navigate = false;
        h.sim.move_to(outdoor(95.0, 0.0));
        h.resume();
        assert!(!h.view().waiting_for_interaction);
        assert!(!h.is_navigating());
        assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
        assert_eq!(h.sim.go_to_place_calls.len(), 1);
        return;
    }
    if arrives_indoors {
        h.sim.move_to(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    }
    h.transition(0, 2, 0, PORTAL);

    assert!(!h.view().waiting_for_interaction);
    if arrives_indoors {
        assert_eq!(h.sim.go_to_place_calls.len(), 1);
        h.sim.move_to(outdoor(95.0, 0.0));
        h.tick(0.1);
    }
    assert_eq!(h.sim.go_to_place_calls[1].0.east_west, 96.0);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
}

#[test]
fn portal_route_waits_for_transition_then_continues_walking() {
    portal_route_waits_for_transition(false, false);
}

#[test]
fn portal_route_arriving_indoors_continues_once_outdoors() {
    portal_route_waits_for_transition(true, false);
}

#[test]
fn portal_route_manual_resume_with_auto_navigate_off_does_not_walk() {
    portal_route_waits_for_transition(false, true);
}

// PortalLandingFarFromItsListedArrivalIsAvoidedAndReplanned
#[test]
fn portal_landing_far_from_its_listed_arrival_is_avoided_and_replanned() {
    let mut h = Harness::new(
        r#"<atlas>
          <location><id>7</id><latitude>0</latitude><longitude>1</longitude><name>Bad Portal</name>
            <type>Wilderness Portal</type><arrival_latitude>0</arrival_latitude><arrival_longitude>99</arrival_longitude></location>
          <location><id>8</id><latitude>0</latitude><longitude>52</longitude><name>Good Portal</name>
            <type>Wilderness Portal</type><arrival_latitude>0</arrival_latitude><arrival_longitude>99</arrival_longitude></location>
          <location><id>9</id><latitude>0</latitude><longitude>100</longitude><name>End</name><type>Town</type></location>
        </atlas>"#,
    );
    h.sim
        .objects
        .push(portal(42, "Bad Portal", outdoor(1.0, 0.0)));
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert!(h.view().waiting_for_interaction);
    assert_eq!(h.vias()[0], "Bad Portal");

    // The portal really lands 49 units short of its listed arrival.
    h.sim.move_to(outdoor(50.0, 0.0));
    h.with(|nav, cx| nav.resume_after_interaction(cx, true));

    assert!(!h.view().has_arrived);
    assert!(
        h.vias().iter().any(|v| v == "Good Portal"),
        "{:?}",
        h.vias()
    );
    assert!(!h.vias().iter().any(|v| v == "Bad Portal"));
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m.contains("'Bad Portal' arrived at"))
    );
    assert_eq!(h.sim.go_to_place_calls.last().unwrap().0.east_west, 52.0);
    let observed = h.view().pending_arrival.clone().expect("a pending arrival");
    assert_eq!(
        (
            observed.id,
            observed.name.as_str(),
            observed.arrival.as_str()
        ),
        (7, "Bad Portal", "0.0N, 50.0E")
    );
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m.contains("/go corrections save"))
    );
    assert_eq!(h.nav.take_pending_arrival(), Some(observed));
    assert_eq!(h.view().pending_arrival, None);
}

// BlockedPortalActivationKeepsStepAndAllowsRetry. This client ends a
// portal's activation only with a transition; a refusal is the server's
// answer to the use, with its error.
fn portal_activation_waiting(auto_navigate: bool) -> Harness {
    let mut h = Harness::new(
        "<locations><loc name='Portal to End' type='WildernessPortal' NS='0' EW='2' exitNS='0' exitEW='95' /><loc name='End' type='Town' NS='0' EW='96' /></locations>",
    );
    h.sim
        .objects
        .push(portal(42, "Portal to End", outdoor(2.0, 0.0)));
    h.sim.snapshot = snapshot(outdoor(1.0, 0.0));
    h.settings.auto_navigate = auto_navigate;
    h.go_to_named("End");
    h.enable();
    h.start();
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![42]);
    assert!(h.view().waiting_for_interaction);
    h
}

const REFUSED: &str =
    "GoArrow: Interaction with 'Portal to End' failed (error 0x0024). Use /go resume to retry.";

#[test]
fn blocked_portal_activation_keeps_step_and_allows_retry() {
    let mut h = portal_activation_waiting(false);

    // Another use's answer, and this one's success, change nothing.
    h.item_use(1, 99, 0x0024);
    h.item_use(2, 42, 0);
    assert!(h.view().waiting_for_interaction);

    h.item_use(3, 42, 0x0024);

    assert!(!h.view().waiting_for_interaction);
    assert!(h.view().can_resume);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    assert!(
        h.failure().contains("failed (error 0x0024)"),
        "{}",
        h.failure()
    );
    assert!(
        h.sim.posted().iter().any(|m| m == REFUSED),
        "{:?}",
        h.sim.posted()
    );
}

// A use the server neither refuses nor answers with a transition fails
// after the interaction timeout.
#[test]
fn portal_activation_without_a_transition_times_out() {
    let mut h = portal_activation_waiting(false);
    for _ in 0..14 {
        h.tick(1.0);
    }
    assert!(h.view().waiting_for_interaction);
    h.tick(1.0);

    assert!(!h.view().waiting_for_interaction);
    assert!(h.view().can_resume);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Portal));
    assert!(
        h.sim.posted().iter().any(|m| m
            == "GoArrow: Interaction with 'Portal to End' failed (TimedOut). Use /go resume to retry."),
        "{:?}",
        h.sim.posted()
    );
}

// Once the transition starts the wait is the transition's, however long.
#[test]
fn portal_activation_wait_ends_its_timeout_when_the_transition_starts() {
    let mut h = portal_activation_waiting(false);
    h.tick(1.0);
    let mut started = navsim::transition(1, 1, 0, PORTAL);
    started.is_completed = false;
    started.is_materialized = false;
    h.event(openac_plugin::Event::PortalTransition(started));
    for _ in 0..30 {
        h.tick(1.0);
    }
    assert!(h.view().waiting_for_interaction);
    assert!(!h.sim.posted().iter().any(|m| m.contains("TimedOut")));
}

// Portal space seen on a tick starts the transition as well.
#[test]
fn portal_activation_wait_ends_its_timeout_in_portal_space() {
    let mut h = portal_activation_waiting(false);
    h.sim.snapshot.is_portal_space = true;
    for _ in 0..30 {
        h.tick(1.0);
    }
    assert!(h.view().waiting_for_interaction);
    assert!(!h.sim.posted().iter().any(|m| m.contains("TimedOut")));
}

const FACILITY_HUB: &str = r#"<locations>
  <loc name="Town Network (S R) to Facility Hub" type="UndergroundPortal"
    NS="0" EW="95" exitNS="0" exitEW="200" />
  <loc name="Facility Hub to End" type="UndergroundPortal"
    NS="0" EW="200" exitNS="0" exitEW="300" />
  <loc name="End" type="Town" NS="0" EW="301" />
</locations>"#;

// FacilityHubCellCompletesMissedIndoorPortalCrossing
fn facility_hub_cell_completes_crossing(route_start_east_west: f64) {
    let mut h = Harness::new(FACILITY_HUB);
    h.sim.snapshot = snapshot(outdoor(route_start_east_west, 0.0));
    h.go_to_named("End");
    h.start();
    assert!(
        h.vias()
            .iter()
            .any(|v| v == "Town Network (S R) to Facility Hub")
    );
    h.sim.move_to(pos(0x8A02_0212, 200.0, 0.0, 0.0, false));

    h.tick(0.1);

    assert!(
        !h.vias()
            .iter()
            .any(|v| v == "Town Network (S R) to Facility Hub"),
        "{:?}",
        h.vias()
    );
    assert!(h.vias().iter().any(|v| v == "Facility Hub to End"));
}

#[test]
fn facility_hub_cell_completes_missed_indoor_portal_crossing() {
    facility_hub_cell_completes_crossing(94.9);
}

#[test]
fn facility_hub_cell_completes_missed_crossing_started_at_the_portal() {
    facility_hub_cell_completes_crossing(95.0);
}

// StartingInFacilityHubPlansFromItsAtlasArrival
#[test]
fn starting_in_facility_hub_plans_from_its_atlas_arrival() {
    let mut h = Harness::new(FACILITY_HUB);
    h.sim.snapshot = snapshot(pos(0x8A02_0212, 200.0, 0.0, 0.0, false));
    h.go_to_named("End");

    h.start();

    assert!(h.destination.current_route().is_some());
    assert!(
        !h.vias()
            .iter()
            .any(|v| v == "Town Network (S R) to Facility Hub")
    );
    assert!(h.vias().iter().any(|v| v == "Facility Hub to End"));
}

fn folthid_estate() -> WorldObject {
    with_destination(
        portal(
            44,
            "Folthid Estate",
            pos(0x8A02_01E3, 200.1, 0.0, 0.0, false),
        ),
        "Folthid Estate (8.8S, 53.5E).",
    )
}

// FacilityHubWalkTargetsVisibleFolthidEstatePortal
fn facility_hub_walk_targets_folthid_estate(portal_arrival_mode: u8) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network (S R) to Facility Hub" type="UndergroundPortal"
            NS="0" EW="95" exitNS="0" exitEW="200" />
          <loc name="Facility Hub (E)(R 1 (10) C) to Folthid Estate" type="UndergroundPortal"
            NS="0" EW="200.1" exitNS="0" exitEW="300" />
          <loc name="End" type="Town" NS="0" EW="301" />
        </locations>"#,
    );
    if portal_arrival_mode == 0 {
        h.sim.objects.push(folthid_estate());
    } else {
        h.sim.objects.push(portal(
            50,
            "Al-Jalima Portal",
            pos(0x0007_010A, 0.0, 0.0, 0.0, false),
        ));
    }
    h.sim.snapshot = snapshot(pos(0x8A02_0211, 200.0, 0.0, 0.0, false));
    h.go_to_named("End");

    h.start();

    if portal_arrival_mode != 0 {
        assert!(h.sim.go_to_calls.is_empty());
        assert!(
            h.failure()
                .contains("Waiting for indoor portal 'Folthid Estate'"),
            "{}",
            h.failure()
        );
        assert!(
            h.failure().contains("Portals in current dungeon: 0"),
            "{}",
            h.failure()
        );
        h.sim.objects.push(folthid_estate());
        if portal_arrival_mode == 1 {
            h.tick(0.6);
        } else {
            h.enable();
            h.created(folthid_estate());
        }
    }

    assert_eq!(h.go_to_objects(), vec![44]);
    assert!(h.is_navigating());
    assert!(
        !h.failure()
            .contains("Waiting for indoor portal 'Facility Hub'")
    );
}

#[test]
fn facility_hub_walk_targets_visible_folthid_estate_portal() {
    facility_hub_walk_targets_folthid_estate(0);
}

#[test]
fn facility_hub_walk_targets_folthid_estate_portal_seen_on_a_later_tick() {
    facility_hub_walk_targets_folthid_estate(1);
}

#[test]
fn facility_hub_walk_targets_folthid_estate_portal_when_it_appears() {
    facility_hub_walk_targets_folthid_estate(2);
}

// IndoorPortalWaitReportsNamedPortalEligibility
fn indoor_portal_wait_reports_eligibility(tracked: bool) {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network (S R) to Facility Hub" type="UndergroundPortal"
            NS="0" EW="95" exitNS="0" exitEW="200" />
          <loc name="Facility Hub (E)(L 1 (10) C) to Folthid Estate" type="UndergroundPortal"
            NS="0" EW="200.1" exitNS="0" exitEW="300" />
          <loc name="End" type="Town" NS="0" EW="301" />
        </locations>"#,
    );
    if tracked {
        h.sim.objects.push(WorldObject {
            object_id: 0x78A0_20AA,
            name: "Folthid Estate".to_owned(),
            object_class: ObjectClass::Unknown,
            position: Some(pos(0x8A02_01E3, 200.1, 0.0, 0.0, false)),
            ..WorldObject::default()
        });
    }
    h.sim.snapshot = snapshot(pos(0x8A02_0212, 200.0, 0.0, 0.0, false));
    h.go_to_named("End");

    h.start();

    assert!(h.sim.go_to_calls.is_empty());
    let expected = if tracked {
        "'Folthid Estate' 78A020AA class=Unknown"
    } else {
        "Objects named 'Folthid Estate': none tracked by client."
    };
    assert!(h.failure().contains(expected), "{}", h.failure());
    if tracked {
        assert!(
            h.failure().contains("eligible=not activatable"),
            "{}",
            h.failure()
        );
    }
}

#[test]
fn indoor_portal_wait_reports_that_no_named_portal_is_tracked() {
    indoor_portal_wait_reports_eligibility(false);
}

#[test]
fn indoor_portal_wait_reports_named_portal_eligibility() {
    indoor_portal_wait_reports_eligibility(true);
}

// UntrackedIndoorPortalIsExploredForOnlyInSealedDungeon
fn untracked_indoor_portal_explored(sealed_dungeon: bool) {
    let hub = |x: f64, cell: u32| {
        pos(
            cell,
            (11.0 * 192.0 + x - 84.0) / 240.0,
            (-125.0 * 192.0 + 10.0 - 84.0) / 240.0,
            0.0,
            false,
        )
    };
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network (S R) to Facility Hub" type="UndergroundPortal"
            NS="0" EW="95" exitNS="-100.31" exitEW="8.49" />
          <loc name="Facility Hub (E)(L 1 (10) C) to Folthid Estate" type="UndergroundPortal"
            NS="-100.31" EW="8.9" exitNS="-8.8" exitEW="53.5" />
          <loc name="End" type="Town" NS="-8.8" EW="53.6" />
        </locations>"#,
    );
    h.sim.snapshot = snapshot(hub(10.0, 0x8A02_0212));
    h.sim.cells = vec![
        navsim::cell(0x8A02_01E0, (40.0, 10.0, 0.0), 0.0),
        navsim::cell(0x8A02_01E3, (80.0, 10.0, 0.0), 0.0),
    ];
    h.sim.sealed = sealed_dungeon;
    h.go_to_named("End");
    h.enable();

    h.start();

    assert!(h.sim.go_to_calls.is_empty());
    if !sealed_dungeon {
        // A building's rooms see the landscape; away from the portal's
        // Atlas location there is nothing to search for.
        assert!(h.sim.go_to_place_calls.is_empty());
        assert!(!h.failure().contains("Exploring dungeon"));
        assert!(
            h.failure()
                .contains("Waiting for indoor portal 'Folthid Estate'"),
            "{}",
            h.failure()
        );
        return;
    }
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert!(
        h.failure()
            .contains("Exploring dungeon for portal 'Folthid Estate'"),
        "{}",
        h.failure()
    );

    h.sim.objects.push(portal(
        0x78A0_20AA,
        "Folthid Estate",
        hub(80.0, 0x8A02_01E3),
    ));
    h.tick(0.1);

    assert_eq!(h.go_to_objects(), vec![0x78A0_20AA]);
    assert!(h.is_navigating());
}

#[test]
fn untracked_indoor_portal_is_explored_for_in_a_sealed_dungeon() {
    untracked_indoor_portal_explored(true);
}

#[test]
fn untracked_indoor_portal_is_not_explored_for_outside_a_sealed_dungeon() {
    untracked_indoor_portal_explored(false);
}

// PortalInsideSurfaceRuinIsFoundByExploringAtItsAtlasLocation
#[test]
fn portal_inside_surface_ruin_is_found_by_exploring_at_its_atlas_location() {
    // Imuth Maer Doquin: the Halls of Metos portal is in the ruin's lower
    // rooms, which see the landscape and so are not sealed.
    let ruin = |x: f64, y: f64, cell: u32| {
        pos(
            cell,
            (72.0 * 192.0 + x - 84.0) / 240.0,
            (-15.0 * 192.0 + y - 84.0) / 240.0,
            0.0,
            (cell & 0xFFFF) < 0x100,
        )
    };
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Halls of Metos (Imuth Maer Doquin)" type="WildernessPortal"
            NS="-11.7" EW="58.0" exitNS="61.4" exitEW="-46.6" />
          <loc name="End" type="Town" NS="61.4" EW="-46.5" />
        </locations>"#,
    );
    h.sim.snapshot = snapshot(ruin(100.0, 155.0, 0xC770_0031));
    h.sim.cells = vec![
        navsim::cell(0xC770_0111, (180.0, 185.0, 0.0), 0.0),
        navsim::cell(0xC770_0112, (150.0, 185.0, 0.0), 0.0),
    ];
    h.go_to_named("End");
    h.enable();
    h.start();
    let walks = h.sim.go_to_place_calls.len();
    assert!(walks > 0);

    h.sim.move_to(ruin(180.0, 155.0, 0xC770_0110));
    let sequence = h.sequence();
    h.report(sequence, GoToState::Arrived, 0, 0.0, None, 1);

    assert!(h.sim.activated.is_empty());
    assert!(!h.view().waiting_for_interaction);
    assert!(
        h.failure()
            .contains("Exploring dungeon for portal 'Halls of Metos (Imuth Maer Doquin)'"),
        "{}",
        h.failure()
    );
    assert_eq!(h.sim.go_to_place_calls.len(), walks + 1);
    let first_target = h.sim.go_to_place_calls.last().unwrap().0.cell_id;

    // The route to the next room leaves through the entrance and comes
    // back in; stepping outside is not leaving the ruin.
    h.sim.report.state = GoToState::Walking;
    h.sim.move_to(ruin(178.0, 150.0, 0xC770_003F));
    h.tick(0.6);
    h.sim.move_to(ruin(180.0, 160.0, 0xC770_0110));
    h.tick(0.6);
    assert_eq!(h.sim.stop_go_to_count, 0);
    assert_eq!(h.sim.go_to_place_calls.len(), walks + 1);
    assert!(h.failure().contains("Exploring dungeon"), "{}", h.failure());

    // A target the client keeps planning for is abandoned for another cell.
    h.sim.report.state = GoToState::Planning;
    h.sim.report.reason = Some("planning".to_owned());
    for _ in 0..16 {
        h.tick(1.0);
    }
    h.tick(0.6);
    assert_eq!(h.sim.stop_go_to_count, 1);
    assert_eq!(h.sim.go_to_place_calls.len(), walks + 2);
    assert_ne!(
        h.sim.go_to_place_calls.last().unwrap().0.cell_id,
        first_target
    );

    h.sim.objects.push(portal(
        0x7C77_0042,
        "Halls of Metos",
        ruin(185.0, 160.0, 0xC770_0113),
    ));
    h.tick(0.1);

    assert_eq!(h.sim.activated, vec![0x7C77_0042]);
}

// GatewayPortalMatchesQualifiedAtlasName
#[test]
fn gateway_portal_matches_qualified_atlas_name() {
    // Imuth Maer Doquin: the live portal stands 28 m from the Atlas spot,
    // beyond the distance an unrelated name is trusted at.
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Halls of Metos (Imuth Maer Doquin)" type="WildernessPortal"
            NS="-11.7" EW="58.0" exitNS="61.4" exitEW="-46.6" />
          <loc name="End" type="Town" NS="61.4" EW="-46.5" />
        </locations>"#,
    );
    h.sim.objects.push(portal(
        0x7C77_0042,
        "Gateway to the Halls of Metos",
        pos(0xC770_003F, 58.0 + 28.0 / 240.0, -11.7, 0.0, true),
    ));
    h.sim.snapshot = snapshot(pos(0xC770_003F, 58.0, -11.7, 0.0, true));
    h.go_to_named("End");
    h.enable();

    h.start();

    assert_eq!(h.sim.activated, vec![0x7C77_0042]);
    assert!(!h.failure().contains("Could not identify"));
}

// A walk report another plugin owns never moves the route on (the C#
// navigator's owner filter, `HandleNavigationReport`).
#[test]
fn reports_of_another_owners_walk_are_ignored() {
    let mut h = Harness::new("<locations><loc name='End' type='Town' NS='0' EW='5' /></locations>");
    // The id the client started the plugin with (GA-40): the default id is
    // then another owner's.
    h.nav.set_owner("tests.goarrow");
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();
    let mut foreign = navsim::report(1, GoToState::Arrived, 0, 0.0, None, 1);
    foreign.owner = Some("openac.goarrow".to_owned());
    h.event(openac_plugin::Event::NavigationChanged(foreign));
    assert!(h.is_navigating());
    assert!(!h.view().has_arrived);
    let mut ours = navsim::report(1, GoToState::Arrived, 0, 0.0, None, 2);
    ours.owner = Some("Tests.GoArrow".to_owned());
    h.event(openac_plugin::Event::NavigationChanged(ours));
    assert!(h.view().has_arrived);
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m == "GoArrow: Arrived at 'End'.")
    );
}

// ---- recall steps ----

/// A route that starts with a recall ("Portal Recall" from Home, where the
/// character stands, to a far town).
fn recall_route() -> Harness {
    let mut h = Harness::new(
        "<locations><loc name='Home' type='Town' NS='0' EW='0' /><loc name='Far Town' type='Town' NS='0' EW='90' /></locations>",
    );
    h.finder
        .database_mut()
        .load_route_starts_csv(["Far Town;Home;Portal Recall"]);
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("Far Town");
    h.enable();
    h
}

fn recall_item(object_id: u32, position: Option<Position>, is_owned: bool) -> WorldObject {
    WorldObject {
        object_id,
        name: "Portal Recall Gem".to_owned(),
        capabilities: capabilities::INTERACTABLE,
        position,
        is_owned,
        ..WorldObject::default()
    }
}

/// Walks the route to its recall step.
fn reach_recall(h: &mut Harness) {
    h.start();
    while h.first_kind() == Some(RouteStepKind::Travel) {
        let sequence = h.sequence();
        let revision = h.sim.report.revision + 1;
        h.sim.report.revision = revision;
        h.report(sequence, GoToState::Arrived, 0, 0.0, None, revision);
    }
    assert_eq!(
        h.first_kind(),
        Some(RouteStepKind::Recall),
        "{:?}",
        h.vias()
    );
}

// The object list holds the character's own things too: one in a pack is
// never the world object a recall step uses.
#[test]
fn recall_step_does_not_use_an_item_in_a_pack() {
    let mut h = recall_route();
    h.sim.objects.push(recall_item(51, None, true));
    reach_recall(&mut h);

    assert!(h.sim.activated.is_empty(), "{:?}", h.sim.activated);
    assert!(h.view().waiting_for_interaction);
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m.starts_with("GoArrow: Could not identify recall 'Portal Recall'")),
        "{:?}",
        h.sim.posted()
    );
}

// One in the world at the step's place is used.
#[test]
fn recall_step_uses_a_world_object_at_its_place() {
    let mut h = recall_route();
    h.sim.objects.push(recall_item(51, None, true));
    h.sim
        .objects
        .push(recall_item(52, Some(outdoor(0.0, 0.0)), false));
    reach_recall(&mut h);

    assert_eq!(h.sim.activated, vec![52]);
}

// ---- the object table, read once ----

fn facility_hub_position(x: f64, cell: u32) -> Position {
    pos(
        cell,
        (11.0 * 192.0 + x - 84.0) / 240.0,
        (-125.0 * 192.0 + 10.0 - 84.0) / 240.0,
        0.0,
        false,
    )
}

/// Indoors, waiting for a portal the route needs (Folthid Estate) that the
/// client does not track: every half second the leg is tried again, and the
/// window's indoor preview looks for a portal named like the destination.
fn waiting_for_an_untracked_indoor_portal() -> Harness {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network (S R) to Facility Hub" type="UndergroundPortal"
            NS="0" EW="95" exitNS="-100.31" exitEW="8.49" />
          <loc name="Facility Hub (E)(L 1 (10) C) to Folthid Estate" type="UndergroundPortal"
            NS="-100.31" EW="8.9" exitNS="-8.8" exitEW="53.5" />
          <loc name="End" type="Town" NS="-8.8" EW="53.6" />
        </locations>"#,
    );
    h.sim.fake.window_attached = true;
    h.sim.snapshot = snapshot(facility_hub_position(10.0, 0x8A02_0212));
    for i in 0..20 {
        h.sim.objects.push(portal(
            0x7100_0000 + i,
            "Some Other Portal",
            facility_hub_position(30.0 + f64::from(i), 0x8A02_0213),
        ));
    }
    h.go_to_named("End");
    h.enable();
    h.start();
    assert!(
        h.failure()
            .contains("Waiting for indoor portal 'Folthid Estate'"),
        "{}",
        h.failure()
    );
    h
}

// Every check of a tick shares one read of the object table (which holds
// the character's inventory too).
#[test]
fn a_tick_reads_the_object_table_once() {
    let mut h = waiting_for_an_untracked_indoor_portal();
    for _ in 0..4 {
        h.sim.object_table_reads = 0;
        h.tick(0.5);
        assert!(
            h.sim.object_table_reads <= 1,
            "{} reads",
            h.sim.object_table_reads
        );
    }
    assert!(
        h.failure()
            .contains("Waiting for indoor portal 'Folthid Estate'"),
        "{}",
        h.failure()
    );
}

// An object change that concerns nothing the navigator waits for reads
// nothing.
#[test]
fn an_unrelated_object_change_reads_nothing() {
    let mut h = waiting_for_an_untracked_indoor_portal();
    h.sim.object_table_reads = 0;
    let other = WorldObject {
        object_id: 0x5000_0001,
        name: "Pyreal".to_owned(),
        is_owned: true,
        ..WorldObject::default()
    };
    h.sim.objects.push(other.clone());
    h.created(other);
    assert_eq!(h.sim.object_table_reads, 0);
}

// ---- the exploration planning timeout ----

// A target the client is still planning a walk to after 15 seconds counts
// as unreachable: the walk is stopped and the search goes on.
#[test]
fn exploration_target_still_planning_after_the_timeout_is_given_up() {
    let mut h = Harness::new(
        r#"<locations>
          <loc name="Town Network (S R) to Facility Hub" type="UndergroundPortal"
            NS="0" EW="95" exitNS="-100.31" exitEW="8.49" />
          <loc name="Facility Hub (E)(L 1 (10) C) to Folthid Estate" type="UndergroundPortal"
            NS="-100.31" EW="8.9" exitNS="-8.8" exitEW="53.5" />
          <loc name="End" type="Town" NS="-8.8" EW="53.6" />
        </locations>"#,
    );
    h.sim.snapshot = snapshot(facility_hub_position(10.0, 0x8A02_0212));
    h.sim.cells = vec![
        navsim::cell(0x8A02_01E0, (40.0, 10.0, 0.0), 0.0),
        navsim::cell(0x8A02_01E3, (80.0, 10.0, 0.0), 0.0),
    ];
    h.sim.sealed = true;
    h.go_to_named("End");
    h.enable();
    h.start();
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert_eq!(h.sim.report.state, GoToState::Planning);

    for _ in 0..14 {
        h.tick(1.0);
    }
    assert_eq!(h.sim.stop_go_to_count, 0);
    assert!(h.is_navigating());
    h.tick(1.0);

    assert_eq!(h.sim.stop_go_to_count, 1);
    assert!(!h.is_navigating());
    // The next target is tried half a second later.
    h.tick(0.5);
    assert_eq!(h.sim.go_to_place_calls.len(), 2);
    assert_ne!(
        h.sim.go_to_place_calls[1].0.cell_id,
        h.sim.go_to_place_calls[0].0.cell_id
    );
}
