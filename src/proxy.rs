use std::convert::Infallible;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};

use bytes::Bytes;

use eventsource_stream::Eventsource;

use futures_util::StreamExt;

use tracing::{error, info, warn};

use crate::state::AppState;

const MAX_BODY: usize = 64 * 1024 * 1024;

pub fn router() -> Router<AppState> {
  Router::new().fallback(proxy)
}

async fn proxy(State(state): State<AppState>, request: Request) -> Response {
  let (parts, body) = request.into_parts();
  let body = match to_bytes(body, MAX_BODY).await {
    Ok(body) => body,
    Err(e) => {
      warn!("failed to read request body - {e}");
      return (StatusCode::BAD_REQUEST, "invalid request body").into_response();
    }
  };
  let path = parts
    .uri
    .path_and_query()
    .map(|pq| pq.as_str())
    .unwrap_or("/");
  let url = format!("{}{}", state.upstream, path);
  let headers = forward_headers(&parts.headers);
  info!("{} {}", parts.method, path);
  if parts.method == Method::POST && is_streaming(&body) {
    stream(state, parts.method, url, headers, body)
  } else {
    forward(&state, parts.method, url, headers, body).await
  }
}

fn is_streaming(body: &[u8]) -> bool {
  serde_json::from_slice::<serde_json::Value>(body)
    .ok()
    .and_then(|body| body.get("stream").and_then(serde_json::Value::as_bool))
    .unwrap_or(false)
}

fn is_hop_by_hop(name: &str) -> bool {
  matches!(
    name,
    "host" | "connection" | "content-length" | "transfer-encoding" | "upgrade"
  )
}

fn forward_headers(headers: &HeaderMap) -> HeaderMap {
  let mut forwarded = HeaderMap::new();
  forwarded.extend(
    headers
      .iter()
      .filter(|(name, _)| !is_hop_by_hop(name.as_str()))
      .map(|(name, value)| (name.clone(), value.clone())),
  );
  forwarded
}

fn stream(
  state: AppState,
  method: Method,
  url: String,
  mut headers: HeaderMap,
  body: Bytes,
) -> Response {
  let AppState {
    http, heartbeat, ..
  } = state;

  headers.remove("accept-encoding");

  let events = async_stream::stream! {
    let response = http.request(method, &url).headers(headers).body(body).send().await;
    let response = match response {
      Ok(response) if response.status().is_success() => response,
      Ok(response) => {
        yield Ok::<Event, Infallible>(sse_error(&format!("upstream returned {}", response.status())));
        return;
      }
      Err(e) => {
        error!("upstream request failed - {e}");
        yield Ok(sse_error(&format!("upstream request failed: {e}")));
        return;
      }
    };
    let mut source = response.bytes_stream().eventsource();
    while let Some(event) = source.next().await {
      match event {
        Ok(event) => yield Ok(Event::default().data(event.data)),
        Err(e) => {
          error!("upstream stream error - {e}");
          break;
        }
      }
    }
  };

  Sse::new(events)
    .keep_alive(
      KeepAlive::new()
        .interval(heartbeat)
        .text("llama-cpr keepalive"),
    )
    .into_response()
}

fn sse_error(message: &str) -> Event {
  Event::default().data(serde_json::json!({ "error": { "message": message } }).to_string())
}

async fn forward(
  state: &AppState,
  method: Method,
  url: String,
  headers: HeaderMap,
  body: Bytes,
) -> Response {
  let response = match state
    .http
    .request(method, &url)
    .headers(headers)
    .body(body)
    .send()
    .await
  {
    Ok(response) => response,
    Err(e) => {
      error!("upstream request failed - {e}");
      return (
        StatusCode::BAD_GATEWAY,
        format!("upstream request failed: {e}"),
      )
        .into_response();
    }
  };

  let status = response.status();
  let headers = forward_headers(response.headers());
  let mut proxied = Response::new(Body::from_stream(response.bytes_stream()));
  *proxied.status_mut() = status;
  *proxied.headers_mut() = headers;
  proxied
}
