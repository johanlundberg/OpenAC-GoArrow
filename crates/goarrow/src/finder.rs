//! The route finder (`RouteFinding/RouteFinder.cs`): owns the location
//! database and the graph built from it (rebuilt after any change to the
//! data), finds the best route from an exact position to a place or a
//! point under a cost profile, and lists distinct alternatives.
//!
//! A route starts at the exact origin: the first step walks from it to the
//! node the search entered the graph at ("Walk to start"), or, when the
//! origin is on that node, the first graph step starts at the origin. A
//! destination off the graph ends with a walk to it.
//!
//! Alternatives differ in their interactions (the sequence of portals and
//! recalls); routes that differ only in walks count as one. They come in
//! increasing cost, at most `maximum` of them from at most 64 candidates.
//! [`RouteJob`] and [`AlternativesJob`] do the same work in bounded steps.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet, VecDeque};
use std::rc::Rc;

use crate::corrections::BlockedStep;
use crate::database::LocationDatabase;
use crate::graph::{
    CostSelector, EdgeRef, GraphBuild, InteractionSearch, NodeRef, PathEdge, PathSearch,
    RouteEdgeKind, RouteGraph, SearchTarget,
};
use crate::location::Location;
use crate::route::{Route, RouteStep, RouteStepKind};
use crate::text;

/// How route searches weigh edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RouteCostProfile {
    /// The edges' own costs: walked distance, portals 0.1, recalls 0.05.
    #[default]
    ShortestWalk,
    /// Every walk 1, every interaction 0.1.
    FewestInteractions,
    /// Recalls and lifestones at a tenth of their cost.
    PreferRecall,
    /// Interactions at a thousand times their cost.
    AvoidInteractions,
}

impl RouteCostProfile {
    /// The profile's number in the settings file (0 to 3).
    #[must_use]
    pub fn index(self) -> i32 {
        match self {
            Self::ShortestWalk => 0,
            Self::FewestInteractions => 1,
            Self::PreferRecall => 2,
            Self::AvoidInteractions => 3,
        }
    }

    /// The profile with a settings number.
    #[must_use]
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::ShortestWalk),
            1 => Some(Self::FewestInteractions),
            2 => Some(Self::PreferRecall),
            3 => Some(Self::AvoidInteractions),
            _ => None,
        }
    }
}

/// The largest number of alternatives the route window offers.
pub const MAX_ALTERNATIVES: usize = 8;

/// The largest number of candidate routes an alternatives search looks at.
pub const MAX_ALTERNATIVE_EXPANSIONS: usize = 64;

/// The finder.
#[derive(Debug)]
pub struct RouteFinder {
    database: LocationDatabase,
    max_walk_distance: f64,
    graph: Option<Rc<RouteGraph>>,
    building: Option<GraphBuild>,
}

impl RouteFinder {
    /// The walking range between graph nodes, map units (2.4 km).
    pub const DEFAULT_WALK_DISTANCE: f64 = 10.0;

    /// A finder over a database, with the default walking range.
    #[must_use]
    pub fn new(database: LocationDatabase) -> Self {
        Self::with_walk_distance(database, Self::DEFAULT_WALK_DISTANCE)
    }

    /// A finder with another walking range.
    #[must_use]
    pub fn with_walk_distance(database: LocationDatabase, max_walk_distance: f64) -> Self {
        Self {
            database,
            max_walk_distance,
            graph: None,
            building: None,
        }
    }

    /// The walking range, map units.
    #[must_use]
    pub fn max_walk_distance(&self) -> f64 {
        self.max_walk_distance
    }

    /// The database.
    #[must_use]
    pub fn database(&self) -> &LocationDatabase {
        &self.database
    }

    /// The database, to change it; the graph is rebuilt before the next
    /// search.
    pub fn database_mut(&mut self) -> &mut LocationDatabase {
        self.invalidate_graph();
        &mut self.database
    }

    /// Discards the graph (and a build under way); the next search or
    /// [`RouteFinder::step_graph`] builds it again.
    pub fn invalidate_graph(&mut self) {
        self.graph = None;
        self.building = None;
    }

    /// Whether the graph is built.
    #[must_use]
    pub fn is_graph_built(&self) -> bool {
        self.graph.is_some()
    }

    /// Builds the graph a little: at most `max_ops` units of
    /// [`GraphBuild`] work. True once it is built.
    pub fn step_graph(&mut self, max_ops: usize) -> bool {
        if self.graph.is_some() {
            return true;
        }
        let reach = self.max_walk_distance;
        let build = self.building.get_or_insert_with(|| GraphBuild::new(reach));
        let mut budget = max_ops;
        match build.step(&self.database, &mut budget) {
            Some(graph) => {
                self.graph = Some(Rc::new(graph));
                self.building = None;
                true
            }
            None => false,
        }
    }

    /// The graph, built now (to the end) if needed.
    pub fn graph(&mut self) -> Rc<RouteGraph> {
        while !self.step_graph(usize::MAX) {}
        self.graph.clone().unwrap_or_default()
    }

    /// The route to a named place (the first of that name), shortest walk.
    /// An unknown name gives an empty route saying so.
    pub fn find_route_to_name(&mut self, origin: &Rc<Location>, destination_name: &str) -> Route {
        match self.database.find_location(destination_name) {
            Some(destination) => {
                self.find_route(origin, &destination, RouteCostProfile::ShortestWalk)
            }
            None => {
                let mut route = Route::new(destination_name);
                route.description = format!("Destination '{destination_name}' not found.");
                route
            }
        }
    }

    /// The best route from an exact position to a place or a point. With
    /// no graph route it is one direct walk (`Direct walk (no graph route
    /// found)`); a destination without coordinates gives an empty route.
    pub fn find_route(
        &mut self,
        origin: &Rc<Location>,
        destination: &Rc<Location>,
        profile: RouteCostProfile,
    ) -> Route {
        self.route_job(origin, destination, profile).run()
    }

    /// [`RouteFinder::find_route`] as a job that runs in bounded steps.
    pub fn route_job(
        &mut self,
        origin: &Rc<Location>,
        destination: &Rc<Location>,
        profile: RouteCostProfile,
    ) -> RouteJob {
        let graph = self.graph();
        let search = destination.has_coordinates().then(|| {
            path_search(
                graph.clone(),
                origin,
                destination,
                self.max_walk_distance,
                CostSelector::for_profile(profile),
                HashSet::new(),
            )
        });
        RouteJob {
            graph,
            origin: origin.clone(),
            destination: destination.clone(),
            search,
        }
    }

    /// Up to `maximum` distinct routes in increasing cost.
    pub fn find_route_alternatives(
        &mut self,
        origin: &Rc<Location>,
        destination: &Rc<Location>,
        profile: RouteCostProfile,
        maximum: usize,
    ) -> Vec<Route> {
        self.alternatives_job(origin, destination, profile, maximum)
            .run()
    }

    /// [`RouteFinder::find_route_alternatives`] as a job that runs in
    /// bounded steps.
    pub fn alternatives_job(
        &mut self,
        origin: &Rc<Location>,
        destination: &Rc<Location>,
        profile: RouteCostProfile,
        maximum: usize,
    ) -> AlternativesJob {
        let mut job = AlternativesJob {
            graph: None,
            origin: origin.clone(),
            destination: destination.clone(),
            reach: self.max_walk_distance,
            cost: CostSelector::for_profile(profile),
            maximum,
            target: SearchTarget::Node(0),
            started: false,
            running: None,
            pending: VecDeque::new(),
            queue: BinaryHeap::new(),
            candidates: Vec::new(),
            sequence: 0,
            expanded: 0,
            choices: Vec::new(),
            signatures: HashSet::new(),
            seen_bans: HashSet::from([Vec::new()]),
            fallback: None,
            done: None,
        };
        if maximum == 0 {
            job.done = Some(Vec::new());
            return job;
        }
        let graph = self.graph();
        if !destination.has_coordinates() {
            job.done = Some(vec![no_coordinates(destination)]);
            return job;
        }
        job.target = match graph.node_index_of(destination) {
            Some(index) => SearchTarget::Node(index),
            None => SearchTarget::Position(destination.coords),
        };
        job.running = Some(Running::Path {
            search: path_search(
                graph.clone(),
                origin,
                destination,
                job.reach,
                job.cost.clone(),
                HashSet::new(),
            ),
            bans: Vec::new(),
        });
        job.fallback = Some(direct_walk(origin, destination));
        job.graph = Some(graph);
        job
    }

    /// The nearest usable place within twice the walking range.
    #[must_use]
    pub fn find_nearest_named_location(&self, position: &Location) -> Option<Rc<Location>> {
        let mut nearest = None;
        let mut nearest_distance = f64::MAX;
        for location in self.database.all_locations() {
            if !location.use_in_route_finding || location.is_retired || !location.has_coordinates()
            {
                continue;
            }
            let distance = position.distance_to(location);
            if distance < nearest_distance && distance < self.max_walk_distance * 2.0 {
                nearest_distance = distance;
                nearest = Some(location.clone());
            }
        }
        nearest
    }

    /// Whether the graph has an edge `step` would block. Only the edges
    /// leaving the nodes the step starts from are looked at (for a walk,
    /// blocked both ways, those its end names too).
    pub fn has_graph_step(&mut self, step: &BlockedStep) -> bool {
        let graph = self.graph();
        let mut starts: Vec<usize> = graph.nodes_matching(&step.from, step.from_id).collect();
        if text::eq_ignore_case(&step.kind, "Travel") {
            starts.extend(graph.nodes_matching(&step.to, step.to_id));
        }
        starts.into_iter().any(|node| {
            graph.edges(node).any(|edge| {
                step.blocks(
                    graph.location(edge.from),
                    graph.location(edge.to),
                    edge.kind,
                )
            })
        })
    }

    /// The places whose name contains the text (ignoring case).
    #[must_use]
    pub fn search_locations(&self, text: &str) -> Vec<Rc<Location>> {
        self.database.search_locations(text)
    }

    /// Every usable, unretired place name once (ignoring case), sorted
    /// ignoring case (DIVERGENCES.md, GA-6).
    #[must_use]
    pub fn all_destination_names(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut names: Vec<String> = self
            .database
            .all_locations()
            .iter()
            .filter(|l| l.use_in_route_finding && !l.is_retired)
            .filter(|l| seen.insert(text::fold(&l.name)))
            .map(|l| l.name.clone())
            .collect();
        names.sort_by(|a, b| {
            a.to_lowercase()
                .cmp(&b.to_lowercase())
                .then_with(|| b.cmp(a))
        });
        names
    }
}

fn path_search(
    graph: Rc<RouteGraph>,
    origin: &Location,
    destination: &Location,
    reach: f64,
    cost: CostSelector,
    bans: HashSet<EdgeRef>,
) -> PathSearch<Rc<RouteGraph>> {
    // A destination outside the graph (a point, an object) may be reached
    // from any node within walking range of it.
    let target = match graph.node_index_of(destination) {
        Some(index) => SearchTarget::Node(index),
        None => SearchTarget::Position(destination.coords),
    };
    PathSearch::from_position(graph, origin, target, reach, cost, bans)
}

fn no_coordinates(destination: &Location) -> Route {
    let mut route = Route::new(destination.name.clone());
    route.description = format!("Destination '{}' has no coordinates.", destination.name);
    route
}

fn direct_walk(origin: &Rc<Location>, destination: &Rc<Location>) -> Route {
    let mut route = Route::new(destination.name.clone());
    route.add_travel_step(origin.clone(), destination.clone(), "Walk");
    route.description = "Direct walk (no graph route found)".to_owned();
    route
}

/// `RouteFinder.RouteFromPath`: the route a search path describes, from
/// the exact origin.
fn route_from_path(
    graph: &RouteGraph,
    origin: &Rc<Location>,
    destination: &Rc<Location>,
    path: &[PathEdge],
) -> Route {
    let connection = &path[0];
    let last = &path[path.len() - 1];
    if last.to == NodeRef::Goal {
        // The destination is off the graph: walk the last stretch to it.
        if path.len() == 1 || (path.len() == 2 && connection.cost == 0.0) {
            let mut walk = Route::new(destination.name.clone());
            let via = if last.cost == 0.0 { "Arrived" } else { "Walk" };
            walk.add_travel_step(origin.clone(), destination.clone(), via);
            return walk;
        }
        let mut onward = route_from_path(graph, origin, destination, &path[..path.len() - 1]);
        if last.cost > 0.0
            && let Some(from) = last.from.node()
        {
            onward.add_travel_step(graph.location(from).clone(), destination.clone(), "Walk");
        }
        return onward;
    }
    let Some(entry) = connection
        .to
        .node()
        .map(|index| graph.location(index).clone())
    else {
        return Route::new(destination.name.clone());
    };
    if path.len() == 1 {
        let mut direct = Route::new(destination.name.clone());
        let via = if connection.cost == 0.0 {
            "Arrived"
        } else {
            "Walk"
        };
        direct.add_travel_step(origin.clone(), entry, via);
        return direct;
    }
    let mut route = graph.to_route(&path[1..], &destination.name);
    if connection.cost > 0.0 {
        route.prepend_step(RouteStep::new(
            RouteStepKind::Travel,
            origin.clone(),
            entry,
            connection.cost,
            "Walk to start",
        ));
    } else {
        route.set_first_step_origin(origin.clone());
    }
    route
}

/// A route search in progress ([`RouteFinder::route_job`]).
#[derive(Debug)]
pub struct RouteJob {
    graph: Rc<RouteGraph>,
    origin: Rc<Location>,
    destination: Rc<Location>,
    search: Option<PathSearch<Rc<RouteGraph>>>,
}

impl RouteJob {
    /// Does at most `max_ops` units of search work (one unit is one node
    /// expanded); `Some` with the route once done.
    pub fn step(&mut self, max_ops: usize) -> Option<Route> {
        let Some(search) = &mut self.search else {
            return Some(no_coordinates(&self.destination));
        };
        let path = search.step(max_ops)?;
        Some(match path {
            Some(path) if !path.is_empty() => {
                route_from_path(&self.graph, &self.origin, &self.destination, &path)
            }
            _ => direct_walk(&self.origin, &self.destination),
        })
    }

    /// Runs to the end.
    #[must_use]
    pub fn run(mut self) -> Route {
        loop {
            if let Some(route) = self.step(usize::MAX) {
                return route;
            }
        }
    }
}

/// A search an alternatives job runs.
#[derive(Debug)]
enum Running {
    /// The best path avoiding a set of interactions.
    Path {
        search: PathSearch<Rc<RouteGraph>>,
        bans: Vec<EdgeRef>,
    },
    /// The best path using at least one interaction.
    Interaction(InteractionSearch<Rc<RouteGraph>>),
}

/// A search waiting to run.
#[derive(Debug)]
enum Pending {
    Interaction,
    Banned(Vec<EdgeRef>),
}

/// An alternatives search in progress ([`RouteFinder::alternatives_job`]).
#[derive(Debug)]
pub struct AlternativesJob {
    graph: Option<Rc<RouteGraph>>,
    origin: Rc<Location>,
    destination: Rc<Location>,
    reach: f64,
    cost: CostSelector,
    maximum: usize,
    target: SearchTarget,
    started: bool,
    running: Option<Running>,
    pending: VecDeque<Pending>,
    /// Candidates by total cost, then by when they were found.
    queue: BinaryHeap<Reverse<(OrderedTotal, u64, usize)>>,
    candidates: Vec<Option<(Vec<EdgeRef>, Vec<PathEdge>)>>,
    sequence: u64,
    expanded: usize,
    choices: Vec<Route>,
    signatures: HashSet<Vec<String>>,
    seen_bans: HashSet<Vec<String>>,
    fallback: Option<Route>,
    done: Option<Vec<Route>>,
}

/// A path's total cost, ordered (no NaN: every edge cost is checked).
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrderedTotal(f64);

impl Eq for OrderedTotal {}

impl PartialOrd for OrderedTotal {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedTotal {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl AlternativesJob {
    /// Does at most `max_ops` units of search work; `Some` with the routes
    /// once done.
    pub fn step(&mut self, max_ops: usize) -> Option<Vec<Route>> {
        let mut budget = max_ops;
        loop {
            if let Some(done) = &self.done {
                return Some(done.clone());
            }
            let graph = self.graph.clone()?;
            if let Some(running) = &mut self.running {
                if budget == 0 {
                    return None;
                }
                let (result, bans) = match running {
                    Running::Path { search, bans } => {
                        (search.step_with(&mut budget)?, std::mem::take(bans))
                    }
                    Running::Interaction(search) => (search.step_with(&mut budget)?, Vec::new()),
                };
                self.running = None;
                if !self.started {
                    self.started = true;
                    match result {
                        Some(path) => self.enqueue(bans, path),
                        None => {
                            self.done = self.fallback.take().map(|route| vec![route]);
                            continue;
                        }
                    }
                } else if let Some(path) = result {
                    self.enqueue(bans, path);
                }
                continue;
            }
            if let Some(next) = self.pending.pop_front() {
                self.running = Some(match next {
                    Pending::Interaction => Running::Interaction(InteractionSearch::new(
                        graph,
                        &self.origin,
                        self.target,
                        self.reach,
                        self.cost.clone(),
                    )),
                    Pending::Banned(bans) => Running::Path {
                        search: path_search(
                            graph,
                            &self.origin,
                            &self.destination,
                            self.reach,
                            self.cost.clone(),
                            bans.iter().copied().collect(),
                        ),
                        bans,
                    },
                });
                continue;
            }
            self.expand_next(&graph);
        }
    }

    fn enqueue(&mut self, bans: Vec<EdgeRef>, path: Vec<PathEdge>) {
        let total: f64 = path.iter().map(|e| self.cost.cost(&e.view())).sum();
        self.queue.push(Reverse((
            OrderedTotal(total),
            self.sequence,
            self.candidates.len(),
        )));
        self.sequence += 1;
        self.candidates.push(Some((bans, path)));
    }

    /// Takes the cheapest candidate: a new interaction sequence is a
    /// choice; then each of its interactions is banned in turn and the
    /// best path without it searched for.
    fn expand_next(&mut self, graph: &RouteGraph) {
        let Some(Reverse((_, _, index))) = self.queue.pop() else {
            self.finish();
            return;
        };
        if self.choices.len() >= self.maximum || self.expanded >= MAX_ALTERNATIVE_EXPANSIONS {
            self.finish();
            return;
        }
        self.expanded += 1;
        let Some((bans, path)) = self.candidates[index].take() else {
            return;
        };
        let interactions: Vec<&PathEdge> = path
            .iter()
            .filter(|e| e.kind != RouteEdgeKind::Walk)
            .collect();
        let signature: Vec<String> = interactions.iter().map(|e| e.key()).collect();
        if signature.is_empty() {
            self.pending.push_back(Pending::Interaction);
        }
        if self.signatures.insert(signature) {
            self.choices.push(route_from_path(
                graph,
                &self.origin,
                &self.destination,
                &path,
            ));
        }
        for edge in interactions {
            let Some(banned) = edge.edge else {
                continue;
            };
            let mut set = bans.clone();
            if !set.contains(&banned) {
                set.push(banned);
            }
            let mut key: Vec<String> = set.iter().map(|e| ban_key(graph, *e)).collect();
            key.sort();
            if self.seen_bans.insert(key) {
                self.pending.push_back(Pending::Banned(set));
            }
        }
    }

    fn finish(&mut self) {
        self.done = Some(std::mem::take(&mut self.choices));
    }

    /// Runs to the end.
    #[must_use]
    pub fn run(mut self) -> Vec<Route> {
        loop {
            if let Some(routes) = self.step(usize::MAX) {
                return routes;
            }
        }
    }
}

fn ban_key(graph: &RouteGraph, edge: EdgeRef) -> String {
    let stored = graph.edge(edge);
    format!(
        "{}:{}:{}:{}",
        stored.from,
        stored.to,
        stored.kind.name(),
        stored.via
    )
}
