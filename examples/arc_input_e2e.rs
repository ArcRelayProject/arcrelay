//! Interactive Arc Input peer used by the two-machine end-to-end test.
//!
//! This example deliberately exercises the production runtime and native
//! platform adapters. It is not included in the packaged ArcRelay app.

#![allow(dead_code)]

#[path = "../src/arc_input/mod.rs"]
mod arc_input;
#[path = "../src/ipc.rs"]
mod ipc;

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use arcrelay_network::{
    DeviceMetadata, NetworkRuntime, NetworkRuntimeConfig, PairingGrantRequest, SqlitePeerRepository,
};
use arcrelay_peer::{CapabilityId, DeviceId, PeerRepository, ServiceInstanceId};

use arc_input::{ArcInputRuntime, ProductIdentity, ProductPaths};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(io::stderr)
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let paths = std::env::var_os("ARC_INPUT_E2E_ROOT")
        .map(|root| ProductPaths::from_root(root.into()))
        .unwrap_or(ProductPaths::default_for_user()?);
    let repository: Arc<dyn PeerRepository> =
        Arc::new(SqlitePeerRepository::open(&paths.data.join("peers.sqlite3")).await?);
    let network = NetworkRuntime::bind(NetworkRuntimeConfig::new(
        paths.data.join("identity"),
        DeviceMetadata {
            name: "Arc Input E2E".into(),
            platform: std::env::consts::OS.into(),
            model: "test-peer".into(),
        },
        repository,
    ))
    .await?;
    let network_cell = Arc::new(tokio::sync::OnceCell::new());
    assert!(network_cell.set(network.clone()).is_ok());
    let identity = Arc::new(ProductIdentity::from_device_id(
        network.device_id().to_string(),
    )?);
    let input = ArcInputRuntime::load(paths, identity, network_cell).await?;
    let background = tokio::spawn(
        input
            .clone()
            .run_with_reserved_incoming(network.subscribe()),
    );

    emit("ready", &input.snapshot())?;
    let (commands, mut lines) = tokio::sync::mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("arc-input-e2e-stdin".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(line) => {
                        if commands.send(line).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        eprintln!("stdin failed: {error}");
                        break;
                    }
                }
            }
        })?;

    while let Some(line) = lines.recv().await {
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else {
            continue;
        };
        let result = match command {
            "snapshot" => Ok(()),
            "pair" => {
                let peer = parse_peer(words.next())?;
                pair_device(&network, &peer).await.map_err(|error| {
                    arc_input::runtime::RuntimeError::InvalidInput(error.to_string())
                })?;
                let device_id = DeviceId::parse(peer.as_str()).map_err(|error| {
                    arc_input::runtime::RuntimeError::InvalidInput(error.to_string())
                })?;
                let advertisement = network.discovery().peer(&device_id).ok_or_else(|| {
                    arc_input::runtime::RuntimeError::InvalidInput(
                        "paired peer is no longer discovered".into(),
                    )
                })?;
                input
                    .connect_peer(peer, advertisement.addresses, advertisement.port)
                    .await
            }
            "enable" => input.set_input_sharing_enabled(true),
            "disable" => input.set_input_sharing_enabled(false),
            "take" => match input.set_input_sharing_enabled(true) {
                Ok(()) => input.take_control().await.map(|_| ()),
                Err(error) => Err(error),
            },
            "release" => input.release_control(),
            "quit" => {
                let _ = input.shutdown();
                background.abort();
                emit("quit", &input.snapshot())?;
                return Ok(());
            }
            other => {
                eprintln!("unknown command: {other}");
                continue;
            }
        };
        match result {
            Ok(()) => emit(command, &input.snapshot())?,
            Err(error) => {
                let value = serde_json::json!({
                    "event": "error",
                    "command": command,
                    "message": error.to_string(),
                    "snapshot": input.snapshot(),
                });
                println!("{}", serde_json::to_string(&value)?);
                io::stdout().flush()?;
            }
        }
    }

    let _ = input.release_control();
    background.abort();
    Ok(())
}

fn parse_peer(value: Option<&str>) -> Result<ServiceInstanceId, arc_input::runtime::RuntimeError> {
    value
        .ok_or_else(|| arc_input::runtime::RuntimeError::InvalidInput("missing peer id".into()))
        .and_then(|value| ServiceInstanceId::parse(value).map_err(Into::into))
}

async fn pair_device(
    network: &Arc<NetworkRuntime>,
    peer: &ServiceInstanceId,
) -> Result<(), Box<dyn std::error::Error>> {
    let device_id = DeviceId::parse(peer.as_str())?;
    if network
        .paired_peers()
        .await?
        .iter()
        .any(|stored| stored.device_id == device_id)
    {
        return Ok(());
    }
    let advertisement = network
        .discovery()
        .peer(&device_id)
        .ok_or("peer is not discovered")?;
    let pending = network
        .begin_pairing(
            &advertisement,
            [
                PairingGrantRequest::outbound(CapabilityId::CrossScreenInject),
                PairingGrantRequest::inbound(CapabilityId::CrossScreenInject),
            ],
        )
        .await?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "event": "pairingCode",
            "peer": peer,
            "code": pending.verification_code(),
        }))?
    );
    io::stdout().flush()?;
    network
        .complete_pairing(pending, std::time::Duration::from_secs(180))
        .await?;
    Ok(())
}

fn emit(
    event: &str,
    snapshot: &arc_input::runtime::RuntimeSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "event": event,
            "snapshot": snapshot,
        }))?
    );
    io::stdout().flush()?;
    Ok(())
}
