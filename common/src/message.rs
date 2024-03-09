use std::{
    collections::{BTreeSet, HashSet},
    mem::size_of,
};

use arrayvec::ArrayVec;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio_util::{
    bytes::Buf,
    codec::{Decoder, Encoder},
};
use zeroize::ZeroizeOnDrop;

use crate::{
    address::TransportAddress, error::RouteWeaverError, noise::PublicKey, router::ApplicationId,
};

#[inline]
pub fn wire_encode<T: Serialize>(to_encode: &T) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_stdvec(to_encode)
}

#[inline]
pub fn wire_decode<T: DeserializeOwned>(to_decode: &[u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(to_decode)
}

#[inline]
pub fn wire_measure_size<T: Serialize>(to_measure: &T) -> Result<usize, postcard::Error> {
    Ok(wire_encode(to_measure)?.len())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PeerToPeerMessage {
    /// Only valid message for handshakes before encryption except for the next one
    /// Valid response messages: Handshake
    Handshake,
    /// Generic response message
    Confirm,
    /// Generic negative response message
    Deny,
    /// Asking if its ok to transmit some data over
    /// Valid response messages: OkToReceiveApplicationData
    StartApplicationData {
        id: ApplicationId,
    },
    /// The application data coming along
    /// Valid response messages: Confirm
    ApplicationData {
        id: ApplicationId,
        index: u8,
        data: Vec<u8>,
    },
    /// End the stream giving a blake2 hash of the data sent in ApplicationData chunks
    /// Valid response messages: Confirm, ApplicationDataMissing
    EndApplicationData {
        id: ApplicationId,
        total_sent: u8,
        sha256: [u8; 32],
    },
    ApplicationDataOk {
        id: ApplicationId,
    },
    /// Send when the hash is bad. Also includes possible missing indexes
    ApplicationDataProblem {
        id: ApplicationId,
        missing_indexes: BTreeSet<u8>,
    },
    RequestPeerList,
    PeerList {
        peer_list: Box<ArrayVec<TransportAddress, 10>>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// What sort of pre encryption transformation we used
/// This also inadvertantly creates a magic byte for the packet filtering out a lot of totally random messages
pub enum PreEncryptionTransformation {
    Plain,
    Lz4,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteWeaverPacket {
    pub source: PublicKey,
    pub destination: PublicKey,
    pub pre_encryption_transformation: PreEncryptionTransformation,
    pub message: Box<ArrayVec<u8, MAX_NOISE_MESSAGE_LENGTH>>,
}

// We allow a little padding to try to account for if the message is compressed
pub const MAX_NOISE_MESSAGE_LENGTH: usize = u16::MAX as usize - 72;

/// I have no clue how noise actually looks like in binary format so we will do this
pub const SERIALIZED_PACKET_SIZE_MAX: usize = (size_of::<PublicKey>() * 2) + u16::MAX as usize * 2;

#[derive(Default, Debug, ZeroizeOnDrop)]
pub struct PacketEncoderDecoder;

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
                    .map_err(|_| RouteWeaverError::PacketManagingFailure)?;
                src.advance(size);

                Ok(Some(packet))
            }
            Err(_) => {
                if src.len() > SERIALIZED_PACKET_SIZE_MAX {
                    src.advance(SERIALIZED_PACKET_SIZE_MAX);

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
        dst.extend_from_slice(&wire_encode(&item)?);

        Ok(())
    }
}
