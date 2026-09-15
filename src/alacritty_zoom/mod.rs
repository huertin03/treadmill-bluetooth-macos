//! Font-only Alacritty IPC automation with serialized recovery and daemon convergence.

pub mod ipc;
pub mod lock;
pub mod operations;
mod retry;
pub use retry::get_font_size;
pub mod worker;

use crate::presence::PresenceState;
use std::time::Duration;

pub const DEFAULT_ZOOM_PT: f64 = 0.625;
pub const MAX_ZOOM_PT: f64 = 8.0;
pub const SIZE_EPSILON: f64 = 1e-3;
pub const IPC_CALL_TIMEOUT: Duration = Duration::from_secs(2);
pub const IPC_MAX_ATTEMPTS: usize = 5;
pub const IPC_RETRY_BACKOFF: [u64; 4] = [50, 100, 200, 400];
pub const INSTANCE_RESCAN_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomConfig {
    pub enabled: bool,
    pub delta_pt: f64,
}
impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            delta_pt: DEFAULT_ZOOM_PT,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomWant {
    pub config: ZoomConfig,
    pub active: bool,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Applied {
    Unknown,
    Base,
    Zoomed { delta_pt: f64 },
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomOp {
    Revert,
    Apply { delta_pt: f64 },
}

pub fn plan(applied: Applied, want: &ZoomWant) -> Option<ZoomOp> {
    if want.config.enabled && want.active {
        if applied
            == (Applied::Zoomed {
                delta_pt: want.config.delta_pt,
            })
        {
            return None;
        }
        return Some(ZoomOp::Apply {
            delta_pt: want.config.delta_pt,
        });
    }
    if applied == Applied::Base {
        return None;
    }
    Some(ZoomOp::Revert)
}

pub fn zoom_intent(state: PresenceState) -> Option<bool> {
    match state {
        PresenceState::Walking => Some(true),
        PresenceState::Paused => Some(false),
        PresenceState::AwayWhileRunning | PresenceState::Unknown => None,
    }
}

pub fn is_valid_delta(pt: f64) -> bool {
    pt.is_finite() && pt > 0.0 && pt <= MAX_ZOOM_PT
}
pub fn sizes_match(a: f64, b: f64) -> bool {
    (a - b).abs() <= SIZE_EPSILON
}
pub fn parse_socket_pid(name: &str) -> Option<i32> {
    let digits = name.strip_prefix("Alacritty-")?.strip_suffix(".sock")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|pid| *pid > 0)
}
pub fn parse_font_size(json: &str) -> Option<f64> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get("font")?
        .get("size")?
        .as_f64()
        .filter(|pt| pt.is_finite() && *pt > 0.0)
}
pub fn format_font_size(pt: f64) -> String {
    format!("{pt:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_every_desired_state() {
        let rows = [
            (Applied::Unknown, false, false, Some(ZoomOp::Revert)),
            (Applied::Unknown, false, true, Some(ZoomOp::Revert)),
            (Applied::Unknown, true, false, Some(ZoomOp::Revert)),
            (Applied::Unknown, true, true, Some(ZoomOp::Apply { delta_pt: 0.625 })),
            (Applied::Base, false, false, None),
            (Applied::Base, false, true, None),
            (Applied::Base, true, false, None),
            (Applied::Base, true, true, Some(ZoomOp::Apply { delta_pt: 0.625 })),
            (Applied::Zoomed { delta_pt: 0.625 }, false, false, Some(ZoomOp::Revert)),
            (Applied::Zoomed { delta_pt: 0.625 }, false, true, Some(ZoomOp::Revert)),
            (Applied::Zoomed { delta_pt: 0.625 }, true, false, Some(ZoomOp::Revert)),
            (Applied::Zoomed { delta_pt: 0.625 }, true, true, None),
            (Applied::Zoomed { delta_pt: 1.0 }, false, false, Some(ZoomOp::Revert)),
            (Applied::Zoomed { delta_pt: 1.0 }, false, true, Some(ZoomOp::Revert)),
            (Applied::Zoomed { delta_pt: 1.0 }, true, false, Some(ZoomOp::Revert)),
            (Applied::Zoomed { delta_pt: 1.0 }, true, true, Some(ZoomOp::Apply { delta_pt: 0.625 })),
        ];
        for (applied, enabled, active, expected) in rows {
            let want = ZoomWant { config: ZoomConfig { enabled, delta_pt: 0.625 }, active };
            assert_eq!(plan(applied, &want), expected, "{applied:?}, {want:?}");
        }
    }
    #[test]
    fn preserves_walk_latch() {
        assert_eq!(zoom_intent(PresenceState::Walking), Some(true));
        assert_eq!(zoom_intent(PresenceState::Paused), Some(false));
        assert_eq!(zoom_intent(PresenceState::AwayWhileRunning), None);
        assert_eq!(zoom_intent(PresenceState::Unknown), None);
    }
    #[test]
    fn parses_and_formats() {
        assert_eq!(parse_socket_pid("Alacritty-48251.sock"), Some(48251));
        for bad in [
            "Alacritty-.sock",
            "Alacritty-x.sock",
            "Alacritty-1.log",
            "foo.sock",
            "Alacritty-0.sock",
            "Alacritty-+1.sock",
        ] {
            assert_eq!(parse_socket_pid(bad), None);
        }
        assert_eq!(parse_font_size(r#"{"font":{"size":14.0}}"#), Some(14.0));
        for bad in ["{}", r#"{"font":{"size":"14"}}"#, "invalid"] {
            assert_eq!(parse_font_size(bad), None);
        }
        for (pt, text) in [(14.625, "14.625"), (15.0, "15"), (14.62500001, "14.625")] {
            assert_eq!(format_font_size(pt), text);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support;
