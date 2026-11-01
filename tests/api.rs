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
            simulator: Arc::new(Simulator::new(Config::default())),
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
        controller.submit((200 + (i % 10) * 5).try_into().unwrap());
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
