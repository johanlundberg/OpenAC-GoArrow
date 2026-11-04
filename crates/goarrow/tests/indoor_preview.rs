//! The indoor path preview, from OpenAC-GoArrow's `IndoorPathPreviewTests`
//! (each test names the C# test it ports). The navigator runs the preview
//! on its tick while a window is attached; a plan arrives as an event.

mod navsim;

use goarrow::coords::NavPosition;
use goarrow::navigator::IndoorPreviewTarget;
use navsim::{Harness, portal, pos, snapshot};
use openac_plugin::navigation::NavigationPlanStatus;
use openac_plugin::world::{ObjectClass, WorldObject};

const CHAMBERS: &str = r#"<locations>
  <loc name="First Chamber" type="Dungeon" cellId="0x12340122" x="35" y="50" z="6" />
  <loc name="Second Chamber" type="Dungeon" cellId="0x12340123" x="55" y="50" z="6" />
</locations>"#;

fn indoor_destination() -> Harness {
    let mut h = Harness::new(CHAMBERS);
    h.sim.fake.window_attached = true;
    h.sim.snapshot = snapshot(pos(0x1234_0100, 10.0, 10.0, 0.0, false));
    h.settings.auto_navigate = false;
    h.go_to_named("First Chamber");
    h
}

fn summary(h: &Harness) -> String {
    h.view()
        .indoor_preview
        .as_ref()
        .map(|p| p.summary.clone())
        .unwrap_or_default()
}

// ShowsLengthAndCellAwareWaypointsWithoutStartingWalk
#[test]
fn shows_length_and_cell_aware_waypoints_without_starting_walk() {
    let mut h = indoor_destination();

    h.tick(0.015);
    assert_eq!(h.sim.previews.len(), 1);
    let place = h.sim.previews[0].place.clone().expect("a place preview");
    assert_eq!(place.cell_id, 0x1234_0122);
    assert_eq!(summary(&h), "Planning indoor path...");
    assert!(matches!(
        h.view().indoor_preview_target,
        Some(IndoorPreviewTarget::Point(_))
    ));
    assert!(h.sim.go_to_place_calls.is_empty());

    let here = h.sim.snapshot.position.clone();
    h.plan_answer(
        1,
        NavigationPlanStatus::Routed,
        vec![here, pos(0x1234_0122, 10.01, 10.02, 0.025, false)],
        37.5,
    );
    h.tick(0.015);

    assert_eq!(summary(&h), "Indoor path: 37.5 m, 2 waypoints");
    let preview = h.view().indoor_preview.clone().unwrap();
    assert_eq!(preview.waypoints.len(), 2);
    assert!(
        preview.waypoints[1].contains("Cell 0x12340122"),
        "{}",
        preview.waypoints[1]
    );
    assert!(
        preview.waypoints[1].contains("Z 6 m"),
        "{}",
        preview.waypoints[1]
    );
    assert_eq!(preview.points.len(), 2);
    assert_eq!(preview.points[1].0, 0x1234_0122);
    assert!(h.sim.go_to_calls.is_empty());
    assert!(h.sim.go_to_place_calls.is_empty());
}

// IgnoresOldPlanAfterTargetChangeAndClearsOnDungeonExit
#[test]
fn ignores_old_plan_after_target_change_and_clears_on_dungeon_exit() {
    let mut h = indoor_destination();

    h.tick(0.015);
    h.go_to_named("Second Chamber");
    h.tick(0.015);
    assert_eq!(h.sim.previews.len(), 2);
    h.plan_answer(1, NavigationPlanStatus::Routed, Vec::new(), 11.0);
    h.tick(0.015);
    assert_eq!(summary(&h), "Planning indoor path...");

    h.plan_answer(2, NavigationPlanStatus::Routed, Vec::new(), 52.0);
    h.tick(0.015);
    assert!(
        summary(&h).starts_with("Indoor path: 52 m"),
        "{}",
        summary(&h)
    );

    h.sim.snapshot.position.is_outdoor = true;
    h.tick(0.015);
    assert!(h.view().indoor_preview.is_none());
}

// SelectedIndoorObjectUsesObjectPreview
#[test]
fn selected_indoor_object_uses_object_preview() {
    let mut h = indoor_destination();
    let chest = WorldObject {
        object_id: 42,
        name: "Chest".to_owned(),
        object_class: ObjectClass::Npc,
        position: Some(pos(0x1234_0122, 10.01, 10.01, 0.0, false)),
        ..WorldObject::default()
    };
    h.go_to_object(&chest);

    h.tick(0.015);
    h.plan_answer(1, NavigationPlanStatus::Routed, Vec::new(), 12.0);
    h.tick(0.015);

    assert_eq!(h.sim.previews.len(), 1);
    assert_eq!(h.sim.previews[0].object_id, 42);
    assert!(h.sim.previews[0].place.is_none());
    assert!(
        summary(&h).starts_with("Indoor path: 12 m"),
        "{}",
        summary(&h)
    );
    assert_eq!(
        h.view().indoor_preview_target,
        Some(IndoorPreviewTarget::Object(42))
    );
    h.destination.update_object(
        42,
        "Chest",
        Some(NavPosition {
            cell_id: 0x1234_0122,
            east_west: 10.011,
            north_south: 10.01,
            elevation: 0.0,
            heading_degrees: 90.0,
            is_outdoor: false,
        }),
    );
    h.tick(0.015);
    assert_eq!(h.sim.previews.len(), 1);
}

// NamedLivePortalCanBePreviewedWithoutChangingNavigatorState
#[test]
fn named_live_portal_can_be_previewed_without_changing_navigator_state() {
    let mut h = Harness::new(
        "<locations><loc name='Sawato Portal' type='TownPortal' NS='-28.7' EW='59.3' /></locations>",
    );
    h.sim.fake.window_attached = true;
    h.sim.snapshot = snapshot(pos(0x1234_0100, 10.0, 10.0, 0.0, false));
    h.sim.objects.push(portal(
        71,
        "Portal to Sawato",
        pos(0x1234_0122, 10.01, 10.0, 0.0, false),
    ));
    h.settings.auto_navigate = false;
    h.go_to_named("Sawato Portal");

    h.tick(0.015);
    h.plan_answer(1, NavigationPlanStatus::Routed, Vec::new(), 45.0);
    h.tick(0.015);

    assert_eq!(h.sim.previews[0].object_id, 71);
    assert!(
        summary(&h).starts_with("Indoor path: 45 m"),
        "{}",
        summary(&h)
    );
    assert!(!h.view().has_indoor_target);
    assert!(h.sim.go_to_calls.is_empty());
}

// The preview runs only while a window is attached (the C# preview's
// `HasUi` check).
#[test]
fn no_preview_without_a_window() {
    let mut h = indoor_destination();
    h.sim.fake.window_attached = false;
    h.tick(0.015);
    assert!(h.sim.previews.is_empty());
    assert!(h.view().indoor_preview.is_none());
    h.sim.fake.window_attached = true;
    h.tick(0.015);
    assert_eq!(h.sim.previews.len(), 1);
    assert_eq!(summary(&h), "Planning indoor path...");
}

// A preview is asked for again once the character has moved on, but not
// within 3 seconds of the last.
#[test]
fn previews_are_asked_for_at_most_every_three_seconds() {
    let mut h = indoor_destination();
    h.tick(0.015);
    assert_eq!(h.sim.previews.len(), 1);
    let here = h.sim.snapshot.position.clone();
    h.plan_answer(1, NavigationPlanStatus::Routed, vec![here], 30.0);
    h.tick(0.015);
    assert!(
        summary(&h).starts_with("Indoor path: 30 m"),
        "{}",
        summary(&h)
    );

    // Into another cell: moved on, but too soon.
    h.sim.move_to(pos(0x1234_0101, 10.0, 10.0, 0.0, false));
    h.tick(1.0);
    h.tick(1.0);
    assert_eq!(h.sim.previews.len(), 1);
    h.tick(0.97);
    assert_eq!(h.sim.previews.len(), 1);
    h.tick(0.1);
    assert_eq!(h.sim.previews.len(), 2);
    assert_eq!(summary(&h), "Planning indoor path...");
}
