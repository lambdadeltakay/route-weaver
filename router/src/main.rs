mod gateway;
mod p2p;

// mod application_streamer;

use clap::Parser;
use log::LevelFilter;
use p2p::P2PCommunicatorBuilder;
use route_weaver_common::noise::{PrivateKey, PublicKey};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use simple_logger::SimpleLogger;
use std::{collections::HashSet, path::PathBuf};
use tokio::fs::read_to_string;

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct MainRouterConfig {
    #[serde(default)]
    enabled_transports: HashSet<String>,
    #[serde_as(as = "DisplayFromStr")]
    private_key: PrivateKey,
    #[serde_as(as = "DisplayFromStr")]
    public_key: PublicKey,
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[arg(short, long)]
    pub config_path: PathBuf,
}

#[tokio::main]
async fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Trace)
        // Shut up tokio
        .with_module_level("tracing", LevelFilter::Info)
        .with_module_level("runtime", LevelFilter::Info)
        .with_module_level("tokio", LevelFilter::Info)
        .with_colors(true)
        .init()
        .unwrap();

    // console_subscriber::init();

    let config: MainRouterConfig =
        toml::from_str(&read_to_string("router.toml").await.unwrap()).unwrap();

    let mut router = P2PCommunicatorBuilder::default()
        .add_public_key(config.public_key)
        .add_private_key(config.private_key);

    #[cfg(feature = "tcp-transport")]
    if config.enabled_transports.contains("tcp") {
        router = router.add_transport::<route_weaver_tcp_transport::TcpTransport>();
    }

    #[cfg(all(feature = "bluetooth-transport", target_os = "linux"))]
    if config.enabled_transports.contains("bluetooth") {
        router = router.add_transport::<route_weaver_bluetooth_transport::BluetoothTransport>();
    }

    #[cfg(all(feature = "unix-transport", target_family = "unix"))]
    if config.enabled_transports.contains("unix") {
        router = router.add_transport::<route_weaver_unix_transport::UnixTransport>();
    }

    let router = router.build().await;

    loop {
        let router = router.clone();
        let sample_key = PublicKey([0; 32]);

        let message = router.receive_message(sample_key).await;
        router.send_message(sample_key, message).await;
    }

    /*

    let tmpdir = std::env::temp_dir();
    let socket_path = tmpdir.join(env!("CARGO_CRATE_NAME"));
    let _ = remove_file(socket_path.clone()).await;
    let socket = UnixListener::bind(socket_path).unwrap();

    loop {
        let router = router.clone();

        if let Ok((stream, _)) = socket.accept().await {
            tokio::spawn(async move {
                let service = ServiceBuilder::new()
                    .buffer(10)
                    .service_fn(move |from_client| {
                        let router = router.clone();
                        async move {
                            router
                                .send_message(config.public_key, PeerToPeerMessage::Handshake)
                                .await;

                            anyhow::Ok(ClientBoundMessage::ApplicationRegistered {
                                application_id: "ping".into(),
                            })
                        }
                    });

                let bincode =
                    AsyncBincodeStream::<_, RouterBoundMessage, ClientBoundMessage, _>::from(
                        stream,
                    )
                    .for_async();

                let server = Server::new(bincode, service);

                server.await
            });
        }
    }
             */
}
