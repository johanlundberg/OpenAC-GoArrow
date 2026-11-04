//! The portal table: every portal in the world, where it stands and where
//! it leads, read at run time from `portals/ace-portals.json` beside the
//! plugin (extracted from the ACE world database by
//! `tools/ace-portals.ps1`; a separate work under its own licence, never
//! compiled in).
//!
//! The game's files hold a dungeon's rooms but not its portals: those are
//! server objects the client hears of only as their rooms come into view.
//! With the table the navigator knows a dungeon's exits before it sees
//! them, and a live portal's destination without asking for it.
//!
//! Positions are a cell and landblock-local metres, as the table has them;
//! [`AcePortal::position`] turns them into the client's map coordinates.

use std::collections::HashMap;

use openac_plugin::Host;
use openac_plugin::world::{Position, WorldObject};
use serde::Deserialize;

use crate::coords::Coordinates;
use crate::location::{Location, LocationType};
use crate::portal_match::{is_portal, loose_name, portal_destination_name};
use crate::text;

/// The table's resource id: `portals/ace-portals.json` in the plugin's folder.
pub const RESOURCE: &str = "portals/ace-portals.json";
/// The table format this reads.
pub const FORMAT: u32 = 1;
/// How close a live portal stands to a table entry to be that entry,
/// metres.
pub const MATCH_METRES: f64 = 1.0;

/// How a portal comes to be in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalSource {
    /// Placed in the world.
    Placed,
    /// Placed, but present only while the generator it is linked to has
    /// spawned it.
    GeneratorLink,
    /// Spawned by a generator at a fixed place.
    Generator,
}

/// One portal of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct AcePortal {
    /// The placed instance's object id (none for a generator's spawn,
    /// whose id is new each time).
    pub guid: Option<u32>,
    /// Its weenie class.
    pub weenie: u32,
    /// Its name, as the client shows it.
    pub name: String,
    /// Its cell.
    pub cell_id: u32,
    /// Landblock-local metres: east, north, height.
    pub local: [f32; 3],
    /// Its heading, degrees clockwise from north.
    pub heading_degrees: f32,
    /// Where it leads: the cell and landblock-local metres.
    pub destination: Option<(u32, [f32; 3])>,
    /// The lowest level it lets through.
    pub min_level: Option<i32>,
    /// The highest level it lets through.
    pub max_level: Option<i32>,
    /// Its restrictions (who may pass), as the server's bits.
    pub restrictions: Option<u32>,
    /// It has nothing to use: it takes whoever walks into it.
    pub walk_on: bool,
    /// Only admins see it: the client is never told of it.
    pub admin_only: bool,
    /// How it comes to be in the world.
    pub source: PortalSource,
}

impl AcePortal {
    /// Where it stands, as the client reports positions.
    #[must_use]
    pub fn position(&self) -> Position {
        Position::from_cell(self.cell_id, self.local, self.heading_degrees)
    }

    /// Where it leads, as the client reports positions.
    #[must_use]
    pub fn destination_position(&self) -> Option<Position> {
        self.destination
            .map(|(cell, local)| Position::from_cell(cell, local, 0.0))
    }

    /// It leads outdoors (to a landscape cell).
    #[must_use]
    pub fn leads_outdoors(&self) -> bool {
        self.destination
            .is_some_and(|(cell, _)| cell & 0xFFFF != 0 && cell & 0xFFFF < 0x100)
    }

    /// The map coordinates it leads to, when it leads outdoors.
    #[must_use]
    pub fn arrival(&self) -> Option<Coordinates> {
        if !self.leads_outdoors() {
            return None;
        }
        self.destination_position()
            .map(|p| Coordinates::new(p.north_south, p.east_west))
    }

    /// The destination an appraisal of it gives, as the server writes it:
    /// its name, then the map coordinates it leads to when it leads
    /// outdoors (`Surface Portal (12.34N, 45.67E).`). None when it leads
    /// nowhere known.
    #[must_use]
    pub fn destination_label(&self) -> Option<String> {
        self.destination?;
        Some(match self.arrival() {
            Some(arrival) => format!(
                "{} ({}).",
                self.name,
                arrival.to_text_with_decimals(2, false)
            ),
            None => self.name.clone(),
        })
    }

    /// A portal the player can see and use: not admin-only, not walked
    /// into.
    #[must_use]
    pub fn is_usable(&self) -> bool {
        !self.walk_on && !self.admin_only
    }

    /// Whether a live object is this portal: the same name, standing
    /// within [`MATCH_METRES`] in the same landblock.
    #[must_use]
    pub fn is_object(&self, object: &WorldObject) -> bool {
        let Some(position) = object.position.as_ref() else {
            return false;
        };
        position.cell_id >> 16 == self.cell_id >> 16
            && text::eq_ignore_case(&object.name, &self.name)
            && distance_metres(position, &self.position()) <= MATCH_METRES
    }
}

/// The straight-line distance between two positions, metres.
#[must_use]
pub fn distance_metres(a: &Position, b: &Position) -> f64 {
    let ground = a.horizontal_distance_metres(b);
    let height = (a.elevation - b.elevation) * crate::coords::METRES_PER_MAP_UNIT;
    (ground * ground + height * height).sqrt()
}

/// The table, by landblock.
#[derive(Debug, Clone, Default)]
pub struct AcePortals {
    by_landblock: HashMap<u16, Vec<AcePortal>>,
    count: usize,
    source: String,
}

// The file's records (short keys: see the file's own "keys").
#[derive(Deserialize)]
struct FileRecord {
    g: Option<u32>,
    w: u32,
    n: String,
    c: u32,
    p: [f32; 3],
    h: Option<f32>,
    d: Option<u32>,
    dp: Option<[f32; 3]>,
    lmin: Option<i32>,
    lmax: Option<i32>,
    pb: Option<u32>,
    walk: Option<u8>,
    inv: Option<u8>,
    src: Option<String>,
}

#[derive(Deserialize)]
struct FileVersion {
    base: String,
    patch: String,
}

#[derive(Deserialize)]
struct File {
    format: u32,
    version: Option<FileVersion>,
    extracted: Option<String>,
    portals: Vec<FileRecord>,
}

impl AcePortals {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The table a file holds.
    ///
    /// # Errors
    /// Malformed JSON, or another format.
    pub fn parse(json: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if file.format != FORMAT {
            return Err(format!(
                "format {} (this GoArrow reads {FORMAT})",
                file.format
            ));
        }
        let mut table = Self {
            source: match (&file.version, &file.extracted) {
                (Some(v), Some(date)) => {
                    format!("ACE world {} {}, extracted {date}", v.base, v.patch)
                }
                (Some(v), None) => format!("ACE world {} {}", v.base, v.patch),
                _ => "ACE world".to_owned(),
            },
            ..Self::default()
        };
        for r in file.portals {
            let portal = AcePortal {
                guid: r.g,
                weenie: r.w,
                name: r.n,
                cell_id: r.c,
                local: r.p,
                heading_degrees: r.h.unwrap_or(0.0),
                destination: r.d.zip(r.dp),
                min_level: r.lmin,
                max_level: r.lmax,
                restrictions: r.pb,
                walk_on: r.walk.is_some_and(|v| v != 0),
                admin_only: r.inv.is_some_and(|v| v != 0),
                source: match r.src.as_deref() {
                    Some("g") => PortalSource::Generator,
                    Some("l") => PortalSource::GeneratorLink,
                    _ => PortalSource::Placed,
                },
            };
            table.insert(portal);
        }
        Ok(table)
    }

    /// Adds a portal.
    pub fn insert(&mut self, portal: AcePortal) {
        self.by_landblock
            .entry((portal.cell_id >> 16) as u16)
            .or_default()
            .push(portal);
        self.count += 1;
    }

    /// Reads the table shipped beside the plugin ([`RESOURCE`]), saying in
    /// one log line what it read. No file is an empty table: the
    /// navigator then explores as before.
    pub fn load(host: &mut Host<'_>) -> Self {
        match host.resource_read(RESOURCE) {
            Ok(Some(bytes)) => match String::from_utf8(bytes)
                .map_err(|e| e.to_string())
                .and_then(|json| Self::parse(&json))
            {
                Ok(table) => {
                    host.info(&format!(
                        "GoArrow: portal table: {} portals ({}).",
                        table.len(),
                        table.source
                    ));
                    table
                }
                Err(e) => {
                    host.warn(&format!(
                        "GoArrow: the portal table ({RESOURCE}) could not be read: {e}"
                    ));
                    Self::new()
                }
            },
            Ok(None) => {
                host.info(&format!(
                    "GoArrow: no portal table ({RESOURCE}); dungeon exits are found by exploring."
                ));
                Self::new()
            }
            Err(e) => {
                host.warn(&format!(
                    "GoArrow: the portal table ({RESOURCE}) could not be read: {e}"
                ));
                Self::new()
            }
        }
    }

    /// How many portals it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Where it came from (`ACE world v0.8.8 v0.9.296, extracted ...`).
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The portals in a cell's landblock.
    #[must_use]
    pub fn in_landblock(&self, cell_id: u32) -> &[AcePortal] {
        self.by_landblock
            .get(&((cell_id >> 16) as u16))
            .map_or(&[], Vec::as_slice)
    }

    /// The portals in a cell's landblock a player can use to leave for
    /// the landscape: those that lead outdoors and are seen and used.
    pub fn exits_outdoors(&self, cell_id: u32) -> impl Iterator<Item = &AcePortal> {
        self.in_landblock(cell_id)
            .iter()
            .filter(|p| p.leads_outdoors() && p.is_usable())
    }

    /// The table's entry for a live portal: same landblock and name,
    /// within [`MATCH_METRES`] (the nearest when several are).
    #[must_use]
    pub fn find_object(&self, object: &WorldObject) -> Option<&AcePortal> {
        let position = object.position.as_ref()?;
        self.in_landblock(position.cell_id)
            .iter()
            .filter(|p| p.is_object(object))
            .min_by(|a, b| {
                distance_metres(position, &a.position())
                    .total_cmp(&distance_metres(position, &b.position()))
            })
    }

    /// The destination of a live portal whose destination the client does
    /// not know yet, from the table: the label an appraisal would give.
    #[must_use]
    pub fn known_destination(&self, object: &WorldObject) -> Option<String> {
        if !is_portal(object)
            || object
                .portal_destination
                .as_deref()
                .is_some_and(|d| !d.trim().is_empty())
        {
            return None;
        }
        self.find_object(object)?.destination_label()
    }

    /// Fills in the destinations the client does not know yet of the live
    /// portals the table has; true when it filled any.
    pub fn fill_destinations(&self, objects: &mut [WorldObject]) -> bool {
        if self.is_empty() {
            return false;
        }
        let mut filled = false;
        for object in objects {
            if let Some(label) = self.known_destination(object) {
                object.portal_destination = Some(label);
                filled = true;
            }
        }
        filled
    }
}

// ---- Atlas portals' arrivals ----

/// How far an Atlas portal's entrance may stand from the table's portal it
/// describes, map units (48 m).
pub const ATLAS_MATCH_DISTANCE: f64 = 0.2;

/// The indoor hubs whose Atlas portals all share one made-up entrance
/// point: an Atlas name beginning so, and the landblock the hub's portals
/// stand in. They are matched by the place they lead to.
pub const HUBS: [(&str, u32); 2] = [
    (crate::route::HUBS[0].0, 0x0007_0000),
    (crate::route::HUBS[1].0, 0x8A02_0000),
];

/// What the table says of an Atlas portal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AtlasMatch {
    /// No table portal is the one it describes (none named for its place
    /// stands where it does).
    None,
    /// Several are, and they lead to different cells.
    Ambiguous,
    /// It is a table portal leading indoors: its arrival has no map
    /// coordinates to replace the Atlas's with.
    Indoors,
    /// It is a table portal leading outdoors, to these map coordinates.
    Arrival(Coordinates),
}

impl AcePortals {
    /// The table portal an Atlas portal describes, and where it leads.
    ///
    /// It is a usable table portal with a destination, named for the
    /// place the Atlas name leads to ([`destination_key`]), standing
    /// within [`ATLAS_MATCH_DISTANCE`] of the Atlas entrance; or, for an
    /// indoor hub's portal ([`HUBS`]), whose Atlas entrance is a made-up
    /// point, standing in the hub. Several must lead to one cell.
    ///
    /// The nearest portal alone is not enough: where a portal is gone,
    /// the table has its walk-in remains and the nearest usable one is
    /// another (the Atlas's `Hidden Valley to Mayoi` would arrive 70 map
    /// units off).
    #[must_use]
    pub fn match_atlas_portal(&self, location: &Location) -> AtlasMatch {
        let wanted = destination_key(&location.name);
        if wanted.is_empty() {
            return AtlasMatch::None;
        }
        let named = |p: &&AcePortal| destination_key(&p.name) == wanted;
        let mut chosen: Vec<&AcePortal> = self
            .candidates_near(location.coords)
            .filter(|p| map_distance(p, location.coords) <= ATLAS_MATCH_DISTANCE)
            .filter(named)
            .collect();
        if chosen.is_empty()
            && let Some(&(_, hub)) = HUBS
                .iter()
                .find(|(prefix, _)| text::starts_with_ignore_case(&location.name, prefix))
        {
            chosen = self
                .in_landblock(hub)
                .iter()
                .filter(|p| is_candidate(p))
                .filter(named)
                .collect();
        }
        let Some(first) = chosen.first() else {
            return AtlasMatch::None;
        };
        let cell = first.destination.map(|(cell, _)| cell);
        if chosen.iter().any(|p| p.destination.map(|(c, _)| c) != cell) {
            return AtlasMatch::Ambiguous;
        }
        first
            .arrival()
            .map_or(AtlasMatch::Indoors, AtlasMatch::Arrival)
    }

    /// The usable table portals with a destination in the landblocks
    /// within [`ATLAS_MATCH_DISTANCE`] of map coordinates.
    fn candidates_near(&self, at: Coordinates) -> impl Iterator<Item = &AcePortal> {
        let reach = ATLAS_MATCH_DISTANCE * crate::coords::METRES_PER_MAP_UNIT;
        let block = |map: f64, offset: f64| {
            ((map * crate::coords::METRES_PER_MAP_UNIT + 84.0 + offset) / 192.0).floor() + 127.0
        };
        let mut landblocks = Vec::new();
        for x in [block(at.ew, -reach), block(at.ew, reach)] {
            for y in [block(at.ns, -reach), block(at.ns, reach)] {
                if (0.0..=255.0).contains(&x) && (0.0..=255.0).contains(&y) {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let id = ((x as u32) << 24) | ((y as u32) << 16);
                    if !landblocks.contains(&id) {
                        landblocks.push(id);
                    }
                }
            }
        }
        landblocks
            .into_iter()
            .flat_map(|id| self.in_landblock(id).iter())
            .filter(|p| is_candidate(p))
    }

    /// The table's arrivals for the Atlas portals (those with an id and an
    /// arrival of their own), by Atlas id: every one [`Self::match_atlas_portal`]
    /// finds leading outdoors.
    #[must_use]
    pub fn atlas_arrivals<'a>(
        &self,
        locations: impl IntoIterator<Item = &'a Location>,
    ) -> HashMap<i32, Coordinates> {
        let mut arrivals = HashMap::new();
        for location in locations {
            if location.id > 0
                && location.has_exit_coords()
                && location.kind.intersects(LocationType::ANY_PORTAL)
                && let AtlasMatch::Arrival(arrival) = self.match_atlas_portal(location)
            {
                arrivals.insert(location.id, arrival);
            }
        }
        arrivals
    }
}

/// A table portal an Atlas portal can be: seen and used, leading
/// somewhere.
fn is_candidate(portal: &AcePortal) -> bool {
    portal.is_usable() && portal.destination.is_some()
}

/// How far a table portal stands from map coordinates across the ground,
/// map units.
fn map_distance(portal: &AcePortal, at: Coordinates) -> f64 {
    let p = portal.position();
    Coordinates::new(p.north_south, p.east_west).distance_to(at)
}

/// The place a portal name leads to, as a key that compares Atlas and
/// table names: [`portal_destination_name`]'s rules, then what follows
/// the last `to ` of `"{from} to {place}"` (`Crater to Neydisa Village
/// Portal`, `Facility Hub (E)(L 2 (25) C C)to Eastham`), without leading
/// `(...)` groups (`(N W) Abandoned Tumerok Site`) or [`loose_name`]'s
/// parts, in lower case with only letters and digits (`Celdiseths` and
/// `Celdiseth's`, `Valley  Portal` and `Valley Portal` compare equal).
#[must_use]
pub fn destination_key(name: &str) -> String {
    let mut place = portal_destination_name(name);
    if let Some(to) = last_to(&place) {
        place = portal_destination_name(&place[to..]);
    }
    let mut rest = place.trim();
    while rest.starts_with('(')
        && let Some(close) = rest.find(')')
    {
        rest = rest[close + 1..].trim_start();
    }
    loose_name(rest)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Where the place begins after the last `to ` that follows a space or a
/// `)`, if any.
fn last_to(name: &str) -> Option<usize> {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut found = None;
    let mut from = 0;
    while let Some(i) = lower[from..].find("to ") {
        let at = from + i;
        if at > 0 && matches!(bytes[at - 1], b' ' | b')') {
            found = Some(at + "to ".len());
        }
        from = at + 1;
    }
    found
}
