use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// monad — Governance-grade polyglot repository runtime for native-tool orchestration, safe evolution, and AI-ready context.
///
/// Wraps native package managers (Go, Bun, Deno, npm/pnpm, Cargo) and only
/// rebuilds what changed via content hashing. Multi-tier cache: local, CI,
/// remote. One-line GitHub Action.
#[derive(Parser, Debug)]
#[command(
    name = "monad",
    version,
    about = "Governance-grade polyglot repository runtime for native-tool orchestration, safe evolution, and AI-ready context.",
    propagate_version = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalFlags,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug, Clone, Default)]
pub struct GlobalFlags {
    /// Emit machine-readable JSON output
    #[arg(long, global = true)]
    pub json: bool,

    /// Bypass cache lookups (still writes results to cache)
    #[arg(long, global = true)]
    pub no_cache: bool,

    /// Restrict to a specific monad (by name)
    #[arg(long, value_name = "NAME", global = true)]
    pub monad: Option<String>,

    /// Base ref for change detection (default: origin/main)
    #[arg(long, value_name = "REF", global = true)]
    pub since: Option<String>,

    /// Enable verbose (debug) tracing
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Write the structured ExecutionReport JSON to this path (in addition
    /// to whatever the command prints to stdout). Used by the GitHub Action
    /// to expose the report as a step output without parsing stdout.
    #[arg(long, global = true, value_name = "PATH")]
    pub report_file: Option<PathBuf>,

    /// Skip the adapter install probe entirely. Use when deps are
    /// already populated (e.g. containerised CI) and the probe cost
    /// is wasted. Does not affect individual task runs.
    #[arg(long, global = true)]
    pub skip_install: bool,

    /// Force `adapter.install()` to run regardless of the probe.
    /// Useful when the probe can't detect a subtle corruption that's
    /// tripping builds.
    #[arg(long, global = true)]
    pub force_install: bool,

    /// Point monad at a workspace other than the current directory.
    /// Resolution order: --workspace > $MONAD_WORKSPACE_ROOT > cwd.
    /// Lets MCP servers, CI harnesses, and orchestration wrappers
    /// run monad against a workspace they haven't `cd`'d into.
    #[arg(long, value_name = "PATH", global = true, env = "MONAD_WORKSPACE_ROOT")]
    pub workspace: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    // ── Scaffolding ────────────────────────────────────────────────
    /// Scaffold monad.toml and profiles/ in the current repo. By default,
    /// walks subdirectories looking for languages monad knows about and
    /// adopts each as a unit (writes only unit.toml, sources untouched).
    Init {
        /// Skip unit detection — write only the placeholder monad.toml +
        /// empty profiles/prod.toml, equivalent to old init behaviour.
        #[arg(long)]
        no_detect: bool,
    },

    /// Convert a competing monorepo tool's config into monad config.
    /// Reads the source tool's config (turbo.json, nx.json, etc.) plus
    /// per-package manifests; emits a workspace monad.toml, per-package
    /// unit.toml, and a starter profiles/prod.toml. Refuses to overwrite
    /// existing files unless `--force` is set.
    #[command(subcommand)]
    Migrate(MigrateSource),

    /// Manage units (apps/services inside a monad)
    #[command(subcommand)]
    Unit(UnitAction),

    /// Manage profiles (deployment units; the outer boxes)
    #[command(subcommand)]
    Box(BoxAction),

    // ── Planning + execution ───────────────────────────────────────
    /// Orient an agent to this workspace — inventory, cache state,
    /// plan preview, and a recommended next verb. Meant as the first
    /// command an agent (or human) runs in a fresh session.
    ///
    /// Unlike `monad doctor`, prime is advisory — nothing fails, every
    /// field is informational. Runs without executing tasks and without
    /// network (use `monad doctor --cloud` for reachability checks).
    Prime,

    /// Show what would build, and why
    Plan {
        /// Monad or unit name (omit for every monad in the workspace).
        /// Same target shape as `build` / `test` / `lint` / `deploy`.
        target: Option<String>,
    },

    /// Plan and execute — the GitHub Action entry point
    Ci,

    /// Install unit dependencies (node_modules, vendor, .venv, …) via
    /// each adapter's native command. The one-liner equivalent of
    /// running `npm ci` / `go mod download` / `composer install` /
    /// `pnpm install` / … per unit, so agents never need to remember
    /// which tool goes with which unit.
    Install {
        /// Monad or unit name. Omit to install every unit.
        target: Option<String>,

        /// Run install unconditionally, ignoring the adapter's probe.
        /// Useful when the probe can't see a subtle `node_modules`
        /// corruption that's tripping builds. Equivalent to the
        /// global `--force-install` flag — use whichever reads better
        /// (`monad install --force` for the install verb itself,
        /// `monad ci --force-install` to force re-install before any
        /// other verb).
        #[arg(long)]
        force: bool,
    },

    /// Build a monad or single unit
    Build {
        /// Monad or unit name (omit for all profiles)
        target: Option<String>,
    },

    /// Type-check a monad or single unit — the language-native
    /// fast-feedback verb (`cargo check`, `go vet`, …). Order of
    /// magnitude faster than `monad build` for catching compile / type
    /// errors during agent iteration loops. Adapter defaults exist
    /// for cargo and go; other ecosystems run the verb only when the
    /// unit defines `[tasks.check]`.
    Check {
        /// Monad or unit name (omit for all profiles)
        target: Option<String>,
    },

    /// Test a monad or single unit
    Test {
        /// Monad or unit name (omit for all profiles)
        target: Option<String>,
    },

    /// Lint a monad or single unit
    Lint {
        /// Monad or unit name (omit for all profiles)
        target: Option<String>,
    },

    /// Deploy units with active deploy integrations (Vercel, Railway, …)
    Deploy {
        /// Monad or unit name. Omit to deploy every unit with a matching
        /// integration task.
        target: Option<String>,

        /// Run preview / staging deploys instead of production
        /// (e.g. `vercel deploy` without `--prod`).
        #[arg(long, conflicts_with = "rollback")]
        preview: bool,

        /// Roll back to the previous deploy. Integrations that don't
        /// support rollback will skip their unit with a clear message.
        #[arg(long)]
        rollback: bool,

        /// Named deploy environment from `monad.toml`
        /// (`[environments.<name>]`). Applies that environment's
        /// `secrets.*` aliases before running.
        #[arg(long, value_name = "NAME")]
        env: Option<String>,

        /// Alias a declared env-var name to a source env-var name,
        /// reading the value from the source and exposing it to the
        /// task under the declared name. Repeatable. Overrides
        /// anything from `--env`. Format: `DECLARED=SOURCE` — e.g.
        /// `--secret-from RAILWAY_TOKEN=RAILWAY_TOKEN_STAGING`.
        /// Never pass literal secret values here; always point at a
        /// host env var.
        #[arg(long, value_name = "DECLARED=SOURCE", value_parser = parse_secret_alias)]
        secret_from: Vec<(String, String)>,

        /// Skip Notify-kind integration tasks (notifications — Slack
        /// posts, Linear status flips, etc). Use when re-deploying
        /// after a fix and you don't want to spam the same channel
        /// twice.
        #[arg(long)]
        no_notify: bool,

        /// Always run Deploy / DeployPreview tasks, even when their
        /// inputs match the last successful deploy on record. Without
        /// this, monad short-circuits unchanged deploys against
        /// `.monad/state/deploys.json`.
        #[arg(long)]
        force: bool,
    },

    /// Re-fire Notify-kind integration tasks (notifications) using the
    /// last deploy's payload — useful after fixing a broken webhook
    /// without re-deploying the code.
    Notify {
        /// Monad or unit name. Omit to notify every unit with a prior
        /// deploy on record.
        target: Option<String>,

        /// Named deploy environment from `monad.toml`
        /// (`[environments.<name>]`). Applies that environment's
        /// `secrets.*` aliases before running — typically the same
        /// env you passed to the original `monad deploy`.
        #[arg(long, value_name = "NAME")]
        env: Option<String>,

        /// Alias a declared env-var name to a source env-var name.
        /// Same semantics as `monad deploy --secret-from`.
        #[arg(long, value_name = "DECLARED=SOURCE", value_parser = parse_secret_alias)]
        secret_from: Vec<(String, String)>,
    },

    // ── Dev experience ─────────────────────────────────────────────
    /// Run all units in a monad with hot reload. `<monad>` is required —
    /// pass the monad name from `monad box list`.
    Serve {
        /// Monad to serve
        monad: String,
    },

    /// Run a single unit in dev mode. `<unit>` is required — pass the
    /// unit name from `monad unit list`.
    Dev {
        /// Unit to run
        unit: String,
    },

    /// Add one or more packages as dependencies of a unit. Wraps the
    /// unit's native package manager (`cargo add`, `bun add`, `npm
    /// install --save`, `pnpm add`, `yarn add`, `go get`) so agents
    /// don't need to know which one applies. Lockfiles + manifests
    /// are updated by the underlying tool.
    ///
    /// Examples:
    ///   monad add tailwindcss --unit dashboard --dev
    ///   monad add tokio anyhow --unit control-plane
    ///
    /// In a single-unit workspace `--unit` can be omitted; multi-unit
    /// workspaces require it (or a positional cwd resolution).
    /// Adapters without a dev / runtime split (Go) ignore `--dev` and
    /// surface the silent demotion as a per-package `note` in the
    /// JSON output.
    Add {
        /// Package specs. Format is adapter-specific — bare names
        /// (`tailwindcss`), version pins (`tailwindcss@3.4.0`), or
        /// the package manager's own grammar (`serde`, `tokio` for
        /// cargo). At least one package required.
        #[arg(required = true)]
        packages: Vec<String>,

        /// Unit to add to. Required when the workspace has more than
        /// one unit; inferred when there's exactly one.
        #[arg(long, value_name = "UNIT")]
        unit: Option<String>,

        /// Add as a dev / build-time dependency. Maps to the package
        /// manager's native dev flag (`cargo add --dev`, `bun add -d`,
        /// `npm install --save-dev`, `pnpm add -D`, `yarn add --dev`).
        #[arg(long)]
        dev: bool,
    },

    /// Invoke a named `[tasks.<name>]` block in a unit, forwarding any
    /// trailing args. Bypasses the cache — use `monad build|test|lint`
    /// for cacheable lifecycle verbs. Stdout/stderr stream straight
    /// through; monad's own status output goes to stderr only.
    ///
    /// Example — given a unit with `[tasks.admin] run = "cargo run --bin my-admin -- \"$@\""`,
    /// invoke it as: `monad run server admin -- migrate --dry-run`
    ///
    /// Use `--` to separate task args from monad's own flags. Args
    /// after `--` are passed to `sh -c` as positional `$1..$N`, so
    /// shell quoting in `run` works the way you'd expect.
    Run {
        /// Unit name (looks up `[tasks.<task>]` in `<unit>/unit.toml`).
        unit: String,
        /// Task name. Must match a `[tasks.<task>]` block with an
        /// explicit `run = "..."` field.
        task: String,
        /// Trailing args forwarded to the task command. The leading
        /// `--` is consumed by clap; everything after it lands here
        /// as positional arguments to the spawned shell.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    // ── Cache ──────────────────────────────────────────────────────
    /// Cache management
    #[command(subcommand)]
    Cache(CacheAction),

    // ── Secrets ────────────────────────────────────────────────────
    /// Manage deploy-target secrets (Cloudflare Workers/Pages, Railway).
    /// Thin wrapper over each platform's native CLI — values never
    /// enter monad's surface beyond the single put call.
    #[command(subcommand)]
    Secret(SecretAction),

    // ── Debugging + introspection ──────────────────────────────────
    /// Explain why a task's cache entry is what it is — prints the
    /// full input manifest (every hashed file, toolchain, env var).
    /// Accepts either `<unit>:<task>` (e.g. `monad why marketing:lint`)
    /// or a hex prefix of a cache key (any prefix; get one from
    /// `monad plan --json` or `monad ci --json`).
    Why {
        /// Target: either `<unit>:<task>` or a cache-key hex prefix.
        target: String,
    },

    /// Print the dependency graph
    Graph {
        /// Monad name (omit for all profiles)
        monad: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value_t = GraphFormat::Ascii)]
        format: GraphFormat,
    },

    /// Health check: config, toolchains, cache, integrations
    Doctor {
        /// Named deploy environment from `monad.toml`
        /// (`[environments.<name>]`). Integration env-var checks
        /// look up aliased source names instead of the declared ones
        /// — e.g. `monad doctor --env staging` checks whether
        /// `$RAILWAY_TOKEN_STAGING` is set instead of
        /// `$RAILWAY_TOKEN`, and the same alias-lookup applies to
        /// every integration that uses `[environments.*]` (Cloudflare
        /// Workers / Pages, Vercel, Slack, Linear).
        #[arg(long, value_name = "NAME")]
        env: Option<String>,

        /// Ad-hoc alias (see `monad deploy --secret-from`). Repeatable.
        /// Overrides anything from `--env`.
        #[arg(long, value_name = "DECLARED=SOURCE", value_parser = parse_secret_alias)]
        secret_from: Vec<(String, String)>,

        /// Add cloud checks: validate the monad:// remote-cache JWT,
        /// ping cache.monad.build/health, ping api.monad.build/v1/healthz.
        /// Off by default since the rest of doctor is non-network.
        #[arg(long)]
        cloud: bool,
    },

    /// List resolved output paths per unit (post-build artefact paths)
    Artifacts,

    /// Print JSON Schema for agent-consumable output types
    Schema {
        /// Schema to print; omit to list available schemas
        #[arg(value_enum)]
        target: Option<SchemaTarget>,
    },

    // ── MCP ────────────────────────────────────────────────────────
    /// Manage the MCP (Model Context Protocol) server entry across
    /// agent clients (Claude Code, Claude Desktop, Cursor, Windsurf).
    #[command(subcommand)]
    Mcp(McpAction),

    // ── Toolchains ─────────────────────────────────────────────────
    /// Toolchain management — install, list, and pin language versions
    #[command(subcommand)]
    Toolchain(ToolchainAction),

    /// Cut a release: bump workspace version, refresh Cargo.lock,
    /// commit, tag locally. Does not push — prints the push commands
    /// for you to run after reviewing.
    Release {
        /// Version to cut. `X.Y.Z` for an explicit version, or one of
        /// `patch` / `minor` / `major` to bump relative to the current
        /// workspace version.
        #[arg(value_name = "VERSION")]
        spec: String,
    },

    /// Sign in to monad.build and stash the returned JWT in the OS
    /// keychain (or `~/.monad/credentials` as a 0600 fallback).
    /// After this, `monad build|ci|…` pick up the token automatically
    /// and you can stop setting `MONAD_CACHE_TOKEN` by hand.
    Login,

    // ── Internal (agent / integration use only) ────────────────────
    /// Internal: Slack webhook poster. Invoked by `SlackIntegration`'s
    /// emitted task; reads a NotificationPayload on stdin and POSTs. Not
    /// a user-facing verb.
    #[command(name = "_slack-post", hide = true)]
    SlackPost {
        /// Env-var name holding the webhook URL.
        #[arg(long, value_name = "NAME", default_value = "SLACK_WEBHOOK_URL")]
        webhook_env: String,

        /// Optional channel override (webhooks pin one at creation time).
        #[arg(long)]
        channel: Option<String>,

        /// Optional username override.
        #[arg(long)]
        username: Option<String>,
    },

    /// Internal: Linear issue transitioner. Invoked by
    /// `LinearIntegration`; reads a NotificationPayload on stdin,
    /// extracts issue IDs, transitions them to a target state via
    /// Linear GraphQL.
    #[command(name = "_linear-notify", hide = true)]
    LinearNotify {
        /// Env-var name holding the Linear Personal API key.
        #[arg(long, value_name = "NAME", default_value = "LINEAR_API_KEY")]
        api_key_env: String,

        /// Workflow-state name to transition matched issues to.
        #[arg(long, default_value = "Deployed")]
        target_state: String,

        /// Fallback issue ID for comments when no issue refs match.
        #[arg(long)]
        fallback_issue_id: Option<String>,

        /// Optional team key to scope state lookups.
        #[arg(long)]
        team: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum MigrateSource {
    /// Migrate a Turborepo workspace. Reads root turbo.json (v2 `tasks`
    /// or v1 `pipeline`), discovers packages via root package.json's
    /// `workspaces` glob, emits per-package unit.toml + workspace
    /// monad.toml + profiles/prod.toml. Per-package turbo.json overrides
    /// are detected and noted but not currently applied — surface
    /// in the migration report so users can hand-port them.
    Turbo {
        /// Workspace root containing turbo.json. Defaults to cwd.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Show what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing monad.toml / unit.toml / profiles/prod.toml.
        /// Without this, the migrator refuses to clobber any of those.
        #[arg(long)]
        force: bool,
    },

    /// Migrate an Nx workspace. Reads root nx.json (targetDefaults,
    /// namedInputs, workspaceLayout) plus per-project project.json
    /// files; emits per-project unit.toml + workspace monad.toml +
    /// profiles/prod.toml. Common Nx executors map to canonical CLI
    /// invocations (`@nx/vite:build` → `vite build`, `@nx/jest:jest`
    /// → `jest`, …); unknown executors emit `nx run …` shims with
    /// an Inferred note. Configurations and per-target dependsOn are
    /// surfaced as notes — monad derives task ordering from the unit
    /// graph, not per-target deps.
    Nx {
        /// Workspace root containing nx.json. Defaults to cwd.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Show what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing monad.toml / unit.toml / profiles/prod.toml.
        /// Without this, the migrator refuses to clobber any of those.
        #[arg(long)]
        force: bool,
    },

    /// Migrate a Lerna workspace. Reads `lerna.json` (packages glob,
    /// useWorkspaces, npmClient) plus each package's `package.json`
    /// scripts. Emits per-package unit.toml mirroring scripts as
    /// `[tasks.<name>]` blocks plus workspace monad.toml + profiles/prod.toml.
    /// Lerna's task graph is shallow (no cross-package dependsOn) so
    /// unit-level ordering is left to the user with a TODO note.
    Lerna {
        /// Workspace root containing lerna.json. Defaults to cwd.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Show what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing monad.toml / unit.toml / profiles/prod.toml.
        #[arg(long)]
        force: bool,
    },

    /// Best-effort `Makefile` migrator. Parses top-level targets,
    /// prerequisites (treated as `dependsOn`), and recipe lines (treated
    /// as shell commands). Cannot translate variable expansion, pattern
    /// rules, or automatic variables — those surface as notes the user
    /// must hand-port. `.PHONY` targets handled best-effort. Single-unit
    /// shape (the Makefile root becomes one monad with one unit).
    Make {
        /// Directory containing the Makefile. Defaults to cwd.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Show what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing monad.toml / unit.toml / profiles/prod.toml.
        #[arg(long)]
        force: bool,
    },

    /// Migrate a moonrepo workspace. Reads `.moon/workspace.yml`
    /// (project glob patterns, vcs, runner) plus each project's
    /// `moon.yml`. Maps moon's task definitions (`command`, `deps`,
    /// `inputs`, `outputs`, `options.cache`, `platform`) onto monad
    /// unit tasks. Moon's first-class language toolchain blocks
    /// (`rust`, `node`, `deno`) surface as notes — monad doesn't
    /// have a direct equivalent yet.
    Moon {
        /// Workspace root containing `.moon/workspace.yml`. Defaults
        /// to cwd.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Show what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing monad.toml / unit.toml / profiles/prod.toml.
        #[arg(long)]
        force: bool,
    },

    /// Migrate a Rush.js workspace. Reads `rush.json` (projects array
    /// with packageName + projectFolder) plus each project's
    /// `package.json` scripts and Rush-specific
    /// `config/rush/command-line.json` (custom bulk commands). Emits
    /// per-package unit.toml mirroring scripts; bulk commands surface
    /// as notes for the user to wire up manually.
    Rush {
        /// Workspace root containing rush.json. Defaults to cwd.
        #[arg(long, value_name = "DIR")]
        path: Option<PathBuf>,

        /// Show what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing monad.toml / unit.toml / profiles/prod.toml.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum UnitAction {
    /// Scaffold a new unit (source files + unit.toml) and wire it into a monad
    Add {
        /// Path to the unit directory (relative to workspace root)
        path: PathBuf,

        /// Language ecosystem to scaffold. Accepted values: `go`,
        /// `cargo`, `python`, `python-uv`, `ruby`, `php`, `maven`,
        /// `gradle`, `node-npm`, `node-pnpm`, `node-yarn`, `bun`,
        /// `deno`. Required when `<path>` is an empty directory;
        /// auto-detected when adopting an existing unit.
        #[arg(long, value_name = "LANG")]
        lang: Option<String>,
    },

    /// List every unit in the workspace with its path, language, and
    /// which profiles include it. Flags orphan `unit.toml` files on disk
    /// that aren't wired into any monad.
    List,
}

#[derive(Subcommand, Debug)]
pub enum BoxAction {
    /// Create a new monad definition
    Add {
        /// Monad name (becomes profiles/<name>.toml)
        name: String,
    },

    /// List every monad in the workspace with its source file and
    /// the units it includes.
    List,
}

#[derive(Subcommand, Debug)]
pub enum CacheAction {
    /// Hit rate, size, and location per tier
    Stats,

    /// Clear the local cache
    Clear,

    /// Push the local cache to remote (force)
    Push,

    /// Pull the remote cache to local (force)
    Pull,
}

#[derive(Subcommand, Debug)]
pub enum SecretAction {
    /// Set or update a secret on the unit's deploy target. Value is
    /// read from stdin so agents can pipe it in; use
    /// `echo -n "$VALUE" | monad secret put <target> <name>`.
    Put {
        /// Unit name, optionally `<unit>:<integration>` when the unit
        /// has more than one secret-capable integration.
        target: String,

        /// Secret name. Platform-specific naming rules apply.
        name: String,
    },

    /// List secret names (not values) on the unit's deploy target.
    List {
        /// Unit name, optionally `<unit>:<integration>`.
        target: String,
    },

    /// Delete a secret on the unit's deploy target.
    Delete {
        /// Unit name, optionally `<unit>:<integration>`.
        target: String,

        /// Secret name to remove.
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// Register `monad-mcp` in one or more agent clients' MCP config.
    /// Supported clients: Claude Code (`~/.claude.json`), Claude
    /// Desktop, Cursor (`~/.cursor/mcp.json`), Windsurf
    /// (`~/.codeium/windsurf/mcp_config.json`), Codex CLI
    /// (`~/.codex/config.toml`), OpenCode
    /// (`~/.config/opencode/opencode.json`), and Zed
    /// (`~/.config/zed/settings.json`). Idempotent — re-running
    /// updates the existing entry.
    Install {
        /// Which client(s) to install for. `auto` (default) detects every
        /// installed client and registers in each.
        #[arg(value_enum, default_value_t = McpClient::Auto)]
        client: McpClient,

        /// Write the project-local config (`.cursor/mcp.json`,
        /// `.mcp.json` at the repo root for Claude Code) instead of
        /// the user-global one. Only meaningful for clients that
        /// support project-local config.
        #[arg(long)]
        local: bool,

        /// Bake `--workspace <PATH>` into the registered command so
        /// `monad-mcp` always pins to that workspace. Defaults to none
        /// (`monad-mcp` resolves cwd at startup). NOTE: this flag is
        /// independent of monad's global `--workspace` /
        /// `$MONAD_WORKSPACE_ROOT` — it controls what gets *written
        /// into the MCP config file*, not where `monad mcp install`
        /// itself runs. Pass an absolute path you want the MCP server
        /// to anchor on.
        #[arg(long, value_name = "PATH")]
        workspace: Option<std::path::PathBuf>,

        /// Override the server-key written into the config (default
        /// `monad` — surfaces as `mcp__monad__<verb>` in client tool
        /// pickers). Useful when a workspace already has an entry
        /// named `monad` pointing somewhere else.
        #[arg(long, default_value = "monad")]
        name: String,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum McpClient {
    /// Auto-detect every installed client and register in each.
    Auto,
    /// Claude Code (anthropic CLI). Config: `~/.claude.json` (user) or
    /// `.mcp.json` at the repo root (with `--local`).
    ClaudeCode,
    /// Claude Desktop (anthropic desktop app). Config:
    /// `~/Library/Application Support/Claude/claude_desktop_config.json`
    /// (macOS) / equivalent paths on Windows + Linux.
    ClaudeDesktop,
    /// Cursor IDE. Config: `~/.cursor/mcp.json` or `.cursor/mcp.json`
    /// (with `--local`).
    Cursor,
    /// Windsurf IDE (Codeium). Config:
    /// `~/.codeium/windsurf/mcp_config.json`.
    Windsurf,
    /// Codex CLI (OpenAI). Config: `~/.codex/config.toml` (user) or
    /// `.codex/config.toml` at the repo root (with `--local`). TOML
    /// shape — entries land under `[mcp_servers.<name>]`.
    Codex,
    /// OpenCode (sst/opencode). Config:
    /// `~/.config/opencode/opencode.json` (user) or `opencode.json`
    /// at the repo root (with `--local`).
    Opencode,
    /// Zed editor. Config: `~/.config/zed/settings.json` (user) or
    /// `.zed/settings.json` at the repo root (with `--local`). MCP
    /// servers register under the top-level `context_servers` key.
    Zed,
}

#[derive(Subcommand, Debug)]
pub enum ToolchainAction {
    /// List installed toolchains
    List,

    /// Install missing toolchains for the current project
    Install,

    /// Pin a toolchain version (e.g. `go=1.22.3`)
    Pin {
        /// `<tool>=<version>` (e.g. `go=1.22.3`)
        pin: String,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum GraphFormat {
    Ascii,
    Dot,
}

/// Output types for which `monad schema` can emit JSON Schema.
///
/// These are the stable agent-integration contract. Bumping the shape of
/// any of these is a breaking change.
#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum SchemaTarget {
    /// Output of `monad plan` (and `--json`).
    Plan,
    /// Output of `monad ci`, `monad build|test|lint`.
    Report,
    /// Output of `monad why <hash>`.
    Why,
    /// Output of `monad unit add --json`.
    Scaffold,
    /// The InputManifest sidecar written alongside each cache entry.
    Manifest,
    /// Output of `monad doctor`.
    Doctor,
    /// The structured error envelope emitted on any failure with `--json`.
    Error,
    /// Structured tool diagnostics — the `diagnostics` array on each
    /// failed task in an ExecutionReport. Compiler/linter records.
    Diagnostics,
    /// Notification payload piped on stdin to Notify-kind integration
    /// tasks after a Deploy task completes.
    NotificationPayload,
    /// Output of `monad prime` (and `--json`).
    Prime,
}

/// Parse `DECLARED=SOURCE` into a name pair. Rejects empty halves so
/// `--secret-from =SOMETHING` and `--secret-from NAME=` both fail at
/// the flag layer rather than silently disabling the alias. Passed to
/// clap via `value_parser`.
fn parse_secret_alias(s: &str) -> Result<(String, String), String> {
    let Some((declared, source)) = s.split_once('=') else {
        return Err(format!("expected DECLARED=SOURCE, got `{s}`"));
    };
    let declared = declared.trim();
    let source = source.trim();
    if declared.is_empty() {
        return Err("declared env-var name is empty".to_string());
    }
    if source.is_empty() {
        return Err("source env-var name is empty".to_string());
    }
    // Catch the most common footgun: an agent passing the resolved
    // *value* (from `${{ secrets.FOO }}`) instead of an env-var name.
    // Real env-var names are shell-identifier-shaped; values typically
    // contain other characters.
    if !source
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
        || source.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return Err(format!(
            "source `{source}` doesn't look like an env-var name \
             (alphanumerics + underscore, not starting with a digit). \
             Did you pass the secret value instead of a var name?"
        ));
    }
    Ok((declared.to_string(), source.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_monad_verb_in_in_tree_docs_resolves_to_a_real_subcommand() {
        // Ground truth: build the verb set by walking the clap CommandFactory
        // tree. Includes top-level subcommands AND second-level sub-actions
        // (e.g. `unit add`, `box list`, `cache push`).
        let mut verbs: std::collections::HashSet<String> = std::collections::HashSet::new();
        let cmd = Cli::command();
        for sub in cmd.get_subcommands() {
            let name = sub.get_name().to_string();
            verbs.insert(name.clone());
            for sub2 in sub.get_subcommands() {
                verbs.insert(format!("{} {}", name, sub2.get_name()));
            }
        }
        // Resolve repository-root docs from Cargo's compile-time crate root
        // instead of relying on the process current working directory.
        //
        // `env!("CARGO_MANIFEST_DIR")` is expanded by Rust at compile time
        // to the package directory for `monad-cli`, which should be:
        //
        //   <repo>/crates/monad-cli
        //
        // From there, `../..` resolves back to the repository root.
        // This makes the test stable even if the runtime current directory
        // changes during test execution.
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        // In-tree agent-facing docs. Paths are relative to the repository root.
        let docs = [
            "README.md",
            "CHANGELOG.md",
            "CLAUDE.md",
            "docs/agents.md",
            "docs/configuration.md",
            "docs/deploying.md",
            "docs/plugins.md",
            "docs/adopt-existing-repo.md",
            "docs/new-project.md",
            "skills/monad/SKILL.md",
        ];

        let mut failures: Vec<String> = Vec::new();
        for relative_path in docs {
            let path = repo_root.join(relative_path);
            let body = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            //   - inside a fenced code block, lines that begin with
            //     `monad ` (after optional `$ ` / `> ` prompt + lead
            //     whitespace) are the user being asked to type the
            //     command.
            //   - outside code blocks, code spans whose content begins
            //     with `monad ` are the same — `` `monad install` ``.
            // Anything else (prose, sample-output rows, comments) is
            // not an invocation and is left alone.
            let mut in_code_block = false;
            for (line_no, line) in body.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("```") {
                    in_code_block = !in_code_block;
                    continue;
                }
                let candidates: Vec<&str> = if in_code_block {
                    invocation_candidate(line).into_iter().collect()
                } else {
                    code_span_segments(line)
                        .into_iter()
                        .filter_map(invocation_candidate)
                        .collect()
                };
                for after_profile in candidates {
                    let Some(token) = parse_verb_token(after_profile) else {
                        continue;
                    };
                    let words: Vec<&str> = token.split_whitespace().collect();
                    let one = words.first().copied().unwrap_or("").to_string();
                    let two = if words.len() >= 2 {
                        format!("{} {}", words[0], words[1])
                    } else {
                        one.clone()
                    };
                    if !verbs.contains(&one) && !verbs.contains(&two) {
                        failures.push(format!(
                            "{}:{} → `monad {}` — not in CLI subcommand set",
                            relative_path,
                            line_no + 1,
                            token,
                        ));
                    }
                }
            }
        }

        if !failures.is_empty() {
            let mut msg = String::from(
                "doc → CLI verb drift (every `monad <verb>` mention in in-tree docs \
                              must resolve to a real clap subcommand):\n",
            );
            for f in &failures {
                msg.push_str("  ");
                msg.push_str(f);
                msg.push('\n');
            }
            panic!("{msg}");
        }
    }

    /// Strip leading whitespace + shell-prompt + the `monad ` literal
    /// from `text`. Returns `Some(rest)` when `text` looks like an
    /// invocation (the user is being asked to type `monad ...`),
    /// `None` for prose, comments, or non-monad commands.
    fn invocation_candidate(text: &str) -> Option<&str> {
        let s = text.trim_start();
        let s = s.strip_prefix("$ ").unwrap_or(s);
        let s = s.strip_prefix("> ").unwrap_or(s);
        s.strip_prefix("monad ")
    }

    /// Given the substring after `monad `, return the verb token
    /// (one- or two-word form) or `None` if this looks like a flag,
    /// a placeholder, or a field reference (`monad version: 0.1`).
    fn parse_verb_token(after_profile: &str) -> Option<String> {
        let bytes = after_profile.as_bytes();
        if bytes.is_empty() {
            return None;
        }
        // Reject `monad -<flag>` — that's a global flag, not a verb.
        // Reject `monad <placeholder>` — explicit angle-bracket form.
        if bytes[0] == b'-' || bytes[0] == b'<' {
            return None;
        }
        let first = take_word(bytes);
        if first.is_empty() {
            return None;
        }
        let after_first = first.len();
        // Reject sample-output forms `monad foo: bar` — the colon
        // means `foo` is a field, not a verb.
        if after_first < bytes.len() && bytes[after_first] == b':' {
            return None;
        }
        let first_str = std::str::from_utf8(first).expect("ascii guaranteed");
        let mut whole = first_str.to_string();
        if after_first < bytes.len() && bytes[after_first] == b' ' {
            let second = take_word(&bytes[after_first + 1..]);
            if !second.is_empty() {
                let second_str = std::str::from_utf8(second).expect("ascii guaranteed");
                whole.push(' ');
                whole.push_str(second_str);
            }
        }
        Some(whole)
    }

    fn take_word(bytes: &[u8]) -> &[u8] {
        let mut end = 0;
        while end < bytes.len() {
            let c = bytes[end];
            if c.is_ascii_lowercase() || c == b'-' || c == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        &bytes[..end]
    }

    /// Return the substrings of `line` that sit between matching
    /// backtick pairs — the code-span content. Unbalanced backticks
    /// (a single ` ` ` on a line) are treated as opening a span that
    /// runs to end-of-line, matching how most markdown renderers cope.
    fn code_span_segments(line: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'`' {
                i += 1;
                continue;
            }
            let span_start = i + 1;
            let mut j = span_start;
            while j < bytes.len() && bytes[j] != b'`' {
                j += 1;
            }
            if j > span_start {
                out.push(&line[span_start..j]);
            }
            i = j + 1;
        }
        out
    }

    #[test]
    fn parses_plan() {
        let cli = Cli::try_parse_from(["monad", "plan"]).unwrap();
        assert!(matches!(cli.command, Command::Plan { target: None }));
    }

    #[test]
    fn parse_secret_alias_accepts_valid_pair() {
        let (declared, source) = parse_secret_alias("RAILWAY_TOKEN=RAILWAY_TOKEN_STAGING").unwrap();
        assert_eq!(declared, "RAILWAY_TOKEN");
        assert_eq!(source, "RAILWAY_TOKEN_STAGING");
    }

    #[test]
    fn parse_secret_alias_rejects_missing_equals() {
        assert!(parse_secret_alias("RAILWAY_TOKEN").is_err());
    }

    #[test]
    fn parse_secret_alias_rejects_empty_halves() {
        assert!(parse_secret_alias("=SOURCE").is_err());
        assert!(parse_secret_alias("DECLARED=").is_err());
    }

    #[test]
    fn parse_secret_alias_rejects_literal_values() {
        // Catch the CI footgun — an agent passing the resolved secret
        // value instead of an env-var name should get a clear error.
        let err = parse_secret_alias("RAILWAY_TOKEN=rlw_sk_abc123+/=").unwrap_err();
        assert!(
            err.contains("doesn't look like an env-var name"),
            "expected clear hint, got: {err}"
        );
    }

    #[test]
    fn parse_secret_alias_rejects_leading_digit() {
        assert!(parse_secret_alias("NAME=1SOURCE").is_err());
    }

    #[test]
    fn parses_deploy_with_env_and_secret_from() {
        let cli = Cli::try_parse_from([
            "monad",
            "deploy",
            "--env",
            "staging",
            "--secret-from",
            "RAILWAY_TOKEN=RAILWAY_TOKEN_STAGING",
            "--secret-from",
            "VERCEL_TOKEN=VERCEL_TOKEN_STAGING",
        ])
        .unwrap();
        match cli.command {
            Command::Deploy {
                env, secret_from, ..
            } => {
                assert_eq!(env.as_deref(), Some("staging"));
                assert_eq!(secret_from.len(), 2);
                assert_eq!(secret_from[0].0, "RAILWAY_TOKEN");
                assert_eq!(secret_from[0].1, "RAILWAY_TOKEN_STAGING");
            }
            _ => panic!("expected Deploy"),
        }
    }

    #[test]
    fn parses_ci_with_json() {
        let cli = Cli::try_parse_from(["monad", "ci", "--json"]).unwrap();
        assert!(matches!(cli.command, Command::Ci));
        assert!(cli.global.json);
    }

    #[test]
    fn parses_build_with_target_and_no_cache() {
        let cli = Cli::try_parse_from(["monad", "build", "api", "--no-cache"]).unwrap();
        match cli.command {
            Command::Build { target } => assert_eq!(target.as_deref(), Some("api")),
            _ => panic!("expected Build"),
        }
        assert!(cli.global.no_cache);
    }

    #[test]
    fn parses_check_with_target() {
        let cli = Cli::try_parse_from(["monad", "check", "api"]).unwrap();
        match cli.command {
            Command::Check { target } => assert_eq!(target.as_deref(), Some("api")),
            _ => panic!("expected Check"),
        }
    }

    #[test]
    fn parses_check_without_target() {
        let cli = Cli::try_parse_from(["monad", "check"]).unwrap();
        assert!(matches!(cli.command, Command::Check { target: None }));
    }

    #[test]
    fn parses_unit_add() {
        let cli = Cli::try_parse_from(["monad", "unit", "add", "apps/api"]).unwrap();
        match cli.command {
            Command::Unit(UnitAction::Add { path, lang }) => {
                assert_eq!(path.to_str(), Some("apps/api"));
                assert!(lang.is_none());
            }
            _ => panic!("expected Unit(Add)"),
        }
    }

    #[test]
    fn parses_unit_add_with_lang_and_profile() {
        let cli = Cli::try_parse_from([
            "monad", "--monad", "prod", "unit", "add", "apps/api", "--lang", "go",
        ])
        .unwrap();
        match cli.command {
            Command::Unit(UnitAction::Add { path, lang }) => {
                assert_eq!(path.to_str(), Some("apps/api"));
                assert_eq!(lang.as_deref(), Some("go"));
            }
            _ => panic!("expected Unit(Add)"),
        }
        assert_eq!(cli.global.monad.as_deref(), Some("prod"));
    }

    #[test]
    fn parses_unit_list() {
        let cli = Cli::try_parse_from(["monad", "unit", "list"]).unwrap();
        match cli.command {
            Command::Unit(UnitAction::List) => {}
            _ => panic!("expected Unit(List)"),
        }
    }

    #[test]
    fn parses_box_list() {
        let cli = Cli::try_parse_from(["monad", "box", "list"]).unwrap();
        match cli.command {
            Command::Box(BoxAction::List) => {}
            _ => panic!("expected Box(List)"),
        }
    }

    #[test]
    fn parses_box_add() {
        let cli = Cli::try_parse_from(["monad", "box", "add", "prod"]).unwrap();
        match cli.command {
            Command::Box(BoxAction::Add { name }) => assert_eq!(name, "prod"),
            _ => panic!("expected Box(Add)"),
        }
    }

    #[test]
    fn parses_cache_stats() {
        let cli = Cli::try_parse_from(["monad", "cache", "stats"]).unwrap();
        assert!(matches!(cli.command, Command::Cache(CacheAction::Stats)));
    }

    #[test]
    fn parses_why_requires_hash() {
        assert!(Cli::try_parse_from(["monad", "why"]).is_err());
        let cli = Cli::try_parse_from(["monad", "why", "abc123"]).unwrap();
        match cli.command {
            Command::Why { target } => assert_eq!(target, "abc123"),
            _ => panic!("expected Why"),
        }
    }

    #[test]
    fn parses_graph_with_dot_format() {
        let cli = Cli::try_parse_from(["monad", "graph", "--format", "dot"]).unwrap();
        match cli.command {
            Command::Graph { format, .. } => assert_eq!(format, GraphFormat::Dot),
            _ => panic!("expected Graph"),
        }
    }

    #[test]
    fn parses_add_single_package_no_unit() {
        let cli = Cli::try_parse_from(["monad", "add", "tailwindcss"]).unwrap();
        match cli.command {
            Command::Add {
                packages,
                unit,
                dev,
            } => {
                assert_eq!(packages, vec!["tailwindcss"]);
                assert_eq!(unit, None);
                assert!(!dev);
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parses_add_multiple_packages_with_unit_and_dev() {
        let cli = Cli::try_parse_from([
            "monad",
            "add",
            "serde",
            "tokio",
            "anyhow",
            "--unit",
            "control-plane",
            "--dev",
        ])
        .unwrap();
        match cli.command {
            Command::Add {
                packages,
                unit,
                dev,
            } => {
                assert_eq!(packages, vec!["serde", "tokio", "anyhow"]);
                assert_eq!(unit.as_deref(), Some("control-plane"));
                assert!(dev);
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parses_add_with_version_pin() {
        let cli = Cli::try_parse_from(["monad", "add", "tailwindcss@3.4.0"]).unwrap();
        match cli.command {
            Command::Add { packages, .. } => {
                assert_eq!(packages, vec!["tailwindcss@3.4.0"]);
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn parses_add_requires_at_least_one_package() {
        assert!(Cli::try_parse_from(["monad", "add"]).is_err());
        assert!(Cli::try_parse_from(["monad", "add", "--unit", "api"]).is_err());
    }

    #[test]
    fn parses_run_with_unit_and_task() {
        let cli = Cli::try_parse_from(["monad", "run", "control-plane", "admin"]).unwrap();
        match cli.command {
            Command::Run { unit, task, args } => {
                assert_eq!(unit, "control-plane");
                assert_eq!(task, "admin");
                assert!(args.is_empty());
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parses_run_with_trailing_args_after_dash_dash() {
        let cli = Cli::try_parse_from([
            "monad",
            "run",
            "control-plane",
            "admin",
            "--",
            "waitlist",
            "broadcast",
            "--dry-run",
            "--message",
            "hi there",
        ])
        .unwrap();
        match cli.command {
            Command::Run { unit, task, args } => {
                assert_eq!(unit, "control-plane");
                assert_eq!(task, "admin");
                assert_eq!(
                    args,
                    vec![
                        "waitlist",
                        "broadcast",
                        "--dry-run",
                        "--message",
                        "hi there",
                    ]
                );
            }
            _ => panic!("expected Run"),
        }
    }

    #[test]
    fn parses_run_requires_unit_and_task() {
        assert!(Cli::try_parse_from(["monad", "run"]).is_err());
        assert!(Cli::try_parse_from(["monad", "run", "control-plane"]).is_err());
    }

    #[test]
    fn parses_serve_requires_monad_name() {
        assert!(Cli::try_parse_from(["monad", "serve"]).is_err());
        let cli = Cli::try_parse_from(["monad", "serve", "prod"]).unwrap();
        match cli.command {
            Command::Serve { monad } => assert_eq!(monad, "prod"),
            _ => panic!("expected Serve"),
        }
    }

    #[test]
    fn parses_global_since_override() {
        let cli = Cli::try_parse_from(["monad", "plan", "--since", "HEAD~5"]).unwrap();
        assert_eq!(cli.global.since.as_deref(), Some("HEAD~5"));
    }

    #[test]
    fn parses_verbose_short_flag() {
        let cli = Cli::try_parse_from(["monad", "-v", "plan"]).unwrap();
        assert!(cli.global.verbose);
    }

    #[test]
    fn parses_toolchain_pin() {
        let cli = Cli::try_parse_from(["monad", "toolchain", "pin", "go=1.22.3"]).unwrap();
        match cli.command {
            Command::Toolchain(ToolchainAction::Pin { pin }) => assert_eq!(pin, "go=1.22.3"),
            _ => panic!("expected Toolchain(Pin)"),
        }
    }
}
