//! Dates as truefit records them: YYYY-MM-DD in UTC, and the days since 1970-01-01.

/// Today's date in UTC, as YYYY-MM-DD.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil(secs / 86400)
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil(days: u64) -> String {
    let z = days + 719_468;
    let (era, doe) = (z / 146_097, z % 146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The days from 1970-01-01 to a YYYY-MM-DD date, or None if it is not one: civil's inverse.
pub fn days(date: &str) -> Option<u64> {
    let mut parts = date.splitn(3, '-').map(|p| p.parse::<u64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1970 {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let (era, yoe) = (y / 400, y % 400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe).checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(59), "1970-03-01");
        assert_eq!(civil(11_016), "2000-02-29");
        assert_eq!(civil(20_778), "2026-11-21");
        for n in [0, 59, 11_016, 20_778] {
            assert_eq!(days(&civil(n)), Some(n));
        }
        for bad in [
            "",
            "2026-13-01",
            "2026-11",
            "x-1-1",
            "1969-12-31",
            "2026-11-32",
        ] {
            assert_eq!(days(bad), None, "{bad}");
        }
    }
}
