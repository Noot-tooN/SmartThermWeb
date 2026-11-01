use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use smartthermweb::{
    api::{AppState, router},
    controller::Controller,
    provider::simulator::{Config, Simulator},
};
use std::sync::Arc;
use tower::ServiceExt;
fn app() -> (axum::Router, Controller) {
    let controller = Controller::new();
    (
        router(AppState {
            controller: controller.clone(),
            simulator: Some(Arc::new(Simulator::new(Config::default()))),
            sessions: None,
        }),
        controller,
    )
}
fn put(body: &str) -> Request<Body> {
    Request::builder()
        .method("PUT")
        .uri("/api/thermostat/target")
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}
#[tokio::test]
async fn target_validation_and_typed_errors() {
    let (app, _) = app();
    for body in [
        r#"{"target_temperature":214}"#,
        r#"{"target_temperature":301}"#,
        r#"{"target_temperature":21.5}"#,
        r#"{"target_temperature":220,"surprise":1}"#,
        r#"{"target_temperature":220,"client_id":"x"}"#,
        "invalid",
    ] {
        let response = app.clone().oneshot(put(body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let value: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(value["error"]["code"], "invalid_request");
    }
    let response = app
        .oneshot(put(r#"{"target_temperature":230}"#))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["desired"], 230);
    assert_eq!(value["command"]["phase"], "queued");
    assert!(value["confirmed"].is_null());
}
#[tokio::test]
async fn late_or_duplicate_browser_requests_cannot_overwrite_newer_intent() {
    let (app, controller) = app();
    assert_eq!(
        app.clone()
            .oneshot(put(
                r#"{"target_temperature":230,"client_id":"browser-a","client_revision":3}"#
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::ACCEPTED
    );
    for revision in [2, 3] {
        assert_eq!(app.clone().oneshot(put(&format!(r#"{{"target_temperature":220,"client_id":"browser-a","client_revision":{revision}}}"#))).await.unwrap().status(), StatusCode::CONFLICT);
    }
    assert_eq!(i16::from(controller.snapshot().desired.unwrap()), 230);
    assert_eq!(controller.snapshot().command.unwrap().id, 1);
}
#[tokio::test]
async fn api_misses_never_serve_html_and_assets_have_content_types() {
    let (app, _) = app();
    for path in ["/api", "/api/missing"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["content-type"], "application/json");
    }
    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/html");
}
#[tokio::test]
async fn sse_initial_snapshot_lag_recovery_and_reconnection() {
    let (app, controller) = app();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();
    let initial = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(String::from_utf8_lossy(&initial).contains("event: state"));
    for i in 0..100 {
        controller
            .submit((200 + (i % 10) * 5).try_into().unwrap())
            .unwrap();
    }
    let latest = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(String::from_utf8_lossy(&latest).contains(r#""revision":100"#));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/events")
                .header("last-event-id", "old-session:1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let latest = response
        .into_body()
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(String::from_utf8_lossy(&latest).contains(r#""revision":100"#));
}
#[tokio::test]
async fn simulator_configuration_is_validated() {
    let (app, _) = app();
    for (ack, expected) in [(900, StatusCode::OK), (30001, StatusCode::BAD_REQUEST)] {
        let request = Request::builder()
            .method("PUT")
            .uri("/api/simulator")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"scenario":"offline","ack_ms":{ack},"apply_ms":2400}}"#
            )))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
}

#[tokio::test]
async fn ocstat_is_read_only_at_api_and_controller_boundaries() {
    let controller = Controller::ocstat_read_only();
    let app = router(AppState {
        controller: controller.clone(),
        simulator: None,
        sessions: Some(Arc::new(smartthermweb::sessions::Sessions::new())),
    });
    for body in [
        r#"{"target_temperature":220}"#,
        r#"{"target_temperature":220,"client_id":"x","client_revision":1}"#,
        "invalid",
    ] {
        let response = app.clone().oneshot(put(body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]["code"],
            "read_only"
        );
    }
    assert!(controller.submit(220.try_into().unwrap()).is_err());
    assert!(
        controller
            .submit_from(220.try_into().unwrap(), "x".into(), 1)
            .is_err()
    );
    let state = controller.snapshot();
    assert!(state.command.is_none() && state.desired.is_none() && state.in_flight.is_none());
    assert_eq!(state.revision, 0);
    for method in ["GET", "PUT"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/api/simulator")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let health: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(health["provider"], "ocstat");
    assert_eq!(health["can_set_target"], false);
}

fn login_request(body: &str, host: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/login")
        .header("content-type", "application/json")
        .header("host", host)
        .header("origin", origin)
        .body(Body::from(body.to_owned()))
        .unwrap()
}
#[tokio::test]
async fn browser_login_rejects_cross_site_requests_and_never_echoes_credentials() {
    let app = cloud_app();
    let body = r#"{"email":"example@example.invalid","password":"TEST_PASSWORD_ONLY"}"#;
    for (host, origin) in [
        ("127.0.0.1:3000", "https://evil.invalid"),
        ("evil.invalid:3000", "http://evil.invalid:3000"),
        ("127.0.0.1:3000", "null"),
        ("127.0.0.1:3000", "http://127.0.0.1:9999"),
    ] {
        let response = app
            .clone()
            .oneshot(login_request(body, host, origin))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for invalid in [
        r#"{"email":{"TEST_PASSWORD_ONLY":1},"password":"TEST_PASSWORD_ONLY"}"#,
        r#"{"email":"","password":"TEST_PASSWORD_ONLY"}"#,
    ] {
        let response = app
            .clone()
            .oneshot(login_request(
                invalid,
                "127.0.0.1:3000",
                "http://127.0.0.1:3000",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(!String::from_utf8_lossy(&bytes).contains("TEST_PASSWORD_ONLY"));
    }
    let response = app
        .clone()
        .oneshot(login_request(
            body,
            "127.0.0.1:3000",
            "http://127.0.0.1:3000",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&bytes).contains("TEST_PASSWORD_ONLY"));
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["login"], "ready");
    assert_eq!(value["can_set_target"], false);
    let mut again = login_request(body, "127.0.0.1:3000", "http://127.0.0.1:3000");
    again
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    assert_eq!(
        app.oneshot(again).await.unwrap().status(),
        StatusCode::CONFLICT
    );
}
#[tokio::test]
async fn browser_login_cannot_replace_an_active_login() {
    for controller in [Controller::new(), Controller::ocstat_read_only()] {
        if controller.snapshot().provider == "ocstat" {
            controller
                .submit_login(
                    "example@example.invalid".into(),
                    "TEST_PASSWORD_ONLY".into(),
                )
                .unwrap();
        }
        let app = router(AppState {
            controller,
            simulator: None,
            sessions: None,
        });
        assert_eq!(
            app.oneshot(login_request(
                r#"{"email":"example@example.invalid","password":"TEST_PASSWORD_ONLY"}"#,
                "localhost:5173",
                "http://localhost:5173"
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::CONFLICT
        );
    }
}

#[tokio::test]
async fn logout_accepts_ocstat_but_rejects_cross_site_requests() {
    let controller = Controller::ocstat_read_only();
    let app = router(AppState {
        controller: controller.clone(),
        simulator: None,
        sessions: Some(Arc::new(smartthermweb::sessions::Sessions::new())),
    });
    for (origin, status) in [
        ("https://evil.invalid", StatusCode::FORBIDDEN),
        ("http://127.0.0.1:3000", StatusCode::OK),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/api/logout")
            .header("host", "127.0.0.1:3000")
            .header("origin", origin)
            .body(Body::empty())
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    assert_eq!(
        controller.snapshot().login,
        Some(smartthermweb::domain::LoginState::Required)
    );
    assert!(controller.snapshot().confirmed.is_none());
    assert!(!controller.snapshot().can_set_target);
}

// Each synthetic account has its own observation. No test makes a cloud request.
#[derive(Default)]
struct FakeAccount(std::sync::Mutex<Option<smartthermweb::domain::DeviceState>>);
impl smartthermweb::provider::Provider for FakeAccount {
    fn login(
        &self,
        email: String,
        password: String,
    ) -> smartthermweb::provider::ProviderFuture<'_, smartthermweb::domain::DeviceState> {
        Box::pin(async move {
            use smartthermweb::{
                domain::{DeviceState, Mode, Temperature},
                provider::ProviderError,
            };
            if password != "TEST_PASSWORD_ONLY" {
                return Err(ProviderError::Authentication);
            }
            let state = DeviceState {
                room_temperature: Temperature(if email.starts_with("second") {
                    190
                } else {
                    210
                }),
                target_temperature: Temperature(215),
                online: true,
                mode: Mode::Unknown("unverified".into()),
                revision: 1,
                applied_command: None,
            };
            *self.0.lock().unwrap() = Some(state.clone());
            Ok(state)
        })
    }
    fn read(
        &self,
    ) -> smartthermweb::provider::ProviderFuture<'_, smartthermweb::domain::DeviceState> {
        Box::pin(async {
            self.0
                .lock()
                .unwrap()
                .clone()
                .ok_or(smartthermweb::provider::ProviderError::LoginRequired)
        })
    }
    fn send_target(
        &self,
        _: u64,
        _: smartthermweb::domain::TargetTemperature,
    ) -> smartthermweb::provider::ProviderFuture<'_, ()> {
        panic!("real-mode writes must never reach the provider")
    }
}
fn cloud_app() -> axum::Router {
    router(AppState {
        controller: Controller::ocstat_read_only(),
        simulator: None,
        sessions: Some(Arc::new(smartthermweb::sessions::Sessions::with_factory(
            Arc::new(|| Ok(Arc::new(FakeAccount::default()))),
        ))),
    })
}
fn get_with_cookie(path: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("cookie", cookie)
        .body(Body::empty())
        .unwrap()
}
async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn sign_in(app: &axum::Router, email: &str) -> String {
    let body = serde_json::json!({"email": email, "password": "TEST_PASSWORD_ONLY"}).to_string();
    let response = app
        .clone()
        .oneshot(login_request(
            &body,
            "localhost:3000",
            "http://localhost:3000",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let header = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(header.contains("HttpOnly") && header.contains("SameSite=Strict"));
    let cookie = header.split(';').next().unwrap().to_owned();
    let state = json(response).await;
    assert_eq!(state["login"], "ready");
    let serialized = state.to_string();
    assert!(
        !serialized.contains(email)
            && !serialized.contains("TEST_PASSWORD_ONLY")
            && !serialized.contains(cookie.split('=').nth(1).unwrap())
    );
    cookie
}
#[tokio::test(start_paused = true)]
async fn browser_sessions_isolate_readings_logout_and_streams() {
    let app = cloud_app();
    for path in ["/api/thermostat", "/api/events"] {
        assert_eq!(
            app.clone()
                .oneshot(get_with_cookie(path, ""))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let anonymous = json(
        app.clone()
            .oneshot(get_with_cookie("/api/session", ""))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(anonymous["login"], "required");
    assert!(anonymous["confirmed"].is_null());
    let first = sign_in(&app, "first@example.invalid").await;
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    let second = sign_in(&app, "second@example.invalid").await;
    assert_ne!(first, second);
    for (cookie, temperature) in [(&first, 210), (&second, 190)] {
        let state = json(
            app.clone()
                .oneshot(get_with_cookie("/api/thermostat", cookie))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(state["confirmed"]["room_temperature"], temperature);
        assert_eq!(state["can_set_target"], false);
    }
    let mut stream = app
        .clone()
        .oneshot(get_with_cookie("/api/events", &first))
        .await
        .unwrap()
        .into_body();
    assert!(stream.frame().await.is_some());
    let logout = Request::builder()
        .method("POST")
        .uri("/api/logout")
        .header("host", "localhost:3000")
        .header("origin", "http://localhost:3000")
        .header("cookie", &first)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(logout).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(json(response).await["login"], "required");
    let closed = stream.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(String::from_utf8_lossy(&closed).contains("signed_out"));
    assert!(stream.frame().await.is_none());
    assert_eq!(
        app.clone()
            .oneshot(get_with_cookie("/api/thermostat", &first))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(get_with_cookie("/api/thermostat", &second))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    // Cookies from a previous process cannot authenticate a new server.
    assert_eq!(
        cloud_app()
            .oneshot(get_with_cookie("/api/thermostat", &second))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    tokio::time::advance(std::time::Duration::from_secs(
        smartthermweb::sessions::SESSION_SECONDS,
    ))
    .await;
    assert_eq!(
        app.oneshot(get_with_cookie("/api/thermostat", &second))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test(start_paused = true)]
async fn rejected_login_never_creates_a_session_and_is_rate_limited() {
    let app = cloud_app();
    let body = r#"{"email":"example@example.invalid","password":"WRONG_TEST_PASSWORD"}"#;
    let response = app
        .clone()
        .oneshot(login_request(
            body,
            "localhost:3000",
            "http://localhost:3000",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(json(response).await["error"]["code"], "authentication");
    let response = app
        .clone()
        .oneshot(login_request(
            body,
            "localhost:3000",
            "http://localhost:3000",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let response = app
        .oneshot(get_with_cookie("/api/session", ""))
        .await
        .unwrap();
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(json(response).await["login"], "required");
}
