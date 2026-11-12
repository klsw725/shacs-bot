use super::browser_snapshot::{self, BrowserNode, BrowserSnapshot};
use super::{
    browser_action_binding, changes_document, event_admitted, recoverable, with_cleanup,
    BrowserCancellation, CdpReply, DevToolsPeer,
};
use serde_json::{json, Value};
use shacs_core::runtime::{
    BrowserAction, BrowserDocument, BrowserError, BrowserPreparedAction, BrowserRead,
};
use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

pub struct CdpDocument {
    peer: Box<dyn DevToolsPeer>,
    target: String,
    session: String,
    closed: bool,
    navigation_started: bool,
    awaiting_popup: bool,
    /// The numbered act binding. Any document or target event drops it.
    snapshot: Option<BrowserSnapshot>,
    /// The last main document the guard verified (or DevTools reported for
    /// the main frame). Observation actions are prepared from it even after
    /// the act binding is gone, so the model can always look again.
    verified: Option<BrowserDocument>,
    /// Events not yet applied to the document state, in arrival order. The
    /// commands an event's handling sends append behind everything already
    /// queued, so handling never runs inside handling.
    inbox: VecDeque<Value>,
    applying: bool,
    /// Counts every applied event that changes what document or target the
    /// checks of an input step ran on. A step records it before its checks and
    /// dispatches only while it is unchanged.
    epoch: u64,
    /// The slot's control revision, part of every binding this document
    /// prepares. The manager raises it at each takeover and each return.
    control: u64,
    notice: Option<String>,
}

/// What a key press is bound to: the approval's snapshot and arguments, and
/// for `type`'s Enter the element the text went to.
struct PressBinding<'a> {
    snapshot: &'a BrowserSnapshot,
    arguments: &'a Value,
    digest: &'a str,
    typed: Option<&'a BrowserNode>,
}

/// Whether an event belongs to the session the document drives now. Events
/// without a session (browser-level ones, and fakes) count as the current one.
fn for_session(event: &Value, session: &str) -> bool {
    event["sessionId"].as_str().is_none_or(|own| own == session)
}

/// Whether an action needs a numbered element or the focused node.
fn targeted(arguments: &Value) -> bool {
    match arguments["action"].as_str() {
        Some("click" | "type" | "press") => true,
        Some("scroll") => arguments.get("element").is_some(),
        _ => false,
    }
}

/// Install the page boundary on an attached page target: downloads denied in
/// its browser context, page and DOM events on, file choosers intercepted
/// (Spec 049 PRD 020 scope 7). Returns the events the replies carried.
fn configure_target(
    peer: &mut dyn DevToolsPeer,
    session: &str,
    browser_context: Option<&str>,
    deadline: Instant,
) -> Result<Vec<Value>, BrowserError> {
    let mut download = json!({"behavior":"deny","eventsEnabled":true});
    if let Some(context) = browser_context {
        download["browserContextId"] = json!(context);
    }
    let mut events = peer
        .request(
            "Browser.setDownloadBehavior",
            download,
            None,
            deadline,
            &mut admit_paused_target,
        )?
        .events;
    for (method, params) in [
        ("Page.enable", json!({})),
        ("DOM.enable", json!({})),
        (
            "Page.setInterceptFileChooserDialog",
            json!({"enabled":true}),
        ),
    ] {
        let reply = peer.request(
            method,
            params,
            Some(session),
            deadline,
            &mut admit_paused_target,
        )?;
        events.extend(reply.events);
    }
    Ok(events)
}

/// A page target attached paused: install the boundary, then resume it. This
/// runs inside the transport while the input that opened the popup still
/// awaits its reply, because Chromium blocks the opener's renderer until the
/// popup resumes.
fn admit_paused_target(
    peer: &mut dyn DevToolsPeer,
    event: &Value,
    deadline: Instant,
) -> Result<Vec<Value>, BrowserError> {
    let session = event["params"]["sessionId"]
        .as_str()
        .ok_or(BrowserError::Protocol)?;
    let context = event["params"]["targetInfo"]["browserContextId"].as_str();
    let mut events = configure_target(peer, session, context, deadline)?;
    let resumed = peer.request(
        "Runtime.runIfWaitingForDebugger",
        json!({}),
        Some(session),
        deadline,
        &mut admit_paused_target,
    )?;
    events.extend(resumed.events);
    Ok(events)
}

impl CdpDocument {
    pub fn prepare(&self, arguments: &Value) -> Result<BrowserPreparedAction, BrowserError> {
        if arguments["action"] == "open" {
            return browser_action_binding::empty(arguments, self.control);
        }
        if targeted(arguments) {
            return self
                .snapshot
                .as_ref()
                .ok_or(BrowserError::StaleRevision)?
                .prepare(arguments, self.control);
        }
        match &self.verified {
            Some(document) => {
                browser_action_binding::observation(arguments, document, self.control)
            }
            None => browser_action_binding::empty(arguments, self.control),
        }
    }

    /// Binds what is prepared from now on to `control`.
    pub fn set_control(&mut self, control: u64) {
        self.control = control;
    }

    /// Control came back from the user: every number and every earlier
    /// binding is void, and the verified document is matched to what the tab
    /// shows now. A tab that went nowhere (`about:blank`) has no verified
    /// document. Nothing here keeps control from coming back.
    pub fn resume_control(&mut self, control: u64, deadline: Instant) {
        self.control = control;
        self.epoch += 1;
        self.snapshot = None;
        if self.closed {
            return;
        }
        let read = self.poll(deadline).and_then(|()| self.main_frame(deadline));
        match read {
            Ok((document, _)) if http_document(&document.url) => self.verified = Some(document),
            Ok(_) => self.verified = None,
            Err(error) => {
                if !recoverable(&error) {
                    self.close(deadline);
                }
            }
        }
    }

    /// Text the user pasted while in control: one `Input.insertText` into
    /// whatever has focus now. A tab that is gone takes none, and that is
    /// not a fault of the browser.
    pub fn insert_pasted(&mut self, text: &str, deadline: Instant) -> Result<(), BrowserError> {
        if self.closed {
            return Err(BrowserError::NotOpen);
        }
        let epoch = self.epoch;
        self.dispatch_input(epoch, "Input.insertText", json!({"text":text}), deadline)
            .map(|_| ())
    }

    pub fn execute_prepared(
        &mut self,
        arguments: &Value,
        digest: &str,
        capture: bool,
        deadline: Instant,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        // Events since the decision come first: a navigation or a new target
        // changes what the approved digest described.
        self.poll(deadline)?;
        // A decision whose subject is gone (the numbers, or the document a new
        // target replaced) is stale; the browser itself is fine, and the next
        // preparation names the real reason.
        let current = self
            .prepare(arguments)
            .map_err(|_| BrowserError::StaleRevision)?;
        if current.digest != digest {
            if targeted(arguments) {
                self.snapshot = None;
            }
            return Err(BrowserError::StaleRevision);
        }
        // The identity the decision was made on. An observation runs, and is
        // read, only while the live main document is still this one.
        let approved = self.verified.clone();
        // Observations are bound to the document they were decided on; the
        // numbered scroll is one too.
        let observation = matches!(
            arguments["action"].as_str(),
            Some("look" | "capture" | "scroll" | "back" | "wait")
        );
        if targeted(arguments) {
            if let Some(snapshot) = self.snapshot.clone() {
                if let Err(error) = snapshot.confirm_document(self, deadline) {
                    // A changed identity is a stale approval; a document that the
                    // request saw close (chooser, forbidden navigation, detach)
                    // stays the fatal error it is.
                    self.snapshot = None;
                    return Err(error);
                }
            }
        }
        if observation {
            if approved.is_none() {
                return Err(BrowserError::StaleRevision);
            }
            self.observed_document(approved.as_ref(), deadline)?;
        }
        let action = match arguments["action"].as_str() {
            Some("open") => BrowserAction::Navigate {
                url: arguments["url"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?
                    .to_owned(),
            },
            Some("look") => {
                return self
                    .read_verified(approved.as_ref(), capture, deadline)
                    .map(Some)
            }
            // The user asked for the screen: the settings that decide whether a
            // `look` carries an image play no part.
            Some("capture") => {
                return self
                    .read_verified(approved.as_ref(), true, deadline)
                    .map(Some)
            }
            Some("close") => {
                self.close(deadline);
                return Ok(None);
            }
            Some("click") => BrowserAction::ClickElement {
                element: arguments["element"]
                    .as_u64()
                    .ok_or(BrowserError::Protocol)? as usize,
                revision: arguments["revision"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?
                    .to_owned(),
            },
            Some("type") => BrowserAction::TypeElement {
                element: arguments["element"]
                    .as_u64()
                    .ok_or(BrowserError::Protocol)? as usize,
                revision: arguments["revision"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?
                    .to_owned(),
                text: arguments["text"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?
                    .to_owned(),
                submit: arguments["submit"].as_bool().unwrap_or(false),
            },
            Some("press") => BrowserAction::Press {
                key: arguments["key"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?
                    .to_owned(),
            },
            Some("scroll") => BrowserAction::ScrollElement {
                element: arguments["element"].as_u64().map(|n| n as usize),
                revision: arguments["revision"].as_str().map(str::to_owned),
                direction: arguments["direction"].as_str().unwrap_or("down").to_owned(),
            },
            Some("back") => BrowserAction::Back,
            Some("wait") => BrowserAction::Wait {
                text: arguments["text"].as_str().map(str::to_owned),
                timeout_ms: arguments["timeoutMs"].as_u64().unwrap_or(1000),
            },
            _ => return Err(BrowserError::Protocol),
        };
        let observed = observation.then_some(approved.as_ref()).flatten();
        let departed = self.act_inner(&action, Some((arguments, digest)), observed, deadline)?;
        // An observation that did not leave its document on purpose (`back`
        // with history does) is read only on the document decided on.
        let expected = (observation && !departed)
            .then_some(approved.as_ref())
            .flatten();
        self.read_verified(expected, capture, deadline).map(Some)
    }

    /// Re-read what the approval described and compare it with the digest.
    /// `focus_moved_by_key` is set after a focus-moving key went down: the new
    /// focus is the key's intended result, while the start node's own role,
    /// name, form and submitter still have to match.
    fn recheck_binding(
        &mut self,
        snapshot: &BrowserSnapshot,
        arguments: &Value,
        digest: &str,
        focus_moved_by_key: bool,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        snapshot.confirm_document(self, deadline)?;
        let mut current = snapshot.clone();
        if arguments["action"] == "press" {
            if focus_moved_by_key {
                if let Some(node) = snapshot.focus["node"].as_u64() {
                    let accessible = browser_action_binding::accessible(self, node, deadline)?;
                    current.focus["role"] = accessible["role"].clone();
                    current.focus["name"] = accessible["name"].clone();
                    current.focus["facts"] = browser_action_binding::facts(self, node, deadline)?;
                }
            } else {
                current.focus = self.live_focus(&snapshot.document.frame_id, deadline)?;
            }
        } else {
            let element = arguments["element"]
                .as_u64()
                .ok_or(BrowserError::StaleRevision)? as usize;
            let node = snapshot.target(self, element, &snapshot.revision, deadline)?;
            let current_node = current
                .elements
                .iter_mut()
                .find(|candidate| candidate.backend_id == node.backend_id)
                .ok_or(BrowserError::StaleRevision)?;
            current_node.facts = browser_action_binding::facts(self, node.backend_id, deadline)?;
        }
        snapshot.confirm_document(self, deadline)?;
        if current.prepare(arguments, self.control)?.digest != digest {
            return Err(BrowserError::StaleRevision);
        }
        Ok(())
    }

    pub fn initialize(
        mut peer: Box<dyn DevToolsPeer>,
        deadline: Instant,
    ) -> Result<Self, BrowserError> {
        peer.request(
            "Browser.setDownloadBehavior",
            json!({"behavior":"deny", "eventsEnabled":true}),
            None,
            deadline,
            &mut admit_paused_target,
        )?;
        peer.request(
            "Target.setDiscoverTargets",
            json!({"discover":true}),
            None,
            deadline,
            &mut admit_paused_target,
        )?;
        let targets = peer.request(
            "Target.getTargets",
            json!({}),
            None,
            deadline,
            &mut admit_paused_target,
        )?;
        let target = targets.result["targetInfos"]
            .as_array()
            .and_then(|targets| targets.iter().find(|target| target["type"] == "page"))
            .and_then(|target| target["targetId"].as_str())
            .ok_or(BrowserError::Protocol)?
            .to_owned();
        let attached = peer.request(
            "Target.attachToTarget",
            json!({"targetId":target, "flatten":true}),
            None,
            deadline,
            &mut admit_paused_target,
        )?;
        let session = attached.result["sessionId"]
            .as_str()
            .ok_or(BrowserError::Protocol)?
            .to_owned();
        let mut document = Self {
            peer,
            target,
            session,
            closed: false,
            navigation_started: false,
            awaiting_popup: false,
            snapshot: None,
            verified: None,
            inbox: VecDeque::new(),
            applying: false,
            epoch: 0,
            control: 0,
            notice: None,
        };
        document.configure_session(deadline)?;
        // New page targets remain paused until the host installs the same boundary.
        document.global(
            "Target.setAutoAttach",
            json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true}),
            deadline,
        )?;
        Ok(document)
    }

    fn configure_session(&mut self, deadline: Instant) -> Result<(), BrowserError> {
        let session = self.session.clone();
        let events = configure_target(&mut *self.peer, &session, None, deadline)?;
        self.events(&events, deadline)
    }

    pub fn poll(&mut self, deadline: Instant) -> Result<(), BrowserError> {
        if self.closed {
            return Err(BrowserError::DocumentChanged);
        }
        let events = self.peer.drain_events()?;
        self.events(&events, deadline)
    }

    /// The deep focused node right now, as the press binding records it.
    fn live_focus(&mut self, frame: &str, deadline: Instant) -> Result<Value, BrowserError> {
        let dom = self.request(
            "DOM.getDocument",
            json!({"depth":-1,"pierce":true}),
            deadline,
        )?;
        browser_action_binding::focus(self, &dom.result, frame, deadline)
    }

    /// Whether the numbered node owns a child browsing context. Such a node is
    /// never a click or type target: focusing it makes it the parent
    /// document's focused element while page focus moves into the child frame
    /// (Chromium 6e148db document.cc:6107-6120, html_frame_element_base.cc:
    /// 241-250), so the AX `focused` property names the owner while keys reach
    /// the child document, which nobody reviewed. Controls inside permitted
    /// child documents are numbered on their own and stay usable.
    fn frame_owner_target(&mut self, node: u64, deadline: Instant) -> Result<bool, BrowserError> {
        let described =
            self.request("DOM.describeNode", json!({"backendNodeId":node}), deadline)?;
        Ok(browser_action_binding::frame_owner(
            &described.result["node"],
        ))
    }

    /// Whether `node` is the element Blink delivers keys and text to right
    /// now, as a check in the middle of a sequence. The check right before an
    /// input is `focus_then_dispatch`.
    fn focus_on_node(&mut self, node: u64, deadline: Instant) -> Result<bool, BrowserError> {
        self.ax_focused(node, deadline)
    }

    /// The accessibility tree's `focused` property of `node`. Blink computes
    /// it browser-side from the document's own focused element, not from a
    /// retargeted `activeElement`: `AXNodeObject::IsFocused` (Chromium
    /// 6e148db, ax_node_object.cc:3310-3323) compares `Document::
    /// FocusedElement()` with the node, so a shadow host whose inner control
    /// holds focus is not focused, closed roots make no difference, and window
    /// activation plays no part. The inspector emits the property only for an
    /// unignored node that is focused (inspector_type_builder_helper.cc:363-367,
    /// :826-878); its absence, an ignored node or a missing node all count as
    /// not focused, so the guard fails closed.
    fn ax_focused(&mut self, node: u64, deadline: Instant) -> Result<bool, BrowserError> {
        let reply = self.request(
            "Accessibility.getPartialAXTree",
            json!({"backendNodeId":node,"fetchRelatives":false}),
            deadline,
        )?;
        let Some(entry) = reply.result["nodes"]
            .as_array()
            .and_then(|nodes| nodes.iter().find(|entry| entry["backendDOMNodeId"] == node))
        else {
            return Ok(false);
        };
        if entry["ignored"] == true {
            return Ok(false);
        }
        Ok(entry["properties"].as_array().is_some_and(|properties| {
            properties
                .iter()
                .any(|property| property["name"] == "focused" && property["value"]["value"] == true)
        }))
    }

    /// The focus check as the last request before an input: one accessibility
    /// query, then the input, with nothing in between. A node that is not the
    /// focused element is a stale approval; the caller decides whether that
    /// ends the browser.
    fn focus_then_dispatch(
        &mut self,
        node: u64,
        epoch: u64,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        if self.ax_focused(node, deadline)? {
            self.dispatch_input(epoch, method, params, deadline)
                .map(|_| ())
        } else {
            Err(BrowserError::StaleRevision)
        }
    }

    /// After the first effect of a `type` went out: focus anywhere but the
    /// approved element, or a failure to tell, ends the browser.
    fn focus_or_terminate(&mut self, node: u64, deadline: Instant) -> Result<(), BrowserError> {
        match self.focus_on_node(node, deadline) {
            Ok(true) => Ok(()),
            Ok(false) => self.terminate(),
            Err(error) => self.fail_closed(Err(error)),
        }
    }

    /// Everything that must still hold between the press and the release: the
    /// binding, and the approved target under the pressed point.
    fn release_allowed(
        &mut self,
        snapshot: &BrowserSnapshot,
        binding: Option<(&Value, &str)>,
        node: &BrowserNode,
        x: f64,
        y: f64,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        if let Some((arguments, digest)) = binding {
            self.recheck_binding(snapshot, arguments, digest, false, deadline)?;
        }
        if node.covers(self, x, y, deadline)? {
            Ok(())
        } else {
            Err(BrowserError::StaleRevision)
        }
    }

    /// After the first input event of a sequence went out: a recoverable
    /// mismatch ends the browser (nothing more may be sent), a fatal error
    /// stays itself.
    fn fail_closed<T>(&mut self, rechecked: Result<T, BrowserError>) -> Result<T, BrowserError> {
        match rechecked {
            Ok(value) => Ok(value),
            Err(error) if recoverable(&error) => self.terminate(),
            Err(error) => Err(error),
        }
    }

    /// Queue `events` and apply the queue in arrival order. A call made while
    /// the queue is being applied (by a command an event handler sent) only
    /// queues; the outer application reaches those events in turn (Spec 049
    /// PRD 010 ordered events, PRD 020 scope 7 creation order).
    fn events(&mut self, events: &[Value], deadline: Instant) -> Result<(), BrowserError> {
        self.inbox.extend(events.iter().cloned());
        if self.applying {
            return Ok(());
        }
        self.applying = true;
        let mut result = Ok(());
        while let Some(event) = self.inbox.pop_front() {
            result = self.apply_event(&event, deadline);
            if result.is_err() {
                // A fatal event closed the document; what follows is moot.
                self.inbox.clear();
                break;
            }
        }
        self.applying = false;
        result
    }

    fn apply_event(&mut self, event: &Value, deadline: Instant) -> Result<(), BrowserError> {
        {
            // The same predicate the transport stamps its read epoch with: a
            // frame removal, a destroyed or renamed target, a chooser count as
            // much as a navigation or a target switch.
            if changes_document(event) {
                self.snapshot = None;
                self.epoch += 1;
            }
            match event["method"].as_str() {
                Some("Page.frameNavigated")
                    if for_session(event, &self.session)
                        && event["params"]["frame"].get("parentId").is_none()
                        && event["params"]["frame"]["url"].as_str().is_some_and(|url| {
                            !http_document(url) && (self.navigation_started || url != "about:blank")
                        }) =>
                {
                    self.close(deadline);
                    return Err(BrowserError::DocumentChanged);
                }
                Some("Page.frameNavigated")
                    if for_session(event, &self.session)
                        && event["params"]["frame"].get("parentId").is_none() =>
                {
                    let frame = &event["params"]["frame"];
                    let url = frame["url"].as_str().unwrap_or("");
                    self.verified = http_document(url).then(|| BrowserDocument {
                        target_id: self.target.clone(),
                        session_id: self.session.clone(),
                        frame_id: frame["id"].as_str().unwrap_or("").to_owned(),
                        loader_id: frame["loaderId"].as_str().unwrap_or("").to_owned(),
                        url: url.to_owned(),
                    });
                }
                Some("Page.navigatedWithinDocument") if for_session(event, &self.session) => {
                    if let Some(verified) = self.verified.as_mut() {
                        let url = event["params"]["url"].as_str().unwrap_or("");
                        if event["params"]["frameId"] == verified.frame_id && http_document(url) {
                            verified.url = url.to_owned();
                        }
                    }
                }
                Some("Page.fileChooserOpened" | "Browser.downloadWillBegin") => {
                    self.close(deadline);
                    return Err(BrowserError::FileTransferDenied);
                }
                Some("Target.attachedToTarget")
                    if event["params"]["targetInfo"]["type"] == "page" =>
                {
                    let target = event["params"]["targetInfo"]["targetId"]
                        .as_str()
                        .ok_or(BrowserError::Protocol)?;
                    let session = event["params"]["sessionId"]
                        .as_str()
                        .ok_or(BrowserError::Protocol)?;
                    if target != self.target || session != self.session {
                        self.navigation_started = false;
                        self.awaiting_popup = true;
                        // A new target is nobody's approval subject until a read has
                        // verified its own document; the opener's identity is not
                        // carried over.
                        self.verified = None;
                        let previous = std::mem::replace(&mut self.target, target.to_owned());
                        let previous_session =
                            std::mem::replace(&mut self.session, session.to_owned());
                        let context = event["params"]["targetInfo"]["browserContextId"].as_str();
                        // The transport admits paused targets while a reply is
                        // pending; such an event only switches here.
                        let nested = if event_admitted(event) {
                            Vec::new()
                        } else if event["params"]["waitingForDebugger"] == true {
                            admit_paused_target(&mut *self.peer, event, deadline)?
                        } else {
                            configure_target(&mut *self.peer, session, context, deadline)?
                        };
                        self.events(&nested, deadline)?;
                        if previous != target {
                            self.global(
                                "Target.closeTarget",
                                json!({"targetId":previous}),
                                deadline,
                            )?;
                        } else {
                            self.global(
                                "Target.detachFromTarget",
                                json!({"sessionId":previous_session}),
                                deadline,
                            )?;
                        }
                    }
                }
                Some("Target.detachedFromTarget")
                    if event["params"]["sessionId"] == self.session =>
                {
                    self.close(deadline);
                    return Err(BrowserError::DocumentChanged);
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn request(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<CdpReply, BrowserError> {
        if self.closed {
            return Err(BrowserError::DocumentChanged);
        }
        let reply = self.peer.request(
            method,
            params,
            Some(&self.session),
            deadline,
            &mut admit_paused_target,
        );
        self.replied(reply, deadline)
    }

    /// Apply the events a reply carried. A recoverable error reply still has
    /// the events that preceded it applied first, so a chooser or detach that
    /// arrived before a stale-node error is the outcome, not the stale node.
    /// A fatal error ends the generation as it is; the attachments a failed
    /// admission left behind are not applied (they would be admitted again).
    fn replied(
        &mut self,
        reply: Result<CdpReply, BrowserError>,
        deadline: Instant,
    ) -> Result<CdpReply, BrowserError> {
        match reply {
            Ok(reply) => {
                self.events(&reply.events, deadline)?;
                Ok(reply)
            }
            Err(error) if recoverable(&error) => {
                let preceding = self.peer.drain_events()?;
                self.events(&preceding, deadline)?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    /// Send one input event (or the `DOM.focus` that starts a `type`) only
    /// while the document is the one every check since `epoch` ran on: no
    /// document-changing event was applied since and the document is open.
    /// Otherwise the approval is stale; a caller past its first input fails
    /// closed on that.
    fn dispatch_input(
        &mut self,
        epoch: u64,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<CdpReply, BrowserError> {
        if self.closed {
            return Err(BrowserError::DocumentChanged);
        }
        if self.epoch != epoch {
            return Err(BrowserError::StaleRevision);
        }
        self.request(method, params, deadline)
    }

    fn global(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<CdpReply, BrowserError> {
        let reply = self
            .peer
            .request(method, params, None, deadline, &mut admit_paused_target);
        self.replied(reply, deadline)
    }

    pub(super) fn frame(
        &mut self,
        deadline: Instant,
    ) -> Result<(BrowserDocument, CdpReply), BrowserError> {
        let (document, reply) = self.main_frame(deadline)?;
        if !http_document(&document.url) {
            return Err(BrowserError::DocumentChanged);
        }
        Ok((document, reply))
    }

    /// The main frame as DevTools reports it, whatever its address.
    fn main_frame(
        &mut self,
        deadline: Instant,
    ) -> Result<(BrowserDocument, CdpReply), BrowserError> {
        let target = self.target.clone();
        let session = self.session.clone();
        let reply = self.request("Page.getFrameTree", json!({}), deadline)?;
        if self.target != target || self.session != session {
            // The document asked about is gone, but the browser has a new
            // current target that a fresh look will observe.
            return Err(BrowserError::StaleRevision);
        }
        let frame = &reply.result["frameTree"]["frame"];
        let document = BrowserDocument {
            target_id: target,
            session_id: session,
            frame_id: frame["id"]
                .as_str()
                .ok_or(BrowserError::Protocol)?
                .to_owned(),
            loader_id: frame["loaderId"]
                .as_str()
                .ok_or(BrowserError::Protocol)?
                .to_owned(),
            url: frame["url"]
                .as_str()
                .ok_or(BrowserError::Protocol)?
                .to_owned(),
        };
        Ok((document, reply))
    }

    pub fn current(&mut self, deadline: Instant) -> Result<BrowserDocument, BrowserError> {
        match self.frame(deadline) {
            Ok((document, _)) => Ok(document),
            Err(error) => {
                if !recoverable(&error) {
                    self.close(deadline);
                }
                Err(error)
            }
        }
    }

    /// The live main document, which an observation may only use while it is
    /// still the one the decision was made on. A different document becomes
    /// the subject of the next decision instead.
    fn observed_document(
        &mut self,
        approved: Option<&BrowserDocument>,
        deadline: Instant,
    ) -> Result<BrowserDocument, BrowserError> {
        let live = self.current(deadline)?;
        if let Some(approved) = approved {
            if *approved != live {
                self.verified = Some(live);
                return Err(BrowserError::StaleRevision);
            }
        }
        Ok(live)
    }

    pub fn read(&mut self, capture: bool, deadline: Instant) -> Result<BrowserRead, BrowserError> {
        self.read_verified(None, capture, deadline)
    }

    /// Read the current document; when `expected` is given, only that document
    /// (the one a decision was made on).
    fn read_verified(
        &mut self,
        expected: Option<&BrowserDocument>,
        capture: bool,
        deadline: Instant,
    ) -> Result<BrowserRead, BrowserError> {
        self.snapshot = None;
        let result = self.read_transaction(expected, capture, deadline);
        if result
            .as_ref()
            .is_err_and(|error| *error != BrowserError::StaleRevision)
        {
            self.close(deadline);
        }
        result
    }

    fn read_transaction(
        &mut self,
        expected: Option<&BrowserDocument>,
        capture: bool,
        deadline: Instant,
    ) -> Result<BrowserRead, BrowserError> {
        self.poll(deadline)?;
        // A new target starts at `about:blank`; read it once its http(s) document
        // has loaded, so the title and target events of that load do not count
        // as a change in the middle of the read.
        while self.awaiting_popup {
            let reply = self.request("Page.getFrameTree", json!({}), deadline)?;
            let url = reply.result["frameTree"]["frame"]["url"]
                .as_str()
                .ok_or(BrowserError::Protocol)?
                .to_owned();
            if url != "about:blank" && !http_document(&url) {
                return Err(BrowserError::DocumentChanged);
            }
            if http_document(&url) && self.evaluate("document.readyState", deadline)? == "complete"
            {
                self.awaiting_popup = false;
                self.navigation_started = true;
                break;
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(BrowserError::PageTimeout)?;
            thread::sleep(Duration::from_millis(10).min(remaining));
        }
        let (before, first) = self.frame(deadline)?;
        if expected.is_some_and(|expected| *expected != before) {
            return Err(BrowserError::StaleRevision);
        }
        let mut accessibility = self
            .request("Accessibility.getFullAXTree", json!({}), deadline)?
            .result;
        let mut frames = Vec::new();
        http_child_frames(&first.result["frameTree"], &mut frames);
        for frame in frames {
            let child = self
                .request(
                    "Accessibility.getFullAXTree",
                    json!({"frameId":frame}),
                    deadline,
                )?
                .result;
            if let (Some(nodes), Some(child_nodes)) = (
                accessibility["nodes"].as_array_mut(),
                child["nodes"].as_array(),
            ) {
                nodes.extend(child_nodes.iter().cloned());
            }
        }
        let text = self.request("Runtime.evaluate", json!({"expression":"({text:document.body ? document.body.innerText : '',title:document.title})","returnByValue":true}), deadline)?.result;
        let snapshot = BrowserSnapshot::collect(
            self,
            before.clone(),
            first.document_epoch,
            &accessibility,
            deadline,
        )?;
        let elements = snapshot
            .elements
            .iter()
            .map(|node| node.element.clone())
            .collect();
        let boxes = snapshot
            .elements
            .iter()
            .map(|node| json!({"backendNodeId":node.backend_id,"rect":node.rect}))
            .collect();
        let screenshot = if capture {
            Some(self.capture(&snapshot, deadline)?)
        } else {
            None
        };
        let (after, last) = self.frame(deadline)?;
        if before != after
            || first.document_epoch != last.document_epoch
            || first.sequence >= last.sequence
        {
            return Err(BrowserError::DocumentChanged);
        }
        self.verified = Some(after.clone());
        let read = BrowserRead {
            revision: snapshot.revision.clone(),
            elements,
            omitted_elements: snapshot.omitted,
            notice: self.notice.take(),
            document: after,
            // Raw AX values can contain password data. The numbered list is the
            // only public element representation after this verified transaction.
            accessibility: json!({"nodes":[]}),
            boxes,
            screenshot,
            visible_text: text["result"]["value"]["text"]
                .as_str()
                .ok_or(BrowserError::Protocol)?
                .to_owned(),
            title: text["result"]["value"]["title"]
                .as_str()
                .ok_or(BrowserError::Protocol)?
                .to_owned(),
        };
        self.snapshot = Some(snapshot);
        Ok(read)
    }

    pub(super) fn node_function(
        &mut self,
        id: u64,
        function: &str,
        arguments: Value,
        deadline: Instant,
    ) -> Result<Value, BrowserError> {
        let object = self
            .request(
                "DOM.resolveNode",
                json!({"backendNodeId":id,"objectGroup":"shacs-browser"}),
                deadline,
            )?
            .result;
        let object_id = object["object"]["objectId"]
            .as_str()
            .ok_or(BrowserError::StaleRevision)?;
        let result = self.request("Runtime.callFunctionOn", json!({
            "objectId":object_id,"functionDeclaration":function,"arguments":arguments,"returnByValue":true
        }), deadline);
        let cleanup = self.request(
            "Runtime.releaseObject",
            json!({"objectId":object_id}),
            deadline,
        );
        let (reply, _) = with_cleanup(result, cleanup)?;
        if reply.result.get("exceptionDetails").is_some() {
            return Err(BrowserError::Protocol);
        }
        Ok(reply.result["result"]["value"].clone())
    }

    fn evaluate(&mut self, expression: &str, deadline: Instant) -> Result<Value, BrowserError> {
        let reply = self.request(
            "Runtime.evaluate",
            json!({"expression":expression,"returnByValue":true}),
            deadline,
        )?;
        if reply.result.get("exceptionDetails").is_some() {
            return Err(BrowserError::Protocol);
        }
        Ok(reply.result["result"]["value"].clone())
    }

    fn capture(
        &mut self,
        snapshot: &BrowserSnapshot,
        deadline: Instant,
    ) -> Result<String, BrowserError> {
        let id = format!("shacs-browser-{}", snapshot.revision);
        let labels: Vec<Value> = snapshot
            .elements
            .iter()
            .map(|node| {
                json!({
                    "number":node.element.element,"x":node.rect[0],"y":node.rect[1]
                })
            })
            .collect();
        let expression = format!(
            r#"(()=>{{const root=document.createElement('div');root.id={};root.style.cssText='position:fixed;inset:0;pointer-events:none!important;z-index:2147483647';document.documentElement.appendChild(root);for(const p of {}){{const label=document.createElement('span');label.textContent=String(p.number);label.style.cssText='position:absolute;background:#ffeb00;color:#000;font: bold 14px sans-serif;padding:2px;pointer-events:none!important';label.style.left=p.x+'px';label.style.top=p.y+'px';root.appendChild(label);}}return true;}})()"#,
            json!(id),
            json!(labels)
        );
        let session = self.session.clone();
        let result = self.evaluate(&expression, deadline).and_then(|_| {
            self.request(
                "Page.captureScreenshot",
                json!({"format":"png","captureBeyondViewport":false,"fromSurface":true}),
                deadline,
            )
        });
        // Use the original session even if an event changed the active target.
        let cleanup = self.peer.request(
            "Runtime.evaluate",
            json!({
                "expression":format!("document.getElementById({})?.remove()", json!(id)),
                "returnByValue":true
            }),
            Some(&session),
            deadline,
            &mut admit_paused_target,
        );
        let (reply, cleanup) = with_cleanup(result, cleanup)?;
        self.events(&cleanup.events, deadline)?;
        if cleanup.result.get("exceptionDetails").is_some() {
            return Err(BrowserError::Protocol);
        }
        reply.result["data"]
            .as_str()
            .map(str::to_owned)
            .ok_or(BrowserError::Protocol)
    }

    pub fn act(&mut self, action: &BrowserAction, deadline: Instant) -> Result<(), BrowserError> {
        self.act_inner(action, None, None, deadline).map(|_| ())
    }

    /// `observed` is the document an observation (`scroll`, `back`, `wait`) was
    /// decided on; every step of it is checked against the live document.
    /// Returns whether the action left that document on purpose.
    fn act_inner(
        &mut self,
        action: &BrowserAction,
        binding: Option<(&Value, &str)>,
        observed: Option<&BrowserDocument>,
        deadline: Instant,
    ) -> Result<bool, BrowserError> {
        let mut departed = false;
        self.poll(deadline)?;
        if let Some(observed) = observed {
            if self.verified.as_ref() != Some(observed) {
                return Err(BrowserError::StaleRevision);
            }
        }
        self.notice = None;
        let snapshot = self.snapshot.take();
        match action {
            BrowserAction::Navigate { url } => {
                if !http_document(url) {
                    return Err(BrowserError::DocumentChanged);
                }
                self.navigation_started = true;
                let reply = self.request("Page.navigate", json!({"url":url}), deadline)?;
                if reply.result.get("errorText").is_some() {
                    return Err(BrowserError::Protocol);
                }
            }
            BrowserAction::Back => {
                // The navigation is the point; the document it starts from is
                // still the one the decision named.
                self.observed_document(observed, deadline)?;
                let history = self.request("Page.getNavigationHistory", json!({}), deadline)?;
                let index = history.result["currentIndex"]
                    .as_u64()
                    .ok_or(BrowserError::Protocol)?;
                // The history request processed events too: the document must
                // still be the decided one right before the navigation goes out.
                self.observed_document(observed, deadline)?;
                if index > 0 {
                    let entry = &history.result["entries"][(index - 1) as usize];
                    self.request(
                        "Page.navigateToHistoryEntry",
                        json!({"entryId":entry["id"]}),
                        deadline,
                    )?;
                    departed = true;
                } else {
                    self.notice = Some("browser_no_previous_history".to_owned());
                }
            }
            // The coordinate click, raw text and wheel have no approval binding
            // (PRD 010 host API); they still send input only on the document
            // the poll above saw, and a press whose document changed is not
            // released.
            BrowserAction::Click { x, y } => {
                let epoch = self.epoch;
                self.dispatch_input(
                    epoch,
                    "Input.dispatchMouseEvent",
                    json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
                    deadline,
                )?;
                let released = self
                    .dispatch_input(
                        epoch,
                        "Input.dispatchMouseEvent",
                        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
                        deadline,
                    )
                    .map(|_| ());
                self.fail_closed(released)?;
            }
            BrowserAction::Type { text } => {
                let epoch = self.epoch;
                self.dispatch_input(epoch, "Input.insertText", json!({"text":text}), deadline)?;
            }
            BrowserAction::Press { key } => {
                self.current(deadline)?;
                if !matches!(key.as_str(), "Tab" | "Shift+Tab" | "Escape")
                    && self.focused_input_restricted(deadline)?
                {
                    return Err(BrowserError::InputDenied);
                }
                let epoch = self.epoch;
                let bound = match binding {
                    Some((arguments, digest)) => {
                        let snapshot = snapshot.as_ref().ok_or(BrowserError::StaleRevision)?;
                        self.recheck_binding(snapshot, arguments, digest, false, deadline)?;
                        Some(PressBinding {
                            snapshot,
                            arguments,
                            digest,
                            typed: None,
                        })
                    }
                    None => None,
                };
                self.press(key, bound, epoch, true, deadline)?;
            }
            BrowserAction::Scroll { x, y } => {
                let epoch = self.epoch;
                self.dispatch_input(
                    epoch,
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseWheel","x":640,"y":360,"deltaX":x,"deltaY":y}),
                    deadline,
                )?;
            }
            BrowserAction::ClickElement { element, revision } => {
                let snapshot = snapshot.ok_or(BrowserError::StaleRevision)?;
                let node = snapshot.target(self, *element, revision, deadline)?;
                if node.kind == "file" || self.frame_owner_target(node.backend_id, deadline)? {
                    return Err(BrowserError::InputDenied);
                }
                // Every check below runs on the document of this epoch; the
                // press goes out only if it is still that document.
                let epoch = self.epoch;
                snapshot.confirm_document(self, deadline)?;
                if let Some((arguments, digest)) = binding {
                    self.recheck_binding(&snapshot, arguments, digest, false, deadline)?;
                }
                // The hit test is the last request before the press: the point
                // is checked against the approved target as it stands now.
                let (x, y) = node.hit(self, deadline)?;
                self.dispatch_input(
                    epoch,
                    "Input.dispatchMouseEvent",
                    json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
                    deadline,
                )?;
                // A mousedown handler may have re-pointed the form, or moved or
                // covered the target. The release is the committing event: when
                // the binding or the physical target no longer holds, or cannot
                // be told, no further input of any kind is safe (pointer capture,
                // a wrapping label or link, and handlers that move the target
                // can all turn a release elsewhere into the activation), so the
                // browser ends.
                let epoch = self.epoch;
                let release = self.release_allowed(&snapshot, binding, &node, x, y, deadline);
                self.fail_closed(release)?;
                let released = self
                    .dispatch_input(
                        epoch,
                        "Input.dispatchMouseEvent",
                        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
                        deadline,
                    )
                    .map(|_| ());
                self.fail_closed(released)?;
            }
            BrowserAction::TypeElement {
                element,
                revision,
                text,
                submit,
            } => {
                let snapshot = snapshot.ok_or(BrowserError::StaleRevision)?;
                let node = snapshot.target(self, *element, revision, deadline)?;
                if node.password
                    || node.kind == "file"
                    || !node.editable
                    || self.frame_owner_target(node.backend_id, deadline)?
                {
                    return Err(BrowserError::InputDenied);
                }
                let epoch = self.epoch;
                snapshot.confirm_document(self, deadline)?;
                if let Some((arguments, digest)) = binding {
                    self.recheck_binding(&snapshot, arguments, digest, false, deadline)?;
                }
                self.dispatch_input(
                    epoch,
                    "DOM.focus",
                    json!({"backendNodeId":node.backend_id}),
                    deadline,
                )?;
                // From the focus on, text and Enter go only to the approved
                // element; a focus, selection or input handler that sends focus
                // elsewhere ends the sequence instead.
                let epoch = self.epoch;
                self.focus_or_terminate(node.backend_id, deadline)?;
                let selected = self.node_function(node.backend_id, "function(){if(this.isContentEditable){const r=this.ownerDocument.createRange();r.selectNodeContents(this);const s=this.ownerDocument.defaultView.getSelection();s.removeAllRanges();s.addRange(r);}else{this.select();}return true;}", json!([]), deadline);
                self.fail_closed(selected.map(|_| ()))?;
                if let Some((arguments, digest)) = binding {
                    let rechecked =
                        self.recheck_binding(&snapshot, arguments, digest, false, deadline);
                    self.fail_closed(rechecked)?;
                }
                // The focus predicate is the last request before the text.
                let inserted = self.focus_then_dispatch(
                    node.backend_id,
                    epoch,
                    "Input.insertText",
                    json!({"text":text}),
                    deadline,
                );
                self.fail_closed(inserted)?;
                if *submit {
                    let epoch = self.epoch;
                    if let Some((arguments, digest)) = binding {
                        let rechecked =
                            self.recheck_binding(&snapshot, arguments, digest, false, deadline);
                        self.fail_closed(rechecked)?;
                    }
                    let bound = binding.map(|(arguments, digest)| PressBinding {
                        snapshot: &snapshot,
                        arguments,
                        digest,
                        typed: Some(&node),
                    });
                    self.press("Enter", bound, epoch, false, deadline)?;
                }
            }
            BrowserAction::ScrollElement {
                element,
                revision,
                direction,
            } => {
                self.observed_document(observed, deadline)?;
                let (horizontal, vertical) = match direction.as_str() {
                    "up" => (0, -1),
                    "down" => (0, 1),
                    "left" => (-1, 0),
                    "right" => (1, 0),
                    _ => return Err(BrowserError::Protocol),
                };
                if let (Some(element), Some(revision)) = (element, revision) {
                    let snapshot = snapshot.ok_or(BrowserError::StaleRevision)?;
                    let node = snapshot.target(self, *element, revision, deadline)?;
                    let object = self.request(
                        "DOM.resolveNode",
                        json!({"backendNodeId":node.backend_id,"objectGroup":"shacs-browser"}),
                        deadline,
                    )?;
                    let object_id = object.result["object"]["objectId"]
                        .as_str()
                        .ok_or(BrowserError::StaleRevision)?
                        .to_owned();
                    // Resolving the node processed events; the document must still
                    // be the decided one right before the scroll goes out.
                    let scrolled = match self.observed_document(observed, deadline) {
                        Ok(_) => self.request("Runtime.callFunctionOn", json!({
                            "objectId":object_id,
                            "functionDeclaration":"function(x,y){this.scrollBy(x*this.clientWidth*0.8,y*this.clientHeight*0.8);return true;}",
                            "arguments":[{"value":horizontal},{"value":vertical}],"returnByValue":true
                        }), deadline),
                        Err(error) => Err(error),
                    };
                    let cleanup = self.request(
                        "Runtime.releaseObject",
                        json!({"objectId":object_id}),
                        deadline,
                    );
                    let (reply, _) = with_cleanup(scrolled, cleanup)?;
                    if reply.result.get("exceptionDetails").is_some() {
                        return Err(BrowserError::Protocol);
                    }
                } else {
                    self.evaluate(&format!("window.scrollBy({horizontal}*innerWidth*0.8,{vertical}*innerHeight*0.8)"), deadline)?;
                }
            }
            BrowserAction::Wait { text, timeout_ms } => {
                let until = (Instant::now() + Duration::from_millis(*timeout_ms)).min(deadline);
                loop {
                    self.observed_document(observed, deadline)?;
                    if let Some(text) = text {
                        let found = self.evaluate(
                            &format!("(document.body?.innerText||'').includes({})", json!(text)),
                            deadline,
                        )?;
                        if found == true {
                            break;
                        }
                    }
                    let now = Instant::now();
                    if now >= until {
                        if text.is_some() {
                            self.notice = Some(
                                "browser_wait_timeout: requested text was not visible".to_owned(),
                            );
                        }
                        break;
                    }
                    thread::sleep(
                        Duration::from_millis(25).min(until.saturating_duration_since(now)),
                    );
                }
            }
        }
        Ok(departed)
    }

    /// Fail closed in the middle of an input sequence: send nothing more and
    /// report a changed document, on which the manager drops this generation
    /// and kills the browser. The input already sent cannot be undone.
    fn terminate<T>(&mut self) -> Result<T, BrowserError> {
        self.closed = true;
        Err(BrowserError::DocumentChanged)
    }

    /// One key press. With a binding, the activation this key causes happens
    /// on `keyDown` (Chromium fires keydown and, for Enter, keypress from it),
    /// which the caller checked at `epoch`; the binding is checked again
    /// before `keyUp`. Tab and Shift+Tab move focus on purpose: their new
    /// focus is accepted when the document tree can place it in this verified
    /// document, while the start element's role, name, form and submitter
    /// still have to hold. Every other key keeps the strict focus check. An
    /// Enter whose approved submission already left the document sends no
    /// `keyUp` into the new document. Any other change ends the browser.
    /// `first_input` says whether the key is the first event of its sequence
    /// (a stale approval there is still recoverable).
    fn press(
        &mut self,
        key: &str,
        binding: Option<PressBinding<'_>>,
        epoch: u64,
        first_input: bool,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        let (key, modifiers) = if key == "Shift+Tab" {
            ("Tab", 8)
        } else {
            (key, 0)
        };
        let code = match key {
            "Enter" => 13,
            "Tab" => 9,
            "Escape" => 27,
            "ArrowUp" => 38,
            "ArrowDown" => 40,
            "ArrowLeft" => 37,
            "ArrowRight" => 39,
            "Home" => 36,
            "End" => 35,
            "PageUp" => 33,
            "PageDown" => 34,
            "Backspace" => 8,
            "Delete" => 46,
            _ => return Err(BrowserError::Protocol),
        };
        let moves_focus = key == "Tab";
        // The element the key is bound to go to: the typed element, or the
        // approved focused node. Its focus predicate is the last request before
        // each key event.
        let start_focus = binding.as_ref().and_then(|bound| {
            bound
                .typed
                .map(|node| node.backend_id)
                .or_else(|| bound.snapshot.focus["node"].as_u64())
        });
        let down_params = json!({"type":"keyDown","key":key,"code":key,"windowsVirtualKeyCode":code,"modifiers":modifiers,"text":if key=="Enter" {"\r"} else {""}});
        let down = match start_focus {
            Some(node) => self.focus_then_dispatch(
                node,
                epoch,
                "Input.dispatchKeyEvent",
                down_params,
                deadline,
            ),
            None => self
                .dispatch_input(epoch, "Input.dispatchKeyEvent", down_params, deadline)
                .map(|_| ()),
        };
        if first_input {
            down?;
        } else {
            self.fail_closed(down)?;
        }
        let epoch = self.epoch;
        let mut up_guard = start_focus;
        if let Some(bound) = binding {
            match bound.snapshot.confirm_document(self, deadline) {
                Ok(()) => {}
                // The submission this Enter was approved for has committed and
                // the document is gone; a keyUp would be input into a document
                // nobody approved, so the sequence ends here.
                Err(BrowserError::StaleRevision) if key == "Enter" => return Ok(()),
                Err(BrowserError::StaleRevision) => return self.terminate(),
                Err(error) => return Err(error),
            }
            let rechecked = self.recheck_binding(
                bound.snapshot,
                bound.arguments,
                bound.digest,
                moves_focus,
                deadline,
            );
            self.fail_closed(rechecked)?;
            if moves_focus {
                // The destination must be a concrete node the verified document
                // tree can place; no focused element, a frame owner, another
                // target or an unverified frame ends the browser before keyUp.
                let destination = self
                    .live_focus(&bound.snapshot.document.frame_id, deadline)
                    .and_then(|focus| focus["node"].as_u64().ok_or(BrowserError::StaleRevision));
                up_guard = Some(self.fail_closed(destination)?);
            }
        }
        let up_params = json!({"type":"keyUp","key":key,"code":key,"windowsVirtualKeyCode":code,"modifiers":modifiers});
        let up = match up_guard {
            Some(node) => {
                self.focus_then_dispatch(node, epoch, "Input.dispatchKeyEvent", up_params, deadline)
            }
            None => self
                .dispatch_input(epoch, "Input.dispatchKeyEvent", up_params, deadline)
                .map(|_| ()),
        };
        self.fail_closed(up)
    }

    fn focused_input_restricted(&mut self, deadline: Instant) -> Result<bool, BrowserError> {
        let (_, tree) = self.frame(deadline)?;
        let mut frames = Vec::new();
        http_child_frames(&tree.result["frameTree"], &mut frames);
        let mut trees = vec![
            self.request("Accessibility.getFullAXTree", json!({}), deadline)?
                .result,
        ];
        for frame in frames {
            trees.push(
                self.request(
                    "Accessibility.getFullAXTree",
                    json!({"frameId":frame}),
                    deadline,
                )?
                .result,
            );
        }
        for tree in trees {
            if let Some(nodes) = tree["nodes"].as_array() {
                for node in nodes {
                    let focused = node["properties"].as_array().is_some_and(|properties| {
                        properties.iter().any(|property| {
                            property["name"] == "focused" && property["value"]["value"] == true
                        })
                    });
                    if focused {
                        let id = node["backendDOMNodeId"]
                            .as_u64()
                            .ok_or(BrowserError::InputDenied)?;
                        let (kind, password, _) = browser_snapshot::node_input(self, id, deadline)?;
                        if password || matches!(kind.as_str(), "file" | "iframe") {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }

    pub fn close(&mut self, deadline: Instant) {
        if !self.closed {
            let _ = self.peer.request(
                "Target.closeTarget",
                json!({"targetId":self.target}),
                None,
                deadline,
                &mut admit_paused_target,
            );
            self.closed = true;
        }
    }

    /// Asks Chromium to end by itself, so that it writes its profile out
    /// before it goes (Spec 050 PRD 020 scope 4). It is sent even after the
    /// manager cancelled every other request, which a shutdown does. An error
    /// means the connection is gone and only a kill is left.
    pub fn quit(&mut self, deadline: Instant) -> Result<(), BrowserError> {
        self.peer.set_cancellation(BrowserCancellation::default());
        self.peer
            .request(
                "Browser.close",
                json!({}),
                None,
                deadline,
                &mut admit_paused_target,
            )
            .map(|_| ())
    }
}

fn http_document(url: &str) -> bool {
    // Reuse the existing URL parser without DNS lookup; the proxy owns resolution.
    shacs_core::tools::parse_http_url(url).is_ok()
}

fn http_child_frames(tree: &Value, frames: &mut Vec<String>) {
    if let Some(children) = tree["childFrames"].as_array() {
        for child in children {
            if child["frame"]["url"].as_str().is_some_and(http_document) {
                if let Some(id) = child["frame"]["id"].as_str() {
                    frames.push(id.to_owned());
                }
                http_child_frames(child, frames);
            }
        }
    }
}
