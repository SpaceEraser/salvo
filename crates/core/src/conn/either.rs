//! JoinListener and it's implements.
use std::io::Result as IoResult;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use pin_project::pin_project;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::CancellationToken;

use crate::async_trait;
use crate::conn::Holding;
use crate::conn::HttpBuilder;
use crate::http::HttpConnection;
use crate::service::HyperHandler;

use super::{Accepted, Acceptor, Listener};

/// `JoinedListener` is a listener that can join two listeners.
#[pin_project(project = EitherListenerProj)]
pub enum EitherListener<L, R> {
    Left(#[pin] L),
    Right(#[pin] R),
}

#[async_trait]
impl<L, R> Listener for EitherListener<L, R>
where
    L: Listener + Send + Unpin + 'static,
    R: Listener + Send + Unpin + 'static,
    L::Acceptor: Acceptor + Send + Unpin + 'static,
    R::Acceptor: Acceptor + Send + Unpin + 'static,
{
    type Acceptor = EitherAcceptor<L::Acceptor, R::Acceptor>;

    async fn try_bind(self) -> IoResult<Self::Acceptor> {
        match self {
            Self::Left(left) => Ok(EitherAcceptor::Left(left.try_bind().await?)),
            Self::Right(right) => Ok(EitherAcceptor::Right(right.try_bind().await?)),
        }
    }
}

pub enum EitherAcceptor<L, R> {
    Left(L),
    Right(R),
}

#[async_trait]
impl<L, R> Acceptor for EitherAcceptor<L, R>
where
    L: Acceptor + Send + Unpin + 'static,
    R: Acceptor + Send + Unpin + 'static,
    L::Conn: HttpConnection + AsyncRead + AsyncWrite + Send + Unpin + 'static,
    R::Conn: HttpConnection + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Conn = EitherStream<L::Conn, R::Conn>;

    #[inline]
    fn holdings(&self) -> &[Holding] {
        match self {
            Self::Left(left) => left.holdings(),
            Self::Right(right) => right.holdings(),
        }
    }

    #[inline]
    async fn accept(&mut self) -> IoResult<Accepted<Self::Conn>> {
        match self {
            Self::Left(left) => Ok(left.accept().await?.map_conn(EitherStream::Left)),
            Self::Right(right) => Ok(right.accept().await?.map_conn(EitherStream::Right)),
        }
    }
}

pub enum EitherStream<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> AsyncRead for EitherStream<L, R>
where
    L: AsyncRead + Send + Unpin + 'static,
    R: AsyncRead + Send + Unpin + 'static,
{
    #[inline]
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<IoResult<()>> {
        match &mut self.get_mut() {
            EitherStream::Left(a) => Pin::new(a).poll_read(cx, buf),
            EitherStream::Right(b) => Pin::new(b).poll_read(cx, buf),
        }
    }
}

impl<L, R> AsyncWrite for EitherStream<L, R>
where
    L: AsyncWrite + Send + Unpin + 'static,
    R: AsyncWrite + Send + Unpin + 'static,
{
    #[inline]
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<IoResult<usize>> {
        match &mut self.get_mut() {
            EitherStream::Left(a) => Pin::new(a).poll_write(cx, buf),
            EitherStream::Right(b) => Pin::new(b).poll_write(cx, buf),
        }
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        match &mut self.get_mut() {
            EitherStream::Left(a) => Pin::new(a).poll_flush(cx),
            EitherStream::Right(b) => Pin::new(b).poll_flush(cx),
        }
    }

    #[inline]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        match &mut self.get_mut() {
            EitherStream::Left(a) => Pin::new(a).poll_shutdown(cx),
            EitherStream::Right(b) => Pin::new(b).poll_shutdown(cx),
        }
    }
}

#[async_trait]
impl<L, R> HttpConnection for EitherStream<L, R>
where
    L: HttpConnection + Send,
    R: HttpConnection + Send,
{
    async fn serve(
        self,
        handler: HyperHandler,
        builder: Arc<HttpBuilder>,
        server_shutdown_token: CancellationToken,
        idle_connection_timeout: Option<Duration>,
    ) -> IoResult<()> {
        match self {
            EitherStream::Left(left) => {
                left.serve(handler, builder, server_shutdown_token, idle_connection_timeout)
                    .await
            }
            EitherStream::Right(right) => {
                right
                    .serve(handler, builder, server_shutdown_token, idle_connection_timeout)
                    .await
            }
        }
    }
}
