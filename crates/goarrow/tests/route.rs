//! Routes and their steps, from OpenAC-GoArrow's `RouteTests` and
//! `TravelDistanceTests.RouteInstructionsNameOnlyTheActionAndTarget`.

use std::rc::Rc;

use goarrow::location::Location;
use goarrow::route::{Route, RouteStep, RouteStepKind};

fn loc(name: &str, ns: f64, ew: f64) -> Rc<Location> {
    Rc::new(Location::new(name, ns, ew))
}

// RouteTests.Route_Create_HasCorrectDestination
#[test]
fn has_its_destination() {
    assert_eq!(Route::new("Holtburg").destination, "Holtburg");
}

// RouteTests.Route_AddTravelStep_IncreasesStepCount / _CalculatesDistance
#[test]
fn travel_steps_count_and_measure() {
    let mut route = Route::new("Dest");
    route.add_travel_step(loc("Start", 0.0, 0.0), loc("Dest", 3.0, 4.0), "Walk");
    assert_eq!(route.step_count(), 1);
    assert!((route.total_distance() - 5.0).abs() < 1e-12);
    assert_eq!(route.steps()[0].distance, route.total_distance());
}

// RouteTests.Route_AddPortalStep_CountsPortal
#[test]
fn portal_steps_count() {
    let mut route = Route::new("Town");
    route.add_portal_step(
        loc("Outside", 10.0, 10.0),
        loc("Town", 10.0, 11.0),
        "Town Portal",
    );
    assert_eq!(route.portal_count(), 1);
    assert_eq!(route.step_count(), 1);
    assert_eq!(route.total_distance(), 0.0);
}

// RouteTests.Route_Clear_ResetsAll
#[test]
fn clear_resets_all() {
    let mut route = Route::new("Dest");
    route.add_travel_step(loc("A", 0.0, 0.0), loc("B", 1.0, 1.0), "Walk");
    route.add_portal_step(loc("A", 0.0, 0.0), loc("B", 1.0, 1.0), "P");
    route.clear();
    assert_eq!(route.step_count(), 0);
    assert_eq!(route.total_distance(), 0.0);
    assert_eq!(route.portal_count(), 0);
}

// RouteTests.Route_Steps_AreReadOnly
#[test]
fn new_route_has_no_steps() {
    assert!(Route::new("Dest").steps().is_empty());
}

// RouteTests.RouteStep_ToString_Travel / _Portal / _Recall
#[test]
fn step_text() {
    let step = RouteStep::new(
        RouteStepKind::Travel,
        loc("A", 0.0, 0.0),
        loc("B", 1.0, 1.0),
        1.414,
        "Walk",
    );
    assert_eq!(step.to_string(), "Walk: B (339 m)");
    let step = RouteStep::new(
        RouteStepKind::Portal,
        loc("A", 0.0, 0.0),
        loc("Town", 10.0, 10.0),
        0.0,
        "Portal Device",
    );
    assert_eq!(step.to_string(), "Portal: Portal Device");
    let step = RouteStep::new(
        RouteStepKind::Recall,
        loc("A", 0.0, 0.0),
        loc("LS", 50.0, 50.0),
        0.0,
        "Primary Portal Recall",
    );
    assert_eq!(step.to_string(), "Recall: Primary Portal Recall");
    let step = RouteStep::new(
        RouteStepKind::Travel,
        loc("A", 0.0, 0.0),
        loc("B", 0.0, 0.0),
        0.0,
        "arrived",
    );
    assert_eq!(step.to_string(), "Arrived: B");
    let step = RouteStep::new(
        RouteStepKind::Portal,
        loc("A", 0.0, 0.0),
        loc("Town", 1.0, 1.0),
        0.0,
        "  ",
    );
    assert_eq!(step.to_string(), "Portal: Town");
    let step = RouteStep::new(
        RouteStepKind::Recall,
        loc("A", 0.0, 0.0),
        loc("LS", 1.0, 1.0),
        0.0,
        "",
    );
    assert_eq!(step.to_string(), "Recall: LS");
}

// RouteStep defaults.
#[test]
fn step_defaults() {
    let from = loc("A", 0.0, 0.0);
    let step = RouteStep::new(
        RouteStepKind::Portal,
        from.clone(),
        loc("B", 1.0, 1.0),
        0.0,
        "P",
    );
    assert!(Rc::ptr_eq(&step.graph_from, &from));
    assert_eq!(step.object_id, 0);
    assert_eq!(step.interaction_timeout.as_secs(), 15);
    assert_eq!(step.max_retries, 1);
}

// TravelDistanceTests.RouteInstructionsNameOnlyTheActionAndTarget
#[test]
fn route_instructions_name_only_the_action_and_target() {
    let start = loc("Current Location", 0.0, 0.0);
    let shoushi = loc("Town Network Portal(Shoushi)", 0.0, 0.13);
    let shoushi_arrival = loc("Town Network Portal(Shoushi) arrival (9299)", 40.0, 40.0);
    let sawato_portal = loc("Town Network (E R 5) to Sawato", 40.0, 40.0);
    let sawato_arrival = loc("Town Network (E R 5) to Sawato arrival (9350)", 80.0, 80.0);
    let sawato = loc("Sawato", 80.0, 80.57);
    let mut route = Route::new("Sawato");
    route.add_travel_step(start, shoushi.clone(), "Walk to start");
    route.add_portal_step(shoushi.clone(), shoushi_arrival.clone(), &shoushi.name);
    route.add_travel_step(shoushi_arrival, sawato_portal.clone(), "Walk");
    route.add_portal_step(
        sawato_portal.clone(),
        sawato_arrival.clone(),
        &sawato_portal.name,
    );
    route.add_travel_step(sawato_arrival, sawato, "Walk");
    let texts: Vec<String> = route.steps().iter().map(ToString::to_string).collect();
    assert_eq!(
        texts,
        [
            "Walk: Town Network Portal(Shoushi) (31 m)",
            "Portal: Town Network Portal(Shoushi)",
            "Walk: Town Network (E R 5) to Sawato (0 m)",
            "Portal: Town Network (E R 5) to Sawato",
            "Walk: Sawato (137 m)",
        ]
    );
    assert_eq!(route.to_string(), "Sawato: 5 steps, 168 m, 2 portals");
}

// Route.PrependStep / SetFirstStepOrigin / RemoveStep / MoveStep
#[test]
fn editing_keeps_the_totals() {
    let mut route = Route::new("C");
    let a = loc("A", 0.0, 0.0);
    let b = loc("B", 0.0, 1.0);
    let c = loc("C", 0.0, 3.0);
    route.add_travel_step(a.clone(), b.clone(), "Walk");
    route.add_portal_step(b.clone(), c.clone(), "P");
    route.prepend_step(RouteStep::new(
        RouteStepKind::Travel,
        loc("O", 0.0, -1.0),
        a.clone(),
        1.0,
        "Walk to start",
    ));
    assert_eq!(route.step_count(), 3);
    assert!((route.total_distance() - 2.0).abs() < 1e-12);

    let origin = loc("Origin", 0.0, -1.0);
    route.set_first_step_origin(origin.clone());
    assert!(Rc::ptr_eq(&route.steps()[0].from, &origin));
    assert_eq!(route.steps()[0].graph_from.name, "O");
    assert_eq!(route.steps()[0].via, "Walk to start");

    assert!(route.move_step(2, 0));
    assert_eq!(route.steps()[0].kind, RouteStepKind::Portal);
    assert!(!route.move_step(0, 3));
    assert!(route.remove_step(0));
    assert_eq!(route.portal_count(), 0);
    assert!((route.total_distance() - 2.0).abs() < 1e-12);
    assert!(!route.remove_step(5));
}
