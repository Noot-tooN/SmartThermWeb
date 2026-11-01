use smartthermweb::{
    api::{AppState, router},
    controller::{Controller, Timing},
    provider::{
        Provider,
        ocstat::{Credentials, Ocstat},
        simulator::{Config, Simulator},
    },
};
use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider_name = std::env::var("SMARTTHERM_PROVIDER").unwrap_or_else(|_| "simulator".into());
    let address: SocketAddr = std::env::var("SMARTTHERM_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    if !address.ip().is_loopback() {
        return Err("This unauthenticated MVP must bind to a loopback address.".into());
    }
    let (provider, simulator, controller, timing): (Arc<dyn Provider>, _, _, _) =
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
                    simulator.clone(),
                    Some(simulator),
                    Controller::new(),
                    Timing::default(),
                )
            }
            "ocstat" => {
                let path = std::env::var("SMARTTHERM_OCSTAT_CONFIG_FILE")
                .map_err(|_| "Set SMARTTHERM_OCSTAT_CONFIG_FILE to a private server-side credential file.")?;
                let credentials = Credentials::from_file(Path::new(&path))?;
                let provider =
                    Ocstat::new(credentials).map_err(|_| "Cannot initialize HTTPS client.")?;
                (
                    Arc::new(provider),
                    None,
                    Controller::ocstat_read_only(),
                    Timing {
                        request: Duration::from_secs(25),
                        poll: Duration::from_secs(30),
                        ..Timing::default()
                    },
                )
            }
            _ => return Err("SMARTTHERM_PROVIDER must be simulator or ocstat.".into()),
        };
    let listener = tokio::net::TcpListener::bind(address).await?;
    let worker = tokio::spawn(controller.clone().run(provider, timing));
    println!(
        "SmartThermWeb {provider_name}: http://{}",
        listener.local_addr()?
    );
    axum::serve(
        listener,
        router(AppState {
            controller,
            simulator,
        }),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    worker.abort();
    Ok(())
}
