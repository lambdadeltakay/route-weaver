use crate::{create_initiator, create_responder};
use either::Either::{self, Left};
use futures::prelude::{sink::SinkExt, stream::StreamExt, Future};
use lru::LruCache;
use ringbuf::Rb;
use ringbuf::StaticRb;
use route_weaver_common::{
    message::{
        Message, RouteWeaverPacket, TransportConnectionReadFramer, TransportConnectionWriteFramer,
    },
    transport::{Transport, TransportAddress},
    PrivateKey, PublicKey,
};
use snow::{HandshakeState, TransportState};
use std::{collections::HashMap, num::NonZeroUsize, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, RwLock},
    time::{interval, sleep, timeout, Interval, MissedTickBehavior},
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
        let communicator = Arc::new(P2PCommunicator {
            public_key: self.public_key.unwrap(),
            private_key: self.private_key.unwrap(),
            out_writers: RwLock::new(Vec::new()),
            inbox: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
            noise_cache: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
        });

        if let Some(seed_node) = self.seed_node {
            tokio::spawn(communicator.clone().keep_up_with_seed_nodes(seed_node));
        }

        for (transport_name, transport) in self.transports.drain() {
            let me = communicator.clone();
            let mut transport = transport.await;

            tokio::spawn(async move {
                loop {
                    if let Some(((read, write), address)) = transport.accept().await {
                        // FIXME: We should not store transports that can't produce addresses
                        let gateway_address = address.unwrap_or_else(|| TransportAddress {
                            protocol: transport_name,
                            data: "unnamed".to_string(),
                        });

                        log::trace!("Accepted connection from {}", gateway_address);

                        me.out_writers.write().await.push(Mutex::new(write));

                        let me = me.clone();

                        tokio::spawn(me.create_gateway_listener(read, gateway_address.clone()));
                    }
                }
            });
        }

        communicator
    }
}

pub struct P2PCommunicator {
    public_key: PublicKey,
    private_key: PrivateKey,
    out_writers: RwLock<Vec<Mutex<TransportConnectionWriteFramer>>>,
    inbox: RwLock<LruCache<PublicKey, StaticRb<Message, 1000>>>,
    noise_cache: RwLock<LruCache<PublicKey, Either<HandshakeState, TransportState>>>,
}

impl P2PCommunicator {
    async fn keep_up_with_seed_nodes(self: Arc<Self>, seed_node: TransportAddress) {
        let mut counter = interval(Duration::from_secs(60));
        counter.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            counter.tick().await;
        }
    }

    async fn route_packet_from_remote(self: Arc<Self>, packet: RouteWeaverPacket) {
        match timeout(
            Duration::from_millis(100),
            self.route_packet(packet.clone()),
        )
        .await
        {
            Ok(_) => {
                log::trace!(
                    "Packet that was requested to be routed by remote node {} successfully",
                    packet.destination
                );
            }
            Err(_) => {
                log::warn!(
                    "Packet that was requested to be routed by remote node {} failed to route",
                    packet.destination
                );
            }
        }
    }

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

                        // Nodes have no actual obligation to do routing so if routing fails we just drop the packet
                        tokio::spawn(self.clone().route_packet_from_remote(packet));
                    } else {
                        // Packet is for us
                        if let Some(noise) = self.noise_cache.write().await.get_mut(&packet.source)
                        {
                            match packet.message.decode(noise.as_mut()) {
                                // A message from our beloved friends has arrived!
                                Ok(message) => {
                                    if matches!(message, Message::Handshake) {
                                        log::trace!("Received handshake from {}", packet.source);
                                    }
                                }
                                Err(err) => {
                                    log::error!(
                                        "A packet marked for us is not decryptable: {}",
                                        err
                                    );
                                    // TODO: Add some kind of counter for undecryptable packets for ratelimiting reasons
                                }
                            }
                        } else {
                            // We don't have noise for this girl. Handshake now
                            log::trace!("Handshake needed from {}", packet.source);

                            // First we will try to decrypt this as if it is a Handshake message
                            let mut new_noise = create_responder(&self.private_key);

                            let treating_message_as_responder_result =
                                packet.clone().message.decode(Left(&mut new_noise));

                            // Ok so treating it as a responder didn't go well
                            if treating_message_as_responder_result.is_err()
                                || !matches!(
                                    treating_message_as_responder_result.unwrap(),
                                    Message::Handshake
                                )
                            {
                                log::warn!("Node {} trying to communicate when it has not had a handshake. Initiating handshake", packet.source);

                                // We will try this again but as a initiator
                                let mut new_noise = create_initiator(&self.private_key);
                                let me = self.clone();

                                tokio::spawn(async move {
                                    // TODO: Make this drop noise if it fails
                                    let _ = timeout(
                                        Duration::from_secs(60),
                                        me.route_packet(RouteWeaverPacket::new(
                                            me.public_key,
                                            packet.destination,
                                            Left(&mut new_noise),
                                            Message::Handshake,
                                        )),
                                    )
                                    .await;
                                });
                            } else {
                                // It worked so store it
                                self.noise_cache
                                    .write()
                                    .await
                                    .put(packet.source, Left(new_noise));
                            }
                        }
                    }
                }
            // Transport suffered a fatal error and probably closed
            // Drop the thread
            } else {
                log::error!("Transport {} closed", gateway_address);
                return;
            }
        }
    }

    /// Waits infinitely until a gateway has taken our packet
    pub async fn route_packet(&self, packet: RouteWeaverPacket) {
        log::trace!(
            "Attempting to route packet to address {}",
            packet.destination
        );

        loop {
            let out_writers = self.out_writers.read().await;
            let mut broken_writer = None;

            for (num, writer) in out_writers.iter().enumerate() {
                // Get the first one for us
                if let Ok(mut writer) = writer.try_lock() {
                    match writer.send(packet.clone()).await {
                        Ok(_) => return,
                        Err(err) => {
                            log::warn!("Failed to send packet on: {}", err);
                            broken_writer = Some(num);
                            continue;
                        }
                    }
                }
            }

            if let Some(num) = broken_writer {
                self.out_writers
                    .write()
                    .await
                    .remove(num)
                    .into_inner()
                    .close()
                    .await
                    .unwrap();
            }

            sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn send_message(&self, destination: PublicKey, message: Message) {
        if destination == self.public_key {
            log::trace!("Message from local system routed to local system");

            self.inbox
                .write()
                .await
                .get_or_insert_mut(self.public_key, Default::default)
                .push_overwrite(message);

            return;
        }

        if let Some(noise) = self.noise_cache.write().await.get_mut(&destination) {
            self.route_packet(RouteWeaverPacket::new(
                self.public_key,
                destination,
                noise.as_mut(),
                message,
            ))
            .await;
        } else {
            todo!()
        }
    }

    pub async fn receive_message(&self, destination: PublicKey) -> Message {
        loop {
            let mut inbox_lock = self.inbox.write().await;

            if let Some(inbox) = inbox_lock.get_mut(&self.public_key) {
                if let Some(message) = inbox.pop() {
                    return message;
                }
            }

            sleep(Duration::from_millis(10)).await;
        }
    }
}
