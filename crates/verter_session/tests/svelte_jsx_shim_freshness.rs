//! Byte-pin freshness guard for the embedded `@verter/svelte-jsx` shim.
//!
//! The real workspace package `packages/svelte-jsx/` is the SINGLE
//! hand-written content authority for the Svelte JSX namespace shim (D-av).
//! `verter_session` carries IN-CRATE MIRROR files
//! (`crates/verter_session/src/framework/svelte_jsx_assets/`) and embeds
//! THOSE via crate-relative `include_str!` (a cross-tree `include_str!` of
//! the package files is FORBIDDEN — it would make this byte-compare vacuous
//! and break crates.io packaging).
//!
//! This pin byte-compares each in-crate mirror against its
//! `packages/svelte-jsx/` canonical. A drift between the authority package
//! and the embedded copy — an edit to one without the other — fails the
//! gate. Mirrors the `typeinfo_proto_ts_freshness` discipline (regenerate +
//! byte-compare), except the authority is the hand-written package file.
//!
//! Regenerate (after an intentional package edit): run with
//! `VERTER_UPDATE_SVELTE_JSX_SHIM=1` set, which copies the canonical package
//! files over the in-crate mirrors, then re-run to confirm green and commit.

use std::path::PathBuf;

use verter_session::framework::svelte_jsx_assets::{
    SVELTE_JSX_DEV_RUNTIME_DTS, SVELTE_JSX_MATHML_DEV_RUNTIME_DTS, SVELTE_JSX_MATHML_RUNTIME_DTS,
    SVELTE_JSX_RUNTIME_DTS, SVELTE_JSX_SVG_DEV_RUNTIME_DTS, SVELTE_JSX_SVG_RUNTIME_DTS,
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

/// The in-crate mirror dir whose bytes the embedded constants `include_str!`.
fn mirror_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/framework/svelte_jsx_assets")
}

/// The canonical package dir — the single hand-written authority.
fn package_dir() -> PathBuf {
    workspace_root().join("packages/svelte-jsx")
}

struct ShimAsset {
    /// The file name shared by the package and the mirror.
    file_name: &'static str,
    /// The embedded constant (`include_str!`'d from the mirror).
    embedded: &'static str,
}

fn assets() -> [ShimAsset; 6] {
    [
        ShimAsset {
            file_name: "jsx-runtime.d.ts",
            embedded: SVELTE_JSX_RUNTIME_DTS,
        },
        ShimAsset {
            file_name: "jsx-dev-runtime.d.ts",
            embedded: SVELTE_JSX_DEV_RUNTIME_DTS,
        },
        // The F10 svg-namespace entrypoint (`@verter/svelte-jsx/svg/jsx-runtime`).
        ShimAsset {
            file_name: "svg/jsx-runtime.d.ts",
            embedded: SVELTE_JSX_SVG_RUNTIME_DTS,
        },
        ShimAsset {
            file_name: "svg/jsx-dev-runtime.d.ts",
            embedded: SVELTE_JSX_SVG_DEV_RUNTIME_DTS,
        },
        // The F10 mathml-namespace entrypoint.
        ShimAsset {
            file_name: "mathml/jsx-runtime.d.ts",
            embedded: SVELTE_JSX_MATHML_RUNTIME_DTS,
        },
        ShimAsset {
            file_name: "mathml/jsx-dev-runtime.d.ts",
            embedded: SVELTE_JSX_MATHML_DEV_RUNTIME_DTS,
        },
    ]
}

#[test]
fn embedded_mirror_is_byte_equal_to_the_canonical_package_file() {
    for asset in assets() {
        let package_path = package_dir().join(asset.file_name);
        let mirror_path = mirror_dir().join(asset.file_name);

        let canonical = std::fs::read_to_string(&package_path).unwrap_or_else(|err| {
            panic!(
                "cannot read the canonical svelte-jsx package file {}: {err}",
                package_path.display()
            )
        });

        // Update path: copy the canonical package file over the in-crate
        // mirror and short-circuit.
        if std::env::var("VERTER_UPDATE_SVELTE_JSX_SHIM").is_ok() {
            std::fs::write(&mirror_path, &canonical).unwrap_or_else(|err| {
                panic!("cannot write mirror {}: {err}", mirror_path.display())
            });
            eprintln!("wrote regenerated mirror {}", mirror_path.display());
            continue;
        }

        // The embedded constant (compiled from the mirror) must equal the
        // canonical package authority byte-for-byte.
        assert_eq!(
            asset.embedded,
            canonical,
            "embedded svelte-jsx shim `{}` drifted from its canonical package \
             authority `{}`. The package file is the single hand-written \
             authority; regenerate the in-crate mirror with \
             `VERTER_UPDATE_SVELTE_JSX_SHIM=1` and commit.",
            asset.file_name,
            package_path.display()
        );

        // And the on-disk mirror must equal the embedded constant — proves
        // the embedded bytes are exactly what `include_str!` read (no stale
        // compiled copy).
        let mirror = std::fs::read_to_string(&mirror_path)
            .unwrap_or_else(|err| panic!("cannot read mirror {}: {err}", mirror_path.display()));
        assert_eq!(
            asset.embedded, mirror,
            "in-crate mirror `{}` drifted from the embedded constant",
            asset.file_name
        );
    }
}

#[test]
fn the_canonical_package_does_not_stub_the_svelte_package() {
    // D-ae(d): the shim never stubs `svelte` itself — it imports the real
    // package. A workspace without `svelte` must fail CLOSED, not be rescued
    // by an ambient stub. So the package file imports `svelte` and never
    // `declare module "svelte"`.
    let runtime = std::fs::read_to_string(package_dir().join("jsx-runtime.d.ts"))
        .expect("read canonical jsx-runtime.d.ts");
    assert!(
        runtime.contains("from \"svelte\""),
        "the shim imports the real svelte package"
    );
    assert!(
        !runtime.contains("declare module \"svelte\""),
        "the shim must NOT stub the svelte package (fail-closed contract)"
    );
}
