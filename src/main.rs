//! HashedNetworks coordination broker — binary entry point.
//!
//! Reads `BIND_ADDR` and `DATABASE_URL` from env (or `.env`), wires the
//! axum router from [`hashednetworks_broker::build_router`], and serves
//! with graceful shutdown.

use clap::Parser;
use hashednetworks_broker::{
    build_router,
    db::{self, DbPool},
    metrics::Metrics,
    presence::PresenceManager,
    registry::PeerRegistry,
    relay::RelayManager,
    signaling::SignalingLimiter,
    AppState,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Parser, Debug, Clone)]
#[command(name = "broker", version)]
struct Cli {
    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:8080")]
    bind: SocketAddr,
    #[arg(long, env = "DATABASE_URL", default_value = "")]
    database_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    init_tracing();
    let cli = Cli::parse();

    let db: Option<DbPool> = if cli.database_url.is_empty() {
        warn!("DATABASE_URL not set — running in no-database mode");
        None
    } else {
        let pool = db::connect(&cli.database_url).await?;
        db::run_migrations(&pool).await?;
        info!("postgres connected; migrations applied");
        Some(pool)
    };

    let state = AppState {
        db,
        registry: PeerRegistry::new(),
        presence: PresenceManager::new(),
        relay: Arc::new(RelayManager::new()),
        signaling_limiter: Arc::new(SignalingLimiter::new()),
        metrics: Metrics::new(),
    };

    let app = build_router(state);

    info!(bind = %cli.bind, "broker starting");
    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    info!("shutdown signal received");
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).with_target(false).init();
}
