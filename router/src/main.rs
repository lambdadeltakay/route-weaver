mod gateway;
mod p2p;
mod router;

// mod application_streamer;

use anyhow::Ok;
use async_bincode::tokio::AsyncBincodeStream;
use clap::Parser;
use futures::prelude::sink::SinkExt;
use route_weaver_common::router::{RouterBoundMessage, ClientBoundMessage};
use router::MainRouter;
use simple_logger::SimpleLogger;
use snow::{Builder, HandshakeState};
use std::{env::temp_dir, net::Ipv6Addr, path::PathBuf, time::Duration};
use tokio::{net::UnixListener, time::sleep};
use tokio_tower::pipeline::Server;
use tower::ServiceBuilder;

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[arg(short, long)]
    pub config_path: PathBuf,
}

#[tokio::main]
async fn main() {
    SimpleLogger::new().with_colors(true).init().unwrap();

    let tmpdir = std::env::temp_dir();
    let socket_path = tmpdir.join(env!("CARGO_CRATE_NAME"));
    let socket = UnixListener::bind(socket_path).unwrap();

    let router = MainRouter::new("router.toml").await;

    let service = ServiceBuilder::new()
        .buffer(1000)
        .service_fn(|from_client| async move {
            Ok(ClientBoundMessage::ApplicationRegistered {
                application_id: "ping".into(),
            })
        });

    let (stream, _) = socket.accept().await.unwrap();
    tokio::spawn(async move {
        let bincode =
            AsyncBincodeStream::<_, RouterBoundMessage, ClientBoundMessage, _>::from(stream)
                .for_async();

        let server = Server::new(bincode, service);

        server.await
    });
}
