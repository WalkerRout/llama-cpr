use tracing::{error, info};

mod proxy;
mod server;
mod state;

use crate::server::Server;
use crate::state::AppState;

#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
  tracing_subscriber::fmt()
    .with_max_level(tracing_subscriber::filter::LevelFilter::INFO)
    .with_target(false)
    .with_thread_ids(true)
    .with_ansi(false)
    .init();

  log_panics::init();

  info!("initializing app state...");
  let state = AppState::new();
  info!("initialized app state");

  info!("initializing llama-cpr...");
  match Server::new(state).await {
    Ok(server) => {
      if let Err(e) = server.run().await {
        error!("llama-cpr error - {e}");
      }
      info!("exiting llama-cpr...");
    }
    Err(e) => {
      error!("failed to initialize llama-cpr... - {e}");
    }
  }
}
