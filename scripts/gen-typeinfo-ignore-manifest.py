#!/usr/bin/env python3
"""Regenerate the two-table typeinfo manifest ledger (§10).

Emits three checked-in, generated-not-hand-maintained files:

1. `crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs`
   — the 362 `IgnoredTestRow`s, each with the full 13-column schema.
2. `crates/verter_session/tests/manifest_data/typeinfo_additional_proof_rows.rs`
   — the CLOSED set of 7 coverage-only `AdditionalProofRow`s.
3. `crates/verter_session/tests/manifest_data/typeinfo_parity_blocks.rs`
   — the `TYPEINFO_PARITY_BLOCKS` DAG (every block + prereqs +
   dominant mechanism + consumed mechanisms).

The per-column data (block_id, capability, mechanism_id, …) is COMPUTED
here from the authoritative §10.4.1 row→block partition in
`docs/arch/native-typeinfo-parity.md` joined with the live
`#[ignore = "..."]` discovery — NOT hand-typed 362 times. The Rust
guard tests only diff/fail; they never write the generated source (repo
rule: generators are scripts, not tests).

Run after adding / removing / renaming an ignored test, or after the
§10.4.1 partition changes:

    python3 scripts/gen-typeinfo-ignore-manifest.py
    # or via pnpm:
    pnpm gen:typeinfo-manifest

Commit the regenerated rows alongside the source changes that prompted
the regeneration.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

# ── Per-file -> substrate mapping (carried forward; the `substrate`
#    column is preserved). ──
FILE_TO_SUBSTRATE: dict[str, str] = {
    "apparent_types.rs": "ApparentTypes",
    "basic.rs": "MacroResolution",
    "branded_types.rs": "ApparentTypes",
    "cache_invalidation.rs": "CacheInvalidation",
    "call_resolution.rs": "CallResolution",
    "class_features.rs": "ClassFeatures",
    "conditional_infer.rs": "ConditionalInfer",
    "const_type_param.rs": "TypeParameterFeatures",
    "contextual_typing.rs": "ContextualTyping",
    "cross_file.rs": "CrossFileResolution",
    "decorators.rs": "ClassFeatures",
    "deep_path.rs": "PathProjection",
    "demand_boundary.rs": "DemandBoundary",
    "enums.rs": "EnumResolution",
    "expansion_boundaries.rs": "ExpansionBoundaries",
    "flow_invalidations.rs": "FlowNarrowing",
    "flow_return_catalog.rs": "FlowNarrowing",
    "footprint.rs": "AuditFootprint",
    "function_advanced.rs": "CallResolution",
    "index_signatures.rs": "IndexSignatures",
    "indexed_utilities.rs": "UtilityComposition",
    "jsx.rs": "JsxResolution",
    "mapped_modifiers.rs": "MappedTypes",
    "mapped_template.rs": "MappedTypes",
    "menu_like.rs": "CompositeSurfaces",
    "message_list_like.rs": "CompositeSurfaces",
    "mode_boundary_invariants.rs": "ModeBoundary",
    "modern_ts_features.rs": "ModernTsFeatures",
    "module_features.rs": "ModuleFeatures",
    "narrow_discriminated_union.rs": "FlowNarrowing",
    "narrow_equality.rs": "FlowNarrowing",
    "narrow_in_operator.rs": "FlowNarrowing",
    "narrow_instanceof.rs": "FlowNarrowing",
    "narrow_truthiness.rs": "FlowNarrowing",
    "narrow_typeof.rs": "FlowNarrowing",
    "no_infer.rs": "ConditionalInfer",
    "recursive_conditional.rs": "ConditionalInfer",
    "relation_semantics.rs": "RelationSemantics",
    "substitution_types.rs": "TypeParameterFeatures",
    "table_like.rs": "CompositeSurfaces",
    "template_literal_inference.rs": "TemplateLiteralInference",
    "tuple_labels.rs": "TupleFeatures",
    "typescript_rules.rs": "TypeScriptRules",
    "union_key_access.rs": "UnionDistribution",
    "unique_symbol.rs": "UniqueSymbol",
    "utility_composition.rs": "UtilityComposition",
    "utility_edge.rs": "UtilityComposition",
    "utility_top_bottom.rs": "UtilityComposition",
    "value_inference.rs": "ValueInference",
    "variadic_tuples.rs": "TupleFeatures",
    "wide_deep.rs": "PathProjection",
}

# ── Block-id text (from §10.4.1, e.g. `U2.RELATION_INFER`) -> the Rust
#    `TypeInfoParityBlockId` variant. ──
BLOCK_TEXT_TO_VARIANT: dict[str, str] = {
    "U0.MANIFEST_SUBSTRATE": "U0ManifestSubstrate",
    "U2.QUERY_VALUE_DOMAIN": "U2QueryValueDomain",
    "U8.WIRE_SURFACE_CLOSURE": "U8WireSurfaceClosure",
    "U12.EXPORTER": "U12Exporter",
    "U13.PROJECTION": "U13Projection",
    "U2.RELATION_INFER": "U2RelationInfer",
    "U2.UTILITIES": "U2Utilities",
    "U2.INDEXED_ACCESS": "U2IndexedAccess",
    "U2.MAPPED_TEMPLATE": "U2MappedTemplate",
    "U2.CLASS_SURFACES": "U2ClassSurfaces",
    "U2.ENUMS": "U2Enums",
    "U2.MODULE_AUGMENTATION": "U2ModuleAugmentation",
    "U2.JSX_FOUNDATIONS": "U2JsxFoundations",
    "U6.FLOW_RETURN_SUBSTRATE": "U6FlowReturnSubstrate",
    "U6.NARROW_TYPEOF": "U6NarrowTypeof",
    "U6.NARROW_EQUALITY": "U6NarrowEquality",
    "U6.NARROW_TRUTHINESS": "U6NarrowTruthiness",
    "U6.NARROW_IN": "U6NarrowIn",
    "U6.NARROW_INSTANCEOF": "U6NarrowInstanceof",
    "U6.NARROW_DISCRIMINATED": "U6NarrowDiscriminated",
    "U6.NARROW_SUBSTITUTION": "U6NarrowSubstitution",
    "U6.NARROW_INVALIDATION": "U6NarrowInvalidation",
    "U6.PREDICATE_ASSERTION": "U6PredicateAssertion",
    "U6.CALL_RESOLVE": "U6CallResolve",
    "U6.CONTEXTUAL_CALLBACK": "U6ContextualCallback",
    "U6.VALUE_INFERENCE": "U6ValueInference",
    "U6.ASYNC_GENERATOR": "U6AsyncGenerator",
    "U6.CROSS_FILE": "U6CrossFile",
    "U6.LOOP_CLOSURE": "U6LoopClosure",
    "U3.CACHE_FACT_MODEL": "U3CacheFactModel",
    "U10.RESULT_DB": "U10ResultDb",
    "U11.PUBLIC_RELATION_SESSION": "U11PublicRelationSession",
    "U14.MACRO_ADAPTER": "U14MacroAdapter",
    "U15.FINAL_LIFT": "U15FinalLift",
}

# ── Each block_id -> its dominant `MechanismId` (one per block;
#    mechanism↔block is a 1:1 ownership bijection — `mechanism_owning_block`
#    is its inverse). This is the BLOCK->mechanism direction, used ONLY for
#    `BlockContractRow.mechanism_id` and to derive a block's
#    `consumed_mechanisms` from its prereqs. A ROW's `mechanism_id` is NOT
#    derived from this map (that would make DAG-guard check 2 tautological);
#    rows derive their mechanism from capability/`file::function` instead —
#    see `CAPABILITY_TO_MECHANISM` + `ROW_MECHANISM_OVERRIDE` +
#    `mechanism_for_row` below. ──
BLOCK_TO_MECHANISM: dict[str, str] = {
    "U0ManifestSubstrate": "LedgerCoverageGate",
    "U2QueryValueDomain": "QueryValueDomainFoundation",
    "U8WireSurfaceClosure": "WireSurfaceClosure",
    "U12Exporter": "ExporterPublication",
    "U13Projection": "StructuralProjectionDecode",
    "U2RelationInfer": "RelateCoinductiveScc",
    "U2Utilities": "UtilityGraphReduction",
    "U2IndexedAccess": "IndexedAccessUnionDistribution",
    "U2MappedTemplate": "MappedTemplateRemap",
    "U2ClassSurfaces": "ClassSurfaceProjection",
    "U2Enums": "EnumValueTypeDuality",
    "U2ModuleAugmentation": "ResolveDeclarationAugmentation",
    "U2JsxFoundations": "ResolveAmbientNamespaceJsx",
    "U6FlowReturnSubstrate": "ReturnPathPeekerTwoFrontier",
    "U6NarrowTypeof": "FlowNarrowingFrameTypeof",
    "U6NarrowEquality": "FlowNarrowingFrameEquality",
    "U6NarrowTruthiness": "FlowNarrowingFrameTruthiness",
    "U6NarrowIn": "FlowNarrowingFrameIn",
    "U6NarrowInstanceof": "FlowNarrowingFrameInstanceof",
    "U6NarrowDiscriminated": "FlowNarrowingFrameDiscriminated",
    "U6NarrowSubstitution": "FlowNarrowingFrameSubstitution",
    "U6NarrowInvalidation": "FlowNarrowingFrameInvalidation",
    "U6PredicateAssertion": "PredicateAssertionEffect",
    "U6CallResolve": "ResolveCallDispatch",
    "U6ContextualCallback": "ContextualCallbackInference",
    "U6ValueInference": "ValueInferenceWidening",
    "U6AsyncGenerator": "AsyncGeneratorCarrier",
    "U6CrossFile": "CrossFileRouteFact",
    "U6LoopClosure": "LoopClosureFixedPoint",
    "U3CacheFactModel": "CacheFactModelAdmission",
    "U10ResultDb": "ResultDbModeDemandExactness",
    "U11PublicRelationSession": "PublicSessionFootprintInvalidation",
    "U14MacroAdapter": "MacroSurfaceAdapter",
    "U15FinalLift": "CompositeSurfaceFinalLift",
}

# Inverse of BLOCK_TO_MECHANISM — mirrors the Rust `mechanism_owning_block`.
MECHANISM_OWNING_BLOCK: dict[str, str] = {m: b for b, m in BLOCK_TO_MECHANISM.items()}

# ── Each block_id -> its direct prerequisite block_ids (from the
#    subplan `Prerequisites:` statements). Edges restricted to parity
#    block_ids in the enum (U1/U4/U5/S5 non-parity prereqs omitted). For
#    blocks the subplans describe as depending on "the whole U2 / U6
#    parent", the edge points at the DEEPEST child block of that parent
#    (`U2.JSX_FOUNDATIONS` transitively pulls every U2 reducer +
#    `U2.QUERY_VALUE_DOMAIN`; `U6.LOOP_CLOSURE` transitively pulls every
#    U6 block) so the transitive closure equals "whole parent". ──
BLOCK_PREREQS: dict[str, list[str]] = {
    "U0ManifestSubstrate": [],
    "U2QueryValueDomain": ["U0ManifestSubstrate"],
    "U2RelationInfer": ["U2QueryValueDomain"],
    "U2IndexedAccess": ["U2QueryValueDomain", "U2RelationInfer"],
    "U2MappedTemplate": ["U2QueryValueDomain", "U2RelationInfer", "U2IndexedAccess"],
    "U2Utilities": [
        "U2QueryValueDomain",
        "U2RelationInfer",
        "U2IndexedAccess",
        "U2MappedTemplate",
    ],
    "U2ClassSurfaces": ["U2QueryValueDomain", "U2RelationInfer"],
    "U2Enums": [
        "U2QueryValueDomain",
        "U2RelationInfer",
        "U2IndexedAccess",
        "U2MappedTemplate",
    ],
    "U2ModuleAugmentation": [
        "U2QueryValueDomain",
        "U2RelationInfer",
        "U2IndexedAccess",
    ],
    "U2JsxFoundations": [
        "U2QueryValueDomain",
        "U2RelationInfer",
        "U2IndexedAccess",
        "U2Utilities",
        "U2ClassSurfaces",
        "U2ModuleAugmentation",
    ],
    "U6FlowReturnSubstrate": ["U2QueryValueDomain", "U2RelationInfer"],
    "U6NarrowTypeof": ["U6FlowReturnSubstrate"],
    "U6NarrowEquality": ["U6FlowReturnSubstrate"],
    "U6NarrowTruthiness": ["U6FlowReturnSubstrate"],
    "U6NarrowIn": ["U6FlowReturnSubstrate"],
    "U6NarrowInstanceof": ["U6FlowReturnSubstrate"],
    "U6NarrowDiscriminated": ["U6FlowReturnSubstrate"],
    "U6NarrowSubstitution": ["U6FlowReturnSubstrate"],
    "U6NarrowInvalidation": ["U6FlowReturnSubstrate"],
    "U6CallResolve": ["U6FlowReturnSubstrate", "U2RelationInfer", "U2ClassSurfaces"],
    "U6PredicateAssertion": [
        "U6FlowReturnSubstrate",
        "U6NarrowInvalidation",
        "U6NarrowSubstitution",
        "U6CallResolve",
    ],
    "U6ContextualCallback": [
        "U6CallResolve",
        "U6FlowReturnSubstrate",
        "U6NarrowDiscriminated",
    ],
    "U6ValueInference": ["U6FlowReturnSubstrate", "U6CallResolve"],
    "U6AsyncGenerator": ["U6FlowReturnSubstrate"],
    "U6CrossFile": ["U6ValueInference", "U6CallResolve"],
    "U6LoopClosure": ["U6CallResolve", "U6PredicateAssertion"],
    # Wire surface depends on U0 + the whole U2 + U6 parents.
    "U8WireSurfaceClosure": [
        "U0ManifestSubstrate",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
    "U3CacheFactModel": ["U8WireSurfaceClosure", "U2JsxFoundations", "U6LoopClosure"],
    "U10ResultDb": [
        "U3CacheFactModel",
        "U8WireSurfaceClosure",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
    "U12Exporter": [
        "U10ResultDb",
        "U8WireSurfaceClosure",
        "U3CacheFactModel",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
    "U11PublicRelationSession": [
        "U12Exporter",
        "U3CacheFactModel",
        "U8WireSurfaceClosure",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
    "U13Projection": [
        "U12Exporter",
        "U8WireSurfaceClosure",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
    "U14MacroAdapter": [
        "U13Projection",
        "U11PublicRelationSession",
        "U10ResultDb",
        "U8WireSurfaceClosure",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
    "U15FinalLift": [
        "U14MacroAdapter",
        "U13Projection",
        "U12Exporter",
        "U11PublicRelationSession",
        "U10ResultDb",
        "U3CacheFactModel",
        "U8WireSurfaceClosure",
        "U2JsxFoundations",
        "U6LoopClosure",
    ],
}

# ── block_id -> (owning_u_block, organ). ──
BLOCK_TO_UBLOCK: dict[str, str] = {
    "U0ManifestSubstrate": "U0",
    "U2QueryValueDomain": "U2",
    "U8WireSurfaceClosure": "U8",
    "U12Exporter": "U12",
    "U13Projection": "U13",
    "U2RelationInfer": "U2",
    "U2Utilities": "U2",
    "U2IndexedAccess": "U2",
    "U2MappedTemplate": "U2",
    "U2ClassSurfaces": "U2",
    "U2Enums": "U2",
    "U2ModuleAugmentation": "U2",
    "U2JsxFoundations": "U2",
    "U6FlowReturnSubstrate": "U6",
    "U6NarrowTypeof": "U6",
    "U6NarrowEquality": "U6",
    "U6NarrowTruthiness": "U6",
    "U6NarrowIn": "U6",
    "U6NarrowInstanceof": "U6",
    "U6NarrowDiscriminated": "U6",
    "U6NarrowSubstitution": "U6",
    "U6NarrowInvalidation": "U6",
    "U6PredicateAssertion": "U6",
    "U6CallResolve": "U6",
    "U6ContextualCallback": "U6",
    "U6ValueInference": "U6",
    "U6AsyncGenerator": "U6",
    "U6CrossFile": "U6",
    "U6LoopClosure": "U6",
    "U3CacheFactModel": "U3",
    "U10ResultDb": "U10",
    "U11PublicRelationSession": "U11",
    "U14MacroAdapter": "U14",
    "U15FinalLift": "U15",
}

BLOCK_TO_ORGAN: dict[str, str] = {
    "U0ManifestSubstrate": "LedgerSubstrate",
    "U2QueryValueDomain": "QueryValueDomain",
    "U8WireSurfaceClosure": "WireSurface",
    "U12Exporter": "Exporter",
    "U13Projection": "Projection",
    "U2RelationInfer": "RelationInferenceEngine",
    "U2Utilities": "TypeConstructorReducers",
    "U2IndexedAccess": "TypeConstructorReducers",
    "U2MappedTemplate": "TypeConstructorReducers",
    "U2ClassSurfaces": "TypeConstructorReducers",
    "U2Enums": "TypeConstructorReducers",
    "U2ModuleAugmentation": "TypeConstructorReducers",
    "U2JsxFoundations": "TypeConstructorReducers",
    "U6FlowReturnSubstrate": "FlowCallSolver",
    "U6NarrowTypeof": "FlowCallSolver",
    "U6NarrowEquality": "FlowCallSolver",
    "U6NarrowTruthiness": "FlowCallSolver",
    "U6NarrowIn": "FlowCallSolver",
    "U6NarrowInstanceof": "FlowCallSolver",
    "U6NarrowDiscriminated": "FlowCallSolver",
    "U6NarrowSubstitution": "FlowCallSolver",
    "U6NarrowInvalidation": "FlowCallSolver",
    "U6PredicateAssertion": "FlowCallSolver",
    "U6CallResolve": "FlowCallSolver",
    "U6ContextualCallback": "FlowCallSolver",
    "U6ValueInference": "FlowCallSolver",
    "U6AsyncGenerator": "FlowCallSolver",
    "U6CrossFile": "FlowCallSolver",
    "U6LoopClosure": "FlowCallSolver",
    "U3CacheFactModel": "CacheFactModel",
    "U10ResultDb": "ResultDb",
    "U11PublicRelationSession": "PublicSession",
    "U14MacroAdapter": "FrameworkAdapter",
    "U15FinalLift": "FinalLift",
}

# ── Per-block REQUIRED GUARDS (§9 / §11.5) ──
#
# `BlockContractRow.required_guards` lists the named guards a block must
# carry green before it lands (the §11.5 `landed_typeinfo_blocks_have_
# required_guards` done-predicate keys off this list; that landing
# enforcement guard itself lands in a later substrate block). The labels are
# the guard NAMES the block's contract in `docs/arch/native-typeinfo-parity*`
# / `native-flow-return.md` declares — forward-declared for not-yet-landed
# blocks (consistent with the AdditionalProofRow RowTestGuard
# forward-declarations). Every list leads with the universal
# `typeinfo_parity_block_dag_…` guard, which validates EVERY block's
# prereq/mechanism/key metadata, then the block-specific guards from its
# contract. The eight `U6.NARROW_*` sub-blocks share the one narrowing-lattice
# substrate's three shared guards (native-flow-return.md "Shared required new
# guards"), cited by every sub-block rather than restated.
_DAG = "typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs"
_NARROW_SHARED = [
    "narrowing_facts_compose_in_predicate_keyed_frames",
    "narrowing_facts_are_program_analysis_not_graph_type_nodes",
    "array_isarray_narrowing_reads_lib_intrinsic_not_text",
]
BLOCK_TO_REQUIRED_GUARDS: dict[str, list[str]] = {
    "U0ManifestSubstrate": [
        _DAG,
        "ignored_test_row_table_holds_exactly_362_rows",
        "additional_proof_row_table_holds_exactly_7_rows",
        "semantic_query_name_mirror_matches_live_tag_set",
    ],
    "U2QueryValueDomain": [
        _DAG,
        "semantic_query_key_spec_table_equals_enum",
        "query_modes_are_presets_over_projection_demand_eval_policy",
        "skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode",
    ],
    "U8WireSurfaceClosure": [
        _DAG,
        "node_taxonomy_complete",
        "no_non_type_value_smuggled_into_graph_type_node",
        "all_public_semantic_type_graph_embeddings_are_payload_wrapped",
    ],
    "U12Exporter": [
        _DAG,
        "no_non_type_value_smuggled_into_graph_type_node",
        "program_analysis_graph_exposes_flow_contextual_queries",
        "relation_proofs_not_graph_type_nodes",
    ],
    "U13Projection": [
        _DAG,
        "capability_rows_map_to_expected_query_fact_mechanisms",
    ],
    "U2RelationInfer": [
        _DAG,
        "relation_negative_and_unknown_paths_are_fast",
        "relation_cycle_sentinel_is_never_warm_admitted",
        "no_infer_not_type_parameter_metadata",
    ],
    "U2Utilities": [
        _DAG,
        "pick_never_yields_empty_object",
        "keyspace_budget_exceeded_admits_nothing",
    ],
    "U2IndexedAccess": [
        _DAG,
        "keyspace_budget_exceeded_admits_nothing",
    ],
    "U2MappedTemplate": [
        _DAG,
        "template_literal_reduce_models_ts_numeric_bigint_lexing",
        "mapped_modifier_minus_optional_strips_optional_and_undefined",
    ],
    "U2ClassSurfaces": [
        _DAG,
        "accessor_decorator_publishes_public_property",
        "accessor_decorator_identity_target_return_keeps_public_property",
    ],
    "U2Enums": [
        _DAG,
        "resolve_enum_do_not_warm_hit",
    ],
    "U2ModuleAugmentation": [
        _DAG,
        "declaration_merge_records_binder_overload_augmentation_order_as_facts",
        "session_overlay_augmentation_fails_closed_until_implemented",
        "node_taxonomy_complete",
    ],
    "U2JsxFoundations": [
        _DAG,
        "jsx_resolution_uses_existing_semantic_queries",
        "jsx_no_dedicated_graph_type_node",
    ],
    "U6FlowReturnSubstrate": [
        _DAG,
        "flow_slice_is_graph_reachability_not_procedural_walk",
        "flow_slice_ir_detaches_from_oxc_arena",
        "substitution_env_canonical_hash_is_order_independent",
    ],
    "U6NarrowTypeof": [_DAG, *_NARROW_SHARED],
    "U6NarrowEquality": [_DAG, *_NARROW_SHARED],
    "U6NarrowTruthiness": [_DAG, *_NARROW_SHARED],
    "U6NarrowIn": [_DAG, *_NARROW_SHARED],
    "U6NarrowInstanceof": [_DAG, *_NARROW_SHARED],
    "U6NarrowDiscriminated": [_DAG, *_NARROW_SHARED],
    "U6NarrowSubstitution": [_DAG, *_NARROW_SHARED],
    "U6NarrowInvalidation": [_DAG, *_NARROW_SHARED],
    "U6PredicateAssertion": [
        _DAG,
        "predicate_assertion_effect_is_signature_metadata_not_published_type_node",
        "flow_invalidations_fi08_asserts_narrows_dotted_member_path",
        "predicate_signature_without_body_audits_signature_only_outcome",
    ],
    "U6CallResolve": [
        _DAG,
        "resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context",
        "cross_engine_cycle_discharge_admits_only_stable_deterministic_results",
        "checker_reentry_graph_spans_flow_call_contextual_narrowing",
    ],
    "U6ContextualCallback": [
        _DAG,
        "this_type_contextual_object_literal_binding_in_contextual_type_at",
        "only_completed_deterministic_sessions_are_admitted",
        "callback_contextual_typing_does_not_pollute_caller_frame",
    ],
    "U6ValueInference": [
        _DAG,
        "freshness_tracks_per_property_spread_taint",
        "satisfies_widens_inner_value_to_primitive_without_as_const",
    ],
    "U6AsyncGenerator": [
        _DAG,
        "await_using_simulated_return_type_resolves_to_primitive",
        "lib_env_hash_drives_generator_return_resolution",
    ],
    "U6CrossFile": [
        _DAG,
        "flow_cycle_sentinel_is_never_admitted_as_cache_entry",
        "flow_cycle_sentinel_does_not_hide_real_base_return_contributor",
        "cross_file_recursion_terminates_with_audit_event",
    ],
    "U6LoopClosure": [
        _DAG,
        "flow_policy_differentiates_cache_candidates",
        "divergent_loop_models_as_void",
        "no_caching_of_partial_or_budget_exceeded_results",
    ],
    "U3CacheFactModel": [
        _DAG,
        "relation_budget_exceeded_admits_nothing",
        "keyspace_budget_exceeded_admits_nothing",
        "program_analysis_fact_domain_validates_flow_slice",
    ],
    "U10ResultDb": [
        _DAG,
        "cache_satisfaction_is_demand_lattice_not_enum_order",
        "cache_candidate_cap_is_per_family_not_uniform",
        "family_eviction_prefers_invalid_then_lru_valid_hit",
    ],
    "U11PublicRelationSession": [
        _DAG,
        "relation_proofs_not_graph_type_nodes",
        "typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node",
    ],
    "U14MacroAdapter": [
        _DAG,
        "component_meta_is_thin_framework_adapter_no_second_resolver",
    ],
    "U15FinalLift": [
        _DAG,
        "all_typeinfo_parity_rows_lifted_except_stop_gates",
        "no_landed_typeinfo_block_has_live_ignored_rows",
        "bench_result_row_reports_cache_mode_sourcemap_batch_thread_hit_fallback",
    ],
}

# ── Per-block VERIFICATION COMMAND LABELS (§9) ──
#
# The verification commands a block runs before it lands. PARENT-UNIFORM
# (native-typeinfo-parity.md §11.11/§11.12 + the per-subplan "Verification
# commands"): every block runs the manifest coverage gate plus the full
# workspace gate (the canonical agent gate — nextest for completeness + the
# shared-process verter_session surface + clippy + fmt). Carried as a label
# list so the contract row records WHAT must pass, not a free-text blob.
BLOCK_VERIFICATION_LABELS: list[str] = [
    "cargo test -p verter_session --test typeinfo_ignored_test_manifest",
    "cargo nextest run --workspace",
    "cargo test -p verter_session --tests",
    "cargo clippy --workspace -- -D warnings",
    "cargo fmt --all --check",
]

# ── ROW-LEVEL mechanism, INDEPENDENT of the `block_id` column ──
#
# A row's dominant `mechanism_id` is its ROW-LEVEL mechanism (§10.4 /
# §10.4.1 / §11.5): the row's `file::function` mechanism is what FIXES
# its owning block, NOT the other way round. So the mechanism MUST be
# derived independently of `block_id`; otherwise the DAG guard's check 2
# (`mechanism_owning_block(mechanism_id) == block_id`) is tautological
# and cannot catch a mis-assigned block.
#
# The mechanism is derived from `(capability [, file::function override
# for capabilities that SPLIT across blocks])`:
#
# - `CAPABILITY_TO_MECHANISM` — the dominant mechanism for every
#   SINGLE-BLOCK capability (one block in the §10.4.1 partition).
# - `ROW_MECHANISM_OVERRIDE` — the per-row mechanism for every row of a
#   capability that SPLITS across blocks by its `file::function`
#   mechanism (e.g. `TypeScriptRules` rows land in
#   `U2.RELATION_INFER` / `U2.UTILITIES` / `U2.INDEXED_ACCESS` /
#   `U2.MAPPED_TEMPLATE` / `U2.CLASS_SURFACES` by what each row's rule
#   actually dispatches; `FlowNarrowing` splits across the eight
#   `U6.NARROW_*` sub-blocks + `U6.PREDICATE_ASSERTION` /
#   `U6.CONTEXTUAL_CALLBACK` / `U6.CROSS_FILE` / `U6.LOOP_CLOSURE`;
#   `fi08`'s dominant mechanism is `PredicateAssertionEffect`, §11.5).
#
# Neither map reads the `block_id` column, so the resulting
# `mechanism_id` is independent of `block_id` and check 2 genuinely
# discriminates: the cross-check `mechanism_for_row` below asserts the
# two independent sources AGREE on the correct table, and an injected
# wrong `block_id` (mismatching its mechanism's owner) FAILS the guard.
#
# The keys for a row are a property of its MECHANISM (§10.4 — "the
# SemanticQueryName set the row's MECHANISM dispatches/reads"), via
# `MECHANISM_TO_KEYS`, NOT a per-block-narrowed subset of a broad
# capability key set. The full mechanism key set is emitted verbatim
# (no reachability pre-filter — that pre-filter was the F3 self-fulfilling
# defect), and every mechanism's keys are reachable from its owning block
# for the correct table (asserted at generation time).
CAPABILITY_TO_MECHANISM: dict[str, str] = {
    "ApparentTypes": "ClassSurfaceProjection",
    "AuditFootprint": "PublicSessionFootprintInvalidation",
    "CacheInvalidation": "PublicSessionFootprintInvalidation",
    "ClassFeatures": "ClassSurfaceProjection",
    "CompositeSurfaces": "CompositeSurfaceFinalLift",
    "ConditionalInfer": "RelateCoinductiveScc",
    "ContextualTyping": "ContextualCallbackInference",
    "CrossFileResolution": "CacheFactModelAdmission",
    "EnumResolution": "EnumValueTypeDuality",
    "ExpansionBoundaries": "ResultDbModeDemandExactness",
    "IndexSignatures": "IndexedAccessUnionDistribution",
    "JsxResolution": "ResolveAmbientNamespaceJsx",
    "MacroResolution": "MacroSurfaceAdapter",
    "MappedTypes": "MappedTemplateRemap",
    "ModeBoundary": "ResultDbModeDemandExactness",
    "ModuleFeatures": "ResolveDeclarationAugmentation",
    "PathProjection": "IndexedAccessUnionDistribution",
    "RelationSemantics": "RelateCoinductiveScc",
    "TemplateLiteralInference": "MappedTemplateRemap",
    "TupleFeatures": "UtilityGraphReduction",
    "UnionDistribution": "IndexedAccessUnionDistribution",
    "UniqueSymbol": "ClassSurfaceProjection",
    "UtilityComposition": "UtilityGraphReduction",
    "ValueInference": "ReturnPathPeekerTwoFrontier",
}

# Capabilities whose rows SPLIT across blocks by their `file::function`
# mechanism — every such row MUST carry a `ROW_MECHANISM_OVERRIDE`
# entry. These do NOT appear in `CAPABILITY_TO_MECHANISM`.
SPLIT_CAPABILITIES: set[str] = {
    "CallResolution",
    "DemandBoundary",
    "FlowNarrowing",
    "ModernTsFeatures",
    "TypeParameterFeatures",
    "TypeScriptRules",
}

# Per-`(file, function)` mechanism for every row of a split capability.
# Authored from the §10.4.1 partition's mechanism-named sub-blocks
# (`U6.NARROW_TYPEOF` = `typeof` narrowing = `FlowNarrowingFrameTypeof`,
# …) + the §11.5 fi08 statement. Keyed by `file::function`, INDEPENDENT
# of the `block_id` column.
ROW_MECHANISM_OVERRIDE: dict[tuple[str, str], str] = {
    ("modern_ts_features.rs", "satisfies_array_literal_widens_to_primitive_array"): "RelateCoinductiveScc",
    ("typescript_rules.rs", "typescript_rules_distributive_conditional_expands_each_union_arm"): "RelateCoinductiveScc",
    ("typescript_rules.rs", "typescript_rules_awaited_recursively_unwraps_promises"): "UtilityGraphReduction",
    ("typescript_rules.rs", "typescript_rules_indexed_access_reduces_terminal_property"): "IndexedAccessUnionDistribution",
    ("typescript_rules.rs", "typescript_rules_keyof_materializes_literal_key_union"): "IndexedAccessUnionDistribution",
    ("typescript_rules.rs", "typescript_rules_tuple_rest_element_resolves_array_element_type"): "IndexedAccessUnionDistribution",
    ("typescript_rules.rs", "typescript_rules_key_remap_exclude_filters_and_renames_keys"): "MappedTemplateRemap",
    ("typescript_rules.rs", "typescript_rules_template_intrinsic_evaluates_union"): "MappedTemplateRemap",
    ("call_resolution.rs", "call_resolution_abstract_constructor_instance_type_projects_class_shape"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_call_construct_hybrid_instance_type_uses_construct_signature"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_call_construct_hybrid_parameters_uses_call_signature"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_call_construct_hybrid_return_type_uses_call_signature"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_class_method_prototype_extraction_projects_parameters"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_class_method_prototype_extraction_projects_return"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_constructor_parameters_publishes_constructor_arg_tuple"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_instance_type_publishes_constructor_return_shape"): "ClassSurfaceProjection",
    ("function_advanced.rs", "function_advanced_return_type_of_overloaded_function_uses_last_overload"): "ClassSurfaceProjection",
    ("modern_ts_features.rs", "variance_annotation_in_substitution_through_consumer_consume_parameters"): "ClassSurfaceProjection",
    ("substitution_types.rs", "substitution_types_sb14_default_type_arg_ignored_by_return_type"): "ClassSurfaceProjection",
    ("substitution_types.rs", "substitution_types_sb15_recursive_generic_substitution"): "ClassSurfaceProjection",
    ("typescript_rules.rs", "typescript_rules_class_instance_type_includes_fields_and_methods"): "ClassSurfaceProjection",
    ("typescript_rules.rs", "typescript_rules_constructor_parameters_resolve_tuple"): "ClassSurfaceProjection",
    ("typescript_rules.rs", "typescript_rules_instance_type_resolves_constructed_object"): "ClassSurfaceProjection",
    ("typescript_rules.rs", "typescript_rules_typeof_const_preserves_readonly_literals"): "ClassSurfaceProjection",
    ("modern_ts_features.rs", "import_attribute_simulated_resolves_imported_json_shape"): "ResolveDeclarationAugmentation",
    ("modern_ts_features.rs", "import_attribute_simulated_string_literal_indexed_member"): "ResolveDeclarationAugmentation",
    ("narrow_typeof.rs", "narrow_typeof_nt01_string_on_binary_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt02_number_on_triple_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt03_boolean_on_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt04_object_on_union_keeps_no_null"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt05_function_on_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt06_undefined_on_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt07_bigint_on_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt08_symbol_on_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt09_string_on_unknown"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt10_string_on_unbound_generic"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt11_negated_on_binary_union"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt12_switch_exhaustive"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt13_negated_guard_early_return"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt14_compare_literal_var_does_not_narrow"): "FlowNarrowingFrameTypeof",
    ("narrow_typeof.rs", "narrow_typeof_nt15_compound_and_property"): "FlowNarrowingFrameTypeof",
    ("narrow_equality.rs", "narrow_equality_eq01_string_literal_on_literal_union"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq02_negated_string_literal_on_literal_union"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq03_number_literal_on_triple_union"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq04_boolean_true_on_boolean"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq05_null_on_nullable_string"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq06_undefined_on_optional_string"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq07_double_equals_null_on_nullish_string"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq08_string_literal_on_string_does_not_narrow"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq09_string_literal_on_primitive_union"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq10_two_unions_mutual_equality_does_not_narrow"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq11_impossible_compound_absorbs_never"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq12_property_equality_discriminant"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq13_as_const_literal_rhs"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq14_number_literal_on_number_does_not_narrow"): "FlowNarrowingFrameEquality",
    ("narrow_equality.rs", "narrow_equality_eq15_nan_equality_does_not_narrow"): "FlowNarrowingFrameEquality",
    ("narrow_truthiness.rs", "narrow_truthiness_tr01_string_or_undefined"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr02_string_or_null"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr03_string_or_nullish"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr04_string_no_nullable_does_not_narrow"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr05_number_literal_union"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr06_string_literal_union"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr07_boolean_union"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr08_negated_string_or_undefined"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr09_property_truthiness"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr10_early_return_guard"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr11_unknown_collapses_to_unknown"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr12_object_or_null"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr13_compound_and_chain"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr14_number_or_undefined_does_not_split_zero"): "FlowNarrowingFrameTruthiness",
    ("narrow_truthiness.rs", "narrow_truthiness_tr15_optional_chain_truthiness"): "FlowNarrowingFrameTruthiness",
    ("narrow_in_operator.rs", "narrow_in_operator_io01_binary_union"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io02_shared_key"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io03_else_branch"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io04_intersection"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io05_optional_property"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io06_on_unknown"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io07_on_object"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io08_compound_conjunction"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io09_negated"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io10_three_arm_union"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io11_generic_constrained"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io12_reassignment_renarrowing"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io13_class_vs_object"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io14_template_literal_key"): "FlowNarrowingFrameIn",
    ("narrow_in_operator.rs", "narrow_in_operator_io15_symbol_key"): "FlowNarrowingFrameIn",
    ("narrow_instanceof.rs", "narrow_instanceof_in01_binary_union"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in02_class_plus_primitive"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in03_on_unknown"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in04_subclass_union"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in05_already_narrowed"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in06_abstract_class"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in07_else_reachability"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in08_interface_union"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in09_negated_early_return"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in10_intersection"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in11_generic_ctor"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in13_array_special_case"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in14_promise_special_case"): "FlowNarrowingFrameInstanceof",
    ("narrow_instanceof.rs", "narrow_instanceof_in15_nullable"): "FlowNarrowingFrameInstanceof",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du01_if_equality_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du02_switch_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du03_switch_default_never"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du04_negated_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du05_multi_property_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du06_nested_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du07_number_literal_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du08_boolean_literal_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du09_destructure_correlation"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du10_in_guard_plus_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du11_switch_per_arm_join"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du12_switch_fall_through"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du14_reassignment_re_narrowing"): "FlowNarrowingFrameDiscriminated",
    ("narrow_discriminated_union.rs", "narrow_discriminated_union_du15_template_literal_discriminant"): "FlowNarrowingFrameDiscriminated",
    ("substitution_types.rs", "substitution_types_sb01_bare_narrowing_of_generic"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb02_narrowing_in_constrained_generic"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb03_substitution_survives_method_calls"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb04_narrowed_substitution_to_return_position"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb05_compound_typeof_and_instanceof"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb06_narrowing_widens_after_reassignment"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb07_constraint_flow_apparent_type"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb08_generic_in_conditional_no_distribute_on_unknown"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb11_generic_narrowed_via_in_operator"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb12_truthiness_on_t_or_undefined"): "FlowNarrowingFrameSubstitution",
    ("substitution_types.rs", "substitution_types_sb13_substitution_carried_across_destructure"): "FlowNarrowingFrameSubstitution",
    ("flow_invalidations.rs", "flow_invalidations_fi01_reassignment_invalidates_string_narrowing"): "FlowNarrowingFrameInvalidation",
    ("flow_invalidations.rs", "flow_invalidations_fi02_narrowing_preserved_across_opaque_call"): "FlowNarrowingFrameInvalidation",
    ("flow_invalidations.rs", "flow_invalidations_fi04_destructured_discriminant_preserves_correlation"): "FlowNarrowingFrameInvalidation",
    ("flow_invalidations.rs", "flow_invalidations_fi05_destructured_discriminant_loses_on_reassignment"): "FlowNarrowingFrameInvalidation",
    ("flow_invalidations.rs", "flow_invalidations_fi09_exhaustive_never_tail_does_not_widen_return"): "FlowNarrowingFrameInvalidation",
    ("flow_invalidations.rs", "flow_invalidations_fi08_asserts_narrows_dotted_member_path"): "PredicateAssertionEffect",
    ("substitution_types.rs", "substitution_types_sb09_asserts_x_is_string_on_generic"): "PredicateAssertionEffect",
    ("substitution_types.rs", "substitution_types_sb10_x_is_t_predicate_on_generic"): "PredicateAssertionEffect",
    ("call_resolution.rs", "call_resolution_extracted_prototype_method_call_returns_declared_return"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_generic_infers_from_callback_return_type"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_generic_infers_from_positional_argument_through_callback_signature"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_generic_infers_object_literal_including_excess_properties"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_optional_overload_picks_first_arity_matching_signature"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_optional_overload_picks_two_arg_signature_when_required"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_rest_overload_picks_rest_signature_when_required"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_specific_literal_argument_picks_matching_overload_first"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_specific_literal_argument_skips_non_matching_first_overload"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_this_receiver_method_call_returns_declared_return"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_union_argument_picks_union_compatible_overload"): "ResolveCallDispatch",
    ("const_type_param.rs", "const_type_param_route_call_preserves_readonly_tuple_with_literal_paths"): "ResolveCallDispatch",
    ("const_type_param.rs", "const_type_param_string_call_preserves_readonly_literal_string_tuple"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_constrained_generic_infers_literal_under_as_const"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_higher_order_composition_returns_concrete_function"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_omit_this_parameter_returns_function_without_this"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_overload_call_picks_matching_signature_return"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_overload_generic_first_binds_to_literal_argument"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_overload_generic_first_widens_t_to_string_for_string_argument"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_this_parameter_type_returns_this_annotation"): "ResolveCallDispatch",
    ("function_advanced.rs", "function_advanced_void_callback_return_preserves_void"): "ResolveCallDispatch",
    ("call_resolution.rs", "call_resolution_contextual_callback_return_picks_first_overload"): "ContextualCallbackInference",
    ("flow_return_catalog.rs", "flow_return_ho09_keeps_unknown_declared_callback_result_opaque"): "ContextualCallbackInference",
    ("modern_ts_features.rs", "satisfies_widens_inner_value_to_primitive_without_as_const"): "ValueInferenceWidening",
    ("modern_ts_features.rs", "await_using_simulated_return_type_resolves_to_primitive"): "AsyncGeneratorCarrier",
    ("flow_return_catalog.rs", "flow_return_xf02_expands_imported_value_function_return"): "CrossFileRouteFact",
    ("flow_return_catalog.rs", "flow_return_xf04_expands_barrel_imported_value_function_return"): "CrossFileRouteFact",
    ("flow_return_catalog.rs", "flow_return_xf04_records_barrel_route_before_selected_leaf"): "CrossFileRouteFact",
    ("flow_return_catalog.rs", "flow_return_xf05_resolves_namespace_import_value_call"): "CrossFileRouteFact",
    ("flow_return_catalog.rs", "flow_return_xf06_keeps_value_type_namespace_separate"): "CrossFileRouteFact",
    ("flow_return_catalog.rs", "flow_return_xf09_terminates_cross_file_recursive_returns"): "CrossFileRouteFact",
    ("flow_invalidations.rs", "flow_invalidations_fi03_closure_capture_preserves_narrowing_at_return"): "LoopClosureFixedPoint",
    ("flow_invalidations.rs", "flow_invalidations_fi06_finally_return_overrides_try_catch_returns"): "LoopClosureFixedPoint",
    ("flow_invalidations.rs", "flow_invalidations_fi07_finally_without_return_preserves_try_catch"): "LoopClosureFixedPoint",
    ("demand_boundary.rs", "demand_boundary_projection_into_selected_alias_loads_needed_but_not_unused"): "ResultDbModeDemandExactness",
    ("demand_boundary.rs", "demand_boundary_terminal_projection_resolves_value_without_unused_branch"): "ResultDbModeDemandExactness",
    ("demand_boundary.rs", "demand_boundary_barrel_resolution_does_not_load_unrequested_reexport"): "PublicSessionFootprintInvalidation",
}

# ── mechanism -> the live `SemanticQueryName`s that MECHANISM
#    dispatches/reads (§10.4). The FULL set per mechanism (no per-block
#    narrowing). For the correct table every key's owner is reachable
#    from the mechanism's owning block — asserted at generation time. ──
MECHANISM_TO_KEYS: dict[str, list[str]] = {
    # Zero-row substrate mechanisms.
    "LedgerCoverageGate": [],
    "QueryValueDomainFoundation": [
        "ResolveDecl",
        "TypeOf",
        "NormalizeUnion",
        "NormalizeIntersection",
        "ResolvedNamedType",
    ],
    "WireSurfaceClosure": [],
    "ExporterPublication": [],
    "StructuralProjectionDecode": [],
    # U2 reducer mechanisms.
    "RelateCoinductiveScc": ["Relate", "Conditional", "Instantiate", "ResolveDecl"],
    "UtilityGraphReduction": ["Instantiate", "IndexedAccess", "KeyOf", "ResolveDecl"],
    "IndexedAccessUnionDistribution": [
        "IndexedAccess",
        "KeyOf",
        "ProjectMember",
        "ProjectPath",
        "ResolveDecl",
    ],
    "MappedTemplateRemap": ["MappedType", "KeyOf", "Instantiate", "Conditional", "ResolveDecl"],
    "ClassSurfaceProjection": ["ResolveDecl", "Instantiate", "Relate"],
    "EnumValueTypeDuality": ["ResolveDecl", "KeyOf", "TypeOf"],
    "ResolveDeclarationAugmentation": ["ResolveDecl", "IndexedAccess"],
    "ResolveAmbientNamespaceJsx": ["ResolveDecl", "IndexedAccess", "KeyOf"],
    # U6 flow / call mechanisms.
    "ReturnPathPeekerTwoFrontier": ["TypeOf", "ResolveDecl"],
    "FlowNarrowingFrameTypeof": ["ResolveDecl", "Relate"],
    "FlowNarrowingFrameEquality": ["ResolveDecl", "Relate"],
    "FlowNarrowingFrameTruthiness": ["ResolveDecl", "Relate"],
    "FlowNarrowingFrameIn": ["ResolveDecl", "Relate"],
    "FlowNarrowingFrameInstanceof": ["ResolveDecl", "Relate"],
    "FlowNarrowingFrameDiscriminated": ["ResolveDecl", "Relate"],
    "FlowNarrowingFrameSubstitution": ["Instantiate", "Relate", "ResolveDecl"],
    "FlowNarrowingFrameInvalidation": ["ResolveDecl", "Relate"],
    "PredicateAssertionEffect": ["ResolveDecl", "Relate", "Instantiate"],
    "ResolveCallDispatch": ["ResolveDecl", "Instantiate", "Relate"],
    "ContextualCallbackInference": ["ResolveDecl", "Relate", "Instantiate"],
    "ValueInferenceWidening": ["TypeOf", "ResolveDecl"],
    "AsyncGeneratorCarrier": ["ResolveDecl", "Instantiate"],
    "CrossFileRouteFact": ["ResolveDecl"],
    "LoopClosureFixedPoint": ["ResolveDecl", "Relate"],
    # Cache / result / session / adapter mechanisms.
    "CacheFactModelAdmission": ["ResolveDecl"],
    "ResultDbModeDemandExactness": ["ResolveDecl", "ProjectPath"],
    "PublicSessionFootprintInvalidation": ["ResolveDecl"],
    "MacroSurfaceAdapter": ["ResolveDecl", "Instantiate", "ResolveMacroPayload"],
    "CompositeSurfaceFinalLift": ["ResolveDecl", "ProjectPath", "Instantiate"],
}


def mechanism_for_row(cap: str, file_: str, fn_name: str) -> str:
    """A row's dominant `mechanism_id`, derived from
    `(capability [, file::function override])` and INDEPENDENT of the
    `block_id` column. Split capabilities MUST carry a
    `ROW_MECHANISM_OVERRIDE` entry; single-block capabilities resolve
    through `CAPABILITY_TO_MECHANISM`."""
    if cap in SPLIT_CAPABILITIES:
        key = (file_, fn_name)
        if key not in ROW_MECHANISM_OVERRIDE:
            raise SystemExit(
                f"split-capability row {file_}::{fn_name} (capability {cap!r}) "
                f"has no ROW_MECHANISM_OVERRIDE entry — author its row-level "
                f"mechanism from §10.4.1 (do NOT fall back to a block-derived "
                f"placeholder)"
            )
        return ROW_MECHANISM_OVERRIDE[key]
    if cap not in CAPABILITY_TO_MECHANISM:
        raise SystemExit(
            f"capability {cap!r} is neither a split capability nor in "
            f"CAPABILITY_TO_MECHANISM — add its row-level mechanism"
        )
    return CAPABILITY_TO_MECHANISM[cap]

# ── capability -> its `ProofRequirement`. Oracle-pinnable capabilities
#    use `Ts7Oracle`; the mode/demand/expansion/cache/footprint/cross-
#    file capabilities use structural / negative guards (they are NOT
#    TS-oracle rows — §10.2). ──
def proof_for_capability(cap: str) -> str:
    oracle = {
        "UtilityComposition": "UtilityComposition",
        "MappedTypes": "MappedTemplate",
        "TemplateLiteralInference": "TemplateLiteral",
        "IndexSignatures": "IndexedAccess",
        "PathProjection": "IndexedAccess",
        "UnionDistribution": "IndexedAccess",
        "EnumResolution": "EnumProjection",
        "ClassFeatures": "ClassSurface",
        "ApparentTypes": "ApparentType",
        "UniqueSymbol": "ApparentType",
        "TupleFeatures": "TupleProjection",
        "ConditionalInfer": "ConditionalInfer",
        "RelationSemantics": "RelationSemantics",
        "FlowNarrowing": "FlowNarrowing",
        "CallResolution": "CallResolution",
        "ContextualTyping": "ContextualTyping",
        "ValueInference": "ValueInference",
        "JsxResolution": "JsxResolution",
        "ModuleFeatures": "ModuleAugmentation",
        "CompositeSurfaces": "CompositeSurface",
        "TypeParameterFeatures": "RelationSemantics",
        "TypeScriptRules": "RelationSemantics",
        "ModernTsFeatures": "RelationSemantics",
        "MacroResolution": "CompositeSurface",
        "CrossFileResolution": "RelationSemantics",
    }
    guard = {
        "ModeBoundary": "ModeBoundaryExactness",
        "ExpansionBoundaries": "ExpansionBoundaryPrecision",
        "DemandBoundary": "DemandBoundaryPrecision",
        "CacheInvalidation": "CacheInvalidationRoute",
        "AuditFootprint": "AuditFootprintAttachment",
    }
    if cap in guard:
        return f"ProofRequirement::StructuralGuard(GuardId::{guard[cap]})"
    if cap in oracle:
        return f"ProofRequirement::Ts7Oracle(OracleId::{oracle[cap]})"
    raise SystemExit(f"no ProofRequirement mapping for capability {cap!r}")


def consumed_mechs_for_block(block_var: str) -> list[str]:
    """A row/block's consumed mechanisms = the dominant mechanisms of
    its block's DIRECT prerequisites (each a transitive prereq, so the
    DAG guard's check 3 holds)."""
    return [BLOCK_TO_MECHANISM[p] for p in BLOCK_PREREQS[block_var]]


# ── live `SemanticQueryName` -> its owning block_id (mirror of the Rust
#    `key_owning_block`). A row may only consume a key whose owner is the
#    row's own block or a transitive prerequisite (DAG guard check 4). ──
KEY_OWNING_BLOCK: dict[str, str] = {
    "ResolveDecl": "U2QueryValueDomain",
    "TypeOf": "U2QueryValueDomain",
    "NormalizeUnion": "U2QueryValueDomain",
    "NormalizeIntersection": "U2QueryValueDomain",
    "ResolvedNamedType": "U2QueryValueDomain",
    "ApparentType": "U2QueryValueDomain",
    "Relate": "U2RelationInfer",
    "Instantiate": "U2RelationInfer",
    "Conditional": "U2RelationInfer",
    "IndexedAccess": "U2IndexedAccess",
    "KeyOf": "U2IndexedAccess",
    "ProjectMember": "U2IndexedAccess",
    "ProjectPath": "U2IndexedAccess",
    "MappedType": "U2MappedTemplate",
    "TemplateLiteralReduce": "U2MappedTemplate",
    "ResolveClassSurface": "U2ClassSurfaces",
    "ResolveEnum": "U2Enums",
    "ResolveAmbientNamespace": "U2JsxFoundations",
    "ResolveOverloadSet": "U6CallResolve",
    "FlowNarrowingAt": "U6FlowReturnSubstrate",
    "ContextualTypeAt": "U6ContextualCallback",
    "ResolveMacroPayload": "U14MacroAdapter",
}


def _reaches(from_block: str, target: str) -> bool:
    """Is `target` == `from_block` or a transitive prerequisite of it?"""
    if from_block == target:
        return True
    seen: set[str] = set()
    frontier = [from_block]
    while frontier:
        cur = frontier.pop()
        if cur in seen:
            continue
        seen.add(cur)
        if cur == target:
            return True
        frontier.extend(BLOCK_PREREQS.get(cur, []))
    return False


def keys_for_row(mech: str) -> list[str]:
    """The row's `semantic_queries`: the FULL set of keys its MECHANISM
    dispatches/reads (§10.4), emitted verbatim with NO per-block
    narrowing. The old reachability pre-filter was the F3 self-fulfilling
    defect — it let a row with a wrong block_id pass DAG-guard check 4 by
    silently dropping the incriminating key. The key set is now a fixed
    property of the mechanism; check 4 validates it honestly against the
    row's block prereqs."""
    return MECHANISM_TO_KEYS[mech]


def escape_rust_string_literal(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"')


def extract_sites(source: str) -> list[tuple[str, str]]:
    """Return `(reason, fn_name)` for every literal-string
    `#[ignore = "..."]` site in `source`."""
    sites: list[tuple[str, str]] = []
    lines = source.splitlines()
    for i, raw in enumerate(lines):
        line = raw.strip()
        if not line.startswith("#[ignore"):
            continue
        rest = line[len("#[ignore"):].lstrip()
        if not rest.startswith("=") or '"' not in rest:
            continue
        m = re.search(r'"((?:[^"\\]|\\.)*)"', rest)
        if not m:
            continue
        reason = m.group(1)
        fn_name: str | None = None
        for j in range(i + 1, min(i + 6, len(lines))):
            fm = re.search(r"fn\s+(\w+)", lines[j])
            if fm:
                fn_name = fm.group(1)
                break
        if fn_name:
            sites.append((reason, fn_name))
    return sites


def parse_partition(doc_text: str) -> dict[tuple[str, str], tuple[str, str]]:
    """Parse the §10.4.1 BEGIN/END coverage table region. Returns
    `(file, function) -> (block_text, capability)`."""
    begin = "<!-- BEGIN U0 row→block coverage table"
    end = "<!-- END U0 row→block coverage table"
    bi = doc_text.find(begin)
    ei = doc_text.find(end)
    if bi < 0 or ei < 0:
        raise SystemExit("could not locate §10.4.1 coverage table BEGIN/END markers")
    region = doc_text[bi:ei]
    out: dict[tuple[str, str], tuple[str, str]] = {}
    current_block: str | None = None
    block_hdr = re.compile(r"^\*\*`([A-Z0-9._]+)`\*\* \(\d+ rows?\):")
    row_re = re.compile(r"^- `([a-z0-9_]+\.rs)::([A-Za-z0-9_]+)` — `([A-Za-z]+)`")
    for line in region.splitlines():
        line = line.strip()
        hm = block_hdr.match(line)
        if hm:
            current_block = hm.group(1)
            continue
        rm = row_re.match(line)
        if rm and current_block is not None:
            file_, fn_, cap = rm.group(1), rm.group(2), rm.group(3)
            out[(file_, fn_)] = (current_block, cap)
    return out


GENERATED_HEADER = (
    "// Auto-generated by `scripts/gen-typeinfo-ignore-manifest.py`\n"
    "// (`pnpm gen:typeinfo-manifest`). DO NOT hand-edit. Each row's\n"
    "// 13-/11-column data is COMPUTED from the authoritative §10.4.1\n"
    "// row->block partition in `docs/arch/native-typeinfo-parity.md`\n"
    "// joined with the live `#[ignore]` discovery + the Capability Map.\n"
    "// The Rust guards only diff/fail; they never write this file.\n"
)


def emit_ignored_rows(rows: list[dict]) -> str:
    out = [GENERATED_HEADER, "", "#[rustfmt::skip]", "const EXPECTED_IGNORE_MANIFEST: &[IgnoredTestRow] = &["]
    for r in rows:
        keys = ", ".join(f"SemanticQueryName::{k}" for k in r["keys"])
        mechs = ", ".join(f"MechanismId::{m}" for m in r["consumed"])
        out.append(
            "    IgnoredTestRow { "
            f'file: "{r["file"]}", '
            f'function: "{r["fn"]}", '
            f"substrate: TargetSubstrate::{r['substrate']}, "
            f"capability: TypeInfoCapability::{r['cap']}, "
            f"organ: ArchitectureOrgan::{r['organ']}, "
            f"owning_u_block: UBlock::{r['ublock']}, "
            f"block_id: TypeInfoParityBlockId::{r['block']}, "
            f"semantic_queries: &[{keys}], "
            f"proof: {r['proof']}, "
            "status: IgnoreStatus::Ignored, "
            f"mechanism_id: MechanismId::{r['mech']}, "
            f"consumed_mechanisms: &[{mechs}], "
            f'unblocker: "{r["unblocker"]}" }},'
        )
    out.append("];")
    return "\n".join(out) + "\n"


def emit_additional_rows(rows: list[dict]) -> str:
    out = [
        GENERATED_HEADER,
        "",
        "#[rustfmt::skip]",
        "const ADDITIONAL_PROOF_ROWS: &[AdditionalProofRow] = &[",
    ]
    for r in rows:
        keys = ", ".join(f"SemanticQueryName::{k}" for k in r["keys"])
        mechs = ", ".join(f"MechanismId::{m}" for m in r["consumed"])
        out.append(
            "    AdditionalProofRow { "
            f'file: "{r["file"]}", '
            f'function: "{r["fn"]}", '
            f"substrate: TargetSubstrate::{r['substrate']}, "
            f"capability: TypeInfoCapability::{r['cap']}, "
            f"organ: ArchitectureOrgan::{r['organ']}, "
            f"owning_u_block: UBlock::{r['ublock']}, "
            f"block_id: TypeInfoParityBlockId::{r['block']}, "
            f"semantic_queries: &[{keys}], "
            f"proof: {r['proof']}, "
            f"mechanism_id: MechanismId::{r['mech']}, "
            f"consumed_mechanisms: &[{mechs}] }},"
        )
    out.append("];")
    return "\n".join(out) + "\n"


def emit_block_rows() -> str:
    out = [
        GENERATED_HEADER,
        "",
        "#[rustfmt::skip]",
        "const TYPEINFO_PARITY_BLOCKS: &[BlockContractRow] = &[",
    ]
    verification = ", ".join(f'"{label}"' for label in BLOCK_VERIFICATION_LABELS)
    for block in BLOCK_TO_MECHANISM:
        prereqs = ", ".join(f"TypeInfoParityBlockId::{p}" for p in BLOCK_PREREQS[block])
        consumed = ", ".join(f"MechanismId::{m}" for m in consumed_mechs_for_block(block))
        guards = ", ".join(f'"{g}"' for g in BLOCK_TO_REQUIRED_GUARDS[block])
        out.append(
            "    BlockContractRow { "
            f"block_id: TypeInfoParityBlockId::{block}, "
            f"owning_u_block: UBlock::{BLOCK_TO_UBLOCK[block]}, "
            f"organ: ArchitectureOrgan::{BLOCK_TO_ORGAN[block]}, "
            f"prereqs: &[{prereqs}], "
            f"mechanism_id: MechanismId::{BLOCK_TO_MECHANISM[block]}, "
            f"consumed_mechanisms: &[{consumed}], "
            f"required_guards: &[{guards}], "
            f"verification_labels: &[{verification}] }},"
        )
    out.append("];")
    return "\n".join(out) + "\n"


# ── The CLOSED set of 7 `AdditionalProofRow`s (§8 + §10.1). All 7 rows
#    are FORWARD-DECLARATION coverage contracts emitting a RowTestGuard:
#    the 6 JSX no-new-key submatrix rows point at FUTURE
#    U2.JSX_FOUNDATIONS test functions, and the 1 mapped companion points
#    at the FUTURE U2.MAPPED_TEMPLATE test function. ──
JSX_NO_NEW_KEY_ROWS = [
    "jsx_library_managed_attributes_via_ambient_namespace_and_indexed_access",
    "jsx_element_attributes_property_via_ambient_namespace_keyof",
    "jsx_element_children_attribute_via_ambient_namespace_keyof",
    "jsx_intrinsic_attributes_via_ambient_namespace_intersection",
    "jsx_element_class_check_via_resolve_class_surface_and_relate",
    "jsx_import_source_module_namespace_via_existing_resolution",
]
MAPPED_COMPANION_FN = (
    "mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property"
)


def build_additional_rows() -> list[dict]:
    rows: list[dict] = []
    for fn in JSX_NO_NEW_KEY_ROWS:
        block = "U2JsxFoundations"
        mech = mechanism_for_row("JsxResolution", "jsx.rs", fn)
        rows.append(
            {
                "file": "jsx.rs",
                "fn": fn,
                "substrate": "JsxResolution",
                "cap": "JsxResolution",
                "organ": BLOCK_TO_ORGAN[block],
                "ublock": BLOCK_TO_UBLOCK[block],
                "block": block,
                "keys": keys_for_row(mech),
                # FORWARD-DECLARATION coverage contract: a RowTestGuard
                # pointing at the FUTURE U2.JSX_FOUNDATIONS test fn (U0
                # does NOT author the body).
                "proof": f'ProofRequirement::RowTestGuard {{ file: "jsx.rs", function: "{fn}" }}',
                "mech": mech,
                "consumed": consumed_mechs_for_block(block),
            }
        )
    block = "U2MappedTemplate"
    mech = mechanism_for_row("MappedTypes", "mapped_modifiers.rs", MAPPED_COMPANION_FN)
    rows.append(
        {
            "file": "mapped_modifiers.rs",
            "fn": MAPPED_COMPANION_FN,
            "substrate": "MappedTypes",
            "cap": "MappedTypes",
            "organ": BLOCK_TO_ORGAN[block],
            "ublock": BLOCK_TO_UBLOCK[block],
            "block": block,
            "keys": keys_for_row(mech),
            # FORWARD-DECLARATION coverage contract, consistent with the
            # 6 JSX rows: a RowTestGuard pointing at the FUTURE
            # U2.MAPPED_TEMPLATE test fn (U0 does NOT author the body).
            "proof": (
                'ProofRequirement::RowTestGuard { file: "mapped_modifiers.rs", '
                f'function: "{MAPPED_COMPANION_FN}" }}'
            ),
            "mech": mech,
            "consumed": consumed_mechs_for_block(block),
        }
    )
    return rows


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    src_dir = repo_root / "crates/verter_session/src/typeinfo/typeinfo_tests"
    if not src_dir.is_dir():
        print(f"typeinfo_tests dir missing: {src_dir}", file=sys.stderr)
        return 2
    out_dir = repo_root / "crates/verter_session/tests/manifest_data"
    out_dir.mkdir(parents=True, exist_ok=True)

    doc = (repo_root / "docs/arch/native-typeinfo-parity.md").read_text()
    partition = parse_partition(doc)

    # Discover live ignore sites + reasons.
    discovered: dict[tuple[str, str], str] = {}
    missing_mappings: list[str] = []
    for fn in sorted(os.listdir(src_dir)):
        if not fn.endswith(".rs"):
            continue
        sites = extract_sites((src_dir / fn).read_text())
        if not sites:
            continue
        if fn not in FILE_TO_SUBSTRATE:
            missing_mappings.append(fn)
            continue
        for reason, fn_name in sites:
            discovered[(fn, fn_name)] = reason

    if missing_mappings:
        print(
            "error: typeinfo-test files without a FILE_TO_SUBSTRATE mapping:",
            file=sys.stderr,
        )
        for fn in missing_mappings:
            print(f"  - {fn}", file=sys.stderr)
        return 3

    # Cross-check discovery vs §10.4.1 partition (must agree row-for-row).
    disc_keys = set(discovered)
    part_keys = set(partition)
    only_disc = sorted(disc_keys - part_keys)
    only_part = sorted(part_keys - disc_keys)
    if only_disc or only_part:
        print("error: §10.4.1 partition does not match the live ignore set:", file=sys.stderr)
        for k in only_disc:
            print(f"  live-only (no partition row): {k[0]} :: {k[1]}", file=sys.stderr)
        for k in only_part:
            print(f"  partition-only (no live ignore): {k[0]} :: {k[1]}", file=sys.stderr)
        return 4

    # Build the IgnoredTestRows in (file, function) sorted order.
    rows: list[dict] = []
    for (file_, fn_name) in sorted(discovered):
        block_text, cap = partition[(file_, fn_name)]
        block_var = BLOCK_TEXT_TO_VARIANT[block_text]
        # mechanism_id is ROW-LEVEL, derived from capability/override —
        # INDEPENDENT of block_var (the partition's block column).
        mech = mechanism_for_row(cap, file_, fn_name)
        rows.append(
            {
                "file": file_,
                "fn": fn_name,
                "substrate": FILE_TO_SUBSTRATE[file_],
                "cap": cap,
                "organ": BLOCK_TO_ORGAN[block_var],
                "ublock": BLOCK_TO_UBLOCK[block_var],
                "block": block_var,
                "keys": keys_for_row(mech),
                "proof": proof_for_capability(cap),
                "mech": mech,
                "consumed": consumed_mechs_for_block(block_var),
                "unblocker": escape_rust_string_literal(discovered[(file_, fn_name)]),
            }
        )

    if len(rows) != 362:
        print(f"error: expected 362 IgnoredTestRows, built {len(rows)}", file=sys.stderr)
        return 5

    # ── Generation-time self-consistency assertions (NON-circular):
    #    the row-level mechanism (from capability/override) and the
    #    partition's block_id are INDEPENDENT sources; the correct table
    #    requires them to agree, and that every mechanism's full key set
    #    is reachable from the row's block. A failure here means the
    #    override/capability mechanism map or the partition disagrees —
    #    fix the source, do NOT silence. ──
    for r in rows:
        owner = MECHANISM_OWNING_BLOCK[r["mech"]]
        if owner != r["block"]:
            raise SystemExit(
                f"mechanism/block disagreement: {r['file']}::{r['fn']} has "
                f"row-level mechanism {r['mech']} owned by {owner}, but the "
                f"§10.4.1 partition places it in {r['block']}. Reconcile "
                f"ROW_MECHANISM_OVERRIDE / CAPABILITY_TO_MECHANISM with the "
                f"partition (do NOT derive mechanism from block)."
            )
        for k in r["keys"]:
            if not _reaches(r["block"], KEY_OWNING_BLOCK[k]):
                raise SystemExit(
                    f"unreachable key: {r['file']}::{r['fn']} (mechanism "
                    f"{r['mech']}) consumes {k} owned by {KEY_OWNING_BLOCK[k]}, "
                    f"not reachable from block {r['block']}. Fix MECHANISM_TO_KEYS "
                    f"or the block prereqs."
                )

    additional = build_additional_rows()

    (out_dir / "typeinfo_ignored_test_manifest_rows.rs").write_text(emit_ignored_rows(rows))
    (out_dir / "typeinfo_additional_proof_rows.rs").write_text(emit_additional_rows(additional))
    (out_dir / "typeinfo_parity_blocks.rs").write_text(emit_block_rows())

    print(
        f"wrote {len(rows)} IgnoredTestRows, {len(additional)} AdditionalProofRows, "
        f"{len(BLOCK_TO_MECHANISM)} BlockContractRows",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
