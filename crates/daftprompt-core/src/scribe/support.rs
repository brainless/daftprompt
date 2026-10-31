//! Small internal helpers: identifiers, content hashes, and timestamps.
//!
//! The graph contract stores these as plain strings. Rust owns them here; the
//! model never sees or produces them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// A process-local sequence used together with the clock to avoid collisions.
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// A unique-enough identifier for an in-memory graph record.
///
/// The database schema uses text primary keys. Including the clock and a
/// process-local counter keeps ids distinct across parses without pulling in
/// a random-number dependency. Tests only rely on uniqueness, never on a
/// particular value.
pub(crate) fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos:x}-{sequence:x}")
}

/// The lowercase hex SHA-256 digest of a payload's text.
pub(crate) fn content_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    format!("{digest:x}")
}

/// The current instant as an RFC 3339 UTC timestamp.
pub(crate) fn now() -> String {
    format(SystemTime::now())
}

/// Format `time` as an RFC 3339 UTC timestamp with second precision.
fn format(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`: a UNIX day count to a Gregorian date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_the_unix_epoch() {
        assert_eq!(format(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_known_instants() {
        let leap_day = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(format(leap_day), "2000-02-29T00:00:00Z");

        let later = UNIX_EPOCH + Duration::from_secs(1_791_000_000);
        assert_eq!(format(later), "2026-10-03T04:00:00Z");
    }

    #[test]
    fn hashes_payloads_with_sha256() {
        assert_eq!(
            content_hash(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            content_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn generated_ids_are_unique() {
        let first = new_id("node");
        let second = new_id("node");
        assert_ne!(first, second);
        assert!(first.starts_with("node-"));
    }
}
