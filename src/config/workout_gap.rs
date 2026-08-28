//! Read-time workout-gap: adjacent activity segments → one displayed workout.

use tracing::warn;

use super::file::{config_path, read_config_value};

/// Default workout-gap in minutes (задача 014): adjacent activity segments
/// separated by a read-time gap ≤ this render as one workout. Used when the
/// config file is missing, the `workout_gap_minutes` key is absent, or its
/// value is invalid.
pub const DEFAULT_WORKOUT_GAP_MINUTES: i64 = 15;

/// Parse outcome of the optional `workout_gap_minutes` key. The three cases are
/// kept distinct so the caller can log the *anomalous* one (present-but-invalid)
/// without spamming on the *normal* one (key absent — every pre-014 config lacks
/// it) on the hot `tm widget` poll path. See [`load_workout_gap_minutes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GapSetting {
    /// Key present and a positive integer.
    Configured(i64),
    /// Key present but not a positive integer, or the file is unreadable/malformed.
    Invalid,
    /// Key absent (normal for a config written before this key existed).
    Unset,
}

/// Read `workout_gap_minutes` from the per-user config. Pure and unit-tested —
/// the logging/fallback decision lives in [`load_workout_gap_minutes`].
fn read_workout_gap_minutes(path: &std::path::Path) -> GapSetting {
    let Some(value) = read_config_value(path) else {
        return GapSetting::Invalid;
    };
    match value.get("workout_gap_minutes") {
        None => GapSetting::Unset,
        Some(v) => match v.as_integer() {
            Some(n) if n > 0 => GapSetting::Configured(n),
            _ => GapSetting::Invalid,
        },
    }
}

/// Load the configured workout-gap (minutes), falling back to
/// [`DEFAULT_WORKOUT_GAP_MINUTES`] when unconfigured or invalid. This is a
/// READ-TIME parameter (задача 014): it groups adjacent activity segments into
/// displayed workouts, so it is loaded by the read commands (`stats`, `status`,
/// `widget`), not the segment-writing daemon.
///
/// Logging is deliberately quiet on the common paths — this runs on `widget`'s
/// ~2s poll: an absent key (every pre-014 config) and a missing file are normal
/// and silent; only a present-but-invalid value is an anomaly worth a WARN.
pub fn load_workout_gap_minutes() -> i64 {
    match config_path() {
        Some(path) if path.exists() => match read_workout_gap_minutes(&path) {
            GapSetting::Configured(minutes) => minutes,
            GapSetting::Unset => DEFAULT_WORKOUT_GAP_MINUTES,
            GapSetting::Invalid => {
                warn!(
                    path = %path.display(),
                    default = DEFAULT_WORKOUT_GAP_MINUTES,
                    "workout_gap_minutes present but not a positive integer — using default",
                );
                DEFAULT_WORKOUT_GAP_MINUTES
            }
        },
        // No file / no resolvable path is the normal uncustomised case.
        _ => DEFAULT_WORKOUT_GAP_MINUTES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_workout_gap_minutes_distinguishes_configured_absent_and_invalid() {
        let dir = std::env::temp_dir().join(format!("tm-gap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let good = dir.join("good.toml");
        std::fs::write(&good, "goals = [8000]\nworkout_gap_minutes = 20\n").unwrap();
        assert_eq!(read_workout_gap_minutes(&good), GapSetting::Configured(20));

        // Key absent — normal for a config written before задача 014.
        let absent = dir.join("absent.toml");
        std::fs::write(&absent, "goals = [8000]\n").unwrap();
        assert_eq!(read_workout_gap_minutes(&absent), GapSetting::Unset);

        // Present but not a positive integer → Invalid (caller WARNs + defaults).
        let zero = dir.join("zero.toml");
        std::fs::write(&zero, "workout_gap_minutes = 0\n").unwrap();
        assert_eq!(read_workout_gap_minutes(&zero), GapSetting::Invalid);
        let neg = dir.join("neg.toml");
        std::fs::write(&neg, "workout_gap_minutes = -5\n").unwrap();
        assert_eq!(read_workout_gap_minutes(&neg), GapSetting::Invalid);
        let str_val = dir.join("str.toml");
        std::fs::write(&str_val, "workout_gap_minutes = \"15\"\n").unwrap();
        assert_eq!(read_workout_gap_minutes(&str_val), GapSetting::Invalid);

        // Malformed TOML → Invalid.
        let junk = dir.join("junk.toml");
        std::fs::write(&junk, "not valid toml").unwrap();
        assert_eq!(read_workout_gap_minutes(&junk), GapSetting::Invalid);

        for f in [good, absent, zero, neg, str_val, junk] {
            std::fs::remove_file(f).ok();
        }
    }
}
