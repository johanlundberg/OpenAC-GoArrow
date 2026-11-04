//! A small client for the navigator tests, in the manner of OpenAC's
//! `FakeNavigationAutomation` and GoArrow's test world objects: the SDK's
//! `FakeHost` for the host services (log, chat, window), with navigation,
//! world objects, dungeon data and storage answered from state kept here.
//! A walk request is recorded and starts a new walk report (its sequence
//! one higher, planning), as the C# fake did; Stop marks it stopped.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use ac_plugin_abi::Wire;
use ac_plugin_abi::items::{ItemResult, ItemStatus, ItemUseCompletion};
use ac_plugin_abi::navigation::{
    GoToPlace, GoToReport, GoToState, NavigationPlan, NavigationPlanStatus, NavigationSnapshot,
    NavigationStatus, PathPreviewRequest,
};
use ac_plugin_abi::storage::{StorageKey, StorageWrite};
use ac_plugin_abi::world::{
    ActivationCompletion, ActivationOutcome, DungeonCell, DungeonLayer, DungeonWall, Floorplan,
    IndoorCell, ObjectChange, ObjectChangeKind, ObjectClass, PortalTransition, Position,
    WorldObject, capabilities,
};
use goarrow::database::LocationDatabase;
use goarrow::destination::Destination;
use goarrow::finder::RouteFinder;
use goarrow::navigator::{NavContext, NavSettings, NavView, Navigator};
use goarrow::route::RouteStepKind;
use goarrow::traversals::TraversalStore;
use openac_plugin::testing::FakeHost;
use openac_plugin::{Event, Host, Import, Transport};

fn decode<T: Wire>(bytes: &[u8]) -> T {
    T::from_bytes(bytes).expect("a request the SDK encoded")
}

/// A position as the C# tests write one: cell, east/west, north/south,
/// elevation (map units), outdoors.
pub fn pos(
    cell_id: u32,
    east_west: f64,
    north_south: f64,
    elevation: f64,
    is_outdoor: bool,
) -> Position {
    Position {
        cell_id,
        east_west,
        north_south,
        elevation,
        heading_degrees: 0.0,
        is_outdoor,
    }
}

/// A snapshot in the world, not moving.
pub fn snapshot(position: Position) -> NavigationSnapshot {
    NavigationSnapshot {
        is_available: true,
        is_portal_space: false,
        local_object_id: 1,
        position,
        ..NavigationSnapshot::default()
    }
}

/// A portal object (classified a portal, portal and interactable).
pub fn portal(object_id: u32, name: &str, position: Position) -> WorldObject {
    WorldObject {
        object_id,
        name: name.to_owned(),
        object_class: ObjectClass::Portal,
        capabilities: capabilities::PORTAL | capabilities::INTERACTABLE,
        position: Some(position),
        ..WorldObject::default()
    }
}

/// The same portal with a destination label.
pub fn with_destination(mut object: WorldObject, destination: &str) -> WorldObject {
    object.portal_destination = Some(destination.to_owned());
    object
}

/// A walk report as the C# tests write one.
pub fn report(
    sequence: i64,
    state: GoToState,
    object_id: u32,
    remaining_meters: f32,
    reason: Option<&str>,
    revision: i64,
) -> GoToReport {
    GoToReport {
        sequence,
        state,
        object_id,
        remaining_meters,
        replans: 0,
        reason: reason.map(str::to_owned),
        blocked_by_object_id: 0,
        revision,
        owner: None,
    }
}

/// A finished transition: revision, generation, destination cell and
/// kind (1 login, 2 portal).
pub fn transition(
    revision: i64,
    generation: i64,
    destination_cell: u32,
    kind: u8,
) -> PortalTransition {
    PortalTransition {
        revision,
        generation,
        destination_cell,
        is_ready: true,
        is_materialized: true,
        is_completed: true,
        is_cancelled: false,
        recall_request_revision: 0,
        kind,
    }
}

/// The C# `DungeonPortalSearchTests.Position(x)`: a point in dungeon cell
/// 0x02AA022F, `x` metres east of the landblock-local origin line.
pub fn dungeon_position(x: f32) -> Position {
    pos(
        0x02AA_022F,
        (-125.0 * 192.0 + f64::from(x) - 84.0) / 240.0,
        (43.0 * 192.0 - 84.0) / 240.0,
        0.0,
        false,
    )
}

/// A floorplan cell.
pub fn cell(cell_id: u32, center: (f32, f32, f32), layer_z: f32) -> DungeonCell {
    DungeonCell {
        cell_id,
        center,
        layer_z,
        ..DungeonCell::default()
    }
}

/// A floorplan layer.
pub fn layer(
    z: f32,
    floors: Vec<Vec<(f32, f32)>>,
    walls: Vec<((f32, f32), (f32, f32))>,
) -> DungeonLayer {
    DungeonLayer {
        z,
        floors,
        walls: walls
            .into_iter()
            .map(|(start, end)| DungeonWall { start, end })
            .collect(),
    }
}

/// The client.
#[derive(Debug)]
pub struct Sim {
    pub fake: FakeHost,
    pub snapshot: NavigationSnapshot,
    pub report: GoToReport,
    pub go_to_answer: NavigationStatus,
    pub objects: Vec<WorldObject>,
    pub identified: Vec<u32>,
    pub identify_statuses: HashMap<u32, ItemStatus>,
    pub activated: Vec<u32>,
    pub go_to_calls: Vec<(u32, f32)>,
    pub go_to_place_calls: Vec<(Position, f32)>,
    pub stop_go_to_count: usize,
    pub previews: Vec<PathPreviewRequest>,
    pub cells: Vec<DungeonCell>,
    pub layers: Vec<DungeonLayer>,
    pub indoor_cells: Vec<IndoorCell>,
    pub sealed: bool,
    pub storage: BTreeMap<String, String>,
    /// How often the whole object table was read.
    pub object_table_reads: usize,
}

impl Sim {
    pub fn new() -> Self {
        Self {
            fake: FakeHost::new("openac.goarrow"),
            snapshot: NavigationSnapshot::default(),
            report: GoToReport::default(),
            go_to_answer: NavigationStatus::Accepted,
            objects: Vec::new(),
            identified: Vec::new(),
            identify_statuses: HashMap::new(),
            activated: Vec::new(),
            go_to_calls: Vec::new(),
            go_to_place_calls: Vec::new(),
            stop_go_to_count: 0,
            previews: Vec::new(),
            cells: Vec::new(),
            layers: Vec::new(),
            indoor_cells: Vec::new(),
            sealed: false,
            storage: BTreeMap::new(),
            object_table_reads: 0,
        }
    }

    /// Moves the character (same snapshot otherwise).
    pub fn move_to(&mut self, position: Position) {
        self.snapshot.position = position;
    }

    /// Replaces an object by id.
    pub fn set_object(&mut self, object: WorldObject) {
        match self
            .objects
            .iter_mut()
            .find(|o| o.object_id == object.object_id)
        {
            Some(existing) => *existing = object,
            None => self.objects.push(object),
        }
    }

    /// Sets an object's destination label.
    pub fn set_destination(&mut self, object_id: u32, destination: &str) {
        let object = self
            .objects
            .iter_mut()
            .find(|o| o.object_id == object_id)
            .expect("an object");
        object.portal_destination = Some(destination.to_owned());
    }

    fn started_walk(&mut self) -> NavigationStatus {
        if self.go_to_answer == NavigationStatus::Accepted {
            self.report.sequence += 1;
            self.report.state = GoToState::Planning;
        }
        self.go_to_answer
    }

    /// The chat lines posted.
    pub fn posted(&self) -> Vec<String> {
        self.fake.posted.iter().map(|p| p.text.clone()).collect()
    }
}

impl Transport for Sim {
    fn call(&mut self, import: Import, bytes: &[u8]) -> Vec<u8> {
        let base = self.fake.call(import, bytes);
        match import {
            Import::NavigationSnapshot => self.snapshot.to_bytes(),
            Import::NavigationGoToReport => self.report.to_bytes(),
            Import::NavigationGoTo => {
                let (id, arrival): (u32, f32) = decode(bytes);
                self.go_to_calls.push((id, arrival));
                self.started_walk().to_bytes()
            }
            Import::NavigationGoToPlace => {
                let request: GoToPlace = decode(bytes);
                self.go_to_place_calls
                    .push((request.position, request.arrival_meters));
                self.started_walk().to_bytes()
            }
            Import::NavigationStopGoTo => {
                self.stop_go_to_count += 1;
                self.report.state = GoToState::Stopped;
                self.report.reason = Some("stopped".to_owned());
                NavigationStatus::Accepted.to_bytes()
            }
            Import::NavigationPreviewPath => {
                let request: PathPreviewRequest = decode(bytes);
                self.previews.push(request);
                (self.previews.len() as u32).to_bytes()
            }
            Import::WorldObjects => {
                self.object_table_reads += 1;
                self.objects.to_bytes()
            }
            Import::WorldObject => {
                let id: u32 = decode(bytes);
                self.objects
                    .iter()
                    .find(|o| o.object_id == id)
                    .cloned()
                    .to_bytes()
            }
            Import::WorldIdentify => {
                let id: u32 = decode(bytes);
                self.identified.push(id);
                ItemResult::of(
                    self.identify_statuses
                        .get(&id)
                        .copied()
                        .unwrap_or(ItemStatus::Started),
                )
                .to_bytes()
            }
            Import::WorldActivate => {
                let id: u32 = decode(bytes);
                self.activated.push(id);
                ItemResult::of(ItemStatus::Started).to_bytes()
            }
            Import::DungeonFloorplan => {
                let landblock_id: u32 = decode(bytes);
                Some(Floorplan {
                    landblock_id,
                    layers: self.layers.clone(),
                    cells: self.cells.clone(),
                    bounds_min: (0.0, 0.0, 0.0),
                    bounds_max: (1.0, 1.0, 1.0),
                })
                .to_bytes()
            }
            Import::DungeonIndoorCells => self.indoor_cells.to_bytes(),
            Import::DungeonIsSealed => self.sealed.to_bytes(),
            Import::StorageReadText => {
                let key: StorageKey = decode(bytes);
                Ok::<Option<String>, String>(self.storage.get(&key.key).cloned()).to_bytes()
            }
            Import::StorageWrite => {
                let write: StorageWrite = decode(bytes);
                self.storage
                    .insert(write.key, String::from_utf8(write.bytes).expect("text"));
                Ok::<(), String>(()).to_bytes()
            }
            Import::StorageDescribe => Some("C:/plugin-data".to_owned()).to_bytes(),
            _ => base,
        }
    }
}

/// The navigator over the simulated client, with its route state.
#[derive(Debug)]
pub struct Harness {
    pub sim: Sim,
    pub nav: Navigator,
    pub destination: Destination,
    pub finder: RouteFinder,
    pub settings: NavSettings,
}

impl Harness {
    /// A navigator over places from location XML, Auto-Navigate on.
    pub fn new(xml: &str) -> Self {
        let mut database = LocationDatabase::new();
        database.load_locations_xml(xml).expect("location XML");
        Self {
            sim: Sim::new(),
            nav: Navigator::new(),
            destination: Destination::new(),
            finder: RouteFinder::new(database),
            settings: NavSettings {
                auto_navigate: true,
                ..NavSettings::default()
            },
        }
    }

    /// With no places.
    pub fn empty() -> Self {
        Self::new("<locations />")
    }

    /// Runs `f` with the navigator and a context, as the plugin would.
    pub fn with<R>(&mut self, f: impl FnOnce(&mut Navigator, &mut NavContext<'_, '_>) -> R) -> R {
        let mut host = Host::with_transport(&mut self.sim);
        let mut cx = NavContext {
            host: &mut host,
            destination: &mut self.destination,
            finder: &mut self.finder,
            settings: &self.settings,
        };
        f(&mut self.nav, &mut cx)
    }

    /// Runs `f` with a bare host.
    pub fn host<R>(&mut self, f: impl FnOnce(&mut Navigator, &mut Host<'_>) -> R) -> R {
        let mut host = Host::with_transport(&mut self.sim);
        f(&mut self.nav, &mut host)
    }

    /// Sets a named destination.
    pub fn go_to_named(&mut self, name: &str) {
        assert!(
            self.destination
                .set_destination_by_name(self.finder.database(), name),
            "no place named {name}"
        );
    }

    /// Sets an object destination.
    pub fn go_to_object(&mut self, object: &WorldObject) {
        let position = object
            .position
            .clone()
            .map(|p| goarrow::coords::NavPosition {
                cell_id: p.cell_id,
                east_west: p.east_west,
                north_south: p.north_south,
                elevation: p.elevation,
                heading_degrees: p.heading_degrees,
                is_outdoor: p.is_outdoor,
            });
        assert!(
            self.destination
                .set_object(object.object_id, &object.name, position)
        );
    }

    pub fn enable(&mut self) {
        self.host(|nav, host| nav.enable(host));
    }

    pub fn start(&mut self) {
        self.with(|nav, cx| nav.start(cx));
    }

    pub fn stop(&mut self) {
        self.with(|nav, cx| nav.stop(cx));
    }

    pub fn resume(&mut self) {
        self.with(|nav, cx| nav.resume(cx));
    }

    pub fn plan(&mut self) -> bool {
        self.with(|nav, cx| nav.plan_route(cx))
    }

    pub fn tick(&mut self, elapsed: f64) {
        self.with(|nav, cx| nav.tick(cx, elapsed));
    }

    pub fn event(&mut self, event: Event) {
        self.with(|nav, cx| nav.on_event(cx, &event));
    }

    /// The client reports on a walk.
    pub fn report(
        &mut self,
        sequence: i64,
        state: GoToState,
        object_id: u32,
        remaining_meters: f32,
        reason: Option<&str>,
        revision: i64,
    ) {
        self.event(Event::NavigationChanged(report(
            sequence,
            state,
            object_id,
            remaining_meters,
            reason,
            revision,
        )));
    }

    /// The current walk's sequence.
    pub fn sequence(&self) -> i64 {
        self.sim.report.sequence
    }

    /// A portal transition completed.
    pub fn transition(&mut self, revision: i64, generation: i64, destination_cell: u32, kind: u8) {
        self.event(Event::PortalTransition(transition(
            revision,
            generation,
            destination_cell,
            kind,
        )));
    }

    /// An appraisal arrived for an object.
    pub fn ident_received(&mut self, object_id: u32) {
        let current = self
            .sim
            .objects
            .iter()
            .find(|o| o.object_id == object_id)
            .cloned();
        self.event(Event::ObjectChanged(ObjectChange {
            object_id,
            kind: ObjectChangeKind::IdentReceived,
            revision: 1,
            changed_fields: 0,
            current,
        }));
    }

    /// An object appeared.
    pub fn created(&mut self, object: WorldObject) {
        self.event(Event::ObjectChanged(ObjectChange {
            object_id: object.object_id,
            kind: ObjectChangeKind::Created,
            revision: 1,
            changed_fields: 0,
            current: Some(object),
        }));
    }

    /// An activation ended.
    pub fn activation(&mut self, revision: i64, object_id: u32, outcome: ActivationOutcome) {
        self.event(Event::ActivationCompleted(ActivationCompletion {
            revision,
            object_id,
            outcome,
            weenie_error: 0,
        }));
    }

    /// A use the client awaited was answered (the server's error code, 0
    /// for none).
    pub fn item_use(&mut self, revision: i64, source_id: u32, weenie_error: u32) {
        self.event(Event::ItemUseCompleted(ItemUseCompletion {
            revision,
            source_id,
            target_id: 0,
            weenie_error,
        }));
    }

    /// An object's description changed.
    pub fn updated(&mut self, object: WorldObject) {
        self.event(Event::ObjectChanged(ObjectChange {
            object_id: object.object_id,
            kind: ObjectChangeKind::Updated,
            revision: 1,
            changed_fields: 0,
            current: Some(object),
        }));
    }

    /// The client answers a preview.
    pub fn plan_answer(
        &mut self,
        handle: u32,
        status: NavigationPlanStatus,
        path: Vec<Position>,
        length: f32,
    ) {
        self.event(Event::NavigationPlan(NavigationPlan {
            handle,
            status,
            path,
            reason: String::new(),
            length_meters: length,
            ..NavigationPlan::default()
        }));
    }

    pub fn view(&self) -> &NavView {
        self.nav.view()
    }

    pub fn failure(&self) -> String {
        self.nav.view().failure_reason.clone().unwrap_or_default()
    }

    pub fn is_navigating(&self) -> bool {
        self.nav.view().is_navigating
    }

    pub fn first_kind(&self) -> Option<RouteStepKind> {
        self.destination
            .current_route()
            .and_then(|r| r.steps().first())
            .map(|s| s.kind)
    }

    pub fn step_count(&self) -> usize {
        self.destination
            .current_route()
            .map_or(0, |r| r.step_count())
    }

    pub fn vias(&self) -> Vec<String> {
        self.destination
            .current_route()
            .map(|r| r.steps().iter().map(|s| s.via.clone()).collect())
            .unwrap_or_default()
    }

    pub fn go_to_objects(&self) -> Vec<u32> {
        self.sim.go_to_calls.iter().map(|c| c.0).collect()
    }
}

/// A dungeon search over the simulated client, with no learned paths.
pub fn search_next(
    sim: &mut Sim,
    search: &mut goarrow::dungeon_search::DungeonPortalSearch,
    store: &TraversalStore,
) -> Option<Position> {
    let mut host = Host::with_transport(sim);
    let objects = host.objects();
    search.try_get_next_target(&mut host, &objects, store, 0.0)
}
