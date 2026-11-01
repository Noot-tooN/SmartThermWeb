pub mod ocstat;
pub mod simulator;

use crate::domain::{DeviceState, TargetTemperature};
use std::{future::Future, pin::Pin};

pub type ProviderFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, ProviderError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    Offline,
    Rejected,
    Transport,
    UnverifiedProtocol,
    Authentication,
    InvalidResponse,
    DeviceNotFound,
    ReadOnly,
}

/// Returning Ok from send_target means cloud acceptance, never device application.
/// Canceling this future cannot be assumed to cancel a real-world side effect.
pub trait Provider: Send + Sync {
    fn read(&self) -> ProviderFuture<'_, DeviceState>;
    fn send_target(&self, id: u64, target: TargetTemperature) -> ProviderFuture<'_, ()>;
}
