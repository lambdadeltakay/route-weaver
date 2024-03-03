use crate::{
    address::TransportAddress, noise::PublicKey, transport::TransportConnection, wire_decode,
    wire_encode, wire_measure_size,
};
use arrayvec::ArrayVec;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{mem::size_of, pin::Pin};
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::{
    bytes::Buf,
    codec::{Decoder, Encoder, FramedRead, FramedWrite},
};
use zeroize::ZeroizeOnDrop;

#[derive(Clone, Debug, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct ApplicationId(pub [u8; 32]);

impl From<&str> for ApplicationId {
    fn from(value: &str) -> Self {
        ApplicationId(Sha256::digest(value.as_bytes()).into())
    }
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
                let size = wire_measure_size(&packet)?;
                log::trace!("Found a packet of size {}", size);
                src.advance(size);
                Ok(Some(packet))
            }
            Err(_) => {
                if src.len() > SERIALIZED_PACKET_SIZE_MAX {
                    log::error!("Data hit the max buffer size without being deserializable, couldn't possibly be a packet");

                    // Drop the rest of the buffer
                    src.clear();
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
    type Error = anyhow::Error;

    fn encode(
        &mut self,
        item: RouteWeaverPacket,
        dst: &mut tokio_util::bytes::BytesMut,
    ) -> Result<(), Self::Error> {
        self.working_buffer.fill(0);
        let len = wire_encode(&mut self.working_buffer, &item)?;
        dst.extend_from_slice(&self.working_buffer[..len]);

        Ok(())
    }
}

pub type TransportConnectionWriteFramer =
    FramedWrite<WriteHalf<Pin<Box<dyn TransportConnection>>>, PacketEncoderDecoder>;

pub type TransportConnectionReadFramer =
    FramedRead<ReadHalf<Pin<Box<dyn TransportConnection>>>, PacketEncoderDecoder>;
