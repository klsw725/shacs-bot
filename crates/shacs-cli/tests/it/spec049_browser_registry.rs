use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_api::{ChatCompletionAdapter, ChatCompletionInvocation};
use shacs_cli::browser::{
    BrowserCancellation, BrowserState, CdpDocument, CdpReply, DevToolsPeer, PausedTargetAdmission,
};
use shacs_cli::AgentLoopChatCompletionAdapter;
use shacs_config::{Config, ConfigBundle, ConfigContext};
use shacs_core::runtime::{
    self, BrowserAction, BrowserCallContext, BrowserDocument, BrowserError, BrowserHost,
    BrowserPreparedAction, BrowserRead, BrowserSession, PermissionMode, PermissionModeSnapshot,
    SubagentExecutionConfig,
};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest,
    ToolCallRequest,
};
use std::collections::VecDeque;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Default)]
struct Facts {
    commands: Vec<(String, Value)>,
    contexts: Vec<BrowserCallContext>,
    covered: bool,
    descendant: bool,
    wrong_frame: bool,
    changed: bool,
    capture_failure: bool,
    overlay_failure: bool,
    extra_nodes: u64,
    child_frame: bool,
    focused_password: bool,
    loader_change: bool,
    popup: bool,
    value: String,
    layout_scroll: [f64; 2],
    visual_offset: [f64; 2],
    child_scroll: [f64; 2],
    target_position: Option<[f64; 2]>,
    frame_position: Option<[f64; 2]>,
    covering_rect: Option<[f64; 4]>,
    no_node: bool,
    mouse_targets: Vec<u64>,
    form: Option<Value>,
    focused_node: Option<u64>,
    mutate_after_hit: Option<Value>,
    mutate_after_press: Option<Value>,
    mutate_after_keydown: Option<u64>,
    late_events: Vec<Value>,
    /// The click handler opens a popup that attaches paused; its reply waits
    /// for the resume, as the real renderer does.
    popup_paused: bool,
    popup_resumed: bool,
    popup_blank_polls: u32,
    popup_loading_polls: u32,
    main_url: Option<String>,
    /// The main frame navigates while a frame-tree reply is produced.
    navigate_on_frame_tree: Option<String>,
    /// Main frame-tree replies to answer normally before that navigation.
    navigate_after_frames: Option<u32>,
    /// Popups that attach, already admitted, in the reply to a mouse release.
    popup_batch: Vec<&'static str>,
    /// A popup that attaches, already admitted, while the original tab is
    /// being closed.
    popup_on_close: Option<&'static str>,
    /// The main frame navigates while the navigation history is produced.
    navigate_on_history: Option<String>,
    history_index: u64,
    /// Events the next main frame-tree reply carries (a fatal one, a popup).
    fatal_on_frame_tree: Option<Vec<Value>>,
    /// `DOM.describeNode` of this object fails as a stale description.
    describe_fails_object: Option<String>,
    /// `Runtime.releaseObject` of this object carries a file chooser event.
    release_fatal_object: Option<String>,
    /// Where a handler moved focus: on `DOM.focus`, or on text insertion.
    focus_moves_on: Option<&'static str>,
    focus_moved: bool,
    navigate_on_resolve: bool,
    navigate_on_scroll: bool,
    /// The target moves after the press, or after the Nth partial AX request.
    move_target_after_press: Option<[f64; 2]>,
    cover_after_press: bool,
    move_target_after_partial_ax: Option<(u32, [f64; 2])>,
    mutate_form_after_keydown: Option<Value>,
    navigate_after_keydown: bool,
    /// The Nth focus-predicate call (0-based) carries a navigation in its own
    /// reply, or in the release that follows it.
    navigate_on_focus_check: Option<u32>,
    /// The Nth focus-predicate reply carries a child frame removal.
    detach_on_focus_check: Option<u32>,
    /// The Nth location lookup reply carries a child frame removal.
    detach_on_lookup: Option<u32>,
    location_lookups: u32,
    /// The window is not active (`document.hasFocus()` is false) while the
    /// document still retains its active element.
    page_unfocused: bool,
    /// Node 1 is a host with a closed shadow root holding element 300.
    shadow_host: bool,
    /// The element inside node 1's closed shadow root that holds focus. The
    /// host is then `document.activeElement` (retargeted), but Blink's focused
    /// element is the inner one.
    inner_focused: Option<u64>,
    /// An input handler moves focus to this inner element on insertion.
    focus_into_shadow_on_insert: Option<u64>,
    /// Node 1's closed shadow root is empty when described (its focused input
    /// is added later).
    shadow_host_empty: bool,
    /// `DOM.focus` went out: the accessibility queries on the typed element
    /// from here on are the focus checks (first check, the recheck's own
    /// query, the guard before the text).
    focus_dispatched: bool,
    /// Node 9 (present with `extra_nodes >= 1`) is an editable `<iframe>`
    /// owning a child frame. Focusing it makes it the parent document's
    /// focused element while page focus moves to the child frame: Blink keeps
    /// the two apart, so the fake does too.
    numbered_frame_owner: bool,
    page_focused_frame: Option<&'static str>,
    /// The subtree description of the click target carries a navigation, or
    /// fails as a stale node once the press went out.
    navigate_on_subtree_describe: bool,
    subtree_describe_fails_after_press: bool,
    subtree_describe_fails: bool,
    /// The focused input sits in a closed shadow root: a walk down through
    /// `shadowRoot` cannot see it.
    closed_shadow_host: bool,
}

impl Facts {
    /// Blink's `Document::FocusedElement()`: the one element that receives
    /// keys and text, regardless of shadow retargeting or window activation.
    fn focused_element(&self) -> Option<u64> {
        if let Some(inner) = self.inner_focused {
            return Some(inner);
        }
        if self.focus_moved {
            return Some(3);
        }
        self.focused_node.or(self.focused_password.then_some(6))
    }

    fn page_offset(&self) -> [f64; 2] {
        [
            self.layout_scroll[0] + self.visual_offset[0],
            self.layout_scroll[1] + self.visual_offset[1],
        ]
    }

    fn target_rect(&self) -> [f64; 4] {
        let [mut x, mut y] = self.target_position.unwrap_or([10.0, 10.0]);
        if self.child_frame {
            let [frame_x, frame_y] = self.frame_position.unwrap_or([200.0, 100.0]);
            x += frame_x - self.child_scroll[0];
            y += frame_y - self.child_scroll[1];
        }
        let [page_x, page_y] = self.page_offset();
        [x - page_x, y - page_y, 100.0, 30.0]
    }

    fn node_at_viewport(&self, x: f64, y: f64) -> Option<(u64, &'static str)> {
        if !(0.0..1280.0).contains(&x) || !(0.0..720.0).contains(&y) {
            return None;
        }
        if self
            .covering_rect
            .is_some_and(|rect| contains_point(rect, x, y))
        {
            return Some((99, "frame"));
        }
        if !contains_point(self.target_rect(), x, y) {
            let [frame_x, frame_y] = self.frame_position.unwrap_or([200.0, 100.0]);
            let inside_child =
                self.child_frame && contains_point([frame_x, frame_y, 400.0, 300.0], x, y);
            return Some((900, if inside_child { "child" } else { "frame" }));
        }
        if self.covered {
            return Some((99, "frame"));
        }
        let frame = if self.wrong_frame {
            "other-frame"
        } else if self.child_frame {
            "child"
        } else {
            "frame"
        };
        Some((if self.descendant { 20 } else { 2 }, frame))
    }
}

fn contains_point([left, top, width, height]: [f64; 4], x: f64, y: f64) -> bool {
    (left..left + width).contains(&x) && (top..top + height).contains(&y)
}

struct Peer {
    facts: Arc<Mutex<Facts>>,
    sequence: u64,
    epoch: u64,
    frames: usize,
}

fn ax(id: u64) -> Value {
    let role = if matches!(id, 2 | 3) {
        "button"
    } else {
        "textbox"
    };
    json!({"nodeId":id.to_string(),"backendDOMNodeId":id,"role":{"value":role},
        "name":{"value":format!("element {id}")},"properties":[{"name":"disabled","value":{"value":id==5}}]})
}

impl DevToolsPeer for Peer {
    fn set_cancellation(&mut self, _cancellation: BrowserCancellation) {}
    fn request(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        deadline: Instant,
        admit: PausedTargetAdmission<'_>,
    ) -> Result<CdpReply, BrowserError> {
        let paused_popup = {
            let mut facts = self
                .facts
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?;
            facts.commands.push((method.to_owned(), params.clone()));
            if method == "Runtime.runIfWaitingForDebugger" && session == Some("popup-session") {
                facts.popup_resumed = true;
            }
            method == "Input.dispatchMouseEvent"
                && params["type"] == "mouseReleased"
                && std::mem::take(&mut facts.popup_paused)
        };
        self.sequence += 1;
        let mut events = Vec::new();
        if paused_popup {
            // Real Chromium: the popup attaches paused while this input's reply
            // is pending, and the opener's renderer (so this reply) stays blocked
            // until the popup resumes.
            let mut attached = json!({"method":"Target.attachedToTarget","params":{"sessionId":"popup-session",
                "waitingForDebugger":true,"targetInfo":{"targetId":"popup","type":"page","url":"","browserContextId":"context-1"}}});
            let nested = admit(self, &attached, deadline)?;
            if !self
                .facts
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?
                .popup_resumed
            {
                return Err(BrowserError::PageTimeout);
            }
            shacs_cli::browser::mark_admitted(&mut attached);
            events.push(attached);
            events.extend(nested);
            self.epoch += 1;
        }
        let batch: Vec<&'static str> = {
            let mut facts = self
                .facts
                .lock()
                .map_err(|_| BrowserError::StateUnavailable)?;
            if method == "Input.dispatchMouseEvent" && params["type"] == "mouseReleased" {
                std::mem::take(&mut facts.popup_batch)
            } else if method == "Target.closeTarget" && params["targetId"] == "target" {
                facts.popup_on_close.take().into_iter().collect()
            } else {
                Vec::new()
            }
        };
        for id in batch {
            // Attached paused and admitted by the transport: the document layer
            // receives it marked, behind everything that arrived before it.
            let mut attached = json!({"method":"Target.attachedToTarget","params":{"sessionId":format!("{id}-session"),
                "waitingForDebugger":true,"targetInfo":{"targetId":id,"type":"page","url":"","browserContextId":"context-1"}}});
            let nested = admit(self, &attached, deadline)?;
            shacs_cli::browser::mark_admitted(&mut attached);
            events.push(attached);
            events.extend(nested);
            self.epoch += 1;
        }
        let mut facts = self
            .facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        let result = match method {
            "Target.getTargets" => json!({"targetInfos":[{"targetId":"target","type":"page"}]}),
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Page.getFrameTree" if session.is_some_and(|session| session != "session") => {
                self.frames += 1;
                let id = session
                    .and_then(|session| session.strip_suffix("-session"))
                    .unwrap_or("popup");
                let url = if facts.popup_blank_polls > 0 {
                    facts.popup_blank_polls -= 1;
                    "about:blank".to_owned()
                } else {
                    format!("https://example.com/{id}")
                };
                json!({"frameTree":{"frame":{"id":format!("{id}-frame"),"loaderId":format!("{id}-loader"),"url":url}}})
            }
            "Page.getFrameTree" => {
                self.frames += 1;
                if let Some(remaining) = facts.navigate_after_frames.as_mut() {
                    if *remaining == 0 {
                        facts.navigate_after_frames = None;
                        facts.navigate_on_frame_tree = Some("https://other.example/".to_owned());
                    } else {
                        *remaining -= 1;
                    }
                }
                if let Some(url) = facts.navigate_on_frame_tree.take() {
                    self.epoch += 1;
                    events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":url}}}));
                    facts.main_url = Some(url);
                }
                if let Some(batch) = facts.fatal_on_frame_tree.take() {
                    self.epoch += 1;
                    events.extend(batch);
                }
                let loader = if facts.main_url.is_some() {
                    "navigated"
                } else if facts.loader_change && self.frames > 1 {
                    "changed"
                } else {
                    "loader"
                };
                let url = facts
                    .main_url
                    .clone()
                    .unwrap_or_else(|| "https://example.com/".to_owned());
                let mut tree =
                    json!({"frameTree":{"frame":{"id":"frame","loaderId":loader,"url":url}}});
                if facts.child_frame {
                    tree["frameTree"]["childFrames"] = json!([{"frame":{"id":"child","parentId":"frame","loaderId":"child-loader","url":"https://example.com/child"}}]);
                }
                tree
            }
            "Accessibility.getFullAXTree" => {
                if params["frameId"] == "child" {
                    json!({"nodes":[ax(2)]})
                } else {
                    let nodes: Vec<Value> = (1..=8 + facts.extra_nodes)
                        .rev()
                        .map(|id| {
                            let mut node = ax(id);
                            if facts.focused_password && id == 6 {
                                node["properties"]
                                    .as_array_mut()
                                    .expect("properties")
                                    .push(json!({"name":"focused","value":{"value":true}}));
                            }
                            node
                        })
                        .collect();
                    json!({"nodes":nodes})
                }
            }
            "Accessibility.getPartialAXTree" => {
                if let Some((remaining, position)) = facts.move_target_after_partial_ax {
                    facts.move_target_after_partial_ax = if remaining == 0 {
                        facts.target_position = Some(position);
                        None
                    } else {
                        Some((remaining - 1, position))
                    };
                }
                let id = params["backendNodeId"]
                    .as_u64()
                    .ok_or(BrowserError::Protocol)?;
                if params["fetchRelatives"] == false && id == 1 && facts.focus_dispatched {
                    // A focus check on the typed element after `DOM.focus`. A
                    // navigation or a frame removal may ride on its reply.
                    if let Some(remaining) = facts.detach_on_focus_check {
                        facts.detach_on_focus_check = if remaining == 0 {
                            self.epoch += 1;
                            events.push(json!({"method":"Page.frameDetached","params":{"frameId":"child","reason":"remove"}}));
                            None
                        } else {
                            Some(remaining - 1)
                        };
                    }
                    if let Some(remaining) = facts.navigate_on_focus_check {
                        facts.navigate_on_focus_check = if remaining == 0 {
                            self.epoch += 1;
                            events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":"https://other.example/"}}}));
                            facts.main_url = Some("https://other.example/".to_owned());
                            None
                        } else {
                            Some(remaining - 1)
                        };
                    }
                }
                let mut node = ax(id);
                // `FillGlobalStates`: `focused` appears only on the element that is
                // the document's focused element, whatever the window state.
                if facts.focused_element() == Some(id) {
                    if let Some(properties) = node["properties"].as_array_mut() {
                        properties.push(json!({"name":"focused","value":{"type":"booleanOrUndefined","value":true}}));
                    }
                }
                if facts.changed {
                    node["name"]["value"] = json!("changed");
                }
                if params["backendNodeId"] == 50 || params["backendNodeId"] == 51 {
                    node["name"]["value"] = json!("Order form");
                    node["role"]["value"] = json!("form");
                }
                json!({"nodes":[node]})
            }
            "DOM.getDocument" => {
                let children:Vec<Value>=(1..=8+facts.extra_nodes).map(|id|{
                    if id==2 && facts.child_frame {
                        json!({"backendNodeId":200,"frameId":"child","contentDocument":{"backendNodeId":201,"documentURL":"https://example.com/child","children":[{"backendNodeId":2}]}})
                    } else {json!({"backendNodeId":id})}
                }).collect();
                json!({"root":{"backendNodeId":1000,"documentURL":"https://example.com/","children":children}})
            }
            "Page.getLayoutMetrics" => {
                let [page_x, page_y] = facts.page_offset();
                json!({
                    "cssLayoutViewport":{"pageX":facts.layout_scroll[0],"pageY":facts.layout_scroll[1],"clientWidth":1280,"clientHeight":720},
                    "cssVisualViewport":{"pageX":page_x,"pageY":page_y,"offsetX":facts.visual_offset[0],"offsetY":facts.visual_offset[1],"clientWidth":1280,"clientHeight":720,"scale":1}
                })
            }
            "DOM.getBoxModel" => {
                let [left, top, width, height] = if params["backendNodeId"] == 2 {
                    facts.target_rect()
                } else {
                    let [page_x, page_y] = facts.page_offset();
                    [10.0 - page_x, 10.0 - page_y, 100.0, 30.0]
                };
                json!({"model":{"border":[left,top,left+width,top,left+width,top+height,left,top+height]}})
            }
            "DOM.resolveNode" => {
                if std::mem::take(&mut facts.navigate_on_resolve) {
                    self.epoch += 1;
                    events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":"https://other.example/"}}}));
                    facts.main_url = Some("https://other.example/".to_owned());
                }
                json!({"object":{"objectId":params["backendNodeId"].to_string()}})
            }
            "Runtime.releaseObject" => {
                if facts.release_fatal_object.as_deref() == params["objectId"].as_str() {
                    self.epoch += 1;
                    events.push(json!({"method":"Page.fileChooserOpened","params":{}}));
                }
                json!({})
            }
            "DOM.focus" => {
                // `Element::Focus`: the node becomes the document's focused element
                // unless a focus handler sends focus elsewhere.
                facts.focused_node = params["backendNodeId"].as_u64();
                facts.focus_dispatched = true;
                if facts.numbered_frame_owner && params["backendNodeId"] == 9 {
                    // `HTMLFrameElementBase::SetFocused`: page focus goes to the child.
                    facts.page_focused_frame = Some("owned-frame-9");
                }
                if facts.focus_moves_on == Some("focus") {
                    facts.focus_moved = true;
                }
                json!({})
            }
            "DOM.describeNode" => {
                if facts.describe_fails_object.as_deref() == params["objectId"].as_str()
                    && params["objectId"].is_string()
                {
                    return Err(BrowserError::StaleRevision);
                }
                if params["depth"] == -1 && params["backendNodeId"] == 2 {
                    if facts.subtree_describe_fails {
                        return Err(BrowserError::StaleRevision);
                    }
                    if std::mem::take(&mut facts.navigate_on_subtree_describe) {
                        self.epoch += 1;
                        events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":"https://other.example/"}}}));
                        facts.main_url = Some("https://other.example/".to_owned());
                    }
                }
                let id = params["backendNodeId"]
                    .as_u64()
                    .or_else(|| params["objectId"].as_str()?.parse::<u64>().ok())
                    .ok_or(BrowserError::Protocol)?;
                let mut node = json!({"node":{"backendNodeId":id,"localName":if id==200 {"iframe"} else if id==210 {"frame"} else if id==220 {"object"} else if id==2 {"button"} else {"input"},
                    "children":if facts.descendant && id==2 {json!([{"backendNodeId":20}])}else{json!([])},
                    "attributes":if id==6 {json!(["type","password"])} else if id==7 {json!(["type","file"])}
                        else if id==8 {json!(["autocomplete","new-password"])} else {json!(["type","text"])}}});
                if id == 220 {
                    // An <object> with a browsing context: Chromium reports its frame.
                    node["node"]["frameId"] = json!("owned-frame");
                }
                if facts.numbered_frame_owner && id == 9 {
                    node["node"]["localName"] = json!("iframe");
                    node["node"]["frameId"] = json!("owned-frame-9");
                }
                if facts.shadow_host && id == 1 && params["depth"] == -1 {
                    let children = if facts.shadow_host_empty {
                        json!([])
                    } else {
                        json!([{"nodeType":1,"backendNodeId":300,"localName":"input","children":[]}])
                    };
                    node["node"]["shadowRoots"] = json!([{"nodeType":11,"backendNodeId":299,"shadowRootType":"closed","children":children}]);
                }
                node
            }
            "Runtime.callFunctionOn" => {
                let id = params["objectId"]
                    .as_str()
                    .and_then(|id| id.parse::<u64>().ok())
                    .ok_or(BrowserError::Protocol)?;
                let function = params["functionDeclaration"].as_str().unwrap_or("");
                if params["returnByValue"] == false {
                    let related = if function.contains("d.activeElement") {
                        // The deep focus. A predicate that asks the window's
                        // activation sees nothing on an unfocused page.
                        let focused = if function.contains("hasFocus") && facts.page_unfocused {
                            None
                        } else {
                            facts.focused_node.or(facts.focused_password.then_some(6))
                        };
                        if facts.child_frame && focused == Some(2) {
                            if id == 1000 {
                                Some(200)
                            } else if id == 201 {
                                Some(2)
                            } else {
                                None
                            }
                        } else if id == 1000 {
                            focused
                        } else {
                            None
                        }
                    } else if let Some(form) = &facts.form {
                        let submit_buttons: Vec<u64> = form["submit_buttons"]
                            .as_array()
                            .map(|buttons| buttons.iter().filter_map(Value::as_u64).collect())
                            .unwrap_or_else(|| vec![2]);
                        if function.contains("this.form||null") {
                            form["owner"].as_u64()
                        } else if function.contains("isSubmit(this)") {
                            // Enter: the focused submit control itself, else the default.
                            if submit_buttons.contains(&id) {
                                Some(id)
                            } else {
                                form["submitter"].as_u64()
                            }
                        } else if function.contains("?this:null") && submit_buttons.contains(&id) {
                            Some(id)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    related.map_or_else(
                        || json!({"result":{"subtype":"null","value":null}}),
                        |id| json!({"result":{"objectId":id.to_string()}}),
                    )
                } else if function.contains("formaction:this.getAttribute") {
                    json!({"result":{"value":facts.form.as_ref().map(|form| {
                        // Per-button overrides live under `overrides[<node id>]`.
                        let overrides = &form["overrides"][id.to_string()];
                        let pick = |key: &str| if overrides[key].is_string() { overrides[key].clone() } else { form[key].clone() };
                        let formaction = pick("formaction");
                        let formmethod = pick("formmethod");
                        json!({
                        "formaction":formaction,"formmethod":formmethod,
                        "formenctype":form["formenctype"],"formtarget":form["formtarget"],
                        "formnovalidate":form["formnovalidate"],"disabled":false,
                        "url":formaction.as_str().unwrap_or(form["action"].as_str().unwrap_or("")),
                        "method":formmethod.as_str().unwrap_or(form["method"].as_str().unwrap_or("get")),
                        "enctype":"application/x-www-form-urlencoded","target":"_self","no_validate":false
                    })})}})
                } else if function.contains("base_url:this.ownerDocument") {
                    json!({"result":{"value":facts.form.as_ref().map(|form|json!({
                        "base_url":form["base"],"action_attribute":form["action"],"action":form["action"],
                        "method":form["method"],"enctype":"application/x-www-form-urlencoded","target":"_self","no_validate":false
                    }))}})
                } else if function.contains("hasFocus") && facts.page_unfocused {
                    json!({"result":{"value":false}})
                } else if function.contains("r.activeElement!==n") && !function.contains("hasFocus")
                {
                    // The read-time shadow-candidate walk: a candidate holds focus
                    // when it is the document's focused element.
                    json!({"result":{"value":facts.focused_element() == Some(id)}})
                } else if function.contains(".shadowRoot") && facts.closed_shadow_host {
                    // A walk down through `shadowRoot` stops at a closed root.
                    json!({"result":{"value":false}})
                } else if function.contains("scrollBy")
                    && std::mem::take(&mut facts.navigate_on_scroll)
                {
                    self.epoch += 1;
                    events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":"https://other.example/"}}}));
                    facts.main_url = Some("https://other.example/".to_owned());
                    json!({"result":{"value":true}})
                } else if function.contains("contains(other)") {
                    json!({"result":{"value":facts.descendant}})
                } else if function.contains("getComputedStyle") {
                    json!({"result":{"value":{
                        "connected":true,"visible":id!=3 && id!=4,"disabled":id==5,
                        "kind":if id==6 {"password"} else if id==7 {"file"} else {"text"},
                        "password":id==6 || id==8,"editable":id!=2,
                        "value":if id==6 || id==7 || id==8 {"DO NOT EXPOSE"} else {facts.value.as_str()}
                    }}})
                } else {
                    json!({"result":{"value":true}})
                }
            }
            "DOM.getNodeForLocation" => {
                let lookup = facts.location_lookups;
                facts.location_lookups += 1;
                if facts.detach_on_lookup == Some(lookup) {
                    facts.detach_on_lookup = None;
                    self.epoch += 1;
                    events.push(json!({"method":"Page.frameDetached","params":{"frameId":"child","reason":"remove"}}));
                }
                if facts.no_node {
                    return Err(BrowserError::TargetCovered);
                }
                let document_x = params["x"].as_i64().ok_or(BrowserError::Protocol)?;
                let document_y = params["y"].as_i64().ok_or(BrowserError::Protocol)?;
                // Chromium subtracts the root layout scroll for hit-testing.
                // Convert that root-frame point to visual-viewport coordinates
                // before comparing it with this fixture's visible geometry.
                let root_x = document_x as f64 - facts.layout_scroll[0];
                let root_y = document_y as f64 - facts.layout_scroll[1];
                let (node, frame) = facts
                    .node_at_viewport(
                        root_x - facts.visual_offset[0],
                        root_y - facts.visual_offset[1],
                    )
                    .ok_or(BrowserError::TargetCovered)?;
                if let Some(form) = facts.mutate_after_hit.take() {
                    facts.form = Some(form);
                }
                json!({"backendNodeId":node,"frameId":frame})
            }
            "Runtime.evaluate" => {
                let expression = params["expression"].as_str().unwrap_or("");
                if facts.overlay_failure && expression.contains("const root=document.createElement")
                {
                    json!({"exceptionDetails":{"text":"fixture overlay error"}})
                } else if expression == "document.readyState" {
                    let state = if facts.popup_loading_polls > 0 {
                        facts.popup_loading_polls -= 1;
                        "loading"
                    } else {
                        "complete"
                    };
                    json!({"result":{"value":state}})
                } else if expression.contains("text:document.body") {
                    json!({"result":{"value":{"text":"visible fixture","title":"Fixture"}}})
                } else if expression.contains(".includes(") {
                    json!({"result":{"value":false}})
                } else {
                    json!({"result":{"value":true}})
                }
            }
            "Page.captureScreenshot" => {
                if facts.capture_failure {
                    return Err(BrowserError::Protocol);
                }
                json!({"data":"iVBORw0KGgpmaXh0dXJl"})
            }
            "Input.insertText" => {
                facts.value = params["text"].as_str().unwrap_or("").to_owned();
                if facts.focus_moves_on == Some("insert") {
                    facts.focus_moved = true;
                }
                if let Some(inner) = facts.focus_into_shadow_on_insert {
                    facts.inner_focused = Some(inner);
                }
                json!({})
            }
            "Input.dispatchMouseEvent" => {
                let x = params["x"].as_f64().ok_or(BrowserError::Protocol)?;
                let y = params["y"].as_f64().ok_or(BrowserError::Protocol)?;
                let (node, _) = facts.node_at_viewport(x, y).ok_or(BrowserError::Protocol)?;
                facts.mouse_targets.push(node);
                if params["type"] == "mousePressed" {
                    // A mousedown handler re-points the form (same loader), moves
                    // the target or covers it.
                    if let Some(form) = facts.mutate_after_press.take() {
                        facts.form = Some(form);
                    }
                    if let Some(position) = facts.move_target_after_press.take() {
                        facts.target_position = Some(position);
                    }
                    if std::mem::take(&mut facts.cover_after_press) {
                        facts.covered = true;
                    }
                    if std::mem::take(&mut facts.subtree_describe_fails_after_press) {
                        facts.subtree_describe_fails = true;
                    }
                }
                if facts.popup && params["type"] == "mouseReleased" && matches!(node, 2 | 20) {
                    facts.popup = false;
                    self.epoch += 1;
                    events.push(json!({"method":"Target.attachedToTarget","params":{"sessionId":"popup-session","targetInfo":{"targetId":"popup","type":"page","url":"https://example.com/popup"}}}));
                }
                json!({})
            }
            "Input.dispatchKeyEvent" => {
                if params["type"] == "keyDown" {
                    // A keydown handler moves focus, re-points the form, or the
                    // key's own submission navigates.
                    if let Some(node) = facts.mutate_after_keydown.take() {
                        facts.focused_node = (node != 0).then_some(node);
                    }
                    if let Some(form) = facts.mutate_form_after_keydown.take() {
                        facts.form = Some(form);
                    }
                    if std::mem::take(&mut facts.navigate_after_keydown) {
                        self.epoch += 1;
                        events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":"https://other.example/"}}}));
                        facts.main_url = Some("https://other.example/".to_owned());
                    }
                }
                json!({})
            }
            "Page.getNavigationHistory" => {
                if let Some(url) = facts.navigate_on_history.take() {
                    self.epoch += 1;
                    events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":url}}}));
                    facts.main_url = Some(url);
                }
                json!({"currentIndex":facts.history_index,"entries":[{"id":1},{"id":2}]})
            }
            _ => json!({}),
        };
        Ok(CdpReply {
            result,
            sequence: self.sequence,
            document_epoch: self.epoch,
            events,
        })
    }
    fn drain_events(&mut self) -> Result<Vec<Value>, BrowserError> {
        let mut facts = self
            .facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        Ok(std::mem::take(&mut facts.late_events))
    }
}

#[derive(Default)]
struct Slot {
    owner: Option<String>,
    document: Option<CdpDocument>,
}

#[derive(Clone, Default)]
struct Host {
    slot: Arc<Mutex<Slot>>,
    facts: Arc<Mutex<Facts>>,
}

impl BrowserHost for Host {
    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        let slot = self
            .slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        if slot
            .owner
            .as_deref()
            .is_some_and(|owner| owner != session_id)
        {
            return Err(BrowserError::Busy);
        }
        if let Some(document) = &slot.document {
            return document.prepare(arguments);
        }
        let origin = if arguments["action"] == "open" {
            let parsed = shacs_core::tools::parse_http_url(
                arguments["url"].as_str().ok_or(BrowserError::Protocol)?,
            )
            .map_err(|_| BrowserError::Protocol)?;
            Some(format!("{}://{}", parsed.scheme, parsed.authority))
        } else if arguments["action"] == "close" {
            None
        } else {
            return Err(BrowserError::StaleRevision);
        };
        let binding = json!({"origin":origin,"role":null,"name":null,"form":null,"focus":null,"revision":null});
        let raw = json!({"arguments":arguments,"binding":binding}).to_string();
        let mut description = binding;
        description["action"] = arguments["action"].clone();
        Ok(BrowserPreparedAction {
            description,
            digest: format!("{:x}", Sha256::digest(raw.as_bytes())),
        })
    }

    fn close_prepared(
        &self,
        context: &BrowserCallContext,
        digest: &str,
    ) -> Result<(), BrowserError> {
        if self
            .prepare(&context.effective_session_id, &json!({"action":"close"}))?
            .digest
            != digest
        {
            return Err(BrowserError::StaleRevision);
        }
        self.close(context)
    }

    fn acquire(
        &self,
        context: &BrowserCallContext,
    ) -> Result<Box<dyn BrowserSession>, BrowserError> {
        self.facts
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .contexts
            .push(context.clone());
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        if slot
            .owner
            .as_ref()
            .is_some_and(|owner| owner != &context.effective_session_id)
        {
            return Err(BrowserError::Busy);
        }
        if slot.document.is_none() {
            slot.document = Some(CdpDocument::initialize(
                Box::new(Peer {
                    facts: Arc::clone(&self.facts),
                    sequence: 0,
                    epoch: 0,
                    frames: 0,
                }),
                deadline(),
            )?);
            slot.owner = Some(context.effective_session_id.clone());
        }
        Ok(Box::new(self.clone()))
    }
}

impl BrowserSession for Host {
    fn execute_prepared(
        &self,
        context: &BrowserCallContext,
        arguments: &Value,
        digest: &str,
    ) -> Result<Option<BrowserRead>, BrowserError> {
        self.slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .document
            .as_mut()
            .ok_or(BrowserError::StaleRevision)?
            .execute_prepared(
                arguments,
                digest,
                context
                    .browser_screenshots
                    .unwrap_or(context.native_image_input_supported),
                deadline(),
            )
    }

    fn current_document(
        &self,
        _context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError> {
        self.slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .document
            .as_mut()
            .ok_or(BrowserError::DocumentChanged)?
            .current(deadline())
    }
    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError> {
        self.slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .document
            .as_mut()
            .ok_or(BrowserError::DocumentChanged)?
            .read(
                context
                    .browser_screenshots
                    .unwrap_or(context.native_image_input_supported),
                deadline(),
            )
    }
    fn act(
        &self,
        context: &BrowserCallContext,
        action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError> {
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        let document = slot
            .document
            .as_mut()
            .ok_or(BrowserError::DocumentChanged)?;
        document.act(action, deadline())?;
        document.read(
            context
                .browser_screenshots
                .unwrap_or(context.native_image_input_supported),
            deadline(),
        )
    }
    fn close(&self, _context: &BrowserCallContext) -> Result<(), BrowserError> {
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?;
        if let Some(mut document) = slot.document.take() {
            document.close(deadline());
        }
        slot.owner = None;
        Ok(())
    }
}

/// Tests that drive the transport directly never meet a paused target.
fn no_admission(
    _peer: &mut dyn DevToolsPeer,
    _event: &Value,
    _deadline: Instant,
) -> Result<Vec<Value>, BrowserError> {
    Ok(Vec::new())
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn context() -> BrowserCallContext {
    BrowserCallContext {
        effective_session_id: "fixture".to_owned(),
        permission_mode_snapshot: PermissionModeSnapshot::default(),
        permission_interactive: false,
        browser_screenshots: Some(true),
        native_image_input_supported: true,
        media_root: None,
    }
}

#[test]
fn decision_bindings_recheck_same_loader_form_and_deep_focus() -> TestResult {
    let original = json!({"owner":50,"action":"https://example.com/pay","base":"https://example.com/",
        "method":"post","submitter":2,"formaction":null,"formmethod":null,
        "formenctype":null,"formtarget":null,"formnovalidate":null});
    for (key, value) in [
        ("action", json!("https://example.com/other")),
        ("base", json!("https://other.example/")),
        ("owner", json!(51)),
        ("method", json!("get")),
        ("submitter", json!(1)),
        ("formaction", json!("https://example.com/override")),
        ("formmethod", json!("get")),
        ("formenctype", json!("text/plain")),
        ("formtarget", json!("_blank")),
        ("formnovalidate", json!("")),
        ("unchanged", Value::Null),
        ("after_hit", json!("https://example.com/after-hit")),
        ("focus", json!(1)),
    ] {
        let root = tempfile::tempdir()?;
        // The form's default submit button only matters for Enter in a text
        // input (node 1); a focused submit button (node 2) is its own submitter.
        let facts = Arc::new(Mutex::new(Facts {
            form: Some(original.clone()),
            focused_node: Some(if key == "submitter" { 1 } else { 2 }),
            child_frame: true,
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let open = json!({"action":"open","url":"https://example.com/"});
        let prepared = host.prepare(&context.effective_session_id, &open)?;
        assert!(
            facts.lock().expect("facts").commands.is_empty(),
            "prepare must not start a browser"
        );
        let session = host.acquire(&context)?;
        let read = session
            .execute_prepared(&context, &open, &prepared.digest)?
            .ok_or("open result")?;
        let action = if matches!(key, "focus" | "submitter") {
            json!({"action":"press","key":"Enter"})
        } else {
            json!({"action":"click","element":2,"revision":read.revision})
        };
        let prepared = host.prepare(&context.effective_session_id, &action)?;
        assert_eq!(prepared.description["form"]["node"], 50);
        assert_eq!(prepared.description["form"]["name"], "Order form");
        assert_eq!(prepared.description["submission"]["method"], "post");
        assert_eq!(prepared.description["submitter"]["node"], 2);
        if key == "focus" {
            assert_eq!(prepared.description["focus"]["frame"], "child");
            assert_eq!(prepared.description["focus"]["node"], 2);
        }
        {
            let mut facts = facts.lock().expect("facts");
            facts.commands.clear();
            if key == "focus" {
                facts.focused_node = Some(1);
            } else if key == "after_hit" {
                let mut form = original.clone();
                form["action"] = value;
                facts.mutate_after_hit = Some(form);
            } else if key != "unchanged" {
                facts.form.as_mut().ok_or("form")?[key] = value;
            }
        }
        let executed = session.execute_prepared(&context, &action, &prepared.digest);
        if key == "unchanged" {
            assert!(executed.is_ok(), "{executed:?}");
        } else if key == "after_hit" {
            // The form changes during the final hit test, the last request before
            // the press: the press goes out, the recheck after it catches the
            // change, and the browser ends without a release.
            assert_eq!(executed, Err(BrowserError::DocumentChanged), "{key}");
            let inputs: Vec<String> = facts
                .lock()
                .expect("facts")
                .commands
                .iter()
                .filter(|(method, _)| method.starts_with("Input."))
                .map(|(_, params)| params["type"].to_string())
                .collect();
            assert_eq!(inputs, ["\"mousePressed\""], "{key}");
            assert_eq!(host.status()?.state, BrowserState::Failed, "{key}");
        } else {
            assert_eq!(executed, Err(BrowserError::StaleRevision), "{key}");
            assert!(
                facts
                    .lock()
                    .expect("facts")
                    .commands
                    .iter()
                    .all(|(method, _)| !method.starts_with("Input.")),
                "{key}"
            );
            assert!(
                host.prepare(&context.effective_session_id, &action)
                    .is_err(),
                "binding must be invalidated: {key}"
            );
            // Recovery: the invalidated act binding does not block a fresh
            // observation, and a fresh decision on the new numbers executes.
            let look = json!({"action":"look"});
            let prepared = host.prepare(&context.effective_session_id, &look)?;
            let read = session
                .execute_prepared(&context, &look, &prepared.digest)?
                .ok_or("look after rejection")?;
            let retry = if matches!(key, "focus" | "submitter") {
                action.clone()
            } else {
                json!({"action":"click","element":2,"revision":read.revision})
            };
            let prepared = host.prepare(&context.effective_session_id, &retry)?;
            let executed = session.execute_prepared(&context, &retry, &prepared.digest);
            assert!(executed.is_ok(), "recovery {key}: {executed:?}");
        }
        host.shutdown()?;
    }
    Ok(())
}

fn order_form() -> Value {
    json!({"owner":50,"action":"https://example.com/pay","base":"https://example.com/",
        "method":"post","submitter":2,"formaction":null,"formmethod":null,
        "formenctype":null,"formtarget":null,"formnovalidate":null})
}

fn opened(
    host: &shacs_cli::browser::ManagedBrowserHost,
    context: &BrowserCallContext,
) -> Result<(Box<dyn BrowserSession>, BrowserRead), Box<dyn Error>> {
    let open = json!({"action":"open","url":"https://example.com/"});
    let prepared = host.prepare(&context.effective_session_id, &open)?;
    let session = host.acquire(context)?;
    let read = session
        .execute_prepared(context, &open, &prepared.digest)?
        .ok_or("open result")?;
    Ok((session, read))
}

#[test]
fn input_mismatch_after_the_first_event_ends_the_browser_without_committing() -> TestResult {
    for scenario in ["click", "click-child-frame", "enter"] {
        let root = tempfile::tempdir()?;
        let mut changed = order_form();
        changed["action"] = json!("https://example.com/changed-by-handler");
        let facts = Arc::new(Mutex::new(Facts {
            form: Some(order_form()),
            focused_node: Some(2),
            child_frame: scenario == "click-child-frame",
            mutate_after_press: (scenario != "enter").then(|| changed.clone()),
            mutate_after_keydown: (scenario == "enter").then_some(1),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, read) = opened(&host, &context)?;
        let action = if scenario == "enter" {
            json!({"action":"press","key":"Enter"})
        } else {
            json!({"action":"click","element":2,"revision":read.revision})
        };
        let prepared = host.prepare(&context.effective_session_id, &action)?;
        assert_eq!(
            prepared.description["submission"]["url"],
            "https://example.com/pay"
        );
        {
            let mut facts = facts.lock().expect("facts");
            facts.commands.clear();
            facts.mouse_targets.clear();
        }
        let executed = session.execute_prepared(&context, &action, &prepared.digest);
        assert_eq!(executed, Err(BrowserError::DocumentChanged), "{scenario}");
        {
            let facts = facts.lock().expect("facts");
            // Only the event already sent: nothing completes or redirects it.
            let inputs: Vec<String> = facts
                .commands
                .iter()
                .filter(|(method, _)| method.starts_with("Input."))
                .map(|(method, params)| format!("{method}:{}", params["type"]))
                .collect();
            let expected = if scenario == "enter" {
                vec!["Input.dispatchKeyEvent:\"keyDown\""]
            } else {
                vec!["Input.dispatchMouseEvent:\"mousePressed\""]
            };
            assert_eq!(inputs, expected, "{scenario}");
            if scenario != "enter" {
                assert_eq!(facts.mouse_targets, vec![2], "{scenario}");
            }
        }
        // The generation is gone: no binding, no observation, a failed state.
        assert_eq!(host.status()?.state, BrowserState::Failed, "{scenario}");
        let look = json!({"action":"look"});
        assert!(host.prepare(&context.effective_session_id, &look).is_err());
        assert!(session.execute_prepared(&context, &look, "any").is_err());
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn popup_attach_is_admitted_while_the_click_reply_is_pending() -> TestResult {
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        popup_paused: true,
        popup_blank_polls: 1,
        popup_loading_polls: 1,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let click = json!({"action":"click","element":2,"revision":read.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    facts.lock().expect("facts").commands.clear();
    let popup = session
        .execute_prepared(&context, &click, &prepared.digest)?
        .ok_or("popup result")?;
    assert_eq!(popup.document.target_id, "popup");
    assert_eq!(popup.document.url, "https://example.com/popup");
    assert_ne!(popup.revision, read.revision);
    {
        let facts = facts.lock().expect("facts");
        assert!(facts.popup_resumed);
        let position = |method: &str, key: &str, value: &str| {
            facts
                .commands
                .iter()
                .position(|(name, params)| {
                    name == method && (key.is_empty() || params[key] == value)
                })
                .ok_or_else(|| format!("{method} {key}={value}: {:?}", facts.commands))
        };
        // The boundary and the resume go out before the input's reply is
        // accepted; the opener closes after the popup is current.
        let order = [
            position("Input.dispatchMouseEvent", "type", "mouseReleased")?,
            position(
                "Browser.setDownloadBehavior",
                "browserContextId",
                "context-1",
            )?,
            position("Page.enable", "", "")?,
            position("DOM.enable", "", "")?,
            position("Page.setInterceptFileChooserDialog", "", "")?,
            position("Runtime.runIfWaitingForDebugger", "", "")?,
            position("Target.closeTarget", "targetId", "target")?,
        ];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
        let ready = facts
            .commands
            .iter()
            .filter(|(method, params)| {
                method == "Runtime.evaluate" && params["expression"] == "document.readyState"
            })
            .count();
        assert!(
            ready >= 2,
            "the blank and loading popup is read only once complete"
        );
    }
    // The popup's own verified read is the observation subject now.
    let look = host.prepare(&context.effective_session_id, &json!({"action":"look"}))?;
    assert_eq!(look.description["target"], "popup");
    assert_eq!(look.description["origin"], "https://example.com");
    assert_eq!(
        host.prepare(
            &context.effective_session_id,
            &json!({"action":"click","element":2,"revision":read.revision}),
        )
        .err(),
        Some(BrowserError::StaleRevision)
    );
    host.shutdown()?;
    Ok(())
}

#[test]
fn observation_is_refused_when_the_document_changed_after_the_decision() -> TestResult {
    for late in [true, false] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts::default()));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, _) = opened(&host, &context)?;
        let look = json!({"action":"look"});
        let prepared = host.prepare(&context.effective_session_id, &look)?;
        assert_eq!(prepared.description["origin"], "https://example.com");
        {
            let mut facts = facts.lock().expect("facts");
            facts.commands.clear();
            if late {
                // The navigation reached the host before this call drained events.
                facts.main_url = Some("https://other.example/".to_owned());
                facts.late_events.push(json!({"method":"Page.frameNavigated","params":{"frame":{"id":"frame","loaderId":"navigated","url":"https://other.example/"}}}));
            } else {
                // The navigation commits while the live document is checked.
                facts.navigate_on_frame_tree = Some("https://other.example/".to_owned());
            }
        }
        assert_eq!(
            session.execute_prepared(&context, &look, &prepared.digest),
            Err(BrowserError::StaleRevision),
            "{late}"
        );
        assert!(
            facts
                .lock()
                .expect("facts")
                .commands
                .iter()
                .all(|(method, _)| method != "Accessibility.getFullAXTree"),
            "{late}: the other document must not be read under the old decision"
        );
        // A fresh decision describes the document actually there.
        let fresh = host.prepare(&context.effective_session_id, &look)?;
        assert_eq!(fresh.description["origin"], "https://other.example");
        let read = session
            .execute_prepared(&context, &look, &fresh.digest)?
            .ok_or("look")?;
        assert_eq!(read.document.url, "https://other.example/");
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn observation_is_refused_when_the_document_changes_during_execution() -> TestResult {
    // The entry check passes; the navigation lands during the action itself.
    for (action, frames_before, step) in [
        (
            json!({"action":"scroll","direction":"down"}),
            1,
            "window.scrollBy",
        ),
        (
            json!({"action":"wait","text":"never","timeoutMs":400}),
            2,
            ".includes(",
        ),
        (json!({"action":"back"}), 1, "Page.getNavigationHistory"),
    ] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts::default()));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, _) = opened(&host, &context)?;
        let prepared = host.prepare(&context.effective_session_id, &action)?;
        assert_eq!(prepared.description["origin"], "https://example.com");
        {
            let mut facts = facts.lock().expect("facts");
            facts.commands.clear();
            facts.navigate_after_frames = Some(frames_before);
        }
        assert_eq!(
            session.execute_prepared(&context, &action, &prepared.digest),
            Err(BrowserError::StaleRevision),
            "{action}"
        );
        let facts = facts.lock().expect("facts");
        let steps = facts
            .commands
            .iter()
            .filter(|(method, params)| {
                method == step
                    || params["expression"]
                        .as_str()
                        .is_some_and(|e| e.contains(step))
            })
            .count();
        // `wait` ran its first iteration on the approved document; `scroll` and
        // `back` never reached their step, and nothing was read afterwards.
        assert_eq!(steps, usize::from(action["action"] == "wait"), "{action}");
        assert!(
            facts
                .commands
                .iter()
                .all(|(method, _)| method != "Accessibility.getFullAXTree"),
            "{action}: the other document must not be read under the old decision"
        );
        drop(facts);
        let fresh = host.prepare(&context.effective_session_id, &json!({"action":"look"}))?;
        assert_eq!(fresh.description["origin"], "https://other.example");
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn popups_attached_while_switching_keep_creation_order() -> TestResult {
    // The click's reply carries popups A and B; C attaches while the original
    // tab is being closed for A. Creation order is A, B, C and C must survive.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        popup_batch: vec!["popup-a", "popup-b"],
        popup_on_close: Some("popup-c"),
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let click = json!({"action":"click","element":2,"revision":read.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    let result = session
        .execute_prepared(&context, &click, &prepared.digest)?
        .ok_or("click result")?;
    assert_eq!(result.document.target_id, "popup-c");
    assert_eq!(result.document.url, "https://example.com/popup-c");
    let closed: Vec<String> = facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .filter(|(method, _)| method == "Target.closeTarget")
        .filter_map(|(_, params)| params["targetId"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(closed, ["target", "popup-a", "popup-b"]);
    let look = host.prepare(&context.effective_session_id, &json!({"action":"look"}))?;
    assert_eq!(look.description["target"], "popup-c");
    host.shutdown()?;
    Ok(())
}

#[test]
fn back_revalidates_its_start_document_before_dispatch_and_read() -> TestResult {
    // (history index, change during the history request, change before the
    // final read, expected outcome)
    for (index, during_history, before_read, expected) in [
        (1, true, false, Err(BrowserError::StaleRevision)),
        (0, true, false, Err(BrowserError::StaleRevision)),
        (0, false, true, Err(BrowserError::StaleRevision)),
        (1, false, false, Ok(())),
    ] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            history_index: index,
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, _) = opened(&host, &context)?;
        let back = json!({"action":"back"});
        let prepared = host.prepare(&context.effective_session_id, &back)?;
        {
            let mut facts = facts.lock().expect("facts");
            facts.commands.clear();
            if during_history {
                facts.navigate_on_history = Some("https://other.example/".to_owned());
            }
            if before_read {
                // Entry check and post-history check pass; the read's own
                // frame check meets the changed document.
                facts.navigate_after_frames = Some(2);
            }
        }
        let executed = session.execute_prepared(&context, &back, &prepared.digest);
        assert_eq!(
            executed.as_ref().map(|_| ()),
            expected.as_ref().map(|_| ()),
            "{index} {during_history} {before_read}"
        );
        let facts = facts.lock().expect("facts");
        let dispatched = facts
            .commands
            .iter()
            .any(|(method, _)| method == "Page.navigateToHistoryEntry");
        assert_eq!(
            dispatched,
            expected.is_ok(),
            "{index} {during_history} {before_read}"
        );
        assert_eq!(
            facts
                .commands
                .iter()
                .any(|(method, _)| method == "Accessibility.getFullAXTree"),
            expected.is_ok(),
            "{index} {during_history} {before_read}: no read under the old decision"
        );
        drop(facts);
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn fatal_errors_survive_validation_and_cleanup() -> TestResult {
    // A chooser opens while the click's document is confirmed; a popup in the
    // same batch is moot. The error stays fatal and the generation ends.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts::default()));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let click = json!({"action":"click","element":2,"revision":read.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    {
        let mut facts = facts.lock().expect("facts");
        facts.commands.clear();
        facts.fatal_on_frame_tree = Some(vec![
            json!({"method":"Page.fileChooserOpened","params":{}}),
            json!({"method":"Target.attachedToTarget","params":{"sessionId":"late-session","waitingForDebugger":false,
                "targetInfo":{"targetId":"late","type":"page","url":"about:blank","browserContextId":"context-1"}}}),
        ]);
    }
    assert_eq!(
        session.execute_prepared(&context, &click, &prepared.digest),
        Err(BrowserError::FileTransferDenied)
    );
    assert_eq!(host.status()?.state, BrowserState::Failed);
    assert!(facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .all(|(method, _)| !method.starts_with("Input.")));
    host.shutdown()?;
    // A stale description whose cleanup sees the document close reports the
    // close, not the stale description.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        form: Some(order_form()),
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let (session, _) = opened(&host, &context)?;
    let look = json!({"action":"look"});
    let prepared = host.prepare(&context.effective_session_id, &look)?;
    {
        let mut facts = facts.lock().expect("facts");
        facts.describe_fails_object = Some("50".to_owned());
        facts.release_fatal_object = Some("50".to_owned());
    }
    assert_eq!(
        session.execute_prepared(&context, &look, &prepared.digest),
        Err(BrowserError::FileTransferDenied)
    );
    assert_eq!(host.status()?.state, BrowserState::Failed);
    host.shutdown()?;
    Ok(())
}

#[test]
fn type_ends_when_focus_leaves_the_approved_element() -> TestResult {
    for moved_on in ["focus", "insert"] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            form: Some(order_form()),
            focus_moves_on: Some(moved_on),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, read) = opened(&host, &context)?;
        let typed = json!({"action":"type","element":1,"revision":read.revision,"text":"hello","submit":true});
        let prepared = host.prepare(&context.effective_session_id, &typed)?;
        facts.lock().expect("facts").commands.clear();
        assert_eq!(
            session.execute_prepared(&context, &typed, &prepared.digest),
            Err(BrowserError::DocumentChanged),
            "{moved_on}"
        );
        let facts = facts.lock().expect("facts");
        let inputs: Vec<&str> = facts
            .commands
            .iter()
            .filter(|(method, _)| method.starts_with("Input."))
            .map(|(method, _)| method.as_str())
            .collect();
        // Focus moved on focus: nothing typed. Moved on input: the text went to
        // the approved element, Enter never went out.
        let expected: Vec<&str> = if moved_on == "focus" {
            Vec::new()
        } else {
            vec!["Input.insertText"]
        };
        assert_eq!(inputs, expected, "{moved_on}");
        drop(facts);
        assert_eq!(host.status()?.state, BrowserState::Failed);
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn numbered_scroll_is_bound_to_the_decided_document() -> TestResult {
    for (on_resolve, on_scroll) in [(true, false), (false, true)] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts::default()));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, read) = opened(&host, &context)?;
        let scroll =
            json!({"action":"scroll","direction":"down","element":2,"revision":read.revision});
        let prepared = host.prepare(&context.effective_session_id, &scroll)?;
        {
            let mut facts = facts.lock().expect("facts");
            facts.commands.clear();
            facts.navigate_on_resolve = on_resolve;
            facts.navigate_on_scroll = on_scroll;
        }
        assert_eq!(
            session.execute_prepared(&context, &scroll, &prepared.digest),
            Err(BrowserError::StaleRevision),
            "{on_resolve} {on_scroll}"
        );
        let facts = facts.lock().expect("facts");
        let scrolled = facts.commands.iter().any(|(method, params)| {
            method == "Runtime.callFunctionOn"
                && params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|f| f.contains("scrollBy"))
        });
        assert_eq!(
            scrolled, on_scroll,
            "the scroll goes out only on the decided document"
        );
        assert!(
            facts
                .commands
                .iter()
                .all(|(method, _)| method != "Accessibility.getFullAXTree"),
            "the other document is not read under the old decision"
        );
        drop(facts);
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn click_hit_test_is_adjacent_to_the_press_and_the_release_is_re_hit_tested() -> TestResult {
    // The target moves during the binding recheck: the hit test after it
    // finds the new place and both events land on the target.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        move_target_after_partial_ax: Some((1, [400.0, 300.0])),
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let click = json!({"action":"click","element":2,"revision":read.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    facts.lock().expect("facts").mouse_targets.clear();
    session
        .execute_prepared(&context, &click, &prepared.digest)?
        .ok_or("click result")?;
    assert_eq!(facts.lock().expect("facts").mouse_targets, vec![2, 2]);
    host.shutdown()?;
    // The target moves or is covered after the press: no release goes out and
    // the browser ends.
    for covered in [false, true] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            move_target_after_press: (!covered).then_some([400.0, 300.0]),
            cover_after_press: covered,
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let (session, read) = opened(&host, &context)?;
        let click = json!({"action":"click","element":2,"revision":read.revision});
        let prepared = host.prepare(&context.effective_session_id, &click)?;
        facts.lock().expect("facts").mouse_targets.clear();
        assert_eq!(
            session.execute_prepared(&context, &click, &prepared.digest),
            Err(BrowserError::DocumentChanged),
            "{covered}"
        );
        let facts = facts.lock().expect("facts");
        assert_eq!(facts.mouse_targets, vec![2], "{covered}");
        assert!(facts
            .commands
            .iter()
            .all(|(method, params)| method != "Input.dispatchMouseEvent"
                || params["type"] != "mouseReleased"));
        drop(facts);
        assert_eq!(host.status()?.state, BrowserState::Failed);
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn focus_moving_keys_complete_while_committing_keys_stay_strict() -> TestResult {
    // (key, focus after keydown, form changes after keydown, expected). Node 200
    // is an iframe element whose document the tree cannot resolve: a focus
    // there is outside the verified document.
    for (key, new_focus, form_changes, expected) in [
        ("Tab", Some(3), false, Ok(())),
        ("Shift+Tab", Some(3), false, Ok(())),
        (
            "ArrowDown",
            Some(3),
            false,
            Err(BrowserError::DocumentChanged),
        ),
        ("Escape", Some(3), false, Err(BrowserError::DocumentChanged)),
        ("Tab", Some(200), false, Err(BrowserError::DocumentChanged)),
        ("Tab", Some(210), false, Err(BrowserError::DocumentChanged)),
        ("Tab", Some(220), false, Err(BrowserError::DocumentChanged)),
        ("Tab", Some(0), false, Err(BrowserError::DocumentChanged)),
        ("Tab", Some(3), true, Err(BrowserError::DocumentChanged)),
        ("Enter", Some(3), false, Err(BrowserError::DocumentChanged)),
    ] {
        let root = tempfile::tempdir()?;
        let mut changed = order_form();
        changed["action"] = json!("https://example.com/changed-by-keydown");
        let facts = Arc::new(Mutex::new(Facts {
            form: Some(order_form()),
            focused_node: Some(1),
            mutate_after_keydown: new_focus,
            mutate_form_after_keydown: form_changes.then_some(changed),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, _) = opened(&host, &context)?;
        let press = json!({"action":"press","key":key});
        let prepared = host.prepare(&context.effective_session_id, &press)?;
        assert_eq!(prepared.description["focus"]["node"], 1);
        facts.lock().expect("facts").commands.clear();
        let executed = session.execute_prepared(&context, &press, &prepared.digest);
        assert_eq!(
            executed.as_ref().map(|_| ()),
            expected.as_ref().map(|_| ()),
            "{key} {new_focus:?} {form_changes}"
        );
        let facts = facts.lock().expect("facts");
        let keys: Vec<&str> = facts
            .commands
            .iter()
            .filter(|(method, _)| method == "Input.dispatchKeyEvent")
            .filter_map(|(_, params)| params["type"].as_str())
            .collect();
        // The key sequence completes for a deliberate focus move; it stops
        // after keydown when something else changed.
        let expected_keys = if expected.is_ok() {
            vec!["keyDown", "keyUp"]
        } else {
            vec!["keyDown"]
        };
        assert_eq!(keys, expected_keys, "{key} {new_focus:?} {form_changes}");
        drop(facts);
        host.shutdown()?;
    }
    // Enter whose submission already navigated: no keyUp into the new document,
    // and the result shows where the page went.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        form: Some(order_form()),
        focused_node: Some(1),
        navigate_after_keydown: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, _) = opened(&host, &context)?;
    let press = json!({"action":"press","key":"Enter"});
    let prepared = host.prepare(&context.effective_session_id, &press)?;
    facts.lock().expect("facts").commands.clear();
    let read = session
        .execute_prepared(&context, &press, &prepared.digest)?
        .ok_or("press result")?;
    assert_eq!(read.document.url, "https://other.example/");
    let keys: Vec<String> = facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .filter(|(method, _)| method == "Input.dispatchKeyEvent")
        .filter_map(|(_, params)| params["type"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(keys, ["keyDown"]);
    host.shutdown()?;
    Ok(())
}

#[test]
fn final_checks_refuse_input_after_a_document_change_in_their_replies() -> TestResult {
    // After `DOM.focus` the typed element is queried three times: the first
    // focus check, the recheck's own query, and the guard right before the
    // text. A navigation or a frame removal in that guard's reply means the
    // text is not sent; the focus already went out, so the browser ends.
    for (navigation, detach) in [(true, false), (false, true)] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            navigate_on_focus_check: navigation.then_some(2),
            detach_on_focus_check: detach.then_some(2),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, read) = opened(&host, &context)?;
        let typed = json!({"action":"type","element":1,"revision":read.revision,"text":"hello"});
        let prepared = host.prepare(&context.effective_session_id, &typed)?;
        facts.lock().expect("facts").commands.clear();
        let executed = session.execute_prepared(&context, &typed, &prepared.digest);
        let facts = facts.lock().expect("facts");
        let sequence: Vec<String> = facts
            .commands
            .iter()
            .map(|(method, params)| format!("{method}({})", params["backendNodeId"]))
            .collect();
        assert_eq!(
            executed,
            Err(BrowserError::DocumentChanged),
            "{navigation} {detach}: {sequence:?}"
        );
        assert!(
            facts
                .commands
                .iter()
                .all(|(method, _)| method != "Input.insertText"),
            "{navigation} {detach}"
        );
        assert!(facts
            .commands
            .iter()
            .any(|(method, _)| method == "DOM.focus"));
        drop(facts);
        assert_eq!(host.status()?.state, BrowserState::Failed);
        host.shutdown()?;
    }
    // Descendant hit: the subtree is read before the location lookup, and a
    // navigation in that reply leaves the press unsent with the browser kept.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        descendant: true,
        navigate_on_subtree_describe: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let click = json!({"action":"click","element":2,"revision":read.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    facts.lock().expect("facts").commands.clear();
    assert_eq!(
        session.execute_prepared(&context, &click, &prepared.digest),
        Err(BrowserError::StaleRevision)
    );
    {
        let facts = facts.lock().expect("facts");
        assert!(facts
            .commands
            .iter()
            .all(|(method, _)| !method.starts_with("Input.")));
        let describe = facts
            .commands
            .iter()
            .position(|(method, params)| method == "DOM.describeNode" && params["depth"] == -1)
            .ok_or("subtree description")?;
        let lookup = facts
            .commands
            .iter()
            .position(|(method, _)| method == "DOM.getNodeForLocation")
            .ok_or("location lookup")?;
        assert!(
            describe < lookup,
            "the subtree is read before the location lookup"
        );
    }
    assert_eq!(host.status()?.state, BrowserState::Running);
    host.shutdown()?;
    Ok(())
}

#[test]
fn post_press_check_errors_end_the_browser_without_a_release() -> TestResult {
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        subtree_describe_fails_after_press: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let click = json!({"action":"click","element":2,"revision":read.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    facts.lock().expect("facts").mouse_targets.clear();
    assert_eq!(
        session.execute_prepared(&context, &click, &prepared.digest),
        Err(BrowserError::DocumentChanged)
    );
    assert_eq!(facts.lock().expect("facts").mouse_targets, vec![2]);
    assert_eq!(host.status()?.state, BrowserState::Failed);
    host.shutdown()?;
    Ok(())
}

#[test]
fn typing_into_a_closed_shadow_root_input_succeeds() -> TestResult {
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        closed_shadow_host: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let typed = json!({"action":"type","element":1,"revision":read.revision,"text":"hello"});
    let prepared = host.prepare(&context.effective_session_id, &typed)?;
    session
        .execute_prepared(&context, &typed, &prepared.digest)?
        .ok_or("type result")?;
    assert_eq!(facts.lock().expect("facts").value, "hello");
    host.shutdown()?;
    Ok(())
}

#[test]
fn frame_removal_in_the_final_hit_test_blocks_the_press_and_the_release() -> TestResult {
    // Lookup 0 is the hit before the press, lookup 1 the re-hit before the
    // release. A child frame removed in either reply counts as a change.
    for lookup in [0, 1] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            child_frame: true,
            detach_on_lookup: Some(lookup),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, read) = opened(&host, &context)?;
        let click = json!({"action":"click","element":2,"revision":read.revision});
        let prepared = host.prepare(&context.effective_session_id, &click)?;
        {
            let mut facts = facts.lock().expect("facts");
            facts.location_lookups = 0;
            facts.mouse_targets.clear();
        }
        let executed = session.execute_prepared(&context, &click, &prepared.digest);
        let facts_guard = facts.lock().expect("facts");
        if lookup == 0 {
            assert_eq!(executed, Err(BrowserError::StaleRevision));
            assert!(
                facts_guard.mouse_targets.is_empty(),
                "no press after the removal"
            );
        } else {
            assert_eq!(executed, Err(BrowserError::DocumentChanged));
            assert_eq!(
                facts_guard.mouse_targets,
                vec![2],
                "pressed, never released"
            );
        }
        drop(facts_guard);
        let expected_state = if lookup == 0 {
            BrowserState::Running
        } else {
            BrowserState::Failed
        };
        assert_eq!(host.status()?.state, expected_state, "{lookup}");
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn focus_predicate_is_the_last_request_before_each_input() -> TestResult {
    // A successful type with Enter: every text and key event is immediately
    // preceded by the accessibility focus query on the approved element, with
    // no other request in between.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        form: Some(order_form()),
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let typed =
        json!({"action":"type","element":1,"revision":read.revision,"text":"hello","submit":true});
    let prepared = host.prepare(&context.effective_session_id, &typed)?;
    facts.lock().expect("facts").commands.clear();
    session
        .execute_prepared(&context, &typed, &prepared.digest)?
        .ok_or("type result")?;
    let facts = facts.lock().expect("facts");
    let inputs: Vec<usize> = facts
        .commands
        .iter()
        .enumerate()
        .filter(|(_, (method, _))| method.starts_with("Input."))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(inputs.len(), 3, "insertText, keyDown, keyUp");
    for index in inputs {
        let (before, params) = &facts.commands[index - 1];
        assert_eq!(before, "Accessibility.getPartialAXTree", "{index}");
        assert_eq!(params["backendNodeId"], 1, "{index}");
        assert_eq!(params["fetchRelatives"], false, "{index}");
    }
    Ok(())
}

#[test]
fn retained_active_element_binds_press_on_an_unfocused_page() -> TestResult {
    // The window is not active, the document still has its active element:
    // Enter is bound to that input and its form, and the focus predicate
    // guards the key.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        form: Some(order_form()),
        focused_node: Some(1),
        page_unfocused: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, _) = opened(&host, &context)?;
    let press = json!({"action":"press","key":"Enter"});
    let prepared = host.prepare(&context.effective_session_id, &press)?;
    assert_eq!(prepared.description["focus"]["node"], 1);
    assert_eq!(prepared.description["role"], "textbox");
    assert_eq!(prepared.description["form"]["node"], 50);
    assert_eq!(
        prepared.description["submission"]["url"],
        "https://example.com/pay"
    );
    facts.lock().expect("facts").commands.clear();
    session
        .execute_prepared(&context, &press, &prepared.digest)?
        .ok_or("press result")?;
    {
        let facts = facts.lock().expect("facts");
        let keys: Vec<usize> = facts
            .commands
            .iter()
            .enumerate()
            .filter(|(_, (method, _))| method == "Input.dispatchKeyEvent")
            .map(|(index, _)| index)
            .collect();
        assert_eq!(keys.len(), 2);
        for index in keys {
            let (before, params) = &facts.commands[index - 1];
            assert_eq!(before, "Accessibility.getPartialAXTree", "{index}");
            assert_eq!(params["backendNodeId"], 1, "{index}");
            assert_eq!(params["fetchRelatives"], false, "{index}");
        }
    }
    host.shutdown()?;
    // The retained recipient is a frame owner over a document the binding may
    // not describe: no press is prepared, nothing is sent.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        focused_node: Some(210),
        page_unfocused: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let (_, _) = opened(&host, &context)?;
    facts.lock().expect("facts").commands.clear();
    assert_eq!(
        host.prepare(&context.effective_session_id, &press).err(),
        Some(BrowserError::StaleRevision)
    );
    assert!(facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .all(|(method, _)| !method.starts_with("Input.")));
    host.shutdown()?;
    // No active element beyond the body: the press stays unbound, as before.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        page_unfocused: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let (session, _) = opened(&host, &context)?;
    let escape = json!({"action":"press","key":"Escape"});
    let prepared = host.prepare(&context.effective_session_id, &escape)?;
    assert!(prepared.description["focus"].is_null());
    session
        .execute_prepared(&context, &escape, &prepared.digest)?
        .ok_or("press result")?;
    host.shutdown()?;
    Ok(())
}

#[test]
fn shadow_host_guard_requires_the_deepest_recipient() -> TestResult {
    // The numbered element is a host with a closed shadow root. After the
    // text, an input handler focuses an inner control: one that was described
    // (and could have been moved out), or one added to a root that was empty
    // when described. Blink's focused element is the inner control either
    // way, so the host's `focused` property is gone and Enter is not sent.
    for (inner, empty_when_described) in [(300, false), (301, true)] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            form: Some(order_form()),
            shadow_host: true,
            shadow_host_empty: empty_when_described,
            focus_into_shadow_on_insert: Some(inner),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, read) = opened(&host, &context)?;
        let typed = json!({"action":"type","element":1,"revision":read.revision,"text":"hello","submit":true});
        let prepared = host.prepare(&context.effective_session_id, &typed)?;
        facts.lock().expect("facts").commands.clear();
        assert_eq!(
            session.execute_prepared(&context, &typed, &prepared.digest),
            Err(BrowserError::DocumentChanged),
            "{inner}"
        );
        {
            let facts = facts.lock().expect("facts");
            let inputs: Vec<&str> = facts
                .commands
                .iter()
                .filter(|(method, _)| method.starts_with("Input."))
                .map(|(method, _)| method.as_str())
                .collect();
            assert_eq!(inputs, ["Input.insertText"], "{inner}");
            // The guard asked the accessibility tree about the host itself.
            assert!(facts.commands.iter().any(|(method, params)| {
                method == "Accessibility.getPartialAXTree"
                    && params["backendNodeId"] == 1
                    && params["fetchRelatives"] == false
            }));
        }
        assert_eq!(host.status()?.state, BrowserState::Failed, "{inner}");
        host.shutdown()?;
    }
    // The same host without a focus move stays the focused element: text and
    // Enter both go out.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        form: Some(order_form()),
        shadow_host: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let typed =
        json!({"action":"type","element":1,"revision":read.revision,"text":"hello","submit":true});
    let prepared = host.prepare(&context.effective_session_id, &typed)?;
    facts.lock().expect("facts").commands.clear();
    session
        .execute_prepared(&context, &typed, &prepared.digest)?
        .ok_or("type result")?;
    let inputs = facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .filter(|(method, _)| method.starts_with("Input."))
        .count();
    assert_eq!(inputs, 3);
    host.shutdown()?;
    Ok(())
}

#[test]
fn numbered_frame_owner_is_never_an_input_target() -> TestResult {
    // An editable <iframe role="textbox"> is numbered like a textbox. Typing
    // into it would make it the parent document's focused element (so the AX
    // guard would pass) while Enter went to the child frame, so it is refused
    // before any input, and so is clicking it.
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts {
        form: Some(order_form()),
        extra_nodes: 1,
        numbered_frame_owner: true,
        ..Facts::default()
    }));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, read) = opened(&host, &context)?;
    let owner = read
        .elements
        .iter()
        .find(|element| element.name == "element 9")
        .ok_or("numbered frame owner")?;
    assert_eq!(owner.role, "textbox");
    let owner_element = owner.element;
    let look = json!({"action":"look"});
    for kind in ["type", "click"] {
        // A refusal consumes the numbers, so each attempt starts from a look.
        let prepared = host.prepare(&context.effective_session_id, &look)?;
        let read = session
            .execute_prepared(&context, &look, &prepared.digest)?
            .ok_or("look")?;
        let action = if kind == "type" {
            json!({"action":"type","element":owner_element,"revision":read.revision,"text":"hello","submit":true})
        } else {
            json!({"action":"click","element":owner_element,"revision":read.revision})
        };
        let prepared = host.prepare(&context.effective_session_id, &action)?;
        facts.lock().expect("facts").commands.clear();
        assert_eq!(
            session.execute_prepared(&context, &action, &prepared.digest),
            Err(BrowserError::InputDenied),
            "{action}"
        );
        let facts = facts.lock().expect("facts");
        assert!(
            facts
                .commands
                .iter()
                .all(|(method, _)| !method.starts_with("Input.") && method != "DOM.focus"),
            "{action}"
        );
        assert_eq!(facts.page_focused_frame, None, "{action}");
    }
    // The browser stays usable: a refusal consumes the numbers (look again),
    // and ordinary controls still work on the fresh ones.
    assert_eq!(host.status()?.state, BrowserState::Running);
    let look = json!({"action":"look"});
    let prepared = host.prepare(&context.effective_session_id, &look)?;
    let read = session
        .execute_prepared(&context, &look, &prepared.digest)?
        .ok_or("look after refusal")?;
    let typed = json!({"action":"type","element":1,"revision":read.revision,"text":"hello"});
    let prepared = host.prepare(&context.effective_session_id, &typed)?;
    session
        .execute_prepared(&context, &typed, &prepared.digest)?
        .ok_or("type result")?;
    host.shutdown()?;
    Ok(())
}

#[test]
fn popup_is_not_observed_under_the_opener_decision() -> TestResult {
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts::default()));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, _) = opened(&host, &context)?;
    let look = json!({"action":"look"});
    let prepared = host.prepare(&context.effective_session_id, &look)?;
    {
        let mut facts = facts.lock().expect("facts");
        facts.commands.clear();
        // A popup the page opened by itself attaches, still blank, before this
        // call drains events.
        facts.late_events.push(json!({"method":"Target.attachedToTarget","params":{"sessionId":"popup-session",
            "waitingForDebugger":false,"targetInfo":{"targetId":"popup","type":"page","url":"about:blank","browserContextId":"context-1"}}}));
    }
    assert_eq!(
        session.execute_prepared(&context, &look, &prepared.digest),
        Err(BrowserError::StaleRevision)
    );
    assert!(facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .all(|(method, _)| method != "Accessibility.getFullAXTree"));
    // Until a read verifies the popup's own document it is nobody's subject;
    // the opener's identity does not carry over.
    assert_eq!(
        host.prepare(&context.effective_session_id, &look).err(),
        Some(BrowserError::DocumentChanged)
    );
    let open = json!({"action":"open","url":"https://example.com/popup"});
    let prepared = host.prepare(&context.effective_session_id, &open)?;
    let read = session
        .execute_prepared(&context, &open, &prepared.digest)?
        .ok_or("open")?;
    assert_eq!(read.document.target_id, "popup");
    let prepared = host.prepare(&context.effective_session_id, &look)?;
    assert_eq!(prepared.description["target"], "popup");
    host.shutdown()?;
    Ok(())
}

#[test]
fn enter_on_focused_submit_button_binds_that_submitter() -> TestResult {
    let mut form = order_form();
    form["submit_buttons"] = json!([2, 3]);
    form["overrides"] = json!({"3":{"formaction":"https://example.com/pay-b","formmethod":"post"}});
    for (focused, submitter, url) in [
        (3, 3, "https://example.com/pay-b"),
        (2, 2, "https://example.com/pay"),
        (1, 2, "https://example.com/pay"),
    ] {
        let root = tempfile::tempdir()?;
        let facts = Arc::new(Mutex::new(Facts {
            form: Some(form.clone()),
            focused_node: Some(focused),
            ..Facts::default()
        }));
        let peer = Peer {
            facts: Arc::clone(&facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
        let context = context();
        let (session, _) = opened(&host, &context)?;
        let press = json!({"action":"press","key":"Enter"});
        let prepared = host.prepare(&context.effective_session_id, &press)?;
        assert_eq!(prepared.description["focus"]["node"], focused);
        assert_eq!(
            prepared.description["submitter"]["node"], submitter,
            "{focused}"
        );
        assert_eq!(prepared.description["submission"]["url"], url, "{focused}");
        assert_eq!(
            prepared.description["role"],
            if focused == 1 { "textbox" } else { "button" }
        );
        session
            .execute_prepared(&context, &press, &prepared.digest)?
            .ok_or("press result")?;
        host.shutdown()?;
    }
    Ok(())
}

#[test]
fn observation_stays_preparable_after_actions() -> TestResult {
    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts::default()));
    let peer = Peer {
        facts: Arc::clone(&facts),
        sequence: 0,
        epoch: 0,
        frames: 0,
    };
    let host = crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(peer))?;
    let context = context();
    let (session, first) = opened(&host, &context)?;
    let typed = json!({"action":"type","element":1,"revision":first.revision,"text":"hello"});
    let prepared = host.prepare(&context.effective_session_id, &typed)?;
    let second = session
        .execute_prepared(&context, &typed, &prepared.digest)?
        .ok_or("type result")?;
    assert_ne!(second.revision, first.revision);
    // A late event about a child frame reaches the host before the next call;
    // it drops the act binding but not the verified document.
    facts.lock().expect("facts").late_events.push(json!({"method":"Page.frameNavigated",
        "params":{"frame":{"id":"child","parentId":"frame","loaderId":"child-2","url":"https://example.com/child"}}}));
    let look = json!({"action":"look"});
    let prepared = host.prepare(&context.effective_session_id, &look)?;
    assert_eq!(prepared.description["origin"], "https://example.com");
    let session = host.acquire(&context)?;
    let third = session
        .execute_prepared(&context, &look, &prepared.digest)?
        .ok_or("look after act")?;
    let stale = json!({"action":"click","element":2,"revision":second.revision});
    assert_eq!(
        host.prepare(&context.effective_session_id, &stale).err(),
        Some(BrowserError::StaleRevision)
    );
    let click = json!({"action":"click","element":2,"revision":third.revision});
    let prepared = host.prepare(&context.effective_session_id, &click)?;
    let fourth = session
        .execute_prepared(&context, &click, &prepared.digest)?
        .ok_or("click result")?;
    assert_ne!(fourth.revision, third.revision);
    for action in [
        json!({"action":"look"}),
        json!({"action":"scroll","direction":"down"}),
        json!({"action":"back"}),
        json!({"action":"wait","timeoutMs":1}),
    ] {
        let prepared = host.prepare(&context.effective_session_id, &action)?;
        assert!(
            session
                .execute_prepared(&context, &action, &prepared.digest)?
                .is_some(),
            "{action}"
        );
    }
    host.shutdown()?;
    Ok(())
}

#[test]
fn devtools_numbering_overlay_and_input_boundaries() -> TestResult {
    let host = Host::default();
    let context = context();
    let session = host.acquire(&context)?;
    let read = session.read(&context)?;
    assert_eq!(
        read.elements
            .iter()
            .map(|element| element.element)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(
        read.elements[0].name, "element 1",
        "DOM order, not AX order, determines numbering"
    );
    assert!(read.elements[2..]
        .iter()
        .all(|element| element.value == "[입력 불가]"));
    for element in [3, 4, 5] {
        let read = session.read(&context)?;
        assert_eq!(
            session.act(
                &context,
                &BrowserAction::TypeElement {
                    element,
                    revision: read.revision,
                    text: "secret".to_owned(),
                    submit: false
                }
            ),
            Err(BrowserError::InputDenied)
        );
    }
    let read = session.read(&context)?;
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::ClickElement {
                element: 4,
                revision: read.revision
            }
        ),
        Err(BrowserError::InputDenied)
    );
    host.facts.lock().expect("facts").focused_password = true;
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::Press {
                key: "Enter".to_owned()
            }
        ),
        Err(BrowserError::InputDenied)
    );
    host.facts.lock().expect("facts").focused_password = false;
    for (covered, wrong_frame) in [(true, false), (false, true)] {
        let read = session.read(&context)?;
        {
            let mut facts = host.facts.lock().expect("facts");
            facts.covered = covered;
            facts.wrong_frame = wrong_frame;
        }
        assert_eq!(
            session.act(
                &context,
                &BrowserAction::ClickElement {
                    element: 2,
                    revision: read.revision
                }
            ),
            Err(BrowserError::TargetCovered)
        );
    }
    assert!(!host
        .facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .any(|(method, _)| method.starts_with("Input.")));
    {
        let mut facts = host.facts.lock().expect("facts");
        facts.covered = false;
        facts.wrong_frame = false;
        facts.descendant = true;
        facts.popup = true;
    }
    let read = session.read(&context)?;
    let popup = session.act(
        &context,
        &BrowserAction::ClickElement {
            element: 2,
            revision: read.revision.clone(),
        },
    )?;
    assert_eq!(popup.document.target_id, "popup");
    assert_ne!(popup.revision, read.revision);
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::ClickElement {
                element: 2,
                revision: read.revision
            }
        ),
        Err(BrowserError::StaleRevision)
    );
    let read = session.read(&context)?;
    host.facts.lock().expect("facts").changed = true;
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::ClickElement {
                element: 2,
                revision: read.revision
            }
        ),
        Err(BrowserError::StaleRevision)
    );
    let read = session.act(&context, &BrowserAction::Back)?;
    assert_eq!(read.notice.as_deref(), Some("browser_no_previous_history"));
    let read = session.act(
        &context,
        &BrowserAction::Wait {
            text: Some("missing".to_owned()),
            timeout_ms: 1,
        },
    )?;
    assert!(read
        .notice
        .as_deref()
        .is_some_and(|notice| notice.contains("browser_wait_timeout")));
    let facts = host.facts.lock().expect("facts");
    let methods: Vec<&str> = facts
        .commands
        .iter()
        .map(|(method, _)| method.as_str())
        .collect();
    let capture = methods
        .iter()
        .position(|method| *method == "Page.captureScreenshot")
        .ok_or("capture")?;
    assert_eq!(methods[capture - 1], "Runtime.evaluate");
    assert_eq!(methods[capture + 1], "Runtime.evaluate");
    assert!(facts
        .commands
        .iter()
        .any(|(method, params)| method == "Target.closeTarget" && params["targetId"] == "target"));
    drop(facts);
    for failure in ["loader", "capture", "overlay"] {
        let host = Host::default();
        let session = host.acquire(&context)?;
        {
            let mut facts = host.facts.lock().expect("facts");
            facts.capture_failure = failure == "capture";
            facts.overlay_failure = failure == "overlay";
            facts.loader_change = failure == "loader";
        }
        assert!(session.read(&context).is_err());
        let facts = host.facts.lock().expect("facts");
        assert!(facts
            .commands
            .iter()
            .any(|(method, _)| method == "Target.closeTarget"));
        if failure != "loader" {
            assert!(facts
                .commands
                .iter()
                .any(|(method, params)| method == "Runtime.evaluate"
                    && params["expression"]
                        .as_str()
                        .is_some_and(|expression| expression.contains("?.remove()"))));
        }
    }
    let host = Host::default();
    host.facts.lock().expect("facts").child_frame = true;
    let session = host.acquire(&context)?;
    let read = session.read(&context)?;
    session.act(
        &context,
        &BrowserAction::ClickElement {
            element: 2,
            revision: read.revision,
        },
    )?;
    assert!(host
        .facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .any(|(method, params)| method == "Input.dispatchMouseEvent"
            && params["x"] == 260.0
            && params["y"] == 125.0));
    let read = session.read(&context)?;
    host.facts.lock().expect("facts").wrong_frame = true;
    let before = host
        .facts
        .lock()
        .expect("facts")
        .commands
        .iter()
        .filter(|(method, _)| method.starts_with("Input."))
        .count();
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::ClickElement {
                element: 2,
                revision: read.revision
            }
        ),
        Err(BrowserError::TargetCovered)
    );
    assert_eq!(
        before,
        host.facts
            .lock()
            .expect("facts")
            .commands
            .iter()
            .filter(|(method, _)| method.starts_with("Input."))
            .count()
    );
    {
        let mut facts = host.facts.lock().expect("facts");
        facts.child_frame = false;
        facts.extra_nodes = 100;
    }
    let read = session.read(&context)?;
    assert_eq!(read.elements.len(), 100);
    assert_eq!(read.omitted_elements, 5);
    for native in [false, true] {
        for screenshots in [None, Some(false), Some(true)] {
            let mut context = context.clone();
            context.native_image_input_supported = native;
            context.browser_screenshots = screenshots;
            host.facts.lock().expect("facts").commands.clear();
            let read = session.read(&context)?;
            assert_eq!(read.screenshot.is_some(), screenshots.unwrap_or(native));
            let facts = host.facts.lock().expect("facts");
            assert_eq!(
                facts
                    .commands
                    .iter()
                    .any(|(method, _)| method == "Page.captureScreenshot"),
                screenshots.unwrap_or(native)
            );
            assert_eq!(
                facts
                    .commands
                    .iter()
                    .any(
                        |(_, params)| params["expression"]
                            .as_str()
                            .is_some_and(|expression| expression
                                .contains("const root=document.createElement"))
                    ),
                screenshots.unwrap_or(native)
            );
        }
    }
    Ok(())
}

#[test]
fn hit_test_coordinates_follow_scrolled_viewports() -> TestResult {
    let cases = [
        (
            Facts {
                layout_scroll: [80.0, 500.0],
                target_position: Some([90.0, 685.0]),
                ..Facts::default()
            },
            [60.0, 200.0],
            [140, 700],
        ),
        (
            Facts {
                child_frame: true,
                layout_scroll: [80.0, 500.0],
                child_scroll: [25.0, 150.0],
                frame_position: Some([280.0, 600.0]),
                target_position: Some([35.0, 170.0]),
                ..Facts::default()
            },
            [260.0, 135.0],
            [340, 635],
        ),
        (
            Facts {
                layout_scroll: [80.0, 500.0],
                visual_offset: [4.25, 8.5],
                target_position: Some([90.0, 685.0]),
                ..Facts::default()
            },
            [55.75, 191.5],
            [140, 700],
        ),
    ];
    for (facts, mouse_point, document_point) in cases {
        let host = Host {
            facts: Arc::new(Mutex::new(facts)),
            ..Host::default()
        };
        let context = context();
        let session = host.acquire(&context)?;
        let read = session.read(&context)?;
        let element = read
            .elements
            .iter()
            .find(|element| element.name == "element 2")
            .ok_or("visible button")?
            .element;
        let after = session.act(
            &context,
            &BrowserAction::ClickElement {
                element,
                revision: read.revision,
            },
        )?;
        {
            let mut facts = host.facts.lock().expect("facts");
            let lookup = &facts
                .commands
                .iter()
                .find(|(method, _)| method == "DOM.getNodeForLocation")
                .ok_or("location lookup")?
                .1;
            assert_eq!(lookup["x"], document_point[0]);
            assert_eq!(lookup["y"], document_point[1]);
            let mouse_events: Vec<&Value> = facts
                .commands
                .iter()
                .filter(|(method, _)| method == "Input.dispatchMouseEvent")
                .map(|(_, params)| params)
                .collect();
            assert_eq!(mouse_events.len(), 2);
            for event in mouse_events {
                assert_eq!(event["x"], mouse_point[0]);
                assert_eq!(event["y"], mouse_point[1]);
            }
            assert_eq!(
                facts.mouse_targets,
                [2, 2],
                "hit-test and mouse must reach the same node"
            );
            // A fixed parent-page overlay covers the actual mouse location,
            // including when the target is inside a separately scrolled frame.
            facts.covering_rect = Some(facts.target_rect());
            facts.commands.clear();
            facts.mouse_targets.clear();
        }
        assert_eq!(
            session.act(
                &context,
                &BrowserAction::ClickElement {
                    element,
                    revision: after.revision,
                }
            ),
            Err(BrowserError::TargetCovered)
        );
        {
            let facts = host.facts.lock().expect("facts");
            assert_eq!(
                facts
                    .commands
                    .iter()
                    .filter(|(method, _)| method == "DOM.getNodeForLocation")
                    .count(),
                3
            );
            assert!(
                facts.mouse_targets.is_empty(),
                "an occluded point must receive no input"
            );
            assert!(!facts
                .commands
                .iter()
                .any(|(method, _)| method == "Target.closeTarget"));
        }
        assert!(
            session.read(&context).is_ok(),
            "covered target must leave the browser usable"
        );
    }
    Ok(())
}

#[test]
fn location_lookup_without_node_preserves_browser() -> TestResult {
    let host = Host::default();
    let context = context();
    let session = host.acquire(&context)?;
    let read = session.read(&context)?;
    {
        let mut facts = host.facts.lock().expect("facts");
        facts.no_node = true;
        facts.commands.clear();
    }
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::ClickElement {
                element: 2,
                revision: read.revision,
            }
        ),
        Err(BrowserError::TargetCovered)
    );
    {
        let mut facts = host.facts.lock().expect("facts");
        assert_eq!(
            facts
                .commands
                .iter()
                .filter(|(method, _)| method == "DOM.getNodeForLocation")
                .count(),
            3
        );
        assert!(facts.mouse_targets.is_empty());
        assert!(!facts
            .commands
            .iter()
            .any(|(method, _)| method == "Target.closeTarget"));
        facts.no_node = false;
    }
    assert!(session.read(&context).is_ok());

    #[cfg(unix)]
    {
        use shacs_cli::browser::CdpConnection;
        use std::io::{self, Read, Write};
        use std::os::unix::net::UnixStream;
        use std::thread;

        let (client, mut server) = UnixStream::pair()?;
        let mut connection = CdpConnection::new(client.try_clone()?.into(), client.into())?;
        let protocol_errors = [
            json!({"code":-32602,"message":"No node found at given location"}),
            json!({"code":-32602,"message":"Invalid parameters"}),
            json!({"code":-32601,"message":"Method not found"}),
            json!({"code":-32000,"message":"Session not found"}),
            json!({"code":-32000,"message":null}),
            json!({"message":"No node found at given location"}),
            json!({"code":"-32000","message":"No node found at given location"}),
            Value::Null,
        ];
        let peer_errors = protocol_errors.clone();
        let peer = thread::spawn(move || -> io::Result<()> {
            for id in 1..=2 + peer_errors.len() {
                let mut bytes = Vec::new();
                loop {
                    let mut byte = [0];
                    server.read_exact(&mut byte)?;
                    if byte[0] == 0 {
                        break;
                    }
                    bytes.push(byte[0]);
                }
                let request: Value = serde_json::from_slice(&bytes)?;
                assert_eq!(request["id"], id);
                let response = if id == 1 {
                    assert_eq!(request["method"], "DOM.getNodeForLocation");
                    json!({"id":id,"sessionId":"page","error":{"code":-32000,"message":"No node found at given location"}})
                } else if id == 2 {
                    assert_eq!(request["method"], "Page.getLayoutMetrics");
                    json!({"id":id,"sessionId":"page","result":{"ok":true}})
                } else {
                    assert_eq!(request["method"], "DOM.getNodeForLocation");
                    json!({"id":id,"sessionId":"page","error":peer_errors[id-3]})
                };
                let mut bytes = serde_json::to_vec(&response)?;
                bytes.push(0);
                server.write_all(&bytes)?;
            }
            Ok(())
        });
        assert_eq!(
            connection
                .request(
                    "DOM.getNodeForLocation",
                    json!({"x":10,"y":20}),
                    Some("page"),
                    deadline(),
                    &mut no_admission,
                )
                .err(),
            Some(BrowserError::TargetCovered)
        );
        assert_eq!(
            connection
                .request(
                    "Page.getLayoutMetrics",
                    json!({}),
                    Some("page"),
                    deadline(),
                    &mut no_admission
                )?
                .result["ok"],
            true
        );
        for error in protocol_errors {
            assert_eq!(
                connection
                    .request(
                        "DOM.getNodeForLocation",
                        json!({"x":10,"y":20}),
                        Some("page"),
                        deadline(),
                        &mut no_admission,
                    )
                    .err(),
                Some(BrowserError::Protocol),
                "unrecognized error must stay fatal: {error}",
            );
        }
        peer.join().map_err(|_| "location peer panicked")??;
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn location_protocol_error_discards_generation_without_retry() -> TestResult {
    use shacs_cli::browser::{BrowserState, CdpConnection};
    use std::io::{self, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::thread;

    let root = tempfile::tempdir()?;
    let facts = Arc::new(Mutex::new(Facts::default()));
    let server_facts = Arc::clone(&facts);
    let (client, mut server) = UnixStream::pair()?;
    server.set_read_timeout(Some(Duration::from_secs(5)))?;
    let connection = CdpConnection::new(client.try_clone()?.into(), client.into())?;
    let peer = thread::spawn(move || -> io::Result<()> {
        let mut fixture = Peer {
            facts: Arc::clone(&server_facts),
            sequence: 0,
            epoch: 0,
            frames: 0,
        };
        loop {
            let mut bytes = Vec::new();
            loop {
                let mut byte = [0];
                match server.read_exact(&mut byte) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                    Err(error) => return Err(error),
                }
                if byte[0] == 0 {
                    break;
                }
                bytes.push(byte[0]);
            }
            let request: Value = serde_json::from_slice(&bytes)?;
            let method = request["method"]
                .as_str()
                .ok_or_else(|| io::Error::other("missing method"))?;
            let params = request["params"].clone();
            let mut response = if method == "DOM.getNodeForLocation" {
                server_facts
                    .lock()
                    .expect("facts")
                    .commands
                    .push((method.to_owned(), params));
                json!({"id":request["id"],"error":{"code":-32000,"message":"Session not found"}})
            } else {
                let reply = fixture
                    .request(
                        method,
                        params,
                        request["sessionId"].as_str(),
                        deadline(),
                        &mut no_admission,
                    )
                    .map_err(io::Error::other)?;
                json!({"id":request["id"],"result":reply.result})
            };
            if let Some(session) = request.get("sessionId") {
                response["sessionId"] = session.clone();
            }
            let mut bytes = serde_json::to_vec(&response)?;
            bytes.push(0);
            server.write_all(&bytes)?;
        }
    });
    let host =
        crate::spec049_browser_runtime::host_with_devtools(root.path(), Box::new(connection))?;
    let context = context();
    let session = host.acquire(&context)?;
    let read = session.read(&context)?;
    assert_eq!(
        session.act(
            &context,
            &BrowserAction::ClickElement {
                element: 2,
                revision: read.revision,
            }
        ),
        Err(BrowserError::Protocol),
        "an unrelated server error must not become browser_target_covered"
    );
    assert_eq!(host.status()?.state, BrowserState::Failed);
    assert_eq!(
        session.current_document(&context),
        Err(BrowserError::ProcessExited),
        "the real manager must discard the failed generation"
    );
    {
        let facts = facts.lock().expect("facts");
        assert_eq!(
            facts
                .commands
                .iter()
                .filter(|(method, _)| method == "DOM.getNodeForLocation")
                .count(),
            1,
            "fatal errors must not retry another click candidate"
        );
        assert!(
            !facts
                .commands
                .iter()
                .any(|(method, _)| method.starts_with("Input.")),
            "fatal location lookup must send no input"
        );
    }
    drop(session);
    drop(host);
    peer.join().map_err(|_| "protocol peer panicked")??;
    Ok(())
}

#[derive(Default)]
struct Provider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
}
impl Provider {
    fn push(&self, action: Value, bridge: bool) {
        let (name, arguments) = if bridge {
            ("tool_call", json!({"name":"browser","arguments":action}))
        } else {
            ("browser", action)
        };
        self.responses.lock().expect("responses").extend([
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    format!(
                        "browser-call-{}",
                        self.requests.lock().expect("requests").len()
                    ),
                    name,
                    arguments.as_object().expect("arguments").clone(),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("done".to_owned()),
                ..LlmResponse::default()
            },
        ]);
    }
}
impl ProviderClient for Provider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let system = request
            .messages
            .first()
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if system.starts_with("You check one tool result") {
            return Ok(LlmResponse {
                content: Some("no".to_owned()),
                ..LlmResponse::default()
            });
        }
        if system.starts_with("You review one pending action") {
            return Ok(LlmResponse {
                content: Some(r#"{"decision":"allow"}"#.to_owned()),
                ..LlmResponse::default()
            });
        }
        self.requests.lock().expect("requests").push(request);
        Ok(self
            .responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or(LlmResponse {
                content: Some("done".to_owned()),
                ..LlmResponse::default()
            }))
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn bundle(root: &Path, mode: &str, model: &str) -> Result<ConfigBundle, Box<dyn Error>> {
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace)?;
    let config: Config = serde_json::from_value(json!({
        "agents":{"defaults":{"provider":"openai","model":model,"workspace":workspace,"unifiedSession":false}},
        "permissions":{"mode":mode,"allow":["browser"]},
        "tools":{"exec":{"enable":false},"toolSearch":{"enabled":"off"}}
    }))?;
    let context = ConfigContext {
        config_path: root.join("config.json"),
        data_dir: root.to_path_buf(),
        workspace,
    };
    fs::write(&context.config_path, serde_json::to_vec(&config)?)?;
    Ok(ConfigBundle {
        config,
        context,
        migrations: Vec::new(),
    })
}
fn run(adapter: &AgentLoopChatCompletionAdapter, session: &str) -> TestResult {
    adapter.complete_chat(ChatCompletionInvocation {
        provider_request: ProviderRequest {
            messages: vec![json!({"role":"user","content":"fixture"})],
            tools: Vec::new(),
            model: "fixture".to_owned(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        requested_model: None,
        session_key: session.to_owned(),
        media_data_urls: Vec::new(),
        media_paths: Vec::new(),
        temperature: None,
        max_tokens: None,
    })?;
    Ok(())
}

#[test]
fn production_surface_excludes_children() -> TestResult {
    let root = tempfile::tempdir()?;
    let bundle = bundle(root.path(), "bypass_permissions", "gpt-4o")?;
    let provider = Arc::new(Provider::default());
    let host = Arc::new(Host::default());
    for side_effects in [false, true] {
        let adapter = AgentLoopChatCompletionAdapter::from_bundle(bundle.clone(), side_effects)?
            .with_browser_clients(provider.clone(), host.clone(), None);
        run(&adapter, "surface")?;
        let requests = provider.requests.lock().expect("requests");
        let latest = requests.last().ok_or("request")?;
        assert_eq!(
            latest
                .tools
                .iter()
                .any(|tool| tool["function"]["name"] == "browser"),
            side_effects
        );
    }
    let mut child = SubagentExecutionConfig::new(&bundle.context.workspace, "gpt-4o");
    assert!(!runtime::build_subagent_tool_registry(&child).has("browser"));
    child.allowed_tools = Some(vec!["browser".to_owned()]);
    assert!(!runtime::build_subagent_tool_registry(&child).has("browser"));
    let real = shacs_cli::browser::process_browser_host(&bundle);
    assert!(matches!(
        real.acquire(&context()),
        Err(BrowserError::Disabled | BrowserError::Unavailable)
    ));
    Ok(())
}

#[test]
fn production_adapter_passes_per_call_browser_context() -> TestResult {
    let root = tempfile::tempdir()?;
    let a_root = root.path().join("a");
    let b_root = root.path().join("b");
    let initial = bundle(&b_root, "default", "gpt-4o")?;
    let provider = Arc::new(Provider::default());
    let host = Arc::new(Host::default());
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(initial, true)?.with_browser_clients(
        provider.clone(),
        host.clone(),
        Some((
            runtime::ContainmentSnapshotRef {
                contained: Some(true),
                backend: Some("fake-browser".to_owned()),
                digest: Some("spec049-fake".to_owned()),
                summary: None,
            },
            runtime::DockerContainmentSnapshot {
                contained: Some(true),
                runtime: runtime::ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: Vec::new(),
                network_mode: runtime::ContainerNetworkMode::Bridge,
                digest: Some("spec049-fake".to_owned()),
                summary: None,
            },
        )),
    );
    let a_bundle = bundle(&a_root, "auto", "text-only")?;
    let a = AgentLoopChatCompletionAdapter::from_bundle(a_bundle, true)?.with_browser_clients(
        provider.clone(),
        host.clone(),
        None,
    );
    // The registry was constructed in default mode; the run reads bypass now.
    bundle(&b_root, "bypass_permissions", "gpt-4o")?;
    provider.push(json!({"action":"open","url":"https://example.com"}), false);
    a.process_websocket_frame(json!("open"), "client", "A")?;
    let a_owner = host
        .slot
        .lock()
        .expect("slot")
        .owner
        .clone()
        .ok_or("A owner")?;
    provider.push(json!({"action":"open","url":"https://example.com"}), false);
    run(&adapter, "B")?;
    assert_eq!(
        host.slot.lock().expect("slot").owner.as_deref(),
        Some(a_owner.as_str())
    );
    provider.push(json!({"action":"close"}), false);
    a.process_websocket_frame(json!("close"), "client", "A")?;
    provider.push(json!({"action":"open","url":"https://example.com"}), false);
    run(&adapter, "B")?;
    let snapshot = host.read(&context())?;
    provider.push(
        json!({"action":"type","element":1,"revision":snapshot.revision,"text":"typed"}),
        false,
    );
    run(&adapter, "B")?;
    let facts = host.facts.lock().expect("facts");
    assert_eq!(
        facts.value,
        "typed",
        "{:?}",
        provider
            .requests
            .lock()
            .expect("requests")
            .last()
            .map(|request| request
                .messages
                .iter()
                .filter(|message| message["role"] == "tool")
                .collect::<Vec<_>>())
    );
    assert!(facts
        .contexts
        .iter()
        .filter(|context| context.effective_session_id == "B")
        .all(
            |context| context.permission_mode_snapshot.mode == PermissionMode::BypassPermissions
                && !context.permission_interactive
                && context.native_image_input_supported
                && context.browser_screenshots.is_none()
        ));
    assert!(facts
        .contexts
        .iter()
        .any(|context| context.effective_session_id == a_owner
            && context.permission_mode_snapshot.mode == PermissionMode::Auto
            && context.permission_interactive
            && !context.native_image_input_supported));
    assert!(facts
        .contexts
        .iter()
        .any(|context| context.effective_session_id == "B"));
    drop(facts);
    provider.push(json!({"action":"close"}), false);
    run(&adapter, "B")?;
    let mut approval_bundle = bundle(&a_root, "auto", "text-only")?;
    approval_bundle.config.permissions =
        serde_json::from_value(json!({"mode":"auto","ask":["browser"]}))?;
    approval_bundle.config.tools.tool_search = serde_json::from_value(json!({"enabled":"on"}))?;
    approval_bundle.config.tools.browser.screenshots = Some(false);
    fs::write(
        &approval_bundle.context.config_path,
        serde_json::to_vec(&approval_bundle.config)?,
    )?;
    let approval = AgentLoopChatCompletionAdapter::from_bundle(approval_bundle, true)?
        .with_browser_clients(provider.clone(), host.clone(), None);
    provider.push(json!({"action":"open","url":"https://example.com"}), true);
    let before = host.facts.lock().expect("facts").contexts.len();
    approval.process_websocket_frame(json!("ask"), "client", "A")?;
    let session = runtime::SessionManager::new(a_root.join("workspace"))?
        .load_existing(&a_owner)
        .ok_or("approval session")?;
    let pending = session
        .metadata
        .get("pending_permission_approval")
        .ok_or("pending approval")?;
    let ceiling = pending["tool_context"]["permission_ceiling_snapshot"]["capability_ceiling"]
        .as_array()
        .ok_or("production ceiling")?;
    assert!(ceiling.contains(&json!("net_outbound")));
    assert!(ceiling.contains(&json!("browser_act")));
    assert_eq!(
        host.facts.lock().expect("facts").contexts.len(),
        before,
        "asking must not start the host"
    );
    bundle(&a_root, "bypass_permissions", "gpt-4o")?;
    approval.process_websocket_frame(json!("1"), "client", "A")?;
    let facts = host.facts.lock().expect("facts");
    let approved = facts.contexts.last().ok_or("approved context")?;
    assert!(facts.contexts.len() > before);
    assert_eq!(approved.permission_mode_snapshot.mode, PermissionMode::Auto);
    assert!(approved.permission_interactive);
    assert!(!approved.native_image_input_supported);
    assert_eq!(approved.browser_screenshots, Some(false));
    assert_eq!(approved.effective_session_id, a_owner);
    Ok(())
}
