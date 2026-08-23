use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, watch};
use zeroize::Zeroizing;

use crate::cache::Cache;
use crate::protocol::{Request, Response, read_line, write_line};
use crate::{gpg, op};

const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

struct State {
    cache: Mutex<Cache>,
    started_at: Instant,
    shutdown: watch::Sender<bool>,
}

pub async fn run(socket: PathBuf, ttl: Duration) -> Result<()> {
    let listener = bind(&socket)?;
    let (shutdown, mut shutdown_rx) = watch::channel(false);
    let state = Arc::new(State {
        cache: Mutex::new(Cache::new(ttl)),
        started_at: Instant::now(),
        shutdown,
    });
    eprintln!(
        "op-cached: listening on {} (ttl={})",
        socket.display(),
        humantime::format_duration(ttl)
    );

    let sweeper = {
        let state = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(SWEEP_INTERVAL);
            tick.tick().await;
            loop {
                tick.tick().await;
                let n = state.cache.lock().await.sweep(Instant::now());
                if n > 0 {
                    eprintln!("op-cached: swept {n} expired entries");
                }
            }
        })
    };

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("accept failed")?;
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, state).await {
                        eprintln!("op-cached: connection error: {e:#}");
                    }
                });
            }
            _ = shutdown_rx.changed() => break,
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
        }
    }

    sweeper.abort();
    let _ = std::fs::remove_file(&socket);
    eprintln!("op-cached: stopped");
    Ok(())
}

fn bind(socket: &Path) -> Result<UnixListener> {
    if socket.exists() {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            anyhow::bail!(
                "another daemon is already listening on {}",
                socket.display()
            );
        }
        std::fs::remove_file(socket)
            .with_context(|| format!("failed to remove stale socket {}", socket.display()))?;
    }
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(socket)
        .with_context(|| format!("failed to bind {}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

async fn handle(stream: UnixStream, state: Arc<State>) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    while let Some(req) = read_line::<_, Request>(&mut r).await? {
        let resp = match req {
            Request::Read { url } => match read_value(&state, &url).await {
                Ok(v) => match String::from_utf8(v.to_vec()) {
                    Ok(s) => Response::value(s),
                    Err(_) => Response::error("value is not valid UTF-8"),
                },
                Err(e) => Response::error(format!("{e:#}")),
            },
            Request::Inject { path } => match inject_file(&state, &path).await {
                Ok(v) => match String::from_utf8(v.to_vec()) {
                    Ok(s) => Response::value(s),
                    Err(_) => Response::error("rendered output is not valid UTF-8"),
                },
                Err(e) => Response::error(format!("{e:#}")),
            },
            Request::Clear => {
                state.cache.lock().await.clear();
                Response::ok()
            }
            Request::Status => {
                let cache = state.cache.lock().await;
                Response::status(
                    cache.len(),
                    cache.ttl().as_secs(),
                    state.started_at.elapsed().as_secs(),
                )
            }
            Request::Stop => {
                write_line(&mut w, &Response::ok()).await?;
                let _ = state.shutdown.send(true);
                return Ok(());
            }
        };
        write_line(&mut w, &resp).await?;
    }
    Ok(())
}

async fn read_value(state: &State, url: &str) -> Result<Zeroizing<Vec<u8>>> {
    if !url.starts_with("op://") {
        anyhow::bail!("not an op:// url: {url}");
    }
    let cached = state
        .cache
        .lock()
        .await
        .get_url(url, Instant::now())
        .map(|c| c.to_vec());
    if let Some(ct) = cached {
        return gpg::decrypt(&ct).await.context("decrypt cached value");
    }
    let plain = op::read(url).await?;
    let ct = gpg::encrypt(&plain).await.context("encrypt value")?;
    state.cache.lock().await.insert_url(url, ct, Instant::now());
    Ok(plain)
}

async fn inject_file(state: &State, path: &str) -> Result<Zeroizing<Vec<u8>>> {
    let path = std::fs::canonicalize(path).with_context(|| format!("cannot resolve {path}"))?;
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .with_context(|| format!("cannot stat {}", path.display()))?;
    let key = path.to_string_lossy().into_owned();
    let cached = state
        .cache
        .lock()
        .await
        .get_file(&key, mtime, Instant::now())
        .map(|c| c.to_vec());
    if let Some(ct) = cached {
        return gpg::decrypt(&ct).await.context("decrypt cached value");
    }
    let plain = op::inject(&path).await?;
    let ct = gpg::encrypt(&plain).await.context("encrypt value")?;
    state
        .cache
        .lock()
        .await
        .insert_file(&key, mtime, ct, Instant::now());
    Ok(plain)
}
