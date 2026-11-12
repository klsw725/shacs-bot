use super::CdpDocument;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::runtime::{BrowserDocument, BrowserError, BrowserPreparedAction};
use std::time::Instant;

// DOM properties resolve external form owners, relative actions and default methods.
const FORM_DETAILS: &str = r#"function() {
 const f=this.form;
 if(!f) return null;
 return {base_url:this.ownerDocument.baseURI,action_attribute:f.getAttribute('action'),
 action:f.action,method:f.method,enctype:f.enctype,target:f.target,no_validate:f.noValidate};
}"#;
const SUBMITTER_DETAILS: &str = r#"function() {
 const f=this.form;
 if(!f) return null;
 return {formaction:this.getAttribute('formaction'),formmethod:this.getAttribute('formmethod'),
 formenctype:this.getAttribute('formenctype'),formtarget:this.getAttribute('formtarget'),
 formnovalidate:this.getAttribute('formnovalidate'),disabled:this.matches(':disabled'),
 url:this.hasAttribute('formaction')?this.formAction:f.action,
 method:this.hasAttribute('formmethod')?this.formMethod:f.method,
 enctype:this.hasAttribute('formenctype')?this.formEnctype:f.enctype,
 target:this.hasAttribute('formtarget')?this.formTarget:f.target,
 no_validate:this.formNoValidate||f.noValidate};
}"#;
// Enter activates a focused submit control itself; only a text-like input
// submits implicitly through the form's default button. Anything else has no
// resolvable submitter and is refused when a form exists.
const ENTER_SUBMITTER: &str = r#"function() {
 const f=this.form;
 if(!f) return null;
 const isSubmit=e=>(e.localName==='button'&&e.type==='submit')||
 (e.localName==='input'&&['submit','image'].includes(e.type));
 if(isSubmit(this)) return this;
 const implicit=this.localName==='input'&&!['button','reset','checkbox','radio','file',
 'hidden','range','color','submit','image'].includes(this.type);
 if(!implicit) return null;
 return Array.from(f.ownerDocument.querySelectorAll('button,input')).find(
 e=>e.form===f&&isSubmit(e))||null;
}"#;
const CLICK_SUBMITTER: &str = r#"function() {
 return this.form&&((this.localName==='button'&&this.type==='submit')||
 (this.localName==='input'&&['submit','image'].includes(this.type)))?this:null;
}"#;
// The element keys reach in this document: its retained active element, down
// through open shadow roots. Window activation (`hasFocus()`) is not asked:
// Chromium restores page focus before it forwards an injected key, so an
// unfocused page's retained control is the recipient all the same. Only a
// document with no active element beyond its body has no recipient.
const DEEP_FOCUS: &str = r#"function() {
 const d=this;
 let n=d.activeElement;
 if(!n||n===d.body||n===d.documentElement)return null;
 while(n&&n.shadowRoot&&n.shadowRoot.activeElement)n=n.shadowRoot.activeElement;
 return n;
}"#;

pub(super) fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

pub(super) fn prepared(arguments: &Value, binding: &Value) -> BrowserPreparedAction {
    let mut description = binding.clone();
    description["action"] = arguments["action"].clone();
    for name in ["text", "submit", "key", "element"] {
        if let Some(value) = arguments.get(name) {
            description[name] = value.clone();
        }
    }
    BrowserPreparedAction {
        digest: digest(&json!({"arguments":arguments,"binding":binding})),
        description,
    }
}

/// Preparation without a page: only `open` and `close` are possible.
/// Every binding carries the slot's control revision, so one prepared before
/// a takeover or a return no longer matches afterwards.
pub(super) fn empty(
    arguments: &Value,
    control: u64,
) -> Result<BrowserPreparedAction, BrowserError> {
    let origin = if arguments["action"] == "open" {
        let url = arguments["url"].as_str().ok_or(BrowserError::Protocol)?;
        Some(origin(url)?)
    } else if arguments["action"] == "close" {
        None
    } else {
        return Err(BrowserError::DocumentChanged);
    };
    Ok(prepared(
        arguments,
        &json!({"origin":origin,"role":null,"name":null,
        "form":null,"focus":null,"revision":null,"control":control}),
    ))
}

/// Preparation of an observation (`look`, `capture`, `scroll`, `back`, `wait`,
/// `close`) on the verified current document. It needs no numbered element,
/// so it stays possible after an act binding was invalidated.
pub(super) fn observation(
    arguments: &Value,
    document: &BrowserDocument,
    control: u64,
) -> Result<BrowserPreparedAction, BrowserError> {
    Ok(prepared(
        arguments,
        &json!({"origin":origin(&document.url)?,"target":document.target_id,
        "session":document.session_id,"frame":document.frame_id,
        "loader":document.loader_id,"url":document.url,
        "role":null,"name":null,"form":null,"focus":null,"revision":null,
        "control":control}),
    ))
}

pub(super) fn origin(url: &str) -> Result<String, BrowserError> {
    let parsed =
        shacs_core::tools::parse_http_url(url).map_err(|_| BrowserError::DocumentChanged)?;
    let authority = parsed.authority.to_ascii_lowercase();
    let default_port = if parsed.scheme == "https" {
        ":443"
    } else {
        ":80"
    };
    let authority = authority.strip_suffix(default_port).unwrap_or(&authority);
    Ok(format!("{}://{authority}", parsed.scheme))
}

pub(super) fn facts(
    cdp: &mut CdpDocument,
    node: u64,
    deadline: Instant,
) -> Result<Value, BrowserError> {
    let form = related(cdp, node, "function(){return this.form||null;}", deadline)?;
    let Some(owner) = form else {
        return Ok(json!({"form":null,"click_submitter":null,"enter_submitter":null}));
    };
    let form_name = accessible(cdp, owner, deadline)?;
    let attributes = cdp.node_function(node, FORM_DETAILS, json!([]), deadline)?;
    let click = submitter(cdp, node, CLICK_SUBMITTER, deadline)?;
    let enter = submitter(cdp, node, ENTER_SUBMITTER, deadline)?;
    Ok(
        json!({"form":{"node":owner,"name":form_name["name"],"attributes":attributes},
        "click_submitter":click,"enter_submitter":enter}),
    )
}

fn submitter(
    cdp: &mut CdpDocument,
    node: u64,
    function: &str,
    deadline: Instant,
) -> Result<Value, BrowserError> {
    let Some(id) = related(cdp, node, function, deadline)? else {
        return Ok(Value::Null);
    };
    let attributes = cdp.node_function(id, SUBMITTER_DETAILS, json!([]), deadline)?;
    Ok(json!({"node":id,"attributes":attributes}))
}

pub(super) fn accessible(
    cdp: &mut CdpDocument,
    id: u64,
    deadline: Instant,
) -> Result<Value, BrowserError> {
    let reply = cdp.request(
        "Accessibility.getPartialAXTree",
        json!({"backendNodeId":id,"fetchRelatives":false}),
        deadline,
    )?;
    let node = reply.result["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| node["backendDOMNodeId"] == id))
        .ok_or(BrowserError::StaleRevision)?;
    Ok(json!({"role":node["role"]["value"],"name":node["name"]["value"]}))
}

/// Resolve node relationships through CDP; page strings never stand in for identity.
fn related(
    cdp: &mut CdpDocument,
    id: u64,
    function: &str,
    deadline: Instant,
) -> Result<Option<u64>, BrowserError> {
    let object = cdp.request(
        "DOM.resolveNode",
        json!({"backendNodeId":id,"objectGroup":"shacs-browser"}),
        deadline,
    )?;
    let object_id = object.result["object"]["objectId"]
        .as_str()
        .ok_or(BrowserError::StaleRevision)?;
    let reply = cdp.request(
        "Runtime.callFunctionOn",
        json!({"objectId":object_id,"functionDeclaration":function,"returnByValue":false}),
        deadline,
    );
    let cleanup = cdp.request(
        "Runtime.releaseObject",
        json!({"objectId":object_id}),
        deadline,
    );
    let (reply, _) = super::with_cleanup(reply, cleanup)?;
    if reply.result.get("exceptionDetails").is_some() {
        return Err(BrowserError::StaleRevision);
    }
    if reply.result["result"]["subtype"] == "null" {
        return Ok(None);
    }
    let related_id = reply.result["result"]["objectId"]
        .as_str()
        .ok_or(BrowserError::StaleRevision)?;
    let described = cdp.request("DOM.describeNode", json!({"objectId":related_id}), deadline);
    let cleanup = cdp.request(
        "Runtime.releaseObject",
        json!({"objectId":related_id}),
        deadline,
    );
    // A stale description must not hide a document the cleanup saw close.
    let (described, _) = super::with_cleanup(described, cleanup)?;
    described.result["node"]["backendNodeId"]
        .as_u64()
        .map(Some)
        .ok_or(BrowserError::StaleRevision)
}

pub(super) fn focus(
    cdp: &mut CdpDocument,
    dom: &Value,
    frame: &str,
    deadline: Instant,
) -> Result<Value, BrowserError> {
    let mut focused = None;
    let mut documents = Vec::new();
    documents_in(&dom["root"], frame, &mut documents);
    let mut unresolved_frame = false;
    for (document, frame) in documents {
        let Some(id) = related(cdp, document, DEEP_FOCUS, deadline)? else {
            continue;
        };
        let id = shadow_focus(cdp, id, deadline)?;
        let described = cdp.request("DOM.describeNode", json!({"backendNodeId":id}), deadline)?;
        // A frame owner holds focus only on behalf of a document inside it.
        // That document is in `documents` when it is a permitted (http(s))
        // one and resolves there; otherwise the focus is in a document the
        // binding may not describe, and the owner element is never the subject.
        if frame_owner(&described.result["node"]) {
            unresolved_frame = true;
            continue;
        }
        if focused.is_some() {
            return Err(BrowserError::StaleRevision);
        }
        let accessible = accessible(cdp, id, deadline)?;
        focused = Some(json!({"node":id,"frame":frame,"role":accessible["role"],
            "name":accessible["name"],"facts":facts(cdp,id,deadline)?}));
    }
    if unresolved_frame && focused.is_none() {
        // Recorded as such so a read still succeeds; a `press` prepared on it
        // is refused and a key that lands on it ends the sequence.
        return Ok(json!({"unresolved":true}));
    }
    Ok(focused.unwrap_or(Value::Null))
}

/// Whether a described DOM node owns a child browsing context: Chromium sets
/// `frameId` (and `contentDocument` when the depth allows) on every frame
/// owner element, and `iframe`, `frame` and `fencedframe` always are one.
pub(super) fn frame_owner(node: &Value) -> bool {
    node["frameId"].is_string()
        || node.get("contentDocument").is_some()
        || matches!(
            node["localName"].as_str(),
            Some("iframe" | "frame" | "fencedframe" | "portal")
        )
}

fn shadow_focus(cdp: &mut CdpDocument, id: u64, deadline: Instant) -> Result<u64, BrowserError> {
    let described = cdp.request(
        "DOM.describeNode",
        json!({"backendNodeId":id,"depth":-1,"pierce":true}),
        deadline,
    )?;
    let mut candidates = Vec::new();
    shadow_candidates(&described.result["node"], 0, &mut candidates);
    let mut deepest = (id, 0);
    for (candidate, depth) in candidates {
        let focused = cdp.node_function(
            candidate,
            r#"function() {
 let n=this;
 for(;;){const r=n.getRootNode();if(r.activeElement!==n)return false;
 if(r.nodeType===9)return true;n=r.host;if(!n)return false;}
}"#,
            json!([]),
            deadline,
        )?;
        if focused == true && depth > deepest.1 {
            deepest = (candidate, depth);
        }
    }
    Ok(deepest.0)
}

fn shadow_candidates(node: &Value, depth: usize, candidates: &mut Vec<(u64, usize)>) {
    if depth > 0 && node["nodeType"] == 1 {
        if let Some(id) = node["backendNodeId"].as_u64() {
            candidates.push((id, depth));
        }
    }
    if let Some(roots) = node["shadowRoots"].as_array() {
        for root in roots {
            shadow_candidates(root, depth + 1, candidates);
        }
    }
    if depth > 0 {
        if let Some(children) = node["children"].as_array() {
            for child in children {
                shadow_candidates(child, depth, candidates);
            }
        }
    }
}

fn documents_in(node: &Value, frame: &str, documents: &mut Vec<(u64, String)>) {
    let frame = node["frameId"].as_str().unwrap_or(frame);
    if let Some(url) = node["documentURL"].as_str() {
        if origin(url).is_err() {
            return;
        }
        if let Some(id) = node["backendNodeId"].as_u64() {
            documents.push((id, frame.to_owned()));
        }
    }
    if let Some(document) = node.get("contentDocument") {
        documents_in(document, frame, documents);
    }
    for key in ["children", "shadowRoots"] {
        if let Some(children) = node[key].as_array() {
            for child in children {
                documents_in(child, frame, documents);
            }
        }
    }
}
