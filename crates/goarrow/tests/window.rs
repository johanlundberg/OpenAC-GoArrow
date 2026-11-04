//! The window (`markup/goarrow.xml`): registered and bound through the
//! client's own UI state, and each control driving the plugin
//! (`GoArrowPanel.cs`; GoArrow's `UiStartupTests`).

mod common;

use std::sync::Arc;

use ac_plugin_ui::ActionValue as Av;
use ac_plugin_ui::MarkupDocument;
use ac_plugin_ui::list;
use ac_plugin_ui::markup::{BoolSource, ColumnKind, Element, ListBody};
use common::{ATLAS_XML, Harness};
use goarrow::window::{self, BINDINGS, Tab, icons};
use openac_plugin::LogLevel;
use openac_plugin::ui::TypedBindingKind as K;

fn atlas() -> Harness {
    Harness::new(|sim| {
        sim.storage
            .insert("data/warcry-atlas.xml".to_owned(), ATLAS_XML.to_owned());
        sim.stand(0.0, 0.0, 0.0);
    })
}

#[test]
fn the_window_registers_on_the_shelf_with_every_binding_pushed() {
    let h = atlas();
    let spec = window::panel_spec(h.plugin.settings());
    let registration = spec.registration();
    assert_eq!(registration.window_id, "main");
    assert_eq!(registration.title, "GoArrow");
    assert!(registration.show_in_side_panel);
    assert_eq!(registration.icon_surface_id, window::ICON);
    assert!(registration.start_visible);
    assert!(
        h.sim
            .fake
            .logs
            .iter()
            .all(|(level, _)| *level != LogLevel::Warn),
        "{:?}",
        h.sim.fake.logs
    );
    for (name, kind) in BINDINGS {
        let handler = matches!(
            kind,
            K::Action | K::IntAction | K::TextAction | K::BoolAction | K::FloatAction
        );
        if !handler {
            assert!(
                h.sim.fake.panel_typed_value(h.panel(), name).is_some(),
                "{name} has no value"
            );
        }
    }
    assert!(h.flag("RouteTab"));
    assert_eq!(h.text("GoLabel"), "Go");
    assert_eq!(h.text("FromText"), "Where I stand");
}

#[test]
fn the_window_starts_closed_when_the_settings_say_so() {
    let spec = window::panel_spec(&goarrow::settings::Settings {
        panel_visible: false,
        ..goarrow::settings::Settings::default()
    });
    assert!(!spec.registration().start_visible);
}

#[test]
fn the_tabs_switch_the_pages() {
    let mut h = atlas();
    for (handler, tab, binding) in [
        ("ShowPlaces", Tab::Places, "PlacesTab"),
        ("ShowOptions", Tab::Options, "OptionsTab"),
        ("ShowRoute", Tab::Route, "RouteTab"),
    ] {
        h.click(handler);
        assert_eq!(h.plugin.window().unwrap().tab(), tab);
        for page in ["RouteTab", "PlacesTab", "OptionsTab"] {
            assert_eq!(h.flag(page), page == binding, "{handler}: {page}");
        }
    }
}

// GoArrowPanel.SearchResults
#[test]
fn typing_in_to_suggests_places_as_goarrow_searched_them() {
    let mut h = atlas();
    h.type_text("TypeTo", "");
    assert!(h.flag("ShowSuggestions"));
    assert_eq!(h.texts("Suggestions"), ["Current Location"]);
    assert_eq!(h.text("SuggestionsLabel"), "Destination matches");
    h.type_text("TypeTo", "rith");
    // Names starting with the text first, then by name.
    assert_eq!(h.texts("Suggestions"), ["Rithwic", "Holtburg to Rithwic"]);
    h.type_text("TypeTo", "cat");
    assert_eq!(h.texts("Suggestions"), ["Current Location"]);
    h.type_text("TypeTo", "holtburg");
    assert_eq!(
        h.texts("Suggestions"),
        [
            "Holtburg",
            "Holtburg Healer",
            "Holtburg Lifestone",
            "Holtburg to Rithwic"
        ]
    );
    // Down, down, up wraps as GoArrow's did; Enter takes the highlighted.
    h.click("SuggestionUp");
    assert_eq!(h.int("SuggestionSelected"), 3);
    h.click("SuggestionDown");
    assert_eq!(h.int("SuggestionSelected"), 0);
    h.click("SuggestionDown");
    h.type_text("SubmitTo", "holtburg");
    assert_eq!(h.plugin.destination().target_name(), "Holtburg Healer");
    assert!(!h.flag("ShowSuggestions"));
    assert_eq!(h.text("ToText"), "Holtburg Healer");
    assert_eq!(h.saved().destination_name, "Holtburg Healer");
}

#[test]
fn picking_a_suggestion_or_typing_a_place_sets_the_destination() {
    let mut h = atlas();
    h.type_text("TypeTo", "rith");
    h.pick("PickSuggestion", 0);
    assert_eq!(h.plugin.destination().target_name(), "Rithwic");
    h.take_chat();
    h.type_text("SubmitTo", "12.5N 3.0W");
    assert_eq!(h.plugin.destination().target_name(), "12.5N 3.0W");
    h.type_text("SubmitTo", "Nowhere");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Select a destination from the search results."]
    );
    h.type_text("SubmitTo", "current location");
    assert_eq!(h.plugin.destination().target_name(), "Current Location");
    // The field follows the destination once no search is open.
    h.with_host(|app, host| app.set_destination_by_name(host, "Holtburg"));
    h.settle();
    assert_eq!(h.text("ToText"), "Holtburg");
}

#[test]
fn from_plans_from_another_place_and_where_i_stand_resets_it() {
    let mut h = atlas();
    h.type_text("TypeFrom", "Holt");
    assert_eq!(h.text("SuggestionsLabel"), "From matches");
    h.type_text("TypeFrom", "");
    assert_eq!(h.texts("Suggestions"), ["Where I stand"]);
    h.type_text("SubmitFrom", "Rithwic");
    assert_eq!(h.plugin.route_from().unwrap().name, "Rithwic");
    assert_eq!(h.text("FromText"), "Rithwic");
    h.take_chat();
    h.type_text("SubmitFrom", "Nowhere");
    assert_eq!(
        h.take_chat(),
        ["GoArrow: Select a From location from the search results."]
    );
    h.type_text("SubmitFrom", "where i stand");
    assert!(h.plugin.route_from().is_none());
    assert_eq!(h.text("FromText"), "Where I stand");
}

#[test]
fn the_steps_list_shows_the_route_with_icons_and_details() {
    let mut h = atlas();
    h.type_text("SubmitTo", "Rithwic");
    h.settle();
    assert_eq!(h.text("StatusText"), "Route ready");
    assert_eq!(h.texts("StepNumbers"), ["1", "2", "3"]);
    // Each step shows its place's icon, as the original GoArrow's route list:
    // the town portal walked to and used, then the town.
    assert_eq!(
        h.uints("StepIcons"),
        [icons::PORTAL, icons::PORTAL, icons::TOWN]
    );
    let texts = h.texts("StepTexts");
    assert_eq!(texts[0], "Walk: Holtburg to Rithwic");
    assert_eq!(texts[1], "Portal: Holtburg to Rithwic");
    assert_eq!(h.texts("StepWalks")[0], "240 m");
    assert_eq!(h.texts("StepWalks")[1], "");
    assert_eq!(h.text("ChoiceText"), "Route 1");
    assert!(h.text("WalkChip").starts_with("Walk "));
    assert_eq!(h.text("PortalChip"), "1 portal or recall");
    assert!(h.text("LineChip").starts_with("Straight line "));
    assert_eq!(h.int("SelectedStep"), 0);
    assert_eq!(h.text("DetailTitle"), "Step 1 of 3 · Holtburg to Rithwic");
    h.pick("SelectStep", 1);
    assert_eq!(h.text("DetailTitle"), "Step 2 of 3 · Holtburg to Rithwic");
    assert_eq!(h.text("DetailWhere"), "Use at 0.0N, 1.0E");
    assert_eq!(h.text("DetailArrival"), "Arrives 50.0N, 50.0E");
    assert_eq!(
        h.text("DetailNote"),
        "No additional notes for this location."
    );
    assert!(h.flag("CanMarkFaulty"));
    assert!(h.flag("ShowSteps") && !h.flag("ShowIndoor"));
    h.take_chat();
    h.click("MarkFaulty");
    assert!(h.take_chat()[0].starts_with("GoArrow: Blocked step 2 (Portal: Holtburg to Rithwic)"),);
}

#[test]
fn go_stop_is_one_button_and_shows_computing_meanwhile() {
    let mut h = atlas();
    h.type_text("SubmitTo", "Rithwic");
    h.settle();
    h.click("GoStop");
    assert_eq!(h.text("GoLabel"), "Stop");
    assert_eq!(h.text("StatusText"), "Computing route...");
    h.click("GoStop");
    assert_eq!(h.text("GoLabel"), "Go");
    h.click("GoStop");
    h.settle();
    assert_eq!(h.text("GoLabel"), "Go");
    assert_eq!(h.text("StatusText"), "Route ready");
    assert!(h.flag("CanGo") && h.flag("HasDestination"));
    h.click("Clear");
    assert!(!h.flag("HasDestination"));
    assert_eq!(h.text("StatusText"), "Idle");
    assert_eq!(h.text("ToText"), "");
}

// UiStartupTests.RouteWindowButtonsCyclePortalChoicesThroughPlugin
#[test]
fn previous_and_next_cycle_the_portal_choices() {
    let mut h = Harness::with_routes();
    h.type_text("SubmitTo", "End");
    h.click("GoStop");
    h.settle();
    let steps = |h: &Harness| h.texts("StepTexts").join(" | ");
    assert!(steps(&h).contains("Portal A"), "{}", steps(&h));
    assert!(h.flag("CanNext") && !h.flag("CanPrevious"));
    h.click("Next");
    h.settle();
    assert!(steps(&h).contains("Portal B"), "{}", steps(&h));
    assert_eq!(h.text("ChoiceText"), "Route 2 of 2+");
    assert!(h.flag("CanPrevious"));
    h.click("Previous");
    assert!(steps(&h).contains("Portal A"), "{}", steps(&h));
    h.click("Next");
    assert!(steps(&h).contains("Portal B"));
    // A new search starts again from the best route.
    h.click("GoStop");
    h.settle();
    assert!(steps(&h).contains("Portal A"), "{}", steps(&h));
}

#[test]
fn the_prefer_menu_picks_the_cost_profile() {
    let mut h = atlas();
    assert_eq!(
        h.texts("Profiles"),
        ["Shortest walk", "Fewest steps", "Recalls", "No portals"]
    );
    assert_eq!(h.text("Profile"), "Shortest walk");
    h.type_text("SetProfile", "No portals");
    assert_eq!(h.text("Profile"), "No portals");
    assert_eq!(h.saved().route_cost_profile, 3);
    assert_eq!(
        h.plugin.destination().profile,
        goarrow::finder::RouteCostProfile::AvoidInteractions
    );
}

#[test]
fn walk_it_and_replan_are_saved() {
    let mut h = atlas();
    h.check("SetAutoNavigate", true);
    assert!(h.flag("AutoNavigate") && h.saved().auto_navigate);
    h.check("SetRecalculate", false);
    assert!(!h.flag("Recalculate") && !h.saved().recalculate_route);
}

#[test]
fn places_lists_nearest_first_by_kind() {
    let mut h = atlas();
    h.click("ShowPlaces");
    assert_eq!(
        h.texts("PlaceNames"),
        [
            "Holtburg Lifestone · Lifestone",
            "Holtburg Healer · Vendor",
            "Holtburg · Town",
            "Holtburg to Rithwic · Town Portal",
            "Rithwic · Town",
        ]
    );
    assert_eq!(h.text("PlaceCount"), "5 shown · nearest first");
    assert_eq!(
        h.uints("PlaceIcons"),
        [
            icons::LIFESTONE,
            icons::VENDOR,
            icons::TOWN,
            icons::PORTAL,
            icons::TOWN
        ]
    );
    assert_eq!(h.texts("PlaceWheres")[2], "0.5S, 0.5E");
    assert_eq!(h.texts("PlaceAways")[2], "170 m");
    for (kind, names) in [
        (1, vec!["Holtburg · Town", "Rithwic · Town"]),
        (2, vec!["Holtburg to Rithwic · Town Portal"]),
        (3, vec![]),
        (4, vec!["Holtburg Lifestone · Lifestone"]),
        (5, vec!["Holtburg Healer · Vendor"]),
    ] {
        h.pick("SetPlaceKind", kind);
        assert_eq!(h.texts("PlaceNames"), names, "kind {kind}");
    }
    h.pick("SetPlaceKind", 0);
    h.type_text("SetPlaceQuery", "rith");
    assert_eq!(h.texts("PlaceNames").len(), 2);
    h.type_text("SetPlaceQuery", "");
    h.go("save Rithwic");
    h.check("SetFavouritesOnly", true);
    assert_eq!(h.texts("PlaceNames"), ["* Rithwic · Town"]);
    h.check("SetFavouritesOnly", false);
    assert!(!h.flag("HasPlace"));
    h.pick("SelectPlace", 2);
    assert_eq!(h.text("RouteToLabel"), "Route to Holtburg");
    assert_eq!(h.text("PlaceFavLabel"), "Favourite");
    h.click("TogglePlaceFavourite");
    assert_eq!(h.text("PlaceFavLabel"), "Unfavourite");
    assert!(
        h.saved()
            .favorite_destinations
            .contains(&"Holtburg".to_owned())
    );
    h.click("RouteToPlace");
    assert_eq!(h.plugin.destination().target_name(), "Holtburg");
    assert!(h.flag("RouteTab"));
    assert_eq!(h.text("ToText"), "Holtburg");
}

#[test]
fn places_shows_at_most_two_hundred() {
    let mut xml = String::from("<locations>");
    for i in 0..250 {
        xml.push_str(&format!(
            "<loc name='Place {i:03}' type='Town' NS='{}' EW='0'/>",
            f64::from(i) / 10.0
        ));
    }
    xml.push_str("</locations>");
    let mut h = Harness::new(|sim| {
        sim.storage.insert("GoArrow/many.xml".to_owned(), xml);
        sim.stand(0.0, 0.0, 0.0);
    });
    h.go("file many.xml");
    h.settle();
    h.click("ShowPlaces");
    assert_eq!(h.texts("PlaceNames").len(), 200);
    assert_eq!(h.texts("PlaceNames")[0], "Place 000 · Town");
    assert_eq!(h.text("PlaceCount"), "200 of 250 shown · nearest first");
}

#[test]
fn the_options_switches_are_saved_and_applied() {
    let mut h = atlas();
    h.click("ShowOptions");
    assert!(!h.flag("ShowMap"), "the map starts hidden (GA-19)");
    h.check("SetShowMap", true);
    for (handler, binding) in [
        ("SetShowArrow", "ShowArrow"),
        ("SetShowMap", "ShowMap"),
        ("SetShowRouteLine", "ShowRouteLine"),
        ("SetShowDungeonMap", "ShowDungeonMap"),
        ("SetShowDistance", "ShowDistance"),
        ("SetShowBearing", "ShowBearing"),
        ("SetUseAutomation", "UseAutomation"),
    ] {
        assert!(h.flag(binding), "{binding}");
        h.check(handler, false);
        assert!(!h.flag(binding), "{binding}");
    }
    let saved = h.saved();
    assert!(!saved.hud_visible && !saved.map_visible && !saved.route_line_visible);
    assert!(!saved.dungeon_map_visible && !saved.show_distance && !saved.show_bearing);
    assert!(!saved.use_navigation_automation);
    let arrow = h.canvas("goarrow.arrow");
    assert!(
        !h.sim
            .fake
            .ui
            .canvases()
            .find(|c| c.id == arrow)
            .unwrap()
            .visible
    );
    h.act("SetArrivalDistance", Av::Float(2.5));
    assert_eq!(h.saved().arrival_distance, 2.5);
    h.act("SetArrivalDistance", Av::Float(0.0));
    assert_eq!(h.saved().arrival_distance, 0.5);
}

#[test]
fn route_fixes_list_the_corrections_and_reload() {
    let mut h = atlas();
    h.click("ShowOptions");
    let nanto = ["Town Network (E R 2) to Nanto: arrives 52.2S, 82.5E"];
    assert_eq!(h.texts("Fixes"), nanto);
    assert!(!h.flag("HasPendingArrival"));
    // Read again, the player's file (written as a copy of the shipped one
    // at the first start) is loaded too: each correction is listed once.
    h.click("ReloadCorrections");
    assert!(h.text("FixesStatus").starts_with("GoArrow: Loaded "));
    assert_eq!(h.texts("Fixes"), nanto);
}

#[test]
fn mark_here_saves_the_indoor_point() {
    let mut h = atlas();
    h.click("ShowPlaces");
    h.sim.stand_indoors(0x1234_0122, 10.0, 10.0);
    h.type_text("SetMarkName", "Altar");
    h.click("MarkHere");
    assert_eq!(h.plugin.destination().target_name(), "Altar");
    assert_eq!(h.text("MarkName"), "");
}

fn document(h: &Harness) -> Arc<MarkupDocument> {
    let panel = h.panel();
    h.sim
        .fake
        .ui
        .panels()
        .find(|p| p.id == panel)
        .expect("the window")
        .document
        .clone()
}

/// The widths a list's columns get across `width` pixels, and the index
/// of the text column showing `items`.
fn column_widths(doc: &MarkupDocument, items: &str, width: f32) -> (Vec<f32>, usize) {
    for node in &doc.nodes {
        let Element::List {
            body: ListBody::Columns(columns),
            ..
        } = &node.element
        else {
            continue;
        };
        let Some(index) = columns
            .iter()
            .position(|c| matches!(&c.kind, ColumnKind::Text { items: i, .. } if i == items))
        else {
            continue;
        };
        let widths = list::column_layout(columns, width)
            .into_iter()
            .map(|(_, w)| w)
            .collect();
        return (widths, index);
    }
    panic!("no list shows {items}");
}

// At 1280x720 the window (690 tall, 719 with its title bar) filled the
// screen and covered the map; it opens about 620 tall now and can be made
// smaller, and its fixed rows keep their size while the lists give room.
#[test]
fn the_window_opens_about_620_tall_and_its_fixed_rows_do_not_shrink() {
    let h = atlas();
    let doc = document(&h);
    let panel = &doc.panel;
    assert!(panel.resizable);
    assert_eq!(panel.authored_size, (Some(500.0), Some(590.0)));
    assert_eq!(panel.authored_min, (Some(460.0), Some(480.0)));
    let page = |index: Option<usize>| {
        index.is_some_and(|i| {
            matches!(&doc.nodes[i].common.visible,
                BoolSource::Bound(b) if b == "RouteTab" || b == "PlacesTab")
        })
    };
    let mut fixed = 0;
    for (i, node) in doc.nodes.iter().enumerate() {
        let top_row = node.parent.is_none() && !matches!(node.common.visible, BoolSource::Bound(_));
        if !(page(node.parent) || top_row) {
            continue;
        }
        let flex = node.flex.as_ref().expect("a flex item");
        if flex.grow == 0.0 {
            fixed += 1;
            assert_eq!(flex.shrink, 0.0, "node {i} shrinks");
        }
    }
    assert!(fixed >= 12, "{fixed}");
}

// The step text ran into the Walk column, and a place's name and kind
// into Where: the text columns take the room the others leave.
#[test]
fn the_text_columns_take_the_room_the_others_leave() {
    let h = atlas();
    let doc = document(&h);
    let (steps, step) = column_widths(&doc, "StepTexts", 468.0);
    let (_, walk) = column_widths(&doc, "StepWalks", 468.0);
    assert!(steps[step] >= 330.0, "{steps:?}");
    assert!(steps[walk] <= 60.0, "{steps:?}");
    let (places, place) = column_widths(&doc, "PlaceNames", 468.0);
    let (_, place_where) = column_widths(&doc, "PlaceWheres", 468.0);
    let (_, away) = column_widths(&doc, "PlaceAways", 468.0);
    assert!(places[place] >= 270.0, "{places:?}");
    assert!(
        places[place_where] <= 96.0 && places[away] <= 60.0,
        "{places:?}"
    );
}

// The search field has no placeholder, so a muted label names it; the
// walking switch says what UseNavigationAutomation does (off: Go and
// Resume only plan).
#[test]
fn the_places_search_is_labelled_and_the_walking_switch_says_what_it_does() {
    let h = atlas();
    let doc = document(&h);
    let field = doc
        .nodes
        .iter()
        .find(|n| {
            matches!(&n.element, Element::Field { .. })
                && format!("{:?}", n.element).contains("PlaceQuery")
        })
        .expect("the search field");
    let label = doc.nodes.iter().find(|n| {
        n.parent == field.parent
            && matches!(&n.element, Element::Label { .. })
            && format!("{:?}", n.element).contains("\"Search places\"")
    });
    assert!(label.is_some(), "a label beside the search field");
    let switch = doc
        .nodes
        .iter()
        .find(|n| matches!(&n.element, Element::Checkbox { checked, .. } if checked == "UseAutomation"))
        .expect("the switch");
    assert!(
        format!("{:?}", switch.element).contains("Let GoArrow walk (off: Go only plans the route)"),
        "{:?}",
        switch.element
    );
}

// The kinds as the Atlas writes them.
#[test]
fn place_kinds_read_as_the_atlas_writes_them() {
    use goarrow::location::LocationType as T;
    assert_eq!(window::kind_text(T::NPC), "NPC");
    assert_eq!(window::kind_text(T::ALLEGIANCE_HALL), "Allegiance Hall");
    assert_eq!(window::kind_text(T::TOWN_PORTAL), "Town Portal");
    assert_eq!(window::kind_text(T::PORTAL_HUB), "Portal Hub");
    assert_eq!(
        window::kind_text(T::TOWN | T::TOWN_PORTAL),
        "Town, Town Portal"
    );
    assert_eq!(window::kind_text(T::UNKNOWN), "Unknown");
}

#[test]
fn a_places_icon_is_the_original_goarrows() {
    use goarrow::location::{Location, LocationType};
    let mut place = Location::new("Somewhere", 0.0, 0.0);
    assert_eq!(window::place_icon(&place), icons::UNKNOWN);
    for (kind, icon) in [
        (LocationType::TOWN, icons::TOWN),
        (LocationType::OUTPOST, icons::OUTPOST),
        (LocationType::VILLAGE, icons::SETTLEMENT),
        (LocationType::DUNGEON, icons::DUNGEON),
        (LocationType::LIFESTONE, icons::LIFESTONE),
        (LocationType::BINDSTONE, icons::BINDSTONE),
        (LocationType::ALLEGIANCE_HALL, icons::ALLEGIANCE_HALL),
        (LocationType::LANDMARK, icons::LANDMARK),
        (LocationType::NPC, icons::VENDOR),
        (LocationType::VENDOR, icons::VENDOR),
        (LocationType::PORTAL_HUB, icons::PORTAL_HUB),
        (LocationType::WILDERNESS_PORTAL, icons::PORTAL),
        (LocationType::SETTLEMENT_PORTAL, icons::PORTAL),
    ] {
        place.kind = kind;
        assert_eq!(window::place_icon(&place), icon, "{kind:?}");
    }
    // A settlement named a mansion or a villa has its own.
    place.kind = LocationType::VILLAGE;
    place.name = "Holtburg Mansion".to_owned();
    assert_eq!(window::place_icon(&place), icons::MANSION);
    place.name = "Villa on the hill".to_owned();
    assert_eq!(window::place_icon(&place), icons::VILLA);
    // An icon the data names wins over the kind; a favourite over both.
    place.specialized_icon = 0x0600_1234;
    assert_eq!(window::place_icon(&place), 0x0600_1234);
    place.is_favorite = true;
    assert_eq!(window::place_icon(&place), icons::FAVORITE);
}
