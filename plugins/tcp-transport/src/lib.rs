use route_weaver_common::{
    message::{
        PacketEncoderDecoder, TransportConnectionReadFramer, TransportConnectionWriteFramer,
    },
    transport::{
        Transport, TransportAddress, TransportConnectionReadHalf, TransportConnectionWriteHalf,
    },
};
use std::pin::pin;
use std::{net::SocketAddr, time::Duration};
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
    socket4: TcpListener,
    socket6: TcpListener,
}

#[async_trait::async_trait]
impl Transport for TcpTransport {
    async fn boxed_new() -> Box<dyn Transport> {
        Box::new(Self {
            socket4: TcpListener::bind(("0.0.0.0", 3434)).await.unwrap(),
            socket6: TcpListener::bind(("::", 3434)).await.unwrap(),
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
        // Accept incoming connections on both IPv4 and IPv6 listeners
        // This assumes that two don't try to connect at the same time. Oops
        let (stream, addr) = tokio::select! {
            Ok((stream, addr)) = self.socket4.accept() => (stream, addr),
            Ok((stream, addr)) = self.socket6.accept() => (stream, addr),
        };

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
            Some(TransportAddress {
                protocol: "tcp",
                data: addr.ip().to_string(),
            }),
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
