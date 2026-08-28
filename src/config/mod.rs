//! Per-user TOML config layer.
//!
//! Resolution order: `TREADMILL_CONFIG` env →
//! `$HOME/.config/treadmill-bluetooth-macos/config.toml` → compiled defaults.
//! Every reader follows the absent-quiet / invalid-WARN convention: a missing
//! key is silent (normal for configs written before that key existed); a
//! present-but-malformed value is a `WARN` and the compiled default is used.

pub mod auto_pause;
pub mod file;
pub mod led_on_connect;
pub mod show_speed;
pub mod workout_gap;

pub use auto_pause::load_auto_pause;
pub(crate) use file::write_atomic;
pub use file::{config_mtime, upsert_top_level_key};
pub use led_on_connect::{LED_ON_CONNECT_KEY, format_led_on_connect, load_led_on_connect};
pub use show_speed::load_show_speed;
pub use workout_gap::load_workout_gap_minutes;
