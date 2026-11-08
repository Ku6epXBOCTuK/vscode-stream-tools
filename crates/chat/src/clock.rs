use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_rfc3339() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_rfc3339(since_epoch.as_secs(), since_epoch.subsec_millis())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// NOTE: hand-rolled UTC formatting keeps the binary free of a date dependency,
// civil_from_days is Howard Hinnant's algorithm
pub fn format_rfc3339(secs: u64, millis: u32) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (hour, min, sec) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::format_rfc3339;

    #[test]
    fn epoch() {
        assert_eq!(format_rfc3339(0, 0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn leap_day() {
        assert_eq!(format_rfc3339(1_709_164_800, 5), "2024-02-29T00:00:00.005Z");
    }

    #[test]
    fn end_of_year() {
        assert_eq!(
            format_rfc3339(1_735_689_599, 999),
            "2024-12-31T23:59:59.999Z"
        );
    }
}
