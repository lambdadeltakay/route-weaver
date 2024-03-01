mod listener;
mod p2p;

// mod application_streamer;

use clap::Parser;
use once_cell::sync::Lazy;
use p2p::P2PCommunicatorBuilder;
use route_weaver_common::{transport::TransportAddress, PrivateKey};
use route_weaver_common::{message::Message, PublicKey};
use simple_logger::SimpleLogger;
use snow::{Builder, HandshakeState};
use std::{net::{Ipv4Addr, Ipv6Addr}, time::Duration};
use tokio::time::sleep;

static NOISE_PROLOGUE: Lazy<String> =
    Lazy::new(|| format!("router-weaver edition {}", env!("CARGO_PKG_VERSION_MAJOR")));

fn create_noise_builder<'a>() -> Builder<'a> {
    Builder::new("Noise_XX_25519_ChaChaPoly_SHA256".parse().unwrap())
        .prologue(NOISE_PROLOGUE.as_bytes())
}

pub fn create_keypair() -> (PublicKey, PrivateKey) {
    let keypair = create_noise_builder().generate_keypair().unwrap();

    (
        PublicKey(keypair.public.try_into().unwrap()),
        PrivateKey(keypair.private.try_into().unwrap()),
    )
}

pub fn create_responder(key: &PrivateKey) -> HandshakeState {
    create_noise_builder()
        .local_private_key(&key.0)
        .build_responder()
        .unwrap()
}

pub fn create_initiator(key: &PrivateKey) -> HandshakeState {
    create_noise_builder()
        .local_private_key(&key.0)
        .build_initiator()
        .unwrap()
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[arg(long, default_value_t = true)]
    bluetooth_transport: bool,

    #[arg(long, default_value_t = true)]
    tcp_transport: bool,

    #[arg(long, default_value_t = true)]
    unix_transport: bool,

    #[arg(long)]
    seed_node: Ipv6Addr
}

#[tokio::main]
async fn main() {
    SimpleLogger::new().with_colors(true).init().unwrap();

    let (public_key, private_key) = create_keypair();

    let router = P2PCommunicatorBuilder::default()
        .add_public_key(public_key)
        .add_private_key(private_key.clone());

    #[cfg(feature = "tcp-transport")]
    let router = router.add_transport::<route_weaver_tcp_transport::TcpTransport>();
    #[cfg(all(feature = "bluetooth-transport", target_os = "linux"))]
    let router = router.add_transport::<route_weaver_bluetooth_transport::BluetoothTransport>();
    #[cfg(all(feature = "unix-transport", target_family = "unix"))]
    let router = router.add_transport::<route_weaver_unix_transport::UnixTransport>();

    let router = router.build().await;

    loop {
        router.send_message(public_key, Message::Handshake).await;

        sleep(Duration::from_secs(1)).await;
    }
}
