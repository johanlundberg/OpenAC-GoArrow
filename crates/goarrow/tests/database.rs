//! The location database, portal devices, route starts and the Atlas
//! check, from OpenAC-GoArrow's `LocationDatabaseTests`,
//! `LocationMetadataTests`, `PortalDeviceTests`, `RouteStartTests` and the
//! validation parts of `WarcryAtlasDataProviderTests`.

use std::collections::HashMap;

use goarrow::coords::Coordinates;
use goarrow::corrections::{AtlasCorrections, SHIPPED};
use goarrow::database::{
    ATLAS_MAX_BYTES, AtlasError, LoadError, LocationDatabase, PortalDevice, RouteStart,
    parse_locations_xml, validate_atlas_download, validate_atlas_xml,
};
use goarrow::location::{Location, LocationType};

// LocationDatabaseTests.LocationDatabase_LoadLocationsXml_ParsesLocations
#[test]
fn load_locations_xml() {
    let mut db = LocationDatabase::new();
    let xml = "<?xml version='1.0' encoding='utf-8'?>
<Locations>
  <Location name='Holtburg'>
    <Coords NS='42.1' EW='33.6' />
  </Location>
  <Location name='Shoushi'>
    <Coords NS='33.4' EW='72.0' />
  </Location>
</Locations>";
    assert_eq!(db.load_locations_xml(xml), Ok(2));
    assert_eq!(db.location_count(), 2);
    assert_eq!(db.find_location("Holtburg").unwrap().name, "Holtburg");
    assert_eq!(db.find_location("shoushi").unwrap().name, "Shoushi");
}

// LocationDatabaseTests.LocationDatabase_LoadLocationsXml_EmptyOnInvalidXml
#[test]
fn load_locations_xml_without_records_is_empty() {
    let mut db = LocationDatabase::new();
    assert_eq!(db.load_locations_xml("<Locations></Locations>"), Ok(0));
    assert_eq!(db.location_count(), 0);
}

// LocationDatabase.LoadLocationsXml: XmlDocument.LoadXml throws on bad
// XML; the loaded places stay.
#[test]
fn malformed_xml_keeps_the_loaded_places() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Holtburg;42.1;33.6"]).unwrap();
    assert!(matches!(
        db.load_locations_xml("<Locations>"),
        Err(LoadError::Xml(_))
    ));
    assert_eq!(db.location_count(), 1);
}

// DIVERGENCES.md, GA-5: a document type declaration is refused.
#[test]
fn a_dtd_is_refused() {
    let xml = "<!DOCTYPE Locations [<!ENTITY n 'Holtburg'>]><Locations><loc name='&n;' NS='1' EW='1'/></Locations>";
    assert!(matches!(parse_locations_xml(xml), Err(LoadError::Xml(_))));
}

// LocationDatabase.LoadLocationsXml: compact `<loc>` and element
// `<Location>` records together, in document order; records without
// coordinates dropped.
#[test]
fn compact_and_element_records_load_in_document_order() {
    let parsed = parse_locations_xml(
        "<locations><loc name='B' NS='1' EW='1'/><Location name='A'><Coords NS='2' EW='2'/></Location>\
         <loc name='NoCoords'/></locations>",
    )
    .unwrap();
    assert!(!parsed.is_atlas);
    let names: Vec<&str> = parsed.locations.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["B", "A"]);
}

// LocationDatabase.LoadLocationsXml: the CSV fallback over the elements'
// text.
#[test]
fn element_text_is_read_as_lines_when_no_record_has_coordinates() {
    let mut db = LocationDatabase::new();
    let count = db
        .load_locations_xml("<Locations><Location>Holtburg; 42.1; 33.6</Location><loc>Arwic;33.6;56.9</loc></Locations>")
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        db.find_location("Arwic").unwrap().coords,
        Coordinates::new(33.6, 56.9)
    );
}

// LocationDatabaseTests.LocationDatabase_LoadLocationsCsv_ParsesLines
#[test]
fn load_locations_csv() {
    let mut db = LocationDatabase::new();
    let count = db
        .load_locations_csv([
            "Holtburg;42.1;33.6",
            "Shoushi;33.4;72.0",
            "# This is a comment",
            "",
        ])
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(db.location_count(), 2);
}

// LocationDatabaseTests.LocationDatabase_SearchLocations_FindsBySubstring
#[test]
fn search_by_substring() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Holtburg;42.1;33.6", "Shoushi;33.4;72.0"])
        .unwrap();
    let results = db.search_locations("burg");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Holtburg");
    assert_eq!(db.search_locations("BURG").len(), 1);
    assert_eq!(db.search_locations("").len(), 2);
}

// LocationDatabaseTests.LocationDatabase_FindLocation_ReturnsNullForUnknown
#[test]
fn find_unknown_is_none() {
    assert!(LocationDatabase::new().find_location("Nowhere").is_none());
}

// LocationDatabase: a name may have several places; a lookup gives the
// first.
#[test]
fn a_name_keeps_every_place() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Town;1;1", "town;2;2", "Other;3;3"])
        .unwrap();
    assert_eq!(db.find_locations("TOWN").len(), 2);
    assert_eq!(
        db.find_location("TOWN").unwrap().coords,
        Coordinates::new(1.0, 1.0)
    );
}

// LocationDatabase: each load replaces the base places; the player's
// marks are put back and replace a loaded place of the same name.
#[test]
fn loads_replace_and_marks_survive() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Holtburg;42.1;33.6", "Cellar;1;1"])
        .unwrap();
    db.upsert_user_location(Location::new("Cellar", 5.0, 5.0));
    assert_eq!(db.location_count(), 2);
    assert_eq!(
        db.find_location("cellar").unwrap().coords,
        Coordinates::new(5.0, 5.0)
    );

    db.load_locations_csv(["Arwic;33.6;56.9", "Cellar;9;9"])
        .unwrap();
    assert!(db.find_location("Holtburg").is_none());
    assert_eq!(db.location_count(), 2);
    assert_eq!(db.find_locations("Cellar").len(), 1);
    assert_eq!(
        db.find_location("Cellar").unwrap().coords,
        Coordinates::new(5.0, 5.0)
    );
    // Marks come last.
    assert_eq!(db.all_locations().last().unwrap().name, "Cellar");

    db.upsert_user_location(Location::new("CELLAR", 6.0, 6.0));
    assert_eq!(db.user_locations().len(), 1);
    assert_eq!(
        db.find_location("Cellar").unwrap().coords,
        Coordinates::new(6.0, 6.0)
    );
}

// LocationDatabaseTests.LocationDatabase_LoadPortalDevicesXml_ParsesDevices
#[test]
fn load_portal_devices_xml() {
    let mut db = LocationDatabase::new();
    db.load_portal_devices_xml(
        "<?xml version='1.0' encoding='utf-8'?>
<PortalDevices>
  <Device Destination='Holtburg' Via='Holtburg Portal Device' Landmass='Dereth' Entrance='Holtburg Plaza' Exit='Holtburg' />
</PortalDevices>",
    )
    .unwrap();
    assert_eq!(db.portal_devices().len(), 1);
    let ports = db.find_portals_to("Holtburg");
    assert_eq!(ports.len(), 1);
    assert_eq!(ports[0].via, "Holtburg Portal Device");
    assert_eq!(ports[0].entrance_location, "Holtburg Plaza");
    assert_eq!(ports[0].exit_location, "Holtburg");
    db.load_portal_devices_xml(
        "<PortalDevices><Device Destination='A' From='B' To='C'/></PortalDevices>",
    )
    .unwrap();
    assert_eq!(db.portal_devices()[0].entrance_location, "B");
    assert_eq!(db.portal_devices()[0].exit_location, "C");
}

// LocationDatabaseTests.LocationDatabase_LoadRouteStartsXml_ParsesStarts
#[test]
fn load_route_starts_xml() {
    let mut db = LocationDatabase::new();
    db.load_route_starts_xml(
        "<?xml version='1.0' encoding='utf-8'?>
<RouteStarts>
  <Start Destination='Holtburg' From='APortal' Via='APortal' />
</RouteStarts>",
    )
    .unwrap();
    assert_eq!(db.route_starts().len(), 1);
    let from = db.find_routes_from("APortal");
    assert_eq!(from.len(), 1);
    assert_eq!(from[0].destination, "Holtburg");
    assert_eq!(db.find_routes_to("holtburg").len(), 1);
}

// LocationDatabaseTests.LocationDatabase_Clear_RemovesAll
#[test]
fn clear_removes_all() {
    let mut db = LocationDatabase::new();
    db.load_locations_csv(["Test;10;20"]).unwrap();
    db.load_portal_devices_csv(["Test;Via;Land"]);
    db.load_route_starts_csv(["Dest;From;Via"]);
    db.clear();
    assert_eq!(db.location_count(), 0);
    assert!(db.portal_devices().is_empty());
    assert!(db.route_starts().is_empty());
}

// LocationMetadataTests.LocationDatabase_LoadsWarcryAtlasLocations
#[test]
fn loads_atlas_locations() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml(
        "<atlas><location><id>1</id><latitude>1</latitude><longitude>2</longitude>\
         <name>Atlas Point</name><type>Landmark</type><retired>N</retired></location></atlas>",
    )
    .unwrap();
    let location = db.find_location("Atlas Point").expect("loaded");
    assert_eq!(location.id, 1);
    assert_eq!(location.kind, LocationType::LANDMARK);
    assert_eq!(location.coords, Coordinates::new(-1.0, 2.0));
}

// LocationMetadataTests.ShippedCorrectionsFixTownNetworkNantoArrival
#[test]
fn shipped_corrections_fix_the_nanto_arrival() {
    let mut db = LocationDatabase::new();
    let mut errors = Vec::new();
    let shipped = AtlasCorrections::parse(SHIPPED).unwrap();
    db.set_corrections(
        AtlasCorrections::arrivals_by_id(&[&shipped], &mut errors),
        Vec::new(),
    );
    db.load_locations_xml(
        "<atlas><location><id>9341</id><latitude>99.1</latitude><longitude>8.7</longitude>\
         <name>Town Network (E R 2) to Nanto</name><type>Wilderness Portal</type>\
         <arrival_latitude>82.200</arrival_latitude><arrival_longitude>82.500</arrival_longitude>\
         <retired>N</retired></location></atlas>",
    )
    .unwrap();
    assert!(errors.is_empty());
    let portal = db.find_location("Town Network (E R 2) to Nanto").unwrap();
    assert_eq!(portal.exit_coords(), Coordinates::new(-52.2, 82.5));
}

// LocationDatabase.SetCorrections: arrivals apply in place to loaded Atlas
// data only.
#[test]
fn corrections_apply_to_loaded_atlas_data_only() {
    let mut db = LocationDatabase::new();
    db.load_locations_xml("<locations><loc id='1' name='P' type='Portal' NS='0' EW='1' exitNS='5' exitEW='5'/></locations>")
        .unwrap();
    let before = db.find_location("P").unwrap();
    db.set_corrections(HashMap::from([(1, Coordinates::new(9.0, 9.0))]), Vec::new());
    assert_eq!(before.exit_coords(), Coordinates::new(5.0, 5.0));

    db.load_locations_xml(
        "<atlas><location><id>1</id><name>P</name><type>Wilderness Portal</type><latitude>0</latitude>\
         <longitude>1</longitude><arrival_latitude>-5</arrival_latitude><arrival_longitude>5</arrival_longitude>\
         </location></atlas>",
    )
    .unwrap();
    let atlas = db.find_location("P").unwrap();
    assert_eq!(atlas.exit_coords(), Coordinates::new(9.0, 9.0));
    db.set_corrections(HashMap::from([(1, Coordinates::new(7.0, 7.0))]), Vec::new());
    assert_eq!(atlas.exit_coords(), Coordinates::new(7.0, 7.0));
}

// PortalDeviceTests (all)
#[test]
fn portal_devices() {
    let pd = PortalDevice::new("Holtburg", "Holtburg South", "Dereth");
    assert_eq!(
        (
            pd.destination.as_str(),
            pd.via.as_str(),
            pd.landmass.as_str()
        ),
        ("Holtburg", "Holtburg South", "Dereth")
    );
    let pd = PortalDevice::from_csv_line("Holtburg;Holtburg South Portal;Dereth");
    assert_eq!(
        (
            pd.destination.as_str(),
            pd.via.as_str(),
            pd.landmass.as_str()
        ),
        ("Holtburg", "Holtburg South Portal", "Dereth")
    );
    let pd = PortalDevice::from_csv_line("Holtburg;Portal Device;");
    assert_eq!(
        (pd.destination.as_str(), pd.via.as_str()),
        ("Holtburg", "Portal Device")
    );
    assert!(pd.landmass.is_empty());
    let pd = PortalDevice::from_csv_line("TownB;TownB Portal;Dereth;TownA;TownB");
    assert_eq!(
        (pd.entrance_location.as_str(), pd.exit_location.as_str()),
        ("TownA", "TownB")
    );
    assert_eq!(pd.to_string(), "TownB;TownB Portal;Dereth;TownA;TownB");
    assert_eq!(
        PortalDevice::from_csv_line(" Lonely ").destination,
        "Lonely"
    );

    assert_eq!(
        PortalDevice::new("Holtburg", "Portal", "Dereth"),
        PortalDevice::new("Holtburg", "Portal", "Dereth")
    );
    assert_eq!(
        PortalDevice::new("Holtburg", "Portal", "Dereth"),
        PortalDevice::new("holtburg", "Portal", "Dereth")
    );
    assert_ne!(
        PortalDevice::new("Holtburg", "Portal", "Dereth"),
        PortalDevice::new("Shoushi", "Portal", "Dereth")
    );
    assert_eq!(
        PortalDevice::new("Test", "Via", "Land").to_string(),
        "Test;Via;Land"
    );
}

// RouteStartTests (all)
#[test]
fn route_starts() {
    let rs = RouteStart::new("Holtburg", "APortal", "Portal Spell");
    assert_eq!(
        (rs.destination.as_str(), rs.from.as_str(), rs.via.as_str()),
        ("Holtburg", "APortal", "Portal Spell")
    );
    let rs = RouteStart::from_csv_line("Holtburg;APortal;Portal Spell");
    assert_eq!(
        (rs.destination.as_str(), rs.from.as_str(), rs.via.as_str()),
        ("Holtburg", "APortal", "Portal Spell")
    );
    let rs = RouteStart::from_csv_line("Holtburg;APortal;");
    assert_eq!(
        (rs.destination.as_str(), rs.from.as_str()),
        ("Holtburg", "APortal")
    );
    assert!(rs.via.is_empty());
    assert_eq!(
        RouteStart::from_csv_line("Holtburg;;x").destination,
        "Holtburg;;x"
    );

    assert_eq!(
        RouteStart::new("Holtburg", "LS", "Portal"),
        RouteStart::new("Holtburg", "LS", "Portal")
    );
    assert_eq!(
        RouteStart::new("Holtburg", "LS", "Portal"),
        RouteStart::new("holtburg", "ls", "Portal")
    );
    assert_ne!(
        RouteStart::new("Holtburg", "LS", "Portal"),
        RouteStart::new("Shoushi", "LS", "Portal")
    );
    assert_eq!(
        RouteStart::new("Dest", "From", "Via").to_string(),
        "Dest;From;Via"
    );
}

// WarcryAtlasDataProviderTests.ValidateAtlasXml_RequiresLocationRecords
#[test]
fn atlas_needs_location_records() {
    assert_eq!(
        validate_atlas_xml("<atlas />"),
        Err(AtlasError::NoLocations)
    );
}

// WarcryAtlasDataProviderTests.ValidateAtlasXml_ReturnsLocationCount
#[test]
fn atlas_location_count() {
    assert_eq!(
        validate_atlas_xml("<atlas><location /><location /></atlas>"),
        Ok(2)
    );
}

// WarcryAtlasDataProvider.ValidateAtlasXml: root `<atlas>` in any case,
// the records matched exactly; no DTD.
#[test]
fn atlas_root_and_dtd() {
    assert_eq!(
        validate_atlas_xml("<locations><location/></locations>"),
        Err(AtlasError::NotAtlas)
    );
    assert_eq!(
        validate_atlas_xml("<ATLAS><location/></ATLAS>"),
        Err(AtlasError::NoLocations)
    );
    assert!(matches!(
        validate_atlas_xml("<!DOCTYPE atlas [<!ENTITY a 'b'>]><atlas><location/></atlas>"),
        Err(AtlasError::Xml(_))
    ));
    assert!(matches!(
        validate_atlas_xml("<atlas><location>"),
        Err(AtlasError::Xml(_))
    ));
    assert_eq!(
        validate_atlas_xml("<atlas><x><location/></x></atlas>"),
        Err(AtlasError::NoLocations)
    );
}

// WarcryAtlasDataProvider.DownloadAsync: empty and oversized bodies.
#[test]
fn atlas_download_checks() {
    assert_eq!(validate_atlas_download(b""), Err(AtlasError::Empty));
    let big = vec![b' '; ATLAS_MAX_BYTES + 1];
    assert_eq!(validate_atlas_download(&big), Err(AtlasError::TooLarge));
    let xml = "<atlas><location><id>1</id><name>Test</name><latitude>1</latitude><longitude>2</longitude></location></atlas>";
    assert_eq!(
        validate_atlas_download(xml.as_bytes()),
        Ok((xml.to_owned(), 1))
    );
    assert_eq!(
        AtlasError::TooLarge.to_string(),
        "Atlas download is larger than the 16 MiB limit."
    );
    assert_eq!(
        AtlasError::NotAtlas.to_string(),
        "Atlas XML root element must be <atlas>."
    );
}
