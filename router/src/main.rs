mod connection_manager;
mod gateway;
mod p2p;

// mod application_streamer;

use clap::Parser;
use log::LevelFilter;
use p2p::P2PCommunicatorBuilder;
use route_weaver_common::{
    address::TransportAddress,
    message::PeerToPeerMessage,
    noise::{PrivateKey, PublicKey},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tokio::{fs::read_to_string, time::sleep};

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct MainRouterConfig {
    #[serde(default)]
    enabled_transports: HashSet<String>,
    #[serde_as(as = "DisplayFromStr")]
    private_key: PrivateKey,
    #[serde_as(as = "DisplayFromStr")]
    public_key: PublicKey,
    seed_node: TransportAddress,
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[arg(short, long)]
    pub config_path: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Arguments::parse();
    let config_path = args.config_path;
    let config = read_to_string(config_path).await.unwrap();
    let config: MainRouterConfig = toml::from_str(&config).expect("Failed to parse router config");

    let public_key_string = config.public_key.to_string();

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {} {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                public_key_string,
                message
            ))
        })
        .level(log::LevelFilter::Trace)
        .chain(std::io::stdout())
        .apply()
        .unwrap();

    let mut router = P2PCommunicatorBuilder::default()
        .add_public_key(config.public_key)
        .add_private_key(config.private_key)
        .add_seed_node(config.seed_node);

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
    let remote = "1B86836D92CF67B3B9146827CEEB06B8EF8F3607DE5F02C67E095F2D19A691DB"
        .parse()
        .unwrap();

    loop {
        let router = router.clone();

        if config.public_key != remote {
            router
                .send_message(remote, PeerToPeerMessage::Handshake)
                .await;
        }

        sleep(Duration::from_secs(1)).await;
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
