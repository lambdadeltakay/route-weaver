use arrayvec::ArrayVec;
use bincode::Options;
use once_cell::sync::Lazy;
use route_weaver_common::{
    address::TransportAddress, error::RouteWeaverError, noise::PublicKey, router::ApplicationId,
    transport::TransportConnection,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{mem::size_of, pin::Pin};
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::{
    bytes::Buf,
    codec::{Decoder, Encoder, FramedRead, FramedWrite},
};

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PeerToPeerMessage {
    /// Only valid message for handshakes before encryption except for the next one
    /// Valid response messages: Handshake
    Handshake,
    /// Generic response message
    Confirm,
    /// Asking if its ok to transmit some data over
    /// Valid response messages: OkToReceiveApplicationData
    StartApplicationData {
        id: ApplicationId,
    },
    /// It's ok to send data as we have a internal plugin listening for this application
    OkToReceiveApplicationData {
        id: ApplicationId,
    },
    /// It's not ok to start a application data stream
    ApplicationDataRefused {
        id: ApplicationId,
    },
    /// The application data coming along
    /// Valid response messages: Confirm
    ApplicationData {
        data: Vec<u8>,
        index: u32,
    },
    /// End the stream giving a blake2 hash of the data sent in ApplicationData chunks
    /// Valid response messages: Confirm, ApplicationDataMissing
    EndApplicationData {
        hash: [u8; 32],
        send_count: u32,
    },
    ApplicationDataMissing {
        missing_indexes: Vec<u32>,
    },
    RequestPeerList,
    PeerList {
        peer_list: Box<ArrayVec<TransportAddress, 10>>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
/// What sort of pre encryption transformation we used
/// This also inadvertantly creates a magic byte for the packet filtering out a lot of totally random messages
pub enum PreEncryptionTransformation {
    Plain,
    Lz4,
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash)]
pub struct RouteWeaverPacket {
    pub source: PublicKey,
    pub destination: PublicKey,
    pub pre_encryption_transformation: PreEncryptionTransformation,
    pub message: Vec<u8>,
}

// We allow a little padding to try to account for if the message is compressed
pub const MAX_NOISE_MESSAGE_LENGTH: usize = u16::MAX as usize - 128;

/// I have no clue how noise actually looks like in binary format so we will do this
pub const SERIALIZED_PACKET_SIZE_MAX: usize = (size_of::<PublicKey>() * 2) + u16::MAX as usize * 2;

#[derive(Default, Debug)]
pub struct PacketEncoderDecoder {
    working_buffer: Vec<u8>,
}

impl Decoder for PacketEncoderDecoder {
    type Item = RouteWeaverPacket;
    type Error = RouteWeaverError;

    fn decode(
        &mut self,
        src: &mut tokio_util::bytes::BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        src.reserve(SERIALIZED_PACKET_SIZE_MAX.saturating_sub(src.len()));

        // On purpose our packets don't have any magic bytes and in its bincode serialized format before decryption
        // Only around 2 fields could be incorrect
        // I've practiced feeding it urandom however and not many totally random (ie corrupted) packets get through

        match wire_decode::<RouteWeaverPacket>(src) {
            Ok(packet) => {
                let size = wire_measure_size(&packet)
                    .map_err(|_| RouteWeaverError::PacketDecodingError)?;
                src.advance(size);

                // Reject packets with empty messages
                if packet.message.is_empty() {
                    log::warn!(
                        "Received an empty packet from {} to {}, discarding it",
                        packet.source,
                        packet.destination
                    );
                    return Ok(None);
                }

                log::trace!("Found a packet of size {}", size);
                log::trace!(
                    "This packet claims to be from {} and going to {}",
                    packet.source,
                    packet.destination
                );
                log::trace!(
                    "The message is {} bytes long and claims to have pre encryption transformation {:?}", 
                    packet.message.len(),
                    packet.pre_encryption_transformation
                );

                Ok(Some(packet))
            }
            Err(_) => {
                if src.len() > SERIALIZED_PACKET_SIZE_MAX {
                    // Drop the rest of the buffer
                    src.advance(SERIALIZED_PACKET_SIZE_MAX.min(src.len()));
                    Ok(None)
                } else {
                    // Packet could be valid but it just hasn't had enough coming in to be deserializable yet
                    Ok(None)
                }
            }
        }
    }
}

impl Encoder<RouteWeaverPacket> for PacketEncoderDecoder {
    type Error = RouteWeaverError;

    fn encode(
        &mut self,
        item: RouteWeaverPacket,
        dst: &mut tokio_util::bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        self.working_buffer.fill(0);
        let len =
            wire_encode(&mut self.working_buffer, &item).expect("Packet encoding should not fail");
        dst.extend_from_slice(&self.working_buffer[..len]);

        Ok(())
    }
}

pub type TransportConnectionWriteFramer =
    FramedWrite<WriteHalf<Pin<Box<dyn TransportConnection>>>, PacketEncoderDecoder>;

pub type TransportConnectionReadFramer =
    FramedRead<ReadHalf<Pin<Box<dyn TransportConnection>>>, PacketEncoderDecoder>;