//! The navigator: walks the destination's route leg by leg. It walks to
//! waypoints, uses portals and recalls on the way, opens doors, tries
//! detours, finds its way out of dungeons and through the Town Network,
//! and re-plans after a wrong arrival (`GoArrowNavigator.cs`).
//!
//! The plugin owns one [`Navigator`] and hands it a [`NavContext`] on every
//! call; the navigator keeps only its own state. Its public surface is the
//! contract between the plugin (window, overlays, commands) and the walking
//! logic: what the window shows comes from [`Navigator::view`].
//!
//! The walk is driven by the client's walk reports (polled every tick and
//! heard as events), portal transitions, activation results, the server's
//! answers to uses (a refused portal, an opened door) and object changes.
//! Only Go and Resume make it walk: planning a route to show it
//! may wait on a dungeon exit or ask a portal where it leads, but never
//! moves the character. Its state is a set of flags, as in GoArrow, since
//! several hold at once (paused for a portal while waiting for a surface
//! exit, say).
//!
//! Timers (portal identification, retries, saves, the exploration and
//! interaction timeouts, the preview interval) count the client's tick
//! time. Each call reads the client's object table at most once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use openac_plugin::chat_markup;
use openac_plugin::items::{ItemStatus, ItemUseCompletion};
use openac_plugin::navigation::{GoToReport, GoToState, NavigationSnapshot, NavigationStatus};
use openac_plugin::world::{
    ActivationCompletion, ActivationOutcome, METRES_PER_MAP_UNIT, ObjectChange, ObjectChangeKind,
    ObjectClass, PortalTransition, Position, WorldObject,
};
use openac_plugin::{Event, Host};

use crate::ace_portals::{AcePortal, AcePortals};
use crate::coords::{Coordinates, NavPosition};
use crate::destination::{Destination, DestinationKind, KnownExit};
use crate::dungeon_search::DungeonPortalSearch;
use crate::finder::RouteFinder;
use crate::indoor_preview::IndoorPathPreview;
use crate::location::{IndoorPosition, Location};
use crate::portal_match::{
    are_nearby_indoor_landblocks, capability_names, has_portal_capability, is_blank, is_portal,
    is_unknown_surface_portal, portal_destination_label, portal_destination_name,
    portal_matches_destination, route_dungeon_exit_name,
};
use crate::route::{RouteStep, RouteStepKind};
use crate::text;
use crate::traversals::TraversalStore;

/// The plugin id walk reports carry when no other is set
/// ([`Navigator::set_owner`]).
pub const DEFAULT_OWNER: &str = "openac.goarrow";
/// The Facility Hub's landblock.
pub const FACILITY_HUB_LANDBLOCK: u32 = 0x8A02_0000;
/// How close a walk into an indoor portal ends, metres: well inside the
/// client's use radius, so activating it does not start a walk of its own.
pub const PORTAL_APPROACH_DISTANCE: f32 = 0.25;
/// How far a portal may land from its listed arrival, map units (1.2 km).
/// Atlas arrivals are estimates, usually within one unit; the wrong ones
/// this catches are typos tens of units away.
pub const WRONG_ARRIVAL_DISTANCE: f64 = 5.0;
/// How far a portal may land from its listed arrival before the landing is
/// used as its arrival for the session, map units (120 m). A route walks
/// on from the listed arrival, and a re-plan from where the character
/// really stands can send it back through the same portal.
pub const LEARNED_ARRIVAL_DISTANCE: f64 = 0.5;
/// How far a dungeon exit may land from the route's arrival to be the way
/// out, metres (a map unit: Atlas arrivals are estimates).
pub const DUNGEON_EXIT_ARRIVAL_METRES: f64 = 240.0;
/// How long the client may plan a walk to an exploration target before
/// the target counts as unreachable, seconds.
pub const EXPLORATION_PLANNING_TIMEOUT_SECONDS: f64 = 15.0;
/// How often one portal is asked for its destination.
pub const MAXIMUM_PORTAL_IDENTIFICATION_ATTEMPTS: u32 = 3;
/// How long between two identification requests for one portal, and how
/// long one unanswered request blocks the others, seconds.
pub const IDENTIFICATION_RETRY_SECONDS: f64 = 5.0;
/// How often learned dungeon paths are saved, seconds.
pub const TRAVERSAL_SAVE_SECONDS: f64 = 15.0;
/// How often a paused indoor leg is tried again, seconds.
pub const INDOOR_RETRY_SECONDS: f64 = 0.5;
/// How many detours are tried toward one goal.
pub const MAXIMUM_OUTDOOR_DETOURS: u32 = 12;
/// The detours tried, (forward, side) metres from where the walk stuck:
/// sideways first, alternating sides, a retreat last.
pub const OUTDOOR_DETOUR_OFFSETS: [(f64, f64); 10] = [
    (0.0, 80.0),
    (0.0, -80.0),
    (50.0, 80.0),
    (50.0, -80.0),
    (-50.0, 80.0),
    (-50.0, -80.0),
    (0.0, 150.0),
    (0.0, -150.0),
    (-80.0, 0.0),
    (80.0, 0.0),
];
/// The file `/go dungeon export` writes when none is named.
pub const DEFAULT_TRAVERSAL_SHARE_FILE: &str = "dungeon-traversals-share.json";
/// How long an interaction may go unanswered, seconds (GoArrow's
/// `InteractionTimeoutSeconds`): a door opened for a blocked walk that
/// does not open, a portal or recall used whose transition does not start.
pub const INTERACTION_TIMEOUT_SECONDS: f64 = 15.0;

/// The settings the navigator reads.
#[derive(Debug, Clone, PartialEq)]
pub struct NavSettings {
    /// Go also walks (GoArrow's Auto-Navigate).
    pub auto_navigate: bool,
    /// Walking is allowed at all; off, Go only plans.
    pub use_navigation_automation: bool,
    /// How close a walk to a waypoint stops, metres.
    pub arrival_distance: f64,
}

impl Default for NavSettings {
    fn default() -> Self {
        Self {
            auto_navigate: false,
            use_navigation_automation: true,
            arrival_distance: 0.5,
        }
    }
}

/// What a navigator call works with: the client and the plugin's route
/// state, borrowed for the call.
#[derive(Debug)]
pub struct NavContext<'a, 'h> {
    /// The client.
    pub host: &'a mut Host<'h>,
    /// The destination and its route.
    pub destination: &'a mut Destination,
    /// The route finder and its places.
    pub finder: &'a mut RouteFinder,
    /// The navigator's settings.
    pub settings: &'a NavSettings,
}

/// A portal arrival seen to differ from the Atlas, waiting for the player
/// to keep it (`/go corrections save`).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingArrival {
    /// The Atlas id of the portal.
    pub id: i32,
    /// The portal's name.
    pub name: String,
    /// Where it landed, map text (`52.2S, 82.5E`).
    pub arrival: String,
    /// Why it was recorded.
    pub note: String,
}

/// Where an indoor path preview should lead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IndoorPreviewTarget {
    /// An object in this dungeon.
    Object(u32),
    /// An exact indoor point.
    Point(IndoorPosition),
}

/// A waypoint of the live dungeon route (recorded path or exploration),
/// for the step list and the dungeon map.
#[derive(Debug, Clone, PartialEq)]
pub struct DungeonWaypoint {
    /// The step text (`Recorded dungeon walk 3: cell 01AB0105, floor 6 m`,
    /// `... 12 more recorded waypoints`, or `Dungeon Searching cell
    /// 01AB0105 (...)` while exploring).
    pub label: String,
    /// The cell (0 for the "more" row).
    pub cell_id: u32,
    /// Landblock-local metres: east, north, height.
    pub local: [f32; 3],
}

/// What the window, the overlays and `/go status` show of the navigator.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NavView {
    /// A walk is under way.
    pub is_navigating: bool,
    /// The client is planning the current walk.
    pub is_planning_path: bool,
    /// Waiting for a portal, recall or other interaction to finish.
    pub waiting_for_interaction: bool,
    /// Indoors, waiting for the portal object the route needs.
    pub waiting_for_indoor_portal: bool,
    /// The destination is an indoor target in this dungeon.
    pub has_indoor_target: bool,
    /// Arrived at the destination.
    pub has_arrived: bool,
    /// Resume would do something.
    pub can_resume: bool,
    /// Why navigation stopped, if it failed.
    pub failure_reason: Option<String>,
    /// The leg being walked (0 for the first).
    pub leg_index: usize,
    /// The live dungeon route, in order.
    pub dungeon_waypoints: Vec<DungeonWaypoint>,
    /// A portal arrival waiting to be kept.
    pub pending_arrival: Option<PendingArrival>,
    /// Where an indoor path preview should lead now, if anywhere.
    pub indoor_preview_target: Option<IndoorPreviewTarget>,
    /// The latest walk report for this plugin's walk.
    pub last_report: Option<GoToReport>,
    /// The client's planned path to the indoor target, while there is one
    /// (the navigator asks for previews only while a window is attached).
    pub indoor_preview: Option<IndoorPreview>,
}

/// The client's planned indoor path, as the window lists it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IndoorPreview {
    /// One line (`Indoor path: 42.5 m, 7 waypoints`, `No indoor path found.`).
    pub summary: String,
    /// The waypoints, one line each (`1. Cell 0x01AB0105: E 12, N 40.5, Z 6 m`).
    pub waypoints: Vec<String>,
    /// The waypoints' positions, for the dungeon map: cell and
    /// landblock-local metres (east, north, height).
    pub points: Vec<(u32, [f32; 3])>,
}

/// A door opened for a blocked walk, waited for until it reads open.
#[derive(Debug, Clone)]
struct DoorWait {
    object_id: u32,
    name: String,
    started: f64,
}

/// One portal's identification requests.
#[derive(Debug, Clone, Copy)]
struct Attempt {
    started: f64,
    count: u32,
    status: ItemStatus,
}

/// The navigator.
#[derive(Debug)]
pub struct Navigator {
    owner: String,
    enabled: bool,
    clock: f64,
    view: NavView,
    is_navigating: bool,
    paused_for_outdoor_route: bool,
    indoor_walk: bool,
    indoor_route_leg: bool,
    indoor_surface_exit_leg: bool,
    waiting_for_surface_exit: bool,
    pending_identification: u32,
    pending_identification_started: f64,
    identification_attempts: HashMap<u32, Attempt>,
    observed_indoor_route: bool,
    search: DungeonPortalSearch,
    traversals: TraversalStore,
    traversal_save_elapsed: f64,
    exploring_dungeon_exit: bool,
    // A surface ruin's rooms are reached through the landscape, so a route
    // between them can step outside without leaving the place searched.
    exploration_may_pass_outdoors: bool,
    waiting_for_dungeon_portal_identification: bool,
    walking_portal_origin: Option<Position>,
    walking_portal_saw_transit: bool,
    landscape_walk_position: Option<Position>,
    outdoor_portal_retry_object_id: u32,
    outdoor_detour_active: bool,
    outdoor_detour_attempts: u32,
    outdoor_detour_tried: Vec<Position>,
    outdoor_detour_goal: Coordinates,
    resolved_indoor_portal_object_id: u32,
    resolved_indoor_portal_position: Option<Position>,
    indoor_retry_elapsed: f64,
    exploration_planning_elapsed: f64,
    current_nav_position: Option<Position>,
    active_sequence: i64,
    last_handled_report_revision: i64,
    active_interaction_object_id: u32,
    interaction_failed: bool,
    last_activation_revision: i64,
    last_transition_revision: i64,
    last_transition_generation: i64,
    interaction_origin_position: Option<Position>,
    interaction_saw_portal_space: bool,
    failure_reason: String,
    waiting_for_interaction: bool,
    has_arrived: bool,
    // Set only by Go or Resume and cleared by Stop.
    go_requested: bool,
    route_id: u64,
    routes_started: u64,
    leg_index: usize,
    transition_generation: i64,
    pending_arrival: Option<PendingArrival>,
    preview: IndoorPathPreview,
    // The client ends no activation of a door: the walk goes on once the
    // door reads open or its use is answered.
    door_wait: Option<DoorWait>,
    last_item_use_revision: i64,
    // When the portal or recall use being waited for was accepted, until
    // its transition starts: a refusal brings no transition.
    interaction_started: Option<f64>,
    // The portal table: where every portal stands and leads.
    portals: Rc<AcePortals>,
    // The table's portal an exit walk heads for while its object is not
    // in view, and those found not to be there (cell and place).
    table_exit: Option<AcePortal>,
    missing_table_exits: Vec<(u32, [f32; 3])>,
    // A portal step completed in portal space, whose arrival is checked
    // once the character is seen in the world.
    arrival_check: Option<RouteStep>,
    // An indoor walk to a portal ended in portal space: the character
    // walked into the portal.
    walked_into_portal: bool,
    // The portals and recalls taken in this navigation (Go to Stop).
    taken: Vec<String>,
    // The object table (inventory included) as read once in this call.
    objects: RefCell<Option<Rc<[WorldObject]>>>,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            owner: DEFAULT_OWNER.to_owned(),
            enabled: false,
            clock: 0.0,
            view: NavView::default(),
            is_navigating: false,
            paused_for_outdoor_route: false,
            indoor_walk: false,
            indoor_route_leg: false,
            indoor_surface_exit_leg: false,
            waiting_for_surface_exit: false,
            pending_identification: 0,
            pending_identification_started: 0.0,
            identification_attempts: HashMap::new(),
            observed_indoor_route: false,
            search: DungeonPortalSearch::new(),
            traversals: TraversalStore::new(),
            traversal_save_elapsed: 0.0,
            exploring_dungeon_exit: false,
            exploration_may_pass_outdoors: false,
            waiting_for_dungeon_portal_identification: false,
            walking_portal_origin: None,
            walking_portal_saw_transit: false,
            landscape_walk_position: None,
            outdoor_portal_retry_object_id: 0,
            outdoor_detour_active: false,
            outdoor_detour_attempts: 0,
            outdoor_detour_tried: Vec::new(),
            outdoor_detour_goal: Coordinates::NONE,
            resolved_indoor_portal_object_id: 0,
            resolved_indoor_portal_position: None,
            indoor_retry_elapsed: 0.0,
            exploration_planning_elapsed: 0.0,
            current_nav_position: None,
            active_sequence: 0,
            last_handled_report_revision: 0,
            active_interaction_object_id: 0,
            interaction_failed: false,
            last_activation_revision: 0,
            last_transition_revision: 0,
            last_transition_generation: 0,
            interaction_origin_position: None,
            interaction_saw_portal_space: false,
            failure_reason: String::new(),
            waiting_for_interaction: false,
            has_arrived: false,
            go_requested: false,
            route_id: 0,
            routes_started: 0,
            leg_index: 0,
            transition_generation: 0,
            pending_arrival: None,
            preview: IndoorPathPreview::new(),
            door_wait: None,
            last_item_use_revision: 0,
            interaction_started: None,
            portals: Rc::new(AcePortals::new()),
            table_exit: None,
            missing_table_exits: Vec::new(),
            arrival_check: None,
            walked_into_portal: false,
            taken: Vec::new(),
            objects: RefCell::new(None),
        }
    }
}

// ---- small helpers ----

fn landblock(cell_id: u32) -> u32 {
    cell_id & 0xFFFF_0000
}

/// A bool as .NET writes it.
fn tf(value: bool) -> &'static str {
    if value { "True" } else { "False" }
}

/// Text the plugin did not write, made safe for the transcript.
fn esc(value: &str) -> String {
    chat_markup::escape(value)
}

fn coords_of(p: &Position) -> Coordinates {
    Coordinates::new(p.north_south, p.east_west)
}

/// An outdoor walk target: map coordinates alone, on the ground.
fn outdoor_place(east_west: f64, north_south: f64) -> Position {
    Position {
        cell_id: 0,
        east_west,
        north_south,
        elevation: f64::NAN,
        heading_degrees: 0.0,
        is_outdoor: true,
    }
}

fn to_nav(p: &Position) -> NavPosition {
    NavPosition {
        cell_id: p.cell_id,
        east_west: p.east_west,
        north_south: p.north_south,
        elevation: p.elevation,
        heading_degrees: p.heading_degrees,
        is_outdoor: p.is_outdoor,
    }
}

fn from_nav(p: &NavPosition) -> Position {
    Position {
        cell_id: p.cell_id,
        east_west: p.east_west,
        north_south: p.north_south,
        elevation: p.elevation,
        heading_degrees: p.heading_degrees,
        is_outdoor: p.is_outdoor,
    }
}

fn position_of(object: &WorldObject) -> Position {
    object.position.clone().unwrap_or_default()
}

fn steps(destination: &Destination) -> &[RouteStep] {
    destination.current_route().map_or(&[], |r| r.steps())
}

fn first_step(destination: &Destination) -> Option<RouteStep> {
    steps(destination).first().cloned()
}

fn has_steps(destination: &Destination) -> bool {
    !steps(destination).is_empty()
}

fn is_in_world(snapshot: &NavigationSnapshot) -> bool {
    snapshot.is_available && !snapshot.is_portal_space
}

fn is_indoors(snapshot: &NavigationSnapshot) -> bool {
    is_in_world(snapshot) && !snapshot.position.is_outdoor
}

fn current_location(p: &Position) -> Rc<Location> {
    Rc::new(Location::new(
        "Current Position",
        p.north_south,
        p.east_west,
    ))
}

fn status_name(status: NavigationStatus) -> String {
    format!("{status:?}")
}

fn state_name(state: GoToState) -> String {
    format!("{state:?}")
}

fn kind_word(kind: RouteStepKind) -> String {
    kind.name().to_lowercase()
}

/// The walk to the portal at the end of a walk step: the step is a walk
/// and the next one a portal.
fn is_walk_to_portal(destination: &Destination, step: &RouteStep) -> bool {
    let all = steps(destination);
    step.kind == RouteStepKind::Travel && all.len() > 1 && all[1].kind == RouteStepKind::Portal
}

/// The live portal a step leads to: the portal at the end of a walk, or
/// the portal a portal step uses.
fn route_portal_name(destination: &Destination, step: &RouteStep) -> Option<String> {
    if is_walk_to_portal(destination, step) {
        Some(step.to.name.clone())
    } else if step.kind == RouteStepKind::Portal {
        Some(step.via.clone())
    } else {
        None
    }
}

/// A walk from a dungeon arrival not followed by a portal: the way out of
/// the dungeon must be found first.
fn needs_dungeon_exit(destination: &Destination, step: &RouteStep) -> bool {
    let all = steps(destination);
    step.kind == RouteStepKind::Travel
        && text::contains_ignore_case(&step.from.name, " arrival (")
        && (all.len() <= 1 || all[1].kind != RouteStepKind::Portal)
}

fn dungeon_exit_key(step: &RouteStep) -> String {
    format!(
        "{}|{}|{}",
        route_dungeon_exit_name(step)
            .map(|n| n.to_uppercase())
            .unwrap_or_default(),
        text::format_roundtrip(step.from.coords.ns),
        text::format_roundtrip(step.from.coords.ew)
    )
}

fn is_indoor_portal_step(destination: &Destination) -> bool {
    let all = steps(destination);
    match all.first() {
        None => false,
        Some(first) => {
            first.kind == RouteStepKind::Portal
                || (first.kind == RouteStepKind::Travel
                    && all.len() > 1
                    && all[1].kind == RouteStepKind::Portal)
        }
    }
}

/// The portal step at the head of the route: the first step, or the
/// second after a walk to it.
fn head_portal(destination: &Destination) -> Option<RouteStep> {
    match steps(destination) {
        [first, ..] if first.kind == RouteStepKind::Portal => Some(first.clone()),
        [first, second, ..]
            if first.kind == RouteStepKind::Travel && second.kind == RouteStepKind::Portal =>
        {
            Some(second.clone())
        }
        _ => None,
    }
}

/// What makes a portal or recall step the same one again: a portal by its
/// device and where it is used, a recall by its spell or device (it leads
/// to one place wherever it is cast). None for a walk.
fn interaction_key(step: &RouteStep) -> Option<String> {
    match step.kind {
        RouteStepKind::Travel => None,
        RouteStepKind::Recall => Some(format!("recall|{}", text::fold(&step.via))),
        RouteStepKind::Portal => Some(format!(
            "portal|{}|{}",
            text::fold(&step.via),
            if step.graph_from.id > 0 {
                step.graph_from.id.to_string()
            } else {
                text::fold(&step.graph_from.name)
            }
        )),
    }
}

fn is_near_route_entrance(snapshot: &NavigationSnapshot, target: &Location) -> bool {
    coords_of(&snapshot.position).distance_to(target.coords) * METRES_PER_MAP_UNIT <= 60.0
}

/// The first check that keeps `object` from being the route's indoor
/// portal, or none when it may be.
fn indoor_portal_rejection(
    snapshot: &NavigationSnapshot,
    object: &WorldObject,
    destination_name: &str,
) -> Option<&'static str> {
    let Some(position) = object.position.as_ref() else {
        return Some(if object.object_id == 0 {
            "no object id"
        } else {
            "no position"
        });
    };
    if object.object_id == 0 {
        Some("no object id")
    } else if !object.can_activate() {
        Some("not activatable")
    } else if is_unknown_surface_portal(object) {
        Some("unidentified Surface Portal")
    } else if !has_portal_capability(object) {
        Some("missing Portal capability")
    } else if position.is_outdoor {
        Some("outdoor")
    } else if position.cell_id == 0 {
        Some("no cell")
    } else if !are_nearby_indoor_landblocks(snapshot.position.cell_id, position.cell_id) {
        Some("landblock not near player")
    } else if !portal_matches_destination(object, destination_name) {
        Some("name/destination mismatch")
    } else {
        None
    }
}

impl Navigator {
    /// A navigator with nothing under way.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the plugin id walk reports of this plugin's walks carry (the
    /// `plugin_id` of the start record; [`DEFAULT_OWNER`] until set).
    /// Reports owned by anyone else never move the route on.
    pub fn set_owner(&mut self, plugin_id: &str) {
        plugin_id.clone_into(&mut self.owner);
    }

    /// The learned dungeon paths.
    #[must_use]
    pub fn traversals(&self) -> &TraversalStore {
        &self.traversals
    }

    /// The portal table ([`crate::ace_portals`]).
    #[must_use]
    pub fn ace_portals(&self) -> &AcePortals {
        &self.portals
    }

    /// Uses `table` as the portal table (enabling reads the shipped one).
    pub fn set_ace_portals(&mut self, table: AcePortals) {
        self.portals = Rc::new(table);
    }

    /// Loads what it keeps between sessions (learned dungeon paths) and
    /// the portal table shipped beside the plugin.
    pub fn enable(&mut self, host: &mut Host<'_>) {
        if !self.enabled {
            self.traversals.load(host);
            self.portals = Rc::new(AcePortals::load(host));
        }
        self.enabled = true;
    }

    /// Saves what it keeps and stops any walk.
    pub fn disable(&mut self, host: &mut Host<'_>) {
        self.enabled = false;
        self.stop_walk(host);
        self.preview.clear();
        self.view = NavView {
            pending_arrival: self.pending_arrival.clone(),
            ..NavView::default()
        };
    }

    /// Plans the route from where the character stands (outdoors, a
    /// dungeon exit, an indoor target). False when it cannot plan now; the
    /// reason is in [`NavView::failure_reason`].
    pub fn plan_route(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        self.forget_objects();
        let planned = self.plan(cx);
        self.refresh_view(cx);
        self.forget_objects();
        planned
    }

    /// Go: starts walking the route (or only plans, when walking is off).
    pub fn start(&mut self, cx: &mut NavContext<'_, '_>) {
        self.forget_objects();
        self.taken.clear();
        self.start_navigation(cx);
        self.refresh_view(cx);
        self.forget_objects();
    }

    /// Stop: ends the walk and every wait.
    pub fn stop(&mut self, cx: &mut NavContext<'_, '_>) {
        self.forget_objects();
        self.stop_walk(cx.host);
        self.refresh_view(cx);
        self.forget_objects();
    }

    /// Resume: carries on after an interaction or a pause.
    pub fn resume(&mut self, cx: &mut NavContext<'_, '_>) {
        self.forget_objects();
        self.resume_navigation(cx);
        self.refresh_view(cx);
        self.forget_objects();
    }

    /// Completes the portal or recall step being waited for, as a finished
    /// portal transition does, and walks on when `start_next_leg`.
    pub fn resume_after_interaction(&mut self, cx: &mut NavContext<'_, '_>, start_next_leg: bool) {
        self.forget_objects();
        self.complete_interaction(cx, start_next_leg);
        self.refresh_view(cx);
        self.forget_objects();
    }

    /// Every tick: follows the walk, the waits and the dungeon search, then
    /// rebuilds the view. The object table is read at most once.
    pub fn tick(&mut self, cx: &mut NavContext<'_, '_>, elapsed_seconds: f64) {
        self.forget_objects();
        self.clock += elapsed_seconds.max(0.0);
        let live = cx.host.navigation();
        if live.is_available {
            self.current_nav_position = Some(live.position.clone());
        }
        self.on_tick(cx, elapsed_seconds);
        let preview_target = self.tick_preview(cx);
        self.refresh_view_with(cx, Some(preview_target));
        self.forget_objects();
    }

    /// Every event: walk reports, portal transitions, activations, use
    /// answers, object changes, and the path previews it asked for. The
    /// view is rebuilt only when the event changed what it shows; the
    /// tick rebuilds it anyway.
    pub fn on_event(&mut self, cx: &mut NavContext<'_, '_>, event: &Event) {
        self.forget_objects();
        let changed = match event {
            Event::NavigationPlan(plan) => {
                if !self.search.plan_arrived(plan) {
                    self.preview.plan_arrived(plan);
                }
                false
            }
            _ if !self.enabled => false,
            Event::NavigationChanged(report) => {
                if self.is_navigating {
                    self.handle_navigation_report(cx, report);
                }
                // The view shows the latest report.
                true
            }
            Event::ActivationCompleted(completion) => {
                self.on_activation_completed(cx, completion);
                true
            }
            Event::PortalTransition(transition) => {
                self.on_portal_transition(cx, transition);
                true
            }
            Event::ObjectChanged(change) => self.on_object_changed(cx, change),
            Event::ItemUseCompleted(completion) => self.on_item_use_completed(cx, completion),
            _ => false,
        };
        if changed {
            self.refresh_view(cx);
        }
        self.forget_objects();
    }

    /// The long `/go status` lines about portals nearby.
    #[must_use]
    pub fn dungeon_portal_diagnostics(&self, cx: &mut NavContext<'_, '_>) -> Vec<String> {
        self.forget_objects();
        let lines = self.portal_diagnostics(cx);
        self.forget_objects();
        lines
    }

    fn portal_diagnostics(&self, cx: &mut NavContext<'_, '_>) -> Vec<String> {
        let snapshot = cx.host.navigation();
        let Some(step) = first_step(cx.destination) else {
            return Vec::new();
        };
        if !is_indoors(&snapshot) || !needs_dungeon_exit(cx.destination, &step) {
            return Vec::new();
        }
        let here = &snapshot.position;
        let objects = self.objects(cx.host);
        let portals: Vec<&WorldObject> = objects.iter().filter(|p| is_portal(p)).collect();
        let eligible = portals
            .iter()
            .filter(|p| {
                p.position.as_ref().is_some_and(|pos| {
                    p.can_activate()
                        && !pos.is_outdoor
                        && are_nearby_indoor_landblocks(here.cell_id, pos.cell_id)
                })
            })
            .count();
        let without_position = portals.iter().filter(|p| p.position.is_none()).count();
        let mut lines = vec![format!(
            "World objects: {} tracked, {} classified portals, {eligible} eligible, \
             {without_position} without position; Atlas arrival: {}; identify pending: {}.",
            objects.len(),
            portals.len(),
            step.from.coords,
            if self.pending_identification == 0 {
                "none".to_owned()
            } else {
                format!("{:08X}", self.pending_identification)
            }
        )];
        let mut nearby: Vec<&&WorldObject> = portals
            .iter()
            .filter(|p| {
                p.position
                    .as_ref()
                    .is_some_and(|pos| are_nearby_indoor_landblocks(here.cell_id, pos.cell_id))
            })
            .collect();
        nearby.sort_by(|a, b| {
            here.horizontal_distance_metres(&position_of(a))
                .total_cmp(&here.horizontal_distance_metres(&position_of(b)))
        });
        for portal in nearby.into_iter().take(8) {
            let pos = position_of(portal);
            let offset = portal
                .portal_destination
                .as_deref()
                .and_then(Coordinates::parse)
                .map_or_else(String::new, |arrival| {
                    format!(
                        ", arrival offset={} m",
                        text::format_fixed(
                            arrival.distance_to(step.from.coords) * METRES_PER_MAP_UNIT,
                            0,
                            0
                        )
                    )
                });
            let identify = match self.identification_attempts.get(&portal.object_id) {
                Some(attempt) if attempt.count > 0 => format!(
                    "identify={}/{MAXIMUM_PORTAL_IDENTIFICATION_ATTEMPTS} {:?}.",
                    attempt.count, attempt.status
                ),
                _ => "identify=not requested.".to_owned(),
            };
            lines.push(format!(
                "Portal {:08X} '{}': {} m, cell {:08X}, outdoor={}, activate={}, destination='{}'{offset}, {identify}",
                portal.object_id,
                portal.name,
                text::format_fixed(here.horizontal_distance_metres(&pos), 0, 0),
                pos.cell_id,
                tf(pos.is_outdoor),
                tf(portal.can_activate()),
                portal.portal_destination.as_deref().unwrap_or("unknown"),
            ));
        }
        let mut unclassified: Vec<&WorldObject> = objects
            .iter()
            .filter(|p| {
                p.object_class == ObjectClass::Unknown
                    && p.position.as_ref().is_some_and(|pos| {
                        landblock(pos.cell_id) == landblock(here.cell_id)
                            && here.horizontal_distance_metres(pos) <= 20.0
                    })
            })
            .collect();
        unclassified.sort_by(|a, b| {
            here.horizontal_distance_metres(&position_of(a))
                .total_cmp(&here.horizontal_distance_metres(&position_of(b)))
        });
        let names: Vec<String> = unclassified
            .iter()
            .take(6)
            .map(|p| format!("{:08X} '{}'", p.object_id, p.name))
            .collect();
        if !names.is_empty() {
            lines.push(format!(
                "Nearby unclassified objects: {}.",
                names.join(", ")
            ));
        }
        lines
    }

    /// Takes the pending arrival correction, if any (the player kept it).
    pub fn take_pending_arrival(&mut self) -> Option<PendingArrival> {
        self.view.pending_arrival = None;
        self.pending_arrival.take()
    }

    /// Exports learned dungeon paths to a storage file (`/go dungeon
    /// export`): a `.json` name in the plugin's storage folder (the plugin
    /// passes [`DEFAULT_TRAVERSAL_SHARE_FILE`] when the player named none).
    ///
    /// # Errors
    /// The words for the player.
    pub fn export_traversals(&mut self, host: &mut Host<'_>, file: &str) -> Result<String, String> {
        check_share_file(file)?;
        let place = openac_plugin::storage::StoragePlace::global(
            openac_plugin::storage::StorageRoot::PluginData,
        );
        host.storage_write_text(&place, file, &self.traversals.export())
            .map_err(|e| format!("Dungeon traversal export failed: {e}"))?;
        let folder = host
            .storage_describe(&place)
            .unwrap_or_else(|| "host managed storage".to_owned());
        Ok(format!(
            "Exported dungeon traversals to '{file}'. Storage folder: {folder}."
        ))
    }

    /// Merges learned dungeon paths from a storage file (`/go dungeon import`).
    ///
    /// # Errors
    /// The words for the player.
    pub fn import_traversals(&mut self, host: &mut Host<'_>, file: &str) -> Result<String, String> {
        check_share_file(file)?;
        let place = openac_plugin::storage::StoragePlace::global(
            openac_plugin::storage::StorageRoot::PluginData,
        );
        let json = host
            .storage_read_text(&place, file)
            .map_err(|e| format!("Dungeon traversal import failed: {e}"))?
            .ok_or_else(|| "Dungeon traversal import failed: File not found.".to_owned())?;
        self.traversals
            .import(host, &json)
            .map_err(|e| format!("Dungeon traversal import failed: {e}"))?;
        Ok(if self.traversals.error().is_empty() {
            format!("Merged dungeon traversals from '{file}'.")
        } else {
            format!("Imported in memory. {}", self.traversals.error())
        })
    }

    /// What the window shows.
    #[must_use]
    pub fn view(&self) -> &NavView {
        &self.view
    }

    // ---- the object table ----

    // Every object the client knows, read once per call: the table holds
    // the inventory too, and one tick asks about it several times. Nothing
    // the navigator does in a call changes it.
    fn objects(&self, host: &mut Host<'_>) -> Rc<[WorldObject]> {
        if let Some(objects) = self.objects.borrow().as_ref() {
            return Rc::clone(objects);
        }
        let mut objects = host.objects();
        // A portal the table has leads where the table says: no need to
        // ask it.
        self.portals.fill_destinations(&mut objects);
        let objects: Rc<[WorldObject]> = objects.into();
        *self.objects.borrow_mut() = Some(Rc::clone(&objects));
        objects
    }

    // One object, its destination filled in from the table as the table's
    // are.
    fn object(&self, host: &mut Host<'_>, object_id: u32) -> Option<WorldObject> {
        let mut object = host.object(object_id)?;
        self.portals
            .fill_destinations(std::slice::from_mut(&mut object));
        Some(object)
    }

    // Each call reads the table afresh.
    fn forget_objects(&self) {
        self.objects.borrow_mut().take();
    }

    // ---- the view ----

    fn refresh_view(&mut self, cx: &mut NavContext<'_, '_>) {
        self.refresh_view_with(cx, None);
    }

    // `known_target`: the indoor preview target this call already found.
    fn refresh_view_with(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        known_target: Option<Option<(u32, Position)>>,
    ) {
        let snapshot = cx.host.navigation();
        let report = cx.host.go_to_report();
        let has_indoor_target = self.can_navigate_indoor_target(cx, &snapshot);
        let waypoints = self.dungeon_route_waypoints(cx, &snapshot);
        let preview_target = known_target
            .unwrap_or_else(|| self.indoor_preview_target(cx, &snapshot))
            .map(|(object_id, position)| {
                if object_id != 0 {
                    IndoorPreviewTarget::Object(object_id)
                } else {
                    IndoorPreviewTarget::Point(IndoorPosition::from_map(
                        position.cell_id,
                        position.east_west,
                        position.north_south,
                        position.elevation,
                    ))
                }
            });
        let ours = report
            .owner
            .as_deref()
            .is_none_or(|owner| text::eq_ignore_case(owner, &self.owner));
        self.view = NavView {
            is_navigating: self.is_navigating,
            is_planning_path: self.is_navigating
                && report.state == GoToState::Planning
                && report.sequence == self.active_sequence,
            waiting_for_interaction: self.waiting_for_interaction || self.door_wait.is_some(),
            waiting_for_indoor_portal: self.paused_for_outdoor_route
                && !self.waiting_for_surface_exit
                && is_indoors(&snapshot)
                && has_steps(cx.destination),
            has_indoor_target,
            has_arrived: self.has_arrived,
            can_resume: self.can_resume(cx),
            failure_reason: (!self.failure_reason.is_empty()).then(|| self.failure_reason.clone()),
            leg_index: self.leg_index,
            dungeon_waypoints: waypoints,
            pending_arrival: self.pending_arrival.clone(),
            indoor_preview_target: preview_target,
            last_report: ours.then_some(report),
            indoor_preview: self.preview.view().cloned(),
        };
    }

    fn can_resume(&self, cx: &NavContext<'_, '_>) -> bool {
        cx.destination.has_destination()
            && !self.is_navigating
            && !self.has_arrived
            && (self.waiting_for_interaction
                || self.door_wait.is_some()
                || self.interaction_failed
                || (cx.settings.auto_navigate && !self.paused_for_outdoor_route))
    }

    fn dungeon_route_waypoints(
        &self,
        cx: &NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
    ) -> Vec<DungeonWaypoint> {
        let Some(step) = first_step(cx.destination) else {
            return Vec::new();
        };
        if !is_indoors(snapshot) || !needs_dungeon_exit(cx.destination, &step) {
            return Vec::new();
        }
        if let Some(exit) = &self.table_exit {
            return vec![DungeonWaypoint {
                label: format!(
                    "Portal '{}' (portal table): cell {:08X}",
                    exit.name, exit.cell_id
                ),
                cell_id: exit.cell_id,
                local: exit.local,
            }];
        }
        let remaining = self.search.remaining_recorded_waypoints();
        if remaining > 0 {
            let points = self.search.recorded_waypoints();
            let mut rows: Vec<DungeonWaypoint> = points
                .iter()
                .enumerate()
                .map(|(i, p)| DungeonWaypoint {
                    label: format!(
                        "Recorded dungeon walk {}: cell {:08X}, floor {} m",
                        i + 1,
                        p.cell_id,
                        text::format_fixed(p.elevation * METRES_PER_MAP_UNIT, 0, 0)
                    ),
                    cell_id: p.cell_id,
                    local: p.landblock_local(),
                })
                .collect();
            if remaining > points.len() {
                rows.push(DungeonWaypoint {
                    label: format!("... {} more recorded waypoints", remaining - points.len()),
                    cell_id: 0,
                    local: [0.0; 3],
                });
            }
            return rows;
        }
        if self.exploring_dungeon_exit {
            let target = self.search.current_target();
            return vec![DungeonWaypoint {
                label: format!("Dungeon {}", self.search.reason()),
                cell_id: target.cell_id,
                local: target.landblock_local(),
            }];
        }
        Vec::new()
    }

    // ---- planning ----

    // Plans from where the character stands; within a navigation, a
    // route that would take a portal or recall already taken stops it
    // (a listed arrival a little off can make going round again look
    // shorter than the walk on).
    fn plan(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        self.walked_into_portal = false;
        if !self.plan_from_here(cx) {
            return false;
        }
        if self.go_requested
            && !self.has_arrived
            && let Some(via) = self.repeated_interaction(cx.destination)
        {
            self.stop_for_repeat(cx, &via);
            return false;
        }
        true
    }

    fn plan_from_here(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        let snapshot = cx.host.navigation();
        if cx.destination.target().is_none() || !snapshot.is_available {
            return false;
        }
        self.outdoor_portal_retry_object_id = 0;
        self.resolved_indoor_portal_object_id = 0;
        self.resolved_indoor_portal_position = None;
        self.table_exit = None;
        if is_in_world(&snapshot)
            && landblock(snapshot.position.cell_id) == FACILITY_HUB_LANDBLOCK
            && cx.destination.current_route().is_none()
            && cx
                .destination
                .try_plan_from_portal_arrival("Town Network (S R) to Facility Hub", cx.finder)
        {
            self.current_nav_position = Some(snapshot.position.clone());
            return true;
        }
        if is_indoors(&snapshot) && cx.destination.current_route().is_none() {
            let known = self.table_dungeon_exits(snapshot.position.cell_id);
            cx.destination
                .try_plan_dungeon_exit_with(snapshot.position.cell_id, cx.finder, &known);
        }
        if snapshot.position.is_outdoor {
            self.search.reset();
        }
        if is_indoors(&snapshot)
            && let Some(pending) = first_step(cx.destination)
            && needs_dungeon_exit(cx.destination, &pending)
        {
            self.current_nav_position = Some(snapshot.position.clone());
            if self.try_resolve_route_dungeon_exit(cx, &snapshot, &pending) {
                self.indoor_surface_exit_leg = true;
                return true;
            }
            self.paused_for_outdoor_route = true;
            self.failure_reason = self.indoor_portal_search_reason(cx, &snapshot, &pending);
            return false;
        }
        if self.can_navigate_indoor_target(cx, &snapshot) {
            self.current_nav_position = Some(snapshot.position.clone());
            cx.destination.clear_route();
            return true;
        }
        if self.try_resolve_destination_portal(cx, &snapshot) {
            self.current_nav_position = Some(snapshot.position.clone());
            cx.destination.clear_route();
            return true;
        }
        if is_indoors(&snapshot)
            && cx.destination.kind() == DestinationKind::Location
            && cx.destination.target_indoor_position().is_none()
            && self.try_resolve_surface_exit(cx, &snapshot, None)
        {
            self.current_nav_position = Some(snapshot.position.clone());
            cx.destination.clear_route();
            self.indoor_surface_exit_leg = true;
            return true;
        }
        if cx.destination.target_indoor_position().is_some()
            || cx
                .destination
                .target_object_position()
                .is_some_and(|p| !p.is_outdoor)
        {
            cx.host
                .warn("GoArrow: Indoor destination is outside the current dungeon.");
            return false;
        }
        // The Atlas graph has outdoor map coordinates but no indoor cells
        // or floors: an indoor map coordinate would attach the route to
        // whichever outdoor entrance happens to be closest on the map.
        if snapshot.is_portal_space || !snapshot.position.is_outdoor {
            cx.host.warn(
                "GoArrow: Outdoor route planning is paused while indoors or in portal space.",
            );
            return false;
        }
        self.current_nav_position = Some(snapshot.position.clone());
        let current = current_location(&snapshot.position);
        cx.destination.calculate_route(&current, cx.finder);
        if !has_steps(cx.destination) {
            cx.host.warn("GoArrow: No route found.");
            return false;
        }
        true
    }

    fn start_navigation(&mut self, cx: &mut NavContext<'_, '_>) {
        if !cx.settings.use_navigation_automation {
            self.plan(cx);
            return;
        }
        let start = cx.host.navigation();
        if !start.is_available {
            cx.host
                .warn("GoArrow: Navigation automation unavailable (not in world?)");
            return;
        }
        if self.is_navigating {
            self.stop_walk(cx.host);
        }
        self.go_requested = true;
        self.paused_for_outdoor_route = false;
        self.indoor_surface_exit_leg = false;
        self.waiting_for_surface_exit = false;
        self.observed_indoor_route = false;
        let keep_chosen_route = is_in_world(&start)
            && cx
                .destination
                .can_keep_selected_alternative(&to_nav(&start.position));
        if !keep_chosen_route && !self.plan(cx) {
            return;
        }
        if keep_chosen_route {
            self.current_nav_position = Some(start.position.clone());
        }
        self.is_navigating = false;
        self.has_arrived = false;
        self.waiting_for_interaction = false;
        self.active_sequence = 0;
        self.last_handled_report_revision = 0;
        self.last_transition_revision = 0;
        self.last_transition_generation = 0;
        self.interaction_origin_position = None;
        self.interaction_saw_portal_space = false;
        self.new_route_id();
        self.failure_reason.clear();
        self.leg_index = 0;
        self.transition_generation = 0;
        let live = cx.host.navigation();
        if self.can_navigate_indoor_target(cx, &live) {
            self.start_indoor_walk(cx);
            return;
        }
        self.start_current_leg(cx);
    }

    // Learns where the character walks; says once when the store is full.
    fn observe_traversal(&mut self, host: &mut Host<'_>, snapshot: &NavigationSnapshot) {
        self.traversals.observe(snapshot);
        if let Some(notice) = self.traversals.take_notice() {
            host.warn(&notice);
        }
    }

    fn new_route_id(&mut self) {
        self.routes_started += 1;
        self.route_id = self.routes_started;
    }

    fn stop_walk(&mut self, host: &mut Host<'_>) {
        self.outdoor_detour_active = false;
        self.outdoor_detour_attempts = 0;
        self.outdoor_detour_tried.clear();
        self.outdoor_detour_goal = Coordinates::NONE;
        self.traversals.save(host);
        self.traversals.break_trace();
        self.exploring_dungeon_exit = false;
        self.waiting_for_dungeon_portal_identification = false;
        if self.is_navigating {
            host.stop_go_to();
        }
        self.is_navigating = false;
        self.go_requested = false;
        self.paused_for_outdoor_route = false;
        self.indoor_walk = false;
        self.indoor_route_leg = false;
        self.indoor_surface_exit_leg = false;
        self.waiting_for_surface_exit = false;
        self.pending_identification = 0;
        self.pending_identification_started = 0.0;
        self.identification_attempts.clear();
        self.observed_indoor_route = false;
        self.landscape_walk_position = None;
        self.walking_portal_origin = None;
        self.walking_portal_saw_transit = false;
        self.search.reset();
        self.resolved_indoor_portal_object_id = 0;
        self.outdoor_portal_retry_object_id = 0;
        self.resolved_indoor_portal_position = None;
        self.table_exit = None;
        self.missing_table_exits.clear();
        self.arrival_check = None;
        self.walked_into_portal = false;
        self.taken.clear();
        self.indoor_retry_elapsed = 0.0;
        self.has_arrived = false;
        self.waiting_for_interaction = false;
        self.active_sequence = 0;
        self.last_handled_report_revision = 0;
        self.active_interaction_object_id = 0;
        self.interaction_failed = false;
        self.last_activation_revision = 0;
        self.last_transition_revision = 0;
        self.last_transition_generation = 0;
        self.interaction_origin_position = None;
        self.interaction_saw_portal_space = false;
        self.failure_reason.clear();
        self.route_id = 0;
        self.leg_index = 0;
        self.transition_generation = 0;
        self.door_wait = None;
        self.interaction_started = None;
    }

    // ---- the tick ----

    #[allow(clippy::too_many_lines)]
    fn on_tick(&mut self, cx: &mut NavContext<'_, '_>, elapsed: f64) {
        // A completed portal event can be missed when the client finishes
        // the transition before the step waits for it; the live position
        // is an independent signal that the portal worked.
        self.check_interaction_position(cx);
        if let Some(step) = self.arrival_check.take() {
            if !is_in_world(&cx.host.navigation()) {
                self.arrival_check = Some(step);
            } else if self.replan_after_wrong_arrival(cx, &step) {
                return;
            }
        }
        if self.check_door_wait(cx) || self.check_interaction_timeout(cx) {
            return;
        }

        let live = cx.host.navigation();
        if self.try_complete_known_indoor_arrival(cx, &live) {
            return;
        }
        if live.is_moving
            || ((self.exploring_dungeon_exit || self.indoor_surface_exit_leg)
                && cx.host.go_to_report().state == GoToState::Walking)
        {
            self.observe_traversal(cx.host, &live);
        } else if !live.is_available || live.is_portal_space || live.position.is_outdoor {
            self.traversals.break_trace();
        }
        self.traversal_save_elapsed += elapsed.max(0.0);
        if self.traversal_save_elapsed >= TRAVERSAL_SAVE_SECONDS {
            self.traversals.save(cx.host);
            self.traversal_save_elapsed = 0.0;
        }
        if self.walking_portal_origin.is_some() && live.is_portal_space {
            self.walking_portal_saw_transit = true;
        }
        if self.try_complete_walking_portal(cx, &live) {
            return;
        }
        self.update_landscape_walk(&live);
        if self.indoor_walk
            && (self.indoor_route_leg || self.indoor_surface_exit_leg)
            && live.is_available
            && live.is_portal_space
        {
            self.pause_for_portal_exit();
            return;
        }
        if is_in_world(&live) {
            if !live.position.is_outdoor
                && is_indoor_portal_step(cx.destination)
                && self.outdoor_portal_retry_object_id == 0
                && self.landscape_walk_position.is_none()
            {
                self.observed_indoor_route = true;
            } else if live.position.is_outdoor
                && !(self.exploring_dungeon_exit && self.exploration_may_pass_outdoors)
                && (self.indoor_route_leg
                    || self.indoor_surface_exit_leg
                    || self.waiting_for_surface_exit
                    || self.observed_indoor_route
                    || (self.route_id != 0 && self.is_near_town_network_exit(cx, &live.position)))
            {
                self.observed_indoor_route = false;
                if is_indoor_portal_step(cx.destination)
                    || self.indoor_surface_exit_leg
                    || self.waiting_for_surface_exit
                {
                    // The character can enter the portal before the walk
                    // reports arrival at its object; a Lost report then
                    // leaves the old indoor walk step at the head.
                    if self.is_navigating {
                        cx.host.stop_go_to();
                    }
                    self.is_navigating = false;
                    self.indoor_walk = false;
                    self.indoor_route_leg = false;
                    self.indoor_surface_exit_leg = false;
                    self.waiting_for_surface_exit = false;
                    self.waiting_for_interaction = false;
                    self.paused_for_outdoor_route = false;
                    self.active_sequence = 0;
                    // Walked into the route's portal: it is taken, and
                    // lands where it lands.
                    if std::mem::take(&mut self.walked_into_portal)
                        && let Some(portal) = head_portal(cx.destination)
                    {
                        self.note_taken(&portal);
                        if self.replan_after_wrong_arrival(cx, &portal) {
                            return;
                        }
                    }
                    if self.plan(cx) {
                        self.failure_reason.clear();
                        self.start_current_leg(cx);
                    }
                    return;
                }
            }
        }

        if self.exploring_dungeon_exit {
            self.supervise_exploration(cx, &live, elapsed);
            return;
        }

        if self.paused_for_outdoor_route && self.go_requested {
            let snapshot = cx.host.navigation();
            if !is_in_world(&snapshot) {
                return;
            }
            if !snapshot.position.is_outdoor {
                if self.waiting_for_surface_exit {
                    return;
                }
                self.indoor_retry_elapsed += elapsed.max(0.0);
                if self.indoor_retry_elapsed >= INDOOR_RETRY_SECONDS {
                    self.indoor_retry_elapsed = 0.0;
                    self.start_current_leg(cx);
                }
                return;
            }
            self.waiting_for_surface_exit = false;
            self.indoor_surface_exit_leg = false;
            self.waiting_for_interaction = false;
            self.active_interaction_object_id = 0;
            self.paused_for_outdoor_route = false;
            if !self.plan(cx) {
                return;
            }
            self.failure_reason.clear();
            self.start_current_leg(cx);
        }

        if !self.is_navigating || cx.destination.target().is_none() {
            return;
        }
        let Some(current) = self.current_nav_position.clone() else {
            return;
        };

        if self.indoor_walk {
            if self.indoor_surface_exit_leg && self.try_take_over_table_exit(cx) {
                cx.host.stop_go_to();
                self.is_navigating = false;
                self.indoor_walk = false;
                self.start_indoor_walk(cx);
                if !self.is_navigating {
                    self.indoor_surface_exit_leg = false;
                    self.paused_for_outdoor_route = true;
                }
                return;
            }
            let snapshot = cx.host.navigation();
            if !self.can_navigate_indoor_target(cx, &snapshot) {
                self.stop_walk(cx.host);
                self.failure_reason = "Indoor target is no longer in this dungeon.".to_owned();
                cx.host
                    .chat_post(&format!("GoArrow: {}", self.failure_reason));
                return;
            }
            let report = cx.host.go_to_report();
            self.handle_navigation_report(cx, &report);
            return;
        }

        cx.destination.update_guidance(&current_location(&current));
        let report = cx.host.go_to_report();
        self.handle_navigation_report(cx, &report);
    }

    fn supervise_exploration(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        live: &NavigationSnapshot,
        elapsed: f64,
    ) {
        let pending = first_step(cx.destination);
        let found = pending.as_ref().is_some_and(|pending| {
            if route_portal_name(cx.destination, pending).is_some() {
                self.is_route_portal_visible(cx, live, pending)
            } else {
                self.try_resolve_route_dungeon_exit(cx, live, pending)
            }
        });
        if found {
            self.exploring_dungeon_exit = false;
            self.waiting_for_dungeon_portal_identification = false;
            self.is_navigating = false;
            cx.host.stop_go_to();
            self.start_current_leg(cx);
        } else if self.has_nearby_pending_portal_identification(cx, live, true) {
            self.search.pause_target();
            self.exploring_dungeon_exit = false;
            self.waiting_for_dungeon_portal_identification = true;
            self.is_navigating = false;
            cx.host.stop_go_to();
            self.paused_for_outdoor_route = true;
            self.indoor_retry_elapsed = 0.0;
            self.failure_reason = "Identifying nearby dungeon portal.".to_owned();
        } else if self.exploration_planning_timed_out(cx, elapsed) {
            let report = cx.host.go_to_report();
            cx.host.stop_go_to();
            self.search.complete_target(&live.position, false);
            let failure = format!(
                "Planning to cell {:08X} did not finish ({}).",
                self.search.current_target().cell_id,
                report.reason.as_deref().unwrap_or("no reason given")
            );
            self.search.record_failure(Some(&failure));
            self.exploring_dungeon_exit = false;
            self.is_navigating = false;
            self.paused_for_outdoor_route = true;
            self.indoor_retry_elapsed = 0.0;
        } else if !{
            let objects = self.objects(cx.host);
            self.search
                .safe_to_continue(cx.host, &objects, self.exploration_may_pass_outdoors)
        } {
            self.search.complete_target(&live.position, false);
            self.exploring_dungeon_exit = false;
            self.is_navigating = false;
            cx.host.stop_go_to();
            self.paused_for_outdoor_route = true;
            self.indoor_retry_elapsed = 0.0;
            self.failure_reason = self.search.reason().to_owned();
        } else {
            let report = cx.host.go_to_report();
            self.handle_navigation_report(cx, &report);
        }
    }

    // The indoor preview, run only while a window is attached; returns
    // the target found, for the view.
    fn tick_preview(&mut self, cx: &mut NavContext<'_, '_>) -> Option<(u32, Position)> {
        let snapshot = cx.host.navigation();
        let target = self.indoor_preview_target(cx, &snapshot);
        let shown = if snapshot.is_available && cx.host.window_attached() {
            target.clone()
        } else {
            None
        };
        self.preview.tick(
            cx.host,
            shown,
            cx.settings.arrival_distance as f32,
            self.clock,
        );
        target
    }

    // ---- resuming ----

    fn resume_navigation(&mut self, cx: &mut NavContext<'_, '_>) {
        if !self.can_resume(cx) {
            return;
        }
        self.go_requested = true;
        if self.waiting_for_interaction {
            self.complete_interaction(cx, cx.settings.auto_navigate);
            return;
        }
        if self.door_wait.is_some() {
            // The player opened it, or wants the walk tried again.
            self.start_current_leg(cx);
            return;
        }
        let snapshot = cx.host.navigation();
        let report = cx.host.go_to_report();
        let all = steps(cx.destination);
        if is_indoors(&snapshot)
            && report.state == GoToState::Lost
            && report
                .reason
                .as_deref()
                .is_some_and(|r| text::contains_ignore_case(r, "portal space"))
            && all.len() > 1
            && all[0].kind == RouteStepKind::Travel
            && all[1].kind == RouteStepKind::Portal
            && !text::contains_ignore_case(&all[0].from.name, " arrival (")
        {
            let origin = all[0].from.coords;
            self.walking_portal_origin = Some(outdoor_place(origin.ew, origin.ns));
            self.walking_portal_saw_transit = true;
            if self.try_complete_walking_portal(cx, &snapshot) {
                return;
            }
        }
        if is_indoors(&snapshot)
            && (is_indoor_portal_step(cx.destination)
                || first_step(cx.destination)
                    .is_some_and(|p| needs_dungeon_exit(cx.destination, &p)))
        {
            if let Some(first) = first_step(cx.destination)
                && first.kind == RouteStepKind::Portal
                && first.to.has_coordinates()
                && coords_of(&snapshot.position).distance_to(first.to.coords) * METRES_PER_MAP_UNIT
                    <= 250.0
            {
                // Stop can clear a pending transition before the entry
                // portal is marked complete; the live indoor position is
                // its arrival.
                self.note_taken(&first);
                cx.destination.advance_step();
                if cx
                    .destination
                    .current_route()
                    .is_some_and(|r| r.step_count() == 0)
                {
                    self.has_arrived = true;
                    self.post_arrived(cx);
                    return;
                }
            }
            self.new_route_id();
            self.leg_index = 0;
            self.observed_indoor_route = true;
            self.paused_for_outdoor_route = false;
            self.failure_reason.clear();
            self.has_arrived = false;
            self.active_sequence = 0;
            self.last_handled_report_revision = 0;
            self.start_current_leg(cx);
            return;
        }
        self.start_navigation(cx);
    }

    fn post_arrived(&mut self, cx: &mut NavContext<'_, '_>) {
        let name = esc(cx.destination.target_name());
        cx.host.chat_post(&format!("GoArrow: Arrived at '{name}'."));
    }

    fn complete_interaction(&mut self, cx: &mut NavContext<'_, '_>, start_next_leg: bool) {
        let Some(completed) = first_step(cx.destination) else {
            return;
        };
        if !self.waiting_for_interaction || completed.kind == RouteStepKind::Travel {
            return;
        }
        cx.destination.advance_step();
        self.note_taken(&completed);
        self.waiting_for_interaction = false;
        self.interaction_started = None;
        self.active_interaction_object_id = 0;
        self.outdoor_portal_retry_object_id = 0;
        self.interaction_origin_position = None;
        self.interaction_saw_portal_space = false;
        self.resolved_indoor_portal_object_id = 0;
        self.resolved_indoor_portal_position = None;
        self.table_exit = None;
        self.leg_index += 1;
        if start_next_leg && self.replan_after_wrong_arrival(cx, &completed) {
            return;
        }
        if !has_steps(cx.destination) {
            self.has_arrived = true;
            self.post_arrived(cx);
            return;
        }
        if start_next_leg {
            self.start_current_leg(cx);
        }
    }

    // ---- legs ----

    #[allow(clippy::too_many_lines)]
    fn start_current_leg(&mut self, cx: &mut NavContext<'_, '_>) {
        // A new leg ends any wait for a door.
        self.door_wait = None;
        let Some(step) = first_step(cx.destination) else {
            return;
        };
        if !self.go_requested {
            return;
        }
        let snapshot = cx.host.navigation();
        if snapshot.is_available && snapshot.is_portal_space {
            self.is_navigating = false;
            self.paused_for_outdoor_route = true;
            self.failure_reason = "Waiting for portal transition.".to_owned();
            return;
        }
        if self.try_complete_known_indoor_arrival(cx, &snapshot) {
            return;
        }
        self.update_landscape_walk(&snapshot);
        if snapshot.is_available
            && !snapshot.position.is_outdoor
            && self.landscape_walk_position.is_none()
        {
            // An envcell can be a cave or a room joined to the landscape:
            // go on to the nearby Atlas entrance by its live object.
            if step.kind == RouteStepKind::Travel
                && !text::starts_with_ignore_case(&step.to.name, "Town Network (")
                && self.try_start_outdoor_portal_walk(cx, false)
            {
                return;
            }
            if step.kind == RouteStepKind::Portal && is_near_route_entrance(&snapshot, &step.from) {
                let entrance = self.find_route_entrance_portal(cx, &step.from, &snapshot, false);
                if let Some(entrance) = entrance {
                    self.outdoor_portal_retry_object_id = entrance.object_id;
                    self.paused_for_outdoor_route = false;
                    self.try_start_interaction(cx, &step);
                    return;
                }
            }
            if self.route_id != 0
                && step.kind == RouteStepKind::Portal
                && text::starts_with_ignore_case(&step.via, "Town Network Portal(")
                && !is_near_route_entrance(&snapshot, &step.from)
            {
                // These Atlas records lead from a town into the Town
                // Network: indoors away from the entrance, the entry is
                // done (its notice missed, or Stop cleared it).
                self.note_taken(&step);
                cx.destination.advance_step();
                self.waiting_for_interaction = false;
                self.resolved_indoor_portal_object_id = 0;
                self.resolved_indoor_portal_position = None;
                self.table_exit = None;
                self.active_interaction_object_id = 0;
                self.leg_index += 1;
                self.start_current_leg(cx);
                return;
            }
            if is_indoor_portal_step(cx.destination) {
                self.observed_indoor_route = true;
            }
            if is_walk_to_portal(cx.destination, &step)
                && self.try_resolve_indoor_portal(cx, &snapshot, &step.to.name)
            {
                self.indoor_route_leg = true;
                self.paused_for_outdoor_route = false;
                self.start_indoor_walk(cx);
                if !self.is_navigating {
                    self.indoor_route_leg = false;
                    self.paused_for_outdoor_route = true;
                }
                return;
            }
            if step.kind == RouteStepKind::Portal
                && (self.resolved_indoor_portal_object_id != 0
                    || self.try_resolve_indoor_portal(cx, &snapshot, &step.via))
            {
                self.paused_for_outdoor_route = false;
                self.transition_generation += 1;
                self.try_start_interaction(cx, &step);
                return;
            }
            let exit_needed = needs_dungeon_exit(cx.destination, &step);
            if exit_needed && self.try_resolve_route_dungeon_exit(cx, &snapshot, &step) {
                self.indoor_surface_exit_leg = true;
                self.paused_for_outdoor_route = false;
                self.start_indoor_walk(cx);
                if !self.is_navigating {
                    self.indoor_surface_exit_leg = false;
                    self.paused_for_outdoor_route = true;
                }
                return;
            }
            if exit_needed {
                if self.waiting_for_dungeon_portal_identification
                    && self.has_nearby_pending_portal_identification(cx, &snapshot, false)
                {
                    self.is_navigating = false;
                    self.paused_for_outdoor_route = true;
                    self.failure_reason = "Identifying nearby dungeon portal.".to_owned();
                    return;
                }
                self.waiting_for_dungeon_portal_identification = false;
                self.search.use_recorded_exit(&dungeon_exit_key(&step));
                self.observe_traversal(cx.host, &snapshot);
                let exit = route_dungeon_exit_name(&step)
                    .unwrap_or_else(|| format!("exit near {}", step.from.coords));
                self.is_navigating = false;
                self.paused_for_outdoor_route = true;
                if !self.try_start_dungeon_exploration(cx, &exit, false) {
                    self.failure_reason = format!(
                        "{} {}",
                        self.indoor_portal_search_reason(cx, &snapshot, &step),
                        self.search.reason()
                    );
                }
                return;
            }
            self.is_navigating = false;
            self.paused_for_outdoor_route = true;
            if self.should_explore_for_portal(cx, &snapshot, &step) {
                self.explore_for_portal(cx, &snapshot, &step);
                return;
            }
            let reason = self.indoor_portal_search_reason(cx, &snapshot, &step);
            if self.failure_reason != reason {
                cx.host.info(&format!("GoArrow: {reason}"));
            }
            self.failure_reason = reason;
            return;
        }

        if step.kind != RouteStepKind::Travel {
            self.transition_generation += 1;
            self.try_start_interaction(cx, &step);
            return;
        }

        let immediate = step.to.clone();
        self.walking_portal_origin =
            is_walk_to_portal(cx.destination, &step).then(|| snapshot.position.clone());
        self.walking_portal_saw_transit = false;
        self.outdoor_portal_retry_object_id = 0;
        self.is_navigating = true;
        if self.try_start_outdoor_portal_walk(cx, false) {
            return;
        }
        let place = outdoor_place(immediate.coords.ew, immediate.coords.ns);
        let status = cx
            .host
            .go_to_place(place.clone(), cx.settings.arrival_distance as f32);
        if status != NavigationStatus::Accepted {
            self.is_navigating = false;
            cx.host.warn(&format!(
                "GoArrow: Navigation request was {}.",
                status_name(status)
            ));
            return;
        }
        self.active_sequence = cx.host.go_to_report().sequence;
        self.last_handled_report_revision = 0;
        self.failure_reason.clear();
        cx.host.info(&format!(
            "GoArrow: Navigating to {} at ({}, {})",
            immediate.name,
            text::format_fixed(place.east_west, 2, 2),
            text::format_fixed(place.north_south, 2, 2)
        ));
    }

    // A cell's outdoor flag says what the cell is, not whether a portal
    // was taken: a short walk into an envcell of the same landblock keeps
    // the outdoor route. Portal space, interactions and jumps end it.
    fn update_landscape_walk(&mut self, snapshot: &NavigationSnapshot) {
        if !is_in_world(snapshot)
            || self.waiting_for_interaction
            || self.indoor_walk
            || self.indoor_route_leg
            || self.indoor_surface_exit_leg
        {
            self.landscape_walk_position = None;
            return;
        }
        if snapshot.position.is_outdoor {
            self.landscape_walk_position = Some(snapshot.position.clone());
            return;
        }
        let continues = self
            .landscape_walk_position
            .as_ref()
            .is_some_and(|previous| {
                (previous.cell_id >> 16) == (snapshot.position.cell_id >> 16)
                    && previous.horizontal_distance_metres(&snapshot.position) <= 60.0
            });
        self.landscape_walk_position = continues.then(|| snapshot.position.clone());
    }

    // Indoor objects reach the client only as their cells come into view,
    // so a portal in another room needs a search: anywhere in a sealed
    // dungeon, elsewhere only at the portal's Atlas location, so a
    // building passed on the way is not explored.
    fn should_explore_for_portal(
        &self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        step: &RouteStep,
    ) -> bool {
        route_portal_name(cx.destination, step).is_some()
            && is_indoors(snapshot)
            && (cx.host.is_sealed_dungeon(snapshot.position.cell_id)
                || is_near_route_entrance(
                    snapshot,
                    if step.kind == RouteStepKind::Portal {
                        &step.from
                    } else {
                        &step.to
                    },
                ))
    }

    // As the leg resolves the portal: by name, or as the live portal at
    // the Atlas entrance whatever its name.
    fn is_route_portal_visible(
        &self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        step: &RouteStep,
    ) -> bool {
        let Some(name) = route_portal_name(cx.destination, step) else {
            return false;
        };
        if self.find_indoor_portal(cx, snapshot, &name).is_some() {
            return true;
        }
        let entrance = if step.kind == RouteStepKind::Portal {
            &step.from
        } else {
            &step.to
        };
        is_near_route_entrance(snapshot, entrance)
            && self
                .find_route_entrance_portal(cx, entrance, snapshot, false)
                .is_some()
    }

    fn exploration_planning_timed_out(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        elapsed: f64,
    ) -> bool {
        if cx.host.go_to_report().state != GoToState::Planning {
            self.exploration_planning_elapsed = 0.0;
            return false;
        }
        self.exploration_planning_elapsed += elapsed.max(0.0);
        self.exploration_planning_elapsed >= EXPLORATION_PLANNING_TIMEOUT_SECONDS
    }

    fn explore_for_portal(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        step: &RouteStep,
    ) {
        let portal_name =
            portal_destination_name(&route_portal_name(cx.destination, step).unwrap_or_default());
        self.search
            .use_recorded_exit(&format!("PORTAL|{}", portal_name.to_uppercase()));
        self.observe_traversal(cx.host, snapshot);
        self.is_navigating = false;
        self.paused_for_outdoor_route = true;
        self.waiting_for_interaction = false;
        let may_pass_outdoors = !cx.host.is_sealed_dungeon(snapshot.position.cell_id);
        if !self.try_start_dungeon_exploration(cx, &portal_name, may_pass_outdoors) {
            self.failure_reason = format!(
                "{} {}",
                self.indoor_portal_search_reason(cx, snapshot, step),
                self.search.reason()
            );
        }
    }

    fn try_start_dungeon_exploration(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        portal_name: &str,
        may_pass_outdoors: bool,
    ) -> bool {
        let objects = self.objects(cx.host);
        let Some(target) =
            self.search
                .try_get_next_target(cx.host, &objects, &self.traversals, self.clock)
        else {
            return false;
        };
        let status = cx.host.go_to_place(target.clone(), 2.5);
        if status != NavigationStatus::Accepted {
            self.failure_reason =
                format!("Dungeon exploration request was {}.", status_name(status));
            return true;
        }
        self.search.target_accepted(target.cell_id);
        self.exploration_planning_elapsed = 0.0;
        self.exploration_may_pass_outdoors = may_pass_outdoors;
        self.exploring_dungeon_exit = true;
        self.paused_for_outdoor_route = false;
        self.is_navigating = true;
        self.active_sequence = cx.host.go_to_report().sequence;
        self.last_handled_report_revision = 0;
        self.failure_reason = format!(
            "Exploring dungeon for portal '{portal_name}'. {}",
            self.search.reason()
        );
        true
    }

    fn indoor_portal_search_reason(
        &self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        step: &RouteStep,
    ) -> String {
        let expected = if is_walk_to_portal(cx.destination, step) {
            portal_destination_name(&step.to.name)
        } else {
            route_dungeon_exit_name(step)
                .or_else(|| {
                    needs_dungeon_exit(cx.destination, step)
                        .then(|| format!("exit near {} toward {}", step.from.coords, step.to.name))
                })
                .unwrap_or_else(|| {
                    if step.kind == RouteStepKind::Portal {
                        step.via.clone()
                    } else {
                        step.to.name.clone()
                    }
                })
        };
        let here = &snapshot.position;
        let objects = self.objects(cx.host);
        let portals: Vec<&WorldObject> = objects.iter().filter(|o| is_portal(o)).collect();
        let in_dungeon = |o: &WorldObject| {
            o.position
                .as_ref()
                .is_some_and(|p| landblock(p.cell_id) == landblock(here.cell_id))
        };
        let current_dungeon_portals = portals.iter().filter(|o| in_dungeon(o)).count();
        let mut visible: Vec<&&WorldObject> = portals.iter().collect();
        visible.sort_by(|a, b| {
            u8::from(!in_dungeon(a))
                .cmp(&u8::from(!in_dungeon(b)))
                .then_with(|| {
                    let da = a
                        .position
                        .as_ref()
                        .map_or(f64::INFINITY, |p| here.horizontal_distance_metres(p));
                    let db = b
                        .position
                        .as_ref()
                        .map_or(f64::INFINITY, |p| here.horizontal_distance_metres(p));
                    da.total_cmp(&db)
                })
        });
        let visible: Vec<String> = visible
            .iter()
            .take(8)
            .map(|o| {
                let pos = position_of(o);
                format!(
                    "'{}' destination='{}' cell={:08X} position={} outdoor={} activate={}",
                    o.name,
                    o.portal_destination
                        .clone()
                        .unwrap_or_else(|| if o.has_appraisal_data {
                            "not provided".to_owned()
                        } else {
                            "unknown".to_owned()
                        }),
                    pos.cell_id,
                    tf(o.position.is_some()),
                    tf(pos.is_outdoor),
                    tf(o.can_activate())
                )
            })
            .collect();
        format!(
            "Waiting for indoor portal '{expected}'; player cell={:08X}. {} \
             Portals in current dungeon: {current_dungeon_portals}; visible portals: {}. \
             Identification pending: {}.",
            here.cell_id,
            named_portal_diagnostic(snapshot, &objects, &expected),
            if visible.is_empty() {
                "none".to_owned()
            } else {
                visible.join("; ")
            },
            if self.pending_identification == 0 {
                "none".to_owned()
            } else {
                format!("{:08X}", self.pending_identification)
            }
        )
    }

    fn try_start_outdoor_portal_walk(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        coordinate_failed: bool,
    ) -> bool {
        let snapshot = cx.host.navigation();
        let all = steps(cx.destination);
        if self.outdoor_portal_retry_object_id != 0
            || !is_in_world(&snapshot)
            || all.len() <= 1
            || all[0].kind != RouteStepKind::Travel
            || all[1].kind != RouteStepKind::Portal
        {
            return false;
        }
        let target = all[0].to.clone();
        if !snapshot.position.is_outdoor && !is_near_route_entrance(&snapshot, &target) {
            return false;
        }
        let Some(portal) =
            self.find_route_entrance_portal(cx, &target, &snapshot, coordinate_failed)
        else {
            return false;
        };
        self.outdoor_portal_retry_object_id = portal.object_id;
        let status = cx.host.go_to(
            portal.object_id,
            2.5f64.max(cx.settings.arrival_distance) as f32,
        );
        if status != NavigationStatus::Accepted {
            return false;
        }
        self.is_navigating = true;
        self.observed_indoor_route = false;
        self.paused_for_outdoor_route = false;
        self.active_sequence = cx.host.go_to_report().sequence;
        self.last_handled_report_revision = 0;
        self.failure_reason.clear();
        cx.host.info(&format!(
            "GoArrow: Approaching live portal '{}' (0x{:08X}).",
            portal.name, portal.object_id
        ));
        true
    }

    // A distant goal can fail because every point reachable now is
    // farther from it. Try a bounded sideways or backward step, then let
    // the client plan the whole route again from the point reached.
    fn try_start_outdoor_detour(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        let snapshot = cx.host.navigation();
        let Some(first) = first_step(cx.destination) else {
            return false;
        };
        if !is_in_world(&snapshot)
            || !snapshot.position.is_outdoor
            || first.kind != RouteStepKind::Travel
        {
            return false;
        }
        let goal = first.to.coords;
        if self.outdoor_detour_goal != goal {
            self.outdoor_detour_goal = goal;
            self.outdoor_detour_attempts = 0;
            self.outdoor_detour_tried.clear();
        }
        if self.outdoor_detour_attempts >= MAXIMUM_OUTDOOR_DETOURS {
            return false;
        }
        let here = &snapshot.position;
        let dx = (goal.ew - here.east_west) * METRES_PER_MAP_UNIT;
        let dy = (goal.ns - here.north_south) * METRES_PER_MAP_UNIT;
        let length = (dx * dx + dy * dy).sqrt();
        if length < 100.0 || !length.is_finite() {
            return false;
        }
        let (forward_x, forward_y) = (dx / length, dy / length);
        let (side_x, side_y) = (-forward_y, forward_x);
        let portals: Vec<Position> = self
            .objects(cx.host)
            .iter()
            .filter(|p| is_portal(p))
            .filter_map(|p| p.position.clone())
            .collect();
        for (forward, side) in OUTDOOR_DETOUR_OFFSETS {
            if self.outdoor_detour_attempts >= MAXIMUM_OUTDOOR_DETOURS {
                break;
            }
            let candidate = outdoor_place(
                here.east_west + (forward_x * forward + side_x * side) / METRES_PER_MAP_UNIT,
                here.north_south + (forward_y * forward + side_y * side) / METRES_PER_MAP_UNIT,
            );
            if self
                .outdoor_detour_tried
                .iter()
                .any(|p| p.horizontal_distance_metres(&candidate) < 25.0)
                || portals
                    .iter()
                    .any(|p| p.horizontal_distance_metres(&candidate) < 20.0)
            {
                continue;
            }
            self.outdoor_detour_tried.push(candidate.clone());
            self.outdoor_detour_attempts += 1;
            if cx.host.go_to_place(candidate.clone(), 4.0) != NavigationStatus::Accepted {
                continue;
            }
            self.outdoor_detour_active = true;
            self.is_navigating = true;
            self.active_sequence = cx.host.go_to_report().sequence;
            self.last_handled_report_revision = 0;
            self.failure_reason = format!(
                "Trying outdoor detour {}/{MAXIMUM_OUTDOOR_DETOURS} toward {}, {}.",
                self.outdoor_detour_attempts,
                text::format_fixed(candidate.north_south, 2, 2),
                text::format_fixed(candidate.east_west, 2, 2)
            );
            return true;
        }
        false
    }

    fn find_route_entrance_portal(
        &self,
        cx: &mut NavContext<'_, '_>,
        target: &Location,
        snapshot: &NavigationSnapshot,
        coordinate_failed: bool,
    ) -> Option<WorldObject> {
        let expected = if text::starts_with_ignore_case(&target.name, "Town Network Portal(") {
            "Town Network".to_owned()
        } else {
            portal_destination_name(&target.name)
        };
        let mut candidates: Vec<(WorldObject, f64, f64, bool)> = self
            .objects(cx.host)
            .iter()
            .filter(|o| {
                o.position.is_some()
                    && o.can_activate()
                    && !is_unknown_surface_portal(o)
                    && is_portal(o)
            })
            .map(|o| {
                let pos = position_of(o);
                let distance = target.coords.distance_to(coords_of(&pos));
                let player_distance = snapshot.position.horizontal_distance_metres(&pos);
                let matches = portal_matches_destination(o, &expected);
                (o.clone(), distance, player_distance, matches)
            })
            .filter(|(_, distance, player_distance, matches)| {
                if *matches {
                    coordinate_failed || *player_distance <= 150.0
                } else {
                    *distance <= 0.1 && (coordinate_failed || *player_distance <= 60.0)
                }
            })
            .collect();
        candidates.sort_by(|a, b| {
            u8::from(!a.3)
                .cmp(&u8::from(!b.3))
                .then(a.2.total_cmp(&b.2))
                .then(a.1.total_cmp(&b.1))
        });
        candidates.into_iter().next().map(|c| c.0)
    }

    fn is_near_town_network_exit(&self, cx: &NavContext<'_, '_>, position: &Position) -> bool {
        let all = steps(cx.destination);
        if all.len() <= 1
            || all[0].kind != RouteStepKind::Travel
            || all[1].kind != RouteStepKind::Portal
            || !text::starts_with_ignore_case(&all[0].to.name, "Town Network ")
            || !all[1].to.has_coordinates()
        {
            return false;
        }
        coords_of(position).distance_to(all[1].to.coords) * METRES_PER_MAP_UNIT <= 250.0
    }

    fn live_object_target(&self, cx: &NavContext<'_, '_>) -> bool {
        cx.destination.kind() == DestinationKind::Object
            && !cx.destination.target_unavailable()
            && cx.destination.target_object_id().is_some_and(|id| id > 0)
    }

    fn can_navigate_indoor_target(
        &self,
        cx: &NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
    ) -> bool {
        let object_target = self.live_object_target(cx);
        let target = if object_target {
            cx.destination
                .target_object_position()
                .map(|p| from_nav(&p))
        } else {
            cx.destination
                .target_indoor_position()
                .map(|p| from_nav(&p.nav_position()))
                .or_else(|| self.resolved_indoor_portal_position.clone())
        };
        let live_object = self.resolved_indoor_portal_object_id != 0 || object_target;
        is_indoors(snapshot)
            && snapshot.position.cell_id != 0
            && target.is_some_and(|t| {
                !t.is_outdoor
                    && t.cell_id != 0
                    && if live_object {
                        are_nearby_indoor_landblocks(snapshot.position.cell_id, t.cell_id)
                    } else {
                        landblock(snapshot.position.cell_id) == landblock(t.cell_id)
                    }
            })
    }

    /// The same indoor target a direct walk would use: an object id (or 0)
    /// and its position.
    fn indoor_preview_target(
        &self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
    ) -> Option<(u32, Position)> {
        if !is_indoors(snapshot) {
            return None;
        }
        let can = self.can_navigate_indoor_target(cx, snapshot);
        if cx.destination.kind() == DestinationKind::Object
            && can
            && let (Some(id), Some(position)) = (
                cx.destination.target_object_id(),
                cx.destination.target_object_position(),
            )
        {
            return Some((id, from_nav(&position)));
        }
        if let Some(indoor) = cx.destination.target_indoor_position()
            && can
        {
            return Some((0, from_nav(&indoor.nav_position())));
        }
        if let Some(exit) = &self.table_exit
            && can
        {
            return Some((0, exit.position()));
        }
        if self.resolved_indoor_portal_object_id != 0
            && can
            && let Some(position) = &self.resolved_indoor_portal_position
        {
            return Some((self.resolved_indoor_portal_object_id, position.clone()));
        }
        if cx.destination.kind() == DestinationKind::Location
            && cx.destination.target_indoor_position().is_none()
        {
            let name = cx.destination.target_name().to_owned();
            if let Some(portal) = self.find_indoor_portal(cx, snapshot, &name) {
                return Some((portal.object_id, position_of(&portal)));
            }
        }
        None
    }

    /// Resolves a live indoor portal matching the destination's name.
    fn try_resolve_destination_portal(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
    ) -> bool {
        if !is_indoors(snapshot)
            || snapshot.position.cell_id == 0
            || cx.destination.kind() != DestinationKind::Location
            || cx.destination.target_indoor_position().is_some()
        {
            return false;
        }
        let name = cx.destination.target_name().to_owned();
        self.try_resolve_indoor_portal(cx, snapshot, &name)
    }

    fn try_resolve_indoor_portal(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        name: &str,
    ) -> bool {
        let Some(portal) = self.find_indoor_portal(cx, snapshot, name) else {
            return false;
        };
        self.resolved_indoor_portal_object_id = portal.object_id;
        self.resolved_indoor_portal_position = portal.position;
        true
    }

    fn find_indoor_portal(
        &self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        name: &str,
    ) -> Option<WorldObject> {
        if !is_indoors(snapshot) || snapshot.position.cell_id == 0 {
            return None;
        }
        let destination_name = portal_destination_name(name);
        if destination_name.is_empty() {
            return None;
        }
        let mut matching: Vec<WorldObject> = self
            .objects(cx.host)
            .iter()
            .filter(|o| indoor_portal_rejection(snapshot, o, &destination_name).is_none())
            .cloned()
            .collect();
        matching.sort_by(|a, b| {
            u8::from(a.portal_destination.is_none())
                .cmp(&u8::from(b.portal_destination.is_none()))
                .then(
                    snapshot
                        .position
                        .horizontal_distance_metres(&position_of(a))
                        .total_cmp(
                            &snapshot
                                .position
                                .horizontal_distance_metres(&position_of(b)),
                        ),
                )
        });
        matching.into_iter().next()
    }

    fn try_resolve_route_dungeon_exit(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        step: &RouteStep,
    ) -> bool {
        let portals: Vec<WorldObject> = self
            .objects(cx.host)
            .iter()
            .filter(|p| {
                p.position.as_ref().is_some_and(|pos| {
                    p.can_activate()
                        && !pos.is_outdoor
                        && has_portal_capability(p)
                        && are_nearby_indoor_landblocks(snapshot.position.cell_id, pos.cell_id)
                })
            })
            .cloned()
            .collect();
        let exit_name = route_dungeon_exit_name(step);
        self.request_portal_destination(
            cx,
            &portals,
            exit_name.as_deref(),
            Some(&snapshot.position),
        );
        let expected = exit_name.unwrap_or_else(|| portal_destination_name(&step.to.name));
        let mut named: Vec<&WorldObject> = portals
            .iter()
            .filter(|p| {
                p.portal_destination.as_deref().is_some_and(|d| {
                    !is_blank(Some(d))
                        && text::eq_ignore_case(&portal_destination_label(d), &expected)
                })
            })
            .collect();
        named.sort_by(|a, b| {
            snapshot
                .position
                .horizontal_distance_metres(&position_of(a))
                .total_cmp(
                    &snapshot
                        .position
                        .horizontal_distance_metres(&position_of(b)),
                )
        });
        let chosen = if let Some(first) = named.first() {
            Some((*first).clone())
        } else {
            // Atlas arrival coordinates are estimates: a destination within
            // one map unit helps when its shown name differs.
            let nearby: Vec<&WorldObject> = portals
                .iter()
                .filter(|p| {
                    p.portal_destination
                        .as_deref()
                        .and_then(Coordinates::parse)
                        .is_some_and(|arrival| {
                            arrival.distance_to(step.from.coords) * METRES_PER_MAP_UNIT
                                <= DUNGEON_EXIT_ARRIVAL_METRES
                        })
                })
                .take(2)
                .collect();
            (nearby.len() == 1).then(|| nearby[0].clone())
        };
        let chosen = match chosen {
            Some(chosen) => chosen,
            None => {
                // The table knows the exits before they are in view: the
                // one it names, or that leads near the arrival.
                let Some(exit) = self.table_route_exit(snapshot, step, &expected) else {
                    return false;
                };
                match portals.iter().find(|p| exit.is_object(p)) {
                    Some(live) => live.clone(),
                    None => {
                        self.resolved_indoor_portal_object_id = 0;
                        self.resolved_indoor_portal_position = Some(exit.position());
                        self.table_exit = Some(exit);
                        return true;
                    }
                }
            }
        };
        self.table_exit = None;
        self.resolved_indoor_portal_object_id = chosen.object_id;
        self.resolved_indoor_portal_position = chosen.position.clone();
        self.traversals
            .remember_exit(cx.host, &dungeon_exit_key(step), &chosen, snapshot);
        true
    }

    /// The ways out of this dungeon the table knows, one for each place
    /// they lead (named after the first portal there), for planning a route
    /// from inside it.
    fn table_dungeon_exits(&self, cell_id: u32) -> Vec<KnownExit> {
        let mut exits: Vec<KnownExit> = Vec::new();
        for portal in self.portals.exits_outdoors(cell_id) {
            let Some(arrival) = portal.arrival() else {
                continue;
            };
            let arrival = arrival.round(4);
            if exits.iter().any(|e| e.arrival == arrival) {
                continue;
            }
            exits.push(KnownExit {
                name: portal.name.clone(),
                id: portal.guid.map_or_else(
                    || format!("portal table, cell {:08X}", portal.cell_id),
                    |guid| format!("portal table {guid:08X}"),
                ),
                arrival,
            });
        }
        exits
    }

    /// The table's way out for a walk from a dungeon arrival: of the usable
    /// portals in this landblock that lead outdoors (and were not found
    /// missing), the nearest whose destination is the exit `expected`, else
    /// the nearest that leads within [`DUNGEON_EXIT_ARRIVAL_METRES`] of the
    /// step's arrival.
    fn table_route_exit(
        &self,
        snapshot: &NavigationSnapshot,
        step: &RouteStep,
        expected: &str,
    ) -> Option<AcePortal> {
        let here = &snapshot.position;
        let exits: Vec<&AcePortal> = self
            .portals
            .exits_outdoors(here.cell_id)
            .filter(|p| !self.missing_table_exits.contains(&(p.cell_id, p.local)))
            .collect();
        let nearest = |candidates: Vec<&&AcePortal>| {
            candidates
                .into_iter()
                .min_by(|a, b| {
                    crate::ace_portals::distance_metres(here, &a.position())
                        .total_cmp(&crate::ace_portals::distance_metres(here, &b.position()))
                })
                .map(|p| (*p).clone())
        };
        let named: Vec<&&AcePortal> = exits
            .iter()
            .filter(|p| {
                p.destination_label().is_some_and(|label| {
                    text::eq_ignore_case(&portal_destination_label(&label), expected)
                })
            })
            .collect();
        if !named.is_empty() {
            return nearest(named);
        }
        nearest(
            exits
                .iter()
                .filter(|p| {
                    p.arrival().is_some_and(|arrival| {
                        arrival.distance_to(step.from.coords) * METRES_PER_MAP_UNIT
                            <= DUNGEON_EXIT_ARRIVAL_METRES
                    })
                })
                .collect(),
        )
    }

    // The walk to the table's exit: once its portal is in view the walk
    // goes to the object, as for a portal found live. True when it did.
    fn try_take_over_table_exit(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        let Some(exit) = self.table_exit.clone() else {
            return false;
        };
        let Some(live) = self
            .objects(cx.host)
            .iter()
            .find(|o| o.can_activate() && has_portal_capability(o) && exit.is_object(o))
            .cloned()
        else {
            return false;
        };
        self.table_exit = None;
        self.resolved_indoor_portal_object_id = live.object_id;
        self.resolved_indoor_portal_position = live.position.clone();
        let snapshot = cx.host.navigation();
        // The rest as for a portal found live.
        if let Some(step) = first_step(cx.destination) {
            self.traversals
                .remember_exit(cx.host, &dungeon_exit_key(&step), &live, &snapshot);
        }
        true
    }

    // Arrived where the table puts the exit, and no such portal is there
    // (the server may not have it, or not now): the next try leaves it
    // out, so another exit or the search follows.
    fn table_exit_missing(&mut self) {
        if let Some(exit) = self.table_exit.take() {
            self.missing_table_exits.push((exit.cell_id, exit.local));
            self.failure_reason = format!(
                "Portal '{}' was not where the portal table puts it (cell {:08X}).",
                exit.name, exit.cell_id
            );
        }
        self.resolved_indoor_portal_object_id = 0;
        self.resolved_indoor_portal_position = None;
        self.table_exit = None;
        self.is_navigating = false;
        self.paused_for_outdoor_route = true;
        self.indoor_retry_elapsed = 0.0;
    }

    fn has_nearby_pending_portal_identification(
        &self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        approaching_only: bool,
    ) -> bool {
        if self.pending_identification == 0 {
            return false;
        }
        let here = &snapshot.position;
        self.objects(cx.host).iter().any(|p| {
            p.object_id != 0
                && p.position.as_ref().is_some_and(|pos| {
                    p.can_activate()
                        && !pos.is_outdoor
                        && has_portal_capability(p)
                        && landblock(pos.cell_id) == landblock(here.cell_id)
                        && is_blank(p.portal_destination.as_deref())
                        && here.horizontal_distance_metres(pos) <= 12.0
                        && (here.elevation - pos.elevation).abs() * METRES_PER_MAP_UNIT <= 6.0
                        && (!approaching_only || self.search.is_approaching_portal(here, pos))
                })
        })
    }

    fn try_resolve_surface_exit(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
        expected_destination: Option<&str>,
    ) -> bool {
        if snapshot.position.cell_id == 0 {
            return false;
        }
        // Atlas routes can put an outdoor portal at the map coordinates of
        // a dungeon exit, and the live exit is often just "Surface Portal".
        // A matching destination wins; a unique Surface Portal is used
        // only once identification gave its destination.
        let portals: Vec<WorldObject> = self
            .objects(cx.host)
            .iter()
            .filter(|o| {
                o.object_id != 0
                    && o.position.as_ref().is_some_and(|pos| {
                        o.can_activate()
                            && has_portal_capability(o)
                            && !pos.is_outdoor
                            && pos.cell_id != 0
                            && are_nearby_indoor_landblocks(snapshot.position.cell_id, pos.cell_id)
                    })
            })
            .cloned()
            .collect();
        let mut chosen: Option<WorldObject> = None;
        if let Some(expected) = expected_destination.filter(|e| !is_blank(Some(e))) {
            let destination = portal_destination_name(expected);
            let matching: Vec<&WorldObject> = portals
                .iter()
                .filter(|o| {
                    o.portal_destination.as_deref().is_some_and(|label| {
                        text::eq_ignore_case(&portal_destination_name(label), &destination)
                    })
                })
                .take(2)
                .collect();
            match matching.len() {
                1 => chosen = Some(matching[0].clone()),
                0 => {}
                _ => return false,
            }
        }
        if chosen.is_none() {
            let surfaces: Vec<WorldObject> = portals
                .iter()
                .filter(|o| text::eq_ignore_case(&o.name, "Surface Portal"))
                .take(2)
                .cloned()
                .collect();
            self.request_portal_destination(cx, &surfaces, None, None);
            if surfaces.len() == 1 && !is_blank(surfaces[0].portal_destination.as_deref()) {
                chosen = Some(surfaces[0].clone());
            }
        }
        let Some(chosen) = chosen else {
            return false;
        };
        self.resolved_indoor_portal_object_id = chosen.object_id;
        self.resolved_indoor_portal_position = chosen.position;
        true
    }

    fn ensure_portal_destination_known(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        object_id: u32,
    ) -> bool {
        let Some(portal) = self.object(cx.host, object_id) else {
            return true;
        };
        if !text::eq_ignore_case(&portal.name, "Surface Portal")
            || !is_blank(portal.portal_destination.as_deref())
        {
            return true;
        }
        self.request_portal_destination(cx, std::slice::from_ref(&portal), None, None);
        self.waiting_for_surface_exit = false;
        self.waiting_for_interaction = false;
        self.paused_for_outdoor_route = true;
        self.failure_reason =
            "Waiting for Surface Portal identification; destination is unknown.".to_owned();
        false
    }

    fn request_portal_destination(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        portals: &[WorldObject],
        preferred_destination: Option<&str>,
        current_position: Option<&Position>,
    ) {
        if self.pending_identification != 0 {
            // A missing answer must not block every later portal; a late
            // one still updates the object for the next tick.
            if self.clock - self.pending_identification_started < IDENTIFICATION_RETRY_SECONDS {
                return;
            }
            self.pending_identification = 0;
            self.pending_identification_started = 0.0;
        }
        let mut ordered: Vec<&WorldObject> = portals.iter().collect();
        ordered.sort_by(|a, b| {
            let distance = |o: &WorldObject| {
                current_position.map_or(0.0, |p| p.horizontal_distance_metres(&position_of(o)))
            };
            let attempted =
                |o: &WorldObject| u8::from(self.identification_attempts.contains_key(&o.object_id));
            let preferred = |o: &WorldObject| {
                preferred_destination
                    .is_some_and(|d| text::eq_ignore_case(&portal_destination_name(&o.name), d))
            };
            distance(a)
                .total_cmp(&distance(b))
                .then(attempted(a).cmp(&attempted(b)))
                .then(preferred(b).cmp(&preferred(a)))
        });
        for portal in ordered {
            if !is_blank(portal.portal_destination.as_deref()) {
                continue;
            }
            let previous = self.identification_attempts.get(&portal.object_id).copied();
            if previous.is_some_and(|p| {
                p.count >= MAXIMUM_PORTAL_IDENTIFICATION_ATTEMPTS
                    || self.clock - p.started < IDENTIFICATION_RETRY_SECONDS
            }) {
                continue;
            }
            let result = cx.host.identify(portal.object_id);
            if result.status == ItemStatus::Busy {
                return;
            }
            self.identification_attempts.insert(
                portal.object_id,
                Attempt {
                    started: self.clock,
                    count: previous.map_or(0, |p| p.count) + 1,
                    status: result.status,
                },
            );
            if result.accepted() {
                self.pending_identification = portal.object_id;
                self.pending_identification_started = self.clock;
                return;
            }
        }
    }

    fn start_indoor_walk(&mut self, cx: &mut NavContext<'_, '_>) {
        if !self.go_requested {
            return;
        }
        let arrival = cx.settings.arrival_distance as f32;
        let status = if cx.destination.kind() == DestinationKind::Object
            && let Some(object_id) = cx.destination.target_object_id()
        {
            cx.host.go_to(object_id, arrival)
        } else if self.resolved_indoor_portal_object_id != 0 {
            cx.host.go_to(
                self.resolved_indoor_portal_object_id,
                arrival.min(PORTAL_APPROACH_DISTANCE),
            )
        } else if let Some(exit) = &self.table_exit {
            // Not in view yet: walk to where the table says it stands.
            cx.host
                .go_to_place(exit.position(), arrival.min(PORTAL_APPROACH_DISTANCE))
        } else if let Some(position) = cx.destination.target_indoor_position() {
            cx.host
                .go_to_place(from_nav(&position.nav_position()), arrival)
        } else {
            return;
        };
        if status != NavigationStatus::Accepted {
            self.failure_reason = format!("Indoor navigation request was {}.", status_name(status));
            cx.host.warn(&format!("GoArrow: {}", self.failure_reason));
            return;
        }
        self.indoor_walk = true;
        self.is_navigating = true;
        self.failure_reason.clear();
        self.active_sequence = cx.host.go_to_report().sequence;
        self.last_handled_report_revision = 0;
        cx.host.info(&format!(
            "GoArrow: Navigating indoors to '{}'.",
            cx.destination.target_name()
        ));
    }

    // ---- reports and events ----

    #[allow(clippy::too_many_lines)]
    fn handle_navigation_report(&mut self, cx: &mut NavContext<'_, '_>, report: &GoToReport) {
        // Reports from another owner or an earlier walk must not move the
        // route on; a later walk of another owner (a plugin or the player)
        // has ended ours.
        if report
            .owner
            .as_deref()
            .is_some_and(|owner| !text::eq_ignore_case(owner, &self.owner))
        {
            if self.is_navigating
                && self.active_sequence != 0
                && report.sequence > self.active_sequence
            {
                self.taken_over(cx);
            }
            return;
        }
        if self.active_sequence != 0
            && report.sequence != 0
            && report.sequence != self.active_sequence
        {
            return;
        }
        if report.revision != 0 && report.revision == self.last_handled_report_revision {
            return;
        }
        let reason = report.reason.as_deref();

        if report.state == GoToState::Lost
            && self.walking_portal_origin.is_some()
            && (cx.host.navigation().is_portal_space
                || reason.is_some_and(|r| text::contains_ignore_case(r, "portal space")))
        {
            self.walking_portal_saw_transit = true;
            self.last_handled_report_revision = report.revision;
            self.pause_for_portal_exit();
            let snapshot = cx.host.navigation();
            self.try_complete_walking_portal(cx, &snapshot);
            return;
        }

        if self.exploring_dungeon_exit {
            match report.state {
                GoToState::Arrived
                | GoToState::ArrivedWithoutSight
                | GoToState::NoRoute
                | GoToState::Lost => {
                    self.last_handled_report_revision = report.revision;
                    let arrived = matches!(
                        report.state,
                        GoToState::Arrived | GoToState::ArrivedWithoutSight
                    );
                    let position = cx.host.navigation().position;
                    self.search.complete_target(&position, arrived);
                    if !arrived {
                        self.search.record_failure(reason);
                    }
                    self.exploring_dungeon_exit = false;
                    self.is_navigating = false;
                    self.paused_for_outdoor_route = true;
                    self.indoor_retry_elapsed = 0.0;
                }
                GoToState::Blocked | GoToState::Interrupted | GoToState::Stopped => {
                    self.stop_walk(cx.host);
                    self.failure_reason =
                        reason.map_or_else(|| state_name(report.state), str::to_owned);
                }
                _ => {}
            }
            return;
        }

        if self.indoor_walk && report.state == GoToState::ArrivedWithoutSight {
            self.last_handled_report_revision = report.revision;
            self.is_navigating = false;
            self.indoor_walk = false;
            self.failure_reason = reason
                .unwrap_or("Indoor destination was not reachable.")
                .to_owned();
            cx.host
                .chat_post(&format!("GoArrow: {}", esc(&self.failure_reason)));
            return;
        }

        if report.state == GoToState::ArrivedWithoutSight {
            self.last_handled_report_revision = report.revision;
            self.is_navigating = false;
            let target = first_step(cx.destination).map_or_else(
                || cx.destination.target_name().to_owned(),
                |s| s.to.name.clone(),
            );
            let distance = if report.remaining_meters.is_finite() && report.remaining_meters > 0.0 {
                format!(
                    " ({} m remaining)",
                    text::format_fixed(f64::from(report.remaining_meters), 0, 0)
                )
            } else {
                String::new()
            };
            self.failure_reason = format!("Stopped short of '{target}'{distance}.");
            if let Some(r) = reason.filter(|r| !r.trim().is_empty()) {
                self.failure_reason.push(' ');
                self.failure_reason.push_str(r);
            }
            cx.host.warn(&format!("GoArrow: {}", self.failure_reason));
            cx.host
                .chat_post(&format!("GoArrow: {}", esc(&self.failure_reason)));
            return;
        }

        if report.state == GoToState::Arrived {
            self.last_handled_report_revision = report.revision;
            if self.outdoor_portal_retry_object_id != 0
                && !self.indoor_walk
                && let Some(first) = first_step(cx.destination)
                && first.kind == RouteStepKind::Portal
            {
                self.try_start_interaction(cx, &first);
                return;
            }
            if self.outdoor_detour_active {
                self.outdoor_detour_active = false;
                self.is_navigating = false;
                let snapshot = cx.host.navigation();
                if is_in_world(&snapshot) && snapshot.position.is_outdoor && self.plan(cx) {
                    self.start_current_leg(cx);
                    return;
                }
                if self.go_requested {
                    self.failure_reason =
                        "Outdoor detour ended without a usable outdoor route.".to_owned();
                }
                return;
            }
            if self.indoor_walk {
                self.is_navigating = false;
                self.indoor_walk = false;
                if self.indoor_surface_exit_leg {
                    self.indoor_surface_exit_leg = false;
                    if self.table_exit.is_some() && !self.try_take_over_table_exit(cx) {
                        self.table_exit_missing();
                        return;
                    }
                    self.start_surface_exit_interaction(cx);
                    return;
                }
                if self.indoor_route_leg {
                    self.indoor_route_leg = false;
                    self.handle_waypoint_reached(cx);
                    return;
                }
                self.has_arrived = true;
                self.active_sequence = 0;
                self.post_arrived(cx);
                return;
            }
            self.handle_waypoint_reached(cx);
            return;
        }

        // Stopped: a stop or a later walk ended ours.
        if matches!(
            report.state,
            GoToState::NoRoute
                | GoToState::Blocked
                | GoToState::Interrupted
                | GoToState::Lost
                | GoToState::Stopped
        ) {
            if report.state == GoToState::NoRoute
                && !self.indoor_walk
                && self.try_start_outdoor_portal_walk(cx, true)
            {
                return;
            }
            if report.state == GoToState::NoRoute
                && !self.indoor_walk
                && (self.outdoor_detour_active
                    || reason.is_some_and(|r| {
                        text::contains_ignore_case(r, "no clear path leads any nearer the goal")
                    }))
                && self.try_start_outdoor_detour(cx)
            {
                return;
            }
            let detour_exhausted = self.outdoor_detour_active && report.state == GoToState::NoRoute;
            self.outdoor_detour_active = false;
            self.last_handled_report_revision = report.revision;
            if report.state == GoToState::Lost
                && (self.indoor_route_leg || self.indoor_surface_exit_leg)
                && cx.host.navigation().is_portal_space
            {
                self.pause_for_portal_exit();
                return;
            }
            if !self.indoor_walk
                && report.state == GoToState::Blocked
                && report.blocked_by_object_id != 0
                && let Some(blocked) = cx.host.object(report.blocked_by_object_id)
                && blocked.object_class == ObjectClass::Door
                && !blocked.is_door_open
                && blocked.can_activate()
            {
                // Not the interaction wait: this client ends no
                // activation of a door (`check_door_wait`).
                self.is_navigating = false;
                self.door_wait = Some(DoorWait {
                    object_id: blocked.object_id,
                    name: blocked.name.clone(),
                    started: self.clock,
                });
                cx.host.activate(blocked.object_id);
                return;
            }
            self.is_navigating = false;
            self.indoor_walk = false;
            self.indoor_route_leg = false;
            self.failure_reason = reason.map_or_else(|| state_name(report.state), str::to_owned);
            if detour_exhausted {
                self.failure_reason.push_str(&format!(
                    " after {} outdoor detour attempts.",
                    self.outdoor_detour_attempts
                ));
            }
            cx.host.warn(&format!(
                "GoArrow: Navigation stopped with state {}: {}.",
                state_name(report.state),
                self.failure_reason
            ));
            cx.host.chat_post(&format!(
                "GoArrow: Navigation stopped ({}).",
                state_name(report.state)
            ));
        }
    }

    // Another owner's walk replaced ours: everything stops, but that walk
    // is not ours to stop.
    fn taken_over(&mut self, cx: &mut NavContext<'_, '_>) {
        self.is_navigating = false;
        self.stop_walk(cx.host);
        self.failure_reason = "Another walk took over.".to_owned();
        cx.host
            .chat_post("GoArrow: Navigation stopped: another walk took over.");
    }

    fn pause_for_portal_exit(&mut self) {
        self.walked_into_portal = true;
        self.is_navigating = false;
        self.indoor_walk = false;
        self.indoor_route_leg = false;
        self.paused_for_outdoor_route = true;
        self.active_sequence = 0;
        self.failure_reason = "Waiting for portal transition.".to_owned();
    }

    fn start_surface_exit_interaction(&mut self, cx: &mut NavContext<'_, '_>) {
        self.active_interaction_object_id = self.resolved_indoor_portal_object_id;
        if !self.ensure_portal_destination_known(cx, self.active_interaction_object_id) {
            return;
        }
        self.waiting_for_surface_exit = true;
        self.paused_for_outdoor_route = true;
        self.waiting_for_interaction = true;
        self.failure_reason = "Waiting for surface portal transition.".to_owned();
        let result = cx.host.activate(self.active_interaction_object_id);
        if !result.accepted() {
            self.waiting_for_surface_exit = false;
            self.paused_for_outdoor_route = false;
            self.waiting_for_interaction = false;
            self.failure_reason = format!(
                "Surface Portal activation was not accepted ({:?}).",
                result.status
            );
            cx.host
                .chat_post(&format!("GoArrow: {}", self.failure_reason));
            return;
        }
        self.interaction_started = Some(self.clock);
    }

    fn handle_waypoint_reached(&mut self, cx: &mut NavContext<'_, '_>) {
        if self.current_nav_position.is_none() {
            self.stop_walk(cx.host);
            return;
        }
        self.walking_portal_origin = None;
        self.walking_portal_saw_transit = false;
        cx.destination.advance_step();
        self.leg_index += 1;
        if !has_steps(cx.destination) {
            self.has_arrived = true;
            self.is_navigating = false;
            cx.host.stop_go_to();
            self.active_sequence = 0;
            self.post_arrived(cx);
            return;
        }
        // Go on with the planned route, so a portal or recall step is not
        // lost to a recalculation.
        self.start_current_leg(cx);
    }

    #[allow(clippy::too_many_lines)]
    fn try_start_interaction(&mut self, cx: &mut NavContext<'_, '_>, step: &RouteStep) {
        self.is_navigating = false;
        self.interaction_failed = false;
        let snapshot = cx.host.navigation();
        if step.kind == RouteStepKind::Portal
            && self.outdoor_portal_retry_object_id == 0
            && snapshot.is_available
            && snapshot.position.is_outdoor
            && let Some(entrance) =
                self.find_route_entrance_portal(cx, &step.from, &snapshot, false)
        {
            self.outdoor_portal_retry_object_id = entrance.object_id;
            if snapshot
                .position
                .horizontal_distance_metres(&position_of(&entrance))
                > 8.0
            {
                let status = cx.host.go_to(
                    entrance.object_id,
                    2.5f64.max(cx.settings.arrival_distance) as f32,
                );
                if status == NavigationStatus::Accepted {
                    self.is_navigating = true;
                    self.active_sequence = cx.host.go_to_report().sequence;
                    self.last_handled_report_revision = 0;
                    self.failure_reason.clear();
                    return;
                }
                self.outdoor_portal_retry_object_id = 0;
                self.failure_reason = format!("Portal approach was {}.", status_name(status));
            }
        }
        self.interaction_origin_position = snapshot.is_available.then(|| snapshot.position.clone());
        self.interaction_saw_portal_space = false;
        self.active_interaction_object_id = if step.object_id != 0 {
            step.object_id
        } else if step.kind == RouteStepKind::Portal && self.outdoor_portal_retry_object_id != 0 {
            self.outdoor_portal_retry_object_id
        } else if step.kind == RouteStepKind::Portal {
            self.resolved_indoor_portal_object_id
        } else {
            0
        };
        if self.active_interaction_object_id == 0 {
            let portal_step = step.kind == RouteStepKind::Portal;
            let mut candidates: Vec<(u32, f64, bool)> = self
                .objects(cx.host)
                .iter()
                // The object list holds the character's own things too: what a
                // step uses stands in the world.
                .filter(|o| {
                    o.position.is_some()
                        && !o.is_owned
                        && o.can_activate()
                        && if portal_step {
                            has_portal_capability(o)
                        } else {
                            o.capabilities & openac_plugin::world::capabilities::INTERACTABLE != 0
                        }
                })
                .map(|o| {
                    let distance = o.position.as_ref().map_or(f64::INFINITY, |p| {
                        coords_of(p).distance_to(step.from.coords)
                    });
                    let name_matches = !step.via.trim().is_empty()
                        && if portal_step {
                            portal_matches_destination(o, &portal_destination_name(&step.via))
                        } else {
                            text::contains_ignore_case(&o.name, &step.via)
                        };
                    (o.object_id, distance, name_matches)
                })
                .filter(|(_, distance, name_matches)| {
                    if *name_matches {
                        *distance <= 1.0
                    } else {
                        portal_step && *distance <= 0.1
                    }
                })
                .collect();
            candidates.sort_by(|a, b| {
                u8::from(!a.2)
                    .cmp(&u8::from(!b.2))
                    .then(a.1.total_cmp(&b.1))
            });
            self.active_interaction_object_id = candidates.first().map_or(0, |c| c.0);
        }

        if self.active_interaction_object_id == 0 {
            if self.should_explore_for_portal(cx, &snapshot, step) {
                self.explore_for_portal(cx, &snapshot, step);
                return;
            }
            self.waiting_for_interaction = true;
            if step.kind == RouteStepKind::Portal {
                let mut nearby: Vec<WorldObject> = self
                    .objects(cx.host)
                    .iter()
                    .filter(|o| o.position.is_some() && is_portal(o))
                    .cloned()
                    .collect();
                nearby.sort_by(|a, b| {
                    snapshot
                        .position
                        .horizontal_distance_metres(&position_of(a))
                        .total_cmp(
                            &snapshot
                                .position
                                .horizontal_distance_metres(&position_of(b)),
                        )
                });
                let nearby: Vec<String> = nearby
                    .iter()
                    .take(6)
                    .map(|o| {
                        format!(
                            "'{}' {} m away, destination='{}', activate={}",
                            o.name,
                            text::format_fixed(
                                snapshot
                                    .position
                                    .horizontal_distance_metres(&position_of(o)),
                                0,
                                0
                            ),
                            o.portal_destination.as_deref().unwrap_or("unknown"),
                            tf(o.can_activate())
                        )
                    })
                    .collect();
                self.failure_reason = format!(
                    "Could not identify portal '{}'. Nearby portals: {}.",
                    step.via,
                    if nearby.is_empty() {
                        "none".to_owned()
                    } else {
                        nearby.join("; ")
                    }
                );
            }
            cx.host.chat_post(&format!(
                "GoArrow: Could not identify {} '{}'. Complete it manually, then use /go resume.",
                kind_word(step.kind),
                esc(&step.via)
            ));
            return;
        }

        if !self.ensure_portal_destination_known(cx, self.active_interaction_object_id) {
            return;
        }
        self.waiting_for_interaction = true;
        let result = cx.host.activate(self.active_interaction_object_id);
        if !result.accepted() {
            self.waiting_for_interaction = false;
            self.interaction_failed = true;
            self.failure_reason = format!(
                "Interaction with '{}' was not accepted ({:?}).",
                step.via, result.status
            );
            cx.host.chat_post(&format!(
                "GoArrow: {} Use /go resume to retry.",
                esc(&self.failure_reason)
            ));
            return;
        }
        self.interaction_started = Some(self.clock);
        self.failure_reason = "Waiting for portal transition.".to_owned();
        cx.host
            .chat_post(&format!("GoArrow: Activating '{}'.", esc(&step.via)));
    }

    fn on_activation_completed(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        completion: &ActivationCompletion,
    ) {
        if !self.waiting_for_interaction
            || completion.object_id != self.active_interaction_object_id
            || completion.revision == 0
            || completion.revision <= self.last_activation_revision
        {
            return;
        }
        self.last_activation_revision = completion.revision;
        let success =
            completion.outcome == ActivationOutcome::Completed && completion.weenie_error == 0;
        if self.waiting_for_surface_exit {
            if !success {
                self.fail_interaction(cx, &format!("{:?}", completion.outcome));
            }
            return;
        }
        if !success {
            self.fail_interaction(cx, &format!("{:?}", completion.outcome));
            return;
        }
        let Some(first) = first_step(cx.destination) else {
            return;
        };
        if first.kind == RouteStepKind::Travel {
            self.waiting_for_interaction = false;
            self.active_interaction_object_id = 0;
            self.start_current_leg(cx);
        }
        // A portal or recall activation only says the interaction worked;
        // the route moves on when the transition completes.
    }

    // The portal, recall or surface exit waited for did not take the
    // character anywhere (`outcome`: why, as the failure line shows it).
    fn fail_interaction(&mut self, cx: &mut NavContext<'_, '_>, outcome: &str) {
        self.interaction_started = None;
        if self.waiting_for_surface_exit {
            self.waiting_for_surface_exit = false;
            self.paused_for_outdoor_route = false;
            self.waiting_for_interaction = false;
            self.failure_reason = format!("Surface Portal activation failed ({outcome}).");
            cx.host
                .chat_post(&format!("GoArrow: {}", self.failure_reason));
            return;
        }
        self.waiting_for_interaction = false;
        self.interaction_failed = true;
        let target = first_step(cx.destination).map_or_else(|| "portal".to_owned(), |s| s.via);
        self.failure_reason = format!("Interaction with '{target}' failed ({outcome}).");
        cx.host.warn(&format!(
            "GoArrow: Interaction failed ({outcome}) for object 0x{:08X}.",
            self.active_interaction_object_id
        ));
        cx.host.chat_post(&format!(
            "GoArrow: {} Use /go resume to retry.",
            esc(&self.failure_reason)
        ));
    }

    // A portal or recall use that the server accepted but whose
    // transition has not started after the timeout failed; true when it
    // did.
    fn check_interaction_timeout(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        let Some(started) = self.interaction_started else {
            return false;
        };
        if !self.waiting_for_interaction || cx.host.navigation().is_portal_space {
            self.interaction_started = None;
            return false;
        }
        if self.clock - started < INTERACTION_TIMEOUT_SECONDS {
            return false;
        }
        self.fail_interaction(cx, &format!("{:?}", ActivationOutcome::TimedOut));
        true
    }

    // The server answered a use: a refused portal or recall fails its
    // step (a refusal brings no transition), the door's opens it (or says
    // why not). True when that changed anything.
    fn on_item_use_completed(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        completion: &ItemUseCompletion,
    ) -> bool {
        if completion.revision == 0 || completion.revision <= self.last_item_use_revision {
            return false;
        }
        self.last_item_use_revision = completion.revision;
        if self.waiting_for_interaction
            && self.active_interaction_object_id != 0
            && completion.source_id == self.active_interaction_object_id
            && completion.weenie_error != 0
        {
            self.fail_interaction(cx, &use_error(completion.weenie_error));
            return true;
        }
        let Some(wait) = self.door_wait.clone() else {
            return false;
        };
        if completion.source_id != wait.object_id {
            return false;
        }
        if completion.weenie_error == 0 {
            self.start_current_leg(cx);
            return true;
        }
        self.door_wait = None;
        self.interaction_failed = true;
        self.failure_reason = format!(
            "Interaction with '{}' failed ({}).",
            wait.name,
            use_error(completion.weenie_error)
        );
        cx.host.warn(&format!(
            "GoArrow: Interaction failed ({}) for object 0x{:08X}.",
            use_error(completion.weenie_error),
            wait.object_id
        ));
        cx.host.chat_post(&format!(
            "GoArrow: {} Use /go resume to retry.",
            esc(&self.failure_reason)
        ));
        true
    }

    // A tick of the wait for a door: true when the walk went on or stopped.
    fn check_door_wait(&mut self, cx: &mut NavContext<'_, '_>) -> bool {
        let Some(wait) = self.door_wait.clone() else {
            return false;
        };
        if cx
            .host
            .object(wait.object_id)
            .is_some_and(|door| door.is_door_open)
        {
            self.start_current_leg(cx);
            return true;
        }
        if self.clock - wait.started < INTERACTION_TIMEOUT_SECONDS {
            return false;
        }
        self.door_wait = None;
        self.interaction_failed = true;
        self.failure_reason = format!("Door '{}' did not open.", wait.name);
        cx.host.chat_post(&format!(
            "GoArrow: {} Use /go resume to retry.",
            esc(&self.failure_reason)
        ));
        true
    }

    // True when the change concerned the navigator (the view is rebuilt).
    fn on_object_changed(&mut self, cx: &mut NavContext<'_, '_>, change: &ObjectChange) -> bool {
        if let Some(wait) = &self.door_wait
            && change.object_id == wait.object_id
            && change
                .current
                .as_ref()
                .is_some_and(|door| door.is_door_open)
        {
            self.start_current_leg(cx);
            return true;
        }
        if change.kind == ObjectChangeKind::Released {
            self.identification_attempts.remove(&change.object_id);
            if change.object_id == self.pending_identification {
                self.pending_identification = 0;
                self.pending_identification_started = 0.0;
            }
        }
        if change.kind == ObjectChangeKind::IdentReceived
            && change.object_id == self.pending_identification
        {
            self.pending_identification = 0;
            self.pending_identification_started = 0.0;
            if !self.go_requested {
                return true;
            }
            if self.paused_for_outdoor_route && has_steps(cx.destination) {
                self.start_current_leg(cx);
            } else if !self.is_navigating
                && !self.waiting_for_interaction
                && cx.destination.has_destination()
            {
                self.start_navigation(cx);
            }
            return true;
        }
        if self.paused_for_outdoor_route
            && is_indoor_portal_step(cx.destination)
            && change.kind != ObjectChangeKind::Released
            && let Some(portal) = &change.current
            && let Some(position) = &portal.position
            && portal.can_activate()
            && has_portal_capability(portal)
        {
            let snapshot = cx.host.navigation();
            if is_indoors(&snapshot)
                && are_nearby_indoor_landblocks(snapshot.position.cell_id, position.cell_id)
                && let Some(first) = first_step(cx.destination)
                && portal_matches_destination(
                    portal,
                    &portal_destination_name(if first.kind == RouteStepKind::Travel {
                        &first.to.name
                    } else {
                        &first.via
                    }),
                )
            {
                self.start_current_leg(cx);
                return true;
            }
        }
        if cx.destination.kind() != DestinationKind::Object
            || cx.destination.target_object_id() != Some(change.object_id)
        {
            return false;
        }
        match (&change.kind, &change.current) {
            (ObjectChangeKind::Released, _) | (_, None) => cx.destination.mark_object_unavailable(),
            (_, Some(current)) => {
                cx.destination.update_object(
                    current.object_id,
                    &current.name,
                    current.position.as_ref().map(to_nav),
                );
            }
        }
        true
    }

    fn try_complete_walking_portal(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
    ) -> bool {
        let Some(origin) = self.walking_portal_origin.clone() else {
            return false;
        };
        let all = steps(cx.destination);
        if !self.walking_portal_saw_transit
            || !is_in_world(snapshot)
            || all.len() <= 1
            || all[0].kind != RouteStepKind::Travel
            || all[1].kind != RouteStepKind::Portal
        {
            return false;
        }
        let here = &snapshot.position;
        let changed_world = here.is_outdoor != origin.is_outdoor
            || (origin.cell_id != 0
                && here.cell_id != 0
                && (origin.cell_id >> 16) != (here.cell_id >> 16));
        if !changed_world && here.horizontal_distance_metres(&origin) <= 20.0 {
            return false;
        }
        // Walking into a portal can teleport before the walk reports
        // arrival: complete the approach and the crossing together,
        // without activating again.
        let portal = all[1].clone();
        self.walking_portal_origin = None;
        self.walked_into_portal = false;
        self.note_taken(&portal);
        self.walking_portal_saw_transit = false;
        cx.destination.advance_step();
        cx.destination.advance_step();
        self.leg_index += 2;
        self.is_navigating = false;
        self.paused_for_outdoor_route = false;
        self.waiting_for_interaction = false;
        self.active_sequence = 0;
        self.outdoor_portal_retry_object_id = 0;
        self.resolved_indoor_portal_object_id = 0;
        self.resolved_indoor_portal_position = None;
        self.table_exit = None;
        self.landscape_walk_position = None;
        self.failure_reason.clear();
        if self.replan_after_wrong_arrival(cx, &portal) {
            return true;
        }
        if has_steps(cx.destination) {
            self.start_current_leg(cx);
        } else {
            self.has_arrived = true;
        }
        true
    }

    /// Atlas arrivals can be wrong. The rest of the route walks on from the
    /// listed arrival, and every re-plan from where the character really
    /// is would choose the same portal again, so a portal that lands far
    /// from its listed arrival is avoided until the plugin is loaded again.
    /// One that lands a little off (more than [`LEARNED_ARRIVAL_DISTANCE`])
    /// arrives there for the rest of the session instead, and the route is
    /// planned again. In portal space the check waits until the character
    /// is seen in the world.
    fn replan_after_wrong_arrival(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        step: &RouteStep,
    ) -> bool {
        if step.kind != RouteStepKind::Portal || !step.to.has_coordinates() {
            return false;
        }
        let snapshot = cx.host.navigation();
        if !is_in_world(&snapshot) {
            self.arrival_check = Some(step.clone());
            return false;
        }
        // Indoor positions have no reliable map coordinates to compare.
        if !snapshot.position.is_outdoor {
            return false;
        }
        let landed = coords_of(&snapshot.position);
        let off = landed.distance_to(step.to.coords);
        if off <= LEARNED_ARRIVAL_DISTANCE {
            return false;
        }
        if off <= WRONG_ARRIVAL_DISTANCE {
            return self.learn_arrival(cx, step, landed);
        }
        cx.destination
            .avoid_step_for_session(step, &format!("Arrived at {landed}"), cx.finder);
        // Only an Atlas portal has an id an arrival correction can name.
        self.pending_arrival =
            (step.graph_from.id > 0 && step.graph_from.has_exit_coords()).then(|| PendingArrival {
                id: step.graph_from.id,
                name: step.graph_from.name.clone(),
                arrival: landed.round(1).to_string(),
                note: format!("Observed arrival; Atlas lists {}.", step.to.coords),
            });
        let message = format!(
            "GoArrow: '{}' arrived at {landed}, not {}. Avoiding it until the plugin is reloaded and recalculating. {}",
            esc(&step.via),
            step.to.coords,
            if self.pending_arrival.is_some() {
                "If this was the right portal, run /go corrections save to keep this arrival."
            } else {
                "Use /go corrections to fix it permanently."
            }
        );
        cx.host.warn(&message);
        cx.host.chat_post(&message);
        self.is_navigating = false;
        self.paused_for_outdoor_route = false;
        cx.destination.clear_route();
        if self.plan(cx) {
            self.failure_reason.clear();
            self.start_current_leg(cx);
        } else if self.go_requested {
            self.failure_reason =
                format!("No route avoids '{}', whose arrival is wrong.", step.via);
        }
        true
    }

    /// An Atlas portal landed `landed`, a little off its listed arrival:
    /// that is its arrival until the plugin is loaded again (kept with
    /// `/go corrections save`), and the route is planned again from here.
    /// False for a portal without an Atlas id.
    fn learn_arrival(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        step: &RouteStep,
        landed: Coordinates,
    ) -> bool {
        let portal = &step.graph_from;
        if portal.id <= 0 || !portal.has_exit_coords() {
            return false;
        }
        cx.finder
            .database_mut()
            .set_session_arrival(portal.id, landed);
        self.pending_arrival = Some(PendingArrival {
            id: portal.id,
            name: portal.name.clone(),
            arrival: landed.round(1).to_string(),
            note: format!("Observed arrival; the route had {}.", step.to.coords),
        });
        let message = format!(
            "GoArrow: '{}' arrived at {landed}, {} map units from {}. Using that arrival until the plugin is reloaded and recalculating. If this was the right portal, run /go corrections save to keep it.",
            esc(&step.via),
            text::format_fixed(landed.distance_to(step.to.coords), 1, 1),
            step.to.coords
        );
        cx.host.info(&message);
        cx.host.chat_post(&message);
        self.is_navigating = false;
        self.paused_for_outdoor_route = false;
        cx.destination.clear_route();
        if self.plan(cx) {
            self.failure_reason.clear();
            self.start_current_leg(cx);
        } else if self.go_requested {
            self.failure_reason = format!("No route from where '{}' arrived.", step.via);
        }
        true
    }

    /// The first portal or recall of the route already taken in this
    /// navigation.
    fn repeated_interaction(&self, destination: &Destination) -> Option<String> {
        steps(destination)
            .iter()
            .find(|s| interaction_key(s).is_some_and(|key| self.taken.contains(&key)))
            .map(|s| s.via.clone())
    }

    /// Records a portal or recall step as taken in this navigation.
    fn note_taken(&mut self, step: &RouteStep) {
        if let Some(key) = interaction_key(step)
            && !self.taken.contains(&key)
        {
            self.taken.push(key);
        }
    }

    fn stop_for_repeat(&mut self, cx: &mut NavContext<'_, '_>, via: &str) {
        self.stop_walk(cx.host);
        self.failure_reason = format!("Navigation stopped: the route would take '{via}' again.");
        cx.host.warn(&format!("GoArrow: {}", self.failure_reason));
        cx.host
            .chat_post(&format!("GoArrow: {}", esc(&self.failure_reason)));
    }

    fn try_complete_known_indoor_arrival(
        &mut self,
        cx: &mut NavContext<'_, '_>,
        snapshot: &NavigationSnapshot,
    ) -> bool {
        if self.route_id == 0
            || !is_indoors(snapshot)
            || landblock(snapshot.position.cell_id) != FACILITY_HUB_LANDBLOCK
        {
            return false;
        }
        let all = steps(cx.destination);
        if all.is_empty() {
            return false;
        }
        let completed = if all[0].kind == RouteStepKind::Portal {
            1
        } else if all.len() > 1
            && all[0].kind == RouteStepKind::Travel
            && all[1].kind == RouteStepKind::Portal
        {
            2
        } else {
            0
        };
        if completed == 0
            || !text::eq_ignore_case(
                &portal_destination_name(&all[completed - 1].via),
                "Facility Hub",
            )
        {
            return false;
        }
        // The live dungeon cell proves the Facility Hub crossing happened,
        // even when portal space and the completion were both missed.
        if self.is_navigating {
            cx.host.stop_go_to();
        }
        self.note_taken(&all[completed - 1].clone());
        for _ in 0..completed {
            cx.destination.advance_step();
        }
        self.leg_index += completed;
        self.is_navigating = false;
        self.indoor_walk = false;
        self.indoor_route_leg = false;
        self.paused_for_outdoor_route = false;
        self.waiting_for_interaction = false;
        self.interaction_failed = false;
        self.active_sequence = 0;
        self.active_interaction_object_id = 0;
        self.resolved_indoor_portal_object_id = 0;
        self.resolved_indoor_portal_position = None;
        self.table_exit = None;
        self.failure_reason.clear();
        cx.host.info(&format!(
            "GoArrow: Recovered the Facility Hub portal crossing from live cell {:08X}.",
            snapshot.position.cell_id
        ));
        if has_steps(cx.destination) {
            self.start_current_leg(cx);
        } else {
            self.has_arrived = true;
        }
        true
    }

    fn on_portal_transition(&mut self, cx: &mut NavContext<'_, '_>, transition: &PortalTransition) {
        const LOGIN: u8 = 1;
        self.traversals.break_trace();
        if transition.kind != LOGIN {
            // The use waited for was not refused.
            self.interaction_started = None;
        }
        if self.walking_portal_origin.is_some()
            && transition.is_completed
            && transition.kind != LOGIN
        {
            self.walking_portal_saw_transit = true;
            let snapshot = cx.host.navigation();
            if self.try_complete_walking_portal(cx, &snapshot) {
                return;
            }
        }
        let Some(first) = first_step(cx.destination) else {
            return;
        };
        if !self.waiting_for_interaction
            || !transition.is_completed
            || transition.kind == LOGIN
            || !matches!(first.kind, RouteStepKind::Portal | RouteStepKind::Recall)
        {
            return;
        }
        if transition.revision > 0 && transition.revision <= self.last_transition_revision {
            return;
        }
        if transition.generation > 0 && transition.generation <= self.last_transition_generation {
            return;
        }
        if transition.revision == 0 && transition.generation == 0 {
            return;
        }
        self.last_transition_revision = transition.revision;
        self.last_transition_generation = transition.generation;
        self.complete_interaction(cx, true);
    }

    fn check_interaction_position(&mut self, cx: &mut NavContext<'_, '_>) {
        let Some(first) = first_step(cx.destination) else {
            return;
        };
        if !self.waiting_for_interaction
            || !matches!(first.kind, RouteStepKind::Portal | RouteStepKind::Recall)
        {
            return;
        }
        let Some(origin) = self.interaction_origin_position.clone() else {
            return;
        };
        let snapshot = cx.host.navigation();
        if !snapshot.is_available {
            return;
        }
        if snapshot.is_portal_space {
            self.interaction_saw_portal_space = true;
            return;
        }
        let current = &snapshot.position;
        let changed_world = current.is_outdoor != origin.is_outdoor
            || (current.cell_id != 0
                && origin.cell_id != 0
                && landblock(current.cell_id) != landblock(origin.cell_id));
        let moved_after_portal_space = self.interaction_saw_portal_space
            && (current.cell_id != origin.cell_id
                || current.horizontal_distance_metres(&origin) > 20.0);
        if !changed_world && !moved_after_portal_space {
            return;
        }
        cx.host.info(&format!(
            "GoArrow: Completed '{}' from the live position after portal transition.",
            first.via
        ));
        self.complete_interaction(cx, true);
    }
}

/// Every tracked object named like the expected portal, whatever its
/// class, so a log shows whether the client lacks it or a check refuses
/// it.
fn named_portal_diagnostic(
    snapshot: &NavigationSnapshot,
    objects: &[WorldObject],
    expected: &str,
) -> String {
    let destination_name = portal_destination_name(expected);
    if destination_name.is_empty() {
        return String::new();
    }
    let here = &snapshot.position;
    let matches: Vec<String> = objects
        .iter()
        .filter(|o| {
            portal_matches_destination(o, &destination_name)
                || text::contains_ignore_case(&o.name, &destination_name)
        })
        .map(|o| {
            let pos = position_of(o);
            let distance = o.position.as_ref().map_or_else(
                || "unknown distance".to_owned(),
                |p| {
                    format!(
                        "{} m",
                        text::format_fixed(here.horizontal_distance_metres(p), 0, 0)
                    )
                },
            );
            format!(
                "'{}' {:08X} class={:?} itemType={:08X} capabilities={} cell={:08X} position={} \
                 outdoor={} {distance} destination='{}' eligible={}",
                o.name,
                o.object_id,
                o.object_class,
                o.item_type,
                capability_names(o.capabilities),
                pos.cell_id,
                tf(o.position.is_some()),
                tf(pos.is_outdoor),
                o.portal_destination.as_deref().unwrap_or("unknown"),
                indoor_portal_rejection(snapshot, o, &destination_name).unwrap_or("yes")
            )
        })
        .collect();
    if matches.is_empty() {
        format!("Objects named '{destination_name}': none tracked by client.")
    } else {
        format!(
            "Objects named '{destination_name}': {}.",
            matches.join("; ")
        )
    }
}

/// A server error code as the failure lines show it (`error 0x0024`).
fn use_error(code: u32) -> String {
    format!("error 0x{code:04X}")
}

/// A share file must be a `.json` name in the storage folder itself.
fn check_share_file(file: &str) -> Result<(), String> {
    if file.is_empty()
        || file.contains('/')
        || file.contains('\\')
        || file == "."
        || file == ".."
        || !text::ends_with_ignore_case(file, ".json")
    {
        return Err("Use a JSON filename in the plugin storage folder.".to_owned());
    }
    Ok(())
}
