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
//! `ShallowValueSymbol` named `default` whose fabricated public
//! instance carries `$props` / `$emit` / `$slots` members keyed by the
//! macro type arguments. The synthesis is cache-owned (it lives on the
//! same `ShallowFileState` that everything else routes through), is
//! built once per content hash per the Shallow File Processing Core
//! Invariant, and depends on data already captured during parse (the
//! macros' authored type-argument payload locators). Downstream
//! consumers — typeinfo's `evaluate_type_expression`,
//! `resolve_named_symbol`, the LSP, the MCP server, and any future tool
//! that walks the `.vue` scope's `default` — therefore project
//! `['$props']` to the concrete props object without learning
//! evaluator-side special cases.

use std::sync::Arc;

use verter_semantic::analysis::types::{AnalyzedMacro, AnalyzedMacroKind};
use verter_type_expr::facts::{
    FactOrLocator, ResolvedLocalShape, SemanticTypeSource, SynthesizedMemberFact,
};
use verter_type_expr::span_origins::{MemberSpansOrigin, SourceSynthetic};

use crate::decl_body_memo::{lowered_value_decl_for_synthesised_default, LoweredValueDecl};

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
/// - `kind = ValueDeclKind::Class`, with a single parameter-less
///   construct-signature FACT — SFC components do not expose
///   constructor parameters at the typeinfo boundary.
/// - The fabricated public-instance shape
///   (`{ $props: ..., $emit: ..., $slots: ... }`) rides the annotation
///   FACT as a synthesized CLOSED source
///   ([`SemanticTypeSource::Synthesized`]); each member value is the
///   macro's authored type-argument PAYLOAD LOCATOR
///   ([`FactOrLocator::MacroPayload`]), lowered on demand through the
///   one shared dispatch — never eagerly, never a stored typed body.
///   `InstanceType<typeof default>` projects the construct signature's
///   return to this surface through the keyed `Instantiate` query.
///
/// The synthesis only walks the macros' `parsed_type_argument`
/// locators (already captured during shallow analysis). It does not
/// re-parse source, does not call into component-meta, and does not
/// lower any declaration body — so adding it to `ShallowFileState`
/// construction stays inside the shallow-processing budget.
#[must_use]
pub fn synthesise_vue_default_value_symbol(macros: &[AnalyzedMacro]) -> Option<LoweredValueDecl> {
    let mut members: Vec<SynthesizedMemberFact> = Vec::new();
    let mut seen_props = false;
    let mut seen_emit = false;
    let mut seen_slots = false;

    for mac in macros {
        if !mac.is_type_based {
            continue;
        }
        let Some(payload_locator) = mac.parsed_type_argument.as_ref() else {
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

        // Synthesized public-instance member (`$props` / `$slots` / …): the
        // member VALUE is the macro's authored type-argument payload
        // locator — the real authored body lowers on demand through the one
        // dispatch (shallow-by-default), never eagerly here.
        members.push(SynthesizedMemberFact {
            name: member_name.to_string(),
            optional: false,
            ty: FactOrLocator::MacroPayload(payload_locator.clone()),
            span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
        });
    }

    if members.is_empty() {
        return None;
    }

    // The fabricated instance shape is a synthesized CLOSED fact; the record
    // assembly (annotation classification, construct-signature fact, value-body
    // fingerprint) is the shared synthesised-default constructor — one recipe
    // for every framework synth leg.
    Some(lowered_value_decl_for_synthesised_default(
        SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(Arc::from(
            members.into_boxed_slice(),
        ))),
    ))
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
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    use verter_semantic::analysis::types::AnalyzedMacro;
    use verter_span::Span;
    use verter_type_expr::locators::{
        AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
    };

    fn payload_locator(macro_index: u32) -> MacroPayloadLocator {
        MacroPayloadLocator {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/App.vue"),
                symbol: Arc::from("default"),
                space: LocatorSymbolSpace::Value,
            },
            macro_index,
            payload: MacroPayloadPosition::TypeArgument,
        }
    }

    fn type_based_macro(kind: AnalyzedMacroKind, macro_index: u32) -> AnalyzedMacro {
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
            parsed_type_argument: Some(payload_locator(macro_index)),
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

    /// The synthesized instance members `(name, ty)` off the annotation-borne
    /// synthesized source.
    fn instance_members(lowered: &LoweredValueDecl) -> Vec<(String, FactOrLocator)> {
        let source = lowered
            .type_annotation
            .annotation
            .as_ref()
            .expect("synthesised default must carry the instance annotation source");
        let SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(members)) = source else {
            panic!("expected a synthesized Object instance source, got {source:?}");
        };
        members
            .iter()
            .map(|m| (m.name.clone(), m.ty.clone()))
            .collect()
    }

    fn member_names(lowered: &LoweredValueDecl) -> Vec<String> {
        instance_members(lowered)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
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
        let macros = vec![type_based_macro(AnalyzedMacroKind::DefineProps, 0)];
        let sym =
            synthesise_vue_default_value_symbol(&macros).expect("defineProps<T>() must synthesise");
        assert_eq!(sym.kind, ValueDeclKind::Class);
        assert_eq!(member_names(&sym), vec!["$props".to_string()]);
        // The construct signature is parameter-less with no authored return
        // position; the instance rides the annotation source, not a signature
        // return slot.
        let sig = sym.signatures.first().expect("construct signature fact");
        assert!(sig.parameters.is_empty());
        assert!(sig.return_ty.is_none());
    }

    #[test]
    fn member_value_is_the_macro_payload_locator_not_a_resolved_body() {
        // Shallow-by-default: the `$props` member value stays the authored
        // macro type-argument PAYLOAD LOCATOR (lowered on demand through the
        // one dispatch), never an eagerly materialised body.
        let macros = vec![type_based_macro(AnalyzedMacroKind::DefineProps, 3)];
        let sym = synthesise_vue_default_value_symbol(&macros).expect("must synthesise");
        let members = instance_members(&sym);
        let (_, ty) = &members[0];
        match ty {
            FactOrLocator::MacroPayload(locator) => {
                assert_eq!(locator.macro_index, 3);
                assert!(matches!(
                    locator.payload,
                    MacroPayloadPosition::TypeArgument
                ));
            }
            other => panic!("expected the macro payload locator carrier, got {other:?}"),
        }
    }

    #[test]
    fn defineprops_and_defineemits_produce_two_members() {
        let macros = vec![
            type_based_macro(AnalyzedMacroKind::DefineProps, 0),
            type_based_macro(AnalyzedMacroKind::DefineEmits, 1),
        ];
        let sym = synthesise_vue_default_value_symbol(&macros)
            .expect("two type-based macros must synthesise");
        let mut names = member_names(&sym);
        names.sort();
        assert_eq!(names, vec!["$emit".to_string(), "$props".to_string()]);
    }

    #[test]
    fn duplicate_defineprops_keeps_first() {
        let macros = vec![
            type_based_macro(AnalyzedMacroKind::DefineProps, 0),
            type_based_macro(AnalyzedMacroKind::DefineProps, 1),
        ];
        let sym = synthesise_vue_default_value_symbol(&macros)
            .expect("first defineProps<T>() must still synthesise");
        // Single `$props` member, not two — and it carries the FIRST macro's
        // payload locator (index 0), not the duplicate's.
        let members = instance_members(&sym);
        assert_eq!(members.len(), 1);
        let (name, ty) = &members[0];
        assert_eq!(name, "$props");
        match ty {
            FactOrLocator::MacroPayload(locator) => assert_eq!(locator.macro_index, 0),
            other => panic!("expected macro payload locator, got {other:?}"),
        }
    }

    #[test]
    fn synthesised_fingerprint_is_honestly_degraded_not_fabricated() {
        // The synthesised record fingerprints through the shared fold: a
        // transient-less record with a classified (synthesized) annotation
        // carries the honest DEGRADED bit — never a fabricated complete
        // fingerprint that would collide across distinct synthesized bodies.
        let macros = vec![type_based_macro(AnalyzedMacroKind::DefineProps, 0)];
        let sym = synthesise_vue_default_value_symbol(&macros).expect("must synthesise");
        assert!(sym.body_hash.budget_exceeded);
        // Non-enum synth: no enum member facts, no enum name inventory.
        assert!(sym.enum_members.is_none());
        assert!(sym.enum_member_names.is_none());
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
            type_based_macro(AnalyzedMacroKind::DefineOptions, 0),
            type_based_macro(AnalyzedMacroKind::DefineModel, 1),
            type_based_macro(AnalyzedMacroKind::DefineExpose, 2),
        ];
        assert!(synthesise_vue_default_value_symbol(&macros).is_none());
    }
}
