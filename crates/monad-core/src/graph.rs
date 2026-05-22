//! Dependency graph for the units inside a monad.
//!
//! A unit's `depends_on` field in `unit.toml` names other units (by their
//! `name`, not path). Those references define a DAG, scoped to each monad:
//! a unit only "sees" dependencies that are also listed in the same
//! `profiles/<name>.toml`. Cross-monad dependencies are intentionally
//! rejected — they would make deploy units non-self-contained.
//!
//! The graph is built once at the top of a `plan` or `ci` invocation and
//! used to (a) run independent units in parallel and (b) refuse to start
//! a dependent unit until all its deps have finished successfully.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use monad_config::Workspace;

/// Result of topologically sorting a monad's unit DAG.
///
/// `levels` is a Kahn-style layering: each inner `Vec<String>` holds unit
/// names whose dependencies are *all* in earlier levels. Within a level,
/// units have no ordering constraint and can be executed in parallel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileGraph {
    pub monad: String,
    pub levels: Vec<Vec<String>>,
}

impl ProfileGraph {
    /// Every unit name referenced by this graph, in the topo-sorted order
    /// they would run (level-major). Useful for tests and graph printers.
    pub fn flattened(&self) -> Vec<&str> {
        self.levels
            .iter()
            .flat_map(|l| l.iter().map(String::as_str))
            .collect()
    }

    /// Total number of units in the graph. Equal to the sum of level sizes.
    pub fn unit_count(&self) -> usize {
        self.levels.iter().map(|l| l.len()).sum()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// A unit names a dependency that isn't part of the same monad.
    #[error(
        "unit '{unit}' in monad '{monad}' depends_on '{missing}', which is not a unit of this monad"
    )]
    UnknownDep {
        monad: String,
        unit: String,
        missing: String,
    },

    /// Cycles are reported with the members of every cycle component so
    /// the user can localise the fix.
    #[error(
        "cycle detected in monad '{monad}' among units: {}",
        cycle.join(" → ")
    )]
    Cycle { monad: String, cycle: Vec<String> },
}

/// Build the DAG for a single monad and return its topologically-layered
/// execution plan.
pub fn build(workspace: &Workspace, monad_name: &str) -> Result<ProfileGraph, GraphError> {
    let monad = workspace
        .profiles
        .get(monad_name)
        .expect("caller must hand in a monad that belongs to this workspace");

    // Unit names included in *this* monad. Only refs within this set are valid.
    let mut units_in_profile: BTreeSet<String> = BTreeSet::new();
    let mut name_by_ref: BTreeMap<String, String> = BTreeMap::new();
    for unit_ref in &monad.config.units {
        let loaded = workspace
            .units_by_path
            .get(std::path::Path::new(unit_ref))
            .expect("workspace load guaranteed this reference resolves");
        units_in_profile.insert(loaded.config.name.clone());
        name_by_ref.insert(unit_ref.clone(), loaded.config.name.clone());
    }

    // Build reverse-adjacency (dep → dependents) + in-degrees.
    //
    // `deps[name]` is the set of units `name` waits on;
    // `dependents[name]` is the set of units that wait on `name`.
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for unit_name in &units_in_profile {
        deps.entry(unit_name.clone()).or_default();
        dependents.entry(unit_name.clone()).or_default();
    }

    for unit_ref in &monad.config.units {
        let loaded = &workspace.units_by_path[std::path::Path::new(unit_ref)];
        let unit_name = &loaded.config.name;
        for dep_name in &loaded.config.depends_on {
            if !units_in_profile.contains(dep_name) {
                return Err(GraphError::UnknownDep {
                    monad: monad_name.to_string(),
                    unit: unit_name.clone(),
                    missing: dep_name.clone(),
                });
            }
            deps.get_mut(unit_name).unwrap().insert(dep_name.clone());
            dependents
                .get_mut(dep_name)
                .unwrap()
                .insert(unit_name.clone());
        }
    }

    // Kahn: seed the queue with zero-in-degree units, peel layer by layer.
    let mut levels: Vec<Vec<String>> = Vec::new();
    let mut remaining: BTreeMap<String, usize> =
        deps.iter().map(|(k, v)| (k.clone(), v.len())).collect();

    let mut ready: VecDeque<String> = remaining
        .iter()
        .filter(|&(_, n)| *n == 0)
        .map(|(k, _)| k.clone())
        .collect();

    while !ready.is_empty() {
        // Snapshot the current ready set as one level.
        let mut level: Vec<String> = ready.drain(..).collect();
        level.sort();
        for name in &level {
            remaining.remove(name);
            for dependent in &dependents[name] {
                if let Some(n) = remaining.get_mut(dependent) {
                    *n -= 1;
                    if *n == 0 {
                        ready.push_back(dependent.clone());
                    }
                }
            }
        }
        levels.push(level);
    }

    if !remaining.is_empty() {
        // Whatever's left is in one or more cycles. Surface them all.
        let cycle: Vec<String> = remaining.keys().cloned().collect();
        return Err(GraphError::Cycle {
            monad: monad_name.to_string(),
            cycle,
        });
    }

    Ok(ProfileGraph {
        monad: monad_name.to_string(),
        levels,
    })
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Builds a workspace with the shape `apps/<name>/unit.toml` for each
    /// `(name, depends_on)` entry. Every unit goes into a single monad
    /// called "prod".
    fn workspace_with_deps(units: &[(&str, &[&str])]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("profiles")).unwrap();

        let unit_refs: Vec<String> = units
            .iter()
            .map(|(name, _)| format!("apps/{name}"))
            .collect();
        let refs_toml = unit_refs
            .iter()
            .map(|d| format!(r#""{d}""#))
            .collect::<Vec<_>>()
            .join(", ");
        std::fs::write(
            root.join("profiles/prod.toml"),
            format!("name = \"prod\"\nunits = [{refs_toml}]\n"),
        )
        .unwrap();

        for (name, deps) in units {
            let dir = root.join(format!("apps/{name}"));
            std::fs::create_dir_all(&dir).unwrap();
            let deps_toml: String = if deps.is_empty() {
                String::new()
            } else {
                let list = deps
                    .iter()
                    .map(|d| format!(r#""{d}""#))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("depends_on = [{list}]\n")
            };
            std::fs::write(
                dir.join("unit.toml"),
                format!("name = \"{name}\"\n{deps_toml}"),
            )
            .unwrap();
        }

        tmp
    }

    #[test]
    fn linear_chain_produces_single_unit_per_level() {
        // api ← web ← worker
        let tmp = workspace_with_deps(&[("api", &[]), ("web", &["api"]), ("worker", &["web"])]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let graph = build(&ws, "prod").unwrap();

        assert_eq!(graph.levels.len(), 3);
        assert_eq!(graph.levels[0], vec!["api"]);
        assert_eq!(graph.levels[1], vec!["web"]);
        assert_eq!(graph.levels[2], vec!["worker"]);
    }

    #[test]
    fn independent_units_collapse_into_one_level() {
        let tmp = workspace_with_deps(&[("api", &[]), ("web", &[]), ("worker", &[])]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let graph = build(&ws, "prod").unwrap();

        // All independent → one level containing all three (sorted).
        assert_eq!(graph.levels.len(), 1);
        assert_eq!(graph.levels[0], vec!["api", "web", "worker"]);
    }

    #[test]
    fn diamond_groups_correctly() {
        //     api
        //    /   \
        //  web   cron
        //    \   /
        //     ui
        let tmp = workspace_with_deps(&[
            ("api", &[]),
            ("web", &["api"]),
            ("cron", &["api"]),
            ("ui", &["web", "cron"]),
        ]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let graph = build(&ws, "prod").unwrap();

        assert_eq!(graph.levels.len(), 3);
        assert_eq!(graph.levels[0], vec!["api"]);
        assert_eq!(graph.levels[1], vec!["cron", "web"]);
        assert_eq!(graph.levels[2], vec!["ui"]);
    }

    #[test]
    fn two_cycle_is_rejected() {
        let tmp = workspace_with_deps(&[("a", &["b"]), ("b", &["a"])]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let err = build(&ws, "prod").unwrap_err();
        match err {
            GraphError::Cycle { monad, cycle } => {
                assert_eq!(monad, "prod");
                assert_eq!(cycle.len(), 2);
                assert!(cycle.contains(&"a".to_string()));
                assert!(cycle.contains(&"b".to_string()));
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn self_loop_is_rejected_as_cycle() {
        let tmp = workspace_with_deps(&[("a", &["a"])]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let err = build(&ws, "prod").unwrap_err();
        assert!(matches!(err, GraphError::Cycle { .. }));
    }

    #[test]
    fn unknown_dep_reports_friendly_error() {
        let tmp = workspace_with_deps(&[("a", &["ghost"])]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let err = build(&ws, "prod").unwrap_err();
        match err {
            GraphError::UnknownDep {
                monad,
                unit,
                missing,
            } => {
                assert_eq!(monad, "prod");
                assert_eq!(unit, "a");
                assert_eq!(missing, "ghost");
            }
            other => panic!("expected UnknownDep, got {other:?}"),
        }
    }

    #[test]
    fn unit_count_matches_total_units() {
        let tmp = workspace_with_deps(&[("api", &[]), ("web", &["api"]), ("cron", &["api"])]);
        let ws = Workspace::load(tmp.path()).unwrap();
        let graph = build(&ws, "prod").unwrap();
        assert_eq!(graph.unit_count(), 3);
    }

    #[test]
    fn dep_that_exists_in_workspace_but_not_this_monad_is_rejected() {
        // Two profiles, "prod" (api only) and "staging" (api + web). web's
        // depends_on = ["api"] is valid in staging but would be invalid
        // in a monad that only contains web.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("profiles")).unwrap();
        std::fs::write(
            root.join("profiles/prod.toml"),
            r#"name = "prod"
units = ["apps/web"]"#,
        )
        .unwrap();
        std::fs::write(
            root.join("profiles/staging.toml"),
            r#"name = "staging"
units = ["apps/api", "apps/web"]"#,
        )
        .unwrap();
        for name in ["api", "web"] {
            let dir = root.join(format!("apps/{name}"));
            std::fs::create_dir_all(&dir).unwrap();
        }
        std::fs::write(root.join("apps/api/unit.toml"), r#"name = "api""#).unwrap();
        std::fs::write(
            root.join("apps/web/unit.toml"),
            r#"name = "web"
depends_on = ["api"]
"#,
        )
        .unwrap();

        let ws = Workspace::load(root).unwrap();
        // staging resolves fine — both units are present.
        build(&ws, "staging").unwrap();
        // prod must reject — 'web' depends on 'api', but prod doesn't
        // include api.
        let err = build(&ws, "prod").unwrap_err();
        assert!(matches!(err, GraphError::UnknownDep { .. }));

        // (quiet the unused-import lint if the test doesn't touch Path)
        let _ = Path::new("");
    }
}
