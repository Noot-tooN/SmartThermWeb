use smartthermweb::{
    api::{AppState, router},
    controller::{Controller, Timing},
    provider::simulator::{Config, Simulator},
};
use std::{net::SocketAddr, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = std::env::var("SMARTTHERM_PROVIDER").unwrap_or_else(|_| "simulator".into());
    if provider != "simulator" {
        return Err("Only the simulator is enabled. OCSTAT requires verified protocol evidence and explicit live-write approval; see docs/ocstat-protocol.md.".into());
    }
    let address: SocketAddr = std::env::var("SMARTTHERM_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    if !address.ip().is_loopback() {
        return Err("This unauthenticated MVP must bind to a loopback address.".into());
    }
    let config = match std::env::var("SMARTTHERM_SIM_CONFIG") {
        Ok(value) => serde_json::from_str::<Config>(&value)?,
        Err(_) => Config::default(),
    };
    if !config.validate() {
        return Err("Invalid simulator timing; see .env.example.".into());
    }
    let simulator = Arc::new(Simulator::new(config));
    let controller = Controller::new();
    let listener = tokio::net::TcpListener::bind(address).await?;
    let worker = tokio::spawn(controller.clone().run(simulator.clone(), Timing::default()));
    println!("SmartThermWeb simulator: http://{}", listener.local_addr()?);
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
