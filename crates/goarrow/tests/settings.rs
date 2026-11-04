//! The settings file (`GoArrowSettings.cs`; GoArrow's `UiStartupTests`):
//! one JSON document with GoArrow's member names and defaults, repaired as
//! GoArrow repaired it when read.

mod common;

use common::{Harness, Sim};
use goarrow::GoArrow;
use goarrow::settings::{CharacterRecalls, SETTINGS_KEY, Settings};

// UiStartupTests.NewInstallKeepsVisibleUiDefaults
#[test]
fn a_new_install_keeps_the_visible_defaults() {
    let h = Harness::new(|_| {});
    let s = h.plugin.settings();
    assert!(s.panel_visible && s.hud_visible && s.toolbar_visible);
    assert!(!s.map_visible, "the map starts hidden (GA-19)");
    assert!(s.dungeon_map_visible && s.show_distance && s.show_bearing);
    assert!(s.recalculate_route && s.use_navigation_automation && !s.auto_navigate);
    assert_eq!(s.external_data_url, "");
    assert_eq!(s.dungeon_map_url, "");
    assert_eq!((s.arrow_offset_x, s.arrow_offset_y), (-70.0, 120.0));
    assert_eq!((s.dungeon_offset_x, s.dungeon_offset_y), (25.0, 95.0));
    assert_eq!(s.arrival_distance, 0.5);
    assert_eq!(s.atlas_cache_max_age_days, 30);
    assert_eq!(s.route_cost_profile, 0);
    assert!(s.route_line_visible);
    assert_eq!(s.map_zoom, 1.0);
}

// UiStartupTests.SaveKeepsSettingsInOneJsonFileAndPreservesCachedData
#[test]
fn settings_round_trip_through_one_json_file_with_csharp_member_names() {
    let mut settings = Settings {
        destination_name: "New destination".to_owned(),
        show_distance: false,
        ..Settings::default()
    };
    settings.recalls_by_character.insert(
        "character/world".to_owned(),
        CharacterRecalls {
            lifestone: "1,2".to_owned(),
            ..CharacterRecalls::default()
        },
    );
    settings.favorite_destinations.push("Holtburg".to_owned());
    let json = settings.to_json();
    for member in [
        "\"DestinationName\"",
        "\"ShowDistance\"",
        "\"RecallsByCharacter\"",
        "\"Lifestone\"",
        "\"FavoriteDestinations\"",
        "\"OverlayPositionVersion\": 1",
        "\"VisibilitySettingsVersion\": 1",
        "\"RouteCostProfile\": 0",
    ] {
        assert!(json.contains(member), "{member} in {json}");
    }
    let read = Settings::from_json(&json).unwrap();
    assert_eq!(read.destination_name, "New destination");
    assert!(!read.show_distance);
    assert_eq!(
        read.recalls_by_character["character/world"].lifestone,
        "1,2"
    );
    assert_eq!(read.favorite_destinations, ["Holtburg"]);
}

// UiStartupTests.PositionsSavedByOldDragHandlerReturnToVisibleDefaults
#[test]
fn positions_from_before_version_one_return_to_the_defaults() {
    let read = Settings::from_json(
        r#"{ "ArrowOffsetX": 5000, "ToolbarOffsetY": 5000, "DungeonOffsetX": 5000,
             "OverlayPositionVersion": 0, "VisibilitySettingsVersion": 1 }"#,
    )
    .unwrap();
    assert_eq!(read.arrow_offset_x, -70.0);
    assert_eq!(read.toolbar_offset_y, 215.0);
    assert_eq!(read.dungeon_offset_x, 25.0);
    let kept =
        Settings::from_json(r#"{ "ArrowOffsetX": 12, "OverlayPositionVersion": 1 }"#).unwrap();
    assert_eq!(kept.arrow_offset_x, 12.0);
}

// UiStartupTests.PreviouslySavedAllHiddenProfileRecoversThePanel
#[test]
fn a_profile_that_hid_everything_shows_the_window_and_arrow_again() {
    let read = Settings::from_json(
        r#"{ "PanelVisible": false, "HudVisible": false, "ToolbarVisible": false,
             "MapVisible": false, "ShowDistance": false, "ShowBearing": false,
             "RecalculateRoute": false, "UseNavigationAutomation": false }"#,
    )
    .unwrap();
    assert!(read.panel_visible && read.hud_visible && !read.toolbar_visible);
    assert!(read.show_distance && read.show_bearing);
    assert!(read.recalculate_route && read.use_navigation_automation);
}

// UiStartupTests.PanelOnlyRepairAlsoRestoresPreviouslyHiddenHud
#[test]
fn the_panel_only_repair_also_shows_the_arrow() {
    let read = Settings::from_json(
        r#"{ "PanelVisible": true, "HudVisible": false, "ToolbarVisible": false,
             "MapVisible": false }"#,
    )
    .unwrap();
    assert!(read.hud_visible && !read.toolbar_visible);
    // Saved by this version: no repair.
    let read = Settings::from_json(
        r#"{ "HudVisible": false, "ToolbarVisible": false, "MapVisible": false,
             "VisibilitySettingsVersion": 1 }"#,
    )
    .unwrap();
    assert!(!read.hud_visible);
}

#[test]
fn unusable_numbers_and_nulls_read_as_the_defaults() {
    let read = Settings::from_json(
        r#"{ "HudScale": 0, "MapWidth": -1, "MapHeight": 0, "ArrivalDistance": -2,
             "MaxNavigationRetries": -3, "InteractionTimeoutSeconds": 0,
             "AtlasCacheMaxAgeDays": -1, "RouteCostProfile": 7, "MapZoom": 40,
             "ExternalDataUrl": null, "DungeonMapUrl": null, "FavoriteDestinations": null,
             "RecallsByCharacter": null, "DestinationName": null }"#,
    )
    .unwrap();
    assert_eq!(read.hud_scale, 1.0);
    assert_eq!((read.map_width, read.map_height), (1000.0, 1000.0));
    assert_eq!(read.arrival_distance, 0.5);
    assert_eq!(read.max_navigation_retries, 0);
    assert_eq!(read.interaction_timeout_seconds, 15.0);
    assert_eq!(read.atlas_cache_max_age_days, 0);
    assert_eq!(read.route_cost_profile, 0);
    assert_eq!(read.map_zoom, 16.0);
    assert_eq!(read.external_data_url, "");
    assert!(read.favorite_destinations.is_empty() && read.recalls_by_character.is_empty());
}

#[test]
fn a_file_that_does_not_read_gives_the_defaults_and_a_warning() {
    let h = Harness::new(|sim| {
        sim.storage
            .insert(SETTINGS_KEY.to_owned(), "{ not json".to_owned());
    });
    assert!(h.plugin.settings().hud_visible);
    assert!(
        h.warnings().iter().any(|w| w.contains("settings.json")),
        "{:?}",
        h.warnings()
    );
}

/// Enabled over the Atlas cache with `destination` saved, not yet ticked:
/// the cache is still being read.
fn loading_with_saved_destination(destination: &str) -> Harness {
    let mut sim = Sim::new();
    sim.storage.insert(
        "data/warcry-atlas.xml".to_owned(),
        common::ATLAS_XML.to_owned(),
    );
    sim.storage.insert(
        SETTINGS_KEY.to_owned(),
        format!(
            r#"{{ "DestinationName": "{destination}", "VisibilitySettingsVersion": 1,
                 "OverlayPositionVersion": 1 }}"#
        ),
    );
    let mut h = Harness {
        sim,
        plugin: GoArrow::default(),
    };
    h.enable();
    assert!(!h.plugin.is_loaded());
    h
}

// GoArrowPlugin.Startup restores DestinationName once the data is read;
// only the destination setters write it (GoArrowDestination.cs).
#[test]
fn the_saved_destination_comes_back_by_name_once_the_data_is_read() {
    let mut h = loading_with_saved_destination("Rithwic");
    // A setting changed while the cache is still being read saves the file.
    h.check("SetShowDistance", false);
    assert_eq!(h.saved().destination_name, "Rithwic");
    h.settle();
    assert_eq!(h.plugin.destination().target_name(), "Rithwic");
    assert_eq!(h.saved().destination_name, "Rithwic");
    assert!(!h.saved().show_distance);
}

#[test]
fn a_saved_destination_missing_from_the_data_is_kept_by_other_saves() {
    let mut h = loading_with_saved_destination("Nowhere");
    h.settle();
    assert!(!h.plugin.destination().has_destination());
    h.check("SetShowBearing", false);
    assert_eq!(h.saved().destination_name, "Nowhere");
    // Clearing the destination is what forgets it.
    h.go("clear");
    assert_eq!(h.saved().destination_name, "");
}
