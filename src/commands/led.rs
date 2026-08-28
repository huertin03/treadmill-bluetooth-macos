//! `tm led` dispatch: live toggle via the control queue, `default` writes config.

use anyhow::Result;

use crate::LedAction;
use crate::LedDefaultValue;
use crate::commands::belt::run_control;
use crate::commands::common::{highlight_config, zone_hold_config_path};
use crate::control_command::ControlCommand;
use crate::goals;
use crate::led::LedState;

pub(crate) async fn run_led(action: LedAction) -> Result<()> {
    match action {
        LedAction::On => run_control(ControlCommand::Led(LedState::On)).await,
        LedAction::Off => run_control(ControlCommand::Led(LedState::Off)).await,
        LedAction::Default { value } => run_led_default(value),
    }
}

fn run_led_default(value: Option<LedDefaultValue>) -> Result<()> {
    match value {
        None => {
            let label = goals::format_led_on_connect(goals::load_led_on_connect());
            println!("led on connect: {}", highlight_config(label));
            Ok(())
        }
        Some(value) => set_led_on_connect(value),
    }
}

fn set_led_on_connect(value: LedDefaultValue) -> Result<()> {
    let path = zone_hold_config_path()?;
    // Quoted TOML string; upsert cannot delete a key, so `none` is written
    // as the explicit no-op the loader treats as absent.
    let toml_value = format!("\"{}\"", value.as_str());
    goals::upsert_top_level_key(&path, goals::LED_ON_CONNECT_KEY, &toml_value)?;
    println!(
        "led on connect set to {}.",
        highlight_config(value.as_str())
    );
    Ok(())
}
