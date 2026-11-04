//! Distances as the player reads them (`TravelDistance.cs`): map units
//! shown in metres or kilometres.

use crate::coords::METRES_PER_MAP_UNIT;
use crate::text::format_fixed;

/// A distance in map units as text: `--` when it is not finite, `1.23 km`
/// above 1000 m, `2.4 m` below 10 m, otherwise whole metres (`0 m`,
/// `31 m`, `1000 m`).
#[must_use]
pub fn format_travel_distance(map_units: f64) -> String {
    let metres = map_units * METRES_PER_MAP_UNIT;
    if !metres.is_finite() {
        return "--".to_owned();
    }
    if metres > 1000.0 + 1e-6 {
        return format!("{} km", format_fixed(metres / 1000.0, 0, 2));
    }
    if metres > 0.0 && metres < 10.0 {
        return format!("{} m", format_fixed(metres, 1, 1));
    }
    format!("{} m", format_fixed(metres, 0, 0))
}
