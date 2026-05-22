//! `monad prime` data layer.
//!
//! Computes a structured snapshot of a workspace: what units and
//! profiles exist, cache state, a plan preview, and a ranked list of
//! recommended next verbs. Shared by the `monad` CLI (via
//! `monad prime` / `monad prime --json`) and `monad-mcp` (via the
//! `monad_prime` tool).
//!
//! Advisory only — every field is informational. Pure read; does not
//! execute tasks and does not make network calls. For reachability /
//! credential checks, use `monad doctor --cloud`.

use anyhow::Result;
use schemars::JsonSchema;
use serde::Serialize;

use monad_config::Workspace;

use crate::{plan_at, scan_orphan_units, MissReason, PlanOptions, TaskStatus};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Output {
    /// Absolute path to the workspace root.
    pub workspace_root: String,
    pub profiles: Vec<ProfileRef>,
    pub units: Vec<UnitRef>,
    /// Workspace-relative paths of `unit.toml` files on disk that aren't
    /// referenced by any monad. Mirrors the `orphans` field of
    /// `monad unit list --json`.
    pub orphan_units: Vec<String>,
    pub cache: CacheStatus,
    pub plan: PlanSnapshot,
    /// Ordered next-step suggestions. Agents should follow the first
    /// and fall back to later ones. Always at least one entry.
    pub recommended_next: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProfileRef {
    pub name: String,
    pub source: String,
    pub unit_count: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UnitRef {
    pub name: String,
    pub path: String,
    pub language: Option<String>,
    pub profiles: Vec<String>,
    /// Stable IDs of any `[integrations.*]` blocks on this unit (e.g.
    /// `cloudflare_pages`, `railway`). Empty when no integrations are
    /// configured.
    pub integrations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CacheStatus {
    /// Local cache directory from `[cache] local` (or the default).
    pub local_dir: String,
    /// Whether `local_dir` exists on disk.
    pub local_exists: bool,
    /// Remote cache URL from `[cache] remote`, if configured.
    pub remote_url: Option<String>,
    /// Env var name holding the remote JWT, if `[cache] remote_token_env`
    /// is set. The value is never read into prime output.
    pub remote_token_env: Option<String>,
    /// `true` when `remote_token_env` is set AND that env var is
    /// present in the current environment. When false with a
    /// configured remote, the remote tier will silently degrade to
    /// local-only.
    pub remote_token_resolved: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlanSnapshot {
    /// First handful of tasks monad would run right now. Ordered by the
    /// default planner traversal (alphabetical over profiles and units).
    /// Capped at 5 entries to keep prime output compact.
    pub preview: Vec<PlanTask>,
    /// Total task count across every monad in the workspace.
    pub total_tasks: usize,
    /// Hit / miss / skipped counts across every monad.
    pub hits: usize,
    pub misses: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlanTask {
    pub unit: String,
    pub task: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss_reason: Option<MissReason>,
}

/// Produce the prime [`Output`] for a loaded workspace.
///
/// Runs `plan_at` internally to compute the preview — read-only, no
/// task execution.
pub fn compute(workspace: &Workspace) -> Result<Output> {
    let profiles = collect_profiles(workspace);
    let units = collect_units(workspace);
    let orphan_units: Vec<String> = scan_orphan_units(workspace)
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let cache = collect_cache(workspace);
    let plan = collect_plan(&workspace.root)?;
    let recommended_next = recommend_next(workspace, &orphan_units, &cache, &plan);

    Ok(Output {
        workspace_root: workspace.root.display().to_string(),
        profiles,
        units,
        orphan_units,
        cache,
        plan,
        recommended_next,
    })
}

fn collect_profiles(ws: &Workspace) -> Vec<ProfileRef> {
    let mut out: Vec<ProfileRef> = ws
        .profiles
        .values()
        .map(|b| ProfileRef {
            name: b.config.name.clone(),
            source: b
                .source
                .strip_prefix(&ws.root)
                .unwrap_or(&b.source)
                .to_string_lossy()
                .to_string(),
            unit_count: b.config.units.len(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn collect_units(ws: &Workspace) -> Vec<UnitRef> {
    let mut out: Vec<UnitRef> = ws
        .units_by_name
        .values()
        .map(|d| {
            let rel = d.rel.to_string_lossy().to_string();
            let profiles = ws
                .profiles
                .values()
                .filter(|b| b.config.units.iter().any(|dp| dp == &rel))
                .map(|b| b.config.name.clone())
                .collect();
            let integrations: Vec<String> = d.config.integrations.keys().cloned().collect();
            UnitRef {
                name: d.config.name.clone(),
                path: rel,
                language: d.config.language.clone(),
                profiles,
                integrations,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn collect_cache(ws: &Workspace) -> CacheStatus {
    let local_dir = crate::default_cache_root()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "(unknown)".to_string());
    let local_exists = std::path::PathBuf::from(&local_dir).is_dir();

    let (remote_url, remote_token_env, remote_token_resolved) = {
        let cache = &ws.repo.cache;
        let url = cache.remote.clone();
        let env_name = cache.remote_token_env.clone();
        let resolved = env_name
            .as_ref()
            .map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()).is_some())
            .unwrap_or(false);
        (url, env_name, resolved)
    };

    CacheStatus {
        local_dir,
        local_exists,
        remote_url,
        remote_token_env,
        remote_token_resolved,
    }
}

fn collect_plan(root: &std::path::Path) -> Result<PlanSnapshot> {
    let plan = plan_at(root, &PlanOptions::default())?;
    let mut preview: Vec<PlanTask> = Vec::new();
    for monad in &plan.profiles {
        for unit in &monad.units {
            for task in &unit.tasks {
                if preview.len() >= 5 {
                    break;
                }
                preview.push(PlanTask {
                    unit: unit.name.clone(),
                    task: task.name.clone(),
                    status: task.status,
                    miss_reason: task.miss_reason,
                });
            }
            if preview.len() >= 5 {
                break;
            }
        }
        if preview.len() >= 5 {
            break;
        }
    }
    Ok(PlanSnapshot {
        preview,
        total_tasks: plan.summary.tasks,
        hits: plan.summary.hits,
        misses: plan.summary.misses,
        skipped: plan.summary.skipped,
    })
}

fn recommend_next(
    ws: &Workspace,
    orphans: &[String],
    cache: &CacheStatus,
    plan: &PlanSnapshot,
) -> Vec<String> {
    let mut steps: Vec<String> = Vec::new();

    if ws.profiles.is_empty() {
        steps.push("run `monad box add <name>` to create your first monad".to_string());
    }
    if ws.units_by_name.is_empty() {
        steps.push("run `monad unit add <path> --lang <lang>` to scaffold a unit".to_string());
    }
    if !orphans.is_empty() {
        steps.push(format!(
            "{} orphan unit.toml file(s) not in any monad — `monad unit list --json` to see them, \
             then `monad unit add <path>` to wire each one",
            orphans.len()
        ));
    }
    if cache.remote_url.is_some() && !cache.remote_token_resolved {
        steps.push(format!(
            "remote cache is configured but {} is not set in the environment — export it or \
             unset `remote_token_env` in [cache]",
            cache
                .remote_token_env
                .as_deref()
                .unwrap_or("the token env var")
        ));
    }
    if plan.misses > 0 && plan.hits == 0 {
        steps.push(
            "no cache yet — run `monad install` then `monad ci` to prime the cache".to_string(),
        );
    } else if plan.misses > 0 {
        steps.push(format!(
            "{} task(s) would miss cache — run `monad ci` to build+cache them",
            plan.misses
        ));
    }
    if steps.is_empty() {
        steps.push(
            "workspace is cache-warm; run `monad plan` to inspect, \
             `monad build <target>` to build, or `monad deploy <target>` to ship"
                .to_string(),
        );
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_cache() -> CacheStatus {
        CacheStatus {
            local_dir: "/tmp".into(),
            local_exists: false,
            remote_url: None,
            remote_token_env: None,
            remote_token_resolved: false,
        }
    }

    fn mk_plan() -> PlanSnapshot {
        PlanSnapshot {
            preview: vec![],
            total_tasks: 0,
            hits: 0,
            misses: 0,
            skipped: 0,
        }
    }

    fn empty_workspace() -> Workspace {
        let tmp = tempfile::tempdir().unwrap();
        Workspace {
            root: tmp.path().to_path_buf(),
            repo: monad_config::RepoConfig::default(),
            profiles: Default::default(),
            units_by_path: Default::default(),
            units_by_name: Default::default(),
        }
    }

    #[test]
    fn recommend_next_always_returns_at_least_one_step() {
        let ws = empty_workspace();
        let steps = recommend_next(&ws, &[], &mk_cache(), &mk_plan());
        assert!(!steps.is_empty());
        assert!(
            steps.iter().any(|s| s.contains("monad box add")),
            "empty workspace should suggest creating a monad first, got {steps:?}"
        );
    }

    #[test]
    fn recommend_flags_missing_remote_token() {
        let ws = empty_workspace();
        let cache = CacheStatus {
            local_dir: "/tmp".into(),
            local_exists: true,
            remote_url: Some("monad://cache.monad.build".into()),
            remote_token_env: Some("MONAD_CACHE_TOKEN".into()),
            remote_token_resolved: false,
        };
        let steps = recommend_next(&ws, &[], &cache, &mk_plan());
        assert!(
            steps.iter().any(|s| s.contains("MONAD_CACHE_TOKEN")),
            "should flag missing remote token env var, got {steps:?}"
        );
    }

    #[test]
    fn recommend_warm_cache_is_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let mut profiles = std::collections::BTreeMap::new();
        profiles.insert(
            "prod".to_string(),
            monad_config::LoadedProfile {
                config: monad_config::ProfileConfig {
                    name: "prod".into(),
                    units: vec!["apps/api".into()],
                },
                source: tmp.path().join("profiles/prod.toml"),
            },
        );
        let mut units = std::collections::BTreeMap::new();
        units.insert(
            "api".to_string(),
            monad_config::LoadedUnit {
                config: monad_config::UnitConfig {
                    name: "api".into(),
                    language: Some("bun".into()),
                    ..Default::default()
                },
                dir: tmp.path().join("apps/api"),
                rel: "apps/api".into(),
            },
        );
        let ws = Workspace {
            root: tmp.path().to_path_buf(),
            repo: monad_config::RepoConfig::default(),
            profiles,
            units_by_path: Default::default(),
            units_by_name: units,
        };
        let plan = PlanSnapshot {
            preview: vec![],
            total_tasks: 3,
            hits: 3,
            misses: 0,
            skipped: 0,
        };
        let cache = CacheStatus {
            local_dir: "/tmp".into(),
            local_exists: true,
            remote_url: None,
            remote_token_env: None,
            remote_token_resolved: false,
        };
        let steps = recommend_next(&ws, &[], &cache, &plan);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].contains("cache-warm"));
    }
}
