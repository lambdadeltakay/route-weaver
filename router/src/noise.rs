use arrayvec::ArrayVec;
use either::{
    for_both,
    Either::{self, Left, Right},
};
use hashlink::LruCache;
use once_cell::sync::Lazy;
use route_weaver_common::{
    error::RouteWeaverError,
    message::{
        wire_decode, wire_encode, PeerToPeerMessage, PreEncryptionTransformation,
        RouteWeaverPacket, SERIALIZED_PACKET_SIZE_MAX,
    },
    noise::{PrivateKey, PublicKey},
};
use snow::{params::NoiseParams, HandshakeState, TransportState};
use std::mem::size_of;
use zeroize::Zeroize;

/// Simple way to make handshakes with old clients not work without harming them
static NOISE_PROLOGUE: Lazy<String> =
    Lazy::new(|| format!("router-weaver edition {}", env!("CARGO_PKG_VERSION_MAJOR")));

static NOISE_PATTERN: Lazy<NoiseParams> =
    Lazy::new(|| "Noise_XX_25519_ChaChaPoly_SHA256".parse().unwrap());

fn create_noise_builder<'a>() -> snow::Builder<'a> {
    snow::Builder::new(NOISE_PATTERN.clone()).prologue(NOISE_PROLOGUE.as_bytes())
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
pub struct Noise {
    noise: LruCache<PublicKey, Either<HandshakeState, TransportState>>,
    working_buffer: Vec<u8>,
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
}

impl Noise {
    pub fn new(private_key: PrivateKey, public_key: PublicKey) -> Self {
        Self {
            noise: LruCache::new(1000),
            working_buffer: vec![0; SERIALIZED_PACKET_SIZE_MAX],
            private_key,
            public_key,
        }
    }

    pub fn reset(&mut self, remote: PublicKey) {
        self.noise.remove(&remote);
    }

    pub fn is_transport_capable(&mut self, remote: PublicKey) -> bool {
        self.noise.contains_key(&remote) && self.noise.get(&remote).unwrap().is_right()
    }

    pub fn is_my_turn(&mut self, remote: PublicKey) -> bool {
        let noise = self
            .noise
            .entry(remote)
            .or_insert_with(|| Left(create_initiator(&self.private_key)));

        noise.is_right() || (noise.as_ref().is_left() && noise.as_ref().unwrap_left().is_my_turn())
    }

    pub fn is_their_turn(&mut self, remote: PublicKey) -> bool {
        let noise = self
            .noise
            .entry(remote)
            .or_insert_with(|| Left(create_responder(&self.private_key)));

        noise.is_right() || (noise.as_ref().is_left() && !noise.as_ref().unwrap_left().is_my_turn())
    }

    pub fn verify_remote_public_key(&mut self, remote: PublicKey) -> Option<bool> {
        if let Some(noise) = self.noise.get(&self.public_key) {
            for_both!(noise, noise => noise.get_remote_static())
                .map(|key| PublicKey(key.try_into().unwrap()) == remote)
        } else {
            None
        }
    }

    pub fn try_encrypt(
        &mut self,
        message: PeerToPeerMessage,
        destination: PublicKey,
    ) -> Result<RouteWeaverPacket, RouteWeaverError> {
        if self.is_transport_capable(destination) && matches!(message, PeerToPeerMessage::Handshake)
        {
            return Err(RouteWeaverError::HandshakeInEstablishedTunnel);
        }

        if let Some(public_key_good) = self.verify_remote_public_key(destination) {
            if !public_key_good {
                return Err(RouteWeaverError::SuspiciousRemoteBehavior);
            }
        }

        if self.is_my_turn(destination) {
            let internal_noise = self.noise.get_mut(&destination).unwrap();

            // Clear buffer
            self.working_buffer.fill(0);

            // Encode our message and extract it from the buffer
            let data = wire_encode(&message).map_err(|_| {
                self.working_buffer.zeroize();
                RouteWeaverError::UnencryptedMessageProcessingError
            })?;

            let pre_encryption_transformation =
                determine_best_prencryption_transformation_for_data(&data);

            let mut data = match pre_encryption_transformation {
                PreEncryptionTransformation::Plain => data,
                // With Lz4 we have to prepend the size ourself
                PreEncryptionTransformation::Lz4 => {
                    let length_encoding_size = size_of::<u32>();

                    // Clear our buffer again
                    self.working_buffer.fill(0);

                    // Reserve the buffer for whats coming next
                    self.working_buffer.resize(
                        length_encoding_size + lz4_flex::block::get_maximum_output_size(data.len()),
                        0,
                    );

                    // Compress into our buffer and release that
                    let len = lz4_flex::block::compress_into(
                        &data,
                        &mut self.working_buffer[length_encoding_size..],
                    )
                    .map_err(|_| RouteWeaverError::UnencryptedMessageProcessingError)?;

                    // Copy in the length encoding
                    self.working_buffer[..length_encoding_size]
                        .copy_from_slice(&(data.len() as u32).to_le_bytes());

                    self.working_buffer[..len].to_vec()
                }
            }
            .to_vec();

            // Clear buffer
            self.working_buffer.fill(0);

            // Reserve for packets
            self.working_buffer.resize(SERIALIZED_PACKET_SIZE_MAX, 0);

            // Return the data finally
            match for_both!(internal_noise, internal_noise => internal_noise.write_message(&data, &mut self.working_buffer))
            {
                Ok(len) => {
                    let result = Ok(RouteWeaverPacket {
                        source: self.public_key,
                        pre_encryption_transformation,
                        destination,
                        message: Box::new(self.working_buffer[..len].try_into().map_err(|_| {
                            self.working_buffer.zeroize();
                            RouteWeaverError::UnencryptedMessageProcessingError
                        })?),
                    });

                    // Clear out sensitive data
                    self.working_buffer.zeroize();
                    data.zeroize();

                    result
                }
                Err(err) => {
                    // Clear out sensitive data
                    self.working_buffer.zeroize();
                    data.zeroize();
                    Err(err.into())
                }
            }
        } else {
            Err(RouteWeaverError::HandshakeOrderingOff)
        }
    }

    pub fn try_decrypt(
        &mut self,
        packet: RouteWeaverPacket,
    ) -> Result<PeerToPeerMessage, RouteWeaverError> {
        if self.is_their_turn(packet.source) {
            let internal_noise = self.noise.get_mut(&packet.source).unwrap();

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
                    )?;

                    // Extract the message
                    let message = wire_decode(&data).map_err(|_| {
                        self.working_buffer.zeroize();
                        data.zeroize();
                        RouteWeaverError::UnencryptedMessageProcessingError
                    })?;

                    // Clear out sensitive data
                    data.zeroize();
                    self.working_buffer.zeroize();

                    if let Left(noise) = internal_noise {
                        // If the remote does not want to communicate correctly it may as well not communicate at all
                        if !matches!(message, PeerToPeerMessage::Handshake) {
                            log::warn!(
                                "Remote is trying to handshake with incorrect message. Resetting tunnel"
                            );
                            self.reset(packet.source);
                            return Err(RouteWeaverError::SuspiciousRemoteBehavior);
                        }

                        // If the handshake is finished and everything looks right lets transition into transport mode
                        if noise.is_handshake_finished() {
                            if noise
                                .get_remote_static()
                                .map(|key| PublicKey(key.try_into().unwrap()))
                                != Some(packet.source)
                            {
                                log::info!(
                                    "{}",
                                    noise
                                        .get_remote_static()
                                        .map(|key| PublicKey(key.try_into().unwrap()))
                                        .unwrap()
                                );
                                log::warn!("A handshake completed but the remote public key didn't match the packets specified source. Resetting tunnel");
                                self.reset(packet.source);
                                return Err(RouteWeaverError::SuspiciousRemoteBehavior);
                            }
                            log::info!("A handshake completed with remote {}", packet.source);
                            let noise = self.noise.remove(&packet.source).unwrap();
                            self.noise.insert(
                                packet.source,
                                Right(noise.unwrap_left().into_transport_mode()?),
                            );
                        }

                        return Ok(message);
                    } else {
                        // We are in transport mode so we return all messages
                        // Unless its a handshake message which we just ignore because if we know what the Message is then the remote is just messing with us
                        if matches!(message, PeerToPeerMessage::Handshake) {
                            log::warn!("Remote sent spurious handshake message when encryption tunnel is operational");
                            return Err(RouteWeaverError::HandshakeInEstablishedTunnel);
                        }
                        return Ok(message);
                    }
                }
                Err(err) => {
                    log::warn!("Failed to decrypt message: {}", err);
                }
            }
        }

        log::trace!("Trying again as a responder");

        // Trying to decrypt it with a new responder

        // Clear the buffer because there is no way to determine if noise actually filled it.
        // Ah well. I bet LLVM knows
        self.working_buffer.fill(0);
        self.working_buffer.resize(SERIALIZED_PACKET_SIZE_MAX, 0);

        let mut new_noise = create_responder(&self.private_key);
        match new_noise.write_message(&packet.message, &mut self.working_buffer) {
            Ok(len) => {
                log::trace!("Message decrypted as a responder");

                let mut data = reverse_pre_encryption_transformations(
                    packet.pre_encryption_transformation,
                    &self.working_buffer[..len],
                )?;

                let message = wire_decode(&data).map_err(|_| {
                    self.working_buffer.zeroize();
                    data.zeroize();
                    RouteWeaverError::UnencryptedMessageProcessingError
                })?;

                data.zeroize();
                self.working_buffer.zeroize();
                self.noise.insert(packet.source, Left(new_noise));
                Ok(message)
            }
            Err(err) => Err(err.into()),
        }
    }
}

fn reverse_pre_encryption_transformations(
    transformation: PreEncryptionTransformation,
    buffer: &[u8],
) -> Result<Vec<u8>, RouteWeaverError> {
    match transformation {
        PreEncryptionTransformation::Plain => Ok(buffer.to_vec()),
        PreEncryptionTransformation::Lz4 => Ok(lz4_flex::decompress_size_prepended(buffer)
            .map_err(|_| RouteWeaverError::UnencryptedMessageProcessingError)?),
    }
}

fn determine_best_prencryption_transformation_for_data(data: &[u8]) -> PreEncryptionTransformation {
    // FIXME: This logic is extremely stupid
    // https://stackoverflow.com/questions/46716095/minimum-file-size-for-compression-algorithms

    if data.len() <= 30 {
        return PreEncryptionTransformation::Plain;
    }

    PreEncryptionTransformation::Lz4
}
