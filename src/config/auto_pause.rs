//! Idle-belt auto-pause threshold (`auto_pause_minutes`).

use std::time::Duration;

use tracing::warn;

use super::file::{config_path, read_config_value};

/// Default idle-belt auto-pause threshold in minutes (задача 020): once the belt
/// has run `AwayWhileRunning` (nobody walking) this long, the daemon pauses it so
/// the machine's own built-in shutoff can then power it down. Used when the
/// config file is missing or the `auto_pause_minutes` key is absent. A configured
/// `0` disables auto-pause entirely.
pub const DEFAULT_AUTO_PAUSE_MINUTES: i64 = 5;

/// Parse outcome of the optional `auto_pause_minutes` key (задача 020). Kept
/// distinct like [`GapSetting`] so the caller logs only the anomalous
/// (present-but-invalid) case, not the normal absent one. Note `0` is a *valid*
/// value here (explicitly disables auto-pause), unlike `workout_gap_minutes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoPauseSetting {
    /// Key present and a non-negative integer (`0` = disabled).
    Configured(i64),
    /// Key present but not a non-negative integer, or the file is unreadable/malformed.
    Invalid,
    /// Key absent (normal for a config written before this key existed).
    Unset,
}

/// Read `auto_pause_minutes` from the per-user config. Pure and unit-tested —
/// the logging/fallback decision lives in [`load_auto_pause`].
fn read_auto_pause_minutes(path: &std::path::Path) -> AutoPauseSetting {
    let Some(value) = read_config_value(path) else {
        return AutoPauseSetting::Invalid;
    };
    match value.get("auto_pause_minutes") {
        None => AutoPauseSetting::Unset,
        // `0` disables (kept), negatives/non-integers are a config mistake.
        Some(v) => match v.as_integer() {
            Some(n) if n >= 0 => AutoPauseSetting::Configured(n),
            _ => AutoPauseSetting::Invalid,
        },
    }
}

/// Load the idle-belt auto-pause threshold (задача 020): `Some(duration)` when
/// enabled, `None` when disabled (configured `0`). Falls back to
/// [`DEFAULT_AUTO_PAUSE_MINUTES`] when the key is absent or invalid.
///
/// Like [`load_workout_gap_minutes`], logging is quiet on the common paths (an
/// absent key and a missing file are normal); only a present-but-invalid value
/// is an anomaly worth a WARN. The daemon reloads this on the goals-config
/// mtime watch (задача 017), so an edit takes effect without a restart.
pub fn load_auto_pause() -> Option<Duration> {
    let minutes = match config_path() {
        Some(path) if path.exists() => match read_auto_pause_minutes(&path) {
            AutoPauseSetting::Configured(minutes) => minutes,
            AutoPauseSetting::Unset => DEFAULT_AUTO_PAUSE_MINUTES,
            AutoPauseSetting::Invalid => {
                warn!(
                    path = %path.display(),
                    default = DEFAULT_AUTO_PAUSE_MINUTES,
                    "auto_pause_minutes present but not a non-negative integer — using default",
                );
                DEFAULT_AUTO_PAUSE_MINUTES
            }
        },
        // No file / no resolvable path is the normal uncustomised case.
        _ => DEFAULT_AUTO_PAUSE_MINUTES,
    };
    // 0 = explicitly disabled; any positive count is a real threshold.
    (minutes > 0).then(|| Duration::from_secs(minutes as u64 * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_auto_pause_minutes_distinguishes_configured_disabled_absent_and_invalid() {
        let dir = std::env::temp_dir().join(format!("tm-autopause-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let good = dir.join("good.toml");
        std::fs::write(&good, "goals = [8000]\nauto_pause_minutes = 7\n").unwrap();
        assert_eq!(
            read_auto_pause_minutes(&good),
            AutoPauseSetting::Configured(7)
        );

        // 0 is a valid value here — it disables auto-pause (not Invalid).
        let disabled = dir.join("disabled.toml");
        std::fs::write(&disabled, "auto_pause_minutes = 0\n").unwrap();
        assert_eq!(
            read_auto_pause_minutes(&disabled),
            AutoPauseSetting::Configured(0),
        );

        // Key absent — normal for a config written before задача 020.
        let absent = dir.join("absent.toml");
        std::fs::write(&absent, "goals = [8000]\n").unwrap();
        assert_eq!(read_auto_pause_minutes(&absent), AutoPauseSetting::Unset);

        // Negative / non-integer → Invalid (caller WARNs + defaults).
        let neg = dir.join("neg.toml");
        std::fs::write(&neg, "auto_pause_minutes = -3\n").unwrap();
        assert_eq!(read_auto_pause_minutes(&neg), AutoPauseSetting::Invalid);
        let str_val = dir.join("str.toml");
        std::fs::write(&str_val, "auto_pause_minutes = \"5\"\n").unwrap();
        assert_eq!(read_auto_pause_minutes(&str_val), AutoPauseSetting::Invalid);
        let junk = dir.join("junk.toml");
        std::fs::write(&junk, "not valid toml").unwrap();
        assert_eq!(read_auto_pause_minutes(&junk), AutoPauseSetting::Invalid);

        for f in [good, disabled, absent, neg, str_val, junk] {
            std::fs::remove_file(f).ok();
        }
    }
}
