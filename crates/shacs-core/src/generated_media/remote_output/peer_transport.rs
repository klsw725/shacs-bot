use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use ureq::unversioned::transport::{Buffers, LazyBuffers, NextTimeout, Transport};

pub(super) struct PeerTcpTransport {
    stream: TcpStream,
    buffers: LazyBuffers,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
}

impl PeerTcpTransport {
    pub(super) const fn new(stream: TcpStream, buffers: LazyBuffers) -> Self {
        Self {
            stream,
            buffers,
            read_timeout: None,
            write_timeout: None,
        }
    }
}

impl std::fmt::Debug for PeerTcpTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PeerTcpTransport")
            .field("peer_bound", &true)
            .finish()
    }
}

impl Transport for PeerTcpTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let duration = *timeout.after;
        if self.write_timeout != Some(duration) {
            self.stream
                .set_write_timeout(Some(duration))
                .map_err(ureq::Error::Io)?;
            self.write_timeout = Some(duration);
        }
        self.stream
            .write_all(&self.buffers.output()[..amount])
            .map_err(ureq::Error::Io)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let duration = *timeout.after;
        if self.read_timeout != Some(duration) {
            self.stream
                .set_read_timeout(Some(duration))
                .map_err(ureq::Error::Io)?;
            self.read_timeout = Some(duration);
        }
        let amount = self
            .stream
            .read(self.buffers.input_append_buf())
            .map_err(ureq::Error::Io)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        false
    }
}
