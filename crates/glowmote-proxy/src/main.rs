use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use glowmote_control::Controller;
use glowmote_proxy::actions::{self, Actions};

#[derive(Parser)]
#[command(
    version,
    about = "Control your lights over HTTP from any browser or script"
)]
struct Cli {
    /// Address to serve on. 0.0.0.0:8484 exposes the lights to the whole network, with no
    /// authentication
    #[arg(long, default_value = "127.0.0.1:8484")]
    listen: SocketAddr,

    /// Connect on start to the light whose name starts with this (e.g. ISP-50542B)
    #[arg(short, long)]
    device: Option<String>,

    /// Seconds to scan for devices
    #[arg(long, default_value_t = 5)]
    scan_secs: u64,

    /// JSON file of named actions, triggered with POST /api/action/<name>
    #[arg(long)]
    actions: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let controller = Arc::new(Controller::new(Duration::from_secs(cli.scan_secs)));
    let defs = match &cli.actions {
        Some(path) => {
            let json = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            actions::parse(&json).map_err(anyhow::Error::msg)?
        }
        None => Default::default(),
    };
    let actions = Arc::new(Actions::new(controller.clone(), defs));

    if let Some(device) = cli.device {
        let controller = controller.clone();
        tokio::spawn(async move {
            match controller.connect_matching(Some(&device)).await {
                Ok(name) => eprintln!("connected to {name}"),
                Err(e) => eprintln!("could not connect to {device}: {e}"),
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    eprintln!("serving on http://{}", listener.local_addr()?);
    axum::serve(listener, glowmote_proxy::app(controller.clone(), actions))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    controller.disconnect().await?;
    Ok(())
}
