use crate::runtime::{
    tool_images, BrowserCallContext, BrowserError, BrowserHost, BrowserPreparedAction, BrowserRead,
    BrowserTakeover, BrowserTakeoverError, BrowserTakeoverRequest,
};
use crate::tools::{self, JsonMap, Tool, ToolCallExecutionContext, ToolResult, ValidationError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct BrowserTool {
    host: Arc<dyn BrowserHost>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Open {
        url: String,
    },
    Look,
    Capture,
    Click {
        element: usize,
        revision: String,
    },
    Type {
        element: usize,
        revision: String,
        text: String,
        #[serde(default, rename = "submit")]
        _submit: bool,
    },
    Press {
        key: String,
    },
    Scroll {
        #[serde(default = "default_direction")]
        direction: String,
        element: Option<usize>,
        revision: Option<String>,
    },
    Back,
    Wait {
        text: Option<String>,
        #[serde(default = "default_timeout", rename = "timeoutMs")]
        timeout_ms: u64,
    },
    Close,
    Handoff {
        reason: String,
    },
}

fn default_direction() -> String {
    "down".to_owned()
}

fn default_timeout() -> u64 {
    1000
}

impl Request {
    fn parse(params: &JsonMap) -> Result<Self, String> {
        if params.values().any(Value::is_null) {
            return Err("browser_invalid_arguments: null is not an argument value".to_owned());
        }
        let allowed: &[&str] = match params.get("action").and_then(Value::as_str) {
            Some("open") => &["action", "url"],
            Some("look" | "capture" | "back" | "close") => &["action"],
            Some("click") => &["action", "element", "revision"],
            Some("type") => &["action", "element", "revision", "text", "submit"],
            Some("press") => &["action", "key"],
            Some("scroll") => &["action", "direction", "element", "revision"],
            Some("wait") => &["action", "text", "timeoutMs"],
            Some("handoff") => &["action", "reason"],
            _ => return Err("browser_invalid_arguments: unknown action".to_owned()),
        };
        if params.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(
                "browser_invalid_arguments: argument is not accepted for this action".to_owned(),
            );
        }
        let request: Self = serde_json::from_value(Value::Object(params.clone()))
            .map_err(|error| format!("browser_invalid_arguments: {error}"))?;
        match &request {
            Self::Open { url } => {
                tools::parse_http_url(url).map_err(|_| {
                    "browser_invalid_url: absolute http(s) without userinfo required".to_owned()
                })?;
                // The shared parser rejects credentials; reject an empty userinfo too.
                if url.split_once("://").is_none_or(|(_, rest)| {
                    rest.split(['/', '?', '#'])
                        .next()
                        .is_some_and(|authority| authority.contains('@'))
                }) {
                    return Err("browser_invalid_url: userinfo is forbidden".to_owned());
                }
            }
            Self::Click { element, revision }
            | Self::Type {
                element, revision, ..
            } => {
                if *element == 0 || revision.is_empty() {
                    return Err(
                        "browser_invalid_arguments: element and revision required".to_owned()
                    );
                }
            }
            Self::Scroll {
                direction,
                element,
                revision,
            } => {
                if !["up", "down", "left", "right"].contains(&direction.as_str())
                    || element.is_some() != revision.is_some()
                    || element == &Some(0)
                    || revision.as_ref().is_some_and(String::is_empty)
                {
                    return Err(
                        "browser_invalid_arguments: invalid scroll target or direction".to_owned(),
                    );
                }
            }
            Self::Press { key } if !KEYS.contains(&key.as_str()) => {
                return Err("browser_invalid_arguments: unsupported key".to_owned());
            }
            Self::Wait { text, timeout_ms }
                if !(1..=10_000).contains(timeout_ms)
                    || text.as_ref().is_some_and(|text| text.chars().count() > 200) =>
            {
                return Err("browser_invalid_arguments: wait limit exceeded".to_owned());
            }
            Self::Handoff { reason }
                if !(1..=300).contains(&reason.chars().count())
                    || reason.contains(['\n', '\r']) =>
            {
                return Err(
                    "browser_invalid_arguments: reason must be one line of 1 to 300 characters"
                        .to_owned(),
                );
            }
            _ => {}
        }
        if let Self::Type { text, .. } = &request {
            if text.chars().count() > 4000 {
                return Err("browser_invalid_arguments: text exceeds 4000 characters".to_owned());
            }
        }
        Ok(request)
    }
}

const KEYS: &[&str] = &[
    "Enter",
    "Tab",
    "Shift+Tab",
    "Escape",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Backspace",
    "Delete",
];

impl BrowserTool {
    pub fn new(host: Arc<dyn BrowserHost>) -> Self {
        Self { host }
    }

    fn run(
        &self,
        params: &JsonMap,
        execution: &ToolCallExecutionContext,
    ) -> Result<ToolResult, String> {
        let context = execution
            .browser_context
            .as_ref()
            .filter(|context| !context.effective_session_id.trim().is_empty())
            .ok_or_else(|| "browser_context_unavailable".to_owned())?;
        // `handoff` reads no page, so it carries no permission binding.
        if let Some(reason) = params
            .get("reason")
            .and_then(Value::as_str)
            .filter(|_| params.get("action").and_then(Value::as_str) == Some("handoff"))
        {
            return self.handoff(context, execution, reason);
        }
        let digest = execution
            .browser_binding_digest
            .as_deref()
            .ok_or_else(|| "browser_context_unavailable: permission binding required".to_owned())?;
        match params.get("action").and_then(Value::as_str) {
            Some("close") => {
                self.host
                    .close_prepared(context, digest)
                    .map_err(|error| self.refusal(context, error))?;
                Ok(text_result("browser_closed"))
            }
            Some("capture") => self.capture(context, digest),
            _ => {
                let session = self
                    .host
                    .acquire(context)
                    .map_err(|error| self.refusal(context, error))?;
                session
                    .execute_prepared(context, &Value::Object(params.clone()), digest)
                    .map(|read| read.map_or_else(|| text_result("browser_closed"), render))
                    .map_err(|error| self.refusal(context, error))
            }
        }
    }

    /// Hands the browser to the user and pauses the turn on the notice the
    /// user answers (Spec 050 PRD 010 scope 8). A failure is an ordinary
    /// result and the turn goes on.
    fn handoff(
        &self,
        context: &BrowserCallContext,
        execution: &ToolCallExecutionContext,
        reason: &str,
    ) -> Result<ToolResult, String> {
        let origin = execution
            .browser_handoff_origin
            .as_ref()
            .ok_or_else(|| "browser_context_unavailable".to_owned())?;
        let request = BrowserTakeoverRequest {
            session_id: context.effective_session_id.clone(),
            channel: origin.channel.clone(),
            chat_id: origin.chat_id.clone(),
            reply_route: origin.reply_route.clone(),
            tool_call_id: Some(origin.tool_call_id.clone()),
        };
        match self.host.takeover(&request) {
            Ok(takeover) => Ok(ToolResult::AskUserInterrupt {
                question: handoff_question(reason, &takeover),
                options: Vec::new(),
            }),
            Err(BrowserTakeoverError::AlreadyTaken) => Err(BrowserError::UserControl.to_string()),
            Err(BrowserTakeoverError::ViewUnavailable(view)) => Err(format!(
                "browser_handoff_unavailable: 화면 보기를 쓸 수 없습니다({view})"
            )),
            Err(BrowserTakeoverError::Failed(why)) => Err(format!(
                "browser_handoff_unavailable: 넘겨주지 못했습니다({why})"
            )),
            Err(BrowserTakeoverError::Browser(error)) => Err(self.refusal(context, error)),
        }
    }

    /// Saves a fresh screenshot and returns only its path: no image part is
    /// made, so the picture never joins a model request.
    fn capture(&self, context: &BrowserCallContext, digest: &str) -> Result<ToolResult, String> {
        let failed = || BrowserError::CaptureFailed.to_string();
        let media_root = context
            .media_root
            .as_deref()
            .filter(|root| root.is_absolute())
            .ok_or_else(failed)?;
        let capture = self
            .host
            .capture_prepared(context, digest)
            .map_err(|error| self.refusal(context, error))?;
        let reference = tool_images::store(media_root, &capture.png).map_err(|_| failed())?;
        let path = media_root.join(reference);
        Ok(text_result(&format!(
            "browser_capture: {}\n이 경로를 그대로 message 도구의 media로 보내세요. 요소 번호를 쓰려면 다시 look하세요.",
            path.display()
        )))
    }

    /// The host's refusal as this call's result. When it is the session's
    /// closure notice, the notice has now been delivered and is acknowledged.
    fn refusal(&self, context: &BrowserCallContext, error: BrowserError) -> String {
        let session_id = &context.effective_session_id;
        let delivered = self
            .host
            .closure_notice(session_id)
            .filter(|notice| notice.error == error);
        if let Some(notice) = delivered {
            self.host
                .acknowledge_closure_notice(session_id, &notice.token);
        }
        error.to_string()
    }
}

impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Open and read http(s) pages in the official container. Use element numbers and the latest revision. Click/type/press require browser_act permission and a verified target. Passwords, file upload and downloads are unavailable; use web_fetch for downloads. Canvas controls may be inaccessible. When a step is the user's to decide (a choice among options, a purchase, payment, submission or other commitment they did not ask for, or a page that differs from the request), do not act: tell the user briefly what you did and what the page shows, list the choices by their exact element names from the latest result, ask them to reply with the name, and end your turn. When the reply arrives, look again and click only if an element with that name is on the page now; otherwise describe the changed page. Never ask the user to send passwords, verification codes or card numbers in chat. For logins, passwords, verification codes, CAPTCHAs or identity checks, and when the user asks to take over the browser, call handoff with a short reason; the user then works on the screen viewer. Any reply to handoff returns the browser: read it, and if the user postponed, do not hand off again; finish what you can and list what remains. While the user controls the browser every action returns browser_user_control. After control returns, look again; earlier element numbers are invalid. Use capture only when the user asks to see the screen; it saves a fresh screenshot and returns a path to send with the message tool. If capture fails, tell the user why and do not send an older image."
    }

    fn exclusive(&self) -> bool {
        true
    }

    fn prepare_browser_action(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        self.host.prepare(session_id, arguments)
    }

    fn browser_host(&self) -> Option<&dyn BrowserHost> {
        Some(self.host.as_ref())
    }

    fn parameters(&self) -> Value {
        let actions = [
            ("open", vec!["url"], vec!["url"]),
            ("look", vec![], vec![]),
            ("capture", vec![], vec![]),
            (
                "click",
                vec!["element", "revision"],
                vec!["element", "revision"],
            ),
            (
                "type",
                vec!["element", "revision", "text", "submit"],
                vec!["element", "revision", "text"],
            ),
            ("press", vec!["key"], vec!["key"]),
            ("scroll", vec!["direction", "element", "revision"], vec![]),
            ("back", vec![], vec![]),
            ("wait", vec!["text", "timeoutMs"], vec![]),
            ("close", vec![], vec![]),
            ("handoff", vec!["reason"], vec!["reason"]),
        ];
        let properties = json!({
            "action":{"type":"string","enum":["open","look","capture","click","type","press","scroll","back","wait","close","handoff"]},
            "url":{"type":"string"}, "element":{"type":"integer","minimum":1},
            "revision":{"type":"string","minLength":1}, "text":{"type":"string","maxLength":4000},
            "submit":{"type":"boolean","default":false}, "key":{"type":"string","enum":KEYS},
            "direction":{"type":"string","enum":["up","down","left","right"],"default":"down"},
            "timeoutMs":{"type":"integer","minimum":1,"maximum":10000,"default":1000},
            "reason":{"type":"string","minLength":1,"maxLength":300}
        });
        let variants: Vec<Value> = actions.into_iter().map(|(action, allowed, required)| {
            let mut fields = JsonMap::new();
            fields.insert("action".to_owned(), json!({"const":action}));
            for key in allowed { fields.insert(key.to_owned(), properties[key].clone()); }
            if action == "wait" { fields.insert("text".to_owned(), json!({"type":"string","maxLength":200})); }
            let mut required_keys = vec!["action"];
            required_keys.extend(required);
            let mut schema = json!({"type":"object","properties":fields,"required":required_keys,"additionalProperties":false});
            if action == "scroll" {
                schema["dependentRequired"] = json!({"element":["revision"],"revision":["element"]});
            }
            schema
        }).collect();
        json!({"type":"object","properties":properties,"required":["action"],"additionalProperties":false,"oneOf":variants})
    }

    fn cast_params(&self, params: JsonMap) -> JsonMap {
        params
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        Request::parse(params)
            .err()
            .map(|message| ValidationError::new("", message))
            .into_iter()
            .collect()
    }

    fn preflight_refusal(&self, params: &JsonMap) -> Option<String> {
        Request::parse(params).err()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        text_result("browser_context_unavailable")
    }

    fn execute_with_context(
        &self,
        params: JsonMap,
        context: &ToolCallExecutionContext,
    ) -> ToolResult {
        Request::parse(&params)
            .and_then(|_| self.run(&params, context))
            .unwrap_or_else(|error| text_result(&error))
    }
}

/// The notice a `handoff` pauses on.
fn handoff_question(reason: &str, takeover: &BrowserTakeover) -> String {
    format!(
        "브라우저를 넘깁니다: {reason}\n\
         화면 보기: {url}\n\
         화면 보기 비밀번호로 연결하세요. 한글이나 긴 글은 화면 아래 붙여넣기 칸을 쓰세요.\n\
         끝나면 이 메시지에 답해 주세요(예: \"다 했어\"). 지금 화면을 볼 수 없으면 \"나중에\"라고 답해 이 단계를 넘길 수 있습니다.\n\
         다른 대화방에서는 /browser return {label}로 돌려줄 수 있습니다.\n\
         비밀번호나 인증 코드는 대화로 보내지 마세요.\n\
         {minutes}분이 지나면 자동으로 돌려받습니다.",
        url = takeover.view_url,
        label = takeover.label,
        minutes = takeover.minutes,
    )
}

fn text_result(text: &str) -> ToolResult {
    ToolResult::Json(json!([{"type":"text","text":text}]))
}

fn limited(text: &str, limit: usize) -> (String, usize) {
    (
        text.chars().take(limit).collect(),
        text.chars().count().saturating_sub(limit),
    )
}

fn render(read: BrowserRead) -> ToolResult {
    let (url, url_omitted) = limited(&read.document.url, 2000);
    let (title, title_omitted) = limited(&read.title, 2000);
    let (text, text_omitted) = limited(&read.visible_text, 12000);
    let mut characters_omitted = url_omitted + title_omitted + text_omitted;
    let mut elements = Vec::new();
    for element in read.elements.iter().take(100) {
        let (name, name_omitted) = limited(&element.name, 200);
        let (value, value_omitted) = limited(&element.value, 200);
        characters_omitted += name_omitted + value_omitted;
        elements
            .push(json!({"element":element.element,"role":element.role,"name":name,"value":value}));
    }
    let elements_omitted = read.omitted_elements + read.elements.len().saturating_sub(100);
    let content = json!({"url":url,"title":title,"revision":read.revision,"elements":elements,"text":text,
        "truncated":{"characters":characters_omitted,"elements":elements_omitted},"notice":read.notice});
    let mut parts = vec![json!({"type":"text","text":content.to_string()})];
    if let Some(image) = read.screenshot {
        parts.push(json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{image}")}}));
    }
    ToolResult::Json(Value::Array(parts))
}
