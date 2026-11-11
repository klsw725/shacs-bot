use shacs_core::runtime::NetworkGuard;
use std::io::{self, Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub trait BrowserResolver: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
}

pub trait BrowserConnector {
    type Connection;
    fn connect(&self, address: SocketAddr) -> io::Result<Self::Connection>;
}

pub struct SystemBrowserNetwork;

impl BrowserResolver for SystemBrowserNetwork {
    fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        let host = host.to_owned();
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let result = (host.as_str(), port)
                .to_socket_addrs()
                .map(Iterator::collect);
            let _ = sender.send(result);
        });
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
}

impl BrowserConnector for SystemBrowserNetwork {
    type Connection = TcpStream;
    fn connect(&self, address: SocketAddr) -> io::Result<TcpStream> {
        TcpStream::connect_timeout(&address, Duration::from_secs(5))
    }
}

/// Resolve exactly once and connect only to members of the fully checked result.
pub fn guarded_connect<C: BrowserConnector>(
    guard: &NetworkGuard,
    resolver: &dyn BrowserResolver,
    connector: &C,
    host: &str,
    port: u16,
) -> io::Result<C::Connection> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    let addresses = match host.parse::<IpAddr>() {
        Ok(address) => vec![SocketAddr::new(address, port)],
        Err(_) => resolver.resolve(&host, port)?,
    };
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| address.port() != port || guard.is_private(address.ip()))
    {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    // Chromium can reconnect; each attempt must resolve and validate afresh.
    // Do not spend an unbounded generation shutdown budget retrying a DNS list.
    connector.connect(addresses[0])
}

pub fn read_socks_connect(stream: &mut (impl Read + Write)) -> io::Result<(String, u16)> {
    let mut greeting = [0; 2];
    stream.read_exact(&mut greeting)?;
    if greeting[0] != 5 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut methods = vec![0; usize::from(greeting[1])];
    stream.read_exact(&mut methods)?;
    if !methods.contains(&0) {
        stream.write_all(&[5, 255])?;
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    stream.write_all(&[5, 0])?;
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    if header[0] != 5 || header[1] != 1 || header[2] != 0 {
        stream.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0])?;
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    let host = match header[3] {
        1 => {
            let mut bytes = [0; 4];
            stream.read_exact(&mut bytes)?;
            Ipv4Addr::from(bytes).to_string()
        }
        4 => {
            let mut bytes = [0; 16];
            stream.read_exact(&mut bytes)?;
            Ipv6Addr::from(bytes).to_string()
        }
        3 => {
            let mut length = [0];
            stream.read_exact(&mut length)?;
            let mut bytes = vec![0; usize::from(length[0])];
            stream.read_exact(&mut bytes)?;
            String::from_utf8(bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?
        }
        _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
    };
    let mut port = [0; 2];
    stream.read_exact(&mut port)?;
    Ok((host, u16::from_be_bytes(port)))
}

pub struct BrowserProxy {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    sockets: Arc<Mutex<Vec<Arc<[TcpStream; 2]>>>>,
    worker: Option<JoinHandle<()>>,
}

impl BrowserProxy {
    pub fn start(guard: NetworkGuard) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let sockets: Arc<Mutex<Vec<Arc<[TcpStream; 2]>>>> = Arc::new(Mutex::new(Vec::new()));
        let worker_stop = Arc::clone(&stop);
        let worker_sockets = Arc::clone(&sockets);
        let worker = thread::spawn(move || {
            let mut connections: Vec<JoinHandle<()>> = Vec::new();
            while !worker_stop.load(Ordering::Acquire) {
                let mut index = 0;
                while index < connections.len() {
                    if connections[index].is_finished() {
                        let _ = connections.swap_remove(index).join();
                    } else {
                        index += 1;
                    }
                }
                match worker_sockets.lock() {
                    Ok(mut active) => active.retain(|pair| Arc::strong_count(pair) > 1),
                    Err(_) => break,
                }
                match listener.accept() {
                    Ok((mut client, _)) => {
                        let guard = guard.clone();
                        let sockets = Arc::clone(&worker_sockets);
                        let stop = Arc::clone(&worker_stop);
                        connections.push(thread::spawn(move || {
                            let _ = client.set_read_timeout(Some(Duration::from_secs(5)));
                            let _ = client.set_write_timeout(Some(Duration::from_secs(5)));
                            let Ok((host, port)) = read_socks_connect(&mut client) else {
                                return;
                            };
                            let Ok(mut remote) = guarded_connect(
                                &guard,
                                &SystemBrowserNetwork,
                                &SystemBrowserNetwork,
                                &host,
                                port,
                            ) else {
                                let _ = client.write_all(&[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]);
                                return;
                            };
                            let Ok(client_copy) = client.try_clone() else {
                                return;
                            };
                            let Ok(remote_copy) = remote.try_clone() else {
                                return;
                            };
                            let Ok(mut active) = sockets.lock() else {
                                return;
                            };
                            if stop.load(Ordering::Acquire) {
                                return;
                            }
                            let Ok(client_shutdown) = client.try_clone() else {
                                return;
                            };
                            let Ok(remote_shutdown) = remote.try_clone() else {
                                return;
                            };
                            let held = Arc::new([client_shutdown, remote_shutdown]);
                            active.push(Arc::clone(&held));
                            drop(active);
                            if client.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).is_err() {
                                return;
                            }
                            let _ = client.set_read_timeout(None);
                            thread::scope(|scope| {
                                scope.spawn(move || {
                                    let mut source = client_copy;
                                    let mut destination = remote_copy;
                                    let _ = io::copy(&mut source, &mut destination);
                                    let _ = destination.shutdown(Shutdown::Write);
                                });
                                let _ = io::copy(&mut remote, &mut client);
                                let _ = client.shutdown(Shutdown::Write);
                            });
                            drop(held);
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
            if let Ok(active) = worker_sockets.lock() {
                for pair in active.iter() {
                    for socket in pair.iter() {
                        let _ = socket.shutdown(Shutdown::Both);
                    }
                }
            }
            for connection in connections {
                let _ = connection.join();
            }
        });
        Ok(Self {
            address,
            stop,
            sockets,
            worker: Some(worker),
        })
    }

    pub const fn address(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for BrowserProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(sockets) = self.sockets.lock() {
            for pair in sockets.iter() {
                for socket in pair.iter() {
                    let _ = socket.shutdown(Shutdown::Both);
                }
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
