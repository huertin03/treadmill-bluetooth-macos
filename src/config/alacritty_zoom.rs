//! Optional top-level font automation keys: absent is quiet, invalid warns.
use super::file::{config_path, read_config_value};
use crate::alacritty_zoom::{ZoomConfig, is_valid_delta};

pub fn load_alacritty_zoom() -> ZoomConfig {
    let Some(path) = config_path().filter(|path| path.exists()) else {
        return ZoomConfig::default();
    };
    let Some(value) = read_config_value(&path) else {
        tracing::warn!(path = %path.display(), "cannot read Alacritty zoom config; using defaults");
        return ZoomConfig::default();
    };
    parse_config(&value)
}
fn parse_config(value: &toml::Value) -> ZoomConfig {
    let mut config = ZoomConfig::default();
    if let Some(value) = value.get("alacritty_zoom") {
        if let Some(enabled) = value.as_bool() {
            config.enabled = enabled;
        } else {
            tracing::warn!(%value, "invalid alacritty_zoom; expected boolean, using false");
        }
    }
    if let Some(value) = value.get("alacritty_zoom_pt") {
        match value
            .as_float()
            .or_else(|| value.as_integer().map(|n| n as f64))
            .filter(|pt| is_valid_delta(*pt))
        {
            Some(pt) => config.delta_pt = pt,
            None => {
                tracing::warn!(%value, default = config.delta_pt, "invalid alacritty_zoom_pt; expected finite 0 < pt <= 8, using default")
            }
        }
    }
    config
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loads_absent_valid_and_invalid_keys() {
        assert_eq!(
            parse_config(&toml::from_str::<toml::Value>("").unwrap()),
            ZoomConfig::default()
        );
        assert_eq!(
            parse_config(
                &toml::from_str::<toml::Value>("alacritty_zoom = true\nalacritty_zoom_pt = 1.25")
                    .unwrap()
            ),
            ZoomConfig {
                enabled: true,
                delta_pt: 1.25
            }
        );
        assert_eq!(
            parse_config(&toml::from_str::<toml::Value>("alacritty_zoom_pt = 8").unwrap()).delta_pt,
            8.0
        );
        for invalid in ["\"yes\"", "0", "-1", "8.01", "nan", "inf", "true"] {
            let value = toml::from_str::<toml::Value>(&format!(
                "alacritty_zoom = 'yes'\nalacritty_zoom_pt = {invalid}"
            ))
            .unwrap();
            assert_eq!(parse_config(&value), ZoomConfig::default());
        }
    }
}
