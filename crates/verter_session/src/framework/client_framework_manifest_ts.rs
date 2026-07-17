#![deny(missing_docs)]
//! Renderer for the generated TypeScript CLIENT FRAMEWORK MANIFEST.
//!
//! The Rust [`FrameworkAdapterRegistry`] descriptor table is the SINGLE
//! authority for which frameworks the VS Code extension + TypeScript-plugin
//! client wiring activates for, what carrier / adapter-module extensions they
//! own, which client language ids they attach to, and how their virtual files
//! are named. The committed TypeScript module
//! `packages/language-shared/src/client-framework-manifest.generated.ts` is a
//! GENERATED, BYTE-PINNED mirror of that authority — the extension's activation
//! events, the LSP document selector, and the `_typescript.configurePlugin`
//! trigger language ids all derive from it, so adding a framework needs no
//! per-framework client edit. (File-watch globs are NOT a client concern: the
//! LSP server owns the `workspace/didChangeWatchedFiles` watcher, derived from
//! the same descriptor authority, so the client manifest carries none.)
//!
//! [`render_client_framework_manifest_ts`] renders the canonical TS module text
//! from the descriptor rows (joined with the [`LanguageRegistry`] extension
//! table for adapter-module extensions); the freshness test
//! (`crates/verter_session/tests/cases/client_framework_manifest_ts_freshness.rs`)
//! byte-compares the committed file against this render, so a hand-edit or a
//! descriptor/registry change without a regen fails the gate. Mirrors the
//! `virtual_file_naming_ts_freshness` discipline (regenerate + byte-compare).
//!
//! [`FrameworkAdapterRegistry`]: crate::framework::FrameworkAdapterRegistry
//! [`LanguageRegistry`]: verter_language::LanguageRegistry

use verter_language::LanguageRegistry;

use crate::framework::descriptor::{
    built_in_descriptors, FrameworkAdapterDescriptor, VirtualPathPolicy,
};

/// The committed path of the generated manifest, relative to the workspace
/// root. The freshness test and any regen path reference this one place.
pub const CLIENT_FRAMEWORK_MANIFEST_TS_PATH: &str =
    "packages/language-shared/src/client-framework-manifest.generated.ts";

/// The base TS/JS language ids the extension ACTIVATES for and configures the
/// built-in VS Code TypeScript-server plugin (`_typescript.configurePlugin`)
/// for. These are NOT framework carriers (no adapter row owns them); they are
/// the full TS/JS document surface including the React (`*react`) dialects the
/// built-in TS server plugin must attach to. Recorded here so the manifest is
/// the single authority for the client's activation + TS-plugin-configure set.
const BASE_TYPESCRIPT_LANGUAGE_IDS: &[&str] = &[
    "javascript",
    "javascriptreact",
    "typescript",
    "typescriptreact",
];

/// The base TS/JS language ids the LSP DOCUMENT SELECTOR attaches to — the
/// plain `javascript` / `typescript` surfaces only. The React dialects are
/// intentionally EXCLUDED from the LSP selector (they activate the extension +
/// configure the built-in TS plugin, but the Verter LSP itself only selects the
/// plain TS/JS + framework-carrier documents). This split preserves the exact
/// pre-manifest Vue document-selector surface (`vue` + `javascript` +
/// `typescript`); the React dialects were never in the LSP selector.
const DOCUMENT_SELECTOR_BASE_LANGUAGE_IDS: &[&str] = &["javascript", "typescript"];

/// Render the canonical TypeScript client framework manifest module from the
/// descriptor registry + language extension table. The output is deterministic
/// and oxfmt-stable (two-space indent, double quotes, trailing newline) so the
/// byte-pin holds.
#[must_use]
pub fn render_client_framework_manifest_ts() -> String {
    let registry = LanguageRegistry::built_in();
    // Collect the per-framework data once, then render both the manifest rows
    // and the flattened derived lists from it — the derived lists are emitted as
    // literal arrays (NOT runtime `flatMap`) so the module needs no ES2019 lib
    // and the byte-pin stays deterministic.
    let frameworks: Vec<FrameworkRow> = built_in_descriptors()
        .iter()
        .filter_map(|descriptor| FrameworkRow::from_descriptor(&registry, descriptor))
        .collect();

    let mut out = String::new();
    out.push_str(HEADER);

    // The per-framework manifest rows — one per built-in carrier descriptor.
    out.push_str("export const CLIENT_FRAMEWORKS: readonly ClientFramework[] = [\n");
    for row in &frameworks {
        row.render_entry(&mut out);
    }
    out.push_str("];\n\n");

    // The base TS/JS language ids the extension activates + configures the
    // built-in TS plugin for (the full TS/JS surface incl. React dialects).
    // Rendered multi-line so the output stays oxfmt-stable.
    render_multiline_str_array(
        &mut out,
        "BASE_TYPESCRIPT_LANGUAGE_IDS",
        BASE_TYPESCRIPT_LANGUAGE_IDS.iter().copied(),
    );
    out.push_str(
        "\n// The base TS/JS language ids the LSP document selector attaches to (plain\n\
         // js/ts only — the React dialects are activation/plugin-configure surfaces,\n\
         // never LSP-selector ones). Preserves the pre-manifest Vue selector surface.\n",
    );
    render_multiline_str_array(
        &mut out,
        "DOCUMENT_SELECTOR_BASE_LANGUAGE_IDS",
        DOCUMENT_SELECTOR_BASE_LANGUAGE_IDS.iter().copied(),
    );

    out.push_str(FOOTER);

    // The flattened derived lists, computed in Rust and emitted as literals.
    let framework_ids: Vec<&str> = frameworks
        .iter()
        .map(|f| f.client_language_id.as_str())
        .collect();
    let activation_ids: Vec<&str> = BASE_TYPESCRIPT_LANGUAGE_IDS
        .iter()
        .copied()
        .chain(framework_ids.iter().copied())
        .collect();
    let selector_ids: Vec<&str> = DOCUMENT_SELECTOR_BASE_LANGUAGE_IDS
        .iter()
        .copied()
        .chain(framework_ids.iter().copied())
        .collect();
    let carrier_exts: Vec<&str> = frameworks.iter().map(|f| f.carrier_ext.as_str()).collect();

    out.push_str(
        "\n// Every language id the extension ACTIVATES for: the base TS/JS surface\n\
         // (incl. React dialects) plus every registered framework's client language ids.\n",
    );
    render_multiline_str_array(
        &mut out,
        "CLIENT_ACTIVATION_LANGUAGE_IDS",
        activation_ids.into_iter(),
    );
    out.push_str(
        "\n// The LSP DOCUMENT SELECTOR language ids: the plain js/ts base plus every\n\
         // registered framework's client language ids (no React dialects).\n",
    );
    render_multiline_str_array(
        &mut out,
        "CLIENT_DOCUMENT_SELECTOR_LANGUAGE_IDS",
        selector_ids.into_iter(),
    );
    out.push_str(
        "\n// The framework client language ids whose documents START the LSP (carriers).\n",
    );
    render_multiline_str_array(
        &mut out,
        "CLIENT_FRAMEWORK_LANGUAGE_IDS",
        framework_ids.iter().copied(),
    );
    out.push_str("\n// Every framework carrier file extension, across registered frameworks.\n");
    render_multiline_str_array(
        &mut out,
        "CLIENT_CARRIER_EXTENSIONS",
        carrier_exts.into_iter(),
    );

    out
}

/// The collected client-wiring data for one framework, derived from its
/// descriptor + the language extension table.
struct FrameworkRow {
    framework_id: String,
    carrier_ext: String,
    client_language_id: String,
    adapter_module_exts: Vec<String>,
    descriptor: FrameworkAdapterDescriptor,
}

impl FrameworkRow {
    /// Collect the row for a descriptor, or `None` for a carrier-less adapter
    /// (no carrier extension ⇒ no client wiring ⇒ no manifest row). Every
    /// built-in adapter today is carrier-backed.
    fn from_descriptor(
        registry: &LanguageRegistry,
        descriptor: &FrameworkAdapterDescriptor,
    ) -> Option<Self> {
        let carrier_language = descriptor.carrier_language.as_ref()?;
        let carrier_ext = format!(".{}", carrier_language.as_str());
        // The client language id is the carrier language id (VS Code's `.vue` =>
        // `vue`, `.svelte` => `svelte`). The trigger language ids that start the
        // LSP / configure the TS plugin are the SAME set for a carrier adapter.
        let client_language_id = carrier_language.as_str().to_string();
        let adapter_module_exts: Vec<String> = registry
            .adapter_module_extensions(&descriptor.id)
            .into_iter()
            .map(|ext| format!(".{ext}"))
            .collect();
        Some(Self {
            framework_id: descriptor.id.as_str().to_string(),
            carrier_ext,
            client_language_id,
            adapter_module_exts,
            descriptor: descriptor.clone(),
        })
    }

    fn render_entry(&self, out: &mut String) {
        out.push_str("  {\n");
        out.push_str(&format!(
            "    frameworkId: {},\n",
            quote(&self.framework_id)
        ));
        out.push_str(&format!(
            "    carrierExtensions: {},\n",
            render_str_array([self.carrier_ext.as_str()].into_iter())
        ));
        out.push_str(&format!(
            "    adapterModuleExtensions: {},\n",
            render_str_array(self.adapter_module_exts.iter().map(String::as_str))
        ));
        out.push_str(&format!(
            "    clientLanguageIds: {},\n",
            render_str_array([self.client_language_id.as_str()].into_iter())
        ));
        out.push_str(&format!(
            "    triggerLanguageIds: {},\n",
            render_str_array([self.client_language_id.as_str()].into_iter())
        ));
        render_virtual_file_suffixes(out, &self.descriptor);
        out.push_str("  },\n");
    }
}

/// oxfmt's default print width. A top-level `export const NAME: readonly
/// string[] = [...]` declaration whose single-line form is at or under this
/// width stays inline; a wider one wraps one entry per line. Replicating the
/// rule keeps the render byte-equal to an oxfmt pass.
const OXFMT_PRINT_WIDTH: usize = 100;

/// Render `export const NAME: readonly string[] = [ ... ];`, choosing the
/// single-line form when it fits within [`OXFMT_PRINT_WIDTH`] and the wrapped
/// one-entry-per-line form otherwise — oxfmt-stable either way.
fn render_multiline_str_array<'a>(
    out: &mut String,
    name: &str,
    values: impl Iterator<Item = &'a str>,
) {
    let items: Vec<String> = values.map(quote).collect();
    let inline = format!(
        "export const {name}: readonly string[] = [{}];",
        items.join(", ")
    );
    if inline.len() <= OXFMT_PRINT_WIDTH {
        out.push_str(&inline);
        out.push('\n');
        return;
    }
    out.push_str(&format!("export const {name}: readonly string[] = [\n"));
    for item in items {
        out.push_str(&format!("  {item},\n"));
    }
    out.push_str("];\n");
}

fn render_virtual_file_suffixes(out: &mut String, descriptor: &FrameworkAdapterDescriptor) {
    let Some(naming) = descriptor.virtual_file_naming.as_ref() else {
        // A descriptor with no virtual-file naming has empty suffix sets — the
        // adapter projects no virtual files. Recorded explicitly so a consumer
        // never has to special-case a missing field.
        out.push_str(
            "    virtualFileSuffixes: { ide: [], importSurface: [], testingApi: null, sidecars: [] },\n",
        );
        return;
    };
    out.push_str("    virtualFileSuffixes: {\n");
    out.push_str(&format!(
        "      ide: {},\n",
        render_str_array(policy_suffixes(&naming.ide).iter().map(String::as_str))
    ));
    out.push_str(&format!(
        "      importSurface: {},\n",
        render_str_array(
            policy_suffixes(&naming.import_surface)
                .iter()
                .map(String::as_str)
        )
    ));
    out.push_str(&format!(
        "      testingApi: {},\n",
        render_opt_str(naming.testing_api_suffix)
    ));
    out.push_str(&format!(
        "      sidecars: {},\n",
        render_str_array(naming.sidecar_suffixes.iter().copied())
    ));
    out.push_str("    },\n");
}

/// The concrete virtual-file suffixes a [`VirtualPathPolicy`] appends. A
/// `Suffix` is one suffix; a `JsxConditional` is both the JSX and non-JSX
/// suffixes; `SelfFile`/`None` append no distinct suffix (the file serves its
/// own path / projects no virtual file), so they contribute an empty set.
fn policy_suffixes(policy: &VirtualPathPolicy) -> Vec<String> {
    match policy {
        VirtualPathPolicy::None | VirtualPathPolicy::SelfFile => Vec::new(),
        VirtualPathPolicy::Suffix(s) => vec![(*s).to_string()],
        VirtualPathPolicy::JsxConditional { jsx, non_jsx } => {
            vec![(*jsx).to_string(), (*non_jsx).to_string()]
        }
    }
}

fn render_str_array<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let inner: Vec<String> = values.map(quote).collect();
    if inner.is_empty() {
        return "[]".to_string();
    }
    format!("[{}]", inner.join(", "))
}

fn render_opt_str(value: Option<&str>) -> String {
    match value {
        None => "null".to_string(),
        Some(s) => quote(s),
    }
}

/// Quote a string as a TS double-quoted literal. The ids / suffixes / globs are
/// static adapter constants (`.ts`, `vue`, `**/*.vue`, …) with no embedded
/// quotes or backslashes, so a plain wrap is exact.
fn quote(s: &str) -> String {
    format!("\"{s}\"")
}

const HEADER: &str = r#"// @generated by verter — DO NOT EDIT BY HAND.
//
// Client framework manifest — the single authority for the VS Code extension +
// TypeScript-plugin CLIENT wiring. Rendered from the Rust framework-adapter
// registry (`crates/verter_session/src/framework/descriptor.rs` joined with the
// `verter_language` extension table) and byte-pinned by
// `client_framework_manifest_ts_freshness`. Regenerate via that test's update
// path after any descriptor / registry change.
//
// The extension's activation events, the LSP document selector, and the
// `_typescript.configurePlugin` trigger language ids all derive from this
// manifest — adding a framework needs no per-framework client edit. (File-watch
// globs are NOT a client concern: the LSP server owns the
// `workspace/didChangeWatchedFiles` watcher, derived from the same descriptor
// authority, so this manifest carries none.)

export interface FrameworkVirtualFileSuffixes {
  // The IDE (type-checked) virtual-file suffixes (e.g. [".jsx", ".tsx"] for a
  // JSX-conditional carrier, [".tsx"] for a fixed one). Empty for a self-file /
  // no-virtual-file surface.
  ide: readonly string[];
  // The import-resolution virtual-file suffix(es) a consuming module resolves
  // the carrier to (e.g. [".ts"]). Empty for a self-file surface.
  importSurface: readonly string[];
  // The testing-API virtual-file suffix, or null when the adapter has none.
  testingApi: string | null;
  // Additional sidecar virtual-file suffixes.
  sidecars: readonly string[];
}

export interface ClientFramework {
  // The framework adapter id (e.g. "vue", "svelte").
  frameworkId: string;
  // The carrier file extension(s) the framework owns (e.g. the Vue / Svelte
  // single-file-component extensions).
  carrierExtensions: readonly string[];
  // The standalone adapter-module extension(s) (e.g. the Svelte rune-module
  // script extensions; empty for a carrier-only adapter like Vue).
  adapterModuleExtensions: readonly string[];
  // The VS Code client language id(s) the framework attaches to (e.g. ["vue"]).
  clientLanguageIds: readonly string[];
  // The language id(s) whose documents trigger LSP start + TS-plugin configure.
  triggerLanguageIds: readonly string[];
  // The framework's virtual-file naming suffixes (from the descriptor column).
  virtualFileSuffixes: FrameworkVirtualFileSuffixes;
}

"#;

/// Emitted verbatim after the `CLIENT_FRAMEWORKS` rows + the base TS/JS array
/// and BEFORE the flattened derived lists. Empty today (the derived lists carry
/// their own leading comments), kept as the single seam for any future static
/// trailer.
const FOOTER: &str = "";

#[cfg(test)]
mod tests {
    use super::*;
    use verter_language::FrameworkAdapterId;

    #[test]
    fn render_is_deterministic_and_lists_vue_and_svelte() {
        let a = render_client_framework_manifest_ts();
        let b = render_client_framework_manifest_ts();
        assert_eq!(a, b, "render must be deterministic");

        assert!(a.starts_with("// @generated by verter — DO NOT EDIT BY HAND.\n"));
        // Both registered frameworks must surface as manifest rows. The carrier
        // extensions are derived from the registry (`carrier_extensions()`), NOT
        // hardcoded literals here — keeping the single-classifier discipline.
        let registry = LanguageRegistry::built_in();
        let carriers = registry.carrier_extensions();
        let vue_ext = format!(".{}", carriers.iter().find(|e| **e == "vue").unwrap());
        let svelte_ext = format!(".{}", carriers.iter().find(|e| **e == "svelte").unwrap());
        assert!(a.contains("frameworkId: \"vue\""));
        assert!(a.contains("frameworkId: \"svelte\""));
        // Vue's carrier + virtual-file naming column surfaces verbatim.
        assert!(a.contains(&format!("carrierExtensions: [\"{vue_ext}\"]")));
        assert!(a.contains("clientLanguageIds: [\"vue\"]"));
        assert!(a.contains("ide: [\".jsx\", \".tsx\"]"));
        assert!(a.contains("testingApi: \".__verter_test.ts\""));
        // File-watch globs are a SERVER concern, not a client-manifest one — the
        // manifest must carry no watch-glob field on a framework row.
        assert!(!a.contains("fileWatchGlobs"));
        // Svelte's carrier, language-sensitive JS/TS IDE surfaces, null testing
        // surface, and rune-module adapter extensions. JavaScript SFCs project
        // through `.svelte.jsx`; TypeScript SFCs through `.svelte.tsx`.
        assert!(a.contains(&format!("carrierExtensions: [\"{svelte_ext}\"]")));
        let svelte_module_exts = registry.adapter_module_extensions(&FrameworkAdapterId::svelte());
        assert_eq!(svelte_module_exts, vec!["svelte.js", "svelte.ts"]);
        assert!(a.contains(&format!(
            "adapterModuleExtensions: [\"{svelte_ext}.js\", \"{svelte_ext}.ts\"]"
        )));
        assert!(a.contains("ide: [\".jsx\", \".tsx\"]"));
        // Vue has no adapter-module extensions.
        assert!(a.contains("adapterModuleExtensions: []"));
        // The base TS/JS surface is recorded (multi-line, oxfmt-stable). The
        // activation/plugin-configure base carries the React dialects; the LSP
        // document-selector base is plain js/ts only (Vue selector preserved).
        assert!(a.contains("BASE_TYPESCRIPT_LANGUAGE_IDS: readonly string[] = [\n"));
        assert!(a.contains("  \"typescript\",\n"));
        assert!(a.contains("  \"javascript\",\n"));
        assert!(a.contains("  \"typescriptreact\",\n"));
        assert!(a.contains("  \"javascriptreact\",\n"));
        // The LSP document-selector base EXCLUDES the React dialects — it is
        // exactly the plain js/ts pair (preserving the pre-manifest Vue
        // selector surface). Slice out its rendered block and assert the
        // membership precisely.
        let sel_block = {
            let start = a
                .find("export const DOCUMENT_SELECTOR_BASE_LANGUAGE_IDS")
                .expect("DOCUMENT_SELECTOR_BASE_LANGUAGE_IDS present");
            let rest = &a[start..];
            let end = rest.find("];").expect("selector base array closes") + 2;
            &rest[..end]
        };
        assert!(sel_block.contains("\"javascript\""));
        assert!(sel_block.contains("\"typescript\""));
        assert!(
            !sel_block.contains("react"),
            "the LSP document-selector base must not carry the React dialects: {sel_block}"
        );
        // The derived flattened lists are emitted as LITERAL arrays (no runtime
        // `flatMap` — ES6-safe, no ES2019 lib dependency).
        assert!(a.contains("CLIENT_ACTIVATION_LANGUAGE_IDS: readonly string[] = [\n"));
        assert!(a.contains("CLIENT_DOCUMENT_SELECTOR_LANGUAGE_IDS: readonly string[] = [\n"));
        // The framework-language-id list is short enough to render inline.
        assert!(
            a.contains("CLIENT_FRAMEWORK_LANGUAGE_IDS: readonly string[] = [\"vue\", \"svelte\"];")
        );
        // No watch-glob aggregate is emitted — watching is server-owned.
        assert!(!a.contains("CLIENT_FILE_WATCH_GLOBS"));
        // The carrier-extension list is short enough to render inline (oxfmt
        // collapses arrays that fit the print width). Built from the registry.
        assert!(a.contains(&format!(
            "CLIENT_CARRIER_EXTENSIONS: readonly string[] = [\"{vue_ext}\", \"{svelte_ext}\"];"
        )));
        // The activation list contains both the base TS surface and both
        // framework client ids (vue + svelte) — Svelte is no longer opt-in.
        assert!(
            !a.contains("flatMap"),
            "derived lists must be literal arrays, not runtime flatMap"
        );
        assert!(a.contains("  \"vue\",\n"));
        assert!(a.contains("  \"svelte\",\n"));
        // The Svelte rune-module extension surfaces on the row's
        // adapterModuleExtensions, not via any watch-glob aggregate.
        assert!(a.contains(&format!(
            "adapterModuleExtensions: [\"{svelte_ext}.js\", \"{svelte_ext}.ts\"]"
        )));
        assert!(a.ends_with("\n"));
    }

    #[test]
    fn svelte_has_no_testing_surface_and_vue_is_jsx_conditional() {
        let rendered = render_client_framework_manifest_ts();
        // Negative: Svelte never carries the Vue-only testing suffix.
        assert!(!rendered.contains(".svelte.__verter_test"));
        // The Vue IDE surface is JSX-conditional (both suffixes), Svelte is not.
        assert!(rendered.contains("ide: [\".jsx\", \".tsx\"]"));
    }
}
