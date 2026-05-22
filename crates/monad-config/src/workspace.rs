//! Workspace discovery: walk a repo, parse every config, return a validated
//! in-memory model.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::schema::{parse_profile, parse_unit, parse_repo, ProfileConfig, UnitConfig, RepoConfig};

/// A loaded monad with its source path.
#[derive(Debug, Clone)]
pub struct LoadedProfile {
    pub config: ProfileConfig,
    pub source: PathBuf,
}

/// A loaded unit with its source directory (parent of `unit.toml`).
#[derive(Debug, Clone)]
pub struct LoadedUnit {
    pub config: UnitConfig,
    /// Directory containing `unit.toml`.
    pub dir: PathBuf,
    /// Relative path from the workspace root (stable key across machines).
    pub rel: PathBuf,
}

/// A fully loaded workspace — repo config, all profiles, all units, and
/// validated cross-references.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub repo: RepoConfig,
    /// Profiles keyed by name.
    pub profiles: BTreeMap<String, LoadedProfile>,
    /// Unites keyed by their relative path from `root` (e.g. `"apps/api"`).
    pub unites_by_path: BTreeMap<PathBuf, LoadedUnit>,
    /// Unites keyed by `UnitConfig.name`.
    pub unites_by_name: BTreeMap<String, LoadedUnit>,
}

impl Workspace {
    /// Discover and load a workspace rooted at `root`.
    ///
    /// - Reads `<root>/monad.toml` if present (otherwise uses defaults).
    /// - Loads every `<root>/profiles/*.toml`.
    /// - Loads each unit referenced by a monad's `units` list.
    /// - Validates: unique monad names, unique unit names, every referenced
    ///   unit path has a `unit.toml`.
    pub fn load(root: &Path) -> Result<Self, ConfigError> {
        let repo = load_repo_config(root)?;
        let profiles = load_profiles(root)?;
        let UnitIndex { by_path, by_name } = load_unites(root, &profiles)?;

        Ok(Workspace {
            root: root.to_path_buf(),
            repo,
            profiles,
            unites_by_path: by_path,
            unites_by_name: by_name,
        })
    }
}

fn load_repo_config(root: &Path) -> Result<RepoConfig, ConfigError> {
    let path = root.join("monad.toml");
    if path.exists() {
        parse_repo(&path)
    } else {
        Ok(RepoConfig::default())
    }
}

fn load_profiles(root: &Path) -> Result<BTreeMap<String, LoadedProfile>, ConfigError> {
    let mut out = BTreeMap::new();
    let dir = root.join("profiles");
    if !dir.exists() {
        return Ok(out);
    }

    let entries = std::fs::read_dir(&dir).map_err(|e| ConfigError::Read {
        path: dir.clone(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| ConfigError::Read {
            path: dir.clone(),
            source: e,
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }

        let config = parse_profile(&path)?;
        let loaded = LoadedProfile {
            config: config.clone(),
            source: path.clone(),
        };
        if let Some(prev) = out.insert(config.name.clone(), loaded) {
            return Err(ConfigError::Duplicate {
                kind: "monad",
                name: config.name,
                path_a: prev.source,
                path_b: path,
            });
        }
    }
    Ok(out)
}

struct UnitIndex {
    by_path: BTreeMap<PathBuf, LoadedUnit>,
    by_name: BTreeMap<String, LoadedUnit>,
}

fn load_unites(
    root: &Path,
    profiles: &BTreeMap<String, LoadedProfile>,
) -> Result<UnitIndex, ConfigError> {
    let mut by_path: BTreeMap<PathBuf, LoadedUnit> = BTreeMap::new();
    let mut by_name: BTreeMap<String, LoadedUnit> = BTreeMap::new();

    for monad in profiles.values() {
        for unit_ref in &monad.config.units {
            let rel = PathBuf::from(unit_ref);
            if by_path.contains_key(&rel) {
                continue; // same unit shared across profiles — load once
            }

            let dir = root.join(&rel);
            let toml_path = dir.join("unit.toml");
            if !toml_path.exists() {
                return Err(ConfigError::DanglingUnitRef {
                    monad: monad.config.name.clone(),
                    unit_path: rel.clone(),
                });
            }

            let config = parse_unit(&toml_path)?;
            let loaded = LoadedUnit {
                config: config.clone(),
                dir,
                rel: rel.clone(),
            };

            if let Some(prev) = by_name.insert(config.name.clone(), loaded.clone()) {
                return Err(ConfigError::Duplicate {
                    kind: "unit",
                    name: config.name,
                    path_a: prev.dir.join("unit.toml"),
                    path_b: toml_path,
                });
            }
            by_path.insert(rel, loaded);
        }
    }

    Ok(UnitIndex { by_path, by_name })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small two-unit sample workspace in a tempdir and return it.
    fn two_unit_fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("monad.toml"),
            r#"
            [defaults]
            parallelism = 4
            [cache]
            local = true
            gha = "auto"
            "#,
        )
        .unwrap();

        std::fs::create_dir(root.join("profiles")).unwrap();
        std::fs::write(
            root.join("profiles/prod.toml"),
            r#"
            name = "prod"
            units = ["apps/api", "apps/web"]
            "#,
        )
        .unwrap();

        let api = root.join("apps/api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(
            api.join("unit.toml"),
            r#"
            name = "sample-api"
            language = "go"

            [tasks.build]
            run = "go build -o bin/api ./cmd/api"
            "#,
        )
        .unwrap();

        let web = root.join("apps/web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(
            web.join("unit.toml"),
            r#"
            name = "sample-web"
            language = "node"
            package_manager = "npm"
            depends_on = ["sample-api"]

            [tasks.build]
            run = "npm run build"
            "#,
        )
        .unwrap();

        tmp
    }

    #[test]
    fn loads_two_unit_workspace() {
        let tmp = two_unit_fixture();
        let ws = Workspace::load(tmp.path()).unwrap();

        assert_eq!(ws.repo.defaults.parallelism, 4);
        assert_eq!(ws.profiles.len(), 1);
        assert_eq!(ws.profiles["prod"].config.units.len(), 2);
        assert_eq!(ws.unites_by_path.len(), 2);
        assert_eq!(ws.unites_by_name.len(), 2);

        let api = &ws.unites_by_name["sample-api"];
        assert_eq!(api.config.language.as_deref(), Some("go"));
        assert_eq!(api.rel, PathBuf::from("apps/api"));

        let web = &ws.unites_by_name["sample-web"];
        assert_eq!(web.config.depends_on, vec!["sample-api"]);
    }

    #[test]
    fn workspace_without_monad_toml_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("profiles")).unwrap();
        std::fs::write(
            tmp.path().join("profiles/empty.toml"),
            r#"
            name = "empty"
            units = ["apps/only"]
            "#,
        )
        .unwrap();
        let only = tmp.path().join("apps/only");
        std::fs::create_dir_all(&only).unwrap();
        std::fs::write(only.join("unit.toml"), r#"name = "only""#).unwrap();

        let ws = Workspace::load(tmp.path()).unwrap();
        assert_eq!(
            ws.repo.defaults.parallelism,
            RepoConfig::default().defaults.parallelism
        );
        assert!(ws.repo.cache.local);
    }

    #[test]
    fn dangling_unit_reference_is_caught() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("profiles")).unwrap();
        std::fs::write(
            tmp.path().join("profiles/prod.toml"),
            r#"
            name = "prod"
            units = ["apps/nowhere"]
            "#,
        )
        .unwrap();

        let err = Workspace::load(tmp.path()).unwrap_err();
        match err {
            ConfigError::DanglingUnitRef { monad, unit_path } => {
                assert_eq!(monad, "prod");
                assert_eq!(unit_path, PathBuf::from("apps/nowhere"));
            }
            other => panic!("expected DanglingUnitRef, got: {other:?}"),
        }
    }

    #[test]
    fn duplicate_monad_name_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("profiles")).unwrap();
        std::fs::write(
            tmp.path().join("profiles/a.toml"),
            r#"name = "prod"
units = ["apps/api"]"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("profiles/b.toml"),
            r#"name = "prod"
units = ["apps/api"]"#,
        )
        .unwrap();

        let api = tmp.path().join("apps/api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(api.join("unit.toml"), r#"name = "api""#).unwrap();

        let err = Workspace::load(tmp.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Duplicate { kind: "monad", .. }));
    }

    #[test]
    fn duplicate_unit_name_across_profiles_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("profiles")).unwrap();
        std::fs::write(
            tmp.path().join("profiles/prod.toml"),
            r#"name = "prod"
units = ["apps/a", "apps/b"]"#,
        )
        .unwrap();

        for (subdir, _) in &[("apps/a", ()), ("apps/b", ())] {
            let d = tmp.path().join(subdir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("unit.toml"), r#"name = "samename""#).unwrap();
        }

        let err = Workspace::load(tmp.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Duplicate { kind: "unit", .. }));
    }

    #[test]
    fn shared_unit_across_profiles_loads_once() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("profiles")).unwrap();
        std::fs::write(
            tmp.path().join("profiles/staging.toml"),
            r#"name = "staging"
units = ["apps/api"]"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("profiles/prod.toml"),
            r#"name = "prod"
units = ["apps/api"]"#,
        )
        .unwrap();

        let api = tmp.path().join("apps/api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(api.join("unit.toml"), r#"name = "api""#).unwrap();

        let ws = Workspace::load(tmp.path()).unwrap();
        assert_eq!(ws.profiles.len(), 2);
        assert_eq!(ws.unites_by_name.len(), 1);
    }
}
