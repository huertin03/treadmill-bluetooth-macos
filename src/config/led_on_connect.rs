//! On-connect LED strip default (`led_on_connect`).

use tracing::warn;

use super::file::{config_path, read_config_value};
use crate::led::LedState;

/// Top-level `config.toml` key for the LED strip state applied on every
/// treadmill connect (задача 059). Quoted string: `"off"` / `"on"` / `"none"`.
pub const LED_ON_CONNECT_KEY: &str = "led_on_connect";

/// Parse outcome of the optional `led_on_connect` key (задача 059). Kept
/// distinct like [`ShowSpeedSetting`] so the caller logs only the anomalous
/// (present-but-invalid) case. Explicit `"none"` is a first-class value: the
/// upsert helper cannot delete a key, so the CLI writes `"none"` to mean
/// "leave the strip alone" — same as the key being absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LedOnConnectSetting {
    /// Key present as `"on"` or `"off"`.
    Configured(LedState),
    /// Key present as `"none"` — same write-target as absent.
    None,
    /// Key present but not `"on"`/`"off"`/`"none"`, or the file is unreadable.
    Invalid,
    /// Key absent (normal — most configs never set this).
    Unset,
}

/// Pure decision (задача 059): a parsed `led_on_connect` value becomes a strip
/// write target, or `None` to leave the treadmill's current state. Invalid is
/// treated as no-op here; the loader WARNs before calling this.
fn led_on_connect_target(setting: LedOnConnectSetting) -> Option<LedState> {
    match setting {
        LedOnConnectSetting::Configured(state) => Some(state),
        LedOnConnectSetting::None | LedOnConnectSetting::Unset | LedOnConnectSetting::Invalid => {
            None
        }
    }
}

/// Human label for a loaded `led_on_connect` value (`off`/`on`/`none`).
#[must_use]
pub fn format_led_on_connect(value: Option<LedState>) -> &'static str {
    match value {
        Some(LedState::On) => "on",
        Some(LedState::Off) => "off",
        None => "none",
    }
}

/// Read `led_on_connect` from the per-user config. Pure and unit-tested — the
/// logging/fallback decision lives in [`load_led_on_connect`]. Parse via
/// [`LedState::from_str`]; `"none"` is the explicit no-op (upsert cannot
/// delete a top-level key).
fn read_led_on_connect(path: &std::path::Path) -> LedOnConnectSetting {
    let Some(value) = read_config_value(path) else {
        return LedOnConnectSetting::Invalid;
    };
    match value.get(LED_ON_CONNECT_KEY) {
        None => LedOnConnectSetting::Unset,
        Some(v) => match v.as_str() {
            Some("none") => LedOnConnectSetting::None,
            Some(s) => match s.parse::<LedState>() {
                Ok(state) => LedOnConnectSetting::Configured(state),
                Err(_) => LedOnConnectSetting::Invalid,
            },
            None => LedOnConnectSetting::Invalid,
        },
    }
}

/// Load the LED-strip state to apply on every treadmill connect (задача 059).
/// `None` = do nothing (key absent, explicit `"none"`, missing file, or
/// invalid). Logging is quiet on the common paths; only a present-but-invalid
/// value is an anomaly worth a WARN — and even then we refuse to guess a
/// write, rather than defaulting to off.
pub fn load_led_on_connect() -> Option<LedState> {
    match config_path() {
        Some(path) if path.exists() => match read_led_on_connect(&path) {
            LedOnConnectSetting::Invalid => {
                warn!(
                    path = %path.display(),
                    "led_on_connect present but not \"on\", \"off\", or \"none\" — ignoring (no strip write)",
                );
                None
            }
            other => led_on_connect_target(other),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_led_on_connect_distinguishes_configured_none_absent_and_invalid() {
        let dir =
            std::env::temp_dir().join(format!("tm-led-on-connect-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let off = dir.join("off.toml");
        std::fs::write(&off, "goals = [8000]\nled_on_connect = \"off\"\n").unwrap();
        assert_eq!(
            read_led_on_connect(&off),
            LedOnConnectSetting::Configured(LedState::Off)
        );

        let on = dir.join("on.toml");
        std::fs::write(&on, "led_on_connect = \"on\"\n").unwrap();
        assert_eq!(
            read_led_on_connect(&on),
            LedOnConnectSetting::Configured(LedState::On)
        );

        let none = dir.join("none.toml");
        std::fs::write(&none, "led_on_connect = \"none\"\n").unwrap();
        assert_eq!(read_led_on_connect(&none), LedOnConnectSetting::None);

        let absent = dir.join("absent.toml");
        std::fs::write(&absent, "goals = [8000]\n").unwrap();
        assert_eq!(read_led_on_connect(&absent), LedOnConnectSetting::Unset);

        // Boolean / unknown string / broken file → Invalid (caller WARNs, no write).
        let bool_val = dir.join("bool.toml");
        std::fs::write(&bool_val, "led_on_connect = true\n").unwrap();
        assert_eq!(read_led_on_connect(&bool_val), LedOnConnectSetting::Invalid);
        let garbage = dir.join("garbage.toml");
        std::fs::write(&garbage, "led_on_connect = \"maybe\"\n").unwrap();
        assert_eq!(read_led_on_connect(&garbage), LedOnConnectSetting::Invalid);
        let junk = dir.join("junk.toml");
        std::fs::write(&junk, "not valid toml").unwrap();
        assert_eq!(read_led_on_connect(&junk), LedOnConnectSetting::Invalid);

        for f in [off, on, none, absent, bool_val, garbage, junk] {
            std::fs::remove_file(f).ok();
        }
    }

    #[test]
    fn led_on_connect_target_writes_only_on_or_off() {
        assert_eq!(
            led_on_connect_target(LedOnConnectSetting::Configured(LedState::Off)),
            Some(LedState::Off)
        );
        assert_eq!(
            led_on_connect_target(LedOnConnectSetting::Configured(LedState::On)),
            Some(LedState::On)
        );
        assert_eq!(led_on_connect_target(LedOnConnectSetting::None), None);
        assert_eq!(led_on_connect_target(LedOnConnectSetting::Unset), None);
        assert_eq!(led_on_connect_target(LedOnConnectSetting::Invalid), None);
    }

    #[test]
    fn format_led_on_connect_maps_option_to_wire_label() {
        assert_eq!(format_led_on_connect(Some(LedState::Off)), "off");
        assert_eq!(format_led_on_connect(Some(LedState::On)), "on");
        assert_eq!(format_led_on_connect(None), "none");
    }
}
