//! Control-command queue drain on the live BLE link (задача 013/039).

use anyhow::Result;
use btleplug::platform::Peripheral;
use chrono::Utc;
use tracing::{info, warn};

use super::SPEED_RESTORE_TIMEOUT;
use crate::control::Controller;
use crate::control_command::ControlCommand;
use crate::store::Store;

use std::time::Duration;

/// Backstop poll cadence for the control-command queue while connected but
/// quiet (no telemetry-driven check). Commands are also processed at the end
/// of every telemetry sample (~1/s), so this only matters during rare silent
/// stretches; keep it snappy but idle-cheap.
pub(super) const CONTROL_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Who initiated a Control Point write (задача 039). Logged on every write so
/// mid-Hold CLI speed overrides are diagnosable; not a priority arbiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlSource {
    Zone,
    Cli,
    AutoPause,
    Restore,
    DefaultSpeed,
}

impl ControlSource {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Zone => "zone",
            Self::Cli => "cli",
            Self::AutoPause => "auto_pause",
            Self::Restore => "restore",
            Self::DefaultSpeed => "default_speed",
        }
    }
}

/// Execute at most one pending control command on the live BLE link (задача
/// 013). Silent on the empty path — this runs ~1/s, so no happy-path log.
///
/// Returns `true` for successful CLI Speed or any attempted explicit start,
/// including uncertain failures, to open the Zone Hold operator override.
///
/// Two safety properties: a *stale* command (queued long ago, or while the
/// daemon was disconnected) is failed without executing, so it can never fire
/// a surprise belt change when the daemon reconnects/restarts; and a failed or
/// timed-out BLE write is logged and recorded on the row, never propagated —
/// a control write must not tear down an otherwise-healthy session. DB errors
/// still propagate, matching the rest of the loop.
///
/// Drains one command per call so a burst cannot block the select loop for
/// N× the execution timeout (15s normally, 19s for explicit start);
/// the next is picked up on the following tick.
pub(super) async fn process_control_commands(
    peripheral: &Peripheral,
    store: &Store,
    link: &mut crate::treadmill_link::TreadmillLink,
) -> Result<bool> {
    let Some(queued) = store.next_pending_control_command()? else {
        return Ok(false);
    };

    let explicit_start = matches!(queued.command, ControlCommand::StartAt(_));
    if queued.command.is_stale_at(queued.created_at, Utc::now()) {
        warn!(id = queued.id, command = %queued.command.to_wire(), "control command is stale — failing without executing");
        store.mark_control_command_failed(queued.id, "stale, not executed")?;
        return Ok(false);
    }

    let source = ControlSource::Cli;
    // Also guard uncertain partial starts: auto-restore must not accelerate
    // the belt after a failed target/cleanup. Bare Start keeps its old behavior.
    if explicit_start {
        link.note_explicit_start(std::time::Instant::now());
    }
    let was_speed = matches!(
        queued.command,
        ControlCommand::Speed(_) | ControlCommand::StartAt(_)
    );
    let execution_timeout = if explicit_start {
        crate::start_speed::EXEC_TIMEOUT
    } else {
        SPEED_RESTORE_TIMEOUT
    };
    let command_wire = queued.command.to_wire();
    match tokio::time::timeout(
        execution_timeout,
        execute_control_command(peripheral, queued.command, source),
    )
    .await
    {
        Ok(Ok(())) => {
            info!(
                id = queued.id,
                command = %command_wire,
                control_source = source.as_str(),
                "executed queued control command"
            );
            store.mark_control_command_done(queued.id)?;
            Ok(was_speed)
        }
        Ok(Err(err)) => {
            warn!(
                %err,
                id = queued.id,
                command = %command_wire,
                control_source = source.as_str(),
                "queued control command write failed"
            );
            store.mark_control_command_failed(queued.id, &err.to_string())?;
            Ok(explicit_start)
        }
        Err(_) => {
            warn!(
                id = queued.id,
                timeout_s = execution_timeout.as_secs(),
                control_source = source.as_str(),
                "queued control command timed out (possible CoreBluetooth hang)"
            );
            store.mark_control_command_failed(
                queued.id,
                "execution timed out; belt state unknown, check the belt and use the physical remote if needed",
            )?;
            Ok(explicit_start)
        }
    }
}

/// Take FTMS control and run one command. Split out so the whole round-trip can
/// be wrapped in a single bounded `timeout` by the caller. Reuses the same
/// take-control path as `restore_speed` and any other Control Point write (see
/// `control::Controller`). `source` is for call-site logging only — this
/// function does not log (the caller owns success/fail messages).
pub(super) async fn execute_control_command(
    peripheral: &Peripheral,
    command: ControlCommand,
    _source: ControlSource,
) -> Result<()> {
    if let ControlCommand::StartAt(speed) = command {
        return crate::start_speed::run(peripheral, speed).await;
    }
    let controller = Controller::take_control(peripheral).await?;
    match command {
        ControlCommand::Start => controller.start().await,
        ControlCommand::StartAt(_) => unreachable!("handled before requesting control"),
        ControlCommand::Stop => controller.stop().await,
        ControlCommand::Speed(kmh) => controller.set_speed(kmh).await,
        ControlCommand::Led(state) => controller.set_led(state).await,
    }
}
