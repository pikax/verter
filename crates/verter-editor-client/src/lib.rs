//! Shared, pure launch-contract logic for Verter's editor clients.
//!
//! Verter ships a native stdio LSP server, `verter-lsp`. Thin editor clients
//! (Lapce on `wasm32-wasip1`, Zed on `wasm32-wasip2`) all need to perform the
//! same host-API-free jobs to launch it:
//!
//! * [`args::build_server_args`] — build the `verter-lsp` launch argv (with the
//!   load-bearing `--type-provider=tsgo` default clamp).
//! * [`init_options::build_initialization_options`] — build the LSP
//!   `initializationOptions` JSON (the server-read parity set).
//! * [`platform`] — map a neutral `(Os, Arch)` to a Rust target triple, the
//!   binary file name, and the release asset name.
//! * [`discovery::resolve_server`] — decide which binary source to launch
//!   (override → managed → opted-in PATH → loud fail).
//!
//! This crate is the SINGLE SOURCE OF TRUTH for that launch contract so the
//! Lapce and Zed clients cannot diverge. It is intentionally pure: std +
//! serde_json only, no host/wasm/network dependencies, and no IO. The host
//! gathers raw signals (platform, filesystem presence, PATH hits) and feeds them
//! in as plain data; this crate only decides and constructs.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod args;
pub mod discovery;
pub mod init_options;
pub mod platform;

pub use args::{build_server_args, clamp_type_provider, DEFAULT_TYPE_PROVIDER};
pub use discovery::{resolve_server, DiscoveryError, DiscoveryInputs, ServerSource};
pub use init_options::build_initialization_options;
pub use platform::{
    asset_name, binary_file_name, from_host, target_triple, Arch, Os, TargetTriple,
    UnsupportedPlatform,
};
