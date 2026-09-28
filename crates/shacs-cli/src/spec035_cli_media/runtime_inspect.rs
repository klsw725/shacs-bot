use super::super::*;

pub(crate) fn runtime_inspect_inner(
    options: RuntimeInspectOptions,
    ensure_dirs: bool,
) -> Result<RuntimeInspectReport, CliError> {
    let owner = shacs_api::observe_trusted_runtime(
        options.config_path.clone(),
        options.workspace_override.clone(),
    )
    .projection;
    runtime_inspect_with_owner(options, ensure_dirs, owner)
}

pub(crate) fn runtime_inspect_with_owner(
    options: RuntimeInspectOptions,
    ensure_dirs: bool,
    owner: shacs_projection::Spec030RuntimeProjection,
) -> Result<RuntimeInspectReport, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let config_exists = config_path.exists();
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    apply_api_key_auth_overlay(&mut bundle)?;
    if ensure_dirs {
        let _runtime_dirs = ensure_runtime_dirs(&bundle.context)?;
    }
    let workspace = bundle.context.workspace.clone();
    let workspace_exists = workspace.exists();
    let mut providers = bundle
        .config
        .providers
        .iter()
        .map(|(name, config)| ProviderStatus {
            name: name.clone(),
            has_api_key: non_empty(config.api_key.as_deref()),
            has_api_base: non_empty(config.api_base.as_deref()),
        })
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    let generated_media =
        inspect_generated_media(&bundle.context.media_dir(Some("image-generation")))?;
    let media_projections =
        shacs_core::runtime::Spec035MediaProjectionStore::new(&bundle.context.data_dir)
            .read()
            .map_err(|_| {
                CliError::Runtime(
                    "Spec035 media projection unavailable: invalid canonical record".to_owned(),
                )
            })?
            .into_iter()
            .collect();
    let capabilities = runtime_capabilities(&bundle);
    let sessions = inspect_runtime_sessions(&workspace)?;
    let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
    let update_marker = read_runtime_update_marker(&marker_path)?;
    let ownership = inspect_runtime_ownership(&bundle.context.data_dir, now_millis())?;
    let stop_request = read_runtime_stop_request_marker(&runtime_stop_request_marker_path(
        &bundle.context.data_dir,
    ))?;
    let compatibility = evaluate_runtime_compatibility(RUNTIME_DATA_SCHEMA_VERSION);
    let migration_plan = plan_durable_migration_for_roots(
        &bundle.context.data_dir,
        &bundle.context.workspace,
        DurableConfigCompatibility::Readable,
    )
    .map_err(|error| CliError::InvalidArguments(redact_string(&error.to_string())))?;
    let migration_ledger = inspect_durable_migration_ledger(&bundle.context.data_dir);
    let durable_recovery = evaluate_runtime_durable_recovery(&bundle.context.data_dir);
    let durable_work = evaluate_runtime_durable_work(&bundle.context.data_dir, &durable_recovery);
    let durable_children =
        durable_child_inspect(durable_recovery.state.as_ref().map(|state| &state.children));
    let durable_diagnostics = inspect_durable_diagnostics(&bundle.context.data_dir);
    let supervision = read_runtime_supervision_state(&bundle.context.data_dir)?;
    let channel_restart = inspect_channel_restart_states(
        &bundle.context.data_dir,
        durable_recovery.state.as_ref().map(|state| &state.work),
    );
    let containment = runtime_containment_inspect(&bundle);
    let plugin_app_readiness =
        spec031_cli::readiness_plugin_app::observe(&bundle).map_err(CliError::Runtime)?;
    let workflow_recipes = workflow_recipes_for_bundle(&bundle)?;
    let trusted_runtime = shacs_core::runtime::trusted_runtime::build_trusted_runtime_projection(
        spec030_fact_store_for_bundle(&bundle)
            .snapshot()
            .into_input(),
    )
    .map_err(|error| CliError::Runtime(error.to_string()))?;
    let spec035_revised = shacs_projection::project_spec035_revised_owner_facts(
        shacs_projection::Spec035RevisedOwnerFacts::new(
            shacs_projection::Spec035OwnerSurface::Cli,
            &trusted_runtime,
        ),
    )
    .map_err(|error| CliError::Runtime(error.to_string()))?;

    Ok(RuntimeInspectReport {
        config_path,
        config_exists,
        workspace,
        workspace_exists,
        data_dir: bundle.context.data_dir,
        model: bundle.config.agents.defaults.model,
        provider: bundle.config.agents.defaults.provider,
        providers,
        generated_media,
        media_projections,
        spec035_revised,
        capabilities,
        sessions,
        lifecycle: RuntimeLifecycleInspect {
            binary_version: VERSION.to_owned(),
            data_schema_version: RUNTIME_DATA_SCHEMA_VERSION,
            data_schema_min_version: RUNTIME_DATA_SCHEMA_MIN_VERSION,
            compatibility,
            ownership,
            stop_request,
            update_marker,
            migration_plan,
            migration_ledger,
            durable_recovery,
            durable_work,
            durable_children,
            durable_diagnostics,
        },
        supervision,
        channel_restart,
        plugin_app_readiness,
        trusted_runtime: owner,
        containment,
        workflow_recipes,
    })
}
