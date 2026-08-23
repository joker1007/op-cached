use anyhow::{Context, Result, bail};
use tokio::process::Command;
use zeroize::Zeroizing;

/// `op read -n <url>`; output is zeroized on drop.
pub async fn read(url: &str) -> Result<Zeroizing<Vec<u8>>> {
    let out = Command::new("op")
        .args(["read", "-n", url])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .context("failed to spawn op")?;
    if !out.status.success() {
        bail!(
            "op read failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(Zeroizing::new(out.stdout))
}

/// `op inject -i <path>`; output is zeroized on drop.
pub async fn inject(path: &std::path::Path) -> Result<Zeroizing<Vec<u8>>> {
    let out = Command::new("op")
        .arg("inject")
        .arg("-i")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .context("failed to spawn op")?;
    if !out.status.success() {
        bail!(
            "op inject failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(Zeroizing::new(out.stdout))
}
