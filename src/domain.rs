use serde::{Deserialize, Serialize};
use std::fmt;

/// All domain temperatures are exact integer tenths of a degree Celsius.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Temperature(pub i16);

impl fmt::Display for Temperature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = i32::from(self.0);
        write!(
            f,
            "{}{}.{:01}°C",
            if value < 0 { "-" } else { "" },
            value.abs() / 10,
            value.abs() % 10
        )
    }
}

/// Target range matches the inspected type-22 Android control screen (5–35°C).
/// This is client-protocol evidence, not physical testing of every setpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i16", into = "i16")]
pub struct TargetTemperature(i16);
impl TryFrom<i16> for TargetTemperature {
    type Error = &'static str;
    fn try_from(value: i16) -> Result<Self, Self::Error> {
        if !(50..=350).contains(&value) || value % 5 != 0 {
            Err("Target must be 50–350 tenths of °C, in steps of 5 (0.5°C).")
        } else {
            Ok(Self(value))
        }
    }
}
impl From<TargetTemperature> for i16 {
    fn from(value: TargetTemperature) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "raw", rename_all = "snake_case")]
pub enum Mode {
    Manual,
    Schedule,
    TemporaryOverride,
    Off,
    Unknown(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connectivity {
    Connecting,
    Online,
    Offline,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Sending,
    AcceptedByCloud,
    AwaitingDevice,
    Confirmed,
    TargetMatched,
    Rejected,
    TransportFailed,
    TimedOut,
    Unknown,
}
impl Phase {
    pub fn allows(self, next: Self) -> bool {
        use Phase::*;
        matches!(
            (self, next),
            (Queued, Sending)
                | (
                    Sending,
                    AcceptedByCloud | Rejected | TransportFailed | TimedOut
                )
                | (AcceptedByCloud, AwaitingDevice)
                | (TargetMatched, Confirmed)
                | (
                    AwaitingDevice,
                    Confirmed | TargetMatched | TimedOut | Unknown
                )
                | (
                    TransportFailed | TimedOut | Unknown,
                    Confirmed | TargetMatched
                )
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub id: u64,
    pub target: TargetTemperature,
    pub phase: Phase,
    /// Set only by an explicit successful upstream write response; survives later phases.
    #[serde(default)]
    pub server_accepted: bool,
    pub message: String,
}
impl Command {
    pub fn transition(&mut self, phase: Phase, message: &str) -> bool {
        if !self.phase.allows(phase) {
            return false;
        }
        if phase == Phase::AcceptedByCloud {
            self.server_accepted = true;
        }
        self.phase = phase;
        self.message = message.into();
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    /// Display name only; never the manufacturer device identifier.
    #[serde(default)]
    pub device_name: Option<String>,
    pub room_temperature: Temperature,
    /// Provider-observed target; OCSTAT supplies cloud status, not verified device state.
    /// Retrieval is not a freshness or command-correlation guarantee.
    pub target_temperature: Temperature,
    /// Cloud-requested target, separate from the reported value (OCSTAT only).
    #[serde(default)]
    pub requested_temperature: Option<Temperature>,
    pub online: bool,
    pub mode: Mode,
    /// Provider monotonic observation revision, not a wall-clock timestamp.
    pub revision: u64,
    /// Present ONLY when the provider has evidence correlating application to a command.
    pub applied_command: Option<u64>,
    /// Local attempt whose target matched two subsequent cloud observations.
    /// This is not device-origin command acknowledgement or a freshness guarantee.
    #[serde(default)]
    pub matched_command: Option<u64>,
    /// Provider supports target changes in this observed operating context.
    #[serde(default)]
    pub target_writable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginState {
    SigningOut,
    Required,
    SigningIn,
    Failed,
    Ready,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// Signed-in email for the account header, absent before login and after logout.
    pub account_email: Option<String>,
    pub session: String,
    pub revision: u64,
    pub provider: String,
    pub can_set_target: bool,
    /// Present for OCSTAT, absent for the simulator; never contains credentials.
    pub login: Option<LoginState>,
    pub read_error: Option<crate::provider::ProviderError>,
    pub connectivity: Connectivity,
    pub last_sync: Option<u64>,
    pub confirmed: Option<DeviceState>,
    pub desired: Option<TargetTemperature>,
    pub command: Option<Command>,
    pub in_flight: Option<Command>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_temperatures_and_validation() {
        assert_eq!(Temperature(215).to_string(), "21.5°C");
        assert_eq!(Temperature(-5).to_string(), "-0.5°C");
        for value in [50, 215, 300, 305, 345, 350] {
            assert!(TargetTemperature::try_from(value).is_ok());
        }
        for value in [-5, 49, 214, 301, 351, 355] {
            assert!(TargetTemperature::try_from(value).is_err());
        }
        assert!(serde_json::from_str::<TargetTemperature>("21.5").is_err());
        assert_eq!(
            serde_json::to_string(&TargetTemperature::try_from(215).unwrap()).unwrap(),
            "215"
        );
    }
    #[test]
    fn transitions_never_equate_acceptance_with_confirmation() {
        use Phase::*;
        assert!(Queued.allows(Sending));
        assert!(Sending.allows(AcceptedByCloud));
        assert!(!Sending.allows(Confirmed));
        assert!(!Rejected.allows(Confirmed));
        assert!(!Confirmed.allows(Sending));
        assert!(TimedOut.allows(Confirmed));
        assert!(TransportFailed.allows(Confirmed));
        assert!(AwaitingDevice.allows(Unknown));
    }
    #[test]
    fn server_acceptance_survives_later_phases_but_cannot_be_inferred_from_matches() {
        let mut command = Command {
            id: 1,
            target: 225.try_into().unwrap(),
            phase: Phase::Queued,
            server_accepted: false,
            message: String::new(),
        };
        command.transition(Phase::Sending, "sending");
        let mut lost_reply = command.clone();
        command.transition(Phase::AcceptedByCloud, "accepted");
        command.transition(Phase::AwaitingDevice, "waiting");
        command.transition(Phase::TargetMatched, "cloud values match");
        let restored: Command =
            serde_json::from_str(&serde_json::to_string(&command).unwrap()).unwrap();
        assert!(restored.server_accepted);
        lost_reply.transition(Phase::TransportFailed, "no reply");
        lost_reply.transition(Phase::TargetMatched, "cloud values match");
        assert!(!lost_reply.server_accepted);
    }
}
