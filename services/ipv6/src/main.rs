use std::{net::Ipv6Addr, sync::Arc, time::Duration};

use ipnet::Ipv6Net;
use once_cell::sync::Lazy;
use route_weaver_common::noise::PublicKey;
use tokio::time::sleep;

static IPRANGE: Lazy<Ipv6Net> = Lazy::new(|| {
    Ipv6Net::new(
        [
            0xfd, 0x34, 0x34, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ]
        .into(),
        104,
    )
    .unwrap()
});

// This is actually relatively safe
pub fn ip_from_public_key(public_key: PublicKey) -> Ipv6Addr {
    let section = &public_key.0[..IPRANGE.prefix_len() as usize / 8];
    let mut ip_bytes = [0; 16];
    ip_bytes[0..2].copy_from_slice(&IPRANGE.network().octets()[0..2]);
    ip_bytes[2..].copy_from_slice(section);
    Ipv6Addr::from(ip_bytes)
}

#[tokio::main]
async fn main() {
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}
