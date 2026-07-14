# Stage-10 B6 P4b — carried debt rows (six-field, committed)

Committed six-field debt ledger for deferrals made during B6 P4b (Stage-10 TypeExpr-terminal-removal,
Wave-3, on-demand-raise completion). Each row records the full six fields the deferral discipline
requires and names an EXECUTABLE fail-closed rail. B8 doc-reconciliation may relocate these rows into
the design-doc carried-obligations enumeration; until then this is their committed home (the `.feedback/`
plan doc is gitignored and is not a committed ledger).

## DEBT ROW #1 — CLOSED: required emit payloads publish faithful PRESENT sources (imported property-style + call-signature composite/rich classes)

- **History.** This row was first marked RETIRED after the same-file leaf-union closure
  (`FactOrLocator::LeafUnion`, `closed_params_tuple_source`, the `ResolvedEmitField` payload-source
  row, and the `define_emits_shape` publication chain — all landed and still in force; the positive
  tracker `evaluated_union_emit_payload_publishes_the_closed_tuple_leaf_union_source` stays GREEN).
  That retirement was FALSE for the imported/composite cases: it held only for the same-file
  leaf-union payload, and the row was RE-OPENED for two REQUIRED-payload classes that first
  degraded to a fabricated `unknown`-as-success and then (the fail-closed honesty cut) to the
  typed `Failed(UnrepresentableRequiredPayload)` interim. Both classes are now REPRESENTABLE and
  the row is CLOSED.

- **Closure mechanism.**
  1. **Callable-parameter projection** (`ProjectedTypeFact::CallableParams { base,
     signature_ordinal, first_param }`, `crates/verter_type_expr/src/facts.rs`): the content-free
     replay address for a realized call-signature payload tuple whose parameters are richer than
     the closed leaf/leaf-union element vocabulary. The emit normalizer
     (`typeinfo/framework_surface/vue_exec/normalize.rs`) mints it for every such row — `base` =
     the macro's stamped type-argument locator, `signature_ordinal` = the surface's
     declaration-order call-signature index (stamped BEFORE event-name expansion/deduplication),
     `first_param = 1` (the event-name strip). Raising it
     (`raise_projected_callable_params`, `project_semantic_dispatch/semantic_source.rs`) replays
     the producing route through the ONE shared dispatch: the same macro-surface projection
     (`resolve_vue_macro_surface_with_ctx`), node-domain ordinal selection, the same
     `published(Navigate)` `CallableNodeView` realization policy, and a TRANSIENT tuple synthesis
     from the raw `FunctionParam`s (labels / optionality / rest / order / nesting / generic
     substitutions preserved; elements keep the raw shallow param nodes). Bounds drift, a missing
     surface, a non-callable ordinal, `first_param` past the parameter list, or an UNRESOLVABLE
     payload-parameter root fails the raise typed (the strict path records the `.tuple[N]`
     interior position) — never an empty-tuple or fabricated-element synthesis.
  2. **Emit-source authority** (`define_emits_shape`,
     `meta_resolve/projectors/define_shapes.rs`): the normalized
     `ResolvedEmitField.payload_source` row is the SOLE emit payload authority; a name-matched
     flat `evaluated_types.emits` field contributes ONLY exactness / execution-status /
     diagnostics metadata. The flat-lane REQUIRED-payload shadow path
     (`MemberValuePosition::RequiredPayload` + the empty-tuple residue projection in the member
     sink) is DELETED — an imported `ImportedEmits { save: [id: number] }` publishes its faithful
     normalized `Closed(Tuple)`, identical to the local authored control.

- **Closure evidence (executable).** The fail-closed trackers FLIPPED to positive real-payload
  assertions in `crates/verter_session/src/meta_tests.rs`:
  `cross_file_call_signature_emit_payload_publishes_the_real_tuple` (was
  `production_component_meta_present_macro_payload_miss_is_typed_failure`) and
  `imported_property_style_emit_payload_matches_the_local_control` (was the imported half of
  `imported_or_composite_required_emit_payload_is_never_unknown_as_success`; imported == local
  pinned by lane equality). New positive repros:
  `composite_call_signature_emit_payload_publishes_all_union_arms` (every union arm present),
  `nested_object_call_signature_emit_payload_stays_a_walkable_shallow_carrier` (shallow object
  carrier; the consumer walk reaches the nested leaf),
  `rich_call_signature_emit_payload_preserves_labels_optionality_rest_and_substitutions`
  (optional generic-instantiated + rest array params). The producer split is pinned at the DTO
  level (`typeinfo_tests/vue_adapter.rs::define_emits_callsig_rich_params_mint_the_callable_params_replay_source`
  — closed tuple / CallableParams / empty-tuple three-way split + the pre-expansion ordinal), the
  fact identity by the `verter_type_expr` witness
  `projected_callable_params_fact_discriminates_base_ordinal_and_first_param`, and NATIVE-binding
  parity by `packages/component-meta/src/native-eval.spec.ts` ("publishes the real payload tuple
  for an imported property-style emit" / "… for a cross-file call-signature emit"). The
  fail-closed rail STAYS EXECUTABLE:
  `call_signature_emit_with_unresolvable_param_stays_a_typed_failure` (cross-file AND local
  forms) pins that a genuinely-unresolvable payload param fails typed with its `.tuple[0]`
  position, and `output_lane_slot_decides_absent_present_and_failed_arms_exhaustively` keeps the
  `Failed → Err` output decision pinned. `Failed(UnrepresentableRequiredPayload)` remains
  reachable ONLY for a call-signature row with NO stamped macro type-argument base (nothing to
  replay off) — a fail-closed guard position, not a supported-class residue.

- **Residual (re-homed, NOT closed here).** The structural MEMBER-VALUE projection for shallow
  published member values (props / exposed / options / slots members and richer index-signature
  key/value positions) previously pointed at this row is NOT emit-payload work and moves to its
  own row — see DEBT ROW #3 below.

- **Deferral count this block.** Closed; contributes 0.

## DEBT ROW #2 — fine-grained graph-side absent-vs-failed `Opaque` provenance (output fail-open CLOSED)

- **Exact item.** The OUTPUT fail-open this row previously carried is CLOSED: a `Present` output
  source whose materialized shape carries an unknown-materializing `Opaque` failure carrier at its
  ROOT or INTERIOR now FAILS output materialization typed
  (`ComponentMetaOutputFailure::UnknownMaterializingSourceInterior` — the conservative whole-tree
  node-domain miss check in `materialize_output_source`, via the shared
  `node_contains_semantic_miss_with_dispatch` fact; the legitimately publishable carriers — a
  recursive reference, a declaration placeholder — are not misses and pass). What REMAINS deferred
  is only the FINE-GRAINED graph-side provenance: the graph does not carry per-position
  absent-vs-failed provenance on the `Opaque` carrier, so the conservative interim cannot let a
  PROVEN-absent nested position (an unannotated parameter, an inferred method return inside a
  composed body) keep rendering the typed `Unknown` — it fails the whole source instead.

- **Rails in force.** Three fail-closed rails cover the output boundary: (a) the typed three-state
  `SourcePosition` producer contract (`Failed(SemanticSourceFailure)` fails output typed;
  `Absent(SchemaAbsence)` renders the centralized typed `Unknown` and stays a valid success);
  (b) the strict interior-miss rail (`raise_semantic_type_source_to_hot_strict` +
  `InteriorFailureSink` → `InteriorSourceMiss` with the nested position path) on every
  PRESENT-but-failed REQUIRED locator dereference of a composed fact shell; (c) the conservative
  interior fail-close (`UnknownMaterializingSourceInterior`) on every successfully-raised source
  whose materialized shape still carries an unknown-materializing `Opaque` anywhere. No `Present`
  source shell-folds an interior failure into a completed `unknown`.

- **PRODUCER CONTRACT (enforced by `SourcePosition`).** A source-construction/resolution FAILURE
  must NEVER be encoded as `Closed(Leaf(Unknown))` — schema-absence, authored `unknown`, and
  source-failure are SEPARATE typed states (`Absent(SchemaAbsence)` / `Present(source)` — including
  the authored/open `Present(Closed(Leaf(unknown)))` success — / `Failed(SemanticSourceFailure)`).
  A producer that cannot construct a faithful source for a REQUIRED position publishes `Failed`;
  only a PROVEN structural absence publishes `Absent`. The shallow prop/exposed/options/slots
  member-value positions in `surface_member_to_expanded_field`
  (`MemberValuePosition::ShallowMember`) and the index-signature key/value positions
  (`ExpandedIndexSignature.{key_type,value_type}: SourcePosition`) carry this three-state
  contract with their faithful PRESENT representation landed (DEBT ROW #3, CLOSED): a KNOWN
  structural value publishes its shallow carrier / replay route; ONLY a genuine miss publishes
  `Failed(UnrepresentableRequiredMemberValue)`.

- **Why the remaining half cannot be built now.** Distinguishing "absent by schema" from "present
  but failed" AT EVERY NESTED POSITION requires the graph to carry typed absent-vs-failed
  provenance on the `Opaque` carrier (or a strict shell-fold that threads the interior-failure
  sink through `fold_to_type_expr`) — a graph-side substrate change in the shared shape engine
  affecting resolver control flow, relation behavior, interning, and cache admission globally.
  Only that precision is deferred; fail-closed output behavior is NOT deferred.

- **Owning future block / closure condition owner.** The shared dispatch / shape-engine follow-up
  (graph-side `Opaque` provenance / strict-shell-fold owner block; B8 doc-reconciliation may
  re-home the row).

- **Temporary behavior.** Lane-level schema-absent positions (`SourcePosition::Absent`) render the
  canonical typed `Unknown` — honest absence, a valid success. A `Present` source with any
  unknown-materializing `Opaque` in its materialized shape fails typed — CONSERVATIVE: a nested
  position that is semantically absent (an inferred method return inside a composed body) fails
  with it until the graph can prove the absence per position. Every producer-declared failure and
  every failed required locator dereference fails typed.

- **EXECUTABLE fail-closed rail.**
  `present_source_with_interior_unknown_materializing_opaque_fails_output`
  (`crates/verter_session/src/meta_tests.rs`) drives the REAL `get_component_meta_output`
  production path over a `Present` source whose deref'd body interns an interior miss and pins the
  typed `UnknownMaterializingSourceInterior` failure;
  `component_meta_output_failed_interior_locator_fails_closed_per_source_family` pins the strict
  `InteriorSourceMiss` nested-position path;
  `prop_member_value_referencing_nonexistent_type_stays_failed` pins the member-value
  genuine-miss producer fail-close. The producer contract is pinned by the `SourcePosition` witnesses
  (`crates/verter_type_expr/src/source_position_witnesses.rs` — the three states never alias),
  the genuine-miss emit control on DEBT ROW #1
  (`call_signature_emit_with_unresolvable_param_stays_a_typed_failure`), and the member-value
  fail-closed trackers listed on DEBT ROW #3.

- **Condition that closes it.** Graph-carried provenance (or a strict shell fold threading the
  sink) distinguishes absent-vs-failed at EVERY nested position: a proven-absent nested position
  renders the typed `Unknown` while a failed one keeps failing typed, and the conservative
  whole-source `UnknownMaterializingSourceInterior` check narrows to failed-provenance positions
  only.

- **Deferral count this block.** 2 of ≤ 3.

## DEBT ROW #3 — CLOSED: structural MEMBER-VALUE projection for shallow published member values

- **History.** Re-homed from DEBT ROW #1's residual as the member-value analogue of the emit
  closure: a props / exposed / options / slots member whose value is a function / inline object /
  non-empty tuple / composite with no authored slot, no use-site slot, no reference identity, and
  no closed fact (the former `MemberValuePosition::ShallowMember` fail-closed residue in
  `surface_member_to_expanded_field`), plus the richer index-signature key/value positions
  (`index_position_source`), carried the typed `Failed(UnrepresentableRequiredMemberValue)`
  interim. Every KNOWN-structure class is now REPRESENTABLE and the row is CLOSED.

- **Closure mechanism.**
  1. **Structural member-source projection**
     (`structural_member_value_source`, `meta_resolve/projectors/published_source.rs`): a member
     value with no authored/use-site/closed source demand-validates through the ONE shared
     structural-fact primitive (`ProjectSemanticDispatch::demand_validated_structural_node` —
     carrier-preserving STRUCTURAL TRANSIT, the same per-root validation the callable-params
     replay applies; validation only, no library keyspace enumerated at publication). A KNOWN
     structural value publishes its faithful shallow carrier: the closed/ref upgrade on the
     DEMANDED node (a resolvable reference publishes its shallow symbol-reference carrier), or
     the projected MEMBER-PATH replay route (`ProjectedTypeFact::MemberPath` — the macro's
     stamped type-argument base + the member name, replayed through the one dispatch's EXISTING
     `ProjectPath` query on demand). Replay-route sources are FINAL consumer-demand addresses:
     the publication finalize and the extraction never re-raise them (the Rule-5 audit-footprint
     fan-out stays closed — the `block_6i` guards pin it).
  2. **Index-position projection** (`ProjectedTypeFact::IndexPosition { base, signature_ordinal,
     position }`, `crates/verter_type_expr/src/facts.rs` + `raise_projected_index_position`,
     `project_semantic_dispatch/semantic_source.rs`): the content-free replay address for a
     richer index-signature KEY/VALUE position (which carries no member name a member-path hop
     could address) — the surface's declaration-order ordinal plus the key/value role, replayed
     through the same macro-surface projection the normalizer read.
  3. **Normalized member-source authority** (`ResolvedPropField` / `ResolvedExposeField`,
     `typeinfo/framework_surface/results.rs`; `ResolvedMacroInput`'s
     `ResolvedPropInput`/`ResolvedEmitInput`/`ResolvedExposeInput` rows): the normalized macro
     rows own every published `SourcePosition` (closed leaf/leaf-union first — two files'
     identical closed members publish the IDENTICAL source value so the output memo shares one
     entry; then the single-candidate authored position; then the shallow-ref / use-site /
     member-path ladder). `define_props_shape` publishes the row source directly and the
     extraction publishes the row sources; the flat `evaluated_types` lanes contribute ONLY
     exactness/status/diagnostics metadata (`merge_evaluated_prop_types_into_meta` and the
     flat→lane source copy are DELETED); the `define_props` lane is mechanically DERIVED from
     the normalized surface and finalized per position through the shared publication finalize
     (`finalize_published_prop_source`). The model prop/event type source comes from the
     normalized `defineModel` surface row — never a same-name flat row from a sibling macro.

- **Closure evidence (executable).** The fail-closed trackers FLIPPED to positive real-structure
  assertions in `crates/verter_session/src/meta_tests.rs`:
  `imported_shallow_function_member_publishes_present_structural_source` (was
  `imported_shallow_member_without_faithful_source_fails_typed`; `onClick: () => void` publishes
  the member-path source and renders the function shape, COMPLETE) and
  `richer_index_signature_value_position_publishes_projected_replay` (was
  `richer_index_signature_value_position_fails_typed`; the value position publishes the
  index-position replay and the consumer demand-walk reaches the nested leaf); the four
  `define_expose_*` member tests assert the Present member-path sources. Sole-authority is pinned
  by `normalized_prop_rows_are_the_published_source_authority` (lane + extracted position ARE the
  normalized member-path source) and
  `model_type_source_comes_from_the_normalized_define_model_surface` (a same-name sibling
  `defineProps` flat row no longer shadows the model's own type), plus the lower-crate pair
  `flat_evaluated_props_contribute_metadata_not_the_source` /
  `flat_evaluated_types_never_shadow_the_row_source`
  (`crates/verter_semantic/src/analysis/component_meta_tests.rs`). The fact identity is pinned by
  `projected_index_position_fact_discriminates_base_ordinal_and_position`
  (`crates/verter_type_expr/src/projected_route_witnesses.rs`); NATIVE-binding parity by
  `packages/component-meta/test/member-value-honesty.test.ts` (the function member publishes the
  shallow function structure; the object member publishes the shallow object and its nested
  leaf). The fail-closed rail STAYS EXECUTABLE:
  `prop_member_value_referencing_nonexistent_type_stays_failed` pins that a genuinely-unresolvable
  member value (a broken reference) still fails typed, non-complete, suppressed — 
  `Failed(UnrepresentableRequiredMemberValue)` is reachable ONLY from genuine misses (an
  unresolvable residual carrier, an unknown-materializing failure, a torn partial read, or no
  stamped replay base); the positive controls
  (`recoverable_shallow_prop_values_still_complete_as_present`,
  `present_source_with_interior_unknown_materializing_opaque_fails_output`) keep the recoverable
  and interior-fail-close arms pinned.

- **Residual.** None on this row. The fine-grained graph-side absent-vs-failed `Opaque`
  provenance remains the ONLY member-side deferral — DEBT ROW #2 above.

- **Deferral count this block.** Closed; contributes 0.

## DEBT ROW #4 — full WASM/NAPI component-meta resolution parity (typed-unavailable honesty LANDED)

- **Exact item.** Full WASM/NAPI component-meta resolution parity. The resolution-less WASM
  `getComponentMeta`/`getComponentMetaBatch` surfaces cannot produce the resolved `type_registry`
  name-overlay + `resolution` sidecar that the type-resolution-seeded NAPI/LSP surfaces (the
  plain NAPI payload entries `getComponentMeta`/`getResolvedComponentMeta`,
  `getComponentMetaWithAudit`, the LSP host) produce. The core prop/event/slot lanes ARE
  identical across the surfaces; ONLY the resolved-registry overlay and the resolution sidecar
  diverge. This row is terminal-bar-orthogonal: an output-honesty deferral, not a
  semantic-`TypeExpr` carrier.

- **Why it cannot be built now.** A resolution-less WASM host lacks an injected canonical
  dependency graph and project/lib environment — no filesystem, project, or package resolver
  stands behind the plain WASM session. Reproducing fs/project/package resolution inside WASM, or
  adding a WASM-only resolver, would violate the single-engine rule (exactly ONE type-resolution
  engine); a second query-time resolution path is the divergence/hang bug class.

- **Owning future block / closure condition owner.** The WASM-resolution-host capability: inject
  the complete canonical dependency graph + project/lib env into the shared `verter_session`
  substrate (the one engine) — NOT a WASM-local resolver.

- **Temporary behavior.** The WASM resolution-less lane reports the typed
  `Unavailable(ResolutionProviderAbsent)` `resolutionStatus` — fail-closed-honest, never a
  fake-exact/successful-looking empty registry; the un-overlaid registry self-describes as
  partial on the resolution axis. The status is an additive, always-serialized wire field
  (`FfiComponentMeta.resolution_status`, `crates/verter_protocol/src/types.rs`) set at the one
  mechanical conversion (`verter_ffi::convert::component_meta`), so every sidecar-less payload
  carries it. NAPI `getResolvedComponentMeta` (`crates/verter_napi/src/meta.rs`) is the SAME
  fully-resolution-seeded lane as `getComponentMeta` (`js_name` kept for wire compatibility);
  the genuinely sidecar-less surfaces are the plain WASM lanes. No fail-open-as-success on
  this axis anywhere.

- **EXECUTABLE fail-closed rail.**
  `resolution_less_conversion_reports_typed_unavailable_status_never_silent_success`
  (`crates/verter_ffi/src/convert/tests.rs`) pins that the resolution-less conversion
  self-describes the typed unavailability on the serialized wire (`resolutionStatus.kind ==
  "unavailable"`, `resolutionStatus.reason == "resolutionProviderAbsent"`), fabricates no
  `resolution` sidecar, and that the resolution-bearing conversion reports `Resolved` — never a
  silent success on either arm.

- **Condition that closes it.** WASM accepts a complete INJECTED canonical dependency graph +
  project/lib environment fed into the shared session substrate, and the WASM lane produces
  output byte-equivalent to NAPI on a hermetic fixture (resolved `type_registry` overlay +
  `resolution` sidecar included) — at which point the lane reports `Resolved`.

- **Deferral count this block.** 2 of ≤ 3 open across the ledger (this row + DEBT ROW #2; rows
  #1 and #3 are CLOSED).
