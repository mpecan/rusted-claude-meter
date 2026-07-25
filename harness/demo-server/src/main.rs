//! A stand-in for claude.ai's usage API, for the Linux desktop harness.
//!
//! Runs on the macOS host; the Lima VMs reach it at `host.lima.internal`. The
//! app under test is the real, unmodified build — it is pointed here with
//! `RCM_API_BASE_URL` (see `src-tauri/src/api_base.rs`), so no real session key
//! and no network access to claude.ai are involved anywhere in the loop.
//!
//! ```console
//! $ just demo-server                       # or: cargo run --manifest-path …
//! $ harness/bin/scenario.sh critical       # change the data, live
//! ```
//!
//! See `harness/README.md` for the full runbook.

mod routes;
mod scenario;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use routes::AppState;
use scenario::Scenarios;

const DEFAULT_PORT: u16 = 8787;
const DEFAULT_SCENARIO: &str = "fresh";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("DEMO_SERVER_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let dir = std::env::var("DEMO_SERVER_SCENARIOS").map_or_else(
        |_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios"),
        PathBuf::from,
    );
    let initial = std::env::var("DEMO_SERVER_SCENARIO")
        .unwrap_or_else(|_| DEFAULT_SCENARIO.to_owned());

    let scenarios = Scenarios::new(dir.clone());
    let available = scenarios.available();
    if available.is_empty() {
        return Err(format!("no scenarios found in {}", dir.display()).into());
    }

    let state = Arc::new(AppState::new(scenarios, initial.clone()));

    // 0.0.0.0, not loopback: the VMs connect from outside the host. macOS will
    // ask once whether to allow incoming connections.
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    eprintln!("demo API on http://0.0.0.0:{port}  (VMs: http://host.lima.internal:{port})");
    eprintln!("scenarios in {}: {}", dir.display(), available.join(", "));
    eprintln!("active: {initial}");

    axum::serve(listener, routes::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
