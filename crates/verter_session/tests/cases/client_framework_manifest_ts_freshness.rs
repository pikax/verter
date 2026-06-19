//! Freshness guard for the generated client framework manifest TS module.
//!
//! The Rust framework-adapter registry (`built_in_descriptors()` joined with
//! the `verter_language` extension table) is the SINGLE authority for the VS
//! Code extension + TypeScript-plugin CLIENT wiring: which frameworks the
//! client activates for, their carrier / adapter-module extensions, the client
//! language ids, the `_typescript.configurePlugin` trigger ids, and the
//! virtual-file naming suffixes. The committed TypeScript module
//! `packages/language-shared/src/client-framework-manifest.generated.ts` is a
//! GENERATED, BYTE-PINNED mirror of that authority. (File-watching is a SERVER
//! concern — the LSP owns the `workspace/didChangeWatchedFiles` watcher from the
//! same descriptor authority — so the client manifest carries no watch globs.)
//!
//! This pin renders the canonical TS module from the descriptor + registry
//! (`render_client_framework_manifest_ts`) and byte-compares it against the
//! committed file. A hand-edit to the generated file, or a descriptor / registry
//! change without a regen, fails this gate. Mirrors the
//! `virtual_file_naming_ts_freshness` discipline (regenerate + byte-compare).
//!
//! Regenerate (after an intentional descriptor / registry change): run this test
//! with `VERTER_UPDATE_CLIENT_FRAMEWORK_MANIFEST_TS=1` set, which writes the
//! rendered module to the committed path, then re-run to confirm green and
//! commit the regenerated file.

use std::path::PathBuf;

use verter_session::framework::client_framework_manifest_ts::{
    render_client_framework_manifest_ts, CLIENT_FRAMEWORK_MANIFEST_TS_PATH,
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
fn client_framework_manifest_ts_is_byte_equal_to_the_rendered_registry() {
    let rendered = render_client_framework_manifest_ts();
    let committed_path = workspace_root().join(CLIENT_FRAMEWORK_MANIFEST_TS_PATH);

    // Update path: write the freshly-rendered module and short-circuit.
    if std::env::var("VERTER_UPDATE_CLIENT_FRAMEWORK_MANIFEST_TS").is_ok() {
        if let Some(parent) = committed_path.parent() {
            std::fs::create_dir_all(parent).expect("create generated dir");
        }
        std::fs::write(&committed_path, &rendered)
            .expect("write generated client-framework-manifest.ts");
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
             Run this test with `VERTER_UPDATE_CLIENT_FRAMEWORK_MANIFEST_TS=1` to generate it.",
            committed_path.display()
        )
    });

    assert_eq!(
        committed, rendered,
        "`{}` is out of sync with the Rust framework-adapter registry. \
         The descriptor + registry tables are the single authority — re-run this test with \
         `VERTER_UPDATE_CLIENT_FRAMEWORK_MANIFEST_TS=1` to regenerate, then commit the file.",
        CLIENT_FRAMEWORK_MANIFEST_TS_PATH,
    );
}

/// The extension's framework wiring (activation / document selector / trigger
/// ids / start decision) must DERIVE from the generated manifest — NOT a
/// per-framework client fork. This scans the VS Code extension source and pins
/// (a) the wiring module reads the manifest, (b) the retired `verter.frameworks`
/// opt-in gate is gone (Svelte is no longer opt-in), (c) the extension consumes
/// the manifest-driven wiring helpers, and (d) the manifest itself is wired into
/// the extension's package.json (the `svelte` language is registered, both
/// `onLanguage:vue` and `onLanguage:svelte` activate). Discriminating: it FAILS
/// against the pre-S2a tree (opt-in gate present, no manifest helpers, no svelte
/// language row).
#[test]
fn client_framework_manifest_drives_extension_wiring() {
    let root = workspace_root();
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel)).unwrap_or_else(|err| panic!("read {rel}: {err}"))
    };

    let wiring = read("packages/vue-vscode/src/frameworkWiring.ts");
    // The wiring module is manifest-driven (imports the generated manifest's
    // derived lists for the document selector + carrier-language set).
    assert!(
        wiring.contains("@verter/language-shared")
            && wiring.contains("CLIENT_DOCUMENT_SELECTOR_LANGUAGE_IDS")
            && wiring.contains("CLIENT_FRAMEWORK_LANGUAGE_IDS"),
        "frameworkWiring.ts must derive from the generated client framework manifest"
    );

    let extension = read("packages/vue-vscode/src/extension.ts");
    // The extension consumes the manifest-driven wiring helpers.
    assert!(
        extension.contains("frameworkDocumentSelector")
            && extension.contains("isFrameworkCarrierLanguageId"),
        "extension.ts must build its document selector + start decision from the manifest"
    );
    // The retired opt-in fork is gone — Svelte is no longer gated behind
    // `verter.frameworks` and the documentSelector is not a hardcoded list.
    assert!(
        !extension.contains("verter.frameworks") && !extension.contains("optInFrameworks"),
        "the `verter.frameworks` opt-in gate must be removed (Svelte is first-class)"
    );
    // The hardcoded per-language document-selector list is gone.
    assert!(
        !extension.contains("{ scheme: \"file\", language: \"vue\" },"),
        "the hardcoded Vue documentSelector entry must be manifest-derived"
    );

    let package_json = read("packages/vue-vscode/package.json");
    // The manifest's frameworks are wired into the extension manifest.
    assert!(
        package_json.contains("\"onLanguage:vue\"")
            && package_json.contains("\"onLanguage:svelte\""),
        "package.json must activate for both vue and svelte"
    );
    assert!(
        package_json.contains("\"id\": \"svelte\""),
        "package.json must register the svelte language minimally"
    );
    // No grammar shipped for svelte (relies on the user's Svelte extension).
    assert!(
        !package_json.contains("source.svelte"),
        "Verter must not ship a Svelte TextMate grammar"
    );
    // The retired opt-in config is gone.
    assert!(
        !package_json.contains("verter.frameworks"),
        "the retired verter.frameworks opt-in config must be removed"
    );
}
