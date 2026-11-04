//! A portal that lets out a little off its listed arrival (more than
//! 0.5 map units, up to 5): the navigator uses where it landed for the
//! rest of the session and plans again from there; and the loop guard,
//! which stops a navigation whose re-planned route would take a portal or
//! recall it has already taken.

mod navsim;

use goarrow::coords::Coordinates;
use goarrow::route::RouteStepKind;
use navsim::{Harness, portal, pos, snapshot, with_destination};
use openac_plugin::navigation::GoToState;
use openac_plugin::world::Position;

const PORTAL: u8 = 2;

fn outdoor(east_west: f64, north_south: f64) -> Position {
    pos(0, east_west, north_south, 0.0, true)
}

/// "Near Portal" (0N 1E) is listed arriving at 0N 60E, a map unit short
/// of End (0N 61E); "Return Portal", at 0N 57.5E, leads back beside it.
const NEAR_PORTAL: &str = r#"<atlas>
  <location><id>7</id><latitude>0</latitude><longitude>1</longitude><name>Near Portal</name>
    <type>Wilderness Portal</type><arrival_latitude>0</arrival_latitude><arrival_longitude>60</arrival_longitude></location>
  <location><id>8</id><latitude>0</latitude><longitude>57.5</longitude><name>Return Portal</name>
    <type>Wilderness Portal</type><arrival_latitude>0</arrival_latitude><arrival_longitude>1.5</arrival_longitude></location>
  <location><id>9</id><latitude>0</latitude><longitude>61</longitude><name>End</name><type>Town</type></location>
</atlas>"#;

/// Uses "Near Portal" from 0N 0E and lands at `landed_ew` east.
fn through_near_portal(landed_ew: f64) -> Harness {
    let mut h = Harness::new(NEAR_PORTAL);
    h.sim
        .objects
        .push(portal(42, "Near Portal", outdoor(1.0, 0.0)));
    h.sim.snapshot = snapshot(outdoor(0.0, 0.0));
    h.go_to_named("End");
    h.enable();
    h.start();
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert!(h.view().waiting_for_interaction);
    assert_eq!(h.vias()[0], "Near Portal");
    h.sim.move_to(outdoor(landed_ew, 0.0));
    h.with(|nav, cx| nav.resume_after_interaction(cx, true));
    h
}

fn near_portal_arrival(h: &Harness) -> Coordinates {
    h.finder
        .database()
        .find_location("Near Portal")
        .unwrap()
        .exit_coords()
}

#[test]
fn a_portal_landing_two_units_off_its_listing_is_learned_and_the_route_planned_again() {
    let mut h = through_near_portal(58.0);
    // The portal's arrival is where it landed, for this session.
    assert_eq!(near_portal_arrival(&h), Coordinates::new(0.0, 58.0));
    // The route is planned again from there: one walk to End.
    let steps = h.destination.current_route().unwrap().steps().to_vec();
    assert!(
        steps.iter().all(|s| s.kind == RouteStepKind::Travel),
        "{:?}",
        h.vias()
    );
    assert!((steps[0].from.coords.ew - 58.0).abs() < 1e-9, "{steps:?}");
    assert!(h.is_navigating());
    assert_eq!(h.sim.go_to_place_calls.last().unwrap().0.east_west, 61.0);
    // Said once, and kept on request.
    let posted = h.sim.posted();
    assert!(
        posted
            .iter()
            .any(|m| m.contains("'Near Portal' arrived at 0.0N, 58.0E")),
        "{posted:?}"
    );
    let pending = h.view().pending_arrival.clone().expect("a pending arrival");
    assert_eq!((pending.id, pending.arrival.as_str()), (7, "0.0N, 58.0E"));
    // Not avoided: it is still the way from the start.
    h.stop();
    h.sim.move_to(outdoor(0.0, 0.0));
    assert!(h.plan());
    assert_eq!(h.vias()[1], "Near Portal");

    // Under half a unit off, the listing stands.
    let h = through_near_portal(59.6);
    assert_eq!(near_portal_arrival(&h), Coordinates::new(0.0, 60.0));
    assert!(h.view().pending_arrival.is_none());
    assert!(
        !h.sim.posted().iter().any(|m| m.contains("arrived at")),
        "{:?}",
        h.sim.posted()
    );
}

/// The Town Network: in at "Town Network Portal(Shoushi)" (0N 2E), out at
/// "Town Network (E R 5) to Sawato", listed arriving at 80N 80E;
/// "Town Network Portal(Sawato)", at 80N `sawato_entrance_ew`, leads back
/// in. Sawato is at 80N 80.57E.
fn town_network(sawato_entrance_ew: f64) -> String {
    let place = |id: u32, name: &str, kind: &str, ns: f64, ew: f64, exit: Option<(f64, f64)>| {
        let exit = exit.map_or(String::new(), |(ns, ew)| {
            format!(
                "<arrival_latitude>{}</arrival_latitude><arrival_longitude>{ew}</arrival_longitude>",
                -ns
            )
        });
        format!(
            "<location><id>{id}</id><latitude>{}</latitude><longitude>{ew}</longitude><name>{name}</name><type>{kind}</type>{exit}</location>",
            -ns
        )
    };
    [
        "<atlas>".to_owned(),
        place(1, "Start", "Town", 0.0, 1.0, None),
        place(
            2,
            "Town Network Portal(Shoushi)",
            "Town Portal",
            0.0,
            2.0,
            Some((40.0, 40.0)),
        ),
        place(
            3,
            "Town Network (E R 5) to Sawato",
            "Underground Portal",
            40.0,
            40.02,
            Some((80.0, 80.0)),
        ),
        place(
            4,
            "Town Network Portal(Sawato)",
            "Town Portal",
            80.0,
            sawato_entrance_ew,
            Some((40.0, 40.0)),
        ),
        place(5, "Sawato", "Town", 80.0, 80.57, None),
        "</atlas>".to_owned(),
    ]
    .concat()
}

/// Into the Town Network and walked into its Sawato exit before the walk
/// reports arrival, landing outdoors at 80N `landed_ew`.
fn out_of_the_town_network(xml: &str, landed_ew: f64) -> Harness {
    let mut h = Harness::new(xml);
    h.sim
        .objects
        .push(portal(42, "Portal to Town Network", outdoor(2.0, 0.0)));
    h.sim.snapshot = snapshot(outdoor(1.0, 0.0));
    h.go_to_named("Sawato");
    h.enable();
    h.start();
    h.report(1, GoToState::Arrived, 0, 0.0, None, 1);
    assert_eq!(h.sim.activated, vec![42]);
    h.sim.move_to(pos(0x1234_0100, 40.0, 40.0, 0.0, false));
    h.transition(0, 1, 0x1234_0100, PORTAL);
    assert_eq!(h.first_kind(), Some(RouteStepKind::Travel));
    h.sim.objects.push(with_destination(
        portal(
            43,
            "Sawato Portal",
            pos(0x1234_0122, 40.02, 40.0, 0.0, false),
        ),
        "Sawato (80N, 80E)",
    ));
    h.tick(0.6);
    assert_eq!(h.go_to_objects(), vec![43]);
    // The character walks into the portal before the walk arrives.
    h.sim.snapshot.is_portal_space = true;
    h.tick(0.1);
    assert_eq!(h.failure(), "Waiting for portal transition.");
    h.sim.snapshot.is_portal_space = false;
    h.sim.move_to(outdoor(landed_ew, 80.0));
    h.tick(0.1);
    h
}

#[test]
fn a_town_network_exit_landing_off_its_listing_does_not_send_the_route_back_in() {
    // Lands 1.8 units west of the listing; the way back in is 0.2 away,
    // and on paper back in and out again is shorter than the walk.
    let h = out_of_the_town_network(&town_network(78.4), 78.2);
    let arrival = h
        .finder
        .database()
        .find_location("Town Network (E R 5) to Sawato")
        .unwrap()
        .exit_coords();
    assert!(
        arrival.distance_to(Coordinates::new(80.0, 78.2)) < 1e-9,
        "{arrival}"
    );
    assert!(
        !h.vias().iter().any(|v| v.starts_with("Town Network")),
        "{:?}",
        h.vias()
    );
    assert!(h.is_navigating(), "{}", h.failure());
    let walk = h.sim.go_to_place_calls.last().unwrap().0.east_west;
    assert!((walk - 80.57).abs() < 0.001, "{walk}");
}

#[test]
fn a_route_that_would_take_a_portal_again_stops_the_navigation() {
    // Lands 0.4 units off (not learned); back in and out again is still
    // shorter on paper than the walk, which would loop.
    let mut h = out_of_the_town_network(&town_network(79.5), 79.6);
    let line = "Navigation stopped: the route would take 'Town Network (E R 5) to Sawato' again.";
    assert_eq!(h.failure(), line);
    assert!(!h.is_navigating());
    assert!(!h.view().waiting_for_interaction);
    assert!(
        h.sim
            .posted()
            .iter()
            .any(|m| m == &format!("GoArrow: {line}")),
        "{:?}",
        h.sim.posted()
    );
    // Stopped: ticks do not walk on.
    let walks = h.sim.go_to_place_calls.len();
    h.tick(1.0);
    assert_eq!(h.sim.go_to_place_calls.len(), walks);
    // A new Go is a new navigation: it may take the portals again.
    h.sim.move_to(outdoor(1.0, 0.0));
    h.start();
    assert!(h.is_navigating(), "{}", h.failure());
}
