//! Go, Stop and Resume at the navigator, from the navigator parts of
//! OpenAC-GoArrow's `RoutePlanningIntegrationTests` (each test names the C#
//! test it ports; the C# went through the plugin's panel and commands,
//! which are the plugin's). The C# used the plugin's own Holtburg; here a
//! small place file stands in for it.

mod navsim;

use navsim::{Harness, pos, snapshot};
use openac_plugin::Event;
use openac_plugin::navigation::GoToState;

const HOLTBURG: &str = "<locations><loc name='Holtburg' type='Town' NS='5' EW='5' /></locations>";

fn outdoors_at_origin() -> Harness {
    let mut h = Harness::new(HOLTBURG);
    h.sim.snapshot = snapshot(pos(0, 0.0, 0.0, 0.0, true));
    h.go_to_named("Holtburg");
    h.enable();
    h
}

// ResumeButtonRestartsStoppedRouteFromLivePosition
#[test]
fn resume_restarts_stopped_route_from_live_position() {
    let mut h = outdoors_at_origin();

    h.start();
    let first_leg_count = h.sim.go_to_place_calls.len();
    assert!(first_leg_count > 0);
    assert!(!h.view().can_resume);

    h.stop();
    assert!(h.view().can_resume);
    h.sim.move_to(pos(0, 1.0, 1.0, 0.0, true));
    h.resume();

    assert!(h.is_navigating());
    assert!(!h.view().can_resume);
    assert!(h.sim.go_to_place_calls.len() > first_leg_count);
    let last = &h.sim.go_to_place_calls.last().unwrap().0;
    assert_eq!((last.east_west, last.north_south), (5.0, 5.0));
}

// ResumeDoesNotStartWalkingWhenAutoNavigateIsOff
#[test]
fn resume_does_not_start_walking_when_auto_navigate_is_off() {
    let mut h = outdoors_at_origin();
    h.start();
    let first_leg_count = h.sim.go_to_place_calls.len();
    h.stop();

    h.settings.auto_navigate = false;
    h.resume();
    assert!(!h.view().can_resume);
    assert_eq!(h.sim.go_to_place_calls.len(), first_leg_count);
    assert!(!h.is_navigating());

    h.settings.auto_navigate = true;
    h.tick(0.015);
    assert!(h.view().can_resume);
    h.resume();
    assert!(h.sim.go_to_place_calls.len() > first_leg_count);
}

// GoDoesNotPlanOutdoorRouteFromIndoorPosition
#[test]
fn go_does_not_plan_outdoor_route_from_indoor_position() {
    let mut h = Harness::new(HOLTBURG);
    h.sim.snapshot = snapshot(pos(0x1234_0100, 95.0, 0.0, 0.0, false));
    h.go_to_named("Holtburg");
    h.enable();

    h.start();

    assert_eq!(h.step_count(), 0);
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(h.sim.fake.logs.iter().any(|(_, line)| line
        == "GoArrow: Outdoor route planning is paused while indoors or in portal space."));
}

// GoPlansRouteWithoutIssuingMovementWhenAutoNavigateIsOff (at the
// navigator: Go with walking turned off only plans)
#[test]
fn go_plans_route_without_issuing_movement_when_walking_is_off() {
    let mut h = outdoors_at_origin();
    h.settings.use_navigation_automation = false;

    h.start();

    assert!(h.step_count() > 0);
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(h.sim.go_to_calls.is_empty());
    assert!(!h.is_navigating());
}

// Arriving at the last waypoint ends the walk with GoArrow's message (the
// C# `HandleWaypointReached`).
#[test]
fn reaching_the_last_waypoint_arrives() {
    let mut h = outdoors_at_origin();
    h.start();
    let sequence = h.sequence();
    h.report(
        sequence,
        openac_plugin::navigation::GoToState::Arrived,
        0,
        0.0,
        None,
        1,
    );
    assert!(h.view().has_arrived);
    assert!(!h.is_navigating());
    assert_eq!(h.sim.stop_go_to_count, 1);
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m == "GoArrow: Arrived at 'Holtburg'.")
    );
    assert_eq!(h.view().leg_index, 1);
}

// ---- walks taken over by another walk ----

fn report_owned(
    sequence: i64,
    state: GoToState,
    revision: i64,
    owner: Option<&str>,
) -> openac_plugin::navigation::GoToReport {
    let mut report = navsim::report(sequence, state, 0, 0.0, None, revision);
    report.owner = owner.map(str::to_owned);
    report
}

const TAKEN_OVER: &str = "GoArrow: Navigation stopped: another walk took over.";

// A later walk of another plugin (or the player) ends ours: the client
// reports the newer walk, owned by someone else. The walk stops without
// stopping the other's walk.
#[test]
fn a_newer_walk_of_another_owner_stops_navigation() {
    let mut h = outdoors_at_origin();
    h.start();
    assert!(h.is_navigating());
    let ours = h.sequence();

    let other = report_owned(ours + 1, GoToState::Planning, 5, Some("someone.else"));
    h.sim.report = other.clone();
    h.event(Event::NavigationChanged(other));

    assert!(!h.is_navigating());
    assert_eq!(h.sim.stop_go_to_count, 0);
    assert!(
        h.sim.posted().iter().any(|m| m == TAKEN_OVER),
        "{:?}",
        h.sim.posted()
    );
    assert!(h.view().can_resume);
}

// The same seen on a tick: the latest report belongs to the player.
#[test]
fn a_newer_walk_of_the_player_seen_on_a_tick_stops_navigation() {
    let mut h = outdoors_at_origin();
    h.start();
    let ours = h.sequence();

    h.sim.report = report_owned(ours + 1, GoToState::Walking, 7, Some("player"));
    h.tick(0.015);

    assert!(!h.is_navigating());
    assert_eq!(h.sim.stop_go_to_count, 0);
    assert!(
        h.sim.posted().iter().any(|m| m == TAKEN_OVER),
        "{:?}",
        h.sim.posted()
    );
}

// Our own walk ended as Stopped (a stop, or a later walk, ended it).
#[test]
fn our_walk_reported_stopped_stops_navigation() {
    let mut h = outdoors_at_origin();
    h.start();
    let ours = h.sequence();

    h.report(
        ours,
        GoToState::Stopped,
        0,
        0.0,
        Some("a later walk ended it"),
        3,
    );

    assert!(!h.is_navigating());
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m == "GoArrow: Navigation stopped (Stopped)."),
        "{:?}",
        h.sim.posted()
    );
    assert_eq!(h.failure(), "a later walk ended it");
}

// ---- doors ----

const DOOR: u32 = 0x7000_0077;

fn closed_door() -> openac_plugin::world::WorldObject {
    use openac_plugin::world::{ObjectClass, WorldObject, capabilities};
    WorldObject {
        object_id: DOOR,
        name: "Door".to_owned(),
        object_class: ObjectClass::Door,
        capabilities: capabilities::INTERACTABLE,
        position: Some(pos(0, 1.0, 1.0, 0.0, true)),
        ..WorldObject::default()
    }
}

/// A walk blocked by a closed door: the navigator opens it.
fn blocked_by_door() -> Harness {
    let mut h = outdoors_at_origin();
    h.sim.objects.push(closed_door());
    h.start();
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    let mut blocked = navsim::report(h.sequence(), GoToState::Blocked, 0, 0.0, None, 2);
    blocked.blocked_by_object_id = DOOR;
    h.sim.report = blocked.clone();
    h.event(Event::NavigationChanged(blocked));
    assert_eq!(h.sim.activated, vec![DOOR]);
    assert!(!h.is_navigating());
    assert!(h.view().waiting_for_interaction);
    h
}

// This client ends no activation of a door: the leg starts again once the
// door reads open.
#[test]
fn a_walk_blocked_by_a_door_restarts_once_the_door_reads_open() {
    let mut h = blocked_by_door();
    h.tick(1.0);
    assert_eq!(h.sim.go_to_place_calls.len(), 1);

    let mut open = closed_door();
    open.is_door_open = true;
    h.sim.set_object(open);
    h.tick(0.015);

    assert_eq!(h.sim.go_to_place_calls.len(), 2);
    assert!(h.is_navigating());
    assert!(!h.view().waiting_for_interaction);
}

// An object change saying the door stands open restarts it at once.
#[test]
fn a_walk_blocked_by_a_door_restarts_when_the_door_is_seen_opening() {
    let mut h = blocked_by_door();
    let mut open = closed_door();
    open.is_door_open = true;
    h.sim.set_object(open.clone());
    h.updated(open);

    assert_eq!(h.sim.go_to_place_calls.len(), 2);
    assert!(h.is_navigating());
}

// The server answering the door's use restarts it too.
#[test]
fn a_walk_blocked_by_a_door_restarts_when_its_use_completes() {
    let mut h = blocked_by_door();
    h.item_use(1, DOOR, 0);

    assert_eq!(h.sim.go_to_place_calls.len(), 2);
    assert!(h.is_navigating());
}

// A refused use (a locked door) stops with GoArrow's failure line.
#[test]
fn a_refused_door_use_stops_navigation() {
    let mut h = blocked_by_door();
    h.item_use(1, DOOR, 0x0024);

    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert!(!h.is_navigating());
    assert!(!h.view().waiting_for_interaction);
    assert!(h.view().can_resume);
    assert!(
        h.sim.posted().iter().any(|m| m
            == "GoArrow: Interaction with 'Door' failed (error 0x0024). Use /go resume to retry."),
        "{:?}",
        h.sim.posted()
    );
}

// A door that stays shut stops the walk after the interaction timeout.
#[test]
fn a_door_that_does_not_open_stops_navigation_after_the_timeout() {
    let mut h = blocked_by_door();
    for _ in 0..14 {
        h.tick(1.0);
    }
    assert!(h.view().waiting_for_interaction);
    h.tick(1.0);

    assert!(!h.view().waiting_for_interaction);
    assert!(!h.is_navigating());
    assert!(h.view().can_resume);
    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m == "GoArrow: Door 'Door' did not open. Use /go resume to retry."),
        "{:?}",
        h.sim.posted()
    );
}
