//! Structured, agent-friendly error output.
//!
//! Every command can fail via `anyhow::Error`. When the CLI is run with
//! `--json`, [`classify`] walks the error chain to produce a [`MonadError`]
//! with a stable `kind` string, plus an optional `hint` and `where`
//! (location) — emitted as one JSON object on stdout.
//!
//! Without `--json`, errors stay human-readable and go to stderr.

use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;

use monad_core::why::WhyTargetError;

use crate::login::LoginError;
use crate::scaffold::ScaffoldError;

/// Classified failures from the deploy / notify preflight.
/// Constructed in `main.rs` when we know the user explicitly targeted
/// a single unit and it has no integration task of the requested kind.
#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error(
        "unit '{unit}' has no integration task of kind '{kind}' — \
         nothing to {kind}"
    )]
    IntegrationNotConfigured {
        unit: String,
        kind: String,
        /// Integration ids (from `[integrations.*]` keys) the unit
        /// DOES declare, even if they don't contribute a task of this
        /// kind. Informational — helps the agent understand why the
        /// kind mismatched.
        configured_integrations: Vec<String>,
    },
}

/// Stable, agent-friendly error envelope. Every command failure with
/// `--json` produces exactly one of these on stdout.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MonadError {
    /// Stable machine identifier. Agents should switch on this string.
    pub kind: String,
    /// Human-readable description of what failed.
    pub message: String,
    /// Suggested next action, if any. For a single primary suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Ordered recovery steps. Always an array (may be empty). Use when
    /// the fix is multi-step or enumerates structured options (e.g.
    /// "here are the available units: a, b, c"). Prefer this over
    /// `hint` for anything an agent would want to pick from rather than
    /// read.
    pub next_steps: Vec<String>,
    /// File path or locator where the error originated, if applicable.
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    /// Link to documentation for this error kind, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs_url: Option<String>,
}

impl MonadError {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            hint: None,
            next_steps: Vec::new(),
            locator: None,
            docs_url: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_next_steps<I, S>(mut self, steps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.next_steps = steps.into_iter().map(Into::into).collect();
        self
    }

    pub fn at(mut self, locator: impl Into<String>) -> Self {
        self.locator = Some(locator.into());
        self
    }
}

/// Classify an `anyhow::Error` by walking its source chain for known types.
/// Unknown errors fall through to `kind = "internal"`.
pub fn classify(err: &anyhow::Error) -> MonadError {
    for cause in err.chain() {
        if let Some(cfg) = cause.downcast_ref::<monad_config::ConfigError>() {
            return classify_config(cfg);
        }
        if let Some(s) = cause.downcast_ref::<ScaffoldError>() {
            return classify_scaffold(s);
        }
        if let Some(w) = cause.downcast_ref::<monad_core::WorkspaceNotFound>() {
            return MonadError::new("workspace_not_found", w.to_string())
                .at(w.start.display().to_string())
                .with_hint(
                    "run this command inside a monad workspace, \
                     or run `monad init` to create one",
                )
                .with_next_steps([
                    "cd into an existing monad workspace (one containing monad.toml)",
                    "or run `monad init` here to create a new workspace",
                ]);
        }
        if let Some(t) = cause.downcast_ref::<monad_core::TargetRefError>() {
            return classify_target_ref(t);
        }
        if let Some(w) = cause.downcast_ref::<WhyTargetError>() {
            return classify_why_target(w);
        }
        if let Some(l) = cause.downcast_ref::<LoginError>() {
            return classify_login(l);
        }
        if let Some(d) = cause.downcast_ref::<DeployError>() {
            return classify_deploy(d);
        }
    }
    MonadError::new("internal", err.to_string())
}

fn classify_deploy(err: &DeployError) -> MonadError {
    use DeployError::*;
    match err {
        IntegrationNotConfigured {
            unit,
            kind,
            configured_integrations,
        } => {
            let mut steps = Vec::new();
            if configured_integrations.is_empty() {
                steps.push(format!(
                    "add `[integrations.<platform>]` (cloudflare_pages, \
                     railway, cloudflare_worker, …) to {unit}/unit.toml"
                ));
            } else {
                steps.push(format!(
                    "unit '{unit}' has these integrations configured: {} — \
                     none of them emit a '{kind}' task",
                    configured_integrations.join(", ")
                ));
                steps.push(format!(
                    "either add an integration that supports '{kind}', or \
                     drop the `monad {verb}` call for this unit",
                    verb = match kind.as_str() {
                        "deploy" | "deploy-preview" => "deploy",
                        "rollback" => "deploy --rollback",
                        "notify" => "notify",
                        _ => "deploy",
                    }
                ));
            }
            steps.push("run `monad doctor --env <env>` to see integration readiness".to_string());
            MonadError::new("integration_not_configured", err.to_string())
                .with_hint(format!(
                    "unit '{unit}' has no '{kind}' integration task — \
                     add an `[integrations.*]` block that covers '{kind}'"
                ))
                .with_next_steps(steps)
        }
    }
}

fn classify_login(err: &LoginError) -> MonadError {
    use LoginError::*;
    match err {
        Expired => MonadError::new("login_expired", err.to_string())
            .with_hint("re-run `monad login` — the device code was revoked or expired before approval")
            .with_next_steps(vec![
                "re-run `monad login` and approve quickly in the browser".to_string(),
            ]),
        Timeout { timeout_secs } => MonadError::new("login_timeout", err.to_string())
            .with_hint(format!(
                "login poll timed out after {timeout_secs}s — re-run `monad login`"
            ))
            .with_next_steps(vec![
                "re-run `monad login`".to_string(),
                "if this keeps happening, check your network reach to api.monad.build".to_string(),
            ]),
        ServerError { stage, status, body } => {
            let short_body: String = body.chars().take(160).collect();
            MonadError::new("login_server_error", err.to_string())
                .with_hint(format!(
                    "api.monad.build {stage} endpoint returned HTTP {status} — \
                     {short_body}"
                ))
                .with_next_steps(vec![
                    format!(
                        "wait a minute + re-run `monad login` (transient {status} responses \
                         usually clear)"
                    ),
                    "if the error persists, report it with the status + body from --json \
                     output"
                        .to_string(),
                ])
        }
        Transport { stage, .. } => MonadError::new("login_transport", err.to_string())
            .with_hint(format!(
                "network error while talking to {stage} — check connectivity to \
                 api.monad.build"
            ))
            .with_next_steps(vec![
                "verify network reach to api.monad.build (try `curl https://api.monad.build/healthz`)"
                    .to_string(),
                "re-run `monad login` once the network settles".to_string(),
            ]),
        InvalidResponse { stage, detail } => {
            MonadError::new("login_invalid_response", err.to_string())
                .with_hint(format!(
                    "api.monad.build {stage} returned a body we couldn't parse — {detail}"
                ))
                .with_next_steps(vec![
                    "this is a remote-cache server issue — try again in a minute, then report if it persists".to_string(),
                ])
        }
    }
}

fn classify_why_target(err: &WhyTargetError) -> MonadError {
    use WhyTargetError::*;
    match err {
        InvalidUnitTask { input } => MonadError::new("why_invalid_target", err.to_string())
            .with_hint(format!(
                "'{input}' is not valid — use `<unit>:<task>` (e.g. `marketing:lint`) \
                 or a cache-key hex prefix"
            ))
            .with_next_steps(vec![
                format!("try `monad why marketing:lint` — replace with your unit:task pair"),
                "or run `monad plan --json` and copy a task's `key` field".to_string(),
            ]),
        UnitNotFound { unit, available } => {
            let mut steps = vec![];
            if available.is_empty() {
                steps.push(
                    "this workspace has no units — run `monad unit add <path>` first".to_string(),
                );
            } else {
                steps.push(format!("available units: {}", available.join(", ")));
                steps.push("run `monad unit list` to see every unit with its profiles".into());
            }
            MonadError::new("why_unit_not_found", err.to_string())
                .with_hint(format!(
                    "no unit named '{unit}' — check `monad unit list` for the canonical name"
                ))
                .with_next_steps(steps)
        }
        TaskNotFound {
            unit,
            task,
            available,
        } => MonadError::new("why_task_not_found", err.to_string())
            .with_hint(format!("unit '{unit}' has no task named '{task}'"))
            .with_next_steps(vec![
                format!("available tasks on '{unit}': {}", available.join(", ")),
                format!("run `monad plan {unit}` to see every task + its key"),
            ]),
        NoCacheEntry { unit, task, key } => MonadError::new("why_no_cache_entry", err.to_string())
            .with_hint(format!(
                "no cache entry yet for {unit}:{task} (key {}) — run `monad build {unit}` or \
                 `monad ci` to produce one",
                &key[..12.min(key.len())]
            ))
            .with_next_steps(vec![
                format!("run `monad build {unit}` (or `monad ci`) to execute + cache this task"),
                format!("then retry `monad why {unit}:{task}`"),
            ]),
    }
}

fn classify_target_ref(err: &monad_core::TargetRefError) -> MonadError {
    use monad_core::TargetRefError::*;
    match err {
        NotFound {
            available_profiles,
            available_units,
            ..
        } => {
            let mut steps = Vec::new();
            if !available_profiles.is_empty() {
                steps.push(format!("available profiles: {}", available_profiles.join(", ")));
            }
            if !available_units.is_empty() {
                steps.push(format!("available units: {}", available_units.join(", ")));
            }
            if available_profiles.is_empty() && available_units.is_empty() {
                steps.push(
                    "this workspace has no profiles or units yet — run `monad init` \
                     or `monad unit add <path>`"
                        .into(),
                );
            } else {
                steps.push("run `monad plan` to see the full dependency graph".into());
            }
            MonadError::new("target_not_found", err.to_string()).with_next_steps(steps)
        }
        Ambiguous { target } => {
            let hint = format!(
                "'{target}' is used by both a monad and a unit; \
                 rename one so the verb is unambiguous"
            );
            MonadError::new("target_ambiguous", err.to_string())
                .with_hint(hint)
                .with_next_steps(vec![
                    format!(
                        "rename either the monad or the unit named '{target}' so the verb is unambiguous"
                    ),
                    "run `monad unit list` to see all known units".to_string(),
                ])
        }
    }
}

fn classify_config(err: &monad_config::ConfigError) -> MonadError {
    use monad_config::ConfigError::*;
    match err {
        Read { path, .. } => MonadError::new("config_read", err.to_string())
            .at(path_string(path))
            .with_hint("check that the file exists and is readable")
            .with_next_steps(vec![
                format!("check that {} exists", path.display()),
                format!("verify read permissions on {}", path.display()),
            ]),
        Parse { path, .. } => MonadError::new("config_parse", err.to_string())
            .at(path_string(path))
            .with_hint("the file is not valid TOML — see the line/column above")
            .with_next_steps(vec![format!(
                "open {} and fix the TOML syntax at the line/column shown in the message",
                path.display()
            )]),
        Invalid { path, .. } => MonadError::new("config_invalid", err.to_string())
            .at(path_string(path))
            .with_hint("see the schema at `monad schema` (coming soon)")
            .with_next_steps(vec![
                format!("correct the invalid field in {}", path.display()),
                "run `monad schema` to see the expected shape".to_string(),
            ]),
        Missing { path } => MonadError::new("config_missing", err.to_string())
            .at(path_string(path))
            .with_hint(format!("create {} or run the command from a directory that contains it", path.display()))
            .with_next_steps(vec![format!(
                "create {} with the expected schema",
                path.display()
            )]),
        Duplicate { kind, name, .. } => MonadError::new("config_duplicate", err.to_string())
            .with_hint(format!("rename one of the conflicting {kind}s ('{name}')"))
            .with_next_steps(vec![format!(
                "rename one of the duplicate {kind}s named '{name}' so every {kind} has a unique name"
            )]),
        DanglingUnitRef { monad, unit_path } => {
            MonadError::new("config_dangling_unit", err.to_string())
                .at(path_string(unit_path))
                .with_hint(format!(
                    "either create {}/unit.toml or remove the entry from monad '{monad}'",
                    unit_path.display()
                ))
                .with_next_steps(vec![
                    format!(
                        "create {}/unit.toml to register the unit",
                        unit_path.display()
                    ),
                    format!(
                        "or remove '{}' from the units list in monad '{monad}'",
                        unit_path.display()
                    ),
                ])
        }
    }
}

fn classify_scaffold(err: &ScaffoldError) -> MonadError {
    use ScaffoldError::*;
    const SUPPORTED_LANGS: &str =
        "go, cargo, python, python-uv, ruby, php, maven, gradle, node-npm, node-pnpm, \
         node-yarn, bun, deno";
    match err {
        MissingLanguage => MonadError::new("scaffold_missing_language", err.to_string())
            .with_hint(format!("pass --lang <one of: {SUPPORTED_LANGS}>"))
            .with_next_steps(vec![format!(
                "re-run with --lang <one of: {SUPPORTED_LANGS}>"
            )]),
        UnsupportedLanguage { .. } => MonadError::new("scaffold_unsupported_language", err.to_string())
            .with_hint(format!("supported: {SUPPORTED_LANGS}"))
            .with_next_steps(vec![format!(
                "pass --lang with one of the supported values: {SUPPORTED_LANGS}"
            )]),
        InvalidUnitPath { path, .. } => MonadError::new("scaffold_invalid_path", err.to_string())
            .at(path.clone())
            .with_hint("pick a path inside the workspace that doesn't escape via `..`")
            .with_next_steps(vec![
                "pick a unit path inside the workspace root".to_string(),
                "avoid `..` or absolute paths — unit paths must be workspace-relative".to_string(),
            ]),
        UnitPathRegistered { path } => MonadError::new("scaffold_unit_exists", err.to_string())
            .at(path.clone())
            .with_hint("pick a different path, or remove the existing unit from the monad")
            .with_next_steps(vec![
                format!("pick a different path (not '{path}') for the new unit"),
                format!("or remove '{path}' from the existing monad first"),
            ]),
        UnitNameCollision { name } => MonadError::new("scaffold_unit_exists", err.to_string())
            .with_hint(format!("pick a different directory name — '{name}' is already in use"))
            .with_next_steps(vec![format!(
                "pick a different directory name — '{name}' is already in use by another unit"
            )]),
        UnitAlreadyConfigured { path } => MonadError::new("scaffold_already_configured", err.to_string())
            .at(path.clone())
            .with_hint("remove the existing unit.toml or pick a different path")
            .with_next_steps(vec![
                format!("remove the existing unit.toml at {path} if you want to re-scaffold"),
                "or pick a different path for the new unit".to_string(),
            ]),
        LanguageUnknown { path } => MonadError::new("scaffold_language_unknown", err.to_string())
            .at(path.clone())
            .with_hint("pass --lang explicitly, or check that the project has a known manifest (go.mod, package.json, Cargo.toml, …)")
            .with_next_steps(vec![
                format!("pass --lang explicitly (one of: {SUPPORTED_LANGS})"),
                format!("or add a known manifest to {path} (go.mod, package.json, Cargo.toml, …) and retry"),
            ]),
        NoProfiles => MonadError::new("scaffold_no_profiles", err.to_string())
            .with_hint("run `monad box add <name>` first")
            .with_next_steps(vec![
                "run `monad box add <name>` to create a monad first".to_string(),
                "then re-run `monad unit add`".to_string(),
            ]),
        MultipleProfiles { available } => MonadError::new("scaffold_monad_ambiguous", err.to_string())
            .with_hint(format!("pass --monad <one of: {available}>"))
            .with_next_steps(vec![format!(
                "re-run with --monad <one of: {available}> to pick which monad owns this unit"
            )]),
        UnknownProfile { name, available } => MonadError::new("scaffold_monad_not_found", err.to_string())
            .with_hint(format!(
                "no monad named '{name}' — known profiles: {available}"
            ))
            .with_next_steps(vec![format!(
                "pass --monad with a known name — available: {available}"
            )]),
        ProfileConfigShape { path } => MonadError::new("scaffold_monad_shape", err.to_string())
            .at(path.clone())
            .with_hint("monad TOML must have a `units = [...]` array")
            .with_next_steps(vec![format!(
                "edit {path} so it has a `units = [...]` array at the top level"
            )]),
        Io { source, .. } => MonadError::new("scaffold_io", source.to_string())
            .with_hint("check that the target directory is writable and has free disk space")
            .with_next_steps(vec![
                "verify the target path is writable (check permissions)".to_string(),
                "verify there is free disk space".to_string(),
            ]),
    }
}

fn path_string(p: &Path) -> String {
    p.display().to_string()
}

/// Print a classified error. When `as_json` is true, emit exactly one JSON
/// object on stdout. Otherwise print a terse `error:` line on stderr.
pub fn emit(err: &anyhow::Error, as_json: bool) {
    if as_json {
        let structured = classify(err);
        // If serde_json somehow fails, fall back to a human line on stderr
        // so the user isn't left with nothing.
        match serde_json::to_string_pretty(&structured) {
            Ok(json) => println!("{json}"),
            Err(e) => eprintln!(
                "{}: {err:#}\n(json emit failed: {e})",
                crate::style::red("error")
            ),
        }
    } else {
        eprintln!("{}: {err:#}", crate::style::red("error"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classify_unknown_falls_back_to_internal() {
        let err = anyhow::anyhow!("something weird");
        let b = classify(&err);
        assert_eq!(b.kind, "internal");
        assert_eq!(b.message, "something weird");
        assert!(b.hint.is_none());
    }

    #[test]
    fn classify_config_parse_error() {
        let cfg = monad_config::ConfigError::Parse {
            kind: "unit.toml",
            path: PathBuf::from("apps/api/unit.toml"),
            message: "expected `=`".into(),
        };
        let err = anyhow::Error::new(cfg);
        let b = classify(&err);
        assert_eq!(b.kind, "config_parse");
        assert_eq!(b.locator.as_deref(), Some("apps/api/unit.toml"));
        assert!(b.hint.is_some());
    }

    #[test]
    fn classify_scaffold_unsupported_language() {
        let err = anyhow::Error::new(ScaffoldError::UnsupportedLanguage {
            lang: "rust".into(),
        });
        let b = classify(&err);
        assert_eq!(b.kind, "scaffold_unsupported_language");
        assert!(b.message.contains("rust"));
        let hint = b.hint.as_deref().unwrap();
        // Hint should enumerate the full SUPPORTED_LANGS set, not a
        // partial subset (regression: the hint used to drift from the
        // SUPPORTED_LANGS const, listing fewer languages).
        for lang in [
            "go",
            "cargo",
            "python",
            "python-uv",
            "ruby",
            "php",
            "maven",
            "gradle",
            "node-npm",
            "node-pnpm",
            "node-yarn",
            "bun",
            "deno",
        ] {
            assert!(hint.contains(lang), "hint missing {lang}: {hint}");
        }
    }

    #[test]
    fn classify_walks_through_anyhow_context() {
        let cfg = monad_config::ConfigError::Missing {
            path: PathBuf::from("monad.toml"),
        };
        let err = anyhow::Error::new(cfg).context("loading workspace");
        let b = classify(&err);
        assert_eq!(b.kind, "config_missing");
    }

    #[test]
    fn error_serializes_where_as_where_key() {
        let b = MonadError::new("k", "m").at("apps/api");
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"where\":\"apps/api\""), "got: {json}");
    }

    #[test]
    fn error_omits_optional_fields_when_absent() {
        let b = MonadError::new("k", "m");
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("hint"));
        assert!(!json.contains("where"));
        assert!(!json.contains("docs_url"));
    }

    #[test]
    fn empty_next_steps_serializes_as_empty_array() {
        let b = MonadError::new("k", "m");
        let json = serde_json::to_string(&b).unwrap();
        assert!(
            json.contains("\"next_steps\":[]"),
            "next_steps should always be present as an array, got: {json}"
        );
    }

    #[test]
    fn next_steps_serialize_as_array_when_present() {
        let b = MonadError::new("k", "m").with_next_steps(["step one", "step two"]);
        let json = serde_json::to_string(&b).unwrap();
        assert!(
            json.contains("\"next_steps\":[\"step one\",\"step two\"]"),
            "got: {json}"
        );
    }

    #[test]
    fn classify_target_not_found_emits_target_not_found_with_next_steps() {
        let cause = monad_core::TargetRefError::NotFound {
            target: "api".into(),
            available_profiles: vec!["prod".into()],
            available_units: vec!["web".into(), "worker".into()],
        };
        let err = anyhow::Error::new(cause);
        let b = classify(&err);
        assert_eq!(b.kind, "target_not_found");
        assert!(b.message.contains("'api'"));
        assert!(
            b.next_steps.iter().any(|s| s.contains("prod")),
            "expected 'prod' in next_steps, got {:?}",
            b.next_steps
        );
        assert!(
            b.next_steps.iter().any(|s| s.contains("web")),
            "expected 'web' in next_steps, got {:?}",
            b.next_steps
        );
    }

    #[test]
    fn classify_target_not_found_empty_workspace_suggests_init() {
        let cause = monad_core::TargetRefError::NotFound {
            target: "anything".into(),
            available_profiles: vec![],
            available_units: vec![],
        };
        let err = anyhow::Error::new(cause);
        let b = classify(&err);
        assert_eq!(b.kind, "target_not_found");
        assert!(
            b.next_steps.iter().any(|s| s.contains("monad init")),
            "expected init hint when workspace is empty, got {:?}",
            b.next_steps
        );
    }

    #[test]
    fn classify_target_ambiguous_emits_target_ambiguous() {
        let cause = monad_core::TargetRefError::Ambiguous {
            target: "shared".into(),
        };
        let err = anyhow::Error::new(cause);
        let b = classify(&err);
        assert_eq!(b.kind, "target_ambiguous");
        assert!(b.hint.is_some());
    }

    // Shape-consistency invariant: every classified error populates
    // `next_steps` with at least one entry. Agents iterate next_steps
    // uniformly without branching on hint presence.

    fn assert_has_next_steps(b: &MonadError) {
        assert!(
            !b.next_steps.is_empty(),
            "{}: next_steps must be non-empty for agent recovery",
            b.kind
        );
    }

    #[test]
    fn workspace_not_found_has_next_steps() {
        let cause = monad_core::WorkspaceNotFound {
            start: PathBuf::from("/tmp/nowhere"),
        };
        let b = classify(&anyhow::Error::new(cause));
        assert_eq!(b.kind, "workspace_not_found");
        assert_has_next_steps(&b);
        assert!(
            b.next_steps.iter().any(|s| s.contains("monad init")),
            "expected init guidance in next_steps, got {:?}",
            b.next_steps
        );
    }

    #[test]
    fn target_ambiguous_has_next_steps() {
        let cause = monad_core::TargetRefError::Ambiguous {
            target: "shared".into(),
        };
        let b = classify(&anyhow::Error::new(cause));
        assert_has_next_steps(&b);
    }

    #[test]
    fn every_config_error_has_next_steps() {
        let cases: Vec<monad_config::ConfigError> = vec![
            monad_config::ConfigError::Read {
                path: PathBuf::from("a/unit.toml"),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "x"),
            },
            monad_config::ConfigError::Parse {
                kind: "unit.toml",
                path: PathBuf::from("a/unit.toml"),
                message: "bad".into(),
            },
            monad_config::ConfigError::Invalid {
                kind: "unit.toml",
                path: PathBuf::from("a/unit.toml"),
                message: "no tasks".into(),
            },
            monad_config::ConfigError::Missing {
                path: PathBuf::from("monad.toml"),
            },
            monad_config::ConfigError::Duplicate {
                kind: "unit",
                name: "api".into(),
                path_a: PathBuf::from("apps/a/unit.toml"),
                path_b: PathBuf::from("apps/b/unit.toml"),
            },
            monad_config::ConfigError::DanglingUnitRef {
                monad: "prod".into(),
                unit_path: PathBuf::from("crates/missing"),
            },
        ];
        for cfg in cases {
            let b = classify(&anyhow::Error::new(cfg));
            assert_has_next_steps(&b);
        }
    }

    #[test]
    fn every_scaffold_error_has_next_steps() {
        let cases: Vec<ScaffoldError> = vec![
            ScaffoldError::MissingLanguage,
            ScaffoldError::UnsupportedLanguage { lang: "x".into() },
            ScaffoldError::InvalidUnitPath {
                path: "..".into(),
                reason: "escapes root".into(),
            },
            ScaffoldError::UnitPathRegistered {
                path: "apps/api".into(),
            },
            ScaffoldError::UnitNameCollision { name: "api".into() },
            ScaffoldError::UnitAlreadyConfigured {
                path: "apps/api".into(),
            },
            ScaffoldError::LanguageUnknown {
                path: "apps/api".into(),
            },
            ScaffoldError::NoProfiles,
            ScaffoldError::MultipleProfiles {
                available: "prod, staging".into(),
            },
            ScaffoldError::UnknownProfile {
                name: "x".into(),
                available: "prod".into(),
            },
            ScaffoldError::ProfileConfigShape {
                path: "profiles/prod.toml".into(),
            },
            ScaffoldError::Io {
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "x"),
            },
        ];
        for s in cases {
            let b = classify(&anyhow::Error::new(s));
            assert_has_next_steps(&b);
        }
    }

    #[test]
    fn internal_error_has_empty_next_steps() {
        // Unclassified failures stay next_steps-empty — the invariant
        // is for CLASSIFIED errors, not the catch-all.
        let b = classify(&anyhow::anyhow!("weird"));
        assert_eq!(b.kind, "internal");
        assert!(b.next_steps.is_empty());
    }
}
