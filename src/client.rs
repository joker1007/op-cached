use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use tokio::io::BufReader;
use tokio::net::UnixStream;

use crate::protocol::{Request, Response, read_line, write_line};

const SPAWN_WAIT: Duration = Duration::from_secs(3);

pub struct Client {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl Client {
    /// Connect, auto-spawning the daemon if nothing is listening.
    pub async fn connect(socket: &Path, auto_spawn: bool) -> Result<Self> {
        match UnixStream::connect(socket).await {
            Ok(s) => return Ok(Self::from_stream(s)),
            Err(_) if auto_spawn => {}
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("daemon not reachable at {}", socket.display()));
            }
        }
        spawn_daemon(socket)?;
        let deadline = Instant::now() + SPAWN_WAIT;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            match UnixStream::connect(socket).await {
                Ok(s) => return Ok(Self::from_stream(s)),
                Err(e) if Instant::now() >= deadline => {
                    return Err(e).context("daemon did not come up after auto-spawn");
                }
                Err(_) => {}
            }
        }
    }

    fn from_stream(s: UnixStream) -> Self {
        let (r, w) = s.into_split();
        Self {
            reader: BufReader::new(r),
            writer: w,
        }
    }

    pub async fn call(&mut self, req: &Request) -> Result<Response> {
        write_line(&mut self.writer, req).await?;
        match read_line::<_, Response>(&mut self.reader).await? {
            Some(Response::Error { error, .. }) => bail!("{error}"),
            Some(r) => Ok(r),
            None => bail!("daemon closed the connection"),
        }
    }

    pub async fn read(&mut self, url: &str) -> Result<String> {
        match self
            .call(&Request::Read {
                url: url.to_string(),
            })
            .await?
        {
            Response::Value { value, .. } => Ok(value),
            other => bail!("unexpected response: {other:?}"),
        }
    }
}

fn spawn_daemon(socket: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--socket")
        .arg(socket)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: setsid is async-signal-safe and only detaches the child from our session.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn().context("failed to spawn daemon")?;
    Ok(())
}

// ---- subcommands ----

pub async fn cmd_read(socket: &Path, url: &str, auto_spawn: bool) -> Result<()> {
    let mut c = Client::connect(socket, auto_spawn).await?;
    let v = c.read(url).await?;
    let mut out = std::io::stdout().lock();
    out.write_all(v.as_bytes())?;
    out.flush()?;
    Ok(())
}

pub async fn cmd_inject(
    socket: &Path,
    input: &Path,
    output: Option<PathBuf>,
    auto_spawn: bool,
) -> Result<()> {
    // Send an absolute path: the daemon may have a different cwd.
    let abs = std::path::absolute(input)
        .with_context(|| format!("cannot resolve {}", input.display()))?;
    let mut c = Client::connect(socket, auto_spawn).await?;
    let rendered = match c
        .call(&Request::Inject {
            path: abs.to_string_lossy().into_owned(),
        })
        .await?
    {
        Response::Value { value, .. } => value,
        other => bail!("unexpected response: {other:?}"),
    };
    match output {
        Some(p) => std::fs::write(&p, rendered)
            .with_context(|| format!("failed to write {}", p.display()))?,
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(rendered.as_bytes())?;
            out.flush()?;
        }
    }
    Ok(())
}

pub async fn cmd_clear(socket: &Path) -> Result<()> {
    let mut c = Client::connect(socket, false).await?;
    c.call(&Request::Clear).await?;
    eprintln!("cache cleared");
    Ok(())
}

pub async fn cmd_status(socket: &Path) -> Result<()> {
    let mut c = Client::connect(socket, false).await?;
    match c.call(&Request::Status).await? {
        Response::Status {
            entries,
            ttl_secs,
            uptime_secs,
            ..
        } => {
            println!("socket:  {}", socket.display());
            println!("entries: {entries}");
            println!(
                "ttl:     {}",
                humantime::format_duration(Duration::from_secs(ttl_secs))
            );
            println!(
                "uptime:  {}",
                humantime::format_duration(Duration::from_secs(uptime_secs))
            );
            Ok(())
        }
        other => bail!("unexpected response: {other:?}"),
    }
}

pub async fn cmd_stop(socket: &Path) -> Result<()> {
    let mut c = Client::connect(socket, false).await?;
    c.call(&Request::Stop).await?;
    eprintln!("daemon stopped");
    Ok(())
}
