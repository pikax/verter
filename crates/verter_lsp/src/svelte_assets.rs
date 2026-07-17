//! Host-side materialization + path-mapping for the `@verter/svelte-jsx`
//! shim and its transitive `svelte` dependency.
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
//! the typed `svelte-package-missing` diagnostic).

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use verter_session::framework::svelte_jsx_assets::{
    SVELTE_JSX_DEV_RUNTIME_DTS, SVELTE_JSX_DEV_RUNTIME_SPECIFIER,
    SVELTE_JSX_MATHML_DEV_RUNTIME_DTS, SVELTE_JSX_MATHML_DEV_RUNTIME_SPECIFIER,
    SVELTE_JSX_MATHML_RUNTIME_DTS, SVELTE_JSX_MATHML_RUNTIME_SPECIFIER, SVELTE_JSX_RUNTIME_DTS,
    SVELTE_JSX_RUNTIME_SPECIFIER, SVELTE_JSX_SVG_DEV_RUNTIME_DTS,
    SVELTE_JSX_SVG_DEV_RUNTIME_SPECIFIER, SVELTE_JSX_SVG_RUNTIME_DTS,
    SVELTE_JSX_SVG_RUNTIME_SPECIFIER,
};

const SVELTE_JSX_HTML_PRAGMA: &str = "/** @jsxImportSource @verter/svelte-jsx */\n";
const SVELTE_JSX_SVG_PRAGMA: &str = "/** @jsxImportSource @verter/svelte-jsx/svg */\n";
const SVELTE_JSX_MATHML_PRAGMA: &str = "/** @jsxImportSource @verter/svelte-jsx/mathml */\n";

#[derive(Debug, Clone, Copy)]
enum SvelteJsxAssetNamespace {
    Html,
    Svg,
    MathMl,
}

impl SvelteJsxAssetNamespace {
    fn from_carrier(content: &str) -> Option<(Self, &'static str)> {
        if content.starts_with(SVELTE_JSX_HTML_PRAGMA) {
            Some((Self::Html, SVELTE_JSX_HTML_PRAGMA))
        } else if content.starts_with(SVELTE_JSX_SVG_PRAGMA) {
            Some((Self::Svg, SVELTE_JSX_SVG_PRAGMA))
        } else if content.starts_with(SVELTE_JSX_MATHML_PRAGMA) {
            Some((Self::MathMl, SVELTE_JSX_MATHML_PRAGMA))
        } else {
            None
        }
    }

    fn directory(self) -> &'static str {
        match self {
            Self::Html => "",
            Self::Svg => "svg",
            Self::MathMl => "mathml",
        }
    }

    fn runtime(self) -> &'static str {
        match self {
            Self::Html => SVELTE_JSX_RUNTIME_DTS,
            Self::Svg => SVELTE_JSX_SVG_RUNTIME_DTS,
            Self::MathMl => SVELTE_JSX_MATHML_RUNTIME_DTS,
        }
    }
}

/// Exact provider bytes for a managed-tsgo Svelte carrier plus the classic JSX
/// adapter they import. The carrier rewrite replaces one generated prelude line
/// with one generated prelude line, preserving every source-map coordinate.
pub(crate) struct PreparedManagedTsgoSvelteCarrier {
    pub(crate) content: String,
    #[cfg(test)]
    pub(crate) shim_path: PathBuf,
    #[cfg(test)]
    pub(crate) shim_content: String,
}

#[derive(Debug)]
struct ResolvedSveltePackage {
    root: PathBuf,
    main_types: PathBuf,
    elements_types: PathBuf,
    version: String,
    package_json: Vec<u8>,
}

/// The typed Verter diagnostic code for a missing `svelte` package install.
/// Emitted on a `.svelte` source file whose owner workspace
/// has no `svelte` install — the shim's `import … from "svelte"` then fails
/// CLOSED (module-not-found), and this typed diagnostic explains WHY.
pub(crate) const SVELTE_PACKAGE_MISSING_CODE: &str = "svelte-package-missing";

/// The host data directory the shim materializes into — a per-host-version
/// subdirectory under the system temp dir (NOT the user workspace). The
/// version stamp keeps the copy matched to the projection the compiler emits.
fn host_shim_dir() -> PathBuf {
    let mut identity = blake3::Hasher::new();
    for asset in [
        SVELTE_JSX_RUNTIME_DTS,
        SVELTE_JSX_DEV_RUNTIME_DTS,
        SVELTE_JSX_SVG_RUNTIME_DTS,
        SVELTE_JSX_SVG_DEV_RUNTIME_DTS,
        SVELTE_JSX_MATHML_RUNTIME_DTS,
        SVELTE_JSX_MATHML_DEV_RUNTIME_DTS,
    ] {
        identity.update(&(asset.len() as u64).to_le_bytes());
        identity.update(asset.as_bytes());
    }
    let asset_key = &identity.finalize().to_hex()[..16];
    std::env::temp_dir().join("verter-host").join(format!(
        "svelte-jsx-{}-{asset_key}",
        env!("CARGO_PKG_VERSION")
    ))
}

fn invalid_package(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(ErrorKind::InvalidData, message.into())
}

fn exported_types(value: &serde_json::Value) -> Option<&str> {
    match value {
        serde_json::Value::String(target) => Some(target),
        serde_json::Value::Array(entries) => entries.iter().find_map(exported_types),
        serde_json::Value::Object(conditions) => conditions
            .get("types")
            .and_then(serde_json::Value::as_str)
            .or_else(|| conditions.get("svelte").and_then(exported_types))
            .or_else(|| conditions.get("import").and_then(exported_types))
            .or_else(|| conditions.get("default").and_then(exported_types)),
        _ => None,
    }
}

fn resolve_declaration_target(
    package_root: &Path,
    target: &str,
    label: &str,
) -> std::io::Result<PathBuf> {
    let relative = target.strip_prefix("./").unwrap_or(target);
    if Path::new(relative).is_absolute()
        || relative.split(['/', '\\']).any(|segment| segment == "..")
    {
        return Err(invalid_package(format!(
            "Svelte {label} declaration target escapes its package: {target}"
        )));
    }
    let path = package_root.join(relative);
    if !path.is_file() {
        return Err(invalid_package(format!(
            "Svelte {label} declaration target does not exist: {}",
            path.display()
        )));
    }
    std::fs::canonicalize(path)
}

fn resolve_svelte_package(candidate: &Path) -> std::io::Result<ResolvedSveltePackage> {
    let package_json_path = candidate.join("package.json");
    let package_json = std::fs::read(&package_json_path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&package_json)
        .map_err(|error| invalid_package(format!("invalid Svelte package.json: {error}")))?;
    if manifest.get("name").and_then(serde_json::Value::as_str) != Some("svelte") {
        return Err(invalid_package(format!(
            "package at {} is not named `svelte`",
            candidate.display()
        )));
    }
    let version = manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_package("Svelte package.json has no string version"))?;
    if version.split('.').next() != Some("5") {
        return Err(invalid_package(format!(
            "unsupported Svelte version {version}; Verter supports Svelte 5"
        )));
    }

    let exports = manifest.get("exports");
    let main_target = exports
        .and_then(|value| value.get("."))
        .and_then(exported_types)
        .or_else(|| manifest.get("types").and_then(serde_json::Value::as_str))
        .ok_or_else(|| invalid_package("Svelte package has no public type declaration target"))?;
    let elements_target = exports
        .and_then(|value| value.get("./elements"))
        .and_then(exported_types)
        .ok_or_else(|| invalid_package("Svelte package has no `./elements` type export"))?;

    let root = std::fs::canonicalize(candidate)?;
    let main_types = resolve_declaration_target(&root, main_target, "root")?;
    let elements_types = resolve_declaration_target(&root, elements_target, "elements")?;
    Ok(ResolvedSveltePackage {
        root,
        main_types,
        elements_types,
        version: version.to_owned(),
        package_json,
    })
}

fn nearest_svelte_for_carrier(
    provider_path: &str,
) -> std::io::Result<Option<ResolvedSveltePackage>> {
    let mut directory = Path::new(provider_path).parent();
    while let Some(current) = directory {
        let candidate = current.join("node_modules/svelte");
        if candidate.join("package.json").is_file() {
            return resolve_svelte_package(&candidate).map(Some);
        }
        directory = current.parent();
    }
    Ok(None)
}

fn normalized_module_path(path: &Path) -> String {
    verter_span::path::canonicalize_path_cow(&path.to_string_lossy()).into_owned()
}

fn owner_asset_key(
    namespace: SvelteJsxAssetNamespace,
    package: &ResolvedSveltePackage,
    runtime: &str,
) -> String {
    let mut identity = blake3::Hasher::new();
    for field in [
        env!("CARGO_PKG_VERSION").as_bytes(),
        namespace.directory().as_bytes(),
        package.version.as_bytes(),
        normalized_module_path(&package.root).as_bytes(),
        normalized_module_path(&package.main_types).as_bytes(),
        normalized_module_path(&package.elements_types).as_bytes(),
        package.package_json.as_slice(),
        runtime.as_bytes(),
    ] {
        identity.update(&(field.len() as u64).to_le_bytes());
        identity.update(field);
    }
    identity.finalize().to_hex()[..24].to_owned()
}

fn owner_bound_runtime(source: &str, package: &ResolvedSveltePackage) -> std::io::Result<String> {
    let svelte = serde_json::to_string(&normalized_module_path(&package.main_types))
        .expect("a filesystem path always serializes as a JSON string");
    let elements = serde_json::to_string(&normalized_module_path(&package.elements_types))
        .expect("a filesystem path always serializes as a JSON string");
    let elements_count = source.matches("\"svelte/elements\"").count();
    let svelte_count = source.matches("\"svelte\"").count();
    if elements_count == 0 || svelte_count == 0 {
        return Err(invalid_package(
            "canonical Svelte JSX runtime no longer has the expected official type imports",
        ));
    }
    let rewritten = source
        .replace("\"svelte/elements\"", &elements)
        .replace("\"svelte\"", &svelte);
    if rewritten.contains("\"svelte") || rewritten.contains("'svelte") {
        return Err(invalid_package(
            "canonical Svelte JSX runtime gained an unhandled bare Svelte import",
        ));
    }
    Ok(rewritten)
}

fn classic_jsx_adapter() -> &'static str {
    r#"import type { JSX as __VerterAutomaticJSX } from "./jsx-runtime";
export function h(...args: unknown[]): __VerterAutomaticJSX.Element;
export const Fragment: unique symbol;
export namespace JSX {
  type Element = __VerterAutomaticJSX.Element;
  type ElementType = __VerterAutomaticJSX.ElementType;
  type LibraryManagedAttributes<Component, FallbackProps> =
    __VerterAutomaticJSX.LibraryManagedAttributes<Component, FallbackProps>;
  type ElementClass = __VerterAutomaticJSX.ElementClass;
  type ElementAttributesProperty = __VerterAutomaticJSX.ElementAttributesProperty;
  type IntrinsicElements = __VerterAutomaticJSX.IntrinsicElements;
}
"#
}

fn collision_free_binding(asset_key: &str, content: &str) -> String {
    for nonce in 0_u64.. {
        let suffix = if nonce == 0 {
            asset_key.to_owned()
        } else {
            blake3::hash(format!("{asset_key}\0{nonce}").as_bytes()).to_hex()[..24].to_owned()
        };
        let candidate = format!("__verter_svelte_jsx_{suffix}");
        if !content.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("the unbounded hash namespace always has a collision-free identifier")
}

/// Specialize a compiler-owned Svelte IDE carrier for managed tsgo without
/// mutating the consumer's tsconfig or workspace. Native tsgo currently treats
/// `workspace/didChangeConfiguration` as user preferences, so configured-project
/// compiler options cannot carry Verter's private JSX runtime. The provider
/// buffer instead imports an owner-bound classic JSX adapter from Verter's host
/// cache. Its nested JSX namespace aliases the same canonical runtime types, and
/// its absolute, quoted imports resolve paths containing spaces on every host.
///
/// `Ok(None)` means the content is not a Svelte JSX carrier, or its normal Node
/// ancestor walk finds no Svelte install. In the latter case the original bare
/// import source remains unresolved and the public `svelte-package-missing`
/// diagnostic explains the fail-closed outcome.
pub(crate) fn prepare_managed_tsgo_svelte_carrier(
    provider_path: &str,
    content: &str,
) -> std::io::Result<Option<PreparedManagedTsgoSvelteCarrier>> {
    let Some((asset_namespace, pragma)) = SvelteJsxAssetNamespace::from_carrier(content) else {
        return Ok(None);
    };
    let Some(package) = nearest_svelte_for_carrier(provider_path)? else {
        return Ok(None);
    };

    let runtime = owner_bound_runtime(asset_namespace.runtime(), &package)?;
    let key = owner_asset_key(asset_namespace, &package, &runtime);
    let factory_namespace = collision_free_binding(&key, content);
    let mut directory = host_shim_dir().join("owners").join(&key);
    let namespace_directory = asset_namespace.directory();
    if !namespace_directory.is_empty() {
        directory.push(namespace_directory);
    }
    std::fs::create_dir_all(&directory)?;

    write_if_changed(&directory.join("jsx-runtime.d.ts"), &runtime)?;
    let shim_content = classic_jsx_adapter();
    let shim_path = directory.join("classic.d.ts");
    write_if_changed(&shim_path, shim_content)?;

    // Import the declaration module through its extensionless module specifier.
    // Direct value imports ending in `.d.ts` are rejected by TypeScript (TS2846).
    let shim_specifier = shim_path.with_extension("");
    let import_path = serde_json::to_string(&normalized_module_path(&shim_specifier))
        .expect("a filesystem path always serializes as a JSON string");
    let provider_intro = format!(
        "/** @jsxRuntime classic */ /** @jsx {factory_namespace}.h */ /** @jsxFrag {factory_namespace}.Fragment */ import * as {factory_namespace} from {import_path};\n"
    );
    let mut prepared = String::with_capacity(content.len() - pragma.len() + provider_intro.len());
    prepared.push_str(&provider_intro);
    prepared.push_str(&content[pragma.len()..]);

    Ok(Some(PreparedManagedTsgoSvelteCarrier {
        content: prepared,
        #[cfg(test)]
        shim_path,
        #[cfg(test)]
        shim_content: shim_content.to_owned(),
    }))
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
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == content => return Ok(()),
        Ok(_) => {
            return Err(std::io::Error::new(
                ErrorKind::AlreadyExists,
                format!(
                    "immutable Svelte JSX asset has unexpected bytes: {}",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("Svelte JSX asset has no parent: {}", path.display()),
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "Svelte JSX asset has no UTF-8 file name: {}",
                    path.display()
                ),
            )
        })?;

    let temp_path = loop {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), id));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                if let Err(error) = file
                    .write_all(content.as_bytes())
                    .and_then(|()| file.sync_all())
                {
                    drop(file);
                    let _ = std::fs::remove_file(&candidate);
                    return Err(error);
                }
                drop(file);
                break candidate;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    match std::fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            // A concurrent process may have published the same content-addressed
            // asset first. Accept only an exact byte match; otherwise fail closed.
            let concurrent_match =
                std::fs::read_to_string(path).is_ok_and(|existing| existing == content);
            let _ = std::fs::remove_file(&temp_path);
            if concurrent_match {
                Ok(())
            } else {
                Err(rename_error)
            }
        }
    }
}

/// Resolve the owner workspace's installed `svelte` package directory, if any.
///
/// `workspace_root` is the owner project root (a filesystem path). Returns the
/// `<root>/node_modules/svelte` directory when it exists. A monorepo with
/// multiple `svelte` installs resolves each project against its own copy (the
/// caller passes the owner project root). When NO `svelte` is installed,
/// returns `None` — no rows are injected and the shim's imports fail closed.
pub(crate) fn resolve_owner_svelte(workspace_root: &str) -> Option<PathBuf> {
    let mut directory = Some(Path::new(workspace_root));
    while let Some(current) = directory {
        let candidate = current.join("node_modules/svelte");
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
        directory = current.parent();
    }
    None
}

/// Inject the svelte-jsx shim + transitive `svelte` rows into a `paths` JSON
/// object for `configure_paths`.
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

/// Build the complete provider path environment for one configured owner.
/// Provider-owned JSX assets are required even when the consumer tsconfig has
/// no `baseUrl`/`paths`; `raw_paths_json` therefore supplies an empty base
/// mapping for every readable config and this layer adds the version-matched
/// Svelte runtime plus the owner's own Svelte dependency.
pub(crate) fn owner_provider_path_config(
    workspace: &dyn verter_workspace::WorkspaceRead,
    tsconfig_path: &str,
    owner_root: &str,
) -> Option<(String, serde_json::Value)> {
    let (base_url, paths) = verter_workspace::config::raw_paths_json(workspace, tsconfig_path)?;
    Some((base_url, inject_svelte_paths(paths, owner_root)))
}

/// Produce the typed `svelte-package-missing` diagnostic for a `.svelte` source
/// file when its owner workspace has NO `svelte` install.
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
    fn owner_provider_paths_inject_assets_when_tsconfig_has_no_paths() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"jsx":"preserve"},"include":["src"]}"#,
        )
        .unwrap();
        let svelte_dir = tmp.path().join("node_modules/svelte");
        std::fs::create_dir_all(&svelte_dir).unwrap();
        std::fs::write(svelte_dir.join("package.json"), r#"{"name":"svelte"}"#).unwrap();
        let workspace = verter_workspace::FilesystemWorkspace::new(
            verter_workspace::FilesystemOptions::default(),
        );
        let root = tmp.path().to_string_lossy().replace('\\', "/");
        let config = tmp
            .path()
            .join("tsconfig.json")
            .to_string_lossy()
            .replace('\\', "/");

        let (base_url, paths) = owner_provider_path_config(&workspace, &config, &root)
            .expect("a readable clean tsconfig still needs provider assets");
        let entries = paths.as_object().expect("provider paths are an object");

        assert_eq!(
            verter_span::path::canonicalize_path_cow(&base_url),
            verter_span::path::canonicalize_path_cow(&root),
        );
        assert!(entries.contains_key("@verter/svelte-jsx/jsx-runtime"));
        assert!(entries.contains_key("svelte"));
        assert!(entries.contains_key("svelte/*"));
    }

    #[test]
    fn svelte_package_missing_diagnostic_emitted_when_owner_has_no_svelte() {
        // A `.svelte` file in a workspace WITHOUT `svelte` gets the
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

    // --- PRODUCTION-TOPOLOGY + ASSET-RESOLUTION TSGO fixtures ---
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

    /// Resolve a `node_modules/.bin` entry to an EXECUTABLE path. On Windows the
    /// extensionless `.bin/<name>` is a POSIX shell script (the runnable forms
    /// are `<name>.cmd` / `<name>.exe`); exec'ing the bare script there fails
    /// with `%1 is not a valid Win32 application`. Probe the platform-appropriate
    /// executable forms first, falling back to the bare name on Unix.
    fn locate_bin(bin_dir: &Path, name: &str) -> Option<std::path::PathBuf> {
        if cfg!(windows) {
            for ext in ["cmd", "CMD", "exe", "EXE", "bat"] {
                let p = bin_dir.join(format!("{name}.{ext}"));
                if p.exists() {
                    return Some(p);
                }
            }
            // No Windows launcher present: a bare POSIX shim is not executable
            // via std::process on Windows, so treat it as absent.
            return None;
        }
        let p = bin_dir.join(name);
        p.exists().then_some(p)
    }

    fn locate_type_checker() -> Option<std::path::PathBuf> {
        let bin = workspace_root().join("node_modules/.bin");
        locate_bin(&bin, "tsgo").or_else(|| locate_bin(&bin, "tsc"))
    }

    /// Vendor minimal `svelte` types into `<root>/node_modules/svelte` from the
    /// in-repo session-gate vendor (hermetic; no npm install).
    fn vendor_svelte_into(root: &Path) {
        let src = workspace_root()
            .join("crates/verter_session/tests/cases/svelte_typecheck_gate/vendor_svelte");
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

        // PRODUCTION TOPOLOGY: shim materialized OUTSIDE the workspace; the
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
        // rows make it work (asset-resolution).
        let (ok_without, out_without) = typecheck_with_paths(root, &serde_json::json!({}));
        assert!(
            !ok_without,
            "WITHOUT the injected rows the shim's import source must fail \
             (module-not-found) — proving the injection is load-bearing:\n{out_without}"
        );
    }

    #[test]
    fn asset_resolution_without_workspace_svelte_npm_dep_resolves_shim_via_mapping() {
        // ASSET-RESOLUTION: a workspace with NO `@verter/svelte-jsx` npm
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

    #[test]
    fn managed_tsgo_carrier_uses_owner_bound_classic_jsx_without_project_paths() {
        let Some(_) = locate_type_checker() else {
            eprintln!("SKIP managed-tsgo JSX carrier: no tsgo/tsc in node_modules/.bin");
            return;
        };
        let tmp = tempdir().unwrap();
        let root = tmp.path();
        vendor_svelte_into(root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let carrier_path = root.join("src/App.svelte.tsx");
        let source = concat!(
            "/** @jsxImportSource @verter/svelte-jsx */\n",
            "import type { Component } from \"svelte\";\n",
            "declare const Child: Component<{ label: string }>;\n",
            "function render() { return (<div><Child label=\"ok\" /></div>); }\n",
            "void render; export {};\n",
        );

        let prepared = prepare_managed_tsgo_svelte_carrier(&carrier_path.to_string_lossy(), source)
            .expect("prepare managed-tsgo carrier")
            .expect("Svelte carrier with an installed owner must be specialized");

        assert_eq!(
            prepared.content.lines().count(),
            source.lines().count(),
            "provider specialization must preserve every generated line/source-map coordinate"
        );
        assert!(prepared.content.starts_with("/** @jsxRuntime classic */"));
        assert!(!prepared
            .content
            .contains("@jsxImportSource @verter/svelte-jsx"));
        assert!(prepared.shim_path.starts_with(std::env::temp_dir()));
        assert!(!prepared.shim_path.starts_with(root));
        assert_eq!(
            std::fs::read_to_string(&prepared.shim_path).expect("materialized classic shim"),
            prepared.shim_content,
            "the imported provider shim bytes must be exactly the materialized bytes"
        );

        std::fs::write(&carrier_path, &prepared.content).unwrap();
        let (ok, out) = typecheck_with_paths(root, &serde_json::json!({}));
        assert!(
            ok,
            "the owner-bound carrier must type-check without mutable paths/baseUrl settings; \
             this covers SvelteHTMLElements intrinsics and callable Component props:\n{out}\n{}",
            prepared.content
        );

        // Negative control proving the managed-tsgo classic namespace checks
        // native Svelte Component props at the authored JSX
        // attribute rather than relying on a separate generated witness.
        let invalid_source = concat!(
            "/** @jsxImportSource @verter/svelte-jsx */\n",
            "import type { Component } from \"svelte\";\n",
            "declare const Child: Component<{ label: string }>;\n",
            "function render() { return (<div><Child label={123} /></div>); }\n",
            "void render; export {};\n",
        );
        let invalid =
            prepare_managed_tsgo_svelte_carrier(&carrier_path.to_string_lossy(), invalid_source)
                .expect("prepare invalid managed-tsgo carrier")
                .expect("Svelte carrier with an installed owner must be specialized");
        std::fs::write(&carrier_path, &invalid.content).unwrap();
        let (invalid_ok, invalid_out) = typecheck_with_paths(root, &serde_json::json!({}));
        assert!(
            !invalid_ok && invalid_out.contains("TS2322"),
            "a number supplied to a native Svelte `Component<{{ label: string }}>` must fail \
             at the JSX attribute with TS2322; output:\n{invalid_out}\n{}",
            invalid.content
        );
    }
}
