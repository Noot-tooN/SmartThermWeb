use super::{Provider, ProviderError, ProviderFuture};
use crate::domain::{DeviceState, Mode, TargetTemperature, Temperature};
use serde::{Deserialize, Serialize};
use std::{sync::Mutex, time::Duration};
use tokio::time::{Instant, sleep};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    #[default]
    Normal,
    Rejected,
    TransportFailure,
    ReplyLost,
    AckTimeout,
    DeviceTimeout,
    Offline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub scenario: Scenario,
    pub ack_ms: u64,
    pub apply_ms: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            scenario: Scenario::Normal,
            ack_ms: 900,
            apply_ms: 2400,
        }
    }
}
impl Config {
    pub fn validate(&self) -> bool {
        self.ack_ms <= 30_000 && self.apply_ms <= 60_000
    }
}
struct Pending {
    due: Instant,
    id: u64,
    target: TargetTemperature,
}
struct Inner {
    config: Config,
    device: DeviceState,
    pending: Vec<Pending>,
}
pub struct Simulator {
    inner: Mutex<Inner>,
}
impl Simulator {
    pub fn new(config: Config) -> Self {
        Self {
            inner: Mutex::new(Inner {
                config,
                device: DeviceState {
                    room_temperature: Temperature(208),
                    target_temperature: Temperature(215),
                    online: true,
                    mode: Mode::Manual,
                    revision: 1,
                    applied_command: None,
                },
                pending: vec![],
            }),
        }
    }
    pub fn config(&self) -> Config {
        self.inner.lock().unwrap().config.clone()
    }
    pub fn configure(&self, config: Config) {
        self.inner.lock().unwrap().config = config;
    }
}
impl Provider for Simulator {
    fn read(&self) -> ProviderFuture<'_, DeviceState> {
        Box::pin(async move {
            let mut inner = self.inner.lock().unwrap();
            if inner.config.scenario == Scenario::Offline {
                return Err(ProviderError::Offline);
            }
            // Simulated device application continues even if the HTTP reply was lost/canceled.
            let mut due = Vec::new();
            inner.pending.retain(|p| {
                if p.due <= Instant::now() {
                    due.push((p.id, p.target));
                    false
                } else {
                    true
                }
            });
            due.sort_by_key(|(id, _)| *id);
            for (id, target) in due {
                if inner.device.applied_command.is_none_or(|last| id > last) {
                    inner.device.target_temperature = Temperature(target.into());
                    inner.device.applied_command = Some(id);
                    inner.device.revision += 1;
                }
            }
            Ok(inner.device.clone())
        })
    }
    fn send_target(&self, id: u64, target: TargetTemperature) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            let config = {
                let mut inner = self.inner.lock().unwrap();
                let config = inner.config.clone();
                if config.scenario == Scenario::Offline {
                    return Err(ProviderError::Offline);
                }
                if matches!(
                    config.scenario,
                    Scenario::Normal | Scenario::ReplyLost | Scenario::AckTimeout
                ) {
                    inner.pending.push(Pending {
                        due: Instant::now() + Duration::from_millis(config.apply_ms),
                        id,
                        target,
                    });
                }
                config
            };
            if config.scenario == Scenario::AckTimeout {
                std::future::pending::<()>().await;
            }
            sleep(Duration::from_millis(config.ack_ms)).await;
            match config.scenario {
                Scenario::Rejected => Err(ProviderError::Rejected),
                Scenario::TransportFailure | Scenario::ReplyLost => Err(ProviderError::Transport),
                _ => Ok(()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn late_old_application_cannot_overwrite_newer_device_revision() {
        let provider = Simulator::new(Config {
            ack_ms: 0,
            apply_ms: 100,
            ..Config::default()
        });
        provider
            .send_target(1, 220.try_into().unwrap())
            .await
            .unwrap();
        provider.configure(Config {
            ack_ms: 0,
            apply_ms: 10,
            ..Config::default()
        });
        provider
            .send_target(2, 230.try_into().unwrap())
            .await
            .unwrap();
        tokio::time::advance(Duration::from_millis(20)).await;
        let newer = provider.read().await.unwrap();
        assert_eq!(newer.applied_command, Some(2));
        tokio::time::advance(Duration::from_millis(100)).await;
        assert_eq!(provider.read().await.unwrap(), newer);
    }
}
