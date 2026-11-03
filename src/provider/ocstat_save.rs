//! APK-derived schedule saves with exact server readback after every operation.
use super::{AuthState, Endpoint, Ocstat};
use crate::{
    provider::{
        ProviderError,
        ocstat_schedule::map_draft,
        ocstat_schedule_tcp::{RawSchedule, Slots},
    },
    schedule::{Group, Preview, SaveRequest, SaveResult, SaveStatus, Schedule},
};
use std::collections::BTreeMap;
use tokio::sync::Mutex;

// Shared by browser sessions and the trial: a second session cannot race the
// preflight/write/readback sequence. No credential/token data is retained here.
pub(super) static SCHEDULE_GATE: Mutex<SaveGate> = Mutex::const_new(SaveGate {
    pending: BTreeMap::new(),
});
pub(super) struct SaveGate {
    pending: BTreeMap<(u64, String), Pending>,
}
enum Pending {
    Mode(u8),
    Group { mode: u8, day: u8, slots: Slots },
}
fn identity(auth: &AuthState) -> Result<(u64, String), ProviderError> {
    Ok((
        auth.session
            .as_ref()
            .ok_or(ProviderError::LoginRequired)?
            .id,
        auth.device_id
            .clone()
            .ok_or(ProviderError::DeviceNotFound)?,
    ))
}
fn reconcile(gate: &mut SaveGate, key: &(u64, String), raw: &RawSchedule) {
    if gate.pending.get(key).is_some_and(|p| match p {
        Pending::Mode(mode) => raw.mode == *mode,
        Pending::Group { mode, day, slots } => {
            raw.mode == *mode && raw.slots.get(day) == Some(slots)
        }
    }) {
        gate.pending.remove(key);
    }
}
fn preview(raw: &RawSchedule, pending: bool) -> Result<Preview, ProviderError> {
    let mut p = raw
        .clean()?
        .preview()
        .map_err(|_| ProviderError::InvalidResponse)?;
    p.can_save = !pending;
    p.save_pending = pending;
    Ok(p)
}
/// Android keeps six slots: removal adds 2880, insertion reuses a disabled
/// slot (prefer its old start time), and submission sorts by start minute.
fn plan(raw: &RawSchedule, desired: &Schedule) -> Result<RawSchedule, ProviderError> {
    let mapped = map_draft(desired.clone()).map_err(|_| ProviderError::Rejected)?;
    if raw.mode != mapped.mode {
        return Err(ProviderError::UnverifiedProtocol);
    }
    let mut changed = raw.clone();
    for group in mapped.programmes {
        let slots = changed
            .slots
            .get_mut(&group.day)
            .ok_or(ProviderError::InvalidResponse)?;
        if slots.iter().any(|r| {
            !(r[0] < 1440 || (2880..4320).contains(&r[0]))
                || r[0] % 10 != 0
                || !(50..=350).contains(&r[1])
                || r[1] % 5 != 0
        }) {
            return Err(ProviderError::InvalidResponse);
        }
        for row in slots.iter_mut() {
            if row[0] < 1440 && !group.active_rows.iter().any(|desired| desired[0] == row[0]) {
                row[0] += 2880;
            }
        }
        for desired in group.active_rows {
            let index = slots
                .iter()
                .position(|r| r[0] == desired[0])
                .or_else(|| slots.iter().position(|r| r[0] == desired[0] + 2880))
                .or_else(|| slots.iter().position(|r| r[0] >= 2880))
                .ok_or(ProviderError::InvalidResponse)?;
            slots[index] = desired;
        }
        slots.sort_by_key(|r| r[0]);
    }
    if changed.clean()? != *desired {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(changed)
}
fn result(
    status: SaveStatus,
    accepted_groups: Vec<Group>,
    message: &str,
    current: Option<Preview>,
) -> SaveResult {
    SaveResult {
        status,
        accepted_groups,
        message: message.into(),
        current,
    }
}
impl Ocstat {
    pub(super) async fn adopt_server_schedule(&self) -> Result<Preview, ProviderError> {
        // Keep the same cross-session lock as save/read. A failed or cancelled
        // fresh read must leave the unresolved attempt intact. This only clears
        // our local journal; an already-dispatched cloud command can still arrive.
        let mut gate = SCHEDULE_GATE.lock().await;
        let mut auth = self.auth.lock().await;
        let raw = self.read_raw_schedule(&mut auth).await?;
        let key = identity(&auth)?;
        let current = preview(&raw, false)?;
        gate.pending.remove(&key);
        Ok(current)
    }
    pub(super) async fn schedule_snapshot(&self) -> Result<Preview, ProviderError> {
        let mut gate = SCHEDULE_GATE.lock().await;
        let mut auth = self.auth.lock().await;
        let raw = self.read_raw_schedule(&mut auth).await?;
        let key = identity(&auth)?;
        reconcile(&mut gate, &key, &raw);
        preview(&raw, gate.pending.contains_key(&key))
    }
    pub(super) async fn save_schedule_checked(
        &self,
        request: SaveRequest,
    ) -> Result<SaveResult, ProviderError> {
        let baseline = request
            .baseline
            .validate()
            .map_err(|_| ProviderError::Rejected)?;
        let desired = request
            .schedule
            .validate()
            .map_err(|_| ProviderError::Rejected)?;
        let mut gate = SCHEDULE_GATE.lock().await;
        let mut auth = self.auth.lock().await;
        let mut fresh = self.read_raw_schedule(&mut auth).await?;
        let key = identity(&auth)?;
        reconcile(&mut gate, &key, &fresh);
        if gate.pending.contains_key(&key) {
            return Ok(result(
                SaveStatus::Unverified,
                vec![],
                "A previous save is still unverified. Check the server again; nothing was resent.",
                Some(preview(&fresh, true)?),
            ));
        }
        if fresh.clean()? != baseline {
            return Ok(result(
                SaveStatus::Conflict,
                vec![],
                "The server schedule changed since you loaded it. Nothing was sent. Your draft is kept.",
                Some(preview(&fresh, false)?),
            ));
        }
        let mapping = map_draft(desired.clone()).map_err(|_| ProviderError::Rejected)?;
        if fresh.mode != mapping.mode {
            if gate.pending.len() >= 128 {
                return Err(ProviderError::ReadOnly);
            }
            gate.pending
                .insert(key.clone(), Pending::Mode(mapping.mode));
            let _reply = self.write_schedule_mode(&mut auth, mapping.mode).await;
            // Switching can activate an existing programme immediately. Do not
            // assume it succeeded or write groups until a complete new-mode read.
            let observed = match self.read_raw_schedule(&mut auth).await {
                Ok(raw) => raw,
                Err(_) => {
                    return Ok(result(
                        SaveStatus::Unverified,
                        vec![],
                        "The grouping may already be active, but its stored schedule could not be read. Your new periods were not sent. Check the server before continuing.",
                        None,
                    ));
                }
            };
            reconcile(&mut gate, &key, &observed);
            if gate.pending.contains_key(&key) {
                return Ok(result(
                    SaveStatus::Unverified,
                    vec![],
                    "The grouping change is not verified yet. No periods were sent. Check the server; nothing was resent.",
                    Some(preview(&observed, true)?),
                ));
            }
            fresh = observed;
        }
        let desired_raw = plan(&fresh, &desired)?;
        let mut accepted = vec![];
        for (mapped, group) in mapping.programmes.iter().zip(&desired.groups) {
            let day = mapped.day;
            let group = group.group;
            let slots = desired_raw.slots[&day];
            if fresh.slots[&day] == slots {
                continue;
            }
            // A bounded journal in memory survives cancellation/logout and other
            // browser sessions. Restarts do not resume or retry schedule writes.
            if gate.pending.len() >= 128 {
                return Err(ProviderError::ReadOnly);
            }
            gate.pending.insert(
                key.clone(),
                Pending::Group {
                    mode: fresh.mode,
                    day,
                    slots,
                },
            );
            let _reply = self.write_schedule_group(&mut auth, day, &slots).await;
            // HTTP response content is not verified acknowledgement. Even a lost
            // reply can be reconciled by a fresh exact six-slot server read.
            let observed = match self.read_raw_schedule(&mut auth).await {
                Ok(raw) => raw,
                Err(_) => {
                    return Ok(result(
                        SaveStatus::Unverified,
                        accepted,
                        "Could not verify the save. Your draft is kept. Check the server before continuing; nothing will be resent automatically.",
                        None,
                    ));
                }
            };
            reconcile(&mut gate, &key, &observed);
            if gate.pending.contains_key(&key) {
                return Ok(result(
                    SaveStatus::Unverified,
                    accepted,
                    "The server has not returned the saved group yet. Your draft is kept. Check again; nothing will be resent automatically.",
                    Some(preview(&observed, true)?),
                ));
            }
            accepted.push(group);
            fresh.slots.insert(day, slots);
            if observed != fresh {
                return Ok(result(
                    SaveStatus::Conflict,
                    accepted,
                    "Some changes were accepted, but the server schedule also changed. Remaining groups were not sent. Your draft is kept.",
                    Some(preview(&observed, false)?),
                ));
            }
            fresh = observed;
        }
        Ok(result(
            SaveStatus::Accepted,
            accepted,
            "Accepted by server",
            Some(preview(&fresh, false)?),
        ))
    }
    pub(super) async fn write_schedule_group(
        &self,
        auth: &mut AuthState,
        day: u8,
        slots: &Slots,
    ) -> Result<(), ProviderError> {
        let mut form = schedule_form(auth)?;
        form.push(("day", day.to_string()));
        form.push((
            "programme",
            serde_json::to_string(slots).map_err(|_| ProviderError::InvalidResponse)?,
        ));
        self.transport
            .post(Endpoint::SetProgramme, form)
            .await
            .map(|_| ())
    }
    async fn write_schedule_mode(
        &self,
        auth: &mut AuthState,
        mode: u8,
    ) -> Result<(), ProviderError> {
        let day = match mode {
            0 => 1,
            1 => 8,
            2 => 10,
            _ => return Err(ProviderError::Rejected),
        };
        let mut form = schedule_form(auth)?;
        form.push(("mode", mode.to_string()));
        form.push(("day", day.to_string()));
        self.transport
            .post(Endpoint::SetProgrammeMode, form)
            .await
            .map(|_| ())
    }
}

fn schedule_form(auth: &mut AuthState) -> Result<super::Form, ProviderError> {
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
    auth.message_count += 1;
    let session = auth.session.as_ref().ok_or(ProviderError::LoginRequired)?;

    Ok(vec![
        ("token", session.token.clone()),
        ("user_id", session.id.to_string()),
        (
            "device_id",
            auth.device_id
                .clone()
                .ok_or(ProviderError::DeviceNotFound)?,
        ),
        ("messageId", message_id),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{Change, DayGroup, Pattern};
    #[test]
    fn replaces_full_slot_sets_and_roundtrips_all_supported_patterns() {
        for pattern in [Pattern::Daily, Pattern::WeekdayWeekend, Pattern::Weekly] {
            for count in 1..=6 {
                let source = Schedule {
                    pattern,
                    groups: pattern
                        .groups()
                        .into_iter()
                        .map(|group| DayGroup {
                            group,
                            changes: (0..6)
                                .map(|i| Change {
                                    start_minute: i * 120,
                                    target_temperature: (185 + i as i16 * 5).try_into().unwrap(),
                                })
                                .collect(),
                        })
                        .collect(),
                };
                let mapped = map_draft(source.clone()).unwrap();
                let raw = RawSchedule {
                    mode: mapped.mode,
                    slots: mapped
                        .programmes
                        .into_iter()
                        .map(|p| (p.day, p.active_rows.try_into().unwrap()))
                        .collect(),
                };
                let desired = Schedule {
                    pattern,
                    groups: pattern
                        .groups()
                        .into_iter()
                        .map(|group| DayGroup {
                            group,
                            changes: (0..count)
                                .map(|i| Change {
                                    start_minute: 60 + i * 120,
                                    target_temperature: (200 + i as i16 * 5).try_into().unwrap(),
                                })
                                .collect(),
                        })
                        .collect(),
                };
                let changed = plan(&raw, &desired).unwrap();
                assert_eq!(changed.clean().unwrap(), desired);
                assert!(
                    changed.slots.values().all(|slots| slots
                        .iter()
                        .filter(|r| r[0] < 1440)
                        .count()
                        == count as usize)
                );
                assert_eq!(plan(&changed, &source).unwrap().clean().unwrap(), source);
            }
        }
    }
    #[test]
    fn unused_slots_keep_values_and_deleted_periods_use_android_disabled_encoding() {
        let raw = RawSchedule {
            mode: 2,
            slots: BTreeMap::from([(
                10,
                [
                    [420, 215],
                    [1410, 185],
                    [3450, 200],
                    [3570, 205],
                    [3690, 210],
                    [3870, 215],
                ],
            )]),
        };
        let mut desired = raw.clean().unwrap();
        desired.groups[0].changes.pop();
        let changed = plan(&raw, &desired).unwrap();
        for inactive in &raw.slots[&10][2..] {
            assert!(changed.slots[&10].contains(inactive));
        }
        assert!(changed.slots[&10].contains(&[4290, 185]));
        desired.groups[0].changes.push(Change {
            start_minute: 1410,
            target_temperature: 190.try_into().unwrap(),
        });
        let restored = plan(&changed, &desired).unwrap();
        for inactive in &raw.slots[&10][2..] {
            assert!(restored.slots[&10].contains(inactive));
        }
        assert!(restored.slots[&10].contains(&[1410, 190]));
    }
}
