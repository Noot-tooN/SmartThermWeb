use smartthermweb::sessions::Sessions;
use smartthermweb::{
    api::{AppState, router_with_browser_access},
    controller::{Controller, Timing},
    provider::{
        Provider,
        simulator::{Config, Simulator},
    },
};
use std::{net::SocketAddr, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider_name = std::env::var("SMARTTHERM_PROVIDER").unwrap_or_else(|_| "ocstat".into());
    let address: SocketAddr = std::env::var("SMARTTHERM_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    if !address.ip().is_loopback() {
        return Err("Bind to loopback and use an HTTPS reverse proxy for remote access.".into());
    }
    let access = std::env::var("SMARTTHERM_PUBLIC_ORIGIN")
        .ok()
        .map(|origin| smartthermweb::browser_access::BrowserAccess::https(&origin))
        .transpose()?
        .unwrap_or_default()
        .with_base_path(&std::env::var("SMARTTHERM_BASE_PATH").unwrap_or_default())?;
    if access.secure_cookie() && provider_name != "ocstat" {
        return Err("Hosted mode requires the authenticated OCSTAT provider.".into());
    }
    let trial = std::env::var_os("SMARTTHERM_SCHEDULE_TRIAL_DIR")
        .map(|directory| {
            smartthermweb::provider::ocstat::trial::TrialStore::open(std::path::Path::new(
                &directory,
            ))
            .map(Arc::new)
        })
        .transpose()?;
    let trial_enabled = trial.is_some();
    let (provider, simulator, controller, sessions): (Option<Arc<dyn Provider>>, _, _, _) =
        match provider_name.as_str() {
            "simulator" => {
                let config = match std::env::var("SMARTTHERM_SIM_CONFIG") {
                    Ok(value) => serde_json::from_str::<Config>(&value)?,
                    Err(_) => Config::default(),
                };
                if !config.validate() {
                    return Err("Invalid simulator timing; see .env.example.".into());
                }
                let simulator = Arc::new(Simulator::new(config));
                (
                    Some(simulator.clone()),
                    Some(simulator),
                    Controller::new(),
                    None,
                )
            }
            "ocstat" => (
                None,
                None,
                Controller::ocstat_read_only(),
                Some(Arc::new(Sessions::with_factory(Arc::new(move || {
                    let provider = smartthermweb::provider::ocstat::Ocstat::new()
                        .map_err(|_| smartthermweb::provider::ProviderError::Transport)?;
                    Ok(Arc::new(match &trial {
                        Some(store) => provider.with_schedule_trial(store.clone()),
                        None => provider,
                    }))
                })))),
            ),
            _ => return Err("SMARTTHERM_PROVIDER must be simulator or ocstat.".into()),
        };
    let listener = tokio::net::TcpListener::bind(address).await?;
    let worker =
        provider.map(|provider| tokio::spawn(controller.clone().run(provider, Timing::default())));
    println!(
        "SmartThermWeb {provider_name}: http://{}",
        listener.local_addr()?
    );
    axum::serve(
        listener,
        router_with_browser_access(
            AppState {
                controller,
                simulator,
                sessions,
            },
            trial_enabled,
            access,
        ),
    )
    .with_graceful_shutdown(async {
        #[cfg(unix)]
        {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("install SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
        }
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    if let Some(worker) = worker {
        worker.abort();
    }
    Ok(())
}
