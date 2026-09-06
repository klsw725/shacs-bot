use super::peer_transport::PeerTcpTransport;
use super::types::{
    ConnectedRemoteHop, GuardedHopRequest, GuardedRemoteTransport, RemoteTransportError,
};
use shacs_security::NetworkGuard;
use std::io::{self, Read};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ureq::http::header::{HeaderMap, HeaderName, CONTENT_TYPE, LOCATION};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{ConnectionDetails, Connector, LazyBuffers, RustlsConnector};

#[derive(Debug, Clone)]
pub struct UreqGuardedRemoteTransport {
    guard: NetworkGuard,
    timeout: Duration,
}

impl UreqGuardedRemoteTransport {
    pub const fn new(guard: NetworkGuard, timeout: Duration) -> Self {
        Self { guard, timeout }
    }
}

impl super::types::sealed::Sealed for UreqGuardedRemoteTransport {}

impl GuardedRemoteTransport for UreqGuardedRemoteTransport {
    fn fetch(
        &self,
        request: GuardedHopRequest<'_>,
    ) -> Result<ConnectedRemoteHop, RemoteTransportError> {
        let peer = Arc::new(Mutex::new(None));
        let connector = PeerBoundTcpConnector {
            guard: self.guard.clone(),
            observed_peer: peer.clone(),
        }
        .chain(RustlsConnector::default());
        let config = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .max_idle_connections(0)
            .max_idle_connections_per_host(0)
            .timeout_global(Some(self.timeout))
            .http_status_as_error(false)
            .build();
        let resolver = PolicyResolver {
            guard: self.guard.clone(),
        };
        let agent = ureq::Agent::with_parts(config, connector, resolver);
        let mut response = agent
            .get(request.url())
            .header("Accept", "image/png,image/jpeg,image/webp")
            .call()
            .map_err(|_| RemoteTransportError::ConnectionFailed)?;
        let status = response.status().as_u16();
        let content_type = single_header(response.headers(), &CONTENT_TYPE)?.unwrap_or_default();
        let location = single_header(response.headers(), &LOCATION)?;
        let peer_addr = peer
            .lock()
            .map_err(|_| RemoteTransportError::ConnectionFailed)?
            .ok_or(RemoteTransportError::ConnectionFailed)?;
        let body = if (300..400).contains(&status) {
            Vec::new()
        } else {
            let limit = u64::try_from(request.max_bytes())
                .map_err(|_| RemoteTransportError::ResponseReadFailed)?;
            let mut body = Vec::new();
            response
                .body_mut()
                .as_reader()
                .take(limit)
                .read_to_end(&mut body)
                .map_err(|_| RemoteTransportError::ResponseReadFailed)?;
            body
        };
        Ok(ConnectedRemoteHop {
            peer_addr,
            status,
            content_type,
            location,
            body,
        })
    }
}

fn single_header(
    headers: &HeaderMap,
    name: &HeaderName,
) -> Result<Option<String>, RemoteTransportError> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(RemoteTransportError::AmbiguousHeaders);
    }
    value
        .to_str()
        .map(str::to_owned)
        .map(Some)
        .map_err(|_| RemoteTransportError::AmbiguousHeaders)
}

#[derive(Debug)]
struct PolicyResolver {
    guard: NetworkGuard,
}

impl Resolver for PolicyResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let resolved = DefaultResolver::default().resolve(uri, config, timeout)?;
        let mut allowed = self.empty();
        for address in &resolved {
            if self.guard.is_private(address.ip()) {
                return Err(policy_io_error(
                    "resolved target rejected by network policy",
                ));
            }
            allowed.push(*address);
        }
        if allowed.is_empty() {
            Err(ureq::Error::HostNotFound)
        } else {
            Ok(allowed)
        }
    }
}

#[derive(Debug)]
struct PeerBoundTcpConnector {
    guard: NetworkGuard,
    observed_peer: Arc<Mutex<Option<std::net::SocketAddr>>>,
}

impl Connector<()> for PeerBoundTcpConnector {
    type Out = PeerTcpTransport;

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        if chained.is_some() {
            return Err(ureq::Error::ConnectionFailed);
        }
        let mut last_error = None;
        for expected in &details.addrs {
            if self.guard.is_private(expected.ip()) {
                return Err(policy_io_error(
                    "resolved target rejected by network policy",
                ));
            }
            let stream = match TcpStream::connect_timeout(expected, *details.timeout.after) {
                Ok(stream) => stream,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            let actual = stream.peer_addr().map_err(ureq::Error::Io)?;
            if actual != *expected || self.guard.is_private(actual.ip()) {
                return Err(policy_io_error("connected peer rejected by network policy"));
            }
            stream
                .set_nodelay(details.config.no_delay())
                .map_err(ureq::Error::Io)?;
            *self
                .observed_peer
                .lock()
                .map_err(|_| policy_io_error("connected peer evidence unavailable"))? =
                Some(actual);
            let buffers = LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            );
            return Ok(Some(PeerTcpTransport::new(stream, buffers)));
        }
        Err(ureq::Error::Io(last_error.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "no allowed address connected",
            )
        })))
    }
}

fn policy_io_error(message: &'static str) -> ureq::Error {
    ureq::Error::Io(io::Error::new(io::ErrorKind::PermissionDenied, message))
}
