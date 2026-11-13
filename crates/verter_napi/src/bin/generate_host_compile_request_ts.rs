//! Writes the published TypeScript declarations of the host compile
//! request and response from the Rust declarations the bindings decode and
//! serialise.
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
//! With `-- --stdout` it prints each render, under a line naming its
//! committed path, instead of writing the files — for a build host whose
//! working tree the caller cannot read back.
//!
//! `required-features` keeps this target out of the default build: the
//! native publish lane cross-compiles this package for seven targets with
//! no target filter, and a generator is not something to link there.

use std::path::PathBuf;
use std::process::ExitCode;

use verter_napi::host_compile_request_ts::generated_declarations;

fn main() -> ExitCode {
    let to_stdout = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--stdout" => true,
        other => {
            eprintln!("unexpected arguments {other:?}; the only option is `--stdout`");
            return ExitCode::FAILURE;
        }
    };
    let root = workspace_root();
    for (relative, rendered) in generated_declarations() {
        if to_stdout {
            print!("==> {relative} <==\n{rendered}");
            continue;
        }
        let path = root.join(relative);
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
    }
    ExitCode::SUCCESS
}

/// The workspace root, resolved from this crate's manifest directory
/// (`<workspace>/crates/verter_napi`), so the command writes the committed
/// paths whatever directory it is invoked from.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR is `<workspace>/crates/verter_napi`")
        .to_path_buf()
}
