//! Wall-clock times as the Atlas cache's metadata file writes them: the
//! ISO 8601 text .NET writes for a `DateTimeOffset`
//! (`2026-11-03T12:34:56.789+00:00`), read back, and a short date for the
//! window (`3 Nov 2026`).

const MS_PER_DAY: i64 = 86_400_000;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days since 1970-01-01 to (year, month 1-12, day 1-31), proleptic
/// Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) to days since 1970-01-01.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A UTC instant, milliseconds since the Unix epoch, as .NET writes a
/// `DateTimeOffset` at offset zero.
#[must_use]
pub fn iso8601_utc(unix_ms: i64) -> String {
    let days = unix_ms.div_euclid(MS_PER_DAY);
    let ms_of_day = unix_ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let seconds = ms_of_day / 1000;
    let millis = ms_of_day % 1000;
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if millis != 0 {
        let fraction = format!("{millis:03}");
        text.push('.');
        text.push_str(fraction.trim_end_matches('0'));
    }
    text.push_str("+00:00");
    text
}

fn number(text: &str) -> Option<i64> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Reads `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)`: milliseconds
/// since the Unix epoch, UTC.
#[must_use]
pub fn parse_iso8601(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = text.split_once(['T', 't', ' '])?;
    let mut parts = date.split('-');
    let year = number(parts.next()?)?;
    let month = u32::try_from(number(parts.next()?)?).ok()?;
    let day = u32::try_from(number(parts.next()?)?).ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset_minutes) = if let Some(clock) = rest.strip_suffix(['Z', 'z']) {
        (clock, 0)
    } else if let Some(at) = rest.rfind(['+', '-']) {
        let (clock, offset) = rest.split_at(at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = offset[1..].split_once(':')?;
        (clock, sign * (number(hours)? * 60 + number(minutes)?))
    } else {
        (rest, 0)
    };
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut fields = whole.split(':');
    let hours = number(fields.next()?)?;
    let minutes = number(fields.next()?)?;
    let seconds = number(fields.next()?)?;
    if fields.next().is_some() || hours > 23 || minutes > 59 || seconds > 60 {
        return None;
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        let digits: String = fraction.chars().chain("000".chars()).take(3).collect();
        number(&digits)?
    };
    let days = days_from_civil(year, month, day);
    Some(
        days * MS_PER_DAY + ((hours * 60 + minutes) * 60 + seconds) * 1000 + millis
            - offset_minutes * 60_000,
    )
}

/// A day as the window shows it: `3 Nov 2026`, in local time
/// (`offset_minutes` east of UTC).
#[must_use]
pub fn short_date(unix_ms: i64, offset_minutes: i32) -> String {
    let local = unix_ms + i64::from(offset_minutes) * 60_000;
    let (year, month, day) = civil_from_days(local.div_euclid(MS_PER_DAY));
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}
