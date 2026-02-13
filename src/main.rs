mod collector;
mod config;
mod error;
mod server;

use clap::Parser;
use collector::cpu::CpuCollector;
use collector::disk::DiskCollector;
use collector::memory::MemoryCollector;
use collector::network::NetworkCollector;
use collector::Registry;
use config::{Cli, Config};
use server::{build_router, AppState};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli)?;

    // Set up tracing
    match config.server.log_format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
                .init();
        }
    }

    // Build registry with enabled collectors
    let mut registry = Registry::new();

    if config.collectors.cpu {
        registry.register(Box::new(CpuCollector));
    }
    if config.collectors.memory {
        registry.register(Box::new(MemoryCollector));
    }
    if config.collectors.disk {
        let collector = DiskCollector::new(&config.collectors.disk_config.exclude_pattern)?;
        registry.register(Box::new(collector));
    }
    if config.collectors.network {
        let collector = NetworkCollector::new(&config.collectors.network_config.exclude_pattern)?;
        registry.register(Box::new(collector));
    }

    let state = Arc::new(AppState {
        registry,
        version: VERSION,
        rustc_version: "stable",
    });

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&config.server.listen).await?;
    tracing::info!(listen = %config.server.listen, "starting sysmetrics-rs");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl-c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { tracing::info!("received SIGINT, shutting down"); }
        _ = terminate => { tracing::info!("received SIGTERM, shutting down"); }
    }
}
