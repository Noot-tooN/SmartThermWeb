//! HTTPS-only, read-only OCSTAT adapter. Never log credentials or upstream bodies.
use super::{Provider, ProviderError, ProviderFuture};
use crate::domain::{DeviceState, Mode, TargetTemperature, Temperature};
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const ORIGIN: &str = "https://www.ocstat-thermostat.com";
const MAX_RESPONSE: usize = 1024 * 1024;

/// Server-only provisioning. Deliberately neither Debug nor Serialize.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    email: String,
    password: String,
    userid_phoneid: String,
    device_id: String,
}
impl Credentials {
    pub fn from_file(path: &Path) -> Result<Self, &'static str> {
        let file = File::open(path).map_err(|_| "Cannot open OCSTAT credential file.")?;
        let metadata = file
            .metadata()
            .map_err(|_| "Cannot inspect OCSTAT credential file.")?;
        if !metadata.is_file() || metadata.len() > 16_384 {
            return Err("OCSTAT credentials must be a regular file of at most 16 KiB.");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err("OCSTAT credential file must have owner-only permissions (chmod 600).");
            }
        }
        let mut bytes = Vec::new();
        file.take(16_385)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read OCSTAT credential file.")?;
        if bytes.len() > 16_384 {
            return Err("OCSTAT credential file is too large.");
        }
        let credentials: Self = serde_json::from_slice(&bytes).map_err(|_| "Invalid OCSTAT credential file; expected email, password, userid_phoneid, device_id.")?;
        credentials.validate()?;
        Ok(credentials)
    }
    fn validate(&self) -> Result<(), &'static str> {
        if [
            &self.email,
            &self.password,
            &self.userid_phoneid,
            &self.device_id,
        ]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 1024)
        {
            return Err("OCSTAT credential fields must be nonempty and at most 1024 bytes.");
        }
        Ok(())
    }
}

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
    device_id: &str,
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
    let mut matches = devices.into_iter().filter(|d| d.device_id == device_id);
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
    login_after: Instant,
    auth_blocked: bool,
    revision: u64,
}
/// No Debug implementation: credentials and the session token never enter logs/API.
pub struct Ocstat {
    credentials: Credentials,
    transport: Box<dyn Transport>,
    auth: Mutex<AuthState>,
}
impl Ocstat {
    pub fn new(credentials: Credentials) -> Result<Self, reqwest::Error> {
        Ok(Self::with_transport(credentials, Box::new(Https::new()?)))
    }
    fn with_transport(credentials: Credentials, transport: Box<dyn Transport>) -> Self {
        Self {
            credentials,
            transport,
            auth: Mutex::new(AuthState {
                session: None,
                login_after: Instant::now(),
                auth_blocked: false,
                revision: 0,
            }),
        }
    }
    async fn read_device(&self) -> Result<DeviceState, ProviderError> {
        let mut auth = self.auth.lock().await;
        if auth.auth_blocked {
            return Err(ProviderError::Authentication);
        }
        if auth.session.is_none() {
            if Instant::now() < auth.login_after {
                return Err(ProviderError::Transport);
            }
            // Set before awaiting so timeouts/cancellation cannot cause a login storm.
            auth.login_after = Instant::now() + Duration::from_secs(60);
            let login = self
                .transport
                .post(
                    Endpoint::Login,
                    vec![
                        ("email", self.credentials.email.clone()),
                        ("password", self.credentials.password.clone()),
                    ],
                )
                .await
                .and_then(|body| decode_login(&body));
            match login {
                Ok(session) => auth.session = Some(session),
                Err(error) => {
                    auth.auth_blocked = error == ProviderError::Authentication;
                    return Err(error);
                }
            }
        }
        let session = auth.session.as_ref().ok_or(ProviderError::Authentication)?;
        let body = self
            .transport
            .post(
                Endpoint::DeviceList,
                vec![
                    ("userid_phoneid", self.credentials.userid_phoneid.clone()),
                    ("user_id", session.id.to_string()),
                    ("token", session.token.clone()),
                    ("pageNum", "1".into()),
                    ("pageSize", "12".into()),
                    ("online", "1".into()),
                    ("device_type", "1".into()),
                ],
            )
            .await;
        if matches!(body, Err(ProviderError::Authentication)) {
            auth.session = None;
            // No speculative refresh or repeated password login on unknown expiry semantics.
            auth.auth_blocked = true;
        }
        let observed = decode_device(&body?, &self.credentials.device_id, auth.revision + 1)?;
        auth.revision += 1;
        Ok(observed)
    }
}
impl Provider for Ocstat {
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
    fn credentials() -> Credentials {
        Credentials {
            email: "example@example.invalid".into(),
            password: "TEST_PASSWORD_ONLY".into(),
            userid_phoneid: "TEST_CLIENT".into(),
            device_id: "TEST_DEVICE".into(),
        }
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
        (
            Ocstat::with_transport(credentials(), Box::new(fake.clone())),
            fake,
        )
    }
    #[tokio::test]
    async fn login_once_reuse_token_and_return_only_safe_status() {
        let (provider, fake) = fixture(vec![
            Ok(LOGIN.to_vec()),
            Ok(LIST.to_vec()),
            Ok(LIST.to_vec()),
        ]);
        let first = provider.read().await.unwrap();
        let second = provider.read().await.unwrap();
        assert_eq!(first.target_temperature, Temperature(215));
        assert_eq!(first.room_temperature, Temperature(211));
        assert!(first.online);
        assert!(first.applied_command.is_none());
        assert!(matches!(first.mode, Mode::Unknown(_)));
        assert_eq!(second.revision, first.revision + 1);
        let json = serde_json::to_string(&second).unwrap();
        for secret in [
            "TEST_TOKEN_ONLY",
            "IGNORED_SECRET",
            "TEST_DEVICE",
            "TEST_CLIENT",
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
            assert!(form.contains(&("user_id", "7".into())));
            assert!(
                !form
                    .iter()
                    .any(|(name, _)| matches!(*name, "password" | "email"))
            );
        }
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
    #[tokio::test(start_paused = true)]
    async fn rejected_credentials_stop_until_restart() {
        let (provider, fake) = fixture(vec![Ok(br#"{"statu":false,"error_code":-1}"#.to_vec())]);
        for _ in 0..3 {
            assert_eq!(provider.read().await, Err(ProviderError::Authentication));
            tokio::time::advance(Duration::from_secs(120)).await;
        }
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
    }
    #[tokio::test(start_paused = true)]
    async fn transport_failure_has_login_cooldown_but_can_recover() {
        let (provider, fake) = fixture(vec![
            Err(ProviderError::Transport),
            Ok(LOGIN.to_vec()),
            Ok(LIST.to_vec()),
        ]);
        assert_eq!(provider.read().await, Err(ProviderError::Transport));
        assert_eq!(provider.read().await, Err(ProviderError::Transport));
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(provider.read().await.is_ok());
    }
    #[tokio::test]
    async fn read_failure_does_not_trigger_password_login_and_auth_failure_stops() {
        let (provider, fake) = fixture(vec![
            Ok(LOGIN.to_vec()),
            Err(ProviderError::Transport),
            Ok(LIST.to_vec()),
            Err(ProviderError::Authentication),
        ]);
        assert_eq!(provider.read().await, Err(ProviderError::Transport));
        assert!(provider.read().await.is_ok());
        assert_eq!(provider.read().await, Err(ProviderError::Authentication));
        assert_eq!(provider.read().await, Err(ProviderError::Authentication));
        assert_eq!(fake.calls.lock().unwrap().len(), 4);
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
            assert!(decode_device(body, "TEST_DEVICE", 1).is_err());
        }
        assert_eq!(
            decode_device(LIST, "MISSING", 1),
            Err(ProviderError::DeviceNotFound)
        );
        let mut value: serde_json::Value = serde_json::from_slice(LIST).unwrap();
        value["error_code"] = 0.into();
        assert_eq!(
            decode_device(&serde_json::to_vec(&value).unwrap(), "TEST_DEVICE", 1),
            Err(ProviderError::InvalidResponse)
        );
        value["error_code"] = 1.into();
        value["devices"][1]["current_temprature"] = "21.5".into();
        assert_eq!(
            decode_device(&serde_json::to_vec(&value).unwrap(), "TEST_DEVICE", 1),
            Err(ProviderError::InvalidResponse)
        );
        value["devices"][1]["current_temprature"] = "321".into();
        value["devices"][1]["online"] = false.into();
        let device = decode_device(&serde_json::to_vec(&value).unwrap(), "TEST_DEVICE", 1).unwrap();
        assert_eq!(device.target_temperature, Temperature(321)); // Not constrained to simulator limits.
        assert!(!device.online);
        let duplicate = value["devices"][1].clone();
        value["devices"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(
            decode_device(&serde_json::to_vec(&value).unwrap(), "TEST_DEVICE", 1),
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
    #[test]
    #[cfg(unix)]
    fn credentials_require_private_file_and_errors_are_sanitized() {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let path = std::env::temp_dir().join(format!(
            "smarttherm-test-credentials-{}",
            std::process::id()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file.write_all(br#"{"email":"example@example.invalid","password":"TEST_PASSWORD_ONLY","userid_phoneid":"TEST_CLIENT","device_id":"TEST_DEVICE"}"#).unwrap();
        assert!(Credentials::from_file(&path).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Credentials::from_file(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, "TEST_PASSWORD_ONLY invalid JSON").unwrap();
        let error = Credentials::from_file(&path).err().unwrap();
        assert!(!error.contains("TEST_PASSWORD_ONLY"));
        std::fs::remove_file(path).unwrap();
    }
}
