// The harness ships with helpers for vendored-clone + hand-crafted
// fixtures; the Phase 1 commit only exercises hand-crafted, so some
// helpers are dead-code under the current set of tests. They're used
// in later phases, so `#[allow(dead_code)]` at the module level keeps
// clippy-with-warnings-as-errors happy without hiding real unused
// code in the CLI itself (this module is test-only).
#![allow(dead_code)]

//! Shared helpers for the monad end-to-end test harness.
//!
//! The harness invokes the real release-profile `monad` binary against
//! real-world fixtures (hand-crafted for deterministic PR gating,
//! vendored-on-demand for realistic tag-push coverage). Tests assert
//! end-to-end behaviour — init, plan, ci, cache hit on rerun — as
//! opposed to the per-module unit tests that live next to each
//! monad-core / monad-adapters module.
//!
//! Design notes:
//!
//! - **`monad` binary**: resolved via `CARGO_BIN_EXE_monad`, which
//!   cargo populates automatically for integration tests in the
//!   owning crate. No manual build step, always fresh.
//! - **Materialisation**: every test gets a throwaway tempdir copy
//!   of the fixture so parallel runs can't step on each other. Hand-
//!   crafted fixtures copy from `tests/e2e/fixtures/<name>/`; vendored
//!   fixtures clone once (cached under `target/e2e-cache/`) and copy
//!   from there.
//! - **Toolchain skipping**: each ecosystem test calls
//!   [`require_toolchain`] at the top, which logs + returns early
//!   when the underlying tool (`go`, `cargo`, `node`, …) isn't on
//!   PATH. CI installs every tool so nothing skips; local dev is
//!   permissive.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Repo root — resolved once via the workspace Cargo.toml so tests
/// work regardless of which CWD `cargo test` was invoked from.
fn workspace_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` for monad-cli is `<root>/crates/monad-cli`.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .expect("monad-cli is two levels below the workspace root")
        .to_path_buf()
}

/// Path to the `tests/e2e/fixtures/` dir at the repo root, where
/// every hand-crafted fixture lives.
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("tests").join("e2e").join("fixtures")
}

/// Cache dir for vendored-on-demand clones. Gitignored via `target/`;
/// first test run fills it, subsequent runs reuse.
pub fn vendored_cache_dir() -> PathBuf {
    workspace_root()
        .join("target")
        .join("e2e-cache")
        .join("vendored")
}

/// The `monad` binary cargo built for these tests. Fresh on every
/// `cargo test` invocation — no stale-binary footguns.
pub fn monad_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_monad"))
}

/// Check that `tool` is on PATH; return true if the caller should
/// proceed. Logs a visible skip message on miss so CI can tell the
/// difference between a pass and a silent no-op.
pub fn require_toolchain(tool: &str) -> bool {
    if which::which(tool).is_ok() {
        return true;
    }
    // Use `println!` rather than tracing — cargo-test captures it
    // under `--nocapture` or on failure, which matches the cargo
    // ergonomic for "informational" test output.
    println!("[e2e] skipping: `{tool}` not on PATH");
    false
}

/// Copy a hand-crafted fixture dir to a fresh tempdir. Returns the
/// tempdir handle (drop destroys it) + the materialized path.
///
/// The fixture is copied into `<tempdir>/<name>/` rather than
/// directly into `<tempdir>`. That keeps the leaf-dir basename
/// stable across runs and hosts — `monad unit add .` derives unit
/// names from the dir basename, and `tempfile::tempdir` picks
/// `.tmpXXXXXX` on some platforms (notably GHA ubuntu-latest runners
/// under certain TMPDIR configs), which would otherwise land leading-
/// dot junk as the unit name in `monad plan --json`.
pub fn materialize_hand_crafted(name: &str) -> (tempfile::TempDir, PathBuf) {
    let src = fixtures_dir().join(name);
    assert!(
        src.is_dir(),
        "hand-crafted fixture missing at {}",
        src.display()
    );
    let tmp = tempfile::tempdir().expect("tempdir");
    let dst = tmp.path().join(name);
    copy_dir_all(&src, &dst).expect("copy fixture");
    (tmp, dst)
}

/// Clone `url` @ `rev` into the vendored cache if absent, then copy
/// to a fresh tempdir so the test has a clean mutable workspace.
/// Returns the tempdir + materialized path.
///
/// `rev` must be a full commit SHA — we shallow-fetch + checkout
/// exactly that to keep clones small and reproducible. Branches /
/// tags are deliberately not accepted: they move under our feet.
pub fn materialize_vendored(
    cache_key: &str,
    url: &str,
    rev: &str,
    subdir: Option<&str>,
) -> Option<(tempfile::TempDir, PathBuf)> {
    if !require_toolchain("git") {
        return None;
    }

    // Serialise clone-into-cache across parallel tests — cargo runs
    // integration tests concurrently and we don't want two threads
    // both fetching the same repo.
    static CLONE_LOCK: Mutex<()> = Mutex::new(());
    let _guard = CLONE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let cache_root = vendored_cache_dir().join(cache_key);
    if !cache_root.join(".git").is_dir() {
        if let Err(e) = ensure_clone(&cache_root, url, rev) {
            println!("[e2e] vendored clone failed for {cache_key}: {e}");
            return None;
        }
    }

    let source = match subdir {
        Some(sub) => cache_root.join(sub),
        None => cache_root.clone(),
    };
    assert!(
        source.is_dir(),
        "vendored fixture {cache_key} missing subdir {source:?}"
    );

    let tmp = tempfile::tempdir().expect("tempdir");
    copy_dir_all(&source, tmp.path()).expect("copy vendored");
    let path = tmp.path().to_path_buf();
    Some((tmp, path))
}

fn ensure_clone(dst: &Path, url: &str, rev: &str) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    // Init + shallow fetch + checkout keeps the clone tiny regardless
    // of the upstream repo's history size.
    let run = |args: &[&str]| -> Result<(), String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(dst)
            .output()
            .map_err(|e| format!("exec git {args:?}: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    };
    run(&["init", "--quiet"])?;
    run(&["remote", "add", "origin", url])?;
    run(&["fetch", "--quiet", "--depth", "1", "origin", rev])?;
    run(&["checkout", "--quiet", "FETCH_HEAD"])?;
    Ok(())
}

/// Recursive `cp -r`. stdlib's `fs::copy` doesn't handle directories.
/// Preserves the executable bit on Unix — `std::fs::copy` copies file
/// mode on Unix in practice, but documentation is silent on the
/// guarantee, so we re-apply the source mode explicitly. Matters for
/// fixtures that ship shell scripts (e.g. `gradle-hello/gradlew`).
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        // Skip any `.git` dir so vendored fixtures materialise as
        // pristine working copies — monad's diff pre-filter would
        // otherwise see them as dirty repos.
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if ft.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&from)?, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&from)?.permissions().mode();
                std::fs::set_permissions(&to, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// Captured outcome of a `monad` subprocess invocation.
#[derive(Debug)]
pub struct ProfileOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl ProfileOutcome {
    /// Parse stdout as JSON — use after `monad <verb> --json`. Panics
    /// on parse failure with a diagnostic that includes a preview of
    /// what was actually printed.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "monad stdout is not valid JSON: {e}\n\
                 stdout (first 500 chars): {}\n\
                 stderr: {}",
                self.stdout.chars().take(500).collect::<String>(),
                self.stderr,
            )
        })
    }
}

/// Invoke the built `monad` binary with the given args, from `cwd`.
/// Isolates cache + config via per-run temp dirs so concurrent tests
/// don't fight for `~/.monad/cache/*`.
pub fn run_profile(cwd: &Path, args: &[&str]) -> ProfileOutcome {
    let cache_dir = tempfile::tempdir().expect("monad cache tempdir");
    let out = Command::new(monad_bin())
        .args(args)
        .current_dir(cwd)
        // Point monad's cache at a per-invocation dir so parallel
        // tests don't collide on `~/.monad/cache`. Inherits everything
        // else (PATH, tool-specific envs) from the harness process.
        .env("MONAD_CACHE_DIR", cache_dir.path())
        // Disable telemetry for test runs — no behavioural effect,
        // but keeps the logs clean and avoids any outbound network.
        .env("MONAD_TELEMETRY", "0")
        .output()
        .expect("spawn monad");
    // Deliberately leak the cache tempdir: the tests that want
    // cross-invocation cache hits explicitly manage their own cache
    // via `run_monad_with_cache` below.
    std::mem::forget(cache_dir);
    ProfileOutcome {
        exit_code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Like [`run_profile`] but uses `cache_dir` instead of a throwaway
/// per-invocation tempdir — so a second call with the same cache_dir
/// sees the first run's cache entries. Used for hit/miss tests.
pub fn run_monad_with_cache(cwd: &Path, cache_dir: &Path, args: &[&str]) -> ProfileOutcome {
    let out = Command::new(monad_bin())
        .args(args)
        .current_dir(cwd)
        .env("MONAD_CACHE_DIR", cache_dir)
        .env("MONAD_TELEMETRY", "0")
        .output()
        .expect("spawn monad");
    ProfileOutcome {
        exit_code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Absolute path to the stub-CLI directory (`tests/e2e/bin/`). Phase-4
/// deploy tests prepend this to PATH so calls to `railway` / `vercel`
/// route to the hermetic stubs instead of the real CLIs.
pub fn stub_bin_dir() -> PathBuf {
    workspace_root().join("tests").join("e2e").join("bin")
}

/// Like [`run_monad_with_cache`] but prepends [`stub_bin_dir()`] to
/// PATH so `railway` / `vercel` / etc. in the child process resolve
/// to the test stubs. Also sets dummy values for the env vars the
/// deploy adapters' `required_env` gate on (`RAILWAY_TOKEN` today).
/// Extra env pairs can be forwarded via `extra_env` — tests use this
/// e.g. to inject a notify-output path for notification assertions.
pub fn run_monad_with_stubs(
    cwd: &Path,
    cache_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> ProfileOutcome {
    let stub_dir = stub_bin_dir();
    assert!(
        stub_dir.is_dir(),
        "stub bin dir missing at {}",
        stub_dir.display()
    );
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let mut new_path = std::ffi::OsString::from(&stub_dir);
    new_path.push(":");
    new_path.push(&existing_path);

    let mut cmd = Command::new(monad_bin());
    cmd.args(args)
        .current_dir(cwd)
        .env("MONAD_CACHE_DIR", cache_dir)
        .env("MONAD_TELEMETRY", "0")
        .env("PATH", new_path)
        // Dummy token — real value doesn't matter, just has to be set
        // so RailwayIntegration's required_env gate passes.
        .env("RAILWAY_TOKEN", "stub-token");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn monad");
    ProfileOutcome {
        exit_code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Opt-in flag for tests that require network access AND extra
/// ecosystem deps beyond the toolchain binary itself (rubygems
/// fetches for `bundle install`, `pip install build` for Python's
/// `python -m build` frontend, the gradle wrapper for `./gradlew`,
/// maven central for plugin resolution). Gated strictly behind
/// `MONAD_E2E_NETWORK=1` — the general `GITHUB_ACTIONS` env alone
/// isn't enough because the existing `test (stable)` CI job only
/// installs rustc. The Phase-5 dedicated e2e workflow opts in
/// explicitly after installing every toolchain + extra dep.
pub fn require_network() -> bool {
    if std::env::var_os("MONAD_E2E_NETWORK").is_some() {
        return true;
    }
    println!("[e2e] skipping: network-gated test; set MONAD_E2E_NETWORK=1 to run");
    false
}

/// Declarative description of one ecosystem's fixture + expectations.
/// The [`standard_suite`] below replays the same four-test pattern
/// across every ecosystem from one of these.
#[derive(Debug, Clone, Copy)]
pub struct EcosystemSpec {
    /// Hand-crafted fixture dir name under `tests/e2e/fixtures/`.
    pub fixture: &'static str,
    /// Binary that must be on PATH for the suite to run (`go`,
    /// `cargo`, `node`, `bundle`, etc). Missing → skip cleanly.
    pub toolchain: &'static str,
    /// Expected value of `unit.toml`'s `language` field after
    /// `monad unit add .` adopts the repo root. `"go"` / `"cargo"` /
    /// `"node-npm"` / …
    pub language_id: &'static str,
    /// Task names the adapter's defaults should surface in
    /// `monad plan --json` (subset check — integrations or user
    /// overrides may add more).
    pub expected_tasks: &'static [&'static str],
    /// `true` when the adapter's default `build` task hits the
    /// network on first run (rubygems, maven central, the gradle
    /// distribution). Gates the `build_caches` / `test_runs` tests
    /// behind [`require_network`].
    pub build_needs_network: bool,
    /// `true` when the adapter's default `test` task can run
    /// without network / external setup (Go / Rust / Node). `false`
    /// when it requires a test runner we don't ship in the fixture
    /// (pytest, rspec, phpunit) — the `test_runs` test skips.
    pub test_runs_offline: bool,
}

/// Standard four-test suite replayed per ecosystem. Each `#[test]`
/// in an ecosystem's module delegates to the corresponding function
/// here so behaviour stays uniform and additions propagate to every
/// ecosystem automatically.
pub mod standard_suite {
    use super::*;

    /// `monad init` + `monad unit add .` adopt the repo root as a
    /// single unit with the ecosystem's `language` id pinned.
    pub fn init_and_adopt(spec: &EcosystemSpec) {
        if !require_toolchain(spec.toolchain) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);

        let init = run_profile(&dir, &["init"]);
        assert_eq!(
            init.exit_code, 0,
            "[{}] monad init should succeed.\nstderr: {}\nstdout: {}",
            spec.fixture, init.stderr, init.stdout
        );
        assert!(
            dir.join("monad.toml").is_file(),
            "[{}] monad init writes monad.toml",
            spec.fixture
        );

        let adopt = run_profile(&dir, &["unit", "add", "."]);
        assert_eq!(
            adopt.exit_code, 0,
            "[{}] monad unit add . should adopt the root.\nstderr: {}\nstdout: {}",
            spec.fixture, adopt.stderr, adopt.stdout
        );
        let unit_toml_path = dir.join("unit.toml");
        assert!(
            unit_toml_path.is_file(),
            "[{}] adoption writes unit.toml at repo root",
            spec.fixture
        );
        let unit_toml = std::fs::read_to_string(&unit_toml_path).unwrap();
        let expected = format!("language = \"{}\"", spec.language_id);
        assert!(
            unit_toml.contains(&expected),
            "[{}] unit.toml should pin `{}`; got:\n{}",
            spec.fixture,
            expected,
            unit_toml
        );
    }

    /// `monad plan --json` surfaces the adapter's default tasks,
    /// and reports every task cache_miss on a fresh workspace.
    pub fn plan_reports_expected_tasks(spec: &EcosystemSpec) {
        if !require_toolchain(spec.toolchain) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);
        assert_eq!(run_profile(&dir, &["init"]).exit_code, 0);
        assert_eq!(
            run_profile(&dir, &["unit", "add", "."]).exit_code,
            0,
            "[{}] adoption must succeed before plan",
            spec.fixture
        );

        let outcome = run_profile(&dir, &["plan", "--json"]);
        assert_eq!(
            outcome.exit_code, 0,
            "[{}] monad plan --json should succeed.\nstderr: {}",
            spec.fixture, outcome.stderr
        );
        let plan = outcome.json();
        let unit = plan
            .pointer("/profiles/0/units/0")
            .unwrap_or_else(|| panic!("[{}] plan missing unit", spec.fixture));
        assert_eq!(
            unit.pointer("/language").and_then(|v| v.as_str()),
            Some(spec.language_id),
            "[{}] unit.language = {}; plan: {plan}",
            spec.fixture,
            spec.language_id
        );
        let task_names: Vec<&str> = unit
            .pointer("/tasks")
            .and_then(|t| t.as_array())
            .expect("tasks array")
            .iter()
            .filter_map(|t| t.pointer("/name").and_then(|n| n.as_str()))
            .collect();
        for required in spec.expected_tasks {
            assert!(
                task_names.contains(required),
                "[{}] plan should include task `{required}`; got: {task_names:?}",
                spec.fixture
            );
        }
        for task in unit.pointer("/tasks").unwrap().as_array().unwrap() {
            assert_eq!(
                task.pointer("/status").and_then(|s| s.as_str()),
                Some("cache_miss"),
                "[{}] first-run task should be cache_miss; task: {task}",
                spec.fixture
            );
        }
    }

    /// `monad build` succeeds, re-running against the same cache
    /// reports built=0 / hits≥1. Bread-and-butter cache invariant.
    pub fn build_caches_across_runs(spec: &EcosystemSpec) {
        if !require_toolchain(spec.toolchain) {
            return;
        }
        if spec.build_needs_network && !require_network() {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);
        assert_eq!(run_profile(&dir, &["init"]).exit_code, 0);
        assert_eq!(run_profile(&dir, &["unit", "add", "."]).exit_code, 0);

        let cache = tempfile::tempdir().expect("cache tempdir");
        let first = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            first.exit_code,
            0,
            "[{}] first monad build should succeed.\nstderr: {}\nstdout (first 500): {}",
            spec.fixture,
            first.stderr,
            first.stdout.chars().take(500).collect::<String>()
        );
        let first_summary = first.json().pointer("/summary").cloned().unwrap();
        assert_eq!(
            first_summary.pointer("/failed").and_then(|v| v.as_u64()),
            Some(0),
            "[{}] first run summary: {first_summary}",
            spec.fixture
        );
        assert!(
            first_summary
                .pointer("/built")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                >= 1,
            "[{}] first run should build ≥1 task; summary: {first_summary}",
            spec.fixture
        );

        let second = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            second.exit_code, 0,
            "[{}] second build stderr: {}",
            spec.fixture, second.stderr
        );
        let second_summary = second.json().pointer("/summary").cloned().unwrap();
        assert_eq!(
            second_summary.pointer("/built").and_then(|v| v.as_u64()),
            Some(0),
            "[{}] second run should not rebuild; summary: {second_summary}",
            spec.fixture
        );
        assert!(
            second_summary
                .pointer("/hits")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                >= 1,
            "[{}] second run should report ≥1 cache hit; summary: {second_summary}",
            spec.fixture
        );
    }

    /// `monad test` runs the adapter's test recipe end-to-end.
    /// Skipped when the adapter's default `test` needs external
    /// tooling (pytest, rspec, phpunit) that we don't ship in the
    /// fixture; those ecosystems have `test_runs_offline = false`.
    pub fn test_runs_to_completion(spec: &EcosystemSpec) {
        if !spec.test_runs_offline {
            println!(
                "[e2e] skipping {}::test_runs_to_completion: needs external test runner",
                spec.fixture
            );
            return;
        }
        if !require_toolchain(spec.toolchain) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);
        assert_eq!(run_profile(&dir, &["init"]).exit_code, 0);
        assert_eq!(run_profile(&dir, &["unit", "add", "."]).exit_code, 0);

        let outcome = run_profile(&dir, &["test", "--json"]);
        assert_eq!(
            outcome.exit_code,
            0,
            "[{}] monad test should succeed.\nstderr: {}\nstdout (first 500): {}",
            spec.fixture,
            outcome.stderr,
            outcome.stdout.chars().take(500).collect::<String>()
        );
        let summary = outcome.json().pointer("/summary").cloned().unwrap();
        assert_eq!(
            summary.pointer("/failed").and_then(|v| v.as_u64()),
            Some(0),
            "[{}] test summary: {summary}",
            spec.fixture
        );
    }
}

/// Declarative description of a polyglot monorepo fixture for the
/// [`monorepo_suite`] helpers. One per pre-populated fixture under
/// `tests/e2e/fixtures/monorepo-*`. The suite asserts plan shape, ci
/// cache semantics across multiple units, and — when the fixture
/// declares a dep-graph edge — cascade invalidation behaviour.
#[derive(Debug, Clone)]
pub struct MonorepoSpec {
    /// Fixture dir name under `tests/e2e/fixtures/`.
    pub fixture: &'static str,
    /// Toolchains every unit in the fixture needs on PATH. Missing any
    /// one skips the whole suite (polyglot by design — partial runs
    /// would give confusing diffs).
    pub toolchains: &'static [&'static str],
    /// Unites the fixture declares, as `(unit_name, language_id)`.
    /// Order-insensitive when asserted against `monad plan --json`.
    pub units: &'static [(&'static str, &'static str)],
    /// Task names expected on every unit's plan (subset check). The
    /// monorepo suite asserts the intersection — adapters may add
    /// more, users may override.
    pub common_tasks: &'static [&'static str],
}

/// Suite for polyglot-monorepo fixtures. Asserts the multi-unit
/// invariants: `plan --json` reports every unit with the right
/// language, `ci` builds all units on first run + hits cache on
/// second run, and auto-detect recovers the unit set after wiping
/// the pre-written configs.
pub mod monorepo_suite {
    use super::*;

    /// `monad plan --json` lists every unit declared by the fixture's
    /// `profiles/prod.toml`, each with the expected `language_id` + the
    /// common task set.
    pub fn plan_lists_every_unit(spec: &MonorepoSpec) {
        if !all_toolchains_present(spec.toolchains, spec.fixture) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);

        let outcome = run_profile(&dir, &["plan", "--json"]);
        assert_eq!(
            outcome.exit_code, 0,
            "[{}] monad plan --json should succeed.\nstderr: {}",
            spec.fixture, outcome.stderr
        );
        let plan = outcome.json();
        let units = plan
            .pointer("/profiles/0/units")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("[{}] plan missing profiles/0/units", spec.fixture));
        assert_eq!(
            units.len(),
            spec.units.len(),
            "[{}] plan reports {} units, expected {}: {plan}",
            spec.fixture,
            units.len(),
            spec.units.len()
        );
        for (name, language) in spec.units {
            let unit = units
                .iter()
                .find(|d| d.pointer("/name").and_then(|v| v.as_str()) == Some(*name))
                .unwrap_or_else(|| panic!("[{}] plan missing unit `{name}`: {plan}", spec.fixture));
            assert_eq!(
                unit.pointer("/language").and_then(|v| v.as_str()),
                Some(*language),
                "[{}] unit `{name}` language: {unit}",
                spec.fixture
            );
            let task_names: Vec<&str> = unit
                .pointer("/tasks")
                .and_then(|t| t.as_array())
                .expect("tasks array")
                .iter()
                .filter_map(|t| t.pointer("/name").and_then(|n| n.as_str()))
                .collect();
            for required in spec.common_tasks {
                assert!(
                    task_names.contains(required),
                    "[{}] unit `{name}` plan should include `{required}`; got: {task_names:?}",
                    spec.fixture
                );
            }
        }
    }

    /// First `monad build` builds every unit; second run against the
    /// same cache hits across the board. The core cache invariant but
    /// at monorepo scope — proves the cache key derivation holds when
    /// multiple units share a run. Uses `build` rather than `ci`
    /// deliberately: the full `ci` pipeline runs lint too, and the
    /// default go / cargo lint recipes need `golangci-lint` / clippy
    /// installed — which would gate the harness behind ambient
    /// dev-machine state we don't want to require.
    pub fn build_caches_across_runs(spec: &MonorepoSpec) {
        if !all_toolchains_present(spec.toolchains, spec.fixture) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);

        let cache = tempfile::tempdir().expect("cache tempdir");
        let first = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            first.exit_code,
            0,
            "[{}] first monad build should succeed.\nstderr: {}\nstdout (first 500): {}",
            spec.fixture,
            first.stderr,
            first.stdout.chars().take(500).collect::<String>()
        );
        let first_summary = first.json().pointer("/summary").cloned().unwrap();
        assert_eq!(
            first_summary.pointer("/failed").and_then(|v| v.as_u64()),
            Some(0),
            "[{}] first build summary: {first_summary}",
            spec.fixture
        );
        let first_built = first_summary
            .pointer("/built")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert!(
            first_built >= spec.units.len() as u64,
            "[{}] first build should cover ≥{} tasks (one per unit min); summary: {first_summary}",
            spec.fixture,
            spec.units.len()
        );

        let second = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            second.exit_code, 0,
            "[{}] second build stderr: {}",
            spec.fixture, second.stderr
        );
        let second_summary = second.json().pointer("/summary").cloned().unwrap();
        assert_eq!(
            second_summary.pointer("/built").and_then(|v| v.as_u64()),
            Some(0),
            "[{}] second build should be pure cache; summary: {second_summary}",
            spec.fixture
        );
        assert!(
            second_summary
                .pointer("/hits")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                >= spec.units.len() as u64,
            "[{}] second build should hit ≥{} cached tasks; summary: {second_summary}",
            spec.fixture,
            spec.units.len()
        );
    }

    /// Wipe the pre-written `monad.toml` + `profiles/` + every
    /// `unit.toml` from a materialised copy of the fixture, then run
    /// `monad init` and assert auto-detect rebuilds the same unit set.
    /// Proves init's subdir walk finds every language monad knows.
    pub fn init_auto_detects_every_subdir(spec: &MonorepoSpec) {
        if !all_toolchains_present(spec.toolchains, spec.fixture) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);

        // Strip all monad metadata so init starts from a pristine
        // polyglot checkout — sources only, no prior monad state.
        std::fs::remove_file(dir.join("monad.toml")).ok();
        if dir.join("profiles").is_dir() {
            std::fs::remove_dir_all(dir.join("profiles")).expect("remove profiles/");
        }
        for (unit_name, _) in spec.units {
            let unit_toml = dir.join(unit_name).join("unit.toml");
            if unit_toml.is_file() {
                std::fs::remove_file(&unit_toml).expect("remove unit.toml");
            }
        }

        let init = run_profile(&dir, &["init"]);
        assert_eq!(
            init.exit_code, 0,
            "[{}] monad init should succeed on pristine polyglot tree.\nstderr: {}\nstdout: {}",
            spec.fixture, init.stderr, init.stdout
        );
        assert!(
            dir.join("monad.toml").is_file(),
            "[{}] init writes monad.toml",
            spec.fixture
        );
        for (unit_name, language) in spec.units {
            let unit_toml = dir.join(unit_name).join("unit.toml");
            assert!(
                unit_toml.is_file(),
                "[{}] init should create {}/unit.toml",
                spec.fixture,
                unit_name
            );
            let body = std::fs::read_to_string(&unit_toml).unwrap();
            let expected = format!("language = \"{}\"", language);
            assert!(
                body.contains(&expected),
                "[{}] init-written unit.toml for `{}` should pin `{}`; got:\n{}",
                spec.fixture,
                unit_name,
                expected,
                body
            );
        }
    }

    /// For a fixture where `dependent` lists `dep` in its
    /// `depends_on`: prime the cache with `monad ci`, mutate `dep`'s
    /// source, re-run — every task on `dependent` must be a miss
    /// (effective-signature cascade), while every task on `dep` must
    /// also be a miss (direct content change). Other units in the
    /// fixture are expected to hit.
    pub fn cascade_invalidates_dependent(
        spec: &MonorepoSpec,
        dep: &str,
        dependent: &str,
        mutate: impl FnOnce(&Path),
    ) {
        if !all_toolchains_present(spec.toolchains, spec.fixture) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);

        let cache = tempfile::tempdir().expect("cache tempdir");
        let first = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            first.exit_code, 0,
            "[{}] priming build must succeed.\nstderr: {}",
            spec.fixture, first.stderr
        );

        mutate(&dir);

        let second = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            second.exit_code, 0,
            "[{}] post-mutation build must succeed.\nstderr: {}",
            spec.fixture, second.stderr
        );
        let plan_units = second
            .json()
            .pointer("/profiles/0/units")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let dep_statuses = unit_task_statuses(&plan_units, dep, spec.fixture);
        let dependent_statuses = unit_task_statuses(&plan_units, dependent, spec.fixture);

        assert!(
            dep_statuses.iter().all(|s| s == "built"),
            "[{}] `{}` (mutated) should be entirely rebuilt; statuses: {:?}",
            spec.fixture,
            dep,
            dep_statuses
        );
        assert!(
            dependent_statuses.iter().all(|s| s == "built"),
            "[{}] `{}` depends on `{}`; cascade should rebuild everything; statuses: {:?}",
            spec.fixture,
            dependent,
            dep,
            dependent_statuses
        );
    }

    /// Twin of [`cascade_invalidates_dependent`] with
    /// `force_independent = true` patched into `dependent`'s
    /// `unit.toml` before the priming run. After mutating `dep`,
    /// `dependent` must *stay* cached — that's the documented escape
    /// hatch and we want a regression test for the day someone
    /// "cleans up" the special case.
    pub fn force_independent_breaks_cascade(
        spec: &MonorepoSpec,
        dep: &str,
        dependent: &str,
        mutate: impl FnOnce(&Path),
    ) {
        if !all_toolchains_present(spec.toolchains, spec.fixture) {
            return;
        }
        let (_tmp, dir) = materialize_hand_crafted(spec.fixture);

        // Patch in force_independent = true on the dependent unit.
        // Simpler than re-parsing TOML: append the line — the existing
        // unit.toml doesn't set it, so appending is unambiguous.
        let unit_toml_path = dir.join(dependent).join("unit.toml");
        let mut body = std::fs::read_to_string(&unit_toml_path).expect("read unit.toml");
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str("force_independent = true\n");
        std::fs::write(&unit_toml_path, body).expect("write unit.toml");

        let cache = tempfile::tempdir().expect("cache tempdir");
        let first = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            first.exit_code, 0,
            "[{}] priming build must succeed.\nstderr: {}",
            spec.fixture, first.stderr
        );

        mutate(&dir);

        let second = run_monad_with_cache(&dir, cache.path(), &["build", "--json"]);
        assert_eq!(
            second.exit_code, 0,
            "[{}] post-mutation build must succeed.\nstderr: {}",
            spec.fixture, second.stderr
        );
        let plan_units = second
            .json()
            .pointer("/profiles/0/units")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let dep_statuses = unit_task_statuses(&plan_units, dep, spec.fixture);
        let dependent_statuses = unit_task_statuses(&plan_units, dependent, spec.fixture);

        assert!(
            dep_statuses.iter().all(|s| s == "built"),
            "[{}] `{}` (mutated) should still be rebuilt; statuses: {:?}",
            spec.fixture,
            dep,
            dep_statuses
        );
        assert!(
            dependent_statuses.iter().all(|s| s == "cache_hit"),
            "[{}] `{}` has force_independent=true; cascade should NOT propagate; statuses: {:?}",
            spec.fixture,
            dependent,
            dependent_statuses
        );
    }

    fn all_toolchains_present(tools: &[&str], fixture: &str) -> bool {
        for tool in tools {
            if which::which(tool).is_err() {
                println!("[e2e] skipping {fixture}: `{tool}` not on PATH");
                return false;
            }
        }
        true
    }

    fn unit_task_statuses(
        units: &[serde_json::Value],
        unit_name: &str,
        fixture: &str,
    ) -> Vec<String> {
        let unit = units
            .iter()
            .find(|d| d.pointer("/name").and_then(|v| v.as_str()) == Some(unit_name))
            .unwrap_or_else(|| panic!("[{fixture}] plan missing unit `{unit_name}`"));
        unit.pointer("/tasks")
            .and_then(|t| t.as_array())
            .expect("tasks array")
            .iter()
            .filter_map(|t| {
                t.pointer("/status")
                    .and_then(|s| s.as_str())
                    .map(String::from)
            })
            .collect()
    }
}
