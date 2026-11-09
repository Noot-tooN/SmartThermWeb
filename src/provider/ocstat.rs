//! HTTPS-only OCSTAT adapter with Auto/temporary-override target changes. Never log credentials or upstream bodies.
use super::{Authenticated, Provider, ProviderError, ProviderFuture};
use crate::domain::{DeviceState, Mode, TargetTemperature, Temperature};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::Mutex;

#[path = "ocstat_save.rs"]
mod save;
#[path = "ocstat_trial.rs"]
pub mod trial;

const ORIGIN: &str = "https://www.ocstat-thermostat.com";
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endpoint {
    Login,
    DeviceList,
    DeviceStatus,
    SetTarget,
    SetProgramme,
    SetProgrammeMode,
}
impl Endpoint {
    fn url(self) -> String {
        format!(
            "{ORIGIN}/ocstat/{}",
            match self {
                Self::Login => "login",
                Self::DeviceList => "getDeviceListNew",
                Self::DeviceStatus => "getDeviceStatus",
                Self::SetTarget => "setThermostatWorkModeNew",
                Self::SetProgramme => "setProgrammeByDay",
                Self::SetProgrammeMode => "setProgrammeModeNew",
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
    device_name: Option<String>,
    device_id: String,
    online: Option<bool>,
    inside_temparature: Option<String>,
    work_mode: Option<String>,
    program_mode: Option<String>,
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
fn decode_device(body: &[u8], device_id: Option<&str>) -> Result<ListedDevice, ProviderError> {
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
    let mut device = matches.next().ok_or(ProviderError::DeviceNotFound)?;
    if matches.next().is_some() {
        return Err(ProviderError::InvalidResponse);
    }
    device.device_name = device.device_name.and_then(|name| {
        let name: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_owned())
    });
    Ok(device)
}
impl ListedDevice {
    fn observation(
        &self,
        status: &StatusObservation,
        revision: u64,
    ) -> Result<DeviceState, ProviderError> {
        let room_temperature = self
            .inside_temparature
            .as_ref()
            .and_then(|v| v.parse::<i16>().ok())
            .map(Temperature)
            .ok_or(ProviderError::InvalidResponse)?;
        Ok(DeviceState {
            device_name: self.device_name.clone(),
            room_temperature,
            // The listing's current_temprature is inconsistent and unnecessary:
            // both targets come from the validated device-status frame.
            target_temperature: status.reported,
            requested_temperature: Some(status.requested),
            online: self.online.ok_or(ProviderError::InvalidResponse)?,
            mode: Mode::Unknown("unverified".into()),
            revision,
            applied_command: None,
            matched_command: None,
            target_writable: false,
        })
    }
}
/// Only the two fields demonstrated by the capture and HTTPS round-trip test.
/// No timestamp or application correlation is inferred from this frame.
#[derive(Debug, PartialEq, Eq)]
struct StatusObservation {
    requested: Temperature,
    reported: Temperature,
    mode_marker: u8,
}
fn decode_status(body: &[u8], device_id: &str) -> Result<StatusObservation, ProviderError> {
    if body.len() != 60
        || !body.iter().all(u8::is_ascii_graphic)
        || !body.starts_with(b"##3cL")
        || device_id.len() != 12
        || &body[5..17] != device_id.as_bytes()
    {
        return Err(ProviderError::InvalidResponse);
    }
    fn target(bytes: &[u8]) -> Result<Temperature, ProviderError> {
        if !bytes.iter().all(u8::is_ascii_hexdigit) {
            return Err(ProviderError::InvalidResponse);
        }
        let raw = std::str::from_utf8(bytes).map_err(|_| ProviderError::InvalidResponse)?;
        let halves = u8::from_str_radix(raw, 16).map_err(|_| ProviderError::InvalidResponse)?;
        Ok(Temperature(i16::from(halves) * 5))
    }
    Ok(StatusObservation {
        requested: target(&body[19..21])?,
        reported: target(&body[36..38])?,
        mode_marker: body[17],
    })
}
#[derive(Deserialize)]
struct WriteResponse {
    statu: bool,
    error_code: i64,
    #[serde(rename = "tcpCode")]
    tcp_code: i64,
    message: String,
}
fn decode_write(body: &[u8]) -> Result<(), ProviderError> {
    let response: WriteResponse =
        serde_json::from_slice(body).map_err(|_| ProviderError::InvalidResponse)?;
    if !response.statu {
        return Err(ProviderError::Rejected);
    }
    if response.error_code != 0
        || response.tcp_code != 0
        || response.message != "modify work mode success"
    {
        return Err(ProviderError::InvalidResponse);
    }
    // Cloud acceptance only. No application evidence is created here.
    Ok(())
}
struct WriteContext {
    work_mode: String,
    program_mode: String,
    status_mode: u8,
    online: bool,
}
impl WriteContext {
    // Type-22 Android updateTemp maps scheduled Auto (0) to temporary override (1).
    // Require agreement between the sequential list/status reads; never guess
    // across a mode change, an unknown program mode, or an unsupported work mode.
    fn observed_mode(&self) -> Mode {
        match (
            self.program_mode.as_str(),
            self.work_mode.as_str(),
            self.status_mode,
        ) {
            ("0" | "1" | "2", "0", b'0') => Mode::Schedule,
            ("0" | "1" | "2", "1", b'1') => Mode::TemporaryOverride,
            _ => Mode::Unknown("unverified".into()),
        }
    }
    fn target_mode(&self) -> Option<&'static str> {
        (self.online
            && matches!(
                self.observed_mode(),
                Mode::Schedule | Mode::TemporaryOverride
            ))
        .then_some("1")
    }
}
struct Attempt {
    id: u64,
    target: Temperature,
    matches: u8,
}
struct AuthState {
    session: Option<Session>,
    device_id: Option<String>,
    revision: u64,
    context: Option<WriteContext>,
    last_attempt: u64,
    message_seed: Option<u16>,
    message_count: u32,
    attempt: Option<Attempt>,
}
/// No Debug implementation: the session token never enters logs/API.
/// Email/password are transient login inputs, never stored in this provider.
pub struct Ocstat {
    transport: Box<dyn Transport>,
    auth: Mutex<AuthState>,
    trial: Option<std::sync::Arc<trial::TrialStore>>,
    schedule_reader: Box<dyn trial::ScheduleReader>,
}
impl Ocstat {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self::with_transport(Box::new(Https::new()?)))
    }
    fn with_transport(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            trial: None,
            schedule_reader: Box::new(trial::TcpScheduleReader),
            auth: Mutex::new(AuthState {
                session: None,
                device_id: None,
                revision: 0,
                context: None,
                last_attempt: 0,
                message_seed: None,
                message_count: 0,
                attempt: None,
            }),
        }
    }
    async fn read_device(&self) -> Result<DeviceState, ProviderError> {
        let mut auth = self.auth.lock().await;
        auth.context = None; // A failed read must never leave writable preflight evidence.
        let result = self.read_locked(&mut auth).await;
        if result.is_err()
            && let Some(attempt) = &mut auth.attempt
        {
            attempt.matches = 0;
        }
        if result == Err(ProviderError::Authentication) {
            auth.session = None;
            auth.device_id = None;
            auth.attempt = None;
        }
        result
    }
    async fn read_locked(&self, auth: &mut AuthState) -> Result<DeviceState, ProviderError> {
        let session = auth.session.as_ref().ok_or(ProviderError::LoginRequired)?;
        let form = vec![
            ("user_id", session.id.to_string()),
            ("token", session.token.clone()),
            ("pageNum", "1".into()),
            ("pageSize", "12".into()),
            ("online", "1".into()),
            ("device_type", "1".into()),
        ];
        let body = self.transport.post(Endpoint::DeviceList, form).await?;
        let device = decode_device(&body, auth.device_id.as_deref())?;
        // Pin even if status fails; subsequent attempts cannot silently select a new device.
        auth.device_id = Some(device.device_id.clone());
        let status = self
            .transport
            .post(
                Endpoint::DeviceStatus,
                vec![
                    ("device_id", device.device_id.clone()),
                    ("user_id", session.id.to_string()),
                    ("token", session.token.clone()),
                ],
            )
            .await?;
        let status = decode_status(&status, &device.device_id)?;
        let mut observed = device.observation(&status, auth.revision + 1)?;
        // List and status are sequential observations, not an atomic device snapshot.
        // Their values can disagree during a change; neither invents command correlation.
        auth.context = Some(WriteContext {
            work_mode: device.work_mode.unwrap_or_default(),
            program_mode: device.program_mode.unwrap_or_default(),
            status_mode: status.mode_marker,
            online: observed.online,
        });
        if let Some(context) = &auth.context {
            observed.mode = context.observed_mode();
            observed.target_writable = context.target_mode().is_some();
        }
        if let Some(attempt) = &mut auth.attempt {
            if observed.target_writable
                && observed.mode == Mode::TemporaryOverride
                && status.requested == attempt.target
                && status.reported == attempt.target
            {
                attempt.matches = attempt.matches.saturating_add(1);
                if attempt.matches >= 2 {
                    observed.matched_command = Some(attempt.id);
                }
            } else {
                attempt.matches = 0;
            }
        }
        auth.revision += 1;
        Ok(observed)
    }
    pub fn with_schedule_trial(mut self, store: std::sync::Arc<trial::TrialStore>) -> Self {
        self.trial = Some(store);
        self
    }
    async fn read_schedule_plaintext(&self) -> Result<crate::schedule::Schedule, ProviderError> {
        let mut auth = self.auth.lock().await;
        self.read_raw_schedule(&mut auth).await?.clean()
    }
    async fn read_raw_schedule(
        &self,
        auth: &mut AuthState,
    ) -> Result<super::ocstat_schedule_tcp::RawSchedule, ProviderError> {
        let before = self.read_locked(auth).await?;
        if !before.online {
            return Err(ProviderError::Offline);
        }
        let mode: u8 = auth
            .context
            .as_ref()
            .ok_or(ProviderError::InvalidResponse)?
            .program_mode
            .parse()
            .map_err(|_| ProviderError::InvalidResponse)?;
        let session = auth.session.as_ref().ok_or(ProviderError::LoginRequired)?;
        let device = auth
            .device_id
            .clone()
            .ok_or(ProviderError::DeviceNotFound)?;
        let tcp_result = self
            .schedule_reader
            .read(session.id, &session.token, &device, mode)
            .await;
        let schedule = tcp_result?;
        let after = self.read_locked(auth).await?;
        if !after.online
            || auth.device_id.as_deref() != Some(device.as_str())
            || auth
                .context
                .as_ref()
                .is_none_or(|c| c.program_mode != mode.to_string())
        {
            return Err(ProviderError::InvalidResponse);
        }
        Ok(schedule)
    }
    async fn send_target_https(
        &self,
        id: u64,
        target: TargetTemperature,
    ) -> Result<(), ProviderError> {
        if id == 0 {
            return Err(ProviderError::UnverifiedProtocol);
        }
        let mut auth = self.auth.lock().await;
        if id <= auth.last_attempt {
            return Err(ProviderError::UnverifiedProtocol);
        }
        auth.context = None;
        auth.attempt = None;
        let preflight = self.read_locked(&mut auth).await;
        if preflight == Err(ProviderError::Authentication) {
            auth.session = None;
            auth.device_id = None;
        }
        preflight?;
        let context = auth
            .context
            .as_ref()
            .ok_or(ProviderError::UnverifiedProtocol)?;
        if !context.online {
            return Err(ProviderError::Offline);
        }
        // Same target action as Android: start (0 -> 1) or continue (1 -> 1)
        // a temporary override. Never send 0, which would request scheduled Auto.
        let mode = context
            .target_mode()
            .ok_or(ProviderError::UnverifiedProtocol)?
            .to_owned();
        if auth.message_count >= 65536 {
            return Err(ProviderError::UnverifiedProtocol);
        }
        let seed = match auth.message_seed {
            Some(seed) => seed,
            None => {
                let mut bytes = [0; 2];
                getrandom::getrandom(&mut bytes).map_err(|_| ProviderError::Transport)?;
                let seed = u16::from_be_bytes(bytes);
                auth.message_seed = Some(seed);
                seed
            }
        };
        let message_id = format!("{:04x}", seed.wrapping_add(auth.message_count as u16));
        // Consume before the exchange: cancellation/lost replies must not reuse IDs.
        auth.message_count += 1;
        auth.last_attempt = id;
        auth.attempt = Some(Attempt {
            id,
            target: Temperature(target.into()),
            matches: 0,
        });
        let session = auth.session.as_ref().ok_or(ProviderError::LoginRequired)?;
        let form = vec![
            (
                "device_id",
                auth.device_id
                    .clone()
                    .ok_or(ProviderError::DeviceNotFound)?,
            ),
            ("user_id", session.id.to_string()),
            ("current_temprature", i16::from(target).to_string()),
            ("messageId", message_id),
            ("work_mode", mode),
            ("token", session.token.clone()),
        ];
        let result = self
            .transport
            .post(Endpoint::SetTarget, form)
            .await
            .and_then(|body| decode_write(&body));
        if result == Err(ProviderError::Authentication) {
            auth.session = None;
            auth.device_id = None;
        }
        if matches!(
            result,
            Err(ProviderError::Rejected | ProviderError::Authentication)
        ) {
            auth.attempt = None;
        }
        auth.context = None;
        result
    }
}
impl Provider for Ocstat {
    fn read_schedule_preview(&self) -> ProviderFuture<'_, crate::schedule::Preview> {
        Box::pin(self.schedule_snapshot())
    }
    fn recover_schedule(&self) -> ProviderFuture<'_, crate::schedule::Preview> {
        Box::pin(self.adopt_server_schedule())
    }
    fn save_schedule(
        &self,
        request: crate::schedule::SaveRequest,
    ) -> ProviderFuture<'_, crate::schedule::SaveResult> {
        Box::pin(self.save_schedule_checked(request))
    }
    fn logout(&self) -> super::LogoutFuture<'_> {
        Box::pin(async move {
            let mut auth = self.auth.lock().await;
            auth.session = None;
            auth.device_id = None;
            auth.context = None;
            auth.last_attempt = 0;
            auth.message_seed = None;
            auth.message_count = 0;
            auth.attempt = None;
        })
    }
    fn login(&self, email: String, password: String) -> ProviderFuture<'_, Authenticated> {
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
            let observation = self.read_device().await;
            // Explicit authentication/discovery failures retain their existing login errors.
            // An unreadable temperature or a transient read failure must not discard a
            // successfully authenticated session. Polling can recover without a password.
            match observation {
                Err(
                    error @ (ProviderError::Authentication
                    | ProviderError::LoginRequired
                    | ProviderError::DeviceNotFound
                    | ProviderError::MultipleDevices),
                ) => Err(error),
                observation => Ok(Authenticated { observation }),
            }
        })
    }
    fn schedule_trial(&self, action: trial::Action) -> ProviderFuture<'_, trial::Status> {
        Box::pin(self.run_schedule_trial(action))
    }
    fn read_schedule(&self) -> ProviderFuture<'_, crate::schedule::Schedule> {
        Box::pin(self.read_schedule_plaintext())
    }
    fn read(&self) -> ProviderFuture<'_, DeviceState> {
        Box::pin(self.read_device())
    }
    fn send_target(&self, id: u64, target: TargetTemperature) -> ProviderFuture<'_, ()> {
        Box::pin(self.send_target_https(id, target))
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
    const LIST: &[u8] = br#"{"statu":true,"error_code":1,"devices":[{"device_id":"OTHER_DEVICE"},{"device_id":"TESTDEVICE01","device_name":"Living room","online":true,"current_temprature":"215","inside_temparature":"211","password":"IGNORED_SECRET","work_mode":"1","program_mode":"1"}]}"#;
    fn single() -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(LIST).unwrap();
        value["devices"].as_array_mut().unwrap().remove(0);
        serde_json::to_vec(&value).unwrap()
    }
    fn status(requested: i16, reported: i16) -> Vec<u8> {
        let mut frame = b"##3cLTESTDEVICE01102b1701011619000162b03-0000420000000010000".to_vec();
        assert_eq!(frame.len(), 60);
        frame[19..21].copy_from_slice(format!("{:02x}", requested / 5).as_bytes());
        frame[36..38].copy_from_slice(format!("{:02x}", reported / 5).as_bytes());
        frame
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
            replies: StdMutex::new(
                replies
                    .into_iter()
                    .flat_map(|reply| {
                        let is_list = reply.as_ref().ok().is_some_and(|body| {
                            serde_json::from_slice::<serde_json::Value>(body)
                                .ok()
                                .is_some_and(|v| v.get("devices").is_some())
                        });
                        let mut pair = vec![reply];
                        if is_list {
                            pair.push(Ok(status(215, 215)));
                        }
                        pair
                    })
                    .collect(),
            ),
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
            .await?
            .observation
    }
    fn decode_fixture_device(
        body: &[u8],
        device_id: Option<&str>,
        revision: u64,
    ) -> Result<DeviceState, ProviderError> {
        super::decode_device(body, device_id)?.observation(
            &StatusObservation {
                requested: Temperature(215),
                reported: Temperature(215),
                mode_marker: b'1',
            },
            revision,
        )
    }
    #[tokio::test]
    async fn inconsistent_listing_target_never_blocks_valid_status_targets() {
        for value in [
            serde_json::Value::Null,
            "".into(),
            "21.5".into(),
            "unavailable".into(),
            "99999".into(),
            215.into(),
        ] {
            let mut list: serde_json::Value = serde_json::from_slice(&single()).unwrap();
            list["devices"][0]["current_temprature"] = value;
            let (provider, fake) = fixture(vec![
                Ok(LOGIN.to_vec()),
                Ok(serde_json::to_vec(&list).unwrap()),
            ]);
            let observed = login(&provider).await.unwrap();
            assert_eq!(observed.requested_temperature, Some(Temperature(215)));
            assert_eq!(observed.target_temperature, Temperature(215));
            assert!(observed.target_writable);
            assert_eq!(fake.calls.lock().unwrap().len(), 3);
        }
    }
    #[tokio::test]
    async fn authenticated_session_survives_missing_room_reading_and_recovers_without_password() {
        let mut list: serde_json::Value = serde_json::from_slice(&single()).unwrap();
        list["devices"][0]
            .as_object_mut()
            .unwrap()
            .remove("current_temprature");
        list["devices"][0]["inside_temparature"] = serde_json::Value::Null;
        let (provider, fake) = fixture(vec![
            Ok(LOGIN.to_vec()),
            Ok(serde_json::to_vec(&list).unwrap()),
            Ok(single()),
        ]);
        let authenticated = provider
            .login(
                "example@example.invalid".into(),
                "TEST_PASSWORD_ONLY".into(),
            )
            .await
            .unwrap();
        assert_eq!(
            authenticated.observation,
            Err(ProviderError::InvalidResponse)
        );
        assert!(provider.auth.lock().await.context.is_none());
        let recovered = provider.read().await.unwrap();
        assert_eq!(recovered.room_temperature, Temperature(211));
        assert!(recovered.target_writable);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.iter().filter(|c| c.0 == Endpoint::Login).count(), 1);
        assert!(calls.iter().all(|c| matches!(
            c.0,
            Endpoint::Login | Endpoint::DeviceList | Endpoint::DeviceStatus
        )));
    }
    #[tokio::test]
    async fn initial_status_failure_never_exposes_writable_context() {
        for error in [
            ProviderError::Transport,
            ProviderError::InvalidResponse,
            ProviderError::Authentication,
        ] {
            let fake = Arc::new(Fake {
                replies: StdMutex::new(VecDeque::from([
                    Ok(LOGIN.to_vec()),
                    Ok(single()),
                    Err(error),
                ])),
                ..Fake::default()
            });
            let provider = Ocstat::with_transport(Box::new(fake));
            let result = provider
                .login(
                    "example@example.invalid".into(),
                    "TEST_PASSWORD_ONLY".into(),
                )
                .await;
            if error == ProviderError::Authentication {
                assert_eq!(result, Err(error));
                assert!(provider.auth.lock().await.session.is_none());
            } else {
                assert_eq!(result.unwrap().observation, Err(error));
            }
            assert!(provider.auth.lock().await.context.is_none());
        }
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
        assert_eq!(first.device_name.as_deref(), Some("Living room"));
        assert_eq!(first.target_temperature, Temperature(215));
        assert_eq!(first.room_temperature, Temperature(211));
        assert_eq!(second.revision, first.revision + 1);
        assert!(second.applied_command.is_none());
        let json = serde_json::to_string(&second).unwrap();
        for secret in [
            "TEST_TOKEN_ONLY",
            "IGNORED_SECRET",
            "TESTDEVICE01",
            "TEST_PASSWORD_ONLY",
            "example@example.invalid",
        ] {
            assert!(!json.contains(secret));
        }
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![
                Endpoint::Login,
                Endpoint::DeviceList,
                Endpoint::DeviceStatus,
                Endpoint::DeviceList,
                Endpoint::DeviceStatus
            ]
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
        assert_eq!(fake.calls.lock().unwrap().len(), 7);
    }
    #[tokio::test]
    async fn logout_and_new_process_both_require_browser_login() {
        let (provider, fake) = fixture(vec![Ok(LOGIN.to_vec()), Ok(single())]);
        assert!(login(&provider).await.is_ok());
        provider.logout().await;
        assert_eq!(provider.read().await, Err(ProviderError::LoginRequired));
        assert_eq!(fake.calls.lock().unwrap().len(), 3);
        let auth = provider.auth.lock().await;
        assert!(auth.session.is_none() && auth.device_id.is_none());
        let fresh = Ocstat::with_transport(Box::new(fake.clone()));
        assert_eq!(fresh.read().await, Err(ProviderError::LoginRequired));
    }
    #[tokio::test]
    async fn signed_out_writes_never_contact_upstream() {
        let (provider, fake) = fixture(vec![]);
        assert_eq!(
            provider.send_target(1, 220.try_into().unwrap()).await,
            Err(ProviderError::LoginRequired)
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }
    #[test]
    fn discovery_never_guesses_between_multiple_devices() {
        assert_eq!(
            decode_fixture_device(LIST, None, 1),
            Err(ProviderError::MultipleDevices)
        );
        assert_eq!(
            decode_fixture_device(br#"{"statu":true,"error_code":1,"devices":[]}"#, None, 1),
            Err(ProviderError::DeviceNotFound)
        );
    }
    #[test]
    fn device_labels_are_optional_bounded_and_do_not_fall_back_to_identifiers() {
        let mut value: serde_json::Value = serde_json::from_slice(&single()).unwrap();
        value["devices"][0]["device_name"] = "  Living\nroom  ".into();
        let decode = |value: &serde_json::Value| {
            decode_fixture_device(&serde_json::to_vec(value).unwrap(), None, 1).unwrap()
        };
        assert_eq!(decode(&value).device_name.as_deref(), Some("Livingroom"));
        value["devices"][0]["device_name"] = "x".repeat(200).into();
        assert_eq!(decode(&value).device_name.unwrap().chars().count(), 120);
        for name in [serde_json::Value::Null, "  ".into()] {
            value["devices"][0]["device_name"] = name;
            assert!(decode(&value).device_name.is_none());
        }
        value["devices"][0]
            .as_object_mut()
            .unwrap()
            .remove("device_name");
        assert!(decode(&value).device_name.is_none());
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
            assert!(decode_fixture_device(body, Some("TESTDEVICE01"), 1).is_err());
        }
        assert_eq!(
            decode_fixture_device(LIST, Some("MISSING"), 1),
            Err(ProviderError::DeviceNotFound)
        );
        let mut value: serde_json::Value = serde_json::from_slice(LIST).unwrap();
        value["error_code"] = 0.into();
        assert_eq!(
            decode_fixture_device(
                &serde_json::to_vec(&value).unwrap(),
                Some("TESTDEVICE01"),
                1
            ),
            Err(ProviderError::InvalidResponse)
        );
        value["error_code"] = 1.into();
        value["devices"][1]["inside_temparature"] = "21.5".into();
        assert_eq!(
            decode_fixture_device(
                &serde_json::to_vec(&value).unwrap(),
                Some("TESTDEVICE01"),
                1
            ),
            Err(ProviderError::InvalidResponse)
        );
        value["devices"][1]["inside_temparature"] = "321".into();
        value["devices"][1]["online"] = false.into();
        let device = decode_fixture_device(
            &serde_json::to_vec(&value).unwrap(),
            Some("TESTDEVICE01"),
            1,
        )
        .unwrap();
        assert_eq!(device.room_temperature, Temperature(321)); // Not constrained to simulator limits.
        assert!(!device.online);
        let duplicate = value["devices"][1].clone();
        value["devices"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(
            decode_fixture_device(
                &serde_json::to_vec(&value).unwrap(),
                Some("TESTDEVICE01"),
                1
            ),
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
        for endpoint in [
            Endpoint::Login,
            Endpoint::DeviceList,
            Endpoint::DeviceStatus,
            Endpoint::SetTarget,
        ] {
            assert!(
                endpoint
                    .url()
                    .starts_with("https://www.ocstat-thermostat.com/ocstat/")
            );
        }
    }
    fn write_fixture(replies: Vec<Result<Vec<u8>, ProviderError>>) -> (Ocstat, Arc<Fake>) {
        // Same provider logic as production; all I/O is intercepted by the fake.
        let fake = Arc::new(Fake {
            replies: StdMutex::new(replies.into()),
            ..Fake::default()
        });
        let provider = Ocstat::with_transport(Box::new(fake.clone()));
        (provider, fake)
    }
    fn accepted() -> Vec<u8> {
        br#"{"statu":true,"error_code":0,"message":"modify work mode success","tcpCode":0}"#
            .to_vec()
    }
    fn preflight(list: Vec<u8>, frame: Vec<u8>) -> Vec<Result<Vec<u8>, ProviderError>> {
        vec![Ok(list), Ok(frame)]
    }
    async fn authenticate_fake(provider: &Ocstat) {
        let mut auth = provider.auth.lock().await;
        auth.session = Some(Session {
            id: 7,
            token: "TEST_TOKEN_ONLY".into(),
        });
        auth.device_id = Some("TESTDEVICE01".into());
        auth.message_seed = Some(0x1234);
    }
    #[test]
    fn status_preserves_requested_reported_distinction_and_rejects_unknown_frames() {
        let lagging = status(215, 225);
        assert_eq!(
            decode_status(&lagging, "TESTDEVICE01").unwrap(),
            StatusObservation {
                requested: Temperature(215),
                reported: Temperature(225),
                mode_marker: b'1',
            }
        );
        assert_eq!(
            decode_status(&status(215, 215), "TESTDEVICE01")
                .unwrap()
                .reported,
            Temperature(215)
        );
        assert!(decode_status(&lagging, "TESTDEVICE02").is_err());
        for index in [0, 3, 4, 5, 19, 20, 36, 37] {
            let mut corrupt = lagging.clone();
            corrupt[index] = b'!';
            assert!(decode_status(&corrupt, "TESTDEVICE01").is_err());
        }
        for body in [
            &lagging[..59],
            b"Illegal access".as_slice(),
            b"<html>login</html>",
        ] {
            assert!(decode_status(body, "TESTDEVICE01").is_err());
        }
        let mut extra = lagging.clone();
        extra.push(b'0');
        assert!(decode_status(&extra, "TESTDEVICE01").is_err());
    }
    #[test]
    fn only_observed_write_success_is_accepted_and_never_means_application() {
        assert_eq!(decode_write(&accepted()), Ok(()));
        let success: serde_json::Value = serde_json::from_slice(&accepted()).unwrap();
        for (key, value) in [
            ("error_code", 1.into()),
            ("tcpCode", 1.into()),
            ("message", "other".into()),
            ("statu", "true".into()),
        ] {
            let mut reply = success.clone();
            reply[key] = value;
            assert_eq!(
                decode_write(&serde_json::to_vec(&reply).unwrap()),
                Err(ProviderError::InvalidResponse)
            );
        }
        for key in ["error_code", "tcpCode", "message", "statu"] {
            let mut reply = success.clone();
            reply.as_object_mut().unwrap().remove(key);
            assert_eq!(
                decode_write(&serde_json::to_vec(&reply).unwrap()),
                Err(ProviderError::InvalidResponse)
            );
        }
        let mut rejected = success;
        rejected["statu"] = false.into();
        assert_eq!(
            decode_write(&serde_json::to_vec(&rejected).unwrap()),
            Err(ProviderError::Rejected)
        );
        for body in [b"Illegal access".as_slice(), b"{}", b"<html>login</html>"] {
            assert_eq!(decode_write(body), Err(ProviderError::InvalidResponse));
        }
    }
    #[tokio::test]
    async fn candidate_write_preserves_captured_mode_and_requires_fresh_read() {
        let mut replies = preflight(single(), status(215, 215));
        replies.push(Ok(accepted()));
        replies.extend(preflight(single(), status(225, 215)));
        replies.extend(preflight(single(), status(225, 225)));
        let (provider, fake) = write_fixture(replies);
        authenticate_fake(&provider).await;
        provider
            .send_target(1, 225.try_into().unwrap())
            .await
            .unwrap();
        let lag = provider.read().await.unwrap();
        assert_eq!(lag.requested_temperature, Some(Temperature(225)));
        assert_eq!(lag.target_temperature, Temperature(215));
        let applied = provider.read().await.unwrap();
        assert_eq!(applied.target_temperature, Temperature(225));
        assert!(lag.applied_command.is_none() && applied.applied_command.is_none());
        assert_eq!(applied.mode, Mode::TemporaryOverride);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.iter().map(|c| c.0).collect::<Vec<_>>(),
            vec![
                Endpoint::DeviceList,
                Endpoint::DeviceStatus,
                Endpoint::SetTarget,
                Endpoint::DeviceList,
                Endpoint::DeviceStatus,
                Endpoint::DeviceList,
                Endpoint::DeviceStatus
            ]
        );
        assert_eq!(
            calls[2].1,
            vec![
                ("device_id", "TESTDEVICE01".into()),
                ("user_id", "7".into()),
                ("current_temprature", "225".into()),
                ("messageId", "1234".into()),
                ("work_mode", "1".into()),
                ("token", "TEST_TOKEN_ONLY".into()),
            ]
        );
    }
    fn mode_reads(work: &str, program: &str, marker: u8) -> Vec<Result<Vec<u8>, ProviderError>> {
        let mut list: serde_json::Value = serde_json::from_slice(&single()).unwrap();
        list["devices"][0]["work_mode"] = work.into();
        list["devices"][0]["program_mode"] = program.into();
        let mut frame = status(215, 215);
        frame[17] = marker;
        preflight(serde_json::to_vec(&list).unwrap(), frame)
    }
    #[tokio::test]
    async fn auto_adjustment_starts_override_and_does_not_invent_application() {
        let mut replies = mode_reads("0", "1", b'0');
        replies.extend(mode_reads("0", "1", b'0'));
        replies.push(Ok(accepted()));
        replies.extend(preflight(single(), status(225, 225)));
        replies.extend(preflight(single(), status(225, 225)));
        let (provider, fake) = write_fixture(replies);
        authenticate_fake(&provider).await;
        let auto = provider.read().await.unwrap();
        assert_eq!(auto.mode, Mode::Schedule);
        assert!(auto.target_writable);
        provider
            .send_target(1, 225.try_into().unwrap())
            .await
            .unwrap();
        let first = provider.read().await.unwrap();
        assert_eq!(first.mode, Mode::TemporaryOverride);
        assert!(first.matched_command.is_none());
        let second = provider.read().await.unwrap();
        assert_eq!(second.matched_command, Some(1));
        assert!(second.applied_command.is_none());
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls[2].0, Endpoint::DeviceList);
        assert_eq!(calls[3].0, Endpoint::DeviceStatus);
        assert_eq!(calls[4].0, Endpoint::SetTarget);
        assert_eq!(
            calls.iter().filter(|c| c.0 == Endpoint::SetTarget).count(),
            1
        );
        assert_eq!(
            calls[4].1,
            vec![
                ("device_id", "TESTDEVICE01".into()),
                ("user_id", "7".into()),
                ("current_temprature", "225".into()),
                ("messageId", "1234".into()),
                ("work_mode", "1".into()),
                ("token", "TEST_TOKEN_ONLY".into()),
            ]
        );
    }
    #[tokio::test]
    async fn all_schedule_patterns_preserve_auto_and_override_target_controls() {
        // Android updateTemp uses work_mode only; grouping does not change
        // the Auto -> temporary-override command. Unknown groupings still fail.
        for program in ["0", "1", "2"] {
            for (work, marker, observed) in [
                ("0", b'0', Mode::Schedule),
                ("1", b'1', Mode::TemporaryOverride),
            ] {
                let mut replies = mode_reads(work, program, marker);
                replies.extend(mode_reads(work, program, marker));
                replies.push(Ok(accepted()));
                let (provider, fake) = write_fixture(replies);
                authenticate_fake(&provider).await;
                let state = provider.read().await.unwrap();
                assert_eq!(state.mode, observed);
                assert!(state.target_writable);
                provider
                    .send_target(1, 225.try_into().unwrap())
                    .await
                    .unwrap();
                let calls = fake.calls.lock().unwrap();
                let write = calls
                    .iter()
                    .find(|(e, _)| *e == Endpoint::SetTarget)
                    .unwrap();
                assert!(write.1.contains(&("work_mode", "1".into())));
            }
        }
    }
    #[tokio::test]
    async fn preflight_mode_changes_cannot_reuse_prior_writability() {
        for (work, program, marker) in [
            ("0", "1", b'1'),
            ("1", "1", b'0'),
            ("0", "3", b'0'),
            ("2", "1", b'2'),
            ("4", "1", b'4'),
            ("6", "1", b'6'),
            ("9", "1", b'9'),
        ] {
            let mut replies = mode_reads("0", "1", b'0');
            replies.extend(mode_reads(work, program, marker));
            let (provider, fake) = write_fixture(replies);
            authenticate_fake(&provider).await;
            assert!(provider.read().await.unwrap().target_writable);
            assert_eq!(
                provider.send_target(1, 225.try_into().unwrap()).await,
                Err(ProviderError::UnverifiedProtocol)
            );
            assert!(
                fake.calls
                    .lock()
                    .unwrap()
                    .iter()
                    .all(|c| c.0 != Endpoint::SetTarget)
            );
        }
    }
    #[tokio::test]
    async fn scheduled_values_do_not_count_as_an_override_match() {
        let mut replies = mode_reads("0", "1", b'0');
        replies.push(Ok(accepted()));
        replies.extend(mode_reads("0", "1", b'0'));
        replies.extend(mode_reads("0", "1", b'0'));
        let (provider, _) = write_fixture(replies);
        authenticate_fake(&provider).await;
        provider
            .send_target(1, 215.try_into().unwrap())
            .await
            .unwrap();
        for _ in 0..2 {
            let reading = provider.read().await.unwrap();
            assert!(reading.target_writable);
            assert!(reading.matched_command.is_none());
            assert!(reading.applied_command.is_none());
        }
    }
    #[tokio::test]
    async fn unsupported_modes_offline_and_invalid_preflight_never_send() {
        for (field, value, error) in [
            ("work_mode", "0".into(), ProviderError::UnverifiedProtocol),
            (
                "program_mode",
                "3".into(),
                ProviderError::UnverifiedProtocol,
            ),
            (
                "work_mode",
                serde_json::Value::Null,
                ProviderError::UnverifiedProtocol,
            ),
            ("online", false.into(), ProviderError::Offline),
        ] {
            let mut list: serde_json::Value = serde_json::from_slice(&single()).unwrap();
            list["devices"][0][field] = value;
            let (provider, fake) = write_fixture(preflight(
                serde_json::to_vec(&list).unwrap(),
                status(215, 215),
            ));
            authenticate_fake(&provider).await;
            assert_eq!(
                provider.send_target(1, 225.try_into().unwrap()).await,
                Err(error)
            );
            assert!(
                !fake
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|c| c.0 == Endpoint::SetTarget)
            );
        }
        for frame in [b"bad status".to_vec(), {
            let mut s = status(215, 215);
            s[17] = b'0';
            s
        }] {
            let (provider, fake) = write_fixture(preflight(single(), frame));
            authenticate_fake(&provider).await;
            assert!(
                provider
                    .send_target(1, 225.try_into().unwrap())
                    .await
                    .is_err()
            );
            assert_eq!(fake.calls.lock().unwrap().len(), 2);
        }
        let (provider, fake) = write_fixture(vec![]);
        authenticate_fake(&provider).await;
        assert_eq!(
            provider.send_target(0, 220.try_into().unwrap()).await,
            Err(ProviderError::UnverifiedProtocol)
        );
        assert!(fake.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn lost_replies_retries_and_repeated_values_never_reuse_message_ids() {
        let mut replies = preflight(single(), status(215, 215));
        replies.push(Err(ProviderError::Transport));
        replies.extend(preflight(single(), status(225, 225)));
        replies.push(Ok(accepted()));
        let (provider, fake) = write_fixture(replies);
        authenticate_fake(&provider).await;
        assert_eq!(
            provider.send_target(1, 225.try_into().unwrap()).await,
            Err(ProviderError::Transport)
        );
        assert_eq!(
            provider.send_target(1, 225.try_into().unwrap()).await,
            Err(ProviderError::UnverifiedProtocol)
        );
        provider
            .send_target(2, 225.try_into().unwrap())
            .await
            .unwrap();
        let calls = fake.calls.lock().unwrap();
        let ids: Vec<_> = calls
            .iter()
            .filter(|c| c.0 == Endpoint::SetTarget)
            .map(|c| c.1.iter().find(|x| x.0 == "messageId").unwrap().1.as_str())
            .collect();
        assert_eq!(ids, vec!["1234", "1235"]);
        assert_eq!(calls.len(), 6); // No automatic retry, not even after the lost reply.
    }
    #[tokio::test]
    async fn read_status_failure_cannot_publish_partial_list_or_reuse_write_context() {
        let mut replies = preflight(single(), status(215, 215));
        replies.extend(vec![
            Ok(single()),
            Err(ProviderError::InvalidResponse),
            Ok(single()),
            Err(ProviderError::Authentication),
        ]);
        let (provider, _) = write_fixture(replies);
        authenticate_fake(&provider).await;
        assert_eq!(provider.read().await.unwrap().revision, 1);
        assert_eq!(provider.read().await, Err(ProviderError::InvalidResponse));
        {
            let auth = provider.auth.lock().await;
            assert_eq!(auth.revision, 1);
            assert!(auth.context.is_none());
        }
        assert_eq!(provider.read().await, Err(ProviderError::Authentication));
        assert_eq!(provider.read().await, Err(ProviderError::LoginRequired));
    }
    struct PendingReply {
        fake: Arc<Fake>,
        started: tokio::sync::Notify,
    }
    impl Transport for Arc<PendingReply> {
        fn post(&self, endpoint: Endpoint, form: Form) -> ProviderFuture<'_, Vec<u8>> {
            if endpoint != Endpoint::SetTarget {
                return self.fake.post(endpoint, form);
            }
            self.fake.calls.lock().unwrap().push((endpoint, form));
            self.started.notify_one();
            Box::pin(std::future::pending())
        }
    }
    #[tokio::test]
    async fn canceled_exchange_consumes_message_id_and_requires_a_new_attempt() {
        let transport = Arc::new(PendingReply {
            fake: Arc::new(Fake {
                replies: StdMutex::new(preflight(single(), status(215, 215)).into()),
                ..Fake::default()
            }),
            started: tokio::sync::Notify::new(),
        });
        let provider = Ocstat::with_transport(Box::new(transport.clone()));
        authenticate_fake(&provider).await;
        let provider = Arc::new(provider);
        let task = tokio::spawn({
            let provider = provider.clone();
            async move { provider.send_target(1, 225.try_into().unwrap()).await }
        });
        transport.started.notified().await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            provider.send_target(1, 225.try_into().unwrap()).await,
            Err(ProviderError::UnverifiedProtocol)
        );
        let auth = provider.auth.lock().await;
        assert_eq!(auth.last_attempt, 1);
        assert_eq!(auth.message_count, 1);
        assert_eq!(transport.fake.calls.lock().unwrap().len(), 3);
    }
    #[tokio::test(start_paused = true)]
    async fn enabled_worker_waits_for_two_matching_reads_without_inventing_acknowledgement() {
        use crate::controller::{Controller, Timing};
        use crate::domain::{LoginState, Phase};
        let mut replies = vec![Ok(LOGIN.to_vec())];
        replies.extend(preflight(single(), status(215, 215)));
        replies.extend(preflight(single(), status(215, 215)));
        replies.push(Ok(accepted()));
        replies.extend(preflight(single(), status(225, 215)));
        replies.extend(preflight(single(), status(225, 225)));
        replies.extend(preflight(single(), status(225, 225)));
        let (provider, fake) = write_fixture(replies);
        let controller = Controller::ocstat();
        let mut events = controller.subscribe();
        controller
            .submit_login("test@example.invalid".into(), "TEST_PASSWORD_ONLY".into())
            .unwrap();
        let worker = tokio::spawn(controller.clone().run(
            Arc::new(provider),
            Timing {
                confirmation: Duration::from_secs(30),
                poll: Duration::from_secs(30),
                ..Timing::default()
            },
        ));
        while events.recv().await.unwrap().login != Some(LoginState::Ready) {}
        assert!(controller.snapshot().can_set_target);
        controller.submit(225.try_into().unwrap()).unwrap();
        let mut saw_acceptance = false;
        let mut saw_lag = false;
        let mut saw_first_match_waiting = false;
        loop {
            let snapshot = events.recv().await.unwrap();
            if let Some(command) = snapshot.command {
                saw_acceptance |= command.phase == Phase::AcceptedByCloud;
                if let Some(read) = snapshot.confirmed {
                    assert!(read.applied_command.is_none());
                    saw_lag |= read.requested_temperature == Some(Temperature(225))
                        && read.target_temperature == Temperature(215);
                    saw_first_match_waiting |= command.phase == Phase::AwaitingDevice
                        && read.target_temperature == Temperature(225)
                        && read.matched_command.is_none();
                    if command.phase == Phase::TargetMatched {
                        assert_eq!(read.matched_command, Some(command.id));
                        break;
                    }
                }
            }
        }
        worker.abort();
        assert!(saw_acceptance && saw_lag && saw_first_match_waiting);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.iter().filter(|c| c.0 == Endpoint::SetTarget).count(),
            1
        );
        assert!(
            !calls
                .iter()
                .any(|c| c.1.iter().any(|f| f.0 == "userid_phoneid"))
        );
    }
    #[tokio::test]
    async fn mismatches_and_read_failures_reset_match_evidence_and_new_attempt_supersedes_old() {
        let mut replies = preflight(single(), status(215, 215));
        replies.push(Err(ProviderError::Transport));
        replies.extend(preflight(single(), status(225, 225)));
        replies.push(Err(ProviderError::Transport));
        replies.extend(preflight(single(), status(225, 225)));
        replies.extend(preflight(single(), status(225, 215)));
        replies.extend(preflight(single(), status(225, 225)));
        replies.extend(preflight(single(), status(225, 225)));
        replies.extend(preflight(single(), status(225, 225)));
        replies.push(Ok(accepted()));
        replies.extend(preflight(single(), status(225, 225)));
        replies.extend(preflight(single(), status(220, 220)));
        replies.extend(preflight(single(), status(220, 220)));
        let (provider, _) = write_fixture(replies);
        authenticate_fake(&provider).await;
        assert_eq!(
            provider.send_target(1, 225.try_into().unwrap()).await,
            Err(ProviderError::Transport)
        );
        assert!(provider.read().await.unwrap().matched_command.is_none());
        assert_eq!(provider.read().await, Err(ProviderError::Transport));
        for _ in 0..3 {
            assert!(provider.read().await.unwrap().matched_command.is_none());
        }
        assert_eq!(provider.read().await.unwrap().matched_command, Some(1));
        provider
            .send_target(2, 220.try_into().unwrap())
            .await
            .unwrap();
        assert!(provider.read().await.unwrap().matched_command.is_none());
        assert!(provider.read().await.unwrap().matched_command.is_none());
        assert_eq!(provider.read().await.unwrap().matched_command, Some(2));
    }
}
