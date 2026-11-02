pub mod ocstat;
pub mod ocstat_schedule;
mod ocstat_schedule_tcp;
pub mod simulator;

use crate::domain::{DeviceState, TargetTemperature};
use std::{future::Future, pin::Pin};

pub type LogoutFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

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
    LoginRequired,
    MultipleDevices,
}

/// Returning Ok from send_target means cloud acceptance, never device application.
/// Canceling this future cannot be assumed to cancel a real-world side effect.
pub trait Provider: Send + Sync {
    /// Forget local account/session state; never contacts the manufacturer.
    fn logout(&self) -> LogoutFuture<'_> {
        Box::pin(async {})
    }
    /// Called only by the controller worker. Credentials must never be logged.
    fn login(&self, _email: String, _password: String) -> ProviderFuture<'_, DeviceState> {
        Box::pin(async { Err(ProviderError::ReadOnly) })
    }
    fn schedule_trial(
        &self,
        _action: ocstat::trial::Action,
    ) -> ProviderFuture<'_, ocstat::trial::Status> {
        Box::pin(async { Err(ProviderError::ReadOnly) })
    }
    fn read_schedule(&self) -> ProviderFuture<'_, crate::schedule::Schedule> {
        Box::pin(async { Err(ProviderError::ReadOnly) })
    }
    fn read_schedule_preview(&self) -> ProviderFuture<'_, crate::schedule::Preview> {
        Box::pin(async {
            self.read_schedule()
                .await?
                .preview()
                .map_err(|_| ProviderError::InvalidResponse)
        })
    }
    fn save_schedule(
        &self,
        _request: crate::schedule::SaveRequest,
    ) -> ProviderFuture<'_, crate::schedule::SaveResult> {
        Box::pin(async { Err(ProviderError::ReadOnly) })
    }
    fn read(&self) -> ProviderFuture<'_, DeviceState>;
    fn send_target(&self, id: u64, target: TargetTemperature) -> ProviderFuture<'_, ()>;
}
