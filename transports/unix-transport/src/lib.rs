use route_weaver_common::{
    address::TransportAddress,
    error::RouteWeaverError,
    transport::{Transport, TransportConnection},
};
use std::pin::{pin, Pin};
use std::{path::PathBuf, task::Poll};
use tokio::{
    fs::remove_file,
    io::{AsyncRead, AsyncWrite},
    net::{UnixListener, UnixStream},
};

#[derive(Debug)]
pub struct UnixTransport {
    socket: UnixListener,
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
    async fn boxed_new() -> Box<dyn Transport> {
        let tmpdir = std::env::temp_dir();
        let socket_path = tmpdir.join(env!("CARGO_CRATE_NAME"));

        let _ = remove_file(socket_path.clone()).await;

        Box::new(Self {
            socket: UnixListener::bind(socket_path).unwrap(),
        })
    }

    fn get_protocol_string() -> &'static str {
        "unix"
    }

    async fn connect(
        &mut self,
        address: TransportAddress,
    ) -> Result<Pin<Box<dyn TransportConnection>>, RouteWeaverError> {
        let path = address
            .data
            .parse::<PathBuf>()
            .map_err(|_| RouteWeaverError::AddressFailedToParse)?;

        UnixStream::connect(path)
            .await
            .map(|stream| {
                Box::pin(UnixTransportConnection { stream }) as Pin<Box<dyn TransportConnection>>
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
            .map(|(stream, addr)| {
                (
                    Box::pin(UnixTransportConnection { stream })
                        as Pin<Box<dyn TransportConnection>>,
                    addr.as_pathname().map(|path| TransportAddress {
                        address_type: "unix".into(),
                        protocol: "unix".into(),
                        data: path.to_string_lossy().into_owned(),
                        port: None,
                    }),
                )
            })
            .map_err(RouteWeaverError::Custom)
    }
}

#[derive(Debug)]
pub struct UnixTransportConnection {
    stream: UnixStream,
}

impl TransportConnection for UnixTransportConnection {}

impl AsyncRead for UnixTransportConnection {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        pin!(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnixTransportConnection {
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
