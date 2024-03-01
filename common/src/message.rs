use std::{mem::size_of, pin::Pin};
use either::{for_both, Either};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::{HandshakeState, TransportState};
use tokio_util::{
    bytes::Buf,
    codec::{Decoder, Encoder, FramedRead, FramedWrite},
};
use zeroize::ZeroizeOnDrop;

use crate::{
    transport::{TransportConnectionReadHalf, TransportConnectionWriteHalf},
    wire_decode, wire_encode, wire_estimate_size, PublicKey,
};

#[derive(Clone, Debug, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct ApplicationId(pub [u8; 32]);

impl From<&str> for ApplicationId {
    fn from(value: &str) -> Self {
        ApplicationId(Sha256::digest(value.as_bytes()).into())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, ZeroizeOnDrop)]
pub enum Message {
    /// Only valid message for handshakes before encryption except for the next one
    /// Valid response messages: Handshake
    Handshake,
    /// Generic response message
    Confirm,
    /// Ping
    /// Valid response messages: Pong
    Ping,
    /// Ping response
    Pong,
    /// Asking if its ok to transmit some data over
    /// Valid response messages: OkToReceiveApplicationData
    StartApplicationData {
        id: ApplicationId,
    },
    /// It's ok to send data as we have a internal plugin listening for this application
    OkToReceiveApplicationData {
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
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum MessageWrapper {
    Plain(Vec<u8>),
    Lz4(Vec<u8>),
}

impl MessageWrapper {
    pub fn encode(
        message: Message,
        noise: Either<&mut HandshakeState, &mut TransportState>,
    ) -> Self {
        let message = wire_encode(&message).unwrap();
        let lz4ed = lz4_flex::compress_prepend_size(&message);

        let (did_we_compress, to_encrypt) = if lz4ed.len() < message.len() {
            log::trace!(
                "lz4 compression chosen for message, saving {} bytes",
                message.len() - lz4ed.len()
            );
            (true, lz4ed)
        } else {
            log::trace!("No compression chosen for message");
            (false, message)
        };

        let mut encryption_buffer = vec![0; u16::MAX as usize];
        let len = for_both!(noise, noise => noise.write_message(&to_encrypt, &mut encryption_buffer).unwrap());

        if did_we_compress {
            Self::Lz4(encryption_buffer[..len].to_vec())
        } else {
            Self::Plain(encryption_buffer[..len].to_vec())
        }
    }

    pub fn decode(
        &self,
        noise: Either<&mut HandshakeState, &mut TransportState>,
    ) -> Result<Message, anyhow::Error> {
        let mut decryption_buffer = vec![0; u16::MAX as usize];

        match self {
            MessageWrapper::Plain(message) => {
                let len =
                    for_both!(noise, noise => noise.read_message(message, &mut decryption_buffer)?);
                Ok(wire_decode(&decryption_buffer[..len])?)
            }
            MessageWrapper::Lz4(message) => {
                let len = for_both!(noise, noise => noise.read_message(
                    &lz4_flex::decompress_size_prepended(message)?, &mut decryption_buffer)?);
                Ok(wire_decode(&decryption_buffer[..len])?)
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct RouteWeaverPacket {
    pub source: PublicKey,
    pub destination: PublicKey,
    pub message: MessageWrapper,
}

impl RouteWeaverPacket {
    pub fn new(
        source: PublicKey,
        destination: PublicKey,
        noise: Either<&mut HandshakeState, &mut TransportState>,
        message: Message,
    ) -> Self {
        let compressed_message = MessageWrapper::encode(message, noise);

        Self {
            source,
            destination,
            message: compressed_message,
        }
    }
}

/// This should be around the max packet size. But i could be wrong
pub const SERIALIZED_PACKET_SIZE_MAX: usize =
    u16::MAX as usize + (size_of::<PublicKey>() * 2) + 1024;

#[derive(Default, Debug)]
pub struct PacketEncoderDecoder;

impl Decoder for PacketEncoderDecoder {
    type Item = RouteWeaverPacket;
    type Error = anyhow::Error;

    fn decode(
        &mut self,
        src: &mut tokio_util::bytes::BytesMut,
    ) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        src.reserve(SERIALIZED_PACKET_SIZE_MAX);

        // On purpose our packets don't have any magic bytes and in its bincode serialized format before decryption
        // Only around 2 fields could be incorrect
        // I've practiced feeding it urandom however and not many totally random (ie corrupted) packets get through

        match wire_decode(src) {
            Ok(packet) => {
                let size = wire_estimate_size(&packet)?;
                log::trace!("Found a packet of size {}", size);
                src.advance(size);
                Ok(Some(packet))
            }
            Err(_) => {
                if src.len() > SERIALIZED_PACKET_SIZE_MAX {
                    log::error!("Data hit the max buffer size without being deserializable, couldn't possibly be a packet");

                    // We don't return an error so the stream is not terminated
                    // Instead discard the buffer and try again
                    src.advance(src.remaining().min(SERIALIZED_PACKET_SIZE_MAX));
                    Ok(None)
                } else {
                    log::trace!("Data incomplete, possibly still a packet");
                    Ok(None)
                }
            }
        }
    }
}

impl Encoder<RouteWeaverPacket> for PacketEncoderDecoder {
    type Error = anyhow::Error;

    fn encode(
        &mut self,
        item: RouteWeaverPacket,
        dst: &mut tokio_util::bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        let buffer = wire_encode(&item)?;
        dst.extend(&buffer);

        Ok(())
    }
}

pub type TransportConnectionWriteFramer =
    FramedWrite<Pin<Box<dyn TransportConnectionWriteHalf>>, PacketEncoderDecoder>;

pub type TransportConnectionReadFramer =
    FramedRead<Pin<Box<dyn TransportConnectionReadHalf>>, PacketEncoderDecoder>;
