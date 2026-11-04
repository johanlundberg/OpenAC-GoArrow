//! Locations and their formats, from OpenAC-GoArrow's `LocationTests` and
//! `LocationMetadataTests`.

use goarrow::coords::Coordinates;
use goarrow::location::{IndoorPosition, Location, LocationError, LocationType};

// LocationTests.Location_Create_StoresValues
#[test]
fn create_stores_values() {
    let loc = Location::new("Holtburg", 42.1, 33.6);
    assert_eq!(loc.name, "Holtburg");
    assert_eq!(loc.coords.ns, 42.1);
    assert_eq!(loc.coords.ew, 33.6);
    assert!(loc.use_in_route_finding);
    assert!(!loc.has_exit_coords());
}

// LocationTests.Location_FromCsvLine_ParsesCorrectly
#[test]
fn csv_line() {
    let loc = Location::from_csv_line("Holtburg;42.1;33.6").unwrap();
    assert_eq!(loc.name, "Holtburg");
    assert_eq!((loc.coords.ns, loc.coords.ew), (42.1, 33.6));
    let loc = Location::from_csv_line("  Yaraq ;  -21.5 ; -1.8  ").unwrap();
    assert_eq!(loc.name, "Yaraq");
    assert_eq!((loc.coords.ns, loc.coords.ew), (-21.5, -1.8));
}

// LocationTests.Location_FromCsvLine_NoCoords_ParsesName
#[test]
fn csv_line_without_coordinates_is_a_name() {
    let loc = Location::from_csv_line("Hello World").unwrap();
    assert_eq!(loc.name, "Hello World");
    assert!(!loc.has_coordinates());
}

// Location.FromCsvLine: double.Parse throws on a number it cannot read.
#[test]
fn csv_line_with_bad_number_is_an_error() {
    assert!(matches!(
        Location::from_csv_line("Town; north; 3"),
        Err(LocationError::InvalidNumber(_))
    ));
}

// LocationTests.Location_HasCoordinates_Valid / _NaN
#[test]
fn has_coordinates() {
    assert!(Location::new("Test", 10.0, 20.0).has_coordinates());
    assert!(!Location::named("Test").has_coordinates());
}

// LocationTests.Location_DistanceTo_ComputesCorrectly
#[test]
fn distance() {
    let d = Location::new("A", 0.0, 0.0).distance_to(&Location::new("B", 3.0, 4.0));
    assert!((d - 5.0).abs() < 1e-10);
}

// LocationTests.Location_ToXml_ProducesValidXml
#[test]
fn to_xml_has_the_values() {
    let xml = Location::new("Test", 10.0, 20.0).to_xml();
    assert!(xml.contains("Test") && xml.contains("10") && xml.contains("20"));
    assert_eq!(
        xml,
        r#"<loc id="0" name="Test" type="Unknown" NS="10" EW="20"></loc>"#
    );
}

// LocationTests.Location_Equality_SameNameAndCoords / CaseInsensitive / DifferentCoords
#[test]
fn equality() {
    assert_eq!(
        Location::new("Test", 10.0, 20.0),
        Location::new("Test", 10.0, 20.0)
    );
    assert_eq!(
        Location::new("Test", 10.0, 20.0),
        Location::new("test", 10.0, 20.0)
    );
    assert_ne!(
        Location::new("Test", 10.0, 20.0),
        Location::new("Test", 30.0, 40.0)
    );
    // Ids decide when both have one.
    let a = Location::with_details(
        5,
        "A",
        LocationType::TOWN,
        Coordinates::new(1.0, 1.0),
        "",
        0,
        Coordinates::NONE,
    );
    let b = Location::with_details(
        5,
        "B",
        LocationType::TOWN,
        Coordinates::new(9.0, 9.0),
        "",
        0,
        Coordinates::NONE,
    );
    assert_eq!(a, b);
}

// LocationMetadataTests.IndoorCellPositionRoundTripsInNamedLocationXml
#[test]
fn indoor_cell_position_round_trips() {
    let location = Location::from_xml(
        r#"<loc name="Lower Chamber" type="Dungeon" cellId="0x12340122" x="35" y="50" z="-6">A note</loc>"#,
    )
    .unwrap();
    let indoor = location.indoor_position.expect("indoor position");
    assert_eq!(indoor.cell_id, 0x1234_0122);
    assert_eq!(location.notes, "A note");
    // The map coordinates come from the cell and the offset.
    let expected_ew = ((0x12 as f64 - 127.0) * 192.0 + 35.0 - 84.0) / 240.0;
    let expected_ns = ((0x34 as f64 - 127.0) * 192.0 + 50.0 - 84.0) / 240.0;
    assert_eq!(location.coords, Coordinates::new(expected_ns, expected_ew));
    assert_eq!(indoor.elevation(), -6.0 / 240.0);

    let copy = Location::from_xml(&location.to_xml()).unwrap();
    assert_eq!(copy.indoor_position, location.indoor_position);
    assert_eq!(copy.notes, "A note");
}

// Location.ParseIndoorPosition: an outdoor cell or a missing offset is an
// error, not a silently dropped point.
#[test]
fn invalid_indoor_position_is_an_error() {
    for xml in [
        r#"<loc name="Out" cellId="0x12340022" x="1" y="1" z="1"/>"#,
        r#"<loc name="Half" cellId="12340122" x="1" y="1"/>"#,
        r#"<loc name="Bad" cellId="zz" x="1" y="1" z="1"/>"#,
    ] {
        assert!(
            matches!(
                Location::from_xml(xml),
                Err(LocationError::InvalidIndoorPosition(_))
            ),
            "{xml}"
        );
    }
    let loc = Location::from_xml(
        r#"<loc name="P"><Position cellId="12340122" x="1" y="2" z="3"/></loc>"#,
    )
    .unwrap();
    assert_eq!(
        loc.indoor_position.map(|p| (p.x, p.y, p.z)),
        Some((1.0, 2.0, 3.0))
    );
}

#[test]
fn indoor_position_from_map_inverts_the_conversion() {
    let p = IndoorPosition::from_map(0x7F7F_0100, 0.0, 0.0, 0.1);
    assert!((p.x - 84.0).abs() < 1e-9 && (p.y - 84.0).abs() < 1e-9 && (p.z - 24.0).abs() < 1e-9);
    let nav = p.nav_position();
    assert_eq!(nav.cell_id, 0x7F7F_0100);
    assert!(!nav.is_outdoor);
    assert_eq!(nav.heading_degrees, 0.0);
}

// LocationMetadataTests.CompactXml_PreservesLocationMetadata
#[test]
fn compact_xml_preserves_metadata() {
    let location = Location::from_xml(
        "<loc id='123' name='Dungeon Entrance' type='Dungeon' NS='10.5' EW='20.5' exitNS='11.5' exitEW='21.5' \
         dungeonId='ABCD' use='false' retired='true' customized='true' icon='00FF00FF'>A useful description</loc>",
    )
    .unwrap();
    assert_eq!(location.id, 123);
    assert_eq!(location.name, "Dungeon Entrance");
    assert_eq!(location.kind, LocationType::DUNGEON);
    assert_eq!((location.coords.ns, location.coords.ew), (10.5, 20.5));
    assert_eq!(location.exit_coords(), Coordinates::new(11.5, 21.5));
    assert_eq!(location.dungeon_id, 0xABCD);
    assert!(!location.use_in_route_finding);
    assert!(location.is_retired);
    assert!(location.is_customized);
    assert_eq!(location.specialized_icon, 0x00FF_00FF);
    assert_eq!(location.notes, "A useful description");
}

// The element form (`<Location><Coords/></Location>`, the embedded
// DefaultLocations.xml shape).
#[test]
fn element_form() {
    let location = Location::from_xml(
        "<Location name='Holtburg' type='Town'><Coords NS='42.1' EW='33.6'/><ExitCoords NS='1' EW='2'/>\
         <DungeonId>1A</DungeonId></Location>",
    )
    .unwrap();
    assert_eq!(location.name, "Holtburg");
    assert_eq!(location.kind, LocationType::TOWN);
    assert_eq!(location.coords, Coordinates::new(42.1, 33.6));
    assert_eq!(location.exit_coords(), Coordinates::new(1.0, 2.0));
    assert_eq!(location.dungeon_id, 0x1A);
    let location =
        Location::from_xml("<Location><Name>Arwic</Name><NS>33.6</NS><EW>56.9</EW></Location>")
            .unwrap();
    assert_eq!(location.name, "Arwic");
    assert_eq!(location.coords, Coordinates::new(33.6, 56.9));
}

// LocationMetadataTests.WarcryXml_ConvertsCoordinatesAndPreservesMetadata
#[test]
fn atlas_xml_converts_coordinates() {
    let location = Location::from_atlas_xml(
        "<location><id>456</id><latitude>28.200</latitude><longitude>13.900</longitude>\
         <name>North Outpost</name><type>Outpost</type>\
         <arrival_latitude>28.100</arrival_latitude><arrival_longitude>13.800</arrival_longitude>\
         <description>Atlas description</description><dungeon_id>1A</dungeon_id>\
         <retired>Y</retired></location>",
    )
    .unwrap();
    assert_eq!(location.id, 456);
    assert_eq!(location.kind, LocationType::OUTPOST);
    assert!((location.coords.ns + 28.2).abs() < 1e-3);
    assert!((location.coords.ew - 13.9).abs() < 1e-3);
    assert!((location.exit_coords().ns + 28.1).abs() < 1e-3);
    assert!((location.exit_coords().ew - 13.8).abs() < 1e-3);
    assert_eq!(location.dungeon_id, 0x1A);
    assert_eq!(location.notes, "Atlas description");
    assert!(location.is_retired);
    assert!(!location.use_in_route_finding);
}

// Location.FromXmlWarcry: an arrival of (0, 0) is no arrival.
#[test]
fn atlas_zero_arrival_is_none() {
    let location = Location::from_atlas_xml(
        "<location><id>3</id><name>Unknown Exit</name><latitude>0</latitude><longitude>3</longitude>\
         <arrival_latitude>0</arrival_latitude><arrival_longitude>0</arrival_longitude></location>",
    )
    .unwrap();
    assert!(!location.has_exit_coords());
}

// LocationMetadataTests.Location_CompactXml_RoundTripsMetadata, with a
// dungeon id of 7 and of 0x1234 (DIVERGENCES.md, GA-1: the C# writer wrote
// decimal, so 0x1234 came back as 0x4660).
#[test]
fn compact_xml_round_trips_metadata() {
    for dungeon_id in [7, 0x1234] {
        let mut source = Location::with_details(
            42,
            "Town",
            LocationType::TOWN,
            Coordinates::new(10.0, 20.0),
            "Notes",
            dungeon_id,
            Coordinates::new(11.0, 21.0),
        );
        source.is_customized = true;
        source.is_favorite = true;
        source.is_retired = true;
        source.use_in_route_finding = false;
        source.specialized_icon = 0x1234;

        let copy = Location::from_xml(&source.to_xml()).unwrap();
        assert_eq!(copy.id, source.id);
        assert_eq!(copy.name, source.name);
        assert_eq!(copy.kind, source.kind);
        assert_eq!(copy.coords, source.coords);
        assert_eq!(copy.exit_coords(), source.exit_coords());
        assert_eq!(copy.dungeon_id, source.dungeon_id);
        assert_eq!(copy.notes, source.notes);
        assert_eq!(copy.is_customized, source.is_customized);
        assert_eq!(copy.is_favorite, source.is_favorite);
        assert_eq!(copy.is_retired, source.is_retired);
        assert_eq!(copy.use_in_route_finding, source.use_in_route_finding);
        assert_eq!(copy.specialized_icon, source.specialized_icon);
    }
}

// The writer's attribute order and spellings (`Location.ToXml`).
#[test]
fn writer_layout() {
    let mut loc = Location::with_details(
        9,
        "A & \"B\"",
        LocationType::PORTAL | LocationType::TOWN,
        Coordinates::new(-1.5, 2.25),
        "x < y",
        0x1A,
        Coordinates::new(3.0, 4.0),
    );
    loc.use_in_route_finding = false;
    loc.specialized_icon = 0xAB;
    assert_eq!(
        loc.to_xml(),
        r#"<loc id="9" name="A &amp; &quot;B&quot;" type="Portal, Town" NS="-1.5" EW="2.25" exitNS="3" exitEW="4" dungeonId="1A" use="False" icon="000000AB">x &lt; y</loc>"#
    );
}

// Location.ParseLocationType and the flag names .NET writes.
#[test]
fn location_types() {
    for (text, expected) in [
        ("Portal", LocationType::PORTAL),
        ("NPC", LocationType::NPC),
        ("npc", LocationType::NPC),
        ("Portal Hub", LocationType::PORTAL_HUB),
        ("PortalHub", LocationType::PORTAL_HUB),
        ("Wilderness Portal", LocationType::WILDERNESS_PORTAL),
        ("Town Portal", LocationType::TOWN_PORTAL),
        ("Settlement", LocationType::VILLAGE),
        ("Village", LocationType::VILLAGE),
        ("Allegiance Hall", LocationType::ALLEGIANCE_HALL),
        ("Town, Portal", LocationType::TOWN | LocationType::PORTAL),
        ("128", LocationType::PORTAL),
        ("AnyPortal", LocationType::ANY_PORTAL),
        ("Town Building", LocationType::UNKNOWN),
        ("Fire Wilderness Portal", LocationType::UNKNOWN),
        ("", LocationType::UNKNOWN),
    ] {
        assert_eq!(LocationType::parse(Some(text)), expected, "{text}");
    }
    assert_eq!(LocationType::parse(None), LocationType::UNKNOWN);
    assert_eq!(LocationType::UNKNOWN.name(), "Unknown");
    assert_eq!(LocationType::DUNGEON.name(), "Dungeon");
    assert_eq!(
        (LocationType::PORTAL | LocationType::TOWN).name(),
        "Portal, Town"
    );
    assert_eq!(LocationType::ANY_PORTAL.name(), "AnyPortal");
    assert_eq!(
        (LocationType::ANY_PORTAL | LocationType::TOWN).name(),
        "Town, AnyPortal"
    );
    assert_eq!(LocationType(1 << 20).name(), "1048576");
}

#[test]
fn display() {
    assert_eq!(
        Location::new("Holtburg", 42.1, 33.6).to_string(),
        "Holtburg [42.1N, 33.6E]"
    );
    assert_eq!(Location::named("Nowhere").to_string(), "Nowhere");
}

#[test]
fn malformed_xml_is_an_error() {
    assert!(matches!(
        Location::from_xml("<loc"),
        Err(LocationError::Xml(_))
    ));
}
