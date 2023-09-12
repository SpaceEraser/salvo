//! TimeoutAcceptor and it's implements.
use std::future::Future;
use std::io::{ErrorKind as IoErrorKind, Result as IoResult};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use pin_project::pin_project;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::{sleep, Sleep},
};
use tokio_util::sync::CancellationToken;

use crate::async_trait;
use crate::conn::Holding;
use crate::conn::HttpBuilder;
use crate::http::HttpConnection;
use crate::service::HyperHandler;

use super::{Accepted, Acceptor};

pub struct TimeoutAcceptor<T> {
    inner: T,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    connection_timeout: Option<Duration>,
}

impl<T> TimeoutAcceptor<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            read_timeout: None,
            write_timeout: None,
            connection_timeout: None,
        }
    }

    pub fn read_timeout(self, read_timeout: Duration) -> Self {
        Self {
            inner: self.inner,
            read_timeout: Some(read_timeout),
            write_timeout: self.write_timeout,
            connection_timeout: self.connection_timeout,
        }
    }

    pub fn write_timeout(self, write_timeout: Duration) -> Self {
        Self {
            inner: self.inner,
            read_timeout: None,
            write_timeout: Some(write_timeout),
            connection_timeout: self.connection_timeout,
        }
    }

    pub fn connection_timeout(self, connection_timeout: Duration) -> Self {
        Self {
            inner: self.inner,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
            connection_timeout: Some(connection_timeout),
        }
    }
}

#[async_trait]
impl<T> Acceptor for TimeoutAcceptor<T>
where
    T: Acceptor + Send + Unpin + 'static,
    T::Conn: HttpConnection + AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Conn = TimeoutStream<T::Conn>;

    #[inline]
    fn holdings(&self) -> &[Holding] {
        self.inner.holdings()
    }

    #[inline]
    async fn accept(&mut self) -> IoResult<Accepted<Self::Conn>> {
        Ok(self
            .inner
            .accept()
            .await?
            .map_conn(|conn| TimeoutStream::new(conn, self.read_timeout, self.write_timeout, self.connection_timeout)))
    }
}

#[pin_project]
pub struct TimeoutStream<T> {
    #[pin]
    inner: T,

    #[pin]
    read_sleep: Option<Pin<Box<Sleep>>>,
    read_timeout: Duration,
    #[pin]
    write_sleep: Option<Pin<Box<Sleep>>>,
    write_timeout: Duration,
    #[pin]
    connection_sleep: Pin<Box<Sleep>>,
}

impl<T> TimeoutStream<T> {
    pub fn new(
        inner: T,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
        connection_timeout: Option<Duration>,
    ) -> Self {
        Self {
            inner,
            read_sleep: None,
            read_timeout: read_timeout.unwrap_or(Duration::MAX),
            write_sleep: None,
            write_timeout: write_timeout.unwrap_or(Duration::MAX),
            connection_sleep: Box::pin(sleep(connection_timeout.unwrap_or(Duration::MAX))),
        }
    }
}

impl<T> AsyncRead for TimeoutStream<T>
where
    T: AsyncRead + Send + Unpin + 'static,
{
    #[inline]
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<IoResult<()>> {
        let mut this = self.project();

        if this.connection_sleep.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        let read_sleep_pinned = if let Some(some) = this.read_sleep.as_mut().as_pin_mut() {
            some
        } else {
            this.read_sleep.set(Some(Box::pin(sleep(*this.read_timeout))));
            this.read_sleep.as_mut().as_pin_mut().unwrap()
        };

        if read_sleep_pinned.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        this.inner.poll_read(cx, buf)
    }
}

impl<T> AsyncWrite for TimeoutStream<T>
where
    T: AsyncWrite + Send + Unpin + 'static,
{
    #[inline]
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<IoResult<usize>> {
        let mut this = self.project();

        if this.connection_sleep.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        let write_sleep_pinned = if let Some(some) = this.write_sleep.as_mut().as_pin_mut() {
            some
        } else {
            this.write_sleep.set(Some(Box::pin(sleep(*this.write_timeout))));
            this.write_sleep.as_mut().as_pin_mut().unwrap()
        };

        if write_sleep_pinned.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        this.inner.poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        let mut this = self.project();

        if this.connection_sleep.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        let write_sleep_pinned = if let Some(some) = this.write_sleep.as_mut().as_pin_mut() {
            some
        } else {
            this.write_sleep.set(Some(Box::pin(sleep(*this.write_timeout))));
            this.write_sleep.as_mut().as_pin_mut().unwrap()
        };

        if write_sleep_pinned.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        this.inner.poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<()>> {
        let mut this = self.project();

        if this.connection_sleep.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        let write_sleep_pinned = if let Some(some) = this.write_sleep.as_mut().as_pin_mut() {
            some
        } else {
            this.write_sleep.set(Some(Box::pin(sleep(*this.write_timeout))));
            this.write_sleep.as_mut().as_pin_mut().unwrap()
        };

        if write_sleep_pinned.poll(cx).is_ready() {
            return Poll::Ready(Err(IoErrorKind::TimedOut.into()));
        }

        this.inner.poll_shutdown(cx)
    }
}

#[async_trait]
impl<T> HttpConnection for TimeoutStream<T>
where
    T: HttpConnection + Send,
{
    async fn serve(
        self,
        handler: HyperHandler,
        builder: Arc<HttpBuilder>,
        server_shutdown_token: CancellationToken,
        idle_connection_timeout: Option<Duration>,
    ) -> IoResult<()> {
        if self.connection_sleep.is_elapsed() {
            return Err(IoErrorKind::TimedOut.into());
        }

        self.inner
            .serve(handler, builder, server_shutdown_token, idle_connection_timeout)
            .await
    }
}
