//! Learned dungeon paths (`DungeonTraversalStore.cs`): the points a
//! character walked through dungeons, the directed steps between them, and
//! where each dungeon exit was found, kept in
//! `GoArrow/dungeon-traversals.json`. Edges are only ever observed
//! movement, never inferred: a teleport or a gap of more than 12 m breaks
//! the trace. Heights and cell ids keep overlapping floors apart.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use openac_plugin::Host;
use openac_plugin::navigation::NavigationSnapshot;
use openac_plugin::storage::{StoragePlace, StorageRoot};
use openac_plugin::world::{METRES_PER_MAP_UNIT, Position, WorldObject};
use serde::{Deserialize, Serialize};

use crate::portal_match::is_blank;

/// The storage key of the learned paths.
pub const TRAVERSALS_KEY: &str = "GoArrow/dungeon-traversals.json";
/// The largest file read, characters.
pub const MAX_FILE_CHARS: usize = 20_000_000;
/// The most points a file may hold.
pub const MAX_POINTS: usize = 50_000;
/// The most edges a file may hold.
pub const MAX_EDGES: usize = 100_000;
/// The most exits a file may hold.
pub const MAX_EXITS: usize = 10_000;

/// A walked point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TraversalPoint {
    /// The cell.
    pub cell_id: u32,
    /// East/west, map units.
    pub east_west: f64,
    /// North/south, map units.
    pub north_south: f64,
    /// Height, map units.
    pub elevation: f64,
}

impl TraversalPoint {
    /// The point as a position (heading 0, indoors).
    #[must_use]
    pub fn position(&self) -> Position {
        Position {
            cell_id: self.cell_id,
            east_west: self.east_west,
            north_south: self.north_south,
            elevation: self.elevation,
            heading_degrees: 0.0,
            is_outdoor: false,
        }
    }
}

/// A step walked from one point to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TraversalEdge {
    /// The point walked from (an index into the points).
    pub from: usize,
    /// The point walked to.
    pub to: usize,
}

/// Where a dungeon exit was found.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TraversalExit {
    /// The landblock (high 16 bits of the cells).
    pub landblock: u32,
    /// The exit's key (`"{EXIT}|{ns}|{ew}"`, or `"PORTAL|{NAME}"`).
    pub key: String,
    /// The portal's name.
    pub name: String,
    /// The portal's appraised destination.
    pub destination: String,
    /// The point the exit was seen from.
    pub anchor: usize,
}

/// The file's contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TraversalData {
    /// The format version (1; a file without one is version 1).
    #[serde(default = "version_one")]
    pub version: i32,
    /// The points.
    #[serde(default)]
    pub points: Vec<TraversalPoint>,
    /// The edges.
    #[serde(default)]
    pub edges: Vec<TraversalEdge>,
    /// The exits.
    #[serde(default)]
    pub exits: Vec<TraversalExit>,
}

fn version_one() -> i32 {
    1
}

impl Default for TraversalData {
    fn default() -> Self {
        Self {
            version: 1,
            points: Vec::new(),
            edges: Vec::new(),
            exits: Vec::new(),
        }
    }
}

fn place() -> StoragePlace {
    StoragePlace::global(StorageRoot::PluginData)
}

/// A point is usable: an indoor cell (index 0x100 and up) and finite
/// coordinates.
fn valid(p: &Position) -> bool {
    (p.cell_id & 0xFFFF) >= 0x100
        && p.east_west.is_finite()
        && p.north_south.is_finite()
        && p.elevation.is_finite()
}

/// The straight distance in three dimensions, metres.
fn distance(a: &Position, b: &Position) -> f64 {
    let across = a.horizontal_distance_metres(b);
    let up = (a.elevation - b.elevation) * METRES_PER_MAP_UNIT;
    (across * across + up * up).sqrt()
}

/// The side of the grid cells points are filed under for find-or-add,
/// metres: the distance within which two points are one, so a point's
/// match is in its own grid cell or one beside it.
const MERGE_METRES: f64 = 1.5;

/// A point's place in the find-or-add grid: its cell, then grid cells of
/// [`MERGE_METRES`] east, north and up.
type GridKey = (u32, i64, i64, i64);

#[allow(clippy::cast_possible_truncation)]
fn grid_key(p: &Position) -> GridKey {
    let at = |v: f64| (v * METRES_PER_MAP_UNIT / MERGE_METRES).floor() as i64;
    (
        p.cell_id,
        at(p.east_west),
        at(p.north_south),
        at(p.elevation),
    )
}

/// The learned paths.
#[derive(Debug, Default)]
pub struct TraversalStore {
    data: TraversalData,
    previous: Option<usize>,
    dirty: bool,
    error: String,
    // The saved file exists but did not load: it is kept, never saved over.
    load_failed: bool,
    // The store reached what a file may hold; said once.
    limit_noted: bool,
    notice: Option<String>,
    grid: HashMap<GridKey, Vec<usize>>,
    edge_set: HashSet<TraversalEdge>,
    exit_set: HashSet<TraversalExit>,
}

impl TraversalStore {
    /// Nothing learned.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the last load or save said went wrong, or empty.
    #[must_use]
    pub fn error(&self) -> &str {
        &self.error
    }

    /// The learned data.
    #[must_use]
    pub fn data(&self) -> &TraversalData {
        &self.data
    }

    /// Whether something learned is not saved yet.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// What to tell the log, once, if anything (the store reached the
    /// most a file may hold).
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    /// Reads the saved file; a missing file leaves nothing learned. A bad
    /// one is reported in [`Self::error`] and the log, and kept: nothing is
    /// saved over it.
    pub fn load(&mut self, host: &mut Host<'_>) {
        let failure = match host.storage_read_text(&place(), TRAVERSALS_KEY) {
            Ok(Some(json)) => match parse(&json) {
                Ok(data) => {
                    self.set_data(data);
                    None
                }
                Err(e) => Some(e),
            },
            Ok(None) => None,
            Err(e) => Some(e),
        };
        if let Some(e) = failure {
            self.load_failed = true;
            self.error = format!("Dungeon traversal data could not be loaded: {e}");
            host.warn(&format!(
                "GoArrow: {} The file is kept; paths learned now are not saved over it.",
                self.error
            ));
        }
    }

    /// Writes what changed; false when it could not (see [`Self::error`])
    /// or must not: the saved file did not load.
    pub fn save(&mut self, host: &mut Host<'_>) -> bool {
        if !self.dirty {
            return true;
        }
        if self.load_failed {
            return false;
        }
        match host.storage_write_text(&place(), TRAVERSALS_KEY, &self.export()) {
            Ok(()) => {
                self.dirty = false;
                self.error.clear();
                true
            }
            Err(e) => {
                self.error = format!("Dungeon traversal data could not be saved: {e}");
                false
            }
        }
    }

    /// Ends the current trace: the next point starts a new one.
    pub fn break_trace(&mut self) {
        self.previous = None;
    }

    /// Records where the character stands, joined to the previous point
    /// when it walked there: same landblock, at most 12 m away. Within 3 m
    /// in the same cell nothing is recorded. At the most points or edges a
    /// file may hold no new point or edge is recorded (noted once in
    /// [`Self::take_notice`]); known points still join the trace.
    pub fn observe(&mut self, snapshot: &NavigationSnapshot) {
        let p = &snapshot.position;
        if !snapshot.is_available || snapshot.is_portal_space || p.is_outdoor || !valid(p) {
            self.break_trace();
            return;
        }
        if let Some(previous) = self.previous {
            let last = self.data.points[previous].position();
            if (last.cell_id & 0xFFFF_0000) != (p.cell_id & 0xFFFF_0000)
                || distance(&last, p) > 12.0
            {
                self.break_trace();
            } else if last.cell_id == p.cell_id && distance(&last, p) < 3.0 {
                return;
            }
        }
        let next = match self.find(p) {
            Some(known) => known,
            None if self.is_full() => {
                self.note_limit();
                self.break_trace();
                return;
            }
            None => self.add(p),
        };
        if let Some(from) = self.previous {
            let edge = TraversalEdge { from, to: next };
            if from != next && !self.edge_set.contains(&edge) {
                if self.data.edges.len() >= MAX_EDGES {
                    self.note_limit();
                } else {
                    self.push_edge(edge);
                    self.dirty = true;
                }
            }
        }
        self.previous = Some(next);
    }

    /// Records that `portal` (whose destination is known) was found from
    /// where the character stands, under `key`, and saves.
    pub fn remember_exit(
        &mut self,
        host: &mut Host<'_>,
        key: &str,
        portal: &WorldObject,
        snapshot: &NavigationSnapshot,
    ) {
        let Some(destination) = portal
            .portal_destination
            .as_deref()
            .filter(|d| !is_blank(Some(d)))
        else {
            return;
        };
        if !valid(&snapshot.position) || snapshot.position.is_outdoor || snapshot.is_portal_space {
            return;
        }
        self.observe(snapshot);
        let Some(anchor) = self.previous else {
            self.log_notice(host);
            return;
        };
        let exit = TraversalExit {
            landblock: snapshot.position.cell_id & 0xFFFF_0000,
            key: key.to_owned(),
            name: portal.name.clone(),
            destination: destination.to_owned(),
            anchor,
        };
        if !self.exit_set.contains(&exit) {
            if self.data.exits.len() >= MAX_EXITS {
                self.note_limit();
            } else {
                self.push_exit(exit);
                self.dirty = true;
            }
        }
        self.log_notice(host);
        self.save(host);
    }

    /// The shortest recorded walk from `start` to an exit recorded under
    /// `key` in its landblock: it starts at a recorded point in the same
    /// cell within 8 m and follows edges in the direction they were
    /// walked. Empty when there is none.
    #[must_use]
    pub fn find_path(&self, start: &Position, key: &str) -> Vec<Position> {
        if !valid(start) {
            return Vec::new();
        }
        let block = start.cell_id & 0xFFFF_0000;
        let goals: HashSet<usize> = self
            .data
            .exits
            .iter()
            .filter(|e| e.landblock == block && e.key == key)
            .map(|e| e.anchor)
            .collect();
        if goals.is_empty() {
            return Vec::new();
        }
        let first = (0..self.data.points.len())
            .filter(|&i| {
                self.data.points[i].cell_id == start.cell_id
                    && distance(start, &self.data.points[i].position()) <= 8.0
            })
            .min_by(|&a, &b| {
                distance(start, &self.data.points[a].position())
                    .total_cmp(&distance(start, &self.data.points[b].position()))
            });
        let Some(first) = first else {
            return Vec::new();
        };
        let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::new();
        for edge in &self.data.edges {
            adjacency.entry(edge.from).or_default().push(edge.to);
        }
        let mut costs: HashMap<usize, f64> = HashMap::from([(first, 0.0)]);
        let mut parents: HashMap<usize, usize> = HashMap::new();
        let mut queue = BinaryHeap::new();
        let mut order = 0u64;
        queue.push(Queued {
            cost: 0.0,
            order,
            node: first,
        });
        while let Some(Queued { cost, node, .. }) = queue.pop() {
            if cost > costs[&node] {
                continue;
            }
            if goals.contains(&node) {
                let mut path = Vec::new();
                let mut at = node;
                loop {
                    path.push(self.data.points[at].position());
                    if at == first {
                        break;
                    }
                    at = parents[&at];
                }
                path.reverse();
                return path;
            }
            for &to in adjacency.get(&node).map_or(&[][..], Vec::as_slice) {
                let next = cost
                    + distance(
                        &self.data.points[node].position(),
                        &self.data.points[to].position(),
                    );
                if costs.get(&to).is_some_and(|&old| old <= next) {
                    continue;
                }
                costs.insert(to, next);
                parents.insert(to, node);
                order += 1;
                queue.push(Queued {
                    cost: next,
                    order,
                    node: to,
                });
            }
        }
        Vec::new()
    }

    // The first point (lowest index) within 1.5 m in the same cell.
    fn find(&self, p: &Position) -> Option<usize> {
        let (cell, x, y, z) = grid_key(p);
        let mut found: Option<usize> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(indices) = self.grid.get(&(cell, x + dx, y + dy, z + dz)) else {
                        continue;
                    };
                    // Filed in index order: the first match is the cell's
                    // lowest.
                    for &i in indices {
                        if found.is_some_and(|f| f <= i) {
                            break;
                        }
                        if distance(&self.data.points[i].position(), p) <= MERGE_METRES {
                            found = Some(i);
                            break;
                        }
                    }
                }
            }
        }
        found
    }

    fn add(&mut self, p: &Position) -> usize {
        self.data.points.push(TraversalPoint {
            cell_id: p.cell_id,
            east_west: p.east_west,
            north_south: p.north_south,
            elevation: p.elevation,
        });
        let index = self.data.points.len() - 1;
        self.grid.entry(grid_key(p)).or_default().push(index);
        self.dirty = true;
        index
    }

    fn push_edge(&mut self, edge: TraversalEdge) {
        self.edge_set.insert(edge);
        self.data.edges.push(edge);
    }

    fn push_exit(&mut self, exit: TraversalExit) {
        self.exit_set.insert(exit.clone());
        self.data.exits.push(exit);
    }

    // No new point: a file holding more would not load.
    fn is_full(&self) -> bool {
        self.data.points.len() >= MAX_POINTS || self.data.edges.len() >= MAX_EDGES
    }

    fn note_limit(&mut self) {
        if !self.limit_noted {
            self.limit_noted = true;
            self.notice = Some(format!(
                "GoArrow: Learned dungeon paths reached the most a file holds \
                 ({MAX_POINTS} points, {MAX_EDGES} edges, {MAX_EXITS} exits); \
                 new walks are not recorded."
            ));
        }
    }

    fn log_notice(&mut self, host: &mut Host<'_>) {
        if let Some(notice) = self.take_notice() {
            host.warn(&notice);
        }
    }

    // Replaces the data, filing it for find-or-add and the duplicate
    // checks.
    fn set_data(&mut self, data: TraversalData) {
        self.grid.clear();
        for (i, p) in data.points.iter().enumerate() {
            self.grid
                .entry(grid_key(&p.position()))
                .or_default()
                .push(i);
        }
        self.edge_set = data.edges.iter().copied().collect();
        self.exit_set = data.exits.iter().cloned().collect();
        self.data = data;
    }

    /// The learned paths as the file stores them (indented JSON).
    #[must_use]
    pub fn export(&self) -> String {
        serde_json::to_string_pretty(&self.data).unwrap_or_default()
    }

    /// Merges another file's paths: the whole file is checked first, and
    /// a merge that would hold more than a file may is undone, so a bad
    /// file changes nothing. Points within 1.5 m in the same cell are one
    /// point; edges and exits already known are not added again. Saves
    /// afterwards; a failed save leaves the merge in memory with
    /// [`Self::error`] set.
    ///
    /// # Errors
    /// Why the file was refused.
    pub fn import(&mut self, host: &mut Host<'_>, json: &str) -> Result<(), String> {
        let incoming = parse(json)?;
        let before = (
            self.data.points.len(),
            self.data.edges.len(),
            self.data.exits.len(),
            self.dirty,
        );
        let indices: Vec<usize> = incoming
            .points
            .iter()
            .map(|p| {
                let p = p.position();
                self.find(&p).unwrap_or_else(|| self.add(&p))
            })
            .collect();
        for e in &incoming.edges {
            let mapped = TraversalEdge {
                from: indices[e.from],
                to: indices[e.to],
            };
            if mapped.from != mapped.to && !self.edge_set.contains(&mapped) {
                self.push_edge(mapped);
            }
        }
        for e in &incoming.exits {
            let mapped = TraversalExit {
                anchor: indices[e.anchor],
                ..e.clone()
            };
            if !self.exit_set.contains(&mapped) {
                self.push_exit(mapped);
            }
        }
        if self.data.points.len() > MAX_POINTS
            || self.data.edges.len() > MAX_EDGES
            || self.data.exits.len() > MAX_EXITS
        {
            let mut data = std::mem::take(&mut self.data);
            data.points.truncate(before.0);
            data.edges.truncate(before.1);
            data.exits.truncate(before.2);
            self.set_data(data);
            self.dirty = before.3;
            return Err("The merged paths would be more than a file may hold.".to_owned());
        }
        self.dirty = true;
        self.save(host);
        Ok(())
    }
}

/// A Dijkstra queue entry: cheapest first, then first queued.
#[derive(Debug)]
struct Queued {
    cost: f64,
    order: u64,
    node: usize,
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
            .cost
            .total_cmp(&self.cost)
            .then(other.order.cmp(&self.order))
    }
}

/// Reads and checks a whole file.
fn parse(json: &str) -> Result<TraversalData, String> {
    if json.chars().count() > MAX_FILE_CHARS {
        return Err("Traversal file is too large.".to_owned());
    }
    let data: Option<TraversalData> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let data = data.ok_or_else(|| "Empty traversal data.".to_owned())?;
    if data.version != 1
        || data.points.len() > MAX_POINTS
        || data.edges.len() > MAX_EDGES
        || data.exits.len() > MAX_EXITS
    {
        return Err("Unsupported or oversized traversal data.".to_owned());
    }
    let count = data.points.len();
    let block = |i: usize| data.points[i].cell_id & 0xFFFF_0000;
    let invalid = data.points.iter().any(|p| !valid(&p.position()))
        || data.edges.iter().any(|e| {
            e.from >= count
                || e.to >= count
                || block(e.from) != block(e.to)
                || distance(
                    &data.points[e.from].position(),
                    &data.points[e.to].position(),
                ) > 12.0
        })
        || data.exits.iter().any(|e| {
            e.anchor >= count
                || e.key.trim().is_empty()
                || e.destination.trim().is_empty()
                || e.landblock != block(e.anchor)
        });
    if invalid {
        return Err("Invalid traversal points, edges, or exits.".to_owned());
    }
    Ok(data)
}
