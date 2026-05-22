//! Configuration for monad: `monad.toml`, `profiles/*.toml`, `unit.toml`.
//!
//! Exposes strongly-typed schemas and a [`Workspace`] that walks a repo,
//! parses every config file, and returns a validated in-memory model.

mod error;
mod schema;
mod workspace;

pub use error::ConfigError;
pub use schema::{
    ProfileConfig, CacheConfig, ContainerMode, Defaults, UnitConfig, Environment, ExecutionConfig,
    GarnishSpec, GhaCache, PluginsConfig, RepoConfig, ServeConfig, Task, TelemetryConfig,
    ToolchainPin,
};
pub use workspace::{LoadedProfile, LoadedUnit, Workspace};
