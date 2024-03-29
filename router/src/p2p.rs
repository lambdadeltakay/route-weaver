use crate::{message_socket::RouteWeaverSocket, noise::Noise};
use dashmap::{DashMap, DashSet};
use futures::prelude::{sink::SinkExt, Future};
use hashlink::LruCache;
use rand::prelude::{IteratorRandom, SeedableRng, SmallRng};
use ringbuf::Rb;
use ringbuf::StaticRb;
use route_weaver_common::{
    address::TransportAddress,
    message::{PeerToPeerMessage, RouteWeaverPacket},
    noise::{PrivateKey, PublicKey},
    router::ApplicationId,
    transport::{Transport, TransportConnectionReader, TransportConnectionWriter},
};
use sha2::Digest;
use sha2::Sha256;
use std::{
    collections::{BTreeSet, HashMap},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, time::interval};
use tokio::{sync::RwLock, time::timeout};
use tokio_stream::StreamExt;

#[derive(Default)]
pub struct P2PCommunicatorBuilder {
    #[allow(clippy::type_complexity)]
    transports: Vec<Pin<Box<dyn Future<Output = Arc<dyn Transport>>>>>,
    public_key: Option<PublicKey>,
    private_key: Option<PrivateKey>,
    seed_node: Vec<TransportAddress>,
}

impl P2PCommunicatorBuilder {
    pub fn add_transport<T: Transport + 'static>(mut self) -> Self {
        self.transports.push(T::arced_new());
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

    pub async fn build(mut self) -> Arc<RouteWeaverTransport> {
        assert!(
            !self.transports.is_empty(),
            "No transports were passed into the builder"
        );

        let mut buffer = StaticRb::default();
        buffer.push_iter(&mut self.seed_node.into_iter());

        let mut transports = HashMap::new();
        for transport in self.transports.drain(..) {
            let transport = transport.await;
            log::info!("Adding protocol type {}", transport.get_protocol_string());
            transports.insert(transport.get_protocol_string(), transport);
        }
        let communicator = Arc::new(RouteWeaverTransport {
            known_gateways: RwLock::new(Box::new(buffer)),
            noise: RwLock::new(Noise::new(
                self.private_key.unwrap(),
                self.public_key.unwrap(),
            )),
            transports,
            connection_tracker: DashSet::new(),
            writers: DashMap::new(),
            listeners: DashMap::new(),
        });

        let (packet_router_pipe_sender, packet_router_pipe_receiver) = mpsc::channel(10000);
        let (transport_packet_pipe_sender, transport_packet_pipe_receiver) = mpsc::channel(10000);

        tokio::spawn(
            communicator
                .clone()
                .packet_router_task(packet_router_pipe_receiver),
        );

        for transport in communicator.transports.values() {
            tokio::spawn(communicator.clone().connection_accepter_task(
                transport.clone(),
                transport_packet_pipe_sender.clone(),
                packet_router_pipe_sender.clone(),
            ));
        }

        tokio::spawn(communicator.clone().occasional_gateway_contactor_task(
            transport_packet_pipe_sender.clone(),
            packet_router_pipe_sender.clone(),
        ));

        let (message_pipe_sender, message_pipe_receiver) = mpsc::channel(10000);

        tokio::spawn(communicator.clone().packet_sorter_task(
            message_pipe_sender,
            packet_router_pipe_sender.clone(),
            transport_packet_pipe_receiver,
        ));

        tokio::spawn(
            communicator
                .clone()
                .message_handler_task(message_pipe_receiver, packet_router_pipe_sender),
        );

        communicator
    }
}

pub struct RouteWeaverSocketHandle {
    to_socket: mpsc::Sender<Vec<u8>>,
    from_socket: mpsc::Receiver<Vec<u8>>,
}

pub struct RouteWeaverTransport {
    noise: RwLock<Noise>,
    transports: HashMap<&'static str, Arc<dyn Transport>>,
    known_gateways: RwLock<Box<StaticRb<TransportAddress, 100>>>,
    writers: DashMap<TransportAddress, mpsc::Sender<RouteWeaverPacket>>,
    listeners: DashMap<(PublicKey, ApplicationId), RouteWeaverSocketHandle>,
    connection_tracker: DashSet<TransportAddress>,
}

impl RouteWeaverTransport {
    pub async fn accept(self: Arc<Self>, id: ApplicationId) -> RouteWeaverSocket {
        todo!()
    }

    pub async fn connect(
        self: Arc<Self>,
        id: ApplicationId,
        remote: PublicKey,
    ) -> RouteWeaverSocket {
        todo!()
    }

    async fn packet_router_task(
        self: Arc<Self>,
        mut packet_router_pipe: mpsc::Receiver<RouteWeaverPacket>,
    ) {
        loop {
            if let Some(packet) = packet_router_pipe.recv().await {
                // TODO: make actual routing algorithm
                self.writers
                    .iter()
                    .next()
                    .unwrap()
                    .send(packet)
                    .await
                    .unwrap();
            }
        }
    }

    async fn packet_sender_task(
        self: Arc<Self>,
        address: TransportAddress,
        mut writer: Pin<Box<dyn TransportConnectionWriter>>,
        // Receives packets that need to be sent
        mut writer_pipe: mpsc::Receiver<RouteWeaverPacket>,
        // For sending packets back that failed to send
        packet_router_pipe: mpsc::Sender<RouteWeaverPacket>,
    ) {
        loop {
            if !self.writers.contains_key(&address) {
                break;
            }

            if let Some(packet) = writer_pipe.recv().await {
                match writer.send(packet.clone()).await {
                    Ok(_) => {
                        log::trace!("Sent packet to: {}", address);
                    }
                    Err(err) => {
                        log::error!("Error writing to transport: {}, sending for rerouting", err);
                        packet_router_pipe.send(packet).await.unwrap();
                        self.writers.remove(&address);
                        break;
                    }
                }
            }
        }
    }

    async fn packet_listener_task(
        self: Arc<Self>,
        address: TransportAddress,
        mut reader: Pin<Box<dyn TransportConnectionReader>>,
        // Sends that need to be evaluated
        transport_packet_pipe: mpsc::Sender<RouteWeaverPacket>,
    ) {
        self.connection_tracker.insert(address.clone());

        loop {
            if let Some(packet) = reader.next().await {
                match packet {
                    Ok(packet) => {
                        transport_packet_pipe.send(packet).await.unwrap();
                    }
                    Err(err) => {
                        log::error!("Error reading from transport: {}", err);
                        self.writers.remove(&address);
                        break;
                    }
                }
            }
        }

        self.connection_tracker.remove(&address);
    }

    async fn connection_accepter_task(
        self: Arc<Self>,
        transport: Arc<dyn Transport>,
        transport_packet_pipe: mpsc::Sender<RouteWeaverPacket>,
        packet_router_pipe: mpsc::Sender<RouteWeaverPacket>,
    ) {
        loop {
            if let Ok(((reader, writer), Some(address))) = transport.accept().await {
                log::info!("Accepted connection from {}", address);

                let (writer_pipe_sender, writer_pipe_receiver) = mpsc::channel(100);

                self.writers.insert(address.clone(), writer_pipe_sender);

                tokio::spawn(self.clone().packet_sender_task(
                    address.clone(),
                    writer,
                    writer_pipe_receiver,
                    packet_router_pipe.clone(),
                ));

                tokio::spawn(self.clone().packet_listener_task(
                    address,
                    reader,
                    transport_packet_pipe.clone(),
                ));
            }
        }
    }

    async fn occasional_gateway_contactor_task(
        self: Arc<Self>,
        transport_packet_pipe: mpsc::Sender<RouteWeaverPacket>,
        packet_router_pipe: mpsc::Sender<RouteWeaverPacket>,
    ) {
        let mut rng = SmallRng::from_entropy();
        let mut interval = interval(Duration::from_secs(30));

        loop {
            // Go talk to someone
            let gateways_lock = self.known_gateways.read().await;

            if let Some(address) = gateways_lock
                .iter()
                .filter(|address| self.connection_tracker.contains(address))
                .choose(&mut rng)
                .cloned()
            {
                drop(gateways_lock);

                log::info!("Contacting {}", address);

                let transport = self.transports.get(address.protocol.as_str()).unwrap();

                if let Ok(Ok((reader, writer))) =
                    timeout(Duration::from_secs(1), transport.connect(address.clone())).await
                {
                    log::info!("Connected to {}", address);

                    let (writer_pipe_sender, writer_pipe_receiver) = mpsc::channel(100);

                    self.writers.insert(address.clone(), writer_pipe_sender);

                    tokio::spawn(self.clone().packet_sender_task(
                        address.clone(),
                        writer,
                        writer_pipe_receiver,
                        packet_router_pipe.clone(),
                    ));

                    tokio::spawn(self.clone().packet_listener_task(
                        address,
                        reader,
                        transport_packet_pipe.clone(),
                    ));
                } else {
                    log::info!("Failed to connect to {}", address);
                }
            }

            interval.tick().await;
        }
    }

    async fn message_handler_task(
        self: Arc<Self>,
        mut message_pipe: mpsc::Receiver<(PublicKey, PeerToPeerMessage)>,
        packet_router_pipe: mpsc::Sender<RouteWeaverPacket>,
    ) {
        let mut application_data_recv_state = LruCache::new(100);

        loop {
            let (public_key, message) = message_pipe.recv().await.unwrap();

            match message {
                PeerToPeerMessage::Handshake => (),
                PeerToPeerMessage::Confirm => todo!(),
                PeerToPeerMessage::Deny => todo!(),
                PeerToPeerMessage::StartApplicationData { id } => {
                    application_data_recv_state
                        .insert((public_key, id), ApplicationDataRecvStateEntry::default());
                }
                PeerToPeerMessage::ApplicationData { id, index, data } => {
                    if let Some(state) = application_data_recv_state.get_mut(&(public_key, id)) {
                        state.hasher.update(&data);
                        state.counts[index as usize] = Some(data);
                    }
                }
                PeerToPeerMessage::EndApplicationData {
                    id,
                    total_sent,
                    sha256,
                } => {
                    if let Some(state) =
                        application_data_recv_state.remove(&(public_key, id.clone()))
                    {
                        let final_sha256: [u8; 32] = state.hasher.finalize().into();

                        let mut missing_indexes = BTreeSet::new();

                        if final_sha256 != sha256 {
                            log::warn!("Application data segment sent is corrupted");

                            for x in 0..total_sent {
                                if state.counts[x as usize].is_none() {
                                    missing_indexes.insert(x);
                                }
                            }

                            if !missing_indexes.is_empty() {
                                log::warn!("Missing indexes: {:?}", missing_indexes);
                            }

                            let response = PeerToPeerMessage::ApplicationDataProblem {
                                id,
                                missing_indexes,
                            };

                            let packet = self
                                .noise
                                .write()
                                .await
                                .try_encrypt(response, public_key)
                                .unwrap();
                            packet_router_pipe.send(packet).await.unwrap();
                        } else {
                            let listener =
                                self.listeners.get_mut(&(public_key, id.clone())).unwrap();

                            for application_data in state.counts {}

                            let response = PeerToPeerMessage::ApplicationDataOk { id };
                        }
                    } else {
                        log::warn!("Received EndApplicationData without StartApplicationData");
                    }
                }
                PeerToPeerMessage::ApplicationDataProblem {
                    id,
                    missing_indexes,
                } => todo!(),

                PeerToPeerMessage::ApplicationDataOk { id } => (),
                PeerToPeerMessage::RequestPeerList => todo!(),
                PeerToPeerMessage::PeerList { peer_list } => todo!(),
            }
        }
    }

    async fn packet_sorter_task(
        self: Arc<Self>,
        message_pipe: mpsc::Sender<(PublicKey, PeerToPeerMessage)>,
        packet_router_pipe: mpsc::Sender<RouteWeaverPacket>,
        mut transport_packet_pipe: mpsc::Receiver<RouteWeaverPacket>,
    ) {
        loop {
            if let Some(packet) = transport_packet_pipe.recv().await {
                log::info!("Found a packet");
                log::info!(
                    "This packet claims to be from {} and going to {}",
                    packet.source,
                    packet.destination
                );
                log::info!(
                    "The message is {} bytes long and claims to have pre encryption transformation {:?}", 
                    packet.message.len(),
                    packet.pre_encryption_transformation
                );

                let mut noise_lock = self.noise.write().await;

                if packet.message.is_empty() {
                    log::warn!("Dropping empty packet from {}", packet.source);
                } else if packet.source == packet.destination {
                    log::warn!(
                        "Packet from {} set to route back to destination. Rejecting such packet",
                        packet.source
                    );
                } else if noise_lock.public_key != packet.destination {
                    log::trace!("Rerouting packet to {}", packet.destination);
                    packet_router_pipe.send(packet).await.unwrap();
                } else {
                    log::trace!("Received packet from {}", packet.source);

                    match noise_lock.try_decrypt(packet.clone()) {
                        Ok(message) => {
                            message_pipe.send((packet.source, message)).await.unwrap();
                        }
                        Err(err) => {
                            log::warn!("Failed to decrypt packet: {}", err);
                        }
                    }
                }
            }
        }
    }
}

pub struct ApplicationDataRecvStateEntry {
    hasher: Sha256,
    counts: [Option<Vec<u8>>; 256],
}

impl ApplicationDataRecvStateEntry {
    const DEFAULT_VALUE: Option<Vec<u8>> = None;
}

impl Default for ApplicationDataRecvStateEntry {
    fn default() -> Self {
        Self {
            hasher: Sha256::new(),
            counts: [Self::DEFAULT_VALUE; 256],
        }
    }
}
