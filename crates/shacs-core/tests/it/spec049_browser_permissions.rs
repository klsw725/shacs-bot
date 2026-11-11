use crate::spec048_auto_mode::harness::{
    agent_loop_config, answer, calls, tool_call, RoutingProvider, BLOCK,
};
use crate::spec048_auto_mode::support::{
    self, Exec, Options, Rules, Setup, TestResult, Verdict, ALL_MODES,
};
use crate::spec049_browser_tool::Host;
use serde_json::{json, Value};
use shacs_config::WorkspacePermissionId;
use shacs_core::runtime::{
    AgentLoop, ApprovalDecisionKind, AutoModeSlots, BrowserCallContext, ContextBuilder, MessageBus,
    PermissionMode, ProjectPermissionStoreConfig, RuntimeToolCall, SafetyCapability,
    SessionManager, SlotOverrides, ToolExecutionContext, ToolSearchMode,
};
use shacs_core::tools::BrowserTool;
use std::fs;
use std::sync::Arc;

fn context(setup: &Setup, options: &Options) -> ToolExecutionContext {
    let mut context = setup.context(options);
    context.browser_context = Some(BrowserCallContext {
        effective_session_id: "cli:direct".to_owned(),
        permission_mode_snapshot: context.permission_mode_snapshot.clone(),
        permission_interactive: options.interactive,
        browser_screenshots: Some(false),
        native_image_input_supported: false,
    });
    context
}

fn arguments(action: &str) -> Value {
    match action {
        "open" => json!({"action":"open","url":"https://example.com/"}),
        "click" => json!({"action":"click","element":2,"revision":"r0"}),
        "type" => json!({"action":"type","element":1,"revision":"r0","text":"hello","submit":true}),
        "press" => json!({"action":"press","key":"Enter"}),
        "wait" => json!({"action":"wait","timeoutMs":1}),
        _ => json!({"action":action}),
    }
}

#[test]
fn action_capabilities_and_mode_matrix() -> TestResult {
    let setup = Setup::new()?;
    for mode in ALL_MODES {
        for interactive in [false, true] {
            for verdict in [None, Some(Verdict::Allow), Some(Verdict::Block)] {
                for action in [
                    "open", "look", "scroll", "back", "wait", "close", "click", "type", "press",
                ] {
                    let (mut registry, _) = setup.registry(Exec::Gated);
                    let host = Host::default();
                    registry.register(BrowserTool::new(Arc::new(host.clone())));
                    let mut options = Options::mode(mode, interactive);
                    options.verdict = verdict;
                    let report = support::run_report(
                        &registry,
                        &context(&setup, &options),
                        RuntimeToolCall::new("browser-matrix", "browser", arguments(action)),
                    );
                    let act = matches!(action, "click" | "type" | "press");
                    assert_eq!(
                        report.permissioned_actions[0].capabilities,
                        [if act {
                            SafetyCapability::BrowserAct
                        } else {
                            SafetyCapability::NetOutbound
                        }],
                        "{mode:?} {interactive} {action}"
                    );
                    let allowed = mode == PermissionMode::BypassPermissions
                        || (!act && mode == PermissionMode::Plan)
                        || (mode == PermissionMode::Auto
                            && (!act || interactive)
                            && verdict == Some(Verdict::Allow));
                    let asked = interactive
                        && matches!(
                            mode,
                            PermissionMode::Default
                                | PermissionMode::AcceptEdits
                                | PermissionMode::Plan
                        )
                        && !(mode == PermissionMode::Plan && !act);
                    assert_eq!(
                        !report.executed_call_ids.is_empty(),
                        allowed,
                        "{mode:?} {interactive} {action} {verdict:?}: {report:?}"
                    );
                    assert_eq!(
                        report.interrupt.is_some(),
                        asked,
                        "{mode:?} {interactive} {action}"
                    );
                    assert_eq!(
                        host.0.lock().expect("state").inputs,
                        usize::from(allowed && act)
                    );
                }
            }
        }
    }
    let (mut registry, _) = setup.registry(Exec::Gated);
    registry.register(BrowserTool::new(Arc::new(Host::default())));
    for interactive in [false, true] {
        let plan = context(&setup, &Options::mode(PermissionMode::Plan, interactive));
        assert!(support::run(
            &registry,
            &plan,
            support::call("web_fetch", json!({"url":"https://example.com"}))
        )
        .refusal()
        .is_some());
    }
    // Precedence: the user's deny and ask rules keep their own reasons ahead of
    // the unattended-input prohibition, in bypass and in the other modes.
    for (rules, reason) in [
        (Rules::deny(&["browser"]), "UserRuleDeny"),
        (Rules::ask(&["browser"]), "UserRuleAsk"),
    ] {
        for mode in [PermissionMode::BypassPermissions, PermissionMode::Default] {
            let ruled = context(&setup, &Options::mode(mode, false).rules(rules.clone()));
            let report = support::run_report(
                &registry,
                &ruled,
                support::call("browser", arguments("press")),
            );
            assert!(report.executed_call_ids.is_empty());
            assert!(
                report.messages[0].content.contains(reason),
                "{mode:?} {reason}: {:?}",
                report.messages
            );
        }
    }
    for mode in [PermissionMode::Default, PermissionMode::DontAsk] {
        let unattended = context(&setup, &Options::mode(mode, false));
        let report = support::run_report(
            &registry,
            &unattended,
            support::call("browser", arguments("press")),
        );
        assert!(
            report.messages[0].content.contains("ModeBaselineDeny"),
            "{mode:?}: {:?}",
            report.messages
        );
    }
    // An unreadable remembered-permission store asks only where someone can
    // answer: unattended browser input is refused with that reason instead.
    for (mode, interactive) in [
        (PermissionMode::DontAsk, true),
        (PermissionMode::Default, false),
    ] {
        let mut unavailable = context(&setup, &Options::mode(mode, interactive));
        unavailable.project_permission_store = Some(ProjectPermissionStoreConfig {
            store_path: setup.data.join("remembered-permissions.json"),
            workspace_id: WorkspacePermissionId::from_canonical_workspace_path("/missing"),
        });
        unavailable.active_workspace = None;
        let report = support::run_report(
            &registry,
            &unavailable,
            support::call("browser", arguments("press")),
        );
        assert!(report.executed_call_ids.is_empty() && report.interrupt.is_none());
        assert!(
            report.messages[0]
                .content
                .contains("RememberedStoreUnavailable"),
            "{mode:?}: {:?}",
            report.messages
        );
    }
    for mode in ALL_MODES
        .into_iter()
        .filter(|mode| *mode != PermissionMode::BypassPermissions)
    {
        let allowed = context(
            &setup,
            &Options::mode(mode, false)
                .rules(Rules::allow(&["browser"]))
                .verdict(Verdict::Allow),
        );
        assert!(support::run_report(
            &registry,
            &allowed,
            support::call("browser", arguments("press"))
        )
        .executed_call_ids
        .is_empty());
    }
    let mut net_only = context(
        &setup,
        &Options::mode(PermissionMode::BypassPermissions, false),
    );
    net_only
        .permission_ceiling_snapshot
        .as_mut()
        .ok_or("ceiling")?
        .capability_ceiling = vec![SafetyCapability::NetOutbound];
    let ceiling = support::run_report(
        &registry,
        &net_only,
        support::call("browser", arguments("press")),
    );
    assert!(ceiling.executed_call_ids.is_empty());
    assert!(
        ceiling.messages[0].content.contains("CeilingViolation"),
        "{:?}",
        ceiling.messages
    );
    let bad = support::run_report(
        &registry,
        &net_only,
        support::call("browser", arguments("invalid")),
    );
    assert!(!bad.permissioned_actions[0].normalization_errors.is_empty());
    assert!(
        bad.messages[0]
            .content
            .contains("browser_invalid_arguments: unknown action"),
        "{:?}",
        bad.messages
    );

    // The real loop must not even ask the classifier for forbidden input modes.
    for (mode, interactive, replies, expected_inputs, expected_requests) in [
        (PermissionMode::Auto, true, vec![Ok("no")], 1, 1),
        (PermissionMode::Auto, true, vec![Ok("yes"), Ok(BLOCK)], 0, 2),
        (PermissionMode::Auto, true, vec![Err("unavailable")], 0, 1),
        (PermissionMode::Auto, false, vec![], 0, 0),
        (PermissionMode::DontAsk, true, vec![], 0, 0),
        (PermissionMode::BypassPermissions, false, vec![], 1, 0),
    ] {
        let setup = Setup::new()?;
        let (mut registry, _) = setup.registry(Exec::Gated);
        let host = Host::default();
        registry.register(BrowserTool::new(Arc::new(host.clone())));
        let provider = RoutingProvider::new(
            vec![
                calls(vec![tool_call("press", "browser", arguments("press"))]),
                answer("done"),
            ],
            replies,
        );
        let mut config = agent_loop_config(&setup, interactive);
        config.permission_mode_snapshot.mode = mode;
        config
            .permission_ceiling_snapshot
            .as_mut()
            .ok_or("ceiling")?
            .parent_mode = mode;
        let mut agent = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(&setup.workspace)?,
            ContextBuilder::new(&setup.workspace),
            &registry,
            &provider,
            config,
        );
        agent.process_direct("press Enter", Some("browser-matrix"))?;
        assert_eq!(host.0.lock().expect("state").inputs, expected_inputs);
        assert_eq!(provider.classifier_requests().len(), expected_requests);
    }
    Ok(())
}

#[test]
fn action_description_is_bound_redacted_and_rechecked() -> TestResult {
    for bridge in [false, true] {
        let setup = Setup::new()?;
        fs::write(
            setup.data.join(".env.json"),
            r#"{"BROWSER_SECRET":"private-browser-value"}"#,
        )?;
        let (mut registry, _) = setup.registry(Exec::Gated);
        let host = Host::default();
        host.0.lock().expect("state").description = json!({
            "name":"Search private-browser-value",
            "form":{"name":"Search form","owner":42,"url":"https://example.com/submit?token=super-secret-token","method":"post"},
            "submitter":{"node":43,"formaction":"/submit","formmethod":"post","formtarget":"_self"},
            "password":"must-not-leak"
        });
        registry.register(BrowserTool::new(Arc::new(host.clone())));
        let call = if bridge {
            tool_call(
                "bound",
                "tool_call",
                json!({"name":"browser","arguments":arguments("type")}),
            )
        } else {
            tool_call("bound", "browser", arguments("type"))
        };
        let provider =
            RoutingProvider::new(vec![calls(vec![call]), answer("done")], vec![Ok("no")]);
        let mut config = agent_loop_config(&setup, true);
        if bridge {
            config.tool_search.enabled = ToolSearchMode::On;
        }
        let mut agent = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(&setup.workspace)?,
            ContextBuilder::new(&setup.workspace),
            &registry,
            &provider,
            config,
        );
        agent.process_direct("search for hello", Some("bound-browser"))?;
        let payloads = provider.payloads();
        let shown = payloads.first().ok_or("classifier payload")?.to_string();
        for text in [
            "example.com",
            "textbox",
            "Search",
            "hello",
            "Search form",
            "post",
            "formaction",
        ] {
            assert!(shown.contains(text), "{text}: {shown}");
        }
        for secret in [
            "private-browser-value",
            "must-not-leak",
            "super-secret-token",
        ] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert!(shown.contains("[secret BROWSER_SECRET]"));
        assert_eq!(host.0.lock().expect("state").inputs, 1);
    }
    let setup = Setup::new()?;
    fs::write(
        setup.data.join(".env.json"),
        r#"{"FIRST":"private-first-value","SECOND":"private-second-value"}"#,
    )?;
    for mutation in [
        json!({"origin":"https://elsewhere.example"}),
        json!({"loader":"new-loader"}),
        json!({"name":"Different element"}),
        json!({"form":{"action":"/changed"}}),
        json!({"form":{"method":"post"}}),
        json!({"form":{"owner":99}}),
        json!({"submitter":{"formaction":"/other"}}),
        json!({"submitter":{"formmethod":"get"}}),
        json!({"form":{"base_url":"https://other.example/"}}),
        json!({"focus":{"node":9,"frame":"child"}}),
    ] {
        let (mut registry, _) = setup.registry(Exec::Gated);
        let host = Host::default();
        registry.register(BrowserTool::new(Arc::new(host.clone())));
        let context = context(&setup, &Options::mode(PermissionMode::Default, true));
        let call = support::call("browser", arguments("press"));
        let first = support::run(&registry, &context, call.clone());
        let question = first.question().ok_or("approval question")?;
        assert!(question.text.contains("example.com") && question.text.contains("Search"));
        let mut approved = context;
        approved.permission_approval_cache =
            Some(question.approval(ApprovalDecisionKind::Approved));
        host.0.lock().expect("state").mutate_on_execute = Some(mutation);
        let result = support::run_report(&registry, &approved, call);
        assert!(
            result.messages[0]
                .content
                .contains("browser_stale_revision"),
            "{result:?}"
        );
        assert_eq!(host.0.lock().expect("state").inputs, 0);
    }
    let (mut registry, _) = setup.registry(Exec::Gated);
    registry.register(BrowserTool::new(Arc::new(Host::default())));
    let context = context(&setup, &Options::mode(PermissionMode::Default, true));
    let mut first = arguments("type");
    first["text"] = json!("private-first-value");
    let original = support::run(&registry, &context, support::call("browser", first));
    let question = original.question().ok_or("question")?;
    assert!(question.once_only());
    let mut approved = context;
    approved.permission_approval_cache = Some(question.approval(ApprovalDecisionKind::Approved));
    let mut changed = arguments("type");
    changed["text"] = json!("private-second-value");
    assert!(
        support::run_report(&registry, &approved, support::call("browser", changed))
            .executed_call_ids
            .is_empty()
    );
    Ok(())
}

#[test]
fn browser_domain_rules_select_the_actual_origin() -> TestResult {
    let setup = Setup::new()?;
    for (rule, tool, args, origin, expected) in [
        (
            "browser(domain:EXAMPLE.COM.)",
            "browser",
            arguments("look"),
            "https://example.com",
            true,
        ),
        (
            "browser(domain:*.example.com)",
            "browser",
            arguments("look"),
            "https://sub.example.com",
            true,
        ),
        (
            "browser(domain:*.example.com)",
            "browser",
            arguments("look"),
            "https://example.com",
            false,
        ),
        (
            "browser(domain:example.com)",
            "browser",
            json!({"action":"open","url":"https://other.example"}),
            "https://example.com",
            false,
        ),
        (
            "browser(domain:example.com)",
            "web_fetch",
            json!({"url":"https://example.com"}),
            "https://example.com",
            false,
        ),
        (
            "web_fetch(domain:example.com)",
            "browser",
            arguments("look"),
            "https://example.com",
            false,
        ),
        ("browser", "browser", arguments("close"), "", true),
        (
            "browser(domain:example.com)",
            "browser",
            arguments("close"),
            "",
            false,
        ),
    ] {
        for effect in ["deny", "ask", "allow"] {
            let (mut registry, _) = setup.registry(Exec::Gated);
            let host = Host::default();
            if args["action"] != "open" {
                host.0.lock().expect("state").description =
                    json!({"origin":if origin.is_empty(){Value::Null}else{json!(origin)}});
            }
            registry.register(BrowserTool::new(Arc::new(host)));
            let rules = match effect {
                "deny" => Rules::deny(&[rule]),
                "ask" => Rules::ask(&[rule]),
                _ => Rules::allow(&[rule]),
            };
            let mode = if effect == "allow" {
                PermissionMode::Default
            } else {
                PermissionMode::BypassPermissions
            };
            let ctx = context(&setup, &Options::mode(mode, true).rules(rules));
            let result = support::run_report(&registry, &ctx, support::call(tool, args.clone()));
            if effect == "deny" {
                assert_eq!(
                    result.executed_call_ids.is_empty(),
                    expected,
                    "{rule} {tool} {result:?}"
                );
            } else if effect == "ask" {
                assert_eq!(result.interrupt.is_some(), expected, "{rule} {tool}");
            } else {
                assert_eq!(
                    !result.executed_call_ids.is_empty(),
                    expected,
                    "{rule} {tool} {result:?}"
                );
            }
        }
    }
    let rules = Rules {
        deny: vec!["browser(domain:example.com)".to_owned()],
        ask: vec!["browser".to_owned()],
        allow: vec!["browser".to_owned()],
        ..Rules::default()
    };
    let (mut registry, _) = setup.registry(Exec::Gated);
    registry.register(BrowserTool::new(Arc::new(Host::default())));
    let result = support::run_report(
        &registry,
        &context(
            &setup,
            &Options::mode(PermissionMode::BypassPermissions, true).rules(rules),
        ),
        support::call("browser", arguments("look")),
    );
    assert!(result.interrupt.is_none() && result.executed_call_ids.is_empty());
    Ok(())
}

#[test]
fn commitment_rule_uses_only_explicit_user_intent() -> TestResult {
    let line="[Browser Commitment] In the browser, submitting or confirming a purchase, booking, payment, message, post, or account-setting change the user did not ask for.";
    let defaults = AutoModeSlots::default();
    assert_eq!(defaults.soft_deny.len(), 8);
    assert_eq!(defaults.soft_deny.last().map(String::as_str), Some(line));
    let expanded = AutoModeSlots::resolve(SlotOverrides {
        soft_deny: Some(&["$defaults".to_owned()]),
        ..SlotOverrides::default()
    });
    assert_eq!(expanded.soft_deny, defaults.soft_deny);
    let setup = Setup::new()?;
    let (mut registry, _) = setup.registry(Exec::Gated);
    let host = Host::default();
    registry.register(BrowserTool::new(Arc::new(host.clone())));
    let block =
        r#"{"decision":"block","rule":"[Browser Commitment]","reason":"purchase not requested"}"#;
    let provider = RoutingProvider::new(
        vec![
            calls(vec![tool_call("purchase1", "browser", arguments("click"))]),
            answer("purchase confirmed by assistant"),
            calls(vec![tool_call("purchase2", "browser", arguments("click"))]),
            answer("done"),
        ],
        vec![Ok("yes"), Ok(block), Ok("no")],
    );
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&setup.workspace)?,
        ContextBuilder::new(&setup.workspace),
        &registry,
        &provider,
        agent_loop_config(&setup, true),
    );
    agent.process_direct("show the price", Some("purchase"))?;
    assert_eq!(host.0.lock().expect("state").inputs, 0);
    agent.process_direct("Buy this exact item now", Some("purchase"))?;
    assert_eq!(host.0.lock().expect("state").inputs, 1);
    let payloads = provider.payloads();
    let earlier = payloads[0]["transcript"].to_string();
    let later = payloads.last().ok_or("payload")?["transcript"].to_string();
    assert!(!earlier.contains("Buy this exact item now"));
    assert!(later.contains("Buy this exact item now"));
    assert!(
        !later.contains("purchase confirmed by assistant") && !later.contains("Visible result")
    );
    Ok(())
}
