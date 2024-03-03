use std::{fmt::Display, str::FromStr};

use data_encoding::HEXLOWER_PERMISSIVE;
use either::{for_both, Either};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, TransportState};
use zeroize::ZeroizeOnDrop;

use crate::{
    message::{Message, PreEncryptionTransformation, RouteWeaverPacket},
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
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PublicKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid public key"))?,
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
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PrivateKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())?
                .try_into()
                .map_err(|_| anyhow::anyhow!("Invalid private key"))?,
        ))
    }
}

/// Simple way to make handshakes with old clients not work without harming them
static NOISE_PROLOGUE: Lazy<String> =
    Lazy::new(|| format!("router-weaver edition {}", env!("CARGO_PKG_VERSION_MAJOR")));

fn create_noise_builder<'a>() -> Builder<'a> {
    Builder::new(
        "Noise_XX_25519_ChaChaPoly_SHA256"
            .parse()
            .unwrap(),
    )
    .prologue(NOISE_PROLOGUE.as_bytes())
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

pub enum DecryptionResult {
    NeedsHandshakeSent,
    UnknownError(anyhow::Error),
}

/// Helper to make working with noise protocol less painful

#[derive(Default)]
pub struct Noise {
    pub internal_noise: Option<Either<HandshakeState, TransportState>>,
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

        if data.len() > 20 {
            return PreEncryptionTransformation::Zlib;
        }

        PreEncryptionTransformation::Plain
    }

    pub fn try_encrypt(
        &mut self,
        private_key: &PrivateKey,
        message: Message,
    ) -> Result<(PreEncryptionTransformation, Vec<u8>), anyhow::Error> {
        let mut buffer = vec![0; u16::MAX as usize];
        let message = wire_encode(&message)?;
        let message = match self.determine_best_prencryption_transformation_for_data(&message) {
            PreEncryptionTransformation::Plain => message,
            PreEncryptionTransformation::Lz4 => lz4_flex::compress_prepend_size(&message),
            PreEncryptionTransformation::Zlib => {
                miniz_oxide::deflate::compress_to_vec_zlib(&message, 10)
            }
        };

        let internal_noise = if self.internal_noise.is_none() {
            self.internal_noise = Some(either::Left(create_initiator(private_key)));
            self.internal_noise.as_mut().unwrap()
        } else {
            self.internal_noise.as_mut().unwrap()
        };

        match for_both!(internal_noise, internal_noise => internal_noise.write_message(&message, &mut buffer))
        {
            Ok(len) => {
                buffer.truncate(len);
                Ok((PreEncryptionTransformation::Lz4, buffer))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn try_decrypt(
        &mut self,
        private_key: &PrivateKey,
        packet: RouteWeaverPacket,
    ) -> Result<Option<Message>, DecryptionResult> {
        let mut buffer = vec![0; u16::MAX as usize];

        if self.internal_noise.is_none() {
            self.internal_noise = Some(either::Left(create_responder(private_key)));
        }

        // Try decrypting normally
        match for_both!(self.internal_noise.as_mut().unwrap(), internal_noise => internal_noise.read_message(&packet.message, &mut buffer))
        {
            // It worked so return
            Ok(len) => {
                buffer.truncate(len);

                let message = wire_decode(&buffer).unwrap();

                if let Either::Left(noise) = self.internal_noise.as_mut().unwrap() {
                    // If the remote does not want to communicate correctly it may as well not communicate at all
                    if !matches!(message, Message::Handshake) {
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
                    Ok(if matches!(message, Message::Handshake) {
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

                // Clear the buffer
                buffer.fill(0);

                // It didn't work so its a little more tricky what we have to do here
                // If we are in handshake state and we are in a responder mode, we will need to send a handshake message and discard this messages
                // If we are in a handshake state and we are in an initiator mode, we need to try again in responder mode and discard this message. If this fails set the internal state to None
                // If we are in a transport state, we need to try again in handshake responder mode. If this fails go on like nothing happened
                // If all else fails, discard the message entirely
                // This should handle all cases of clients getting confused

                // The implications of this is that handshakes with remote nodes could be interrupted to be started again but with another node
                // This doesn't actually mean someone can fake their identity but it does mean that we are vulnerable to talking to random gateway nodes.
                // In reality we will try to peer with as many people as possible so I doubt its a issue

                match self.internal_noise.as_mut().unwrap() {
                    Either::Left(noise) => {
                        if noise.is_initiator() {
                            // Try to go into responder mode for this message
                            let mut new_noise = create_responder(private_key);
                            // Going into responder mode worked!
                            if let Ok(len) = new_noise.read_message(&packet.message, &mut buffer) {
                                buffer.truncate(len);
                                self.internal_noise = Some(either::Left(new_noise));
                                log::info!("Message was successfully decrypted in responder mode");
                                return Ok(Some(wire_decode(&buffer).unwrap()));
                            } else {
                                log::error!("Failed to decrypt message in responder mode. Aborting handshake");
                                self.internal_noise = None;
                                return Ok(None);
                            }
                        }
                    }
                    Either::Right(noise) => todo!(),
                }

                todo!()
            }
        }
    }
}
