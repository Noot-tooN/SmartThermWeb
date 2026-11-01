use crate::{
    controller::{Controller, SubmitError},
    domain::TargetTemperature,
    provider::simulator::{Config, Simulator},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{State, rejection::JsonRejection},
    http::{StatusCode, Uri, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::json;
use std::{convert::Infallible, sync::Arc};

#[derive(Clone)]
pub struct AppState {
    pub controller: Controller,
    pub simulator: Arc<Simulator>,
}
#[derive(rust_embed::RustEmbed)]
#[folder = "$SMARTTHERM_ASSETS"]
struct Assets;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/thermostat", get(snapshot))
        .route("/api/thermostat/target", put(set_target))
        .route("/api/events", get(events))
        .route(
            "/api/health",
            get(|| async { Json(json!({ "status": "ok", "provider": "simulator" })) }),
        )
        .route(
            "/api/simulator",
            get(simulator_config).put(configure_simulator),
        )
        .fallback(assets)
        .with_state(state)
}
#[derive(Debug)]
pub enum ApiError {
    Invalid(String),
    StaleRevision,
    TooManyClients,
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            Self::StaleRevision => (
                StatusCode::CONFLICT,
                "stale_revision",
                "A newer request from this browser is already known. Refresh to synchronize."
                    .into(),
            ),
            Self::TooManyClients => (
                StatusCode::TOO_MANY_REQUESTS,
                "client_limit",
                "Client limit reached; restart the local simulator.".into(),
            ),
        };
        (
            status,
            Json(json!({"error": {"code": code, "message": message}})),
        )
            .into_response()
    }
}
async fn snapshot(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.controller.snapshot()),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetRequest {
    target_temperature: TargetTemperature,
    client_id: Option<String>,
    client_revision: Option<u64>,
}
async fn set_target(
    State(state): State<AppState>,
    body: Result<Json<TargetRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Json(body) = body.map_err(|e| ApiError::Invalid(e.body_text()))?;
    let snapshot = match (body.client_id, body.client_revision) {
        (None, None) => state.controller.submit(body.target_temperature),
        (Some(client), Some(revision))
            if !client.is_empty() && client.len() <= 80 && revision > 0 =>
        {
            state
                .controller
                .submit_from(body.target_temperature, client, revision)
                .map_err(|error| match error {
                    SubmitError::StaleRevision => ApiError::StaleRevision,
                    SubmitError::TooManyClients => ApiError::TooManyClients,
                })?
        }
        _ => {
            return Err(ApiError::Invalid(
                "Provide both client_id (1–80 bytes) and positive client_revision, or neither."
                    .into(),
            ));
        }
    };
    Ok((StatusCode::ACCEPTED, Json(snapshot)))
}
async fn simulator_config(State(state): State<AppState>) -> Json<Config> {
    Json(state.simulator.config())
}
async fn configure_simulator(
    State(state): State<AppState>,
    body: Result<Json<Config>, JsonRejection>,
) -> Result<Json<Config>, ApiError> {
    let Json(config) = body.map_err(|e| ApiError::Invalid(e.body_text()))?;
    if !config.validate() {
        return Err(ApiError::Invalid(
            "ack_ms must be ≤ 30000 and apply_ms ≤ 60000.".into(),
        ));
    }
    state.simulator.configure(config.clone());
    Ok(Json(config))
}
async fn events(State(state): State<AppState>) -> impl IntoResponse {
    // Subscribe BEFORE snapshot to close the initial-read/subscription race.
    let mut receiver = state.controller.subscribe();
    let controller = state.controller;
    let stream = async_stream::stream! {
        let first = controller.snapshot();
        let mut revision = first.revision;
        yield Ok::<_, Infallible>(event(&first));
        loop {
            let state = match receiver.recv().await {
                Ok(state) => state,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => controller.snapshot(),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            if state.revision > revision {
                revision = state.revision;
                yield Ok(event(&state));
            }
        }
    };
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    )
}
fn event(snapshot: &crate::domain::Snapshot) -> Event {
    Event::default()
        .event("state")
        .id(format!("{}:{}", snapshot.session, snapshot.revision))
        .json_data(snapshot)
        .expect("snapshot is serializable")
}
async fn assets(uri: Uri) -> Response {
    if uri.path() == "/api" || uri.path().starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"code": "not_found", "message": "Unknown API endpoint."}})),
        )
            .into_response();
    }
    let path = if uri.path() == "/" {
        "index.html"
    } else {
        uri.path().trim_start_matches('/')
    };
    match Assets::get(path) {
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                mime_guess::from_path(path).first_or_octet_stream().as_ref(),
            )
            .header(
                header::CACHE_CONTROL,
                if path.starts_with("assets/") {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                },
            )
            .header("x-content-type-options", "nosniff")
            .body(Body::from(file.data.into_owned()))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
