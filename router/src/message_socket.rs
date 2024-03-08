use crate::noise::Noise;
use futures::{prelude::sink::SinkExt, ready, Sink, Stream};
use route_weaver_common::{
    error::RouteWeaverError,
    noise::{PrivateKey, PublicKey},
};
use std::{pin::Pin, task::Poll};
use tokio::sync::mpsc::Sender;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc::Receiver,
};
use tokio_util::sync::PollSender;

#[derive(Debug)]
pub struct RouteWeaverSocket {
    out_buf: PollSender<Vec<u8>>,
    in_buf: Receiver<Vec<u8>>,
    remote_address: PublicKey,
    public_key: PublicKey,
}

impl RouteWeaverSocket {
    pub fn new(
        out_buf: Sender<Vec<u8>>,
        in_buf: Receiver<Vec<u8>>,
        public_key: PublicKey,
        remote_address: PublicKey,
    ) -> Self {
        Self {
            out_buf: PollSender::new(out_buf),
            in_buf,
            remote_address,
            public_key,
        }
    }
}

impl AsyncRead for RouteWeaverSocket {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        todo!()
    }
}

impl AsyncWrite for RouteWeaverSocket {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        todo!()
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        todo!()
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        todo!()
    }
}
