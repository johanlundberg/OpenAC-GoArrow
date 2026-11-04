//! The route graph (`RouteFinding/RouteGraph.cs`): every usable place is a
//! node, each Atlas portal's arrival another; places within walking range
//! (10 map units, 2.4 km) are joined by walks, a portal joins its entrance
//! to its arrival, portal devices and route starts add their edges, and
//! blocked steps are left out. A search is Dijkstra's over these edges
//! (portals make straight-line distance a wrong estimate), from an exact
//! position joined to every node in range, to a node or to a point off
//! the graph.
//!
//! Walk edges are found with a grid of cells as wide as the walking range,
//! which gives the same edges, in the same order, as comparing every pair.
//! Searches are [`PathSearch`] and [`InteractionSearch`], which can run in
//! bounded steps so a long one can be spread over several frames.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;
use std::ops::Deref;
use std::rc::Rc;

use crate::coords::Coordinates;
use crate::corrections::BlockedStep;
use crate::database::LocationDatabase;
use crate::finder::RouteCostProfile;
use crate::location::{Location, LocationType};
use crate::route::Route;
use crate::text;

/// How an edge is travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RouteEdgeKind {
    /// On foot.
    Walk,
    /// Through a portal or portal device.
    Portal,
    /// By a recall spell.
    Recall,
    /// By a lifestone recall.
    Lifestone,
}

impl RouteEdgeKind {
    /// The kind's name (`Walk`, `Portal`, `Recall`, `Lifestone`).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Walk => "Walk",
            Self::Portal => "Portal",
            Self::Recall => "Recall",
            Self::Lifestone => "Lifestone",
        }
    }

    /// The kind a route start's `via` text names (`RouteGraph.InferEdgeKind`):
    /// a recall, a lifestone, a portal or device, else a walk.
    #[must_use]
    pub fn from_via(via: &str) -> Self {
        let lower = via.trim().to_lowercase();
        if lower.contains("recall") {
            Self::Recall
        } else if lower.contains("lifestone") || lower.contains("life stone") {
            Self::Lifestone
        } else if lower.contains("portal") || lower.contains("device") {
            Self::Portal
        } else {
            Self::Walk
        }
    }
}

/// One end of a path edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeRef {
    /// The exact position a search starts from.
    Origin,
    /// A graph node.
    Node(usize),
    /// A destination off the graph (a coordinate or an object).
    Goal,
}

impl NodeRef {
    /// The graph node, if this is one.
    #[must_use]
    pub fn node(self) -> Option<usize> {
        match self {
            Self::Node(index) => Some(index),
            _ => None,
        }
    }

    /// The index GoArrow gave this end (-1 origin, -2 goal).
    fn index(self) -> i64 {
        match self {
            Self::Origin => -1,
            Self::Node(index) => index as i64,
            Self::Goal => -2,
        }
    }
}

/// A stored graph edge: the node it leaves and its place in that node's
/// list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeRef {
    /// The node the edge leaves.
    pub from: u32,
    /// The edge's index among that node's edges.
    pub slot: u32,
}

/// What a cost selector sees of an edge.
#[derive(Debug, Clone, Copy)]
pub struct EdgeView<'a> {
    /// How it is travelled.
    pub kind: RouteEdgeKind,
    /// Its cost: map units for a walk, 0.1 for a portal, 0.05 for a
    /// recall.
    pub cost: f64,
    /// The portal's, device's or recall's name, or `Walk`.
    pub via: &'a str,
}

/// An edge of a found path.
#[derive(Debug, Clone, PartialEq)]
pub struct PathEdge {
    /// Where it leaves from.
    pub from: NodeRef,
    /// Where it goes.
    pub to: NodeRef,
    /// How it is travelled.
    pub kind: RouteEdgeKind,
    /// Its cost (see [`EdgeView::cost`]).
    pub cost: f64,
    /// The portal's, device's or recall's name; `Walk` or `Walk to start`
    /// for walks.
    pub via: Rc<str>,
    /// The stored edge, or `None` for a search's own connections (from
    /// the origin, to an off-graph goal).
    pub edge: Option<EdgeRef>,
}

impl PathEdge {
    /// The edge as a cost selector sees it.
    #[must_use]
    pub fn view(&self) -> EdgeView<'_> {
        EdgeView {
            kind: self.kind,
            cost: self.cost,
            via: &self.via,
        }
    }

    /// GoArrow's text key of an edge (`from:to:kind:via`), which names an
    /// interaction in alternative signatures and ban sets.
    pub(crate) fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.from.index(),
            self.to.index(),
            self.kind.name(),
            self.via
        )
    }
}

/// A stored edge, borrowed from the graph.
#[derive(Debug, Clone, Copy)]
pub struct GraphEdge<'a> {
    /// The node it leaves.
    pub from: usize,
    /// The node it reaches.
    pub to: usize,
    /// How it is travelled.
    pub kind: RouteEdgeKind,
    /// Its cost.
    pub cost: f64,
    /// The portal's, device's or recall's name, or `Walk`.
    pub via: &'a str,
    /// Which stored edge it is.
    pub edge: EdgeRef,
}

/// The cost a search gives an edge.
#[derive(Clone, Default)]
pub enum CostSelector {
    /// The edge's own cost.
    #[default]
    Base,
    /// A route cost profile's weighting.
    Profile(RouteCostProfile),
    /// Any weighting.
    Custom(Rc<dyn Fn(&EdgeView<'_>) -> f64>),
}

impl CostSelector {
    /// The selector a profile searches with.
    #[must_use]
    pub fn for_profile(profile: RouteCostProfile) -> Self {
        match profile {
            RouteCostProfile::ShortestWalk => Self::Base,
            other => Self::Profile(other),
        }
    }

    /// The cost of an edge.
    #[must_use]
    pub fn cost(&self, edge: &EdgeView<'_>) -> f64 {
        match self {
            Self::Base | Self::Profile(RouteCostProfile::ShortestWalk) => edge.cost,
            Self::Profile(RouteCostProfile::FewestInteractions) => {
                if edge.kind == RouteEdgeKind::Walk {
                    1.0
                } else {
                    0.1
                }
            }
            Self::Profile(RouteCostProfile::PreferRecall) => match edge.kind {
                RouteEdgeKind::Recall | RouteEdgeKind::Lifestone => edge.cost * 0.1,
                _ => edge.cost,
            },
            Self::Profile(RouteCostProfile::AvoidInteractions) => {
                if edge.kind == RouteEdgeKind::Walk {
                    edge.cost
                } else {
                    edge.cost * 1000.0
                }
            }
            Self::Custom(select) => select(edge),
        }
    }

    fn walk(&self, distance: f64, via: &str) -> f64 {
        self.cost(&EdgeView {
            kind: RouteEdgeKind::Walk,
            cost: distance,
            via,
        })
    }
}

impl fmt::Debug for CostSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base => f.write_str("Base"),
            Self::Profile(profile) => write!(f, "Profile({profile:?})"),
            Self::Custom(_) => f.write_str("Custom"),
        }
    }
}

/// Where a search goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchTarget {
    /// A graph node.
    Node(usize),
    /// A point off the graph; the path leaves the graph at any node within
    /// reach of it, or walks straight there.
    Position(Coordinates),
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    to: u32,
    via: u32,
    kind: RouteEdgeKind,
    cost: f64,
}

const WALK: &str = "Walk";
const WALK_TO_START: &str = "Walk to start";

/// The graph.
#[derive(Debug)]
pub struct RouteGraph {
    nodes: Vec<Rc<Location>>,
    name_index: HashMap<String, usize>,
    lookup: NodeLookup,
    adjacency: Vec<Vec<Edge>>,
    vias: Vec<Rc<str>>,
    walk_to_start: Rc<str>,
}

impl Default for RouteGraph {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            name_index: HashMap::new(),
            lookup: NodeLookup::default(),
            adjacency: Vec::new(),
            vias: vec![Rc::from(WALK)],
            walk_to_start: Rc::from(WALK_TO_START),
        }
    }
}

fn is_atlas_portal(location: &Location) -> bool {
    location.kind.intersects(LocationType::ANY_PORTAL) && location.has_exit_coords()
}

/// Every node (arrivals too) by Atlas id and by name ignoring case: the
/// nodes one end of a blocked step names.
#[derive(Debug, Default)]
struct NodeLookup {
    by_id: HashMap<i32, Vec<u32>>,
    by_name: HashMap<String, Vec<u32>>,
}

impl NodeLookup {
    fn new(nodes: &[Rc<Location>]) -> Self {
        let mut lookup = Self::default();
        for (index, node) in nodes.iter().enumerate() {
            let index = index as u32;
            if node.id > 0 {
                lookup.by_id.entry(node.id).or_default().push(index);
            }
            lookup
                .by_name
                .entry(text::fold(&node.name))
                .or_default()
                .push(index);
        }
        lookup
    }

    /// The nodes a blocked step's end matches: by id when it is positive,
    /// else by name (as [`BlockedStep::blocks`] matches them).
    fn matching(&self, name: &str, id: i32) -> &[u32] {
        let found = if id > 0 {
            self.by_id.get(&id)
        } else {
            self.by_name.get(&text::fold(name))
        };
        found.map_or(&[], Vec::as_slice)
    }
}

/// The class of edge a blocked step's kind names: walks (`Travel`),
/// portals, recalls (recalls and lifestones).
fn step_class(kind: &str) -> Option<u8> {
    [("Travel", 0), ("Portal", 1), ("Recall", 2)]
        .into_iter()
        .find(|(name, _)| text::eq_ignore_case(kind, name))
        .map(|(_, class)| class)
}

fn edge_class(kind: RouteEdgeKind) -> u8 {
    match kind {
        RouteEdgeKind::Walk => 0,
        RouteEdgeKind::Portal => 1,
        RouteEdgeKind::Recall | RouteEdgeKind::Lifestone => 2,
    }
}

/// The blocked steps as the node pairs they block, so checking a pair is a
/// lookup rather than a scan of every step: the same answers as asking
/// each step's [`BlockedStep::blocks`].
#[derive(Debug, Default)]
struct BlockedPairs {
    pairs: HashSet<(u32, u32, u8)>,
    /// Nodes some blocked pair leaves from.
    from: Vec<bool>,
}

impl BlockedPairs {
    fn new<'a>(
        steps: impl Iterator<Item = &'a BlockedStep>,
        lookup: &NodeLookup,
        nodes: usize,
    ) -> Self {
        let mut blocked = Self {
            pairs: HashSet::new(),
            from: vec![false; nodes],
        };
        for step in steps {
            let Some(class) = step_class(&step.kind) else {
                continue;
            };
            let froms = lookup.matching(&step.from, step.from_id);
            let tos = lookup.matching(&step.to, step.to_id);
            for &from in froms {
                for &to in tos {
                    blocked.insert(from, to, class);
                    // A blocked walk is blocked both ways.
                    if class == 0 {
                        blocked.insert(to, from, class);
                    }
                }
            }
        }
        blocked
    }

    fn insert(&mut self, from: u32, to: u32, class: u8) {
        self.pairs.insert((from, to, class));
        self.from[from as usize] = true;
    }

    fn blocks(&self, from: usize, to: usize, kind: RouteEdgeKind) -> bool {
        self.from[from]
            && self
                .pairs
                .contains(&(from as u32, to as u32, edge_class(kind)))
    }
}

/// How far a [`GraphBuild`] has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildStage {
    /// The nodes, their indexes and the blocked pairs.
    Nodes,
    /// The walks, one node at a time.
    Walks,
    /// Portals, devices and route starts.
    Links,
}

/// Building a [`RouteGraph`] in bounded steps, so a large database does
/// not hold the session thread for one long call. The result is the
/// graph [`RouteGraph::build`] gives (it runs this to the end).
#[derive(Debug)]
pub struct GraphBuild {
    reach: f64,
    stage: BuildStage,
    graph: RouteGraph,
    atlas_portals: Vec<(usize, usize)>,
    blocked: BlockedPairs,
    /// Grid cells, as wide as the walking range, and the cell width; none
    /// when the range is not a positive finite number (every pair is
    /// compared instead).
    grid: Option<(Grid, f64)>,
    adjacency: Vec<Vec<Edge>>,
    next: usize,
}

/// Node indexes by grid cell (row, column).
type Grid = HashMap<(i64, i64), Vec<usize>>;

fn grid_key(location: &Location, cell: f64) -> (i64, i64) {
    (
        (location.coords.ns / cell).floor() as i64,
        (location.coords.ew / cell).floor() as i64,
    )
}

fn is_finite_point(location: &Location) -> bool {
    location.coords.ns.is_finite() && location.coords.ew.is_finite()
}

impl GraphBuild {
    /// A build joining places within `max_walk_distance` map units.
    #[must_use]
    pub fn new(max_walk_distance: f64) -> Self {
        Self {
            reach: max_walk_distance,
            stage: BuildStage::Nodes,
            graph: RouteGraph::default(),
            atlas_portals: Vec::new(),
            blocked: BlockedPairs::default(),
            grid: None,
            adjacency: Vec::new(),
            next: 0,
        }
    }

    /// Does at most `budget` units of work (the nodes, one node's walks,
    /// the links: one unit each) over `db`, which must not change between
    /// steps; `Some` with the graph once built.
    pub fn step(&mut self, db: &LocationDatabase, budget: &mut usize) -> Option<RouteGraph> {
        while *budget > 0 {
            *budget -= 1;
            match self.stage {
                BuildStage::Nodes => self.nodes(db),
                BuildStage::Walks => self.walks_of_next_node(),
                BuildStage::Links => {
                    self.links(db);
                    let mut graph = std::mem::take(&mut self.graph);
                    graph.adjacency = std::mem::take(&mut self.adjacency);
                    return Some(graph);
                }
            }
        }
        None
    }

    /// Runs to the end.
    #[must_use]
    pub fn run(mut self, db: &LocationDatabase) -> RouteGraph {
        loop {
            let mut budget = usize::MAX;
            if let Some(graph) = self.step(db, &mut budget) {
                return graph;
            }
        }
    }

    fn nodes(&mut self, db: &LocationDatabase) {
        // Usable places, one per name, except Atlas portals: several
        // distinct portals can share a name, each with its own arrival.
        let mut seen_names = HashSet::new();
        let mut nodes: Vec<Rc<Location>> = db
            .all_locations()
            .iter()
            .filter(|l| l.use_in_route_finding && !l.is_retired && l.has_coordinates())
            .filter(|l| is_atlas_portal(l) || seen_names.insert(text::fold(&l.name)))
            .cloned()
            .collect();
        nodes.sort_by(|a, b| a.cmp_by_name(b));
        for (index, node) in nodes.iter().enumerate() {
            self.graph
                .name_index
                .entry(text::fold(&node.name))
                .or_insert(index);
        }

        // Each Atlas portal's arrival is a node of its own, so the walk
        // after a portal starts where it lets you out.
        let entrance_count = nodes.len();
        for entrance in 0..entrance_count {
            let portal = &nodes[entrance];
            if !is_atlas_portal(portal) {
                continue;
            }
            let arrival = Location::at(
                format!("{} arrival ({})", portal.name, portal.id),
                portal.exit_coords(),
            );
            self.atlas_portals.push((entrance, nodes.len()));
            nodes.push(Rc::new(arrival));
        }
        self.graph.lookup = NodeLookup::new(&nodes);
        self.blocked = BlockedPairs::new(db.blocked_steps(), &self.graph.lookup, nodes.len());
        self.adjacency = vec![Vec::new(); nodes.len()];
        let reach = self.reach;
        if reach > 0.0 && reach.is_finite() {
            // Wider than the reach, so a pair within reach is never more
            // than one cell apart even with rounding.
            let cell = reach * 1.0001;
            let mut grid: Grid = HashMap::new();
            for (index, node) in nodes.iter().enumerate() {
                // A point with an infinite or NaN value is at no finite
                // distance.
                if is_finite_point(node) {
                    grid.entry(grid_key(node, cell)).or_default().push(index);
                }
            }
            self.grid = Some((grid, cell));
        }
        self.stage = if nodes.is_empty() {
            BuildStage::Links
        } else {
            BuildStage::Walks
        };
        self.graph.nodes = nodes;
    }

    /// The walks between the next node and the nodes after it within
    /// reach; that node has all its walks then, so they are put in target
    /// order (the order comparing every pair in index order gives).
    fn walks_of_next_node(&mut self) {
        let nodes = &self.graph.nodes;
        let i = self.next;
        if i >= nodes.len() {
            self.stage = BuildStage::Links;
            return;
        }
        self.next += 1;
        let node = &nodes[i];
        let mut pairs = Vec::new();
        match &self.grid {
            None => {
                for (j, other) in nodes.iter().enumerate().skip(i + 1) {
                    let distance = node.distance_to(other);
                    if distance <= self.reach {
                        pairs.push((j, distance));
                    }
                }
            }
            Some(_) if !is_finite_point(node) => {}
            Some((grid, cell)) => {
                let (row, column) = grid_key(node, *cell);
                for dr in -1..=1i64 {
                    for dc in -1..=1i64 {
                        let key = (row.saturating_add(dr), column.saturating_add(dc));
                        let Some(members) = grid.get(&key) else {
                            continue;
                        };
                        for &j in members {
                            if j <= i {
                                continue;
                            }
                            let distance = node.distance_to(&nodes[j]);
                            if distance <= self.reach {
                                pairs.push((j, distance));
                            }
                        }
                    }
                }
            }
        }
        for (j, distance) in pairs {
            if self.blocked.blocks(i, j, RouteEdgeKind::Walk) {
                continue;
            }
            let walk = |to: usize| Edge {
                to: to as u32,
                via: 0,
                kind: RouteEdgeKind::Walk,
                cost: distance,
            };
            self.adjacency[i].push(walk(j));
            self.adjacency[j].push(walk(i));
        }
        self.adjacency[i].sort_by_key(|e: &Edge| e.to);
        if self.next == self.graph.nodes.len() {
            self.stage = BuildStage::Links;
        }
    }

    fn links(&mut self, db: &LocationDatabase) {
        let graph = &mut self.graph;
        let adjacency = &mut self.adjacency;
        let blocked = &self.blocked;
        let nodes = &graph.nodes;
        let mut vias: HashMap<String, u32> = HashMap::from([(WALK.to_owned(), 0)]);
        let mut via_texts: Vec<Rc<str>> = vec![Rc::from(WALK)];
        let mut intern = |via: &str| -> u32 {
            *vias.entry(via.to_owned()).or_insert_with(|| {
                via_texts.push(Rc::from(via));
                (via_texts.len() - 1) as u32
            })
        };

        for &(entrance, arrival) in &self.atlas_portals {
            if blocked.blocks(entrance, arrival, RouteEdgeKind::Portal) {
                continue;
            }
            adjacency[entrance].push(Edge {
                to: arrival as u32,
                via: intern(&nodes[entrance].name),
                kind: RouteEdgeKind::Portal,
                cost: 0.1,
            });
        }

        // Older device records name only the destination and the device;
        // one with an entrance can be routed without guessing where it is.
        for device in db.portal_devices() {
            if device.entrance_location.trim().is_empty() {
                continue;
            }
            let exit_name = if device.exit_location.trim().is_empty() {
                &device.destination
            } else {
                &device.exit_location
            };
            let (Some(from), Some(to)) = (
                graph.node_index(&device.entrance_location),
                graph.node_index(exit_name),
            ) else {
                continue;
            };
            if from == to || blocked.blocks(from, to, RouteEdgeKind::Portal) {
                continue;
            }
            adjacency[from].push(Edge {
                to: to as u32,
                via: intern(&device.via),
                kind: RouteEdgeKind::Portal,
                cost: 0.1,
            });
        }

        for start in db.route_starts() {
            let (Some(from), Some(to)) = (
                graph.node_index(&start.from),
                graph.node_index(&start.destination),
            ) else {
                continue;
            };
            let kind = RouteEdgeKind::from_via(&start.via);
            if blocked.blocks(from, to, kind) {
                continue;
            }
            let cost = match kind {
                RouteEdgeKind::Walk => nodes[from].distance_to(&nodes[to]),
                RouteEdgeKind::Portal => 0.1,
                RouteEdgeKind::Recall | RouteEdgeKind::Lifestone => 0.05,
            };
            adjacency[from].push(Edge {
                to: to as u32,
                via: intern(&start.via),
                kind,
                cost,
            });
        }
        graph.vias = via_texts;
    }
}

impl RouteGraph {
    /// Builds the graph from a database; places within `max_walk_distance`
    /// map units are joined by walks. [`GraphBuild`] does it in steps.
    #[must_use]
    pub fn build(db: &LocationDatabase, max_walk_distance: f64) -> Self {
        GraphBuild::new(max_walk_distance).run(db)
    }

    /// The nodes (arrivals too) the end of a blocked step names: by Atlas
    /// id when `id` is positive, else by name ignoring case.
    pub fn nodes_matching(&self, name: &str, id: i32) -> impl Iterator<Item = usize> + '_ {
        self.lookup
            .matching(name, id)
            .iter()
            .map(|&index| index as usize)
    }

    /// The number of nodes (places and arrivals).
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The number of stored edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.adjacency.iter().map(Vec::len).sum()
    }

    /// The place at a node.
    #[must_use]
    pub fn location(&self, index: usize) -> &Rc<Location> {
        &self.nodes[index]
    }

    /// The first node with this name (ignoring case); arrival nodes have
    /// no name entry.
    #[must_use]
    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.name_index.get(&text::fold(name)).copied()
    }

    /// The node holding this very location, else the first with its name
    /// (the Atlas has distinct portals that share a name).
    #[must_use]
    pub fn node_index_of(&self, location: &Location) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| std::ptr::eq(&**node, location))
            .or_else(|| self.node_index(&location.name))
    }

    /// The edges leaving a node, in search order.
    pub fn edges(&self, node: usize) -> impl Iterator<Item = GraphEdge<'_>> {
        self.adjacency[node]
            .iter()
            .enumerate()
            .map(move |(slot, edge)| GraphEdge {
                from: node,
                to: edge.to as usize,
                kind: edge.kind,
                cost: edge.cost,
                via: &self.vias[edge.via as usize],
                edge: EdgeRef {
                    from: node as u32,
                    slot: slot as u32,
                },
            })
    }

    /// The edge a reference names.
    #[must_use]
    pub fn edge(&self, edge: EdgeRef) -> GraphEdge<'_> {
        let stored = self.adjacency[edge.from as usize][edge.slot as usize];
        GraphEdge {
            from: edge.from as usize,
            to: stored.to as usize,
            kind: stored.kind,
            cost: stored.cost,
            via: &self.vias[stored.via as usize],
            edge,
        }
    }

    fn path_edge(&self, edge: EdgeRef) -> PathEdge {
        let stored = self.adjacency[edge.from as usize][edge.slot as usize];
        PathEdge {
            from: NodeRef::Node(edge.from as usize),
            to: NodeRef::Node(stored.to as usize),
            kind: stored.kind,
            cost: stored.cost,
            via: self.vias[stored.via as usize].clone(),
            edge: Some(edge),
        }
    }

    /// The cheapest path between two nodes; empty when they are the same,
    /// `None` when there is none.
    #[must_use]
    pub fn find_shortest_path(
        &self,
        from: usize,
        to: usize,
        cost: &CostSelector,
    ) -> Option<Vec<PathEdge>> {
        PathSearch::between_nodes(self, from, to, cost.clone()).run()
    }

    /// [`RouteGraph::find_shortest_path`] between named nodes, base costs.
    #[must_use]
    pub fn find_shortest_path_by_name(&self, from: &str, to: &str) -> Option<Vec<PathEdge>> {
        let (from, to) = (self.node_index(from)?, self.node_index(to)?);
        self.find_shortest_path(from, to, &CostSelector::Base)
    }

    /// The cheapest path from an exact position, joined for this search to
    /// every node within `reach`; the first edge leaves
    /// [`NodeRef::Origin`].
    #[must_use]
    pub fn find_shortest_path_from_position(
        &self,
        position: &Location,
        to: usize,
        reach: f64,
        cost: &CostSelector,
        excluded: Option<&HashSet<EdgeRef>>,
    ) -> Option<Vec<PathEdge>> {
        let excluded = excluded.cloned().unwrap_or_default();
        PathSearch::from_position(
            self,
            position,
            SearchTarget::Node(to),
            reach,
            cost.clone(),
            excluded,
        )
        .run()
    }

    /// Like [`RouteGraph::find_shortest_path_from_position`] to a point off
    /// the graph: the last edge reaches [`NodeRef::Goal`]. Walking straight
    /// there is always a candidate, so finite coordinates always give a
    /// path.
    #[must_use]
    pub fn find_shortest_path_to_position(
        &self,
        position: &Location,
        destination: &Location,
        reach: f64,
        cost: &CostSelector,
        excluded: Option<&HashSet<EdgeRef>>,
    ) -> Option<Vec<PathEdge>> {
        let excluded = excluded.cloned().unwrap_or_default();
        let target = SearchTarget::Position(destination.coords);
        PathSearch::from_position(self, position, target, reach, cost.clone(), excluded).run()
    }

    /// The cheapest path that uses at least one portal or recall, never
    /// passing through the destination.
    #[must_use]
    pub fn find_shortest_path_using_interaction(
        &self,
        position: &Location,
        target: SearchTarget,
        reach: f64,
        cost: &CostSelector,
    ) -> Option<Vec<PathEdge>> {
        InteractionSearch::new(self, position, target, reach, cost.clone()).run()
    }

    /// The route a path of graph edges describes: walks are travel steps,
    /// portals portal steps, recalls and lifestones recall steps. An empty
    /// path is one "Arrived" step at the named destination.
    #[must_use]
    pub fn to_route(&self, path: &[PathEdge], destination_name: &str) -> Route {
        let mut route = Route::new(destination_name);
        if path.is_empty() {
            if let Some(index) = self.node_index(destination_name) {
                let location = self.nodes[index].clone();
                route.add_travel_step(location.clone(), location, "Arrived");
            }
            return route;
        }
        for edge in path {
            let (Some(from), Some(to)) = (edge.from.node(), edge.to.node()) else {
                continue;
            };
            let (from, to) = (self.nodes[from].clone(), self.nodes[to].clone());
            match edge.kind {
                RouteEdgeKind::Walk => route.add_travel_step(from, to, &edge.via),
                RouteEdgeKind::Portal => route.add_portal_step(from, to, &edge.via),
                RouteEdgeKind::Recall | RouteEdgeKind::Lifestone => {
                    route.add_recall_step(from, to, &edge.via)
                }
            }
        }
        route
    }
}

/// A cost with a total order (no NaN reaches a queue).
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrderedCost(f64);

impl Eq for OrderedCost {}

impl PartialOrd for OrderedCost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedCost {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

const NONE: u32 = u32::MAX;

/// A search's own walk from the origin: to a node, or straight to the
/// goal.
#[derive(Debug, Clone, Copy)]
struct Connection {
    to: NodeRef,
    distance: f64,
}

/// How a search reached a node.
#[derive(Debug, Clone, Copy)]
enum Step {
    None,
    Graph(EdgeRef),
    /// The final walk from a node to the off-graph goal.
    Goal(f64),
}

impl<G: Deref<Target = RouteGraph>> PathSearch<G> {
    fn connection_edge(&self, connection: Connection) -> PathEdge {
        let via = if connection.to == NodeRef::Goal {
            self.graph.vias[0].clone()
        } else {
            self.graph.walk_to_start.clone()
        };
        PathEdge {
            from: NodeRef::Origin,
            to: connection.to,
            kind: RouteEdgeKind::Walk,
            cost: connection.distance,
            via,
            edge: None,
        }
    }
}

fn step_edge(graph: &RouteGraph, step: Step, from: usize) -> Option<PathEdge> {
    match step {
        Step::None => None,
        Step::Graph(edge) => Some(graph.path_edge(edge)),
        Step::Goal(distance) => Some(PathEdge {
            from: NodeRef::Node(from),
            to: NodeRef::Goal,
            kind: RouteEdgeKind::Walk,
            cost: distance,
            via: graph.vias[0].clone(),
            edge: None,
        }),
    }
}

/// A Dijkstra search in progress over a graph (`&RouteGraph` or
/// `Rc<RouteGraph>`). Ties go to the cheaper entry, then the lower node,
/// then the earlier one, as in GoArrow's ordered set.
#[derive(Debug)]
pub struct PathSearch<G: Deref<Target = RouteGraph>> {
    graph: G,
    to: usize,
    goal: Option<(Coordinates, f64)>,
    cost: CostSelector,
    excluded: HashSet<EdgeRef>,
    score: Vec<f64>,
    came_from: Vec<u32>,
    came_from_step: Vec<Step>,
    origin: Vec<u32>,
    source: Vec<Option<Connection>>,
    closed: Vec<bool>,
    open: BinaryHeap<Reverse<(OrderedCost, u32, u32)>>,
    tie: u32,
    finished: bool,
}

impl<G: Deref<Target = RouteGraph>> PathSearch<G> {
    fn with_seeds(
        graph: G,
        to: usize,
        goal: Option<(Coordinates, f64)>,
        cost: CostSelector,
        excluded: HashSet<EdgeRef>,
        seeds: Vec<(usize, Option<Connection>, f64)>,
    ) -> Self {
        let n = graph.nodes.len() + usize::from(goal.is_some());
        let mut search = Self {
            graph,
            to,
            goal,
            cost,
            excluded,
            score: vec![f64::MAX; n],
            came_from: vec![NONE; n],
            came_from_step: vec![Step::None; n],
            origin: vec![NONE; n],
            source: vec![None; n],
            closed: vec![false; n],
            open: BinaryHeap::new(),
            tie: 0,
            finished: to >= n || seeds.is_empty(),
        };
        if search.finished {
            return search;
        }
        for (index, connection, cost) in seeds {
            if cost >= search.score[index] {
                continue;
            }
            search.score[index] = cost;
            search.origin[index] = index as u32;
            search.source[index] = connection;
            search.push(cost, index);
        }
        search
    }

    fn push(&mut self, cost: f64, node: usize) {
        self.open
            .push(Reverse((OrderedCost(cost), node as u32, self.tie)));
        self.tie += 1;
    }

    /// A search from one node to another (no origin connection).
    #[must_use]
    pub fn between_nodes(graph: G, from: usize, to: usize, cost: CostSelector) -> Self {
        let seeds = if from < graph.nodes.len() {
            vec![(from, None, 0.0)]
        } else {
            Vec::new()
        };
        Self::with_seeds(graph, to, None, cost, HashSet::new(), seeds)
    }

    /// A search from an exact position joined to every node within
    /// `reach`, to a node or a point, skipping `excluded` edges.
    #[must_use]
    pub fn from_position(
        graph: G,
        position: &Location,
        target: SearchTarget,
        reach: f64,
        cost: CostSelector,
        excluded: HashSet<EdgeRef>,
    ) -> Self {
        let node_count = graph.nodes.len();
        let mut seeds = Vec::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            let distance = position.distance_to(node);
            if !distance.is_finite() || distance > reach {
                continue;
            }
            let seed_cost = if distance == 0.0 {
                0.0
            } else {
                cost.walk(distance, WALK_TO_START)
            };
            if seed_cost.is_finite() && seed_cost >= 0.0 {
                let connection = Connection {
                    to: NodeRef::Node(index),
                    distance,
                };
                seeds.push((index, Some(connection), seed_cost));
            }
        }
        let (to, goal) = match target {
            SearchTarget::Node(index) => (index, None),
            SearchTarget::Position(destination) => {
                let direct = position.coords.distance_to(destination);
                if direct.is_finite() {
                    let direct_cost = if direct == 0.0 {
                        0.0
                    } else {
                        cost.walk(direct, WALK)
                    };
                    if direct_cost.is_finite() && direct_cost >= 0.0 {
                        let connection = Connection {
                            to: NodeRef::Goal,
                            distance: direct,
                        };
                        seeds.push((node_count, Some(connection), direct_cost));
                    }
                }
                (node_count, Some((destination, reach)))
            }
        };
        Self::with_seeds(graph, to, goal, cost, excluded, seeds)
    }

    /// Expands at most `max_ops` nodes; `Some` once the search is over,
    /// with the path or `None` when there is none. After that it stays
    /// over and answers `Some(None)`.
    pub fn step(&mut self, max_ops: usize) -> Option<Option<Vec<PathEdge>>> {
        let mut budget = max_ops;
        self.step_with(&mut budget)
    }

    /// Runs the search to the end.
    #[must_use]
    pub fn run(mut self) -> Option<Vec<PathEdge>> {
        loop {
            if let Some(result) = self.step(usize::MAX) {
                return result;
            }
        }
    }

    pub(crate) fn step_with(&mut self, budget: &mut usize) -> Option<Option<Vec<PathEdge>>> {
        if self.finished {
            return Some(None);
        }
        let node_count = self.graph.nodes.len();
        while *budget > 0 {
            let Some(Reverse((_, current, _))) = self.open.pop() else {
                self.finished = true;
                return Some(None);
            };
            let current = current as usize;
            if self.closed[current] {
                continue;
            }
            self.closed[current] = true;
            *budget -= 1;
            if current == self.to {
                self.finished = true;
                return Some(Some(self.reconstruct()));
            }
            // A dungeon entrance is not a place to walk on from: only the
            // node a search starts on may.
            let started_here = self.came_from[current] == NONE
                && self.source[self.origin[current] as usize].is_none();
            let no_walks = self.graph.nodes[current]
                .kind
                .intersects(LocationType::DUNGEON)
                && !started_here;
            let base = self.score[current];
            for slot in 0..self.graph.adjacency[current].len() {
                let edge = self.graph.adjacency[current][slot];
                let edge_ref = EdgeRef {
                    from: current as u32,
                    slot: slot as u32,
                };
                if (no_walks && edge.kind == RouteEdgeKind::Walk)
                    || self.excluded.contains(&edge_ref)
                {
                    continue;
                }
                let neighbor = edge.to as usize;
                if self.closed[neighbor] {
                    continue;
                }
                let edge_cost = self.cost.cost(&EdgeView {
                    kind: edge.kind,
                    cost: edge.cost,
                    via: &self.graph.vias[edge.via as usize],
                });
                if edge_cost.is_nan() || edge_cost < 0.0 {
                    continue;
                }
                let tentative = base + edge_cost;
                if tentative >= self.score[neighbor] {
                    continue;
                }
                self.came_from[neighbor] = current as u32;
                self.came_from_step[neighbor] = Step::Graph(edge_ref);
                self.origin[neighbor] = self.origin[current];
                self.score[neighbor] = tentative;
                self.push(tentative, neighbor);
            }
            if let Some((goal, reach)) = self.goal
                && current < node_count
                && !no_walks
                && !self.closed[self.to]
            {
                let distance = self.graph.nodes[current].coords.distance_to(goal);
                if distance.is_finite() && distance <= reach {
                    let edge_cost = self.cost.walk(distance, WALK);
                    let tentative = base + edge_cost;
                    if !(edge_cost.is_nan() || edge_cost < 0.0) && tentative < self.score[self.to] {
                        let to = self.to;
                        self.came_from[to] = current as u32;
                        self.came_from_step[to] = Step::Goal(distance);
                        self.origin[to] = self.origin[current];
                        self.score[to] = tentative;
                        self.push(tentative, to);
                    }
                }
            }
        }
        None
    }

    fn reconstruct(&self) -> Vec<PathEdge> {
        let mut path = Vec::new();
        let mut at = self.to;
        while self.came_from[at] != NONE {
            let from = self.came_from[at] as usize;
            if let Some(edge) = step_edge(&self.graph, self.came_from_step[at], from) {
                path.push(edge);
            }
            at = from;
        }
        path.reverse();
        if let Some(connection) = self.source[self.origin[self.to] as usize] {
            path.insert(0, self.connection_edge(connection));
        }
        path
    }
}

/// A search for the cheapest path that uses at least one portal or recall
/// ([`RouteGraph::find_shortest_path_using_interaction`]), in progress.
/// Each node has two states: before and after the first interaction.
#[derive(Debug)]
pub struct InteractionSearch<G: Deref<Target = RouteGraph>> {
    graph: G,
    to: usize,
    goal: Option<(Coordinates, f64)>,
    cost: CostSelector,
    score: Vec<f64>,
    parent: Vec<u32>,
    parent_step: Vec<Step>,
    first: Vec<Option<Connection>>,
    closed: Vec<bool>,
    open: BinaryHeap<Reverse<(OrderedCost, u64, u32)>>,
    sequence: u64,
    finished: bool,
}

impl<G: Deref<Target = RouteGraph>> InteractionSearch<G> {
    /// A search from an exact position joined to every node within
    /// `reach`.
    #[must_use]
    pub fn new(
        graph: G,
        position: &Location,
        target: SearchTarget,
        reach: f64,
        cost: CostSelector,
    ) -> Self {
        let node_count = graph.nodes.len();
        let (to, goal, valid) = match target {
            SearchTarget::Node(index) => (index, None, index < node_count),
            SearchTarget::Position(destination) => (node_count, Some((destination, reach)), true),
        };
        let states = (node_count + usize::from(goal.is_some())) * 2;
        let mut search = Self {
            graph,
            to,
            goal,
            cost,
            score: vec![f64::INFINITY; states],
            parent: vec![NONE; states],
            parent_step: vec![Step::None; states],
            first: vec![None; states],
            closed: vec![false; states],
            open: BinaryHeap::new(),
            sequence: 0,
            finished: !valid,
        };
        if !valid {
            return search;
        }
        for index in 0..node_count {
            let distance = position.distance_to(&search.graph.nodes[index]);
            if !distance.is_finite() || distance > reach {
                continue;
            }
            let seed_cost = search.cost.walk(distance, WALK_TO_START);
            if !seed_cost.is_finite() || seed_cost < 0.0 {
                continue;
            }
            let state = index * 2;
            search.score[state] = seed_cost;
            search.first[state] = Some(Connection {
                to: NodeRef::Node(index),
                distance,
            });
            search.push(seed_cost, state);
        }
        search
    }

    fn push(&mut self, cost: f64, state: usize) {
        self.open
            .push(Reverse((OrderedCost(cost), self.sequence, state as u32)));
        self.sequence += 1;
    }

    /// Expands at most `max_ops` states; `Some` once the search is over.
    pub fn step(&mut self, max_ops: usize) -> Option<Option<Vec<PathEdge>>> {
        let mut budget = max_ops;
        self.step_with(&mut budget)
    }

    /// Runs the search to the end.
    #[must_use]
    pub fn run(mut self) -> Option<Vec<PathEdge>> {
        loop {
            if let Some(result) = self.step(usize::MAX) {
                return result;
            }
        }
    }

    pub(crate) fn step_with(&mut self, budget: &mut usize) -> Option<Option<Vec<PathEdge>>> {
        if self.finished {
            return Some(None);
        }
        let node_count = self.graph.nodes.len();
        while *budget > 0 {
            let Some(Reverse((OrderedCost(score), _, state))) = self.open.pop() else {
                self.finished = true;
                return Some(None);
            };
            let state = state as usize;
            if self.closed[state] || score > self.score[state] {
                continue;
            }
            self.closed[state] = true;
            *budget -= 1;
            let node = state / 2;
            let interacted = state % 2 == 1;
            if node == self.to && interacted {
                self.finished = true;
                return Some(Some(self.reconstruct(state)));
            }
            if node == self.to {
                // Never pass through the destination to make an alternative.
                continue;
            }
            let dungeon = self.graph.nodes[node]
                .kind
                .intersects(LocationType::DUNGEON);
            for slot in 0..self.graph.adjacency[node].len() {
                let edge = self.graph.adjacency[node][slot];
                if dungeon && edge.kind == RouteEdgeKind::Walk {
                    continue;
                }
                let edge_cost = self.cost.cost(&EdgeView {
                    kind: edge.kind,
                    cost: edge.cost,
                    via: &self.graph.vias[edge.via as usize],
                });
                let next = edge.to as usize * 2
                    + usize::from(interacted || edge.kind != RouteEdgeKind::Walk);
                let step = Step::Graph(EdgeRef {
                    from: node as u32,
                    slot: slot as u32,
                });
                self.relax(state, score, edge_cost, next, step);
            }
            if let Some((goal, reach)) = self.goal
                && interacted
                && !dungeon
                && node < node_count
            {
                let distance = self.graph.nodes[node].coords.distance_to(goal);
                if distance.is_finite() && distance <= reach {
                    let edge_cost = self.cost.walk(distance, WALK);
                    self.relax(
                        state,
                        score,
                        edge_cost,
                        self.to * 2 + 1,
                        Step::Goal(distance),
                    );
                }
            }
        }
        None
    }

    fn relax(&mut self, state: usize, score: f64, edge_cost: f64, next: usize, step: Step) {
        if !edge_cost.is_finite() || edge_cost < 0.0 {
            return;
        }
        let total = score + edge_cost;
        if total >= self.score[next] {
            return;
        }
        self.score[next] = total;
        self.parent[next] = state as u32;
        self.parent_step[next] = step;
        self.first[next] = self.first[state];
        self.push(total, next);
    }

    fn reconstruct(&self, state: usize) -> Vec<PathEdge> {
        let mut path = Vec::new();
        let mut at = state;
        while self.parent[at] != NONE {
            let from = self.parent[at] as usize;
            if let Some(edge) = step_edge(&self.graph, self.parent_step[at], from / 2) {
                path.push(edge);
            }
            at = from;
        }
        path.reverse();
        if let Some(connection) = self.first[state] {
            path.insert(
                0,
                PathEdge {
                    from: NodeRef::Origin,
                    to: connection.to,
                    kind: RouteEdgeKind::Walk,
                    cost: connection.distance,
                    via: self.graph.walk_to_start.clone(),
                    edge: None,
                },
            );
        }
        path
    }
}
