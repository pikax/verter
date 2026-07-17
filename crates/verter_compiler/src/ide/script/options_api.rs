//! Options-API + dual-script processing (ownership-domain
//! analysis).
//!
//! Hosts:
//! - `process_companion_for_tsx`: integrates a companion `<script>` block
//!   alongside `<script setup>` (dual-script case).
//! - `process_tsx_script_only`: handles SFCs with only an Options-API
//!   `<script>` block (no `<script setup>`).
//! - `extract_component_aliases`: rebuilds template-visible bindings from
//!   `components: { Alias: ImportedComp }` entries.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectPropertyKind, Program, PropertyKey, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use crate::ast::types::TemplateAst;
use crate::code_transform::CodeTransform;
use crate::ide::IdeScriptOptions;
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::{parse_script, DefaultExportType, ScriptItem, ScriptMode};

use super::{
    apply_template_ref_call_inference, collect_binding_names, directive_accessor_declaration,
    emit_helper_imports, emit_helper_imports_with_define_component, emit_type_constructs,
    instance_declaration_ambient, should_infer_function_types,
};

// ── Companion Script Processing ──────────────────────────────────

/// Process a companion `<script>` block for TSX output.
///
/// When both `<script>` and `<script setup>` exist, the companion script
/// needs to be integrated into the TSX output:
/// 1. Remove `<script>` and `</script>` tags (they're not valid TSX)
/// 2. Hoist imports to file top (same as setup imports)
/// 3. Hoist type declarations to file top
/// 4. Remove `export default { ... }` (runtime-only Options API config)
/// 5. Register non-type import bindings for template resolution
pub(super) fn process_companion_for_tsx<'alloc>(
    companion: &RootNodeScript,
    source: &str,
    ct: &mut CodeTransform<'_>,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    alloc: &'alloc Allocator,
    hoist_pos: u32,
) {
    // Remove companion <script> open tag
    ct.remove(companion.tag_open.start, companion.tag_open.end);
    // Remove companion </script> close tag
    if let Some(tag_close) = &companion.tag_close {
        ct.remove(tag_close.start, tag_close.end);
    }

    let content_span = match &companion.content {
        Some(span) => span,
        None => return, // Self-closing <script /> — nothing to process
    };

    let comp_start = content_span.start;
    let comp_str = &source[content_span.start as usize..content_span.end as usize];

    // Parse companion content with OXC
    let oxc_alloc = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, comp_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Options,
        comp_start,
        comp_str,
    );

    // Hoist imports to file top (preserving sourcemap).
    for item in &parse_result.items {
        if let ScriptItem::Import(imp) = item {
            let abs_start = comp_start + imp.span.start;
            let abs_end = comp_start + imp.span.end;
            // The in-project bare `.vue` specifier is emitted verbatim — it
            // resolves natively to the `.d.vue.ts` declaration carrier (no
            // compile-time specifier rewrite).
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");

            // Register non-type import bindings for template resolution.
            // Companion imports (e.g., component imports) need to be in the
            // bindings map so template binding resolution works.
            if !imp.is_type_only {
                for binding in &imp.bindings {
                    if !binding.is_type_only {
                        let alloc_name = alloc.alloc_str(binding.name);
                        bindings
                            .entry(alloc_name)
                            .or_insert(BindingType::SetupImport);
                    }
                }
            }
        }
    }

    // Hoist type declarations to file top (preserving sourcemap).
    for item in &parse_result.items {
        if let ScriptItem::TypeDeclaration(td) = item {
            let abs_start = comp_start + td.span.start;
            let abs_end = comp_start + td.span.end;
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");
        }
    }

    // In-project `.vue` re-export and dynamic-import specifiers are emitted
    // verbatim — a bare framework-carrier import resolves natively to the
    // `.d.vue.ts` declaration carrier, so neither form is rewritten.

    // Remove `export default { ... }` — runtime-only Options API config.
    for item in &parse_result.items {
        if let ScriptItem::DefaultExport(de) = item {
            let abs_start = comp_start + de.span.start;
            let abs_end = comp_start + de.span.end;
            out.overwrite(abs_start, abs_end, "");
        }
    }
}

// ── Script Only (Options API) Processing ──────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn process_tsx_script_only<'alloc>(
    script: &RootNodeScript,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    type_constructs: &mut String,
    _alloc: &'alloc Allocator,
    options: &IdeScriptOptions<'_>,
    builtin_components: &[&str],
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

    if should_infer_function_types(script.lang) {
        let available_bindings = collect_binding_names(&parse_result.bindings, source, content_str);
        apply_template_ref_call_inference(
            &parser_ret.program.body,
            template_ast,
            source,
            content_str,
            content_start,
            &available_bindings,
            out,
        );
    }

    // Extract bindings from Options API
    // Unlike script setup, Options API binding spans are ALL content-relative
    // (extract_options_bindings doesn't add content_offset). Use content_str for all.
    // Bounds-checked: partial ASTs may produce garbage spans, skip invalid ones.
    for (span, bt) in &parse_result.bindings {
        let name = match content_str.get(span.start as usize..span.end as usize) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let alloc_name = out.alloc_str(name);
        bindings.insert(alloc_name, *bt);
    }

    // Extract component aliases from `components: { Alias: ImportedComp }`.
    // For each alias where key != value, emit `const Alias = ImportedComp;`
    // so the template JSX `<Alias />` resolves to the imported component.
    let component_aliases = extract_component_aliases(&parser_ret.program, content_str);

    // Detect if the default export is a plain object (needs defineComponent wrapping
    // for type inference). Only applies to JS mode — TS uses native type syntax.
    let needs_define_component_wrap = options.is_jsx
        && parse_result.items.iter().any(|item| {
            matches!(
                item,
                ScriptItem::DefaultExport(de) if de.export_type == DefaultExportType::Object
            )
        });

    // Remove script tags, emit wrapper + content
    // The Options API wraps the script content in a TemplateBindingFN for type construct parity.
    let hoist_pos = script.tag_open.start;
    out.overwrite(script.tag_open.start, script.tag_open.end, "");
    if let Some(tag_close) = &script.tag_close {
        // Append export default at end
        let mut close = String::with_capacity(128);
        close.push_str("\nexport default __sfc__;\n");
        // Emit component alias declarations
        for (alias, value) in &component_aliases {
            close.push_str(&format!("const {alias} = {value};\n"));
        }
        // Ambient instance declaration for template property access.
        close.push_str(&instance_declaration_ambient(
            options.filename,
            options.is_jsx,
            needs_define_component_wrap,
        ));
        close.push_str(&directive_accessor_declaration(options.is_jsx));
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

    // Emit helper imports + type constructs (same as template-only and setup paths)
    if needs_define_component_wrap {
        emit_helper_imports_with_define_component(
            out,
            hoist_pos,
            options,
            builtin_components,
            template_ast,
        );
    } else {
        emit_helper_imports(out, hoist_pos, options, builtin_components, template_ast);
    }

    emit_type_constructs(
        type_constructs,
        &None, // no generics (Options API can't have them)
        &None, // no attrs (Options API doesn't use script attrs)
        source,
        options,
        false, // no getCurrentInstance detection for Options API
        true,  // emit attributes type
    );
}

// ── Options API Component Alias Extraction ──────────────────────────

/// Extract `components: { Alias: Value }` entries from the default export.
///
/// Returns `(alias_name, value_name)` pairs for entries where the key
/// differs from the value identifier name (i.e., actual aliases).
/// Shorthand `{ SomeComp }` is skipped since `SomeComp` is already in scope.
fn extract_component_aliases<'a>(
    program: &Program<'a>,
    _content_str: &str,
) -> Vec<(String, String)> {
    let mut aliases = Vec::new();
    for stmt in &program.body {
        let Statement::ExportDefaultDeclaration(export) = stmt else {
            continue;
        };
        let Some(expr) = export.declaration.as_expression() else {
            continue;
        };
        // Unwrap defineComponent() to get the inner object
        let obj = match expr {
            Expression::ObjectExpression(obj) => Some(obj.as_ref()),
            Expression::CallExpression(call) => call
                .arguments
                .first()
                .and_then(|a| a.as_expression())
                .and_then(|e| {
                    if let Expression::ObjectExpression(obj) = e {
                        Some(obj.as_ref())
                    } else {
                        None
                    }
                }),
            _ => None,
        };
        let Some(obj) = obj else { continue };
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                continue;
            };
            let key_name = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                _ => continue,
            };
            if key_name != "components" {
                continue;
            }
            let Expression::ObjectExpression(comp_obj) = &p.value else {
                continue;
            };
            for comp_prop in &comp_obj.properties {
                let ObjectPropertyKind::ObjectProperty(cp) = comp_prop else {
                    continue;
                };
                // Skip shorthand: `{ SomeComp }` — key and value are the same identifier
                if cp.shorthand {
                    continue;
                }
                let alias = match &cp.key {
                    PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                    PropertyKey::StringLiteral(s) => s.value.to_string(),
                    _ => continue,
                };
                let value = match &cp.value {
                    Expression::Identifier(id) => id.name.to_string(),
                    _ => continue,
                };
                if alias != value {
                    aliases.push((alias, value));
                }
            }
        }
    }
    aliases
}
