//! The plugin (`GoArrowPlugin.cs`): its lifecycle, the destination and
//! route work the window, the overlays and `/go` use, the location data
//! (loaded a little every tick), the corrections, recalls and marks.
//!
//! Every call into the plugin stays short: reading location data, building
//! the route graph, route searches and alternatives searches run as the
//! core's step jobs, a few milliseconds of them per tick, and say
//! "Computing route..." meanwhile. Searches, Go and Resume wait for the
//! graph; it is built in the background while there is a destination, so
//! the navigator's own searches find it built.

use std::collections::VecDeque;
use std::rc::Rc;

use openac_plugin::chat_coordinates;
use openac_plugin::chat_markup::escape;
use openac_plugin::net::NetEvent;
use openac_plugin::social::{RecallKind, RecallStatus};
use openac_plugin::ui::UiEvent;
use openac_plugin::world::PortalTransition;
use openac_plugin::{
    Callback, CallbackAnswer, CommandDef, Event, Host, Plugin, StartInfo, TICK_SECONDS,
};

use crate::commands;
use crate::coords::{Coordinates, METRES_PER_MAP_UNIT, NavPosition};
use crate::corrections::{self, ArrivalCorrection, AtlasCorrections, BlockedStep};
use crate::data::{
    self, CACHE_KEY, CORRECTIONS_KEY, CacheMetadata, INDOOR_KEY, LoadJob, LoadPurpose, LoadResult,
    LoadTask, METADATA_KEY,
};
use crate::database::{AtlasError, LoadError, LocationDatabase, parse_locations_xml};
use crate::destination::{CyclePlan, Destination};
use crate::finder::{AlternativesJob, RouteCostProfile, RouteFinder, RouteJob};
use crate::ground::Ground;
use crate::hud::Hud;
use crate::loading::{AtlasJob, LocationsJob};
use crate::location::{IndoorPosition, Location};
use crate::map::MapOverlay;
use crate::map_icons::MapIcons;
use crate::navigator::{NavContext, NavSettings, Navigator};
use crate::settings::{self, Settings};
use crate::sprites::Sprites;
use crate::text;
use crate::timestamp;
use crate::window::Window;

/// The verb.
pub const VERB: &str = "go";
/// Its alias.
pub const ALIAS: &str = "goarrow";
/// The plugin's id.
pub const PLUGIN_ID: &str = "openac.goarrow";
/// The name a destination at the player's own position has.
pub const CURRENT_LOCATION: &str = "Current Location";

/// `GoArrow v0.1.0`: what `/go status` starts with.
#[must_use]
pub fn display_title() -> String {
    format!("GoArrow v{}", env!("CARGO_PKG_VERSION"))
}

/// Ticks before queued Go or Resume work runs while a window is attached,
/// so a frame shows "Computing route..." first (GoArrow's 50 ms).
pub const WORK_DELAY_TICKS: u64 = 4;
/// Ticks between idle re-plans (GoArrow's 1 s).
pub const REPLAN_TICKS: u64 = (1.0 / TICK_SECONDS) as u64 + 1;
/// How far the player moves before an idle re-plan, metres.
pub const REPLAN_METRES: f64 = 20.0;
/// The most milliseconds of step work in one tick.
pub const WORK_BUDGET_MS: u64 = 8;
/// The most chunks of step work in one tick (bounds a tick when the clock
/// does not move, as under test).
pub const WORK_CHUNKS: usize = 16;
/// Units of reading work per chunk (one unit is one record).
pub const LOAD_CHUNK: usize = 64;
/// Units of route search work per chunk (one unit is one place expanded).
pub const ROUTE_CHUNK: usize = 256;
/// Units of graph building per chunk (one unit is one place's walks).
pub const GRAPH_CHUNK: usize = 128;

/// Runs `step` in chunks of `chunk` units until it finishes or this tick's
/// budget is spent.
fn budgeted<T>(
    host: &mut Host<'_>,
    chunk: usize,
    mut step: impl FnMut(usize) -> Option<T>,
) -> Option<T> {
    let start = host.clock().monotonic_ms;
    for _ in 0..WORK_CHUNKS {
        if let Some(done) = step(chunk) {
            return Some(done);
        }
        if host.clock().monotonic_ms.saturating_sub(start) >= WORK_BUDGET_MS {
            break;
        }
    }
    None
}

/// Where the character is, from one navigation snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Here {
    /// In the world, and the rest is real.
    pub available: bool,
    /// In portal space.
    pub portal_space: bool,
    /// Where.
    pub position: NavPosition,
}

impl Here {
    /// Reads the snapshot.
    pub fn read(host: &mut Host<'_>) -> Self {
        let snapshot = host.navigation();
        let p = snapshot.position;
        Self {
            available: snapshot.is_available,
            portal_space: snapshot.is_portal_space,
            position: NavPosition {
                cell_id: p.cell_id,
                east_west: p.east_west,
                north_south: p.north_south,
                elevation: p.elevation,
                heading_degrees: p.heading_degrees,
                is_outdoor: p.is_outdoor,
            },
        }
    }

    /// Outdoors in the world.
    #[must_use]
    pub fn outdoors(&self) -> bool {
        self.available && !self.portal_space && self.position.is_outdoor
    }

    /// In the world but indoors or in portal space: outdoor routes wait.
    #[must_use]
    pub fn paused(&self) -> bool {
        self.available && (self.portal_space || !self.position.is_outdoor)
    }

    /// The position as a place to plan from.
    #[must_use]
    pub fn location(&self) -> Rc<Location> {
        Rc::new(Location::new(
            "Current Position",
            self.position.north_south,
            self.position.east_west,
        ))
    }
}

/// Go or Resume, queued for a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Work {
    Go,
    Resume,
}

/// A route search; its job is made once the graph is built.
#[derive(Debug)]
struct PlanJob {
    origin: Rc<Location>,
    position: Option<NavPosition>,
    target: Rc<Location>,
    profile: RouteCostProfile,
    job: Option<RouteJob>,
}

/// An alternatives search; its job is made once the graph is built.
#[derive(Debug)]
struct CycleJob {
    direction: i32,
    origin: Rc<Location>,
    requested: usize,
    target: Rc<Location>,
    profile: RouteCostProfile,
    job: Option<AlternativesJob>,
}

/// The plugin as the client loads it.
#[derive(Debug)]
pub struct GoArrow {
    settings: Settings,
    finder: RouteFinder,
    destination: Destination,
    navigator: Navigator,
    nav_settings: NavSettings,
    window: Option<Window>,
    hud: Option<Hud>,
    map: Option<MapOverlay>,
    sprites: Sprites,
    map_icons: MapIcons,
    tick_started_ms: u64,
    ground: Ground,
    ticks: u64,
    loads: VecDeque<LoadTask>,
    loaded: bool,
    download_handle: Option<u32>,
    download_url: String,
    updating: bool,
    location_status: String,
    pending_work: Option<(Work, u64)>,
    plan: Option<PlanJob>,
    cycle: Option<CycleJob>,
    last_preview_position: Option<NavPosition>,
    last_preview_tick: Option<u64>,
    route_from: Option<Rc<Location>>,
    recall_request_revision: i64,
    corrections: Vec<AtlasCorrections>,
    metadata: Option<CacheMetadata>,
}

impl Default for GoArrow {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            finder: RouteFinder::new(LocationDatabase::new()),
            destination: Destination::new(),
            navigator: Navigator::new(),
            nav_settings: NavSettings::default(),
            window: None,
            hud: None,
            map: None,
            sprites: Sprites::default(),
            map_icons: MapIcons::default(),
            tick_started_ms: 0,
            ground: Ground::default(),
            ticks: 0,
            loads: VecDeque::new(),
            loaded: false,
            download_handle: None,
            download_url: String::new(),
            updating: false,
            location_status: String::new(),
            pending_work: None,
            plan: None,
            cycle: None,
            last_preview_position: None,
            last_preview_tick: None,
            route_from: None,
            recall_request_revision: 0,
            corrections: Vec::new(),
            metadata: None,
        }
    }
}

/// Shows a GoArrow line in chat (escaped: it may repeat what was typed or
/// downloaded).
pub fn say(host: &mut Host<'_>, text: &str) {
    host.chat_post(&escape(text));
}

impl Plugin for GoArrow {
    fn enable(&mut self, host: &mut Host<'_>, info: &StartInfo) -> Result<(), String> {
        self.settings = settings::load(host);
        self.destination.profile = self.settings.profile();
        self.load_scoped_recalls(host);
        self.load_corrections(host);
        self.read_metadata(host);
        if !self.start_loading(host) {
            self.download_at_start(host);
        }
        self.navigator.set_owner(&info.plugin_id);
        self.navigator.enable(host);
        self.use_portal_table(host);
        host.register_command_def(&CommandDef {
            verb: VERB.to_owned(),
            aliases: vec![ALIAS.to_owned()],
            description: commands::DESCRIPTION.to_owned(),
        })?;
        self.window = Some(Window::open(host, &self.settings)?);
        self.hud = Some(Hud::open(host, &self.settings)?);
        self.map = Some(MapOverlay::open(host, &self.settings)?);
        self.refresh_window(host);
        host.info("GoArrow enabled");
        Ok(())
    }

    fn tick(&mut self, host: &mut Host<'_>) {
        self.ticks += 1;
        self.tick_started_ms = host.clock().monotonic_ms;
        self.step_loads(host);
        let graph_ready = self.step_graph(host);
        if let Some((work, ready)) = self.pending_work
            && self.ticks >= ready
            && graph_ready
        {
            self.pending_work = None;
            match work {
                Work::Go => self.execute_go(host),
                Work::Resume => self.execute_resume(host),
            }
        }
        if graph_ready {
            self.step_route_jobs(host);
        }
        let here = Here::read(host);
        if here.available {
            if self.loaded
                && self.settings.destination_name == CURRENT_LOCATION
                && !self.destination.has_destination()
            {
                self.set_current_location_destination(host);
            }
            if here.paused() {
                self.last_preview_tick = None;
            }
            if self.should_replan(&here) {
                let origin = self.route_from.clone().unwrap_or_else(|| here.location());
                self.start_plan(origin, Some(here.position));
            } else if self.destination.has_destination() && here.outdoors() {
                let origin = self.route_from.clone().unwrap_or_else(|| here.location());
                self.destination.update_guidance(&origin);
            }
        }
        self.nav(host, |n, cx| n.tick(cx, TICK_SECONDS));
        if let Some(mut window) = self.window.take() {
            window.tick(self, host);
            self.window = Some(window);
        }
        self.sprites.ensure(host);
        if self.settings.map_visible {
            self.map_icons.ensure(host);
        }
        if let Some(mut hud) = self.hud.take() {
            hud.tick(self, host, &here);
            self.hud = Some(hud);
        }
        if let Some(mut map) = self.map.take() {
            map.tick(self, host, &here);
            self.map = Some(map);
        }
        let mut ground = std::mem::take(&mut self.ground);
        ground.tick(self, host, &here);
        self.ground = ground;
    }

    fn disable(&mut self, host: &mut Host<'_>) {
        self.pending_work = None;
        self.plan = None;
        self.cycle = None;
        self.navigator.disable(host);
        self.nav(host, |n, cx| n.stop(cx));
        let mut ground = std::mem::take(&mut self.ground);
        ground.clear(host);
        self.save_settings(host);
        host.info("GoArrow disabled");
    }

    fn on_event(&mut self, host: &mut Host<'_>, event: Event) {
        match &event {
            Event::Ui(UiEvent::Action(action)) => {
                if let Some(mut window) = self.window.take() {
                    if window.panel_id() == action.panel {
                        window.act(self, host, action);
                    }
                    self.window = Some(window);
                }
                return;
            }
            Event::Ui(UiEvent::Pointer(pointer)) => {
                if let Some(mut hud) = self.hud.take() {
                    if hud.canvas_id() == pointer.canvas {
                        hud.pointer(self, host, pointer);
                    }
                    self.hud = Some(hud);
                }
                if let Some(mut map) = self.map.take() {
                    if map.canvas_id() == pointer.canvas {
                        map.pointer(self, host, pointer);
                    }
                    self.map = Some(map);
                }
                return;
            }
            Event::Command(call)
                if call.verb.eq_ignore_ascii_case(VERB)
                    || call.verb.eq_ignore_ascii_case(ALIAS) =>
            {
                commands::run(self, host, &call.arguments);
                self.refresh_window(host);
                return;
            }
            Event::ChatLinkClicked(_) => {
                if let Some(c) = chat_coordinates::clicked_coordinate(&event) {
                    self.set_coordinate_destination(host, c.north_south, c.east_west, None);
                }
            }
            Event::SelectionChanged(change) => {
                if self.destination.kind() == crate::destination::DestinationKind::Object
                    && let Some(target) = self.destination.target_object_id()
                    && change.selected != Some(target)
                {
                    self.destination.mark_object_unavailable();
                }
            }
            Event::PortalTransition(transition) => self.learn_recall(host, transition),
            Event::Net(NetEvent::HttpCompleted(reply))
                if self.download_handle == Some(reply.handle) =>
            {
                self.download_handle = None;
                self.download_finished(host, reply);
            }
            Event::LoginComplete => self.load_scoped_recalls(host),
            _ => {}
        }
        if let Some(mut window) = self.window.take() {
            window.on_event(host, &event);
            self.window = Some(window);
        }
        self.sprites.on_event(&event);
        self.map_icons.on_event(&event);
        if let Some(hud) = self.hud.as_mut() {
            hud.on_event(&event);
        }
        if let Some(map) = self.map.as_mut() {
            map.on_event(&event);
        }
        self.nav(host, |n, cx| n.on_event(cx, &event));
    }

    fn callback(&mut self, _host: &mut Host<'_>, callback: Callback) -> CallbackAnswer {
        match callback {
            Callback::CommandComplete(call) => {
                CallbackAnswer::Completions(commands::complete(self, &call.arguments))
            }
            _ => CallbackAnswer::Nothing,
        }
    }
}

impl GoArrow {
    // ---- The navigator. ----

    fn sync_nav_settings(&mut self) {
        self.nav_settings = NavSettings {
            auto_navigate: self.settings.auto_navigate,
            use_navigation_automation: self.settings.use_navigation_automation,
            arrival_distance: self.settings.arrival_distance,
        };
    }

    /// Runs `f` on the navigator with this call's context.
    pub fn nav<R>(
        &mut self,
        host: &mut Host<'_>,
        f: impl FnOnce(&mut Navigator, &mut NavContext<'_, '_>) -> R,
    ) -> R {
        self.sync_nav_settings();
        let mut cx = NavContext {
            host,
            destination: &mut self.destination,
            finder: &mut self.finder,
            settings: &self.nav_settings,
        };
        f(&mut self.navigator, &mut cx)
    }

    /// The navigator.
    #[must_use]
    pub fn navigator(&self) -> &Navigator {
        &self.navigator
    }

    /// The destination and its route.
    #[must_use]
    pub fn destination(&self) -> &Destination {
        &self.destination
    }

    /// The route finder and its places.
    #[must_use]
    pub fn finder(&self) -> &RouteFinder {
        &self.finder
    }

    /// The loaded places.
    #[must_use]
    pub fn database(&self) -> &LocationDatabase {
        self.finder.database()
    }

    /// Ticks since the plugin started.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    // ---- Settings. ----

    /// The settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Writes the settings. The destination's name in them changes only
    /// where a destination is set or cleared, so a save before the saved
    /// destination is restored (or while it is not in the data) keeps it.
    pub fn save_settings(&mut self, host: &mut Host<'_>) {
        settings::save(host, &self.settings);
    }

    /// Changes the settings and saves them.
    pub fn update_settings(&mut self, host: &mut Host<'_>, change: impl FnOnce(&mut Settings)) {
        change(&mut self.settings);
        self.save_settings(host);
    }

    /// Changes the route cost profile: the route is planned again with it.
    pub fn set_profile(&mut self, host: &mut Host<'_>, profile: RouteCostProfile) {
        self.settings.route_cost_profile = profile.index();
        self.destination.profile = profile;
        self.cancel_route_jobs();
        self.destination.clear_route();
        self.last_preview_tick = None;
        self.save_settings(host);
    }

    /// Shows or hides the dungeon floorplan indoors.
    pub fn set_dungeon_map_visible(&mut self, host: &mut Host<'_>, visible: bool) {
        self.update_settings(host, |s| s.dungeon_map_visible = visible);
    }

    /// `/go lock` and `/go unlock` (the flag is kept, never read).
    pub fn set_navigation_lock(&mut self, host: &mut Host<'_>, locked: bool) {
        self.update_settings(host, |s| s.navigation_locked = locked);
    }

    // ---- Location data. ----

    /// Starts reading the cached Atlas data; false when there is none.
    fn start_loading(&mut self, host: &mut Host<'_>) -> bool {
        match host.storage_read_text(&settings::storage(), CACHE_KEY) {
            Ok(Some(text)) if !text.trim().is_empty() => self.loads.push_back(LoadTask {
                purpose: LoadPurpose::StartupCache,
                job: LoadJob::Atlas(AtlasJob::new(text)),
            }),
            Ok(_) => {}
            Err(error) => host.warn(&format!(
                "GoArrow: Ignoring invalid cached Atlas data: {error}"
            )),
        }
        if self.loads.is_empty() {
            self.finish_startup(host);
            false
        } else {
            self.location_status = "Loading places...".to_owned();
            true
        }
    }

    /// With no places cached, downloads them as the Download button does:
    /// from the saved address, else from the suggested one, which is then
    /// saved (the original waited for the player to download).
    fn download_at_start(&mut self, host: &mut Host<'_>) {
        if !self.settings.download_data_at_start {
            return;
        }
        if self.settings.external_data_url.trim().is_empty() {
            self.update_settings(host, |s| {
                s.external_data_url = data::SUGGESTED_DATA_URL.to_owned();
            });
        }
        say(
            host,
            &format!(
                "GoArrow: No places yet, so downloading them from {} (change the address in Options).",
                self.settings.external_data_url
            ),
        );
        self.update_data(host);
    }

    /// Whether the startup data has been read.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn finish_startup(&mut self, host: &mut Host<'_>) {
        self.load_saved_indoor_locations(host);
        self.loaded = true;
        if self.location_status == "Loading places..." {
            self.location_status.clear();
        }
        let name = self.settings.destination_name.clone();
        if !name.is_empty() && name != CURRENT_LOCATION {
            let database = self.finder.database();
            if let Some(location) = database.find_location(&name) {
                self.destination.set_destination(location);
                self.settings.destination_name = self.destination.saved_name().to_owned();
            }
        }
        self.refresh_window(host);
    }

    fn load_saved_indoor_locations(&mut self, host: &mut Host<'_>) {
        let Ok(Some(xml)) = host.storage_read_text(&settings::storage(), INDOOR_KEY) else {
            return;
        };
        if xml.trim().is_empty() {
            return;
        }
        match parse_locations_xml(&xml) {
            Ok(parsed) => {
                for location in parsed.locations {
                    if location.indoor_position.is_some() {
                        self.finder.database_mut().upsert_user_location(location);
                    }
                }
            }
            Err(error) => host.warn(&format!(
                "GoArrow: Ignoring invalid saved indoor locations: {error}"
            )),
        }
    }

    fn step_loads(&mut self, host: &mut Host<'_>) {
        let Some(mut task) = self.loads.pop_front() else {
            return;
        };
        match budgeted(host, LOAD_CHUNK, |n| task.job.step(n)) {
            None => self.loads.push_front(task),
            Some(result) => {
                self.load_finished(host, task.purpose, result);
                self.refresh_window(host);
            }
        }
    }

    fn replace_locations(
        &mut self,
        host: &mut Host<'_>,
        parsed: crate::database::ParsedLocations,
    ) -> usize {
        self.finder.database_mut().replace_locations(parsed);
        self.use_portal_table(host);
        self.route_data_changed();
        self.finder.database().location_count()
    }

    /// Takes the Atlas portals' arrivals from the portal table where it
    /// has them, saying in one log line how many, when Atlas data is
    /// loaded and the table is read.
    fn use_portal_table(&mut self, host: &mut Host<'_>) {
        let table = self.navigator.ace_portals();
        let database = self.finder.database_mut();
        let count = database.use_portal_table(table);
        if database.is_atlas_loaded() && !table.is_empty() {
            host.info(&format!(
                "GoArrow: {count} Atlas portal arrivals taken from the portal table."
            ));
        }
    }

    /// The places or the corrections changed: searches under way (over
    /// the old graph) are dropped, and a route shown while idle is
    /// planned again with the new data (a walk keeps its route).
    fn route_data_changed(&mut self) {
        self.finder.invalidate_graph();
        self.cancel_route_jobs();
        let view = self.navigator.view();
        if !view.is_navigating && !view.waiting_for_interaction {
            self.destination.clear_route();
            self.last_preview_tick = None;
        }
    }

    #[allow(clippy::too_many_lines)]
    fn load_finished(&mut self, host: &mut Host<'_>, purpose: LoadPurpose, result: LoadResult) {
        match (purpose, result) {
            (LoadPurpose::StartupCache, LoadResult::Atlas(result)) => {
                match result {
                    Ok((parsed, _)) => {
                        let count = self.replace_locations(host, parsed);
                        host.info(&format!("GoArrow: Loaded {count} cached Atlas locations."));
                    }
                    Err(AtlasError::Unsplit) => host.error(&unsplit_line(CACHE_KEY)),
                    Err(error) => host.warn(&format!(
                        "GoArrow: Ignoring invalid cached Atlas data: {error}"
                    )),
                }
                self.finish_startup(host);
            }
            (LoadPurpose::Download { text, url }, LoadResult::Atlas(result)) => match result {
                Ok((parsed, records)) => {
                    self.write_cache(host, &text, records, &url);
                    let count = self.replace_locations(host, parsed);
                    say(
                        host,
                        &format!("GoArrow: Loaded {count} locations from the Atlas data source."),
                    );
                    self.location_status = format!("Loaded {count} locations.");
                    self.updating = false;
                }
                Err(AtlasError::Unsplit) => {
                    let line = unsplit_line(&url);
                    self.download_failed(host, line.trim_start_matches("GoArrow: "));
                }
                Err(error) => self.download_failed(host, &error.to_string()),
            },
            (LoadPurpose::CacheFallback, LoadResult::Atlas(result)) => {
                match result {
                    Ok((parsed, _)) => {
                        let count = self.replace_locations(host, parsed);
                        say(
                            host,
                            &format!("GoArrow: Download failed; loaded {count} cached locations."),
                        );
                        self.location_status = "Download failed; loaded cached data.".to_owned();
                    }
                    Err(error) => {
                        if error == AtlasError::Unsplit {
                            host.error(&unsplit_line(CACHE_KEY));
                        } else {
                            host.warn(&format!(
                                "GoArrow: Cached location data is invalid: {error}"
                            ));
                        }
                        self.keep_existing(host);
                    }
                }
                self.updating = false;
            }
            (LoadPurpose::File { name, key }, LoadResult::Locations(result)) => match result {
                Ok(parsed) if !parsed.locations.is_empty() => {
                    let count = self.replace_locations(host, parsed);
                    host.info(&format!(
                        "GoArrow: Loaded {count} locations from storage key '{key}'."
                    ));
                    say(
                        host,
                        &format!("GoArrow: Loaded location data from '{name}'."),
                    );
                }
                Ok(_) => say(host, commands::FILE_USAGE),
                Err(LoadError::Unsplit) => {
                    host.error(&unsplit_line(&key));
                    say(host, &unsplit_line(&name));
                }
                Err(error) => {
                    host.error(&format!(
                        "GoArrow: Failed to load location data from '{key}': {error}"
                    ));
                    say(host, commands::FILE_USAGE);
                }
            },
            _ => {}
        }
    }

    fn keep_existing(&mut self, host: &mut Host<'_>) {
        say(
            host,
            "GoArrow: Location-data update failed; keeping the existing database.",
        );
        self.location_status = "Download failed; existing data kept.".to_owned();
    }

    fn write_cache(&mut self, host: &mut Host<'_>, text: &str, records: usize, url: &str) {
        let place = settings::storage();
        let metadata = CacheMetadata {
            downloaded_at: timestamp::iso8601_utc(host.clock().utc_unix_ms),
            location_count: records,
            source_url: url.to_owned(),
        };
        let json = serde_json::to_string_pretty(&metadata).unwrap_or_default();
        if let Err(error) = host
            .storage_write_text(&place, CACHE_KEY, text)
            .and_then(|()| host.storage_write_text(&place, METADATA_KEY, &json))
        {
            host.warn(&format!("GoArrow: Could not cache the Atlas data: {error}"));
        }
        self.metadata = Some(metadata);
    }

    fn read_metadata(&mut self, host: &mut Host<'_>) {
        self.metadata = host
            .storage_read_text(&settings::storage(), METADATA_KEY)
            .ok()
            .flatten()
            .and_then(|text| serde_json::from_str(&text).ok());
    }

    /// `/go file`: reads `GoArrow/<name>` over a few ticks and replaces the
    /// loaded places with it; false (the usage) when the name is not one
    /// or there is no such file.
    pub fn load_data_file(&mut self, host: &mut Host<'_>, name: &str) -> bool {
        let Some(key) = data::data_file_key(name) else {
            return false;
        };
        let Ok(Some(xml)) = host.storage_read_text(&settings::storage(), &key) else {
            return false;
        };
        if xml.trim().is_empty() {
            return false;
        }
        self.loads.push_back(LoadTask {
            purpose: LoadPurpose::File {
                name: name.to_owned(),
                key,
            },
            job: LoadJob::Locations(LocationsJob::new(xml)),
        });
        true
    }

    /// Changes and saves the download address; false when it is not an
    /// absolute `http` or `https` address.
    pub fn set_external_data_url(&mut self, host: &mut Host<'_>, url: &str) -> bool {
        let Some(url) = data::normalize_download_url(url) else {
            return false;
        };
        self.update_settings(host, |s| s.external_data_url = url);
        true
    }

    /// `/go update`: downloads the location data (the answer arrives as a
    /// network event).
    pub fn update_data(&mut self, host: &mut Host<'_>) {
        if self.settings.external_data_url.trim().is_empty() {
            self.location_status = "Set a location data URL first.".to_owned();
            say(
                host,
                "GoArrow: Set a location data URL in Options before downloading.",
            );
            return;
        }
        if self.updating {
            say(host, "GoArrow: A location-data update is already running.");
            return;
        }
        self.updating = true;
        self.location_status = "Downloading location data...".to_owned();
        say(host, "GoArrow: Downloading location data...");
        let url = self.settings.external_data_url.clone();
        let request = openac_plugin::net::HttpRequest {
            method: "GET".into(),
            url: url.clone(),
            follow_redirects: true,
            max_bytes: u32::try_from(crate::database::ATLAS_MAX_BYTES).unwrap_or(u32::MAX),
            ..openac_plugin::net::HttpRequest::default()
        };
        match host.http_request(request) {
            Ok(handle) => {
                self.download_handle = Some(handle);
                self.download_url = url;
            }
            Err(error) => self.download_failed(host, &error),
        }
    }

    /// Whether an update is under way.
    #[must_use]
    pub fn is_updating(&self) -> bool {
        self.updating
    }

    fn download_finished(&mut self, host: &mut Host<'_>, reply: &openac_plugin::net::HttpReply) {
        if reply.error != openac_plugin::net::NetError::None {
            let message = if reply.message.is_empty() {
                format!("{:?}", reply.error)
            } else {
                reply.message.clone()
            };
            self.download_failed(host, &message);
            return;
        }
        if reply.status != 200 {
            self.download_failed(
                host,
                &format!(
                    "Atlas download returned HTTP {} ({}).",
                    reply.status, reply.reason
                ),
            );
            return;
        }
        match data::download_text(&reply.body) {
            Ok(text) => {
                let url = std::mem::take(&mut self.download_url);
                self.loads.push_back(LoadTask {
                    job: LoadJob::Atlas(AtlasJob::new(text.clone())),
                    purpose: LoadPurpose::Download { text, url },
                });
            }
            Err(error) => self.download_failed(host, &error.to_string()),
        }
    }

    fn download_failed(&mut self, host: &mut Host<'_>, message: &str) {
        host.error(&format!("GoArrow: Location-data update failed. {message}"));
        let place = settings::storage();
        let days = self.settings.atlas_cache_max_age_days;
        let fresh = days <= 0
            || host
                .storage_read_text(&place, METADATA_KEY)
                .ok()
                .flatten()
                .and_then(|t| serde_json::from_str::<CacheMetadata>(&t).ok())
                .and_then(|m| timestamp::parse_iso8601(&m.downloaded_at))
                .is_some_and(|at| host.clock().utc_unix_ms - at <= i64::from(days) * 86_400_000);
        let cached = if fresh {
            host.storage_read_text(&place, CACHE_KEY)
                .ok()
                .flatten()
                .filter(|t| !t.trim().is_empty())
        } else {
            None
        };
        match cached {
            Some(text) => self.loads.push_back(LoadTask {
                purpose: LoadPurpose::CacheFallback,
                job: LoadJob::Atlas(AtlasJob::new(text)),
            }),
            None => {
                self.keep_existing(host);
                self.updating = false;
            }
        }
    }

    /// The download's status for the Options tab: what is happening, or how
    /// many places are loaded and when they were downloaded.
    #[must_use]
    pub fn data_status(&self, offset_minutes: i32) -> String {
        if !self.location_status.is_empty() {
            return self.location_status.clone();
        }
        let count = self.database().location_count();
        let mut text = if count == 0 {
            "No data yet: 0 places".to_owned()
        } else {
            format!("{} places", group_thousands(count))
        };
        if count > 0
            && let Some(at) = self
                .metadata
                .as_ref()
                .and_then(|m| timestamp::parse_iso8601(&m.downloaded_at))
        {
            text.push_str(&format!(
                " · downloaded {}",
                timestamp::short_date(at, offset_minutes)
            ));
        }
        text
    }

    /// What the window's status line beside the tabs says about the data.
    #[must_use]
    pub fn location_status(&self) -> &str {
        &self.location_status
    }

    /// Forgets a finished status (the window shows the counts again).
    pub fn clear_location_status(&mut self) {
        if !self.updating && self.loaded {
            self.location_status.clear();
        }
    }

    // ---- Corrections. ----

    /// Loads the shipped and the player's corrections (the player's
    /// arrivals win); a missing file of the player's is written from the
    /// shipped one. The chat line.
    pub fn load_corrections(&mut self, host: &mut Host<'_>) -> String {
        let place = settings::storage();
        let mut errors = Vec::new();
        let shipped = AtlasCorrections::parse(corrections::SHIPPED).unwrap_or_default();
        let mut sources = vec![shipped];
        match host.storage_read_text(&place, CORRECTIONS_KEY) {
            Ok(Some(user)) => match AtlasCorrections::parse(&user) {
                Ok(user) => sources.push(user),
                Err(e) => errors.push(format!("'{CORRECTIONS_KEY}' could not be read: {e}")),
            },
            Ok(None) => {
                if let Err(error) =
                    host.storage_write_text(&place, CORRECTIONS_KEY, corrections::SHIPPED)
                {
                    host.warn(&format!("GoArrow: {error}"));
                }
            }
            Err(e) => errors.push(format!("'{CORRECTIONS_KEY}' could not be read: {e}")),
        }
        let refs: Vec<&AtlasCorrections> = sources.iter().collect();
        let arrivals = AtlasCorrections::arrivals_by_id(&refs, &mut errors);
        let blocked: Vec<BlockedStep> = sources
            .iter()
            .flat_map(|s| s.blocked_steps.iter().cloned())
            .collect();
        let (arrival_count, blocked_count) = (arrivals.len(), blocked.len());
        self.finder
            .database_mut()
            .set_corrections(arrivals, blocked);
        self.route_data_changed();
        for error in &errors {
            host.warn(&format!("GoArrow: {error}"));
        }
        self.corrections = sources;
        let tail = if errors.is_empty() {
            ".".to_owned()
        } else {
            format!("; {} could not be used (see log).", errors.len())
        };
        format!(
            "GoArrow: Loaded {arrival_count} arrival corrections and {blocked_count} blocked steps{tail}"
        )
    }

    /// The corrections as loaded: the shipped file, then the player's.
    #[must_use]
    pub fn corrections(&self) -> &[AtlasCorrections] {
        &self.corrections
    }

    /// `/go corrections path`.
    pub fn corrections_path(host: &mut Host<'_>) -> String {
        let root = host
            .storage_describe(&settings::storage())
            .unwrap_or_else(|| "host managed storage".to_owned());
        format!("GoArrow: Route corrections file: {CORRECTIONS_KEY} in {root}.")
    }

    fn update_user_corrections(
        &mut self,
        host: &mut Host<'_>,
        change: impl FnOnce(&mut AtlasCorrections),
    ) -> Option<String> {
        let place = settings::storage();
        let read = host.storage_read_text(&place, CORRECTIONS_KEY);
        let result = read.and_then(|text| {
            let mut user = match text {
                Some(json) => AtlasCorrections::parse(&json).map_err(|e| e.to_string())?,
                None => AtlasCorrections::default(),
            };
            change(&mut user);
            host.storage_write_text(&place, CORRECTIONS_KEY, &user.to_json())
        });
        if let Err(error) = result {
            return Some(format!(
                "GoArrow: Could not update '{CORRECTIONS_KEY}': {error}"
            ));
        }
        self.load_corrections(host);
        None
    }

    /// `/go block` and the window's Mark faulty: saves a route step as
    /// blocked and plans again without it. The chat line.
    pub fn block_route_step(&mut self, host: &mut Host<'_>, index: usize, note: &str) -> String {
        let Some(step) = self
            .destination
            .current_route()
            .and_then(|r| r.steps().get(index))
            .cloned()
        else {
            return format!("GoArrow: There is no route step {}.", index + 1);
        };
        let blocked = BlockedStep::from_route(&step, note);
        if !self.finder.has_graph_step(&blocked) {
            return format!(
                "GoArrow: Step {} ({step}) is not a stored route link and cannot be blocked.",
                index + 1
            );
        }
        if let Some(error) =
            self.update_user_corrections(host, |user| user.blocked_steps.push(blocked))
        {
            return error;
        }
        self.nav(host, |n, cx| n.stop(cx));
        self.cancel_route_jobs();
        self.destination.clear_route();
        self.last_preview_tick = None;
        format!(
            "GoArrow: Blocked step {} ({step}); the route will be recalculated without it.",
            index + 1
        )
    }

    /// `/go corrections save` and the window's Keep last arrival.
    pub fn save_observed_arrival(&mut self, host: &mut Host<'_>) -> String {
        let Some(observed) = self.navigator.view().pending_arrival.clone() else {
            return "GoArrow: No observed portal arrival is waiting to be saved.".to_owned();
        };
        let entry = ArrivalCorrection {
            id: observed.id,
            name: observed.name.clone(),
            arrival: observed.arrival.clone(),
            note: observed.note.clone(),
        };
        if let Some(error) = self.update_user_corrections(host, |user| {
            user.arrivals.retain(|a| a.id != entry.id);
            user.arrivals.push(entry);
        }) {
            return error;
        }
        self.navigator.take_pending_arrival();
        self.finder
            .database_mut()
            .remove_session_blocked_steps(observed.id);
        self.route_data_changed();
        format!(
            "GoArrow: Saved {} as the arrival of '{}'.",
            observed.arrival, observed.name
        )
    }

    /// `/go dungeon export|import`.
    pub fn share_traversals(
        &mut self,
        host: &mut Host<'_>,
        export: bool,
        file: Option<&str>,
    ) -> String {
        let file = file.unwrap_or(data::DEFAULT_SHARE_FILE);
        if !data::share_file_ok(file) {
            return "GoArrow: Use a JSON filename in the plugin storage folder.".to_owned();
        }
        let result = if export {
            self.navigator.export_traversals(host, file)
        } else {
            self.navigator.import_traversals(host, file)
        };
        let words = result.unwrap_or_else(|e| e);
        if words.starts_with("GoArrow") {
            words
        } else {
            format!("GoArrow: {words}")
        }
    }

    // ---- Destinations. ----

    fn destination_changed(&mut self, host: &mut Host<'_>) {
        self.pending_work = None;
        self.cancel_route_jobs();
        self.last_preview_tick = None;
        self.settings.destination_name = self.destination.saved_name().to_owned();
        self.save_settings(host);
    }

    /// Sets the first place of this name; posts `Destination '...' set.`.
    pub fn set_destination_by_name(&mut self, host: &mut Host<'_>, name: &str) -> bool {
        let Some(location) = self.finder.database().find_location(name) else {
            return false;
        };
        self.nav(host, |n, cx| n.stop(cx));
        self.destination.set_destination(location);
        self.destination_changed(host);
        say(host, &format!("GoArrow: Destination '{name}' set."));
        true
    }

    /// Sets a place (the Places tab).
    pub fn set_destination_location(&mut self, host: &mut Host<'_>, location: Rc<Location>) {
        let name = location.name.clone();
        self.nav(host, |n, cx| n.stop(cx));
        self.destination.set_destination(location);
        self.destination_changed(host);
        say(host, &format!("GoArrow: Destination '{name}' set."));
    }

    /// Sets a map point (`display` names it; none names it by its
    /// coordinates).
    pub fn set_coordinate_destination(
        &mut self,
        host: &mut Host<'_>,
        ns: f64,
        ew: f64,
        display: Option<&str>,
    ) {
        self.nav(host, |n, cx| n.stop(cx));
        self.destination.set_coordinate(ns, ew, display);
        self.destination_changed(host);
    }

    /// Text with exactly one coordinate pair sets that point, named by the
    /// text; false otherwise.
    pub fn try_set_coordinate_destination(&mut self, host: &mut Host<'_>, text: &str) -> bool {
        let Some(c) = chat_coordinates::try_parse(text) else {
            return false;
        };
        self.set_coordinate_destination(host, c.north_south, c.east_west, Some(text));
        true
    }

    /// The player's own position as the destination, "Current Location".
    pub fn set_current_location_destination(&mut self, host: &mut Host<'_>) -> bool {
        let here = Here::read(host);
        if !here.available {
            return false;
        }
        self.set_coordinate_destination(
            host,
            here.position.north_south,
            here.position.east_west,
            Some(CURRENT_LOCATION),
        );
        true
    }

    /// `/go selected`: the selected object as the destination.
    pub fn set_selected_object_destination(&mut self, host: &mut Host<'_>) -> bool {
        let Some(id) = host.selection().selected else {
            return false;
        };
        let Some(object) = host.object(id) else {
            return false;
        };
        let position = object.position.map(|p| NavPosition {
            cell_id: p.cell_id,
            east_west: p.east_west,
            north_south: p.north_south,
            elevation: p.elevation,
            heading_degrees: p.heading_degrees,
            is_outdoor: p.is_outdoor,
        });
        if !self.destination.set_object(id, &object.name, position) {
            return false;
        }
        self.nav(host, |n, cx| n.stop(cx));
        self.destination_changed(host);
        true
    }

    /// Clears the destination and its route.
    pub fn clear_destination(&mut self, host: &mut Host<'_>) {
        self.nav(host, |n, cx| n.stop(cx));
        self.destination.clear_destination();
        self.destination_changed(host);
    }

    /// `/go mark`: the indoor point the player stands on as a named place,
    /// saved and made the destination.
    pub fn mark_indoor(&mut self, host: &mut Host<'_>, name: &str) -> bool {
        let name = name.trim();
        let here = Here::read(host);
        let p = here.position;
        if name.is_empty()
            || !here.available
            || here.portal_space
            || p.is_outdoor
            || (p.cell_id & 0xFFFF) <= 0x40
        {
            return false;
        }
        let mut location = Location::new(name, p.north_south, p.east_west);
        location.indoor_position = Some(IndoorPosition::from_map(
            p.cell_id,
            p.east_west,
            p.north_south,
            p.elevation,
        ));
        location.is_customized = true;
        location.use_in_route_finding = false;
        let mut xml = String::from("<locations>");
        for mark in self.finder.database().user_locations() {
            if !text::eq_ignore_case(&mark.name, name) {
                xml.push_str(&mark.to_xml());
            }
        }
        xml.push_str(&location.to_xml());
        xml.push_str("</locations>");
        if let Err(error) = host.storage_write_text(&settings::storage(), INDOOR_KEY, &xml) {
            host.warn(&format!("GoArrow: Could not save indoor location: {error}"));
            return false;
        }
        let mark = self.finder.database_mut().upsert_user_location(location);
        self.finder.invalidate_graph();
        self.nav(host, |n, cx| n.stop(cx));
        self.destination.set_destination(mark);
        self.destination_changed(host);
        true
    }

    /// Places whose name contains the text (ignoring case).
    #[must_use]
    pub fn search_locations(&self, query: &str) -> Vec<Rc<Location>> {
        self.finder.search_locations(query)
    }

    /// Every destination name once, sorted.
    #[must_use]
    pub fn all_destination_names(&self) -> Vec<String> {
        self.finder.all_destination_names()
    }

    /// The favourites.
    #[must_use]
    pub fn favorites(&self) -> &[String] {
        &self.settings.favorite_destinations
    }

    /// Adds a favourite (ignoring case) and saves.
    pub fn add_favorite(&mut self, host: &mut Host<'_>, name: &str) {
        if self.settings.add_favorite(name) {
            self.save_settings(host);
        }
    }

    /// Removes a favourite and saves.
    pub fn remove_favorite(&mut self, host: &mut Host<'_>, name: &str) {
        if self.settings.remove_favorite(name) {
            self.save_settings(host);
        }
    }

    /// `GoArrow: Current position: 42.1N, 33.6E` (or unavailable).
    pub fn current_position_text(host: &mut Host<'_>, prefix: &str) -> String {
        let here = Here::read(host);
        if !here.available {
            return format!("{prefix} is unavailable (not in world).");
        }
        format!("{prefix}: {}", here.position.coordinates())
    }

    /// `/go dest`.
    #[must_use]
    pub fn destination_position_text(&self) -> String {
        match self.destination.target() {
            Some(location) => format!(
                "GoArrow: Destination '{}': {}",
                location.name, location.coords
            ),
            None => "GoArrow: No destination set.".to_owned(),
        }
    }

    /// The route's steps as `/go route` lists them, then the live dungeon
    /// waypoints.
    #[must_use]
    pub fn route_step_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .destination
            .current_route()
            .map(|r| r.steps().iter().map(ToString::to_string).collect())
            .unwrap_or_default();
        lines.extend(
            self.navigator
                .view()
                .dungeon_waypoints
                .iter()
                .map(|w| w.label.clone()),
        );
        lines
    }

    // ---- Recalls. ----

    /// `/go recall`.
    pub fn recall(&mut self, host: &mut Host<'_>, value: &str) {
        let Some(kind) = recall_kind(value) else {
            say(
                host,
                "GoArrow: Recall must be lifestone, marketplace, house, mansion, or allegiance.",
            );
            return;
        };
        if !host.recalls().is_available {
            say(
                host,
                "GoArrow: Recall is unavailable outside a live session.",
            );
            return;
        }
        let result = host.recall(kind);
        if result.status != RecallStatus::Started {
            say(
                host,
                &format!(
                    "GoArrow: Recall was not accepted ({}).",
                    recall_status_name(result.status)
                ),
            );
            return;
        }
        self.recall_request_revision = host.recalls().last_request.revision;
        say(
            host,
            &format!("GoArrow: {} recall started.", recall_kind_name(kind)),
        );
    }

    fn recall_key(host: &mut Host<'_>) -> Option<String> {
        host.character()
            .filter(|c| c.id != 0 && !c.world.trim().is_empty())
            .map(|c| format!("{}/{}", c.id, c.world))
    }

    fn load_scoped_recalls(&mut self, host: &mut Host<'_>) {
        let Some(key) = Self::recall_key(host) else {
            return;
        };
        if let Some(recalls) = self.settings.recalls_by_character.get(&key).cloned() {
            self.settings.last_portal_recall = recalls.lifestone;
            self.settings.last_secondary_recall = recalls.marketplace;
            self.settings.last_allegiance_recall = recalls.allegiance;
            self.settings.last_house_recall = recalls.house;
            self.settings.last_mansion_recall = recalls.mansion;
        }
    }

    fn learn_recall(&mut self, host: &mut Host<'_>, transition: &PortalTransition) {
        if !transition.is_completed
            || transition.recall_request_revision == 0
            || transition.recall_request_revision != self.recall_request_revision
        {
            return;
        }
        let state = host.recalls();
        let Some(known) = state
            .locations
            .iter()
            .find(|l| l.kind == state.last_request.kind && l.is_known)
        else {
            return;
        };
        let value = format!(
            "{},{}",
            text::format_roundtrip(known.position.north_south),
            text::format_roundtrip(known.position.east_west)
        );
        let s = &mut self.settings;
        match known.kind {
            RecallKind::Lifestone => s.last_portal_recall.clone_from(&value),
            RecallKind::Marketplace => s.last_secondary_recall.clone_from(&value),
            RecallKind::Allegiance => s.last_allegiance_recall.clone_from(&value),
            RecallKind::House => s.last_house_recall.clone_from(&value),
            RecallKind::Mansion => s.last_mansion_recall.clone_from(&value),
        }
        if let Some(key) = Self::recall_key(host) {
            let recalls = self.settings.recalls_by_character.entry(key).or_default();
            match known.kind {
                RecallKind::Lifestone => recalls.lifestone = value,
                RecallKind::Marketplace => recalls.marketplace = value,
                RecallKind::Allegiance => recalls.allegiance = value,
                RecallKind::House => recalls.house = value,
                RecallKind::Mansion => recalls.mansion = value,
            }
        }
        self.save_settings(host);
    }

    // ---- Route work. ----

    /// The route is being computed (queued, or a search under way).
    #[must_use]
    pub fn is_computing_route(&self) -> bool {
        self.pending_work.is_some() || self.plan.is_some() || self.cycle.is_some()
    }

    fn cancel_route_jobs(&mut self) {
        self.plan = None;
        self.cycle = None;
    }

    /// Builds the graph a little when something needs it (a destination,
    /// queued work, a search); true once it is built.
    fn step_graph(&mut self, host: &mut Host<'_>) -> bool {
        if self.finder.is_graph_built() {
            return true;
        }
        let wanted =
            self.loaded && (self.destination.has_destination() || self.is_computing_route());
        if !wanted {
            return false;
        }
        let finder = &mut self.finder;
        budgeted(host, GRAPH_CHUNK, |n| finder.step_graph(n).then_some(())).is_some()
    }

    fn start_plan(&mut self, origin: Rc<Location>, position: Option<NavPosition>) {
        let Some(target) = self.destination.target().cloned() else {
            return;
        };
        self.cycle = None;
        self.plan = Some(PlanJob {
            origin,
            position,
            target,
            profile: self.destination.profile,
            job: None,
        });
    }

    /// Steps the searches; the graph is built.
    fn step_route_jobs(&mut self, host: &mut Host<'_>) {
        let view = self.navigator.view();
        if view.is_navigating || view.waiting_for_interaction {
            // A walk started meanwhile: an idle re-plan must not replace
            // the route it walks.
            self.plan = None;
        }
        if let Some(mut plan) = self.plan.take() {
            let finder = &mut self.finder;
            let job = plan
                .job
                .get_or_insert_with(|| finder.route_job(&plan.origin, &plan.target, plan.profile));
            match budgeted(host, ROUTE_CHUNK, |n| job.step(n)) {
                None => self.plan = Some(plan),
                Some(route) => {
                    self.destination.route_calculated(&plan.origin, route);
                    self.note_preview(plan.position);
                    self.refresh_window(host);
                }
            }
        }
        if let Some(mut cycle) = self.cycle.take() {
            let finder = &mut self.finder;
            let job = cycle.job.get_or_insert_with(|| {
                finder.alternatives_job(
                    &cycle.origin,
                    &cycle.target,
                    cycle.profile,
                    cycle.requested,
                )
            });
            match budgeted(host, ROUTE_CHUNK, |n| job.step(n)) {
                None => self.cycle = Some(cycle),
                Some(routes) => {
                    let found = Some((routes, cycle.requested));
                    if self
                        .destination
                        .finish_cycle(cycle.direction, &cycle.origin, found)
                    {
                        self.cycled(host);
                    }
                    self.refresh_window(host);
                }
            }
        }
    }

    fn note_preview(&mut self, position: Option<NavPosition>) {
        if position.is_some() {
            self.last_preview_position = position;
        }
        self.last_preview_tick = Some(self.ticks);
    }

    fn should_replan(&self, here: &Here) -> bool {
        let view = self.navigator.view();
        let route = self.destination.current_route();
        self.loaded
            && !self.is_computing_route()
            && self.destination.has_destination()
            && here.outdoors()
            && !view.is_navigating
            && !view.waiting_for_interaction
            && !view.has_arrived
            && view.failure_reason.as_deref().is_none_or(str::is_empty)
            && (route.is_none() || self.settings.recalculate_route)
            && self
                .last_preview_tick
                .is_none_or(|at| self.ticks - at >= REPLAN_TICKS)
            && (route.is_none()
                || self.last_preview_position.is_none_or(|last| {
                    here.position.horizontal_distance_metres(&last) >= REPLAN_METRES
                }))
    }

    /// The Go button (the window's or the arrow's): a "Current Location"
    /// destination moves to where the player stands first, as GoArrow's
    /// panel did.
    pub fn go_pressed(&mut self, host: &mut Host<'_>) {
        if self.destination.target_name() == CURRENT_LOCATION
            && !self.set_current_location_destination(host)
        {
            say(host, "GoArrow: Current location is unavailable.");
            return;
        }
        self.go(host);
    }

    /// Go: shows the route, and walks it when "Walk it" is on. With a
    /// window attached it waits a frame so "Computing route..." shows.
    pub fn go(&mut self, host: &mut Host<'_>) {
        if host.window_attached() {
            self.pending_work = Some((Work::Go, self.ticks + WORK_DELAY_TICKS));
        } else if !self.finder.is_graph_built() {
            // The graph is built over the next ticks first.
            self.pending_work = Some((Work::Go, self.ticks));
        } else {
            self.execute_go(host);
        }
    }

    fn execute_go(&mut self, host: &mut Host<'_>) {
        // An idle re-plan under way would replace the route Go shows or
        // walks when it finished.
        self.cancel_route_jobs();
        let here = Here::read(host);
        if let Some(from) = self.route_from.clone() {
            self.nav(host, |n, cx| n.stop(cx));
            self.start_plan(from, here.available.then_some(here.position));
            return;
        }
        if self.destination.target_name() == CURRENT_LOCATION {
            self.nav(host, |n, cx| n.plan_route(cx));
        } else if self.settings.auto_navigate {
            self.nav(host, |n, cx| n.start(cx));
        } else if here.outdoors() {
            // A search always shows the best route first; an earlier choice
            // among the alternatives is not carried into it.
            self.nav(host, |n, cx| n.stop(cx));
            self.destination.clear_route();
            self.start_plan(here.location(), Some(here.position));
            return;
        } else {
            self.nav(host, |n, cx| n.stop(cx));
            self.destination.clear_route();
            self.nav(host, |n, cx| n.plan_route(cx));
        }
        if here.available {
            self.note_preview(Some(here.position));
        }
    }

    /// Stop: ends the walk and any queued route work.
    pub fn stop_navigation(&mut self, host: &mut Host<'_>) {
        self.pending_work = None;
        self.cancel_route_jobs();
        self.nav(host, |n, cx| n.stop(cx));
    }

    /// Resume: carries on after an interaction or a pause.
    pub fn resume(&mut self, host: &mut Host<'_>) {
        if !self.navigator.view().can_resume {
            return;
        }
        if self.navigator.view().waiting_for_interaction
            || (!host.window_attached() && self.finder.is_graph_built())
        {
            self.execute_resume(host);
        } else if host.window_attached() {
            self.pending_work = Some((Work::Resume, self.ticks + WORK_DELAY_TICKS));
        } else {
            // The graph is built over the next ticks first.
            self.pending_work = Some((Work::Resume, self.ticks));
        }
    }

    fn execute_resume(&mut self, host: &mut Host<'_>) {
        self.cancel_route_jobs();
        self.nav(host, |n, cx| n.resume(cx));
    }

    /// A walk is under way or about to be (the Go/Stop button reads Stop).
    #[must_use]
    pub fn is_navigating(&self) -> bool {
        self.navigator.view().is_navigating || self.is_computing_route()
    }

    /// The place routes are planned from instead of the player's position.
    #[must_use]
    pub fn route_from(&self) -> Option<&Rc<Location>> {
        self.route_from.as_ref()
    }

    /// Plans from a named place (none: from where the player stands);
    /// false when there is no such place with coordinates.
    pub fn set_route_from(&mut self, host: &mut Host<'_>, name: Option<&str>) -> bool {
        self.pending_work = None;
        self.cancel_route_jobs();
        self.last_preview_tick = None;
        let Some(name) = name else {
            self.route_from = None;
            self.destination.clear_route();
            return true;
        };
        let Some(location) = self
            .finder
            .database()
            .find_location(name)
            .filter(|l| l.has_coordinates())
        else {
            return false;
        };
        self.set_route_from_location(host, location);
        true
    }

    /// Plans from a place (one of the map's, or a map point).
    pub fn set_route_from_location(&mut self, host: &mut Host<'_>, location: Rc<Location>) {
        self.pending_work = None;
        self.cancel_route_jobs();
        self.last_preview_tick = None;
        self.nav(host, |n, cx| n.stop(cx));
        self.route_from = Some(location);
        self.destination.clear_route();
    }

    // ---- The map's clicks and menu (the original GoArrow's map). ----

    /// "Point Arrow Here" on a place: the place as the destination, and
    /// the arrow shown, as the original's map did.
    pub fn point_arrow_at(&mut self, host: &mut Host<'_>, location: Rc<Location>) {
        self.set_destination_location(host, location);
        self.set_arrow_visible(host, true);
    }

    /// "Point Arrow Here" on a point of the map.
    pub fn point_arrow_at_coordinates(&mut self, host: &mut Host<'_>, at: Coordinates) {
        let at = at.round(1);
        self.set_coordinate_destination(host, at.ns, at.ew, None);
        self.set_arrow_visible(host, true);
    }

    /// "Start Route Here" (or Shift+click) on a place: the route finder's
    /// From, and the window opened on its Route tab.
    pub fn set_route_start(&mut self, host: &mut Host<'_>, location: Rc<Location>) {
        let name = location.name.clone();
        self.set_route_from_location(host, location);
        self.show_route_tab(host, Some(&name));
    }

    /// "Start Route Here" on a point of the map: From is that point, named
    /// by its coordinates.
    pub fn set_route_start_at(&mut self, host: &mut Host<'_>, at: Coordinates) {
        let at = at.round(1);
        let name = at.to_string();
        self.set_route_from_location(host, Rc::new(Location::at(name.clone(), at)));
        self.show_route_tab(host, Some(&name));
    }

    /// "End Route Here" (or Ctrl+click) on a place: the route finder's To,
    /// which in this port is the destination, and the window opened on its
    /// Route tab.
    pub fn set_route_end(&mut self, host: &mut Host<'_>, location: Rc<Location>) {
        self.set_destination_location(host, location);
        self.show_route_tab(host, None);
    }

    /// "End Route Here" on a point of the map.
    pub fn set_route_end_at(&mut self, host: &mut Host<'_>, at: Coordinates) {
        let at = at.round(1);
        self.set_coordinate_destination(host, at.ns, at.ew, None);
        self.show_route_tab(host, None);
    }

    /// "Show Details" on a place: the window opened on its Places tab with
    /// the place found and selected.
    pub fn show_place_details(&mut self, host: &mut Host<'_>, location: &Location) {
        if let Some(mut window) = self.window.take() {
            window.show_place(host, &location.name);
            window.refresh(self, host);
            self.window = Some(window);
        }
    }

    fn show_route_tab(&mut self, host: &mut Host<'_>, from: Option<&str>) {
        if let Some(mut window) = self.window.take() {
            window.show_route(host, from);
            window.refresh(self, host);
            self.window = Some(window);
        }
    }

    /// Whether Previous and Next can be used at all.
    #[must_use]
    pub fn can_cycle(&self) -> bool {
        let view = self.navigator.view();
        self.destination.has_destination()
            && self.destination.current_route().is_some()
            && !view.is_navigating
            && !view.waiting_for_interaction
            && !self.is_computing_route()
    }

    /// Whether Previous can be used.
    #[must_use]
    pub fn can_previous(&self) -> bool {
        self.can_cycle() && self.destination.alternative_index() > 0
    }

    /// Whether Next can be used.
    #[must_use]
    pub fn can_next(&self) -> bool {
        let d = &self.destination;
        self.can_cycle()
            && (d.alternative_count() == 0
                || d.alternative_index() + 1 < d.alternative_count()
                || d.alternative_has_more())
    }

    /// Previous (-1) or Next (1) route; Next may search for more over a
    /// few ticks. False when it cannot.
    pub fn cycle_route_alternative(&mut self, host: &mut Host<'_>, direction: i32) -> bool {
        let view = self.navigator.view();
        if view.is_navigating || view.waiting_for_interaction || self.is_computing_route() {
            return false;
        }
        let origin = match self.route_from.clone() {
            Some(origin) => origin,
            None => {
                let here = Here::read(host);
                if !here.outdoors() {
                    return false;
                }
                here.location()
            }
        };
        if self.destination.current_route().is_none() {
            return false;
        }
        match self.destination.prepare_cycle(direction, &origin) {
            CyclePlan::Refused => false,
            CyclePlan::Move => {
                let moved = self.destination.finish_cycle(direction, &origin, None);
                if moved {
                    self.cycled(host);
                }
                moved
            }
            CyclePlan::Fetch(requested) => {
                let Some(target) = self.destination.target().cloned() else {
                    return false;
                };
                self.cycle = Some(CycleJob {
                    direction,
                    origin,
                    requested,
                    target,
                    profile: self.destination.profile,
                    job: None,
                });
                true
            }
        }
    }

    fn cycled(&mut self, host: &mut Host<'_>) {
        self.nav(host, |n, cx| n.stop(cx));
        let here = Here::read(host);
        if here.available {
            self.note_preview(Some(here.position));
        }
    }

    /// What the route is doing, in GoArrow's order of importance.
    #[must_use]
    pub fn nav_status_text(&self, here: &Here) -> &'static str {
        let view = self.navigator.view();
        if self.is_computing_route() {
            "Computing route..."
        } else if view.is_planning_path {
            "Client planning path..."
        } else if view.is_navigating {
            "Navigating..."
        } else if view.waiting_for_interaction {
            "Waiting for interaction"
        } else if view.has_arrived {
            "Arrived!"
        } else if self.destination.target_name().is_empty() {
            "Idle"
        } else if view.waiting_for_indoor_portal {
            "Finding indoor portal"
        } else if view.has_indoor_target {
            "Indoor target ready"
        } else if here.paused() {
            "Outdoor route paused indoors"
        } else if view
            .failure_reason
            .as_deref()
            .is_some_and(|r| !r.is_empty())
        {
            "Navigation stopped"
        } else if self
            .destination
            .current_route()
            .is_some_and(|r| r.step_count() > 0)
        {
            "Route ready"
        } else {
            "Ready"
        }
    }

    /// The straight-line distance to the destination, metres, from the
    /// player.
    #[must_use]
    pub fn straight_line_metres(&self, here: &Here) -> f64 {
        match self.destination.target() {
            Some(target) if here.available && target.has_coordinates() => {
                here.position.coordinates().distance_to(target.coords) * METRES_PER_MAP_UNIT
            }
            _ => f64::NAN,
        }
    }

    // ---- The window and the overlays. ----

    /// The window.
    #[must_use]
    pub fn window(&self) -> Option<&Window> {
        self.window.as_ref()
    }

    /// The arrow.
    #[must_use]
    pub fn hud(&self) -> Option<&Hud> {
        self.hud.as_ref()
    }

    /// When this tick started, the client clock's milliseconds: the overlays
    /// spend what is left of the tick's work budget ([`WORK_BUDGET_MS`]).
    #[must_use]
    pub fn tick_started_ms(&self) -> u64 {
        self.tick_started_ms
    }

    /// The map.
    #[must_use]
    pub fn map(&self) -> Option<&MapOverlay> {
        self.map.as_ref()
    }

    /// The smooth shapes the overlays draw.
    #[must_use]
    pub fn sprites(&self) -> &Sprites {
        &self.sprites
    }

    /// The map's icons the client holds.
    #[must_use]
    pub fn map_icons(&self) -> &MapIcons {
        &self.map_icons
    }

    /// The route on the ground.
    #[must_use]
    pub fn ground(&self) -> &Ground {
        &self.ground
    }

    /// Pushes the window's values now.
    pub fn refresh_window(&mut self, host: &mut Host<'_>) {
        if let Some(mut window) = self.window.take() {
            window.refresh(self, host);
            self.window = Some(window);
        }
    }

    /// Shows or hides the arrow.
    pub fn set_arrow_visible(&mut self, host: &mut Host<'_>, visible: bool) {
        self.update_settings(host, |s| s.hud_visible = visible);
        if let Some(hud) = self.hud.as_mut() {
            hud.set_visible(host, visible);
        }
    }

    /// Shows or hides the map.
    pub fn set_map_visible(&mut self, host: &mut Host<'_>, visible: bool) {
        self.update_settings(host, |s| s.map_visible = visible);
        if let Some(map) = self.map.as_mut() {
            map.set_visible(host, visible);
        }
    }

    /// Shows or hides the route on the ground.
    pub fn set_route_line_visible(&mut self, host: &mut Host<'_>, visible: bool) {
        self.update_settings(host, |s| s.route_line_visible = visible);
    }

    /// Where the player is, now.
    #[must_use]
    pub fn here(host: &mut Host<'_>) -> Here {
        Here::read(host)
    }

    /// The coordinates of the place routes start from: the From place, or
    /// the player.
    #[must_use]
    pub fn origin_coordinates(&self, here: &Here) -> Option<Coordinates> {
        match &self.route_from {
            Some(from) => Some(from.coords),
            None => here.available.then(|| here.position.coordinates()),
        }
    }
}

/// A location file over the one-call limit that does not split into
/// records was not read.
fn unsplit_line(file: &str) -> String {
    format!(
        "GoArrow: Could not read '{file}' in parts; {}.",
        crate::database::UNSPLIT
    )
}

fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// GoArrow's `Enum.TryParse` of a recall kind: its name in any case, or its
/// number.
#[must_use]
pub fn recall_kind(value: &str) -> Option<RecallKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "lifestone" | "0" => Some(RecallKind::Lifestone),
        "marketplace" | "1" => Some(RecallKind::Marketplace),
        "house" | "2" => Some(RecallKind::House),
        "mansion" | "3" => Some(RecallKind::Mansion),
        "allegiance" | "4" => Some(RecallKind::Allegiance),
        _ => None,
    }
}

/// A recall kind's name as GoArrow wrote it (`Lifestone`).
#[must_use]
pub fn recall_kind_name(kind: RecallKind) -> &'static str {
    match kind {
        RecallKind::Lifestone => "Lifestone",
        RecallKind::Marketplace => "Marketplace",
        RecallKind::House => "House",
        RecallKind::Mansion => "Mansion",
        RecallKind::Allegiance => "Allegiance",
    }
}

fn recall_status_name(status: RecallStatus) -> &'static str {
    match status {
        RecallStatus::Unavailable => "Unavailable",
        RecallStatus::Unsupported => "Unsupported",
        RecallStatus::Started => "Started",
        RecallStatus::Refused => "Refused",
    }
}
