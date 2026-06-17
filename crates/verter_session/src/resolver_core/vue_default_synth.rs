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

use crate::decl_body_memo::LoweredValueDecl;

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
/// - `signatures[0].return_type` is the instance shape
///   (`{ $props: ..., $emit: ..., $slots: ... }`). `InstanceType<T>`
///   then projects the construct signature's return type to this
///   surface.
/// - `signatures[0].parameters` is empty — SFC components do not
///   expose constructor parameters at the typeinfo boundary.
///
/// The synthesis only walks `parsed_type_argument` from each macro
/// (already cached during shallow analysis). It does not re-parse
/// source, does not call into component-meta, and does not allocate
/// beyond the `ObjectExpr` wrapper plus the per-member entries — so
/// adding it to `ShallowFileState` construction stays inside the
/// shallow-processing budget.
#[must_use]
pub fn synthesise_vue_default_value_symbol(macros: &[AnalyzedMacro]) -> Option<LoweredValueDecl> {
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
        members.push(ObjectMember::Property(ObjectProperty::synthetic_public(
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

    // EAGER macro-producer result: the synthesized public-instance shape is
    // a fully lowered value body (a class with one construct signature), not
    // a re-lowered declaration. It is the same `LoweredValueDecl` body carrier
    // the lazy path produces; the caller stores it in the dedicated
    // synthesised-body map and derives the header symbol (which carries the
    // `is_synthesised_component_default` provenance flag) from it.
    Some(LoweredValueDecl {
        kind: ValueDeclKind::Class,
        type_annotation: None,
        signatures: vec![FunctionSignature {
            parameters: Vec::new(),
            return_type: Some(instance_shape),
            type_parameters: Vec::new(),
            has_implementation_body: true,
        }],
        object_shape: None,
        enum_members: None,
    })
}

/// URI prefix typeinfo scratch files use. A scratch that inlines a
/// `.vue` scope's eval-source (see
/// [`super::super::typeinfo::evaluate_type_expression`]) carries the
/// inlined macros, so the synthesizing framework's leg fabricates the
/// `default` from them — even though the scratch classifies by its own
/// `.ts` suffix.
const TYPEINFO_SCRATCH_URI_PREFIX: &str = "verter://typeinfo/";

/// Whether `canonical_id` is a typeinfo evaluation scratch surface.
///
/// A typeinfo scratch is a host-internal evaluation file that inlines an
/// arbitrary scope's eval-source as a prelude; it classifies by its `.ts`
/// suffix yet must synthesize the inlined scope's `default`. The neutral
/// synth-injection selector
/// ([`crate::VerterHost::inject_component_default_into_shallow_state`])
/// routes a scratch to the synthesizing framework's leg.
#[must_use]
pub fn is_typeinfo_scratch(canonical_id: &str) -> bool {
    canonical_id.starts_with(TYPEINFO_SCRATCH_URI_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::types::AnalyzedMacro;
    use verter_span::Span;
    use verter_type_expr::TypeExpr;

    fn type_based_macro(kind: AnalyzedMacroKind, type_text: &str) -> AnalyzedMacro {
        let parsed =
            verter_semantic::analysis::jsdoc::parse_jsdoc_tag_type_payload(type_text, None);
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

    fn instance_members(lowered: &LoweredValueDecl) -> Vec<String> {
        let sig = lowered
            .signatures
            .first()
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
    fn is_typeinfo_scratch_accepts_only_the_scratch_prefix() {
        // The scratch surface is recognised by its URI prefix; a `.vue` file is
        // NOT a scratch (it routes to the synth leg by its framework
        // classification, not by this predicate).
        assert!(is_typeinfo_scratch("verter://typeinfo/abc123.ts"));
        assert!(!is_typeinfo_scratch("/workspace/src/App.vue"));
        assert!(!is_typeinfo_scratch("/workspace/src/types.ts"));
        assert!(!is_typeinfo_scratch("/workspace/src/runtime.js"));
        assert!(!is_typeinfo_scratch(""));
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
