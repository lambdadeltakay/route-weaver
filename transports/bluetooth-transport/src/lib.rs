use bluer::l2cap::{SocketAddr, Stream, StreamListener};
use route_weaver_common::{
    address::TransportAddress,
    error::RouteWeaverError,
    transport::{
        GenericBincodeConnectionReader, GenericBincodeConnectionWriter, Transport,
        TransportConnectionReader, TransportConnectionWriter,
    },
};
use std::{pin::Pin, sync::Arc};

#[derive(Debug)]
pub struct BluetoothTransport {
    socket: StreamListener,
}

#[async_trait::async_trait]
impl Transport for BluetoothTransport {
    async fn arced_new() -> Arc<dyn Transport> {
        Arc::new(Self {
            socket: StreamListener::bind(SocketAddr::any_le())
                .await
                .unwrap(),
        })
    }

    fn get_protocol_string(&self) -> &'static str {
        "bluetooth"
    }

    async fn connect(
        &self,
        address: TransportAddress,
    ) -> Result<
        (
            Pin<Box<dyn TransportConnectionReader>>,
            Pin<Box<dyn TransportConnectionWriter>>,
        ),
        RouteWeaverError,
    > {
        let addr = SocketAddr::new(
            address.data.parse().unwrap(),
            bluer::AddressType::LePublic,
            address.port.unwrap(),
        );

        Stream::connect(addr)
            .await
            .map(|stream| {
                let (reader, writer) = stream.into_split();
                (
                    GenericBincodeConnectionReader::new(reader)
                        as Pin<Box<dyn TransportConnectionReader>>,
                    GenericBincodeConnectionWriter::new(writer)
                        as Pin<Box<dyn TransportConnectionWriter>>,
                )
            })
            .map_err(RouteWeaverError::Custom)
    }

    async fn accept(
        &self,
    ) -> Result<
        (
            (
                Pin<Box<dyn TransportConnectionReader>>,
                Pin<Box<dyn TransportConnectionWriter>>,
            ),
            Option<TransportAddress>,
        ),
        RouteWeaverError,
    > {
        self.socket
            .accept()
            .await
            .map(|(stream, addr)| {
                let (reader, writer) = stream.into_split();
                (
                    (
                        GenericBincodeConnectionReader::new(reader)
                            as Pin<Box<dyn TransportConnectionReader>>,
                        GenericBincodeConnectionWriter::new(writer)
                            as Pin<Box<dyn TransportConnectionWriter>>,
                    ),
                    Some(TransportAddress {
                        address_type: "bluetooth".try_into().unwrap(),
                        protocol: self.get_protocol_string().try_into().unwrap(),
                        data: addr.addr.to_string().as_str().try_into().unwrap(),
                        port: Some(addr.psm),
                    }),
                )
            })
            .map_err(RouteWeaverError::Custom)
    }
}
