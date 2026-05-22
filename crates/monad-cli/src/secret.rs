//! `monad secret put|list|delete` — thin wrapper over per-platform
//! secret CLIs (wrangler, railway, vercel). The integration trait
//! owns the per-platform invocation; this module's job is target
//! resolution, input/output shaping, and error surfacing.
//!
//! Values flow through `put` once (stdin → integration method) and
//! are never persisted, logged, or returned by monad.

use std::io::Read;

use anyhow::{Context, Result};

use crate::cli::{GlobalFlags, SecretAction};

pub fn run(global: &GlobalFlags, action: SecretAction) -> anyhow::Result<i32> {
    let root = crate::resolve_workspace_root(global)?;
    let workspace = monad_config::Workspace::load(&root)?;
    let integrations = monad_adapters::IntegrationRegistry::builtin();

    match action {
        SecretAction::Put { target, name } => {
            let (unit, integration, config) =
                resolve_target(&workspace, &integrations, &target, "put")?;
            let value = read_value_from_stdin()?;
            integration
                .put_secret(&unit.dir, &config, &name, &value)
                .with_context(|| format!("{}:{} put {name}", unit.config.name, integration.id()))?;
            eprintln!(
                "monad secret: set {} on {} ({})",
                name,
                unit.config.name,
                integration.id()
            );
            Ok(0)
        }
        SecretAction::List { target } => {
            let (unit, integration, config) =
                resolve_target(&workspace, &integrations, &target, "list")?;
            let mut names = integration
                .list_secrets(&unit.dir, &config)
                .with_context(|| format!("{}:{} list", unit.config.name, integration.id()))?;
            names.sort();
            for n in &names {
                println!("{n}");
            }
            Ok(0)
        }
        SecretAction::Delete { target, name } => {
            let (unit, integration, config) =
                resolve_target(&workspace, &integrations, &target, "delete")?;
            integration
                .delete_secret(&unit.dir, &config, &name)
                .with_context(|| {
                    format!("{}:{} delete {name}", unit.config.name, integration.id())
                })?;
            eprintln!(
                "monad secret: deleted {} on {} ({})",
                name,
                unit.config.name,
                integration.id()
            );
            Ok(0)
        }
    }
}

/// Resolve a `<unit>[:<integration>]` target string to the concrete
/// unit + integration + `[integrations.<id>]` config block. Errors
/// with a friendly message when the unit is unknown, has no secret-
/// capable integration, or has multiple without an explicit disambig.
fn resolve_target<'a>(
    workspace: &'a monad_config::Workspace,
    integrations: &'a monad_adapters::IntegrationRegistry,
    target: &str,
    op: &str,
) -> Result<(
    &'a monad_config::LoadedUnit,
    &'a dyn monad_adapters::Integration,
    toml::Table,
)> {
    let (unit_name, integration_hint) = match target.split_once(':') {
        Some((d, i)) => (d, Some(i)),
        None => (target, None),
    };

    let unit = workspace
        .units_by_path
        .values()
        .find(|d| d.config.name == unit_name)
        .ok_or_else(|| {
            let known: Vec<_> = workspace
                .units_by_path
                .values()
                .map(|d| d.config.name.clone())
                .collect();
            anyhow::anyhow!(
                "no unit named '{unit_name}' in this workspace (known: {})",
                known.join(", ")
            )
        })?;

    // Integrations that both (a) declare secret support and (b) are
    // wired up on this unit — either via filesystem detect() or via an
    // explicit `[integrations.<id>]` block.
    let candidates: Vec<&dyn monad_adapters::Integration> = integrations
        .ids()
        .into_iter()
        .filter_map(|id| integrations.by_id(&id))
        .filter(|i| i.supports_secrets())
        .filter(|i| i.detect(&unit.dir) || unit.config.integrations.contains_key(i.id()))
        .collect();

    if candidates.is_empty() {
        anyhow::bail!(
            "unit '{unit_name}' has no secret-capable deploy integration \
             (cloudflare_worker, cloudflare_pages, railway). Add an \
             [integrations.<id>] block to {}/unit.toml, or use the \
             underlying CLI directly for {op}.",
            unit.rel.display()
        );
    }

    let integration: &dyn monad_adapters::Integration = match integration_hint {
        Some(id) => *candidates.iter().find(|i| i.id() == id).ok_or_else(|| {
            let names: Vec<_> = candidates.iter().map(|i| i.id().to_string()).collect();
            anyhow::anyhow!(
                "integration '{id}' not enabled on unit '{unit_name}' \
                     (available: {})",
                names.join(", "),
            )
        })?,
        None => {
            if candidates.len() > 1 {
                let names: Vec<_> = candidates.iter().map(|i| i.id().to_string()).collect();
                anyhow::bail!(
                    "unit '{unit_name}' has multiple secret-capable integrations \
                     ({}). Disambiguate: `monad secret {op} {unit_name}:<integration> ...`",
                    names.join(", "),
                );
            }
            candidates[0]
        }
    };

    // Empty table when the unit has no explicit `[integrations.<id>]`
    // block — mirrors the shape passed to `detected_tasks`. Most
    // integrations' secret methods read nothing from config (the
    // target is derived from cwd/wrangler.toml/railway config); the
    // Pages + Railway cases do pull `project` / `service`.
    let config = unit
        .config
        .integrations
        .get(integration.id())
        .cloned()
        .unwrap_or_default();

    Ok((unit, integration, config))
}

/// Read exactly one secret value from stdin. Strips a single trailing
/// newline so `echo "$VAL" | monad secret put ...` behaves the way
/// users expect; explicit multi-line secrets (rare — RSA keys, JWT
/// private keys) work if piped without a trailing newline, since we
/// only strip ONE terminator.
fn read_value_from_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading secret value from stdin")?;
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
    if buf.is_empty() {
        anyhow::bail!(
            "empty stdin — pipe the secret value in, e.g. \
             `echo -n \"$VAL\" | monad secret put <target> NAME`"
        );
    }
    Ok(buf)
}
