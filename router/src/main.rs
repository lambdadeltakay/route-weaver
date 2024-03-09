mod message_socket;
mod noise;
mod p2p;

use clap::Parser;
use log::LevelFilter;
use p2p::P2PCommunicatorBuilder;
use route_weaver_common::{
    address::TransportAddress,
    noise::{PrivateKey, PublicKey},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tokio::{
    fs::{read_to_string, remove_file},
    net::UnixListener,
    time::sleep,
};

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct MainRouterConfig {
    #[serde(default)]
    enabled_transports: HashSet<String>,
    #[serde_as(as = "DisplayFromStr")]
    private_key: PrivateKey,
    #[serde_as(as = "DisplayFromStr")]
    public_key: PublicKey,
    #[serde_as(as = "Option<DisplayFromStr>")]
    seed_node: Option<TransportAddress>,
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[arg(short, long)]
    pub config_path: PathBuf,
}

#[tokio::main]
async fn main() {
    let console_appender = log4rs::append::console::ConsoleAppender::builder().build();
    let config = log4rs::Config::builder()
        .appender(log4rs::config::Appender::builder().build("console", Box::new(console_appender)))
        .build(
            log4rs::config::Root::builder()
                .appender("console")
                .build(LevelFilter::Info),
        )
        .unwrap();

    log4rs::init_config(config).unwrap();

    let args = Arguments::parse();
    let config_path = args.config_path;
    let config = read_to_string(config_path).await.unwrap();
    let config: MainRouterConfig = toml::from_str(&config).expect("Failed to parse router config");

    let mut router = P2PCommunicatorBuilder::default()
        .add_public_key(config.public_key)
        .add_private_key(config.private_key);

    if let Some(seed_node) = config.seed_node {
        router = router.add_seed_node(seed_node);
    }

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
    let remote: PublicKey = "1B86836D92CF67B3B9146827CEEB06B8EF8F3607DE5F02C67E095F2D19A691DB"
        .parse()
        .unwrap();

    sleep(Duration::from_secs(1000)).await;

    let tmpdir = std::env::temp_dir();
    let socket_path = tmpdir.join(env!("CARGO_CRATE_NAME"));
    let _ = remove_file(socket_path.clone()).await;
    let socket = UnixListener::bind(socket_path).unwrap();

    while let Ok((stream, _)) = socket.accept().await {
        let router = router.clone();
        tokio::spawn(async move {});
    }
}
