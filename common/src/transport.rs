use std::{fmt::Debug, pin::Pin, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    address::TransportAddress, error::RouteWeaverError, message::SERIALIZED_PACKET_SIZE_MAX,
};

#[async_trait::async_trait]
pub trait Transport: Send + Sync + Debug + 'static {
    async fn arced_new() -> Arc<dyn Transport>
    where
        Self: Sized;
    fn get_protocol_string() -> &'static str
    where
        Self: Sized;

    async fn connect(
        &self,
        _address: TransportAddress,
    ) -> Result<Pin<Box<dyn TransportConnection>>, RouteWeaverError> {
        Err(RouteWeaverError::UnsupportedOperationRequestedOnTransport)
    }

    async fn accept(
        &self,
    ) -> Result<(Pin<Box<dyn TransportConnection>>, Option<TransportAddress>), RouteWeaverError>
    {
        Err(RouteWeaverError::UnsupportedOperationRequestedOnTransport)
    }
}

pub trait TransportConnection: Send + Sync + Debug + AsyncWrite + AsyncRead {
    fn recommended_rate_limit(&self) -> Duration {
        Duration::from_secs(0)
    }

    fn recommended_packet_size(&self) -> usize {
        SERIALIZED_PACKET_SIZE_MAX
    }
}
