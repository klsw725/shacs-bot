use super::super::*;
use shacs_core::generated_media::ArtifactStore;

pub(crate) struct ProductionTooling {
    pub(crate) registry: ToolRegistry,
    pub(crate) message_tool: Option<MessageTool>,
    pub(crate) mcp_runtime: Option<McpRuntime>,
    pub(crate) mcp_reports: Vec<McpServerConnectionReport>,
}

pub(crate) fn production_tool_registry(
    bundle: &ConfigBundle,
    allow_side_effect_tools: bool,
    spec030_facts: &shacs_core::runtime::trusted_runtime::Spec030FactStore,
) -> Result<ProductionTooling, CliError> {
    let workspace = &bundle.context.workspace;
    fs::create_dir_all(workspace)?;
    let media_dir = bundle.context.media_dir(Some("api"));
    fs::create_dir_all(&media_dir)?;
    let path_context = PathContext {
        workspace: Some(workspace.clone()),
        allowed_dir: Some(workspace.clone()),
        media_dir: Some(media_dir),
        extra_allowed_dirs: Vec::new(),
    };
    let file_state = Arc::new(Mutex::new(FileState::new()));
    let mut registry = ToolRegistry::new();
    let cron_service = Arc::new(
        PersistentCronService::new(bundle.context.runtime_subdir("cron").join("jobs.json"))
            .map_err(CliError::Io)?,
    );
    registry.register(CronTool::with_timezone(
        cron_service.clone(),
        bundle.config.agents.defaults.timezone.clone(),
    ));
    registry.register(ReadFileTool::with_file_state(
        path_context.clone(),
        file_state.clone(),
    ));
    if allow_side_effect_tools {
        registry.register(WriteFileTool::with_file_state(
            path_context.clone(),
            file_state.clone(),
        ));
        registry.register(EditFileTool::with_file_state(
            path_context.clone(),
            file_state,
        ));
    }
    registry.register(ListDirTool::new(path_context.clone()));
    registry.register(GlobTool::new(path_context.clone()));
    registry.register(GrepTool::new(path_context.clone()));
    let message_tool = if allow_side_effect_tools {
        let tool = MessageTool::new(workspace).with_media_roots([bundle.context.media_dir(None)]);
        registry.register(tool.clone());
        Some(tool)
    } else {
        None
    };
    if allow_side_effect_tools && bundle.config.tools.exec.enable {
        let mut exec_config = ExecConfig::new(path_context.clone());
        exec_config.network_guard = NetworkGuard::with_ssrf_whitelist(
            bundle
                .config
                .tools
                .ssrf_whitelist
                .iter()
                .map(String::as_str),
        );
        exec_config.timeout_seconds = u64::from(bundle.config.tools.exec.timeout);
        exec_config.restrict_to_workspace = bundle.config.tools.restrict_to_workspace;
        exec_config.sandbox = non_empty(Some(bundle.config.tools.exec.sandbox.as_str()))
            .then(|| bundle.config.tools.exec.sandbox.clone());
        exec_config.apply_sandbox_policy(&bundle.config.tools.exec.sandbox_policy, workspace);
        exec_config.path_append = non_empty(Some(bundle.config.tools.exec.path_append.as_str()))
            .then(|| bundle.config.tools.exec.path_append.clone());
        exec_config.allowed_env_keys = bundle.config.tools.exec.allowed_env_keys.clone();
        exec_config.env = configured_exec_env(&bundle.config);
        registry
            .register(ExecTool::new(exec_config).with_spec030_fact_store(spec030_facts.clone()));
    }
    if allow_side_effect_tools && bundle.config.tools.image_generation.enable {
        let image_config = &bundle.config.tools.image_generation;
        let provider_registry = ProviderRegistry::new();
        let mut providers = bundle.config.providers.clone();
        for (provider_id, auth) in load_auth_store(&bundle.context.auth_path())?.providers {
            if auth.kind == "apiKey" {
                providers.entry(provider_id).or_default().api_key = Some(auth.access);
            }
        }
        let resolved = resolve_image_generation_client(
            &provider_registry,
            &image_config.provider,
            &image_config.model,
            &providers,
        )
        .map_err(|error| {
            CliError::Config(ConfigError::Env(format!(
                "image_generate provider could not be configured: {}",
                render_image_generation_provider_error(error)
            )))
        })?;
        let image_media_dir = bundle.context.media_dir(Some("image-generation"));
        fs::create_dir_all(&image_media_dir)?;
        let codex_native_media = resolved.provider_id == "openai_codex";
        let mut image_tool = ImageGenerateTool::new(
            resolved.client,
            image_media_dir,
            ImageGenerateToolConfig {
                provider_id: resolved.provider_id,
                model_id: resolved.model,
                default_format: image_config.default_format.clone(),
                max_count: image_config.max_count,
                max_bytes: image_config.max_bytes,
            },
        );
        if codex_native_media {
            let artifact_store =
                ArtifactStore::open(bundle.context.media_dir(None)).map_err(|error| {
                    CliError::Config(ConfigError::Env(format!(
                        "image_generate artifact store could not be configured: {error}"
                    )))
                })?;
            image_tool = image_tool.with_artifact_store(artifact_store);
        }
        registry.register(image_tool);
    }
    if bundle.config.tools.web.enable {
        let network_guard = NetworkGuard::with_ssrf_whitelist(
            bundle
                .config
                .tools
                .ssrf_whitelist
                .iter()
                .map(String::as_str),
        );
        let user_agent = bundle
            .config
            .tools
            .web
            .user_agent
            .clone()
            .unwrap_or_else(|| "Mozilla/5.0 (shacs-bot)".to_owned());
        registry.register(WebFetchTool::with_config(
            WebFetchConfig {
                user_agent: user_agent.clone(),
                network_guard: network_guard.clone(),
                ..WebFetchConfig::default()
            },
            Arc::new(shacs_core::tools::UreqWebClient),
        ));
        registry.register(WebSearchTool::new(WebSearchConfig {
            provider: bundle.config.tools.web.search.provider.clone(),
            api_key: bundle.config.tools.web.search.api_key.clone(),
            base_url: bundle.config.tools.web.search.base_url.clone(),
            max_results: bundle.config.tools.web.search.max_results as usize,
            timeout: Duration::from_secs(u64::from(bundle.config.tools.web.search.timeout)),
            user_agent,
            network_guard,
        }));
    }
    registry.register(AskUserTool::new());
    registry.register(SelfTool::with_modify_allowed(
        Arc::new(Mutex::new(SelfRuntimeState::new())),
        allow_side_effect_tools && bundle.config.tools.my.allow_set,
    ));
    let plugin_discovery = if allow_side_effect_tools {
        Some(discover_plugins(
            &bundle.config,
            &bundle.context,
            &ProcessEnv,
        )?)
    } else {
        None
    };
    if let Some(discovery) = &plugin_discovery {
        let _diagnostics = register_plugin_runtime_tools(&mut registry, &discovery.plugins);
    }
    let specs = production_mcp_server_specs(
        bundle,
        plugin_discovery
            .as_ref()
            .map(|discovery| discovery.plugins.as_slice())
            .unwrap_or(&[]),
    )?;
    let (mcp_runtime, mcp_reports) = if specs.is_empty() {
        (None, Vec::new())
    } else {
        let runtime = McpRuntime::new(Some(Arc::new(StdioMcpConnector::new())));
        let reports = runtime.connect_and_register(&mut registry, &specs);
        (Some(runtime), reports)
    };
    Ok(ProductionTooling {
        registry,
        message_tool,
        mcp_runtime,
        mcp_reports,
    })
}
