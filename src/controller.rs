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
    sync::{Notify, broadcast},
    time::{Instant, sleep, timeout},
};

struct Core {
    state: Snapshot,
    next_id: u64,
    pending: Option<Command>,
    clients: HashMap<String, u64>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum SubmitError {
    StaleRevision,
    TooManyClients,
    ReadOnly,
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
        Self::configured("ocstat", false)
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
                    session,
                    revision: 0,
                    provider: provider.into(),
                    can_set_target,
                    read_error: None,
                    connectivity: Connectivity::Connecting,
                    last_sync: None,
                    confirmed: None,
                    desired: None,
                    command: None,
                    in_flight: None,
                },
                next_id: 0,
                pending: None,
                clients: HashMap::new(),
            })),
            events,
            wake: Arc::new(Notify::new()),
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
        if core
            .state
            .confirmed
            .as_ref()
            .is_some_and(|old| old.revision > observed.revision)
        {
            return;
        }
        core.state.connectivity = if observed.online {
            Connectivity::Online
        } else {
            Connectivity::Offline
        };
        core.state.read_error = None;
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
        if let Some(command) = core.state.command.as_mut() {
            reconcile(command, &observed);
        }
        if let Some(command) = core.state.in_flight.as_mut() {
            reconcile(command, &observed);
        }
        core.state.confirmed = Some(observed);
        self.publish(&mut core);
    }
    fn read_failed(&self, error: ProviderError) {
        let mut core = self.core.lock().unwrap();
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
            if let Some(command) = self.take_pending() {
                self.execute(provider.as_ref(), command, timing).await;
            } else {
                tokio::select! {
                    _ = self.wake.notified() => {},
                    _ = sleep(timing.poll) => self.sync(provider.as_ref(), timing).await,
                }
            }
        }
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
                    "Request rejected. Check simulator settings and retry.",
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
            self.sync(provider, timing).await;
            let confirmed = self
                .snapshot()
                .in_flight
                .is_some_and(|c| c.id == id && c.phase == Phase::Confirmed);
            if confirmed {
                break;
            }
            if Instant::now() >= deadline {
                self.phase(id, Phase::Unknown, "Device application is unverified. Automatic reconciliation continues; refresh or retry when connected.");
                break;
            }
            sleep(timing.poll).await;
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
    if observed.applied_command == Some(command.id)
        && observed.target_temperature.0 == i16::from(command.target)
    {
        command.transition(
            Phase::Confirmed,
            "Device confirmed this target temperature.",
        );
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
            revision,
            applied_command: id,
            target_temperature: Temperature(target_value),
            online: true,
            room_temperature: Temperature(208),
            mode: Mode::Manual,
        }
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
