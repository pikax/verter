use super::legacy_test_support::{compile, CodegenOptions, VerterCompileOptions};
use super::*;

/// Check if a string contains a `/*digits,digits*/` offset comment pattern.
fn has_offset_comment(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 4 < len {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Found /*, look for digits,digits*/
            let start = i + 2;
            let mut j = start;
            // Skip digits
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start && j < len && bytes[j] == b',' {
                let comma = j;
                j += 1;
                while j < len && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > comma + 1 && j + 1 < len && bytes[j] == b'*' && bytes[j + 1] == b'/' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn compile_sfc(source: &str) -> VerterCompileResult {
    compile_sfc_with_semantics(source, &VueMacroSemanticInput::Unavailable)
}

fn compile_sfc_with_runtime(
    source: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> VerterCompileResult {
    compile_sfc_with_semantics(source, &VueMacroSemanticInput::Runtime(runtime))
}

fn compile_sfc_with_semantics(
    source: &str,
    macro_semantics: &VueMacroSemanticInput,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, macro_semantics, &alloc)
}

fn compile_sfc_no_hoist(source: &str) -> VerterCompileResult {
    compile_sfc_no_hoist_with_semantics(source, &VueMacroSemanticInput::Unavailable)
}

fn compile_sfc_no_hoist_with_semantics(
    source: &str,
    macro_semantics: &VueMacroSemanticInput,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        hoist_static: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, macro_semantics, &alloc)
}

fn compile_sfc_vapor(source: &str) -> VerterCompileResult {
    compile_sfc_vapor_with_semantics(source, &VueMacroSemanticInput::Unavailable)
}

fn compile_sfc_vapor_with_semantics(
    source: &str,
    macro_semantics: &VueMacroSemanticInput,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, macro_semantics, &alloc)
}

fn compile_and_validate_vapor_template(source: &str) -> String {
    let result = compile_sfc_vapor(source);
    validate_vapor_template_result(result)
}

fn compile_and_validate_vapor_template_with_runtime(
    source: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> String {
    let semantics = VueMacroSemanticInput::Runtime(runtime);
    validate_vapor_template_result(compile_sfc_vapor_with_semantics(source, &semantics))
}

fn validate_vapor_template_result(result: VerterCompileResult) -> String {
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Vapor template JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

/// Compile with hoist_static=false and assert template output is syntactically valid JS.
fn compile_and_validate_template_no_hoist(source: &str) -> String {
    let result = compile_sfc_no_hoist(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Template JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

/// Compile and assert template output is syntactically valid JS.
/// Returns the template code string for further assertion.
fn compile_and_validate_template(source: &str) -> String {
    let result = compile_sfc(source);
    validate_template_result(result)
}

fn compile_and_validate_template_with_runtime(
    source: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> String {
    validate_template_result(compile_sfc_with_runtime(source, runtime))
}

fn validate_template_result(result: VerterCompileResult) -> String {
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Parse with OXC to ensure valid JS
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Template JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

// ==================== Cross-file const prop optimization ====================

fn compile_sfc_with_const_props(source: &str, const_props: &[&str]) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Child.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        prop_constness_overrides: Some(const_props.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

fn compile_sfc_vapor_with_const_props(source: &str, const_props: &[&str]) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Child.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        prop_constness_overrides: Some(const_props.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

// ======================== export type hoisting (keep TS) ========================

fn compile_sfc_keep_ts(source: &str) -> VerterCompileResult {
    compile_sfc_keep_ts_with_semantics(source, &VueMacroSemanticInput::Unavailable)
}

fn compile_sfc_keep_ts_with_runtime(
    source: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> VerterCompileResult {
    compile_sfc_keep_ts_with_semantics(source, &VueMacroSemanticInput::Runtime(runtime))
}

fn compile_sfc_keep_ts_with_semantics(
    source: &str,
    macro_semantics: &VueMacroSemanticInput,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, macro_semantics, &alloc)
}

// ── TSX codegen integration tests ─────────────────────────────────

fn compile_tsx(source: &str) -> VerterCompileResult {
    compile_tsx_with_semantics(source, &VueMacroSemanticInput::Unavailable)
}

fn compile_tsx_with_runtime(
    source: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> VerterCompileResult {
    compile_tsx_with_semantics(source, &VueMacroSemanticInput::Runtime(runtime))
}

fn compile_tsx_with_semantics(
    source: &str,
    macro_semantics: &VueMacroSemanticInput,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, macro_semantics, &alloc)
}

fn compile_tsx_with_force_js(source: &str, force_js: bool) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        force_js,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

fn compile_tsx_with_custom_elements(source: &str, prefixes: &[&str]) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        custom_elements: Some(prefixes.iter().map(|p| p.to_string()).collect()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

// ── TSX source map round-trip integration tests ───────────────────

fn compile_tsx_with_source_map(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

fn compile_tsx_with_template_data(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        extract_template_data: true,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

/// Verify that every source map token in the TSX output maps back to valid
/// positions in the original Vue SFC source (no out-of-bounds lines/cols).
fn verify_sourcemap_tokens_in_bounds(source: &str, tsx: &VerterTsxBlock) {
    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");

    let vue_lines: Vec<&str> = source.lines().collect();
    let tsx_lines: Vec<&str> = tsx.code.lines().collect();

    for token in sm.get_tokens() {
        // Only check tokens that reference a source file
        if token.get_source_id().is_none() {
            continue;
        }

        let src_line = token.get_src_line() as usize;
        let src_col = token.get_src_col() as usize;
        let dst_line = token.get_dst_line() as usize;
        let dst_col = token.get_dst_col() as usize;

        // Source position must be in bounds of Vue SFC
        assert!(
            src_line < vue_lines.len(),
            "Source map token points to Vue line {} but SFC only has {} lines.\n\
             TSX gen position: {}:{}\nTSX code:\n{}",
            src_line,
            vue_lines.len(),
            dst_line,
            dst_col,
            tsx.code
        );

        // Generated position must be in bounds of TSX output
        assert!(
            dst_line < tsx_lines.len(),
            "Source map token points to TSX line {} but TSX only has {} lines.\n\
             Vue src position: {}:{}\nTSX code:\n{}",
            dst_line,
            tsx_lines.len(),
            src_line,
            src_col,
            tsx.code
        );
    }
}

// ── Source map position mapping regression tests ──────────────────

/// Helper: find `needle` in `haystack` and return its 0-indexed (line, col).
/// Panics if `needle` is not found.
fn find_line_col(haystack: &str, needle: &str) -> (u32, u32) {
    let offset = haystack
        .find(needle)
        .unwrap_or_else(|| panic!("'{}' not found in text", needle));
    let line = haystack[..offset].matches('\n').count() as u32;
    let col = (offset - haystack[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0)) as u32;
    (line, col)
}

fn find_line_col_at(haystack: &str, offset: usize) -> (u32, u32) {
    let line = haystack[..offset].matches('\n').count() as u32;
    let col = (offset - haystack[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0)) as u32;
    (line, col)
}

// ==================== Static subtree hoisting ====================

/// Compile with hoist_static=true (default) and validate JS output.
fn compile_and_validate_hoisted(source: &str) -> String {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        hoist_static: Some(true),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Parse with OXC to ensure valid JS
    let parse_alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&parse_alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Template JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

/// Compile with hoist_static=false and validate JS output.
fn compile_and_validate_no_hoist(source: &str) -> String {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        hoist_static: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Parse with OXC to ensure valid JS
    let parse_alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&parse_alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Template JS parse error (no-hoist): {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

// ── TSX OXC parse-validity tests ─────────────────────────────────────
// These compile a full Vue SFC to TSX and then parse the entire output
// with OXC to verify it is syntactically valid TypeScript/TSX.

/// Compile a Vue SFC to TSX and assert the output parses without errors.
fn assert_tsx_parses(source: &str, label: &str) {
    assert_tsx_result_parses(compile_tsx(source), label);
}

fn assert_tsx_parses_with_runtime(
    source: &str,
    label: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) {
    assert_tsx_result_parses(compile_tsx_with_runtime(source, runtime), label);
}

fn assert_tsx_result_parses(result: VerterCompileResult, label: &str) {
    assert!(
        result.errors.is_empty(),
        "[{}] compile errors: {:?}",
        label,
        result.errors
    );
    let tsx = result
        .tsx
        .as_ref()
        .unwrap_or_else(|| panic!("[{}] no tsx block", label));
    let alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "[{}] OXC parse errors: {:?}\n--- TSX output ---\n{}",
        label,
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
}

/// Verify that a JS SFC (no lang attribute) produces valid JSX (JavaScript) output,
/// with `is_jsx: true` on the tsx block, and no TypeScript syntax.
fn assert_jsx_parses(source: &str, label: &str) {
    let result = compile_tsx(source);
    assert!(
        result.errors.is_empty(),
        "[{}] compile errors: {:?}",
        label,
        result.errors
    );
    let tsx = result
        .tsx
        .as_ref()
        .unwrap_or_else(|| panic!("[{}] no tsx block", label));
    // Positive: JS SFC must set is_jsx = true
    assert!(tsx.is_jsx, "[{}] is_jsx should be true for JS SFC", label);
    // Verify the output is valid JSX (JavaScript) — no TypeScript parse errors
    let alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::jsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "[{}] OXC parse errors (should be valid JS):\n{:?}\n--- JSX output ---\n{}",
        label,
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
    // Negative: no TypeScript syntax in output
    assert!(
        !tsx.code.contains("as unknown"),
        "[{}] JSX output must not contain 'as unknown':\n{}",
        label,
        tsx.code
    );
    assert!(
        !tsx.code.contains("import type"),
        "[{}] JSX output must not contain 'import type':\n{}",
        label,
        tsx.code
    );
    assert!(
        !tsx.code.contains("declare let"),
        "[{}] JSX output must not contain 'declare let':\n{}",
        label,
        tsx.code
    );
    assert!(
        !tsx.code.contains("!:"),
        "[{}] JSX output must not contain definite assignment '!:':\n{}",
        label,
        tsx.code
    );
}

// ── Strict slot children integration tests (full SFC) ─────────────

fn compile_tsx_strict_slots(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        strict_slots: true,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

// ── Slot-summary representative SFCs (shared by counter + byte tests) ──

const SLOT_SFC_NESTED: &str = r#"<script setup lang="ts">
import Card from './Card.vue'
import Panel from './Panel.vue'
import Row from './Row.vue'
</script>
<template>
  <Card>
    <template #header="{ title }">
      <Row>{{ title }}</Row>
    </template>
    <template #default>
      <Panel />
      leading text
    </template>
  </Card>
</template>"#;

const SLOT_SFC_NO_SLOT: &str = r#"<script setup lang="ts">
const msg = 'hi'
</script>
<template>
  <div class="a"><span>{{ msg }}</span></div>
</template>"#;

const SLOT_SFC_DEEP: &str = r#"<script setup lang="ts">
import Outer from './Outer.vue'
import Inner from './Inner.vue'
import Leaf from './Leaf.vue'
</script>
<template>
  <div>
    <section>
      <Outer>
        <template #body>
          <Inner>
            <Leaf />
          </Inner>
        </template>
      </Outer>
    </section>
  </div>
</template>"#;

/// Normalize line endings before a byte comparison. The repo is LF-only, but
/// the cross-platform rule requires normalizing checked-out text before a
/// byte-equality assertion.
fn norm_eol(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Source-map JSON is a single logical record. Git/checkouts may add one final
/// line terminator to the fixture file, while the compiler API intentionally
/// returns the JSON record without it. Remove exactly one fixture terminator;
/// embedded/trailing blank lines still fail the byte comparison.
fn norm_map_golden_eol(s: &str) -> String {
    let mut normalized = norm_eol(s);
    if normalized.ends_with('\n') {
        normalized.pop();
    }
    normalized
}

/// Assert the compiled TSX's `code` AND `source_map` both equal their committed
/// goldens (byte-for-byte after EOL normalization). These fixtures pin the
/// current public IDE-carrier contract, including Vue slot-body isolation and
/// the mapped empty-pair closing tags used for navigation.
fn assert_tsx_code_and_map_match(tsx: &VerterTsxBlock, code_golden: &str, map_golden: &str) {
    assert_eq!(
        norm_eol(&tsx.code),
        norm_eol(code_golden),
        "emitted TSX code must be byte-identical to its golden"
    );
    assert!(
        !tsx.source_map.is_empty(),
        "source maps are enabled, so a source map must be emitted"
    );
    assert_eq!(
        norm_eol(&tsx.source_map),
        norm_map_golden_eol(map_golden),
        "emitted TSX source map must be byte-identical to its golden"
    );
}

// ── Shared read-only template-expression overlay ──────────────────────
//
// A TS SFC compiled for a combined runtime + TSX target parses its template
// expressions once per `ide_completion` value: the runtime lane (`false`) and
// the IDE/TSX lane (`true`) record different binding facts, so each keeps its
// own overlay entry while reusing it across every consumer in that lane.
// A JS SFC must NOT share across source types either: its TSX lane parses with
// `jsx()` while the runtime lane parses with `tsx()`, so the two never reuse
// one overlay.

use crate::template::oxc::{
    parse_template_expressions_call_count, parse_template_expressions_source_types,
    reset_parse_template_expressions_calls,
};

fn compile_with_target(source: &str, target: CompileTarget, force_js: bool) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

const TS_OVERLAY_SFC: &str = r#"<script setup lang="ts">
const msg: string = 'hello'
const count: number = 1
</script>

<template>
  <div :class="msg" v-if="count > 0">{{ msg }}{{ count }}</div>
  <span v-for="(item, i) in [count]" :key="i">{{ item }}</span>
</template>
"#;

const JS_OVERLAY_SFC: &str = r#"<script setup>
const msg = 'hello'
const count = 1
</script>

<template>
  <div :class="msg" v-if="count > 0">{{ msg }}{{ count }}</div>
  <span v-for="(item, i) in [count]" :key="i">{{ item }}</span>
</template>
"#;

// A TS SFC whose only interpolation `{{ it }}` is a PARTIAL prefix of the
// `v-for` scope local `item`. This is the exact case where the two lanes
// diverge: the runtime lane (`ide_completion = false`) treats `it` as a real
// reference, while the IDE/TSX lane (`ide_completion = true`) keeps it bare for
// scoped completion. A shared overlay keyed without `ide_completion` would hand
// both lanes one set of binding facts and corrupt one of the two outputs.
const TS_OVERLAY_SCOPED_COMPLETION_SFC: &str = r#"<script setup lang="ts">
const items = [1]
</script>

<template>
  <span v-for="item in items">{{ it }}</span>
</template>
"#;

// A minimal TS SFC whose runtime and TSX outputs are pinned byte-for-byte
// below. It exercises the shared overlay (a bound attribute `:title="msg"` and
// an interpolation `{{ msg }}` both parse through it) while keeping the pinned
// output small enough to review.
const TS_OVERLAY_GOLDEN_SFC: &str = r#"<script setup lang="ts">
const msg = 'hi'
</script>

<template>
  <p :title="msg">{{ msg }}</p>
</template>
"#;

// The exact runtime render-function output for `TS_OVERLAY_GOLDEN_SFC`. Pinning
// the absolute bytes (not just combined-equals-pure) proves the shared overlay
// path emits the same output an independent parse would, anchoring the relative
// byte-identity checks against a known-good baseline.
const TS_OVERLAY_GOLDEN_RUNTIME: &str = r#"const _hoisted_1 = ["title"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
return (_openBlock(), _createElementBlock("p", { title: $setup.msg }, _toDisplayString( $setup.msg ), 9 /* TEXT, PROPS */, _hoisted_1)
)
}"#;

// The exact TSX output for `TS_OVERLAY_GOLDEN_SFC` (combined target). The
// destructure line ends with a trailing space in the real output; it is spliced
// in as the explicit `" \n"` segment so no source line carries (invisible,
// strip-prone) trailing whitespace while the golden still pins the exact bytes.
const TS_OVERLAY_GOLDEN_TSX: &str = concat!(
    r#"/** @jsxImportSource vue */
import type { Prettify as ___VERTER___Prettify, ExtractComponentProps as ___VERTER___ExtractComponentProps, ExtractLeafElement as ___VERTER___ExtractLeafElement, GlobalComponentType as ___VERTER___GlobalComponentType, GlobalComponentKebabType as ___VERTER___GlobalComponentKebabType } from "@verter/types";
import { shallowUnwrapRef as ___VERTER___shallowUnwrapRef, enhanceElementWithProps as ___VERTER___enhanceElementWithProps, extractRenderComponent as ___VERTER___extractRenderComponent, instantiateComponent as ___VERTER___instantiateComponent, componentConstructor as ___VERTER___componentConstructor, extractArgumentsFromRenderSlot as ___VERTER___extractArgumentsFromRenderSlot, runCustomDirective as ___VERTER___runCustomDirective, retrieveSetupDirectives as ___VERTER___retrieveSetupDirectives, strictRenderSlot as ___VERTER___strictRenderSlot, checkRequiredSlots as ___VERTER___checkRequiredSlots, globalComponentsNav as ___VERTER___globalComponentsNav } from "@verter/types";
;export function ___VERTER___TemplateBindingFN() {

const msg = 'hi'

let ___VERTER___instance!: Omit<InstanceType<typeof import("./App.vue.verter.js")['default']>, '$attrs'> & { $attrs: ___VERTER___Attrs };
void ___VERTER___instance;
const ___VERTER___directiveAccessor = ___VERTER___retrieveSetupDirectives(___VERTER___instance);
void ___VERTER___directiveAccessor;

const ___VERTER___unwrapped = ___VERTER___shallowUnwrapRef({
    msg: msg as unknown as typeof msg
  });
{ /* verter-destructured-start */const {"#,
    " \n",
    r#"    msg } = ___VERTER___unwrapped; /* verter-destructured-end */
<>
  <><p title={msg}></p>{ msg }</>
</>
} // close block scope

function ___VERTER___Comp66() {
  return {} as HTMLElementTagNameMap["p"];
}
function ___VERTER___getRootComponent() { return ___VERTER___Comp66(); }
function ___VERTER___getRootComponentPassedProps() { return {"title": msg}; }
type ___VERTER___RootElement = ReturnType<typeof ___VERTER___getRootComponent>;
type ___VERTER___RootElementProps = ___VERTER___Prettify<Omit<
  ___VERTER___ExtractComponentProps<___VERTER___RootElement>,
  keyof ReturnType<typeof ___VERTER___getRootComponentPassedProps>
>>;

type ___VERTER___Attrs = ___VERTER___attributes & ___VERTER___RootElementProps;

void ___VERTER___getRootComponent; void ___VERTER___getRootComponentPassedProps;
void ___VERTER___Comp66;
void (___VERTER___instance).valueOf;

return {};
} // close templateBindingFN

export { default } from "./App.vue.verter.js";

type ___VERTER___attributes = {};
"#
);

// A TS SFC with a deliberately malformed interpolation (`{{ count + }}` — a
// binary expression missing its right operand). The interpolation tokenizes
// cleanly at the SFC level (so template codegen still runs), but OXC fails to
// parse the inner expression, surfacing an `XInvalidExpression` warning.
const TS_OVERLAY_MALFORMED_SFC: &str = r#"<script setup lang="ts">
const count: number = 1
</script>

<template>
  <div>{{ count + }}</div>
</template>
"#;

// ── Spread-path component event typing: end-to-end through compile() ────────
//
// These exercise the FULL pipeline — script generation builds the shared component
// inventory (local bindings + GlobalComponents fallback consts) and threads it through
// `IdeScriptGenResult` into template generation, where the `@event` spread path consumes
// it. The unit tests in `ide/template/tests.rs` supply the inventory manually; these
// prove it is actually produced and consumed across the script/template boundary.
//
// `result.errors.is_empty()` here asserts the Rust-side compile diagnostic set is empty
// (the generated TSX is syntactically well-formed); the `tsx.code.contains` checks assert
// the byte-exact formula string the codegen must emit. This is the string-exact codegen
// contract — NOT a tsgo type-check of that formula resolving to the payload.

/// Assert the generated TSX is free of every spread-event-typing antipattern the shared
/// component-binding inventory exists to prevent. Every spread-event compile() test calls
/// this so the negative contract is uniform and discriminating — it FAILS if codegen
/// regresses to an untyped `$event`, the intrinsic-attribute surface, the retired
/// `eventCallbacks` helper, or the tsgo-unresolvable `GlobalComponents[...]` indexed event
/// type.
fn assert_no_spread_event_antipatterns(code: &str) {
    assert!(
        !code.contains("$event: any"),
        "spread $event must be precisely typed, never `any`: {code}"
    );
    assert!(
        !code.contains("IntrinsicElementAttributes"),
        "spread event typing must not fall back to the IntrinsicElementAttributes surface: {code}"
    );
    assert!(
        !code.contains("___VERTER___eventCallbacks"),
        "spread event typing must not use the retired eventCallbacks helper: {code}"
    );
    assert!(
        !code.contains("GlobalComponents["),
        "spread event typing must not use the tsgo-unresolvable GlobalComponents[...] indexed event type: {code}"
    );
}

// ── ISSUE-7: unused `<script setup>` local → TS6133 (full compile path) ─────
//
// End-to-end through `compile_tsx`, which computes `template_used_vars` in the
// compile pipeline and plumbs it into IDE codegen. A binding used NOWHERE must
// be OMITTED from the `___VERTER___unwrapped` object + destructure block (so its
// SOURCE decl is its sole occurrence and TS6133 lands on the MAPPED declaration,
// not an unmapped destructure copy that collapses to line 1); a binding used in
// the template must keep its value read.

struct CompiledIdentifierFacts<'name> {
    name: &'name str,
    bindings: usize,
    references: usize,
}

impl<'a> oxc_ast_visit::Visit<'a> for CompiledIdentifierFacts<'_> {
    fn visit_binding_identifier(&mut self, identifier: &oxc_ast::ast::BindingIdentifier<'a>) {
        if identifier.name == self.name {
            self.bindings += 1;
        }
        oxc_ast_visit::walk::walk_binding_identifier(self, identifier);
    }

    fn visit_identifier_reference(&mut self, identifier: &oxc_ast::ast::IdentifierReference<'a>) {
        if identifier.name == self.name {
            self.references += 1;
        }
        oxc_ast_visit::walk::walk_identifier_reference(self, identifier);
    }
}

fn compiled_tsx_identifier_facts_with_runtime<'name>(
    source: &str,
    name: &'name str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> CompiledIdentifierFacts<'name> {
    use oxc_ast_visit::Visit;

    let result = compile_tsx_with_runtime(source, runtime);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = &result.tsx.as_ref().expect("tsx block").code;
    let allocator = Allocator::new();
    let parsed = verter_parser::oxc_parse::Parser::new(&allocator, code, SourceType::tsx()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "generated IDE carrier must remain valid TSX: {:?}\n---\n{code}",
        parsed
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    let mut facts = CompiledIdentifierFacts {
        name,
        bindings: 0,
        references: 0,
    };
    facts.visit_program(&parsed.program);
    facts
}

// =========================================================================
// Inline-template (official production topology)
// =========================================================================
//
// Official `@vue/compiler-sfc` `compileScript({ inlineTemplate: true })`
// inlines the render function into `setup()` as a returned closure that
// references setup bindings DIRECTLY (no `$setup.` prefix, `.value` unwrap
// for refs). Hoisted statics live at module scope. This is the production
// default (`resolve_inline` = `inline.unwrap_or(is_production)`); VDOM-only
// (Vapor inline is deferred).

fn compile_sfc_inline(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        inline: Some(true),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    compile(
        source,
        &options,
        &verter_opts,
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    )
}

// =========================================================================
// Companion default export + defineOptions merging (official 3.6.0-rc.5)
// =========================================================================
//
// Official `@vue/compiler-sfc` non-inline gates on `defaultExport ||
// definedOptions` PRESENCE — the companion default export (ANY expression)
// is rebound to `const __default__ = <expr>` and merged, never dropped:
// - JS: `Object.assign(__default__, <definedOptions>?, { <runtime> })`
// - TS: `_defineComponent({ ...__default__, ...<definedOptions>?, <runtime> })`

/// Compiles an SFC and returns the script block code, asserting no errors
/// and valid-JS output.
fn compile_sfc_script_code(source: &str) -> String {
    let result = compile_sfc(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &script.code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "output must be valid JS: {:?}\n---\n{}",
        parsed.diagnostics,
        script.code
    );
    script.code.clone()
}

// =========================================================================
// D1 — defineOptions() must not reference setup-local bindings
// =========================================================================
//
// Official `@vue/compiler-sfc` (3.6.0-rc.5) emits a compile ERROR when a
// `defineOptions()` argument references a locally declared (setup) variable,
// because the argument is hoisted outside `setup()`:
//   "`defineOptions()` in <script setup> cannot reference locally declared
//    variables because it will be hoisted outside of the setup() function.
//    If your component options require initialization in the module scope,
//    use a separate normal <script> to export the options instead."
// Literal-const bindings and imports stay valid.

const D1_OFFICIAL_MESSAGE: &str = "in <script setup> cannot reference locally declared variables";

// =========================================================================
// The statement descent is EXHAUSTIVE, and descending into a statement is not
// the same as descending into all of its PARTS. A `v-on` value is a full
// statement list, so every statement form and every identifier position inside
// it resolves — a partially-resolved statement is worse than an unresolved
// one, because the bare half is a write to a setup-scope `const`.
//
// Every expectation below is the byte-for-byte handler body that
// `@vue/compiler-sfc` 3.6.0-rc.5 emits for the same SFC.
// =========================================================================

/// Compile in inline mode and assert the emitted script is parseable JS,
/// returning it. Inline is where an unresolved WRITE becomes a build failure:
/// setup bindings are lexical `const`s, so a bare assignment is a
/// const-reassignment that rolldown rejects (`ILLEGAL_REASSIGNMENT`).
fn compile_and_validate_inline_script(source: &str) -> String {
    let result = compile_sfc_inline(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let code = result.script.as_ref().expect("script block").code.clone();
    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &code, oxc_span::SourceType::mjs()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Inline script JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        code
    );
    code
}

// =========================================================================
// A `for…in` / `for…of` head whose target is NOT a declaration.
//
// `for (const y of xs)` DECLARES `y`, so the loop variable is local. Drop the
// keyword and the same position becomes an assignment target: `for (x of xs)`
// WRITES to the existing binding `x` once per iteration, so the target, the
// iterated expression and the body are all real references that must resolve.
//
// Every `$setup` expectation below is the byte-for-byte handler body that
// `@vue/compiler-sfc` 3.6.0-rc.5 emits for the same SFC (with the one documented
// divergence called out in the destructuring case).
// =========================================================================

/// Shared fixture for the non-declaration loop-target cases: every identifier a
/// head can mention is a genuine setup binding, so anything left bare in the
/// output is an unresolved reference.
fn loop_target_sfc(head: &str) -> String {
    format!(
        r#"<script setup>
import {{ ref }} from 'vue'
const x = ref(0)
const a = ref(0)
const b = ref(0)
const obj = ref({{}})
const xs = ref([])
const log = (v) => v
</script>
<template><button @click="{head}">x</button></template>"#
    )
}

#[path = "compile_tests/directives.rs"]
mod directives;
#[path = "compile_tests/events.rs"]
mod events;
#[path = "compile_tests/general.rs"]
mod general;
#[path = "compile_tests/models.rs"]
mod models;
#[path = "compile_tests/scripts.rs"]
mod scripts;
#[path = "compile_tests/slots.rs"]
mod slots;
#[path = "compile_tests/sourcemaps.rs"]
mod sourcemaps;
#[path = "compile_tests/styles.rs"]
mod styles;
#[path = "compile_tests/targets.rs"]
mod targets;
#[path = "compile_tests/templates.rs"]
mod templates;
use directives::{
    comment_between_v_if_branches_does_not_leak_in_prod, destructured_prop_in_v_bind,
    multiple_v_if_chains_in_same_parent, nested_v_if_chains_no_overlap,
    template_v_for_with_v_if_children_renders_as_fragment, template_v_if_renders_as_fragment,
    tsx_component_with_v_if_and_v_for_preserves_component_tags,
    tsx_parent_v_if_with_child_v_for_contains_outer_condition,
    tsx_v_for_with_v_if_combination_contains_condition_and_map,
    v_if_after_sibling_has_comma_separator, v_if_as_root_single_child, v_if_chain_after_sibling,
    v_if_chain_without_v_else_after_sibling, v_if_else_chain_with_whitespace_valid_output,
    v_if_followed_by_sibling_valid_js, v_if_in_multi_root_fragment,
    v_if_inside_v_for_with_whitespace, v_if_nested_inside_v_for, v_if_v_else_as_root,
    v_if_v_else_if_v_else_complete_chain, v_if_v_else_no_comment_fallback,
    v_if_with_comment_between_branches, v_if_with_whitespace_between_branches,
};
use events::{
    destructured_prop_in_event_handler, duplicate_event_handlers_same_event_merged_into_array,
    dynamic_event_names_not_merged, event_modifier_capture_goes_into_key,
    event_modifier_empty_handler_with_prevent, event_modifier_keyup_enter_uses_with_keys,
    event_modifier_on_component_generates_import, event_modifier_once_goes_into_key,
    event_modifier_passive_goes_into_key, event_modifier_prevent_only_no_value,
    event_modifier_prevent_uses_with_modifiers, event_modifier_stop_prevent_combined,
    key_modifiers_same_event_merged, mixed_duplicate_and_unique_events,
    multiple_event_handlers_same_event_merged_into_array, optional_tuple_element_in_define_emits,
    single_event_handler_no_merge, template_only_scoped_style_emits_scope_id_in_script,
    tsx_infer_function_component_events_from_imported_components,
    tsx_template_tag_empty_template_emits_empty_fragment,
    type_based_define_emits_call_signature_generates_emits_option,
    type_based_define_emits_generates_emits_option, v_if_only_emits_comment_fallback,
    v_if_standalone_emits_comment_vnode, v_if_v_else_if_no_v_else_emits_comment_fallback,
    v_on_and_v_bind_on_same_event_merged,
};
use general::{
    analysis_panel_regression_valid_js, bare_type_and_interface_stripped_when_force_js,
    basic_sfc_compiles, custom_blocks_extracted, data_and_aria_attributes_not_camelized,
    different_option_modifiers_produce_different_keys, export_interface_stripped_when_force_js,
    export_type_hoisted_when_keep_ts, export_type_stripped_when_force_js,
    handler_with_mixed_key_and_runtime_modifiers_merged, html_entities_in_bind_value_decoded,
    html_entity_copy_decoded, literal_boolean_in_bind_no_ctx_prefix,
    mouse_left_right_as_runtime_modifiers_merged, ts_return_type_annotation_in_computed,
    ts_return_type_no_strip_mode, tsx_basic_sfc,
    tsx_binding_type_assertions_do_not_prefix_type_members, tsx_binding_v5_process_parity_matrix,
    tsx_force_js_toggle_does_not_change_code, with_defaults_cross_block_type_uses_key_name,
    with_defaults_resolvable_type_still_works, with_defaults_type_reference,
    with_defaults_unresolvable_type_no_defaults,
};
use models::{
    define_model_declares_prop_and_emit, define_model_named_declares_prop_and_emit,
    define_model_with_authoritative_defaults_runtime_variable,
    define_model_with_defaults_resolved_type,
    define_model_with_define_emits_uses_merge_models_for_emits,
    define_model_with_define_props_object_uses_merge_models, define_model_with_typed_with_defaults,
    v_model_named_on_component, v_model_named_with_explicit_update_handler_merges_into_array,
    v_model_on_checkbox_generates_with_directives, v_model_on_component_expands_to_props,
    v_model_on_dynamic_type_input_uses_dynamic, v_model_on_input_with_trim_modifier,
    v_model_on_native_input_generates_with_directives, v_model_on_radio_generates_with_directives,
    v_model_on_select_generates_with_directives, v_model_on_textarea_generates_with_directives,
    v_model_on_unresolved_component, v_model_with_explicit_update_handler_merges_into_array,
};
use scripts::{
    builtin_component_in_imports_list, companion_script_import_available_in_template,
    companion_script_import_used_in_template_in_returned,
    companion_script_type_only_import_not_in_returned, component_is_self_closing_with_props,
    component_is_with_prop_binding_and_vbind,
    component_kebab_case_resolves_to_pascal_setup_binding, component_resolves_to_setup_binding,
    dual_script_export_default_merged_as_options, import_used_in_template_should_be_in_returned,
    imported_component_uses_setup_binding_not_resolve_component,
    imported_function_in_template_gets_setup_prefix, setup_returns_bindings_for_template_refs,
    setup_returns_bindings_with_define_props, static_and_dynamic_class_merged_into_single_prop,
    tsx_template_ref_options_api_setup_function_is_supported,
};
use sourcemaps::{tsx_force_js_toggle_does_not_change_source_map, tsx_source_map_is_generated};
use styles::{
    static_style_compiled_to_object, static_style_multiple_properties, style_block_extracted,
    template_heavy_vue_full_css_scoping, template_only_no_scoped_style_no_script_block,
    template_only_scoped_style_css_is_scoped,
    template_only_scoped_style_grid_layout_scope_id_consistency,
};
use templates::test_vbind_template_literal_with_html_entities;
