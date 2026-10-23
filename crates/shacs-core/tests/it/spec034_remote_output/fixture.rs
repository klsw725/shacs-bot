use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

pub struct LoopbackFixture {
    address: std::net::SocketAddr,
    worker: JoinHandle<Result<String, String>>,
}

pub struct RawResponseFixture {
    address: std::net::SocketAddr,
    worker: JoinHandle<Result<(), String>>,
}

pub struct NoRequestFixture {
    address: std::net::SocketAddr,
    worker: JoinHandle<Result<bool, String>>,
}

impl NoRequestFixture {
    pub fn start() -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let worker = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok(_) => return Ok(true),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::yield_now();
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            Ok(false)
        });
        Ok(Self { address, worker })
    }

    pub fn url(&self) -> String {
        format!("http://{}/never-request", self.address)
    }

    pub fn finish(self) -> Result<bool, String> {
        self.worker
            .join()
            .map_err(|_| "no-request fixture worker panicked".to_owned())?
    }
}

impl RawResponseFixture {
    pub fn start(response: Vec<u8>) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            read_request(&mut stream).map_err(|error| error.to_string())?;
            stream
                .write_all(&response)
                .map_err(|error| error.to_string())
        });
        Ok(Self { address, worker })
    }

    pub fn url(&self) -> String {
        format!("http://{}/start", self.address)
    }

    pub fn finish(self) -> Result<(), String> {
        self.worker
            .join()
            .map_err(|_| "raw fixture worker panicked".to_owned())?
    }
}

pub struct UnreadRedirectBodyFixture {
    address: std::net::SocketAddr,
    worker: JoinHandle<Result<bool, String>>,
}

impl UnreadRedirectBodyFixture {
    pub fn start(final_body: Vec<u8>) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let worker = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().map_err(|error| error.to_string())?;
            read_request(&mut first).map_err(|error| error.to_string())?;
            first
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 1048576\r\n\r\n",
                )
                .map_err(|error| error.to_string())?;
            listener
                .set_nonblocking(true)
                .map_err(|error| error.to_string())?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut second, _)) => {
                        second
                            .set_nonblocking(false)
                            .map_err(|error| error.to_string())?;
                        read_request(&mut second).map_err(|error| error.to_string())?;
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            final_body.len()
                        );
                        second
                            .write_all(header.as_bytes())
                            .and_then(|()| second.write_all(&final_body))
                            .map_err(|error| error.to_string())?;
                        return Ok(true);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::yield_now();
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            Ok(false)
        });
        Ok(Self { address, worker })
    }

    pub fn url(&self) -> String {
        format!("http://{}/start", self.address)
    }

    pub fn finish(self) -> Result<bool, String> {
        self.worker
            .join()
            .map_err(|_| "redirect fixture worker panicked".to_owned())?
    }
}

impl LoopbackFixture {
    pub fn start(body: Vec<u8>) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
            let request = read_request(&mut stream).map_err(|error| error.to_string())?;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(header.as_bytes())
                .and_then(|()| stream.write_all(&body))
                .map_err(|error| error.to_string())?;
            Ok(request)
        });
        Ok(Self { address, worker })
    }

    pub fn url(&self) -> String {
        format!("http://{}/image.png", self.address)
    }

    pub fn finish(self) -> Result<String, String> {
        self.worker
            .join()
            .map_err(|_| "fixture worker panicked".to_owned())?
    }
}

fn read_request(stream: &mut TcpStream) -> Result<String, std::io::Error> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
