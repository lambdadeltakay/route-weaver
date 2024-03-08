use std::{fmt::Debug, pin::Pin, sync::Arc};

use futures::{prelude::Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::{
    address::TransportAddress,
    error::RouteWeaverError,
    message::{PacketEncoderDecoder, RouteWeaverPacket},
};

#[async_trait::async_trait]
pub trait Transport: Send + Sync + Debug + 'static {
    async fn arced_new() -> Arc<dyn Transport>
    where
        Self: Sized;
    fn get_protocol_string(&self) -> &'static str;

    async fn connect(
        &self,
        _address: TransportAddress,
    ) -> Result<
        (
            Pin<Box<dyn TransportConnectionReader>>,
            Pin<Box<dyn TransportConnectionWriter>>,
        ),
        RouteWeaverError,
    > {
        Err(RouteWeaverError::UnsupportedOperationRequestedOnTransport)
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
        Err(RouteWeaverError::UnsupportedOperationRequestedOnTransport)
    }
}

pub trait TransportConnectionWriter:
    Send + Sync + Debug + Sink<RouteWeaverPacket, Error = RouteWeaverError>
{
}

pub trait TransportConnectionReader:
    Send + Sync + Debug + Stream<Item = Result<RouteWeaverPacket, RouteWeaverError>>
{
}

#[derive(Debug)]
pub struct GenericBincodeConnectionWriter<T: AsyncWrite + Debug + Send + Sync> {
    writer: FramedWrite<Pin<Box<T>>, PacketEncoderDecoder>,
}

impl<T: AsyncWrite + Debug + Send + Sync> GenericBincodeConnectionWriter<T> {
    pub fn new(writer: T) -> Pin<Box<Self>> {
        Box::pin(Self {
            writer: FramedWrite::new(Box::pin(writer), PacketEncoderDecoder::default()),
        })
    }
}

impl<T: AsyncWrite + Debug + Send + Sync> TransportConnectionWriter
    for GenericBincodeConnectionWriter<T>
{
}

impl<T: AsyncWrite + Debug + Send + Sync> Sink<RouteWeaverPacket>
    for GenericBincodeConnectionWriter<T>
{
    type Error = RouteWeaverError;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().writer).poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: RouteWeaverPacket) -> Result<(), Self::Error> {
        Pin::new(&mut self.get_mut().writer).start_send(item)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().writer).poll_flush(cx)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().writer).poll_close(cx)
    }
}

#[derive(Debug)]
pub struct GenericBincodeConnectionReader<T: AsyncRead + Debug + Send + Sync> {
    reader: FramedRead<Pin<Box<T>>, PacketEncoderDecoder>,
}

impl<T: AsyncRead + Debug + Send + Sync> GenericBincodeConnectionReader<T> {
    pub fn new(reader: T) -> Pin<Box<Self>> {
        Box::pin(Self {
            reader: FramedRead::new(Box::pin(reader), PacketEncoderDecoder::default()),
        })
    }
}

impl<T: AsyncRead + Debug + Send + Sync> TransportConnectionReader
    for GenericBincodeConnectionReader<T>
{
}

impl<T: AsyncRead + Debug + Send + Sync> Stream for GenericBincodeConnectionReader<T> {
    type Item = Result<RouteWeaverPacket, RouteWeaverError>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().reader).poll_next(cx)
    }
}

#[allow(clippy::type_complexity)]
pub fn split_connection<T: AsyncRead + AsyncWrite + Debug + Send + Sync + 'static>(
    connection: T,
) -> (
    Pin<Box<dyn TransportConnectionReader>>,
    Pin<Box<dyn TransportConnectionWriter>>,
) {
    let (reader, writer) = tokio::io::split(connection);
    (
        GenericBincodeConnectionReader::new(reader),
        GenericBincodeConnectionWriter::new(writer),
    )
}
