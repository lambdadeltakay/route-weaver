use route_weaver_common::address::TransportAddress;
use std::collections::HashSet;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct ConnectionManager {
    pub tracked_connections: HashSet<TransportAddress>,
}

impl ConnectionManager {
    pub fn is_tracked(&self, address: &TransportAddress) -> bool {
        self.tracked_connections.contains(&address.without_port())
    }

    pub fn track_connection(&mut self, address: &TransportAddress) {
        self.tracked_connections.insert(address.without_port());
    }

    pub fn untrack_connection(&mut self, address: &TransportAddress) {
        if !self.tracked_connections.remove(&address.without_port()) {
            log::warn!(
                "Tried to untrack a connection that we have no evidence of happening: {}",
                address
            );
        }
    }
}
