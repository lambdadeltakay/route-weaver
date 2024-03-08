use route_weaver_common::{
    address::TransportAddress,
    error::RouteWeaverError,
    transport::{
        GenericBincodeConnectionReader, GenericBincodeConnectionWriter, Transport,
        TransportConnectionReader, TransportConnectionWriter,
    },
};
use socket2::Socket;
use std::sync::Arc;
use std::{
    net::{IpAddr, SocketAddr},
    pin::Pin,
};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

#[derive(Debug)]
pub struct TcpTransport {
    socket: TcpListener,
}

#[async_trait::async_trait]
impl Transport for TcpTransport {
    async fn arced_new() -> Arc<dyn Transport> {
        let socket = Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .unwrap();

        socket.set_only_v6(false).unwrap();
        socket.set_nonblocking(true).unwrap();

        socket
            .bind(&SocketAddr::new("::".parse().unwrap(), 3434).into())
            .unwrap();
        socket.listen(5).unwrap();

        Arc::new(Self {
            socket: TcpListener::from_std(std::net::TcpListener::from(socket)).unwrap(),
        })
    }

    fn get_protocol_string(&self) -> &'static str {
        "tcp"
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
        let addr = SocketAddr::new(address.data.parse().unwrap(), 3434);

        TcpStream::connect(addr)
            .await
            .map(|stream| {
                let (read, write) = stream.into_split();

                (
                    GenericBincodeConnectionReader::new(read)
                        as Pin<Box<dyn TransportConnectionReader>>,
                    GenericBincodeConnectionWriter::new(write)
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
            .map(|(stream, address)| {
                let (read, write) = stream.into_split();
                (
                    (
                        GenericBincodeConnectionReader::new(read)
                            as Pin<Box<dyn TransportConnectionReader>>,
                        GenericBincodeConnectionWriter::new(write)
                            as Pin<Box<dyn TransportConnectionWriter>>,
                    ),
                    {
                        match address.ip() {
                            IpAddr::V4(ip) => Some(TransportAddress {
                                address_type: "ip".try_into().unwrap(),
                                protocol: self.get_protocol_string().try_into().unwrap(),
                                data: ip.to_string().as_str().try_into().unwrap(),
                                port: Some(address.port()),
                            }),
                            IpAddr::V6(ip) => Some(TransportAddress {
                                address_type: "ip".try_into().unwrap(),
                                protocol: self.get_protocol_string().try_into().unwrap(),
                                data: ip
                                    .to_ipv4_mapped()
                                    .map_or_else(|| ip.to_string(), |ip| ip.to_string())
                                    .as_str()
                                    .try_into()
                                    .unwrap(),
                                port: Some(address.port()),
                            }),
                        }
                    },
                )
            })
            .map_err(RouteWeaverError::Custom)
    }
}
