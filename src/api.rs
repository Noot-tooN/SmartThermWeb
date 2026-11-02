use crate::{
    controller::{Controller, SubmitError},
    domain::{LoginState, TargetTemperature},
    provider::simulator::{Config, Simulator},
    sessions::{
        BrowserSession, SessionError, Sessions, clear_cookie, cookie_token, session_cookie,
    },
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::json;
use std::{convert::Infallible, sync::Arc};

#[derive(Clone)]
pub struct AppState {
    pub controller: Controller,
    pub simulator: Option<Arc<Simulator>>,
    pub sessions: Option<Arc<Sessions>>,
}
#[derive(rust_embed::RustEmbed)]
#[folder = "$SMARTTHERM_ASSETS"]
struct Assets;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/login",
            post(login).layer(DefaultBodyLimit::max(16_384)),
        )
        .route("/api/logout", post(logout))
        .route("/api/session", get(session_status))
        .route("/api/thermostat", get(snapshot))
        .route("/api/thermostat/target", put(set_target))
        .route(
            "/api/thermostat/schedule/preview",
            post(preview_schedule).layer(DefaultBodyLimit::max(16_384)),
        )
        .route("/api/events", get(events))
        .route("/api/health", get(health))
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
    ReadOnly,
    NotSimulator,
    LoginUnavailable,
    LoginRateLimited,
    InvalidLogin,
    InvalidOrigin,
    LogoutUnavailable,
    Unauthenticated,
    CloudLoginFailed,
    CloudUnavailable,
    MultipleDevices,
    NoDevice,
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Unauthenticated => (
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "Sign in with your OCSTAT account.".into(),
            ),
            Self::CloudLoginFailed => (
                StatusCode::UNAUTHORIZED,
                "authentication",
                "OCSTAT did not accept the email or password.".into(),
            ),
            Self::CloudUnavailable => (
                StatusCode::BAD_GATEWAY,
                "cloud_unavailable",
                "Could not connect to OCSTAT. Try again.".into(),
            ),
            Self::MultipleDevices => (
                StatusCode::CONFLICT,
                "multiple_devices",
                "Multiple-device selection is not supported yet.".into(),
            ),
            Self::NoDevice => (
                StatusCode::NOT_FOUND,
                "device_not_found",
                "No thermostat was returned for this account.".into(),
            ),
            Self::LogoutUnavailable => (
                StatusCode::CONFLICT,
                "logout_unavailable",
                "Logout is available only for the OCSTAT provider.".into(),
            ),
            Self::InvalidOrigin => (
                StatusCode::FORBIDDEN,
                "invalid_origin",
                "Use the local SmartThermWeb page for account actions.".into(),
            ),
            Self::InvalidLogin => (
                StatusCode::BAD_REQUEST,
                "invalid_login",
                "Provide an email and password, each at most 1024 bytes.".into(),
            ),
            Self::LoginUnavailable => (
                StatusCode::CONFLICT,
                "login_unavailable",
                "You are already signed in, or login is unavailable for this provider.".into(),
            ),
            Self::LoginRateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "login_rate_limited",
                "Wait 10 seconds before trying to sign in again.".into(),
            ),
            Self::ReadOnly => (
                StatusCode::FORBIDDEN,
                "read_only",
                "Temperature changes are unavailable for the current session or device state."
                    .into(),
            ),
            Self::NotSimulator => (
                StatusCode::NOT_FOUND,
                "not_found",
                "Simulator settings are unavailable for this provider.".into(),
            ),
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
            [(header::CACHE_CONTROL, "no-store")],
            Json(json!({"error": {"code": code, "message": message}})),
        )
            .into_response()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: String,
}
fn local_same_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<Uri>().ok())
    else {
        return false;
    };
    matches!(origin.scheme_str(), Some("http" | "https"))
        && origin.authority().is_some_and(|a| a.as_str() == host)
        && origin.path() == "/"
        && origin.query().is_none()
        && matches!(origin.host(), Some("127.0.0.1" | "localhost" | "[::1]"))
}
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    if !local_same_origin(&headers) {
        return Err(ApiError::InvalidOrigin);
    }
    // Never echo JSON extractor errors: they can include user-supplied values.
    let Json(body) = body.map_err(|_| ApiError::InvalidLogin)?;
    let email = body.email.trim().to_owned();
    if email.is_empty()
        || body.password.trim().is_empty()
        || email.len() > 1024
        || body.password.len() > 1024
    {
        return Err(ApiError::InvalidLogin);
    }
    let sessions = state.sessions.as_ref().ok_or(ApiError::LoginUnavailable)?;
    let old_token = cookie_token(&headers);
    if old_token
        .as_deref()
        .and_then(|token| sessions.get(token))
        .is_some_and(|entry| entry.controller.snapshot().login == Some(LoginState::Ready))
    {
        return Err(ApiError::LoginUnavailable);
    }
    let (token, snapshot) =
        sessions
            .login(email, body.password)
            .await
            .map_err(|error| match error {
                SessionError::RateLimited | SessionError::Capacity => ApiError::LoginRateLimited,
                SessionError::Login(crate::provider::ProviderError::Authentication) => {
                    ApiError::CloudLoginFailed
                }
                SessionError::Login(crate::provider::ProviderError::MultipleDevices) => {
                    ApiError::MultipleDevices
                }
                SessionError::Login(crate::provider::ProviderError::DeviceNotFound) => {
                    ApiError::NoDevice
                }
                _ => ApiError::CloudUnavailable,
            })?;
    if let Some(old) = old_token {
        sessions.logout(&old);
    }
    Ok((
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (header::SET_COOKIE, session_cookie(&token)),
        ],
        Json(snapshot),
    ))
}
async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    if !local_same_origin(&headers) {
        return Err(ApiError::InvalidOrigin);
    }
    let sessions = state.sessions.as_ref().ok_or(ApiError::LogoutUnavailable)?;
    if let Some(token) = cookie_token(&headers) {
        sessions.logout(&token);
    }
    Ok((
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::SET_COOKIE, clear_cookie()),
        ],
        Json(state.controller.snapshot()),
    ))
}
fn browser_session(state: &AppState, headers: &HeaderMap) -> Option<Arc<BrowserSession>> {
    let sessions = state.sessions.as_ref()?;
    sessions.get(&cookie_token(headers)?)
}
async fn session_status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = browser_session(&state, &headers) {
        return (
            [(header::CACHE_CONTROL, "no-store")],
            Json(session.controller.snapshot()),
        )
            .into_response();
    }
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.controller.snapshot()),
    )
        .into_response()
}
async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    let controller = if state.sessions.is_some() {
        browser_session(&state, &headers)
            .ok_or(ApiError::Unauthenticated)?
            .controller
            .clone()
    } else {
        state.controller
    };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(controller.snapshot()),
    ))
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
    headers: HeaderMap,
    body: Result<Json<TargetRequest>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let controller = if state.sessions.is_some() {
        if !local_same_origin(&headers) {
            return Err(ApiError::InvalidOrigin);
        }
        browser_session(&state, &headers)
            .ok_or(ApiError::Unauthenticated)?
            .controller
            .clone()
    } else {
        state.controller
    };
    if !controller.snapshot().can_set_target {
        return Err(ApiError::ReadOnly);
    }
    let Json(body) = body.map_err(|e| ApiError::Invalid(e.body_text()))?;
    let snapshot = match (body.client_id, body.client_revision) {
        (None, None) => controller
            .submit(body.target_temperature)
            .map_err(|_| ApiError::ReadOnly)?,
        (Some(client), Some(revision))
            if !client.is_empty() && client.len() <= 80 && revision > 0 =>
        {
            controller
                .submit_from(body.target_temperature, client, revision)
                .map_err(|error| match error {
                    SubmitError::StaleRevision => ApiError::StaleRevision,
                    SubmitError::TooManyClients => ApiError::TooManyClients,
                    SubmitError::ReadOnly => ApiError::ReadOnly,
                    SubmitError::LoginUnavailable => ApiError::LoginUnavailable,
                    SubmitError::LoginRateLimited => ApiError::LoginRateLimited,
                    SubmitError::LogoutUnavailable => ApiError::LogoutUnavailable,
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
// Pure calculation: this route never invokes a provider or persists a schedule.
async fn preview_schedule(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<crate::schedule::Schedule>, JsonRejection>,
) -> Result<impl IntoResponse, ApiError> {
    if state.sessions.is_some() {
        if !local_same_origin(&headers) {
            return Err(ApiError::InvalidOrigin);
        }
        let session = browser_session(&state, &headers).ok_or(ApiError::Unauthenticated)?;
        if session.controller.snapshot().login != Some(LoginState::Ready) {
            return Err(ApiError::Unauthenticated);
        }
    }
    let Json(schedule) = body.map_err(|_| {
        ApiError::Invalid(
            "Provide a valid schedule with temperatures from 5–35°C in 0.5°C steps.".into(),
        )
    })?;
    let preview = schedule
        .preview()
        .map_err(|e| ApiError::Invalid(e.into()))?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(preview)))
}
async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let snapshot = state.controller.snapshot();
    Json(
        json!({ "status": "ok", "provider": snapshot.provider, "can_set_target": snapshot.can_set_target }),
    )
}
async fn simulator_config(State(state): State<AppState>) -> Result<Json<Config>, ApiError> {
    Ok(Json(
        state
            .simulator
            .as_ref()
            .ok_or(ApiError::NotSimulator)?
            .config(),
    ))
}
async fn configure_simulator(
    State(state): State<AppState>,
    body: Result<Json<Config>, JsonRejection>,
) -> Result<Json<Config>, ApiError> {
    let simulator = state.simulator.as_ref().ok_or(ApiError::NotSimulator)?;
    let Json(config) = body.map_err(|e| ApiError::Invalid(e.body_text()))?;
    if !config.validate() {
        return Err(ApiError::Invalid(
            "ack_ms must be ≤ 30000 and apply_ms ≤ 60000.".into(),
        ));
    }
    simulator.configure(config.clone());
    Ok(Json(config))
}
async fn events(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let browser = browser_session(&state, &headers);
    if state.sessions.is_some() && browser.is_none() {
        return Err(ApiError::Unauthenticated);
    }
    let controller = browser
        .as_ref()
        .map(|session| session.controller.clone())
        .unwrap_or(state.controller);
    let mut receiver = controller.subscribe();
    // A sender retained in the stream keeps the simulator's cancellation channel open.
    let (fallback_sender, fallback_receiver) = tokio::sync::watch::channel(false);
    let mut revoked = browser
        .as_ref()
        .map(|session| session.revoked.subscribe())
        .unwrap_or(fallback_receiver);
    let stream = async_stream::stream! {
        let _fallback_sender = fallback_sender;
        let _browser = browser;
        if *revoked.borrow() { return; }
        let first = controller.snapshot();
        let mut revision = first.revision;
        yield Ok::<_, Infallible>(event(&first));
        loop {
            tokio::select! {
                _ = revoked.changed() => {
                    yield Ok(Event::default().event("signed_out").data("{}"));
                    break;
                }
                update = receiver.recv() => {
                    if *revoked.borrow() {
                        yield Ok(Event::default().event("signed_out").data("{}"));
                        break;
                    }
                    let state = match update {
                        Ok(state) => state,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => controller.snapshot(),
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    if state.revision > revision { revision = state.revision; yield Ok(event(&state)); }
                }
            }
        }
    };
    Ok((
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    )
        .into_response())
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
