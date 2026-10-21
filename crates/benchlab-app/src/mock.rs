//! Deterministic localhost mock target used by the demo and tests.

use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
struct MockState {
    requests: u64,
    paths: BTreeMap<String, u64>,
    last_headers: Vec<(String, String)>,
    sequence: u64,
}

impl MockState {
    fn record(&mut self, path: &str, headers: &HeaderMap) {
        self.requests += 1;
        *self.paths.entry(path.to_owned()).or_default() += 1;
        self.sequence = self.sequence.wrapping_add(1);
        self.last_headers = headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();
    }
}

#[derive(Clone)]
struct MockApp {
    state: Arc<Mutex<MockState>>,
}

pub struct MockTarget {
    base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl MockTarget {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|error| format!("could not bind the mock target: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("could not configure the mock target: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("could not read the mock target address: {error}"))?;
        let base_url = format!("http://127.0.0.1:{}", address.port());
        let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let app = router();
        let thread = thread::Builder::new()
            .name("benchlab-mock-target".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        tracing::error!(%error, "could not start the mock target runtime");
                        return;
                    }
                };
                runtime.block_on(async move {
                    let listener = match tokio::net::TcpListener::from_std(listener) {
                        Ok(listener) => listener,
                        Err(error) => {
                            tracing::error!(%error, "could not adopt the mock target listener");
                            return;
                        }
                    };
                    if let Err(error) = axum::serve(listener, app)
                        .with_graceful_shutdown(async move {
                            let _ = shutdown_rx.await;
                        })
                        .await
                    {
                        tracing::error!(%error, "mock target stopped with an error");
                    }
                });
            })
            .map_err(|error| format!("could not start the mock target thread: {error}"))?;

        tracing::info!(%base_url, "mock target listening");
        Ok(Self {
            base_url,
            shutdown: Some(shutdown),
            thread: Some(thread),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for MockTarget {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Lazily started mock target. A single target is shared by the demo and UI.
#[derive(Default)]
pub struct MockTargetManager {
    target: Mutex<Option<MockTarget>>,
}

impl MockTargetManager {
    pub fn ensure(&self) -> Result<String, String> {
        let mut guard = self
            .target
            .lock()
            .map_err(|_| "the mock target lock was poisoned".to_owned())?;
        if let Some(target) = guard.as_ref() {
            return Ok(target.base_url().to_owned());
        }
        let target = MockTarget::start()?;
        let base_url = target.base_url().to_owned();
        *guard = Some(target);
        Ok(base_url)
    }
}

fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/items", get(items))
        .route("/items/{id}", get(item))
        .route("/echo", post(echo))
        .route("/slow", get(slow))
        .route("/status/{code}", get(status))
        .route("/flaky", get(flaky))
        .route("/bytes/{n}", get(bytes))
        .route("/reset", post(reset))
        .route("/stats", get(stats))
        .route("/slow-tail", get(slow_tail))
        .with_state(MockApp {
            state: Arc::new(Mutex::new(MockState::default())),
        })
}

async fn health(State(app): State<MockApp>, headers: HeaderMap) -> Json<Value> {
    record(&app, "/health", &headers);
    Json(json!({"ok": true}))
}

async fn items(State(app): State<MockApp>, headers: HeaderMap) -> Json<Value> {
    record(&app, "/items", &headers);
    Json(Value::Array((1..=20).map(item_value).collect()))
}

async fn item(State(app): State<MockApp>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    record(&app, &format!("/items/{id}"), &headers);
    let Ok(id) = id.parse::<u64>() else {
        return not_found("Item id must be a number");
    };
    if id == 0 || id > 20 {
        return not_found("Item not found");
    }
    Json(item_value(id)).into_response()
}

async fn echo(State(app): State<MockApp>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    record(&app, "/echo", &headers);
    match serde_json::from_slice::<Value>(&body) {
        Ok(value) => ([(header::CONTENT_TYPE, "application/json")], Json(value)).into_response(),
        Err(_) => (
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            String::from_utf8_lossy(&body).into_owned(),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct SlowQuery {
    #[serde(default = "default_slow_ms")]
    ms: u64,
}

fn default_slow_ms() -> u64 {
    250
}

async fn slow(
    State(app): State<MockApp>,
    Query(query): Query<SlowQuery>,
    headers: HeaderMap,
) -> Json<Value> {
    record(&app, "/slow", &headers);
    let ms = query.ms.min(10_000);
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Json(json!({"slept_ms": ms}))
}

async fn status(State(app): State<MockApp>, Path(code): Path<u16>, headers: HeaderMap) -> Response {
    record(&app, &format!("/status/{code}"), &headers);
    if !(100..=599).contains(&code) {
        return (
            StatusCode::BAD_REQUEST,
            "Status code must be between 100 and 599",
        )
            .into_response();
    }
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST);
    (status, Json(json!({"status": code}))).into_response()
}

#[derive(Deserialize)]
struct FlakyQuery {
    #[serde(default = "default_seed")]
    seed: u64,
    #[serde(default = "default_fail")]
    fail: f64,
}

fn default_seed() -> u64 {
    1
}

fn default_fail() -> f64 {
    0.1
}

async fn flaky(
    State(app): State<MockApp>,
    Query(query): Query<FlakyQuery>,
    headers: HeaderMap,
) -> Response {
    let (sequence, path) = {
        let mut state = app.state.lock().expect("mock state lock");
        state.record("/flaky", &headers);
        (state.sequence, "/flaky".to_owned())
    };
    let fraction = deterministic_fraction(query.seed, sequence, &path);
    if fraction < query.fail.clamp(0.0, 1.0) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "deterministic failure", "seed": query.seed})),
        )
            .into_response()
    } else {
        Json(json!({"ok": true, "seed": query.seed})).into_response()
    }
}

async fn bytes(State(app): State<MockApp>, Path(n): Path<usize>, headers: HeaderMap) -> Response {
    record(&app, &format!("/bytes/{n}"), &headers);
    let n = n.min(MAX_BODY_BYTES);
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "x".repeat(n),
    )
        .into_response()
}

async fn reset(State(app): State<MockApp>, headers: HeaderMap) -> Json<Value> {
    let mut state = app.state.lock().expect("mock state lock");
    state.record("/reset", &headers);
    state.requests = 0;
    state.paths.clear();
    state.last_headers.clear();
    state.sequence = 0;
    Json(json!({"ok": true}))
}

async fn stats(State(app): State<MockApp>, headers: HeaderMap) -> Json<Value> {
    let mut state = app.state.lock().expect("mock state lock");
    state.record("/stats", &headers);
    Json(json!({
        "request_count": state.requests,
        "per_path": state.paths,
        "last_headers": state.last_headers,
    }))
}

async fn slow_tail(State(app): State<MockApp>, headers: HeaderMap) -> Json<Value> {
    let (sequence, path) = {
        let mut state = app.state.lock().expect("mock state lock");
        state.record("/slow-tail", &headers);
        (state.sequence, "/slow-tail".to_owned())
    };
    let tail = deterministic_hash(7, sequence, &path) % 100 < 5;
    let ms = if tail { 400 } else { 5 };
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Json(json!({"tail": tail, "ms": ms}))
}

fn record(app: &MockApp, path: &str, headers: &HeaderMap) {
    app.state
        .lock()
        .expect("mock state lock")
        .record(path, headers);
}

fn item_value(id: u64) -> Value {
    json!({
        "id": id,
        "name": format!("Item {id}"),
        "category": if id.is_multiple_of(2) { "even" } else { "odd" },
        "price": (id * 125) as f64 / 100.0,
        "tags": ["demo", "deterministic"]
    })
}

fn not_found(message: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": message}))).into_response()
}

fn deterministic_fraction(seed: u64, sequence: u64, path: &str) -> f64 {
    (deterministic_hash(seed, sequence, path) % 10_000) as f64 / 10_000.0
}

fn deterministic_hash(seed: u64, sequence: u64, path: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64 ^ seed;
    for byte in path.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= sequence.wrapping_mul(0x9E3779B97F4A7C15);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51afd7ed558ccd);
    hash ^= hash >> 33;
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> reqwest::blocking::Response {
        reqwest::blocking::Client::new()
            .get(url)
            .timeout(Duration::from_secs(5))
            .send()
            .unwrap()
    }

    #[test]
    fn mock_target_serves_deterministic_endpoints_and_stats() {
        let target = MockTarget::start().unwrap();
        let base = target.base_url();

        let health = get(&format!("{base}/health")).json::<Value>().unwrap();
        assert_eq!(health, json!({"ok": true}));

        let items = get(&format!("{base}/items")).json::<Value>().unwrap();
        assert_eq!(items.as_array().unwrap().len(), 20);
        assert_eq!(items[0]["id"], 1);

        assert_eq!(get(&format!("{base}/items/0")).status(), 404);
        assert_eq!(get(&format!("{base}/items/1")).status(), 200);
        assert_eq!(get(&format!("{base}/status/418")).status(), 418);

        let echoed = reqwest::blocking::Client::new()
            .post(format!("{base}/echo"))
            .json(&json!({"hello": "world"}))
            .send()
            .unwrap()
            .json::<Value>()
            .unwrap();
        assert_eq!(echoed, json!({"hello": "world"}));

        let bytes = get(&format!("{base}/bytes/32")).bytes().unwrap();
        assert_eq!(bytes.len(), 32);

        let stats = get(&format!("{base}/stats")).json::<Value>().unwrap();
        assert!(stats["request_count"].as_u64().unwrap() >= 7);
        assert!(stats["per_path"]["/health"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn flaky_endpoint_is_deterministic_for_a_seed() {
        let target = MockTarget::start().unwrap();
        let base = target.base_url();
        let first = get(&format!("{base}/flaky?seed=9&fail=0.5")).status();
        reqwest::blocking::Client::new()
            .post(format!("{base}/reset"))
            .send()
            .unwrap();
        let second = get(&format!("{base}/flaky?seed=9&fail=0.5")).status();
        assert_eq!(first, second);
    }

    #[test]
    fn manager_returns_one_target() {
        let manager = MockTargetManager::default();
        let first = manager.ensure().unwrap();
        let second = manager.ensure().unwrap();
        assert_eq!(first, second);
    }
}
