use std::{fmt::Display, mem::size_of, str::FromStr};

use data_encoding::HEXLOWER_PERMISSIVE;
use either::{for_both, Either};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use snow::{params::NoiseParams, Builder, HandshakeState, TransportState};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    error::RouteWeaverError,
    message::{
        PeerToPeerMessage, PreEncryptionTransformation, RouteWeaverPacket,
        SERIALIZED_PACKET_SIZE_MAX,
    },
    wire_decode, wire_encode,
};

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl FromStr for PublicKey {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PublicKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?
                .try_into()
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?,
        ))
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize, ZeroizeOnDrop)]
pub struct PrivateKey(pub [u8; 32]);

impl Display for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl FromStr for PrivateKey {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PrivateKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?
                .try_into()
                .map_err(|_| RouteWeaverError::KeyFailedToParse)?,
        ))
    }
}

/// Simple way to make handshakes with old clients not work without harming them
static NOISE_PROLOGUE: Lazy<String> =
    Lazy::new(|| format!("router-weaver edition {}", env!("CARGO_PKG_VERSION_MAJOR")));

static NOISE_PATTERN: Lazy<NoiseParams> =
    Lazy::new(|| "Noise_XX_25519_ChaChaPoly_SHA256".parse().unwrap());

fn create_noise_builder<'a>() -> Builder<'a> {
    Builder::new(NOISE_PATTERN.clone()).prologue(NOISE_PROLOGUE.as_bytes())
}

pub fn create_keypair() -> (PublicKey, PrivateKey) {
    let keypair = create_noise_builder().generate_keypair().unwrap();

    (
        PublicKey(keypair.public.try_into().unwrap()),
        PrivateKey(keypair.private.try_into().unwrap()),
    )
}

fn create_responder(key: &PrivateKey) -> HandshakeState {
    create_noise_builder()
        .local_private_key(&key.0)
        .build_responder()
        .unwrap()
}

fn create_initiator(key: &PrivateKey) -> HandshakeState {
    create_noise_builder()
        .local_private_key(&key.0)
        .build_initiator()
        .unwrap()
}

/// Helper to make working with noise protocol less painful

#[derive(Default)]
pub struct Noise {
    pub internal_noise: Option<Either<HandshakeState, TransportState>>,
    pub working_buffer: Vec<u8>,
}

impl Noise {
    pub fn is_transport_capable(&self) -> bool {
        self.internal_noise.is_some() && self.internal_noise.as_ref().unwrap().is_right()
    }

    pub fn is_my_turn(&self) -> bool {
        self.internal_noise.is_some()
            && (self.internal_noise.as_ref().unwrap().is_right()
                || self
                    .internal_noise
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unwrap_left()
                    .is_my_turn())
    }

    fn determine_best_prencryption_transformation_for_data(
        &self,
        data: &[u8],
    ) -> PreEncryptionTransformation {
        // FIXME: This logic is extremely stupid
        // https://stackoverflow.com/questions/46716095/minimum-file-size-for-compression-algorithms

        if data.len() > 30 {
            return PreEncryptionTransformation::Lz4;
        }

        PreEncryptionTransformation::Plain
    }

    pub fn try_encrypt(
        &mut self,
        private_key: &PrivateKey,
        message: PeerToPeerMessage,
    ) -> Result<(PreEncryptionTransformation, Vec<u8>), RouteWeaverError> {
        // Clear buffer
        self.working_buffer.fill(0);

        // Encode our message and extract it from the buffer
        let len = wire_encode(&mut self.working_buffer, &message)?;

        let pre_encryption_transformation =
            self.determine_best_prencryption_transformation_for_data(&self.working_buffer[..len]);

        let mut data = match pre_encryption_transformation {
            PreEncryptionTransformation::Plain => &self.working_buffer[..len],
            // With Lz4 we have to prepend the size ourself
            PreEncryptionTransformation::Lz4 => {
                let length_encoding_size = size_of::<u32>();

                // Extract our data
                let data = self.working_buffer[..len].to_vec();

                // Clear our buffer again
                self.working_buffer.fill(0);

                // Reserve the buffer for whats coming next
                self.working_buffer.resize(
                    length_encoding_size + lz4_flex::block::get_maximum_output_size(len),
                    0,
                );

                // Compress into our buffer and release that
                let len = lz4_flex::block::compress_into(
                    &data,
                    &mut self.working_buffer[length_encoding_size..],
                )?;

                // Copy in the length encoding
                self.working_buffer[..length_encoding_size]
                    .copy_from_slice(&(len as u32).to_le_bytes());

                &self.working_buffer[..len]
            }
        }
        .to_vec();

        let internal_noise = self
            .internal_noise
            .get_or_insert_with(|| either::Left(create_initiator(private_key)));

        // Clear buffer
        self.working_buffer.fill(0);

        // Reserve for packets
        self.working_buffer.resize(SERIALIZED_PACKET_SIZE_MAX, 0);

        // Return the data finally
        match for_both!(internal_noise, internal_noise => internal_noise.write_message(&data, &mut self.working_buffer))
        {
            Ok(len) => {
                // Clear out sensitive data
                self.working_buffer.zeroize();
                data.zeroize();

                Ok((
                    pre_encryption_transformation,
                    self.working_buffer[..len].to_vec(),
                ))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn try_decrypt(
        &mut self,
        private_key: &PrivateKey,
        packet: RouteWeaverPacket,
    ) -> Result<Option<PeerToPeerMessage>, RouteWeaverError> {
        let internal_noise = self
            .internal_noise
            .get_or_insert_with(|| either::Left(create_responder(private_key)));

        // Reserve the buffer
        self.working_buffer.resize(SERIALIZED_PACKET_SIZE_MAX, 0);

        // Clear out internal buffer
        self.working_buffer.fill(0);

        // Try decrypting normally
        match for_both!(internal_noise, internal_noise => internal_noise.read_message(&packet.message, &mut self.working_buffer))
        {
            // It worked so return
            Ok(len) => {
                // Decompress the data
                let mut data = reverse_pre_encryption_transformations(
                    packet.pre_encryption_transformation,
                    &self.working_buffer[..len],
                )
                .unwrap();
                // Extract the message
                let message = wire_decode(&data).unwrap();

                // Clear out sensitive data
                data.zeroize();

                if let Either::Left(noise) = internal_noise {
                    // If the remote does not want to communicate correctly it may as well not communicate at all
                    if !matches!(message, PeerToPeerMessage::Handshake) {
                        self.internal_noise = None;
                    // If the handshake is finished and everything looks right lets transition into transport mode
                    } else if noise.is_handshake_finished() {
                        if noise.get_remote_static().is_none()
                            || noise.get_remote_static() != Some(&packet.source.0)
                        {
                            log::error!("A handshake completed but the remote public key didn't match the packets specified source. Aborting tunnel");
                            self.internal_noise = None;
                        } else {
                            log::info!("A handshake completed with remote {}", packet.source);
                            let old_noise = self.internal_noise.take();
                            self.internal_noise = Some(either::Right(
                                old_noise
                                    .unwrap()
                                    .unwrap_left()
                                    .into_transport_mode()
                                    .unwrap(),
                            ))
                        };
                    }

                    // We are in handshake mode so we don't return anything
                    Ok(None)
                } else {
                    // We are in transport mode so we return all messages
                    // Unless its a handshake message which we just ignore because if we know what the Message is then the remote is just messing with us
                    Ok(if matches!(message, PeerToPeerMessage::Handshake) {
                        log::warn!("Remote sent spurious handshake message when encryption tunnel is operational");
                        None
                    } else {
                        Some(message)
                    })
                }
            }
            Err(err) => {
                log::warn!(
                    "Failed to decrypt message: {}, retrying to decrypt it in a new mode",
                    err
                );

                // Clear the buffer because there is no way to determine if noise actually filled it.
                // Ah well. I bet LLVM knows
                self.working_buffer.fill(0);

                // The implications of this is that handshakes with remote nodes could be interrupted to be started again but with another node
                // This doesn't actually mean someone can fake their identity but it does mean that we are vulnerable to talking to random gateway nodes.
                // In reality we will try to peer with as many people as possible so I doubt its a issue

                // Check if we are the initiator or we are currently in a tunnel
                if (internal_noise.is_left()
                    && internal_noise.as_ref().unwrap_left().is_initiator())
                    || internal_noise.is_right()
                {
                    // Try to go into responder mode for this message
                    let mut new_noise = create_responder(private_key);

                    // Going into responder mode worked!
                    if let Ok(len) =
                        new_noise.read_message(&packet.message, &mut self.working_buffer)
                    {
                        self.internal_noise = Some(either::Left(new_noise));
                        log::info!("Message was successfully decrypted in responder mode");

                        let message = wire_decode(
                            &reverse_pre_encryption_transformations(
                                packet.pre_encryption_transformation,
                                &self.working_buffer[..len],
                            )
                            .unwrap(),
                        )
                        .unwrap();

                        // Clear out sensitive data
                        self.working_buffer.zeroize();

                        Ok(Some(message))
                    } else {
                        log::error!(
                            "Failed to decrypt message in responder mode. Aborting handshake"
                        );
                        self.internal_noise = None;
                        Ok(None)
                    }
                } else {
                    todo!()
                }
            }
        }
    }
}

fn reverse_pre_encryption_transformations(
    transformation: PreEncryptionTransformation,
    buffer: &[u8],
) -> Result<Vec<u8>, RouteWeaverError> {
    match transformation {
        PreEncryptionTransformation::Plain => Ok(buffer.to_vec()),
        PreEncryptionTransformation::Lz4 => Ok(lz4_flex::decompress_size_prepended(buffer)?),
    }
}
