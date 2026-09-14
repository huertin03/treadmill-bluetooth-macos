//! Explicit start target: validation and a bounded, testable command sequence.
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use btleplug::api::Peripheral as _;
use btleplug::platform::Peripheral;
use tokio::time::timeout;
use uuid::uuid;

use crate::{control::Controller, speed::CentiKmh};

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(3);
const START_TIMEOUT: Duration = Duration::from_secs(8);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const EXEC_TIMEOUT: Duration = Duration::from_secs(19);
pub(crate) const POLL_TIMEOUT: Duration = Duration::from_secs(25);

pub(crate) fn parse_target(raw: &str) -> Result<CentiKmh> {
    let kmh: f32 = raw.parse().context("expected speed in km/h")?;
    ensure!(
        kmh.is_finite() && kmh > 0.0 && kmh <= 25.0,
        "start speed must be finite, positive and at most 25 km/h"
    );
    let speed = CentiKmh::from_kmh_f32(kmh).context("invalid start speed")?;
    ensure!(speed > CentiKmh::ZERO, "start speed rounds to zero");
    Ok(speed)
}

fn validate_range(bytes: &[u8], target: CentiKmh) -> Result<()> {
    ensure!(bytes.len() == 6, "malformed Supported Speed Range");
    let min = u16::from_le_bytes([bytes[0], bytes[1]]);
    let max = u16::from_le_bytes([bytes[2], bytes[3]]);
    let step = u16::from_le_bytes([bytes[4], bytes[5]]);
    ensure!(min <= max && step > 0, "invalid Supported Speed Range");
    let raw = target.to_wire();
    ensure!(
        raw >= min && raw <= max && (raw - min).is_multiple_of(step),
        "target {target} km/h is outside the device range/increment"
    );
    Ok(())
}

pub(crate) async fn run(peripheral: &Peripheral, target: CentiKmh) -> Result<()> {
    // Validate again at the execution boundary (including persisted commands).
    parse_target(&target.to_string())?;
    let characteristic = peripheral
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == uuid!("00002ad4-0000-1000-8000-00805f9b34fb"))
        .context("Supported Speed Range missing; explicit start not attempted")?;
    let range = timeout(PREFLIGHT_TIMEOUT, peripheral.read(&characteristic))
        .await
        .context("speed range read timed out; start not attempted")??;
    validate_range(&range, target)?;
    let controller = timeout(PREFLIGHT_TIMEOUT, Controller::take_control(peripheral))
        .await
        .context("request control timed out; start not attempted")??;
    start_with_target(&controller, target).await
}

trait StartControl {
    async fn start(&self) -> Result<()>;
    async fn speed(&self, target: CentiKmh) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}

impl StartControl for Controller<'_> {
    async fn start(&self) -> Result<()> {
        Controller::start(self).await
    }
    async fn speed(&self, target: CentiKmh) -> Result<()> {
        self.set_speed(target).await
    }
    async fn stop(&self) -> Result<()> {
        Controller::stop(self).await
    }
}

async fn start_with_target(control: &impl StartControl, target: CentiKmh) -> Result<()> {
    let attempt = timeout(START_TIMEOUT, async {
        control.start().await.context("Start not acknowledged")?;
        control
            .speed(target)
            .await
            .context("target speed not acknowledged")
    })
    .await;
    let failure = match attempt {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(err)) => format!("{err:#}"),
        Err(_) => "start/target sequence timed out".to_string(),
    };
    // An absent acknowledgement does not prove the belt stayed stopped.
    let cleanup = match timeout(CLEANUP_TIMEOUT, control.stop()).await {
        Ok(Ok(())) => "Stop acknowledged; verify the belt has stopped".to_string(),
        Ok(Err(err)) => format!("Stop failed: {err:#}; use the physical remote to stop"),
        Err(_) => "Stop timed out; use the physical remote to stop".to_string(),
    };
    bail!("{failure}; {cleanup}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        calls: RefCell<Vec<&'static str>>,
        fail: &'static str,
    }
    impl StartControl for Fake {
        async fn start(&self) -> Result<()> {
            self.calls.borrow_mut().push("start");
            ensure!(self.fail != "start", "rejected");
            Ok(())
        }
        async fn speed(&self, target: CentiKmh) -> Result<()> {
            self.calls.borrow_mut().push("speed");
            assert_eq!(target, CentiKmh::from_wire(400));
            if self.fail == "timeout" {
                std::future::pending::<()>().await;
            }
            ensure!(
                !matches!(self.fail, "speed" | "stop" | "stop-timeout"),
                "rejected"
            );
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            self.calls.borrow_mut().push("stop");
            if self.fail == "stop-timeout" {
                std::future::pending::<()>().await;
            }
            ensure!(self.fail != "stop", "disconnected");
            Ok(())
        }
    }

    #[test]
    fn rejects_invalid_input_and_device_targets_before_start() {
        for raw in ["NaN", "inf", "-1", "0", "0.001", "25.001", "fast"] {
            assert!(parse_target(raw).is_err(), "{raw}");
        }
        assert_eq!(parse_target("4").unwrap(), CentiKmh::from_wire(400));
        // W2 Pro snapshot: 0.50–6.10, increment 0.10 km/h.
        let range = [0x32, 0, 0x62, 2, 10, 0];
        for raw in [50, 400, 610] {
            assert!(validate_range(&range, CentiKmh::from_wire(raw)).is_ok());
        }
        for raw in [0, 49, 401, 620, 2500] {
            assert!(validate_range(&range, CentiKmh::from_wire(raw)).is_err());
        }
        for bytes in [&range[..5], &[50, 0, 0, 0, 10, 0], &[50, 0, 100, 0, 0, 0]] {
            assert!(validate_range(bytes, CentiKmh::from_wire(400)).is_err());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn ordered_sequence_and_bounded_failure_cleanup() {
        for fail in ["", "start", "speed", "timeout", "stop", "stop-timeout"] {
            let fake = Fake {
                calls: RefCell::new(vec![]),
                fail,
            };
            let started = tokio::time::Instant::now();
            let result = start_with_target(&fake, CentiKmh::from_wire(400)).await;
            assert!(started.elapsed() <= START_TIMEOUT + CLEANUP_TIMEOUT);
            if fail.is_empty() {
                assert!(result.is_ok());
                assert_eq!(*fake.calls.borrow(), ["start", "speed"]);
            } else {
                let message = result.unwrap_err().to_string();
                assert!(message.contains("Stop"));
                if fail.starts_with("stop") {
                    assert!(message.contains("physical remote"));
                }
                let expected = if fail == "start" {
                    vec!["start", "stop"]
                } else {
                    vec!["start", "speed", "stop"]
                };
                assert_eq!(*fake.calls.borrow(), expected);
            }
        }
    }
}
