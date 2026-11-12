use super::{browser_action_binding, CdpDocument};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::runtime::{BrowserDocument, BrowserElement, BrowserError, BrowserPreparedAction};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) struct BrowserSnapshot {
    pub revision: String,
    pub document: BrowserDocument,
    pub epoch: u64,
    pub elements: Vec<BrowserNode>,
    pub omitted: usize,
    pub focus: Value,
}

#[derive(Clone)]
pub(super) struct BrowserNode {
    pub element: BrowserElement,
    pub backend_id: u64,
    pub frame_id: String,
    pub kind: String,
    pub password: bool,
    pub editable: bool,
    pub rect: [f64; 4],
    pub facts: Value,
}

const ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "option",
    "switch",
    "slider",
    "spinbutton",
    "menuitem",
    "tab",
];

// These scripts are fixed implementation details, never model-supplied programs.
const NODE_DETAILS: &str = r#"function() {
 const w=this.ownerDocument.defaultView, s=w.getComputedStyle(this);
 const r=this.getBoundingClientRect(), a=(this.getAttribute('autocomplete')||'').toLowerCase().split(/\s+/);
 const kind=(this.getAttribute('type')||this.localName||'').toLowerCase();
 const password=kind==='password'||a.includes('current-password')||a.includes('new-password');
 return {connected:this.isConnected,visible:s.display!=='none'&&s.visibility!=='hidden'&&s.visibility!=='collapse'&&r.width>0&&r.height>0&&r.right>0&&r.bottom>0&&r.left<w.innerWidth&&r.top<w.innerHeight,
 kind,password,editable:!this.disabled&&!this.readOnly&&(this.isContentEditable||this.localName==='textarea'||(this.localName==='input'&&['text','search','email','url','tel','number','password',''].includes(this.type))),
 disabled:!!this.disabled,value:password||kind==='file'?'[입력 불가]':String(this.value||'')};
}"#;

impl BrowserSnapshot {
    pub fn collect(
        cdp: &mut CdpDocument,
        document: BrowserDocument,
        epoch: u64,
        accessibility: &Value,
        deadline: Instant,
    ) -> Result<Self, BrowserError> {
        let dom = cdp
            .request(
                "DOM.getDocument",
                json!({"depth":-1,"pierce":true}),
                deadline,
            )?
            .result;
        let mut order = HashMap::new();
        index_dom(&dom["root"], &document.frame_id, true, &mut order);
        let layout = cdp
            .request("Page.getLayoutMetrics", json!({}), deadline)?
            .result;
        let viewport = &layout["cssVisualViewport"];
        let width = viewport["clientWidth"]
            .as_f64()
            .ok_or(BrowserError::Protocol)?;
        let height = viewport["clientHeight"]
            .as_f64()
            .ok_or(BrowserError::Protocol)?;
        let mut elements = Vec::new();
        if let Some(nodes) = accessibility["nodes"].as_array() {
            for node in nodes {
                let Some(id) = node["backendDOMNodeId"].as_u64() else {
                    continue;
                };
                let role = node["role"]["value"].as_str().unwrap_or("");
                if !ROLES.contains(&role) || node["ignored"] == true || disabled(node) {
                    continue;
                }
                let Some((_, frame_id, permitted)) = order.get(&id) else {
                    continue;
                };
                if !permitted {
                    continue;
                }
                let details = cdp.node_function(id, NODE_DETAILS, json!([]), deadline)?;
                if details["connected"] != true
                    || details["visible"] != true
                    || details["disabled"] == true
                {
                    continue;
                }
                let reply =
                    match cdp.request("DOM.getBoxModel", json!({"backendNodeId":id}), deadline) {
                        Ok(reply) => reply,
                        Err(BrowserError::GeometryUnavailable) => continue,
                        Err(error) => return Err(error),
                    };
                let Some(rect) = visible_rect(&reply.result["model"]["border"], width, height)
                    .or_else(|| visible_rect(&reply.result["model"]["content"], width, height))
                else {
                    continue;
                };
                let (kind, password, readonly) = node_input(cdp, id, deadline)?;
                elements.push(BrowserNode {
                    element: BrowserElement {
                        element: 0,
                        role: role.to_owned(),
                        name: node["name"]["value"].as_str().unwrap_or("").to_owned(),
                        value: if password || kind == "file" {
                            "[입력 불가]".to_owned()
                        } else {
                            details["value"].as_str().unwrap_or("").to_owned()
                        },
                    },
                    backend_id: id,
                    frame_id: frame_id.clone(),
                    kind,
                    password,
                    editable: details["editable"] == true && !readonly,
                    rect,
                    facts: browser_action_binding::facts(cdp, id, deadline)?,
                });
            }
        }
        elements.sort_by_key(|node| order.get(&node.backend_id).map(|entry| entry.0));
        elements.dedup_by_key(|node| node.backend_id);
        let omitted = elements.len().saturating_sub(100);
        elements.truncate(100);
        for (index, node) in elements.iter_mut().enumerate() {
            node.element.element = index + 1;
        }
        let sequence = SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let binding = format!(
            "{sequence}:{}:{}:{}:{epoch}",
            document.target_id, document.session_id, document.loader_id
        );
        let revision = format!("{:x}", Sha256::digest(binding.as_bytes()));
        let focus = browser_action_binding::focus(cdp, &dom, &document.frame_id, deadline)?;
        Ok(Self {
            revision,
            document,
            epoch,
            elements,
            omitted,
            focus,
        })
    }

    pub fn prepare(
        &self,
        arguments: &Value,
        control: u64,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        let action = arguments["action"].as_str().ok_or(BrowserError::Protocol)?;
        let mut material = json!({
            "origin":browser_action_binding::origin(&self.document.url)?,
            "revision":self.revision,"target":self.document.target_id,
            "session":self.document.session_id,"frame":self.document.frame_id,
            "loader":self.document.loader_id,"url":self.document.url,"epoch":self.epoch,
            "role":null,"name":null,"form":null,"focus":null,"submitter":null,
            "control":control
        });
        let facts = match action {
            "click" | "type" => {
                if arguments["revision"] != self.revision {
                    return Err(BrowserError::StaleRevision);
                }
                let node = self
                    .elements
                    .iter()
                    .find(|node| arguments["element"].as_u64() == Some(node.element.element as u64))
                    .ok_or(BrowserError::StaleRevision)?;
                material["node"] = json!(node.backend_id);
                material["frame"] = json!(node.frame_id);
                material["role"] = json!(node.element.role);
                material["name"] = json!(node.element.name);
                Some(&node.facts)
            }
            "press" => {
                // Focus inside a document the binding may not describe (a frame
                // owner over a non-http or unverified child) has no subject.
                if self.focus["unresolved"] == true {
                    return Err(BrowserError::StaleRevision);
                }
                material["focus"] = self.focus.clone();
                material["role"] = self.focus["role"].clone();
                material["name"] = self.focus["name"].clone();
                Some(&self.focus["facts"])
            }
            _ => None,
        };
        if let Some(facts) = facts {
            material["form"] = facts["form"].clone();
            let enter = (action == "type" && arguments["submit"] == true)
                || (action == "press" && arguments["key"] == "Enter");
            let submitter = if enter {
                &facts["enter_submitter"]
            } else if action == "click" {
                &facts["click_submitter"]
            } else {
                &Value::Null
            };
            if enter
                && !facts["form"].is_null()
                && (submitter.is_null() || submitter["attributes"]["disabled"] == true)
            {
                return Err(BrowserError::StaleRevision);
            }
            material["submitter"] = submitter.clone();
            material["submission"] = if submitter.is_null() {
                facts["form"]["attributes"].clone()
            } else {
                submitter["attributes"].clone()
            };
            // Bind both possible submitters as well as the effective submission.
            material["form_binding"] = facts.clone();
        }
        Ok(browser_action_binding::prepared(arguments, &material))
    }

    pub fn target(
        &self,
        cdp: &mut CdpDocument,
        element: usize,
        revision: &str,
        deadline: Instant,
    ) -> Result<BrowserNode, BrowserError> {
        if self.revision != revision {
            return Err(BrowserError::StaleRevision);
        }
        self.confirm_document(cdp, deadline)?;
        let node = self
            .elements
            .iter()
            .find(|node| node.element.element == element)
            .ok_or(BrowserError::StaleRevision)?;
        let ax = cdp
            .request(
                "Accessibility.getPartialAXTree",
                json!({"backendNodeId":node.backend_id,"fetchRelatives":false}),
                deadline,
            )?
            .result;
        let current = ax["nodes"]
            .as_array()
            .and_then(|nodes| {
                nodes
                    .iter()
                    .find(|candidate| candidate["backendDOMNodeId"] == node.backend_id)
            })
            .ok_or(BrowserError::StaleRevision)?;
        let details = cdp.node_function(node.backend_id, NODE_DETAILS, json!([]), deadline)?;
        let (kind, password, readonly) = node_input(cdp, node.backend_id, deadline)?;
        if current["ignored"] == true
            || disabled(current)
            || current["role"]["value"] != node.element.role
            || current["name"]["value"].as_str().unwrap_or("") != node.element.name
            || details["connected"] != true
            || details["visible"] != true
            || details["disabled"] == true
            || kind != node.kind
            || password != node.password
            || (details["editable"] == true && !readonly) != node.editable
        {
            return Err(BrowserError::StaleRevision);
        }
        self.confirm_document(cdp, deadline)?;
        Ok(node.clone())
    }

    pub fn confirm_document(
        &self,
        cdp: &mut CdpDocument,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        let (document, reply) = cdp.frame(deadline)?;
        if document != self.document || reply.document_epoch != self.epoch {
            return Err(BrowserError::StaleRevision);
        }
        Ok(())
    }
}

/// The root visual viewport: client width, height and the page offset.
fn visual_viewport(
    cdp: &mut CdpDocument,
    deadline: Instant,
) -> Result<(f64, f64, f64, f64), BrowserError> {
    let layout = cdp
        .request("Page.getLayoutMetrics", json!({}), deadline)?
        .result;
    let viewport = &layout["cssVisualViewport"];
    let width = viewport["clientWidth"]
        .as_f64()
        .ok_or(BrowserError::TargetCovered)?;
    let height = viewport["clientHeight"]
        .as_f64()
        .ok_or(BrowserError::TargetCovered)?;
    let page_x = viewport["pageX"]
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or(BrowserError::TargetCovered)?;
    let page_y = viewport["pageY"]
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or(BrowserError::TargetCovered)?;
    Ok((width, height, page_x, page_y))
}

impl BrowserNode {
    pub fn hit(
        &self,
        cdp: &mut CdpDocument,
        deadline: Instant,
    ) -> Result<(f64, f64), BrowserError> {
        let model = cdp
            .request(
                "DOM.getBoxModel",
                json!({"backendNodeId":self.backend_id}),
                deadline,
            )?
            .result;
        let (width, height, page_x, page_y) = visual_viewport(cdp, deadline)?;
        let rect = visible_rect(&model["model"]["border"], width, height)
            .or_else(|| visible_rect(&model["model"]["content"], width, height))
            .ok_or(BrowserError::TargetCovered)?;
        let subtree = self.subtree(cdp, deadline)?;
        // Box quads include child-frame transforms and use root visual-viewport
        // CSS coordinates. Location lookup takes root-document CSS coordinates.
        // Quantize that integer lookup first, then derive the exact mouse point.
        for (horizontal, vertical) in [(0.5, 0.5), (0.25, 0.25), (0.75, 0.75)] {
            let document_x = (rect[0] + rect[2] * horizontal + page_x).round();
            let document_y = (rect[1] + rect[3] * vertical + page_y).round();
            let x = document_x - page_x;
            let y = document_y - page_y;
            if x < rect[0] || x >= rect[0] + rect[2] || y < rect[1] || y >= rect[1] + rect[3] {
                continue;
            }
            if self.point_hits(cdp, &subtree, document_x, document_y, deadline)? {
                return Ok((x, y));
            }
        }
        Err(BrowserError::TargetCovered)
    }

    /// This node's subtree (shadow roots and pseudo elements included), read
    /// before a location lookup so that the lookup is the last request before
    /// the caller dispatches.
    fn subtree(&self, cdp: &mut CdpDocument, deadline: Instant) -> Result<Value, BrowserError> {
        Ok(cdp
            .request(
                "DOM.describeNode",
                json!({"backendNodeId":self.backend_id,"depth":-1,"pierce":true}),
                deadline,
            )?
            .result)
    }

    /// Whether the viewport point a press went to still hit-tests to this
    /// node or a descendant in its own frame, with the page offset as it is
    /// now. `Ok(false)` when something else is there.
    pub fn covers(
        &self,
        cdp: &mut CdpDocument,
        x: f64,
        y: f64,
        deadline: Instant,
    ) -> Result<bool, BrowserError> {
        let (_, _, page_x, page_y) = visual_viewport(cdp, deadline)?;
        let subtree = self.subtree(cdp, deadline)?;
        self.point_hits(cdp, &subtree, x + page_x, y + page_y, deadline)
    }

    fn point_hits(
        &self,
        cdp: &mut CdpDocument,
        subtree: &Value,
        document_x: f64,
        document_y: f64,
        deadline: Instant,
    ) -> Result<bool, BrowserError> {
        let document_x = document_x.round();
        let document_y = document_y.round();
        let coordinate_range = f64::from(i32::MIN)..=f64::from(i32::MAX);
        if !coordinate_range.contains(&document_x) || !coordinate_range.contains(&document_y) {
            return Ok(false);
        }
        let hit = match cdp.request("DOM.getNodeForLocation",
            json!({"x":document_x as i32,"y":document_y as i32,"includeUserAgentShadowDOM":true,"ignorePointerEventsNone":false}), deadline) {
            Ok(reply) => reply.result,
            Err(BrowserError::TargetCovered) => return Ok(false),
            Err(error) => return Err(error),
        };
        // Root hit-testing includes child contents and parent occluders.
        // A covering parent iframe element cannot match the child's frame.
        if hit["frameId"] != self.frame_id {
            return Ok(false);
        }
        let Some(id) = hit["backendNodeId"].as_u64() else {
            return Ok(false);
        };
        Ok(id == self.backend_id || contains_backend(&subtree["node"], id))
    }
}

fn contains_backend(node: &Value, id: u64) -> bool {
    node["backendNodeId"] == id
        || ["children", "shadowRoots", "pseudoElements"]
            .iter()
            .any(|key| {
                node[*key].as_array().is_some_and(|children| {
                    children.iter().any(|child| contains_backend(child, id))
                })
            })
}

fn disabled(node: &Value) -> bool {
    node["properties"].as_array().is_some_and(|properties| {
        properties
            .iter()
            .any(|property| property["name"] == "disabled" && property["value"]["value"] == true)
    })
}

fn index_dom(
    node: &Value,
    frame: &str,
    permitted: bool,
    order: &mut HashMap<u64, (usize, String, bool)>,
) {
    let frame = node["frameId"].as_str().unwrap_or(frame);
    let permitted = node["documentURL"].as_str().map_or(permitted, |url| {
        shacs_core::tools::parse_http_url(url).is_ok()
    });
    if let Some(id) = node["backendNodeId"].as_u64() {
        let index = order.len();
        order.insert(id, (index, frame.to_owned(), permitted));
    }
    for key in ["children", "shadowRoots"] {
        if let Some(children) = node[key].as_array() {
            for child in children {
                index_dom(child, frame, permitted, order);
            }
        }
    }
    if let Some(document) = node.get("contentDocument") {
        index_dom(document, frame, permitted, order);
    }
}

fn visible_rect(quad: &Value, width: f64, height: f64) -> Option<[f64; 4]> {
    let values = quad.as_array()?;
    if values.len() != 8 {
        return None;
    }
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    let points = values.as_chunks::<2>().0;
    for point in points {
        let x = point[0].as_f64()?;
        let y = point[1].as_f64()?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    left = left.max(0.0);
    top = top.max(0.0);
    right = right.min(width);
    bottom = bottom.min(height);
    (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
}

/// Sensitive input decisions use Chromium's DOM attributes, not overridable
/// JavaScript getters supplied by the page.
pub(super) fn node_input(
    cdp: &mut CdpDocument,
    id: u64,
    deadline: Instant,
) -> Result<(String, bool, bool), BrowserError> {
    let reply = cdp
        .request(
            "DOM.describeNode",
            json!({"backendNodeId":id,"depth":0}),
            deadline,
        )?
        .result;
    let node = &reply["node"];
    let name = node["localName"]
        .as_str()
        .ok_or(BrowserError::StaleRevision)?;
    let mut kind = name.to_owned();
    let mut autocomplete = String::new();
    let mut readonly = false;
    if let Some(attributes) = node["attributes"].as_array() {
        let attributes = attributes.as_chunks::<2>().0;
        for [key, value] in attributes {
            match key.as_str() {
                Some("type") if name == "input" => {
                    kind = value.as_str().unwrap_or("").to_ascii_lowercase()
                }
                Some("autocomplete") => {
                    autocomplete = value.as_str().unwrap_or("").to_ascii_lowercase()
                }
                Some("readonly" | "disabled") => readonly = true,
                _ => {}
            }
        }
    }
    let password = kind == "password"
        || autocomplete
            .split_ascii_whitespace()
            .any(|token| matches!(token, "current-password" | "new-password"));
    Ok((kind, password, readonly))
}
