//! HTTPS-only, read-only OCSTAT adapter. Never log credentials or upstream bodies.
use super::{Provider, ProviderError, ProviderFuture};
use crate::domain::{DeviceState, Mode, TargetTemperature, Temperature};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::Mutex;

const ORIGIN: &str = "https://www.ocstat-thermostat.com";
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endpoint {
    Login,
    DeviceList,
}
impl Endpoint {
    fn url(self) -> String {
        format!(
            "{ORIGIN}/ocstat/{}",
            match self {
                Self::Login => "login",
                Self::DeviceList => "getDeviceListNew",
            }
        )
    }
}
type Form = Vec<(&'static str, String)>;
trait Transport: Send + Sync {
    fn post(&self, endpoint: Endpoint, form: Form) -> ProviderFuture<'_, Vec<u8>>;
}
struct Https(reqwest::Client);
impl Https {
    fn new() -> Result<Self, reqwest::Error> {
        Ok(Self(
            reqwest::Client::builder()
                .https_only(true)
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        ))
    }
}
impl Transport for Https {
    fn post(&self, endpoint: Endpoint, form: Form) -> ProviderFuture<'_, Vec<u8>> {
        Box::pin(async move {
            let mut response = self
                .0
                .post(endpoint.url())
                .form(&form)
                .send()
                .await
                .map_err(|_| ProviderError::Transport)?;
            if matches!(response.status().as_u16(), 401 | 403) {
                return Err(ProviderError::Authentication);
            }
            // Redirects and every non-2xx response fail closed. Never follow Location.
            if !response.status().is_success() {
                return Err(ProviderError::Transport);
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_RESPONSE as u64)
            {
                return Err(ProviderError::InvalidResponse);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| ProviderError::Transport)?
            {
                if bytes.len() + chunk.len() > MAX_RESPONSE {
                    return Err(ProviderError::InvalidResponse);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
    }
}

// Only the fields needed for reads are retained. Upstream credential-like extras
// (smartPwd, password, etc.) are deliberately ignored during deserialization.
#[derive(Deserialize)]
struct LoginResponse {
    statu: bool,
    error_code: i64,
    user: Option<Session>,
}
#[derive(Deserialize)]
struct Session {
    id: u64,
    token: String,
}
#[derive(Deserialize)]
struct ListResponse {
    statu: bool,
    error_code: i64,
    devices: Option<Vec<ListedDevice>>,
}
#[derive(Deserialize)]
struct ListedDevice {
    device_id: String,
    online: Option<bool>,
    current_temprature: Option<String>,
    inside_temparature: Option<String>,
}
fn decode_login(body: &[u8]) -> Result<Session, ProviderError> {
    let response: LoginResponse =
        serde_json::from_slice(body).map_err(|_| ProviderError::InvalidResponse)?;
    if !response.statu {
        return Err(ProviderError::Authentication);
    }
    if response.error_code != 0 {
        return Err(ProviderError::InvalidResponse);
    }
    let session = response.user.ok_or(ProviderError::InvalidResponse)?;
    if session.id == 0 || session.token.trim().is_empty() || session.token.len() > 4096 {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(session)
}
fn decode_device(
    body: &[u8],
    device_id: Option<&str>,
    revision: u64,
) -> Result<DeviceState, ProviderError> {
    let response: ListResponse =
        serde_json::from_slice(body).map_err(|_| ProviderError::InvalidResponse)?;
    if !response.statu {
        return Err(ProviderError::Rejected);
    }
    // Success codes differ between login and listing in the verified exchanges.
    if response.error_code != 1 {
        return Err(ProviderError::InvalidResponse);
    }
    let devices = response.devices.ok_or(ProviderError::InvalidResponse)?;
    if device_id.is_none() && devices.len() > 1 {
        return Err(ProviderError::MultipleDevices);
    }
    let mut matches = devices
        .into_iter()
        .filter(|d| device_id.is_none_or(|id| d.device_id == id));
    let device = matches.next().ok_or(ProviderError::DeviceNotFound)?;
    if matches.next().is_some() {
        return Err(ProviderError::InvalidResponse);
    }
    fn temperature(raw: Option<String>) -> Result<Temperature, ProviderError> {
        raw.and_then(|v| v.parse::<i16>().ok())
            .map(Temperature)
            .ok_or(ProviderError::InvalidResponse)
    }
    Ok(DeviceState {
        room_temperature: temperature(device.inside_temparature)?,
        target_temperature: temperature(device.current_temprature)?,
        online: device.online.ok_or(ProviderError::InvalidResponse)?,
        mode: Mode::Unknown("unverified".into()),
        revision,
        applied_command: None,
    })
}
struct AuthState {
    session: Option<Session>,
    device_id: Option<String>,
    revision: u64,
}
/// No Debug implementation: the session token never enters logs/API.
/// Email/password are transient login inputs, never stored in this provider.
pub struct Ocstat {
    transport: Box<dyn Transport>,
    auth: Mutex<AuthState>,
}
impl Ocstat {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self::with_transport(Box::new(Https::new()?)))
    }
    fn with_transport(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            auth: Mutex::new(AuthState {
                session: None,
                device_id: None,
                revision: 0,
            }),
        }
    }
    async fn read_device(&self) -> Result<DeviceState, ProviderError> {
        let mut auth = self.auth.lock().await;
        let session = auth.session.as_ref().ok_or(ProviderError::LoginRequired)?;
        let form = vec![
            ("user_id", session.id.to_string()),
            ("token", session.token.clone()),
            ("pageNum", "1".into()),
            ("pageSize", "12".into()),
            ("online", "1".into()),
            ("device_type", "1".into()),
        ];
        let body = self.transport.post(Endpoint::DeviceList, form).await;
        if matches!(body, Err(ProviderError::Authentication)) {
            auth.session = None;
            auth.device_id = None;
        }
        let body = body?;
        let observed = decode_device(&body, auth.device_id.as_deref(), auth.revision + 1)?;
        if auth.device_id.is_none() {
            // Pin only a uniquely discovered device; do not switch accounts/devices on later polls.
            let list: ListResponse =
                serde_json::from_slice(&body).map_err(|_| ProviderError::InvalidResponse)?;
            auth.device_id = Some(
                list.devices
                    .and_then(|items| items.into_iter().next())
                    .ok_or(ProviderError::DeviceNotFound)?
                    .device_id,
            );
        }
        auth.revision += 1;
        Ok(observed)
    }
}
impl Provider for Ocstat {
    fn logout(&self) -> super::LogoutFuture<'_> {
        Box::pin(async move {
            let mut auth = self.auth.lock().await;
            auth.session = None;
            auth.device_id = None;
        })
    }
    fn login(&self, email: String, password: String) -> ProviderFuture<'_, DeviceState> {
        Box::pin(async move {
            self.logout().await;
            if [&email, &password]
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 1024)
            {
                return Err(ProviderError::Authentication);
            }
            // Move the only credential strings into this one request. No password
            // is retained for polling, refresh, retries, or subsequent server starts.
            let body = self
                .transport
                .post(
                    Endpoint::Login,
                    vec![("email", email), ("password", password)],
                )
                .await?;
            let session = decode_login(&body)?;
            self.auth.lock().await.session = Some(session);
            self.read_device().await
        })
    }
    fn read(&self) -> ProviderFuture<'_, DeviceState> {
        Box::pin(self.read_device())
    }
    fn send_target(&self, _id: u64, _target: TargetTemperature) -> ProviderFuture<'_, ()> {
        Box::pin(async { Err(ProviderError::ReadOnly) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex as StdMutex},
    };
    const LOGIN: &[u8] = br#"{"statu":true,"error_code":0,"user":{"id":7,"token":"TEST_TOKEN_ONLY","smartPwd":"IGNORED_SECRET"}}"#;
    const LIST: &[u8] = br#"{"statu":true,"error_code":1,"devices":[{"device_id":"OTHER_DEVICE"},{"device_id":"TEST_DEVICE","online":true,"current_temprature":"215","inside_temparature":"211","password":"IGNORED_SECRET","work_mode":"1"}]}"#;
    fn single() -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(LIST).unwrap();
        value["devices"].as_array_mut().unwrap().remove(0);
        serde_json::to_vec(&value).unwrap()
    }
    #[derive(Default)]
    struct Fake {
        replies: StdMutex<VecDeque<Result<Vec<u8>, ProviderError>>>,
        calls: StdMutex<Vec<(Endpoint, Form)>>,
    }
    impl Transport for Arc<Fake> {
        fn post(&self, endpoint: Endpoint, form: Form) -> ProviderFuture<'_, Vec<u8>> {
            self.calls.lock().unwrap().push((endpoint, form));
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected upstream call");
            Box::pin(async move { reply })
        }
    }
    fn fixture(replies: Vec<Result<Vec<u8>, ProviderError>>) -> (Ocstat, Arc<Fake>) {
        let fake = Arc::new(Fake {
            replies: StdMutex::new(replies.into()),
            ..Fake::default()
        });
        (Ocstat::with_transport(Box::new(fake.clone())), fake)
    }
    async fn login(provider: &Ocstat) -> Result<DeviceState, ProviderError> {
        provider
            .login(
                "example@example.invalid".into(),
                "TEST_PASSWORD_ONLY".into(),
            )
            .await
    }
    #[tokio::test]
    async fn reads_before_browser_login_never_contact_upstream() {
        let (provider, fake) = fixture(vec![]);
        for _ in 0..3 {
            assert_eq!(provider.read().await, Err(ProviderError::LoginRequired));
        }
        assert!(fake.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn explicit_login_discovers_and_pins_device_then_reuses_only_token() {
        let (provider, fake) = fixture(vec![Ok(LOGIN.to_vec()), Ok(single()), Ok(LIST.to_vec())]);
        let first = login(&provider).await.unwrap();
        let second = provider.read().await.unwrap();
        assert_eq!(first.target_temperature, Temperature(215));
        assert_eq!(first.room_temperature, Temperature(211));
        assert_eq!(second.revision, first.revision + 1);
        assert!(second.applied_command.is_none());
        let json = serde_json::to_string(&second).unwrap();
        for secret in [
            "TEST_TOKEN_ONLY",
            "IGNORED_SECRET",
            "TEST_DEVICE",
            "TEST_PASSWORD_ONLY",
            "example@example.invalid",
        ] {
            assert!(!json.contains(secret));
        }
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![Endpoint::Login, Endpoint::DeviceList, Endpoint::DeviceList]
        );
        for (_, form) in &calls[1..] {
            assert!(form.contains(&("token", "TEST_TOKEN_ONLY".into())));
            assert!(
                !form
                    .iter()
                    .any(|(key, _)| matches!(*key, "email" | "password" | "userid_phoneid"))
            );
        }
    }
    #[tokio::test]
    async fn rejected_or_failed_login_is_never_retried_by_reads() {
        for failure in [
            Ok(br#"{"statu":false,"error_code":-1}"#.to_vec()),
            Err(ProviderError::Transport),
        ] {
            let (provider, fake) = fixture(vec![failure, Ok(LOGIN.to_vec()), Ok(single())]);
            assert!(login(&provider).await.is_err());
            for _ in 0..3 {
                assert_eq!(provider.read().await, Err(ProviderError::LoginRequired));
            }
            assert_eq!(fake.calls.lock().unwrap().len(), 1);
            assert!(login(&provider).await.is_ok()); // Only a new explicit submission retries.
        }
    }
    #[tokio::test]
    async fn read_failures_do_not_trigger_password_login_and_rejection_clears_token() {
        let (provider, fake) = fixture(vec![
            Ok(LOGIN.to_vec()),
            Ok(single()),
            Err(ProviderError::Transport),
            Ok(single()),
            Err(ProviderError::Authentication),
        ]);
        assert!(login(&provider).await.is_ok());
        assert_eq!(provider.read().await, Err(ProviderError::Transport));
        assert!(provider.read().await.is_ok());
        assert_eq!(provider.read().await, Err(ProviderError::Authentication));
        assert_eq!(provider.read().await, Err(ProviderError::LoginRequired));
        assert_eq!(fake.calls.lock().unwrap().len(), 5);
    }
    #[tokio::test]
    async fn logout_and_new_process_both_require_browser_login() {
        let (provider, fake) = fixture(vec![Ok(LOGIN.to_vec()), Ok(single())]);
        assert!(login(&provider).await.is_ok());
        provider.logout().await;
        assert_eq!(provider.read().await, Err(ProviderError::LoginRequired));
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        let auth = provider.auth.lock().await;
        assert!(auth.session.is_none() && auth.device_id.is_none());
        let fresh = Ocstat::with_transport(Box::new(fake.clone()));
        assert_eq!(fresh.read().await, Err(ProviderError::LoginRequired));
    }
    #[tokio::test]
    async fn writes_never_contact_upstream() {
        let (provider, fake) = fixture(vec![]);
        assert_eq!(
            provider.send_target(1, 220.try_into().unwrap()).await,
            Err(ProviderError::ReadOnly)
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }
    #[test]
    fn discovery_never_guesses_between_multiple_devices() {
        assert_eq!(
            decode_device(LIST, None, 1),
            Err(ProviderError::MultipleDevices)
        );
        assert_eq!(
            decode_device(br#"{"statu":true,"error_code":1,"devices":[]}"#, None, 1),
            Err(ProviderError::DeviceNotFound)
        );
    }
    #[test]
    fn decoders_fail_closed_without_leaking_response_data() {
        for body in [
            b"Illegal access".as_slice(),
            b"##3cLREDACTED",
            b"<html>login</html>",
            br#"{"statu":true,"error_code":0,"user":{"id":7,"token":""}}"#,
        ] {
            assert!(decode_login(body).is_err());
            assert!(decode_device(body, Some("TEST_DEVICE"), 1).is_err());
        }
        assert_eq!(
            decode_device(LIST, Some("MISSING"), 1),
            Err(ProviderError::DeviceNotFound)
        );
        let mut value: serde_json::Value = serde_json::from_slice(LIST).unwrap();
        value["error_code"] = 0.into();
        assert_eq!(
            decode_device(&serde_json::to_vec(&value).unwrap(), Some("TEST_DEVICE"), 1),
            Err(ProviderError::InvalidResponse)
        );
        value["error_code"] = 1.into();
        value["devices"][1]["current_temprature"] = "21.5".into();
        assert_eq!(
            decode_device(&serde_json::to_vec(&value).unwrap(), Some("TEST_DEVICE"), 1),
            Err(ProviderError::InvalidResponse)
        );
        value["devices"][1]["current_temprature"] = "321".into();
        value["devices"][1]["online"] = false.into();
        let device =
            decode_device(&serde_json::to_vec(&value).unwrap(), Some("TEST_DEVICE"), 1).unwrap();
        assert_eq!(device.target_temperature, Temperature(321)); // Not constrained to simulator limits.
        assert!(!device.online);
        let duplicate = value["devices"][1].clone();
        value["devices"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(
            decode_device(&serde_json::to_vec(&value).unwrap(), Some("TEST_DEVICE"), 1),
            Err(ProviderError::InvalidResponse)
        );
    }
    #[tokio::test]
    async fn production_client_rejects_http_before_sending() {
        let transport = Https::new().unwrap();
        let error = transport
            .0
            .post("http://127.0.0.1:9/ocstat/login")
            .send()
            .await
            .unwrap_err();
        assert!(error.is_builder());
        for endpoint in [Endpoint::Login, Endpoint::DeviceList] {
            assert!(
                endpoint
                    .url()
                    .starts_with("https://www.ocstat-thermostat.com/ocstat/")
            );
        }
    }
}
