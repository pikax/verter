#![deny(missing_docs)]
//! Renderer for the generated TypeScript virtual-file naming mirror.
//!
//! The Rust [`FrameworkAdapterDescriptor`] table (the
//! [`VirtualFileNaming`] column) is the SINGLE authority for an adapter's
//! IDE / API / testing-API / sidecar virtual-file suffixes. The committed
//! TypeScript module `packages/typescript-plugin/src/generated/virtual-file-naming.ts`
//! is a GENERATED, BYTE-PINNED mirror of that column — the LSP / ts-plugin
//! naming derivations consume it (the consumer rewiring is a later
//! vertical; this lands the column + the mirror + the freshness pin).
//!
//! [`render_virtual_file_naming_ts`] renders the canonical TS module text
//! from the descriptor rows; the freshness test
//! (`crates/verter_session/tests/.../virtual_file_naming_ts_freshness.rs`)
//! byte-compares the committed file against this render, so a hand-edit or
//! a descriptor-row change without a regen fails the gate.

use crate::framework::descriptor::{
    svelte_descriptor, svelte_rune_module_naming, vue_descriptor, FrameworkAdapterDescriptor,
    VirtualFileNaming, VirtualPathPolicy,
};

/// The committed path of the generated mirror, relative to the workspace
/// root. The freshness test and any regen path reference this one place.
pub const VIRTUAL_FILE_NAMING_TS_PATH: &str =
    "packages/typescript-plugin/src/generated/virtual-file-naming.ts";

/// Every adapter descriptor that carries a virtual-file naming column, in
/// a deterministic order. Adding an adapter row here (and regenerating)
/// is the only way to extend the mirror.
fn descriptors_with_naming() -> Vec<FrameworkAdapterDescriptor> {
    vec![vue_descriptor(), svelte_descriptor()]
}

/// Render the canonical TypeScript mirror module from the descriptor
/// table. The output is deterministic and oxfmt-stable (two-space
/// indent, double quotes, trailing newline) so the byte-pin holds.
#[must_use]
pub fn render_virtual_file_naming_ts() -> String {
    let mut out = String::new();
    out.push_str(HEADER);

    for descriptor in descriptors_with_naming() {
        let Some(naming) = descriptor.virtual_file_naming.as_ref() else {
            continue;
        };
        // The carrier extension (`.vue` / `.svelte`) is `.{carrier_language}`.
        // It is the prefix every virtual-file suffix appends to, so the TS
        // plugin builds its carrier-extension + per-suffix regexes from it.
        let carrier_ext = descriptor
            .carrier_language
            .as_ref()
            .map(|lang| format!(".{}", lang.as_str()));
        render_descriptor_entry(
            &mut out,
            descriptor.tag_const_name(),
            carrier_ext.as_deref(),
            naming,
        );
    }

    // The Svelte standalone rune-module naming — NOT a carrier
    // descriptor row (a rune module is a script). It is keyed by the
    // rune-module language id and carries the carrier extension `.svelte.ts`
    // (the longest-suffix the rune-module classification matches). Its
    // self-file policy tells the TS plugin the module serves its own path.
    render_descriptor_entry(
        &mut out,
        SVELTE_RUNE_MODULE_TS_KEY,
        Some(".svelte.ts"),
        &svelte_rune_module_naming(),
    );

    out.push_str(FOOTER);
    out
}

/// The TS mirror key for the Svelte rune-module naming row (not a wire tag —
/// a rune module is a script, addressed by its language id).
const SVELTE_RUNE_MODULE_TS_KEY: &str = "SVELTE_RUNE_MODULE";

fn render_descriptor_entry(
    out: &mut String,
    tag: &str,
    carrier_ext: Option<&str>,
    naming: &VirtualFileNaming,
) {
    out.push_str(&format!("  {tag}: {{\n"));
    out.push_str(&format!(
        "    carrierExtension: {},\n",
        render_opt_str(carrier_ext)
    ));
    out.push_str(&format!("    ide: {},\n", render_path_policy(&naming.ide)));
    out.push_str(&format!(
        "    importSurface: {},\n",
        render_path_policy(&naming.import_surface)
    ));
    out.push_str(&format!(
        "    testingApiSuffix: {},\n",
        render_opt_str(naming.testing_api_suffix)
    ));
    out.push_str(&format!(
        "    sidecarSuffixes: {},\n",
        render_str_array(naming.sidecar_suffixes)
    ));
    out.push_str("  },\n");
}

fn render_path_policy(policy: &VirtualPathPolicy) -> String {
    match policy {
        VirtualPathPolicy::None => "{ kind: \"none\" }".to_string(),
        VirtualPathPolicy::SelfFile => "{ kind: \"selfFile\" }".to_string(),
        VirtualPathPolicy::Suffix(suffix) => {
            format!("{{ kind: \"suffix\", suffix: {} }}", quote(suffix))
        }
        VirtualPathPolicy::JsxConditional { jsx, non_jsx } => format!(
            "{{ kind: \"jsxConditional\", jsx: {}, nonJsx: {} }}",
            quote(jsx),
            quote(non_jsx)
        ),
    }
}

fn render_opt_str(value: Option<&str>) -> String {
    match value {
        None => "null".to_string(),
        Some(s) => quote(s),
    }
}

fn render_str_array(values: &[&str]) -> String {
    if values.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = values.iter().map(|s| quote(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// Quote a string as a TS double-quoted literal. The suffixes are static
/// adapter constants (`.ts`, `.tsx`, …) with no embedded quotes or
/// backslashes, so a plain wrap is exact.
fn quote(s: &str) -> String {
    format!("\"{s}\"")
}

impl FrameworkAdapterDescriptor {
    /// The TS object-key constant name for this adapter's tag (the wire
    /// tag's `as_str_name`, e.g. `FRAMEWORK_TAG_VUE`), used as the mirror
    /// map key so the generated module is keyed by the closed wire tag.
    fn tag_const_name(&self) -> &'static str {
        self.tag.as_str_name()
    }
}

const HEADER: &str = r#"// @generated by verter — DO NOT EDIT BY HAND.
//
// Mirror of the Rust framework-adapter virtual-file naming column
// (`crates/verter_session/src/framework/descriptor.rs`). The descriptor
// table is the single authority; this file is rendered from it and
// byte-pinned by `virtual_file_naming_ts_freshness`. Regenerate via that
// test's update path after any descriptor-row change.
//
// Keyed by the closed wire `FrameworkTag` name (`as_str_name`) for component
// carriers, plus `SVELTE_RUNE_MODULE` for the standalone rune-module naming.
// Suffix policies append to the FULL carrier canonical (e.g. `App.vue` + `.ts`
// => `App.vue.ts`); a `selfFile` policy means the file serves its own path.

export interface VirtualPathNone {
  kind: "none";
}

export interface VirtualPathSelfFile {
  kind: "selfFile";
}

export interface VirtualPathSuffix {
  kind: "suffix";
  suffix: string;
}

export interface VirtualPathJsxConditional {
  kind: "jsxConditional";
  jsx: string;
  nonJsx: string;
}

export type VirtualPathPolicy =
  | VirtualPathNone
  | VirtualPathSelfFile
  | VirtualPathSuffix
  | VirtualPathJsxConditional;

export interface VirtualFileNaming {
  carrierExtension: string | null;
  ide: VirtualPathPolicy;
  importSurface: VirtualPathPolicy;
  testingApiSuffix: string | null;
  sidecarSuffixes: string[];
}

export const VIRTUAL_FILE_NAMING: Readonly<Record<string, VirtualFileNaming>> = {
"#;

const FOOTER: &str = "};\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_is_deterministic_and_contains_the_vue_row() {
        let a = render_virtual_file_naming_ts();
        let b = render_virtual_file_naming_ts();
        assert_eq!(a, b, "render must be deterministic");

        // The Vue row's live column must surface verbatim.
        assert!(a.contains("FRAMEWORK_TAG_VUE: {"));
        assert!(a.contains("carrierExtension: \".vue\""));
        assert!(a.contains("carrierExtension: \".svelte\""));
        assert!(a.contains("kind: \"jsxConditional\", jsx: \".jsx\", nonJsx: \".tsx\""));
        // The import surface is the reserved `.verter.ts` API file (suffix
        // policy) — the redirect-reached infix, never a bare `.ts`.
        assert!(a.contains("importSurface: { kind: \"suffix\", suffix: \".verter.ts\" }"));
        assert!(a.contains("testingApiSuffix: \".__verter_test.ts\""));
        assert!(a.contains("sidecarSuffixes: []"));
        // The Svelte COMPONENT row renders its fixed `.tsx` IDE suffix policy,
        // the `.verter.ts` import surface, and a NULL testing surface.
        assert!(a.contains("FRAMEWORK_TAG_SVELTE: {"));
        assert!(a.contains("ide: { kind: \"suffix\", suffix: \".tsx\" }"));
        // The Svelte row's testing surface is null (no `.svelte.__verter_test`).
        assert!(!a.contains(".svelte.__verter_test"));
        // The rune-module row: same-file model (selfFile/selfFile),
        // carrier extension `.svelte.ts`, no testing surface.
        assert!(a.contains("SVELTE_RUNE_MODULE: {"));
        assert!(a.contains("carrierExtension: \".svelte.ts\""));
        assert!(a.contains("ide: { kind: \"selfFile\" }"));
        assert!(a.contains("importSurface: { kind: \"selfFile\" }"));
        // No descriptor without naming leaks an empty entry.
        assert!(a.ends_with("};\n"));
    }

    #[test]
    fn render_starts_with_the_do_not_edit_banner() {
        let rendered = render_virtual_file_naming_ts();
        assert!(rendered.starts_with("// @generated by verter — DO NOT EDIT BY HAND.\n"));
    }
}
