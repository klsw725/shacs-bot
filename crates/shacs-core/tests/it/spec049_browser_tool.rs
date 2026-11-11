use crate::spec048_auto_mode::harness::{
    agent_loop_config, answer, calls, tool_call, RoutingProvider,
};
use crate::spec048_auto_mode::support::{Exec, Options, Rules, Setup, TestResult};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    AgentLoop, BrowserAction, BrowserCallContext, BrowserDocument, BrowserElement, BrowserError,
    BrowserHost, BrowserPreparedAction, BrowserRead, BrowserSession, ContextBuilder, MessageBus,
    PermissionMode, PermissionModeSnapshot, SessionManager, ToolImageSettings,
};
use shacs_core::tools::{BrowserTool, Tool, ToolCallExecutionContext};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct State {
    owner: Option<String>,
    sequence: usize,
    generation: usize,
    pub inputs: usize,
    contexts: Vec<BrowserCallContext>,
    commands: Vec<String>,
    long_text: bool,
    pub description: Value,
    pub mutate_on_execute: Option<Value>,
}

#[derive(Clone, Default)]
pub(crate) struct Host(pub Arc<Mutex<State>>);

impl BrowserHost for Host {
    fn prepare(
        &self,
        session_id: &str,
        arguments: &Value,
    ) -> Result<BrowserPreparedAction, BrowserError> {
        let state = self.0.lock().map_err(|_| BrowserError::StateUnavailable)?;
        if state
            .owner
            .as_deref()
            .is_some_and(|owner| owner != session_id)
        {
            return Err(BrowserError::Busy);
        }
        if matches!(arguments["action"].as_str(), Some("click" | "type"))
            && arguments["revision"] != format!("r{}", state.sequence)
        {
            return Err(BrowserError::StaleRevision);
        }
        let mut description = json!({"action":arguments["action"],
            "origin":if arguments["action"]=="open" {arguments["url"].clone()}else{json!("https://example.com")},
            "role":if arguments["element"]==2 {"button"}else{"textbox"},
            "name":"Search","revision":format!("r{}",state.sequence),
            "frame":"frame","loader":"loader","form":null,"focus":null,
            "text":arguments["text"],"submit":arguments["submit"],"key":arguments["key"]});
        if let Some(fields) = state.description.as_object() {
            description
                .as_object_mut()
                .ok_or(BrowserError::Protocol)?
                .extend(fields.clone());
        }
        let raw = json!({"arguments":arguments,"description":description}).to_string();
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
        let mut state = self.0.lock().map_err(|_| BrowserError::StateUnavailable)?;
        state.contexts.push(context.clone());
        if state
            .owner
            .as_ref()
            .is_some_and(|owner| owner != &context.effective_session_id)
        {
            return Err(BrowserError::Busy);
        }
        if state.owner.is_none() {
            state.generation += 1;
        }
        state.owner = Some(context.effective_session_id.clone());
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
        {
            let mut state = self.0.lock().map_err(|_| BrowserError::StateUnavailable)?;
            if let Some(description) = state.mutate_on_execute.take() {
                state.description = description;
            }
        }
        if self
            .prepare(&context.effective_session_id, arguments)?
            .digest
            != digest
        {
            return Err(BrowserError::StaleRevision);
        }
        let action = match arguments["action"].as_str() {
            Some("look") => return self.read(context).map(Some),
            Some("open") => BrowserAction::Navigate {
                url: arguments["url"]
                    .as_str()
                    .ok_or(BrowserError::Protocol)?
                    .to_owned(),
            },
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
            Some("back") => BrowserAction::Back,
            Some("scroll") => BrowserAction::ScrollElement {
                element: None,
                revision: None,
                direction: "down".to_owned(),
            },
            Some("wait") => BrowserAction::Wait {
                text: None,
                timeout_ms: 1,
            },
            _ => return Err(BrowserError::Protocol),
        };
        self.act(context, &action).map(Some)
    }

    fn current_document(
        &self,
        _context: &BrowserCallContext,
    ) -> Result<BrowserDocument, BrowserError> {
        Ok(BrowserDocument {
            target_id: "page".to_owned(),
            session_id: "cdp".to_owned(),
            frame_id: "frame".to_owned(),
            loader_id: "loader".to_owned(),
            url: "https://example.com/".to_owned(),
        })
    }
    fn read(&self, context: &BrowserCallContext) -> Result<BrowserRead, BrowserError> {
        let mut state = self.0.lock().map_err(|_| BrowserError::StateUnavailable)?;
        state.sequence += 1;
        let capture = context
            .browser_screenshots
            .unwrap_or(context.native_image_input_supported);
        if capture {
            state
                .commands
                .extend(["overlay", "capture", "remove"].map(str::to_owned));
        }
        Ok(BrowserRead {
            document: self.current_document(context)?,
            accessibility: json!({}),
            boxes: Vec::new(),
            title: if state.long_text {
                "x".repeat(2001)
            } else {
                "Fixture".to_owned()
            },
            visible_text: if state.long_text {
                "x".repeat(12001)
            } else {
                "Visible result".to_owned()
            },
            screenshot: capture.then(|| STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture")),
            revision: format!("r{}", state.sequence),
            elements: vec![
                BrowserElement {
                    element: 1,
                    role: "textbox".to_owned(),
                    name: "Search".to_owned(),
                    value: String::new(),
                },
                BrowserElement {
                    element: 2,
                    role: "button".to_owned(),
                    name: "Go".to_owned(),
                    value: String::new(),
                },
            ],
            omitted_elements: 0,
            notice: None,
        })
    }
    fn act(
        &self,
        context: &BrowserCallContext,
        action: &BrowserAction,
    ) -> Result<BrowserRead, BrowserError> {
        {
            let mut state = self.0.lock().map_err(|_| BrowserError::StateUnavailable)?;
            state.contexts.push(context.clone());
            match action {
                BrowserAction::ClickElement { revision, .. }
                | BrowserAction::TypeElement { revision, .. } => {
                    if revision != &format!("r{}", state.sequence) {
                        return Err(BrowserError::StaleRevision);
                    }
                    state.inputs += 1;
                }
                BrowserAction::Press { .. } => state.inputs += 1,
                _ => {}
            }
            state.commands.push(format!("{action:?}"));
        }
        self.read(context)
    }
    fn close(&self, _context: &BrowserCallContext) -> Result<(), BrowserError> {
        self.0
            .lock()
            .map_err(|_| BrowserError::StateUnavailable)?
            .owner = None;
        Ok(())
    }
}

fn context(
    mode: PermissionMode,
    interactive: bool,
    screenshots: Option<bool>,
    native: bool,
) -> ToolCallExecutionContext {
    ToolCallExecutionContext::default().with_browser_context(Some(BrowserCallContext {
        effective_session_id: "test:browser".to_owned(),
        permission_mode_snapshot: PermissionModeSnapshot {
            mode,
            ..PermissionModeSnapshot::default()
        },
        permission_interactive: interactive,
        browser_screenshots: screenshots,
        native_image_input_supported: native,
    }))
}

fn invoke(tool: &BrowserTool, arguments: Value, context: &ToolCallExecutionContext) -> Value {
    let mut context = context.clone();
    if tool
        .validate_params(arguments.as_object().expect("object fixture"))
        .is_empty()
    {
        if let Some(browser) = &context.browser_context {
            match tool.prepare_browser_action(&browser.effective_session_id, &arguments) {
                Ok(prepared) => context.browser_binding_digest = Some(prepared.digest),
                Err(error) => return json!([{"type":"text","text":error.to_string()}]),
            }
        }
    }
    let result = tool.execute_with_context(
        arguments.as_object().expect("object fixture").clone(),
        &context,
    );
    serde_json::from_str(&result.into_text()).expect("content parts")
}

#[test]
fn agent_loop_operates_numbered_browser_across_turns() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    tools.register(BrowserTool::new(Arc::new(host.clone())));
    let responses = [
        json!({"action":"open","url":"https://example.com"}),
        json!({"action":"look"}),
        json!({"action":"type","element":1,"revision":"r2","text":"hello"}),
        json!({"action":"click","element":2,"revision":"r3"}),
        json!({"action":"look"}),
    ];
    let mut script = Vec::new();
    for (index, arguments) in responses.into_iter().enumerate() {
        script.push(calls(vec![tool_call(
            &format!("b{index}"),
            "browser",
            arguments,
        )]));
    }
    script.push(answer("first done"));
    script.push(calls(vec![tool_call(
        "stale",
        "browser",
        json!({"action":"click","element":2,"revision":"r3"}),
    )]));
    script.push(answer("second done"));
    let provider = RoutingProvider::new(script, Vec::new());
    let mut config = agent_loop_config(&setup, true);
    config.permission_mode_snapshot.mode = PermissionMode::BypassPermissions;
    config.permission_ceiling_snapshot = setup
        .context(&Options::mode(PermissionMode::BypassPermissions, true))
        .permission_ceiling_snapshot;
    config.permission_auto_approval.enabled = false;
    config.tool_images = Some(ToolImageSettings {
        media_root: setup.media.clone(),
        attach: true,
    });
    for (prompt, expected) in [
        ("operate fixture", "first done"),
        ("continue", "second done"),
    ] {
        let mut agent = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(&setup.workspace)?,
            ContextBuilder::new(&setup.workspace),
            &tools,
            &provider,
            config.clone(),
        );
        assert_eq!(
            agent
                .process_direct(prompt, Some("test:browser"))?
                .final_content
                .as_deref(),
            Some(expected)
        );
    }
    let state = host.0.lock().expect("host state lock poisoned");
    assert_eq!(state.generation, 1);
    assert_eq!(state.inputs, 2);
    assert!(state
        .contexts
        .iter()
        .all(|context| context.effective_session_id == "test:browser"));
    let stored = SessionManager::new(&setup.workspace)?
        .read_session_file("test:browser")
        .ok_or("no session")?;
    assert!(stored.to_string().contains("tool-results/"));
    assert!(stored.to_string().contains("browser_stale_revision"));
    assert!(!stored
        .to_string()
        .contains(&STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture")));
    assert!(provider
        .assistant_requests()
        .iter()
        .any(
            |request| request.messages.iter().any(|message| message["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image_url")))
        ));
    Ok(())
}

#[test]
fn screenshots_and_overlay_follow_run_settings() {
    for native in [false, true] {
        for screenshots in [None, Some(false), Some(true)] {
            let host = Host::default();
            let tool = BrowserTool::new(Arc::new(host.clone()));
            let execution = context(
                PermissionMode::BypassPermissions,
                false,
                screenshots,
                native,
            );
            let result = invoke(&tool, json!({"action":"look"}), &execution);
            let capture = screenshots.unwrap_or(native);
            assert_eq!(
                result.as_array().expect("parts").len(),
                if capture { 2 } else { 1 }
            );
            assert_eq!(
                host.0.lock().expect("state").commands,
                if capture {
                    vec!["overlay", "capture", "remove"]
                } else {
                    vec![]
                }
            );
            let result = invoke(
                &tool,
                json!({"action":"type","element":1,"revision":"r1","text":"hello"}),
                &execution,
            );
            assert!(result[0]["text"].as_str().expect("text").contains("r2"));
        }
    }
}

#[test]
fn element_visibility_and_input_boundaries() {
    let host = Host::default();
    let tool = BrowserTool::new(Arc::new(host.clone()));
    let execution = context(PermissionMode::BypassPermissions, true, None, false);
    for url in [
        "file:///secret",
        "chrome://settings",
        "devtools://x",
        "view-source:https://example.com",
        "data:text/plain,secret",
        "javascript:alert(1)",
        "/relative",
        "https://user:password@example.com",
        "https://@example.com",
    ] {
        assert!(!tool
            .validate_params(
                json!({"action":"open","url":url})
                    .as_object()
                    .expect("object")
            )
            .is_empty());
    }
    for arguments in [
        json!({"action":"look","url":"https://example.com"}),
        json!({"action":"click","element":1}),
        json!({"action":"click","element":1.5,"revision":"r1"}),
        json!({"action":"press","key":"Control+L"}),
        json!({"action":"scroll","element":1}),
        json!({"action":"wait","timeoutMs":0}),
        json!({"action":"wait","text":"x".repeat(201)}),
        json!({"action":"type","element":1,"revision":"r1","text":"x".repeat(4001)}),
        json!({"action":"look","session":"other"}),
    ] {
        assert!(invoke(&tool, arguments, &execution)[0]["text"]
            .as_str()
            .expect("error")
            .contains("browser_invalid"));
    }
    assert!(host.0.lock().expect("state").contexts.is_empty());
    let result = invoke(&tool, json!({"action":"look"}), &execution);
    let payload: Value =
        serde_json::from_str(result[0]["text"].as_str().expect("text")).expect("payload");
    assert_eq!(payload["elements"][0]["role"], "textbox");
    assert_eq!(payload["elements"][1]["element"], 2);
    invoke(&tool, json!({"action":"look"}), &execution);
    let result = invoke(
        &tool,
        json!({"action":"click","element":1,"revision":"r1"}),
        &execution,
    );
    assert!(result.to_string().contains("browser_stale_revision"));
    assert_eq!(host.0.lock().expect("state").inputs, 0);
    host.0.lock().expect("state").long_text = true;
    let result = invoke(&tool, json!({"action":"look"}), &execution);
    let payload: Value =
        serde_json::from_str(result[0]["text"].as_str().expect("text")).expect("payload");
    assert_eq!(payload["truncated"]["characters"], 2);
    assert_eq!(
        payload["text"].as_str().expect("page text").chars().count(),
        12000
    );
}

#[test]
fn navigation_popup_wait_and_temporary_policy() -> TestResult {
    let host = Host::default();
    let tool = BrowserTool::new(Arc::new(host.clone()));
    assert!(tool
        .execute(
            json!({"action":"look"})
                .as_object()
                .expect("object")
                .clone()
        )
        .into_text()
        .contains("browser_context_unavailable"));
    for mode in [
        PermissionMode::Auto,
        PermissionMode::Default,
        PermissionMode::Plan,
        PermissionMode::DontAsk,
        PermissionMode::AcceptEdits,
    ] {
        for interactive in [false, true] {
            for arguments in [
                json!({"action":"press","key":"Enter"}),
                json!({"action":"click","element":1,"revision":"r1"}),
                json!({"action":"type","element":1,"revision":"r1","text":"x"}),
            ] {
                let result = tool.execute_with_context(
                    arguments.as_object().ok_or("arguments")?.clone(),
                    &context(mode, interactive, None, false),
                );
                assert!(result.into_text().contains("browser_context_unavailable"));
            }
        }
    }
    assert!(host.0.lock().expect("state").contexts.is_empty());
    for interactive in [false, true] {
        let execution = context(PermissionMode::BypassPermissions, interactive, None, false);
        assert!(
            !invoke(&tool, json!({"action":"press","key":"Enter"}), &execution)
                .to_string()
                .contains("policy_unavailable")
        );
        for arguments in [
            json!({"action":"back"}),
            json!({"action":"scroll"}),
            json!({"action":"wait","timeoutMs":1}),
            json!({"action":"close"}),
        ] {
            invoke(&tool, arguments, &execution);
        }
    }
    assert_eq!(host.0.lock().expect("state").inputs, 2);
    assert!(tool.process_adapter_kind().is_none());
    assert_eq!(tool.parameters()["additionalProperties"], false);
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    tools.register(BrowserTool::new(Arc::new(host.clone())));
    let mut mismatch = setup.context(&Options::mode(PermissionMode::BypassPermissions, false));
    mismatch.session_key = Some("real-session".to_owned());
    mismatch.browser_context =
        context(PermissionMode::BypassPermissions, false, None, false).browser_context;
    let executor = shacs_core::runtime::RuntimeToolExecutor::new(&tools);
    let report = executor.execute_tool_calls(
        vec![shacs_core::runtime::RuntimeToolCall::new(
            "mismatch",
            "browser",
            json!({"action":"look"}),
        )],
        &mismatch,
    );
    assert!(
        report.messages[0]
            .content
            .contains("browser_context_unavailable"),
        "{:?}",
        report.messages
    );
    assert!(host.0.lock().expect("state").contexts.is_empty());
    let provider = RoutingProvider::new(
        vec![
            calls(vec![tool_call(
                "bridge",
                "tool_call",
                json!({"name":"browser","arguments":{"action":"open","url":"https://example.com"}}),
            )]),
            answer("bridge done"),
        ],
        Vec::new(),
    );
    let mut config = agent_loop_config(&setup, true);
    config.permission_mode_snapshot.mode = PermissionMode::Default;
    config.permission_environment = Some(setup.environment(&Rules::ask(&["browser"])));
    config.tool_search.enabled = shacs_core::runtime::ToolSearchMode::On;
    config.tool_images = Some(ToolImageSettings {
        media_root: setup.media.clone(),
        attach: false,
    });
    {
        let mut agent = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(&setup.workspace)?,
            ContextBuilder::new(&setup.workspace),
            &tools,
            &provider,
            config.clone(),
        );
        assert_eq!(
            agent
                .process_direct("open fixture", Some("test:approved-browser"))?
                .stop_reason,
            "ask_user"
        );
    }
    config.permission_mode_snapshot.mode = PermissionMode::BypassPermissions;
    config.browser_screenshots = Some(true);
    config.tool_images.as_mut().ok_or("images")?.attach = true;
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&setup.workspace)?,
        ContextBuilder::new(&setup.workspace),
        &tools,
        &provider,
        config,
    );
    assert_eq!(
        agent
            .process_direct("1", Some("test:approved-browser"))?
            .final_content
            .as_deref(),
        Some("bridge done")
    );
    let state = host.0.lock().expect("state");
    assert!(!state.contexts.is_empty());
    assert!(state
        .contexts
        .iter()
        .all(
            |context| context.effective_session_id == "test:approved-browser"
                && context.permission_mode_snapshot.mode == PermissionMode::Default
                && context.permission_interactive
                && !context.native_image_input_supported
                && context.browser_screenshots.is_none()
        ));
    Ok(())
}
