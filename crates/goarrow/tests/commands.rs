//! The `/go` commands (`GoArrowCommands.cs`; GoArrow's
//! `CommandIntegrationTests`): each subcommand's chat lines, the help and
//! completion.

mod common;

use ac_plugin_abi::social::{
    RecallKind, RecallLocation, RecallRequest, RecallResult, RecallState, RecallStatus,
};
use ac_plugin_abi::world::{Position, SelectionChange, WorldObject};
use common::{ATLAS_XML, Harness};
use goarrow::commands::{self, HELP, parse_arguments};
use goarrow::data::{CORRECTIONS_KEY, INDOOR_KEY};
use openac_plugin::{Callback, CallbackAnswer, CommandCall, Event, calls};

fn atlas() -> Harness {
    Harness::new(|sim| {
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 0.0);
    })
}

#[test]
fn arguments_split_as_openac_splits_them() {
    assert_eq!(
        parse_arguments(r#"to "Lord Cambarth" 'a b' c\ d"#).unwrap(),
        ["to", "Lord Cambarth", "a b", "c d"]
    );
    assert!(parse_arguments("   ").unwrap().is_empty());
    assert_eq!(
        parse_arguments("save 'open").unwrap_err(),
        "Unterminated quoted command argument."
    );
}

#[test]
fn no_arguments_and_help_list_the_commands() {
    let mut h = atlas();
    h.take_chat();
    h.go("");
    assert_eq!(h.take_chat(), HELP);
    h.go("?");
    assert_eq!(h.take_chat()[0], "GoArrow Commands:");
    h.go("say 'unclosed");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Unterminated quoted command argument."]
    );
}

// CommandIntegrationTests.StatusShowsLoadedVersionAndCurrentNavigationState
#[test]
fn status_shows_the_version_and_the_navigation_state() {
    let mut h = atlas();
    h.take_chat();
    h.go("status");
    let chat = h.take_chat();
    assert_eq!(
        chat[0],
        format!(
            "GoArrow v{}: Idle; outdoors; client None; next no active step.",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(
        chat.last().unwrap(),
        "GoArrow: No destination set. Use /go <name>"
    );
    h.go("Rithwic");
    h.settle();
    h.take_chat();
    h.go("status");
    let chat = h.take_chat();
    assert!(chat[0].contains("Route ready; outdoors"), "{chat:?}");
    assert!(
        chat[0].contains("next Walk: Holtburg to Rithwic"),
        "{chat:?}"
    );
    assert_eq!(
        chat.last().unwrap(),
        "GoArrow: Destination 'Rithwic'. Use /go stop to cancel."
    );
}

#[test]
fn a_name_sets_the_destination_with_both_lines() {
    let mut h = atlas();
    h.take_chat();
    h.go("rithwic");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Destination 'rithwic' set.",
            "GoArrow: Destination set to 'rithwic'."
        ]
    );
    assert_eq!(h.plugin.destination().target_name(), "Rithwic");
    assert_eq!(h.saved().destination_name, "Rithwic");
    h.go("Nowhere Town");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Destination 'Nowhere Town' not found in location database."]
    );
}

// CommandIntegrationTests.CoordinateCommandAndChatLinkSetCoordinateDestination
#[test]
fn coordinates_and_chat_links_set_a_map_point() {
    let mut h = atlas();
    h.take_chat();
    h.go("to 42.1N 33.6E");
    assert_eq!(h.take_chat(), ["GoArrow: Destination set."]);
    assert_eq!(h.plugin.destination().target_name(), "42.1N 33.6E");
    h.go("12.3S 4W");
    assert_eq!(h.take_chat(), ["GoArrow: Coordinate destination set."]);
    h.send(Event::ChatLinkClicked(openac_plugin::ChatLinkClicked {
        kind: 0,
        display_text: "7N 8E".to_owned(),
        coordinate: Some((8.0, 7.0)),
    }));
    // DIVERGENCES.md GA-2: the map's own text.
    assert_eq!(h.plugin.destination().target_name(), "7.0N, 8.0E");
}

#[test]
fn to_here_from_loc_and_dest_report_positions() {
    let mut h = atlas();
    h.take_chat();
    h.go("to");
    h.go("to here");
    h.go("from");
    h.go("loc");
    h.go("dest");
    h.go("to Rithwic");
    h.go("dest");
    h.go("to Nowhere");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Usage: /go to <location|coordinates|here>",
            "GoArrow: Current position: 0.0N, 0.0E",
            "GoArrow: Route origin: 0.0N, 0.0E",
            "GoArrow: Current position: 0.0N, 0.0E",
            "GoArrow: No destination set.",
            "GoArrow: Destination 'Rithwic' set.",
            "GoArrow: Destination set.",
            "GoArrow: Destination 'Rithwic': 50.0N, 51.0E",
            "GoArrow: Destination 'Nowhere' not found.",
        ]
    );
    h.sim.snapshot.is_available = false;
    h.go("loc");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Current position is unavailable (not in world)."]
    );
}

#[test]
fn list_and_search_show_twenty_names_at_most() {
    let mut h = atlas();
    h.take_chat();
    h.go("list");
    h.go("search holt");
    h.go("find zzz");
    h.go("search");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow Locations: Holtburg, Holtburg Healer, Holtburg Lifestone, Holtburg to Rithwic, Rithwic",
            "GoArrow matches: Holtburg to Rithwic, Holtburg, Holtburg Lifestone, Holtburg Healer",
            "GoArrow: No locations match 'zzz'.",
            "GoArrow: Usage: /go search <term>",
        ]
    );
    let mut empty = Harness::new(|_| {});
    empty.take_chat();
    empty.go("list");
    assert_eq!(empty.take_chat(), ["GoArrow: No locations loaded."]);
}

#[test]
fn stop_end_reset_clear_lock_and_unlock() {
    let mut h = atlas();
    h.go("Rithwic");
    h.take_chat();
    h.go("stop");
    h.go("cancel");
    h.go("end");
    h.go("lock");
    assert!(h.saved().navigation_locked);
    h.go("unlock");
    h.go("reset");
    assert!(!h.plugin.destination().has_destination());
    h.go("Rithwic");
    h.take_chat();
    h.go("clear");
    assert_eq!(h.take_chat(), ["GoArrow: Destination cleared."]);
    assert_eq!(h.saved().destination_name, "");
}

#[test]
fn the_stop_family_chat_lines() {
    let mut h = atlas();
    h.take_chat();
    for args in ["stop", "end", "lock", "unlock", "reset"] {
        h.go(args);
    }
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Navigation stopped.",
            "GoArrow: Current route leg ended; use /go resume to continue.",
            "GoArrow: Navigation locked.",
            "GoArrow: Navigation unlocked.",
            "GoArrow: Route reset.",
        ]
    );
}

#[test]
fn favourites_are_saved_once_ignoring_case() {
    let mut h = atlas();
    h.take_chat();
    h.go("favorites");
    h.go("save");
    h.go("save Rithwic");
    h.go("save rithwic");
    h.go("save \"Holtburg Lifestone\"");
    h.go("favs");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: No favorites saved. Use /go save <name>",
            "GoArrow: Usage: /go save <name>",
            "GoArrow: 'Rithwic' saved to favorites.",
            "GoArrow: 'rithwic' saved to favorites.",
            "GoArrow: 'Holtburg Lifestone' saved to favorites.",
            "GoArrow Favorites: Rithwic, Holtburg Lifestone",
        ]
    );
    assert_eq!(
        h.saved().favorite_destinations,
        ["Rithwic", "Holtburg Lifestone"]
    );
}

// CommandIntegrationTests.MarkCommandAddsAnIndoorLocationToSearchableData
#[test]
fn mark_saves_an_indoor_point_as_a_searchable_destination() {
    let mut h = atlas();
    h.take_chat();
    h.go("mark Nowhere");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Stand at an indoor point and use /go mark <name>."]
    );
    h.sim.stand_indoors(0x1234_0122, 10.0, 10.0);
    h.go("mark Lower Chamber");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Saved indoor location 'Lower Chamber'."]
    );
    assert_eq!(h.plugin.destination().target_name(), "Lower Chamber");
    let found = h.plugin.search_locations("Lower");
    assert_eq!(
        found[0].indoor_position.map(|p| p.cell_id),
        Some(0x1234_0122)
    );
    let saved = &h.sim.storage[INDOOR_KEY];
    assert!(
        saved.starts_with("<locations>") && saved.contains("Lower Chamber"),
        "{saved}"
    );
    // Read back at the next start.
    let storage = h.sim.storage.clone();
    let again = Harness::new(|sim| sim.storage = storage);
    assert_eq!(again.plugin.destination().target_name(), "Lower Chamber");
}

// CommandIntegrationTests.DungeonCommandControlsAutomaticMapVisibility
#[test]
fn dungeon_on_off_and_toggle_show_the_dungeon_map() {
    let mut h = atlas();
    h.take_chat();
    h.go("dungeon off");
    assert!(!h.plugin.settings().dungeon_map_visible);
    assert!(!h.saved().dungeon_map_visible);
    h.go("dungeon on");
    assert!(h.plugin.settings().dungeon_map_visible);
    h.go("dungeon");
    h.go("dungeon sideways");
    h.go("dungeon path");
    h.go("dungeon export a/b.json");
    h.go("dungeon import");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Dungeon map disabled.",
            "GoArrow: Dungeon map enabled.",
            "GoArrow: Dungeon map disabled.",
            "GoArrow: Usage: /go dungeon [on|off|toggle|path|reload]",
            "GoArrow: The dungeon map is drawn from the game's own data; there are no map files to load.",
            "GoArrow: Use a JSON filename in the plugin storage folder.",
            "GoArrow: Dungeon traversal import failed: File not found.",
        ]
    );
}

#[test]
fn url_and_update_check_the_address() {
    let mut h = atlas();
    h.take_chat();
    h.go("url");
    h.go("url ftp://example.com/x.xml");
    h.go("url https://Example.com");
    assert_eq!(
        h.plugin.settings().external_data_url,
        "https://example.com/"
    );
    h.go("update notaurl");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Usage: /go url <http:// or https:// URL>",
            "GoArrow: Usage: /go url <http:// or https:// URL>",
            "GoArrow: Location-data URL set to 'https://Example.com'.",
            "GoArrow: URL must be an absolute http:// or https:// URL.",
        ]
    );
    let mut empty = Harness::new(|_| {});
    empty.take_chat();
    empty.go("update");
    assert_eq!(
        empty.take_chat(),
        ["GoArrow: Set a location data URL in Options before downloading."]
    );
}

#[test]
fn file_loads_location_data_from_the_goarrow_folder() {
    let mut h = Harness::with_routes();
    assert_eq!(h.plugin.database().location_count(), 3);
    for bad in [
        "file",
        "file ../x.xml",
        "file GoArrow/x.xml",
        "file x.txt",
        "file missing.xml",
    ] {
        h.go(bad);
    }
    assert_eq!(h.take_chat(), [commands::FILE_USAGE; 5]);
    h.sim
        .storage
        .insert("GoArrow/empty.xml".to_owned(), "<locations/>".to_owned());
    h.go("file empty.xml");
    h.settle();
    assert_eq!(h.take_chat(), [commands::FILE_USAGE]);
    h.go("file routes.xml");
    assert!(h.take_chat().is_empty(), "the answer waits for the read");
    h.settle();
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Loaded location data from 'routes.xml'."]
    );
}

#[test]
fn route_lists_the_steps_and_block_saves_a_faulty_one() {
    let mut h = Harness::with_routes();
    h.go("route");
    h.go("to End");
    h.settle();
    h.take_chat();
    h.go("route");
    let route = h.take_chat();
    assert_eq!(route[0], "GoArrow route:");
    assert!(route[1].starts_with("  1. Walk: Portal A"), "{route:?}");
    assert!(route[2].starts_with("  2. Portal: Portal A"), "{route:?}");
    h.go("block");
    h.go("block 9");
    h.go("block 2 wrong arrival");
    let chat = h.take_chat();
    assert_eq!(chat[0], "GoArrow: Usage: /go block <step number> [note]");
    assert_eq!(chat[1], "GoArrow: There is no route step 9.");
    assert_eq!(
        chat[2],
        "GoArrow: Blocked step 2 (Portal: Portal A); the route will be recalculated without it."
    );
    let user = &h.sim.storage[CORRECTIONS_KEY];
    assert!(
        user.contains("\"From\": \"Portal A\"") && user.contains("wrong arrival"),
        "{user}"
    );
    h.settle();
    h.go("route");
    let again = h.take_chat();
    assert!(again.iter().any(|l| l.contains("Portal B")), "{again:?}");
}

#[test]
fn corrections_path_reload_and_save() {
    let mut h = atlas();
    h.take_chat();
    h.go("corrections");
    h.go("corrections reload");
    h.go("corrections save");
    h.go("corrections nonsense");
    let chat = h.take_chat();
    assert_eq!(
        chat[0],
        "GoArrow: Route corrections file: GoArrow/atlas-corrections.json in host managed storage."
    );
    assert!(
        chat[1].starts_with("GoArrow: Loaded ") && chat[1].ends_with(" blocked steps."),
        "{chat:?}"
    );
    assert_eq!(
        chat[2],
        "GoArrow: No observed portal arrival is waiting to be saved."
    );
    assert_eq!(
        chat[3],
        "GoArrow: Usage: /go corrections [path|reload|save]"
    );
    // The first start writes the shipped file for the player to edit.
    assert!(h.sim.storage.contains_key(CORRECTIONS_KEY));
    h.sim
        .storage
        .insert(CORRECTIONS_KEY.to_owned(), "{ broken".to_owned());
    h.go("corrections reload");
    let chat = h.take_chat();
    assert!(
        chat[0].ends_with("; 1 could not be used (see log)."),
        "{chat:?}"
    );
}

#[test]
fn recall_asks_the_client_and_learns_where_it_lands() {
    let mut h = atlas();
    h.take_chat();
    h.go("recall sideways");
    h.go("recall");
    h.sim.fake.set_reply::<calls::RecallState>(&RecallState {
        is_available: true,
        last_request: RecallRequest {
            revision: 7,
            kind: RecallKind::Marketplace,
            status: RecallStatus::Started,
        },
        locations: vec![RecallLocation {
            kind: RecallKind::Marketplace,
            position: Position {
                north_south: -1.5,
                east_west: 2.25,
                ..Position::default()
            },
            is_known: true,
            ..RecallLocation::default()
        }],
    });
    h.sim.fake.set_reply::<calls::RecallGo>(&RecallResult {
        status: RecallStatus::Refused,
        notice: None,
    });
    h.go("recall marketplace");
    h.sim.fake.set_reply::<calls::RecallGo>(&RecallResult {
        status: RecallStatus::Started,
        notice: None,
    });
    h.go("recall Marketplace");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Recall must be lifestone, marketplace, house, mansion, or allegiance.",
            "GoArrow: Recall is unavailable outside a live session.",
            "GoArrow: Recall was not accepted (Refused).",
            "GoArrow: Marketplace recall started.",
        ]
    );
    h.send(Event::PortalTransition(
        openac_plugin::world::PortalTransition {
            is_completed: true,
            recall_request_revision: 7,
            ..openac_plugin::world::PortalTransition::default()
        },
    ));
    assert_eq!(h.saved().last_secondary_recall, "-1.5,2.25");
}

#[test]
fn selected_attaches_the_selected_object() {
    let mut h = atlas();
    h.take_chat();
    h.go("selected");
    h.sim
        .fake
        .set_reply::<calls::SelectionState>(&openac_plugin::social::SelectionState {
            selected: Some(0x8000_0001),
            previous: None,
        });
    h.sim
        .fake
        .set_reply::<calls::WorldObject>(&Some(WorldObject {
            object_id: 0x8000_0001,
            name: "Drudge".to_owned(),
            position: Some(Position {
                north_south: 1.0,
                east_west: 2.0,
                is_outdoor: true,
                ..Position::default()
            }),
            ..WorldObject::default()
        }));
    h.go("attach");
    assert_eq!(
        h.take_chat(),
        [
            "GoArrow: Selected object is unavailable or has no position.",
            "GoArrow: Selected object attached as destination.",
        ]
    );
    assert_eq!(h.plugin.destination().target_name(), "Drudge");
}

// GoArrowPlugin.OnSelectionChanged: selecting something else marks an
// object destination unavailable; reselecting it or a place destination
// is left alone.
#[test]
fn selecting_another_object_marks_the_object_destination_unavailable() {
    let mut h = atlas();
    h.sim
        .fake
        .set_reply::<calls::SelectionState>(&openac_plugin::social::SelectionState {
            selected: Some(0x8000_0001),
            previous: None,
        });
    h.sim
        .fake
        .set_reply::<calls::WorldObject>(&Some(WorldObject {
            object_id: 0x8000_0001,
            name: "Drudge".to_owned(),
            position: Some(Position {
                north_south: 1.0,
                east_west: 2.0,
                is_outdoor: true,
                ..Position::default()
            }),
            ..WorldObject::default()
        }));
    h.go("selected");
    assert_eq!(h.plugin.destination().target_name(), "Drudge");
    let select = |h: &mut Harness, selected: Option<u32>| {
        h.send(Event::SelectionChanged(SelectionChange {
            previous: None,
            selected,
        }));
    };
    select(&mut h, Some(0x8000_0001));
    assert!(!h.plugin.destination().target_unavailable());
    select(&mut h, Some(0x8000_0002));
    assert!(h.plugin.destination().target_unavailable());
    // A place destination does not care what is selected.
    h.go("Rithwic");
    select(&mut h, None);
    assert!(!h.plugin.destination().target_unavailable());
}

#[test]
fn completion_offers_subcommands_then_places() {
    let mut h = atlas();
    let mut ask = |arguments: &str| -> Vec<String> {
        let answer = h.sim.fake.ask(
            &mut h.plugin,
            &Callback::CommandComplete(CommandCall {
                verb: "go".to_owned(),
                arguments: arguments.to_owned(),
                raw: format!("/go {arguments}"),
            }),
        );
        match answer {
            CallbackAnswer::Completions(list) => list.into_iter().map(|c| c.text).collect(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(ask("st"), ["start", "status", "stop"]);
    assert_eq!(ask("").len(), commands::SUBCOMMANDS.len());
    assert_eq!(ask("to rith"), ["Holtburg to Rithwic", "Rithwic"]);
    assert!(ask("list x").is_empty());
}

#[test]
fn the_verb_and_alias_are_registered() {
    let h = atlas();
    let def = &h.sim.fake.commands[0];
    assert_eq!(def.verb, "go");
    assert_eq!(def.aliases, ["goarrow"]);
    assert_eq!(def.description, commands::DESCRIPTION);
}

// `/go file` of a file over 1 MB that does not split into records is
// refused with a line saying so, rather than read in one long call.
#[test]
fn file_reports_a_large_file_it_cannot_read_in_parts() {
    let mut xml = String::from("<locations xmlns='urn:x'>");
    let mut i = 0;
    while xml.len() < 1_100_000 {
        xml.push_str(&format!("<loc name='Place {i}' NS='1' EW='1'/>"));
        i += 1;
    }
    xml.push_str("</locations>");
    let mut h = Harness::new(|sim| {
        sim.storage.insert("GoArrow/big.xml".to_owned(), xml);
    });
    h.take_chat();
    h.go("file big.xml");
    h.settle();
    let chat = h.take_chat();
    assert_eq!(chat.len(), 1, "{chat:?}");
    assert!(
        chat[0].starts_with("GoArrow: Could not read 'big.xml' in parts; it is over 1 MB"),
        "{chat:?}"
    );
    assert_eq!(h.plugin.database().location_count(), 0);
}
