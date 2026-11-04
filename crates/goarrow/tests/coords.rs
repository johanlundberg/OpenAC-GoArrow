//! Map coordinates and distance text, from OpenAC-GoArrow's
//! `CoordinatesTests` and `TravelDistanceTests`.

use std::f64::consts::PI;

use goarrow::coords::{Coordinates, NavPosition};
use goarrow::distance::format_travel_distance;

fn close(a: f64, b: f64, places: i32) -> bool {
    (a - b).abs() < 0.5 * 10f64.powi(-places)
}

// CoordinatesTests.Coordinates_Create_StoresValues
#[test]
fn create_stores_values() {
    let c = Coordinates::new(42.1, 33.6);
    assert_eq!(c.ns, 42.1);
    assert_eq!(c.ew, 33.6);
}

// CoordinatesTests.Coordinates_NoCoordinates_IsNaN
#[test]
fn no_coordinates_is_nan() {
    assert!(Coordinates::NONE.ns.is_nan());
    assert!(Coordinates::NONE.ew.is_nan());
    assert!(Coordinates::NONE.is_none());
}

// CoordinatesTests.Coordinates_DistanceTo_ComputesCorrectly
#[test]
fn distance_is_straight_line() {
    let d = Coordinates::new(0.0, 0.0).distance_to(Coordinates::new(3.0, 4.0));
    assert!(close(d, 5.0, 10));
}

// CoordinatesTests.Coordinates_DistanceTo_NaNReturnsInfinity
#[test]
fn distance_from_nothing_is_infinite() {
    let d = Coordinates::NONE.distance_to(Coordinates::new(0.0, 0.0));
    assert!(d.is_infinite() && d > 0.0);
}

// CoordinatesTests.Coordinates_AngleTo_ComputesCorrectly
#[test]
fn angle_is_clockwise_from_north() {
    let a = Coordinates::new(0.0, 0.0);
    assert!(close(a.angle_to(Coordinates::new(1.0, 0.0)), 0.0, 10));
    assert!(close(a.angle_to(Coordinates::new(0.0, 1.0)), PI / 2.0, 10));
}

// CoordinatesTests.Coordinates_TryParse_FindsCoords
#[test]
fn parse_finds_coordinates() {
    let c = Coordinates::parse("42.1N 33.6E").expect("parses");
    assert!(close(c.ns, 42.1, 4));
    assert!(close(c.ew, 33.6, 4));
}

// CoordinatesTests.Coordinates_TryParse_SouthWest
#[test]
fn parse_south_west_is_negative() {
    let c = Coordinates::parse("10.5S 20.3W").expect("parses");
    assert!(close(c.ns, -10.5, 4));
    assert!(close(c.ew, -20.3, 4));
}

// CoordinatesTests.Coordinates_TryParse_NoCoordsReturnsFalse
#[test]
fn parse_without_coordinates_fails() {
    assert_eq!(Coordinates::parse("Hello World"), None);
}

// The parser's pattern (`Coordinates.cs`, `CoordSearchRegex`): the first
// match in the text, in any case, with the separators it allows, and the
// leftmost start that can match (so `1234N` reads `234N`).
#[test]
fn parse_follows_the_search_pattern() {
    let c = Coordinates::parse("Portal to Town (52.2s; 82.5e).").expect("parses");
    assert_eq!((c.ns, c.ew), (-52.2, 82.5));
    let c = Coordinates::parse("x .5N,/ 3E").expect("parses");
    assert_eq!((c.ns, c.ew), (0.5, 3.0));
    let c = Coordinates::parse("1234N 5E").expect("parses");
    assert_eq!((c.ns, c.ew), (234.0, 5.0));
    let c = Coordinates::parse("1.23456N 5E").expect("parses");
    assert_eq!((c.ns, c.ew), (456.0, 5.0));
    // East/west first does not read; more than four separators do not.
    assert_eq!(Coordinates::parse("33.6E 42.1N"), None);
    assert_eq!(Coordinates::parse("1N ;;;;; 2E"), None);
    assert_eq!(
        Coordinates::parse("1N;;;; 2E"),
        Some(Coordinates::new(1.0, 2.0))
    );
    assert_eq!(
        Coordinates::parse("1 n 2 w"),
        Some(Coordinates::new(1.0, -2.0))
    );
}

// Coordinates.TryParse(string, allowNoCoords, out)
#[test]
fn parse_allow_none_reads_the_none_texts() {
    for text in ["", "<None>", "<Unknown>"] {
        assert!(Coordinates::parse_allow_none(text).expect(text).is_none());
    }
    assert_eq!(Coordinates::parse_allow_none("nowhere"), None);
    assert_eq!(
        Coordinates::parse_allow_none("1N 2E"),
        Some(Coordinates::new(1.0, 2.0))
    );
}

// CoordinatesTests.Coordinates_ToString_FormatsCorrectly
#[test]
fn text_has_one_decimal() {
    assert_eq!(Coordinates::new(42.1, 33.6).to_string(), "42.1N, 33.6E");
}

// CoordinatesTests.Coordinates_ToString_SouthWest
#[test]
fn text_south_west() {
    assert_eq!(Coordinates::new(-10.5, -20.3).to_string(), "10.5S, 20.3W");
}

// The two-decimal rule (`Coordinates.cs`, `ToString(bool)`).
#[test]
fn text_has_two_decimals_when_either_needs_them() {
    assert_eq!(Coordinates::new(-52.25, 82.5).to_string(), "52.25S, 82.50E");
    assert_eq!(Coordinates::new(0.0, 2.0).to_string(), "0.0N, 2.0E");
    assert_eq!(Coordinates::new(0.125, 0.0).to_string(), "0.13N, 0.00E");
}

// CoordinatesTests.Coordinates_NoCoordinates_ToString_ReturnsNone
#[test]
fn no_coordinates_text() {
    assert_eq!(Coordinates::NONE.to_string(), Coordinates::NONE_TEXT);
    assert_eq!(Coordinates::NONE.to_text(true), Coordinates::UNKNOWN_TEXT);
}

// CoordinatesTests.Coordinates_Equality_SameValues / DifferentValues / NoCoordinates
#[test]
fn equality() {
    assert_eq!(Coordinates::new(42.1, 33.6), Coordinates::new(42.1, 33.6));
    assert_ne!(Coordinates::new(42.1, 33.6), Coordinates::new(10.0, 20.0));
    assert_eq!(Coordinates::NONE, Coordinates::NONE);
}

// CoordinatesTests.Coordinates_Round_RoundsCorrectly
#[test]
fn round() {
    let r = Coordinates::new(42.12345, 33.67890).round(2);
    assert!(close(r.ns, 42.12, 2));
    assert!(close(r.ew, 33.68, 2));
}

// CoordinatesTests.Coordinates_LandcellConversion_ProducesValidCoords,
// plus the values the formula gives.
#[test]
fn landcell_conversion() {
    let c = Coordinates::from_landcell(0x00E7_0000, 12.0, 12.0);
    assert!(!c.ns.is_nan() && !c.ew.is_nan());
    // The map's centre: landblock 0x7F7F at (84, 84) metres.
    let centre = Coordinates::from_landcell(0x7F7F_0001, 84.0, 84.0);
    assert!(centre.ns.abs() < 1e-9 && centre.ew.abs() < 1e-9);
    // One landblock (192 m, 0.8 units) east and north.
    let c = Coordinates::from_landcell(0x8080_0001, 84.0, 84.0);
    assert!(close(c.ns, 0.8, 9) && close(c.ew, 0.8, 9));
    assert_eq!(
        Coordinates::from_landcell_rounded(0x8080_0001, 85.0, 85.0, 1),
        Coordinates::new(0.8, 0.8)
    );
}

// GoArrowDestination.UpdateGuidance's bearing rule.
#[test]
fn bearing_is_compass_degrees() {
    let o = Coordinates::new(0.0, 0.0);
    assert!(close(
        o.bearing_degrees_to(Coordinates::new(0.0, 1.0)),
        90.0,
        6
    ));
    assert!(close(
        o.bearing_degrees_to(Coordinates::new(-1.0, 0.0)),
        180.0,
        6
    ));
    assert!(close(
        o.bearing_degrees_to(Coordinates::new(0.0, -1.0)),
        270.0,
        6
    ));
    assert!(close(
        o.bearing_degrees_to(Coordinates::new(1.0, -1.0)),
        315.0,
        6
    ));
}

#[test]
fn nav_position_distance_is_metres() {
    let a = NavPosition {
        east_west: 0.0,
        north_south: 0.0,
        ..NavPosition::default()
    };
    let b = NavPosition {
        east_west: 0.3,
        north_south: 0.4,
        ..NavPosition::default()
    };
    assert!(close(a.horizontal_distance_metres(&b), 120.0, 9));
    assert_eq!(b.coordinates(), Coordinates::new(0.4, 0.3));
}

// TravelDistanceTests.DisplaysCoordinateDistanceInMetersOrKilometers
#[test]
fn travel_distance_text() {
    for (units, expected) in [
        (0.0, "0 m"),
        (0.01, "2.4 m"),
        (0.13, "31 m"),
        (1000.0 / 240.0, "1000 m"),
        (5.0, "1.2 km"),
        (f64::INFINITY, "--"),
        (f64::NAN, "--"),
        (10.0, "2.4 km"),
        (4.2, "1.01 km"),
    ] {
        assert_eq!(format_travel_distance(units), expected, "{units}");
    }
}
