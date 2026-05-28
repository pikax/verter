//! Synthesise the implicit `default` export for a Vue Single File
//! Component scope.
//!
//! A `<script setup>` block does not contain a literal `export default`
//! statement — the SFC's default export is produced by the runtime
//! compiler from the macro calls (`defineProps`, `defineEmits`,
//! `defineSlots`, …). The shallow file state for a `.vue` file
//! therefore lacks a `default` symbol by default, which means
//! type-driven queries such as `typeof default` or
//! `InstanceType<typeof default>['$props']` evaluated against a
//! `.vue` scope cannot reduce to a concrete shape.
//!
//! This module bridges that gap inside the Rust substrate: when a
//! `.vue` file's analysis reports type-based macros, we synthesise a
//! `ShallowValueSymbol` named `default` whose construct signature
//! returns an instance object carrying `$props` / `$emit` / `$slots`
//! members keyed by the macro type arguments. The synthesis is
//! cache-owned (it lives on the same `ShallowFileState` that
//! everything else routes through), is built once per content hash
//! per the Shallow File Processing Core Invariant, and depends on
//! data already captured during parse (the macros' parsed type
//! arguments). Downstream consumers — typeinfo's
//! `evaluate_type_expression`, `resolve_named_symbol`, the LSP, the
//! MCP server, and any future tool that walks the `.vue` scope's
//! `default` — therefore project `['$props']` to the concrete props
//! object without learning evaluator-side special cases.

use std::sync::Arc;

use verter_semantic::analysis::type_eval::{FunctionSignature, ValueDeclKind};
use verter_semantic::analysis::types::{AnalyzedMacro, AnalyzedMacroKind};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

use super::shallow_file_state::{ShallowFileState, ShallowValueSymbol};

/// Property name on the synthesised SFC instance that carries the
/// `defineProps<T>()` shape.
pub const VUE_INSTANCE_PROPS_MEMBER: &str = "$props";

/// Property name on the synthesised SFC instance that carries the
/// `defineEmits<E>()` shape.
pub const VUE_INSTANCE_EMIT_MEMBER: &str = "$emit";

/// Property name on the synthesised SFC instance that carries the
/// `defineSlots<S>()` shape.
pub const VUE_INSTANCE_SLOTS_MEMBER: &str = "$slots";

/// Build the synthetic `default` value symbol for a `.vue` scope from
/// the SFC's parsed macro list. Returns `None` when no type-based
/// macro contributed an instance member — in that case the shallow
/// state is left untouched and a `default` lookup falls through to
/// the existing miss handling.
///
/// The returned symbol mimics a userland `class default { ... }`:
///
/// - `kind = ValueDeclKind::Class` — `build_typeof` lowers the
///   declaration as an Object surface carrying a single
///   ConstructSignature whose return type is the instance shape.
/// - `function_signature.return_type` is the instance shape
///   (`{ $props: ..., $emit: ..., $slots: ... }`). `InstanceType<T>`
///   then projects the construct signature's return type to this
///   surface.
/// - `function_signature.parameters` is empty — SFC components do not
///   expose constructor parameters at the typeinfo boundary.
///
/// The synthesis only walks `parsed_type_argument` from each macro
/// (already cached during shallow analysis). It does not re-parse
/// source, does not call into component-meta, and does not allocate
/// beyond the `ObjectExpr` wrapper plus the per-member entries — so
/// adding it to `ShallowFileState` construction stays inside the
/// shallow-processing budget.
#[must_use]
pub fn synthesise_vue_default_value_symbol(macros: &[AnalyzedMacro]) -> Option<ShallowValueSymbol> {
    let mut members: Vec<ObjectMember> = Vec::new();
    let mut seen_props = false;
    let mut seen_emit = false;
    let mut seen_slots = false;

    for mac in macros {
        if !mac.is_type_based {
            continue;
        }
        let Some(type_arg) = mac.parsed_type_argument.as_ref() else {
            continue;
        };

        let (member_name, already_seen) = match mac.kind {
            AnalyzedMacroKind::DefineProps => (VUE_INSTANCE_PROPS_MEMBER, &mut seen_props),
            AnalyzedMacroKind::DefineEmits => (VUE_INSTANCE_EMIT_MEMBER, &mut seen_emit),
            AnalyzedMacroKind::DefineSlots => (VUE_INSTANCE_SLOTS_MEMBER, &mut seen_slots),
            // `withDefaults(defineProps<T>(), ...)` surfaces as both a
            // `WithDefaults` macro and the inner `DefineProps` macro
            // in `analysis.macros`; the inner macro carries the
            // `parsed_type_argument` we want, so we ignore the outer
            // one here. Other kinds (DefineModel, DefineExpose,
            // DefineOptions) do not contribute instance-side members
            // that the public-instance contract walks through
            // `InstanceType<typeof default>['<member>']`.
            _ => continue,
        };

        if *already_seen {
            // Defensive: a second `defineProps<T>()` in the same
            // SFC is a user error caught elsewhere; do not let the
            // duplicate clobber the first member.
            continue;
        }
        *already_seen = true;

        // Synthesized public-instance member (`$props` / `$slots` / …) — no
        // source declaration site for the synthetic member name.
        members.push(ObjectMember::Property(ObjectProperty::synthetic(
            member_name.to_string(),
            type_arg.as_ref().clone(),
            false,
            false,
        )));
    }

    if members.is_empty() {
        return None;
    }

    let instance_shape = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));

    Some(ShallowValueSymbol {
        kind: ValueDeclKind::Class,
        type_annotation: None,
        function_signature: Some(FunctionSignature {
            parameters: Vec::new(),
            return_type: Some(instance_shape),
            type_parameters: Vec::new(),
        }),
        object_shape: None,
        enum_members: None,
    })
}

/// URI prefix typeinfo scratch files use. Synthesis fires for
/// these so a scratch that inlines a `.vue` scope's eval-source
/// (see [`super::super::typeinfo::evaluate_type_expression`])
/// picks up the synthesised `default` from the inlined macros.
const TYPEINFO_SCRATCH_URI_PREFIX: &str = "verter://typeinfo/";

/// Whether `canonical_id` is a candidate for `default`-symbol
/// synthesis — i.e. a `.vue` SFC or a typeinfo scratch URI that
/// inlines a `.vue` scope's eval-source. Plain `.ts` / `.js` files
/// that happen to call something locally named `defineProps` do
/// NOT qualify; only the two known producers of SFC-style macros
/// flow through this seam.
fn is_synthesis_candidate(canonical_id: &str) -> bool {
    canonical_id.ends_with(".vue") || canonical_id.starts_with(TYPEINFO_SCRATCH_URI_PREFIX)
}

/// Inject the synthesised `default` value symbol into `state` when
/// `macros` contributes one. No-op when synthesis returns `None`
/// (no type-based macros), when `state` already carries a userland
/// `default` value symbol (userland always wins), or when
/// `canonical_id` is not a recognised SFC / scratch surface
/// ([`is_synthesis_candidate`]).
///
/// This is the single architectural seam between the shallow-state
/// builder and the Vue-default synthesis policy. Both `.vue`
/// canonical files and typeinfo scratch files that inline a
/// `.vue` scope's eval-source flow through it.
pub fn inject_vue_default_into_shallow_state(
    canonical_id: &str,
    state: &mut ShallowFileState,
    macros: &[AnalyzedMacro],
) {
    if !is_synthesis_candidate(canonical_id) {
        return;
    }
    if state.value_symbol("default").is_some() {
        return;
    }
    if let Some(default_symbol) = synthesise_vue_default_value_symbol(macros) {
        state.insert_synthesised_value_symbol("default", default_symbol);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::types::AnalyzedMacro;
    use verter_span::Span;
    use verter_type_expr::TypeExpr;

    fn type_based_macro(kind: AnalyzedMacroKind, type_text: &str) -> AnalyzedMacro {
        let parsed = verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(type_text);
        AnalyzedMacro {
            kind,
            is_type_based: true,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: Some(Arc::new(parsed)),
            parsed_type_argument_scope: Some(verter_type_expr::TypeExprScope::new("")),
            span: Span::new(0, 0),
        }
    }

    fn runtime_macro(kind: AnalyzedMacroKind) -> AnalyzedMacro {
        AnalyzedMacro {
            kind,
            is_type_based: false,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
            span: Span::new(0, 0),
        }
    }

    fn instance_members(symbol: &ShallowValueSymbol) -> Vec<String> {
        let sig = symbol
            .function_signature
            .as_ref()
            .expect("synthesised default must carry a construct signature");
        let return_type = sig
            .return_type
            .as_ref()
            .expect("synthesised default's signature must carry a return type");
        match return_type {
            TypeExpr::Object(obj) => obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.clone()),
                    _ => None,
                })
                .collect(),
            other => panic!("expected Object instance shape, got {other:?}"),
        }
    }

    #[test]
    fn no_type_based_macros_returns_none() {
        let macros = vec![runtime_macro(AnalyzedMacroKind::DefineProps)];
        assert!(synthesise_vue_default_value_symbol(&macros).is_none());
    }

    #[test]
    fn empty_macros_returns_none() {
        assert!(synthesise_vue_default_value_symbol(&[]).is_none());
    }

    #[test]
    fn defineprops_alone_produces_dollar_props_member() {
        let macros = vec![type_based_macro(
            AnalyzedMacroKind::DefineProps,
            "{ msg: string }",
        )];
        let sym =
            synthesise_vue_default_value_symbol(&macros).expect("defineProps<T>() must synthesise");
        assert_eq!(sym.kind, ValueDeclKind::Class);
        assert_eq!(instance_members(&sym), vec!["$props".to_string()]);
    }

    #[test]
    fn defineprops_and_defineemits_produce_two_members() {
        let macros = vec![
            type_based_macro(AnalyzedMacroKind::DefineProps, "{ msg: string }"),
            type_based_macro(AnalyzedMacroKind::DefineEmits, "{ change: [v: number] }"),
        ];
        let sym = synthesise_vue_default_value_symbol(&macros)
            .expect("two type-based macros must synthesise");
        let mut names = instance_members(&sym);
        names.sort();
        assert_eq!(names, vec!["$emit".to_string(), "$props".to_string()]);
    }

    #[test]
    fn duplicate_defineprops_keeps_first() {
        let macros = vec![
            type_based_macro(AnalyzedMacroKind::DefineProps, "{ a: string }"),
            type_based_macro(AnalyzedMacroKind::DefineProps, "{ b: number }"),
        ];
        let sym = synthesise_vue_default_value_symbol(&macros)
            .expect("first defineProps<T>() must still synthesise");
        // Single `$props` member, not two.
        assert_eq!(instance_members(&sym), vec!["$props".to_string()]);
    }

    #[test]
    fn is_synthesis_candidate_accepts_vue_files_and_typeinfo_scratches() {
        assert!(is_synthesis_candidate("/workspace/src/App.vue"));
        assert!(is_synthesis_candidate("verter://typeinfo/abc123.ts"));
    }

    #[test]
    fn is_synthesis_candidate_rejects_plain_ts_and_js_files() {
        assert!(!is_synthesis_candidate("/workspace/src/types.ts"));
        assert!(!is_synthesis_candidate("/workspace/src/runtime.js"));
        assert!(!is_synthesis_candidate("/workspace/src/decl.d.ts"));
        assert!(!is_synthesis_candidate(""));
    }

    #[test]
    fn unhandled_macro_kinds_do_not_contribute() {
        // DefineOptions / DefineModel / DefineExpose are present but
        // type-based DefineProps is not — synthesis must return
        // None so we don't fabricate an empty instance shape.
        let macros = vec![
            type_based_macro(AnalyzedMacroKind::DefineOptions, "{ name: string }"),
            type_based_macro(AnalyzedMacroKind::DefineModel, "string"),
            type_based_macro(AnalyzedMacroKind::DefineExpose, "{}"),
        ];
        assert!(synthesise_vue_default_value_symbol(&macros).is_none());
    }
}
