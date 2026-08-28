//! Widget live-speed toggle (`show_speed`).

use tracing::warn;

use super::file::{config_path, read_config_value};

/// Default for the `show_speed` widget toggle (задача 029): off, so the
/// widget's live-speed field stays opt-in — `tm speed-widget on` enables it.
pub const DEFAULT_SHOW_SPEED: bool = false;

/// Parse outcome of the optional `show_speed` key (задача 029). Kept distinct
/// like [`GapSetting`]/[`AutoPauseSetting`] so the caller logs only the
/// anomalous (present-but-invalid) case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShowSpeedSetting {
    /// Key present and a boolean.
    Configured(bool),
    /// Key present but not a boolean, or the file is unreadable/malformed.
    Invalid,
    /// Key absent (normal — most configs never set this).
    Unset,
}

/// Read `show_speed` from the per-user config. Pure and unit-tested — the
/// logging/fallback decision lives in [`load_show_speed`].
fn read_show_speed(path: &std::path::Path) -> ShowSpeedSetting {
    let Some(value) = read_config_value(path) else {
        return ShowSpeedSetting::Invalid;
    };
    match value.get("show_speed") {
        None => ShowSpeedSetting::Unset,
        Some(v) => match v.as_bool() {
            Some(b) => ShowSpeedSetting::Configured(b),
            None => ShowSpeedSetting::Invalid,
        },
    }
}

/// Load the `show_speed` widget toggle (задача 029): whether `tm widget`
/// should populate its live belt-speed field. Falls back to
/// [`DEFAULT_SHOW_SPEED`] (off) when unconfigured or invalid. Read-time, like
/// `workout_gap_minutes`/`auto_pause_minutes` — loaded by `widget`, not the
/// daemon. Logging is quiet on the common paths (absent key, missing file);
/// only a present-but-invalid value is an anomaly worth a WARN.
pub fn load_show_speed() -> bool {
    match config_path() {
        Some(path) if path.exists() => match read_show_speed(&path) {
            ShowSpeedSetting::Configured(enabled) => enabled,
            ShowSpeedSetting::Unset => DEFAULT_SHOW_SPEED,
            ShowSpeedSetting::Invalid => {
                warn!(
                    path = %path.display(),
                    default = DEFAULT_SHOW_SPEED,
                    "show_speed present but not a boolean — using default",
                );
                DEFAULT_SHOW_SPEED
            }
        },
        // No file / no resolvable path is the normal uncustomised case.
        _ => DEFAULT_SHOW_SPEED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_show_speed_distinguishes_configured_absent_and_invalid() {
        let dir = std::env::temp_dir().join(format!("tm-showspeed-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let on = dir.join("on.toml");
        std::fs::write(&on, "goals = [8000]\nshow_speed = true\n").unwrap();
        assert_eq!(read_show_speed(&on), ShowSpeedSetting::Configured(true));

        let off = dir.join("off.toml");
        std::fs::write(&off, "show_speed = false\n").unwrap();
        assert_eq!(read_show_speed(&off), ShowSpeedSetting::Configured(false));

        // Key absent — normal, most configs never set this.
        let absent = dir.join("absent.toml");
        std::fs::write(&absent, "goals = [8000]\n").unwrap();
        assert_eq!(read_show_speed(&absent), ShowSpeedSetting::Unset);

        // Present but not a boolean → Invalid (caller WARNs + defaults).
        let str_val = dir.join("str.toml");
        std::fs::write(&str_val, "show_speed = \"yes\"\n").unwrap();
        assert_eq!(read_show_speed(&str_val), ShowSpeedSetting::Invalid);

        let junk = dir.join("junk.toml");
        std::fs::write(&junk, "not valid toml").unwrap();
        assert_eq!(read_show_speed(&junk), ShowSpeedSetting::Invalid);

        for f in [on, off, absent, str_val, junk] {
            std::fs::remove_file(f).ok();
        }
    }
}
