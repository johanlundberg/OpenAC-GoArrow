//! A named place (`RouteFinding/Location.cs`): its id, name, kind, map
//! coordinates, a portal's arrival, a dungeon id, an exact indoor point,
//! notes and route-finding flags; and the four ways GoArrow reads one: the
//! compact `<loc>` element (which it also writes), the `<Location>` element
//! form, a `name; NS; EW` line, and the Atlas export's `<location>`.

use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;
use std::ops::{BitAnd, BitOr};

use roxmltree::Node;

use crate::coords::{Coordinates, METRES_PER_MAP_UNIT, NavPosition};
use crate::text;
use crate::xml;

/// What kind of place a location is: GoArrow's flag set (the same bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct LocationType(pub u32);

impl LocationType {
    /// No kind.
    pub const UNKNOWN: Self = Self(0);
    /// An allegiance hall.
    pub const ALLEGIANCE_HALL: Self = Self(1 << 0);
    /// A bindstone.
    pub const BINDSTONE: Self = Self(1 << 1);
    /// A dungeon entrance.
    pub const DUNGEON: Self = Self(1 << 2);
    /// A landmark.
    pub const LANDMARK: Self = Self(1 << 3);
    /// A lifestone.
    pub const LIFESTONE: Self = Self(1 << 4);
    /// A person.
    pub const NPC: Self = Self(1 << 5);
    /// An outpost.
    pub const OUTPOST: Self = Self(1 << 6);
    /// A portal.
    pub const PORTAL: Self = Self(1 << 7);
    /// A portal hub.
    pub const PORTAL_HUB: Self = Self(1 << 8);
    /// A settlement portal.
    pub const SETTLEMENT_PORTAL: Self = Self(1 << 9);
    /// A village.
    pub const VILLAGE: Self = Self(1 << 10);
    /// A town.
    pub const TOWN: Self = Self(1 << 11);
    /// A town portal.
    pub const TOWN_PORTAL: Self = Self(1 << 12);
    /// An underground portal.
    pub const UNDERGROUND_PORTAL: Self = Self(1 << 13);
    /// A vendor.
    pub const VENDOR: Self = Self(1 << 14);
    /// A wilderness portal.
    pub const WILDERNESS_PORTAL: Self = Self(1 << 15);
    /// Every portal kind.
    pub const ANY_PORTAL: Self = Self(
        Self::PORTAL.0
            | Self::PORTAL_HUB.0
            | Self::SETTLEMENT_PORTAL.0
            | Self::TOWN_PORTAL.0
            | Self::UNDERGROUND_PORTAL.0
            | Self::WILDERNESS_PORTAL.0,
    );
    /// Every bit.
    pub const ANY: Self = Self(u32::MAX);

    /// The flag names, ascending by value; `NPC` is another name for `Npc`.
    const NAMES: [(&'static str, Self); 20] = [
        ("Unknown", Self::UNKNOWN),
        ("AllegianceHall", Self::ALLEGIANCE_HALL),
        ("Bindstone", Self::BINDSTONE),
        ("Dungeon", Self::DUNGEON),
        ("Landmark", Self::LANDMARK),
        ("Lifestone", Self::LIFESTONE),
        ("Npc", Self::NPC),
        ("NPC", Self::NPC),
        ("Outpost", Self::OUTPOST),
        ("Portal", Self::PORTAL),
        ("PortalHub", Self::PORTAL_HUB),
        ("SettlementPortal", Self::SETTLEMENT_PORTAL),
        ("Village", Self::VILLAGE),
        ("Town", Self::TOWN),
        ("TownPortal", Self::TOWN_PORTAL),
        ("UndergroundPortal", Self::UNDERGROUND_PORTAL),
        ("Vendor", Self::VENDOR),
        ("WildernessPortal", Self::WILDERNESS_PORTAL),
        ("AnyPortal", Self::ANY_PORTAL),
        ("Any", Self::ANY),
    ];

    /// Whether any bit is shared.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Reads a kind as GoArrow did: a flag name in any case (`Portal`,
    /// `NPC`, `Town, Portal`, or the number), else one of the Atlas
    /// spellings (`portal hub`, `wilderness portal`, `village` or
    /// `settlement`, ...); anything else is [`LocationType::UNKNOWN`].
    #[must_use]
    pub fn parse(text: Option<&str>) -> Self {
        let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
            return Self::UNKNOWN;
        };
        if let Some(kind) = Self::parse_flag_names(text) {
            return kind;
        }
        match text.trim().to_lowercase().as_str() {
            "allegiance hall" => Self::ALLEGIANCE_HALL,
            "bindstone" => Self::BINDSTONE,
            "dungeon" => Self::DUNGEON,
            "landmark" => Self::LANDMARK,
            "lifestone" => Self::LIFESTONE,
            "npc" => Self::NPC,
            "outpost" => Self::OUTPOST,
            "portal hub" => Self::PORTAL_HUB,
            "settlement portal" => Self::SETTLEMENT_PORTAL,
            "town" => Self::TOWN,
            "town portal" => Self::TOWN_PORTAL,
            "underground portal" => Self::UNDERGROUND_PORTAL,
            "vendor" => Self::VENDOR,
            "village" | "settlement" => Self::VILLAGE,
            "wilderness portal" => Self::WILDERNESS_PORTAL,
            "portal" => Self::PORTAL,
            _ => Self::UNKNOWN,
        }
    }

    /// `Enum.TryParse(text, ignoreCase: true)`: a number, or names joined
    /// by commas.
    fn parse_flag_names(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        let first = trimmed.chars().next()?;
        if first.is_ascii_digit() || first == '+' || first == '-' {
            return trimmed.parse::<u32>().ok().map(Self);
        }
        let mut kind = Self::UNKNOWN;
        for part in trimmed.split(',') {
            let part = part.trim();
            let (_, value) = Self::NAMES
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(part))?;
            kind = kind | *value;
        }
        Some(kind)
    }

    /// The flag names, as .NET writes a flag set (`Portal`, `Portal,
    /// Town`, `AnyPortal`, `Unknown`); a number when a bit has no name.
    #[must_use]
    pub fn name(self) -> String {
        if let Some((name, _)) = Self::NAMES.iter().find(|(_, value)| *value == self) {
            return (*name).to_owned();
        }
        // Largest named value first, as .NET does; written smallest first.
        let mut rest = self.0;
        let mut found = Vec::new();
        for (name, value) in Self::NAMES.iter().rev().filter(|(name, _)| *name != "NPC") {
            if value.0 != 0 && rest & value.0 == value.0 {
                rest -= value.0;
                found.push(*name);
            }
        }
        if rest != 0 {
            return self.0.to_string();
        }
        found.reverse();
        found.join(", ")
    }
}

impl BitOr for LocationType {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for LocationType {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

/// An exact indoor point: the cell and the offset within its landblock,
/// metres, as GoArrow's indoor marks store it (`cellId x y z`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndoorPosition {
    /// The full cell id; its low 16 bits are above `0x40` (a dungeon or
    /// building cell).
    pub cell_id: u32,
    /// East offset within the landblock, metres.
    pub x: f64,
    /// North offset within the landblock, metres.
    pub y: f64,
    /// Height, metres.
    pub z: f64,
}

impl IndoorPosition {
    fn block_x(&self) -> f64 {
        f64::from((self.cell_id >> 24) & 0xFF)
    }

    fn block_y(&self) -> f64 {
        f64::from((self.cell_id >> 16) & 0xFF)
    }

    /// The point at map coordinates (map units) in a cell.
    #[must_use]
    pub fn from_map(cell_id: u32, east_west: f64, north_south: f64, elevation: f64) -> Self {
        let mut p = Self {
            cell_id,
            x: 0.0,
            y: 0.0,
            z: elevation * METRES_PER_MAP_UNIT,
        };
        p.x = east_west * METRES_PER_MAP_UNIT + 84.0 - (p.block_x() - 127.0) * 192.0;
        p.y = north_south * METRES_PER_MAP_UNIT + 84.0 - (p.block_y() - 127.0) * 192.0;
        p
    }

    /// East/west, map units.
    #[must_use]
    pub fn east_west(&self) -> f64 {
        ((self.block_x() - 127.0) * 192.0 + self.x - 84.0) / METRES_PER_MAP_UNIT
    }

    /// North/south, map units.
    #[must_use]
    pub fn north_south(&self) -> f64 {
        ((self.block_y() - 127.0) * 192.0 + self.y - 84.0) / METRES_PER_MAP_UNIT
    }

    /// Height, map units.
    #[must_use]
    pub fn elevation(&self) -> f64 {
        self.z / METRES_PER_MAP_UNIT
    }

    /// The map coordinates.
    #[must_use]
    pub fn coordinates(&self) -> Coordinates {
        Coordinates::new(self.north_south(), self.east_west())
    }

    /// The point as a client position: heading 0, indoors.
    #[must_use]
    pub fn nav_position(&self) -> NavPosition {
        NavPosition {
            cell_id: self.cell_id,
            east_west: self.east_west(),
            north_south: self.north_south(),
            elevation: self.elevation(),
            heading_degrees: 0.0,
            is_outdoor: false,
        }
    }
}

/// Why a location could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationError {
    /// The XML is not well formed (or has a DTD).
    Xml(String),
    /// A `cellId x y z` position is incomplete or out of range; the name
    /// is the location's.
    InvalidIndoorPosition(String),
    /// A `name; NS; EW` line whose numbers do not read.
    InvalidNumber(String),
}

impl fmt::Display for LocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xml(message) => write!(f, "{message}"),
            Self::InvalidIndoorPosition(name) => {
                write!(f, "Location '{name}' has an invalid indoor position.")
            }
            Self::InvalidNumber(text) => write!(f, "'{text}' is not a number."),
        }
    }
}

impl std::error::Error for LocationError {}

/// A named place.
#[derive(Debug, Clone)]
pub struct Location {
    /// The source's id (an Atlas id), or 0.
    pub id: i32,
    /// The name shown and searched.
    pub name: String,
    /// What kind of place it is.
    pub kind: LocationType,
    /// Where it is.
    pub coords: Coordinates,
    exit_coords: Cell<Coordinates>,
    /// The dungeon (landblock) a dungeon entrance leads into, or 0.
    pub dungeon_id: i32,
    /// An exact indoor point, for a place marked inside.
    pub indoor_position: Option<IndoorPosition>,
    /// Description or notes.
    pub notes: String,
    /// Marked as a favourite.
    pub is_favorite: bool,
    /// Made or changed by the player.
    pub is_customized: bool,
    /// The source marked it retired.
    pub is_retired: bool,
    /// Route finding may use it as a graph node.
    pub use_in_route_finding: bool,
    /// An icon id kept from the source.
    pub specialized_icon: i32,
}

impl Location {
    /// A location with a name and no coordinates.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self::at(name, Coordinates::NONE)
    }

    /// A location at map coordinates.
    #[must_use]
    pub fn new(name: impl Into<String>, ns: f64, ew: f64) -> Self {
        Self::at(name, Coordinates::new(ns, ew))
    }

    /// A location at map coordinates.
    #[must_use]
    pub fn at(name: impl Into<String>, coords: Coordinates) -> Self {
        Self::with_details(
            0,
            name,
            LocationType::UNKNOWN,
            coords,
            "",
            0,
            Coordinates::NONE,
        )
    }

    /// A location with every field the sources give.
    #[must_use]
    pub fn with_details(
        id: i32,
        name: impl Into<String>,
        kind: LocationType,
        coords: Coordinates,
        notes: impl Into<String>,
        dungeon_id: i32,
        exit_coords: Coordinates,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            coords,
            exit_coords: Cell::new(exit_coords),
            dungeon_id,
            indoor_position: None,
            notes: notes.into(),
            is_favorite: false,
            is_customized: false,
            is_retired: false,
            use_in_route_finding: true,
            specialized_icon: 0,
        }
    }

    /// Where a portal lets you out, or [`Coordinates::NONE`].
    #[must_use]
    pub fn exit_coords(&self) -> Coordinates {
        self.exit_coords.get()
    }

    /// Replaces the arrival (an arrival correction applies to a loaded
    /// location in place, as in GoArrow).
    pub fn set_exit_coords(&self, exit: Coordinates) {
        self.exit_coords.set(exit);
    }

    /// Whether it has coordinates.
    #[must_use]
    pub fn has_coordinates(&self) -> bool {
        !self.coords.is_none()
    }

    /// Whether it has an arrival.
    #[must_use]
    pub fn has_exit_coords(&self) -> bool {
        !self.exit_coords().is_none()
    }

    /// Whether the id is one of GoArrow's internal (negative) ids.
    #[must_use]
    pub fn is_internal_location(&self) -> bool {
        self.id < 0
    }

    /// The distance to another location, map units.
    #[must_use]
    pub fn distance_to(&self, other: &Self) -> f64 {
        self.coords.distance_to(other.coords)
    }

    /// The direction to another location, radians (0 north, clockwise).
    #[must_use]
    pub fn angle_to(&self, other: &Self) -> f64 {
        self.coords.angle_to(other.coords)
    }

    /// Whether the kind is `kind` or shares a bit with it.
    #[must_use]
    pub fn type_matches(&self, kind: LocationType) -> bool {
        self.kind == kind || self.kind.intersects(kind)
    }

    /// Reads a `name; NS; EW` line. A line without two `;` is a name
    /// alone; numbers that do not read are an error.
    pub fn from_csv_line(line: &str) -> Result<Self, LocationError> {
        let Some((name, ns, ew)) = csv_fields(line) else {
            return Ok(Self::named(line.trim()));
        };
        let number = |t: &str| {
            text::parse_float_strict(t).ok_or_else(|| LocationError::InvalidNumber(t.to_owned()))
        };
        Ok(Self::new(name.trim(), number(ns)?, number(ew)?))
    }

    /// Reads a document whose root element is one location in the compact
    /// (`<loc ... NS= EW=>`) or element (`<Location><Coords/></Location>`)
    /// form.
    pub fn from_xml(xml: &str) -> Result<Self, LocationError> {
        let doc = xml::parse(xml).map_err(LocationError::Xml)?;
        Self::from_element(doc.root_element())
    }

    /// Reads a document whose root element is one Atlas `<location>`.
    pub fn from_atlas_xml(xml: &str) -> Result<Self, LocationError> {
        let doc = xml::parse(xml).map_err(LocationError::Xml)?;
        Ok(Self::from_atlas_element(doc.root_element()))
    }

    /// Reads a compact or element-form record (`Location.FromXml`).
    pub(crate) fn from_element(element: Node<'_, '_>) -> Result<Self, LocationError> {
        let attr = |name: &str| element.attribute(name);
        let compact = attr("NS").is_some() || element.tag_name().name().eq_ignore_ascii_case("loc");
        let id = text::parse_int(attr("id")).unwrap_or(0);
        let name = attr("name")
            .map(str::to_owned)
            .or_else(|| xml::child_text(element, "Name"))
            .unwrap_or_default();
        let type_text = attr("type")
            .map(str::to_owned)
            .or_else(|| xml::child_text(element, "Type"));
        let kind = LocationType::parse(type_text.as_deref());

        let mut coords;
        let mut exit = Coordinates::NONE;
        if compact {
            coords = Coordinates::new(text::parse_float(attr("NS")), text::parse_float(attr("EW")));
            if attr("exitNS").is_some() || attr("exitEW").is_some() {
                exit = Coordinates::new(
                    text::parse_float(attr("exitNS")),
                    text::parse_float(attr("exitEW")),
                );
            }
        } else {
            coords = match xml::child(element, "Coords") {
                Some(c) => Coordinates::new(
                    text::parse_float(c.attribute("NS")),
                    text::parse_float(c.attribute("EW")),
                ),
                None => Coordinates::new(
                    text::parse_float(xml::child_text(element, "NS").as_deref()),
                    text::parse_float(xml::child_text(element, "EW").as_deref()),
                ),
            };
            if let Some(e) = xml::child(element, "ExitCoords") {
                exit = Coordinates::new(
                    text::parse_float(e.attribute("NS")),
                    text::parse_float(e.attribute("EW")),
                );
            }
        }

        let indoor = parse_indoor_position(element)?;
        if let Some(indoor) = indoor
            && (!coords.ns.is_finite() || !coords.ew.is_finite())
        {
            coords = indoor.coordinates();
        }

        let dungeon_text = attr("dungeonId")
            .map(str::to_owned)
            .or_else(|| xml::child_text(element, "DungeonId"));
        let mut location = Self::with_details(
            id,
            name,
            kind,
            coords,
            String::new(),
            text::parse_hex_int(dungeon_text.as_deref()).unwrap_or(0),
            exit,
        );
        location.notes = match attr("notes") {
            Some(notes) => notes.to_owned(),
            None => xml::inner_text(element).trim().to_owned(),
        };
        location.is_customized = text::parse_bool(attr("customized")).unwrap_or(false);
        location.is_favorite = text::parse_bool(attr("favorite")).unwrap_or(false);
        location.is_retired = text::parse_bool(attr("retired")).unwrap_or(false);
        location.use_in_route_finding =
            text::parse_bool(attr("use").or_else(|| attr("useInRouteFinding"))).unwrap_or(true);
        location.specialized_icon = text::parse_hex_int(attr("icon")).unwrap_or(0);
        location.indoor_position = indoor;
        Ok(location)
    }

    /// Reads an Atlas `<location>` (`Location.FromXmlWarcry`). Atlas
    /// latitude is positive south, so north/south is its negative; an
    /// arrival of (0, 0) is none.
    pub(crate) fn from_atlas_element(element: Node<'_, '_>) -> Self {
        let child = |name: &str| xml::child_text(element, name);
        let latitude = text::parse_float(child("latitude").as_deref());
        let longitude = text::parse_float(child("longitude").as_deref());
        let arrival_ns = text::parse_float(child("arrival_latitude").as_deref());
        let arrival_ew = text::parse_float(child("arrival_longitude").as_deref());
        let exit = if arrival_ns.is_finite()
            && arrival_ew.is_finite()
            && (arrival_ns != 0.0 || arrival_ew != 0.0)
        {
            Coordinates::new(-arrival_ns, arrival_ew)
        } else {
            Coordinates::NONE
        };
        let mut location = Self::with_details(
            text::parse_int(child("id").as_deref()).unwrap_or(0),
            child("name").unwrap_or_default(),
            LocationType::parse(child("type").as_deref()),
            Coordinates::new(-latitude, longitude),
            child("description").unwrap_or_default(),
            text::parse_hex_int(child("dungeon_id").as_deref()).unwrap_or(0),
            exit,
        );
        location.is_retired = child("retired").is_some_and(|r| r.eq_ignore_ascii_case("Y"));
        location.use_in_route_finding = !location.is_retired;
        location
    }

    /// The compact `<loc>` element with every field.
    #[must_use]
    pub fn to_xml(&self) -> String {
        let mut out = String::from("<loc");
        let mut attribute = |name: &str, value: &str| {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            xml::escape_attribute(value, &mut out);
            out.push('"');
        };
        attribute("id", &self.id.to_string());
        attribute("name", &self.name);
        attribute("type", &self.kind.name());
        attribute("NS", &text::format_roundtrip(self.coords.ns));
        attribute("EW", &text::format_roundtrip(self.coords.ew));
        if self.has_exit_coords() {
            let exit = self.exit_coords();
            attribute("exitNS", &text::format_roundtrip(exit.ns));
            attribute("exitEW", &text::format_roundtrip(exit.ew));
        }
        if self.dungeon_id != 0 {
            // Hexadecimal, as the reader reads it (DIVERGENCES.md, GA-1).
            attribute("dungeonId", &format!("{:X}", self.dungeon_id));
        }
        if let Some(indoor) = self.indoor_position {
            attribute("cellId", &format!("0x{:08X}", indoor.cell_id));
            attribute("x", &text::format_roundtrip(indoor.x));
            attribute("y", &text::format_roundtrip(indoor.y));
            attribute("z", &text::format_roundtrip(indoor.z));
        }
        if !self.use_in_route_finding {
            attribute("use", "False");
        }
        if self.is_customized {
            attribute("customized", "True");
        }
        if self.is_favorite {
            attribute("favorite", "True");
        }
        if self.is_retired {
            attribute("retired", "True");
        }
        if self.specialized_icon != 0 {
            attribute("icon", &format!("{:08X}", self.specialized_icon));
        }
        out.push('>');
        xml::escape_text(&self.notes, &mut out);
        out.push_str("</loc>");
        out
    }

    /// GoArrow's order: by name ignoring case, then id.
    #[must_use]
    pub fn cmp_by_name(&self, other: &Self) -> Ordering {
        text::cmp_ignore_case(&self.name, &other.name).then(self.id.cmp(&other.id))
    }
}

/// `Location.ParseIndoorPosition`: `cellId x y z` on the element or its
/// `<Position>` child; none without a cell id.
fn parse_indoor_position(element: Node<'_, '_>) -> Result<Option<IndoorPosition>, LocationError> {
    let source = xml::child(element, "Position").unwrap_or(element);
    let Some(cell_text) = source.attribute("cellId").filter(|t| !t.trim().is_empty()) else {
        return Ok(None);
    };
    let hex = if cell_text.len() >= 2 && cell_text[..2].eq_ignore_ascii_case("0x") {
        &cell_text[2..]
    } else {
        cell_text
    };
    let invalid =
        || LocationError::InvalidIndoorPosition(element.attribute("name").unwrap_or("").to_owned());
    let cell_id = text::parse_hex_u32(hex)
        .filter(|c| c & 0xFFFF > 0x40)
        .ok_or_else(invalid)?;
    let read =
        |name: &str| Some(text::parse_float(source.attribute(name))).filter(|v| v.is_finite());
    let (Some(x), Some(y), Some(z)) = (read("x"), read("y"), read("z")) else {
        return Err(invalid());
    };
    Ok(Some(IndoorPosition { cell_id, x, y, z }))
}

/// The fields of `^\s*(?<name>[^;]*);\s*(?<NS>.+?)\s*;\s*(?<EW>.+?)\s*$`,
/// matched as the regex matches them (`.` is anything but a line feed).
fn csv_fields(line: &str) -> Option<(&str, &str, &str)> {
    let semicolon = line.find(';')?;
    let name = &line[..semicolon];
    let rest = &line[semicolon + 1..];
    let chars: Vec<(usize, char)> = rest.char_indices().collect();
    let offset = |i: usize| chars.get(i).map_or(rest.len(), |&(o, _)| o);
    let lead = chars.iter().take_while(|(_, c)| c.is_whitespace()).count();
    for ns_start in (0..=lead).rev() {
        for ns_end in ns_start + 1..=chars.len() {
            if chars[ns_end - 1].1 == '\n' {
                break;
            }
            let mut k = ns_end;
            while k < chars.len() && chars[k].1.is_whitespace() {
                k += 1;
            }
            if k < chars.len()
                && chars[k].1 == ';'
                && let Some(ew) = csv_last_field(&rest[offset(k + 1)..])
            {
                return Some((name, &rest[offset(ns_start)..offset(ns_end)], ew));
            }
        }
    }
    None
}

/// `\s*(?<EW>.+?)\s*$` on what follows the second `;`.
fn csv_last_field(rest: &str) -> Option<&str> {
    let chars: Vec<(usize, char)> = rest.char_indices().collect();
    let offset = |i: usize| chars.get(i).map_or(rest.len(), |&(o, _)| o);
    let lead = chars.iter().take_while(|(_, c)| c.is_whitespace()).count();
    for start in (0..=lead).rev() {
        for end in start + 1..=chars.len() {
            if chars[end - 1].1 == '\n' {
                break;
            }
            if chars[end..].iter().all(|(_, c)| c.is_whitespace()) {
                return Some(&rest[offset(start)..offset(end)]);
            }
        }
    }
    None
}

/// The same place: equal ids when both have one, else the same name
/// (ignoring case) and coordinates.
impl PartialEq for Location {
    fn eq(&self, other: &Self) -> bool {
        if self.id != 0 && other.id != 0 {
            return self.id == other.id;
        }
        text::eq_ignore_case(&self.name, &other.name) && self.coords == other.coords
    }
}

/// `Name [42.1N, 33.6E]`, or the name alone without coordinates.
impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.has_coordinates() {
            write!(f, "{} [{}]", self.name, self.coords)
        } else {
            f.write_str(&self.name)
        }
    }
}
