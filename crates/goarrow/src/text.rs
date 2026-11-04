//! The text rules the C# plugin inherited from .NET, written out: ordinal
//! case-insensitive comparison of names, invariant-culture number parsing
//! (`double.TryParse`, `int.TryParse`, hexadecimal, `bool.TryParse`) and the
//! number formats its strings use (`0.0`, `0.##`, round-trip). Names and
//! numbers in location files and on screen must come out as they did there.

use std::cmp::Ordering;

/// One character as ordinal case-insensitive comparison sees it: its
/// simple upper-case mapping, or itself when that is not one character.
pub(crate) fn fold_char(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_uppercase();
    }
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// The key two names share when they are equal ignoring case.
pub(crate) fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// Ordinal case-insensitive equality.
pub(crate) fn eq_ignore_case(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    a.chars().map(fold_char).eq(b.chars().map(fold_char))
}

/// Ordinal case-insensitive order.
pub(crate) fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    a.chars().map(fold_char).cmp(b.chars().map(fold_char))
}

/// Ordinal case-insensitive prefix test.
pub(crate) fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    let mut chars = text.chars();
    for p in prefix.chars() {
        match chars.next() {
            Some(c) if fold_char(c) == fold_char(p) => {}
            _ => return false,
        }
    }
    true
}

/// Whether `needle` starts at byte `start` of `text`, ignoring case;
/// the byte just past the match when it does.
fn match_at_ignore_case(text: &str, start: usize, needle: &str) -> Option<usize> {
    let mut chars = text[start..].char_indices();
    let mut end = start;
    for n in needle.chars() {
        match chars.next() {
            Some((offset, c)) if fold_char(c) == fold_char(n) => {
                end = start + offset + c.len_utf8();
            }
            _ => return None,
        }
    }
    Some(end)
}

/// Ordinal case-insensitive suffix test.
pub(crate) fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    let count = suffix.chars().count();
    let Some((start, _)) = text.char_indices().rev().nth(count.wrapping_sub(1)) else {
        return count == 0;
    };
    count > 0 && match_at_ignore_case(text, start, suffix) == Some(text.len())
}

/// The byte index of the first `needle` in `text`, ignoring case.
pub(crate) fn find_ignore_case(text: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    text.char_indices()
        .map(|(i, _)| i)
        .find(|&i| match_at_ignore_case(text, i, needle).is_some())
}

/// The byte index of the last `needle` in `text`, ignoring case.
pub(crate) fn rfind_ignore_case(text: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(text.len());
    }
    text.char_indices()
        .map(|(i, _)| i)
        .rev()
        .find(|&i| match_at_ignore_case(text, i, needle).is_some())
}

/// Ordinal case-insensitive containment.
pub(crate) fn contains_ignore_case(text: &str, needle: &str) -> bool {
    find_ignore_case(text, needle).is_some()
}

/// The white space .NET number parsing skips at either end.
fn is_number_white(c: char) -> bool {
    matches!(c, '\u{9}'..='\u{d}' | ' ')
}

fn trim_number(text: &str) -> &str {
    text.trim_matches(|c: char| is_number_white(c) || c == '\0')
}

/// `double.TryParse(text, NumberStyles.Float, InvariantCulture)`; NaN when
/// it fails, as the C# helper returned.
pub(crate) fn parse_float(text: Option<&str>) -> f64 {
    text.and_then(|t| parse_float_with(t, false))
        .unwrap_or(f64::NAN)
}

/// `double.Parse(text, InvariantCulture)`: the float rules plus group
/// separators (`1,234.5`) in the integer part.
pub(crate) fn parse_float_strict(text: &str) -> Option<f64> {
    parse_float_with(text, true)
}

fn parse_float_with(text: &str, allow_thousands: bool) -> Option<f64> {
    let body = trim_number(text);
    let (negative, unsigned) = match body.as_bytes().first() {
        Some(b'-') => (true, &body[1..]),
        Some(b'+') => (false, &body[1..]),
        _ => (false, body),
    };
    if unsigned.eq_ignore_ascii_case("infinity") {
        return Some(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if unsigned.eq_ignore_ascii_case("nan") {
        return Some(f64::NAN);
    }
    let mut cleaned = String::with_capacity(unsigned.len() + 1);
    if negative {
        cleaned.push('-');
    }
    let mut digits = 0usize;
    let mut seen_point = false;
    let mut chars = unsigned.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '0'..='9' => {
                digits += 1;
                cleaned.push(c);
            }
            '.' if !seen_point => {
                seen_point = true;
                cleaned.push(c);
            }
            ',' if allow_thousands && !seen_point && digits > 0 => {}
            _ => break,
        }
        chars.next();
    }
    if digits == 0 {
        return None;
    }
    if let Some(&e) = chars.peek()
        && (e == 'e' || e == 'E')
    {
        chars.next();
        cleaned.push('e');
        if let Some(&sign) = chars.peek()
            && (sign == '+' || sign == '-')
        {
            chars.next();
            cleaned.push(sign);
        }
        let mut exponent_digits = 0;
        while let Some(&c) = chars.peek() {
            if !c.is_ascii_digit() {
                break;
            }
            exponent_digits += 1;
            cleaned.push(c);
            chars.next();
        }
        if exponent_digits == 0 {
            return None;
        }
    }
    if chars.next().is_some() {
        return None;
    }
    cleaned.parse().ok()
}

/// `int.TryParse(text, NumberStyles.Integer, InvariantCulture)`.
pub(crate) fn parse_int(text: Option<&str>) -> Option<i32> {
    let body = trim_number(text?);
    if body.is_empty() {
        return None;
    }
    body.parse().ok()
}

/// `int.TryParse(text, NumberStyles.HexNumber, InvariantCulture)`: hex
/// digits only, no prefix or sign, at most 32 bits (`FFFFFFFF` is -1).
pub(crate) fn parse_hex_int(text: Option<&str>) -> Option<i32> {
    parse_hex_u32(text?).map(|v| v as i32)
}

/// `uint.TryParse(text, NumberStyles.HexNumber, InvariantCulture)`.
pub(crate) fn parse_hex_u32(text: &str) -> Option<u32> {
    let body = trim_number(text);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let significant = body.trim_start_matches('0');
    if significant.len() > 8 {
        return None;
    }
    if significant.is_empty() {
        return Some(0);
    }
    u32::from_str_radix(significant, 16).ok()
}

/// `bool.TryParse`: `true` or `false` in any case, white space around.
pub(crate) fn parse_bool(text: Option<&str>) -> Option<bool> {
    let body = text?.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if body.eq_ignore_ascii_case("true") {
        Some(true)
    } else if body.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// A .NET custom number format of the shape `0.0`, `0.00`, `0.##` or `0`:
/// at least `min_decimals` and at most `max_decimals` digits after the
/// point. Like .NET, the value is first taken to 15 significant digits,
/// then rounded half away from zero at the last place shown.
pub(crate) fn format_fixed(value: f64, min_decimals: usize, max_decimals: usize) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let scientific = format!("{:.14e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    // Place the point: digits[0] is the 10^exponent place.
    let (mut integer, mut fraction): (Vec<u8>, Vec<u8>) = if exponent >= 0 {
        let split = (exponent + 1) as usize;
        let mut integer: Vec<u8> = digits.iter().take(split).copied().collect();
        while integer.len() < split {
            integer.push(b'0');
        }
        (integer, digits.iter().skip(split).copied().collect())
    } else {
        let mut fraction = vec![b'0'; (-exponent - 1) as usize];
        fraction.extend_from_slice(&digits);
        (vec![b'0'], fraction)
    };
    if fraction.len() > max_decimals {
        let round_up = fraction[max_decimals] >= b'5';
        fraction.truncate(max_decimals);
        if round_up {
            let mut carry = true;
            for d in fraction.iter_mut().rev().chain(integer.iter_mut().rev()) {
                if *d == b'9' {
                    *d = b'0';
                } else {
                    *d += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                integer.insert(0, b'1');
            }
        }
    }
    while fraction.len() > min_decimals && fraction.last() == Some(&b'0') {
        fraction.pop();
    }
    while fraction.len() < min_decimals {
        fraction.push(b'0');
    }
    while integer.len() > 1 && integer[0] == b'0' {
        integer.remove(0);
    }
    let mut out = String::new();
    let is_zero = integer.iter().chain(fraction.iter()).all(|&d| d == b'0');
    if value < 0.0 && !is_zero {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&integer).unwrap_or("0"));
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(std::str::from_utf8(&fraction).unwrap_or(""));
    }
    out
}

/// .NET's default `double.ToString()` (the shortest text that reads back
/// as the same value): positional from 0.0001 up to 1E+15, scientific
/// (`1E-05`, `1.5E+20`) outside.
pub(crate) fn format_roundtrip(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if (-4..15).contains(&exponent) {
        return format!("{value}");
    }
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}E{sign}{:02}", exponent.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_formats_round_like_dotnet() {
        assert_eq!(format_fixed(42.1, 1, 1), "42.1");
        assert_eq!(format_fixed(82.5, 2, 2), "82.50");
        assert_eq!(format_fixed(0.125, 2, 2), "0.13");
        assert_eq!(format_fixed(1.005, 2, 2), "1.01");
        assert_eq!(format_fixed(31.2, 0, 0), "31");
        assert_eq!(format_fixed(999.5, 0, 0), "1000");
        assert_eq!(format_fixed(1.2, 0, 2), "1.2");
        assert_eq!(format_fixed(1.0, 0, 2), "1");
        assert_eq!(format_fixed(0.0, 1, 1), "0.0");
        assert_eq!(format_fixed(0.0004, 0, 3), "0");
    }

    #[test]
    fn roundtrip_matches_dotnet_default() {
        assert_eq!(format_roundtrip(10.0), "10");
        assert_eq!(format_roundtrip(42.1), "42.1");
        assert_eq!(format_roundtrip(-96.6), "-96.6");
        assert_eq!(format_roundtrip(0.00001), "1E-05");
        assert_eq!(format_roundtrip(1e15), "1E+15");
        assert_eq!(format_roundtrip(0.0001), "0.0001");
    }

    #[test]
    fn numbers_parse_like_dotnet() {
        assert_eq!(parse_float(Some(" 1.5 ")), 1.5);
        assert_eq!(parse_float(Some(".5")), 0.5);
        assert_eq!(parse_float(Some("-2e1")), -20.0);
        assert!(parse_float(Some("1,5")).is_nan());
        assert_eq!(parse_float_strict("1,234.5"), Some(1234.5));
        assert!(parse_float(Some("abc")).is_nan());
        assert!(parse_float(None).is_nan());
        assert_eq!(parse_hex_int(Some("ABCD")), Some(0xABCD));
        assert_eq!(parse_hex_int(Some("FFFFFFFF")), Some(-1));
        assert_eq!(parse_hex_int(Some("0x10")), None);
        assert_eq!(parse_hex_int(Some("100000000")), None);
        assert_eq!(parse_int(Some(" 42 ")), Some(42));
        assert_eq!(parse_bool(Some("TRUE")), Some(true));
        assert_eq!(parse_bool(Some("yes")), None);
    }

    #[test]
    fn case_rules_are_ordinal() {
        assert!(eq_ignore_case("Holtburg", "HOLTBURG"));
        assert!(starts_with_ignore_case("Cave to Town", "cave to "));
        assert_eq!(cmp_ignore_case("a", "B"), Ordering::Less);
    }
}
