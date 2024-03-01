use route_weaver_common::{
    message::{
        PacketEncoderDecoder, TransportConnectionReadFramer, TransportConnectionWriteFramer,
    },
    transport::{
        Transport, TransportAddress, TransportConnectionReadHalf, TransportConnectionWriteHalf,
    },
};
use std::pin::pin;
use std::{path::PathBuf, task::Poll};
use tokio::{
    fs::remove_file,
    io::{AsyncRead, AsyncWrite},
    net::{
        unix::{OwnedReadHalf, OwnedWriteHalf},
        UnixListener, UnixStream,
    },
};

#[derive(Debug)]
pub struct UnixTransport {
    socket: UnixListener,
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
    async fn boxed_new() -> Box<dyn Transport> {
        let _ = remove_file("/tmp/route-weaver-unix-transport").await;

        Box::new(Self {
            socket: UnixListener::bind("/tmp/route-weaver-unix-transport").unwrap(),
        })
    }

    fn get_protocol_string() -> &'static str {
        "unix"
    }

    async fn connect(
        &mut self,
        address: TransportAddress,
    ) -> Option<(
        TransportConnectionReadFramer,
        TransportConnectionWriteFramer,
    )> {
        let path = address.data.parse::<PathBuf>().unwrap();

        UnixStream::connect(path).await.ok().map(|stream| {
            let (read, write) = stream.into_split();

            (
                TransportConnectionReadFramer::new(
                    Box::pin(UnixTransportConnectionReadHalf { stream: read }),
                    PacketEncoderDecoder,
                ),
                TransportConnectionWriteFramer::new(
                    Box::pin(UnixTransportConnectionWriteHalf { stream: write }),
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
                        Box::pin(UnixTransportConnectionReadHalf { stream: read }),
                        PacketEncoderDecoder,
                    ),
                    TransportConnectionWriteFramer::new(
                        Box::pin(UnixTransportConnectionWriteHalf { stream: write }),
                        PacketEncoderDecoder,
                    ),
                ),
                addr.as_pathname().map(|path| TransportAddress {
                    protocol: "unix",
                    data: path.to_str().unwrap().to_string(),
                }),
            )
        })
    }
}

#[derive(Debug)]
pub struct UnixTransportConnectionReadHalf {
    stream: OwnedReadHalf,
}

impl TransportConnectionReadHalf for UnixTransportConnectionReadHalf {}

impl AsyncRead for UnixTransportConnectionReadHalf {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_read(cx, buf)
    }
}

#[derive(Debug)]
pub struct UnixTransportConnectionWriteHalf {
    stream: OwnedWriteHalf,
}

impl TransportConnectionWriteHalf for UnixTransportConnectionWriteHalf {}

impl AsyncWrite for UnixTransportConnectionWriteHalf {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        pin!(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_shutdown(cx)
    }
}
