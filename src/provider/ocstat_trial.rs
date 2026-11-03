//! Explicit, one-period write/restoration trial. Never used by normal Save.
use super::{AuthState, Ocstat};
use crate::provider::{
    ProviderError, ProviderFuture,
    ocstat_schedule_tcp::{self, RawSchedule},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tokio::sync::Mutex;

pub(super) trait ScheduleReader: Send + Sync {
    fn read<'a>(
        &'a self,
        user: u64,
        token: &'a str,
        device: &'a str,
        mode: u8,
    ) -> ProviderFuture<'a, RawSchedule>;
}
pub(super) struct TcpScheduleReader;
impl ScheduleReader for TcpScheduleReader {
    fn read<'a>(
        &'a self,
        user: u64,
        token: &'a str,
        device: &'a str,
        mode: u8,
    ) -> ProviderFuture<'a, RawSchedule> {
        Box::pin(ocstat_schedule_tcp::read(user, token, device, mode))
    }
}
#[derive(Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status {},
    Prepare {},
    Apply {
        trial_id: String,
        confirm_change: bool,
    },
    Restore {
        trial_id: String,
    },
    Check {
        trial_id: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Prepared,
    ApplySent,
    Applied,
    RestoreSent,
    Restored,
}
#[derive(Clone, Serialize)]
pub struct Status {
    pub trial_id: Option<String>,
    pub phase: Option<Phase>,
    pub group: &'static str,
    pub start_minute: Option<u16>,
    pub original_temperature: Option<u16>,
    pub trial_temperature: Option<u16>,
    pub message: String,
}
// No Debug: the disk backup contains identity, but never credentials or tokens.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    trial_id: String,
    user: u64,
    device: String,
    baseline: RawSchedule,
    changed: RawSchedule,
    index: usize,
    phase: Phase,
}
impl Record {
    fn status(&self, message: &str) -> Status {
        Status {
            trial_id: Some(self.trial_id.clone()),
            phase: Some(self.phase),
            group: "Weekends",
            start_minute: Some(self.baseline.slots[&9][self.index][0]),
            original_temperature: Some(self.baseline.slots[&9][self.index][1]),
            trial_temperature: Some(self.changed.slots[&9][self.index][1]),
            message: message.into(),
        }
    }
    fn matches(&self, raw: &RawSchedule, restored: bool) -> bool {
        raw.mode == 1
            && raw.slots.get(&9)
                == if restored {
                    self.baseline.slots.get(&9)
                } else {
                    self.changed.slots.get(&9)
                }
    }
}
/// One trial per server, shared across browser sessions. A private durable backup
/// allows the same account to resume checking/restoring after logout or restart.
pub struct TrialStore {
    path: PathBuf,
    record: Mutex<Option<Record>>,
}
fn local_error() -> std::io::Error {
    std::io::Error::other("Cannot use the private schedule trial backup directory")
}
impl TrialStore {
    pub fn open(directory: &Path) -> std::io::Result<Self> {
        if !directory.is_absolute() {
            return Err(local_error());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            if !directory.exists() {
                fs::DirBuilder::new().mode(0o700).create(directory)?;
            }
            let metadata = fs::symlink_metadata(directory)?;
            if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
                return Err(local_error());
            }
        }
        #[cfg(not(unix))]
        {
            return Err(local_error());
        }
        let path = directory.join("schedule-trial.json");
        let record = if path.exists() {
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file() || meta.len() > 16384 {
                return Err(local_error());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o077 != 0 {
                    return Err(local_error());
                }
            }
            let record: Record =
                serde_json::from_slice(&fs::read(&path)?).map_err(|_| local_error())?;
            let (changed, index) = candidate(&record.baseline).map_err(|_| local_error())?;
            if changed != record.changed || index != record.index || record.trial_id.len() != 32 {
                return Err(local_error());
            }
            Some(record)
        } else {
            None
        };
        Ok(Self {
            path,
            record: Mutex::new(record),
        })
    }
    fn persist(&self, record: &Record) -> Result<(), ProviderError> {
        let bytes = serde_json::to_vec(record).map_err(|_| ProviderError::Transport)?;
        let temp = self.path.with_extension(format!("{}.tmp", random_id()?));
        let result = (|| -> std::io::Result<()> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            fs::File::open(self.path.parent().ok_or_else(local_error)?)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result.map_err(|_| ProviderError::Transport)
    }
}
fn random_id() -> Result<String, ProviderError> {
    let mut bytes = [0; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| ProviderError::Transport)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn candidate(baseline: &RawSchedule) -> Result<(RawSchedule, usize), ProviderError> {
    baseline.clean()?;
    if baseline.slots.len() != 2
        || baseline.slots.values().flatten().any(|row| {
            !(row[0] < 1440 || (2880..4320).contains(&row[0]))
                || row[0] % 10 != 0
                || !(50..=350).contains(&row[1])
                || row[1] % 5 != 0
        })
    {
        return Err(ProviderError::InvalidResponse);
    }
    if baseline.mode != 1 {
        return Err(ProviderError::UnverifiedProtocol);
    }
    let rows = baseline
        .slots
        .get(&9)
        .ok_or(ProviderError::InvalidResponse)?;
    // Keep all time boundaries and disabled slots byte-for-byte equivalent.
    let index = rows
        .iter()
        .position(|row| row[0] < 1440 && row[1] <= 345)
        .ok_or(ProviderError::UnverifiedProtocol)?;
    let mut changed = baseline.clone();
    changed.slots.get_mut(&9).unwrap()[index][1] += 5;
    changed.clean()?;
    Ok((changed, index))
}
impl Ocstat {
    pub(super) async fn run_schedule_trial(&self, action: Action) -> Result<Status, ProviderError> {
        let _gate = super::save::SCHEDULE_GATE.lock().await;
        let store = self.trial.as_ref().ok_or(ProviderError::ReadOnly)?;
        let mut record = store.record.lock().await;
        let mut auth = self.auth.lock().await;
        let session = auth.session.as_ref().ok_or(ProviderError::LoginRequired)?;
        let user = session.id;
        let device = auth
            .device_id
            .clone()
            .ok_or(ProviderError::DeviceNotFound)?;
        if let Some(existing) = record.as_ref()
            && (existing.user != user || existing.device != device)
        {
            return Err(ProviderError::ReadOnly);
        }
        if matches!(action, Action::Prepare {}) {
            if record
                .as_ref()
                .is_some_and(|r| !matches!(r.phase, Phase::Prepared | Phase::Restored))
            {
                return Err(ProviderError::ReadOnly);
            }
            let baseline = self.read_raw_schedule(&mut auth).await?;
            let (changed, index) = candidate(&baseline)?;
            let plan = Record {
                trial_id: random_id()?,
                user,
                device,
                baseline,
                changed,
                index,
                phase: Phase::Prepared,
            };
            store.persist(&plan)?;
            *record = Some(plan);
        }
        let Some(plan) = record.as_mut() else {
            return Ok(Status {
                trial_id: None,
                phase: None,
                group: "Weekends",
                start_minute: None,
                original_temperature: None,
                trial_temperature: None,
                message:
                    "Prepare reads and backs up the current schedule. It does not send a change."
                        .into(),
            });
        };
        let id = match &action {
            Action::Apply { trial_id, .. }
            | Action::Restore { trial_id }
            | Action::Check { trial_id } => Some(trial_id),
            _ => None,
        };
        if id.is_some_and(|id| id != &plan.trial_id) {
            return Err(ProviderError::Rejected);
        }
        match action {
            Action::Prepare {} | Action::Status {} => {
                Ok(plan.status("Backup retained. Use Check to read the current server schedule."))
            }
            Action::Check { .. } => self.check_trial(&mut auth, store, plan).await,
            Action::Apply { confirm_change, .. } => {
                if !confirm_change || plan.phase != Phase::Prepared {
                    return Err(ProviderError::ReadOnly);
                }
                let fresh = self.read_raw_schedule(&mut auth).await?;
                if fresh != plan.baseline {
                    return Ok(plan.status("The schedule changed since preparation. Nothing was sent. Reload the original schedule before continuing."));
                }
                plan.phase = Phase::ApplySent;
                // Durable before dispatch, including cancellation/timeouts. Never auto-retry.
                store.persist(plan)?;
                let reply = self.write_trial_group(&mut auth, &plan.changed).await;
                let mut status = self.check_trial(&mut auth, store, plan).await?;
                if reply.is_err() {
                    status.message = format!(
                        "The HTTPS reply was unavailable or unsuccessful. {}",
                        status.message
                    );
                }
                Ok(status)
            }
            Action::Restore { .. } => {
                if !matches!(plan.phase, Phase::Applied | Phase::ApplySent) {
                    return Err(ProviderError::ReadOnly);
                }
                let fresh = self.read_raw_schedule(&mut auth).await?;
                if !plan.matches(&fresh, false) {
                    return Ok(plan.status("The server does not currently show the exact test group. Nothing was sent; check again before deciding how to recover."));
                }
                plan.phase = Phase::RestoreSent;
                store.persist(plan)?;
                let reply = self.write_trial_group(&mut auth, &plan.baseline).await;
                let mut status = self.check_trial(&mut auth, store, plan).await?;
                if reply.is_err() {
                    status.message = format!(
                        "The HTTPS reply was unavailable or unsuccessful. {}",
                        status.message
                    );
                }
                Ok(status)
            }
        }
    }
    async fn check_trial(
        &self,
        auth: &mut AuthState,
        store: &TrialStore,
        plan: &mut Record,
    ) -> Result<Status, ProviderError> {
        let fresh = match self.read_raw_schedule(auth).await {
            Ok(raw) => raw,
            Err(_) => return Ok(plan.status("Read-back failed. The outcome is unverified. Keep the backup and use Check; do not resend.")),
        };
        let message = if matches!(plan.phase, Phase::ApplySent | Phase::Applied)
            && plan.matches(&fresh, false)
        {
            plan.phase = Phase::Applied;
            "Server read-back matches the test group, including all six slots. Physical thermostat application is unverified. Restore the original schedule next."
        } else if plan.phase == Phase::RestoreSent && plan.matches(&fresh, true) {
            plan.phase = Phase::Restored;
            "Server read-back matches the original group, including all six slots. Restoration verified on the server; physical application is unverified."
        } else if plan.phase == Phase::Prepared && fresh == plan.baseline {
            "The server still matches the backup. No test change has been sent."
        } else if plan.phase == Phase::Restored && plan.matches(&fresh, true) {
            "The server still matches the restored original group."
        } else {
            "Read-back differs from the expected state. No retry or rollback was sent. Keep the backup for recovery."
        };
        store.persist(plan)?;
        Ok(plan.status(message))
    }
    async fn write_trial_group(
        &self,
        auth: &mut AuthState,
        raw: &RawSchedule,
    ) -> Result<(), ProviderError> {
        self.write_schedule_group(auth, 9, &raw.slots[&9]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        Provider,
        ocstat::{Endpoint, Form, Session, Transport},
    };
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex as StdMutex},
    };
    #[derive(Clone, Copy)]
    enum WriteBehavior {
        Normal,
        LostReply,
        NoChange,
        SecondNoChange,
        ReadbackFails,
        Pending,
    }
    struct Fake {
        raw: StdMutex<RawSchedule>,
        bank: StdMutex<BTreeMap<u8, RawSchedule>>,
        writes: StdMutex<Vec<Form>>,
        behavior: StdMutex<WriteBehavior>,
    }
    impl Transport for Arc<Fake> {
        fn post(&self, endpoint: Endpoint, form: Form) -> ProviderFuture<'_, Vec<u8>> {
            Box::pin(async move {
                match endpoint {
                    Endpoint::DeviceList => Ok(serde_json::to_vec(&serde_json::json!({"statu":true,"error_code":1,"devices":[{"device_id":"TESTDEVICE01","online":true,"current_temprature":"215","inside_temparature":"211","work_mode":"1","program_mode":self.raw.lock().unwrap().mode.to_string()}]})).unwrap()),
                    Endpoint::DeviceStatus => Ok(b"##3cLTESTDEVICE01102b1701011619000162b03-0000420000000010000".to_vec()),
                    Endpoint::SetProgrammeMode => {
                        self.writes.lock().unwrap().push(form.clone());
                        let behavior = *self.behavior.lock().unwrap();
                        if matches!(behavior, WriteBehavior::Pending) { return std::future::pending().await; }
                        if matches!(behavior, WriteBehavior::NoChange) { return Err(ProviderError::Transport); }
                        let mode: u8 = form.iter().find(|(k,_)| *k == "mode").unwrap().1.parse().unwrap();
                        let day = form.iter().find(|(k,_)| *k == "day").unwrap().1.parse::<u8>().unwrap();
                        assert_eq!(day, match mode { 0=>1, 1=>8, 2=>10, _=>panic!("unknown mode") });
                        let mut raw = self.raw.lock().unwrap();
                        let mut bank = self.bank.lock().unwrap();
                        bank.insert(raw.mode, raw.clone());
                        *raw = bank.get(&mode).cloned().unwrap_or_else(|| {
                            let days: Vec<u8> = match mode { 0=>(1..=7).collect(), 1=>vec![8,9], _=>vec![10] };
                            RawSchedule { mode, slots: days.into_iter().map(|d| (d, [[0,180],[3300,185],[3450,200],[3570,200],[3690,200],[3870,210]])).collect() }
                        });
                        if matches!(behavior, WriteBehavior::LostReply) { Err(ProviderError::Transport) } else { Ok(b"unverified mode response".to_vec()) }
                    }
                    Endpoint::SetProgramme => {
                        self.writes.lock().unwrap().push(form.clone());
                        let behavior = *self.behavior.lock().unwrap();
                        if matches!(behavior,WriteBehavior::Pending) { return std::future::pending().await; }
                        if matches!(behavior,WriteBehavior::NoChange) || matches!(behavior,WriteBehavior::SecondNoChange) && self.writes.lock().unwrap().len() == 2 { return Err(ProviderError::Transport); }
                        let rows = serde_json::from_str(&form.iter().find(|(key,_)| *key=="programme").unwrap().1).unwrap();
                        let day = form.iter().find(|(key,_)| *key=="day").unwrap().1.parse().unwrap();
                        self.raw.lock().unwrap().slots.insert(day,rows);
                        if matches!(behavior,WriteBehavior::LostReply) { Err(ProviderError::Transport) } else { Ok(b"unverified HTTP reply".to_vec()) }
                    }
                    _ => panic!("Schedule saves must not log in or send target commands"),
                }
            })
        }
    }
    impl ScheduleReader for Arc<Fake> {
        fn read<'a>(
            &'a self,
            _user: u64,
            _token: &'a str,
            _device: &'a str,
            _mode: u8,
        ) -> ProviderFuture<'a, RawSchedule> {
            Box::pin(async {
                if matches!(*self.behavior.lock().unwrap(), WriteBehavior::ReadbackFails)
                    && !self.writes.lock().unwrap().is_empty()
                {
                    return Err(ProviderError::Transport);
                }
                Ok(self.raw.lock().unwrap().clone())
            })
        }
    }
    struct Fixture {
        provider: Ocstat,
        fake: Arc<Fake>,
        dir: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
    fn raw() -> RawSchedule {
        let slots = [
            [420, 215],
            [1410, 185],
            [3450, 200],
            [3570, 200],
            [3690, 200],
            [3870, 210],
        ];
        RawSchedule {
            mode: 1,
            slots: BTreeMap::from([(8, slots), (9, slots)]),
        }
    }
    async fn provider(fake: Arc<Fake>, store: Arc<TrialStore>) -> Ocstat {
        let mut provider =
            Ocstat::with_transport(Box::new(fake.clone())).with_schedule_trial(store);
        provider.schedule_reader = Box::new(fake);
        {
            let mut auth = provider.auth.lock().await;
            auth.session = Some(Session {
                id: 7,
                token: "TEST_TOKEN_ONLY".into(),
            });
            auth.device_id = Some("TESTDEVICE01".into());
        }
        provider
    }
    async fn fixture() -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("smarttherm-trial-test-{}", random_id().unwrap()));
        let store = Arc::new(TrialStore::open(&dir).unwrap());
        let fake = Arc::new(Fake {
            raw: StdMutex::new(raw()),
            bank: StdMutex::new(BTreeMap::new()),
            writes: StdMutex::new(vec![]),
            behavior: StdMutex::new(WriteBehavior::Normal),
        });
        Fixture {
            provider: provider(fake.clone(), store).await,
            fake,
            dir,
        }
    }
    async fn prepare(f: &Fixture) -> String {
        f.provider
            .schedule_trial(Action::Prepare {})
            .await
            .unwrap()
            .trial_id
            .unwrap()
    }
    fn apply(id: &str) -> Action {
        Action::Apply {
            trial_id: id.into(),
            confirm_change: true,
        }
    }
    #[tokio::test]
    async fn prepares_without_writes_and_restores_all_six_slots_exactly() {
        let f = fixture().await;
        let id = prepare(&f).await;
        assert!(f.fake.writes.lock().unwrap().is_empty());
        let original = raw();
        let result = f.provider.schedule_trial(apply(&id)).await.unwrap();
        assert_eq!(result.phase, Some(Phase::Applied));
        let modified = f.fake.raw.lock().unwrap().clone();
        let mut expected = original.clone();
        expected.slots.get_mut(&9).unwrap()[0][1] = 220;
        assert_eq!(modified, expected);
        let response = f
            .provider
            .schedule_trial(Action::Restore {
                trial_id: id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(response.phase, Some(Phase::Restored));
        assert_eq!(*f.fake.raw.lock().unwrap(), original);
        let writes = f.fake.writes.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert!(
            writes
                .iter()
                .all(|form| form.len() == 6 && form.contains(&("day", "9".into())))
        );
        let ids: Vec<_> = writes
            .iter()
            .map(|form| &form.iter().find(|(key, _)| *key == "messageId").unwrap().1)
            .collect();
        assert_ne!(ids[0], ids[1]);
        let backup = fs::read_to_string(f.dir.join("schedule-trial.json")).unwrap();
        assert!(!backup.contains("TEST_TOKEN_ONLY"));
        let public = serde_json::to_string(&response).unwrap();
        assert!(!public.contains("TESTDEVICE01"));
    }
    #[tokio::test]
    async fn conflicts_stale_pages_and_missing_confirmation_never_write() {
        let f = fixture().await;
        let id = prepare(&f).await;
        assert!(f.provider.schedule_trial(apply("stale")).await.is_err());
        assert!(
            f.provider
                .schedule_trial(Action::Apply {
                    trial_id: id.clone(),
                    confirm_change: false
                })
                .await
                .is_err()
        );
        f.fake.raw.lock().unwrap().slots.get_mut(&8).unwrap()[0][1] = 225;
        let result = f.provider.schedule_trial(apply(&id)).await.unwrap();
        assert_eq!(result.phase, Some(Phase::Prepared));
        assert!(f.fake.writes.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn lost_reply_uses_readback_and_duplicate_actions_never_resend() {
        let f = fixture().await;
        let id = prepare(&f).await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::LostReply;
        assert_eq!(
            f.provider.schedule_trial(apply(&id)).await.unwrap().phase,
            Some(Phase::Applied)
        );
        assert!(f.provider.schedule_trial(apply(&id)).await.is_err());
        f.fake.raw.lock().unwrap().slots.get_mut(&9).unwrap()[1][1] = 190;
        assert_eq!(
            f.provider
                .schedule_trial(Action::Restore { trial_id: id })
                .await
                .unwrap()
                .phase,
            Some(Phase::Applied)
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn unmatched_readback_is_uncertain_and_check_never_resends() {
        let f = fixture().await;
        let id = prepare(&f).await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::NoChange;
        assert_eq!(
            f.provider.schedule_trial(apply(&id)).await.unwrap().phase,
            Some(Phase::ApplySent)
        );
        assert_eq!(
            f.provider
                .schedule_trial(Action::Check {
                    trial_id: id.clone()
                })
                .await
                .unwrap()
                .phase,
            Some(Phase::ApplySent)
        );
        assert!(f.provider.schedule_trial(apply(&id)).await.is_err());
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
    }
    #[tokio::test(start_paused = true)]
    async fn cancellation_and_restart_preserve_uncertainty_and_recovery_backup() {
        let f = fixture().await;
        let id = prepare(&f).await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::Pending;
        let mut write = f.provider.schedule_trial(apply(&id));
        tokio::select! {
            _ = &mut write => panic!("write should remain pending"),
            _ = async { loop { if f.fake.writes.lock().unwrap().len() == 1 { break; } tokio::task::yield_now().await; } } => {}
        }
        drop(write);
        let store = Arc::new(TrialStore::open(&f.dir).unwrap());
        let restarted = provider(f.fake.clone(), store).await;
        assert_eq!(
            restarted
                .schedule_trial(Action::Status {})
                .await
                .unwrap()
                .phase,
            Some(Phase::ApplySent)
        );
        assert!(restarted.schedule_trial(apply(&id)).await.is_err());
        // A late upstream application is observed, then restored explicitly.
        *f.fake.raw.lock().unwrap() = candidate(&raw()).unwrap().0;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::Normal;
        assert_eq!(
            restarted
                .schedule_trial(Action::Check {
                    trial_id: id.clone()
                })
                .await
                .unwrap()
                .phase,
            Some(Phase::Applied)
        );
        assert_eq!(
            restarted
                .schedule_trial(Action::Restore { trial_id: id })
                .await
                .unwrap()
                .phase,
            Some(Phase::Restored)
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn disabled_provider_and_wrong_account_cannot_use_trial() {
        let f = fixture().await;
        prepare(&f).await;
        let store = Arc::new(TrialStore::open(&f.dir).unwrap());
        let other = provider(f.fake.clone(), store).await;
        other.auth.lock().await.session.as_mut().unwrap().id = 8;
        assert!(other.schedule_trial(Action::Status {}).await.is_err());
        assert!(
            Ocstat::with_transport(Box::new(f.fake.clone()))
                .schedule_trial(Action::Prepare {})
                .await
                .is_err()
        );
        assert!(f.fake.writes.lock().unwrap().is_empty());
    }
    #[test]
    fn candidate_rejects_other_modes_and_invalid_disabled_slots() {
        let mut raw = raw();
        raw.mode = 2;
        assert!(candidate(&raw).is_err());
        raw.mode = 1;
        raw.slots.get_mut(&9).unwrap()[2] = [5000, 200];
        assert!(candidate(&raw).is_err());
    }
    async fn save_fixture() -> Fixture {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);
        let f = fixture().await;
        f.provider.auth.lock().await.session.as_mut().unwrap().id =
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        f
    }
    fn save_request() -> crate::schedule::SaveRequest {
        let baseline = raw().clean().unwrap();
        let mut desired = baseline.clone();
        for group in &mut desired.groups {
            group.changes[0].target_temperature = 220.try_into().unwrap();
        }
        crate::schedule::SaveRequest {
            baseline,
            schedule: desired,
        }
    }
    #[tokio::test]
    async fn normal_save_serializes_only_changed_groups_and_preserves_disabled_slots() {
        use crate::schedule::SaveStatus;
        let f = save_fixture().await;
        assert!(f.provider.read_schedule_preview().await.unwrap().can_save);
        let request = save_request();
        let reply = f.provider.save_schedule(request.clone()).await.unwrap();
        assert_eq!(reply.status, SaveStatus::Accepted);
        assert_eq!(reply.accepted_groups.len(), 2);
        assert_eq!(reply.current.unwrap().schedule, request.schedule);
        for day in [8, 9] {
            assert_eq!(
                f.fake.raw.lock().unwrap().slots[&day][2..],
                raw().slots[&day][2..]
            );
        }
        {
            let writes = f.fake.writes.lock().unwrap();
            assert_eq!(writes.len(), 2);
            assert!(writes[0].contains(&("day", "8".into())));
            assert!(writes[1].contains(&("day", "9".into())));
        }
        // Reposting the old baseline never repeats writes.
        assert_eq!(
            f.provider.save_schedule(request).await.unwrap().status,
            SaveStatus::Conflict
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn normal_save_rejects_conflicts_without_writes() {
        let f = save_fixture().await;
        let request = save_request();
        f.fake.raw.lock().unwrap().slots.get_mut(&8).unwrap()[0][1] = 225;
        assert_eq!(
            f.provider.save_schedule(request).await.unwrap().status,
            crate::schedule::SaveStatus::Conflict
        );
        assert!(f.fake.writes.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn normal_save_lost_reply_reconciles_but_unmatched_readback_stops_remaining_groups() {
        use crate::schedule::SaveStatus;
        let f = save_fixture().await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::LostReply;
        assert_eq!(
            f.provider
                .save_schedule(save_request())
                .await
                .unwrap()
                .status,
            SaveStatus::Accepted
        );
        let f = save_fixture().await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::NoChange;
        assert_eq!(
            f.provider
                .save_schedule(save_request())
                .await
                .unwrap()
                .status,
            SaveStatus::Unverified
        );
        assert!(
            f.provider
                .read_schedule_preview()
                .await
                .unwrap()
                .save_pending
        );
        assert_eq!(
            f.provider
                .save_schedule(save_request())
                .await
                .unwrap()
                .status,
            SaveStatus::Unverified
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn normal_save_reports_partial_acceptance_and_read_only_check_observes_late_application()
    {
        use crate::schedule::{Group, SaveStatus};
        let f = save_fixture().await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::SecondNoChange;
        let reply = f.provider.save_schedule(save_request()).await.unwrap();
        assert_eq!(reply.status, SaveStatus::Unverified);
        assert_eq!(reply.accepted_groups, vec![Group::Weekdays]);
        assert_eq!(f.fake.writes.lock().unwrap().len(), 2);
        f.fake.raw.lock().unwrap().slots.get_mut(&9).unwrap()[0][1] = 220;
        let checked = f.provider.read_schedule_preview().await.unwrap();
        assert!(!checked.save_pending);
        assert_eq!(checked.schedule, save_request().schedule);
        assert_eq!(f.fake.writes.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn normal_save_cancellation_remains_pending_across_browser_sessions() {
        let f = save_fixture().await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::Pending;
        let mut save = f.provider.save_schedule(save_request());
        tokio::select! {
            _ = &mut save => panic!("write should be pending"),
            _ = async { loop { if f.fake.writes.lock().unwrap().len() == 1 { break; } tokio::task::yield_now().await; } } => {}
        }
        drop(save);
        let other = provider(f.fake.clone(), Arc::new(TrialStore::open(&f.dir).unwrap())).await;
        other.auth.lock().await.session.as_mut().unwrap().id =
            f.provider.auth.lock().await.session.as_ref().unwrap().id;
        assert!(other.read_schedule_preview().await.unwrap().save_pending);
        assert_eq!(
            other.save_schedule(save_request()).await.unwrap().status,
            crate::schedule::SaveStatus::Unverified
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn normal_save_failed_readback_does_not_repeat_or_send_second_group() {
        let f = save_fixture().await;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::ReadbackFails;
        let reply = f.provider.save_schedule(save_request()).await.unwrap();
        assert_eq!(reply.status, crate::schedule::SaveStatus::Unverified);
        assert!(reply.current.is_none());
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
        *f.fake.behavior.lock().unwrap() = WriteBehavior::Normal;
        assert!(
            !f.provider
                .read_schedule_preview()
                .await
                .unwrap()
                .save_pending
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn full_save_changes_boundaries_inserts_deletes_and_restores() {
        use crate::schedule::{Change, SaveRequest, SaveStatus};
        let f = save_fixture().await;
        let baseline = raw().clean().unwrap();
        let mut draft = baseline.clone();
        draft.groups[0].changes[0].start_minute = 450;
        draft.groups[0].changes.insert(
            1,
            Change {
                start_minute: 720,
                target_temperature: 200.try_into().unwrap(),
            },
        );
        draft.groups[1].changes.remove(1);
        let response = f
            .provider
            .save_schedule(SaveRequest {
                baseline: baseline.clone(),
                schedule: draft.clone(),
            })
            .await
            .unwrap();
        assert_eq!(response.status, SaveStatus::Accepted);
        assert_eq!(response.current.unwrap().schedule, draft);
        // Deleted entries retain the original time+2880 and temperature.
        assert!(f.fake.raw.lock().unwrap().slots[&9].contains(&[4290, 185]));
        assert_eq!(f.fake.writes.lock().unwrap().len(), 2);
        let restored = f
            .provider
            .save_schedule(SaveRequest {
                baseline: draft,
                schedule: baseline.clone(),
            })
            .await
            .unwrap();
        assert_eq!(restored.status, SaveStatus::Accepted);
        assert_eq!(restored.current.unwrap().schedule, baseline);
    }
    #[tokio::test]
    async fn full_save_switches_mode_then_writes_all_groups_and_reads_them_back() {
        use crate::schedule::{Change, DayGroup, Pattern, SaveRequest, SaveStatus};
        let f = save_fixture().await;
        let mut baseline = raw().clean().unwrap();
        for pattern in [Pattern::Daily, Pattern::Weekly, Pattern::WeekdayWeekend] {
            let desired = crate::schedule::Schedule {
                pattern,
                groups: pattern
                    .groups()
                    .into_iter()
                    .map(|group| DayGroup {
                        group,
                        changes: vec![Change {
                            start_minute: 0,
                            target_temperature: 205.try_into().unwrap(),
                        }],
                    })
                    .collect(),
            };
            let previous_writes = f.fake.writes.lock().unwrap().len();
            let response = f
                .provider
                .save_schedule(SaveRequest {
                    baseline,
                    schedule: desired.clone(),
                })
                .await
                .unwrap();
            assert_eq!(response.status, SaveStatus::Accepted);
            assert_eq!(response.current.unwrap().schedule, desired);
            let writes = f.fake.writes.lock().unwrap().clone();
            assert!(writes[previous_writes].iter().any(|(k, _)| *k == "mode"));
            assert_eq!(writes.len() - previous_writes, 1 + pattern.groups().len());
            let ids: std::collections::BTreeSet<_> = writes
                .iter()
                .map(|f| f.iter().find(|(k, _)| *k == "messageId").unwrap().1.clone())
                .collect();
            assert_eq!(ids.len(), writes.len());
            baseline = desired;
        }
    }
    #[tokio::test]
    async fn unverified_mode_never_writes_periods_and_a_lost_reply_can_reconcile() {
        use crate::schedule::{Group, Pattern, SaveStatus};
        for behavior in [
            WriteBehavior::NoChange,
            WriteBehavior::LostReply,
            WriteBehavior::ReadbackFails,
        ] {
            let f = save_fixture().await;
            let mut request = save_request();
            request.schedule.pattern = Pattern::Daily;
            request.schedule.groups.truncate(1);
            request.schedule.groups[0].group = Group::EveryDay;
            *f.fake.behavior.lock().unwrap() = behavior;
            let response = f.provider.save_schedule(request.clone()).await.unwrap();
            if matches!(behavior, WriteBehavior::LostReply) {
                assert_eq!(response.status, SaveStatus::Accepted);
                assert_eq!(response.current.unwrap().schedule, request.schedule);
            } else {
                assert_eq!(response.status, SaveStatus::Unverified);
                assert!(response.accepted_groups.is_empty());
                assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
            }
        }
    }
    #[tokio::test]
    async fn mode_success_then_group_failure_reports_actual_mode_and_preserves_recovery() {
        use crate::schedule::{Group, Pattern, SaveStatus};
        let f = save_fixture().await;
        let mut request = save_request();
        request.schedule.pattern = Pattern::Daily;
        request.schedule.groups.truncate(1);
        request.schedule.groups[0].group = Group::EveryDay;
        *f.fake.behavior.lock().unwrap() = WriteBehavior::SecondNoChange;
        let response = f.provider.save_schedule(request).await.unwrap();
        assert_eq!(response.status, SaveStatus::Unverified);
        let current = response.current.unwrap();
        assert_eq!(current.schedule.pattern, Pattern::Daily);
        assert!(current.save_pending);
        assert!(response.accepted_groups.is_empty());
        assert_eq!(f.fake.writes.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn explicit_recovery_unlocks_mode_and_group_attempts_without_resending() {
        use crate::schedule::{Group, Pattern, SaveStatus};
        for mode_change in [false, true] {
            let f = save_fixture().await;
            let mut request = save_request();
            if mode_change {
                request.schedule.pattern = Pattern::Daily;
                request.schedule.groups.truncate(1);
                request.schedule.groups[0].group = Group::EveryDay;
            }
            *f.fake.behavior.lock().unwrap() = WriteBehavior::NoChange;
            assert_eq!(
                f.provider
                    .save_schedule(request.clone())
                    .await
                    .unwrap()
                    .status,
                SaveStatus::Unverified
            );
            assert!(
                f.provider
                    .read_schedule_preview()
                    .await
                    .unwrap()
                    .save_pending
            );
            let original = f.fake.raw.lock().unwrap().clone();
            let recovered = f.provider.recover_schedule().await.unwrap();
            assert_eq!(recovered.schedule, original.clean().unwrap());
            assert!(!recovered.save_pending);
            assert!(recovered.can_save);
            assert_eq!(*f.fake.raw.lock().unwrap(), original);
            assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
            // Editing is available again, but only a separate explicit Save writes.
            *f.fake.behavior.lock().unwrap() = WriteBehavior::Normal;
            assert_eq!(
                f.provider.save_schedule(request).await.unwrap().status,
                SaveStatus::Accepted
            );
        }
    }
    #[tokio::test]
    async fn recovery_requires_a_complete_read_and_is_scoped_to_the_account_device() {
        let f = save_fixture().await;
        let other = save_fixture().await;
        for fixture in [&f, &other] {
            *fixture.fake.behavior.lock().unwrap() = WriteBehavior::NoChange;
            fixture
                .provider
                .save_schedule(save_request())
                .await
                .unwrap();
        }
        *f.fake.behavior.lock().unwrap() = WriteBehavior::ReadbackFails;
        assert!(f.provider.recover_schedule().await.is_err());
        *f.fake.behavior.lock().unwrap() = WriteBehavior::Normal;
        assert!(
            f.provider
                .read_schedule_preview()
                .await
                .unwrap()
                .save_pending
        );
        f.provider.recover_schedule().await.unwrap();
        assert!(
            !f.provider
                .read_schedule_preview()
                .await
                .unwrap()
                .save_pending
        );
        assert!(
            other
                .provider
                .read_schedule_preview()
                .await
                .unwrap()
                .save_pending
        );
        assert_eq!(f.fake.writes.lock().unwrap().len(), 1);
        assert_eq!(other.fake.writes.lock().unwrap().len(), 1);
    }
}
