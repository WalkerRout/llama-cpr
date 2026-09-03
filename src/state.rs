use std::env;
use std::time::Duration;

use reqwest::Client;

use tracing::info;

#[derive(Clone)]
pub struct AppState {
  pub http: Client,
  pub upstream: String,
  pub heartbeat: Duration,
}

impl AppState {
  pub fn new() -> Self {
    let upstream =
      env::var("LLAMA_CPR_UPSTREAM").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());
    let heartbeat = Duration::from_secs(
      env::var("LLAMA_CPR_HEARTBEAT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10),
    );

    info!("forwarding to {upstream}, heartbeat every {heartbeat:?}");

    // todo add enum error
    let http = Client::builder()
      .build()
      .expect("failed to build http client");

    Self {
      http,
      upstream,
      heartbeat,
    }
  }
}
