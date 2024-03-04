use futures::prelude::{future::FutureExt, sink::SinkExt, stream::StreamExt, Future};
use lru::LruCache;
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
    transport::Transport,
};
use std::{collections::HashMap, num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    io::split,
    sync::RwLock,
    time::{sleep, timeout},
};

#[derive(Default)]
pub struct P2PCommunicatorBuilder {
    transports: HashMap<&'static str, Pin<Box<dyn Future<Output = Box<dyn Transport>>>>>,
    public_key: Option<PublicKey>,
    private_key: Option<PrivateKey>,
    seed_node: Option<TransportAddress>,
}

impl P2PCommunicatorBuilder {
    pub fn add_transport<T: Transport + 'static>(mut self) -> Self {
        let transport = T::boxed_new();
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
        self.seed_node = Some(seed_node);
        self
    }

    pub async fn build(mut self) -> Arc<P2PCommunicator> {
        assert!(
            !self.transports.is_empty(),
            "No transports were passed into the builder"
        );

        let communicator = Arc::new(P2PCommunicator {
            public_key: self.public_key.unwrap(),
            private_key: self.private_key.unwrap(),
            inbox: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
            writers: RwLock::new(HashMap::new()),
            noise_cache: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
        });

        for (transport_name, transport) in self.transports.drain() {
            let me = communicator.clone();
            let mut transport = transport.await;

            tokio::task::Builder::new()
                .name(&format!("{} transport listener", transport_name))
                .spawn(async move {
                    loop {
                        match transport.accept().await {
                            Ok((transport, address)) => {
                                let (read, write) = split(transport);
                                let (read, write) = (
                                    TransportConnectionReadFramer::new(
                                        read,
                                        PacketEncoderDecoder::default(),
                                    ),
                                    TransportConnectionWriteFramer::new(
                                        write,
                                        PacketEncoderDecoder::default(),
                                    ),
                                );

                                // FIXME: We should not store transports that can't produce addresses
                                let gateway_address = address.unwrap_or_else(|| TransportAddress {
                                    protocol: transport_name.to_string(),
                                    address_type: transport_name.to_string(),
                                    data: "unnameable".to_string(),
                                    port: None,
                                });

                                log::trace!("Accepted connection from {}", gateway_address);

                                me.writers
                                    .write()
                                    .await
                                    .entry(transport_name)
                                    .or_insert_with(|| {
                                        LruCache::new(NonZeroUsize::new(100).unwrap())
                                    })
                                    .push(gateway_address.clone(), write);

                                tokio::task::Builder::new()
                                    .name(&format!("{} gateway listener", gateway_address))
                                    .spawn(
                                        me.clone()
                                            .create_gateway_listener(read, gateway_address.clone()),
                                    )
                                    .unwrap();
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
                    }
                })
                .unwrap();
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
}

impl P2PCommunicator {
    async fn create_gateway_listener(
        self: Arc<Self>,
        mut read: TransportConnectionReadFramer,
        gateway_address: TransportAddress,
    ) {
        loop {
            if let Some(packet) = read.next().await {
                // Packet decoded ok
                if let Ok(packet) = packet {
                    if packet.source == packet.destination {
                        log::warn!("Packet is set to route back to sender, which was done likely to overload this node. Dropping");
                        continue;
                    }

                    if packet.destination != self.public_key {
                        log::trace!("Packet is not for us, rerouting to {}", packet.destination);

                        match timeout(
                            Duration::from_millis(250),
                            self.route_packet(packet.clone()),
                        )
                        .await
                        {
                            Ok(_) => {
                                log::trace!("Rerouted remote packet to {}", packet.destination);
                            }
                            Err(_) => {
                                log::trace!(
                                    "Rerouting remote packet to {} failed",
                                    packet.destination
                                );
                            }
                        }
                    } else if let Some(message) = self
                        .noise_cache
                        .write()
                        .await
                        .get_or_insert_mut(packet.source, Noise::default)
                        .try_decrypt(&self.private_key, packet.clone())
                        .unwrap()
                    {
                        log::trace!("Trying to decrypt packet from {}", packet.source);

                        self.inbox
                            .write()
                            .await
                            .get_mut(&packet.source)
                            .unwrap()
                            .push_overwrite(message);
                    } else {
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

                return;
            }
        }
    }

    pub async fn send_message(&self, destination: PublicKey, message: PeerToPeerMessage) {
        if destination == self.public_key {
            log::trace!("Message sent to loopback");

            self.inbox
                .write()
                .await
                .get_mut(&self.public_key)
                .unwrap()
                .push_overwrite(message);
        } else {
            log::trace!("Trying to send message to {}", destination);

            loop {
                let mut noise_lock = self.noise_cache.write().await;

                let noise = noise_lock.get_or_insert_mut(destination, Noise::default);

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

    async fn route_packet(&self, packet: RouteWeaverPacket) {
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
