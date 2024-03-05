use bluer::l2cap::{SocketAddr, Stream, StreamListener};
use route_weaver_common::{
    address::TransportAddress, error::RouteWeaverError, transport::{Transport, TransportConnection}
};
use std::{pin::{pin, Pin}, sync::Arc};
use std::task::Poll;
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug)]
pub struct BluetoothTransport {
    socket: StreamListener,
}

#[async_trait::async_trait]
impl Transport for BluetoothTransport {
    async fn arced_new() -> Arc<dyn Transport> {
        Arc::new(Self {
            socket: StreamListener::bind(SocketAddr::any_br_edr())
                .await
                .unwrap(),
        })
    }

    fn get_protocol_string() -> &'static str {
        "bluetooth"
    }

    async fn connect(
        &self,
        address: TransportAddress,
    ) -> Result<Pin<Box<dyn TransportConnection>>, RouteWeaverError> {
        let addr = SocketAddr::new(
            address.data.parse().unwrap(),
            bluer::AddressType::LePublic,
            address.port.unwrap(),
        );

        Stream::connect(addr)
            .await
            .map(|stream| {
                Box::pin(BluetoothTransportConnection { stream })
                    as Pin<Box<dyn TransportConnection>>
            })
            .map_err(RouteWeaverError::Custom)
    }

    async fn accept(
        &self,
    ) -> Result<(Pin<Box<dyn TransportConnection>>, Option<TransportAddress>), RouteWeaverError> {
        self.socket
            .accept()
            .await
            .map(|(stream, addr)| {
                (
                    Box::pin(BluetoothTransportConnection { stream })
                        as Pin<Box<dyn TransportConnection>>,
                    Some(TransportAddress {
                        address_type: "bluetooth".into(),
                        protocol: "bluetooth".into(),
                        data: addr.addr.to_string(),
                        port: Some(addr.psm),
                    }),
                )
            })
            .map_err(RouteWeaverError::Custom)
    }
}

#[derive(Debug)]
pub struct BluetoothTransportConnection {
    stream: Stream,
}

impl TransportConnection for BluetoothTransportConnection {}

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
