# Changelog

All notable changes to monad will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-05-17

Single-fix patch driven by a downstream consumer hitting a parallel-install symlink race on a bun workspace deploy. CI had been passing — deploy's higher effective concurrency was the only thing that opened the race window wide enough for the symlink-creation step to collide. The dedup primitive (`LanguageAdapter::install_scope`) generalises beyond the four node-family adapters; future adapters with shared install side effects can opt in by overriding the trait method.

### Fixed

- **Concurrent installs no longer race on shared JS workspaces.** Unites that share a `package.json` `"workspaces"` set (npm / yarn / bun) or a `pnpm-workspace.yaml` (pnpm) all resolve to the same hoisted `node_modules/`. Before this fix, `monad ci` / `build` / `deploy` would spawn `bun install` / `pnpm install` per unit in parallel, both racing to create the same workspace symlink under `node_modules/@scope/<pkg>` — one would EEXIST and the deploy would die mid-build. Reported downstream after a CI-passing branch deployed broken. New `LanguageAdapter::install_scope()` returns the workspace root for the four node-family adapters; the executor dedupes `install()` calls against a per-scope `OnceLock`, so the first probe-Missing unit in a scope runs install at the root and concurrent siblings block-then-skip. Install failures on the winner propagate as a synthetic `InstallRecord` on every unit in the scope, so the whole workspace fails fast instead of half-running tasks against poisoned deps. Adapters outside node-family (go, cargo, python, php, ...) inherit the trait default (`dir`) and keep one-install-per-unit semantics.

## [0.1.1] - 2026-05-04

Bug-fix release driven by the first real CI consumer (gosho-io/gosho-app) hitting the `[toolchain]`-pin install path on a clean GitHub-Actions runner. Two `monad toolchain install` regressions ship fixed; the underlying generalization (co-required tools) opens the door for pnpm / yarn / composer / bundler / mvn to land in the same shape later.

### Added

- **Co-required toolchains.** New `Tool::co_required()` returns `&[CoRequired { tool, default_version }]` — the declarative way to say "this tool needs these others installed and on PATH". `PythonTool` declares `uv` at default `0.5.0`; users override via `[toolchain] uv = "..."`. `monad toolchain install` and the per-unit `ensure_toolchain` both expand the primary plan with co-required tools, install them ahead of primaries, and prepend each freshly-installed bin dir to the parent process's PATH so the primary's `delegated_ensure` (e.g. `uv python install <version>`) discovers its sibling automatically. Future-proofs pnpm / yarn / composer / bundler / mvn — those are pure registrations on the existing trait now (tracked at monad-cloud-j5le).
- **Native uv installer (`UvTool`).** Downloads the astral-sh/uv standalone tarball from GitHub releases, verifies the sibling `.sha256`, post-extracts `uv` + `uvx` into the canonical `<install>/bin/` layout. Tolerates pre-0.4.0 pins that ship `uv` only without `uvx`. Lives at `~/.monad/tools/uv/<version>/bin/`, indexed by `monad toolchain list` like every other tool.
- **`Tool::post_extract()` hook.** Optional per-tool tree-rewrite step that runs against the staged install dir before commit. Lets tools whose upstream archive puts binaries at the wrapper-dir root (uv today, bun + deno when those land natively) synthesise a `bin/` subdir to match monad's `<install>/bin/<binary>` store invariant.
- **`monad toolchain install` reports unsupported tools as `skipped` instead of failing.** New `skipped: [{tool, version, used_by, reason}]` array on the JSON output. Tools without a built-in installer (currently `bun` + `deno` until zip-archive support lands at monad-cloud-9jtl) appear here so the GitHub Action wrapper can fall back to upstream install scripts without `monad toolchain install` returning non-zero. `installed` entries gain optional `co_required_for: [...]` alongside `used_by` so the JSON shows why a tool the user didn't pin shows up.
- **GitHub Action bootstraps bun when pinned but unbuilt.** Composite action's `install-toolchains` step now reads `monad.toml`'s `[toolchain]` block in pure bash (works on macOS runners — no GNU-awk dep) and curl-installs bun via `bun.sh/install bun-v<version>` when monad doesn't have a native installer yet. Goes away once `BunTool` ships.
- **Action-selftest exercises the clean-runner toolchain-install path.** Two new fixture trees + jobs: `smoke (toolchain install, python+uv, ubuntu+macos)` and `smoke (toolchain install, bun, ubuntu)`. Each runs the action with no `setup-*` steps and asserts the unit's build task ran with the pinned tools on PATH. Closes the regression class that let the underlying bug ship — the original `monorepo-go-node` fixture has no `[toolchain]` block, so the install path was untested.

### Fixed

- **`bun` + `deno` adapters resolve their own toolchain pins.** `primary_tool_name` in the resolver collapsed both adapters onto the `"node"` tool key, so a `[toolchain] bun = "1.3.12"` repo pin was silently shadowed by the `node` pin and never reached the installer. First real CI consumer hit it: only node got installed; every bun + python unit failed downstream with command-not-found. Bun and Deno are independently versioned runtimes — they have their own pin slots now. The bun adapter's per-unit `required_toolchain` fallback to a node version (when `.bun-version` is absent) is unaffected; that's a per-unit detection, not a primary-tool collapse.
- **`ensure_toolchain` prepends co-required PATH before primary install.** When a python unit's tasks were about to run, `ensure_toolchain` correctly installed uv as co-required but didn't prepend its bin dir to the parent process's PATH before calling the python install — so `PythonTool::delegated_ensure`'s `Command::new("uv")` failed to find uv even though it had just been installed. Mirrored the same `set_var("PATH", ...)` pattern from `install_all` into a shared `prepend_path_env` helper.
- **`ensure_toolchain` no longer hard-errors on tools without a built-in installer.** Same regression class as above: after the bun/deno resolver fix, units pinning `bun` reached `installer.ensure("bun", ...)` which errored with "no built-in tool registered for 'bun'" and broke task execution. Now falls through to the system PATH (where the action wrapper has already put bun via `$GITHUB_PATH`) with a `tracing::info!` line so the skip is observable. Same posture as `install_all`'s `skipped` reporting.
- **`monad plan` warns when unit-level `inputs` is silently shadowed.** Adapters that ship their own `default.inputs` (cargo, go, node-{npm,pnpm,yarn}, bun, ruby, maven) override anything declared at the unit-root `inputs = [...]` field for lifecycle tasks (build/test/lint, plus check on cargo + go). The resolution behaviour itself is unchanged; monad now emits a `tracing::warn!` listing the affected tasks at plan time so users see the dropped globs instead of hitting silent cache-key drift. `docs/configuration.md` corrected to match — unit-level `inputs` only feeds custom (non-lifecycle) tasks; restate every glob you want under `[tasks.<name>] inputs = [...]` for lifecycle tasks.
- **Telemetry opt-out actually opts out.** `[telemetry] enabled = false` in `monad.toml` was previously a silent no-op — the `report::send` site never read the config flag, so build reports were emitted to the configured `monad://` remote regardless. Now `enabled = false` short-circuits before any URL construction or env lookup, and the `MONAD_TELEMETRY` env var (`0` / `false` / `no` / `off`) provides a per-machine override that cannot be flipped back on by config (precedence: either says off → off). New `monad doctor` check `telemetry.posture` surfaces the resolved opt-in/opt-out state so users can verify. `docs/configuration.md` § `[telemetry]` rewritten to document the wire shape, both opt-out paths, and self-hoster behaviour.

## [0.1.0] - 2026-05-03

Initial public release.

### CLI

- **`monad prime`** — agent-orientation verb: workspace inventory, cache state, plan preview, recommended next verb. Schema-stable JSON via `monad prime --json` / `monad schema prime`.
- **`monad plan`** — cache-aware task plan with hit/miss prediction per task; `--json` includes `miss_reason` per task.
- **`monad ci`** — plan + execute everything; the GitHub Action entry point. Default kinds (build / check / test / lint) run; side-effectful integration tasks (Deploy / Notify / Rollback) are excluded by default.
- **`monad build | check | test | lint [target]`** — single-task variants targetable at a monad or a unit. `monad check` runs the adapter-native fast type-check (`cargo check`, `go vet`).
- **`monad install`** — runs each unit's adapter install command (`npm ci`, `go mod download`, `composer install`, `pnpm install --frozen-lockfile`, …) under one CLI. Node-family adapters fall back to non-frozen on cold projects without a lockfile.
- **`monad init`** — bootstraps `monad.toml` + `profiles/release.toml`; in a non-empty monorepo walks subdirs (depth-bounded, ignoring `node_modules`/`vendor`/`target`/etc.), auto-detects every unit monad knows about, captures toolchain pins, and pre-populates each generated `unit.toml` with `[tasks.<name>]` blocks mirroring every script declared in the project (`package.json` for node-{npm,pnpm,yarn}+bun, `composer.json` for php). Also drops `AGENTS.md` (the cross-tool [agent-instructions standard](https://agents.md) read by Cursor / Codex / Aider / OpenCode / 60k+ projects) and `CLAUDE.md` (a thin `@AGENTS.md` import for Claude Code) at the workspace root, so a fresh agent picks up the monad verb surface immediately without the user pasting a session-start instruction. Skips clobbering hand-crafted versions.
- **`monad unit add <path>`** — scaffold mode (empty dir + `--lang`) or adopt mode (existing dir, language auto-detected). Scaffold supports all 13 built-in adapters; adopt covers the same set plus any installed plugin languages. `monad box list` / `monad unit list` enumerate the workspace.
- **`monad add <pkg>... [--unit <d>] [--dev]`** — first-class dependency add. Wraps each unit's native package manager (`cargo add`, `bun add`, `npm install --save`, `pnpm add`, `yarn add`, `go get`).
- **`monad run <unit> <task> -- <args>`** — ad-hoc task invocation. Looks up `[tasks.<task>]` in `<unit>/unit.toml`, spawns the `run` command, and forwards trailing args to a `sh -c` invocation. Bypasses the cache — meant for CLIs, migrations, and one-off scripts.
- **`monad migrate <tool>`** — config migrators covering the six most common monorepo tools: Turborepo (`migrate turbo`), Nx (`migrate nx`), Lerna (`migrate lerna`), Makefile (`migrate make`, best-effort), moonrepo (`migrate moon`), and Rush.js (`migrate rush`). Each reads the source tool's manifests, walks the package layout, emits per-package `unit.toml` mirroring scripts/tasks plus root `monad.toml` + `profiles/prod.toml`. Untranslatable concepts (Turbo `cache: false`, Make pattern rules, Moon cross-project deps, Rush bulk commands, Lerna `command.*` config, …) surface as structured notes (`Skipped` / `Inferred` / `NotYetImplemented` / `Conflict`) rather than silent loss. `--dry-run` reports without writing, `--force` opts in to clobber. End-to-end tested: `monad plan --json` succeeds on every migrated fixture.
- **`monad dev <unit>`** + **`monad serve <monad>`** — file-watch + hot-reload for one unit or every unit in a monad with a `[serve]` block. Per-child stdout/stderr prefixed in `monad serve`.
- **`monad why <hash>`** — full input manifest behind any cache key: unit, task, command, monad version, adapter + resolved toolchain, env-var names (values never stored on disk), every hashed file with its blake3 digest and size. Supports short-prefix lookup.
- **`monad graph [monad]`** — dependency DAG in ASCII (default), Graphviz DOT (`--format dot`), or JSON.
- **`monad doctor`** — structured health check: workspace discovery, config parse + cross-refs, each pinned toolchain's install state, local cache (writability + entries + size), remote cache (URL + auth), GHA cache (when `GITHUB_ACTIONS=true`), git repo + base ref, and per-integration env / CLI preflight. `--cloud` adds remote-cache JWT validation + endpoint reachability. `CheckStatus` is `ok | warn | fail | skipped`. Exit 1 on any fail.
- **`monad artifacts`** — read-only post-build summary listing resolved output paths per unit. JSON shape `{unit_name: [absolute_paths...]}` for downstream packaging steps.
- **`monad cache stats | clear | push | pull`** — inspect, wipe, and bulk-sync cache tiers.
- **`monad toolchain list | install | pin`** — manage pinned language toolchains under `~/.monad/tools/`.
- **`monad mcp install [client]`** — register `monad-mcp` as an MCP server across the major agent clients. JSON-shape clients: Claude Code (`~/.claude.json`), Claude Desktop (`~/Library/Application Support/Claude/claude_desktop_config.json`), Cursor (`~/.cursor/mcp.json`), Windsurf (`~/.codeium/windsurf/mcp_config.json`), Zed (`~/.config/zed/settings.json` under the `context_servers` key), OpenCode (`~/.config/opencode/opencode.json` under the `mcp` key with a `type: "local"` discriminator). TOML-shape clients: Codex CLI (`~/.codex/config.toml`, entries land under `[mcp_servers.<name>]`). `auto` (default) detects every installed client and registers in each. `--local` writes the project-scoped variant (`.cursor/mcp.json`, `.mcp.json` at the repo root for Claude Code, `.codex/config.toml` for Codex, etc.). Idempotent — re-running updates the existing entry rather than duplicating it.
- **`monad box add <name>`** — create a new monad (deployment unit) at `profiles/<name>.toml` with starter content. Follows naturally from `monad prime`'s recommended-next-verb path on an empty workspace.
- **`monad secret put | list | delete`** — thin wrapper over each platform's secret CLI (Railway, Vercel, …) so secrets are managed through one verb.
- **`monad release <spec>`** — bump workspace version (`X.Y.Z`, `patch`, `minor`, or `major`), update internal path-deps, refresh `Cargo.lock`, commit, and tag.
- **`monad login`** — interactive device-code flow for the monad.build hosted cache. POSTs `/v1/cli/device-code` (overridable via `$MONAD_API_BASE`), polls `/v1/cli/exchange`, and stashes the JWT in the OS keychain (entry `("monad", "cache-token")`) — falls back to `~/.monad/credentials` (0600) on headless / keychain-less hosts.
- **`monad schema [target]`** — emit JSON Schema for every agent-consumable output type (`plan`, `report`, `why`, `scaffold`, `manifest`, `doctor`, `error`, `diagnostics`, `notification-payload`, `prime`).
- Global flags `--json`, `--no-cache`, `--monad`, `--since`, `-v`, `--workspace <PATH>`, and `--report-file <path>` (writes the ExecutionReport JSON to a file independently of stdout).

### MCP server (`monad-mcp`)

- Stdio JSON-RPC server (built on the `rmcp` crate) that exposes monad's verb surface as typed tool calls for MCP clients (Claude Desktop, Claude Code, Cursor, Codex). Ships in the same release tarball as `monad`.
- Read-only tools: `monad_prime`, `monad_schema`, `monad_plan`, `monad_unit_list`, `monad_box_list`, `monad_doctor`, `monad_why`, `monad_artifacts`.
- Execution tools: `monad_install`, `monad_build`, `monad_check`, `monad_test`, `monad_lint`, `monad_ci`.
- Destructive-external tools: `monad_deploy`, `monad_notify` (carry the `destructiveHint`).
- Single-workspace per server — pass `--workspace <PATH>` or set `$MONAD_WORKSPACE_ROOT`.

### Configuration

- **TOML config** — `monad.toml` (repo defaults: cache, telemetry, execution, toolchain, plugins), `profiles/*.toml` (deployment groupings — environment OR logical layer OR release stage; monad is unopinionated), `unit.toml` (tasks, serve, depends_on, force_independent, toolchain pins, integrations, notifications). `Workspace::load(root)` walks the tree and validates cross-references (duplicate names, dangling unit references, shared-unit semantics, orphan `unit.toml`).
- **Multi-monad / shared units** — the same unit referenced by multiple profiles is loaded once, produces identical cache keys across profiles, and hits the cache on the second monad's visit in the same `ci` run.
- **`monad init` flags** — default detects units; `--no-detect` opts out for empty-placeholder behaviour. `--json` output includes `unites_detected`, `toolchain_pins`, `toolchain_conflicts`.

### Language adapters (13)

`go`, `cargo`, `python`, `python-uv`, `ruby`, `php`, `maven`, `gradle`, `node-npm`, `node-pnpm`, `node-yarn`, `bun`, `deno`. Each:

- Detects its canonical manifest/lockfile.
- Fingerprints the full idiomatic set (lockfiles, toolchain pin files, `.tool-versions`, etc.).
- Resolves a toolchain pin from the ecosystem's standard. Broad detection chain across all adapters: per-tool version files (`.nvmrc`, `.python-version`, `.ruby-version`, `.java-version`, `.bun-version`, `.deno-version`), `.tool-versions` (asdf/mise), `.sdkmanrc` (sdkman), and ecosystem-specific in-package conventions (`engines.node`, `volta.node` in `package.json`; `require.php` in `composer.json`; `maven.compiler.release` in `pom.xml`; `JavaLanguageVersion.of(N)` in `build.gradle.kts`; etc.). Node-family adapters also fall back to `@types/node`'s major version (returned as `^N`) — common pseudo-pin in TS projects without an explicit `.nvmrc`.
- Runs the native install step (`go mod download`, `cargo fetch --locked`, `pip install`, `uv sync --frozen` (uv variant), `bundle install`, `composer install`, `mvn dependency:resolve`, `./gradlew dependencies`, `npm ci`, `pnpm install --frozen-lockfile`, `yarn install --immutable`, `bun install --frozen-lockfile`, `deno install --frozen=true`).
- Ships `build` / `test` / `lint` defaults the unit's `unit.toml` can override via `[tasks.<name>]`. `cargo` and `go` ship `check` defaults too.
- Mixed-lockfile repos (pnpm + npm etc.) resolve to the more specific manager.

### Cache (3 tiers + remote)

- **Local CAS** at `~/.monad/cache/<key>.tar`. Streaming blake3 with format tags + length-prefixed file additions to prevent input smuggling. Atomic tar-bundle writes; `get` restores outputs into the unit dir; `put_manifest` stores the input manifest as a sidecar.
- **Toolchain-resolved fingerprint** mixed into every key — a system `go 1.22.3 → 1.22.5` bump invalidates even when `go.mod` still says `go 1.22`. Probes memoised per `(program, args)` so a 100-unit monorepo pays the subprocess cost at most once per tool.
- **Pessimistic dep cascade** — a unit's task cache key folds in the effective signature of each `depends_on` (topo order). Library change → every dependent's cache misses. Set `force_independent = true` to skip the fold per unit. Task-level `depends_on` cascades into the dependent task's hash, so deploy-state hits actually invalidate when build inputs change.
- **GHA tier** — composite action wraps `~/.monad/cache` with `actions/cache@v4`, scoped per-branch via the standard GHA cache API. No extra config — use the action and caching happens.
- **S3-compatible remote tier** — `[cache] remote = "s3://<bucket>/<prefix>"` enables a read-through / write-back remote cache via the `object_store` crate. Credentials from the standard AWS environment chain. Works with AWS S3, Cloudflare R2, MinIO, Backblaze B2, and any S3-API service. Optional `remote_endpoint` for non-AWS hosts. Every remote op best-effort: a network failure never fails the build.
- **`monad://` HTTP remote** — Bearer-auth JWT remote-cache scheme (`monad://cache.monad.build` for the monad.build hosted service, or any compatible self-hosted endpoint). Token resolution: `$<remote_token_env>` → OS keychain (`monad` / `cache-token`) → `~/.monad/credentials` (0600 fallback). Defaults to no write-through on local-hit so warm runs don't HEAD the remote on every invocation; opt in with `[cache] remote_write_through = true`. Presigned-URL upload path for bundles >95 MB to bypass Cloudflare's edge body-size limit.
- **`BuildReport`** — `monad ci` and `monad build` POST a build report to `<monad://-base>/report/build` (cache hit ratio, status, duration, branch, sha). Best-effort: any failure is logged and swallowed — telemetry never fails the build.
- **Git-diff pre-filter** — `GitDiff::changed_files(base_ref)` / `changed_dirs(...)` for coarse dir-level change detection.
- **`monad cache push` / `monad cache pull`** do bulk sync between tiers.

### Executor

- **Cross-unit dependency graph + parallelism** — per-monad Kahn-layered DAG from each unit's `depends_on`; rejects cycles and cross-monad refs at load time. Walks levels top-down concurrently, throttled to `defaults.parallelism`.
- **Retry + flakiness detection per task** — `[tasks.<name>] retry = N` (default 0). Re-runs on nonzero exit up to N additional times. A task that succeeds on attempt > 1 is marked `flaky: true` in the report and contributes to a new `summary.flaky` count.
- **Fail-fast** gates the next dep-graph level rather than mid-level in-flight units.
- **Adapter `install_probe()`** — cheap filesystem probe (no hashing, no subprocess calls) the executor uses to decide whether to re-install deps before any tasks run. Defaults: node-{npm,pnpm,yarn} check the relevant `node_modules/` marker; bun checks `node_modules/` non-empty; php checks `vendor/autoload.php`. Other ecosystems inherit the `Ready` default. `--skip-install` / `--force-install` flags override.
- **Opt-in container execution** — `[execution] container = auto | always | never` with `image = "<ref>"`. Wraps each task in `<runtime> run --rm -u <uid>:<gid> -v <unit>:/work -w /work --env <name> <image> sh -c "<run>"`. Runtime auto-detected (docker → podman → nerdctl). Image folded into the cache key so a tag/digest change invalidates.

### Toolchain manager

- **Embedded mini-mise** — fetches every pinned toolchain into `~/.monad/tools/<tool>/<version>/` and prepends to PATH for the child process running each task. Honours per-unit `[toolchain]` and repo `[toolchain]` pins; auto-installs only when an explicit pin is set (adapter-detected versions feed the cache key but don't install).
- **Built-in tools**: `go` (direct download from go.dev), `node` (direct download from nodejs.org), and `python` (delegated — monad shells out to `uv python install <version>` and asks uv where the interpreter landed). Delegation lets monad route to the right specialist (uv knows Python distributions better than monad ever could) without owning the on-disk layout.
- **`monad toolchain install`** — pre-warms every pinned toolchain in the workspace; JSON output of `{installed: [{tool, version, bin_dir, used_by}], failed: <count>}`.

### Deploy + integrations

- **`monad deploy [target] [--preview|--rollback]`** — runs integration-emitted tasks of the selected kind on units with a matching integration wired up. Build is included as the canonical prerequisite. Unites without matching integration tasks are skipped with a clear `<no-deploy>` marker. Idempotent: deploys short-circuit when inputs match the last successful deploy's input manifest (`.monad/state/deploys.json`); `--force` overrides.
- **`Integration` trait** — sibling to `LanguageAdapter`. Adapters classify a unit's language (one per unit); integrations *augment* a unit with additional tasks (deploy, rollback, release, notify). A unit can have one adapter and zero-or-more integrations active simultaneously.
- **Built-in integrations: Vercel + Railway + Cloudflare Pages + Cloudflare Workers**.
  - `vercel`: detects `vercel.json` or `.vercel/project.json`. Emits `vercel:deploy` + `vercel:preview`. Requires `VERCEL_TOKEN`.
  - `railway`: detects `railway.toml`, `railway.json`, or `.railway/`. Emits `railway:deploy` (using `railway up --ci` so non-TTY callers actually block on the server-side build outcome). Requires `RAILWAY_TOKEN`. Multi-service fan-out via `services = [...]` in `[integrations.railway]`.
  - `cloudflare_pages`, `cloudflare_workers`: detect wrangler config + `[integrations.cloudflare_pages|workers]`. Emit deploy + preview tasks.
- **`[integrations.<id>]` per-unit config** — flat `key = "value"` map keyed by integration id. Integrations read fields they recognise and ignore the rest; unknown keys don't error at load.
- **Secret aliases (`--env` / `--secret-from`)** — env-var alias indirection for integration secrets. Integrations declare a canonical name (`RAILWAY_TOKEN`, `VERCEL_TOKEN`); users/agents control which host env var actually supplies the value. `[environments.<name>]` blocks in `monad.toml` for saved alias profiles. `--secret-from DECLARED=SOURCE` for ad-hoc aliasing — rejects literal-looking values at the flag parser.
- **Integration preflight** — per-integration `required_env` + `required_cli` checks surface as `integration.<id>.env` / `integration.<id>.cli` doctor checks. Failure detail names the unit(es) where the integration was detected.

### Notifications (post-deploy notifications)

- **`NotificationPayload`** — every Notify-kind task receives a single newline-terminated JSON object on **stdin** (never env vars, never argv). Schema published as `monad schema notification-payload`. Fields: `schema_version`, `monad_version`, `environment`, `trigger.{task_name, unit_name, monad_name, outcome, exit_code, duration_ms, cache_key, integration_kind, output_excerpt, stderr_excerpt}`.
- **Built-in notifications: Slack + Linear**.
  - `slack`: `[integrations.slack]` opt-in. POSTs a templated message to a Slack Incoming Webhook via ureq (no host deps). Outcome-driven emoji, URL auto-extraction from deploy output, stderr excerpt as a code block on failure.
  - `linear`: `[integrations.linear]` opt-in. Scans the payload for `[A-Z]{2,}-\d+` issue identifiers and transitions matched issues to a configurable `target_state` via Linear's GraphQL API. Failed deploys skip transitions.
- **`[[notifications]]` block in `unit.toml`** — inline Notify-kind tasks with full `env` / `required_env` / `required_cli` preflight. Escape hatch for bespoke hooks (PagerDuty, custom log forwards, GitHub PR comments) where writing a full `Integration` is overkill.
- **`monad notify [target] [--env NAME] [--secret-from DECLARED=SOURCE]`** — re-fires Notify-kind tasks against the last deploy's cached payload. Used to re-send a Slack post / Linear flip after fixing a broken webhook, without re-running the deploy.
- **Sidecar persistence** — every completed Deploy writes `{workspace}/.monad/notification/<monad>/<unit>/<task>.json` containing the payload. Survives across invocations.
- **`monad deploy --no-notify`** — opt out of the notification fan-out.

### Subprocess plugin protocol

- **Out-of-process language adapters** — drop a binary named `monad-adapter-<id>` on `$PATH` to teach monad a new language without forking. JSON-RPC 2.0 over stdio, LSP-style `Content-Length` framing.
- **Built-ins always win on id collision**; between plugins, first-on-`$PATH` wins; binary suffix must match the announced adapter id.
- **Filter via `monad.toml`** — `[plugins] disable = [...]` and `allowlist = [...]`.
- **Reference noop plugin** — `examples/monad-adapter-noop/` is ~200 lines of pure-`std` Rust depending only on `serde` / `serde_json`. Demonstrates the protocol from outside `monad-plugin`.
- **Lifecycle**: `initialize` → typed manifest, then `detect` / `requiredToolchain` / `resolvedToolchainFingerprint` / `install` / `parseDiagnostics` as needed; teardown is `shutdown` → 2s grace → SIGTERM → SIGKILL. Per-call timeouts: 30s queries, 30min install. See `docs/plugins.md` for the full wire spec.

### Structured tool diagnostics

- **On task failure**, when the adapter declares a diagnostic hook for that task, monad re-runs the task with the hook's modifier (e.g. `--message-format=json`), parses the captured output via the registered parser, and surfaces a `Vec<Diagnostic>` on the failed task in the report. Strictly additive — failure of the diagnostic re-run never blocks the build.
- **Built-in parsers** for `cargo --message-format=json`, `golangci-lint --out-format=json`, `eslint --format=json`, and `ruff check --output-format=json`.
- **Adapter hooks** declared for cargo (build/test/lint), go (lint), node-{npm,pnpm,yarn} + bun (lint via eslint Replace), python (lint).
- **Plugin extension** — plugins declare `diagnostic_hooks` in their manifest and (optionally) implement the `parseDiagnostics` RPC method.
- **Diagnostic shape** — LSP-inspired `{file, line, col?, end_line?, end_col?, severity, message, rule?, source}`. Paths workspace-relative + forward-slash for direct agent `Read()`. Schema published as `monad schema diagnostics`.
- **Human output** unchanged — the tool's familiar stderr still prints; failed tasks with diagnostics get a one-line footer `→ N diagnostics captured; pass --json to extract.`

### GitHub Action

- **Composite action at `action.yml`** — single step in your workflow. Installs monad, restores three cache tiers, fetches pinned toolchains by default, runs the build.
- **Three caches wired up automatically**: monad content cache (`~/.monad/cache`), toolchain cache (`~/.monad/tools`), per-tool global dep caches (`~/.npm`, `~/.m2/repository`, `~/.cache/composer`, `~/go/pkg/mod`, `~/.cargo/registry`, `~/.gradle/caches`, etc.). `actions/cache@v4` silently skips non-existent paths so caching all of them is free for languages a workspace doesn't use.
- **`install-toolchains` input** (default `'true'`) — runs `monad toolchain install` to fetch every pinned toolchain via the embedded mini-mise. Set `'false'` to chain `actions/setup-*` yourself (typically combined with `[toolchain] use_system = true`).
- **Action inputs**: `version`, `monad`, `task`, `target`, `workspace-path`, `json`, `cache-key-suffix`, `source-path`, `install-toolchains`, `no-notify`.
- **Action outputs**: `report` (full ExecutionReport JSON, always set), `artifacts` (`{unit_name: [absolute_paths...]}`), `toolchains-installed` (set when install-toolchains: true), `json` (back-compat alias).

### Quality of life

- **Structured errors** — every command failure with `--json` emits `{kind, message, hint?, where?, docs_url?, next_steps?}` with a stable `kind` taxonomy (config, scaffold, workspace_not_found, target_not_found, target_ambiguous, integration_not_configured, login_*, internal). Without `--json`, errors stay human-readable on stderr.
- **TTY-aware colours** — tiny `style` module wraps ANSI when stdout is a terminal; passes through when piped; honours `NO_COLOR` and `CLICOLOR_FORCE`.
- **CI annotations** — when `GITHUB_ACTIONS=true`, failed tasks emit `::error file=<unit path>,title=<unit>/<task> failed::<exit code + stderr>` before the human output, so GitHub surfaces failures inline on the workflow summary.
- **Output excerpt** — integration-sourced tasks (Deploy / DeployPreview / Rollback / Notify / Release) capture a 4 KB tail of stdout+stderr on the `ExecutedTask`. Build-log URLs and deploy IDs surface inline rather than getting buried in the cache bundle.
- **`monad schema`** — JSON Schema for every output type; the stable agent integration contract.

### Distribution

- **Cargo workspace** — 10 crates: `monad-cli` (binary), `monad-mcp` (binary), `monad-core` (planner + executor), `monad-config`, `monad-cache`, `monad-adapters`, `monad-toolchain`, `monad-watch`, `monad-cas-protocol`, `monad-plugin`.
- **Release pipeline** — `.github/workflows/release.yml` builds four targets (`x86_64` + `aarch64` on Linux and macOS) on every `v*` tag; publishes tarballs (carrying both `monad` and `monad-mcp`) + per-file SHA256s + aggregated `SHA256SUMS` to the GitHub Release.
- **`install.sh`** — curl-pipe-sh installer with platform detection and SHA256 verification. Available at `https://monad.build/install`. Drops the Claude Code skill bundle at `~/.claude/skills/monad/` automatically so a fresh Claude Code session picks up the monad verb surface without manual file copying. `MONAD_FORCE_SKILL=1` overwrites an existing user-customised bundle; `MONAD_SKILL_DIR` retargets the install (e.g. for self-hosted Claude paths). Linux + macOS only this release; Windows binaries land in v0.2.
- **Release pipeline gate** — `release.yml` runs a `preflight` job (fmt + clippy + tests on the exact tagged commit) before the cross-compile build matrix fires. A tag whose tests fail can never publish binaries.
- **`skills/monad/SKILL.md`** — Claude Code skill bundle with anti-pattern reference + PreToolUse `monad-guard` hook that steers agents to monad verbs over native package managers (covers all 13 adapters: pip / uv / composer / mvn / gradle / bundle / npm / pnpm / yarn / bun / cargo / go / deno + publish/install variants).
- **Documentation** — README plus `docs/configuration.md` (exhaustive config reference), `docs/agents.md` (agent-integration guide), `docs/deploying.md` (deploy + integrations + notifications), `docs/plugins.md` (plugin authoring guide), `docs/adopt-existing-repo.md` and `docs/new-project.md` (10-minute walkthroughs).
- Dual MIT / Apache-2.0 license.

[0.1.1]: https://github.com/thomascarter613/monad-next/releases/tag/v0.1.1
[0.1.0]: https://github.com/thomascarter613/monad-next/releases/tag/v0.1.0
