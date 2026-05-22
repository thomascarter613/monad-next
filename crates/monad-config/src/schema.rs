//! Serde types for `monad.toml`, `profiles/*.toml`, and `unit.toml`.
//!
//! Each parse function returns a [`ConfigError`] with source file context.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

// ── monad.toml (repo-level defaults, optional) ─────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
    /// Repo-level toolchain pins (opt-in). When set, monad ensures each
    /// unit runs against the pinned version — installed into its own
    /// `~/.monad/tools/<tool>/<version>/` and only prepended to `PATH`
    /// for the child process that runs the task.
    ///
    /// Unites can override per-tool versions via `[toolchain]` in their
    /// own `unit.toml`.
    #[serde(default)]
    pub toolchain: ToolchainPin,
    #[serde(default)]
    pub plugins: PluginsConfig,
    /// Named deploy environments (staging, prod, preview, …) with
    /// saved secret aliases. `monad deploy --env <name>` loads the
    /// matching profile's `secrets.*` map; each entry maps a declared
    /// env-var name (what integrations look for) to a source env-var
    /// name (what the host shell / CI secret layer exports). Never
    /// holds actual secret values — only name-to-name aliases.
    #[serde(default)]
    pub environments: BTreeMap<String, Environment>,
}

/// One entry in `[environments.<name>]`. Currently only `secrets`
/// aliases; future versions may add per-env overrides for execution
/// (container image pin, fail_fast, etc.).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    /// Declared-var → source-var aliases. `RAILWAY_TOKEN =
    /// "RAILWAY_TOKEN_STAGING"` reads `$RAILWAY_TOKEN_STAGING` from
    /// the host env and exposes it to tasks under `RAILWAY_TOKEN`.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
}

/// Filters applied to subprocess plugin discovery (binaries on `$PATH`
/// matching `monad-adapter-<id>`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    /// Adapter ids to never load even if found on `$PATH`.
    #[serde(default)]
    pub disable: Vec<String>,
    /// If set, ONLY these adapter ids are loaded; anything else is
    /// skipped silently.
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default = "default_parallelism")]
    pub parallelism: usize,
    #[serde(default = "default_true")]
    pub fail_fast: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            parallelism: default_parallelism(),
            fail_fast: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    #[serde(default = "default_true")]
    pub local: bool,
    #[serde(default)]
    pub gha: GhaCache,
    /// Remote cache URL. Two schemes supported:
    ///
    /// - **`s3://<bucket>/<optional/prefix>`** — any S3-compatible
    ///   object store (AWS S3, Cloudflare R2, MinIO, Backblaze B2).
    ///   Credentials come from the standard AWS environment chain
    ///   (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, optional
    ///   `AWS_SESSION_TOKEN`). For non-AWS services also set
    ///   `remote_endpoint`.
    /// - **`monad://<host>[/<prefix>]`** — hosted monad cache (or any
    ///   server implementing the same Bearer-auth HTTP protocol).
    ///   Credential is a JWT read from the env var named by
    ///   `remote_token_env` (default: `MONAD_CACHE_TOKEN`).
    #[serde(default)]
    pub remote: Option<String>,
    /// AWS region (S3 scheme only). Required when `remote = "s3://…"`.
    /// For R2 use the literal `"auto"`; MinIO typically `"us-east-1"`.
    #[serde(default)]
    pub remote_region: Option<String>,
    /// Non-AWS S3 endpoint URL (S3 scheme only). Omit for AWS S3 —
    /// object_store derives the endpoint from the region.
    #[serde(default)]
    pub remote_endpoint: Option<String>,
    /// Name of the env var holding the bearer JWT for the
    /// `monad://` scheme. Defaults to `MONAD_CACHE_TOKEN`. Monad never
    /// stores the token itself — only the env-var name, so secrets
    /// flow through the host shell / CI secret layer.
    #[serde(default)]
    pub remote_token_env: Option<String>,
    /// On a LOCAL cache hit, also fire a background HEAD+PUT against
    /// the remote. Useful when a dev has a populated local cache
    /// pre-dating the remote-cache config — the first run after
    /// wiring the remote lazily pushes those bundles up. Unnecessary
    /// in the steady-state: the MISS→BUILT path always pushes, so
    /// teammates pulling fresh already get bundles from remote.
    ///
    /// Defaults to `false` (Turborepo / Nx convention: local is the
    /// fast lane; a local hit returns without touching the network).
    /// Set to `true` to opt into the one-time catch-up behaviour or
    /// for CI runners that share a stale local-cache volume across
    /// builds where remote is the source of truth.
    #[serde(default)]
    pub remote_write_through: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            local: true,
            gha: GhaCache::default(),
            remote: None,
            remote_region: None,
            remote_endpoint: None,
            remote_token_env: None,
            remote_write_through: false,
        }
    }
}

/// The GHA cache tier can be `true`, `false`, or `"auto"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GhaCache {
    On,
    Off,
    #[default]
    Auto,
}

impl<'de> Deserialize<'de> for GhaCache {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Bool(bool),
            Str(String),
        }
        match Repr::deserialize(de)? {
            Repr::Bool(true) => Ok(GhaCache::On),
            Repr::Bool(false) => Ok(GhaCache::Off),
            Repr::Str(s) if s == "auto" => Ok(GhaCache::Auto),
            Repr::Str(s) => Err(serde::de::Error::custom(format!(
                "expected `true`, `false`, or `\"auto\"`, got `\"{s}\"`"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    #[serde(default)]
    pub container: ContainerMode,
    #[serde(default)]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerMode {
    Auto,
    Always,
    #[default]
    Never,
}

// ── profiles/<name>.toml ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    pub name: String,
    pub units: Vec<String>,
}

// ── <unit>/unit.toml ───────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnitConfig {
    pub name: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub package_manager: Option<String>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub force_independent: bool,
    #[serde(default)]
    pub tasks: BTreeMap<String, Task>,
    #[serde(default)]
    pub serve: Option<ServeConfig>,
    #[serde(default)]
    pub toolchain: Option<ToolchainPin>,
    /// Per-integration config, keyed by integration id
    /// (`railway`, `vercel`, `sentry`, …). Each integration
    /// interprets its own sub-map. For Railway:
    /// `[integrations.railway] service = "Backend"` supplies the
    /// service name; `services = ["Frontend", "Landing Page"]`
    /// fans out to multiple deploy tasks (one per service).
    ///
    /// Values are full TOML values (strings, arrays, nested
    /// tables) so integrations aren't forced to flatten
    /// multi-value config. `UnitConfig` drops its `Eq` derive
    /// because `toml::Value` can't impl `Eq` (it holds `f64`);
    /// `PartialEq` is kept for tests. Nothing in the codebase
    /// uses `UnitConfig` as a hash key.
    ///
    /// Unknown keys don't error — integrations ignore what they
    /// don't recognise.
    #[serde(default)]
    pub integrations: BTreeMap<String, toml::Table>,
    /// Custom-script notifications — Notify-kind tasks declared inline
    /// in `unit.toml` rather than via a built-in integration. Each
    /// entry becomes a Notify task that fires after every Deploy
    /// task in this unit completes (same fan-out rules as Slack /
    /// Linear notifications). Escape hatch for bespoke hooks where
    /// writing a full `Integration` is overkill.
    ///
    /// ```toml
    /// [[notifications]]
    /// name = "github-comment"
    /// run  = "./notify-github.sh"
    /// env  = ["GITHUB_TOKEN"]
    /// required_env = ["GITHUB_TOKEN"]
    /// ```
    #[serde(default)]
    pub notifications: Vec<NotificationSpec>,
}

/// One custom-script notification declared in `unit.toml`'s `[[notifications]]`
/// array. Resolved into a Notify-kind task that fans out after each
/// Deploy in the unit.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationSpec {
    /// Task name surfaced in the ExecutionReport. Must be unique
    /// within the unit (collides with integration-emitted names like
    /// `slack:notify` the same way user `[tasks.<name>]` does — the
    /// later declaration wins).
    pub name: String,
    /// Shell command to invoke. Receives the NotificationPayload JSON on
    /// stdin — same shape as `monad schema notification-payload`.
    pub run: String,
    /// Env-var allowlist (same shape as `[tasks.<name>] env = [...]`).
    /// Names resolved through the workspace's `secret_aliases` map
    /// so notifications can consume aliased tokens uniformly.
    #[serde(default)]
    pub env: Vec<String>,
    /// Env vars that MUST be set at runtime. Preflight fails the
    /// notification (with a clear message) if any are missing — same
    /// semantics as `Integration::required_env`.
    #[serde(default)]
    pub required_env: Vec<String>,
    /// CLI binaries that MUST be on PATH at runtime. Each entry is
    /// `"binary"` or `"binary: install hint"`.
    #[serde(default)]
    pub required_cli: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Shell command to invoke. Optional: when omitted, the task inherits
    /// `run` from the adapter default, integration task, or notification with
    /// the same name — letting users add `outputs`/`inputs`/`env`/`retry`
    /// to a built-in task without re-declaring the command. A task with
    /// no `run` and no entry to inherit from is a resolve-time error.
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub inputs: Option<Vec<String>>,
    #[serde(default)]
    pub outputs: Option<Vec<String>>,
    /// Extra output globs anchored at the **monad workspace root** rather
    /// than the unit dir. Use when the build writes artefacts outside the
    /// unit — e.g. a cargo workspace member whose compiled binary lives at
    /// `<workspace-root>/target/release/<bin>`. Opt-in; the default empty
    /// set keeps existing behaviour. Restored on cache-hit into the same
    /// workspace-relative path on any machine.
    ///
    /// ```toml
    /// [tasks.build]
    /// workspace_outputs = ["target/release/my-bin"]
    /// ```
    #[serde(default)]
    pub workspace_outputs: Option<Vec<String>>,
    #[serde(default)]
    pub env: Vec<String>,
    /// Number of additional attempts to make if the task fails. Default 0
    /// (run exactly once). A task that succeeds on attempt > 1 is reported
    /// as `flaky: true` in the execution report.
    #[serde(default)]
    pub retry: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServeConfig {
    pub run: String,
}

/// Toolchain pinning. `use_system` is a known flag; all other keys are
/// treated as `<tool>=<version>` pairs (e.g. `go = "1.22.3"`). `flatten`
/// and `deny_unknown_fields` are mutually exclusive, so this struct opts
/// out of strict-schema checking.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ToolchainPin {
    #[serde(default)]
    pub use_system: bool,
    /// Per-tool version pins captured from any remaining keys.
    #[serde(default, flatten)]
    pub pins: BTreeMap<String, String>,
}

// ── Parsers ────────────────────────────────────────────────────────

pub fn parse_repo(path: &Path) -> Result<RepoConfig, ConfigError> {
    parse(path, "monad.toml")
}

pub fn parse_profile(path: &Path) -> Result<ProfileConfig, ConfigError> {
    let config: ProfileConfig = parse(path, "profiles/*.toml")?;
    validate_name("monad", &config.name, path)?;
    // An empty `units` array is valid — a freshly-initialised workspace
    // has nothing to build yet, and `monad unit add` fills it in.
    Ok(config)
}

pub fn parse_unit(path: &Path) -> Result<UnitConfig, ConfigError> {
    let config: UnitConfig = parse(path, "unit.toml")?;
    validate_name("unit", &config.name, path)?;
    for (task_name, task) in &config.tasks {
        if let Some(run) = &task.run {
            if run.trim().is_empty() {
                return Err(ConfigError::Invalid {
                    kind: "unit",
                    path: path.to_path_buf(),
                    message: format!("task '{task_name}' has an empty 'run' field"),
                });
            }
        }
    }
    Ok(config)
}

fn parse<T: for<'de> Deserialize<'de>>(path: &Path, kind: &'static str) -> Result<T, ConfigError> {
    let raw = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
        path: path.to_path_buf(),
        source: e,
    })?;
    toml::from_str(&raw).map_err(|e| ConfigError::Parse {
        kind,
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn validate_name(kind: &'static str, name: &str, path: &Path) -> Result<(), ConfigError> {
    if name.trim().is_empty() {
        return Err(ConfigError::Invalid {
            kind,
            path: path.to_path_buf(),
            message: "name must not be empty".to_string(),
        });
    }
    if name.contains('/') || name.contains(std::path::MAIN_SEPARATOR) {
        return Err(ConfigError::Invalid {
            kind,
            path: path.to_path_buf(),
            message: format!("name '{name}' must not contain path separators"),
        });
    }
    Ok(())
}

fn default_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn default_true() -> bool {
    true
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_temp(toml: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, toml).unwrap();
        (dir, path)
    }

    // ── RepoConfig ──────────────────────────────────────────

    #[test]
    fn repo_empty_is_default() {
        let config: RepoConfig = toml::from_str("").unwrap();
        assert_eq!(config, RepoConfig::default());
        assert!(config.cache.local);
        assert_eq!(config.cache.gha, GhaCache::Auto);
        assert!(config.telemetry.enabled);
        assert_eq!(config.execution.container, ContainerMode::Never);
    }

    #[test]
    fn repo_full_example() {
        let config: RepoConfig = toml::from_str(
            r#"
            [defaults]
            parallelism = 4
            fail_fast = false

            [cache]
            local = true
            gha = "auto"
            remote = "s3://monad-cache/team-xyz"
            remote_region = "us-east-1"

            [telemetry]
            enabled = false

            [execution]
            container = "auto"
            image = "ghcr.io/monad-build/runner:1"
            "#,
        )
        .unwrap();

        assert_eq!(config.defaults.parallelism, 4);
        assert!(!config.defaults.fail_fast);
        assert_eq!(config.cache.gha, GhaCache::Auto);
        assert_eq!(
            config.cache.remote.as_deref(),
            Some("s3://monad-cache/team-xyz"),
        );
        assert_eq!(config.cache.remote_region.as_deref(), Some("us-east-1"));
        assert!(!config.telemetry.enabled);
        assert_eq!(config.execution.container, ContainerMode::Auto);
    }

    #[test]
    fn repo_plugins_default_empty() {
        let config: RepoConfig = toml::from_str("").unwrap();
        assert!(config.plugins.disable.is_empty());
        assert!(config.plugins.allowlist.is_none());
    }

    #[test]
    fn repo_plugins_disable_and_allowlist_parse() {
        let config: RepoConfig = toml::from_str(
            r#"
            [plugins]
            disable = ["zig"]
            allowlist = ["erlang", "elixir"]
            "#,
        )
        .unwrap();
        assert_eq!(config.plugins.disable, vec!["zig".to_string()]);
        assert_eq!(
            config.plugins.allowlist,
            Some(vec!["erlang".into(), "elixir".into()])
        );
    }

    #[test]
    fn repo_plugins_rejects_unknown_field() {
        let err = toml::from_str::<RepoConfig>(
            r#"
            [plugins]
            mystery = ["x"]
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    #[test]
    fn repo_toolchain_pins_parse() {
        let config: RepoConfig = toml::from_str(
            r#"
            [toolchain]
            go = "1.22.3"
            node = "22.1.0"
            "#,
        )
        .unwrap();
        assert!(!config.toolchain.use_system);
        assert_eq!(config.toolchain.pins.get("go").unwrap(), "1.22.3");
        assert_eq!(config.toolchain.pins.get("node").unwrap(), "22.1.0");
    }

    #[test]
    fn repo_toolchain_use_system_alone_is_valid() {
        let config: RepoConfig = toml::from_str(
            r#"
            [toolchain]
            use_system = true
            "#,
        )
        .unwrap();
        assert!(config.toolchain.use_system);
        assert!(config.toolchain.pins.is_empty());
    }

    #[test]
    fn gha_cache_accepts_true_false_and_auto() {
        let on: RepoConfig = toml::from_str("[cache]\ngha = true").unwrap();
        assert_eq!(on.cache.gha, GhaCache::On);

        let off: RepoConfig = toml::from_str("[cache]\ngha = false").unwrap();
        assert_eq!(off.cache.gha, GhaCache::Off);

        let auto: RepoConfig = toml::from_str(
            r#"[cache]
gha = "auto""#,
        )
        .unwrap();
        assert_eq!(auto.cache.gha, GhaCache::Auto);

        let err = toml::from_str::<RepoConfig>(
            r#"[cache]
gha = "sometimes""#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("auto"), "got: {err}");
    }

    #[test]
    fn repo_rejects_unknown_field() {
        let err = toml::from_str::<RepoConfig>("[cache]\nmystery = true").unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    // ── ProfileConfig ─────────────────────────────────────────

    #[test]
    fn monad_parses_via_public_api() {
        let (_dir, path) = write_temp(
            r#"
            name = "prod"
            units = ["apps/api", "apps/web"]
            "#,
        );
        let config = parse_profile(&path).unwrap();
        assert_eq!(config.name, "prod");
        assert_eq!(config.units, vec!["apps/api", "apps/web"]);
    }

    #[test]
    fn monad_accepts_empty_units() {
        // `monad init` generates an empty monad; making that valid lets
        // users run `monad plan` immediately after init without a
        // confusing config error.
        let (_dir, path) = write_temp(
            r#"
            name = "prod"
            units = []
            "#,
        );
        let config = parse_profile(&path).unwrap();
        assert_eq!(config.name, "prod");
        assert!(config.units.is_empty());
    }

    #[test]
    fn monad_rejects_name_with_slash() {
        let (_dir, path) = write_temp(
            r#"
            name = "prod/staging"
            units = ["apps/api"]
            "#,
        );
        let err = parse_profile(&path).unwrap_err();
        assert!(err.to_string().contains("path separator"), "got: {err}");
    }

    // ── UnitConfig ──────────────────────────────────────────

    #[test]
    fn unit_parses_via_public_api() {
        let (_dir, path) = write_temp(
            r#"
            name = "sample-api"
            language = "go"
            inputs = ["**/*.go", "go.mod"]
            outputs = ["bin/api"]
            depends_on = ["sample-config"]

            [tasks.build]
            run = "go build -o bin/api ./cmd/api"

            [tasks.test]
            run = "go test ./..."
            env = ["CGO_ENABLED"]

            [serve]
            run = "air"
            "#,
        );
        let config = parse_unit(&path).unwrap();
        assert_eq!(config.name, "sample-api");
        assert_eq!(config.language.as_deref(), Some("go"));
        assert_eq!(config.inputs, vec!["**/*.go", "go.mod"]);
        assert_eq!(config.depends_on, vec!["sample-config"]);
        assert_eq!(config.tasks.len(), 2);
        assert_eq!(
            config.tasks["build"].run.as_deref(),
            Some("go build -o bin/api ./cmd/api")
        );
        assert_eq!(config.tasks["test"].env, vec!["CGO_ENABLED"]);
        assert_eq!(config.serve.as_ref().unwrap().run, "air");
    }

    #[test]
    fn unit_rejects_empty_run() {
        let (_dir, path) = write_temp(
            r#"
            name = "api"

            [tasks.build]
            run = "   "
            "#,
        );
        let err = parse_unit(&path).unwrap_err();
        assert!(err.to_string().contains("empty 'run'"), "got: {err}");
    }

    #[test]
    fn unit_allows_omitted_run_for_partial_override() {
        // Partial overrides — `[tasks.build] outputs = [...]` with no
        // `run` — parse cleanly. Resolve-time logic (not parse-time)
        // is responsible for inheriting the adapter default or erroring
        // if there's nothing to inherit from.
        let (_dir, path) = write_temp(
            r#"
            name = "api"

            [tasks.build]
            outputs = ["dist/"]
            "#,
        );
        let config = parse_unit(&path).unwrap();
        assert_eq!(config.tasks["build"].run, None);
        assert_eq!(
            config.tasks["build"].outputs.as_deref(),
            Some(&["dist/".to_string()][..])
        );
    }

    #[test]
    fn unit_rejects_empty_name() {
        let (_dir, path) = write_temp(r#"name = """#);
        let err = parse_unit(&path).unwrap_err();
        assert!(err.to_string().contains("must not be empty"), "got: {err}");
    }

    #[test]
    fn unit_minimal() {
        let (_dir, path) = write_temp(r#"name = "api""#);
        let config = parse_unit(&path).unwrap();
        assert_eq!(config.name, "api");
        assert!(config.tasks.is_empty());
        assert!(config.serve.is_none());
        assert!(!config.force_independent);
    }

    #[test]
    fn toolchain_pin_parses_per_tool_pins() {
        let (_dir, path) = write_temp(
            r#"
            name = "api"

            [toolchain]
            go = "1.22.3"
            node = "22.1.0"
            "#,
        );
        let config = parse_unit(&path).unwrap();
        let toolchain = config.toolchain.unwrap();
        assert!(!toolchain.use_system);
        assert_eq!(toolchain.pins.get("go").unwrap(), "1.22.3");
        assert_eq!(toolchain.pins.get("node").unwrap(), "22.1.0");
    }

    // ── error surface ───────────────────────────────────────

    #[test]
    fn parse_error_preserves_path_context() {
        let (_dir, path) = write_temp("name = [not-valid-toml");
        let err = parse_unit(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unit.toml"), "got: {msg}");
        assert!(msg.contains(&path.display().to_string()), "got: {msg}");
    }

    #[test]
    fn missing_file_returns_read_error() {
        let err = parse_unit(Path::new("/nonexistent/unit.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }));
    }
}
