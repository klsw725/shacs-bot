use serde_json::{json, Value};
use shacs_core::runtime::BrowserError;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::Duration;
use std::time::Instant;

#[derive(Clone, Default)]
pub struct BrowserCancellation(Arc<AtomicBool>);

impl BrowserCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn check(&self) -> Result<(), BrowserError> {
        if self.0.load(Ordering::Acquire) {
            Err(BrowserError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct CdpReply {
    pub result: Value,
    pub sequence: u64,
    pub document_epoch: u64,
    pub events: Vec<Value>,
}

/// Runs for a page target that attached paused while a reply is pending, with
/// the peer that reply is awaited on. Returns the events its own requests
/// collected.
pub type PausedTargetAdmission<'a> = &'a mut dyn FnMut(
    &mut dyn DevToolsPeer,
    &mut Value,
    Instant,
) -> Result<Vec<Value>, BrowserError>;

/// How many paused page targets may be admitted inside one another before the
/// transport gives up with a fatal error. Chromium opens popups one at a time
/// from a blocked renderer, so legitimate nesting stays shallow.
pub const MAX_ADMISSION_DEPTH: usize = 4;

/// A page target the browser attached paused (`waitForDebuggerOnStart`) and
/// nobody admitted yet.
pub fn paused_page_attachment(event: &Value) -> bool {
    event["method"] == "Target.attachedToTarget"
        && event["params"]["targetInfo"]["type"] == "page"
        && event["params"]["waitingForDebugger"] == true
}

/// Record on an attachment event that the transport installed the page
/// boundary and resumed the target. The document layer then only switches.
pub fn mark_admitted(event: &mut Value) {
    event["params"]["waitingForDebugger"] = json!(false);
    event["params"]["admitted"] = json!(event["params"]["rejected"] != true);
}

/// Whether `mark_admitted` ran for this attachment event.
pub fn event_admitted(event: &Value) -> bool {
    event["params"]["admitted"] == true
}

/// Errors that leave the browser usable, where the model looks again. Every
/// other error ends the generation.
pub fn recoverable(error: &BrowserError) -> bool {
    matches!(
        error,
        BrowserError::StaleRevision | BrowserError::TargetCovered | BrowserError::InputDenied
    )
}

/// Combine an operation's outcome with the cleanup command that followed it.
/// A fatal error on either side wins over a recoverable one, so a document
/// that closed during the cleanup is never reported as a stale approval.
pub fn with_cleanup<T>(
    primary: Result<T, BrowserError>,
    cleanup: Result<CdpReply, BrowserError>,
) -> Result<(T, CdpReply), BrowserError> {
    match (primary, cleanup) {
        (Ok(value), Ok(reply)) => Ok((value, reply)),
        (Err(error), Ok(_)) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            if recoverable(&error) && !recoverable(&cleanup) {
                Err(cleanup)
            } else {
                Err(error)
            }
        }
    }
}

/// Internal host transport, never exposed as a model tool.
pub trait DevToolsPeer: Send {
    fn set_cancellation(&mut self, cancellation: BrowserCancellation);
    /// Send one command and wait for its reply. A page target that attaches
    /// paused while the reply is pending is handed to `admit` at once, before
    /// waiting continues: Chromium blocks the opener's renderer, and with it
    /// the reply to the input that opened the popup, until the new target
    /// resumes (Spec 049 PRD 020 scope 7). The returned events keep arrival
    /// order: the attachment sits where it arrived, marked admitted, and the
    /// events the admission itself received follow it.
    fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        deadline: Instant,
        admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError>;
    fn drain_events(&mut self) -> Result<Vec<Value>, BrowserError>;
}

/// `request` for a peer that answers at once: admit the paused attachments
/// the reply carried, after the fact.
pub fn admit_after_reply<P: DevToolsPeer>(
    peer: &mut P,
    mut reply: CdpReply,
    deadline: Instant,
    admit: PausedTargetAdmission<'_>,
) -> Result<CdpReply, BrowserError> {
    let mut events = Vec::new();
    for mut event in std::mem::take(&mut reply.events) {
        if paused_page_attachment(&event) {
            // Reserve the attachment's place first; what the admission receives
            // follows it.
            let reserved = events.len();
            events.push(event.clone());
            let nested = admit(peer, &mut event, deadline)?;
            events[reserved] = event;
            mark_admitted(&mut events[reserved]);
            events.extend(nested);
        } else {
            events.push(event);
        }
    }
    reply.events = events;
    Ok(reply)
}

#[cfg(unix)]
struct Received {
    value: Value,
    sequence: u64,
    document_epoch: u64,
}

#[cfg(unix)]
pub struct CdpConnection {
    writer: File,
    received: Receiver<Result<Received, BrowserError>>,
    next_id: u64,
    /// Events received while replies were awaited, in arrival order. The
    /// outermost request returns them with its reply.
    pending_events: Vec<Value>,
    /// Replies to outer requests that arrived while an admission's nested
    /// request was awaited.
    stray_replies: Vec<Received>,
    depth: usize,
    /// Admissions currently in progress, one inside the other.
    admissions: usize,
    cancellation: BrowserCancellation,
    closed: BrowserCancellation,
}

#[cfg(unix)]
impl CdpConnection {
    pub fn new(reader: OwnedFd, writer: OwnedFd) -> Result<Self, BrowserError> {
        let mut reader = File::from(reader);
        let writer = File::from(writer);
        nonblocking(&reader)?;
        nonblocking(&writer)?;
        let closed = BrowserCancellation::default();
        let reader_closed = closed.clone();
        let (sender, received) = mpsc::sync_channel(256);
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut chunk = [0; 8192];
            let mut sequence = 0;
            let mut document_epoch = 0;
            loop {
                match wait_ready(
                    &reader,
                    libc::POLLIN,
                    Instant::now() + Duration::from_millis(20),
                    &reader_closed,
                ) {
                    Ok(()) => {}
                    Err(BrowserError::PageTimeout) => continue,
                    Err(BrowserError::Cancelled) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
                let count = match reader.read(&mut chunk) {
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            || error.kind() == io::ErrorKind::Interrupted =>
                    {
                        continue
                    }
                    Ok(0) | Err(_) => {
                        let _ = sender.send(Err(BrowserError::TransportClosed));
                        break;
                    }
                    Ok(count) => count,
                };
                for byte in &chunk[..count] {
                    if *byte != 0 {
                        bytes.push(*byte);
                        if bytes.len() > 32 * 1024 * 1024 {
                            let _ = sender.send(Err(BrowserError::Protocol));
                            return;
                        }
                        continue;
                    }
                    let value: Value = match serde_json::from_slice(&bytes) {
                        Ok(value) => value,
                        Err(_) => {
                            let _ = sender.send(Err(BrowserError::Protocol));
                            return;
                        }
                    };
                    bytes.clear();
                    sequence += 1;
                    if changes_document(&value) {
                        document_epoch += 1;
                    }
                    // One FIFO for BOTH replies and events. The epoch is stamped by
                    // this sole reader before publishing a reply. Scheduling the
                    // caller ahead of an event consumer cannot hide a navigation.
                    if sender
                        .send(Ok(Received {
                            value,
                            sequence,
                            document_epoch,
                        }))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        });
        Ok(Self {
            writer,
            received,
            next_id: 0,
            pending_events: Vec::new(),
            stray_replies: Vec::new(),
            depth: 0,
            admissions: 0,
            cancellation: BrowserCancellation::default(),
            closed,
        })
    }

    fn wait_reply(
        &mut self,
        id: u64,
        method: &str,
        session: Option<&str>,
        deadline: Instant,
        admit: PausedTargetAdmission<'_>,
    ) -> Result<Received, BrowserError> {
        loop {
            self.cancellation.check()?;
            let stray = self
                .stray_replies
                .iter()
                .position(|received| received.value.get("id").and_then(Value::as_u64) == Some(id));
            let received = match stray {
                Some(index) => self.stray_replies.remove(index),
                None => {
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .ok_or(BrowserError::PageTimeout)?;
                    match self
                        .received
                        .recv_timeout(remaining.min(Duration::from_millis(20)))
                    {
                        Ok(received) => received?,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return Err(BrowserError::TransportClosed)
                        }
                    }
                }
            };
            if received.value.get("id").and_then(Value::as_u64) == Some(id) {
                if received.value.get("sessionId").and_then(Value::as_str) != session {
                    return Err(BrowserError::Protocol);
                }
                if let Some(error) = received.value.get("error") {
                    return Err(reply_error(method, error));
                }
                return Ok(received);
            }
            if received.value.get("method").is_none() {
                self.stray_replies.push(received);
                continue;
            }
            let event = received.value;
            if paused_page_attachment(&event) {
                // The opener's renderer, and so the pending reply, stays blocked
                // until this target resumes; admit it now. The attachment keeps
                // its arrival slot, so a target attached during this admission
                // follows it, as do the navigation events of either.
                if self.admissions >= MAX_ADMISSION_DEPTH {
                    return Err(BrowserError::Protocol);
                }
                let reserved = self.pending_events.len();
                self.pending_events.push(event.clone());
                self.admissions += 1;
                let mut event = event;
                let admitted = admit(self, &mut event, deadline);
                self.admissions -= 1;
                let nested = admitted?;
                self.pending_events[reserved] = event;
                mark_admitted(&mut self.pending_events[reserved]);
                self.pending_events.extend(nested);
            } else {
                self.pending_events.push(event);
            }
        }
    }
}

/// The server errors (code -32000) that mean the node a number or an object
/// id named is gone or unusable, as Chromium 6e148db946ad40745b3ecffb758336a412819d13
/// words them: `InspectorDOMAgent::AssertNode` ("No node found for given backend
/// id", "Could not find node with given id"), `NodeForRemoteObjectId` ("Object
/// id doesn't reference a Node"; the V8 inspector's "Could not find object with
/// given id"), `resolveNode`, `describeNode`, `focus`, and the accessibility
/// agent's detached-frame and inactive-document checks. Anything else is a
/// protocol failure.
pub const STALE_NODE_MESSAGES: &[&str] = &[
    "No node found for given backend id",
    "Could not find node with given id",
    "No node with given id found",
    "Node with given id does not belong to the document",
    "Node not found",
    "Object id doesn't reference a Node",
    "Could not find object with given id",
    "Node is not an Element",
    "Element is not focusable",
    "Frame is detached.",
    "Document is not active.",
];

/// Classify an error reply. Only recognized stale-node errors of the node
/// commands are recoverable; a box model that cannot be computed skips the
/// element; everything else ends the generation.
pub fn reply_error(method: &str, error: &Value) -> BrowserError {
    let server_error = error.get("code").and_then(Value::as_i64) == Some(-32000);
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    if method == "DOM.getBoxModel" {
        // `InspectorDOMAgent::getBoxModel`: a node without a box (display:none,
        // detached layout) is skipped; a node that is gone is stale; anything
        // else is a protocol failure.
        if server_error && message == "Could not compute box model." {
            BrowserError::GeometryUnavailable
        } else if server_error && STALE_NODE_MESSAGES.contains(&message) {
            BrowserError::StaleRevision
        } else {
            BrowserError::Protocol
        }
    } else if method == "DOM.getNodeForLocation"
        && server_error
        && message == "No node found at given location"
    {
        BrowserError::TargetCovered
    } else if matches!(
        method,
        "DOM.resolveNode" | "DOM.describeNode" | "DOM.focus" | "Accessibility.getPartialAXTree"
    ) && server_error
        && STALE_NODE_MESSAGES.contains(&message)
    {
        BrowserError::StaleRevision
    } else {
        BrowserError::Protocol
    }
}

#[cfg(unix)]
impl Drop for CdpConnection {
    fn drop(&mut self) {
        self.closed.cancel();
    }
}

#[cfg(unix)]
fn nonblocking(file: &File) -> Result<(), BrowserError> {
    // SAFETY: the borrowed descriptor remains live; these fcntl commands take integer flags.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(BrowserError::TransportClosed);
    }
    // SAFETY: file owns the descriptor and O_NONBLOCK changes no ownership.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(BrowserError::TransportClosed);
    }
    Ok(())
}

#[cfg(unix)]
fn wait_ready(
    file: &File,
    events: libc::c_short,
    deadline: Instant,
    cancellation: &BrowserCancellation,
) -> Result<(), BrowserError> {
    loop {
        cancellation.check()?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(BrowserError::PageTimeout)?;
        let timeout = remaining.min(Duration::from_millis(20)).as_millis().max(1) as i32;
        let mut descriptor = libc::pollfd {
            fd: file.as_raw_fd(),
            events,
            revents: 0,
        };
        // SAFETY: poll receives one initialized pollfd referring to a live owned descriptor.
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(BrowserError::TransportClosed);
        }
        if descriptor.revents & events != 0 {
            return Ok(());
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(BrowserError::TransportClosed);
        }
    }
}

/// An event after which the document, target or frame tree a check ran on may
/// no longer be the one an input would reach. The transport stamps the read
/// epoch with it and the document layer bumps its dispatch epoch with it, so
/// both sides agree on what counts as a change.
pub fn changes_document(value: &Value) -> bool {
    matches!(
        value.get("method").and_then(Value::as_str),
        Some(
            "Page.frameNavigated"
                | "Page.frameDetached"
                | "Page.navigatedWithinDocument"
                | "Target.attachedToTarget"
                | "Target.detachedFromTarget"
                | "Target.targetDestroyed"
                | "Target.targetInfoChanged"
                | "Page.fileChooserOpened"
                | "Browser.downloadWillBegin"
        )
    )
}

#[cfg(unix)]
impl DevToolsPeer for CdpConnection {
    fn set_cancellation(&mut self, cancellation: BrowserCancellation) {
        self.cancellation = cancellation;
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        deadline: Instant,
        admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        self.cancellation.check()?;
        if Instant::now() >= deadline {
            return Err(BrowserError::PageTimeout);
        }
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let mut bytes = serde_json::to_vec(&request).map_err(|_| BrowserError::Protocol)?;
        bytes.push(0);
        let mut written = 0;
        while written < bytes.len() {
            wait_ready(&self.writer, libc::POLLOUT, deadline, &self.cancellation)?;
            match self.writer.write(&bytes[written..]) {
                Ok(0) => return Err(BrowserError::TransportClosed),
                Ok(count) => written += count,
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        || error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Err(BrowserError::TransportClosed),
            }
        }
        // Nested requests of an admission leave their events here; only the
        // outermost request hands the whole arrival-ordered list to the caller.
        self.depth += 1;
        let waited = self.wait_reply(id, method, session, deadline, admit);
        self.depth -= 1;
        let received = waited?;
        let events = if self.depth == 0 {
            std::mem::take(&mut self.pending_events)
        } else {
            Vec::new()
        };
        Ok(CdpReply {
            result: received
                .value
                .get("result")
                .cloned()
                .ok_or(BrowserError::Protocol)?,
            sequence: received.sequence,
            document_epoch: received.document_epoch,
            events,
        })
    }

    fn drain_events(&mut self) -> Result<Vec<Value>, BrowserError> {
        let mut events = std::mem::take(&mut self.pending_events);
        loop {
            match self.received.try_recv() {
                Ok(value) => {
                    let value = value?;
                    if value.value.get("method").is_some() {
                        events.push(value.value);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(events),
                Err(mpsc::TryRecvError::Disconnected) => return Err(BrowserError::TransportClosed),
            }
        }
    }
}
