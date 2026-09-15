//! Verification owns retry policy; individual failures are DEBUG until exhausted.
use super::ipc::{AlacrittyInstance, AlacrittyIpc};
use super::{IPC_MAX_ATTEMPTS, IPC_RETRY_BACKOFF, format_font_size, parse_font_size, sizes_match};
use anyhow::{Result, bail};

pub async fn get_font_size(
    ipc: &mut impl AlacrittyIpc,
    instance: &AlacrittyInstance,
) -> Result<f64> {
    retry(ipc, instance, None).await
}
pub async fn set_font_size(
    ipc: &mut impl AlacrittyIpc,
    instance: &AlacrittyInstance,
    pt: f64,
) -> Result<()> {
    retry(ipc, instance, Some(pt)).await.map(|_| ())
}
async fn retry(
    ipc: &mut impl AlacrittyIpc,
    instance: &AlacrittyInstance,
    target: Option<f64>,
) -> Result<f64> {
    let mut last_error = String::new();
    let mut last_size = None;
    for attempt in 0..IPC_MAX_ATTEMPTS {
        if !ipc.is_live(instance) {
            bail!("instance exited before IPC");
        }
        if let Some(pt) = target {
            match ipc
                .call(
                    instance,
                    vec![
                        "config".into(),
                        "-w".into(),
                        "-1".into(),
                        format!("font.size={}", format_font_size(pt)),
                    ],
                )
                .await
            {
                Ok(reply) if reply.success => {}
                Ok(reply) => {
                    last_error = reply.stderr;
                    tracing::debug!(pid = instance.identity.pid, attempt = attempt + 1, stderr = %last_error, "Alacritty config exited non-zero; verifying delivery");
                }
                Err(error) => {
                    last_error = error.to_string();
                    tracing::debug!(pid = instance.identity.pid, attempt = attempt + 1, %error, "Alacritty config failed; verifying delivery");
                }
            }
        }
        if !ipc.is_live(instance) {
            bail!("instance exited before read-back");
        }
        match ipc
            .call(
                instance,
                vec!["get-config".into(), "-w".into(), "-1".into()],
            )
            .await
        {
            Ok(reply) => {
                last_size = parse_font_size(&reply.stdout);
                if reply.success
                    && let Some(size) = last_size
                    && target.is_none_or(|pt| sizes_match(pt, size))
                {
                    return Ok(size);
                }
                last_error = format!(
                    "get-config success={} stderr={:?}; prior={last_error}",
                    reply.success, reply.stderr
                );
            }
            Err(error) => {
                last_error = error.to_string();
            }
        }
        tracing::debug!(pid = instance.identity.pid, attempt = attempt + 1, ?target, ?last_size, error = %last_error, "Alacritty IPC retry");
        if !ipc.is_live(instance) {
            bail!("instance exited mid-op");
        }
        if let Some(delay) = IPC_RETRY_BACKOFF.get(attempt) {
            tokio::time::sleep(std::time::Duration::from_millis(*delay)).await;
        }
    }
    bail!(
        "Alacritty IPC exhausted {IPC_MAX_ATTEMPTS} attempts; target={target:?} last read size={last_size:?}; {last_error}"
    )
}
