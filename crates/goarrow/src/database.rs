//! The location database (`RouteFinding/LocationDatabase.cs`): the loaded
//! places, the player's indoor marks, the portal devices and route starts,
//! and the route corrections; and the Atlas download check
//! (`RouteFinding/WarcryAtlasDataProvider.cs`).
//!
//! Each location load replaces every loaded place (the player's marks are
//! put back); nothing merges, as in GoArrow. Names are looked up ignoring
//! case; a name may have several places, and a lookup gives the first.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use roxmltree::Node;

use crate::ace_portals::AcePortals;
use crate::coords::Coordinates;
use crate::corrections::BlockedStep;
use crate::location::{Location, LocationError};
use crate::text;
use crate::xml;

/// A portal device record (`RouteFinding/PortalDevice.cs`): a device (an
/// item or a portal) that takes you to a destination. Only a record with
/// an entrance location makes a route edge.
#[derive(Debug, Clone, Default)]
pub struct PortalDevice {
    /// Where it takes you.
    pub destination: String,
    /// The device's name.
    pub via: String,
    /// The landmass the destination is on.
    pub landmass: String,
    /// The location where it is used, or empty.
    pub entrance_location: String,
    /// The location you arrive at when it is not the destination, or
    /// empty.
    pub exit_location: String,
}

impl PortalDevice {
    /// A record without entrance or exit.
    #[must_use]
    pub fn new(destination: &str, via: &str, landmass: &str) -> Self {
        Self {
            destination: destination.to_owned(),
            via: via.to_owned(),
            landmass: landmass.to_owned(),
            ..Self::default()
        }
    }

    /// Reads `destination;via;landmass[;entrance[;exit]]`; fewer than three
    /// fields is a destination alone.
    #[must_use]
    pub fn from_csv_line(line: &str) -> Self {
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 3 {
            return Self {
                destination: line.trim().to_owned(),
                ..Self::default()
            };
        }
        let field = |i: usize| fields.get(i).map_or(String::new(), |f| f.trim().to_owned());
        Self {
            destination: field(0),
            via: field(1),
            landmass: field(2),
            entrance_location: field(3),
            exit_location: field(4),
        }
    }
}

/// Equal when the destination and the device are (ignoring case).
impl PartialEq for PortalDevice {
    fn eq(&self, other: &Self) -> bool {
        text::eq_ignore_case(&self.destination, &other.destination)
            && text::eq_ignore_case(&self.via, &other.via)
    }
}

/// `destination;via;landmass`, plus `;entrance;exit` when either is set.
impl fmt::Display for PortalDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{};{};{}", self.destination, self.via, self.landmass)?;
        if !self.entrance_location.is_empty() || !self.exit_location.is_empty() {
            write!(f, ";{};{}", self.entrance_location, self.exit_location)?;
        }
        Ok(())
    }
}

/// A route start record (`RouteFinding/RouteStart.cs`): a known first leg
/// from one named place to another, and how (a recall, a lifestone, a
/// portal or a walk, read from `via`).
#[derive(Debug, Clone, Default)]
pub struct RouteStart {
    /// Where it goes.
    pub destination: String,
    /// Where it starts.
    pub from: String,
    /// How.
    pub via: String,
}

impl RouteStart {
    /// A record.
    #[must_use]
    pub fn new(destination: &str, from: &str, via: &str) -> Self {
        Self {
            destination: destination.to_owned(),
            from: from.to_owned(),
            via: via.to_owned(),
        }
    }

    /// Reads `destination;from;via`; anything else is a destination alone.
    #[must_use]
    pub fn from_csv_line(line: &str) -> Self {
        // `^\s*(?<dest>[^;]+)\s*;\s*(?<from>[^;]+)\s*;\s*(?<via>.*)$`: two
        // non-empty fields, then anything without a line break before the
        // end.
        let mut parts = line.splitn(3, ';');
        if let (Some(destination), Some(from), Some(via)) =
            (parts.next(), parts.next(), parts.next())
        {
            let via_line = via.strip_suffix('\n').unwrap_or(via);
            if !destination.is_empty() && !from.is_empty() && !via_line.contains('\n') {
                return Self::new(destination.trim(), from.trim(), via.trim());
            }
        }
        Self {
            destination: line.trim().to_owned(),
            ..Self::default()
        }
    }
}

/// Equal when the destination and the start are (ignoring case).
impl PartialEq for RouteStart {
    fn eq(&self, other: &Self) -> bool {
        text::eq_ignore_case(&self.destination, &other.destination)
            && text::eq_ignore_case(&self.from, &other.from)
    }
}

/// `destination;from;via`.
impl fmt::Display for RouteStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{};{};{}", self.destination, self.from, self.via)
    }
}

/// Why a data file could not be loaded; the database is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The XML is not well formed (or has a DTD).
    Xml(String),
    /// A record could not be read.
    Location(LocationError),
    /// The file is larger than [crate::loading::WHOLE_DOCUMENT_LIMIT]
    /// and does not split into records, so it is not read in one call.
    Unsplit,
}

/// Why a file over the one-call limit that does not split is refused.
pub const UNSPLIT: &str = "it is over 1 MB and does not split into records (one element each), as Atlas exports and GoArrow's location files do";

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(message) => f.write_str(message),
            Self::Location(error) => write!(f, "{error}"),
            Self::Unsplit => f.write_str(UNSPLIT),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<LocationError> for LoadError {
    fn from(error: LocationError) -> Self {
        Self::Location(error)
    }
}

/// Locations read from a file, before they replace the loaded ones.
#[derive(Debug, Clone, Default)]
pub struct ParsedLocations {
    /// The places with coordinates, in file order.
    pub locations: Vec<Location>,
    /// Whether the file was an Atlas export (root `<atlas>`).
    pub is_atlas: bool,
}

fn is_goarrow_record(node: Node<'_, '_>) -> bool {
    xml::is_named(node, "Location") || xml::is_named(node, "loc")
}

/// Reads a location file: an Atlas export (root `<atlas>`, every
/// `<location>`), else every `<Location>` and `<loc>` element; places
/// without coordinates are dropped. When none is found, the text inside
/// those elements is read as `name; NS; EW` lines. One parse of the whole
/// file; [`crate::loading::LocationsJob`] reads it in steps.
pub fn parse_locations_xml(xml: &str) -> Result<ParsedLocations, LoadError> {
    let doc = xml::parse(xml).map_err(LoadError::Xml)?;
    let is_atlas = doc
        .root_element()
        .tag_name()
        .name()
        .eq_ignore_ascii_case("atlas");
    let mut locations = Vec::new();
    for node in doc.descendants() {
        let location = if is_atlas {
            if !xml::is_named(node, "location") {
                continue;
            }
            Location::from_atlas_element(node)
        } else {
            if !is_goarrow_record(node) {
                continue;
            }
            Location::from_element(node)?
        };
        if location.has_coordinates() {
            locations.push(location);
        }
    }
    if locations.is_empty() {
        for node in doc.descendants().filter(|n| is_goarrow_record(*n)) {
            for line in xml::own_texts(node) {
                let location = Location::from_csv_line(line)?;
                if location.has_coordinates() {
                    locations.push(location);
                }
            }
        }
    }
    Ok(ParsedLocations {
        locations,
        is_atlas,
    })
}

/// The places, devices, route starts and corrections GoArrow plans with.
#[derive(Debug, Default)]
pub struct LocationDatabase {
    locations: Vec<Rc<Location>>,
    by_name: HashMap<String, Vec<Rc<Location>>>,
    user_locations: Vec<Rc<Location>>,
    portal_devices: Vec<PortalDevice>,
    route_starts: Vec<RouteStart>,
    arrival_corrections: HashMap<i32, Coordinates>,
    // The Atlas's own arrival of each Atlas place loaded (none for a place
    // without one), by id: what is used when nothing replaces it.
    listed_arrivals: HashMap<i32, Coordinates>,
    // The portal table's arrivals for Atlas portals, by id.
    table_arrivals: HashMap<i32, Coordinates>,
    // Arrivals seen on the way this session, by id.
    session_arrivals: HashMap<i32, Coordinates>,
    blocked_steps: Vec<BlockedStep>,
    session_blocked_steps: Vec<BlockedStep>,
    atlas_loaded: bool,
}

impl LocationDatabase {
    /// An empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every loaded place, in load order (the player's marks last).
    #[must_use]
    pub fn all_locations(&self) -> &[Rc<Location>] {
        &self.locations
    }

    /// The player's indoor marks.
    #[must_use]
    pub fn user_locations(&self) -> &[Rc<Location>] {
        &self.user_locations
    }

    /// Adds or replaces (by name, ignoring case) a player's mark, which
    /// survives later loads; it replaces any loaded place of that name.
    pub fn upsert_user_location(&mut self, location: Location) -> Rc<Location> {
        let location = Rc::new(location);
        match self
            .user_locations
            .iter_mut()
            .find(|existing| text::eq_ignore_case(&existing.name, &location.name))
        {
            Some(existing) => *existing = location.clone(),
            None => self.user_locations.push(location.clone()),
        }
        self.add_or_replace(location.clone());
        location
    }

    fn add_or_replace(&mut self, location: Rc<Location>) {
        self.locations
            .retain(|existing| !text::eq_ignore_case(&existing.name, &location.name));
        self.by_name
            .insert(text::fold(&location.name), vec![location.clone()]);
        self.locations.push(location);
    }

    /// The portal device records.
    #[must_use]
    pub fn portal_devices(&self) -> &[PortalDevice] {
        &self.portal_devices
    }

    /// The route start records.
    #[must_use]
    pub fn route_starts(&self) -> &[RouteStart] {
        &self.route_starts
    }

    /// The number of loaded places.
    #[must_use]
    pub fn location_count(&self) -> usize {
        self.locations.len()
    }

    /// The steps route searches must not use: the corrections file's, then
    /// this session's.
    pub fn blocked_steps(&self) -> impl Iterator<Item = &BlockedStep> {
        self.blocked_steps
            .iter()
            .chain(self.session_blocked_steps.iter())
    }

    /// Unblocks this session's blocks of steps that leave from an Atlas
    /// location.
    pub fn remove_session_blocked_steps(&mut self, from_id: i32) {
        self.session_blocked_steps
            .retain(|step| step.from_id != from_id);
    }

    /// Blocks a step until the plugin is next loaded; nothing is saved.
    pub fn add_session_blocked_step(&mut self, step: BlockedStep) {
        self.session_blocked_steps.push(step);
    }

    /// Sets the corrections: arrivals by Atlas id replace the Atlas values
    /// in the loaded Atlas data and in Atlas data loaded later (a removed
    /// one gives the place back the arrival it had without it); blocked
    /// steps replace the file's blocks.
    pub fn set_corrections(
        &mut self,
        arrivals: HashMap<i32, Coordinates>,
        blocked_steps: Vec<BlockedStep>,
    ) {
        self.arrival_corrections = arrivals;
        self.blocked_steps = blocked_steps;
        self.apply_arrivals();
    }

    /// Takes the Atlas portals' arrivals from the portal table where it has
    /// the portal ([`AcePortals::atlas_arrivals`]), for the loaded Atlas
    /// data and Atlas data loaded later; the corrections still win.
    /// Returns how many Atlas portals have the table's arrival.
    pub fn use_portal_table(&mut self, table: &AcePortals) -> usize {
        self.table_arrivals = if self.atlas_loaded {
            // The Atlas's own data decides which places are its portals,
            // whatever a correction gave them.
            table.atlas_arrivals(self.locations.iter().filter_map(|location| {
                let listed = self.listed_arrivals.get(&location.id)?;
                (!listed.is_none()).then_some(location.as_ref())
            }))
        } else {
            HashMap::new()
        };
        self.apply_arrivals();
        self.table_arrivals.len()
    }

    /// Whether the loaded places are Atlas data.
    #[must_use]
    pub fn is_atlas_loaded(&self) -> bool {
        self.atlas_loaded
    }

    /// How many loaded Atlas portals have the portal table's arrival.
    #[must_use]
    pub fn table_arrival_count(&self) -> usize {
        self.table_arrivals.len()
    }

    /// The arrival the Atlas itself lists for an Atlas place, if loaded.
    #[must_use]
    pub fn listed_arrival(&self, id: i32) -> Option<Coordinates> {
        self.listed_arrivals.get(&id).copied()
    }

    /// Uses an arrival seen on the way for an Atlas portal until the
    /// plugin is loaded again (over the corrections, the table and the
    /// Atlas); nothing is saved.
    pub fn set_session_arrival(&mut self, id: i32, arrival: Coordinates) {
        self.session_arrivals.insert(id, arrival);
        self.apply_arrivals();
    }

    /// Gives each loaded Atlas place its arrival: one seen this session,
    /// else the corrections', else the portal table's, else the Atlas's.
    fn apply_arrivals(&self) {
        if !self.atlas_loaded {
            return;
        }
        for location in &self.locations {
            let Some(listed) = self.listed_arrivals.get(&location.id) else {
                continue;
            };
            let arrival = self
                .session_arrivals
                .get(&location.id)
                .or_else(|| self.arrival_corrections.get(&location.id))
                .or_else(|| self.table_arrivals.get(&location.id))
                .unwrap_or(listed);
            location.set_exit_coords(*arrival);
        }
    }

    /// Loads a location file ([`parse_locations_xml`]), replacing every
    /// loaded place; returns how many it has.
    pub fn load_locations_xml(&mut self, xml: &str) -> Result<usize, LoadError> {
        let parsed = parse_locations_xml(xml)?;
        Ok(self.replace_locations(parsed))
    }

    /// Replaces every loaded place with parsed ones (Atlas arrival
    /// corrections applied); returns how many there are. The portal
    /// table's arrivals were for the old places: use it again
    /// ([`Self::use_portal_table`]).
    pub fn replace_locations(&mut self, parsed: ParsedLocations) -> usize {
        self.atlas_loaded = parsed.is_atlas;
        self.locations.clear();
        self.by_name.clear();
        self.listed_arrivals.clear();
        self.table_arrivals.clear();
        for location in parsed.locations {
            if parsed.is_atlas && location.id > 0 {
                self.listed_arrivals
                    .entry(location.id)
                    .or_insert_with(|| location.exit_coords());
            }
            let location = Rc::new(location);
            self.by_name
                .entry(text::fold(&location.name))
                .or_default()
                .push(location.clone());
            self.locations.push(location);
        }
        self.apply_arrivals();
        for mark in self.user_locations.clone() {
            self.add_or_replace(mark);
        }
        self.locations.len()
    }

    /// Loads portal devices (`<Device Destination Via Landmass [Entrance|From]
    /// [Exit|To]/>`, else `Device` text as lines), replacing the loaded ones.
    pub fn load_portal_devices_xml(&mut self, xml: &str) -> Result<(), LoadError> {
        let doc = xml::parse(xml).map_err(LoadError::Xml)?;
        let devices: Vec<Node<'_, '_>> = doc
            .descendants()
            .filter(|n| xml::is_named(*n, "Device"))
            .collect();
        let mut loaded: Vec<PortalDevice> = devices
            .iter()
            .map(|node| {
                let attr = |name: &str| node.attribute(name);
                PortalDevice {
                    destination: attr("Destination").unwrap_or("").to_owned(),
                    via: attr("Via").unwrap_or("").to_owned(),
                    landmass: attr("Landmass").unwrap_or("").to_owned(),
                    entrance_location: attr("Entrance")
                        .or_else(|| attr("From"))
                        .unwrap_or("")
                        .to_owned(),
                    exit_location: attr("Exit").or_else(|| attr("To")).unwrap_or("").to_owned(),
                }
            })
            .collect();
        if loaded.is_empty() {
            loaded = devices
                .iter()
                .flat_map(|node| xml::own_texts(*node))
                .map(PortalDevice::from_csv_line)
                .collect();
        }
        self.portal_devices = loaded;
        Ok(())
    }

    /// Loads route starts (`<Start Destination From Via/>`, else `Start`
    /// text as lines), replacing the loaded ones.
    pub fn load_route_starts_xml(&mut self, xml: &str) -> Result<(), LoadError> {
        let doc = xml::parse(xml).map_err(LoadError::Xml)?;
        let starts: Vec<Node<'_, '_>> = doc
            .descendants()
            .filter(|n| xml::is_named(*n, "Start"))
            .collect();
        let mut loaded: Vec<RouteStart> = starts
            .iter()
            .map(|node| {
                let attr = |name: &str| node.attribute(name).unwrap_or("");
                RouteStart::new(attr("Destination"), attr("From"), attr("Via"))
            })
            .collect();
        if loaded.is_empty() {
            loaded = starts
                .iter()
                .flat_map(|node| xml::own_texts(*node))
                .map(RouteStart::from_csv_line)
                .collect();
        }
        self.route_starts = loaded;
        Ok(())
    }

    /// Loads `name; NS; EW` lines (blank lines and `#` comments skipped),
    /// replacing every loaded place; returns how many it has.
    pub fn load_locations_csv<I, S>(&mut self, lines: I) -> Result<usize, LoadError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut locations = Vec::new();
        for line in lines {
            let trimmed = line.as_ref().trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let location = Location::from_csv_line(trimmed)?;
            if location.has_coordinates() {
                locations.push(location);
            }
        }
        Ok(self.replace_locations(ParsedLocations {
            locations,
            is_atlas: false,
        }))
    }

    /// Loads portal device lines, replacing the loaded ones.
    pub fn load_portal_devices_csv<I, S>(&mut self, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.portal_devices = csv_records(lines, PortalDevice::from_csv_line);
    }

    /// Loads route start lines, replacing the loaded ones.
    pub fn load_route_starts_csv<I, S>(&mut self, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.route_starts = csv_records(lines, RouteStart::from_csv_line);
    }

    /// The first place with this name (ignoring case).
    #[must_use]
    pub fn find_location(&self, name: &str) -> Option<Rc<Location>> {
        self.by_name
            .get(&text::fold(name))
            .and_then(|all| all.first().cloned())
    }

    /// Every place with this name (ignoring case).
    #[must_use]
    pub fn find_locations(&self, name: &str) -> Vec<Rc<Location>> {
        self.by_name
            .get(&text::fold(name))
            .cloned()
            .unwrap_or_default()
    }

    /// Every place whose name contains the text (ignoring case), retired
    /// ones too, in load order.
    #[must_use]
    pub fn search_locations(&self, text: &str) -> Vec<Rc<Location>> {
        let needle = text::fold(text);
        self.locations
            .iter()
            .filter(|l| text::fold(&l.name).contains(&needle))
            .cloned()
            .collect()
    }

    /// The devices that go to a destination.
    #[must_use]
    pub fn find_portals_to(&self, destination: &str) -> Vec<&PortalDevice> {
        self.portal_devices
            .iter()
            .filter(|p| text::eq_ignore_case(&p.destination, destination))
            .collect()
    }

    /// The route starts from a place.
    #[must_use]
    pub fn find_routes_from(&self, origin: &str) -> Vec<&RouteStart> {
        self.route_starts
            .iter()
            .filter(|r| text::eq_ignore_case(&r.from, origin))
            .collect()
    }

    /// The route starts to a place.
    #[must_use]
    pub fn find_routes_to(&self, destination: &str) -> Vec<&RouteStart> {
        self.route_starts
            .iter()
            .filter(|r| text::eq_ignore_case(&r.destination, destination))
            .collect()
    }

    /// Removes every place, mark, device and route start (corrections
    /// stay).
    pub fn clear(&mut self) {
        self.locations.clear();
        self.user_locations.clear();
        self.by_name.clear();
        self.listed_arrivals.clear();
        self.table_arrivals.clear();
        self.portal_devices.clear();
        self.route_starts.clear();
    }
}

fn csv_records<I, S, T>(lines: I, read: impl Fn(&str) -> T) -> Vec<T>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    lines
        .into_iter()
        .filter_map(|line| {
            let trimmed = line.as_ref().trim();
            (!trimmed.is_empty() && !trimmed.starts_with('#')).then(|| read(trimmed))
        })
        .collect()
}

/// The largest Atlas download, bytes (and characters in a document).
pub const ATLAS_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Why Atlas data was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtlasError {
    /// The download was empty.
    Empty,
    /// The download was larger than [`ATLAS_MAX_BYTES`].
    TooLarge,
    /// The XML is not well formed, has a DTD, or is too long.
    Xml(String),
    /// The root element is not `<atlas>`.
    NotAtlas,
    /// There is no `/atlas/location`.
    NoLocations,
    /// See [LoadError::Unsplit].
    Unsplit,
}

impl fmt::Display for AtlasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Atlas download was empty."),
            Self::TooLarge => write!(
                f,
                "Atlas download is larger than the {} MiB limit.",
                ATLAS_MAX_BYTES / (1024 * 1024)
            ),
            Self::Xml(message) => f.write_str(message),
            Self::NotAtlas => f.write_str("Atlas XML root element must be <atlas>."),
            Self::NoLocations => f.write_str("Atlas XML contains no location records."),
            Self::Unsplit => f.write_str(UNSPLIT),
        }
    }
}

impl std::error::Error for AtlasError {}

/// Checks Atlas XML: well formed without a DTD, at most
/// [`ATLAS_MAX_BYTES`] characters, root `<atlas>` (any case), at least one
/// `/atlas/location`. Returns how many there are.
/// [`crate::loading::AtlasJob`] checks and reads it in steps.
pub fn validate_atlas_xml(xml: &str) -> Result<usize, AtlasError> {
    if xml.len() > ATLAS_MAX_BYTES && xml.chars().count() > ATLAS_MAX_BYTES {
        return Err(AtlasError::Xml(
            "The input document has exceeded a limit set by MaxCharactersInDocument.".to_owned(),
        ));
    }
    let doc = xml::parse(xml).map_err(AtlasError::Xml)?;
    let root = doc.root_element();
    if !root.tag_name().name().eq_ignore_ascii_case("atlas") {
        return Err(AtlasError::NotAtlas);
    }
    let count = if xml::is_named(root, "atlas") {
        root.children()
            .filter(|c| xml::is_named(*c, "location"))
            .count()
    } else {
        0
    };
    if count == 0 {
        return Err(AtlasError::NoLocations);
    }
    Ok(count)
}

/// Checks a downloaded Atlas body (after an HTTP 200): not empty, at most
/// [`ATLAS_MAX_BYTES`], then [`validate_atlas_xml`] on its UTF-8 text.
/// Returns the text and the location count.
pub fn validate_atlas_download(body: &[u8]) -> Result<(String, usize), AtlasError> {
    if body.is_empty() {
        return Err(AtlasError::Empty);
    }
    if body.len() > ATLAS_MAX_BYTES {
        return Err(AtlasError::TooLarge);
    }
    let text = String::from_utf8_lossy(body).into_owned();
    let count = validate_atlas_xml(&text)?;
    Ok((text, count))
}
