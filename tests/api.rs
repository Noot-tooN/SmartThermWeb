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
        r#"{"target_temperature":49}"#,
        r#"{"target_temperature":351}"#,
        r#"{"target_temperature":355}"#,
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
    for target in [50, 305, 350] {
        let response = app
            .clone()
            .oneshot(put(&format!(r#"{{"target_temperature":{target}}}"#)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
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
async fn signed_out_ocstat_cannot_write_or_access_simulator_settings() {
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
        let mut request = put(body);
        request
            .headers_mut()
            .insert("host", "localhost:3000".parse().unwrap());
        request
            .headers_mut()
            .insert("origin", "http://localhost:3000".parse().unwrap());
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]["code"],
            "unauthenticated"
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
    assert_eq!(value["can_set_target"], true);
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
                device_name: Some(
                    if email.starts_with("second") {
                        "Bedroom"
                    } else {
                        "Living room"
                    }
                    .into(),
                ),
                room_temperature: Temperature(if email.starts_with("second") {
                    190
                } else {
                    210
                }),
                target_temperature: Temperature(215),
                requested_temperature: None,
                online: true,
                mode: Mode::Unknown("unverified".into()),
                revision: 1,
                applied_command: None,
                matched_command: None,
                target_writable: true,
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
    fn schedule_trial(
        &self,
        _action: smartthermweb::provider::ocstat::trial::Action,
    ) -> smartthermweb::provider::ProviderFuture<'_, smartthermweb::provider::ocstat::trial::Status>
    {
        Box::pin(async {
            let guard = self.0.lock().unwrap();
            let state = guard
                .as_ref()
                .ok_or(smartthermweb::provider::ProviderError::LoginRequired)?;
            Ok(smartthermweb::provider::ocstat::trial::Status {
                trial_id: None,
                phase: None,
                group: "Weekends",
                start_minute: None,
                original_temperature: Some(state.room_temperature.0 as u16),
                trial_temperature: None,
                message: "Synthetic trial status".into(),
            })
        })
    }
    fn recover_schedule(
        &self,
    ) -> smartthermweb::provider::ProviderFuture<'_, smartthermweb::schedule::Preview> {
        self.read_schedule_preview()
    }
    fn read_schedule(
        &self,
    ) -> smartthermweb::provider::ProviderFuture<'_, smartthermweb::schedule::Schedule> {
        Box::pin(async {
            let guard = self.0.lock().unwrap();
            let state = guard
                .as_ref()
                .ok_or(smartthermweb::provider::ProviderError::LoginRequired)?;
            Ok(smartthermweb::schedule::Schedule {
                pattern: smartthermweb::schedule::Pattern::Daily,
                groups: vec![smartthermweb::schedule::DayGroup {
                    group: smartthermweb::schedule::Group::EveryDay,
                    changes: vec![smartthermweb::schedule::Change {
                        start_minute: 0,
                        target_temperature: state.room_temperature.0.try_into().unwrap(),
                    }],
                }],
            })
        })
    }
    fn send_target(
        &self,
        id: u64,
        target: smartthermweb::domain::TargetTemperature,
    ) -> smartthermweb::provider::ProviderFuture<'_, ()> {
        Box::pin(async move {
            let mut guard = self.0.lock().unwrap();
            let state = guard
                .as_mut()
                .ok_or(smartthermweb::provider::ProviderError::LoginRequired)?;
            state.target_temperature = smartthermweb::domain::Temperature(target.into());
            state.requested_temperature = Some(state.target_temperature);
            state.matched_command = Some(id);
            state.revision += 1;
            Ok(())
        })
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
    assert_eq!(state["account_email"], email);
    let serialized = state.to_string();
    assert!(
        !serialized.contains("TEST_PASSWORD_ONLY")
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
    assert!(anonymous["account_email"].is_null());
    let first = sign_in(&app, "first@example.invalid").await;
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    let second = sign_in(&app, "second@example.invalid").await;
    assert_ne!(first, second);
    for (cookie, temperature, email, name) in [
        (&first, 210, "first@example.invalid", "Living room"),
        (&second, 190, "second@example.invalid", "Bedroom"),
    ] {
        let state = json(
            app.clone()
                .oneshot(get_with_cookie("/api/thermostat", cookie))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(state["confirmed"]["room_temperature"], temperature);
        assert_eq!(state["account_email"], email);
        assert_eq!(state["confirmed"]["device_name"], name);
        assert_eq!(state["can_set_target"], true);
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
    let signed_out = json(response).await;
    assert_eq!(signed_out["login"], "required");
    assert!(signed_out["account_email"].is_null());
    assert!(signed_out["confirmed"].is_null());
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

#[tokio::test(start_paused = true)]
async fn authenticated_target_writes_require_same_origin_and_stay_in_their_browser_session() {
    let app = cloud_app();
    let first = sign_in(&app, "first@example.invalid").await;
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    let second = sign_in(&app, "second@example.invalid").await;
    let request = |cookie: &str, origin: &str| {
        Request::builder()
            .method("PUT")
            .uri("/api/thermostat/target")
            .header("host", "localhost:3000")
            .header("origin", origin)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"target_temperature":225,"client_id":"browser-a","client_revision":1}"#,
            ))
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request("", "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(request(&first, "https://example.invalid"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = app
        .clone()
        .oneshot(request(&first, "http://localhost:3000"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let queued = json(response).await;
    assert_eq!(queued["desired"], 225);
    assert_eq!(queued["account_email"], "first@example.invalid");
    assert_eq!(
        app.clone()
            .oneshot(request(&first, "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    let other = json(
        app.clone()
            .oneshot(get_with_cookie("/api/thermostat", &second))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(other["desired"], 215);
    assert!(other["command"].is_null());
}

fn schedule_request(body: &str, cookie: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/thermostat/schedule/preview")
        .header("content-type", "application/json")
        .header("host", "localhost:3000")
        .header("origin", origin)
        .header("cookie", cookie)
        .body(Body::from(body.to_owned()))
        .unwrap()
}
const DRAFT: &str = r#"{"pattern":"daily","groups":[{"group":"every_day","changes":[{"start_minute":420,"target_temperature":215},{"start_minute":1410,"target_temperature":185}]}]}"#;
#[tokio::test]
async fn schedule_preview_validates_without_mutating_thermostat_state() {
    let (app, controller) = app();
    let before = serde_json::to_value(controller.snapshot()).unwrap();
    let response = app
        .clone()
        .oneshot(schedule_request(DRAFT, "", "http://localhost:3000"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = json(response).await;
    assert_eq!(body["can_save"], false);
    assert_eq!(body["days"].as_array().unwrap().len(), 7);
    assert_eq!(body["days"][0]["periods"][0]["target_temperature"], 185);
    for body in [
        DRAFT.replace("420", "421"),
        DRAFT.replace("215", "355"),
        DRAFT.replace("215", "214"),
        DRAFT.replace("every_day", "monday"),
        "{}".into(),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(schedule_request(&body, "", "http://localhost:3000"))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(serde_json::to_value(controller.snapshot()).unwrap(), before);
    // Saving always requires an authenticated same-origin browser session.
    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/thermostat/schedule")
                .body(Body::from(DRAFT))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
#[tokio::test]
async fn schedule_preview_requires_signed_in_same_origin_session() {
    let app = cloud_app();
    assert_eq!(
        app.clone()
            .oneshot(schedule_request(DRAFT, "", "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = sign_in(&app, "one@example.test").await;
    assert_eq!(
        app.clone()
            .oneshot(schedule_request(DRAFT, &cookie, "https://other.example"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.oneshot(schedule_request(DRAFT, &cookie, "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test(start_paused = true)]
async fn schedule_read_is_authenticated_isolated_and_never_writes() {
    let app = cloud_app();
    let path = "/api/thermostat/schedule";
    assert_eq!(
        app.clone()
            .oneshot(get_with_cookie(path, ""))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let one = sign_in(&app, "one@example.test").await;
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    let two = sign_in(&app, "second@example.test").await;
    let before = json(
        app.clone()
            .oneshot(get_with_cookie("/api/thermostat", &one))
            .await
            .unwrap(),
    )
    .await;
    let a = app
        .clone()
        .oneshot(get_with_cookie(path, &one))
        .await
        .unwrap();
    assert_eq!(a.status(), StatusCode::OK);
    assert_eq!(a.headers()["cache-control"], "no-store");
    let a = json(a).await;
    let b = json(
        app.clone()
            .oneshot(get_with_cookie(path, &two))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(a["can_save"], false);
    assert_ne!(
        a["schedule"]["groups"][0]["changes"][0]["target_temperature"],
        b["schedule"]["groups"][0]["changes"][0]["target_temperature"]
    );
    let after = json(
        app.clone()
            .oneshot(get_with_cookie("/api/thermostat", &one))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(before["desired"], after["desired"]);
    assert_eq!(before["command"], after["command"]);
}

#[tokio::test(start_paused = true)]
async fn trial_requires_opt_in_same_origin_auth_and_valid_actions() {
    use smartthermweb::api::router_with_schedule_trial;
    fn request(body: &str, cookie: &str, origin: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/thermostat/schedule/trial")
            .header("host", "localhost:3000")
            .header("origin", origin)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }
    let body = r#"{"action":"status"}"#;
    assert_eq!(
        cloud_app()
            .oneshot(request(body, "", "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let app = router_with_schedule_trial(
        AppState {
            controller: Controller::ocstat_read_only(),
            simulator: None,
            sessions: Some(Arc::new(smartthermweb::sessions::Sessions::with_factory(
                Arc::new(|| Ok(Arc::new(FakeAccount::default()))),
            ))),
        },
        true,
    );
    assert_eq!(
        app.clone()
            .oneshot(request(body, "", "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let one = sign_in(&app, "one@example.test").await;
    assert_eq!(
        app.clone()
            .oneshot(request(body, &one, "https://other.example"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for invalid in [
        r#"{"action":"apply"}"#,
        r#"{"action":"status","extra":true}"#,
        r#"{"action":"apply","trial_id":"x","confirm_change":"yes"}"#,
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request(invalid, &one, "http://localhost:3000"))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let result = app
        .clone()
        .oneshot(request(body, &one, "http://localhost:3000"))
        .await
        .unwrap();
    assert_eq!(result.headers()["cache-control"], "no-store");
    assert_eq!(json(result).await["original_temperature"], 210);
    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    let two = sign_in(&app, "second@example.test").await;
    let result = app
        .oneshot(request(body, &two, "http://localhost:3000"))
        .await
        .unwrap();
    assert_eq!(json(result).await["original_temperature"], 190);
}

#[tokio::test(start_paused = true)]
async fn schedule_save_requires_auth_origin_and_valid_baseline_and_does_not_enable_other_providers()
{
    fn req(body: &str, cookie: &str, origin: &str) -> Request<Body> {
        Request::builder()
            .method("PUT")
            .uri("/api/thermostat/schedule")
            .header("host", "localhost:3000")
            .header("origin", origin)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap()
    }
    let app = cloud_app();
    let body = format!("{{\"baseline\":{DRAFT},\"schedule\":{DRAFT}}}");
    assert_eq!(
        app.clone()
            .oneshot(req(&body, "", "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = sign_in(&app, "one@example.test").await;
    assert_eq!(
        app.clone()
            .oneshot(req(&body, &cookie, "https://other.example"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for invalid in [
        DRAFT.to_owned(),
        body.replace("215", "214"),
        body.replace("420", "421"),
        body.replace("\"baseline\"", "\"unexpected\""),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(req(&invalid, &cookie, "http://localhost:3000"))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let response = app
        .oneshot(req(&body, &cookie, "http://localhost:3000"))
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK); // Default providers remain read-only.
}

#[tokio::test(start_paused = true)]
async fn schedule_recovery_is_authenticated_same_origin_and_read_only() {
    let app = cloud_app();
    let request = |cookie: &str, origin: &str| {
        Request::builder()
            .method("POST")
            .uri("/api/thermostat/schedule/recover")
            .header("host", "localhost:3000")
            .header("origin", origin)
            .header("cookie", cookie)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request("", "http://localhost:3000"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = sign_in(&app, "one@example.test").await;
    assert_eq!(
        app.clone()
            .oneshot(request(&cookie, "https://other.example"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let before = json(
        app.clone()
            .oneshot(get_with_cookie("/api/thermostat", &cookie))
            .await
            .unwrap(),
    )
    .await;
    let recovered = app
        .clone()
        .oneshot(request(&cookie, "http://localhost:3000"))
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(recovered.headers()["cache-control"], "no-store");
    assert_eq!(json(recovered).await["save_pending"], false);
    let after = json(
        app.oneshot(get_with_cookie("/api/thermostat", &cookie))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(before["desired"], after["desired"]);
    assert_eq!(before["command"], after["command"]);
}
