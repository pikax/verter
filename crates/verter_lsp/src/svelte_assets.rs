//! Host-side materialization + path-mapping for the `@verter/svelte-jsx`
//! shim and its transitive `svelte` dependency (D-av / D-ay).
//!
//! The Svelte IDE projection opens each `.svelte.tsx` with the per-file pragma
//! `/** @jsxImportSource @verter/svelte-jsx */`. TSGO and the inferred project
//! read REAL files (virtual content cannot serve them), so the host
//! MATERIALIZES the Verter-owned shim once per host version into its OWN data
//! directory (NEVER the user workspace) and path-maps the inferred project at
//! it through `configure_paths`. The host-selected copy is authoritative and
//! version-matched to the projection the compiler emits.
//!
//! The shim's own imports (`svelte`, `svelte/elements`, `svelte/attachments`)
//! cannot resolve from the host data directory (the node_modules ancestor walk
//! never reaches the user workspace's `svelte`; `baseUrl` does not rescue
//! node-style specifiers under `moduleResolution: "bundler"`), so the SAME
//! injection adds per-owner-project `paths` rows mapping `svelte` + `svelte/*`
//! to the OWNER WORKSPACE's installed `svelte` package. A workspace with NO
//! `svelte` install gets NO `svelte` rows and fails CLOSED (module-not-found +
//! the typed `svelte-package-missing` diagnostic, D-ae(d)).

use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use verter_session::framework::svelte_jsx_assets::{
    SVELTE_JSX_DEV_RUNTIME_DTS, SVELTE_JSX_DEV_RUNTIME_SPECIFIER,
    SVELTE_JSX_MATHML_DEV_RUNTIME_DTS, SVELTE_JSX_MATHML_DEV_RUNTIME_SPECIFIER,
    SVELTE_JSX_MATHML_RUNTIME_DTS, SVELTE_JSX_MATHML_RUNTIME_SPECIFIER, SVELTE_JSX_RUNTIME_DTS,
    SVELTE_JSX_RUNTIME_SPECIFIER, SVELTE_JSX_SVG_DEV_RUNTIME_DTS,
    SVELTE_JSX_SVG_DEV_RUNTIME_SPECIFIER, SVELTE_JSX_SVG_RUNTIME_DTS,
    SVELTE_JSX_SVG_RUNTIME_SPECIFIER,
};

/// The typed Verter diagnostic code for a missing `svelte` package install
/// (D-ae(d) / D-ay). Emitted on a `.svelte` source file whose owner workspace
/// has no `svelte` install — the shim's `import … from "svelte"` then fails
/// CLOSED (module-not-found), and this typed diagnostic explains WHY.
pub(crate) const SVELTE_PACKAGE_MISSING_CODE: &str = "svelte-package-missing";

/// The host data directory the shim materializes into — a per-host-version
/// subdirectory under the system temp dir (NOT the user workspace). The
/// version stamp keeps the copy matched to the projection the compiler emits.
fn host_shim_dir() -> PathBuf {
    std::env::temp_dir()
        .join("verter-host")
        .join(concat!("svelte-jsx-", env!("CARGO_PKG_VERSION")))
}

/// Materialize the embedded `@verter/svelte-jsx` shim into the host data
/// directory once per host version, returning the directory path.
///
/// Idempotent: re-writes only when the on-disk bytes differ. The directory is
/// the host's own — never the user workspace.
pub(crate) fn materialize_svelte_jsx_shim() -> std::io::Result<PathBuf> {
    let dir = host_shim_dir();
    std::fs::create_dir_all(&dir)?;
    write_if_changed(&dir.join("jsx-runtime.d.ts"), SVELTE_JSX_RUNTIME_DTS)?;
    write_if_changed(
        &dir.join("jsx-dev-runtime.d.ts"),
        SVELTE_JSX_DEV_RUNTIME_DTS,
    )?;
    // The F10 svg / mathml namespace entrypoints (selected by a top-level
    // `<svelte:options namespace="svg|mathml">` via the per-file pragma).
    std::fs::create_dir_all(dir.join("svg"))?;
    std::fs::create_dir_all(dir.join("mathml"))?;
    write_if_changed(
        &dir.join("svg/jsx-runtime.d.ts"),
        SVELTE_JSX_SVG_RUNTIME_DTS,
    )?;
    write_if_changed(
        &dir.join("svg/jsx-dev-runtime.d.ts"),
        SVELTE_JSX_SVG_DEV_RUNTIME_DTS,
    )?;
    write_if_changed(
        &dir.join("mathml/jsx-runtime.d.ts"),
        SVELTE_JSX_MATHML_RUNTIME_DTS,
    )?;
    write_if_changed(
        &dir.join("mathml/jsx-dev-runtime.d.ts"),
        SVELTE_JSX_MATHML_DEV_RUNTIME_DTS,
    )?;
    // A minimal package.json so node-style resolution of the subpaths works if
    // a consumer resolves the package directory rather than the path mapping.
    write_if_changed(
        &dir.join("package.json"),
        r#"{"name":"@verter/svelte-jsx","types":"jsx-runtime.d.ts","exports":{"./jsx-runtime":{"types":"./jsx-runtime.d.ts"},"./jsx-dev-runtime":{"types":"./jsx-dev-runtime.d.ts"},"./svg/jsx-runtime":{"types":"./svg/jsx-runtime.d.ts"},"./svg/jsx-dev-runtime":{"types":"./svg/jsx-dev-runtime.d.ts"},"./mathml/jsx-runtime":{"types":"./mathml/jsx-runtime.d.ts"},"./mathml/jsx-dev-runtime":{"types":"./mathml/jsx-dev-runtime.d.ts"}}}"#,
    )?;
    Ok(dir)
}

/// Write `content` to `path` only when it differs from what is already there.
fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == content {
            return Ok(());
        }
    }
    std::fs::write(path, content)
}

/// Resolve the owner workspace's installed `svelte` package directory, if any.
///
/// `workspace_root` is the owner project root (a filesystem path). Returns the
/// `<root>/node_modules/svelte` directory when it exists. A monorepo with
/// multiple `svelte` installs resolves each project against its own copy (the
/// caller passes the owner project root). When NO `svelte` is installed,
/// returns `None` — no rows are injected and the shim's imports fail closed.
pub(crate) fn resolve_owner_svelte(workspace_root: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(workspace_root).join("node_modules/svelte");
    if candidate.join("package.json").exists() {
        Some(candidate)
    } else {
        None
    }
}

/// Inject the svelte-jsx shim + transitive `svelte` rows into a `paths` JSON
/// object for `configure_paths` (D-av / D-ay).
///
/// * `@verter/svelte-jsx/jsx-runtime` + `jsx-dev-runtime` map to the
///   host-materialized shim copy;
/// * `svelte` + `svelte/*` map to the owner workspace's installed `svelte`
///   when present — ABSENT when it is not (fail-closed).
///
/// `paths` may be `Value::Null` (no existing mapping) — it is promoted to an
/// object. Existing rows are preserved.
pub(crate) fn inject_svelte_paths(
    mut paths: serde_json::Value,
    workspace_root: &str,
) -> serde_json::Value {
    let Ok(shim_dir) = materialize_svelte_jsx_shim() else {
        // Materialization failed — do not inject the shim rows (the projection
        // will surface module-not-found, the honest fail-closed signal).
        return paths;
    };
    let shim = shim_dir.to_string_lossy().replace('\\', "/");

    if !paths.is_object() {
        paths = serde_json::json!({});
    }
    let obj = paths.as_object_mut().expect("paths promoted to object");

    obj.insert(
        SVELTE_JSX_RUNTIME_SPECIFIER.to_string(),
        serde_json::json!([format!("{shim}/jsx-runtime.d.ts")]),
    );
    obj.insert(
        SVELTE_JSX_DEV_RUNTIME_SPECIFIER.to_string(),
        serde_json::json!([format!("{shim}/jsx-dev-runtime.d.ts")]),
    );
    // F10 svg / mathml namespace entrypoints.
    obj.insert(
        SVELTE_JSX_SVG_RUNTIME_SPECIFIER.to_string(),
        serde_json::json!([format!("{shim}/svg/jsx-runtime.d.ts")]),
    );
    obj.insert(
        SVELTE_JSX_SVG_DEV_RUNTIME_SPECIFIER.to_string(),
        serde_json::json!([format!("{shim}/svg/jsx-dev-runtime.d.ts")]),
    );
    obj.insert(
        SVELTE_JSX_MATHML_RUNTIME_SPECIFIER.to_string(),
        serde_json::json!([format!("{shim}/mathml/jsx-runtime.d.ts")]),
    );
    obj.insert(
        SVELTE_JSX_MATHML_DEV_RUNTIME_SPECIFIER.to_string(),
        serde_json::json!([format!("{shim}/mathml/jsx-dev-runtime.d.ts")]),
    );

    // Transitive `svelte` rows — only when the owner workspace installs it.
    if let Some(svelte_dir) = resolve_owner_svelte(workspace_root) {
        let svelte = svelte_dir.to_string_lossy().replace('\\', "/");
        obj.insert("svelte".to_string(), serde_json::json!([svelte.clone()]));
        obj.insert(
            "svelte/*".to_string(),
            serde_json::json!([format!("{svelte}/*")]),
        );
    }

    paths
}

/// Produce the typed `svelte-package-missing` diagnostic for a `.svelte` source
/// file (D-ae(d) / D-ay) when its owner workspace has NO `svelte` install.
///
/// Returns `Some(Diagnostic)` exactly when `resolve_owner_svelte(owner_root)`
/// is `None` — the same fail-closed condition that suppresses the `svelte`
/// `paths` rows. The diagnostic anchors at the file head (line 0) so it is
/// always placed even before the projection materialises. `None` is returned
/// for a non-`.svelte` file or when `svelte` IS installed (no false positive).
pub(crate) fn svelte_package_missing_diagnostic(
    canonical_id: &str,
    owner_root: &str,
    source: &str,
) -> Option<Diagnostic> {
    // Carrier classification routes through the language registry (the single
    // static classification authority) — never a hand-matched extension literal.
    let is_svelte = verter_session::LanguageRegistry::global()
        .classify_static(canonical_id)
        .static_resolution()
        .is_svelte();
    if !is_svelte {
        return None;
    }
    if resolve_owner_svelte(owner_root).is_some() {
        return None;
    }
    // Anchor at the file head — cover the first line so the squiggle is visible.
    let first_line_len = source.lines().next().map(str::len).unwrap_or(0) as u32;
    Some(Diagnostic {
        range: Range {
            start: Position::new(0, 0),
            end: Position::new(0, first_line_len),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(
            SVELTE_PACKAGE_MISSING_CODE.to_string(),
        )),
        source: Some("verter".to_string()),
        message: format!(
            "`svelte` is not installed in this workspace ({owner_root}). The \
             Svelte IDE type-check imports `svelte` types; install `svelte` to \
             enable type checking for `.svelte` components."
        ),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn materialize_writes_the_shim_into_the_host_data_dir_not_the_workspace() {
        let dir = materialize_svelte_jsx_shim().expect("materialize");
        // The host dir is under the system temp dir, NOT a workspace path.
        assert!(dir.starts_with(std::env::temp_dir()));
        assert!(dir.join("jsx-runtime.d.ts").exists());
        assert!(dir.join("jsx-dev-runtime.d.ts").exists());
        let runtime = std::fs::read_to_string(dir.join("jsx-runtime.d.ts")).unwrap();
        assert_eq!(runtime, SVELTE_JSX_RUNTIME_DTS);
    }

    #[test]
    fn inject_adds_the_shim_rows_always() {
        let injected = inject_svelte_paths(serde_json::Value::Null, "/nonexistent-workspace");
        let obj = injected.as_object().unwrap();
        assert!(obj.contains_key("@verter/svelte-jsx/jsx-runtime"));
        assert!(obj.contains_key("@verter/svelte-jsx/jsx-dev-runtime"));
    }

    #[test]
    fn inject_adds_the_svg_and_mathml_namespace_rows() {
        // F10: the svg / mathml namespace entrypoints are path-mapped at the
        // host-materialized shim copies (selected by the `<svelte:options
        // namespace>` pragma variant).
        let injected = inject_svelte_paths(serde_json::Value::Null, "/nonexistent-workspace");
        let obj = injected.as_object().unwrap();
        assert!(obj.contains_key("@verter/svelte-jsx/svg/jsx-runtime"));
        assert!(obj.contains_key("@verter/svelte-jsx/svg/jsx-dev-runtime"));
        assert!(obj.contains_key("@verter/svelte-jsx/mathml/jsx-runtime"));
        assert!(obj.contains_key("@verter/svelte-jsx/mathml/jsx-dev-runtime"));
    }

    #[test]
    fn materialize_writes_the_svg_and_mathml_entrypoints() {
        let dir = materialize_svelte_jsx_shim().expect("materialize");
        assert!(dir.join("svg/jsx-runtime.d.ts").exists());
        assert!(dir.join("svg/jsx-dev-runtime.d.ts").exists());
        assert!(dir.join("mathml/jsx-runtime.d.ts").exists());
        assert!(dir.join("mathml/jsx-dev-runtime.d.ts").exists());
        let svg = std::fs::read_to_string(dir.join("svg/jsx-runtime.d.ts")).unwrap();
        assert_eq!(svg, SVELTE_JSX_SVG_RUNTIME_DTS);
    }

    #[test]
    fn inject_omits_svelte_rows_when_the_workspace_has_no_svelte_install() {
        // Fail-closed: a workspace without `svelte` gets NO svelte rows.
        let injected = inject_svelte_paths(serde_json::Value::Null, "/nonexistent-workspace");
        let obj = injected.as_object().unwrap();
        assert!(!obj.contains_key("svelte"), "no svelte row (fail-closed)");
        assert!(!obj.contains_key("svelte/*"));
    }

    #[test]
    fn inject_adds_svelte_rows_when_the_workspace_installs_svelte() {
        let tmp = tempdir().unwrap();
        let svelte_dir = tmp.path().join("node_modules/svelte");
        std::fs::create_dir_all(&svelte_dir).unwrap();
        std::fs::write(svelte_dir.join("package.json"), r#"{"name":"svelte"}"#).unwrap();

        let root = tmp.path().to_string_lossy().to_string();
        let injected = inject_svelte_paths(serde_json::Value::Null, &root);
        let obj = injected.as_object().unwrap();
        assert!(obj.contains_key("svelte"), "svelte row present");
        assert!(obj.contains_key("svelte/*"), "svelte/* row present");
    }

    #[test]
    fn svelte_package_missing_diagnostic_emitted_when_owner_has_no_svelte() {
        // D-ae(d): a `.svelte` file in a workspace WITHOUT `svelte` gets the
        // typed `svelte-package-missing` diagnostic on the source file.
        let diag = svelte_package_missing_diagnostic(
            "/ws/src/App.svelte",
            "/nonexistent-workspace",
            "<div>hi</div>",
        )
        .expect("diagnostic present");
        assert_eq!(
            diag.code,
            Some(NumberOrString::String("svelte-package-missing".to_string()))
        );
        assert_eq!(diag.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diag.source.as_deref(), Some("verter"));
        assert!(diag.message.contains("svelte"));
    }

    #[test]
    fn svelte_package_missing_diagnostic_absent_when_svelte_installed() {
        // DISCRIMINATING: with `svelte` installed, NO diagnostic (no false
        // positive).
        let tmp = tempdir().unwrap();
        let svelte_dir = tmp.path().join("node_modules/svelte");
        std::fs::create_dir_all(&svelte_dir).unwrap();
        std::fs::write(svelte_dir.join("package.json"), r#"{"name":"svelte"}"#).unwrap();
        let root = tmp.path().to_string_lossy().to_string();
        assert!(
            svelte_package_missing_diagnostic("/ws/src/App.svelte", &root, "<div/>").is_none(),
            "no diagnostic when svelte is installed"
        );
    }

    #[test]
    fn svelte_package_missing_diagnostic_absent_for_non_svelte_file() {
        // A non-`.svelte` file never gets the diagnostic, even with no svelte.
        assert!(
            svelte_package_missing_diagnostic("/ws/src/App.vue", "/nonexistent", "x").is_none(),
            "no diagnostic for a non-.svelte file"
        );
    }

    #[test]
    fn inject_preserves_existing_paths_rows() {
        let existing = serde_json::json!({ "@/*": ["src/*"] });
        let injected = inject_svelte_paths(existing, "/nonexistent");
        let obj = injected.as_object().unwrap();
        assert!(obj.contains_key("@/*"), "existing row preserved");
        assert!(obj.contains_key("@verter/svelte-jsx/jsx-runtime"));
    }

    // --- D-ay PRODUCTION-TOPOLOGY + D-av ASSET-RESOLUTION TSGO fixtures ---
    //
    // These exercise the REAL row-injection mechanism (`inject_svelte_paths`):
    // the shim materializes OUTSIDE the fixture workspace (host data dir), and
    // `svelte` types live ONLY inside the workspace's own `node_modules`
    // (vendored, hermetic). No `svelte`/`@verter/svelte-jsx` mapping exists
    // beyond the rows the injection itself adds — so ONLY the mechanism makes the
    // shim's imports resolve. Removing the injected rows fails it (discriminating
    // both ways). GATED behind a locally-resolvable `tsgo`/`tsc`.

    fn workspace_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("crate is <ws>/crates/verter_lsp")
            .to_path_buf()
    }

    fn locate_type_checker() -> Option<std::path::PathBuf> {
        let bin = workspace_root().join("node_modules/.bin");
        let tsgo = bin.join("tsgo");
        if tsgo.exists() {
            return Some(tsgo);
        }
        let tsc = bin.join("tsc");
        if tsc.exists() {
            return Some(tsc);
        }
        None
    }

    /// Vendor minimal `svelte` types into `<root>/node_modules/svelte` from the
    /// in-repo session-gate vendor (hermetic; no npm install).
    fn vendor_svelte_into(root: &Path) {
        let src = workspace_root()
            .join("crates/verter_session/tests/svelte_typecheck_gate/vendor_svelte");
        let dst = root.join("node_modules/svelte");
        std::fs::create_dir_all(&dst).unwrap();
        for f in [
            "index.d.ts",
            "elements.d.ts",
            "attachments.d.ts",
            "package.json",
        ] {
            std::fs::copy(src.join(f), dst.join(f)).unwrap();
        }
    }

    /// Run the type checker over `root` with `paths` and return `(ok, output)`.
    fn typecheck_with_paths(root: &Path, paths: &serde_json::Value) -> (bool, String) {
        let checker = locate_type_checker().expect("type checker present (gated by caller)");
        let tsconfig = serde_json::json!({
            "compilerOptions": {
                "module": "esnext",
                "target": "esnext",
                "moduleResolution": "bundler",
                "jsx": "preserve",
                "jsxImportSource": "vue",
                "strict": true,
                "noEmit": true,
                "skipLibCheck": true,
                "allowImportingTsExtensions": true,
                "paths": paths,
            },
            "include": ["**/*.ts", "**/*.tsx"],
        });
        std::fs::write(
            root.join("tsconfig.json"),
            serde_json::to_string_pretty(&tsconfig).unwrap(),
        )
        .unwrap();
        let output = std::process::Command::new(&checker)
            .arg("--noEmit")
            .arg("-p")
            .arg(root.join("tsconfig.json"))
            .current_dir(root)
            .output()
            .expect("run type checker");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status.success(), combined)
    }

    /// A minimal projected `.svelte.tsx` opening with the pragma + importing the
    /// `svelte` types through the shim (so the shim's transitive `svelte` import
    /// must resolve via the injected rows).
    const PROJECTED_TSX: &str = "/** @jsxImportSource @verter/svelte-jsx */\n\
        ;function __verter_render() { return (<button onclick={() => {}}>ok</button>); }\n\
        export {};\n";

    #[test]
    fn production_topology_resolves_via_injected_rows_and_fails_without_them() {
        let Some(_) = locate_type_checker() else {
            eprintln!("SKIP production-topology: no tsgo/tsc in node_modules/.bin");
            return;
        };
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("App.svelte.tsx"), PROJECTED_TSX).unwrap();
        // `svelte` types ONLY inside the workspace node_modules (vendored).
        vendor_svelte_into(root);
        let root_str = root.to_string_lossy().to_string();

        // D-ay PRODUCTION TOPOLOGY: shim materialized OUTSIDE the workspace; the
        // ONLY `@verter/svelte-jsx` + `svelte` mapping is what the injection adds.
        let injected = inject_svelte_paths(serde_json::Value::Null, &root_str);
        let (ok, out) = typecheck_with_paths(root, &injected);
        assert!(
            ok,
            "with the injected shim + svelte rows the projection must type-check \
             CLEAN (production topology):\n{out}"
        );

        // DISCRIMINATING: remove the injected rows → the shim's imports can no
        // longer resolve → module-not-found (fails). Proves ONLY the mechanism's
        // rows make it work (D-av asset-resolution).
        let (ok_without, out_without) = typecheck_with_paths(root, &serde_json::json!({}));
        assert!(
            !ok_without,
            "WITHOUT the injected rows the shim's import source must fail \
             (module-not-found) — proving the injection is load-bearing:\n{out_without}"
        );
    }

    #[test]
    fn asset_resolution_without_workspace_svelte_npm_dep_resolves_shim_via_mapping() {
        // D-av ASSET-RESOLUTION: a workspace with NO `@verter/svelte-jsx` npm
        // dependency resolves the shim PURELY through the provider mapping (the
        // injected rows), and removing the shim mapping fails the pragma import.
        let Some(_) = locate_type_checker() else {
            eprintln!("SKIP asset-resolution: no tsgo/tsc in node_modules/.bin");
            return;
        };
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("App.svelte.tsx"), PROJECTED_TSX).unwrap();
        vendor_svelte_into(root);
        let root_str = root.to_string_lossy().to_string();

        let injected = inject_svelte_paths(serde_json::Value::Null, &root_str);
        let (ok, out) = typecheck_with_paths(root, &injected);
        assert!(
            ok,
            "the shim resolves through the injected mapping with NO workspace \
             @verter/svelte-jsx npm dep:\n{out}"
        );

        // DISCRIMINATING: drop ONLY the shim rows (keep svelte) → the pragma's
        // `@verter/svelte-jsx/jsx-runtime` import source is module-not-found.
        let mut shim_dropped = injected.as_object().unwrap().clone();
        shim_dropped.remove("@verter/svelte-jsx/jsx-runtime");
        shim_dropped.remove("@verter/svelte-jsx/jsx-dev-runtime");
        let (ok_dropped, out_dropped) =
            typecheck_with_paths(root, &serde_json::Value::Object(shim_dropped));
        assert!(
            !ok_dropped,
            "removing the shim mapping must fail the pragma import \
             (module-not-found):\n{out_dropped}"
        );
    }
}
