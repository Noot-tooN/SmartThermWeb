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

/// Simulator bounds, NOT an assertion about the real thermostat's capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i16", into = "i16")]
pub struct TargetTemperature(i16);
impl TryFrom<i16> for TargetTemperature {
    type Error = &'static str;
    fn try_from(value: i16) -> Result<Self, Self::Error> {
        if !(50..=300).contains(&value) || value % 5 != 0 {
            Err("Target must be 50–300 tenths of °C, in steps of 5 (0.5°C).")
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
                | (AwaitingDevice, Confirmed | TimedOut | Unknown)
                | (TransportFailed | TimedOut | Unknown, Confirmed)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub id: u64,
    pub target: TargetTemperature,
    pub phase: Phase,
    pub message: String,
}
impl Command {
    pub fn transition(&mut self, phase: Phase, message: &str) -> bool {
        if !self.phase.allows(phase) {
            return false;
        }
        self.phase = phase;
        self.message = message.into();
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    pub room_temperature: Temperature,
    pub target_temperature: Temperature,
    pub online: bool,
    pub mode: Mode,
    /// Provider monotonic observation revision, not a wall-clock timestamp.
    pub revision: u64,
    /// Present ONLY when the provider has evidence correlating application to a command.
    pub applied_command: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub session: String,
    pub revision: u64,
    pub provider: String,
    pub can_set_target: bool,
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
        for value in [50, 215, 300] {
            assert!(TargetTemperature::try_from(value).is_ok());
        }
        for value in [-5, 49, 214, 301] {
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
}
