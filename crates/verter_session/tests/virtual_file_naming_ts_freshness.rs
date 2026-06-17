//! Freshness guard for the generated virtual-file naming TS mirror.
//!
//! The Rust framework-adapter descriptor table (the `VirtualFileNaming`
//! column in `crates/verter_session/src/framework/descriptor.rs`) is the
//! SINGLE authority for an adapter's IDE / API / testing-API / sidecar
//! virtual-file suffixes. The committed TypeScript module
//! `packages/typescript-plugin/src/generated/virtual-file-naming.ts` is a
//! GENERATED, BYTE-PINNED mirror of that column.
//!
//! This pin renders the canonical TS module from the descriptor rows
//! (`render_virtual_file_naming_ts`) and byte-compares it against the
//! committed file. A hand-edit to the generated file, or a descriptor-row
//! change without a regen, fails this gate. Mirrors the
//! `typeinfo_proto_ts_freshness` discipline (regenerate + byte-compare),
//! except the authority is the Rust descriptor table rather than the
//! proto schema.
//!
//! Regenerate (after an intentional descriptor change): run this test
//! with `VERTER_UPDATE_VIRTUAL_FILE_NAMING_TS=1` set, which writes the
//! rendered module to the committed path, then re-run to confirm green
//! and commit the regenerated file.

use std::path::PathBuf;

use verter_session::framework::virtual_file_naming_ts::{
    render_virtual_file_naming_ts, VIRTUAL_FILE_NAMING_TS_PATH,
};

/// Resolve the workspace root from this crate's manifest dir
/// (`<workspace>/crates/verter_session`).
fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

#[test]
fn virtual_file_naming_ts_is_byte_equal_to_the_rendered_descriptor_column() {
    let rendered = render_virtual_file_naming_ts();
    let committed_path = workspace_root().join(VIRTUAL_FILE_NAMING_TS_PATH);

    // Update path: write the freshly-rendered module and short-circuit.
    if std::env::var("VERTER_UPDATE_VIRTUAL_FILE_NAMING_TS").is_ok() {
        if let Some(parent) = committed_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated dir");
        }
        std::fs::write(&committed_path, &rendered).expect("write generated virtual-file-naming.ts");
        eprintln!(
            "wrote regenerated {} ({} bytes)",
            committed_path.display(),
            rendered.len()
        );
        return;
    }

    let committed = std::fs::read_to_string(&committed_path).unwrap_or_else(|err| {
        panic!(
            "freshness check should be able to read `{}`: {err}.\n\
             Run this test with `VERTER_UPDATE_VIRTUAL_FILE_NAMING_TS=1` to generate it.",
            committed_path.display()
        )
    });

    assert_eq!(
        committed, rendered,
        "`{}` is out of sync with the Rust framework-adapter virtual-file naming column. \
         The descriptor table is the single authority — re-run this test with \
         `VERTER_UPDATE_VIRTUAL_FILE_NAMING_TS=1` to regenerate, then commit the file.",
        VIRTUAL_FILE_NAMING_TS_PATH,
    );
}
