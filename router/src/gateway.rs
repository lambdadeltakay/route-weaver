use std::{
    collections::{HashMap, HashSet},
    pin::Pin,
};

use dashmap::DashMap;
use hashlink::LruCache;
use route_weaver_common::{
    address::TransportAddress,
    message::RouteWeaverPacket,
    noise::PublicKey,
    transport::{TransportConnectionReader, TransportConnectionWriter},
};

#[derive(Default)]
pub struct GatewayScoreEntry {
    pub relative_score: u8,
}

pub struct GatewayManager {
    readers:
        HashMap<PublicKey, LruCache<TransportAddress, Pin<Box<dyn TransportConnectionReader>>>>,
    writers:
        HashMap<PublicKey, LruCache<TransportAddress, Pin<Box<dyn TransportConnectionWriter>>>>,
    gateway_scores: LruCache<TransportAddress, GatewayScoreEntry>,
    gateway_blacklist: HashSet<TransportAddress>,
}

impl GatewayManager {
    pub fn new() -> Self {
        Self {
            readers: HashMap::new(),
            writers: HashMap::new(),
            gateway_scores: LruCache::new(100),
            gateway_blacklist: HashSet::new(),
        }
    }
}

impl GatewayManager {
    pub fn register_gateway(&mut self, gateway: TransportAddress) {
        if !self.gateway_blacklist.contains(&gateway) {
            self.gateway_scores
                .entry(gateway)
                .or_insert_with(GatewayScoreEntry::default);
        }
    }

    pub fn is_gateway_blacklisted(&self, gateway: TransportAddress) -> bool {
        self.gateway_blacklist.contains(&gateway)
    }

    pub fn blacklist_gateway(&mut self, gateway: TransportAddress) {
        self.gateway_blacklist.insert(gateway);
    }

    

    pub async fn route_packet(&mut self, packet: RouteWeaverPacket, high_priority: bool) {
        let Some((best_gateway, _)) = self
            .gateway_scores
            .iter()
            .max_by_key(|(address, entry)| entry.relative_score)
        else {
            return;
        };


    }
}
