mod proxy;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ktrx-proxy",
    version,
    about = "Standalone local HTTP/HTTPS interception proxy"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a local CA certificate for decrypting test HTTPS traffic.
    InitCa {
        #[arg(long)]
        directory: Option<PathBuf>,
    },
    /// Start the intercepting proxy on a loopback address.
    Start {
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
        #[arg(long)]
        ca_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::InitCa { directory } => {
            proxy::init_ca(&directory.unwrap_or_else(proxy::default_ca_dir))
        }
        Command::Start { listen, ca_dir } => {
            proxy::start(listen, &ca_dir.unwrap_or_else(proxy::default_ca_dir))
        }
    }
}
