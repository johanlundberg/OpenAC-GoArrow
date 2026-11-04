//! Map coordinates: north/south and east/west in map units of 240 metres,
//! north and east positive, as the game's map and chat show them
//! (`42.1N, 33.6E`). GoArrow's [`Coordinates`] (`RouteFinding/Coordinates.cs`)
//! with its parser, its text form and the landcell conversion; and
//! [`NavPosition`], a player or object position as the client reports it.

use std::fmt;

use crate::text;

/// Metres in one map unit.
pub const METRES_PER_MAP_UNIT: f64 = 240.0;

/// A point on the map, in map units. NaN north/south means "no
/// coordinates" ([`Coordinates::NONE`]).
#[derive(Debug, Clone, Copy)]
pub struct Coordinates {
    /// North (positive) or south, map units.
    pub ns: f64,
    /// East (positive) or west, map units.
    pub ew: f64,
}

impl Coordinates {
    /// No coordinates (both NaN).
    pub const NONE: Self = Self {
        ns: f64::NAN,
        ew: f64::NAN,
    };
    /// The text of [`Coordinates::NONE`].
    pub const NONE_TEXT: &'static str = "<None>";
    /// The text of [`Coordinates::NONE`] where "unknown" reads better.
    pub const UNKNOWN_TEXT: &'static str = "<Unknown>";

    /// A point.
    #[must_use]
    pub const fn new(ns: f64, ew: f64) -> Self {
        Self { ns, ew }
    }

    /// The point at a landcell and its offset within the landblock, metres.
    #[must_use]
    pub fn from_landcell(landcell: u32, y_offset: f64, x_offset: f64) -> Self {
        Self::new(
            Self::landblock_to_ns(landcell, y_offset),
            Self::landblock_to_ew(landcell, x_offset),
        )
    }

    /// [`Coordinates::from_landcell`], each value rounded to `precision`
    /// decimals (half to even).
    #[must_use]
    pub fn from_landcell_rounded(
        landcell: u32,
        y_offset: f64,
        x_offset: f64,
        precision: i32,
    ) -> Self {
        Self::from_landcell(landcell, y_offset, x_offset).round(precision)
    }

    /// North/south of a landcell's row plus an offset in metres.
    #[must_use]
    pub fn landblock_to_ns(landcell: u32, y_offset: f64) -> f64 {
        // One landblock is 8 tenths of a unit; 1019.5 tenths puts row 0x7F's
        // 84 m point at 0.
        let row = f64::from(((landcell >> 16) & 0xFF) * 8);
        (row + y_offset / 24.0 - 1019.5) / 10.0
    }

    /// East/west of a landcell's column plus an offset in metres.
    #[must_use]
    pub fn landblock_to_ew(landcell: u32, x_offset: f64) -> f64 {
        let column = f64::from(((landcell >> 24) & 0xFF) * 8);
        (column + x_offset / 24.0 - 1019.5) / 10.0
    }

    /// Whether these are no coordinates: north/south is NaN (an east/west
    /// of NaN alone still counts as coordinates, as in GoArrow).
    #[must_use]
    pub fn is_none(self) -> bool {
        self.ns.is_nan()
    }

    /// The direction to `dest`, radians: 0 north, positive clockwise.
    #[must_use]
    pub fn angle_to(self, dest: Self) -> f64 {
        (dest.ew - self.ew).atan2(dest.ns - self.ns)
    }

    /// The compass bearing to `dest`, degrees in `0..360` (0 north, 90
    /// east).
    #[must_use]
    pub fn bearing_degrees_to(self, dest: Self) -> f64 {
        let bearing = self.angle_to(dest) * (180.0 / std::f64::consts::PI);
        if bearing < 0.0 {
            bearing + 360.0
        } else {
            bearing
        }
    }

    /// The straight-line distance to `dest`, map units; infinite when
    /// either point has a NaN.
    #[must_use]
    pub fn distance_to(self, dest: Self) -> f64 {
        let x = dest.ew - self.ew;
        let y = dest.ns - self.ns;
        if (x + y).is_nan() {
            return f64::INFINITY;
        }
        (x * x + y * y).sqrt()
    }

    /// `dest` minus this point.
    #[must_use]
    pub fn relative_to(self, dest: Self) -> Self {
        Self::new(dest.ns - self.ns, dest.ew - self.ew)
    }

    /// Each value rounded to `precision` decimals, half to even.
    #[must_use]
    pub fn round(self, precision: i32) -> Self {
        let scale = 10f64.powi(precision);
        Self::new(
            (self.ns * scale).round_ties_even() / scale,
            (self.ew * scale).round_ties_even() / scale,
        )
    }

    /// The first coordinates written in `text`: a number of up to three
    /// digits and four decimals (or only decimals), `N` or `S`, up to four
    /// of `; / ,` and white space, a number, `E` or `W`, in any case
    /// (`42.1N 33.6E`, `10.5s,20.3w`). North/south comes first.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let chars: Vec<char> = text.chars().collect();
        (0..chars.len()).find_map(|start| match_at(&chars, start))
    }

    /// [`Coordinates::parse`], but "", `<None>` and `<Unknown>` read as
    /// [`Coordinates::NONE`].
    #[must_use]
    pub fn parse_allow_none(text: &str) -> Option<Self> {
        if text.is_empty() || text == Self::NONE_TEXT || text == Self::UNKNOWN_TEXT {
            return Some(Self::NONE);
        }
        Self::parse(text)
    }

    /// The text form, `42.1N, 33.6E`: one decimal when both values have at
    /// most one, else two (`52.25S, 82.50E`). No coordinates read
    /// `<None>`, or `<Unknown>` with `use_unknown_text`.
    #[must_use]
    pub fn to_text(self, use_unknown_text: bool) -> String {
        let ns10 = self.ns * 10.0;
        let ew10 = self.ew * 10.0;
        let decimals = if ns10.floor() != ns10 || ew10.floor() != ew10 {
            2
        } else {
            1
        };
        self.to_text_with_decimals(decimals, use_unknown_text)
    }

    /// The text form with a fixed number of decimals.
    #[must_use]
    pub fn to_text_with_decimals(self, decimals: usize, use_unknown_text: bool) -> String {
        if self.ns.is_nan() || self.ew.is_nan() {
            return if use_unknown_text {
                Self::UNKNOWN_TEXT
            } else {
                Self::NONE_TEXT
            }
            .to_owned();
        }
        format!(
            "{}{}, {}{}",
            text::format_fixed(self.ns.abs(), decimals, decimals),
            if self.ns >= 0.0 { 'N' } else { 'S' },
            text::format_fixed(self.ew.abs(), decimals, decimals),
            if self.ew >= 0.0 { 'E' } else { 'W' },
        )
    }
}

/// The parser's pattern at one start: number, `\s*`, `[ns]`,
/// `[;/,\s]{0,4}`, `\s*`, number, `\s*`, `[ew]`, trying the longest
/// alternatives first as a backtracking regex does.
fn match_at(chars: &[char], start: usize) -> Option<Coordinates> {
    for ns_end in number_ends(chars, start) {
        for ns_char_at in spaces_ends(chars, ns_end, usize::MAX) {
            let Some(ns_char) = chars
                .get(ns_char_at)
                .filter(|c| matches!(c, 'n' | 'N' | 's' | 'S'))
            else {
                continue;
            };
            let after_ns = ns_char_at + 1;
            for separated in separator_ends(chars, after_ns) {
                for ew_start in spaces_ends(chars, separated, usize::MAX) {
                    for ew_end in number_ends(chars, ew_start) {
                        for ew_char_at in spaces_ends(chars, ew_end, usize::MAX) {
                            let Some(ew_char) = chars
                                .get(ew_char_at)
                                .filter(|c| matches!(c, 'e' | 'E' | 'w' | 'W'))
                            else {
                                continue;
                            };
                            let ns = number(&chars[start..ns_end])?;
                            let ew = number(&chars[ew_start..ew_end])?;
                            return Some(Coordinates::new(
                                if ns_char.eq_ignore_ascii_case(&'s') {
                                    -ns
                                } else {
                                    ns
                                },
                                if ew_char.eq_ignore_ascii_case(&'w') {
                                    -ew
                                } else {
                                    ew
                                },
                            ));
                        }
                    }
                }
            }
        }
    }
    None
}

fn number(chars: &[char]) -> Option<f64> {
    chars.iter().collect::<String>().parse().ok()
}

fn digit_run(chars: &[char], from: usize, max: usize) -> usize {
    chars[from.min(chars.len())..]
        .iter()
        .take(max)
        .take_while(|c| c.is_ascii_digit())
        .count()
}

/// Where `(\d{1,3}(\.\d{1,4})?)|(\.\d{1,4})` can end, in the order a
/// regex tries them.
fn number_ends(chars: &[char], start: usize) -> Vec<usize> {
    let mut ends = Vec::new();
    let digits = digit_run(chars, start, 3);
    for d in (1..=digits).rev() {
        let after = start + d;
        if chars.get(after) == Some(&'.') {
            let decimals = digit_run(chars, after + 1, 4);
            ends.extend((1..=decimals).rev().map(|f| after + 1 + f));
        }
        ends.push(after);
    }
    if chars.get(start) == Some(&'.') {
        let decimals = digit_run(chars, start + 1, 4);
        ends.extend((1..=decimals).rev().map(|f| start + 1 + f));
    }
    ends
}

/// Where a greedy run of at most `max` white-space characters can end,
/// longest first.
fn spaces_ends(chars: &[char], start: usize, max: usize) -> impl Iterator<Item = usize> {
    let run = chars[start.min(chars.len())..]
        .iter()
        .take(max)
        .take_while(|c| c.is_whitespace())
        .count();
    (0..=run).rev().map(move |k| start + k)
}

/// Where `[;/,\s]{0,4}` can end, longest first.
fn separator_ends(chars: &[char], start: usize) -> impl Iterator<Item = usize> {
    let run = chars[start.min(chars.len())..]
        .iter()
        .take(4)
        .take_while(|c| matches!(c, ';' | '/' | ',') || c.is_whitespace())
        .count();
    (0..=run).rev().map(move |k| start + k)
}

impl Default for Coordinates {
    fn default() -> Self {
        Self::NONE
    }
}

/// Equal when both values are equal, or when both north/south values are
/// NaN (GoArrow's rule).
impl PartialEq for Coordinates {
    fn eq(&self, other: &Self) -> bool {
        (self.ns == other.ns && self.ew == other.ew) || (self.ns.is_nan() && other.ns.is_nan())
    }
}

impl fmt::Display for Coordinates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text(false))
    }
}

/// A position as the client reports it: the cell, map coordinates,
/// height, heading and whether the cell is outdoors.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NavPosition {
    /// The full cell id (landblock in the high 16 bits).
    pub cell_id: u32,
    /// East (positive) or west, map units.
    pub east_west: f64,
    /// North (positive) or south, map units.
    pub north_south: f64,
    /// Height, map units.
    pub elevation: f64,
    /// Compass heading, degrees (0 north, 90 east).
    pub heading_degrees: f32,
    /// The cell is on the landscape.
    pub is_outdoor: bool,
}

impl NavPosition {
    /// The map coordinates.
    #[must_use]
    pub fn coordinates(&self) -> Coordinates {
        Coordinates::new(self.north_south, self.east_west)
    }

    /// The distance to `other` across the ground, metres.
    #[must_use]
    pub fn horizontal_distance_metres(&self, other: &Self) -> f64 {
        let dx = self.east_west - other.east_west;
        let dy = self.north_south - other.north_south;
        (dx * dx + dy * dy).sqrt() * METRES_PER_MAP_UNIT
    }
}
