//! Writes the published TypeScript declaration of the host compile
//! request from the Rust declarations that decode it.
//!
//! Generation is a COMMAND, never a test: the freshness guard renders in
//! memory and byte-compares, so no check can make itself pass by writing
//! the file it is checking.
//!
//! The command is `pnpm gen:host-request-ts`, which is
//!
//! ```text
//! cargo run -p verter_napi --features generate-host-request-ts \
//!     --bin generate_host_compile_request_ts
//! ```
//!
//! `required-features` keeps this target out of the default build: the
//! native publish lane cross-compiles this package for seven targets with
//! no target filter, and a generator is not something to link there.

use std::path::PathBuf;
use std::process::ExitCode;

use verter_napi::host_compile_request_ts::{
    render_host_compile_request_ts, HOST_COMPILE_REQUEST_TS_PATH,
};

fn main() -> ExitCode {
    let rendered = render_host_compile_request_ts();
    let path = workspace_root().join(HOST_COMPILE_REQUEST_TS_PATH);

    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!("could not create {}: {error}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    if let Err(error) = std::fs::write(&path, &rendered) {
        eprintln!("could not write {}: {error}", path.display());
        return ExitCode::FAILURE;
    }

    println!("wrote {} ({} bytes)", path.display(), rendered.len());
    ExitCode::SUCCESS
}

/// The workspace root, resolved from this crate's manifest directory
/// (`<workspace>/crates/verter_napi`), so the command writes the committed
/// path whatever directory it is invoked from.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR is `<workspace>/crates/verter_napi`")
        .to_path_buf()
}
