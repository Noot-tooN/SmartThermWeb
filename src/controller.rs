use crate::{
    domain::*,
    provider::{Provider, ProviderError},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Notify, broadcast, oneshot},
    time::{Instant, sleep, timeout},
};

use crate::provider::ocstat::trial::{Action as TrialAction, Status as TrialStatus};
enum ScheduleJob {
    Read(ScheduleReply),
    Save(
        crate::schedule::SaveRequest,
        oneshot::Sender<Result<crate::schedule::SaveResult, ProviderError>>,
    ),
    Trial(
        TrialAction,
        oneshot::Sender<Result<TrialStatus, ProviderError>>,
    ),
}
impl ScheduleJob {
    fn reject(self) {
        match self {
            Self::Read(reply) => {
                let _ = reply.send(Err(ProviderError::LoginRequired));
            }
            Self::Save(_, reply) => {
                let _ = reply.send(Err(ProviderError::LoginRequired));
            }
            Self::Trial(_, reply) => {
                let _ = reply.send(Err(ProviderError::LoginRequired));
            }
        }
    }
}
type ScheduleReply = oneshot::Sender<Result<crate::schedule::Preview, ProviderError>>;

struct Core {
    state: Snapshot,
    next_id: u64,
    writes_allowed: bool,
    pending: Option<Command>,
    clients: HashMap<String, u64>,
    pending_login: Option<(String, String)>,
    pending_logout: bool,
    pending_schedule: Option<ScheduleJob>,
    schedule_loading: bool,
    next_login: Instant,
}
#[derive(Debug, PartialEq, Eq)]
pub enum SubmitError {
    StaleRevision,
    TooManyClients,
    ReadOnly,
    LoginUnavailable,
    LoginRateLimited,
    LogoutUnavailable,
}
#[derive(Clone)]
pub struct Controller {
    core: Arc<Mutex<Core>>,
    events: broadcast::Sender<Snapshot>,
    wake: Arc<Notify>,
}
#[derive(Clone, Copy)]
pub struct Timing {
    pub request: Duration,
    pub confirmation: Duration,
    pub poll: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            request: Duration::from_secs(3),
            confirmation: Duration::from_secs(6),
            poll: Duration::from_millis(500),
        }
    }
}
impl Controller {
    pub fn new() -> Self {
        Self::configured("simulator", true)
    }
    pub fn ocstat_read_only() -> Self {
        let controller = Self::configured("ocstat", false);
        controller.core.lock().unwrap().state.login = Some(LoginState::Required);
        controller
    }
    pub fn ocstat() -> Self {
        let controller = Self::ocstat_read_only();
        controller.core.lock().unwrap().writes_allowed = true;
        controller
    }
    fn configured(provider: &str, can_set_target: bool) -> Self {
        let (events, _) = broadcast::channel(64);
        let session = format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            std::process::id()
        );
        Self {
            core: Arc::new(Mutex::new(Core {
                state: Snapshot {
                    account_email: None,
                    session,
                    revision: 0,
                    provider: provider.into(),
                    can_set_target,
                    login: None,
                    read_error: None,
                    connectivity: Connectivity::Connecting,
                    last_sync: None,
                    confirmed: None,
                    desired: None,
                    command: None,
                    in_flight: None,
                },
                next_id: 0,
                writes_allowed: can_set_target,
                pending: None,
                clients: HashMap::new(),
                pending_login: None,
                pending_logout: false,
                pending_schedule: None,
                schedule_loading: false,
                next_login: Instant::now(),
            })),
            events,
            wake: Arc::new(Notify::new()),
        }
    }
    pub async fn read_schedule(&self) -> Result<crate::schedule::Preview, ProviderError> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut core = self.core.lock().unwrap();
            if core.state.login != Some(LoginState::Ready) || core.pending_logout {
                return Err(ProviderError::LoginRequired);
            }
            if core.schedule_loading {
                return Err(ProviderError::Transport);
            }
            core.schedule_loading = true;
            core.pending_schedule = Some(ScheduleJob::Read(sender));
        }
        self.wake.notify_one();
        receiver.await.unwrap_or(Err(ProviderError::LoginRequired))
    }
    pub async fn schedule_trial(&self, action: TrialAction) -> Result<TrialStatus, ProviderError> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut core = self.core.lock().unwrap();
            if core.state.login != Some(LoginState::Ready) || core.pending_logout {
                return Err(ProviderError::LoginRequired);
            }
            if core.schedule_loading {
                return Err(ProviderError::Transport);
            }
            core.schedule_loading = true;
            core.pending_schedule = Some(ScheduleJob::Trial(action, sender));
        }
        self.wake.notify_one();
        receiver.await.unwrap_or(Err(ProviderError::LoginRequired))
    }
    pub async fn save_schedule(
        &self,
        request: crate::schedule::SaveRequest,
    ) -> Result<crate::schedule::SaveResult, ProviderError> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut core = self.core.lock().unwrap();
            if core.state.login != Some(LoginState::Ready) || core.pending_logout {
                return Err(ProviderError::LoginRequired);
            }
            if !core.writes_allowed {
                return Err(ProviderError::ReadOnly);
            }
            if core.schedule_loading {
                return Err(ProviderError::Transport);
            }
            core.schedule_loading = true;
            core.pending_schedule = Some(ScheduleJob::Save(request, sender));
        }
        self.wake.notify_one();
        receiver.await.unwrap_or(Err(ProviderError::LoginRequired))
    }
    fn take_schedule(&self) -> Option<ScheduleJob> {
        self.core.lock().unwrap().pending_schedule.take()
    }
    fn finish_schedule<T>(
        &self,
        reply: oneshot::Sender<Result<T, ProviderError>>,
        result: Result<T, ProviderError>,
    ) {
        let mut core = self.core.lock().unwrap();
        core.schedule_loading = false;
        let result = if core.pending_logout || core.state.login != Some(LoginState::Ready) {
            Err(ProviderError::LoginRequired)
        } else {
            result
        };
        let _ = reply.send(result);
    }
    async fn execute_schedule(&self, provider: &dyn Provider, job: ScheduleJob, timing: Timing) {
        match job {
            ScheduleJob::Read(reply) => {
                let result = if reply.is_closed() {
                    Err(ProviderError::Transport)
                } else {
                    timeout(timing.request, provider.read_schedule_preview())
                        .await
                        .unwrap_or(Err(ProviderError::Transport))
                };
                self.finish_schedule(reply, result);
            }
            ScheduleJob::Save(request, reply) => {
                let result = if reply.is_closed() {
                    Err(ProviderError::Transport)
                } else {
                    timeout(Duration::from_secs(360), provider.save_schedule(request))
                        .await
                        .unwrap_or(Err(ProviderError::Transport))
                };
                self.finish_schedule(reply, result);
            }
            ScheduleJob::Trial(action, reply) => {
                let result = if reply.is_closed() {
                    Err(ProviderError::Transport)
                } else {
                    timeout(Duration::from_secs(90), provider.schedule_trial(action))
                        .await
                        .unwrap_or(Err(ProviderError::Transport))
                };
                self.finish_schedule(reply, result);
            }
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.core.lock().unwrap().state.clone()
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Snapshot> {
        self.events.subscribe()
    }
    fn publish(&self, core: &mut Core) {
        core.state.revision += 1;
        let _ = self.events.send(core.state.clone());
    }
    pub fn submit_logout(&self) -> Result<Snapshot, SubmitError> {
        let mut core = self.core.lock().unwrap();
        if core.state.provider != "ocstat" {
            return Err(SubmitError::LogoutUnavailable);
        }
        if core.pending_logout {
            return Ok(core.state.clone());
        }
        core.pending_logout = true;
        core.pending_login = None;
        if let Some(job) = core.pending_schedule.take() {
            job.reject();
        }
        core.schedule_loading = false;
        core.pending = None;
        core.state.login = Some(LoginState::SigningOut);
        core.state.can_set_target = false;
        core.state.confirmed = None;
        core.state.account_email = None;
        core.state.desired = None;
        core.state.command = None;
        core.state.in_flight = None;
        core.state.last_sync = None;
        core.state.read_error = None;
        core.state.connectivity = Connectivity::Connecting;
        self.publish(&mut core);
        self.wake.notify_one();
        Ok(core.state.clone())
    }
    fn logout_requested(&self) -> bool {
        self.core.lock().unwrap().pending_logout
    }
    async fn execute_logout(&self, provider: &dyn Provider) {
        provider.logout().await;
        let mut core = self.core.lock().unwrap();
        core.pending_logout = false;
        core.state.login = Some(LoginState::Required);
        // Keep any outstanding login cooldown; logout must not bypass rate limits.
        self.publish(&mut core);
    }
    pub fn submit_login(&self, email: String, password: String) -> Result<Snapshot, SubmitError> {
        let mut core = self.core.lock().unwrap();
        if !matches!(
            core.state.login,
            Some(LoginState::Required | LoginState::Failed)
        ) {
            return Err(SubmitError::LoginUnavailable);
        }
        if Instant::now() < core.next_login {
            return Err(SubmitError::LoginRateLimited);
        }
        core.next_login = Instant::now() + Duration::from_secs(10);
        core.pending_login = Some((email, password));
        core.state.login = Some(LoginState::SigningIn);
        core.state.read_error = None;
        core.state.connectivity = Connectivity::Connecting;
        self.publish(&mut core);
        self.wake.notify_one();
        Ok(core.state.clone())
    }
    fn take_login(&self) -> Option<(String, String)> {
        self.core.lock().unwrap().pending_login.take()
    }
    async fn execute_login(
        &self,
        provider: &dyn Provider,
        email: String,
        password: String,
        timing: Timing,
    ) {
        let account_email = email.clone();
        match timeout(timing.request, provider.login(email, password)).await {
            Ok(Ok(observed)) => {
                {
                    let mut core = self.core.lock().unwrap();
                    if core.pending_logout {
                        return;
                    }
                    core.state.login = Some(LoginState::Ready);
                    core.state.account_email = Some(account_email);
                }
                self.observe(observed);
            }
            result => {
                {
                    let mut core = self.core.lock().unwrap();
                    if core.pending_logout {
                        return;
                    }
                    core.state.login = Some(LoginState::Failed);
                }
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => ProviderError::Transport,
                };
                self.read_failed(error);
            }
        }
    }
    pub fn submit(&self, target: TargetTemperature) -> Result<Snapshot, SubmitError> {
        let mut core = self.core.lock().unwrap();
        if !core.state.can_set_target {
            return Err(SubmitError::ReadOnly);
        }
        Ok(self.enqueue(&mut core, target))
    }
    pub fn submit_from(
        &self,
        target: TargetTemperature,
        client: String,
        revision: u64,
    ) -> Result<Snapshot, SubmitError> {
        let mut core = self.core.lock().unwrap();
        if !core.state.can_set_target {
            return Err(SubmitError::ReadOnly);
        }
        if core
            .clients
            .get(&client)
            .is_some_and(|last| *last >= revision)
        {
            return Err(SubmitError::StaleRevision);
        }
        if !core.clients.contains_key(&client) && core.clients.len() >= 256 {
            return Err(SubmitError::TooManyClients);
        }
        core.clients.insert(client, revision);
        Ok(self.enqueue(&mut core, target))
    }
    fn enqueue(&self, core: &mut Core, target: TargetTemperature) -> Snapshot {
        core.next_id += 1;
        let command = Command {
            id: core.next_id,
            target,
            phase: Phase::Queued,
            server_accepted: false,
            message: "Queued; waiting to send.".into(),
        };
        core.state.desired = Some(target);
        core.state.command = Some(command.clone());
        // One pending slot: replaces obsolete work without disturbing the active request.
        core.pending = Some(command);
        self.publish(core);
        self.wake.notify_one();
        core.state.clone()
    }
    fn take_pending(&self) -> Option<Command> {
        let mut core = self.core.lock().unwrap();
        let mut command = core.pending.take()?;
        command.transition(
            Phase::Sending,
            "Sending request; device state is not yet confirmed.",
        );
        if core
            .state
            .command
            .as_ref()
            .is_some_and(|c| c.id == command.id)
        {
            core.state.command = Some(command.clone());
        }
        core.state.in_flight = Some(command.clone());
        self.publish(&mut core);
        Some(command)
    }
    fn phase(&self, id: u64, phase: Phase, message: &str) {
        let mut core = self.core.lock().unwrap();
        let mut changed = false;
        if let Some(command) = core.state.command.as_mut().filter(|c| c.id == id) {
            changed |= command.transition(phase, message);
        }
        if let Some(command) = core.state.in_flight.as_mut().filter(|c| c.id == id) {
            changed |= command.transition(phase, message);
        }
        if changed {
            self.publish(&mut core);
        }
    }
    fn finish(&self, id: u64) {
        let mut core = self.core.lock().unwrap();
        if core.state.in_flight.as_ref().is_some_and(|c| c.id == id) {
            core.state.in_flight = None;
            self.publish(&mut core);
        }
    }
    fn observe(&self, observed: DeviceState) {
        let mut core = self.core.lock().unwrap();
        if core.pending_logout {
            return;
        }
        if core
            .state
            .confirmed
            .as_ref()
            .is_some_and(|old| old.revision > observed.revision)
        {
            return;
        }
        let fresh = core
            .state
            .confirmed
            .as_ref()
            .is_none_or(|old| old.revision < observed.revision);
        core.state.connectivity = if observed.online {
            Connectivity::Online
        } else {
            Connectivity::Offline
        };
        core.state.read_error = None;
        if core.state.provider == "ocstat" {
            core.state.can_set_target = core.writes_allowed
                && core.state.login == Some(LoginState::Ready)
                && observed.target_writable
                && observed.online;
            // Follow changes made in the Android app only when no local intent is outstanding.
            if core.writes_allowed
                && core.pending.is_none()
                && core.state.in_flight.is_none()
                && core
                    .state
                    .command
                    .as_ref()
                    .is_none_or(|c| matches!(c.phase, Phase::Confirmed | Phase::TargetMatched))
            {
                let current = observed
                    .requested_temperature
                    .unwrap_or(observed.target_temperature)
                    .0
                    .try_into()
                    .ok();
                if core.state.desired != current {
                    core.state.command = None;
                }
                core.state.desired = current;
            }
        }
        core.state.last_sync = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        // Initial synchronization only; subsequent reads must never replace user intent.
        if core.state.can_set_target && core.state.desired.is_none() {
            core.state.desired = observed.target_temperature.0.try_into().ok();
        }
        if fresh {
            if let Some(command) = core.state.command.as_mut() {
                reconcile(command, &observed);
            }
            if let Some(command) = core.state.in_flight.as_mut() {
                reconcile(command, &observed);
            }
            core.state.confirmed = Some(observed);
        }
        self.publish(&mut core);
    }
    fn read_failed(&self, error: ProviderError) {
        let mut core = self.core.lock().unwrap();
        if core.pending_logout {
            return;
        }
        if matches!(
            error,
            ProviderError::Authentication | ProviderError::LoginRequired
        ) && core.state.login == Some(LoginState::Ready)
        {
            core.state.login = Some(LoginState::Failed);
            core.state.confirmed = None;
            core.state.account_email = None;
            core.state.last_sync = None;
            core.pending = None;
            core.state.desired = None;
            core.state.command = None;
            core.state.in_flight = None;
        }
        if core.state.provider == "ocstat" {
            core.state.can_set_target = false;
        }
        let connectivity = if error == ProviderError::Offline {
            Connectivity::Offline
        } else {
            Connectivity::Unavailable
        };
        if core.state.connectivity != connectivity || core.state.read_error != Some(error) {
            core.state.connectivity = connectivity;
            core.state.read_error = Some(error);
            self.publish(&mut core);
        }
    }
    async fn sync(&self, provider: &dyn Provider, timing: Timing) {
        if matches!(
            self.snapshot().login,
            Some(
                LoginState::Required
                    | LoginState::SigningIn
                    | LoginState::SigningOut
                    | LoginState::Failed
            )
        ) {
            return;
        }
        match timeout(timing.request, provider.read()).await {
            Ok(Ok(state)) => self.observe(state),
            Ok(Err(error)) => self.read_failed(error),
            Err(_) => self.read_failed(ProviderError::Transport),
        }
    }
    /// Exactly one worker owns outbound writes and reads, avoiding read/write races.
    pub async fn run(self, provider: Arc<dyn Provider>, timing: Timing) {
        self.sync(provider.as_ref(), timing).await;
        loop {
            if self.logout_requested() {
                self.execute_logout(provider.as_ref()).await;
            } else if let Some((email, password)) = self.take_login() {
                self.execute_login(provider.as_ref(), email, password, timing)
                    .await;
            } else if let Some(command) = self.take_pending() {
                self.execute(provider.as_ref(), command, timing).await;
            } else if let Some(reply) = self.take_schedule() {
                self.execute_schedule(provider.as_ref(), reply, timing)
                    .await;
            } else {
                tokio::select! {
                    _ = self.wake.notified() => {},
                    _ = sleep(timing.poll) => self.sync(provider.as_ref(), timing).await,
                }
            }
        }
    }
    fn observation_should_yield(&self) -> bool {
        let core = self.core.lock().unwrap();
        core.pending_logout || core.pending.is_some() || core.pending_schedule.is_some()
    }
    async fn execute(&self, provider: &dyn Provider, command: Command, timing: Timing) {
        let id = command.id;
        match timeout(timing.request, provider.send_target(id, command.target)).await {
            Ok(Ok(())) => {
                self.phase(
                    id,
                    Phase::AcceptedByCloud,
                    "Server acknowledged the request. Device application is still unverified.",
                );
                self.phase(
                    id,
                    Phase::AwaitingDevice,
                    "Server acknowledged; waiting for a device observation.",
                );
            }
            Ok(Err(
                ProviderError::Rejected
                | ProviderError::UnverifiedProtocol
                | ProviderError::ReadOnly,
            )) => {
                self.phase(
                    id,
                    Phase::Rejected,
                    "Request rejected. The desired target is retained.",
                );
                self.finish(id);
                return;
            }
            Ok(Err(_)) => self.phase(
                id,
                Phase::TransportFailed,
                "No reliable server reply. The command may have applied; checking device state.",
            ),
            Err(_) => self.phase(
                id,
                Phase::TimedOut,
                "Server reply timed out. Outcome is uncertain; checking device state.",
            ),
        }
        let deadline = Instant::now() + timing.confirmation;
        loop {
            // Finish the active write before yielding. A newer target supersedes
            // only observation of the old target, never its in-flight exchange.
            if self.observation_should_yield() {
                break;
            }
            self.sync(provider, timing).await;
            // A request may have arrived while the read was in progress.
            if self.observation_should_yield() {
                break;
            }
            let confirmed = self.snapshot().in_flight.is_some_and(|c| {
                c.id == id && matches!(c.phase, Phase::Confirmed | Phase::TargetMatched)
            });
            if confirmed {
                break;
            }
            if Instant::now() >= deadline {
                self.phase(id, Phase::Unknown, "Device application is unverified. Automatic reconciliation continues; refresh or retry when connected.");
                break;
            }
            let interval = if self.snapshot().provider == "ocstat" {
                Duration::from_secs(2)
            } else {
                timing.poll
            };
            let poll = sleep(interval);
            tokio::pin!(poll);
            loop {
                tokio::select! {
                    _ = &mut poll => break,
                    _ = self.wake.notified() => {
                        if self.observation_should_yield() {
                            break;
                        }
                        // An enqueue notification can predate the active write.
                        // Consuming it must not shorten the next polling interval.
                    },
                }
            }
        }
        self.finish(id);
    }
}
impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}
fn reconcile(command: &mut Command, observed: &DeviceState) {
    if observed.online
        && observed.applied_command == Some(command.id)
        && observed.target_temperature.0 == i16::from(command.target)
    {
        command.transition(
            Phase::Confirmed,
            "Device confirmed this target temperature.",
        );
    } else if observed.online
        && observed.matched_command == Some(command.id)
        && observed.requested_temperature == Some(Temperature(command.target.into()))
        && observed.target_temperature.0 == i16::from(command.target)
    {
        command.transition(Phase::TargetMatched, "The requested target and cloud status match on two consecutive reads. This is a cloud observation, not a command acknowledgement from the device.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::simulator::{Config, Scenario, Simulator};
    fn target(value: i16) -> TargetTemperature {
        value.try_into().unwrap()
    }
    fn observation(revision: u64, id: Option<u64>, target_value: i16) -> DeviceState {
        DeviceState {
            device_name: None,
            revision,
            applied_command: id,
            matched_command: None,
            target_writable: true,
            target_temperature: Temperature(target_value),
            requested_temperature: None,
            online: true,
            room_temperature: Temperature(208),
            mode: Mode::Manual,
        }
    }
    struct ScheduleAccount {
        started: Notify,
        release: Notify,
    }
    impl Provider for ScheduleAccount {
        fn read(&self) -> crate::provider::ProviderFuture<'_, DeviceState> {
            Box::pin(async { Ok(observation(1, None, 215)) })
        }
        fn read_schedule(&self) -> crate::provider::ProviderFuture<'_, crate::schedule::Schedule> {
            Box::pin(async {
                self.started.notify_one();
                self.release.notified().await;
                Ok(crate::schedule::Schedule {
                    pattern: crate::schedule::Pattern::Daily,
                    groups: vec![crate::schedule::DayGroup {
                        group: crate::schedule::Group::EveryDay,
                        changes: vec![crate::schedule::Change {
                            start_minute: 0,
                            target_temperature: target(185),
                        }],
                    }],
                })
            })
        }
        fn send_target(
            &self,
            _: u64,
            _: TargetTemperature,
        ) -> crate::provider::ProviderFuture<'_, ()> {
            Box::pin(async { panic!("schedule read must not write") })
        }
    }
    #[tokio::test(start_paused = true)]
    async fn schedule_reads_are_bounded_and_logout_rejects_late_results() {
        let c = Controller::ocstat();
        c.core.lock().unwrap().state.login = Some(LoginState::Ready);
        let provider = Arc::new(ScheduleAccount {
            started: Notify::new(),
            release: Notify::new(),
        });
        let worker = tokio::spawn(c.clone().run(
            provider.clone(),
            Timing {
                request: Duration::from_secs(20),
                poll: Duration::from_secs(30),
                confirmation: Duration::from_secs(1),
            },
        ));
        let copy = c.clone();
        let reader = tokio::spawn(async move { copy.read_schedule().await });
        provider.started.notified().await;
        assert_eq!(c.read_schedule().await, Err(ProviderError::Transport));
        c.submit_logout().unwrap();
        provider.release.notify_one();
        assert_eq!(reader.await.unwrap(), Err(ProviderError::LoginRequired));
        worker.abort();
    }
    #[tokio::test(start_paused = true)]
    async fn schedule_read_timeout_releases_the_queue() {
        let c = Controller::ocstat();
        c.core.lock().unwrap().state.login = Some(LoginState::Ready);
        let provider = Arc::new(ScheduleAccount {
            started: Notify::new(),
            release: Notify::new(),
        });
        let worker = tokio::spawn(c.clone().run(
            provider.clone(),
            Timing {
                request: Duration::from_secs(2),
                poll: Duration::from_secs(30),
                confirmation: Duration::from_secs(1),
            },
        ));
        assert_eq!(c.read_schedule().await, Err(ProviderError::Transport));
        assert!(!c.core.lock().unwrap().schedule_loading);
        worker.abort();
    }
    struct SlowAccount {
        started: Notify,
        release: Notify,
        reads: std::sync::atomic::AtomicUsize,
        cleared: std::sync::atomic::AtomicBool,
    }
    impl Provider for SlowAccount {
        fn login(&self, _: String, _: String) -> crate::provider::ProviderFuture<'_, DeviceState> {
            self.read()
        }
        fn read(&self) -> crate::provider::ProviderFuture<'_, DeviceState> {
            Box::pin(async {
                self.reads
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.started.notify_one();
                self.release.notified().await;
                Ok(observation(1, None, 215))
            })
        }
        fn logout(&self) -> crate::provider::LogoutFuture<'_> {
            Box::pin(async {
                self.cleared
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            })
        }
        fn send_target(
            &self,
            _: u64,
            _: TargetTemperature,
        ) -> crate::provider::ProviderFuture<'_, ()> {
            panic!("logout must not write")
        }
    }
    #[tokio::test(start_paused = true)]
    async fn logout_discards_in_flight_reads_and_logins_and_stops_polling() {
        for browser in [false, true] {
            let c = Controller::ocstat_read_only();
            if !browser {
                c.core.lock().unwrap().state.login = Some(LoginState::Ready);
            }
            let provider = Arc::new(SlowAccount {
                started: Notify::new(),
                release: Notify::new(),
                reads: 0.into(),
                cleared: false.into(),
            });
            let mut events = c.subscribe();
            if browser {
                c.submit_login(
                    "example@example.invalid".into(),
                    "TEST_PASSWORD_ONLY".into(),
                )
                .unwrap();
            }
            let worker = tokio::spawn(c.clone().run(provider.clone(), Timing::default()));
            provider.started.notified().await;
            let logged_out = c.submit_logout().unwrap();
            assert_eq!(logged_out.login, Some(LoginState::SigningOut));
            assert!(logged_out.confirmed.is_none());
            assert!(c.submit_login("x".into(), "x".into()).is_err());
            provider.release.notify_one();
            loop {
                let state = events.recv().await.unwrap();
                if state.revision < logged_out.revision {
                    continue;
                }
                assert!(state.confirmed.is_none() && state.last_sync.is_none());
                assert_ne!(state.login, Some(LoginState::Ready));
                if state.login == Some(LoginState::Required) {
                    break;
                }
            }
            assert!(provider.cleared.load(std::sync::atomic::Ordering::Relaxed));
            tokio::time::advance(Duration::from_secs(30)).await;
            tokio::task::yield_now().await;
            assert_eq!(provider.reads.load(std::sync::atomic::Ordering::Relaxed), 1);
            assert!(
                c.submit_login(
                    "example@example.invalid".into(),
                    "TEST_PASSWORD_ONLY".into()
                )
                .is_ok()
            );
            worker.abort();
        }
    }
    #[tokio::test(start_paused = true)]
    async fn logout_drops_pending_credentials_without_bypassing_login_cooldown() {
        let c = Controller::ocstat_read_only();
        c.submit_login(
            "example@example.invalid".into(),
            "TEST_PASSWORD_ONLY".into(),
        )
        .unwrap();
        c.submit_logout().unwrap();
        assert!(c.take_login().is_none());
        c.execute_logout(&Simulator::new(Config::default())).await;
        assert!(matches!(
            c.submit_login("x".into(), "x".into()),
            Err(SubmitError::LoginRateLimited)
        ));
        assert!(Controller::new().submit_logout().is_err());
    }
    struct LoginProvider {
        succeed: bool,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Provider for LoginProvider {
        fn login(
            &self,
            _email: String,
            _password: String,
        ) -> crate::provider::ProviderFuture<'_, DeviceState> {
            self.calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(async move {
                if self.succeed {
                    Ok(observation(1, None, 215))
                } else {
                    Err(ProviderError::Authentication)
                }
            })
        }
        fn read(&self) -> crate::provider::ProviderFuture<'_, DeviceState> {
            Box::pin(async { Ok(observation(2, None, 215)) })
        }
        fn send_target(
            &self,
            _: u64,
            _: TargetTemperature,
        ) -> crate::provider::ProviderFuture<'_, ()> {
            panic!("browser login must not write")
        }
    }
    #[tokio::test(start_paused = true)]
    async fn only_worker_performs_queued_login_and_publishes_safe_read_only_state() {
        let c = Controller::ocstat_read_only();
        let provider = Arc::new(LoginProvider {
            succeed: true,
            calls: 0.into(),
        });
        let mut events = c.subscribe();
        let queued = c
            .submit_login(
                "example@example.invalid".into(),
                "TEST_PASSWORD_ONLY".into(),
            )
            .unwrap();
        assert_eq!(queued.login, Some(LoginState::SigningIn));
        let json = serde_json::to_string(&queued).unwrap();
        assert!(!json.contains("TEST_PASSWORD_ONLY") && !json.contains("example@example.invalid"));
        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        let worker = tokio::spawn(c.clone().run(provider.clone(), Timing::default()));
        while events.recv().await.unwrap().login != Some(LoginState::Ready) {}
        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(!c.snapshot().can_set_target);
        assert!(c.snapshot().confirmed.is_some());
        assert_eq!(
            c.snapshot().account_email.as_deref(),
            Some("example@example.invalid")
        );
        assert!(c.submit_login("other".into(), "other".into()).is_err());
        assert!(c.submit(target(220)).is_err());
        c.read_failed(ProviderError::Authentication);
        assert!(c.snapshot().account_email.is_none());
        assert!(c.snapshot().confirmed.is_none());
        worker.abort();
    }
    #[tokio::test(start_paused = true)]
    async fn failed_browser_login_can_be_retried_only_after_cooldown() {
        let c = Controller::ocstat_read_only();
        let provider = LoginProvider {
            succeed: false,
            calls: 0.into(),
        };
        c.submit_login(
            "example@example.invalid".into(),
            "TEST_PASSWORD_ONLY".into(),
        )
        .unwrap();
        let (email, password) = c.take_login().unwrap();
        c.execute_login(&provider, email, password, Timing::default())
            .await;
        assert_eq!(c.snapshot().login, Some(LoginState::Failed));
        assert_eq!(c.snapshot().read_error, Some(ProviderError::Authentication));
        assert!(matches!(
            c.submit_login("x".into(), "x".into()),
            Err(SubmitError::LoginRateLimited)
        ));
        c.sync(&provider, Timing::default()).await;
        assert_eq!(c.snapshot().login, Some(LoginState::Failed));
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(c.submit_login("x".into(), "x".into()).is_ok());
    }
    #[test]
    fn read_only_observations_follow_cloud_and_preserve_last_read_on_errors() {
        let c = Controller::ocstat_read_only();
        c.observe(observation(1, None, 215));
        c.observe(observation(2, None, 230));
        assert!(c.snapshot().desired.is_none());
        assert_eq!(
            c.snapshot().confirmed.as_ref().unwrap().target_temperature,
            Temperature(230)
        );
        let last_sync = c.snapshot().last_sync;
        c.read_failed(ProviderError::Transport);
        assert_eq!(c.snapshot().connectivity, Connectivity::Unavailable);
        assert_eq!(c.snapshot().last_sync, last_sync);
        assert_eq!(
            c.snapshot().confirmed.as_ref().unwrap().target_temperature,
            Temperature(230)
        );
        let mut offline = observation(3, None, 225);
        offline.online = false;
        c.observe(offline);
        assert_eq!(c.snapshot().connectivity, Connectivity::Offline);
        assert_eq!(c.snapshot().read_error, None);
        c.observe(observation(4, None, 220));
        assert_eq!(c.snapshot().connectivity, Connectivity::Online);
        assert!(c.snapshot().command.is_none());
    }
    #[test]
    fn pending_commands_coalesce_and_stale_ack_cannot_change_intent() {
        let c = Controller::new();
        c.submit(target(220)).unwrap();
        let first = c.take_pending().unwrap();
        c.submit(target(225)).unwrap();
        c.submit(target(230)).unwrap();
        c.phase(first.id, Phase::AcceptedByCloud, "old acknowledgement");
        let latest = c.snapshot().command.unwrap();
        assert_eq!(latest.phase, Phase::Queued);
        assert_eq!(latest.target, target(230));
        assert_eq!(c.take_pending().unwrap().target, target(230));
        assert!(c.take_pending().is_none());
    }
    #[test]
    fn out_of_order_observations_and_false_matching_values_are_ignored() {
        let c = Controller::new();
        c.observe(observation(5, None, 215));
        c.submit(target(230)).unwrap();
        let command = c.take_pending().unwrap();
        c.phase(command.id, Phase::AcceptedByCloud, "ack");
        c.phase(command.id, Phase::AwaitingDevice, "wait");
        c.observe(observation(4, Some(command.id), 230));
        assert_eq!(c.snapshot().confirmed.unwrap().revision, 5);
        c.observe(observation(6, Some(999), 230));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::AwaitingDevice);
        c.observe(observation(7, Some(command.id), 230));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::Confirmed);
        c.observe(observation(6, None, 215));
        assert_eq!(c.snapshot().desired, Some(target(230)));
        assert_eq!(c.snapshot().confirmed.unwrap().revision, 7);
    }
    #[test]
    fn cloud_match_can_later_gain_verified_application_evidence() {
        let c = Controller::new();
        c.observe(observation(1, None, 215));
        c.submit(target(225)).unwrap();
        let command = c.take_pending().unwrap();
        c.phase(command.id, Phase::AcceptedByCloud, "ack");
        c.phase(command.id, Phase::AwaitingDevice, "wait");
        let mut matched = observation(2, None, 225);
        matched.matched_command = Some(command.id);
        matched.requested_temperature = Some(Temperature(225));
        c.observe(matched);
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::TargetMatched);
        // A duplicate observation cannot introduce evidence; nor can another command.
        c.observe(observation(2, Some(command.id), 225));
        c.observe(observation(3, Some(command.id + 1), 225));
        let mut offline = observation(4, Some(command.id), 225);
        offline.online = false;
        c.observe(offline);
        c.observe(observation(5, Some(command.id), 215));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::TargetMatched);
        c.observe(observation(6, Some(command.id), 225));
        let final_command = c.snapshot().command.unwrap();
        assert_eq!(final_command.phase, Phase::Confirmed);
        assert!(final_command.server_accepted);
    }

    #[test]
    fn repeated_target_requires_new_application_evidence() {
        let c = Controller::new();
        c.submit(target(220)).unwrap();
        let first = c.take_pending().unwrap();
        c.phase(first.id, Phase::AcceptedByCloud, "ack");
        c.phase(first.id, Phase::AwaitingDevice, "wait");
        c.observe(observation(2, Some(first.id), 220));
        c.finish(first.id);
        c.submit(target(220)).unwrap();
        let second = c.take_pending().unwrap();
        c.phase(second.id, Phase::AcceptedByCloud, "ack");
        c.phase(second.id, Phase::AwaitingDevice, "wait");
        c.observe(observation(2, Some(first.id), 220));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::AwaitingDevice);
        c.observe(observation(3, Some(second.id), 220));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::Confirmed);
    }

    #[test]
    fn requested_equality_offline_and_duplicate_reads_do_not_confirm() {
        let c = Controller::new();
        c.observe(observation(1, None, 215));
        c.submit(target(225)).unwrap();
        let command = c.take_pending().unwrap();
        c.phase(command.id, Phase::AcceptedByCloud, "accepted");
        c.phase(command.id, Phase::AwaitingDevice, "waiting");
        let mut lagging = observation(2, None, 215);
        lagging.requested_temperature = Some(Temperature(225));
        c.observe(lagging);
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::AwaitingDevice);
        // Even a newer matching reported value has no OCSTAT command correlation.
        c.observe(observation(3, None, 225));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::AwaitingDevice);
        c.observe(observation(3, Some(command.id), 225));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::AwaitingDevice);
        let mut offline = observation(4, Some(command.id), 225);
        offline.online = false;
        c.observe(offline);
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::AwaitingDevice);
        c.observe(observation(5, Some(command.id), 225));
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::Confirmed);
    }

    #[test]
    fn reconnect_retains_last_confirmed_state_and_sync() {
        let c = Controller::new();
        c.observe(observation(1, None, 215));
        let before = c.snapshot();
        c.read_failed(ProviderError::Offline);
        assert_eq!(c.snapshot().connectivity, Connectivity::Offline);
        assert_eq!(c.snapshot().confirmed, before.confirmed);
        assert_eq!(c.snapshot().last_sync, before.last_sync);
        c.observe(observation(2, None, 220));
        assert_eq!(c.snapshot().connectivity, Connectivity::Online);
    }
    #[tokio::test(start_paused = true)]
    async fn worker_emits_lifecycle_and_serializes_latest_pending() {
        let c = Controller::new();
        let mut events = c.subscribe();
        let simulator = Arc::new(Simulator::new(Config {
            ack_ms: 100,
            apply_ms: 300,
            ..Config::default()
        }));
        let worker = tokio::spawn(c.clone().run(simulator, Timing::default()));
        c.submit(target(220)).unwrap();
        // Await the first sending event, then queue two requests while it is in flight.
        while events.recv().await.unwrap().in_flight.is_none() {}
        c.submit(target(225)).unwrap();
        c.submit(target(230)).unwrap();
        let mut sent = Vec::new();
        let mut saw_ack = false;
        loop {
            let state = events.recv().await.unwrap();
            if let Some(active) = state.in_flight {
                if active.phase == Phase::Sending && sent.last() != Some(&active.id) {
                    sent.push(active.id);
                }
                saw_ack |= active.phase == Phase::AcceptedByCloud;
            }
            if state
                .command
                .is_some_and(|c| c.id == 3 && c.phase == Phase::Confirmed)
            {
                break;
            }
        }
        assert!(saw_ack);
        assert!(!sent.contains(&2));
        assert_eq!(
            c.snapshot().confirmed.unwrap().target_temperature,
            Temperature(230)
        );
        worker.abort();
    }
    #[tokio::test(start_paused = true)]
    async fn newer_target_waits_for_write_but_not_obsolete_confirmation() {
        for scenario in [
            Scenario::Normal,
            Scenario::ReplyLost,
            Scenario::AckTimeout,
            Scenario::Rejected,
        ] {
            let c = Controller::new();
            let mut events = c.subscribe();
            let provider = Arc::new(Simulator::new(Config {
                scenario,
                ack_ms: 100,
                apply_ms: 60_000,
            }));
            let timing = Timing {
                request: Duration::from_millis(200),
                confirmation: Duration::from_secs(30),
                poll: Duration::from_secs(2),
            };
            let worker = tokio::spawn(c.clone().run(provider, timing));
            c.submit(target(220)).unwrap();
            while !events
                .recv()
                .await
                .unwrap()
                .in_flight
                .is_some_and(|c| c.phase == Phase::Sending)
            {}
            let started = Instant::now();
            c.submit(target(225)).unwrap();
            c.submit(target(230)).unwrap();
            tokio::time::advance(Duration::from_millis(99)).await;
            tokio::task::yield_now().await;
            assert_eq!(
                c.snapshot().in_flight.unwrap().id,
                1,
                "must not cancel an active write"
            );
            assert_eq!(c.snapshot().command.unwrap().phase, Phase::Queued);
            loop {
                let state = events.recv().await.unwrap();
                if let Some(active) = state.in_flight {
                    assert_ne!(
                        active.id, 2,
                        "intermediate pending target must be coalesced"
                    );
                    if active.id == 3 && active.phase == Phase::Sending {
                        break;
                    }
                }
            }
            let expected = if scenario == Scenario::AckTimeout {
                200
            } else {
                100
            };
            assert_eq!(started.elapsed(), Duration::from_millis(expected));
            assert_eq!(c.snapshot().desired, Some(target(230)));
            // Neither a superseded reply nor the switch to a newer request is
            // evidence of application, or acceptance of that newer request.
            assert!(!c.snapshot().command.unwrap().server_accepted);
            worker.abort();
            let _ = worker.await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn new_target_wakes_observation_sleep_without_shortening_normal_polling() {
        let c = Controller::new();
        let mut events = c.subscribe();
        let provider = Arc::new(Simulator::new(Config {
            ack_ms: 100,
            apply_ms: 60_000,
            ..Config::default()
        }));
        let timing = Timing {
            request: Duration::from_secs(3),
            confirmation: Duration::from_secs(30),
            poll: Duration::from_secs(2),
        };
        c.submit(target(220)).unwrap();
        let worker = tokio::spawn(c.clone().run(provider, timing));
        // A snapshot after acceptance is the first observation, then the worker sleeps.
        let mut awaiting_seen = false;
        let observed_revision = loop {
            let state = events.recv().await.unwrap();
            if state
                .command
                .as_ref()
                .is_some_and(|c| c.phase == Phase::AwaitingDevice)
            {
                if awaiting_seen {
                    break state.revision;
                }
                awaiting_seen = true;
            }
        };
        tokio::time::advance(Duration::from_millis(50)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            c.snapshot().revision,
            observed_revision,
            "stale wake must not trigger an extra read"
        );
        let queued = Instant::now();
        c.submit(target(225)).unwrap();
        loop {
            if events
                .recv()
                .await
                .unwrap()
                .in_flight
                .is_some_and(|c| c.id == 2 && c.phase == Phase::Sending)
            {
                break;
            }
        }
        assert_eq!(
            queued.elapsed(),
            Duration::ZERO,
            "must wake before the two-second poll timer"
        );
        worker.abort();
        let _ = worker.await;
    }

    #[tokio::test(start_paused = true)]
    async fn newer_target_waits_for_active_read_and_ignores_old_application_evidence() {
        struct SlowRead {
            reads: std::sync::atomic::AtomicUsize,
            started: Notify,
            release: Notify,
        }
        impl Provider for SlowRead {
            fn read(&self) -> crate::provider::ProviderFuture<'_, DeviceState> {
                Box::pin(async {
                    let read = self
                        .reads
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if read == 1 {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    Ok(observation(read as u64 + 1, (read > 0).then_some(1), 220))
                })
            }
            fn send_target(
                &self,
                id: u64,
                _: TargetTemperature,
            ) -> crate::provider::ProviderFuture<'_, ()> {
                Box::pin(async move {
                    if id > 1 {
                        // Keep the new write open so its lack of acceptance can be asserted.
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                })
            }
        }
        let c = Controller::new();
        let provider = Arc::new(SlowRead {
            reads: 0.into(),
            started: Notify::new(),
            release: Notify::new(),
        });
        let mut events = c.subscribe();
        c.submit(target(220)).unwrap();
        let worker = tokio::spawn(c.clone().run(provider.clone(), Timing::default()));
        provider.started.notified().await;
        // Same target, different request: old evidence cannot acknowledge this one.
        c.submit(target(220)).unwrap();
        tokio::task::yield_now().await;
        assert_eq!(c.snapshot().in_flight.unwrap().id, 1);
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::Queued);
        let released = Instant::now();
        provider.release.notify_one();
        loop {
            if events
                .recv()
                .await
                .unwrap()
                .in_flight
                .is_some_and(|c| c.id == 2 && c.phase == Phase::Sending)
            {
                break;
            }
        }
        assert_eq!(released.elapsed(), Duration::ZERO);
        assert!(!c.snapshot().command.unwrap().server_accepted);
        assert_eq!(c.snapshot().confirmed.unwrap().applied_command, Some(1));
        assert_eq!(provider.reads.load(std::sync::atomic::Ordering::Relaxed), 2);
        worker.abort();
        let _ = worker.await;
    }

    async fn exercise(scenario: Scenario) -> (Controller, Vec<Phase>) {
        let c = Controller::new();
        let provider = Simulator::new(Config {
            scenario,
            ack_ms: 10,
            apply_ms: 70,
        });
        c.submit(target(230)).unwrap();
        let command = c.take_pending().unwrap();
        let mut events = c.subscribe();
        c.execute(
            &provider,
            command,
            Timing {
                request: Duration::from_millis(40),
                confirmation: Duration::from_millis(120),
                poll: Duration::from_millis(10),
            },
        )
        .await;
        let mut phases = Vec::new();
        while let Ok(state) = events.try_recv() {
            phases.push(state.command.unwrap().phase);
        }
        (c, phases)
    }
    #[tokio::test(start_paused = true)]
    async fn reply_lost_and_ack_timeout_reconcile_without_false_failure() {
        for (scenario, phase) in [
            (Scenario::ReplyLost, Phase::TransportFailed),
            (Scenario::AckTimeout, Phase::TimedOut),
        ] {
            let (c, phases) = exercise(scenario).await;
            assert!(phases.contains(&phase));
            assert_eq!(c.snapshot().command.unwrap().phase, Phase::Confirmed);
        }
    }
    #[tokio::test(start_paused = true)]
    async fn rejection_transport_failure_device_timeout_and_offline() {
        for (scenario, phase) in [
            (Scenario::Rejected, Phase::Rejected),
            (Scenario::TransportFailure, Phase::TransportFailed),
            (Scenario::DeviceTimeout, Phase::Unknown),
            (Scenario::Offline, Phase::TransportFailed),
        ] {
            let (c, _) = exercise(scenario).await;
            assert_eq!(c.snapshot().command.unwrap().phase, phase);
            assert!(c.snapshot().in_flight.is_none());
            assert_eq!(c.snapshot().desired, Some(target(230)));
            if scenario == Scenario::Offline {
                assert_eq!(c.snapshot().connectivity, Connectivity::Offline);
            }
        }
    }
    #[tokio::test(start_paused = true)]
    async fn late_application_reconciles_after_deadline_and_reconnect() {
        let c = Controller::new();
        let provider = Simulator::new(Config {
            scenario: Scenario::Normal,
            ack_ms: 0,
            apply_ms: 200,
        });
        c.submit(target(230)).unwrap();
        c.execute(
            &provider,
            c.take_pending().unwrap(),
            Timing {
                request: Duration::from_millis(20),
                confirmation: Duration::from_millis(40),
                poll: Duration::from_millis(10),
            },
        )
        .await;
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::Unknown);
        provider.configure(Config {
            scenario: Scenario::Offline,
            ..provider.config()
        });
        c.sync(&provider, Timing::default()).await;
        tokio::time::advance(Duration::from_millis(300)).await;
        provider.configure(Config::default());
        c.sync(&provider, Timing::default()).await;
        assert_eq!(c.snapshot().command.unwrap().phase, Phase::Confirmed);
        assert_eq!(c.snapshot().connectivity, Connectivity::Online);
    }
}
