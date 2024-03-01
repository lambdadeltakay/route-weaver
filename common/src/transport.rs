use std::{
    fmt::{Debug, Display}, str::FromStr, time::Duration
};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::message::{
    TransportConnectionReadFramer, TransportConnectionWriteFramer, SERIALIZED_PACKET_SIZE_MAX,
};

#[async_trait::async_trait]
pub trait Transport: Send + Sync + Debug + 'static {
    async fn boxed_new() -> Box<dyn Transport>
    where
        Self: Sized;
    fn get_protocol_string() -> &'static str
    where
        Self: Sized;

    async fn connect(
        &mut self,
        _address: TransportAddress,
    ) -> Option<(
        TransportConnectionReadFramer,
        TransportConnectionWriteFramer,
    )> {
        None
    }

    async fn accept(
        &mut self,
    ) -> Option<(
        (
            TransportConnectionReadFramer,
            TransportConnectionWriteFramer,
        ),
        Option<TransportAddress>,
    )> {
        None
    }
}

pub trait TransportConnectionWriteHalf: Send + Sync + Debug + AsyncWrite {
    fn recommended_rate_limit(&self) -> Duration {
        Duration::from_secs(0)
    }

    fn recommended_packet_size(&self) -> usize {
        SERIALIZED_PACKET_SIZE_MAX
    }
}

pub trait TransportConnectionReadHalf: Send + Sync + Debug + AsyncRead {}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct TransportAddress {
    pub protocol: &'static str,
    pub data: String,
}

impl Display for TransportAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.protocol, self.data)
    }
}
