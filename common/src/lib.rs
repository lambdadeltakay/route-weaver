use bincode::Options;
use message::SERIALIZED_PACKET_SIZE_MAX;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Serialize};

pub mod address;
pub mod message;
pub mod noise;
pub mod transport;
pub mod router;

// Rust doesn't have autotyping for statics moment
#[allow(clippy::type_complexity)]
pub static BINCODE_CONFIG: Lazy<
    bincode::config::WithOtherLimit<
        bincode::config::WithOtherIntEncoding<
            bincode::config::WithOtherEndian<
                bincode::config::WithOtherTrailing<
                    bincode::DefaultOptions,
                    bincode::config::AllowTrailing,
                >,
                bincode::config::BigEndian,
            >,
            bincode::config::VarintEncoding,
        >,
        bincode::config::Bounded,
    >,
> = Lazy::new(|| {
    bincode::DefaultOptions::default()
        .allow_trailing_bytes()
        .with_big_endian()
        .with_varint_encoding()
        .with_limit(SERIALIZED_PACKET_SIZE_MAX as u64)
});

pub fn wire_encode<T: Serialize>(to_encode: &T) -> Result<Vec<u8>, anyhow::Error> {
    Ok(BINCODE_CONFIG.serialize(to_encode)?)
}

pub fn wire_decode<T: DeserializeOwned>(to_decode: &[u8]) -> Result<T, anyhow::Error> {
    Ok(BINCODE_CONFIG.deserialize(to_decode)?)
}

pub fn wire_measure_size<T: Serialize>(to_encode: &T) -> Result<usize, anyhow::Error> {
    Ok(BINCODE_CONFIG.serialized_size(to_encode)? as usize)
}
