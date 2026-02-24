//! TSX script generation.
//!
//! Generates the script portion of TSX output from `<script setup>` and `<script>` blocks.
//! Unlike the normal script codegen (which transforms macros into runtime code), this
//! preserves TypeScript types and macro call syntax for IDE type checking.
//!
//! ## Output structure
//!
//! For `<script setup>`:
//! ```tsx
//! // Hoisted imports
//! import { ref } from 'vue'
//! import type { Props } from './types'
//!
//! // Hoisted type declarations
//! interface Foo { ... }
//!
//! // Component function wrapper
//! function __verter_tsx_<ComponentName>(__props: ..., __ctx: ...) {
//!   // Setup body (macros preserved, bindings extracted)
//!   const count = ref(0)
//!   const props = defineProps<Props>()
//!
//!   return (
//!     // Template JSX goes here (separate block)
//!   )
//! }
//! ```

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use crate::code_transform::CodeTransform;
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::{parse_script, parse_script_with_companion, ScriptItem, ScriptMode};

use super::TsxScriptOptions;

/// Result of TSX script generation (internal, before building string).
pub struct TsxScriptGenResult<'alloc> {
    /// Binding metadata for template TSX generation.
    pub bindings: FxHashMap<&'alloc str, BindingType>,
}

/// Generate TSX script output from script blocks.
///
/// Returns the generated code, source map, and bindings for template generation.
pub fn generate_tsx_script<'alloc>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
) -> TsxScriptGenResult<'alloc> {
    let mut out = CodeGenOutput::new(alloc);
    let mut bindings = FxHashMap::default();

    match (script, script_setup) {
        (_, Some(setup)) => {
            process_tsx_script_setup(
                setup,
                script,
                source,
                &mut out,
                &mut bindings,
                alloc,
                options,
            );
        }
        (Some(normal), None) => {
            process_tsx_script_only(normal, source, &mut out, &mut bindings, alloc, options);
        }
        (None, None) => {
            // No script blocks — emit minimal component wrapper
            emit_minimal_wrapper(&mut out, options, 0);
        }
    }

    // Apply accumulated operations
    out.apply_to(ct);

    TsxScriptGenResult { bindings }
}

// ── Script Setup Processing ───────────────────────────────────────

fn process_tsx_script_setup<'alloc>(
    setup: &RootNodeScript,
    _normal_script: Option<&RootNodeScript>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
) {
    let content_span = match &setup.content {
        Some(span) => span,
        None => {
            // Self-closing <script setup />
            emit_minimal_wrapper(out, options, setup.tag_open.start);
            return;
        }
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];
    let hoist_pos = setup.tag_open.start;

    // Parse with OXC
    let oxc_alloc = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();

    let parse_result = parse_script_with_companion(
        &parser_ret.program,
        ScriptMode::Setup,
        content_start,
        content_str,
        None, // No companion types needed for TSX — we preserve types as-is
    );

    // Hoist imports to file top (before component wrapper)
    for item in &parse_result.items {
        if let ScriptItem::Import(imp) = item {
            let abs_start = content_start + imp.span.start;
            let abs_end = content_start + imp.span.end;

            // Hoist verbatim (keep all imports including type-only)
            let import_text = &source[abs_start as usize..abs_end as usize];
            out.overwrite(abs_start, abs_end, "");
            out.prepend_alloc(hoist_pos, &format!("{}\n", import_text));
        }
    }

    // Hoist type declarations to file top
    for item in &parse_result.items {
        if let ScriptItem::TypeDeclaration(td) = item {
            let abs_start = content_start + td.span.start;
            let abs_end = content_start + td.span.end;

            let td_text = &source[abs_start as usize..abs_end as usize];
            out.overwrite(abs_start, abs_end, "");
            out.prepend_alloc(hoist_pos, &format!("{}\n", td_text));
        }
    }

    // Extract bindings
    for (span, bt) in &parse_result.bindings {
        let name = &content_str[span.start as usize..span.end as usize];
        let alloc_name = alloc.alloc_str(name);
        bindings.insert(alloc_name, *bt);
    }

    // Build component function wrapper opening
    // Replace <script setup> tag with function declaration
    let wrapper_start = format!("function __verter_tsx_{}() {{\n", options.js_component_name,);
    out.overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);

    // Replace </script> tag with closing
    if let Some(tag_close) = &setup.tag_close {
        let mut wrapper_end = String::with_capacity(128);
        wrapper_end.push_str("\nreturn (\n<>");

        // Placeholder — template JSX will be appended by the consumer
        wrapper_end.push_str("</>");
        wrapper_end.push_str("\n)\n}\n");

        out.overwrite(tag_close.start, tag_close.end, &wrapper_end);
    }
}

// ── Script Only (Options API) Processing ──────────────────────────

fn process_tsx_script_only<'alloc>(
    script: &RootNodeScript,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    _alloc: &'alloc Allocator,
    _options: &TsxScriptOptions<'_>,
) {
    let content_span = match &script.content {
        Some(span) => span,
        None => return,
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];

    // Parse with OXC
    let oxc_alloc = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Options,
        content_start,
        content_str,
    );

    // Extract bindings from Options API
    for (span, bt) in &parse_result.bindings {
        let name = &content_str[span.start as usize..span.end as usize];
        let alloc_name = out.alloc_str(name);
        bindings.insert(alloc_name, *bt);
    }

    // Remove script tags, pass content through
    out.overwrite(script.tag_open.start, script.tag_open.end, "");
    if let Some(tag_close) = &script.tag_close {
        // Append export default at end
        let mut close = String::with_capacity(32);
        close.push_str("\nexport default __sfc__;\n");
        out.overwrite(tag_close.start, tag_close.end, &close);
    }

    // Convert `export default` to `const __sfc__ =`
    for item in &parse_result.items {
        if let ScriptItem::DefaultExport(de) = item {
            let abs_start = content_start + de.span.start;
            let export_default_text = "export default";
            let replace_end = abs_start + export_default_text.len() as u32;
            out.overwrite(abs_start, replace_end, "const __sfc__ =");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn emit_minimal_wrapper(out: &mut CodeGenOutput<'_>, options: &TsxScriptOptions<'_>, pos: u32) {
    let wrapper = format!(
        "function __verter_tsx_{}() {{\n  return (<></>\n  )\n}}\n",
        options.js_component_name,
    );
    out.prepend_alloc(pos, &wrapper);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_transform::CodeTransform;

    fn gen_tsx_script(source: &str) -> (String, FxHashMap<String, BindingType>) {
        let alloc = Allocator::new();
        let mut ct = CodeTransform::new(source, &alloc);

        // Parse SFC to extract script blocks
        let bytes = source.as_bytes();
        let mut syntax = crate::parser::Syntax::new(false);
        crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
            syntax.handle(
                &e,
                &crate::diagnostics::SyntaxPluginContext {
                    input: source,
                    bytes,
                    options: &crate::diagnostics::SyntaxPluginOptions::default(),
                    diagnostics: Vec::new(),
                },
            )
        });

        let options = TsxScriptOptions {
            component_name: "App",
            js_component_name: "App",
            scope_id: "data-v-abc123",
            has_scoped_style: false,
            runtime_module_name: "vue",
            is_vapor: false,
        };

        let result = generate_tsx_script(
            syntax.script(),
            syntax.script_setup(),
            source,
            &mut ct,
            &alloc,
            &options,
        );

        // Remove template/style blocks from output
        if let Some(tpl) = syntax.template_ast() {
            let start = tpl.root.tag_open.start;
            let end = tpl
                .root
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(tpl.root.tag_open.end);
            ct.remove(start, end);
        }

        let code = ct.build_string();
        let bindings: FxHashMap<String, BindingType> = result
            .bindings
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        (code, bindings)
    }

    #[test]
    fn basic_script_setup() {
        let (code, bindings) = gen_tsx_script(
            r#"<script setup>
const msg = 'hello'
</script>"#,
        );

        assert!(code.contains("function __verter_tsx_App()"));
        assert!(code.contains("const msg = 'hello'"));
        assert!(bindings.contains_key("msg"));
    }

    #[test]
    fn script_setup_with_imports() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
import { ref } from 'vue'
import type { Foo } from './types'
const count = ref(0)
</script>"#,
        );

        // Imports should be hoisted above the function wrapper
        let fn_pos = code.find("function __verter_tsx_App").unwrap();
        let import_ref_pos = code.find("import { ref } from 'vue'").unwrap();
        let import_type_pos = code.find("import type { Foo } from './types'").unwrap();

        assert!(
            import_ref_pos < fn_pos,
            "Runtime import should be hoisted above function"
        );
        assert!(
            import_type_pos < fn_pos,
            "Type import should be hoisted above function"
        );
    }

    #[test]
    fn script_setup_with_type_declarations() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
interface Props {
  msg: string
}
const msg = 'hello'
</script>"#,
        );

        // Type declaration should be hoisted
        let fn_pos = code.find("function __verter_tsx_App").unwrap();
        let interface_pos = code.find("interface Props").unwrap();
        assert!(
            interface_pos < fn_pos,
            "Interface should be hoisted above function"
        );
    }

    #[test]
    fn script_setup_preserves_macros() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>"#,
        );

        // Macros should be preserved in the body (not transformed)
        assert!(code.contains("defineProps"));
    }

    #[test]
    fn script_setup_extracts_ref_bindings() {
        let (_, bindings) = gen_tsx_script(
            r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#,
        );

        assert_eq!(
            bindings.get("count").copied(),
            Some(BindingType::SetupRef),
            "ref() binding should be SetupRef"
        );
    }

    #[test]
    fn script_setup_extracts_const_bindings() {
        let (_, bindings) = gen_tsx_script(
            r#"<script setup>
const msg = 'hello'
const fn = () => {}
</script>"#,
        );

        assert!(
            matches!(
                bindings.get("msg").copied(),
                Some(BindingType::SetupConst) | Some(BindingType::LiteralConst)
            ),
            "String constant should be SetupConst or LiteralConst"
        );
    }

    #[test]
    fn options_api_script() {
        let (code, _) = gen_tsx_script(
            r#"<script>
export default {
  data() {
    return { msg: 'hello' }
  }
}
</script>"#,
        );

        assert!(
            code.contains("const __sfc__ ="),
            "export default should be converted to const __sfc__ ="
        );
        assert!(
            code.contains("export default __sfc__"),
            "Should have export default __sfc__ at the end"
        );
    }

    #[test]
    fn no_script_blocks() {
        let (code, _) = gen_tsx_script(
            r#"<template>
  <div>hello</div>
</template>"#,
        );

        assert!(
            code.contains("function __verter_tsx_App()"),
            "Should emit minimal component wrapper"
        );
    }
}
