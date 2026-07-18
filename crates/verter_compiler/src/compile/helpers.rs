//! Utility functions for the compilation pipeline.
//!
//! Small helpers that compute derived values (hashes, component names, scope IDs)
//! and convert between internal and public types.

use sha2::{Digest, Sha256};

use crate::diagnostics::{Diagnostic, DiagnosticSeverity};
use crate::parser::types::ParsedSfc;

use super::types::{CompileDiagnostic, CompileDiagnosticSeverity};

/// Synthetic script block for a completely EMPTY SFC — one with no script,
/// no template, no styles, and no custom blocks (e.g. a freshly created
/// file). Such a file is a valid Vue component: an EMPTY one. The emitted
/// shell (`defineComponent({ __name })` + `export default`) keeps the file
/// exporting a component with an empty public surface ($props: {}, no
/// slots) instead of producing no runtime output at all — which the host
/// would surface as a missing virtual node. Returns `None` when the SFC has
/// any block (the caller's other branches own those shapes).
///
/// Both interpolated strings are emitted JS-escaped in double-quoted
/// literals (a filename stem may contain quotes or backslashes).
pub(super) fn empty_sfc_script_block(
    parsed: &ParsedSfc,
    custom_blocks: &[super::types::VerterCustomBlock],
    component_name: &str,
    runtime_module: &str,
    duration_ms: f64,
) -> Option<super::types::VerterScriptBlock> {
    use crate::template::code_gen::shared::helpers::escape_js_string_into;
    if parsed.template_ast().is_some()
        || !parsed.style_nodes().is_empty()
        || !custom_blocks.is_empty()
    {
        return None;
    }
    let mut code = String::with_capacity(160 + runtime_module.len());
    code.push_str("import { defineComponent as _defineComponent } from \"");
    escape_js_string_into(&mut code, runtime_module);
    code.push_str("\";\nconst __sfc__ = /*@__PURE__*/_defineComponent({\n  __name: \"");
    escape_js_string_into(&mut code, component_name);
    code.push_str("\",\n});\nexport default __sfc__;\n");
    Some(super::types::VerterScriptBlock {
        code,
        duration_ms,
        source_map: String::new(),
        setup: false,
        attrs: Vec::new(),
    })
}

/// Remove inter-block gaps from the code transform.
///
/// SFC source may contain content between root-level blocks (e.g., HTML comments,
/// whitespace). This content is not valid JS and must be blanked out.
pub(crate) fn remove_inter_block_gaps(
    code_transform: &mut crate::code_transform::CodeTransform,
    input_len: u32,
    ranges: &[(u32, u32)],
) {
    if ranges.is_empty() {
        // A block-less SFC (empty / whitespace-only / comments-only): the
        // entire input is one inter-block gap. Remove it all so stray
        // top-level content (e.g. HTML comments) never leaks into generated
        // module output.
        if input_len > 0 {
            code_transform.remove(0, input_len);
        }
        return;
    }

    // Remove gap before first block
    if ranges[0].0 > 0 {
        code_transform.remove(0, ranges[0].0);
    }

    // Remove gaps between consecutive blocks
    for i in 0..ranges.len() - 1 {
        let gap_start = ranges[i].1;
        let gap_end = ranges[i + 1].0;
        if gap_start < gap_end {
            code_transform.remove(gap_start, gap_end);
        }
    }

    // Remove gap after last block
    let last_end = ranges[ranges.len() - 1].1;
    if last_end < input_len {
        code_transform.remove(last_end, input_len);
    }
}

/// SHA-256 hash → 8 hex chars (first 4 bytes).
///
/// Used for scope IDs (`data-v-{hash}`) and component IDs. SHA-256 is chosen
/// for compatibility with `@vue/compiler-sfc` which uses the same algorithm.
/// See `verter_session::hash` module docs for the full hash algorithm rationale.
pub fn get_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..4])
}

/// Extract component name from a filename.
pub fn extract_component_name(filename: &str) -> String {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let name = name.strip_suffix(".vue").unwrap_or(name);
    let name = name.strip_suffix(".ts").unwrap_or(name);
    let name = name.strip_suffix(".js").unwrap_or(name);
    name.to_string()
}

/// Compute scope_id as 8 hex chars from component name.
pub(crate) fn compute_scope_id(component_name: &str) -> [u8; 8] {
    let hash = get_hash(component_name);
    let hash_bytes = hash.as_bytes();
    let mut scope_id = [0u8; 8];
    scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
    scope_id
}

/// Convert plugin diagnostics to public `CompileDiagnostic` structs.
pub(crate) fn convert_diagnostics(diagnostics: &[Diagnostic]) -> Vec<CompileDiagnostic> {
    diagnostics
        .iter()
        .map(|d| CompileDiagnostic {
            severity: match d.severity {
                DiagnosticSeverity::Error => CompileDiagnosticSeverity::Error,
                DiagnosticSeverity::Warning => CompileDiagnosticSeverity::Warning,
                DiagnosticSeverity::Info => CompileDiagnosticSeverity::Info,
            },
            code: format!("{:?}", d.code),
            message: d.message.clone(),
            span: d.span,
        })
        .collect()
}

/// Format a runtime import specifier for the `import { ... } from "vue"` line.
///
/// Internal helper names use a `_` prefix (e.g., `_defineComponent`), while Vue
/// exports them without the prefix (`defineComponent`). This function produces
/// the `exportName as _localName` form when needed.
pub fn format_import_specifier(name: &str) -> String {
    if let Some(stripped) = name.strip_prefix('_') {
        if stripped.is_empty() {
            name.to_string()
        } else {
            format!("{} as {}", stripped, name)
        }
    } else {
        name.to_string()
    }
}

/// Extract SFC block byte ranges from root nodes for inter-block gap removal.
pub(super) fn extract_block_ranges(parsed: &ParsedSfc, _input: &str) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();

    // Template
    if let Some(ast) = parsed.template_ast() {
        let start = ast.root.tag_open.start;
        let end = ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(ast.root.tag_open.end);
        ranges.push((start, end));
    }

    // Script(s)
    for script in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
    {
        let start = script.tag_open.start;
        let end = script
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(script.tag_open.end);
        ranges.push((start, end));
    }

    // Styles
    for style in parsed.style_nodes() {
        let start = style.tag_open.start;
        let end = style
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style.tag_open.end);
        ranges.push((start, end));
    }

    // Unknown blocks
    for node in parsed.unknown_nodes() {
        let start = node.tag_open.start;
        let end = node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(node.tag_open.end);
        ranges.push((start, end));
    }

    ranges.sort_by_key(|&(s, _)| s);
    ranges
}

/// Extract attribute key-value pairs from `NodeProp` list.
pub(super) fn extract_attrs(
    props: &[crate::types::NodeProp],
    input: &str,
) -> Vec<(String, String)> {
    props
        .iter()
        .filter(|p| !p.is_directive)
        .map(|p| {
            let name = &input[p.start as usize..p.name_end as usize];
            let value = match (p.value_start, p.value_end) {
                (Some(vs), Some(ve)) => input[vs as usize..ve as usize].to_string(),
                _ => String::new(),
            };
            (name.to_string(), value)
        })
        .collect()
}
