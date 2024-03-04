use bincode::Options;
use message::SERIALIZED_PACKET_SIZE_MAX;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Serialize};

pub mod address;
pub mod error;
pub mod message;
pub mod noise;
pub mod router;
pub mod transport;

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
            bincode::config::FixintEncoding,
        >,
        bincode::config::Bounded,
    >,
> = Lazy::new(|| {
    bincode::DefaultOptions::default()
        .allow_trailing_bytes()
        .with_big_endian()
        .with_fixint_encoding()
        .with_limit(SERIALIZED_PACKET_SIZE_MAX as u64)
});

#[inline]
pub fn wire_encode<T: Serialize>(
    buffer: &mut Vec<u8>,
    to_encode: &T,
) -> Result<usize, bincode::Error> {
    let len = wire_measure_size(to_encode)?;
    buffer.reserve(len.saturating_sub(buffer.len()));
    BINCODE_CONFIG.serialize_into(buffer, to_encode)?;
    Ok(len)
}

#[inline]
pub fn wire_decode<T: DeserializeOwned>(to_decode: &[u8]) -> Result<T, bincode::Error> {
    BINCODE_CONFIG.deserialize(to_decode)
}

#[inline]
pub fn wire_measure_size<T: Serialize>(to_encode: &T) -> Result<usize, bincode::Error> {
    Ok(BINCODE_CONFIG.serialized_size(to_encode)? as usize)
}
