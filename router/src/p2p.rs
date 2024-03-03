use futures::prelude::{sink::SinkExt, stream::StreamExt, Future};
use lru::LruCache;
use ringbuf::Rb;
use ringbuf::StaticRb;
use route_weaver_common::{
    address::TransportAddress,
    message::{
        PacketEncoderDecoder, PeerToPeerMessage, RouteWeaverPacket, TransportConnectionReadFramer,
        TransportConnectionWriteFramer,
    },
    noise::{Noise, PrivateKey, PublicKey},
    transport::Transport,
};
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    pin::Pin,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};
use tokio::{
    io::split,
    sync::{
        mpsc::{channel, Sender},
        Mutex, RwLock,
    },
    time::{interval, sleep, timeout, MissedTickBehavior},
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
        assert!(!self.transports.is_empty());

        let communicator = Arc::new(P2PCommunicator {
            ticket_counter: Mutex::new(0),
            public_key: self.public_key.unwrap(),
            private_key: self.private_key.unwrap(),
            inbox: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
            outbox: RwLock::new(LruCache::new(NonZeroUsize::new(1000).unwrap())),
            remote_packet_outbox: RwLock::new(LruCache::new(NonZeroUsize::new(10).unwrap())),
            writers: RwLock::new(LruCache::new(
                NonZeroUsize::new(self.transports.len()).unwrap(),
            )),
            noise_cache: RwLock::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
        });

        if let Some(seed_node) = self.seed_node {
            tokio::spawn(communicator.clone().keep_up_with_seed_nodes(seed_node));
        }

        let me = communicator.clone();

        tokio::spawn(async move {
            loop {
                let outbox_lock = me.outbox.write().await;
                if outbox_lock.is_empty() {
                    sleep(Duration::from_millis(10)).await;
                    continue;
                }

                let mut writers_lock = me.writers.write().await;
            }
        });

        for (transport_name, transport) in self.transports.drain() {
            let me = communicator.clone();
            let mut transport = transport.await;

            tokio::spawn(async move {
                loop {
                    if let Some((transport, address)) = transport.accept().await {
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
                            .get_or_insert_mut(transport_name, Vec::default)
                            .push(write);

                        tokio::spawn(
                            me.clone()
                                .create_gateway_listener(read, gateway_address.clone()),
                        );
                    }
                }
            });
        }

        communicator
    }
}

pub struct P2PCommunicator {
    ticket_counter: Mutex<u16>,
    public_key: PublicKey,
    private_key: PrivateKey,
    inbox: RwLock<LruCache<PublicKey, StaticRb<PeerToPeerMessage, 1000>>>,
    outbox: RwLock<LruCache<u16, (PeerToPeerMessage, PublicKey)>>,
    remote_packet_outbox: RwLock<LruCache<PublicKey, RouteWeaverPacket>>,
    writers: RwLock<LruCache<&'static str, Vec<TransportConnectionWriteFramer>>>,
    noise_cache: RwLock<LruCache<PublicKey, Noise>>,
}

impl P2PCommunicator {
    async fn keep_up_with_seed_nodes(self: Arc<Self>, seed_node: TransportAddress) {
        let mut counter = interval(Duration::from_secs(60));
        counter.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            counter.tick().await;
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

                        self.remote_packet_outbox
                            .write()
                            .await
                            .push(packet.destination, packet);
                    } else {
                        let message = self
                            .noise_cache
                            .write()
                            .await
                            .get_or_insert_mut(packet.source, Noise::default);
                    }
                }
            // Transport suffered a fatal error and probably closed
            // Drop the thread
            } else {
                log::error!("Connection from {} closed", gateway_address);
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
            let mut ticket_lock = self.ticket_counter.lock().await;
            let ticket = *ticket_lock;
            *ticket_lock = ticket_lock.wrapping_add(1);

            log::trace!("Creating ticket for message {}", ticket);

            self.outbox
                .write()
                .await
                .push(ticket, (message, destination));

            loop {
                if !self.outbox.read().await.contains(&ticket) {
                    // Either our packet was sent or the LRU cache overflowed / our ticket overflowed (this one is unlikely)
                    log::trace!("Message with ticket {} sent or dropped", ticket);

                    return;
                } else {
                    sleep(Duration::from_millis(10)).await
                }
            }
        }
    }

    pub async fn receive_message(&self, destination: PublicKey) -> PeerToPeerMessage {
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
}
