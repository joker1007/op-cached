use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use zeroize::Zeroizing;

async fn run(args: &[&str], input: &[u8]) -> Result<Vec<u8>> {
    let mut child = Command::new("gpg")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn gpg")?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_vec();
    let writer = tokio::spawn(async move {
        let _ = stdin.write_all(&input).await;
        drop(stdin);
    });
    let out = child.wait_with_output().await.context("gpg failed")?;
    let _ = writer.await;
    if !out.status.success() {
        bail!(
            "gpg exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// Encrypt to the default key (`--default-recipient-self`).
pub async fn encrypt(plaintext: &[u8]) -> Result<Vec<u8>> {
    run(
        &[
            "--batch",
            "--yes",
            "--quiet",
            "--default-recipient-self",
            "--encrypt",
        ],
        plaintext,
    )
    .await
}

/// Decrypt via gpg-agent. Output is zeroized on drop.
pub async fn decrypt(ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    run(&["--batch", "--quiet", "--decrypt"], ciphertext)
        .await
        .map(Zeroizing::new)
}
