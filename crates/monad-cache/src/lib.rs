//! Content-addressable store and local cache for monad.
//!
//! - [`Hasher`] — streaming blake3 hasher over file contents and named extras.
//! - [`CacheKey`] — hex-encoded 256-bit hash that identifies a build result.
//! - [`LocalCache`] — put / get / clear / stats over `~/.monad/cache/`.
//! - [`RemoteCache`] — trait for remote-tier backends; implemented by
//!   [`S3Remote`] (S3-compat, any bucket) and [`BearerRemote`] (monad://
//!   JWT-auth, hosted cache at `cache.monad.build` or any compatible server).
//!
//! Bundle format (v1): a plain tar archive containing
//!   `meta.json`  — `{ "exit_code": i32, "version": 1 }`
//!   `stdout`     — raw stdout bytes
//!   `stderr`     — raw stderr bytes
//!   `outputs/…`  — each output file at its relative-to-`unit_dir` path
//!
//! Compression (zstd) lands with the remote cache work in P3.

mod key;
mod local;
mod manifest;
mod remote;
mod remote_api;
mod remote_bearer;
pub mod token;

pub use key::{CacheKey, Hasher};
pub use local::{CacheStats, LocalCache, TaskResult};
pub use manifest::{InputManifest, ManifestFile};
pub use remote::S3Remote;
pub use remote_api::{build_remote, RemoteCache, RemoteScheme};
pub use remote_bearer::BearerRemote;
