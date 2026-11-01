use smartthermweb::sessions::Sessions;
use smartthermweb::{
    api::{AppState, router},
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
        return Err("This local build must bind to a loopback address.".into());
    }
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
                Some(Arc::new(Sessions::new())),
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
        router(AppState {
            controller,
            simulator,
            sessions,
        }),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    if let Some(worker) = worker {
        worker.abort();
    }
    Ok(())
}
