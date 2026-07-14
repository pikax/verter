# Terminal `TypeExpr` Authority Manifest

The committed classification of EVERY production `TypeExpr` authority site — every
production item whose code names the bare `TypeExpr` identifier — across
`verter_session`, `verter_semantic`, `verter_session_query`, `verter_parser`,
`verter_compiler`, and the two vocabulary crates. Every site carries exactly one
of three dispositions; a site fitting none of them is a residual migration hole
to escalate, never a row to paper over.

## Status — transitional migration-completeness rail (WIP class)

This manifest and its joining guard
(`crates/verter_session/tests/cases/terminal_type_expr_authority_manifest.rs`)
are a TRANSITIONAL/WIP migration-completeness rail. They SUPERSEDE the
definition-only residual body-reader inventory
(`crates/verter_session/tests/cases/residual_type_expr_body_reader_inventory.rs`),
which tracks `<recv>.body.<method>` reader DEFINITIONS, cannot see the
RESOLVED/GENERATED query-time class, and does not test caller-reachability. The
DURABLE replacement is the F3 crate-boundary + `NoTypeExpr` structural rail
(P5b); this manifest, its guard, and the residual inventory are deleted together
at B8 when that structural rail lands. Neither is a landed structural guard and
neither may be extended into one (landed-scanner bar).

## The three classes

| Class | Meaning |
|---|---|
| `C1` | **Sealed output / syntax.** The sealed `OutputProjector` materialiser + capability-gated carriers, wire/JSON/display payloads, JSDoc `{Type}` text, display rendering, oracle/diagnostic vocabulary, guard-pinned test-only probe surfaces. A `TypeExpr` here is materialised-for-output or display/wire text — never a query-time semantic decision. |
| `C2` | **Sanctioned lowering ingress.** The transient OXC→typed-IR product; producer decl-body / macro-payload / JSDoc-text / wire-type-arg lowering handed straight to the one shared dispatch (`DeclBodyMemo` → `DeclLoweringService`, the shared shallow lowerer); producer-side fact minting (route facts, key-source, heritage, closedness, fingerprints, dependency harvests) over transient authored bodies; and syntactic route/shape classification over the TRANSIENT AUTHORED payload on its way INTO dispatch. Consumed once, turned into content-free nodes/facts — never re-inspected for a semantic decision after lowering. |
| `C3` | **Dead — zero production consumers.** A plain `C3` row is a TOMBSTONE: the item is deleted and the guard REDS if it reappears. A `C3-pending` row is a compiler-certified dead item still in-tree (rustc `dead_code` on the default lib target, and/or `allow(dead_code)`-annotated parity-oracle legs whose only callers are `#[cfg(test)]` code) — enumerated and pinned here; its deletion rides the tracked dead-code debt (the pre-existing clippy `-D warnings` baseline counts exactly this population). The guard REDS when a `C3-pending` row goes stale, forcing the flip to a `C3` tombstone. |

## Scope, method, exclusions

- Enumeration is structural (`syn`), per top-level item, keyed `(crate, file,
  item)`; an item is a row iff its token stream contains the bare ident
  `TypeExpr` (`NoTypeExpr` / `TypeExprId` / doc prose / string literals never
  match) or a file-local `use ... TypeExpr as X;` rename of it (the alias
  matches too — a rename does not slip past the scan).
- Impl blocks enumerate PER MEMBER: each `fn` / `const` / `type` member that
  individually names `TypeExpr` keys its own `impl <Name>::<kind> <name>`
  row, so a NEW `TypeExpr`-naming method inside an already-classified impl
  mints a NEW unclassified key instead of riding the impl's row. The impl
  HEADER (generics / self type / trait path, plus rare non-fn/const/type
  members) keys the bare `impl <Name>` row only when it names `TypeExpr`
  itself.
- `cfg` gating is negation-aware: an item is excluded only when its predicate
  is definitely FALSE in the default production build — `cfg(test)`,
  `cfg(any(test, feature = "oracle-gen"))`, `cfg(any(test, feature =
  "test-support"))` (both features guard-verified production-unreachable:
  `oracle-gen` by the guard's own gate re-assertions, `test-support` by
  `tests/cases/g_misc1/test_support_feature_off_in_default_build.rs`).
  `cfg(not(test))` is PRODUCTION and enumerates; unknown atoms (platform
  cfgs, other features) conservatively enumerate.
- Excluded subtrees, each behind a gate the guard re-asserts structurally:
  `typeinfo/oracle_core/**` (`#[cfg(any(test, feature = "oracle-gen"))]`),
  `typeinfo/typeinfo_tests/**` (`#[cfg(test)]`), the `oracle_gen` /
  `oracle_upgrade` bins (`required-features = ["oracle-gen"]`; no production
  consumer enables `oracle-gen`), and `*_tests.rs` / `tests/` files.
- `verter_protocol` / `verter_ffi` (sealed wire/output DTO crates above the
  session) are outside this enumeration: their `TypeExpr` surface is the
  permitted PROTOCOL survivor class owned by the Typeinfo Wire Contract
  guards (`typeinfo_graph_taxonomy`, `typeinfo_proto_ts_freshness`, ...).

## Crate-level blanket rows (vocabulary crates)

Structurally justified by the leaf assertion in the guard: neither crate may
depend on `verter_semantic` / `verter_session`, so neither can reach the store
or dispatch to make a query-time decision.

| Class | Crate | File | Item | Justification |
|---|---|---|---|---|
| C1 | verter_type_expr | (crate) | (vocabulary) | the typed-IR definition crate itself — the syntax vocabulary both sanctioned classes speak (IR enum, JSON wire encoding, traversal/Drop/Hash, span transforms, locators, closed fact families, intrinsic catalog); a leaf crate with no resolver access |
| C2 | verter_type_expr_oxc | (crate) | (producer) | the OXC→TypeExpr lowering producer (`lower_ts_type`): purely syntactic, no symbol resolution, no cache/store access; the single lowering front-end the C2 producers call |

## Tombstones — dead route/ref-walker cluster deleted with this manifest

The registry's superseded `TypeExpr` walkers (replaced by the node-domain
`*_node` siblings) had zero production callers — verified by external-reference
search plus intra-file reachability from every live entry point.

| Class | Crate | File | Item | Justification |
|---|---|---|---|---|
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_route_root_name | deleted: dead route-root extractor, zero callers |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_raw_member_path_surface | deleted: dead raw member-path projector (test-only caller), superseded by node-domain materialisation |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_expr_references_name | deleted: dead self-recursive ref-check walker |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn collect_component_meta_registry_refs | deleted: dead TypeExpr ref-collector, superseded by collect_component_meta_registry_refs_node |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn collect_component_meta_registry_function_surface_refs | deleted: dead function-surface ref-collector (mutual recursion with the collector above) |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_ref_name | deleted: dead ref-name extractor, superseded by component_meta_registry_node_ref_name |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_direct_public_ref | deleted: dead direct-ref extractor |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_field_expr_has_actionable_route | deleted: dead actionable-route predicate, superseded by component_meta_registry_node_has_actionable_route |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn collect_component_meta_registry_public_surface_refs | deleted: dead public-surface ref-collector, superseded by collect_component_meta_registry_public_surface_refs_node |
| C3 | verter_session | src/resolver_core/component_meta_registry.rs | fn collect_component_meta_registry_member_surface_refs | deleted: dead member-surface ref-collector, superseded by collect_registry_member_surface_refs_node |

## The classification table

| Class | Crate | File | Item | Justification |
|---|---|---|---|---|
| C3-pending | verter_parser | src/utils/oxc/script/raw_surface.rs | fn lowered_body_rejectable_variant | oracle-harness leg: zero production callers (only raw_surface_tests); the module doc marks the file the TypeExpr-projection oracle harness |
| C2 | verter_semantic | src/analysis/fact_projection.rs | fn value_type_annotation_fact | reads the transient authored annotation once to mint the content-free ValueTypeAnnotationFact; the fact carries no TypeExpr |
| C2 | verter_semantic | src/analysis/framework_facts/svelte.rs | enum PropsAnnotationLowering | producer-side svelte script-fact lowering: authored payloads lower once (lower_ts_type), are fingerprinted or projected to display leaf vocabulary, and are dropped — the persisted facts carry AuthoredTypePayloadRef, never a TypeExpr |
| C2 | verter_semantic | src/analysis/framework_facts/svelte.rs | fn authored_type_payload_ref | producer-side svelte script-fact lowering: authored payloads lower once (lower_ts_type), are fingerprinted or projected to display leaf vocabulary, and are dropped — the persisted facts carry AuthoredTypePayloadRef, never a TypeExpr |
| C2 | verter_semantic | src/analysis/framework_facts/svelte.rs | fn leaf_members_from_lowered | producer-side svelte script-fact lowering: authored payloads lower once (lower_ts_type), are fingerprinted or projected to display leaf vocabulary, and are dropped — the persisted facts carry AuthoredTypePayloadRef, never a TypeExpr |
| C2 | verter_semantic | src/analysis/jsdoc.rs | fn extract_jsdoc_param_types_at_offset | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/jsdoc.rs | fn extract_jsdoc_return_type_at_offset | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/jsdoc.rs | fn extract_jsdoc_type_at_offset | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/jsdoc.rs | fn lower_first_jsdoc_tag_type | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/jsdoc.rs | fn lower_jsdoc_tag_type | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/jsdoc.rs | fn parse_jsdoc_tag_type_payload | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/jsdoc.rs | struct JsdocTypedef | the sanctioned JSDoc {Type} text exception: tag payloads are inherently text, lowered through the dedicated OXC path into transient typed IR at the producer boundary (JsdocTypedef carries the lowered alias body on the shallow-analysis carrier) |
| C2 | verter_semantic | src/analysis/macros.rs | enum MacroFieldPayloadLowering | macro-payload producer lowering: OXC macro type arguments / field payloads lower to owned typed IR at the producer boundary and hand straight to the consumer lowering path |
| C2 | verter_semantic | src/analysis/macros.rs | fn lower_emit_field_payload_at_span | macro-payload producer lowering: OXC macro type arguments / field payloads lower to owned typed IR at the producer boundary and hand straight to the consumer lowering path |
| C2 | verter_semantic | src/analysis/macros.rs | fn lower_macro_type_argument_at_span | macro-payload producer lowering: OXC macro type arguments / field payloads lower to owned typed IR at the producer boundary and hand straight to the consumer lowering path |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn annotation_source | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn append_spread_array_element_types | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn append_union_members | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn collect_array_element_types_from_type | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn collect_class | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn collect_enum | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn collect_return_types | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn dedupe_type_exprs | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn enrich_params_and_return_with_jsdoc | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn extract_object_literal_as_type | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn infer_expression_type | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn infer_expression_type_ctx | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn infer_return_type | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn is_const_assertion_type_expr | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn lower_function_params | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn lower_interface_member | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn lower_jsdoc_typedef_named | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn lower_named_interface_parts | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn lower_value_expression | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn lower_variable_parts | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn member_header_facts_from_body | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn object_member_value | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn object_shape_fact | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn parse_value_expression_type | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn widen_literal_type | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | fn widen_shallow_literal | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | struct LoweredSignatureParts | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | struct LoweredTypeDeclParts | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_eval_build.rs | struct LoweredValueDeclParts | OXC AST -> typed-IR decl/value/signature lowering and literal widening/inference producers over TRANSIENT lowered parts; consumers read the minted facts, never re-walk a body |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn body_is_closed_object_shape | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn body_supports_direct_member_projection | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn classify_forwarding_kind | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn classify_key_remap | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn classify_projection_inner | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn classify_wrapper_shape_inner | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn closedness_recipe_of | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn collect_all_arms | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn collect_heritage_base_facts | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn collect_key_domain_closedness_fact | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn extract_forward_payload | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn is_bare_ref | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn is_param_or_param_intersect_string | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn is_passthrough_value | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | fn scalar_key_shape | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | impl PreparedTypeDecl::fn build_member_index | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | impl PreparedTypeDecl::fn classify_projection | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | impl PreparedTypeDecl::fn classify_wrapper_shape | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/analysis/type_solver/prepared.rs | impl PreparedTypeDecl::fn index_transparent_object_members | prepared-decl fact minting over transient authored contributor bodies (member index, wrapper/projection classification, heritage-base facts, key-domain closedness recipes); dispatch reads the stored facts in place of any query-time authored-body walk |
| C2 | verter_semantic | src/facts/hashing.rs | enum TransientTypeBody | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | fn collect_direct_object_members | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | fn compute_semantic_hash | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | fn scalar_to_type_expr | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | fn type_body_fingerprint | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | impl ValueBodyFingerprintInput::fn new | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | impl Walker::fn emit_property | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | impl Walker::fn node_identity_key | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | impl Walker::fn walk | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | impl Walker::fn walk_node | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | impl Walker::fn walk_ref | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/hashing.rs | struct ValueBodyFingerprintInput | alpha-normalised structural fingerprint producer over transient authored bodies; assembled TypeExpr views are fact-production intermediates, immediately dropped |
| C2 | verter_semantic | src/facts/route_facts.rs | fn bare_ref_name | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn collect_direct_object_members | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn collect_direct_object_properties | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn collect_key_source_arms | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn collect_type_refs | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn direct_object_properties | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn produce_key_source_fact | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | fn produce_shallow_route_facts | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn classify_routed_base | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn emit_indexed_access_edges | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn emit_routed_base | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn emit_utility_edges | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn extract_indexed_access_base | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn extract_literal_keys | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn extract_literal_keys_inner | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_semantic | src/facts/route_facts.rs | impl RouteFactProducer::fn walk_whole_route | route-fact / key-source producers over transient contributor bodies (graph-free by construction, no produce-time follow); the shared closure core consumes only the content-free facts |
| C2 | verter_session | src/decl_body_memo.rs | fn lowered_type_decl_from_group | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | impl DeclBodyMemo::fn transient_augmentation_type_bodies | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | impl DeclBodyMemo::fn transient_jsdoc_typedef_body | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | impl DeclBodyMemo::fn transient_macro_type_argument | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | impl DeclBodyMemo::fn transient_type_bodies | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | impl DeclBodyMemo::fn transient_type_bodies_for | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | struct RetainedTypeTransients | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | struct RetainedValueTransients | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo.rs | struct TransientValueParts | the lazy decl-body memo: retained-parse transients (contributor bodies / value annotations) re-lowered on demand as fact-production intermediates and lowering inputs; never stored as hot carriers |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | enum DerefedBodyShape | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | enum NavigatePosition | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | fn member_value_expr | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | fn navigate_expr | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | fn navigate_signature_parts | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | fn navigate_value_parts | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | fn transient_body_shape | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | fn unwrap_parenthesized | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | impl DeclBodyMemo::fn deref_locator_body | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/decl_body_memo/locator_deref.rs | impl DeclBodyMemo::fn deref_type_arg | the locator-deref worker: derefs the retained snapshot to the demanded authored position (syntactic navigation over the transient body) and hands owned typed IR to the shared lowerer |
| C2 | verter_session | src/host_manage.rs | fn collect_type_expr_symbol_refs | authored decl-body symbol-ref harvest feeding import-dependency facts; syntactic collection over the transient body |
| C2 | verter_session | src/host_manage/jsdoc_resolve.rs | fn resolve_jsdoc_tag_type | the sanctioned JSDoc {Type} text exception: parses the tag payload, lowers/dispatches once through the shared class-A path, renders display + wire snapshot, then discards the TypeExpr |
| C2 | verter_session | src/host_manage/jsdoc_resolve.rs | impl HostComponentMetaResolver::fn owner_local_macro_root_surface_presence | constructs a TypeExpr::Ref for the owner-local macro root and hands it to dispatch; presence is decided node-domain off the SurfaceView |
| C2 | verter_session | src/host_manage/prepared_decl.rs | impl VerterHost::fn cache_only_lookup_symbol_dependencies_for_expr | imported-symbol dependency harvest: collect_type_expr_symbol_refs over the authored body resolves names to import-dependency facts |
| C2 | verter_session | src/host_manage/prepared_decl.rs | impl VerterHost::fn imported_symbol_dependencies | imported-symbol dependency harvest: collect_type_expr_symbol_refs over the authored body resolves names to import-dependency facts |
| C2 | verter_session | src/host_manage/prepared_decl.rs | impl VerterHost::fn imported_symbol_dependencies_for_expr | imported-symbol dependency harvest: collect_type_expr_symbol_refs over the authored body resolves names to import-dependency facts |
| C2 | verter_session | src/mapper_binder_registry.rs | fn hash_function_expr | structural content-hash of the authored TypeExpr::Mapped payload during lowering, assigning the mapper-binder ordinal; a content-address identity, no semantic branch |
| C2 | verter_session | src/mapper_binder_registry.rs | fn hash_object_member | structural content-hash of the authored TypeExpr::Mapped payload during lowering, assigning the mapper-binder ordinal; a content-address identity, no semantic branch |
| C2 | verter_session | src/mapper_binder_registry.rs | fn hash_recursive_conditional_frame | structural content-hash of the authored TypeExpr::Mapped payload during lowering, assigning the mapper-binder ordinal; a content-address identity, no semantic branch |
| C2 | verter_session | src/mapper_binder_registry.rs | fn hash_type_expr_structurally | structural content-hash of the authored TypeExpr::Mapped payload during lowering, assigning the mapper-binder ordinal; a content-address identity, no semantic branch |
| C2 | verter_session | src/mapper_binder_registry.rs | impl MapperFingerprint::fn from_components | structural content-hash of the authored TypeExpr::Mapped payload during lowering, assigning the mapper-binder ordinal; a content-address identity, no semantic branch |
| C2 | verter_session | src/meta_resolve/dispatch_helpers.rs | fn decompose_indexed_access_chain | syntactic decomposition of the authored route payload into path-precise ProjectPath segments on its way into the one dispatch |
| C2 | verter_session | src/meta_resolve/dispatch_helpers.rs | fn lower_and_project_to_expanded_node_via_host_threaded | budget-guarded bridge: authored expr lowered once through dispatch to the admitted node (no materialisation) |
| C2 | verter_session | src/meta_resolve/dispatch_helpers.rs | fn project_expr_class_a_node_via_dispatch_threaded | ingress route classification over the authored payload: registry fast-path or terminal, both resolving through the one dispatch; returns the admitted node |
| C2 | verter_session | src/meta_resolve/dispatch_helpers.rs | fn project_expr_class_a_via_dispatch | ingress bridge over the authored payload; the returned TypeExpr is the sink-materialised publication produced downstream by the sealed surface sink |
| C2 | verter_session | src/meta_resolve/dispatch_helpers.rs | fn route_outer_utility_is_shadowed | extracts the OUTER identifier syntactically from the authored route shape; the shadow verdict comes from scope facts and only selects WHICH dispatch route runs |
| C1 | verter_session | src/meta_resolve/output.rs | struct MaterializedComponentMetaTypeLanes | the request-local component-meta output envelope's 11 positional wire type lanes (props, event payloads, slot bindings, models, exposed, public-instance members, registry entries, accepted props, accepted event payloads, fallthrough props/event payloads): materialised-for-output values assembled at the terminal sink and consumed by value at the wire boundary; never re-read for a query-time decision |
| C1 | verter_session | src/meta_resolve/projectors/output_sink/envelope.rs | struct OutputSourceMemo | the request-local `(effective scope, source identity)` output-materialization memo shared across the 11 lanes: stores already-materialised output values for verbatim positional reuse (dedupe transport, no decision on any materialised value) |
| C1 | verter_session | src/meta_resolve/projectors/output_sink/envelope.rs | impl OutputSourceMemo::fn materialize_output_lane_slot | one lane slot through the memoized per-source terminal (or the centralized missing-source policy); fail-closed typed error with the lane's positional indices — output transport, no decision on the materialised value |
| C1 | verter_session | src/meta_resolve/projectors/output_sink.rs | fn materialize_output_source | the output-envelope per-source terminal (HOT_TERMINAL_SINKS member): closed leaf/leaf-union rendered verbatim, every other source shell-raised ONCE and capability-unwrapped into the envelope lane value; fail-closed typed error on a raise/shell miss — never a decision on the materialised value |
| C1 | verter_session | src/meta_resolve/projectors/output_sink.rs | fn missing_source_output_type_expr | the ONE centralized missing-source output policy: the canonical typed Unknown emitted for a None analysis source; a constant output value, no semantic decision |
| C1 | verter_session | src/meta_resolve/projectors/output_sink.rs | fn raise_node_to_sealed_carrier | the terminal one-shot projector sink: node materialised into the sealed MaterializedOutputTypeExpr under a projector capability; makes no decision on the materialised value |
| C2 | verter_session | src/project_semantic_dispatch/build.rs | impl ProjectSemanticDispatch::fn build_typeof | the body-lowering producer: build_typeof projects enum-member facts into a transient object and lower_class_heritage_args derefs authored heritage args, both feeding the shared lowerer |
| C2 | verter_session | src/project_semantic_dispatch/build.rs | impl ProjectSemanticDispatch::fn lower_class_heritage_args | the body-lowering producer: build_typeof projects enum-member facts into a transient object and lower_class_heritage_args derefs authored heritage args, both feeding the shared lowerer |
| C2 | verter_session | src/project_semantic_dispatch/locator_shape.rs | impl ProjectSemanticDispatch::fn lower_locator_shape_args | carrier-only locator-shape lowering: interns the FIXED authored shape into content-free carriers; never dispatches a query |
| C2 | verter_session | src/project_semantic_dispatch/locator_shape.rs | impl ProjectSemanticDispatch::fn lower_locator_shape_node | carrier-only locator-shape lowering: interns the FIXED authored shape into content-free carriers; never dispatches a query |
| C2 | verter_session | src/project_semantic_dispatch/locator_shape.rs | impl ProjectSemanticDispatch::fn lower_type_expr_for_locator_shape | carrier-only locator-shape lowering: interns the FIXED authored shape into content-free carriers; never dispatches a query |
| C2 | verter_session | src/project_semantic_dispatch/locator_shape.rs | impl ProjectSemanticDispatch::fn resolve_locator_ref_head | carrier-only locator-shape lowering: interns the FIXED authored shape into content-free carriers; never dispatches a query |
| C2 | verter_session | src/project_semantic_dispatch/lower.rs | fn enum_scalar_type_expr | the ONE eager lowering authority: the shallow structural-transit walker over the transient authored body, plus closed-fact projections feeding it; output is content-free nodes |
| C2 | verter_session | src/project_semantic_dispatch/lower.rs | fn leaf_type_fact_expr | the ONE eager lowering authority: the shallow structural-transit walker over the transient authored body, plus closed-fact projections feeding it; output is content-free nodes |
| C2 | verter_session | src/project_semantic_dispatch/lower.rs | impl ProjectSemanticDispatch::fn resolve_enum_member_value | the ONE eager lowering authority: the shallow structural-transit walker over the transient authored body, plus closed-fact projections feeding it; output is content-free nodes |
| C2 | verter_session | src/project_semantic_dispatch/lower.rs | impl ProjectSemanticDispatch::fn shallow_lower_type_expr_with_context | the ONE eager lowering authority: the shallow structural-transit walker over the transient authored body, plus closed-fact projections feeding it; output is content-free nodes |
| C2 | verter_session | src/project_semantic_dispatch/mod.rs | impl ProjectSemanticDispatch::fn lower_type_expr_in_scope_with_context | the lower_type_expr_in_scope_* lowering entries: authored expr in, SemanticNodeId out |
| C2 | verter_session | src/project_semantic_dispatch/mod.rs | impl ProjectSemanticDispatch::fn lower_type_expr_in_scope_with_mode | the lower_type_expr_in_scope_* lowering entries: authored expr in, SemanticNodeId out |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization_guards.rs | macro assert_not_impl_any | compile-time canaries banning Deref/AsRef/Borrow<TypeExpr> escapes from the sealed output carriers |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization.rs | carrier::impl OutputTypeExpr::fn from_raise | the sealed OutputProjector seam: the output payload vault plus capability-gated unwrap; the inner TypeExpr is unreachable outside the seam |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization.rs | carrier::impl OutputTypeExpr::fn into_type_expr | the sealed OutputProjector seam: the output payload vault plus capability-gated unwrap; the inner TypeExpr is unreachable outside the seam |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization.rs | carrier::payload::impl OutputPayload::fn into_type_expr | the sealed OutputProjector seam: the output payload vault plus capability-gated unwrap; the inner TypeExpr is unreachable outside the seam |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization.rs | carrier::payload::impl OutputPayload::fn new | the sealed OutputProjector seam: the output payload vault plus capability-gated unwrap; the inner TypeExpr is unreachable outside the seam |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization.rs | carrier::payload::struct OutputPayload | the sealed OutputProjector seam: the output payload vault plus capability-gated unwrap; the inner TypeExpr is unreachable outside the seam |
| C1 | verter_session | src/project_semantic_dispatch/output_materialization.rs | fn wrap_output_type_expr | the sealed OutputProjector seam: the output payload vault plus capability-gated unwrap; the inner TypeExpr is unreachable outside the seam |
| C2 | verter_session | src/project_semantic_dispatch/raise.rs | fn deref_slot_body | recipe-escape supply line: derefs ONE authored body position lease-only and hands the transient TypeExpr to the shared lowerer; decision-free |
| C2 | verter_session | src/project_semantic_dispatch/raise.rs | fn lower_body_under_env | recipe-escape lowering bridge: feeds the derefed transient body to the one shared shallow lowerer under the live binding environment |
| C3-pending | verter_session | src/project_semantic_dispatch/raise.rs | fn node_raised_shape_for_eq | cfg-allow(dead_code) parity-oracle leg of the shape-eq gate; only test callers |
| C2 | verter_session | src/project_semantic_dispatch/raise.rs | fn node_raised_shape_for_eq_with_dispatch | folds the PRE-dispatch input expr into a content-free RaisedShapeKey for the route no-op/changed convergence gate; the TypeExpr is consumed into a key |
| C3-pending | verter_session | src/project_semantic_dispatch/raise.rs | fn raised_shape_eq_node_type_expr | cfg-allow(dead_code) parity-oracle wrapper; no production caller |
| C1 | verter_session | src/project_semantic_dispatch/raise.rs | impl ProjectSemanticDispatch::fn raise_and_reduce_with_context | the raise half of the sealed output seam: the module-private node-to-TypeExpr shell materialisation reached only through the OutputProjector capability |
| C1 | verter_session | src/project_semantic_dispatch/raise.rs | impl ProjectSemanticDispatch::fn raise_node_to_type_expr | the raise half of the sealed output seam: the module-private node-to-TypeExpr shell materialisation reached only through the OutputProjector capability |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | fn fold_to_type_expr | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn array | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn build_function | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn conditional | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn constructor_to_out | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn empty_object | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn function_to_out | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn import_type | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn indexed_access | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn infer | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn intersection | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn is_empty_object | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn is_object_surface_sentinel | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn key_of | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn literal | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn mapped | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn member_index_signature | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn member_property | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn object_from_members | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn opaque_sentinel | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn out_as_function | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn primitive | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn recursive_ref | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn reference | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn synthetic_slot_binding | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn template_literal | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn tuple | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn type_of | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn type_parameter | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn union | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::fn unknown | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C1 | verter_session | src/project_semantic_dispatch/raise/shape_engine/materialize.rs | impl RaisedShapeAlgebra for MaterializeTypeExprAlg::type Out | Algebra 1 (Out = TypeExpr): the output materialiser fold reached only through the sealed OutputProjector seam; every arm constructs an output variant |
| C2 | verter_session | src/project_semantic_dispatch/raise/shape_engine/mod.rs | fn project_node_shape_for_eq | folds an input TypeExpr into the shared content-free interner key space for route equality; consumed into a key, never walked for type semantics |
| C3-pending | verter_session | src/project_semantic_dispatch/raise/shape_engine/mod.rs | fn raised_shape_eq_node_type_expr | backs the production-dead raise-side wrapper; parity-oracle only |
| C2 | verter_session | src/project_semantic_dispatch/raise/shape_engine/node_domain.rs | fn type_expr_to_key | Algebra 3 entry: interns an existing input TypeExpr into the content-free RaisedShapeKey space (consumed to a key) |
| C2 | verter_session | src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs | impl ComponentMetaQueryEngine::fn evaluate_fallthrough_value_node | TypeOf shape-check plus value-literal TypeExpr construction over the freshly-parsed transient override expression, handed straight to the shared lowerer; all reads node-domain |
| C2 | verter_session | src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs | impl ComponentMetaQueryEngine::fn lower_value_literal_node | TypeOf shape-check plus value-literal TypeExpr construction over the freshly-parsed transient override expression, handed straight to the shared lowerer; all reads node-domain |
| C2 | verter_session | src/resolver_core/component_meta_query_engine/fallthrough_value_eval.rs | impl ComponentMetaQueryEngine::fn value_expression_override_node | TypeOf shape-check plus value-literal TypeExpr construction over the freshly-parsed transient override expression, handed straight to the shared lowerer; all reads node-domain |
| C2 | verter_session | src/resolver_core/component_meta_query_engine/intrinsic_surface.rs | impl ComponentMetaQueryEngine::fn project_intrinsic_root_shape | constructs TypeExpr::named for the intrinsic root and hands it to the class-A dispatch; the shape verdict is node-domain off the SurfaceView |
| C1 | verter_session | src/resolver_core/component_meta_query_engine/registry_decl.rs | impl ComponentMetaQueryEngine::fn materialize_member_surface_node_core | member-surface publication materialisation through the sealed registry output cap; materialize_pick_member_surface survives only as the cfg-allow demand-API contract leg |
| C1 | verter_session | src/resolver_core/component_meta_query_engine/registry_decl.rs | impl ComponentMetaQueryEngine::fn materialize_pick_member_surface | member-surface publication materialisation through the sealed registry output cap; materialize_pick_member_surface survives only as the cfg-allow demand-API contract leg |
| C3-pending | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn dispatch_route_expr_is_materialized | cfg-allow parity oracle; production reads node-domain RaisedShapeFacts |
| C2 | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn lower_and_project_to_expanded_node | lowers the input route expr once through dispatch to the admitted node |
| C1 | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn materialize_published_node | node materialised to the published TypeExpr through the sealed surface cap |
| C1 | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn materialize_route_projection_node | admitted route node materialised once at the sealed surface sink |
| C1 | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn project_class_a_published | the class-A publication tail: node projected then materialised once at the sealed sink |
| C2 | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn project_class_a_terminal_node | decomposes and lowers the input route expr through dispatch to the admitted node |
| C3-pending | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn type_expr_contains_semantic_miss | cfg-allow parity oracle; only test callers |
| C3-pending | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn type_expr_is_budget_exceeded_sentinel | no production caller (test_only probe + tests); the sentinel is recognised node-domain in production |
| C3-pending | verter_session | src/resolver_core/component_meta_query_engine/surface.rs | fn type_expr_is_expanded_surface | cfg-allow parity oracle; dead in production |
| C2 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_public_indexed_access_route | syntactic route extractor over the AUTHORED payload (Root['a']['b'] to a member-path RouteDemand); selects the registry fast-path, resolution runs through the one dispatch |
| C2 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_public_utility_route | syntactic route extractor over the AUTHORED payload (Pick/Omit head + literal keys to a RouteDemand); route selection only, resolution through the one dispatch |
| C2 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_string_literal_keys | literal-key extraction from the authored utility key argument feeding the route demand |
| C2 | verter_session | src/resolver_core/component_meta_registry.rs | fn component_meta_registry_utility_inner_ref_name | inner-ref name extraction from the authored utility argument feeding the route demand |
| C2 | verter_session | src/resolver_core/fallthrough.rs | fn collect_dynamic_root_candidates_from_type | fallthrough value evaluation over the freshly-parsed TRANSIENT expression (typeof substitution / annotation projection / dynamic-root harvest) before node projection; the raw-parse step stays syntactic |
| C2 | verter_session | src/resolver_core/fallthrough.rs | fn concrete_annotation_expr | fallthrough value evaluation over the freshly-parsed TRANSIENT expression (typeof substitution / annotation projection / dynamic-root harvest) before node projection; the raw-parse step stays syntactic |
| C3-pending | verter_session | src/resolver_core/fallthrough.rs | fn known_spread_keys_from_type_expr | no production caller; superseded by the node-domain known_spread_keys_from_node twin |
| C2 | verter_session | src/resolver_core/fallthrough.rs | fn structural_substitute_typeof_refs | fallthrough value evaluation over the freshly-parsed TRANSIENT expression (typeof substitution / annotation projection / dynamic-root harvest) before node projection; the raw-parse step stays syntactic |
| C2 | verter_session | src/resolver_core/prepared_decl.rs | struct TypeParamBinding | unlowered authored type-parameter bound slots, interned on demand by the shared shallow lowerer into TypeParam nodes |
| C2 | verter_session | src/resolver_core/shallow_file_state.rs | fn collect_type_refs | authored decl-body ref-name / typeof-root harvest feeding dependency signatures (fact production over the transient body) |
| C2 | verter_session | src/resolver_core/shallow_file_state.rs | fn collect_typeof_roots | authored decl-body ref-name / typeof-root harvest feeding dependency signatures (fact production over the transient body) |
| C2 | verter_session | src/resolver_core/shallow_file_state.rs | impl ShallowFileState::fn compat_type_contributors_for_typeinfo | the allow(dead_code) compat/oracle-only contributor accessor over memo-owned lease-only re-borrowed contributor bodies; the route-closure driver itself names no bare TypeExpr — its deferred key-source fact mint is the C2 facts/route_facts.rs producer rows |
| C1 | verter_session | src/resolver_core/surface_projector.rs | fn render_type_expr_display | display-string rendering of a TypeExpr; display-only by contract — semantic decisions read the typed form |
| C1 | verter_session | src/resolver_core/surface_projector.rs | fn render_type_expr_display_inner | display-string rendering of a TypeExpr; display-only by contract — semantic decisions read the typed form |
| C2 | verter_session | src/structural_carrier_producer/macro_arg_producer.rs | fn collect_extends_infer_binder_names | structural lowering of the OXC-produced macro payload into dormant carriers; performs no name/import/type resolution |
| C2 | verter_session | src/structural_carrier_producer/macro_arg_producer.rs | fn lower_args | structural lowering of the OXC-produced macro payload into dormant carriers; performs no name/import/type resolution |
| C2 | verter_session | src/structural_carrier_producer/macro_arg_producer.rs | fn lower_node | structural lowering of the OXC-produced macro payload into dormant carriers; performs no name/import/type resolution |
| C2 | verter_session | src/structural_carrier_producer/macro_arg_producer.rs | fn lower_type_expr_structural | structural lowering of the OXC-produced macro payload into dormant carriers; performs no name/import/type resolution |
| C2 | verter_session | src/structural_carrier_producer/macro_arg_producer.rs | fn lower_union_or_intersection | structural lowering of the OXC-produced macro payload into dormant carriers; performs no name/import/type resolution |
| C1 | verter_session | src/test_only.rs | budget_sentinel::fn is_budget_exceeded_sentinel | test-only probe surface: production-unconsumed by the guard test_only_module_is_only_consumed_by_test_files |
| C1 | verter_session | src/test_only.rs | mapper_fingerprint::impl MapperFingerprintProbe::fn from_components | test-only probe surface: production-unconsumed by the guard test_only_module_is_only_consumed_by_test_files |
| C1 | verter_session | src/typeinfo/framework_surface/graph_export.rs | impl GraphArena::fn encode_member_value | zero-dispatch wire output encoder: maps a shallow member-value TypeExpr onto wire node kinds; real values ride the sealed NamedTypeMemberOutput |
| C1 | verter_session | src/typeinfo/framework_surface/results.rs | impl NamedTypeMemberOutput::fn classify_shallow | classify_shallow reads the producer-transient raised TypeExpr ONCE at the publication boundary into the sealed shallow output vocabulary, then discards it |
| C1 | verter_session | src/typeinfo/framework_surface/svelte_exec.rs | fn materialize_payload_tuple | decide-free terminal sink: node-domain params minted into a display TypeExpr::Tuple via the sealed svelte cap |
| C1 | verter_session | src/typeinfo/framework_surface/vue_exec/mod.rs | fn raise_member_value | materialises a member value node into a sealed carrier for DTO/display output; no decision on the result |
| C1 | verter_session | src/typeinfo/framework_surface/vue_exec/normalize.rs | fn materialize_payload_tuple | output/display materialisation via the sealed caps (payload tuple, slot return, symbolic slot-binding display); every normaliser decision is node-domain via CallableNodeView / node_data_for |
| C1 | verter_session | src/typeinfo/framework_surface/vue_exec/normalize_slots.rs | fn materialize_slot_return_node | output/display materialisation via the sealed caps (payload tuple, slot return, symbolic slot-binding display); every normaliser decision is node-domain via CallableNodeView / node_data_for |
| C1 | verter_session | src/typeinfo/framework_surface/vue_exec/normalize_slots.rs | fn slot_binding_field | output/display materialisation via the sealed caps (payload tuple, slot return, symbolic slot-binding display); every normaliser decision is node-domain via CallableNodeView / node_data_for |
| C2 | verter_session | src/typeinfo/resolve_named_symbol.rs | enum NamedSymbolTypeArgs | wire-decoded type-argument carrier (protocol DTO to transient TypeExpr); lowered into SemanticNodeIds at the host boundary — no TypeExpr enters the semantic API |
| C2 | verter_session | src/typeinfo/resolve_named_symbol.rs | impl VerterHost::fn resolve_named_symbol_wire_with_audit | resolve_named_symbol_wire_with_audit lowers the wire type-args inside the audited request under one view before dispatch |
| C2 | verter_session_query | src/lib.rs | enum AuthoredBodyShape | body-source lowering-product DTO: legitimately owns the transient authored TypeExpr on its way into the shared lowerer; not a stored hot carrier |

## The flagged free-walker / route-classifier surfaces — explicit dispositions

Each was read first-hand; none walks a resolved/materialised `TypeExpr` for a
query-time semantic decision:

1. **`component_meta_registry` route classification** — the surviving
   extractors (`component_meta_registry_public_utility_route`,
   `component_meta_registry_public_indexed_access_route`,
   `component_meta_registry_string_literal_keys`,
   `component_meta_registry_utility_inner_ref_name`) are `C2`: syntactic
   RouteDemand extraction over the AUTHORED payload, selecting the registry
   fast-path; resolution runs through the one dispatch on both arms. The raw
   object-surface predicate pair (`..._has_explicit_object_surface` /
   `..._has_non_object_top_level_surface`) is rustc-dead (`C3-pending`; the
   node_root_* twins are the live predicates), and the superseded ref-walker
   cluster is deleted (`C3` tombstones above).
2. **`dispatch_helpers` route / utility-shadow / indexed-access walkers** —
   all `C2`: `route_outer_utility_is_shadowed` extracts the OUTER identifier
   syntactically (the shadow verdict comes from scope facts and only selects
   WHICH dispatch route runs); `decompose_indexed_access_chain` decomposes the
   authored payload into path-precise `ProjectPath` segments; the class-A
   projections lower the authored payload once through dispatch and return
   admitted nodes (the one materialising variant returns the sealed-sink
   publication).
3. **`oracle_core` / `normalize` canonicalizer** — NOT a production authority
   site: the whole `typeinfo/oracle_core` subtree (including `normalize.rs`
   and `source_walk.rs`) is `#[cfg(any(test, feature = "oracle-gen"))]`-gated
   and no production consumer enables `oracle-gen` (both facts re-asserted by
   the guard). The production `vue_exec/normalize.rs` was checked separately:
   its three `TypeExpr`-bearing items are `C1` sealed-cap display
   materialisation; every normaliser decision is node-domain.

## Residual body-reader inventory fold-in (superseded subset)

The residual inventory's live rows map onto this manifest as follows; the
inventory FILE stays in-tree until B8 (its five automatic rails still run) but
this manifest is the authoritative superset.

| Inventory row (class) | Manifest row |
|---|---|
| `lower_decl_body_to_node` (GraphBackedMigrated) | not enumerated — the migrated hot-route anchor names no bare `TypeExpr` (routes through `decl_body_hot_ref`) |
| `lower_decl_body_with_provenance` (ProducerLowering) | not enumerated at method granularity — its tokens name no bare `TypeExpr` (bodies flow through `PreparedTypeDecl` / locator carriers); the file's `TypeExpr`-naming producer methods are the C2 `project_semantic_dispatch/build.rs` `impl ProjectSemanticDispatch::fn build_typeof` / `::fn lower_class_heritage_args` rows |
| `prepare_type_decl_from_lowered` / `prepare_local_value_decl` (ProducerLowering) | not enumerated — both read content-free carriers (`TypeDeclBody` merge shape / `ValueTypeAnnotationFact`), no bare `TypeExpr` token |
| `transient_body_shape` (ProducerLowering) | C2 `decl_body_memo/locator_deref.rs` `fn transient_body_shape` |
| `deref_slot_body` / `lower_body_under_env` (ProducerLowering) | C2 `project_semantic_dispatch/raise.rs` rows of the same names |
| `route_closure` (GraphFreeDto) | not enumerated at method granularity — the driver names no bare `TypeExpr`; its deferred key-source fact mint over lease-only re-borrowed contributor bodies is the C2 `verter_semantic` `facts/route_facts.rs` producer rows |
| `resolve_through_export` / `resolve_one` (GraphFreeDto) | not enumerated — the frontier rows carry graph-free locators, no bare `TypeExpr` token |
| `peel_value_decl_alias_graph_native` / `dependency_value_symbol_graph_native` (GraphFreeDto) | not enumerated — both read the content-free `ValueTypeAnnotationFact` |
| `compat_type_body_hash_input` / `compat_value_body_hash_input` / `LazyBodyFactSource::compute` (OutputCompat) | not enumerated — all three read stored fact/hash inputs, no bare `TypeExpr` token |
| `compat_type_contributors_for_typeinfo` / oracle `walk` (OutputCompat) | C2 `resolver_core/shallow_file_state.rs` `impl ShallowFileState::fn compat_type_contributors_for_typeinfo` (the `allow(dead_code)` oracle-only leg); the oracle `walk` lives in the gated `oracle_core` subtree (excluded, gate asserted) |

## Census verdict

**Zero unexplained rows.** Every production `TypeExpr` authority site enumerated
by the guard — at per-method impl granularity — classifies as C1 (69), C2 (167),
or C3/C3-pending (43 pending + 10 tombstones). Four independent census passes
over the dispatch core, the meta-resolve/projector layer, the
resolver/host/typeinfo layer, and the lower-crate producers each returned zero
SUSPECT sites, and the per-method re-keying of the impl rows surfaced no new
site outside the already-censused impl bodies (every method row inherits its
impl's first-hand-read disposition): no live production site makes a query-time
semantic decision by walking a resolved or materialised `TypeExpr`. The
`C3-pending` population is the compiler-certified dead residue of the
node-domain migration and deletes with the tracked dead-code debt.
