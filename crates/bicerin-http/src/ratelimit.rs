//! Simple in-memory per-IP rate limiting (fixed 60s window).
//!
//! Limitation: keys by the immediate peer IP, so a server deployed behind a
//! reverse proxy without PROXY-protocol/`X-Forwarded-For` trust configuration
//! will see every client as the proxy's IP. Good enough as a first line of
//! defense against unauthenticated abuse (e.g. `/login` brute force); not a
//! substitute for a proxy-level WAF in a multi-instance deployment.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_TRACKED_KEYS: usize = 10_000;

struct Window {
    started_at: Instant,
    count: u32,
}

pub struct RateLimiter {
    enabled: bool,
    general_per_minute: u32,
    sensitive_per_minute: u32,
    buckets: Mutex<HashMap<(IpAddr, bool), Window>>,
}

impl RateLimiter {
    pub fn new(enabled: bool, general_per_minute: u32, sensitive_per_minute: u32) -> Self {
        Self {
            enabled,
            general_per_minute,
            sensitive_per_minute,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `true` if the request is allowed, `false` if the caller should
    /// be rejected with `M_LIMIT_EXCEEDED`.
    pub fn check(&self, ip: IpAddr, sensitive: bool) -> bool {
        if !self.enabled {
            return true;
        }
        let limit = if sensitive {
            self.sensitive_per_minute
        } else {
            self.general_per_minute
        };
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate limiter lock poisoned");

        if buckets.len() > MAX_TRACKED_KEYS {
            buckets.retain(|_, window| now.duration_since(window.started_at) < WINDOW);
        }

        let window = buckets.entry((ip, sensitive)).or_insert_with(|| Window {
            started_at: now,
            count: 0,
        });
        if now.duration_since(window.started_at) >= WINDOW {
            window.started_at = now;
            window.count = 0;
        }
        window.count += 1;
        window.count <= limit.max(1)
    }
}

pub async fn rate_limit_middleware(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, bicerin_error::BicerinError> {
    let path = request.uri().path();
    let sensitive = path.ends_with("/login") || path.ends_with("/register");
    if !state.rate_limiter.check(addr.ip(), sensitive) {
        return Err(bicerin_error::BicerinError::RateLimited);
    }
    Ok(next.run(request).await)
}
