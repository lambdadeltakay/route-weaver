use std::fmt::Display;
use bincode::Options;
use message::SERIALIZED_PACKET_SIZE_MAX;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Write;
use zeroize::ZeroizeOnDrop;

pub mod message;
pub mod transport;

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, b| {
        let _ = write!(output, "{b:02x}");
        output
    })
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex_encode(&self.0))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct PrivateKey(pub [u8; 32]);

fn form_bincode_config() -> impl bincode::Options {
    bincode::DefaultOptions::default()
        .allow_trailing_bytes()
        .with_big_endian()
        .with_varint_encoding()
        .with_limit(SERIALIZED_PACKET_SIZE_MAX as u64)
}

pub fn wire_encode<T: Serialize>(to_encode: &T) -> Result<Vec<u8>, anyhow::Error> {
    Ok(form_bincode_config().serialize(to_encode)?)
}

pub fn wire_decode<T: DeserializeOwned>(to_decode: &[u8]) -> Result<T, anyhow::Error> {
    Ok(form_bincode_config().deserialize(to_decode)?)
}

pub fn wire_estimate_size<T: Serialize>(to_encode: &T) -> Result<usize, anyhow::Error> {
    Ok(form_bincode_config().serialized_size(to_encode)? as usize)
}
