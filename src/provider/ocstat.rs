//! Evidence-limited adapter. No network requests are made by this module.
//! See docs/ocstat-protocol.md before implementing any decoder or enabling writes.
use super::{Provider, ProviderError, ProviderFuture};
use crate::domain::{DeviceState, TargetTemperature};
use serde::Serialize;
use std::time::Duration;

/// No Debug implementation: this object may eventually own sensitive credentials.
pub struct Ocstat {
    _client: reqwest::Client,
}
impl Ocstat {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            _client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
}

/// Known observed form field names only; values and mode semantics need evidence.
/// Never log this structure, persist it, or derive Debug.
#[derive(Serialize)]
pub struct ObservedSetpointForm<'a> {
    pub userid_phoneid: &'a str,
    pub device_id: &'a str,
    pub user_id: &'a str,
    pub current_temprature: i16,
    #[serde(rename = "messageId")]
    pub message_id: &'a str,
    pub work_mode: &'a str,
    pub token: &'a str,
}

impl Provider for Ocstat {
    fn read(&self) -> ProviderFuture<'_, DeviceState> {
        Box::pin(async { Err(ProviderError::UnverifiedProtocol) })
    }
    fn send_target(&self, _id: u64, _target: TargetTemperature) -> ProviderFuture<'_, ()> {
        // Fail closed even if a caller accidentally constructs this provider.
        Box::pin(async { Err(ProviderError::UnverifiedProtocol) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_writes_are_disabled() {
        let provider = Ocstat::new().unwrap();
        assert_eq!(
            provider
                .send_target(1, TargetTemperature::try_from(220).unwrap())
                .await,
            Err(ProviderError::UnverifiedProtocol)
        );
        assert_eq!(
            provider.read().await,
            Err(ProviderError::UnverifiedProtocol)
        );
    }
    #[test]
    fn observed_spelling_is_preserved() {
        let form = ObservedSetpointForm {
            userid_phoneid: "REDACTED",
            device_id: "REDACTED",
            user_id: "REDACTED",
            current_temprature: 215,
            message_id: "REDACTED",
            work_mode: "UNKNOWN",
            token: "REDACTED",
        };
        let value = serde_json::to_value(form).unwrap();
        assert_eq!(value["current_temprature"], 215);
        assert!(value.get("messageId").is_some());
    }
}
