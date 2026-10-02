//! Basic HTTP-level observability: a Prometheus-format `/_bicerin/metrics`
//! endpoint plus a request-count/duration middleware. Deliberately simple
//! (no distributed tracing/span export) — see todo.txt for what's left.

use axum::extract::MatchedPath;
use std::time::Instant;

pub async fn metrics_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = request.method().to_string();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());

    let start = Instant::now();
    let response = next.run(request).await;
    let elapsed = start.elapsed();
    let status = response.status().as_u16().to_string();

    metrics::counter!(
        "bicerin_http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "bicerin_http_request_duration_seconds",
        "method" => method,
        "path" => path,
    )
    .record(elapsed.as_secs_f64());

    response
}

pub async fn get_metrics(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> String {
    state.metrics_handle.render()
}
