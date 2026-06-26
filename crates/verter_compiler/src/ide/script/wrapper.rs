//! Wrapper helpers + glue for IDE TSX script generation (D10 of
//! ownership-domain analysis).
//!
//! Hosts the `PREFIX` const, the `___VERTER___instance` /
//! `___VERTER___directiveAccessor` / `___VERTER___TemplateBindingFN`
//! emission helpers, the global-component fallback emitter, and the
//! `to_pascal_case` / `should_infer_function_types` glue helpers.

use rustc_hash::FxHashSet;

use crate::ast::types::{AstNodeKind, TemplateAst};
use crate::cursor::ScriptLanguage;
use crate::ide::{IdeScriptOptions, CARRIER_API_VIRTUAL_SUFFIX};
use crate::template::code_gen::types::CodeGenOutput;

/// Prefix for all emitted ___VERTER___ types/functions.
pub(super) const PREFIX: &str = "___VERTER___";

pub(super) fn should_infer_function_types(lang: Option<ScriptLanguage>) -> bool {
    matches!(lang, Some(ScriptLanguage::TypeScript | ScriptLanguage::TSX))
}

/// Collect the GlobalComponents fallback const names for every unresolved component
/// referenced in the template.
///
/// A fallback const is materialized for each component tag that is NOT a builtin, NOT a
/// member-expression tag (`Foo.Bar`), and NOT already bound by `is_bound` — exactly the
/// set [`emit_global_component_fallbacks`] writes. Names are returned in first-seen
/// source order and deduplicated.
///
/// Collection is split from emission so that ONE list feeds both the emitted consts and
/// the template/event-typing inventory ([`crate::ide::TemplateComponentBindings`]): a
/// globally-registered component then types identically wherever it is referenced
/// (`@event` spread payloads, simple-handler param inference), via the in-scope
/// `InstanceType<typeof Pascal>["$props"]` const rather than `import('vue').GlobalComponents[...]`
/// (which the `tsgo` TypeProvider cannot resolve).
pub(super) fn collect_global_component_fallbacks(
    template_ast: Option<&TemplateAst>,
    source: &str,
    is_bound: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut fallbacks: Vec<String> = Vec::new();
    let ast = match template_ast {
        Some(a) => a,
        None => return fallbacks,
    };

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
            if is_bound(pascal.as_str()) || is_bound(tag_name) {
                continue;
            }

            // Skip member expressions like Foo.Bar
            if tag_name.contains('.') {
                continue;
            }

            if seen.insert(pascal.clone()) {
                fallbacks.push(pascal);
            }
        }
    }

    fallbacks
}

/// Emit global component fallback consts inside templateBindingFN from a collected list
/// (see [`collect_global_component_fallbacks`]). The emitted const names are exactly the
/// list members, so the template/event-typing inventory and the emitted scaffolding never
/// disagree.
pub(super) fn emit_global_component_fallbacks(
    buf: &mut String,
    fallbacks: &[String],
    is_jsx: bool,
) {
    use std::fmt::Write;
    for pascal in fallbacks {
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
/// Uses an `import()` type expression to get the full component instance type
/// from the component's PUBLIC-API carrier default export
/// (`{filename}.verter.ts`, the redirect-reached `.d.ts`-equivalent surface),
/// providing type checking for `$slots`, `$emit`, `$props`, `$attrs`, and any
/// custom global properties (e.g., `$t`, `$router`). The API-carrier suffix is
/// the descriptor-derived [`CARRIER_API_VIRTUAL_SUFFIX`] — the IDE carrier's
/// SELF-import targets the API surface (where the public default is
/// synthesised), never its own `.vue.tsx` IDE output.
pub(super) fn instance_declaration(filename: &str, is_jsx: bool, override_attrs: bool) -> String {
    // The IDE carrier's SELF-import targets its sibling API carrier
    // (`Comp.vue.verter.ts`) in the SAME directory, so the `./`-relative
    // specifier must use the BASENAME (the live publish path passes the full
    // canonical path; a `./d:/…/Comp.vue.verter.ts` specifier resolves to nothing).
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    if is_jsx {
        format!(
            "\n/** @type {{any}} */\nvar {P}instance = /** @type {{any}} */ (null);\nvoid {P}instance;\n",
            P = PREFIX,
        )
    } else if override_attrs {
        // With Comp functions + attrs type aliases: override $attrs with composed type
        format!(
            "\n// @ts-ignore\nlet {P}instance!: Omit<InstanceType<import('./{basename}{API}')['default']>, '$attrs'> & {{ $attrs: {P}Attrs }};\nvoid {P}instance;\n",
            P = PREFIX,
            basename = basename,
            API = CARRIER_API_VIRTUAL_SUFFIX,
        )
    } else {
        format!(
            "\n// @ts-ignore\nlet {P}instance!: InstanceType<import('./{basename}{API}')['default']>;\nvoid {P}instance;\n",
            P = PREFIX,
            basename = basename,
            API = CARRIER_API_VIRTUAL_SUFFIX,
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
    // The self-import targets the sibling API carrier in the same directory, so
    // the `./`-relative specifier must use the BASENAME (the live publish path
    // passes the full canonical path).
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    if is_jsx {
        if needs_define_component_wrap {
            // Inline defineComponent wrapping — avoids self-import, works with TSGO + tsserver
            format!(
                "\nconst {P}dc = ({P}defineComponent)(__sfc__);\n/** @type {{InstanceType<typeof {P}dc>}} */\nvar {P}instance = /** @type {{*}} */ (null);\n",
                P = PREFIX,
            )
        } else {
            // Already has defineComponent — self-import the typed default export
            // from the PUBLIC-API carrier (`.verter.ts`), not the IDE output.
            format!(
                "\n/** @type {{InstanceType<import('./{basename}{API}')['default']>}} */\nvar {P}instance = /** @type {{*}} */ (null);\n",
                P = PREFIX,
                basename = basename,
                API = CARRIER_API_VIRTUAL_SUFFIX,
            )
        }
    } else {
        format!(
            "\n// @ts-ignore\ndeclare let {P}instance: InstanceType<import('./{basename}{API}')['default']>;\n",
            P = PREFIX,
            basename = basename,
            API = CARRIER_API_VIRTUAL_SUFFIX,
        )
    }
}

/// Emit the component's PUBLIC-FACADE re-export onto the IDE carrier.
///
/// The IDE carrier (`{name}.vue.tsx`) is the in-project bare-import target
/// (`import Comp from "./Comp.vue"` resolves here — the bare-import-probe
/// identity, §2.2/§2.9). A consumer importing it must therefore see the
/// component's PUBLIC type as a clean `export default`. The public default is
/// SYNTHESISED on the API carrier (`{name}.verter.ts`, the redirect-reached
/// public-API surface produced by the higher-layer API projector), so the IDE
/// carrier RE-EXPORTS it:
///
/// ```text
/// export { default } from './{name}.verter.ts';
/// ```
///
/// This is ADDITIVE to the template-checking body — all template internals
/// (`___VERTER___TemplateBindingFN`, `__VerterProps`, binding scaffolding) stay
/// LOCAL (non-exported). The IDE carrier already self-imports the API carrier
/// to type `___VERTER___instance`, so this re-export introduces no new coupling
/// — it surfaces the public default the API carrier already owns.
///
/// JS/JSX carriers re-export identically (`export { default } from` is valid
/// in `.jsx`); the API surface is `.verter.ts` for every carrier (the reserved
/// `.verter.` infix is uniform across adapters).
pub(super) fn public_facade_reexport(filename: &str) -> String {
    // The IDE carrier (`Comp.vue.tsx`) and the API carrier (`Comp.vue.verter.ts`)
    // are siblings in the same directory, so the `./`-relative specifier must use
    // the BASENAME — a caller passing the full canonical path (the live publish
    // path does) would otherwise emit `./d:/…/Comp.vue.verter.ts`, which resolves
    // to nothing and breaks the public default re-export for plain-script imports.
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    format!(
        "\nexport {{ default }} from './{basename}{API}';\n",
        basename = basename,
        API = CARRIER_API_VIRTUAL_SUFFIX,
    )
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
