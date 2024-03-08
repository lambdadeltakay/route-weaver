use route_weaver_common::{
    address::TransportAddress,
    error::RouteWeaverError,
    transport::{
        GenericBincodeConnectionReader, GenericBincodeConnectionWriter, Transport,
        TransportConnectionReader, TransportConnectionWriter,
    },
};
use std::path::PathBuf;
use std::{pin::Pin, sync::Arc};
use tokio::{
    fs::remove_file,
    net::{UnixListener, UnixStream},
};

#[derive(Debug)]
pub struct UnixTransport {
    socket: UnixListener,
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
    async fn arced_new() -> Arc<dyn Transport> {
        let tmpdir = std::env::temp_dir();
        let socket_path = tmpdir.join(env!("CARGO_CRATE_NAME"));

        let _ = remove_file(socket_path.clone()).await;

        Arc::new(Self {
            socket: UnixListener::bind(socket_path).unwrap(),
        })
    }

    fn get_protocol_string(&self) -> &'static str {
        "unix"
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
        let path = address
            .data
            .parse::<PathBuf>()
            .map_err(|_| RouteWeaverError::AddressFailedToParse)?;

        UnixStream::connect(path)
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
            .map(|(stream, addr)| {
                let (read, write) = stream.into_split();

                (
                    (
                        GenericBincodeConnectionReader::new(read)
                            as Pin<Box<dyn TransportConnectionReader>>,
                        GenericBincodeConnectionWriter::new(write)
                            as Pin<Box<dyn TransportConnectionWriter>>,
                    ),
                    addr.as_pathname().map(|path| TransportAddress {
                        address_type: "unix".try_into().unwrap(),
                        protocol: "unix".try_into().unwrap(),
                        data: path.to_string_lossy().as_ref().try_into().unwrap(),
                        port: None,
                    }),
                )
            })
            .map_err(RouteWeaverError::Custom)
    }
}
