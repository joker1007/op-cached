mod cache;
mod client;
mod config;
mod daemon;
mod gpg;
mod op;
mod protocol;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};

/// Cache `op read` results in memory, GPG-encrypted, behind a Unix socket daemon.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Unix socket path (env: OP_CACHED_SOCKET; default: $XDG_RUNTIME_DIR/op-cached.sock)
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    /// Do not auto-start the daemon when it is not running (env: OP_CACHED_NO_SPAWN)
    #[arg(long, global = true)]
    no_spawn: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the cache daemon in the foreground
    Daemon {
        /// Cache TTL, e.g. "7d", "12h", "30m" (env: OP_CACHED_TTL; default: 7d)
        #[arg(long, value_parser = config::parse_duration)]
        ttl: Option<Duration>,
    },
    /// Read a secret by op:// URL (cached)
    Read {
        /// op://vault/item/field
        url: String,
    },
    /// Run `op inject` on a template file (result cached per file, invalidated on mtime change)
    Inject {
        /// Template file
        #[arg(short, long)]
        input: PathBuf,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Clear all cached entries
    Clear,
    /// Show daemon status
    Status,
    /// Stop the daemon
    Stop,
}

#[tokio::main]
async fn main() {
    if let Err(e) = real_main().await {
        eprintln!("op-cached: {e:#}");
        std::process::exit(1);
    }
}

async fn real_main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let socket = config::resolve_socket(cli.socket);
    let auto_spawn = config::resolve_auto_spawn(cli.no_spawn);
    match cli.cmd {
        Cmd::Daemon { ttl } => daemon::run(socket, config::resolve_ttl(ttl)?).await,
        Cmd::Read { url } => client::cmd_read(&socket, &url, auto_spawn).await,
        Cmd::Inject { input, output } => {
            client::cmd_inject(&socket, &input, output, auto_spawn).await
        }
        Cmd::Clear => client::cmd_clear(&socket).await,
        Cmd::Status => client::cmd_status(&socket).await,
        Cmd::Stop => client::cmd_stop(&socket).await,
    }
}
