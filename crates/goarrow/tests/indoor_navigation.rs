//! Walking to indoor targets, from OpenAC-GoArrow's
//! `IndoorNavigationTests` (each test names the C# test it ports). The two
//! C# tests of marking a place and loading a data file
//! (`MarkedIndoorPointRemainsNamedAfterLocationDataReloadAndPluginRestart`,
//! `LoadedLocationCanBeSelectedByNameAndWalkedFromPanel`) are the plugin's.

mod navsim;

use navsim::{Harness, portal, pos, snapshot};
use openac_plugin::navigation::GoToState;
use openac_plugin::world::{ObjectClass, WorldObject};

fn logged(h: &Harness, line: &str) -> bool {
    h.sim.fake.logs.iter().any(|(_, l)| l == line)
}

const PAUSED_INDOORS: &str =
    "GoArrow: Outdoor route planning is paused while indoors or in portal space.";
const OUTSIDE_DUNGEON: &str = "GoArrow: Indoor destination is outside the current dungeon.";

fn host_at(cell_id: u32, xml: &str) -> Harness {
    let mut h = Harness::new(xml);
    h.sim.snapshot = snapshot(pos(cell_id, 10.0, 10.0, 0.0, false));
    h
}

fn object_at(cell_id: u32, is_outdoor: bool) -> WorldObject {
    WorldObject {
        object_id: 42,
        name: "Dungeon Target".to_owned(),
        object_class: ObjectClass::Npc,
        position: Some(pos(cell_id, 10.01, 10.01, 0.0, is_outdoor)),
        ..WorldObject::default()
    }
}

fn indoor_portal(id: u32, name: &str, cell_id: u32, east_west: f64) -> WorldObject {
    portal(id, name, pos(cell_id, east_west, 10.0, 0.0, false))
}

// NamedSawatoLocationWalksToSawatoPortalInsideTownNetwork (at the
// navigator: the C# test went through the plugin's panel)
#[test]
fn named_sawato_location_walks_to_sawato_portal_inside_town_network() {
    let mut h = host_at(
        0x1234_0100,
        "<locations><loc name='Sawato' type='Town' NS='-28.7' EW='59.3' /></locations>",
    );
    h.sim
        .objects
        .push(indoor_portal(41, "Holtburg Portal", 0x1234_0111, 10.001));
    h.sim
        .objects
        .push(indoor_portal(42, "Portal to Sawato", 0x1234_0122, 10.01));
    h.sim
        .objects
        .push(indoor_portal(43, "Sawato Portal", 0x5678_0122, 10.0001));
    h.go_to_named("Sawato");
    h.enable();

    h.start();

    assert_eq!(h.go_to_objects(), vec![42]);
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(h.view().is_planning_path);
}

// NamedLocationWithoutMatchingLocalPortalDoesNotUseOutdoorCoordinateIndoors
#[test]
fn named_location_without_matching_local_portal_does_not_use_outdoor_coordinate_indoors() {
    let mut h = host_at(
        0x1234_0100,
        "<locations><loc name='Sawato Portal' type='TownPortal' NS='-28.7' EW='59.3' /></locations>",
    );
    h.sim
        .objects
        .push(indoor_portal(41, "Holtburg Portal", 0x1234_0111, 10.001));
    h.settings.auto_navigate = false;
    h.go_to_named("Sawato Portal");

    h.start();

    assert!(h.sim.go_to_calls.is_empty());
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(logged(&h, PAUSED_INDOORS), "{:?}", h.sim.fake.logs);
}

// NamedLocationWithIndoorCellWalksAcrossCellsOnItsFloor
#[test]
fn named_location_with_indoor_cell_walks_across_cells_on_its_floor() {
    let mut h = host_at(
        0x1234_0100,
        r#"<locations>
          <loc name="Dungeon Chest" type="Dungeon" cellId="0x12340122" x="35" y="50" z="6" />
        </locations>"#,
    );
    h.settings.auto_navigate = false;
    h.go_to_named("Dungeon Chest");

    h.start();

    assert_eq!(h.sim.go_to_place_calls.len(), 1);
    let walk = &h.sim.go_to_place_calls[0].0;
    assert_eq!(walk.cell_id, 0x1234_0122);
    let local = walk.landblock_local();
    assert!((local[0] - 35.0).abs() < 0.001, "{local:?}");
    assert!((local[1] - 50.0).abs() < 0.001, "{local:?}");
    assert!((local[2] - 6.0).abs() < 0.001, "{local:?}");
    assert!(h.is_navigating());
    assert!(h.sim.go_to_calls.is_empty());
}

// NamedIndoorLocationInAnotherDungeonIsNotMappedToOutdoorCoordinates
#[test]
fn named_indoor_location_in_another_dungeon_is_not_mapped_to_outdoor_coordinates() {
    let mut h = host_at(
        0x1234_0100,
        r#"<locations><loc name="Other Dungeon Chest" NS="10" EW="10"
          cellId="0x56780122" x="35" y="50" z="6" /></locations>"#,
    );
    h.settings.auto_navigate = false;
    h.go_to_named("Other Dungeon Chest");

    h.start();

    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(!h.is_navigating());
    assert!(logged(&h, OUTSIDE_DUNGEON), "{:?}", h.sim.fake.logs);
}

// SelectedObjectInSameDungeonUsesHostObjectPathAndArrives
#[test]
fn selected_object_in_same_dungeon_uses_host_object_path_and_arrives() {
    let mut h = host_at(0x1234_0100, "<locations />");
    h.settings.auto_navigate = false;
    h.go_to_object(&object_at(0x1234_0122, false));
    h.enable();
    h.start();

    assert!(h.is_navigating());
    assert_eq!(h.go_to_objects(), vec![42]);
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(h.destination.current_route().is_none());

    h.report(1, GoToState::Arrived, 42, 0.0, None, 1);

    assert!(h.view().has_arrived);
    assert!(!h.is_navigating());
    assert_eq!(h.go_to_objects(), vec![42]);
}

// IndoorObjectOutsideCurrentDungeonOrOutdoorsIsNotRouted
fn indoor_object_not_routed(target_cell: u32, target_outdoor: bool, warning: &str) {
    let mut h = host_at(0x1234_0100, "<locations />");
    h.settings.auto_navigate = false;
    h.go_to_object(&object_at(target_cell, target_outdoor));

    h.start();

    assert!(!h.is_navigating());
    assert!(h.sim.go_to_calls.is_empty());
    assert!(h.sim.go_to_place_calls.is_empty());
    assert!(logged(&h, warning), "{:?}", h.sim.fake.logs);
}

#[test]
fn indoor_object_outside_current_dungeon_is_not_routed() {
    indoor_object_not_routed(0x5678_0122, false, OUTSIDE_DUNGEON);
}

#[test]
fn outdoor_object_seen_from_indoors_is_not_routed() {
    indoor_object_not_routed(0x1234_0122, true, PAUSED_INDOORS);
}

// StoppingIndoorWalkCancelsHostRequest
#[test]
fn stopping_indoor_walk_cancels_host_request() {
    let mut h = host_at(0x1234_0100, "<locations />");
    h.go_to_object(&object_at(0x1234_0122, false));

    h.start();
    h.stop();

    assert!(!h.is_navigating());
    assert_eq!(h.sim.stop_go_to_count, 1);

    h.resume();

    assert!(h.is_navigating());
    assert_eq!(h.sim.go_to_calls.len(), 2);
}
