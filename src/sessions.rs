//! Browser sessions established only by a successful OCSTAT login.
//! These are not local user accounts. Never log cookies or upstream tokens.
use crate::{
    controller::{Controller, Timing},
    domain::{LoginState, Snapshot},
    provider::{Provider, ProviderError, ocstat::Ocstat},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    task::JoinHandle,
    time::{Instant, sleep_until, timeout},
};

pub const COOKIE_NAME: &str = "smarttherm_session";
pub const SESSION_SECONDS: u64 = 12 * 60 * 60;
const MAX_SESSIONS: usize = 32;
pub type ProviderFactory = Arc<dyn Fn() -> Result<Arc<dyn Provider>, ProviderError> + Send + Sync>;

pub struct BrowserSession {
    pub controller: Controller,
    pub expires: Instant,
    pub revoked: watch::Sender<bool>,
    worker: JoinHandle<()>,
}
impl BrowserSession {
    fn revoke(&self) {
        let _ = self.controller.submit_logout();
        self.revoked.send_replace(true);
        self.worker.abort();
    }
    pub fn active(&self) -> bool {
        Instant::now() < self.expires && !*self.revoked.borrow()
    }
}
impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.revoked.send_replace(true);
        self.worker.abort();
    }
}
struct Inner {
    entries: HashMap<String, Arc<BrowserSession>>,
    next_login: Instant,
}
pub struct Sessions {
    inner: Mutex<Inner>,
    login_gate: AsyncMutex<()>,
    factory: ProviderFactory,
}
#[derive(Debug, PartialEq, Eq)]
pub enum SessionError {
    RateLimited,
    Capacity,
    Login(ProviderError),
    Unavailable,
}
impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}
impl Sessions {
    pub fn new() -> Self {
        Self::with_factory(Arc::new(|| {
            Ok(Arc::new(
                Ocstat::new().map_err(|_| ProviderError::Transport)?,
            ))
        }))
    }
    pub fn with_factory(factory: ProviderFactory) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                next_login: Instant::now(),
            }),
            login_gate: AsyncMutex::new(()),
            factory,
        }
    }
    pub fn get(&self, token: &str) -> Option<Arc<BrowserSession>> {
        let mut inner = self.inner.lock().unwrap();
        inner.entries.retain(|_, entry| entry.active());
        inner.entries.get(token).cloned()
    }
    pub fn logout(&self, token: &str) {
        if let Some(entry) = self.inner.lock().unwrap().entries.remove(token) {
            entry.revoke();
        }
    }
    pub async fn login(
        &self,
        email: String,
        password: String,
    ) -> Result<(String, Snapshot), SessionError> {
        let _gate = self
            .login_gate
            .try_lock()
            .map_err(|_| SessionError::RateLimited)?;
        {
            let mut inner = self.inner.lock().unwrap();
            inner.entries.retain(|_, entry| entry.active());
            if Instant::now() < inner.next_login {
                return Err(SessionError::RateLimited);
            }
            if inner.entries.len() >= MAX_SESSIONS {
                return Err(SessionError::Capacity);
            }
            inner.next_login = Instant::now() + Duration::from_secs(10);
        }
        let provider = (self.factory)().map_err(SessionError::Login)?;
        let controller = Controller::ocstat();
        let mut events = controller.subscribe();
        controller
            .submit_login(email, password)
            .map_err(|_| SessionError::Unavailable)?;
        let expires = Instant::now() + Duration::from_secs(SESSION_SECONDS);
        let (revoked, mut cancel) = watch::channel(false);
        let worker_controller = controller.clone();
        let done = revoked.clone();
        let worker = tokio::spawn(async move {
            tokio::select! {
                _ = worker_controller.run(provider, Timing { request: Duration::from_secs(35), poll: Duration::from_secs(30), confirmation: Duration::from_secs(30) }) => {},
                _ = sleep_until(expires) => {},
                _ = cancel.changed() => {},
            }
            done.send_replace(true);
        });
        // Drop aborts the worker if the login task is canceled or login fails.
        let entry = Arc::new(BrowserSession {
            controller,
            expires,
            revoked,
            worker,
        });
        let snapshot = timeout(Duration::from_secs(36), async {
            loop {
                let state = events.recv().await.map_err(|_| SessionError::Unavailable)?;
                match state.login {
                    Some(LoginState::Ready) => return Ok(state),
                    Some(LoginState::Failed) => {
                        return Err(SessionError::Login(
                            state.read_error.unwrap_or(ProviderError::Transport),
                        ));
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| SessionError::Unavailable)??;
        let mut random = [0u8; 32];
        getrandom::getrandom(&mut random).map_err(|_| SessionError::Unavailable)?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        self.inner
            .lock()
            .unwrap()
            .entries
            .insert(token.clone(), entry);
        Ok((token, snapshot))
    }
}

/// HttpOnly keeps the opaque session identifier out of frontend JavaScript.
/// Hosted mode always uses Secure; loopback development can use HTTP.
pub fn session_cookie(token: &str, secure: bool) -> String {
    let suffix = if secure { "; Secure" } else { "" };
    format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_SECONDS}{suffix}"
    )
}
pub fn clear_cookie(secure: bool) -> String {
    let suffix = if secure { "; Secure" } else { "" };
    format!("smarttherm_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{suffix}")
}
pub fn cookie_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let mut found = None;
    for header in headers.get_all(axum::http::header::COOKIE) {
        for part in header.to_str().ok()?.split(';') {
            let Some((name, value)) = part.trim().split_once('=') else {
                continue;
            };
            if name != COOKIE_NAME {
                continue;
            }
            if found.is_some() || value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            found = Some(value.to_owned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{DeviceState, TargetTemperature},
        provider::ProviderFuture,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct PendingProvider(Arc<AtomicUsize>);
    impl Drop for PendingProvider {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl Provider for PendingProvider {
        fn login(&self, _: String, _: String) -> ProviderFuture<'_, DeviceState> {
            Box::pin(std::future::pending())
        }
        fn read(&self) -> ProviderFuture<'_, DeviceState> {
            panic!("no reads before login")
        }
        fn send_target(&self, _: u64, _: TargetTemperature) -> ProviderFuture<'_, ()> {
            panic!("read-only")
        }
    }
    fn pending_sessions(dropped: Arc<AtomicUsize>) -> Arc<Sessions> {
        Arc::new(Sessions::with_factory(Arc::new(move || {
            Ok(Arc::new(PendingProvider(dropped.clone())))
        })))
    }
    #[tokio::test(start_paused = true)]
    async fn canceling_login_drops_provider_without_detached_polling() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let sessions = pending_sessions(dropped.clone());
        let task_sessions = sessions.clone();
        let task = tokio::spawn(async move {
            task_sessions
                .login("test@example.invalid".into(), "TEST_PASSWORD".into())
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        task.abort();
        let _ = task.await;
        tokio::task::yield_now().await;
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(sessions.inner.lock().unwrap().entries.is_empty());
    }
    #[tokio::test(start_paused = true)]
    async fn login_timeout_drops_provider_without_issuing_cookie() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let sessions = pending_sessions(dropped.clone());
        assert!(
            sessions
                .login("test@example.invalid".into(), "TEST_PASSWORD".into())
                .await
                .is_err()
        );
        tokio::task::yield_now().await;
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(sessions.inner.lock().unwrap().entries.is_empty());
    }
    #[test]
    fn malformed_or_ambiguous_cookies_are_rejected() {
        let token = "a".repeat(64);
        let mut headers = axum::http::HeaderMap::new();
        for invalid in [
            "smarttherm_session=bad".to_owned(),
            format!("smarttherm_session={}; smarttherm_session={}", token, token),
            format!("smarttherm_session={}", "z".repeat(64)),
        ] {
            headers.insert("cookie", invalid.parse().unwrap());
            assert!(cookie_token(&headers).is_none());
        }
        headers.insert(
            "cookie",
            format!("other=value; smarttherm_session={token}")
                .parse()
                .unwrap(),
        );
        assert_eq!(cookie_token(&headers), Some(token.clone()));
        headers.append(
            "cookie",
            format!("smarttherm_session={token}").parse().unwrap(),
        );
        assert!(cookie_token(&headers).is_none());
    }
}
