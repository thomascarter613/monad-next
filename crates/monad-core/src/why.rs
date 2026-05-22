//! `monad why` data layer.
//!
//! Looks up a cache entry by either `<unit>:<task>` (resolved via a
//! plan pass) or a cache-key hex prefix, and returns the stored
//! [`InputManifest`] + key for each match. Pure read; no mutation, no
//! network. Shared by the `monad` CLI (via `monad why`) and by
//! `monad-mcp` (via the `monad_why` tool).
//!
//! Two input forms:
//!   - `<unit>:<task>` — resolved via [`resolve_unit_task_key`]; misspellings
//!     hit [`WhyTargetError::UnitNotFound`] / [`WhyTargetError::TaskNotFound`]
//!     with next_steps enumerating the available names.
//!   - `<hex-prefix>` — any prefix of a cache key, passed straight to
//!     [`explain`].

use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;

use monad_cache::{InputManifest, LocalCache};
use monad_config::Workspace;

use crate::{plan_at, PlanOptions, TaskStatus};

#[derive(Debug, Serialize, JsonSchema)]
pub struct Explanation {
    pub key: String,
    pub manifest: Option<InputManifest>,
}

/// Classified failures when resolving a `monad why` target. Downcast
/// through the CLI's error classifier so each variant becomes a
/// distinct `kind` in the structured envelope.
#[derive(Debug, thiserror::Error)]
pub enum WhyTargetError {
    #[error("invalid target '{input}' — must be `<unit>:<task>` or a cache-key hex prefix")]
    InvalidUnitTask { input: String },

    #[error("no unit named '{unit}' in this workspace")]
    UnitNotFound {
        unit: String,
        available: Vec<String>,
    },

    #[error("unit '{unit}' has no task named '{task}'")]
    TaskNotFound {
        unit: String,
        task: String,
        available: Vec<String>,
    },

    #[error("no cache entry for {unit}:{task} yet (key {key})")]
    NoCacheEntry {
        unit: String,
        task: String,
        key: String,
    },
}

/// Look up a single cache key by `unit:task`. Runs a plan pass to get
/// the key — cheap because planning is read-only (no adapter execution).
pub fn resolve_unit_task_key(workspace_root: &Path, target: &str) -> anyhow::Result<String> {
    let (unit_name, task_name) =
        target
            .split_once(':')
            .ok_or_else(|| WhyTargetError::InvalidUnitTask {
                input: target.to_string(),
            })?;

    let workspace = Workspace::load(workspace_root)?;
    let available_units: Vec<String> = workspace.units_by_name.keys().cloned().collect();
    if !workspace.units_by_name.contains_key(unit_name) {
        return Err(WhyTargetError::UnitNotFound {
            unit: unit_name.to_string(),
            available: available_units,
        }
        .into());
    }

    let plan = plan_at(
        workspace_root,
        &PlanOptions {
            unit_filter: Some(unit_name.to_string()),
            ..Default::default()
        },
    )?;

    for monad in &plan.profiles {
        for unit in &monad.units {
            if unit.name != unit_name {
                continue;
            }
            if let Some(task) = unit.tasks.iter().find(|t| t.name == task_name) {
                // `NoAdapter` stubs have no real key — they shouldn't reach
                // the `monad why` flow.
                if matches!(task.status, TaskStatus::NoAdapter) {
                    return Err(WhyTargetError::TaskNotFound {
                        unit: unit_name.to_string(),
                        task: task_name.to_string(),
                        available: unit
                            .tasks
                            .iter()
                            .filter(|t| !matches!(t.status, TaskStatus::NoAdapter))
                            .map(|t| t.name.clone())
                            .collect(),
                    }
                    .into());
                }
                return Ok(task.key.clone());
            }
            return Err(WhyTargetError::TaskNotFound {
                unit: unit_name.to_string(),
                task: task_name.to_string(),
                available: unit.tasks.iter().map(|t| t.name.clone()).collect(),
            }
            .into());
        }
    }
    // Unit exists in units_by_name but didn't appear in the plan — this
    // shouldn't happen with the current planner, but keep the error
    // typed in case it ever does.
    Err(WhyTargetError::UnitNotFound {
        unit: unit_name.to_string(),
        available: available_units,
    }
    .into())
}

/// Look up every cache entry with a key starting with `prefix` and
/// return the stored [`InputManifest`] for each match. Empty vec when
/// no keys match the prefix.
pub fn explain(cache: &LocalCache, prefix: &str) -> anyhow::Result<Vec<Explanation>> {
    let keys = cache.find_by_prefix(prefix)?;
    let mut out = Vec::with_capacity(keys.len());
    for key in keys {
        let manifest = cache.read_manifest(&key)?;
        out.push(Explanation {
            key: key.as_hex().to_string(),
            manifest,
        });
    }
    Ok(out)
}
