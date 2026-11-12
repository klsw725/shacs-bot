//! Tells the producer of a chat response when that response was written to its connection.

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use axum::Router;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

/// Runs once a chat response was written to the client in full and flushed.
pub type ChatResponseWritten = Box<dyn FnOnce() + Send>;

/// The writes of one connection that a response producer waits for.
#[derive(Clone, Default)]
pub(crate) struct ConnectionWrites {
    waiting: Arc<Mutex<Vec<ChatResponseWritten>>>,
}

impl ConnectionWrites {
    /// Runs `written` at the next successful flush of the connection. The HTTP layer
    /// flushes the socket only after it wrote out everything handed to it, so call this
    /// at the moment the last bytes of the response are handed over. A connection that
    /// ends before that flush never runs it.
    pub(crate) fn after_next_flush(&self, written: ChatResponseWritten) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.push(written);
        }
    }

    fn flushed(&self) {
        let waiting = match self.waiting.lock() {
            Ok(mut waiting) => std::mem::take(&mut *waiting),
            Err(_) => return,
        };
        for written in waiting {
            written();
        }
    }
}

/// A connection that reports each successful flush to its [`ConnectionWrites`].
pub(crate) struct WriteObservedIo<T> {
    io: T,
    writes: ConnectionWrites,
}

impl<T> WriteObservedIo<T> {
    pub(crate) fn new(io: T) -> Self {
        Self {
            io,
            writes: ConnectionWrites::default(),
        }
    }

    pub(crate) fn writes(&self) -> ConnectionWrites {
        self.writes.clone()
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for WriteObservedIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for WriteObservedIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let flushed = Pin::new(&mut self.io).poll_flush(cx);
        if matches!(flushed, Poll::Ready(Ok(()))) {
            self.writes.flushed();
        }
        flushed
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }
}

pub(crate) struct WriteObservedListener(TcpListener);

impl Listener for WriteObservedListener {
    type Io = WriteObservedIo<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (io, addr) = <TcpListener as Listener>::accept(&mut self.0).await;
        (WriteObservedIo::new(io), addr)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.0.local_addr()
    }
}

impl Connected<IncomingStream<'_, WriteObservedListener>> for ConnectionWrites {
    fn connect_info(stream: IncomingStream<'_, WriteObservedListener>) -> Self {
        stream.io().writes()
    }
}

/// Serves `router` so that its handlers can learn when a response was written out.
pub(crate) async fn serve_router(
    listener: TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(
        WriteObservedListener(listener),
        router.into_make_service_with_connect_info::<ConnectionWrites>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

#[cfg(test)]
mod tests;
