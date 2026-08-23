use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum Request {
    Read { url: String },
    Inject { path: String },
    Clear,
    Status,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Value {
        ok: bool,
        value: String,
    },
    Status {
        ok: bool,
        entries: usize,
        ttl_secs: u64,
        uptime_secs: u64,
    },
    Error {
        ok: bool,
        error: String,
    },
    Ok {
        ok: bool,
    },
}

impl Response {
    pub fn ok() -> Self {
        Response::Ok { ok: true }
    }
    pub fn value(value: String) -> Self {
        Response::Value { ok: true, value }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Response::Error {
            ok: false,
            error: msg.into(),
        }
    }
    pub fn status(entries: usize, ttl_secs: u64, uptime_secs: u64) -> Self {
        Response::Status {
            ok: true,
            entries,
            ttl_secs,
            uptime_secs,
        }
    }
}

pub async fn write_line<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let mut buf = serde_json::to_vec(msg)?;
    buf.push(b'\n');
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// Read one JSON line. Returns Ok(None) on clean EOF.
pub async fn read_line<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(
    r: &mut BufReader<R>,
) -> Result<Option<T>> {
    let mut line = String::new();
    let n = r.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    let trimmed = line.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        bail!("empty message");
    }
    let msg = serde_json::from_str(trimmed).context("invalid message")?;
    Ok(Some(msg))
}
