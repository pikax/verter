//! Wrapper helpers + glue for IDE TSX script generation (D10 of
//! ownership-domain analysis).
//!
//! Hosts the `PREFIX` const, the `___VERTER___instance` /
//! `___VERTER___directiveAccessor` / `___VERTER___TemplateBindingFN`
//! emission helpers, the global-component fallback emitter, and the
//! `to_pascal_case` / `should_infer_function_types` glue helpers.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::types::{AstNodeKind, TemplateAst};
use crate::cursor::ScriptLanguage;
use crate::ide::IdeScriptOptions;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;

/// Prefix for all emitted ___VERTER___ types/functions.
pub(super) const PREFIX: &str = "___VERTER___";

pub(super) fn should_infer_function_types(lang: Option<ScriptLanguage>) -> bool {
    matches!(lang, Some(ScriptLanguage::TypeScript | ScriptLanguage::TSX))
}

/// Emit global component fallback consts for unresolved components inside templateBindingFN.
pub(super) fn emit_global_component_fallbacks(
    buf: &mut String,
    template_ast: Option<&TemplateAst>,
    source: &str,
    bindings: &FxHashMap<&str, BindingType>,
    is_jsx: bool,
) {
    let ast = match template_ast {
        Some(a) => a,
        None => return,
    };

    let binding_names: FxHashSet<&str> = bindings.keys().copied().collect();
    let mut seen = FxHashSet::default();

    for node in &ast.nodes {
        if let AstNodeKind::Element(ref el) = node.kind {
            if !el.tag_type.is_component() {
                continue;
            }
            let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

            // Skip builtins
            if crate::template::code_gen::shared::helpers::is_builtin_component(tag_name).is_some()
            {
                continue;
            }

            // Convert to PascalCase for binding lookup
            let pascal = to_pascal_case(tag_name);
            if binding_names.contains(pascal.as_str()) || binding_names.contains(tag_name) {
                continue;
            }

            // Skip member expressions like Foo.Bar
            if tag_name.contains('.') {
                continue;
            }

            if seen.insert(pascal.clone()) {
                use std::fmt::Write;
                if is_jsx {
                    write!(
                        buf,
                        "\nconst {pascal} = /** @type {{unknown}} */ ({{}});",
                        pascal = pascal,
                    )
                    .expect("write to String is infallible");
                } else {
                    write!(
                        buf,
                        "\nconst {pascal} = {{}} as import('vue').GlobalComponents extends {{ {pascal}: infer C }} ? C : unknown;",
                        pascal = pascal,
                    )
                    .expect("write to String is infallible");
                }
            }
        }
    }
}

/// Convert a kebab-case or camelCase tag name to PascalCase.
pub(super) fn to_pascal_case(tag: &str) -> String {
    if tag.contains('-') {
        tag.split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        format!("{}{}", upper, chars.as_str())
                    }
                    None => String::new(),
                }
            })
            .collect()
    } else {
        // Already PascalCase or camelCase — capitalize first letter
        let mut chars = tag.chars();
        match chars.next() {
            Some(c) => {
                let upper: String = c.to_uppercase().collect();
                format!("{}{}", upper, chars.as_str())
            }
            None => String::new(),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

pub(super) fn emit_minimal_wrapper(
    out: &mut CodeGenOutput<'_>,
    options: &IdeScriptOptions<'_>,
    pos: u32,
    template_end: Option<u32>,
) -> Option<String> {
    if template_end.is_some() {
        // Unified CT: function start at pos, close deferred
        let mut start = format!("export function {}TemplateBindingFN() {{\n", PREFIX);
        // Declare instance for instance property access in template.
        // Minimal wrapper: no Comp functions, so no $attrs override
        start.push_str(&instance_declaration(
            options.filename,
            options.is_jsx,
            false,
        ));
        start.push_str(&directive_accessor_declaration(options.is_jsx));
        out.prepend_alloc(pos, &start);
        let mut close = String::from("\n");
        close.push_str(&instance_probe_line());
        close.push_str("return {};\n}\n");
        Some(close)
    } else {
        // No template: emit everything at pos
        let wrapper = format!(
            "export function {}TemplateBindingFN() {{\nreturn {{}};\n}}\n",
            PREFIX,
        );
        out.prepend_alloc(pos, &wrapper);
        None
    }
}

/// Emit the `___VERTER___instance` declaration and void suppression.
///
/// Uses `import()` type expression to get the full component instance type from the
/// `.vue.d.ts` default export, providing type checking for `$slots`, `$emit`, `$props`,
/// `$attrs`, and any custom global properties (e.g., `$t`, `$router`).
pub(super) fn instance_declaration(filename: &str, is_jsx: bool, override_attrs: bool) -> String {
    if is_jsx {
        format!(
            "\n/** @type {{any}} */\nvar {P}instance = /** @type {{any}} */ (null);\nvoid {P}instance;\n",
            P = PREFIX,
        )
    } else if override_attrs {
        // With Comp functions + attrs type aliases: override $attrs with composed type
        format!(
            "\n// @ts-ignore\nlet {P}instance!: Omit<InstanceType<import('./{filename}.ts')['default']>, '$attrs'> & {{ $attrs: {P}Attrs }};\nvoid {P}instance;\n",
            P = PREFIX,
            filename = filename,
        )
    } else {
        format!(
            "\n// @ts-ignore\nlet {P}instance!: InstanceType<import('./{filename}.ts')['default']>;\nvoid {P}instance;\n",
            P = PREFIX,
            filename = filename,
        )
    }
}

/// Ambient variant for Options API (file scope, no TDZ issues).
///
/// Uses `declare let` so the declaration is available regardless of position in file.
/// Needed because template JSX may appear before the script block.
///
/// For JS mode with plain object exports (`needs_define_component_wrap = true`), we
/// inline a `defineComponent(__sfc__)` call to get proper instance typing without
/// relying on self-import (which TSGO cannot resolve for virtual `.vue.jsx` files).
pub(super) fn instance_declaration_ambient(
    filename: &str,
    is_jsx: bool,
    needs_define_component_wrap: bool,
) -> String {
    if is_jsx {
        if needs_define_component_wrap {
            // Inline defineComponent wrapping — avoids self-import, works with TSGO + tsserver
            format!(
                "\nconst {P}dc = ({P}defineComponent)(__sfc__);\n/** @type {{InstanceType<typeof {P}dc>}} */\nvar {P}instance = /** @type {{*}} */ (null);\n",
                P = PREFIX,
            )
        } else {
            // Already has defineComponent — use self-import for the typed default export
            format!(
                "\n/** @type {{InstanceType<import('./{filename}.ts')['default']>}} */\nvar {P}instance = /** @type {{*}} */ (null);\n",
                P = PREFIX,
                filename = filename,
            )
        }
    } else {
        format!(
            "\n// @ts-ignore\ndeclare let {P}instance: InstanceType<import('./{filename}.ts')['default']>;\n",
            P = PREFIX,
            filename = filename,
        )
    }
}

/// Emit the `___VERTER___directiveAccessor` declaration.
///
/// Extracts both local setup directives and global Vue directives from the
/// component instance, providing type-safe access for custom directive
/// type checking in template JSX output.
pub(super) fn directive_accessor_declaration(is_jsx: bool) -> String {
    if is_jsx {
        format!(
            "var {P}directiveAccessor = {P}retrieveSetupDirectives({P}instance);\nvoid {P}directiveAccessor;\n",
            P = PREFIX,
        )
    } else {
        format!(
            "const {P}directiveAccessor = {P}retrieveSetupDirectives({P}instance);\nvoid {P}directiveAccessor;\n",
            P = PREFIX,
        )
    }
}

/// Emit the instance completion probe line.
///
/// Creates a member-access expression at a known position that the LSP can use
/// to request TSGO completions for all instance members.
pub(super) fn instance_probe_line() -> String {
    format!("\nvoid ({P}instance).valueOf;\n", P = PREFIX)
}
