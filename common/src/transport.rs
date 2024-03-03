use std::{fmt::Debug, pin::Pin, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{address::TransportAddress, message::SERIALIZED_PACKET_SIZE_MAX};

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
    ) -> Option<Pin<Box<dyn TransportConnection>>> {
        None
    }

    async fn accept(
        &mut self,
    ) -> Option<(Pin<Box<dyn TransportConnection>>, Option<TransportAddress>)> {
        None
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
