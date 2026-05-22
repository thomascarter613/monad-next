mod artifacts;
mod cli;
mod errors;
mod init;
mod linear_notify;
mod login;
mod mcp;
mod migrate;
mod plan_print;
mod plugins;
mod prime;
mod release;
mod run_print;
mod scaffold;
mod schema;
mod secret;
mod slack_post;
mod style;
mod toolchain;
mod why;

use std::path::PathBuf;

use anyhow::Context as _;
use clap::Parser;
use tracing_subscriber::{fmt, EnvFilter};

use monad_core::{
    ci_at, notify_at, plan_at, resolve_target, CiOptions, Executor, IntegrationRegistry,
    IntegrationTaskKind, LocalCache, PlanOptions, TargetRef, Workspace,
};

use cli::{
    BoxAction, CacheAction, Cli, Command, UnitAction, GlobalFlags, McpAction, MigrateSource,
};

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.global.verbose);
    let as_json = cli.global.json;
    let exit_code = match run(cli) {
        Ok(code) => code,
        Err(err) => {
            errors::emit(&err, as_json);
            1
        }
    };
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn init_tracing(verbose: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(if verbose { "debug" } else { "info" }));
    // Route tracing to stderr so `monad ... --json` stdout stays
    // parseable. `tracing_subscriber::fmt()` defaults to stdout,
    // which silently corrupts machine-readable output for any
    // JSON-emitting verb — a bug surfaced by the e2e harness.
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .with_writer(std::io::stderr)
        .init();
}

fn run(cli: Cli) -> anyhow::Result<i32> {
    match cli.command {
        Command::Init { no_detect } => return run_init(&cli.global, no_detect),
        Command::Migrate(source) => return run_migrate(&cli.global, source),
        Command::Artifacts => return artifacts::run(&cli.global),
        Command::Unit(UnitAction::Add { path, lang }) => {
            return run_unit_add(&cli.global, path, lang);
        }
        Command::Unit(UnitAction::List) => return run_unit_list(&cli.global),
        Command::Box(BoxAction::Add { name }) => return run_box_add(&cli.global, name),
        Command::Box(BoxAction::List) => return run_box_list(&cli.global),
        Command::Prime => return prime::run(&cli.global),
        Command::Plan { target } => run_plan(&cli.global, target)?,
        Command::Ci => return run_ci(&cli.global),
        Command::Install { target, force } => return run_install(&cli.global, target, force),
        Command::Build { target } => return run_task_command(&cli.global, "build", target),
        Command::Check { target } => return run_task_command(&cli.global, "check", target),
        Command::Test { target } => return run_task_command(&cli.global, "test", target),
        Command::Lint { target } => return run_task_command(&cli.global, "lint", target),
        Command::Deploy {
            target,
            preview,
            rollback,
            env,
            secret_from,
            no_notify,
            force,
        } => {
            return run_deploy(
                &cli.global,
                target,
                preview,
                rollback,
                env,
                secret_from,
                no_notify,
                force,
            );
        }
        Command::Notify {
            target,
            env,
            secret_from,
        } => return run_notify(&cli.global, target, env, secret_from),
        Command::Serve { monad } => return run_serve(&cli.global, monad),
        Command::Dev { unit } => return run_dev(&cli.global, unit),
        Command::Run { unit, task, args } => return run_task(&cli.global, unit, task, args),
        Command::Add {
            packages,
            unit,
            dev,
        } => return run_add(&cli.global, packages, unit, dev),
        Command::Cache(action) => return run_cache(&cli.global, action),
        Command::Secret(action) => return secret::run(&cli.global, action),
        Command::Why { target } => return run_why(&cli.global, &target),
        Command::Graph { monad, format } => return run_graph(&cli.global, monad, format),
        Command::Doctor {
            env,
            secret_from,
            cloud,
        } => {
            return run_doctor(&cli.global, env, secret_from, cloud);
        }
        Command::Schema { target } => return schema::run(cli.global.json, target),
        Command::Mcp(McpAction::Install {
            client,
            local,
            workspace,
            name,
        }) => return mcp::run(cli.global.json, client, local, workspace, name),
        Command::Toolchain(action) => return toolchain::run(&cli.global, action),
        Command::Release { spec } => return release::run(&spec),
        Command::Login => return login::run(),
        Command::SlackPost {
            webhook_env,
            channel,
            username,
        } => match slack_post::run(&webhook_env, channel.as_deref(), username.as_deref()) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                eprintln!("monad: slack post failed: {e:#}");
                std::process::exit(1);
            }
        },
        Command::LinearNotify {
            api_key_env,
            target_state,
            fallback_issue_id,
            team,
        } => {
            match linear_notify::run(
                &api_key_env,
                &target_state,
                fallback_issue_id.as_deref(),
                team.as_deref(),
            ) {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    eprintln!("monad: linear notify failed: {e:#}");
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(0)
}

/// Resolve the workspace root honouring the global `--workspace` flag
/// (which also honours `$MONAD_WORKSPACE_ROOT` via clap's `env` attr).
/// Falls back to walking upward from the current directory.
///
/// Callers that want to write *into* a monad-free directory (like
/// `monad init`) should use `current_dir` directly — this helper
/// requires the target already be inside a workspace.
pub(crate) fn resolve_workspace_root(global: &GlobalFlags) -> anyhow::Result<std::path::PathBuf> {
    let start = match &global.workspace {
        Some(p) => p.clone(),
        None => std::env::current_dir()?,
    };
    monad_core::find_workspace_root(&start)
}

fn run_plan(global: &GlobalFlags, target: Option<String>) -> anyhow::Result<()> {
    let root = resolve_workspace_root(global)?;

    // Start from any --monad given via the global flag …
    let mut monad_filter = global.monad.clone();
    let mut unit_filter: Option<String> = None;

    // … then let the positional target override / add. Resolves
    // against the workspace so unknown names hit the classified
    // `target_not_found` envelope with next_steps instead of a
    // terse clap error.
    if let Some(t) = target {
        let workspace = Workspace::load(&root)?;
        match resolve_target(&workspace, &t)? {
            TargetRef::Monad(name) => monad_filter = Some(name),
            TargetRef::Unit(name) => unit_filter = Some(name),
        }
    }

    let opts = PlanOptions {
        monad_filter,
        unit_filter,
        no_cache: global.no_cache,
        since: global.since.clone(),
    };
    let plan = plan_at(&root, &opts)?;
    if global.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        plan_print::print_human(&plan);
    }
    Ok(())
}

fn run_ci(global: &GlobalFlags) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;

    // Capture cache + telemetry config up front so we still have them
    // after `ci_at` consumes its own internal Workspace load. Two TOML
    // loads is ~10ms — fine for a CLI; alternative is widening
    // `ci_at`'s public signature, which isn't worth it for telemetry
    // plumbing.
    let (cache_remote, cache_token_env, telemetry_enabled) = match Workspace::load(&root) {
        Ok(w) => (
            w.repo.cache.remote.clone(),
            w.repo.cache.remote_token_env.clone(),
            w.repo.telemetry.enabled,
        ),
        // Conservative default: if config fails to load, treat
        // telemetry as off. We can't have meaningfully consented to
        // sending data when we couldn't even read the consent flag.
        Err(_) => (None, None, false),
    };

    let opts = CiOptions {
        monad_filter: global.monad.clone(),
        unit_filter: None,
        task_filter: None,
        no_cache: global.no_cache,
        fail_fast: None,
        skip_install: global.skip_install,
        force_install: global.force_install,
        task_kind_filter: None,
        install_only: false,
        secret_aliases: std::collections::BTreeMap::new(),
        run_notify_kinds: false,
        environment: None,
        force_deploy: false,
    };
    let report = ci_at(&root, &opts)?;
    emit_report(&report, global.json, global.report_file.as_deref())?;

    // Best-effort build-report POST to the configured `monad://`
    // remote. Always after emit_report so user-visible output isn't
    // gated on telemetry.
    let package = global.monad.clone().unwrap_or_else(|| "all".to_string());
    monad_core::report::send(
        telemetry_enabled,
        cache_remote.as_deref(),
        cache_token_env.as_deref(),
        &report,
        package,
    );

    Ok(
        if report.summary.failed > 0 || report.summary.install_failures > 0 {
            1
        } else {
            0
        },
    )
}

/// Drive `monad build|test|lint [target]` — a targeted variant of `monad ci`.
fn run_task_command(
    global: &GlobalFlags,
    task: &'static str,
    target: Option<String>,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = Workspace::load(&root)?;

    // Start from any --monad given via the global flag …
    let mut monad_filter = global.monad.clone();
    let mut unit_filter: Option<String> = None;

    // … then let the positional target override / add.
    if let Some(t) = target {
        match resolve_target(&workspace, &t)? {
            TargetRef::Monad(name) => monad_filter = Some(name),
            TargetRef::Unit(name) => unit_filter = Some(name),
        }
    }

    let opts = CiOptions {
        monad_filter: monad_filter.clone(),
        unit_filter: unit_filter.clone(),
        task_filter: Some(vec![task.to_string()]),
        no_cache: global.no_cache,
        fail_fast: None,
        skip_install: global.skip_install,
        force_install: global.force_install,
        task_kind_filter: None,
        install_only: false,
        secret_aliases: std::collections::BTreeMap::new(),
        run_notify_kinds: false,
        environment: None,
        force_deploy: false,
    };

    // Capture cache + telemetry config before the workspace moves into
    // Executor so we can fire the build report after the run.
    let cache_remote = workspace.repo.cache.remote.clone();
    let cache_token_env = workspace.repo.cache.remote_token_env.clone();
    let telemetry_enabled = workspace.repo.telemetry.enabled;

    // Run with the pre-loaded workspace to avoid a second TOML pass.
    let registry = plugins::build_registry(&workspace);
    let cache = LocalCache::new(monad_core::default_cache_root()?);
    let report = Executor::new(workspace, registry, cache).execute(&opts)?;

    emit_report(&report, global.json, global.report_file.as_deref())?;

    // Best-effort build-report POST. Only fires for `monad build` —
    // test/lint runs go to the same code path but aren't "builds" in
    // dashboard terms (the recent-builds table would get noisy).
    if task == "build" {
        let package = monad_filter
            .or(unit_filter)
            .unwrap_or_else(|| "all".to_string());
        monad_core::report::send(
            telemetry_enabled,
            cache_remote.as_deref(),
            cache_token_env.as_deref(),
            &report,
            package,
        );
    }

    Ok(
        if report.summary.failed > 0 || report.summary.install_failures > 0 {
            1
        } else {
            0
        },
    )
}

/// Drive `monad install [target] [--force]` — runs each unit's
/// adapter install command (`npm ci` / `go mod download` / …) so
/// agents don't need to know which invocation goes with which unit.
fn run_install(global: &GlobalFlags, target: Option<String>, force: bool) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = Workspace::load(&root)?;

    let mut monad_filter = global.monad.clone();
    let mut unit_filter: Option<String> = None;
    if let Some(t) = target {
        match resolve_target(&workspace, &t)? {
            TargetRef::Monad(name) => monad_filter = Some(name),
            TargetRef::Unit(name) => unit_filter = Some(name),
        }
    }

    let opts = CiOptions {
        monad_filter,
        unit_filter,
        task_filter: None,
        no_cache: false,
        fail_fast: None,
        // install is the whole point here — --skip-install would be nonsense.
        skip_install: false,
        force_install: force,
        task_kind_filter: None,
        install_only: true,
        secret_aliases: std::collections::BTreeMap::new(),
        run_notify_kinds: false,
        environment: None,
        force_deploy: false,
    };

    let registry = plugins::build_registry(&workspace);
    let integrations = IntegrationRegistry::builtin();
    let cache = LocalCache::new(monad_core::default_cache_root()?);
    let report = Executor::new(workspace, registry, cache)
        .with_integrations(integrations)
        .execute(&opts)?;

    emit_report(&report, global.json, global.report_file.as_deref())?;
    Ok(if report.summary.install_failures > 0 {
        1
    } else {
        0
    })
}

/// Drive `monad deploy [target] [--preview|--rollback]` — runs
/// integration-emitted tasks of the selected kind (Deploy by default)
/// on units that have a matching integration wired up. Build is
/// included as a prerequisite so deploys never ship stale artefacts.
// 8 args is intentional: this is the CLI flag forwarder for
// `monad deploy`. Bundling them into a struct would just push the
// noise one layer up without making any caller clearer.
#[allow(clippy::too_many_arguments)]
fn run_deploy(
    global: &GlobalFlags,
    target: Option<String>,
    preview: bool,
    rollback: bool,
    env: Option<String>,
    secret_from: Vec<(String, String)>,
    no_notify: bool,
    force: bool,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = Workspace::load(&root)?;

    let kind = if rollback {
        IntegrationTaskKind::Rollback
    } else if preview {
        IntegrationTaskKind::DeployPreview
    } else {
        IntegrationTaskKind::Deploy
    };

    let mut monad_filter = global.monad.clone();
    let mut unit_filter: Option<String> = None;
    if let Some(t) = target {
        match resolve_target(&workspace, &t)? {
            TargetRef::Monad(name) => monad_filter = Some(name),
            TargetRef::Unit(name) => unit_filter = Some(name),
        }
    }

    let secret_aliases = resolve_secret_aliases(&workspace, env.as_deref(), &secret_from)?;

    let opts = CiOptions {
        monad_filter,
        unit_filter,
        // Build is the prerequisite for every deploy we ship with; users
        // needing a different precondition override via unit.toml.
        task_filter: Some(vec!["build".to_string()]),
        no_cache: global.no_cache,
        fail_fast: None,
        skip_install: global.skip_install,
        force_install: global.force_install,
        task_kind_filter: Some(kind),
        install_only: false,
        secret_aliases,
        run_notify_kinds: !no_notify,
        environment: env.clone(),
        force_deploy: force,
    };

    let registry = plugins::build_registry(&workspace);
    let integrations = IntegrationRegistry::builtin();
    let cache = LocalCache::new(monad_core::default_cache_root()?);

    // Capture the single-unit target (if any) + its declared
    // integrations BEFORE the workspace moves into the Executor.
    // Needed post-run to classify the "unit has no <kind> integration
    // task" path as integration_not_configured when the user picked
    // that unit explicitly.
    let single_unit_preflight: Option<(String, Vec<String>)> =
        opts.unit_filter.as_ref().and_then(|name| {
            workspace.unites_by_name.get(name).map(|d| {
                (
                    name.clone(),
                    d.config.integrations.keys().cloned().collect(),
                )
            })
        });

    let report = Executor::new(workspace, registry, cache)
        .with_integrations(integrations)
        .execute(&opts)?;

    // Explicit single-unit target that produced only <no-{kind}> stubs →
    // classified `integration_not_configured` error (agent-parseable).
    if let Some((unit, configured)) = single_unit_preflight {
        let kind_str = kind.as_str();
        let no_integration_marker = format!("<no-{kind_str}>");
        let unit_in_report = report
            .profiles
            .iter()
            .flat_map(|b| &b.units)
            .find(|d| d.name == unit);
        if let Some(d) = unit_in_report {
            let all_skips =
                !d.tasks.is_empty() && d.tasks.iter().all(|t| t.name == no_integration_marker);
            if all_skips {
                return Err(errors::DeployError::IntegrationNotConfigured {
                    unit,
                    kind: kind_str.to_string(),
                    configured_integrations: configured,
                }
                .into());
            }
        }
    }

    emit_report(&report, global.json, global.report_file.as_deref())?;
    Ok(
        if report.summary.failed > 0 || report.summary.install_failures > 0 {
            1
        } else {
            0
        },
    )
}

/// Drive `monad notify [target] [--env] [--secret-from]` — replays
/// Notify-kind tasks (notifications) against the persisted payload from
/// the last deploy. Used when re-sending after fixing a broken webhook
/// without touching the code. Unites with no prior-deploy sidecar
/// report a clear Skipped row; notify failures never fail the build.
fn run_notify(
    global: &GlobalFlags,
    target: Option<String>,
    env: Option<String>,
    secret_from: Vec<(String, String)>,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = Workspace::load(&root)?;

    let mut monad_filter = global.monad.clone();
    let mut unit_filter: Option<String> = None;
    if let Some(t) = target {
        match resolve_target(&workspace, &t)? {
            TargetRef::Monad(name) => monad_filter = Some(name),
            TargetRef::Unit(name) => unit_filter = Some(name),
        }
    }

    let secret_aliases = resolve_secret_aliases(&workspace, env.as_deref(), &secret_from)?;

    let opts = CiOptions {
        monad_filter,
        unit_filter,
        task_filter: None,
        no_cache: false,
        fail_fast: None,
        skip_install: true,
        force_install: false,
        task_kind_filter: Some(IntegrationTaskKind::Notify),
        install_only: false,
        secret_aliases,
        run_notify_kinds: true,
        environment: env,
        // `monad notify` re-fires notifications only — not Deploy tasks —
        // so the skip-if-unchanged gate for Deploy is irrelevant here.
        force_deploy: false,
    };

    let report = notify_at(&root, &opts)?;
    emit_report(&report, global.json, global.report_file.as_deref())?;
    // Notify failures never fail the build (same rule as `monad deploy`).
    Ok(
        if report.summary.failed > 0 || report.summary.install_failures > 0 {
            1
        } else {
            0
        },
    )
}

/// Build the declared→source env-var alias map for a deploy/doctor
/// invocation. Resolution order (later wins):
///   1. Start empty.
///   2. If `--env <name>` was given, merge in that environment's
///      `secrets.*` map from `monad.toml`. Unknown env name errors out.
///   3. Apply `--secret-from` flags on top, so an ad-hoc flag can
///      override any alias from the named environment.
fn resolve_secret_aliases(
    workspace: &Workspace,
    env: Option<&str>,
    secret_from: &[(String, String)],
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut aliases = std::collections::BTreeMap::new();
    if let Some(name) = env {
        let Some(environment) = workspace.repo.environments.get(name) else {
            let known: Vec<&String> = workspace.repo.environments.keys().collect();
            anyhow::bail!(
                "environment `{name}` is not defined in monad.toml \
                 (known: {known:?}). Add an `[environments.{name}]` block \
                 with `secrets.<VAR> = \"<SOURCE_VAR>\"` entries."
            );
        };
        for (declared, source) in &environment.secrets {
            aliases.insert(declared.clone(), source.clone());
        }
    }
    // --secret-from flags layer on top so ad-hoc can override a named env.
    for (declared, source) in secret_from {
        aliases.insert(declared.clone(), source.clone());
    }
    Ok(aliases)
}

fn emit_report(
    report: &monad_core::ExecutionReport,
    as_json: bool,
    report_file: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    if let Some(path) = report_file {
        std::fs::write(path, serde_json::to_string_pretty(report)?)
            .with_context(|| format!("writing --report-file {}", path.display()))?;
    }
    if as_json {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        run_print::print_human(report);
    }
    Ok(())
}

fn run_unit_add(
    global: &GlobalFlags,
    path: std::path::PathBuf,
    lang: Option<String>,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;

    let req = scaffold::ScaffoldRequest {
        workspace_root: &root,
        unit_rel: &path,
        language: lang.as_deref(),
        monad: global.monad.as_deref(),
    };

    let result = scaffold::run(req, &workspace)?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_scaffold_human(&result);
    }
    Ok(0)
}

fn print_scaffold_human(result: &scaffold::ScaffoldResult) {
    let verb = match result.mode {
        scaffold::ScaffoldMode::Scaffolded => "scaffolded",
        scaffold::ScaffoldMode::Adopted => "adopted",
    };
    println!(
        "{} {verb} unit '{}' ({}) into monad '{}'",
        style::green("✓"),
        style::cyan(&result.unit_name),
        style::dim(&result.language),
        style::cyan(&result.monad_name),
    );
    println!();
    println!("files:");
    for f in &result.files_written {
        println!("  {}", f.display());
    }
    println!();
    println!("next:");
    for step in &result.next_steps {
        println!("  {step}");
    }
}

// ── monad unit list / monad box list ──────────────────────────────────

use monad_core::inventory::{BoxListOutput, UnitListOutput};

fn run_unit_list(global: &GlobalFlags) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;
    let out = monad_core::inventory::unit_list(&workspace);

    if global.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print_unit_list_human(&out);
    }
    Ok(0)
}

fn run_box_list(global: &GlobalFlags) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;
    let out = monad_core::inventory::box_list(&workspace);

    if global.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print_box_list_human(&out);
    }
    Ok(0)
}

fn run_box_add(global: &GlobalFlags, name: String) -> anyhow::Result<i32> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "monad name must be non-empty and contain only ASCII letters, digits, '-', or '_' \
             (got {name:?})"
        );
    }
    let root = resolve_workspace_root(global)?;
    let profiles_dir = root.join("profiles");
    std::fs::create_dir_all(&profiles_dir)
        .with_context(|| format!("creating {}", profiles_dir.display()))?;
    let target = profiles_dir.join(format!("{name}.toml"));
    if target.exists() {
        anyhow::bail!(
            "monad '{name}' already exists at {} — pick a different name or edit the file",
            target.display()
        );
    }
    let body = render_monad_starter(&name);
    std::fs::write(&target, body.as_bytes())
        .with_context(|| format!("writing {}", target.display()))?;

    if global.json {
        let out = serde_json::json!({
            "name": name,
            "path": target.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Created {} (empty monad). Edit `units = [...]` to wire units into it, then \
             `monad ci` / `monad deploy` will pick them up.",
            target.display()
        );
    }
    Ok(0)
}

fn render_monad_starter(name: &str) -> String {
    format!(
        "# profiles/{name}.toml — monad (deployment unit).\n\
         #\n\
         # A unit name (derived from the directory basename) can appear\n\
         # in more than one monad. Its cache is shared across profiles.\n\
         \n\
         name = \"{name}\"\n\
         units = []\n"
    )
}

fn print_unit_list_human(out: &UnitListOutput) {
    if out.units.is_empty() && out.orphans.is_empty() {
        println!("{}", style::dim("no units in this workspace"));
        println!(
            "{}: run `monad unit add <path>` to scaffold one",
            style::dim("hint")
        );
        return;
    }

    if !out.units.is_empty() {
        let name_w = out
            .units
            .iter()
            .map(|d| d.name.len())
            .max()
            .unwrap_or(0)
            .max("NAME".len());
        let path_w = out
            .units
            .iter()
            .map(|d| d.path.len())
            .max()
            .unwrap_or(0)
            .max("PATH".len());
        let lang_w = out
            .units
            .iter()
            .map(|d| d.language.as_deref().unwrap_or("-").len())
            .max()
            .unwrap_or(0)
            .max("LANGUAGE".len());

        let header = format!(
            "{:<nw$}  {:<pw$}  {:<lw$}  {}",
            "NAME",
            "PATH",
            "LANGUAGE",
            "PROFILES",
            nw = name_w,
            pw = path_w,
            lw = lang_w,
        );
        println!("{}", style::bold(&header));
        for d in &out.units {
            let profiles = if d.profiles.is_empty() {
                style::yellow("(none)")
            } else {
                d.profiles.join(", ")
            };
            println!(
                "{:<nw$}  {:<pw$}  {:<lw$}  {}",
                d.name,
                d.path,
                d.language.as_deref().unwrap_or("-"),
                profiles,
                nw = name_w,
                pw = path_w,
                lw = lang_w,
            );
        }
    }

    if !out.orphans.is_empty() {
        println!();
        println!(
            "{} ({}):",
            style::yellow("orphan unit.toml (not in any monad)"),
            out.orphans.len()
        );
        for p in &out.orphans {
            println!("  {p}");
        }
        println!();
        println!(
            "{}: `monad unit add <path>` to wire them into a monad",
            style::dim("hint")
        );
    }
}

fn print_box_list_human(out: &BoxListOutput) {
    if out.profiles.is_empty() {
        println!("{}", style::dim("no profiles in this workspace"));
        println!(
            "{}: run `monad box add <name>` to create one",
            style::dim("hint")
        );
        return;
    }
    let name_w = out
        .profiles
        .iter()
        .map(|b| b.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let src_w = out
        .profiles
        .iter()
        .map(|b| b.source.len())
        .max()
        .unwrap_or(0)
        .max("SOURCE".len());
    let header = format!(
        "{:<nw$}  {:<sw$}  {}",
        "NAME",
        "SOURCE",
        "UNITS",
        nw = name_w,
        sw = src_w,
    );
    println!("{}", style::bold(&header));
    for b in &out.profiles {
        let units = if b.units.is_empty() {
            style::yellow("(empty)")
        } else {
            b.units.join(", ")
        };
        println!(
            "{:<nw$}  {:<sw$}  {}",
            b.name,
            b.source,
            units,
            nw = name_w,
            sw = src_w,
        );
    }
}

fn run_serve(global: &GlobalFlags, monad_name: String) -> anyhow::Result<i32> {
    use std::sync::Arc;
    use std::thread;

    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;

    let Some(monad) = workspace.profiles.get(&monad_name) else {
        anyhow::bail!(
            "no monad named '{monad_name}' (known: {})",
            workspace
                .profiles
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    };

    let registry = Arc::new(plugins::build_registry(&workspace));

    let mut targets: Vec<(monad_config::LoadedUnit, Vec<String>)> = Vec::new();
    for unit_ref in &monad.config.units {
        let loaded = workspace
            .unites_by_path
            .get(std::path::Path::new(unit_ref))
            .expect("workspace load guaranteed this")
            .clone();
        if loaded.config.serve.is_none() {
            continue;
        }
        let mut globs: Vec<String> = loaded.config.inputs.clone();
        let adapter = if let Some(id) = &loaded.config.language {
            registry.by_id(id)
        } else {
            registry.detect(&loaded.dir)
        };
        if let Some(a) = adapter {
            for f in a.fingerprint_files() {
                if !globs.contains(&f) {
                    globs.push(f);
                }
            }
        }
        targets.push((loaded, globs));
    }

    if targets.is_empty() {
        anyhow::bail!("monad '{monad_name}' has no units with a [serve] block — nothing to serve",);
    }

    println!(
        "monad serve: {} unit{} in '{monad_name}'",
        targets.len(),
        if targets.len() == 1 { "" } else { "es" },
    );
    for (loaded, _) in &targets {
        println!(
            "  [{:<10}] {}",
            loaded.config.name,
            loaded.config.serve.as_ref().unwrap().run,
        );
    }
    println!();

    let handles: Vec<_> = targets
        .into_iter()
        .map(|(loaded, globs)| thread::spawn(move || supervise_unit(loaded, globs)))
        .collect();

    // Threads loop forever; join blocks until one panics or the process
    // group is SIGINT'd. Propagate the first error we see.
    for h in handles {
        if let Err(e) = h.join().expect("supervisor panicked") {
            eprintln!("serve: {e:#}");
        }
    }
    Ok(0)
}

fn supervise_unit(loaded: monad_config::LoadedUnit, globs: Vec<String>) -> anyhow::Result<()> {
    let label = loaded.config.name.clone();
    let serve_run = loaded.config.serve.as_ref().unwrap().run.clone();
    let unit_dir = loaded.dir.clone();

    let mut child = spawn_serve_piped(&unit_dir, &serve_run, &label)?;
    let watcher =
        monad_watch::UnitWatcher::new(&unit_dir, &globs, std::time::Duration::from_millis(200))?;

    while let Some(batch) = watcher.next_batch() {
        let summary = batch
            .paths
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown)".to_string());
        println!(
            "[{label}] ↻ change in {}{}, restarting",
            summary,
            if batch.paths.len() > 1 {
                format!(" (+{} more)", batch.paths.len() - 1)
            } else {
                String::new()
            },
        );
        let _ = child.kill();
        let _ = child.wait();
        child = spawn_serve_piped(&unit_dir, &serve_run, &label)?;
    }

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn spawn_serve_piped(
    unit_dir: &std::path::Path,
    run: &str,
    label: &str,
) -> anyhow::Result<std::process::Child> {
    use anyhow::Context;
    use std::process::Stdio;
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(run)
        .current_dir(unit_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning `{run}`"))?;

    // Forward child stdout / stderr to ours, prefixed with the unit label.
    if let Some(out) = child.stdout.take() {
        let label_out = label.to_string();
        std::thread::spawn(move || forward_lines(out, &label_out, false));
    }
    if let Some(err) = child.stderr.take() {
        let label_err = label.to_string();
        std::thread::spawn(move || forward_lines(err, &label_err, true));
    }
    Ok(child)
}

fn forward_lines<R: std::io::Read>(pipe: R, label: &str, is_stderr: bool) {
    use std::io::{BufRead, BufReader};
    let reader = BufReader::new(pipe);
    for line in reader.lines().map_while(Result::ok) {
        if is_stderr {
            eprintln!("[{label}] {line}");
        } else {
            println!("[{label}] {line}");
        }
    }
}

fn run_dev(global: &GlobalFlags, unit_name: String) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;

    let Some(loaded) = workspace.unites_by_name.get(&unit_name) else {
        anyhow::bail!(
            "no unit named '{unit_name}' (known: {})",
            workspace
                .unites_by_name
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        );
    };

    let Some(serve) = loaded.config.serve.as_ref() else {
        anyhow::bail!(
            "unit '{unit_name}' has no [serve] block in unit.toml \
             (add `[serve]\\nrun = \"...\"`)"
        );
    };

    // Watch the unit's declared inputs plus the adapter's fingerprint files.
    let registry = plugins::build_registry(&workspace);
    let adapter: Option<&dyn monad_core::LanguageAdapter> =
        if let Some(id) = &loaded.config.language {
            registry.by_id(id)
        } else {
            registry.detect(&loaded.dir)
        };
    let mut globs: Vec<String> = loaded.config.inputs.clone();
    if let Some(a) = adapter {
        for f in a.fingerprint_files() {
            if !globs.contains(&f) {
                globs.push(f);
            }
        }
    }

    let watcher =
        monad_watch::UnitWatcher::new(&loaded.dir, &globs, std::time::Duration::from_millis(200))?;

    println!("monad dev: watching {}", loaded.dir.display());
    println!("           → `{}`", serve.run);
    println!();

    let mut child = spawn_serve(&loaded.dir, &serve.run)?;

    while let Some(batch) = watcher.next_batch() {
        let first = batch
            .paths
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown)".to_string());
        println!(
            "↻ change detected ({}{}) — restarting",
            first,
            if batch.paths.len() > 1 {
                format!(" and {} more", batch.paths.len() - 1)
            } else {
                String::new()
            },
        );
        let _ = child.kill();
        let _ = child.wait();
        child = spawn_serve(&loaded.dir, &serve.run)?;
    }

    let _ = child.kill();
    let _ = child.wait();
    Ok(0)
}

fn spawn_serve(unit_dir: &std::path::Path, run: &str) -> anyhow::Result<std::process::Child> {
    use anyhow::Context;
    std::process::Command::new("sh")
        .arg("-c")
        .arg(run)
        .current_dir(unit_dir)
        .spawn()
        .with_context(|| format!("spawning `{run}`"))
}

/// Drive `monad run <unit> <task> -- <args...>` — invoke a named
/// `[tasks.<task>]` block in `<unit>/unit.toml`, forwarding trailing
/// args as `$1..$N` to a `sh -c` invocation. Bypasses the cache
/// entirely (ad-hoc tasks like CLIs / migrations / one-off scripts
/// shouldn't carry inputs hashing). Inherits the parent process env
/// — task `env = [...]` allowlists are a cache-key concept and don't
/// apply here.
fn run_task(
    global: &GlobalFlags,
    unit_name: String,
    task_name: String,
    args: Vec<String>,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;

    let Some(loaded) = workspace.unites_by_name.get(&unit_name) else {
        anyhow::bail!(
            "no unit named '{unit_name}' (known: {})",
            workspace
                .unites_by_name
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        );
    };

    let Some(task) = loaded.config.tasks.get(&task_name) else {
        let known = loaded
            .config
            .tasks
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        if known.is_empty() {
            anyhow::bail!("unit '{unit_name}' has no `[tasks.*]` blocks declared in unit.toml");
        }
        anyhow::bail!(
            "unit '{unit_name}' has no task '{task_name}' (declared: {})",
            known.join(", "),
        );
    };

    // Adapter-defaulted lifecycle tasks (build/test/lint without an
    // explicit `run` in unit.toml) deliberately fall outside `monad
    // run` — those are the cached path. Surface a hint pointing at
    // the right verb instead of silently swallowing it.
    let Some(run) = task.run.as_deref() else {
        anyhow::bail!(
            "task '{task_name}' in unit '{unit_name}' inherits its `run` from \
             the adapter default — use `monad {task_name}` (or add an explicit \
             `run = \"...\"` to `[tasks.{task_name}]` to opt into ad-hoc invocation)"
        );
    };

    use anyhow::Context;
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(run).arg("sh");
    for a in &args {
        cmd.arg(a);
    }
    cmd.current_dir(&loaded.dir);
    let status = cmd
        .status()
        .with_context(|| format!("spawning `{run}` in {}", loaded.dir.display()))?;
    Ok(status.code().unwrap_or(1))
}

/// Drive `monad add <packages...> [--unit <unit>] [--dev]` — wraps
/// the unit's native package manager so agents don't need to know
/// which `(npm | bun | cargo | go | pnpm | yarn) add` invocation
/// goes with which unit. Lockfile + manifest writes happen via the
/// underlying tool. Bypasses monad's cache (these are write
/// operations on declared inputs; caching them would be wrong).
fn run_add(
    global: &GlobalFlags,
    packages: Vec<String>,
    unit: Option<String>,
    dev: bool,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;

    let unit_name = match unit {
        Some(n) => n,
        None => {
            let units: Vec<&String> = workspace.unites_by_name.keys().collect();
            match units.as_slice() {
                [single] => (*single).clone(),
                [] => anyhow::bail!(
                    "this workspace has no units — run `monad unit add <path>` first"
                ),
                _ => anyhow::bail!(
                    "workspace has {} units — pass `--unit <name>` (known: {})",
                    units.len(),
                    units
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            }
        }
    };

    let Some(loaded) = workspace.unites_by_name.get(&unit_name) else {
        anyhow::bail!(
            "no unit named '{unit_name}' (known: {})",
            workspace
                .unites_by_name
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        );
    };

    // Resolve adapter the same way `monad dev` does: prefer an
    // explicit `language` pin in unit.toml, fall back to detection.
    let registry = plugins::build_registry(&workspace);
    let adapter: &dyn monad_core::LanguageAdapter = if let Some(id) = &loaded.config.language {
        registry.by_id(id).ok_or_else(|| {
            anyhow::anyhow!(
                "unit '{unit_name}' declares language='{id}' but no adapter is registered"
            )
        })?
    } else {
        registry.detect(&loaded.dir).ok_or_else(|| {
            anyhow::anyhow!(
                "couldn't detect a language adapter for unit '{unit_name}' at {} — \
                 add `language = \"<id>\"` to its unit.toml",
                loaded.dir.display()
            )
        })?
    };

    let ctx = monad_adapters::TaskContext::new(&loaded.dir, &loaded.config.name);
    let opts = monad_adapters::AddOptions { dev };
    let pkg_refs: Vec<&str> = packages.iter().map(String::as_str).collect();

    let added = adapter.add(&ctx, &pkg_refs, opts).with_context(|| {
        format!(
            "adding {} to unit '{unit_name}' via the {} adapter",
            packages.join(", "),
            adapter.id()
        )
    })?;

    if global.json {
        let body = serde_json::json!({
            "unit": unit_name,
            "adapter": adapter.id(),
            "dev": dev,
            "added": added.iter().map(|a| serde_json::json!({
                "package": a.package,
                "version": a.version,
                "note": a.note,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&body)?);
    } else {
        let kind = if dev { "dev dependency" } else { "dependency" };
        let plural = if added.len() == 1 { "" } else { "y" };
        println!(
            "added {} {kind}{plural} to {unit_name} ({})",
            added.len(),
            adapter.id()
        );
        for a in &added {
            match (&a.version, &a.note) {
                (Some(v), _) => println!("  • {} v{v}", a.package),
                _ => println!("  • {}", a.package),
            }
            if let Some(note) = &a.note {
                println!("    note: {note}");
            }
        }
    }
    Ok(0)
}

fn run_cache(global: &GlobalFlags, action: CacheAction) -> anyhow::Result<i32> {
    match action {
        CacheAction::Stats => run_cache_stats(global),
        CacheAction::Clear => run_cache_clear(global),
        CacheAction::Push => run_cache_push(global),
        CacheAction::Pull => run_cache_pull(global),
    }
}

/// Upload every local bundle not yet present on the remote. HEADs each
/// local key; on 404 or transport error, PUTs. Keeps a running count.
fn run_cache_push(global: &GlobalFlags) -> anyhow::Result<i32> {
    let (remote, local, local_root) = load_remote_and_local(global)?;
    let bundles = list_local_bundles(&local_root)?;
    let mut pushed = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for (key, path) in &bundles {
        if remote.has(key) {
            skipped += 1;
            continue;
        }
        match remote.put(key, path) {
            Ok(()) => pushed += 1,
            Err(e) => {
                failed += 1;
                if !global.json {
                    eprintln!("  ! {}: {e:#}", key.short());
                }
            }
        }
    }
    let _ = local; // silence unused when JSON branch takes the tuple below
    emit_transfer(global, "push", &*remote, pushed, skipped, failed)
}

/// Download every bundle the local cache already knows about a key for
/// but doesn't have on disk. We'd need a remote LIST endpoint to pull
/// arbitrary keys; for now `pull` reconciles keys the local cache has
/// seen (manifest sidecars without their bundle) — useful after a
/// partial `cache clear` that kept manifests.
fn run_cache_pull(global: &GlobalFlags) -> anyhow::Result<i32> {
    let (remote, _local, local_root) = load_remote_and_local(global)?;
    let keys = list_keys_needing_bundle(&local_root)?;
    let mut pulled = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for key in &keys {
        let bundle = local_root.join(format!("{}.tar", key.as_hex()));
        match remote.get(key, &bundle) {
            Ok(true) => pulled += 1,
            Ok(false) => skipped += 1,
            Err(e) => {
                failed += 1;
                if !global.json {
                    eprintln!("  ! {}: {e:#}", key.short());
                }
            }
        }
    }
    emit_transfer(global, "pull", &*remote, pulled, skipped, failed)
}

fn load_remote_and_local(
    global: &GlobalFlags,
) -> anyhow::Result<(
    Box<dyn monad_core::RemoteCache>,
    monad_core::LocalCache,
    std::path::PathBuf,
)> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;
    let cache_cfg = &workspace.repo.cache;
    let Some(url) = cache_cfg.remote.as_deref() else {
        anyhow::bail!(
            "no remote cache configured — set [cache] remote = \
             \"s3://<bucket>/<prefix>\" or \"monad://<host>\" in monad.toml first"
        );
    };
    let region = cache_cfg.remote_region.as_deref();
    let endpoint = cache_cfg.remote_endpoint.as_deref();
    let token_env = cache_cfg
        .remote_token_env
        .as_deref()
        .unwrap_or("MONAD_CACHE_TOKEN");
    let token = std::env::var(token_env).ok();
    let remote = monad_core::build_remote(url, region, endpoint, token.as_deref())?;
    let local_root = monad_core::default_cache_root()?;
    let local = monad_core::LocalCache::new(&local_root);
    Ok((remote, local, local_root))
}

fn list_local_bundles(
    root: &std::path::Path,
) -> anyhow::Result<Vec<(monad_core::CacheKey, std::path::PathBuf)>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(hex) = name.strip_suffix(".tar") {
            out.push((monad_core::CacheKey::from_hex(hex), path));
        }
    }
    Ok(out)
}

fn list_keys_needing_bundle(root: &std::path::Path) -> anyhow::Result<Vec<monad_core::CacheKey>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(hex) = name.strip_suffix(".inputs.json") {
            if !root.join(format!("{hex}.tar")).exists() {
                out.push(monad_core::CacheKey::from_hex(hex));
            }
        }
    }
    Ok(out)
}

fn emit_transfer(
    global: &GlobalFlags,
    verb: &str,
    remote: &dyn monad_core::RemoteCache,
    ok: u32,
    skipped: u32,
    failed: u32,
) -> anyhow::Result<i32> {
    if global.json {
        let payload = serde_json::json!({
            "remote": remote.display_url(),
            "direction": verb,
            "transferred": ok,
            "skipped": skipped,
            "failed": failed,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "{} {verb}: {} transferred · {} skipped · {} failed  (remote: {})",
            style::green("✓"),
            ok,
            skipped,
            failed,
            style::cyan(remote.display_url()),
        );
    }
    Ok(if failed > 0 { 1 } else { 0 })
}

fn run_cache_stats(global: &GlobalFlags) -> anyhow::Result<i32> {
    let root = monad_core::default_cache_root()?;
    let cache = monad_core::LocalCache::new(&root);
    let stats = cache.stats()?;

    if global.json {
        let payload = serde_json::json!({
            "root": root.display().to_string(),
            "entries": stats.entries,
            "total_bytes": stats.total_bytes,
            "oldest_unix_seconds": stats.oldest_unix_seconds,
            "newest_unix_seconds": stats.newest_unix_seconds,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "{}: {}",
            style::bold("monad cache"),
            style::cyan(&root.display().to_string()),
        );
        println!();
        println!("  entries:    {}", stats.entries);
        println!("  total size: {}", format_bytes(stats.total_bytes));
        if let (Some(oldest), Some(newest)) = (stats.oldest_unix_seconds, stats.newest_unix_seconds)
        {
            println!("  oldest:     {}", format_age(oldest));
            println!("  newest:     {}", format_age(newest));
        } else {
            println!("  oldest:     {}", style::dim("—"));
            println!("  newest:     {}", style::dim("—"));
        }
    }
    Ok(0)
}

fn run_cache_clear(global: &GlobalFlags) -> anyhow::Result<i32> {
    let root = monad_core::default_cache_root()?;
    let cache = monad_core::LocalCache::new(&root);
    let before = cache.stats()?;
    cache.clear()?;

    if global.json {
        let payload = serde_json::json!({
            "root": root.display().to_string(),
            "cleared_entries": before.entries,
            "cleared_bytes": before.total_bytes,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "{} cleared {} entr{} ({}) from {}",
            style::green("✓"),
            before.entries,
            if before.entries == 1 { "y" } else { "ies" },
            format_bytes(before.total_bytes),
            style::cyan(&root.display().to_string()),
        );
    }
    Ok(0)
}

fn format_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if n >= GB {
        format!("{:.2} GiB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.2} MiB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.2} KiB", n as f64 / KB as f64)
    } else {
        format!("{n} B")
    }
}

/// Render a UNIX timestamp as a human-readable "N days ago" string.
/// Deliberately low-fidelity: we only need order-of-magnitude context
/// in the stats table, and a dep-free formatter beats pulling in a
/// calendar crate.
fn format_age(unix_seconds: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let delta = now.saturating_sub(unix_seconds);
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const WEEK: u64 = 7 * DAY;
    if delta < MINUTE {
        "just now".to_string()
    } else if delta < HOUR {
        format!("{}m ago", delta / MINUTE)
    } else if delta < DAY {
        format!("{}h ago", delta / HOUR)
    } else if delta < WEEK {
        format!("{}d ago", delta / DAY)
    } else {
        format!("{}w ago", delta / WEEK)
    }
}

fn run_migrate(global: &GlobalFlags, source: MigrateSource) -> anyhow::Result<i32> {
    match source {
        MigrateSource::Turbo {
            path,
            dry_run,
            force,
        } => {
            let root = match path {
                Some(p) => p,
                None => std::env::current_dir()?,
            };
            let report = migrate::turbo::run(migrate::turbo::Options {
                root,
                dry_run,
                force,
            })?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                migrate::print_human(&report);
            }
            // Conflicts → exit 1 so CI fails clearly when a re-run is
            // needed. Other notes (Skipped, Inferred) don't fail.
            Ok(if report.has_conflicts() { 1 } else { 0 })
        }
        MigrateSource::Nx {
            path,
            dry_run,
            force,
        } => {
            let root = match path {
                Some(p) => p,
                None => std::env::current_dir()?,
            };
            let report = migrate::nx::run(migrate::nx::Options {
                root,
                dry_run,
                force,
            })?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                migrate::print_human(&report);
            }
            Ok(if report.has_conflicts() { 1 } else { 0 })
        }
        MigrateSource::Lerna {
            path,
            dry_run,
            force,
        } => {
            let root = match path {
                Some(p) => p,
                None => std::env::current_dir()?,
            };
            let report = migrate::lerna::run(migrate::lerna::Options {
                root,
                dry_run,
                force,
            })?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                migrate::print_human(&report);
            }
            Ok(if report.has_conflicts() { 1 } else { 0 })
        }
        MigrateSource::Make {
            path,
            dry_run,
            force,
        } => {
            let root = match path {
                Some(p) => p,
                None => std::env::current_dir()?,
            };
            let report = migrate::make::run(migrate::make::Options {
                root,
                dry_run,
                force,
            })?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                migrate::print_human(&report);
            }
            Ok(if report.has_conflicts() { 1 } else { 0 })
        }
        MigrateSource::Moon {
            path,
            dry_run,
            force,
        } => {
            let root = match path {
                Some(p) => p,
                None => std::env::current_dir()?,
            };
            let report = migrate::moon::run(migrate::moon::Options {
                root,
                dry_run,
                force,
            })?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                migrate::print_human(&report);
            }
            Ok(if report.has_conflicts() { 1 } else { 0 })
        }
        MigrateSource::Rush {
            path,
            dry_run,
            force,
        } => {
            let root = match path {
                Some(p) => p,
                None => std::env::current_dir()?,
            };
            let report = migrate::rush::run(migrate::rush::Options {
                root,
                dry_run,
                force,
            })?;
            if global.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                migrate::print_human(&report);
            }
            Ok(if report.has_conflicts() { 1 } else { 0 })
        }
    }
}

fn run_init(global: &GlobalFlags, no_detect: bool) -> anyhow::Result<i32> {
    // `monad init` intentionally writes into a monad-free directory, so
    // don't walk upward looking for an existing workspace. --workspace
    // (or $MONAD_WORKSPACE_ROOT) retargets the write; otherwise use cwd.
    let cwd = match &global.workspace {
        Some(p) => p.clone(),
        None => std::env::current_dir()?,
    };
    let monad_toml = cwd.join("monad.toml");
    let profiles_dir = cwd.join("profiles");
    let prod_toml = profiles_dir.join("prod.toml");

    if monad_toml.exists() || prod_toml.exists() {
        return Err(anyhow::anyhow!(
            "workspace already initialised (found {}). Refusing to overwrite.",
            if monad_toml.exists() {
                monad_toml.display().to_string()
            } else {
                prod_toml.display().to_string()
            }
        ));
    }

    // Detection happens with built-in adapters only — no plugins, since
    // there's no monad.toml to read [plugins] filters from yet.
    let registry = monad_core::AdapterRegistry::builtin();
    let detected = if no_detect {
        Vec::new()
    } else {
        init::detect_unites(&cwd, &registry)
    };
    let toolchains = init::merge_toolchains(&detected);

    std::fs::create_dir_all(&profiles_dir).with_context_msg("creating profiles/")?;
    std::fs::write(&monad_toml, init::render_monad_toml(&toolchains.pins))
        .with_context_msg("writing monad.toml")?;
    let unit_rels: Vec<String> = detected.iter().map(|d| d.rel.clone()).collect();
    std::fs::write(&prod_toml, init::render_prod_toml(&unit_rels))
        .with_context_msg("writing profiles/prod.toml")?;
    let unit_toml_paths =
        init::write_unit_tomls(&detected, &registry).context("writing unit.toml files")?;

    // AGENTS.md (cross-tool agent-instructions standard) + CLAUDE.md
    // (Claude Code-specific, @imports AGENTS.md). Idempotent merge:
    // create when absent, append a marker-delimited monad block when
    // the file exists without our markers (preserving the user's
    // prose), update the block in place on re-run.
    //
    // Symlink-aware. Repos that point CLAUDE.md → AGENTS.md (a real
    // convention — the file serves both Claude Code and the cross-tool
    // standard) only need one write; the second would resolve through
    // the symlink and overwrite the first via the marker-replace path,
    // landing the CLAUDE.md `@AGENTS.md` snippet on top of the
    // canonical AGENTS.md snippet. Detect via `canonicalize` and skip.
    let agents_md = cwd.join("AGENTS.md");
    let claude_md = cwd.join("CLAUDE.md");
    let same_underlying_file = std::fs::canonicalize(&agents_md)
        .ok()
        .zip(std::fs::canonicalize(&claude_md).ok())
        .map(|(a, c)| a == c)
        .unwrap_or(false);

    let agents_action = init::install_agents_md(&agents_md).context("merging AGENTS.md")?;
    let mut agent_file_results: Vec<(PathBuf, init::AgentFileAction)> =
        vec![(agents_md.clone(), agents_action)];
    if !same_underlying_file {
        let claude_action = init::install_claude_md(&claude_md).context("merging CLAUDE.md")?;
        agent_file_results.push((claude_md.clone(), claude_action));
    }

    let mut files: Vec<PathBuf> = vec![
        monad_toml
            .strip_prefix(&cwd)
            .unwrap_or(&monad_toml)
            .to_path_buf(),
        prod_toml
            .strip_prefix(&cwd)
            .unwrap_or(&prod_toml)
            .to_path_buf(),
    ];
    files.extend(
        unit_toml_paths
            .iter()
            .map(|p: &PathBuf| p.strip_prefix(&cwd).unwrap_or(p).to_path_buf()),
    );
    files.extend(
        agent_file_results
            .iter()
            .map(|(p, _)| p.strip_prefix(&cwd).unwrap_or(p).to_path_buf()),
    );

    if global.json {
        let payload = serde_json::json!({
            "root": cwd.display().to_string(),
            "files_written": files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            "unites_detected": detected
                .iter()
                .map(|d| serde_json::json!({
                    "name": d.name,
                    "path": d.rel,
                    "language": d.language,
                    "toolchain": d.toolchain.as_ref().map(|(t, v)| serde_json::json!({"tool": t, "version": v})),
                }))
                .collect::<Vec<_>>(),
            "toolchain_pins": toolchains.pins,
            "toolchain_conflicts": toolchains.conflicts,
            "agent_files": agent_file_results
                .iter()
                .map(|(p, action)| serde_json::json!({
                    "path": p.strip_prefix(&cwd).unwrap_or(p).display().to_string(),
                    "action": action.as_str(),
                }))
                .collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "{} initialised monad workspace at {}",
            style::green("✓"),
            style::cyan(&cwd.display().to_string()),
        );
        println!();
        if detected.is_empty() {
            println!("files:");
            for f in &files {
                println!("  {}", f.display());
            }
            println!();
            println!("next:");
            println!("  {}", style::dim("monad unit add apps/api --lang go"));
            println!("  {}", style::dim("monad plan"));
        } else {
            println!("detected {} unit(es):", detected.len());
            for d in &detected {
                match d.toolchain.as_ref() {
                    Some((tool, version)) => {
                        println!(
                            "  {} {} ({}){}",
                            style::green("✓"),
                            style::cyan(&d.rel),
                            d.language,
                            style::dim(&format!("  {tool} {version}")),
                        );
                    }
                    None => {
                        println!(
                            "  {} {} ({}){}",
                            style::yellow("⚠"),
                            style::cyan(&d.rel),
                            d.language,
                            style::dim("  no toolchain pin"),
                        );
                    }
                }
            }
            if !toolchains.pins.is_empty() {
                println!();
                println!("captured toolchain pins in monad.toml:");
                for (tool, version) in &toolchains.pins {
                    println!("  {tool} = \"{version}\"");
                }
            }
            let unpinned: Vec<&str> = detected
                .iter()
                .filter(|d| d.toolchain.is_none())
                .map(|d| d.rel.as_str())
                .collect();
            if !unpinned.is_empty() {
                println!();
                println!(
                    "{} {} unit(es) have no detected toolchain pin ({}). monad can't lock to a specific version. \
                     Add a per-tool version file (.nvmrc / .python-version / .ruby-version / .java-version), \
                     a project-wide .tool-versions (asdf / mise), or the equivalent in package.json (volta.node, engines.node) \
                     for reproducible builds.",
                    style::yellow("note:"),
                    unpinned.len(),
                    unpinned.join(", "),
                );
            }
            for note in &toolchains.conflicts {
                println!();
                println!("{} {}", style::yellow("note:"), note);
            }
            println!();
            println!("files:");
            for f in &files {
                println!("  {}", f.display());
            }
            println!();
            println!("next:");
            println!("  {}", style::dim("monad plan"));
            println!("  {}", style::dim("monad ci"));
        }
    }
    Ok(0)
}

/// Tiny helper so the init function's .with_context lines stay scannable.
trait InitIoExt<T> {
    fn with_context_msg(self, msg: &'static str) -> anyhow::Result<T>;
}

impl<T> InitIoExt<T> for std::io::Result<T> {
    fn with_context_msg(self, msg: &'static str) -> anyhow::Result<T> {
        use anyhow::Context;
        self.with_context(|| msg.to_string())
    }
}

fn run_graph(
    global: &GlobalFlags,
    monad: Option<String>,
    format: cli::GraphFormat,
) -> anyhow::Result<i32> {
    let root = resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;

    // Honour both the --monad global flag and the positional monad arg;
    // the positional form wins if both are provided.
    let filter = monad.or_else(|| global.monad.clone());

    let mut names: Vec<&String> = workspace.profiles.keys().collect();
    if let Some(f) = &filter {
        names.retain(|n| *n == f);
        if names.is_empty() {
            anyhow::bail!(
                "no monad named '{f}' (known: {})",
                workspace
                    .profiles
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
    }

    let graphs: Vec<monad_core::ProfileGraph> = names
        .iter()
        .map(|n| monad_core::build_graph(&workspace, n))
        .collect::<Result<_, _>>()?;

    if global.json {
        emit_graph_json(&graphs)?;
    } else {
        match format {
            cli::GraphFormat::Ascii => emit_graph_ascii(&workspace, &graphs),
            cli::GraphFormat::Dot => emit_graph_dot(&workspace, &graphs),
        }
    }
    Ok(0)
}

fn emit_graph_json(graphs: &[monad_core::ProfileGraph]) -> anyhow::Result<()> {
    use serde::Serialize;
    #[derive(Serialize)]
    struct View<'a> {
        monad: &'a str,
        levels: &'a [Vec<String>],
    }
    let views: Vec<View<'_>> = graphs
        .iter()
        .map(|g| View {
            monad: &g.monad,
            levels: &g.levels,
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&views)?);
    Ok(())
}

fn emit_graph_ascii(workspace: &monad_config::Workspace, graphs: &[monad_core::ProfileGraph]) {
    for (i, g) in graphs.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!(
            "monad: {} ({} unit{})",
            g.monad,
            g.unit_count(),
            if g.unit_count() == 1 { "" } else { "es" },
        );
        for (level_idx, level) in g.levels.iter().enumerate() {
            println!("  level {level_idx}:");
            for unit_name in level {
                let deps = dep_list(workspace, unit_name);
                if deps.is_empty() {
                    println!("    {unit_name}");
                } else {
                    println!("    {unit_name}  ← {}", deps.join(", "));
                }
            }
        }
    }
}

fn emit_graph_dot(workspace: &monad_config::Workspace, graphs: &[monad_core::ProfileGraph]) {
    println!("digraph monad {{");
    println!("  rankdir=LR;");
    println!("  node [shape=box, fontname=\"Helvetica\"];");
    for g in graphs {
        let cluster = g.monad.replace(|c: char| !c.is_alphanumeric(), "_");
        println!("  subgraph cluster_{cluster} {{");
        println!("    label = \"{}\";", g.monad);
        for level in &g.levels {
            for unit in level {
                println!("    \"{unit}\";");
            }
        }
        for level in &g.levels {
            for unit in level {
                for dep in dep_list(workspace, unit) {
                    println!("    \"{dep}\" -> \"{unit}\";");
                }
            }
        }
        println!("  }}");
    }
    println!("}}");
}

fn dep_list(workspace: &monad_config::Workspace, unit_name: &str) -> Vec<String> {
    workspace
        .unites_by_name
        .get(unit_name)
        .map(|loaded| loaded.config.depends_on.clone())
        .unwrap_or_default()
}

fn run_doctor(
    global: &GlobalFlags,
    env: Option<String>,
    secret_from: Vec<(String, String)>,
    cloud: bool,
) -> anyhow::Result<i32> {
    // doctor walks upward from `start` looking for a workspace, so
    // --workspace (or $MONAD_WORKSPACE_ROOT) just retargets where the
    // walk begins. When unset, start from cwd.
    let start = match &global.workspace {
        Some(p) => p.clone(),
        None => std::env::current_dir()?,
    };
    // Resolve aliases if either flag was given; pass through to the
    // doctor so integration env-var checks use the aliased sources.
    let aliases = if env.is_some() || !secret_from.is_empty() {
        let root = monad_core::find_workspace_root(&start)?;
        let workspace = Workspace::load(&root)?;
        resolve_secret_aliases(&workspace, env.as_deref(), &secret_from)?
    } else {
        std::collections::BTreeMap::new()
    };
    let options = monad_core::doctor::DoctorOptions { cloud };
    let report = monad_core::doctor::run_with_options(&start, &aliases, options)?;

    if global.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_doctor_human(&report);
    }
    Ok(report.exit_code())
}

fn print_doctor_human(report: &monad_core::DoctorReport) {
    use monad_core::CheckStatus;

    let name_width = report
        .checks
        .iter()
        .map(|c| c.name.len())
        .max()
        .unwrap_or(0);

    for c in &report.checks {
        let (marker, tag) = match c.status {
            CheckStatus::Ok => (style::green("✓"), style::green("ok  ")),
            CheckStatus::Warn => (style::yellow("!"), style::yellow("warn")),
            CheckStatus::Fail => (style::red("✗"), style::red("fail")),
            CheckStatus::Skipped => (style::dim("·"), style::dim("skip")),
        };
        println!(
            "  {marker} {name:<name_width$}  [{tag}]  {detail}",
            marker = marker,
            name = c.name,
            name_width = name_width,
            tag = tag,
            detail = c.detail,
        );
    }
    println!();
    let s = &report.summary;
    println!(
        "summary: {} total · {} ok · {} warn · {} fail · {} skipped",
        s.total,
        style::green(&s.ok.to_string()),
        style::yellow(&s.warn.to_string()),
        style::red(&s.fail.to_string()),
        style::dim(&s.skipped.to_string()),
    );
}

fn run_why(global: &GlobalFlags, target: &str) -> anyhow::Result<i32> {
    let cache = LocalCache::new(monad_core::default_cache_root()?);

    // Two accepted forms: `<unit>:<task>` (resolved via a plan pass) or
    // a cache-key hex prefix (used verbatim). The `:` is the
    // distinguishing character — unit names can't contain it, and
    // cache keys are pure hex.
    let prefix_owned: String;
    let prefix: &str = if target.contains(':') {
        let root = resolve_workspace_root(global)?;
        prefix_owned = monad_core::why::resolve_unit_task_key(&root, target)?;
        &prefix_owned
    } else {
        // Reject non-hex input up front so agents get the classified
        // envelope instead of an ambiguous empty-result. Cache keys are
        // pure lowercase hex; anything else is user error.
        if target.is_empty() || !target.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(monad_core::why::WhyTargetError::InvalidUnitTask {
                input: target.to_string(),
            }
            .into());
        }
        target
    };

    let results = monad_core::why::explain(&cache, prefix)?;
    if results.is_empty() && target.contains(':') {
        // We resolved a real unit:task but the key wasn't in the cache.
        // Surface that as a classified error rather than a bare empty
        // print — agents reading JSON need the signal.
        let (unit, task) = target.split_once(':').unwrap();
        return Err(monad_core::why::WhyTargetError::NoCacheEntry {
            unit: unit.to_string(),
            task: task.to_string(),
            key: prefix.to_string(),
        }
        .into());
    }

    if global.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        why::print_human(prefix, &results);
    }
    Ok(if results.is_empty() { 1 } else { 0 })
}

#[cfg(test)]
mod workspace_root_tests {
    use super::*;
    use std::fs;

    fn mk_workspace() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("monad.toml"), "").unwrap();
        fs::create_dir_all(tmp.path().join("profiles")).unwrap();
        tmp
    }

    fn mk_global(workspace: Option<PathBuf>) -> GlobalFlags {
        GlobalFlags {
            json: false,
            no_cache: false,
            monad: None,
            since: None,
            verbose: false,
            report_file: None,
            skip_install: false,
            force_install: false,
            workspace,
        }
    }

    #[test]
    fn explicit_workspace_flag_takes_precedence() {
        let tmp = mk_workspace();
        let global = mk_global(Some(tmp.path().to_path_buf()));
        let got = resolve_workspace_root(&global).unwrap();
        // canonicalize to handle macOS /var -> /private/var etc.
        assert_eq!(
            got.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn falls_back_to_cwd_when_no_flag() {
        let tmp = mk_workspace();
        let global = mk_global(None);
        // Set cwd to tmp and confirm we find it. (Uses the process-wide
        // cwd, so keep this test single-threaded.)
        std::env::set_current_dir(tmp.path()).unwrap();
        let got = resolve_workspace_root(&global).unwrap();
        assert_eq!(
            got.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn explicit_workspace_in_subdir_walks_up_to_root() {
        let tmp = mk_workspace();
        let sub = tmp.path().join("crates/thing");
        fs::create_dir_all(&sub).unwrap();
        let global = mk_global(Some(sub));
        let got = resolve_workspace_root(&global).unwrap();
        assert_eq!(
            got.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }
}
