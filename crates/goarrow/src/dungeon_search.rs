//! The search for a portal the route needs inside a dungeon
//! (`DungeonPortalSearch.cs`): a depth-first walk over the client's
//! floorplan that visits rooms not seen yet, deeper storeys first, and
//! backtracks when a branch has nothing left. A learned path to the exit
//! ([`crate::traversals`]) is followed first when there is one.
//!
//! Indoor objects reach the client only as their cells come into view, so
//! visiting rooms is how the portal is found. A target whose straight line
//! passes near a known portal is first checked with a path preview, and a
//! walk stops before it approaches a portal: walking into one would leave
//! the dungeon.
//!
//! Geometry is landblock-local metres (x east, y north) on the floorplan's
//! layers; positions are the client's map coordinates.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use openac_plugin::Host;
use openac_plugin::navigation::{NavigationPlan, NavigationPlanStatus};
use openac_plugin::world::{
    DungeonCell, DungeonLayer, DungeonWall, Floorplan, METRES_PER_MAP_UNIT, Position, WorldObject,
};

use crate::portal_match::is_portal;
use crate::traversals::TraversalStore;

/// A walk this close (metres of walking) counts as nearby.
pub const NEARBY_METRES: f64 = 40.0;
/// Cells further apart than this on one floor are joined through the cells
/// between them, metres.
pub const WALK_LINK_METRES: f32 = 30.0;
/// How many recorded waypoints the window lists.
pub const MAX_LISTED_RECORDED_WAYPOINTS: usize = 80;
/// The arrival distance of the clearance preview, metres.
pub const CLEARANCE_ARRIVAL_METRES: f32 = 2.5;

type V2 = (f32, f32);

fn sub(a: V2, b: V2) -> V2 {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: V2, b: V2) -> V2 {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: V2, s: f32) -> V2 {
    (a.0 * s, a.1 * s)
}

fn dot(a: V2, b: V2) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: V2, b: V2) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

fn length_squared(a: V2) -> f32 {
    dot(a, a)
}

fn length(a: V2) -> f32 {
    length_squared(a).sqrt()
}

fn dist(a: V2, b: V2) -> f32 {
    length(sub(a, b))
}

fn normalize(a: V2) -> V2 {
    let l = length(a);
    (a.0 / l, a.1 / l)
}

fn lerp(a: V2, b: V2, t: f32) -> V2 {
    add(a, scale(sub(b, a), t))
}

fn centre(cell: &DungeonCell) -> V2 {
    (cell.center.0, cell.center.1)
}

fn local_xy(p: &Position) -> V2 {
    let local = p.landblock_local();
    (local[0], local[1])
}

fn walls_cross(walls: &[DungeonWall], start: V2, end: V2) -> bool {
    walls
        .iter()
        .any(|w| intersects_wall(start, end, w.start, w.end))
}

/// The ground distance plus the height difference, metres.
fn search_distance(from: &Position, to: &Position) -> f64 {
    from.horizontal_distance_metres(to)
        + (from.elevation - to.elevation).abs() * METRES_PER_MAP_UNIT
}

/// A clearance preview asked for and not answered yet.
#[derive(Debug, Clone)]
struct PendingPreview {
    handle: u32,
    started: f64,
    result: Option<NavigationPlan>,
}

/// A candidate cell to visit.
#[derive(Debug, Clone)]
struct Candidate {
    position: Position,
    height: f32,
}

/// The search's state.
#[derive(Debug, Default)]
pub struct DungeonPortalSearch {
    exit_key: String,
    checked_recorded_path: bool,
    following_recorded_path: bool,
    recorded_path: VecDeque<Position>,
    landblock: u32,
    attempted: HashSet<u32>,
    covered: HashSet<u32>,
    shallow_dead_end_cells: HashSet<u32>,
    recess_plan: Option<Floorplan>,
    walk_plan: Option<Floorplan>,
    walk_links: HashMap<u32, Vec<(u32, f32)>>,
    branch: Vec<Position>,
    returning: bool,
    last_failure: String,
    target: Position,
    pending: Option<PendingPreview>,
    last_position: Option<Position>,
    reason: String,
}

impl DungeonPortalSearch {
    /// A search with nothing visited.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the search is doing, or why it stopped.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The recorded path still to walk (at most 80 points).
    #[must_use]
    pub fn recorded_waypoints(&self) -> Vec<Position> {
        self.recorded_path
            .iter()
            .take(MAX_LISTED_RECORDED_WAYPOINTS)
            .cloned()
            .collect()
    }

    /// How many recorded points are left.
    #[must_use]
    pub fn remaining_recorded_waypoints(&self) -> usize {
        self.recorded_path.len()
    }

    /// The cell being walked to.
    #[must_use]
    pub fn current_target(&self) -> Position {
        self.target.clone()
    }

    /// The handle of a clearance preview being waited for.
    #[must_use]
    pub fn pending_preview(&self) -> Option<u32> {
        self.pending.as_ref().map(|p| p.handle)
    }

    /// Forgets everything visited.
    pub fn reset(&mut self) {
        self.landblock = 0;
        self.attempted.clear();
        self.covered.clear();
        self.shallow_dead_end_cells.clear();
        self.recess_plan = None;
        self.branch.clear();
        self.returning = false;
        self.last_failure.clear();
        self.pending = None;
        self.last_position = None;
        self.checked_recorded_path = false;
        self.following_recorded_path = false;
        self.recorded_path.clear();
    }

    /// Searches for the exit `key` names; a different exit starts over.
    pub fn use_recorded_exit(&mut self, key: &str) {
        if self.exit_key == key {
            return;
        }
        self.reset();
        key.clone_into(&mut self.exit_key);
    }

    /// A clearance preview's answer; false when it is not this search's.
    pub fn plan_arrived(&mut self, plan: &NavigationPlan) -> bool {
        match &mut self.pending {
            Some(pending) if pending.handle == plan.handle => {
                pending.result = Some(plan.clone());
                true
            }
            _ => false,
        }
    }

    /// The next place to walk to, or none: none while a clearance preview
    /// is answered, or when nothing is left ([`Self::reason`] says which).
    /// `objects`: the client's object table, as read this tick.
    pub fn try_get_next_target(
        &mut self,
        host: &mut Host<'_>,
        objects: &[WorldObject],
        traversals: &TraversalStore,
        now: f64,
    ) -> Option<Position> {
        let snapshot = host.navigation();
        let block = snapshot.position.cell_id & 0xFFFF_0000;
        if !snapshot.is_available || snapshot.is_portal_space || snapshot.position.is_outdoor {
            return None;
        }
        if self.landblock != block {
            self.reset();
            self.landblock = block;
        }
        if self.branch.is_empty() {
            self.branch.push(snapshot.position.clone());
        }
        let portals: Vec<Position> = objects
            .iter()
            .filter(|p| is_portal(p))
            .filter_map(|p| p.position.clone())
            .filter(|p| (p.cell_id & 0xFFFF_0000) == block)
            .collect();
        if let Some(pending) = &self.pending {
            let Some(plan) = pending.result.clone() else {
                self.reason = format!(
                    "Checking portal clearance for cell {:08X} ({}s).",
                    self.target.cell_id,
                    crate::text::format_fixed(now - pending.started, 0, 0)
                );
                return None;
            };
            self.pending = None;
            if plan.status == NavigationPlanStatus::Routed
                && !plan.path.is_empty()
                && !Self::crosses_portal(&plan.path, &portals)
            {
                self.last_position = Some(snapshot.position.clone());
                self.set_target_reason();
                return Some(self.target.clone());
            }
            if self.following_recorded_path {
                self.complete_target(&snapshot.position, false);
            } else {
                self.attempted.insert(self.target.cell_id);
            }
            self.last_failure = plan.reason;
            if self.returning {
                self.complete_target(&snapshot.position, false);
            }
        }
        if !self.checked_recorded_path {
            self.checked_recorded_path = true;
            let trail = traversals.find_path(&snapshot.position, &self.exit_key);
            let known = host.dungeon_floorplan(block).unwrap_or_default();
            self.recorded_path
                .extend(Self::simplify_recorded_path(&trail, &known, &portals));
        }
        while let Some(point) = self.recorded_path.front() {
            if snapshot.position.horizontal_distance_metres(point) <= 2.5
                && (snapshot.position.elevation - point.elevation).abs() * METRES_PER_MAP_UNIT
                    <= 2.5
            {
                self.recorded_path.pop_front();
            } else {
                break;
            }
        }
        if let Some(recorded) = self.recorded_path.front().cloned() {
            self.following_recorded_path = true;
            self.target = recorded;
            return self.prepare_target(host, &snapshot.position, &portals, now);
        }
        let plan = host.dungeon_floorplan(block).unwrap_or_default();
        self.update_shallow_recesses(&plan);
        let current_cell = plan
            .cells
            .iter()
            .find(|c| c.cell_id == snapshot.position.cell_id);
        let current_floor = current_cell
            .map_or(snapshot.position.elevation * METRES_PER_MAP_UNIT, |c| {
                f64::from(c.layer_z)
            });
        self.mark_visible_cells(&snapshot.position, &plan);
        let floors: HashMap<u32, f32> = host
            .indoor_cells(block)
            .iter()
            .map(|c| (c.cell_id, c.origin.2))
            .collect();
        let here = snapshot.position.clone();
        let candidates: Vec<Candidate> = plan
            .cells
            .iter()
            .filter(|c| is_on_floor(c, &plan))
            .map(|c| Candidate {
                position: Self::to_position(c, &floors),
                height: c.layer_z,
            })
            .filter(|c| {
                let id = c.position.cell_id;
                !self.attempted.contains(&id)
                    && !self.covered.contains(&id)
                    && (!self.shallow_dead_end_cells.contains(&id)
                        || portals.iter().any(|portal| {
                            (portal.elevation - c.position.elevation).abs() * METRES_PER_MAP_UNIT
                                <= 6.0
                                && portal.horizontal_distance_metres(&c.position) <= 36.0
                        }))
                    && !self.branch.iter().any(|p| p.cell_id == id)
                    && id != here.cell_id
                    && (here.horizontal_distance_metres(&c.position) > 8.0
                        || (current_floor - f64::from(c.height)).abs() > 3.0)
                    && !portals
                        .iter()
                        .any(|portal| portal.horizontal_distance_metres(&c.position) < 12.0)
            })
            .collect();
        // Distances follow the floorplan's open floor, so a corridor that
        // bends back still reads as leading on from here.
        self.update_walk_graph(&plan);
        let from_here = self.walk_distances(&here, &plan);
        let previous = (self.branch.len() > 1).then(|| self.branch[self.branch.len() - 2].clone());
        let from_previous = previous.as_ref().map(|p| self.walk_distances(p, &plan));
        let walk = |walks: &HashMap<u32, f32>,
                    from: &Position,
                    to: &Position,
                    other: Option<&HashMap<u32, f32>>|
         -> f64 {
            match walks.get(&to.cell_id) {
                Some(&w) if other.is_none_or(|o| o.contains_key(&to.cell_id)) => f64::from(w),
                _ => search_distance(from, to),
            }
        };
        // A target a shorter walk from here than from the previous
        // checkpoint continues this branch; a nearby way down always does.
        let forward: Vec<Candidate> = match (&previous, &from_previous) {
            (Some(previous), Some(from_previous)) => candidates
                .iter()
                .filter(|c| {
                    (f64::from(c.height) < current_floor - 3.0
                        && here.horizontal_distance_metres(&c.position) <= NEARBY_METRES)
                        || walk(&from_here, &here, &c.position, Some(from_previous)) + 0.5
                            < walk(from_previous, previous, &c.position, Some(&from_here))
                })
                .cloned()
                .collect(),
            _ => candidates.clone(),
        };
        self.returning = forward.is_empty() && self.branch.len() > 1;
        if self.returning {
            self.target = self.branch[self.branch.len() - 2].clone();
        } else if candidates.is_empty() {
            self.reason = if plan.cells.is_empty() {
                "Dungeon floorplan is unavailable.".to_owned()
            } else {
                format!(
                    "No exploration targets remain ({} attempted, {} covered). {}",
                    self.attempted.len(),
                    self.covered.len(),
                    self.last_failure
                )
            };
            return None;
        } else {
            let ranked: Vec<(Position, f32, f64)> = forward
                .iter()
                .map(|c| {
                    (
                        c.position.clone(),
                        c.height,
                        walk(&from_here, &here, &c.position, None),
                    )
                })
                .collect();
            let nearby: Vec<(Position, f32, f64)> = ranked
                .iter()
                .filter(|c| c.2 <= NEARBY_METRES)
                .cloned()
                .collect();
            let pool = if nearby.is_empty() { ranked } else { nearby };
            let storey = |height: f32| {
                let h = f64::from(height);
                if h < current_floor - 3.0 {
                    0
                } else if h <= current_floor + 3.0 {
                    1
                } else {
                    2
                }
            };
            let cost =
                |c: &(Position, f32, f64)| c.2 + (f64::from(c.1) - current_floor).max(0.0) * 2.0;
            // Lower storeys first, then the shortest walk; climbing costs
            // extra so a level route is taken over going back upstairs.
            let best = pool.iter().min_by(|a, b| {
                storey(a.1)
                    .cmp(&storey(b.1))
                    .then(cost(a).total_cmp(&cost(b)))
                    .then(a.0.cell_id.cmp(&b.0.cell_id))
            });
            if let Some(best) = best {
                self.target = best.0.clone();
            }
        }
        self.prepare_target(host, &here, &portals, now)
    }

    fn prepare_target(
        &mut self,
        host: &mut Host<'_>,
        here: &Position,
        portals: &[Position],
        now: f64,
    ) -> Option<Position> {
        if Self::crosses_portal(&[here.clone(), self.target.clone()], portals) {
            let handle = host.preview_path_to_place(self.target.clone(), CLEARANCE_ARRIVAL_METRES);
            self.pending = Some(PendingPreview {
                handle,
                started: now,
                result: None,
            });
            self.reason = format!(
                "Checking portal clearance for cell {:08X}.",
                self.target.cell_id
            );
            return None;
        }
        self.last_position = Some(here.clone());
        self.set_target_reason();
        Some(self.target.clone())
    }

    fn set_target_reason(&mut self) {
        let doing = if self.following_recorded_path {
            "Following recorded path to"
        } else if self.returning {
            "Backtracking to"
        } else {
            "Searching"
        };
        self.reason = format!(
            "{doing} cell {:08X} ({} attempted, {} covered, {} branch depth).",
            self.target.cell_id,
            self.attempted.len(),
            self.covered.len(),
            self.branch.len() as i64 - 1
        );
    }

    /// A recorded walk with the turns it does not need taken out: from
    /// each point, the farthest later point (at most 63 ahead, 45 m away)
    /// on the same layer that a straight line reaches over open floor,
    /// without crossing a wall or passing a portal, within 3 m of height.
    #[must_use]
    pub fn simplify_recorded_path(
        path: &[Position],
        plan: &Floorplan,
        portals: &[Position],
    ) -> Vec<Position> {
        if path.len() < 3 {
            return path.to_vec();
        }
        let mut simplified = vec![path[0].clone()];
        let layers: HashMap<u32, f32> = plan.cells.iter().map(|c| (c.cell_id, c.layer_z)).collect();
        let mut from = 0;
        while from < path.len() - 1 {
            let mut farthest = from + 1;
            let from_layer = layers.get(&path[from].cell_id).copied().unwrap_or(0.0);
            for to in (from + 2)..path.len().min(from + 64) {
                match layers.get(&path[to - 1].cell_id) {
                    Some(&middle) if middle == from_layer => {}
                    _ => break,
                }
                // A shortcut spans only a modest walk on one floor.
                if path[from].horizontal_distance_metres(&path[to]) > 45.0 {
                    continue;
                }
                if can_shortcut(&path[from], &path[to], plan, portals) {
                    farthest = to;
                }
            }
            simplified.push(path[farthest].clone());
            from = farthest;
        }
        simplified
    }

    /// The walk to the current target ended, there or not.
    pub fn complete_target(&mut self, position: &Position, arrived: bool) {
        if self.following_recorded_path {
            if arrived && !self.recorded_path.is_empty() {
                self.recorded_path.pop_front();
            } else {
                // Fall back to exploration when a saved segment fails.
                self.recorded_path.clear();
            }
            self.following_recorded_path = false;
            return;
        }
        if self.returning {
            if arrived && self.branch.len() > 1 {
                self.branch.pop();
            } else {
                // An obstructed return cannot remain a usable branch.
                self.branch.clear();
            }
        } else if arrived && !self.branch.iter().any(|p| p.cell_id == position.cell_id) {
            self.branch.push(position.clone());
        }
        self.returning = false;
    }

    /// The walk was paused for a portal's identification: the target may
    /// be tried again.
    pub fn pause_target(&mut self) {
        if !self.following_recorded_path && !self.returning {
            self.attempted.remove(&self.target.cell_id);
        }
        self.returning = false;
    }

    /// Whether `position` is closer to `portal` than the last position
    /// the search saw.
    #[must_use]
    pub fn is_approaching_portal(&self, position: &Position, portal: &Position) -> bool {
        self.last_position.as_ref().is_some_and(|previous| {
            position.horizontal_distance_metres(portal) + 0.05
                < previous.horizontal_distance_metres(portal)
        })
    }

    /// False when the walk must stop: out of the dungeon (unless it may
    /// pass outdoors), in portal space, or closing on a portal within 8 m
    /// on the same floor. `objects`: the client's object table, as read
    /// this tick.
    pub fn safe_to_continue(
        &mut self,
        host: &mut Host<'_>,
        objects: &[WorldObject],
        may_pass_outdoors: bool,
    ) -> bool {
        let snapshot = host.navigation();
        if !snapshot.is_available
            || snapshot.is_portal_space
            || (snapshot.position.is_outdoor && !may_pass_outdoors)
        {
            return false;
        }
        let previous = self.last_position.replace(snapshot.position.clone());
        let Some(previous) = previous else {
            return true;
        };
        for portal in objects {
            let Some(position) = portal.position.as_ref() else {
                continue;
            };
            if (position.cell_id & 0xFFFF_0000) != self.landblock || !is_portal(portal) {
                continue;
            }
            let distance = snapshot.position.horizontal_distance_metres(position);
            if distance < 8.0
                && distance + 0.05 < previous.horizontal_distance_metres(position)
                && (!position.elevation.is_finite()
                    || (snapshot.position.elevation - position.elevation).abs()
                        * METRES_PER_MAP_UNIT
                        <= 6.0)
            {
                self.reason = format!(
                    "Stopped exploration before approaching portal '{}'.",
                    portal.name
                );
                return false;
            }
        }
        true
    }

    /// Whether a path passes within 8 m of a portal (and within 6 m of its
    /// height). Leaving a portal on the first segment is allowed while the
    /// segment heads away from it.
    #[must_use]
    pub fn crosses_portal(path: &[Position], portals: &[Position]) -> bool {
        for portal in portals {
            for i in 1..path.len() {
                let start = &path[i - 1];
                let end = &path[i];
                let dx = end.east_west - start.east_west;
                let dy = end.north_south - start.north_south;
                let length = dx * dx + dy * dy;
                let projection = (portal.east_west - start.east_west) * dx
                    + (portal.north_south - start.north_south) * dy;
                if i == 1 && start.horizontal_distance_metres(portal) < 8.0 && projection <= 0.0 {
                    continue;
                }
                let t = if length > 0.0 {
                    (projection / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let px = start.east_west + t * dx - portal.east_west;
                let py = start.north_south + t * dy - portal.north_south;
                let height = start.elevation + t * (end.elevation - start.elevation);
                if (px * px + py * py) * METRES_PER_MAP_UNIT * METRES_PER_MAP_UNIT < 64.0
                    && (!height.is_finite()
                        || !portal.elevation.is_finite()
                        || (height - portal.elevation).abs() * METRES_PER_MAP_UNIT <= 6.0)
                {
                    return true;
                }
            }
        }
        false
    }

    /// A walk to `cell` was accepted.
    pub fn target_accepted(&mut self, cell_id: u32) {
        if !self.following_recorded_path {
            self.attempted.insert(cell_id);
        }
    }

    /// Why the last walk failed.
    pub fn record_failure(&mut self, reason: Option<&str>) {
        reason
            .unwrap_or("The last cell was unreachable.")
            .clone_into(&mut self.last_failure);
    }

    fn mark_visible_cells(&mut self, position: &Position, plan: &Floorplan) {
        if plan.layers.is_empty() {
            return;
        }
        let z = plan
            .cells
            .iter()
            .find(|c| c.cell_id == position.cell_id)
            .map_or((position.elevation * METRES_PER_MAP_UNIT) as f32, |c| {
                c.layer_z
            });
        let Some(layer) = nearest_layer(plan, z) else {
            return;
        };
        if (layer.z - z).abs() > 6.0 || layer.floors.is_empty() {
            return;
        }
        let start = local_xy(position);
        for cell in &plan.cells {
            let end = centre(cell);
            // Nearby cells seen from here add no new search coverage;
            // walls and storeys separate rooms.
            if (cell.layer_z - layer.z).abs() < 0.1
                && dist(start, end) <= 40.0
                && is_on_floor(cell, plan)
                && !walls_cross(&layer.walls, start, end)
            {
                self.covered.insert(cell.cell_id);
            }
        }
    }

    // Links cells that see each other across open floor, and cells on
    // neighbouring storeys nearly over each other (stairs and ramps). A
    // floorplan without layers gets no graph.
    fn update_walk_graph(&mut self, plan: &Floorplan) {
        if self.walk_plan.as_ref() == Some(plan) {
            return;
        }
        self.walk_plan = Some(plan.clone());
        self.walk_links.clear();
        if plan.layers.is_empty() {
            return;
        }
        let cells: Vec<&DungeonCell> = plan.cells.iter().filter(|c| is_on_floor(c, plan)).collect();
        for cell in &cells {
            self.walk_links.insert(cell.cell_id, Vec::new());
        }
        for i in 0..cells.len() {
            for j in (i + 1)..cells.len() {
                let (a, b) = (cells[i], cells[j]);
                let (start, end) = (centre(a), centre(b));
                let across = dist(start, end);
                let rise = (a.layer_z - b.layer_z).abs();
                let distance = if rise < 0.1 {
                    let layer = plan.layers.iter().find(|l| (l.z - a.layer_z).abs() < 0.1);
                    match layer {
                        Some(layer)
                            if across <= WALK_LINK_METRES
                                && !walls_cross(&layer.walls, start, end) =>
                        {
                            across
                        }
                        _ => continue,
                    }
                } else if rise <= 6.5 && across <= 12.0 {
                    across + rise
                } else {
                    continue;
                };
                if let Some(links) = self.walk_links.get_mut(&a.cell_id) {
                    links.push((b.cell_id, distance));
                }
                if let Some(links) = self.walk_links.get_mut(&b.cell_id) {
                    links.push((a.cell_id, distance));
                }
            }
        }
    }

    // The walking distance from a position to every cell the graph reaches.
    fn walk_distances(&self, position: &Position, plan: &Floorplan) -> HashMap<u32, f32> {
        let mut distances: HashMap<u32, f32> = HashMap::new();
        if self.walk_links.is_empty() {
            return distances;
        }
        let mut queue = BinaryHeap::new();
        let mut order = 0u64;
        let mut seed = |distances: &mut HashMap<u32, f32>,
                        queue: &mut BinaryHeap<Queued>,
                        cell_id: u32,
                        distance: f32| {
            if distances
                .get(&cell_id)
                .is_some_and(|&known| known <= distance)
            {
                return;
            }
            distances.insert(cell_id, distance);
            order += 1;
            queue.push(Queued {
                distance,
                order,
                cell_id,
            });
        };
        if self.walk_links.contains_key(&position.cell_id) {
            seed(&mut distances, &mut queue, position.cell_id, 0.0);
        } else {
            let z = (position.elevation * METRES_PER_MAP_UNIT) as f32;
            if let Some(layer) = nearest_layer(plan, z) {
                let start = local_xy(position);
                for cell in &plan.cells {
                    let end = centre(cell);
                    let distance = dist(start, end);
                    if (cell.layer_z - layer.z).abs() < 0.1
                        && distance <= WALK_LINK_METRES
                        && self.walk_links.contains_key(&cell.cell_id)
                        && !walls_cross(&layer.walls, start, end)
                    {
                        seed(&mut distances, &mut queue, cell.cell_id, distance);
                    }
                }
            }
        }
        while let Some(Queued {
            distance, cell_id, ..
        }) = queue.pop()
        {
            if distance > distances[&cell_id] {
                continue;
            }
            for &(next, step) in &self.walk_links[&cell_id] {
                seed(&mut distances, &mut queue, next, distance + step);
            }
        }
        distances
    }

    fn update_shallow_recesses(&mut self, plan: &Floorplan) {
        if self.recess_plan.as_ref() == Some(plan) {
            return;
        }
        self.recess_plan = Some(plan.clone());
        self.shallow_dead_end_cells.clear();
        for layer in &plan.layers {
            if layer.floors.is_empty() || layer.walls.len() < 3 {
                continue;
            }
            let cells: Vec<&DungeonCell> = plan
                .cells
                .iter()
                .filter(|c| (c.layer_z - layer.z).abs() < 0.1 && is_on_floor(c, plan))
                .collect();
            let mut neighbors: HashMap<u32, Vec<(u32, f32)>> = HashMap::new();
            for cell in &cells {
                let start = centre(cell);
                let mut visible: Vec<(&DungeonCell, V2)> = cells
                    .iter()
                    .filter(|other| other.cell_id != cell.cell_id)
                    .map(|other| (*other, sub(centre(other), start)))
                    .filter(|(_, delta)| {
                        let l = length(*delta);
                        (2.0..=24.0).contains(&l)
                            && !walls_cross(&layer.walls, start, add(start, *delta))
                    })
                    .collect();
                visible.sort_by(|a, b| length_squared(a.1).total_cmp(&length_squared(b.1)));
                let mut directions: Vec<V2> = Vec::new();
                let mut links: Vec<(u32, f32)> = Vec::new();
                for (other, delta) in visible {
                    let direction = normalize(delta);
                    // Cell centres along one straight corridor are one way
                    // out, not separate branches.
                    if directions.iter().any(|known| dot(*known, direction) > 0.8) {
                        continue;
                    }
                    directions.push(direction);
                    links.push((other.cell_id, length(delta)));
                }
                neighbors.insert(cell.cell_id, links);
            }
            let by_id: HashMap<u32, &DungeonCell> = cells.iter().map(|c| (c.cell_id, *c)).collect();
            for leaf in &cells {
                let leaf_links = &neighbors[&leaf.cell_id];
                if leaf_links.len() != 1
                    || !is_enclosed_recess(centre(leaf), leaf_links[0].1, &layer.walls)
                {
                    continue;
                }
                let mut branch: Vec<u32> = Vec::new();
                let mut previous = 0u32;
                let mut current = leaf.cell_id;
                let mut depth = 0.0f32;
                loop {
                    let links = &neighbors[&current];
                    let onward: Vec<&(u32, f32)> =
                        links.iter().filter(|n| n.0 != previous).collect();
                    let radius = if links.is_empty() {
                        8.0
                    } else {
                        links.iter().map(|n| n.1).fold(f32::INFINITY, f32::min)
                    };
                    if !is_enclosed_recess(centre(by_id[&current]), radius, &layer.walls)
                        || onward.len() != 1
                    {
                        break;
                    }
                    branch.push(current);
                    depth += onward[0].1;
                    if depth > 32.0 || branch.len() > 3 {
                        branch.clear();
                        break;
                    }
                    previous = current;
                    current = onward[0].0;
                    if branch.contains(&current) {
                        branch.clear();
                        break;
                    }
                }
                if !branch.is_empty() && depth <= 32.0 && current != leaf.cell_id {
                    self.shallow_dead_end_cells.extend(branch);
                }
            }
        }
    }

    /// Where a walk to a floorplan cell goes: the cell's middle, at the
    /// height its piece's floor is built (`floors`: each cell's origin
    /// height) when that lies within 6 m of its layer, else the layer's.
    #[must_use]
    pub fn to_position(cell: &DungeonCell, floors: &HashMap<u32, f32>) -> Position {
        // Geometry centres may sit high above a room's floor, and an
        // unknown height makes the client resolve the point at terrain or
        // current height, which can turn a downstairs goal into an
        // upstairs walk. The layer only bounds the storey.
        let height = match floors.get(&cell.cell_id) {
            Some(&origin) if (origin - cell.layer_z).abs() <= 6.0 => origin,
            _ => cell.layer_z,
        };
        let mut position =
            Position::from_cell(cell.cell_id, [cell.center.0, cell.center.1, height], 0.0);
        position.is_outdoor = false;
        position
    }
}

/// A queue entry of the walk-distance search: nearest first, then first
/// queued.
#[derive(Debug)]
struct Queued {
    distance: f32,
    order: u64,
    cell_id: u32,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .distance
            .total_cmp(&self.distance)
            .then(other.order.cmp(&self.order))
    }
}

/// The layer nearest a height (the first of equals).
fn nearest_layer(plan: &Floorplan, z: f32) -> Option<&DungeonLayer> {
    plan.layers
        .iter()
        .min_by(|a, b| (a.z - z).abs().total_cmp(&(b.z - z).abs()))
}

fn can_shortcut(start: &Position, end: &Position, plan: &Floorplan, portals: &[Position]) -> bool {
    if (start.elevation - end.elevation).abs() * METRES_PER_MAP_UNIT > 3.0
        || DungeonPortalSearch::crosses_portal(&[start.clone(), end.clone()], portals)
    {
        return false;
    }
    let start_cell = plan.cells.iter().find(|c| c.cell_id == start.cell_id);
    let end_cell = plan.cells.iter().find(|c| c.cell_id == end.cell_id);
    let (Some(start_cell), Some(end_cell)) = (start_cell, end_cell) else {
        return false;
    };
    if start_cell.cell_id == 0
        || end_cell.cell_id == 0
        || (start_cell.layer_z - end_cell.layer_z).abs() > 0.1
    {
        return false;
    }
    let Some(layer) = plan
        .layers
        .iter()
        .find(|l| (l.z - start_cell.layer_z).abs() < 0.1)
    else {
        return false;
    };
    if layer.floors.is_empty() {
        return false;
    }
    let a = local_xy(start);
    let b = local_xy(end);
    if walls_cross(&layer.walls, a, b) {
        return false;
    }
    let samples = ((dist(a, b) / 2.0).ceil() as i32).max(1);
    (0..=samples).all(|i| {
        let point = lerp(a, b, i as f32 / samples as f32);
        layer.floors.iter().any(|polygon| contains(polygon, point))
    })
}

fn is_enclosed_recess(center: V2, entry_distance: f32, walls: &[DungeonWall]) -> bool {
    let radius = entry_distance.clamp(6.0, 16.0);
    let blocked = (0..8)
        .filter(|&direction| {
            let angle = direction as f32 * std::f32::consts::PI / 4.0;
            let end = add(center, scale((angle.cos(), angle.sin()), radius));
            walls_cross(walls, center, end)
        })
        .count();
    blocked >= 5
}

/// Whether a cell's middle lies on a floor polygon of its layer; a cell
/// whose layer the floorplan does not list counts as on the floor.
fn is_on_floor(cell: &DungeonCell, plan: &Floorplan) -> bool {
    let Some(layer) = plan
        .layers
        .iter()
        .find(|l| (l.z - cell.layer_z).abs() < 0.1)
    else {
        return true;
    };
    let point = centre(cell);
    layer.floors.iter().any(|polygon| contains(polygon, point))
}

/// Inside a polygon, or on its edge.
fn contains(polygon: &[V2], point: V2) -> bool {
    let mut inside = false;
    let count = polygon.len();
    if count == 0 {
        return false;
    }
    let mut j = count - 1;
    for i in 0..count {
        let a = polygon[j];
        let b = polygon[i];
        let edge = sub(b, a);
        if length_squared(edge) > 0.0 {
            let t = (dot(sub(point, a), edge) / length_squared(edge)).clamp(0.0, 1.0);
            if length_squared(sub(point, add(a, scale(edge, t)))) < 0.0001 {
                return true;
            }
        }
        if (a.1 > point.1) != (b.1 > point.1)
            && point.0 < (b.0 - a.0) * (point.1 - a.1) / (b.1 - a.1) + a.0
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Whether the segment from `start` to `end` meets the wall from `a` to
/// `b` (the start point itself excluded).
fn intersects_wall(start: V2, end: V2, a: (f32, f32), b: (f32, f32)) -> bool {
    let direction = sub(end, start);
    let wall = sub(b, a);
    let denominator = cross(direction, wall);
    if denominator.abs() < 0.0001 {
        if cross(sub(a, start), direction).abs() > 0.0001 || length_squared(direction) < 0.0001 {
            return false;
        }
        let first = dot(sub(a, start), direction) / length_squared(direction);
        let last = dot(sub(b, start), direction) / length_squared(direction);
        return first.max(last) > 0.001 && first.min(last) <= 1.0;
    }
    let t = cross(sub(a, start), wall) / denominator;
    let u = cross(sub(a, start), direction) / denominator;
    t > 0.001 && t <= 1.0 && (0.0..=1.0).contains(&u)
}
