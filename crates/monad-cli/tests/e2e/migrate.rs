//! Cross-migrator end-to-end harness — for each `monad migrate <tool>`,
//! materialise the source-tool fixture into a tempdir, run the
//! migrator, and validate the migrated workspace by running `monad
//! plan --json` against it. Each test asserts:
//!
//!   - the migrator exits 0,
//!   - the report has `applied: true` and at least one `files_written`
//!     entry,
//!   - the resulting workspace passes `monad plan --json` without
//!     error and reports at least one monad with at least one unit.
//!
//! Fixture monorepos live under `tests/e2e/fixtures/migrate-<tool>/`.
//! They're parser-only — no `node_modules`, no source code, just the
//! source tool's manifests + per-package `package.json` (or
//! equivalent) — so the test stays hermetic and fast.
//!
//! Per-migrator unit tests inside `crates/monad-cli/src/migrate/<tool>.rs`
//! cover the parser/emitter logic in isolation. This file's job is the
//! end-to-end contract: a real monad binary, a realistic source-tool
//! workspace, and a valid plan as the success criterion.

use std::path::Path;

use super::common::{
    materialize_hand_crafted, materialize_vendored, require_network, run_profile, ProfileOutcome,
};

/// Run `monad migrate <tool> --force --json` against the materialised
/// fixture, assert success + that the report claims to have written
/// files, then run `monad plan --json` against the migrated result and
/// assert the plan parses + contains at least one unit.
fn migrate_and_plan(fixture: &str, tool: &str) {
    let (_tmp, workspace) = materialize_hand_crafted(fixture);

    // Step 1 — invoke the migrator.
    let migrate = run_profile(&workspace, &["migrate", tool, "--force", "--json"]);
    assert_migrate_succeeded(&migrate, tool, &workspace);

    let report = migrate.json();
    let applied = report
        .get("applied")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        applied,
        "[migrate {tool}] report.applied was not true:\n{}",
        migrate.stdout,
    );
    let files_written = report
        .get("files_written")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert!(
        files_written > 0,
        "[migrate {tool}] report listed zero files_written; full report:\n{}",
        migrate.stdout,
    );

    // Step 2 — sanity-check the canonical monad outputs landed.
    assert!(
        workspace.join("monad.toml").exists(),
        "[migrate {tool}] monad.toml missing after migrate",
    );
    assert!(
        workspace.join("profiles/prod.toml").exists(),
        "[migrate {tool}] profiles/prod.toml missing after migrate",
    );

    // Step 3 — plan the migrated workspace. This is the proof the
    // migrator emitted *valid* config, not just any config.
    let plan = run_profile(&workspace, &["plan", "--json"]);
    assert_eq!(
        plan.exit_code, 0,
        "[migrate {tool} → plan] exit={} stderr=\n{}\nstdout=\n{}",
        plan.exit_code, plan.stderr, plan.stdout,
    );

    let plan_json = plan.json();
    let profiles = plan_json
        .get("profiles")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!(
                "[migrate {tool}] plan JSON had no `profiles` array:\n{}",
                plan.stdout
            )
        });
    assert!(
        !profiles.is_empty(),
        "[migrate {tool}] plan emitted zero profiles; the migrator is producing config the planner can't see",
    );
    let total_units: usize = profiles
        .iter()
        .map(|b| {
            b.get("units")
                .and_then(|d| d.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        })
        .sum();
    assert!(
        total_units > 0,
        "[migrate {tool}] plan reported zero units across {} monad(s); migration didn't wire anything in",
        profiles.len(),
    );
}

fn assert_migrate_succeeded(out: &ProfileOutcome, tool: &str, workspace: &Path) {
    assert_eq!(
        out.exit_code,
        0,
        "[migrate {tool}] in {} exited {}; stderr=\n{}\nstdout=\n{}",
        workspace.display(),
        out.exit_code,
        out.stderr,
        out.stdout,
    );
}

#[test]
fn lerna_workspace_migrates_and_plans() {
    migrate_and_plan("migrate-lerna", "lerna");
}

#[test]
fn makefile_migrates_and_plans() {
    migrate_and_plan("migrate-make", "make");
}

#[test]
fn moon_workspace_migrates_and_plans() {
    migrate_and_plan("migrate-moon", "moon");
}

#[test]
fn rush_workspace_migrates_and_plans() {
    migrate_and_plan("migrate-rush", "rush");
}

// ── Vendored real-world fixtures ───────────────────────────────────
//
// The synthesized fixtures above keep CI hermetic and fast; these
// tests below clone real published OSS monorepos at pinned commits
// and run the migrator against them. They catch the class of bugs
// where "the synthesized fixture was too clean to expose this real-
// world quirk" — per-package overrides, hybrid configs, JSONC tail
// commas, deeply nested project files, etc.
//
// Gated on `MONAD_E2E_NETWORK=1` so the default test invocation never
// reaches out. Tag-push CI runs with the flag set so launch-day
// releases exercise these paths. Each test skips cleanly if `git`
// isn't on PATH or the clone fails.

/// Drive `monad migrate <tool> --force --json` against a vendored
/// real-world fixture, assert the migrator exits 0, then validate the
/// migrated workspace with `monad plan --json`. Skips when the network
/// flag is off or the clone fails (stale rev, offline runner, etc.).
fn migrate_and_plan_vendored(
    cache_key: &str,
    url: &str,
    rev: &str,
    subdir: Option<&str>,
    tool: &str,
) {
    if !require_network() {
        eprintln!(
            "[migrate {tool}] skipping vendored fixture {cache_key}: MONAD_E2E_NETWORK not set"
        );
        return;
    }
    let Some((_tmp, workspace)) = materialize_vendored(cache_key, url, rev, subdir) else {
        eprintln!("[migrate {tool}] skipping vendored fixture {cache_key}: clone failed");
        return;
    };

    let migrate = run_profile(&workspace, &["migrate", tool, "--force", "--json"]);
    assert_migrate_succeeded(&migrate, tool, &workspace);

    let report = migrate.json();
    let applied = report
        .get("applied")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        applied,
        "[migrate {tool} / {cache_key}] report.applied was not true:\n{}",
        migrate.stdout,
    );
    let files_written = report
        .get("files_written")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    // Vendored fixtures can legitimately have no migratable content
    // in a given subdir — surface that as a clear assertion failure
    // (the wrong subdir was pinned) rather than silently passing.
    assert!(
        files_written > 0,
        "[migrate {tool} / {cache_key}] report listed zero files_written; full report:\n{}",
        migrate.stdout,
    );

    assert_workspace_files_present(&workspace, tool, cache_key);
    assert_plan_succeeds(&workspace, tool, cache_key);
}

fn assert_workspace_files_present(workspace: &Path, tool: &str, label: &str) {
    assert!(
        workspace.join("monad.toml").exists(),
        "[migrate {tool} / {label}] monad.toml missing after migrate",
    );
    assert!(
        workspace.join("profiles/prod.toml").exists(),
        "[migrate {tool} / {label}] profiles/prod.toml missing after migrate",
    );
}

fn assert_plan_succeeds(workspace: &Path, tool: &str, label: &str) {
    let plan = run_profile(workspace, &["plan", "--json"]);
    assert_eq!(
        plan.exit_code, 0,
        "[migrate {tool} / {label} → plan] exit={} stderr=\n{}\nstdout=\n{}",
        plan.exit_code, plan.stderr, plan.stdout,
    );
    let plan_json = plan.json();
    let profiles = plan_json
        .get("profiles")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!(
                "[migrate {tool} / {label}] plan JSON had no `profiles` array:\n{}",
                plan.stdout
            )
        });
    assert!(
        !profiles.is_empty(),
        "[migrate {tool} / {label}] plan emitted zero profiles",
    );
    let total_units: usize = profiles
        .iter()
        .map(|b| {
            b.get("units")
                .and_then(|d| d.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        })
        .sum();
    assert!(
        total_units > 0,
        "[migrate {tool} / {label}] plan reported zero units across {} monad(s)",
        profiles.len(),
    );
}

#[test]
fn turbo_vercel_examples_basic_migrates_and_plans() {
    // vercel/turbo's official `examples/basic` template — production-shape
    // Turborepo with apps/ + packages/ + pnpm-workspaces. Pinned to a
    // mid-2026 main commit; bumping is a `git log` + paste away.
    migrate_and_plan_vendored(
        "vercel-turbo-examples-basic",
        "https://github.com/vercel/turbo.git",
        "09900b9151a852a3fa289aee007b03cee1d32288",
        Some("examples/basic"),
        "turbo",
    );
}

#[test]
fn lerna_self_hosted_workspace_migrates_and_plans() {
    // lerna/lerna IS itself a Lerna monorepo — the most authoritative
    // real-world Lerna config we can point at. Exercises the
    // `npmClient` pick + many-package walk + post-Nx-acquisition
    // hybrid shape (lerna 7+ delegates internally to nx).
    migrate_and_plan_vendored(
        "lerna-lerna",
        "https://github.com/lerna/lerna.git",
        "f4387d673bfdf4923ab62cd52d3498dec6dc7f2c",
        None,
        "lerna",
    );
}

#[test]
fn moon_examples_repo_migrates_and_plans() {
    // moonrepo/examples is the official multi-language moon example
    // repository — exercises every adapter the moon migrator touches
    // (typescript, rust, deno, etc.) plus toolchain blocks at the
    // workspace.yml top level.
    migrate_and_plan_vendored(
        "moonrepo-examples",
        "https://github.com/moonrepo/examples.git",
        "b38838408ab50c9af6647a252f06d761b3a5a4f2",
        None,
        "moon",
    );
}
