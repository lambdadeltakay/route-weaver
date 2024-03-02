use route_weaver_common::{
    message::{
        PacketEncoderDecoder, TransportConnectionReadFramer, TransportConnectionWriteFramer,
    },
    transport::{
        Transport, TransportAddress, TransportConnectionReadHalf, TransportConnectionWriteHalf,
    },
};
use socket2::Socket;
use std::net::SocketAddr;
use std::pin::pin;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener, TcpStream,
    },
};

// We use two listeners because operating system like BSD can't do dual stack
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

        // These options are technically not strictly required
        let _ = socket.set_only_v6(false);
        let _ = socket.set_cloexec(true);
        let _ = socket.set_nonblocking(true);

        socket
            .bind(&SocketAddr::new("::".parse().unwrap(), 3434).into())
            .unwrap();
        socket.listen(1).unwrap();

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
    ) -> Option<(
        TransportConnectionReadFramer,
        TransportConnectionWriteFramer,
    )> {
        let addr = SocketAddr::new(address.data.parse().unwrap(), 3434);

        let stream = TcpStream::connect(addr).await.ok()?;
        let (read, write) = stream.into_split();

        Some((
            TransportConnectionReadFramer::new(
                Box::pin(TcpTransportConnectionReadHalf { stream: read }),
                PacketEncoderDecoder,
            ),
            TransportConnectionWriteFramer::new(
                Box::pin(TcpTransportConnectionWriteHalf { stream: write }),
                PacketEncoderDecoder,
            ),
        ))
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
        let stream = self.socket.accept().await.map(|(stream, _)| stream).ok()?;
        let (read, write) = stream.into_split();
        Some((
            (
                TransportConnectionReadFramer::new(
                    Box::pin(TcpTransportConnectionReadHalf { stream: read }),
                    PacketEncoderDecoder,
                ),
                TransportConnectionWriteFramer::new(
                    Box::pin(TcpTransportConnectionWriteHalf { stream: write }),
                    PacketEncoderDecoder,
                ),
            ),
            None,
        ))
    }
}

#[derive(Debug)]
struct TcpTransportConnectionReadHalf {
    stream: OwnedReadHalf,
}

impl TransportConnectionReadHalf for TcpTransportConnectionReadHalf {}

impl AsyncRead for TcpTransportConnectionReadHalf {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_read(cx, buf)
    }
}

#[derive(Debug)]
struct TcpTransportConnectionWriteHalf {
    stream: OwnedWriteHalf,
}

impl TransportConnectionWriteHalf for TcpTransportConnectionWriteHalf {}

impl AsyncWrite for TcpTransportConnectionWriteHalf {
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
