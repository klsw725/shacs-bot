use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct SmtpSink {
    pub address: SocketAddr,
    pub reject: Arc<AtomicBool>,
    pub messages: mpsc::Receiver<String>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl SmtpSink {
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let reject = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_reject = reject.clone();
        let worker_stop = stop.clone();
        let (sender, messages) = mpsc::channel();
        let worker = thread::spawn(move || {
            for accepted in listener.incoming() {
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                match accepted.and_then(|socket| session(socket, &worker_reject, &sender)) {
                    Ok(()) => {}
                    Err(error) => eprintln!("local SMTP sink: {error}"),
                }
            }
        });
        Ok(Self {
            address,
            reject,
            messages,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for SmtpSink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Err(error) = TcpStream::connect(self.address) {
            eprintln!("SMTP stop: {error}");
        }
        if let Some(worker) = self.worker.take() {
            assert!(worker.join().is_ok());
        }
    }
}

fn session(
    socket: TcpStream,
    reject: &AtomicBool,
    sender: &mpsc::Sender<String>,
) -> std::io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(socket.try_clone()?);
    let mut writer = socket;
    writer.write_all(b"220 localhost ESMTP\r\n")?;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line.starts_with("EHLO")
            || line.starts_with("HELO")
            || line.starts_with("MAIL")
            || line.starts_with("RSET")
        {
            writer.write_all(b"250 localhost\r\n")?;
        } else if line.starts_with("RCPT") {
            writer.write_all(if reject.load(Ordering::SeqCst) {
                b"550 recipient rejected\r\n"
            } else {
                b"250 recipient accepted\r\n"
            })?;
        } else if line.starts_with("DATA") {
            writer.write_all(b"354 send data\r\n")?;
            let mut message = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line)? == 0 {
                    return Ok(());
                }
                if line == ".\r\n" {
                    break;
                }
                message.push_str(&line);
            }
            sender.send(message).map_err(std::io::Error::other)?;
            writer.write_all(b"250 queued locally\r\n")?;
        } else if line.starts_with("QUIT") {
            writer.write_all(b"221 bye\r\n")?;
            return Ok(());
        } else {
            writer.write_all(b"500 unsupported\r\n")?;
        }
    }
}
