//! Apply `led_on_connect` after a treadmill BLE connect (задача 059).

use anyhow::Result;
use btleplug::platform::Peripheral;
use tracing::{info, warn};

use super::SPEED_RESTORE_TIMEOUT;
use crate::control::Controller;
use crate::led::LedState;

/// Once-per-session LED write after connect. `None` = key absent / `"none"` —
/// leave the strip as the treadmill currently has it. Failure is WARN-and-
/// continue: the strip is cosmetic and must never tear down telemetry.
pub(super) async fn try_apply_led_on_connect(
    peripheral: &Peripheral,
    configured: Option<LedState>,
) {
    let Some(state) = configured else {
        return;
    };
    match tokio::time::timeout(SPEED_RESTORE_TIMEOUT, apply_led(peripheral, state)).await {
        Ok(Ok(())) => {
            info!(%state, "applied led_on_connect after treadmill connect");
        }
        Ok(Err(err)) => {
            warn!(%err, %state, "failed to apply led_on_connect — leaving strip as is");
        }
        Err(_) => {
            warn!(
                timeout_s = SPEED_RESTORE_TIMEOUT.as_secs(),
                %state,
                "led_on_connect timed out (possible CoreBluetooth hang)"
            );
        }
    }
}

/// Take FTMS control and write the strip. Split so the whole round-trip can
/// be wrapped in one bounded `timeout` in [`try_apply_led_on_connect`].
async fn apply_led(peripheral: &Peripheral, state: LedState) -> Result<()> {
    let controller = Controller::take_control(peripheral).await?;
    controller.set_led(state).await
}
