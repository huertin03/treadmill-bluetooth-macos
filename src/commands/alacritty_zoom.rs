//! Direct font calibration and recovery; no BLE or daemon dependency.
use super::common::{highlight_config, zone_hold_config_path};
use crate::alacritty_zoom::ipc::{AlacrittyIpc, SystemIpc};
use crate::alacritty_zoom::operations::{ZoomCore, matches_record, record_identity};
use crate::alacritty_zoom::{ZoomConfig, ZoomOp, format_font_size, get_font_size, is_valid_delta};
use crate::{config, store::Store};
use anyhow::{Result, ensure};
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum ZoomAction {
    /// Enable walking font automation.
    On,
    /// Disable automation and restore recorded font sizes now.
    Off,
    /// Set a positive font delta in points, at most 8.
    Pt { value: f64 },
    /// Apply the configured delta now, even when disabled.
    Preview,
    /// Restore recorded base sizes, preserving other runtime overrides.
    Reset,
}
pub(crate) fn print_setting(config: ZoomConfig) {
    let label = if config.enabled {
        format!("on (+{} pt)", format_font_size(config.delta_pt))
    } else {
        "off".into()
    };
    println!("alacritty zoom: {}", highlight_config(label));
}
pub(crate) async fn run_zoom(action: Option<ZoomAction>) -> Result<()> {
    match action {
        Some(ZoomAction::On) => set_key("alacritty_zoom", "true"),
        Some(ZoomAction::Pt { value }) => {
            if !is_valid_delta(value) {
                tracing::warn!(
                    value,
                    "invalid Alacritty zoom delta; expected finite 0 < pt <= 8"
                );
            }
            ensure!(
                is_valid_delta(value),
                "font delta must be finite and 0 < pt <= 8"
            );
            // Persist the user's precision; only IPC values round to three decimals.
            set_key("alacritty_zoom_pt", &value.to_string())
        }
        action => {
            let config = config::load_alacritty_zoom();
            if matches!(action, Some(ZoomAction::Off)) {
                set_key("alacritty_zoom", "false")?;
            }
            let mut core = ZoomCore::new(SystemIpc::new(std::env::temp_dir(), Store::open()?)?);
            match action {
                None => print_probe(&mut core, config).await,
                Some(ZoomAction::Preview) => {
                    core.run_op(ZoomOp::Apply {
                        delta_pt: config.delta_pt,
                    })
                    .await?;
                    print_probe(&mut core, config).await?;
                    println!(
                        "preview: +{} pt; the daemon re-converges on the next presence transition.",
                        highlight_config(format_font_size(config.delta_pt))
                    );
                    ensure!(
                        core.failed.is_empty(),
                        "preview failed for some Alacritty instances; see warnings"
                    );
                    Ok(())
                }
                _ => {
                    core.run_op(ZoomOp::Revert).await?;
                    ensure!(
                        core.failed.is_empty(),
                        "font restore failed for some Alacritty instances; records retained for recovery"
                    );
                    println!("alacritty zoom: recorded font sizes restored.");
                    Ok(())
                }
            }
        }
    }
}
fn set_key(key: &str, value: &str) -> Result<()> {
    config::upsert_top_level_key(&zone_hold_config_path()?, key, value)?;
    println!("{key}: {}", highlight_config(value));
    Ok(())
}
async fn print_probe<I: AlacrittyIpc>(core: &mut ZoomCore<I>, config: ZoomConfig) -> Result<()> {
    print_setting(config);
    let path = core.ipc.lock_path().ok_or_else(|| anyhow::anyhow!("missing Alacritty zoom lock path"))?;
    let guard = crate::alacritty_zoom::lock::ZoomLock::try_acquire(&path)?;
    let instances = if guard.is_some() {
        core.discover_and_prune()?
    } else {
        println!("daemon operation in progress");
        core.ipc.discover_instances()?
    };
    if instances.is_empty() {
        println!("alacritty: not running");
    }
    for instance in instances {
        match get_font_size(&mut core.ipc, &instance).await {
            Ok(current) => {
                let record = core.ipc.zoom_records()?.into_iter().find(|record| {
                    record_identity(record) == instance.identity && matches_record(current, record)
                });
                let base = record.map_or(current, |record| record.base_pt);
                println!(
                    "alacritty: running pid {} — base {} pt → walking {} pt ({} pt)",
                    instance.identity.pid,
                    format_font_size(base),
                    format_font_size(base + config.delta_pt),
                    highlight_config(format!("+{}", format_font_size(config.delta_pt)))
                );
            }
            Err(error) => {
                if !core.ipc.is_live(&instance) {
                    tracing::debug!(pid = instance.identity.pid, %error, "Alacritty instance exited mid-probe");
                    if guard.is_some() {
                        core.ipc.delete_zoom_record(instance.identity)?;
                    }
                    println!(
                        "alacritty: pid {} exited during probe",
                        instance.identity.pid
                    );
                    continue;
                }
                tracing::warn!(pid = instance.identity.pid, %error, "Alacritty status probe failed");
                println!(
                    "alacritty: running pid {} — font unavailable",
                    instance.identity.pid
                );
            }
        }
    }
    println!("a window zoomed by hand (⌘=/⌘-) ignores automation until ⌘0");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn parses_all_actions_without_ble_arguments() {
        for arguments in [
            vec![],
            vec!["on"],
            vec!["off"],
            vec!["pt", "0.625"],
            vec!["preview"],
            vec!["reset"],
        ] {
            let cli =
                crate::Cli::try_parse_from(["tm", "alacritty-zoom"].into_iter().chain(arguments))
                    .unwrap();
            assert!(matches!(
                cli.command,
                Some(crate::Commands::AlacrittyZoom { .. })
            ));
        }
        assert!(crate::Cli::try_parse_from(["tm", "alacritty-zoom", "pt", "invalid"]).is_err());
    }
    #[tokio::test]
    async fn invalid_deltas_fail_before_writing_config_or_opening_store() {
        for value in [0.0, -1.0, 8.1, f64::NAN, f64::INFINITY] {
            assert!(run_zoom(Some(ZoomAction::Pt { value })).await.is_err());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn busy_probe_is_read_only_and_never_waits_for_lock() {
        use crate::alacritty_zoom::lock::ZoomLock;
        use crate::alacritty_zoom::test_support::{Fake, TestLockDir};
        let dir = TestLockDir::create();
        let fake = Fake::default();
        let old = fake.add(1, 10, 14.625);
        fake.record(old, 14.0, 14.625);
        fake.remove(old);
        let live = fake.add(2, 20, 12.0);
        fake.0.lock().unwrap().lock_path = Some(dir.path());
        let mut core = ZoomCore::new(fake.clone());
        let guard = ZoomLock::acquire(&dir.path()).await.unwrap();
        let started = tokio::time::Instant::now();
        print_probe(&mut core, ZoomConfig::default()).await.unwrap();
        assert_eq!(started.elapsed(), std::time::Duration::ZERO);
        assert_eq!(fake.0.lock().unwrap().records.len(), 1);
        assert!(fake.0.lock().unwrap().log.iter().all(|call| call.contains("get-config")));
        assert_eq!(fake.size(live), 12.0);
        drop(guard);
        print_probe(&mut core, ZoomConfig::default()).await.unwrap();
        assert!(fake.0.lock().unwrap().records.is_empty());
    }

}
