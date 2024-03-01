use bluer::l2cap::{
    stream::{OwnedReadHalf, OwnedWriteHalf},
    SocketAddr, Stream, StreamListener,
};
use route_weaver_common::{
    message::{
        PacketEncoderDecoder, TransportConnectionReadFramer, TransportConnectionWriteFramer,
    },
    transport::{
        Transport, TransportAddress, TransportConnectionReadHalf, TransportConnectionWriteHalf,
    },
};
use std::pin::pin;
use std::task::Poll;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug)]
pub struct BluetoothTransport {
    socket: StreamListener,
}

#[async_trait::async_trait]
impl Transport for BluetoothTransport {
    async fn boxed_new() -> Box<dyn Transport> {
        Box::new(Self {
            socket: StreamListener::bind(SocketAddr::any_br_edr())
                .await
                .unwrap(),
        })
    }

    fn get_protocol_string() -> &'static str {
        "bluetooth"
    }

    async fn connect(
        &mut self,
        address: TransportAddress,
    ) -> Option<(
        TransportConnectionReadFramer,
        TransportConnectionWriteFramer,
    )> {
        let addr = SocketAddr::new(
            address.data.parse().unwrap(),
            bluer::AddressType::LePublic,
            3434,
        );
        Stream::connect(addr).await.ok().map(|stream| {
            let (read, write) = stream.into_split();

            (
                TransportConnectionReadFramer::new(
                    Box::pin(BluetoothTransportConnectionReadHalf { stream: read }),
                    PacketEncoderDecoder,
                ),
                TransportConnectionWriteFramer::new(
                    Box::pin(BluetoothTransportConnectionWriteHalf { stream: write }),
                    PacketEncoderDecoder,
                ),
            )
        })
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
        self.socket.accept().await.ok().map(|(stream, addr)| {
            let (read, write) = stream.into_split();
            (
                (
                    TransportConnectionReadFramer::new(
                        Box::pin(BluetoothTransportConnectionReadHalf { stream: read }),
                        PacketEncoderDecoder,
                    ),
                    TransportConnectionWriteFramer::new(
                        Box::pin(BluetoothTransportConnectionWriteHalf { stream: write }),
                        PacketEncoderDecoder,
                    ),
                ),
                Some(TransportAddress {
                    protocol: "bluetooth",
                    data: addr.addr.to_string(),
                }),
            )
        })
    }
}

#[derive(Debug)]
pub struct BluetoothTransportConnectionWriteHalf {
    stream: OwnedWriteHalf,
}

impl TransportConnectionWriteHalf for BluetoothTransportConnectionWriteHalf {}

impl AsyncWrite for BluetoothTransportConnectionWriteHalf {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        pin!(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        pin!(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        pin!(&mut self.stream).poll_shutdown(cx)
    }
}

#[derive(Debug)]
pub struct BluetoothTransportConnectionReadHalf {
    stream: OwnedReadHalf,
}

impl TransportConnectionReadHalf for BluetoothTransportConnectionReadHalf {}

impl AsyncRead for BluetoothTransportConnectionReadHalf {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_read(cx, buf)
    }
}

#[derive(Debug)]
pub struct BluetoothTransportConnection {
    stream: Stream,
}

impl AsyncRead for BluetoothTransportConnection {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for BluetoothTransportConnection {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        pin!(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        pin!(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        pin!(&mut self.stream).poll_shutdown(cx)
    }
}
