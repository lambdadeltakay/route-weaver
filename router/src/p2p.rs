use futures::prelude::{sink::SinkExt, stream::StreamExt, Future};
use lru::LruCache;
use rand::prelude::{IteratorRandom, SeedableRng, SmallRng};
use ringbuf::Rb;
use ringbuf::StaticRb;
use route_weaver_common::{
    address::TransportAddress,
    error::RouteWeaverError,
    message::{
        PacketEncoderDecoder, PeerToPeerMessage, RouteWeaverPacket, TransportConnectionReadFramer,
        TransportConnectionWriteFramer,
    },
    noise::{Noise, PrivateKey, PublicKey},
    transport::{Transport, TransportConnection},
};
use std::{collections::HashMap, num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    io::split,
    sync::RwLock,
    time::{sleep, timeout},
};

use crate::connection_manager::ConnectionManager;

#[derive(Default)]
pub struct P2PCommunicatorBuilder {
    #[allow(clippy::type_complexity)]
    transports: HashMap<&'static str, Pin<Box<dyn Future<Output = Arc<dyn Transport>>>>>,
    public_key: Option<PublicKey>,
    private_key: Option<PrivateKey>,
    seed_node: Vec<TransportAddress>,
}

impl P2PCommunicatorBuilder {
    pub fn add_transport<T: Transport + 'static>(mut self) -> Self {
        let transport = T::arced_new();
        log::info!("Adding protocol type {}", T::get_protocol_string());
        self.transports.insert(T::get_protocol_string(), transport);

        self
    }

    pub fn add_public_key(mut self, public_key: PublicKey) -> Self {
        self.public_key = Some(public_key);
        self
    }

    pub fn add_private_key(mut self, private_key: PrivateKey) -> Self {
        self.private_key = Some(private_key);
        self
    }

    pub fn add_seed_node(mut self, seed_node: TransportAddress) -> Self {
        self.seed_node.push(seed_node);
        self
    }

    pub async fn build(mut self) -> Arc<P2PCommunicator> {
        assert!(
            !self.transports.is_empty(),
            "No transports were passed into the builder"
        );

        let mut buffer = StaticRb::default();
        buffer.push_iter(&mut self.seed_node.into_iter());

        let communicator = Arc::new(P2PCommunicator {
            public_key: self.public_key.unwrap(),
            private_key: self.private_key.unwrap(),
            inbox: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
            writers: RwLock::new(HashMap::new()),
            noise_cache: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
            known_gateways: RwLock::new(Box::new(buffer)),
            connected_gateways: RwLock::new(ConnectionManager::default()),
        });

        for (transport_name, transport) in self.transports.drain() {
            let me = communicator.clone();

            let transport = transport.await;
            let thread_transport = transport.clone();

            tokio::spawn(async move {
                let mut rng = SmallRng::from_entropy();

                loop {
                    // Go talk to someone
                    let gateways_lock = me.known_gateways.read().await;
                    let connected_gateways_lock = me.connected_gateways.read().await;

                    if let Some(address) = gateways_lock
                        .iter()
                        .filter(|address| {
                            !connected_gateways_lock.is_tracked(address)
                                && address.protocol == transport_name
                        })
                        .choose(&mut rng)
                        .cloned()
                    {
                        log::info!("Contacting {}", address);

                        match thread_transport.connect(address.clone()).await {
                            Ok(transport) => {
                                me.clone()
                                    .handle_new_connection(transport_name, transport, address)
                                    .await;
                            }
                            Err(err) => {
                                log::warn!("Failed to connect to {} because of {}", address, err);
                                sleep(Duration::from_secs(10)).await;
                            }
                        }
                    }

                    sleep(Duration::from_millis(10)).await;
                }
            });

            let me = communicator.clone();

            tokio::spawn(async move {
                loop {
                    match transport.accept().await {
                        Ok((transport, address)) => {
                            // FIXME: We should not store transports that can't produce addresses
                            let address = address.unwrap_or_else(|| TransportAddress {
                                protocol: transport_name.to_string(),
                                address_type: transport_name.to_string(),
                                data: "unnameable".to_string(),
                                port: None,
                            });

                            if !me.connected_gateways.read().await.is_tracked(&address) {
                                log::trace!("Accepted connection from {}", address);

                                me.clone()
                                    .handle_new_connection(transport_name, transport, address)
                                    .await;
                            }
                        }
                        Err(err) => {
                            if matches!(
                                err,
                                RouteWeaverError::UnsupportedOperationRequestedOnTransport
                            ) {
                                log::warn!(
                                    "{} accepting connections is not support for",
                                    transport_name
                                );
                                return;
                            }

                            log::error!("Failed to accept connection: {}", err);
                        }
                    }

                    sleep(Duration::from_millis(10)).await;
                }
            });
        }

        communicator
    }
}

pub struct P2PCommunicator {
    public_key: PublicKey,
    private_key: PrivateKey,
    inbox: RwLock<LruCache<PublicKey, StaticRb<PeerToPeerMessage, 1000>>>,
    writers:
        RwLock<HashMap<&'static str, LruCache<TransportAddress, TransportConnectionWriteFramer>>>,
    noise_cache: RwLock<LruCache<PublicKey, Noise>>,
    known_gateways: RwLock<Box<StaticRb<TransportAddress, 100>>>,
    connected_gateways: RwLock<ConnectionManager>,
}

impl P2PCommunicator {
    async fn handle_new_connection(
        self: Arc<Self>,
        transport_name: &'static str,
        connection: Pin<Box<dyn TransportConnection>>,
        address: TransportAddress,
    ) {
        let (read, write) = split(connection);
        let (read, write) = (
            TransportConnectionReadFramer::new(read, PacketEncoderDecoder::default()),
            TransportConnectionWriteFramer::new(write, PacketEncoderDecoder::default()),
        );

        self.writers
            .write()
            .await
            .entry(transport_name)
            .or_insert_with(|| LruCache::new(NonZeroUsize::new(100).unwrap()))
            .push(address.clone(), write);

        tokio::spawn(self.clone().create_gateway_listener(read, address.clone()));
    }

    async fn create_gateway_listener(
        self: Arc<Self>,
        mut read: TransportConnectionReadFramer,
        gateway_address: TransportAddress,
    ) {
        loop {
            self.connected_gateways
                .write()
                .await
                .track_connection(&gateway_address);

            if let Some(packet) = read.next().await {
                log::trace!("Received packet from {}", gateway_address);

                // Packet decoded ok
                if let Ok(packet) = packet {
                    if packet.source == packet.destination {
                        log::warn!("Packet is set to route back to sender, which was done likely to overload this node. Dropping");
                        continue;
                    }

                    if packet.destination != self.public_key {
                        log::trace!("Packet is not for us, rerouting to {}", packet.destination);

                        let me = self.clone();
                        tokio::spawn(async move {
                            let _ = timeout(
                                Duration::from_millis(250),
                                me.route_packet(packet.clone()),
                            )
                            .await;
                        });
                    } else {
                        let mut noise_lock = self.noise_cache.write().await;
                        let noise = noise_lock.get_or_insert_mut(packet.source, || {
                            Noise::new(&self.private_key, false)
                        });

                        if noise.is_their_turn() {
                            match noise.try_decrypt(&self.private_key, packet.clone()) {
                                Ok(message) => {
                                    if let Some(message) = message {
                                        let _ = self
                                            .inbox
                                            .write()
                                            .await
                                            .get_or_insert_mut(packet.source, StaticRb::default)
                                            .push(message);
                                    } else {
                                        // The absorbed message was some kind of handshake message
                                        let me = self.clone();

                                        tokio::spawn(async move {
                                            let _ = timeout(
                                                Duration::from_millis(250),
                                                me.send_message(
                                                    packet.source,
                                                    PeerToPeerMessage::Handshake,
                                                ),
                                            )
                                            .await;
                                        });
                                    }
                                }
                                Err(_) => todo!(),
                            }
                        } else {
                            log::warn!(
                                "Remote tried to speak out of turn. Discarding their message"
                            );
                        }

                        log::trace!("Packet from {} did not result in a message likely due to it being a handshake related message", packet.source);
                    }
                }
            // Transport suffered a fatal error and probably closed
            // Drop the thread
            } else {
                log::error!("Connection from {} closed", gateway_address);

                // Discard the writer too
                self.writers
                    .write()
                    .await
                    .get_mut(gateway_address.protocol.as_str())
                    .unwrap()
                    .pop(&gateway_address);

                // Discard the connected entry too
                self.connected_gateways
                    .write()
                    .await
                    .untrack_connection(&gateway_address);

                return;
            }
        }
    }

    pub async fn send_message(&self, destination: PublicKey, message: PeerToPeerMessage) {
        if destination == self.public_key {
            log::trace!("Message sent to loopback");

            let _ = self
                .inbox
                .write()
                .await
                .get_or_insert_mut(self.public_key, StaticRb::default)
                .push(message);
        } else {
            log::trace!("Trying to send message to {}", destination);

            loop {
                let mut noise_lock = self.noise_cache.write().await;

                let noise = noise_lock
                    .get_or_insert_mut(destination, || Noise::new(&self.private_key, true));

                if noise.is_transport_capable() {
                    let (pre_encryption_transformation, message) = noise
                        .try_encrypt(&self.private_key, message.clone())
                        .unwrap();

                    self.route_packet(RouteWeaverPacket {
                        source: self.public_key,
                        destination,
                        pre_encryption_transformation,
                        message,
                    })
                    .await;

                    return;
                }

                if noise.is_my_turn() {
                    log::info!("Trying to send message to {}", destination);

                    let (pre_encryption_transformation, message) = noise
                        .try_encrypt(&self.private_key, PeerToPeerMessage::Handshake)
                        .unwrap();

                    self.route_packet(RouteWeaverPacket {
                        source: self.public_key,
                        destination,
                        pre_encryption_transformation,
                        message,
                    })
                    .await;
                }

                sleep(Duration::from_millis(10)).await;
            }
        }
    }

    pub async fn receive_message(&self, destination: PublicKey) -> PeerToPeerMessage {
        log::trace!("Trying to receive message from {}", destination);

        loop {
            let mut inbox_lock = self.inbox.write().await;

            if let Some(inbox) = inbox_lock.get_mut(&destination) {
                log::info!("Got message from {}", destination);

                if let Some(message) = inbox.pop() {
                    return message;
                }
            }

            sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn route_packet(&self, packet: RouteWeaverPacket) {
        loop {
            let mut writers_lock = self.writers.write().await;
            let mut writers_remove = Vec::new();

            for (transport_name, transport) in writers_lock.iter_mut() {
                if let Some((gateway_address, mut write)) = transport.pop_lru() {
                    match write.send(packet.clone()).await {
                        Ok(_) => {
                            log::trace!("Routed packet to {}", gateway_address);
                            return;
                        }
                        Err(err) => {
                            log::trace!(
                                "Failed to route packet to {} because of {}",
                                gateway_address,
                                err
                            );
                            writers_remove.push((*transport_name, gateway_address));
                        }
                    }
                }
            }

            for (transport_name, gateway_address) in writers_remove {
                writers_lock
                    .get_mut(transport_name)
                    .unwrap()
                    .pop(&gateway_address);
            }

            sleep(Duration::from_millis(10)).await;
        }
    }
}
