//! The window: one themed panel (`markup/goarrow.xml`) with three tabs over
//! the plugin, [`GoArrow`]: Route (where to and from, with suggestions as
//! GoArrow's search results gave them; the route's choices and its cost
//! profile; the steps; the selected step; the status; Go/Stop, Resume and
//! Clear), Places (every place, nearest first, by kind) and Options.
//!
//! GoArrow's panel (`GoArrowPanel.cs`) had Route, Config and Details tabs;
//! the details are a box under the steps here, and Places is new.
//!
//! The SDK pushes values: [`Window::refresh`] computes the bound values of
//! the tab shown and the panel sends only what changed. It runs after each
//! control the player uses and every quarter second while the window is
//! open.

use std::rc::Rc;

use openac_plugin::ui::{
    ActionValue, Panel, PanelSpec, TypedBindingKind as K, TypedValue, UiAction,
};
use openac_plugin::{Event, Host, TICK_SECONDS};

use crate::coords::{Coordinates, METRES_PER_MAP_UNIT};
use crate::corrections::AtlasCorrections;
use crate::data::SUGGESTED_DATA_URL;
use crate::distance::format_travel_distance;
use crate::finder::RouteCostProfile;
use crate::location::{Location, LocationType};
use crate::plugin::{CURRENT_LOCATION, GoArrow, Here, say};
use crate::route::{RouteStep, RouteStepKind, hub_at};
use crate::settings::Settings;
use crate::text;

/// The window's markup.
pub const MARKUP: &str = include_str!("../markup/goarrow.xml");
/// The window's id.
pub const WINDOW_ID: &str = "main";
/// The window's title and shelf name.
pub const TITLE: &str = "GoArrow";
/// The shelf icon: the game's arrow (GoArrow's `IconSurfaceId`).
pub const ICON: u32 = 0x0600_1C74;

/// A recall step's icon.
pub const RECALL_ICON: u32 = 0x0600_1355;

/// What the From field calls the player's own position.
pub const WHERE_I_STAND: &str = "Where I stand";

/// The most suggestions (GoArrow's six, and its own entry).
pub const MAX_SUGGESTIONS: usize = 6;
/// The most rows the Places list shows.
pub const MAX_PLACES: usize = 200;

/// The Prefer menu: the four cost profiles, in their settings order.
pub const PROFILES: [&str; 4] = ["Shortest walk", "Fewest steps", "Recalls", "No portals"];

/// The Places kind filter, in the order of its segmented choice.
pub const PLACE_KINDS: [&str; 6] = [
    "All",
    "Towns",
    "Portals",
    "Dungeons",
    "Lifestones",
    "People",
];

/// The fewest ticks between two refreshes a tick makes (a quarter second).
pub const REFRESH_TICKS: u64 = (0.25 / TICK_SECONDS) as u64 + 1;

/// The window's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// Where to, the route and Go.
    #[default]
    Route,
    /// Every place.
    Places,
    /// The settings.
    Options,
}

/// Which field the suggestions are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The destination.
    To,
    /// Where the route starts.
    From,
}

/// The suggestions for a typed text (GoArrow's search results): its own
/// entry alone for nothing typed; else up to six places with coordinates
/// whose names contain the text, those starting with it first and then by
/// name, after its own entry when that contains the text too.
#[must_use]
pub fn suggestions(app: &GoArrow, query: &str, own: &str) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        return vec![own.to_owned()];
    }
    let mut matches: Vec<Rc<Location>> = app
        .search_locations(query)
        .into_iter()
        .filter(|l| l.has_coordinates())
        .collect();
    matches.sort_by(|a, b| {
        let starts = |l: &Location| !text::starts_with_ignore_case(&l.name, query);
        starts(a)
            .cmp(&starts(b))
            .then_with(|| text::cmp_ignore_case(&a.name, &b.name))
    });
    let mut names: Vec<String> = matches
        .iter()
        .take(MAX_SUGGESTIONS)
        .map(|l| l.name.clone())
        .collect();
    if text::fold(own).contains(&text::fold(query)) {
        names.insert(0, own.to_owned());
    }
    names
}

/// Whether a place is of a Places kind ([`PLACE_KINDS`]).
#[must_use]
pub fn place_kind_matches(location: &Location, kind: usize) -> bool {
    let kinds = match kind {
        1 => LocationType::TOWN | LocationType::VILLAGE | LocationType::OUTPOST,
        2 => LocationType::ANY_PORTAL,
        3 => LocationType::DUNGEON,
        4 => LocationType::LIFESTONE,
        5 => LocationType::NPC | LocationType::VENDOR,
        _ => return true,
    };
    location.kind.intersects(kinds)
}

/// The game icons the original GoArrow showed for places, by kind.
pub mod icons {
    /// A place of no known kind.
    pub const UNKNOWN: u32 = 0x0600_10E8;
    /// Any portal (town, settlement, wilderness, underground).
    pub const PORTAL: u32 = 0x0600_29ED;
    /// An allegiance hall.
    pub const ALLEGIANCE_HALL: u32 = 0x0600_2019;
    /// An allegiance bindstone.
    pub const BINDSTONE: u32 = 0x0600_218C;
    /// A dungeon.
    pub const DUNGEON: u32 = 0x0600_106B;
    /// A landmark.
    pub const LANDMARK: u32 = 0x0600_2B04;
    /// A lifestone.
    pub const LIFESTONE: u32 = 0x0600_24E1;
    /// An NPC or a vendor.
    pub const VENDOR: u32 = 0x0600_1036;
    /// An outpost.
    pub const OUTPOST: u32 = 0x0600_12D3;
    /// A portal hub.
    pub const PORTAL_HUB: u32 = 0x0600_5B1E;
    /// A town.
    pub const TOWN: u32 = 0x0600_2B6C;
    /// A settlement.
    pub const SETTLEMENT: u32 = 0x0600_2181;
    /// A settlement named a mansion.
    pub const MANSION: u32 = 0x0600_218B;
    /// A settlement named a villa.
    pub const VILLA: u32 = 0x0600_218E;
    /// A place the player made.
    pub const CUSTOM: u32 = 0x0600_1F88;
    /// A favourite, whatever its kind.
    pub const FAVORITE: u32 = 0x0600_1E13;
}

/// The icon for a place, as the original GoArrow chose it: a favourite's
/// star, else the icon the data names, else its kind's.
#[must_use]
pub fn place_icon(location: &Location) -> u32 {
    if location.is_favorite {
        return icons::FAVORITE;
    }
    if location.specialized_icon != 0 {
        return location.specialized_icon as u32;
    }
    let kind = location.kind;
    if kind == LocationType::VILLAGE {
        if location.name.contains("Mansion") {
            return icons::MANSION;
        }
        if location.name.contains("Villa") {
            return icons::VILLA;
        }
    }
    // The hub before the portals, whose set includes it.
    let by_kind = [
        (LocationType::PORTAL_HUB, icons::PORTAL_HUB),
        (LocationType::ANY_PORTAL, icons::PORTAL),
        (LocationType::TOWN, icons::TOWN),
        (LocationType::OUTPOST, icons::OUTPOST),
        (LocationType::VILLAGE, icons::SETTLEMENT),
        (LocationType::DUNGEON, icons::DUNGEON),
        (LocationType::LIFESTONE, icons::LIFESTONE),
        (LocationType::BINDSTONE, icons::BINDSTONE),
        (LocationType::ALLEGIANCE_HALL, icons::ALLEGIANCE_HALL),
        (LocationType::LANDMARK, icons::LANDMARK),
        (LocationType::NPC | LocationType::VENDOR, icons::VENDOR),
    ];
    by_kind
        .into_iter()
        .find(|(k, _)| kind.intersects(*k))
        .map_or(icons::UNKNOWN, |(_, icon)| icon)
}

/// Each place kind as the Atlas writes it, in flag order.
const KIND_NAMES: [(LocationType, &str); 16] = [
    (LocationType::ALLEGIANCE_HALL, "Allegiance Hall"),
    (LocationType::BINDSTONE, "Bindstone"),
    (LocationType::DUNGEON, "Dungeon"),
    (LocationType::LANDMARK, "Landmark"),
    (LocationType::LIFESTONE, "Lifestone"),
    (LocationType::NPC, "NPC"),
    (LocationType::OUTPOST, "Outpost"),
    (LocationType::PORTAL, "Portal"),
    (LocationType::PORTAL_HUB, "Portal Hub"),
    (LocationType::SETTLEMENT_PORTAL, "Settlement Portal"),
    (LocationType::VILLAGE, "Village"),
    (LocationType::TOWN, "Town"),
    (LocationType::TOWN_PORTAL, "Town Portal"),
    (LocationType::UNDERGROUND_PORTAL, "Underground Portal"),
    (LocationType::VENDOR, "Vendor"),
    (LocationType::WILDERNESS_PORTAL, "Wilderness Portal"),
];

/// A place kind as the Places list writes it, as the Atlas does (`NPC`,
/// `Town Portal`, `Allegiance Hall`); several joined by commas.
#[must_use]
pub fn kind_text(kind: LocationType) -> String {
    if kind == LocationType::UNKNOWN {
        return "Unknown".to_owned();
    }
    let names: Vec<&str> = KIND_NAMES
        .iter()
        .filter(|(flag, _)| kind.intersects(*flag))
        .map(|(_, name)| *name)
        .collect();
    let named = KIND_NAMES.iter().fold(0, |bits, (flag, _)| bits | flag.0);
    if kind.0 & !named != 0 {
        // A bit no kind names: the number, as the settings write it.
        return kind.name();
    }
    names.join(", ")
}

/// A step as the steps list writes it: `Walk: Holtburg`, `Portal: ...`,
/// `Recall: ...` (the distance has its own column), ` [notes]` when its
/// place has notes.
#[must_use]
pub fn step_text(step: &RouteStep) -> String {
    let full = step.to_string();
    let text = match step.kind {
        RouteStepKind::Travel if !full.starts_with("Arrived:") => {
            format!("Walk: {}", step.to.name)
        }
        _ => full,
    };
    if details_location(step).notes.trim().is_empty() {
        text
    } else {
        format!("{text} [notes]")
    }
}

/// The step's place for its details: where a portal stands, else where the
/// step goes.
#[must_use]
pub fn details_location(step: &RouteStep) -> &Rc<Location> {
    if step.kind == RouteStepKind::Portal {
        &step.from
    } else {
        &step.to
    }
}

/// A step's icon, as the original GoArrow's route list: the icon of its
/// place (the portal for a walk to it and for its use), a recall's own.
#[must_use]
pub fn step_icon(step: &RouteStep) -> u32 {
    match step.kind {
        RouteStepKind::Travel | RouteStepKind::Portal => place_icon(details_location(step)),
        RouteStepKind::Recall => RECALL_ICON,
    }
}

fn text_of(value: &ActionValue) -> &str {
    match value {
        ActionValue::Text(text) => text,
        _ => "",
    }
}

fn int_of(value: &ActionValue) -> Option<i32> {
    match value {
        ActionValue::Int(v) => Some(*v),
        _ => None,
    }
}

fn index(i: usize) -> i32 {
    i32::try_from(i).unwrap_or(i32::MAX)
}

fn metres_text(metres: f64) -> String {
    format_travel_distance(metres / METRES_PER_MAP_UNIT)
}

/// Every name the markup binds, with its kind.
pub const BINDINGS: &[(&str, K)] = &[
    ("RouteTab", K::Bool),
    ("PlacesTab", K::Bool),
    ("OptionsTab", K::Bool),
    ("ShowRoute", K::Action),
    ("ShowPlaces", K::Action),
    ("ShowOptions", K::Action),
    ("Status", K::Text),
    // Route: where to and from.
    ("ToText", K::Text),
    ("TypeTo", K::TextAction),
    ("SubmitTo", K::TextAction),
    ("FromText", K::Text),
    ("TypeFrom", K::TextAction),
    ("SubmitFrom", K::TextAction),
    ("SuggestionUp", K::Action),
    ("SuggestionDown", K::Action),
    ("IsFavourite", K::Bool),
    ("ToggleFavourite", K::Action),
    ("ShowSuggestions", K::Bool),
    ("SuggestionsLabel", K::Text),
    ("Suggestions", K::TextList),
    ("SuggestionSelected", K::Int),
    ("PickSuggestion", K::IntAction),
    // Route: the summary.
    ("ChoiceText", K::Text),
    ("CanPrevious", K::Bool),
    ("CanNext", K::Bool),
    ("Previous", K::Action),
    ("Next", K::Action),
    ("Profiles", K::TextList),
    ("Profile", K::Text),
    ("SetProfile", K::TextAction),
    ("WalkChip", K::Text),
    ("PortalChip", K::Text),
    ("LineChip", K::Text),
    // Route: the steps.
    ("ShowSteps", K::Bool),
    ("SelectedStep", K::Int),
    ("SelectStep", K::IntAction),
    ("StepNumbers", K::TextList),
    ("StepIcons", K::UintList),
    ("StepTexts", K::TextList),
    ("StepWalks", K::TextList),
    ("ShowIndoor", K::Bool),
    ("IndoorSummary", K::Text),
    ("IndoorWaypoints", K::TextList),
    ("IndoorSelected", K::Int),
    // Route: the details.
    ("DetailTitle", K::Text),
    ("DetailWhere", K::Text),
    ("DetailArrival", K::Text),
    ("DetailNote", K::Text),
    ("CanMarkFaulty", K::Bool),
    ("MarkFaulty", K::Action),
    // Route: status and buttons.
    ("StatusText", K::Text),
    ("AutoNavigate", K::Bool),
    ("SetAutoNavigate", K::BoolAction),
    ("Recalculate", K::Bool),
    ("SetRecalculate", K::BoolAction),
    ("GoLabel", K::Text),
    ("CanGo", K::Bool),
    ("GoStop", K::Action),
    ("CanResume", K::Bool),
    ("Resume", K::Action),
    ("HasDestination", K::Bool),
    ("Clear", K::Action),
    // Places.
    ("PlaceQuery", K::Text),
    ("SetPlaceQuery", K::TextAction),
    ("PlaceKind", K::Int),
    ("SetPlaceKind", K::IntAction),
    ("FavouritesOnly", K::Bool),
    ("SetFavouritesOnly", K::BoolAction),
    ("PlaceCount", K::Text),
    ("PlaceSelected", K::Int),
    ("SelectPlace", K::IntAction),
    ("PlaceIcons", K::UintList),
    ("PlaceNames", K::TextList),
    ("PlaceWheres", K::TextList),
    ("PlaceAways", K::TextList),
    ("RouteToLabel", K::Text),
    ("HasPlace", K::Bool),
    ("RouteToPlace", K::Action),
    ("PlaceFavLabel", K::Text),
    ("TogglePlaceFavourite", K::Action),
    ("MarkName", K::Text),
    ("SetMarkName", K::TextAction),
    ("SubmitMark", K::TextAction),
    ("MarkHere", K::Action),
    // Options.
    ("DataUrl", K::Text),
    ("SetDataUrl", K::TextAction),
    ("SubmitDataUrl", K::TextAction),
    ("Download", K::Action),
    ("UseSuggestedData", K::Action),
    ("DataStatus", K::Text),
    ("ShowArrow", K::Bool),
    ("SetShowArrow", K::BoolAction),
    ("ShowMap", K::Bool),
    ("SetShowMap", K::BoolAction),
    ("ShowRouteLine", K::Bool),
    ("SetShowRouteLine", K::BoolAction),
    ("ShowDungeonMap", K::Bool),
    ("ShowAllLocations", K::Bool),
    ("SetShowDungeonMap", K::BoolAction),
    ("SetShowAllLocations", K::BoolAction),
    ("ShowDistance", K::Bool),
    ("SetShowDistance", K::BoolAction),
    ("ShowBearing", K::Bool),
    ("SetShowBearing", K::BoolAction),
    ("ArrivalDistance", K::Float),
    ("SetArrivalDistance", K::FloatAction),
    ("UseAutomation", K::Bool),
    ("SetUseAutomation", K::BoolAction),
    ("Fixes", K::TextList),
    ("FixSelected", K::Int),
    ("ReloadCorrections", K::Action),
    ("HasPendingArrival", K::Bool),
    ("KeepArrival", K::Action),
    ("FixesStatus", K::Text),
];

/// The window as the client registers it: on the shelf with the game's
/// arrow ("GA" when it cannot be drawn), open at start unless the settings
/// say otherwise.
#[must_use]
pub fn panel_spec(settings: &Settings) -> PanelSpec {
    let spec = BINDINGS.iter().fold(
        PanelSpec::new(WINDOW_ID, TITLE, MARKUP)
            .binding_type("GoArrowWindow")
            .icon_text("GA")
            .icon(ICON),
        |spec, (name, kind)| spec.bind_typed(name, *kind),
    );
    if settings.panel_visible {
        spec
    } else {
        spec.start_hidden()
    }
}

/// A row of the Places list.
#[derive(Debug, Clone)]
pub struct PlaceRow {
    /// The place.
    pub location: Rc<Location>,
    /// How far, metres (NaN when the player's position is not known).
    pub metres: f64,
}

/// The window's own state, beside the plugin's.
#[derive(Debug)]
pub struct Window {
    panel: Panel,
    tab: Tab,
    to_text: String,
    from_text: String,
    editing: Option<Field>,
    suggestion: i32,
    selected_step: usize,
    route_key: Option<(String, usize, usize)>,
    place_query: String,
    place_kind: usize,
    favourites_only: bool,
    place_selected: Option<usize>,
    /// A place to select once the Places list is next made (the map's
    /// Show Details).
    place_to_select: Option<String>,
    shown_places: Vec<PlaceRow>,
    mark_name: String,
    url_text: String,
    url_status: String,
    fixes_status: String,
    places_pushed: bool,
    ticks: u64,
    last_refresh: u64,
}

impl Window {
    /// Registers the window.
    ///
    /// # Errors
    /// The client refused the markup or its bindings.
    pub fn open(host: &mut Host<'_>, settings: &Settings) -> Result<Self, String> {
        let panel = host.register_panel(&panel_spec(settings))?;
        Ok(Self {
            panel,
            tab: Tab::Route,
            to_text: settings.destination_name.clone(),
            from_text: WHERE_I_STAND.to_owned(),
            editing: None,
            suggestion: -1,
            selected_step: 0,
            route_key: None,
            place_query: String::new(),
            place_kind: 0,
            favourites_only: false,
            place_selected: None,
            place_to_select: None,
            shown_places: Vec::new(),
            mark_name: String::new(),
            url_text: settings.external_data_url.clone(),
            url_status: String::new(),
            fixes_status: String::new(),
            places_pushed: false,
            ticks: 0,
            last_refresh: 0,
        })
    }

    /// The panel's number (its events' `panel`).
    #[must_use]
    pub fn panel_id(&self) -> u32 {
        self.panel.id()
    }

    /// The tab shown.
    #[must_use]
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The field the suggestions are for, if they show.
    #[must_use]
    pub fn editing(&self) -> Option<Field> {
        self.editing
    }

    /// Opens the window on its Route tab (the map's Start and End Route
    /// Here), with From reading `from` when given.
    pub fn show_route(&mut self, host: &mut Host<'_>, from: Option<&str>) {
        self.tab = Tab::Route;
        self.editing = None;
        self.selected_step = 0;
        if let Some(from) = from {
            from.clone_into(&mut self.from_text);
        }
        self.panel.show(host, true);
    }

    /// Opens the window on its Places tab with one place found and
    /// selected (the map's Show Details).
    pub fn show_place(&mut self, host: &mut Host<'_>, name: &str) {
        self.tab = Tab::Places;
        self.editing = None;
        name.clone_into(&mut self.place_query);
        self.place_kind = 0;
        self.favourites_only = false;
        self.place_to_select = Some(name.to_owned());
        self.panel.show(host, true);
    }

    /// The rows the Places list shows.
    #[must_use]
    pub fn shown_places(&self) -> &[PlaceRow] {
        &self.shown_places
    }

    /// One tick: keeps the To field on the destination's name, and
    /// refreshes every [`REFRESH_TICKS`] while the window is open.
    pub fn tick(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        self.ticks += 1;
        if self.ticks - self.last_refresh < REFRESH_TICKS {
            return;
        }
        self.last_refresh = self.ticks;
        if self.panel.is_visible(host) {
            self.refresh(app, host);
        }
    }

    /// An event the plugin heard.
    pub fn on_event(&mut self, _host: &mut Host<'_>, event: &Event) {
        if let Event::LoginComplete = event {
            self.last_refresh = 0;
        }
    }

    /// One of the window's controls was used; the window is refreshed.
    pub fn act(&mut self, app: &mut GoArrow, host: &mut Host<'_>, action: &UiAction) {
        self.handle(app, host, &action.handler, &action.value);
        self.refresh(app, host);
    }

    #[allow(clippy::too_many_lines)]
    fn handle(
        &mut self,
        app: &mut GoArrow,
        host: &mut Host<'_>,
        handler: &str,
        value: &ActionValue,
    ) {
        let text = text_of(value).to_owned();
        match handler {
            "ShowRoute" => self.tab = Tab::Route,
            "ShowPlaces" => {
                self.editing = None;
                self.tab = Tab::Places;
            }
            "ShowOptions" => {
                self.editing = None;
                self.tab = Tab::Options;
            }
            "TypeTo" => {
                self.to_text = text;
                self.editing = Some(Field::To);
                self.suggestion = -1;
            }
            "TypeFrom" => {
                self.from_text = text;
                self.editing = Some(Field::From);
                self.suggestion = -1;
            }
            "SubmitTo" => {
                self.to_text = text;
                self.editing = Some(Field::To);
                self.submit_field(app, host);
            }
            "SubmitFrom" => {
                self.from_text = text;
                self.editing = Some(Field::From);
                self.submit_field(app, host);
            }
            "SuggestionUp" => self.move_suggestion(app, -1),
            "SuggestionDown" => self.move_suggestion(app, 1),
            "PickSuggestion" => {
                if let Some(i) = int_of(value).and_then(|i| usize::try_from(i).ok()) {
                    self.pick(app, host, i);
                }
            }
            "ToggleFavourite" => {
                let name = app.destination().target_name().to_owned();
                if !name.is_empty() {
                    if app.settings().is_favorite(&name) {
                        app.remove_favorite(host, &name);
                    } else {
                        app.add_favorite(host, &name);
                    }
                }
            }
            "Previous" => {
                app.cycle_route_alternative(host, -1);
                self.selected_step = 0;
            }
            "Next" => {
                app.cycle_route_alternative(host, 1);
                self.selected_step = 0;
            }
            "SetProfile" => {
                if let Some(i) = PROFILES.iter().position(|p| *p == text)
                    && let Some(profile) = RouteCostProfile::from_index(index(i))
                {
                    app.set_profile(host, profile);
                }
            }
            "SelectStep" => {
                if let Some(i) = int_of(value).and_then(|i| usize::try_from(i).ok()) {
                    self.selected_step = i;
                }
            }
            "MarkFaulty" => {
                let count = app
                    .destination()
                    .current_route()
                    .map_or(0, |r| r.step_count());
                if self.selected_step < count {
                    let line = app.block_route_step(host, self.selected_step, "");
                    say(host, &line);
                }
            }
            "SetAutoNavigate" => {
                if let Some(on) = value.as_bool() {
                    app.update_settings(host, |s| s.auto_navigate = on);
                }
            }
            "SetRecalculate" => {
                if let Some(on) = value.as_bool() {
                    app.update_settings(host, |s| s.recalculate_route = on);
                }
            }
            "GoStop" => {
                if app.is_navigating() {
                    app.stop_navigation(host);
                } else {
                    self.go(app, host);
                }
            }
            "Resume" => app.resume(host),
            "Clear" => {
                app.clear_destination(host);
                self.to_text.clear();
                self.editing = None;
            }
            "SetPlaceQuery" => {
                self.place_query = text;
                self.place_selected = None;
            }
            "SetPlaceKind" => {
                if let Some(i) = int_of(value).and_then(|i| usize::try_from(i).ok())
                    && i < PLACE_KINDS.len()
                {
                    self.place_kind = i;
                    self.place_selected = None;
                }
            }
            "SetFavouritesOnly" => {
                if let Some(on) = value.as_bool() {
                    self.favourites_only = on;
                    self.place_selected = None;
                }
            }
            "SelectPlace" => {
                self.place_selected = int_of(value)
                    .and_then(|i| usize::try_from(i).ok())
                    .filter(|i| *i < self.shown_places.len());
            }
            "RouteToPlace" => {
                if let Some(row) = self.selected_place().cloned() {
                    app.set_destination_location(host, row.location.clone());
                    self.to_text.clone_from(&row.location.name);
                    self.editing = None;
                    self.selected_step = 0;
                    self.tab = Tab::Route;
                }
            }
            "TogglePlaceFavourite" => {
                if let Some(row) = self.selected_place().cloned() {
                    let name = &row.location.name;
                    if app.settings().is_favorite(name) {
                        app.remove_favorite(host, name);
                    } else {
                        app.add_favorite(host, name);
                    }
                }
            }
            "SetMarkName" => self.mark_name = text,
            "SubmitMark" => {
                self.mark_name = text;
                self.mark(app, host);
            }
            "MarkHere" => self.mark(app, host),
            "SetDataUrl" => {
                self.url_text = text;
                self.url_status.clear();
            }
            "SubmitDataUrl" => {
                self.url_text = text;
                self.save_url(app, host);
            }
            "Download" if self.save_url(app, host) => {
                self.url_status.clear();
                app.update_data(host);
            }
            "UseSuggestedData" => {
                SUGGESTED_DATA_URL.clone_into(&mut self.url_text);
                self.url_status.clear();
            }
            "SetShowArrow" => {
                if let Some(on) = value.as_bool() {
                    app.set_arrow_visible(host, on);
                }
            }
            "SetShowMap" => {
                if let Some(on) = value.as_bool() {
                    app.set_map_visible(host, on);
                }
            }
            "SetShowRouteLine" => {
                if let Some(on) = value.as_bool() {
                    app.set_route_line_visible(host, on);
                }
            }
            "SetShowDungeonMap" => {
                if let Some(on) = value.as_bool() {
                    app.set_dungeon_map_visible(host, on);
                }
            }
            "SetShowAllLocations" => {
                if let Some(on) = value.as_bool() {
                    app.update_settings(host, |s| s.show_locations_all_zooms = on);
                }
            }
            "SetShowDistance" => {
                if let Some(on) = value.as_bool() {
                    app.update_settings(host, |s| s.show_distance = on);
                }
            }
            "SetShowBearing" => {
                if let Some(on) = value.as_bool() {
                    app.update_settings(host, |s| s.show_bearing = on);
                }
            }
            "SetArrivalDistance" => {
                if let ActionValue::Float(metres) = value {
                    let metres = f64::from(*metres);
                    let metres = if metres > 0.0 { metres } else { 0.5 };
                    app.update_settings(host, |s| s.arrival_distance = metres);
                }
            }
            "SetUseAutomation" => {
                if let Some(on) = value.as_bool() {
                    app.update_settings(host, |s| s.use_navigation_automation = on);
                }
            }
            "ReloadCorrections" => self.fixes_status = app.load_corrections(host),
            "KeepArrival" => self.fixes_status = app.save_observed_arrival(host),
            _ => {}
        }
    }

    fn go(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        self.selected_step = 0;
        app.go_pressed(host);
    }

    fn mark(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        let name = self.mark_name.trim().to_owned();
        if app.mark_indoor(host, &name) {
            say(host, &format!("GoArrow: Saved indoor location '{name}'."));
            self.mark_name.clear();
            self.to_text = name;
        } else {
            say(
                host,
                "GoArrow: Stand at an indoor point and use /go mark <name>.",
            );
        }
    }

    fn save_url(&mut self, app: &mut GoArrow, host: &mut Host<'_>) -> bool {
        if app.set_external_data_url(host, &self.url_text) {
            self.url_text.clone_from(&app.settings().external_data_url);
            "Location data URL saved.".clone_into(&mut self.url_status);
            true
        } else {
            "Enter a valid http:// or https:// URL.".clone_into(&mut self.url_status);
            false
        }
    }

    fn selected_place(&self) -> Option<&PlaceRow> {
        self.place_selected.and_then(|i| self.shown_places.get(i))
    }

    /// The suggestions shown now.
    #[must_use]
    pub fn current_suggestions(&self, app: &GoArrow) -> Vec<String> {
        match self.editing {
            Some(Field::To) => suggestions(app, &self.to_text, CURRENT_LOCATION),
            Some(Field::From) => suggestions(app, &self.from_text, WHERE_I_STAND),
            None => Vec::new(),
        }
    }

    fn move_suggestion(&mut self, app: &GoArrow, direction: i32) {
        let count = index(self.current_suggestions(app).len());
        if count == 0 {
            return;
        }
        self.suggestion = if self.suggestion < 0 {
            if direction > 0 { 0 } else { count - 1 }
        } else {
            (self.suggestion + direction + count) % count
        };
    }

    fn pick(&mut self, app: &mut GoArrow, host: &mut Host<'_>, i: usize) {
        let list = self.current_suggestions(app);
        let Some(choice) = list.get(i).cloned() else {
            return;
        };
        self.suggestion = -1;
        match self.editing {
            Some(Field::To) => {
                self.to_text = choice;
                self.submit_to(app, host);
            }
            Some(Field::From) => {
                self.from_text = choice;
                self.submit_from(app, host);
            }
            None => {}
        }
    }

    fn submit_field(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        if let Ok(i) = usize::try_from(self.suggestion) {
            self.pick(app, host, i);
            return;
        }
        match self.editing {
            Some(Field::To) => self.submit_to(app, host),
            Some(Field::From) => self.submit_from(app, host),
            None => {}
        }
    }

    fn submit_to(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        let value = self.to_text.trim().to_owned();
        if value.is_empty() {
            return;
        }
        if value.eq_ignore_ascii_case(CURRENT_LOCATION) {
            if !app.set_current_location_destination(host) {
                say(host, "GoArrow: Current location is unavailable.");
                return;
            }
            CURRENT_LOCATION.clone_into(&mut self.to_text);
        } else if app.try_set_coordinate_destination(host, &value)
            || app.set_destination_by_name(host, &value)
        {
            self.to_text = value;
        } else {
            say(
                host,
                "GoArrow: Select a destination from the search results.",
            );
            return;
        }
        self.selected_step = 0;
        self.editing = None;
    }

    fn submit_from(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        let value = self.from_text.trim().to_owned();
        if value.is_empty()
            || value.eq_ignore_ascii_case(WHERE_I_STAND)
            || value.eq_ignore_ascii_case(CURRENT_LOCATION)
        {
            app.set_route_from(host, None);
            WHERE_I_STAND.clone_into(&mut self.from_text);
        } else if app.set_route_from(host, Some(&value)) {
            self.from_text = value;
        } else {
            say(
                host,
                "GoArrow: Select a From location from the search results.",
            );
            return;
        }
        self.selected_step = 0;
        self.editing = None;
    }

    /// Pushes what the tab shown shows; the panel sends only what changed.
    pub fn refresh(&mut self, app: &mut GoArrow, host: &mut Host<'_>) {
        let here = Here::read(host);
        // GoArrow kept the destination field on the destination's name
        // while no search was open.
        let target = app.destination().target_name().to_owned();
        if self.editing != Some(Field::To)
            && self.to_text != target
            && (app.destination().has_destination()
                || app.settings().destination_name != CURRENT_LOCATION)
        {
            self.to_text = target;
        }
        let status = if app.location_status().is_empty() {
            String::new()
        } else {
            app.location_status().to_owned()
        };
        let mut values: Vec<(&'static str, TypedValue)> = vec![
            ("RouteTab", (self.tab == Tab::Route).into()),
            ("PlacesTab", (self.tab == Tab::Places).into()),
            ("OptionsTab", (self.tab == Tab::Options).into()),
            ("Status", status.into()),
        ];
        self.route_values(app, &here, &mut values);
        if self.tab == Tab::Places || !self.places_pushed {
            self.places_pushed = true;
            self.place_values(app, &here, &mut values);
        }
        self.option_values(app, host, &mut values);
        for (name, value) in values {
            self.panel.set(name, value);
        }
        if let Err(error) = self.panel.flush(host) {
            host.warn(&format!(
                "GoArrow: The window could not be updated: {error}"
            ));
        }
        self.last_refresh = self.ticks;
    }

    #[allow(clippy::too_many_lines)]
    fn route_values(
        &mut self,
        app: &GoArrow,
        here: &Here,
        values: &mut Vec<(&'static str, TypedValue)>,
    ) {
        let destination = app.destination();
        let view = app.navigator().view();
        let route = destination.current_route();
        let steps: Vec<RouteStep> = route.map(|r| r.steps().to_vec()).unwrap_or_default();
        let waypoints = &view.dungeon_waypoints;
        let key = (
            destination.target_name().to_owned(),
            destination.alternative_index(),
            steps.len(),
        );
        if self.route_key.as_ref() != Some(&key) {
            self.route_key = Some(key);
            self.selected_step = 0;
        }
        let rows = steps.len() + waypoints.len();
        if self.selected_step >= rows {
            self.selected_step = 0;
        }

        // The suggestions.
        let suggestions = self.current_suggestions(app);
        let label = match self.editing {
            Some(Field::From) => "From matches",
            _ => "Destination matches",
        };

        // The summary.
        let indoor = view.indoor_preview.as_ref();
        let choice = if indoor.is_some() {
            "Indoor path".to_owned()
        } else if route.is_none() {
            "No route".to_owned()
        } else if destination.alternative_count() > 0 {
            format!(
                "Route {} of {}{}",
                destination.alternative_index() + 1,
                destination.alternative_count(),
                if destination.alternative_has_more() {
                    "+"
                } else {
                    ""
                }
            )
        } else {
            "Route 1".to_owned()
        };
        let walk: f64 = steps
            .iter()
            .filter(|s| s.kind == RouteStepKind::Travel)
            .map(|s| s.distance)
            .sum();
        let interactions = steps
            .iter()
            .filter(|s| s.kind != RouteStepKind::Travel)
            .count();
        let (walk_chip, portal_chip) = if route.is_some() {
            (
                format!("Walk {}", format_travel_distance(walk)),
                match interactions {
                    1 => "1 portal or recall".to_owned(),
                    n => format!("{n} portals or recalls"),
                },
            )
        } else {
            (String::new(), String::new())
        };
        let line = app.straight_line_metres(here);
        let line_chip = if line.is_finite() {
            format!("Straight line {}", metres_text(line))
        } else {
            String::new()
        };

        // The steps.
        let mut numbers = Vec::with_capacity(rows);
        let mut icons = Vec::with_capacity(rows);
        let mut texts = Vec::with_capacity(rows);
        let mut walks = Vec::with_capacity(rows);
        for (i, step) in steps.iter().enumerate() {
            numbers.push((i + 1).to_string());
            icons.push(step_icon(step));
            texts.push(step_text(step));
            walks.push(
                if step.kind == RouteStepKind::Travel && hub_at(&steps, step.to.coords).is_none() {
                    format_travel_distance(step.distance)
                } else {
                    String::new()
                },
            );
        }
        for (i, waypoint) in waypoints.iter().enumerate() {
            numbers.push((steps.len() + i + 1).to_string());
            icons.push(self::icons::CUSTOM);
            texts.push(waypoint.label.clone());
            walks.push(String::new());
        }

        // The details.
        let (title, where_text, arrival, note, can_block) = self.details(&steps, waypoints, rows);

        let (indoor_summary, indoor_rows) = indoor.map_or_else(
            || (String::new(), Vec::new()),
            |p| (p.summary.clone(), p.waypoints.clone()),
        );
        let has_destination = destination.has_destination();
        let navigating = app.is_navigating();
        values.extend([
            ("ToText", self.to_text.as_str().into()),
            ("FromText", self.from_text.as_str().into()),
            (
                "IsFavourite",
                (has_destination && app.settings().is_favorite(destination.target_name())).into(),
            ),
            (
                "ShowSuggestions",
                (self.editing.is_some() && !suggestions.is_empty()).into(),
            ),
            ("SuggestionsLabel", label.into()),
            ("Suggestions", suggestions.into()),
            ("SuggestionSelected", self.suggestion.into()),
            ("ChoiceText", choice.into()),
            ("CanPrevious", app.can_previous().into()),
            ("CanNext", app.can_next().into()),
            (
                "Profiles",
                PROFILES
                    .iter()
                    .map(|p| (*p).to_owned())
                    .collect::<Vec<_>>()
                    .into(),
            ),
            (
                "Profile",
                PROFILES[usize::try_from(destination.profile.index()).unwrap_or(0)].into(),
            ),
            ("WalkChip", walk_chip.into()),
            ("PortalChip", portal_chip.into()),
            ("LineChip", line_chip.into()),
            ("ShowSteps", indoor.is_none().into()),
            ("ShowIndoor", indoor.is_some().into()),
            ("IndoorSummary", indoor_summary.into()),
            ("IndoorWaypoints", indoor_rows.into()),
            ("IndoorSelected", (-1).into()),
            (
                "SelectedStep",
                (if rows == 0 {
                    -1
                } else {
                    index(self.selected_step)
                })
                .into(),
            ),
            ("StepNumbers", numbers.into()),
            ("StepIcons", icons.into()),
            ("StepTexts", texts.into()),
            ("StepWalks", walks.into()),
            ("DetailTitle", title.into()),
            ("DetailWhere", where_text.into()),
            ("DetailArrival", arrival.into()),
            ("DetailNote", note.into()),
            ("CanMarkFaulty", can_block.into()),
            ("StatusText", app.nav_status_text(here).into()),
            ("AutoNavigate", app.settings().auto_navigate.into()),
            ("Recalculate", app.settings().recalculate_route.into()),
            ("GoLabel", (if navigating { "Stop" } else { "Go" }).into()),
            ("CanGo", (navigating || has_destination).into()),
            (
                "CanResume",
                (view.can_resume && !app.is_computing_route()).into(),
            ),
            ("HasDestination", has_destination.into()),
        ]);
    }

    /// The details box: title, where, arrival or kind, the notes' first
    /// line, and whether Mark faulty applies.
    fn details(
        &self,
        steps: &[RouteStep],
        waypoints: &[crate::navigator::DungeonWaypoint],
        rows: usize,
    ) -> (String, String, String, String, bool) {
        if rows == 0 {
            return (
                "Select a route step to see details.".to_owned(),
                String::new(),
                String::new(),
                String::new(),
                false,
            );
        }
        if let Some(step) = steps.get(self.selected_step) {
            let location = details_location(step);
            let title = format!(
                "Step {} of {} · {}",
                self.selected_step + 1,
                steps.len(),
                location.name
            );
            let coords = if location.has_coordinates() {
                location.coords.to_string()
            } else {
                "Unknown".to_owned()
            };
            // A hub's made-up point is no place on the map: it is named.
            let hub = hub_at(steps, location.coords);
            let where_text = match (step.kind, hub) {
                (RouteStepKind::Travel, Some(hub)) => format!("Walk inside {hub}"),
                (RouteStepKind::Travel, None) => format!(
                    "Walk to {coords} · {}",
                    format_travel_distance(step.distance)
                ),
                (_, Some(hub)) => format!("Use in {hub}"),
                (_, None) => format!("Use at {coords}"),
            };
            let arrives = |at: Coordinates| match hub_at(steps, at) {
                Some(hub) => format!("Arrives in {hub}"),
                None => format!("Arrives {at}"),
            };
            let arrival = if step.kind == RouteStepKind::Portal {
                let at = if step.from.has_exit_coords() {
                    step.from.exit_coords()
                } else {
                    step.to.coords
                };
                arrives(at)
            } else if location.has_exit_coords() {
                arrives(location.exit_coords())
            } else if step.kind == RouteStepKind::Travel && location.kind == LocationType::UNKNOWN {
                "Walk".to_owned()
            } else {
                kind_text(location.kind)
            };
            let note = location
                .notes
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map_or_else(
                    || "No additional notes for this location.".to_owned(),
                    str::to_owned,
                );
            return (title, where_text, arrival, note, true);
        }
        let i = self.selected_step - steps.len();
        let Some(waypoint) = waypoints.get(i) else {
            return (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                false,
            );
        };
        let position =
            openac_plugin::world::Position::from_cell(waypoint.cell_id, waypoint.local, 0.0);
        let coords = Coordinates::new(position.north_south, position.east_west);
        (
            format!("Dungeon waypoint {} of {}", i + 1, waypoints.len()),
            format!("{coords}, floor {:.0} m", waypoint.local[2]),
            "Walk".to_owned(),
            waypoint.label.clone(),
            false,
        )
    }

    /// The Places list: filtered, nearest first, at most [`MAX_PLACES`].
    fn place_values(
        &mut self,
        app: &GoArrow,
        here: &Here,
        values: &mut Vec<(&'static str, TypedValue)>,
    ) {
        let origin = app.origin_coordinates(here);
        let query = text::fold(self.place_query.trim());
        let selected_name = self
            .place_to_select
            .take()
            .or_else(|| self.selected_place().map(|r| r.location.name.clone()));
        let mut rows: Vec<PlaceRow> = app
            .database()
            .all_locations()
            .iter()
            .filter(|l| !l.is_retired)
            .filter(|l| place_kind_matches(l, self.place_kind))
            .filter(|l| query.is_empty() || text::fold(&l.name).contains(&query))
            .filter(|l| !self.favourites_only || app.settings().is_favorite(&l.name))
            .map(|l| PlaceRow {
                location: l.clone(),
                metres: match origin {
                    Some(o) if l.has_coordinates() => o.distance_to(l.coords) * METRES_PER_MAP_UNIT,
                    _ => f64::NAN,
                },
            })
            .collect();
        let total = rows.len();
        rows.sort_by(|a, b| {
            let key = |r: &PlaceRow| {
                if r.metres.is_finite() {
                    r.metres
                } else {
                    f64::MAX
                }
            };
            key(a)
                .total_cmp(&key(b))
                .then_with(|| text::cmp_ignore_case(&a.location.name, &b.location.name))
        });
        rows.truncate(MAX_PLACES);
        self.place_selected =
            selected_name.and_then(|name| rows.iter().position(|r| r.location.name == name));
        let count = if total > rows.len() {
            format!("{} of {total} shown · nearest first", rows.len())
        } else {
            format!("{total} shown · nearest first")
        };
        let selected = self.place_selected.and_then(|i| rows.get(i));
        let route_label = selected.map_or_else(
            || "Route to the selected place".to_owned(),
            |r| format!("Route to {}", r.location.name),
        );
        let fav_label = match selected {
            Some(r) if app.settings().is_favorite(&r.location.name) => "Unfavourite",
            _ => "Favourite",
        };
        let names: Vec<String> = rows
            .iter()
            .map(|r| {
                let star = if app.settings().is_favorite(&r.location.name) {
                    "* "
                } else {
                    ""
                };
                format!("{star}{} · {}", r.location.name, kind_text(r.location.kind))
            })
            .collect();
        values.extend([
            ("PlaceQuery", self.place_query.as_str().into()),
            ("PlaceKind", index(self.place_kind).into()),
            ("FavouritesOnly", self.favourites_only.into()),
            ("PlaceCount", count.into()),
            (
                "PlaceSelected",
                self.place_selected.map_or(-1, index).into(),
            ),
            (
                "PlaceIcons",
                rows.iter()
                    .map(|r| place_icon(&r.location))
                    .collect::<Vec<_>>()
                    .into(),
            ),
            ("PlaceNames", names.into()),
            (
                "PlaceWheres",
                rows.iter()
                    .map(|r| r.location.coords.to_string())
                    .collect::<Vec<_>>()
                    .into(),
            ),
            (
                "PlaceAways",
                rows.iter()
                    .map(|r| {
                        if r.metres.is_finite() {
                            metres_text(r.metres)
                        } else {
                            String::new()
                        }
                    })
                    .collect::<Vec<_>>()
                    .into(),
            ),
            ("RouteToLabel", route_label.into()),
            ("HasPlace", selected.is_some().into()),
            ("PlaceFavLabel", fav_label.into()),
            ("MarkName", self.mark_name.as_str().into()),
        ]);
        self.shown_places = rows;
    }

    fn option_values(
        &mut self,
        app: &GoArrow,
        host: &mut Host<'_>,
        values: &mut Vec<(&'static str, TypedValue)>,
    ) {
        let s = app.settings();
        let data_status = if self.url_status.is_empty() {
            let offset = host.clock().utc_offset_minutes;
            app.data_status(offset)
        } else {
            self.url_status.clone()
        };
        let merged = AtlasCorrections::merged(app.corrections());
        let mut fixes = Vec::new();
        for arrival in &merged.arrivals {
            fixes.push(format!("{}: arrives {}", arrival.name, arrival.arrival));
        }
        for step in &merged.blocked_steps {
            let note = if step.note.trim().is_empty() {
                String::new()
            } else {
                format!(" ({})", step.note.trim())
            };
            fixes.push(format!(
                "Blocked {}: {} to {}{note}",
                step.kind, step.from, step.to
            ));
        }
        values.extend([
            ("DataUrl", self.url_text.as_str().into()),
            ("DataStatus", data_status.into()),
            ("ShowArrow", s.hud_visible.into()),
            ("ShowMap", s.map_visible.into()),
            ("ShowRouteLine", s.route_line_visible.into()),
            ("ShowDungeonMap", s.dungeon_map_visible.into()),
            ("ShowAllLocations", s.show_locations_all_zooms.into()),
            ("ShowDistance", s.show_distance.into()),
            ("ShowBearing", s.show_bearing.into()),
            ("ArrivalDistance", (s.arrival_distance as f32).into()),
            ("UseAutomation", s.use_navigation_automation.into()),
            ("Fixes", fixes.into()),
            ("FixSelected", (-1).into()),
            (
                "HasPendingArrival",
                app.navigator().view().pending_arrival.is_some().into(),
            ),
            ("FixesStatus", self.fixes_status.as_str().into()),
        ]);
    }
}
