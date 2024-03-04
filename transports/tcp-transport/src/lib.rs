use route_weaver_common::{
    address::TransportAddress,
    error::RouteWeaverError,
    transport::{Transport, TransportConnection},
};
use socket2::Socket;
use std::pin::pin;
use std::{
    net::{IpAddr, SocketAddr},
    pin::Pin,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
};

#[derive(Debug)]
pub struct TcpTransport {
    socket: TcpListener,
}

#[async_trait::async_trait]
impl Transport for TcpTransport {
    async fn boxed_new() -> Box<dyn Transport> {
        let socket = Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .unwrap();

        let _ = socket.set_cloexec(true);
        let _ = socket.set_only_v6(false);
        let _ = socket.set_nonblocking(true);

        socket
            .bind(&SocketAddr::new("::".parse().unwrap(), 3434).into())
            .unwrap();
        socket.listen(5).unwrap();

        Box::new(Self {
            socket: TcpListener::from_std(std::net::TcpListener::from(socket)).unwrap(),
        })
    }

    fn get_protocol_string() -> &'static str {
        "tcp"
    }

    async fn connect(
        &mut self,
        address: TransportAddress,
    ) -> Result<Pin<Box<dyn TransportConnection>>, RouteWeaverError> {
        let addr = SocketAddr::new(address.data.parse().unwrap(), 3434);

        TcpStream::connect(addr)
            .await
            .map(|stream| {
                Box::pin(TcpTransportConnection { stream }) as Pin<Box<dyn TransportConnection>>
            })
            .map_err(RouteWeaverError::Custom)
    }

    async fn accept(
        &mut self,
    ) -> Result<(Pin<Box<dyn TransportConnection>>, Option<TransportAddress>), RouteWeaverError>
    {
        self.socket
            .accept()
            .await
            .map(|(stream, address)| {
                (
                    Box::pin(TcpTransportConnection { stream })
                        as Pin<Box<dyn TransportConnection>>,
                    {
                        match address.ip() {
                            IpAddr::V4(ip) => Some(TransportAddress {
                                address_type: "ip".to_string(),
                                protocol: "tcp".to_string(),
                                data: ip.to_string(),
                                port: Some(address.port()),
                            }),
                            IpAddr::V6(ip) => Some(TransportAddress {
                                address_type: "ip".to_string(),
                                protocol: "tcp".to_string(),
                                data: ip
                                    .to_ipv4_mapped()
                                    .map_or_else(|| ip.to_string(), |ip| ip.to_string()),
                                port: Some(address.port()),
                            }),
                        }
                    },
                )
            })
            .map_err(RouteWeaverError::Custom)
    }
}

#[derive(Debug)]
struct TcpTransportConnection {
    stream: TcpStream,
}

impl TransportConnection for TcpTransportConnection {}

impl AsyncRead for TcpTransportConnection {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TcpTransportConnection {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        pin!(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_shutdown(cx)
    }
}
