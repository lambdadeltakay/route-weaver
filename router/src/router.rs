use std::{collections::HashSet, fs::read_to_string, path::PathBuf, pin::Pin, sync::Arc};

use crate::p2p::{P2PCommunicator, P2PCommunicatorBuilder};
use route_weaver_common::{
    noise::{PrivateKey, PublicKey},
    router::{RouterBoundMessage, ClientBoundMessage},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use serde_with::DisplayFromStr;

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

pub struct MainRouter {
    p2p: Arc<P2PCommunicator>,
}

impl MainRouter {
    pub async fn handle_request(&self, request: RouterBoundMessage) -> ClientBoundMessage {
        match request {
            RouterBoundMessage::RegisterApplication { application_id } => todo!(),
        }
    }

    pub async fn new(config_path: impl Into<PathBuf>) -> Self {
        let config: MainRouterConfig =
            toml::from_str(&read_to_string(config_path.into()).unwrap()).unwrap();

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

        Self { p2p: router }
    }
}
