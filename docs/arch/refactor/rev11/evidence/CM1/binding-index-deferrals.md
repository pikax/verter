# Deferred findings — owner-aware root value-binding index

Per CLAUDE.md's Fix Quality / explicit finding disposition rule: every
scope-deviating correctness finding is dispositioned before related work
continues.

## D1 — nullable constructor-array element (`defineProps({ label: [String, null] })`)

**Disposition: DEFER.**

**Finding.** Whether a `null` entry in a runtime-constructor array means
"this prop may also be `null`" (Vue's own documented nullable-constructor
convention) is a real Vue-runtime-semantics question this block did not
verify against Vue's own compiler/runtime behavior. The prior deleted
mechanism's own evidence packet (the `block/cm1` attempt, reverted at
`a7bf8c696`) flagged the same interpretation as unconfirmed rather than
self-evident, and the ratified design
([`binding-index-design.md`](binding-index-design.md), "Amendment (v3)" §
"Nullable constructor-array element — DEFER, do not silently decide")
carried the same instruction forward.

**What is implemented now.** A `null` array element resolves to
`ConstructorBindingOutcome::Indeterminate` — the SAME fail-closed channel a
`with`/sloppy-`eval`/ambiguous-topology reference resolves to. This is
deliberately NOT a guess at Vue's nullable-constructor semantics (it does
NOT add `PrimitiveName::Null` to a `LeafUnion`, and it does NOT silently
drop the element) — it is the honest "static resolution does not decide
this" outcome, which fails the position closed
(`SemanticSourceFailure::UnrepresentableRequiredMemberValue`) rather than
publishing a fabricated type. See
`nullable_constructor_array_element_is_indeterminate_not_guessed` in
`crates/verter_semantic/src/analysis/root_binding_index_tests.rs`.

**Owner:** this evidence directory (CM1).

**Resolution gate:** before this block's final review, OR a follow-up block
that verifies Vue's actual nullable-constructor runtime behavior (does
`[String, null]` genuinely mean "nullable string prop", and if so does the
published type need `PrimitiveName::Null` added to the `LeafUnion`, or a
different representation) and updates
`constructor_binding_source_position` in
`crates/verter_semantic/src/analysis/component_meta.rs` accordingly, with a
test asserting the confirmed interpretation.

**Why DEFER and not ADOPT-NOW:** adopting a guessed semantics now would risk
landing an incorrect published type for every SFC author using this
(fairly common) Vue idiom — worse than the current fail-closed
`Indeterminate`, which at least never fabricates a wrong answer.

**Why DEFER and not REJECT:** the underlying question (does Verter support
nullable runtime-constructor props at all) is real product scope, not
something to discard — it needs verification, not rejection.

## D2 — `defineModel` runtime-constructor gating (`defineModel({ type: String })`)

**Disposition: DEFER.**

**Finding.** An independent adversarial review (codex xhigh) of the
implementation found `defineModel` runtime constructors are not gated
through `RootBindingIndex` at all. Verified directly: `extract_define_model_type`
(`crates/verter_semantic/src/analysis/macros.rs`) extracts ONLY the
type-argument form (`defineModel<T>()`) — it returns an EMPTY `Vec` for
every runtime-argument form (`defineModel({ type: String })`, named
`defineModel('name', { type: String })`) on trunk TODAY, independent of
this block. There is no existing runtime-constructor extraction for
`defineModel` for this block to gate; the design doc's "Consumer wiring"
and discriminating-test-matrix sections enumerate only `defineProps` and
Options-API `props:`. `StartScope`'s doc comment and
`root_binding_index.rs`'s module doc comment mention `defineModel`
aspirationally (matching the design's stated intent for where
`ProgramRoot` resolution WOULD apply once such extraction exists), which
is accurate future-facing documentation, not a claim that the extraction
exists today.

**What is implemented now.** Nothing new for `defineModel` runtime
arguments — unchanged from trunk (still no field for a runtime-only
`defineModel` options object).

**Owner:** this evidence directory (CM1), or a new charter for
`defineModel` runtime-argument support generally.

**Resolution gate:** a follow-up block that (a) implements
`defineModel`'s runtime-options-object extraction (a NEW capability, not
a gating fix — `extract_define_model_type` needs a whole new branch
mirroring `extract_prop_fields_from_runtime`'s shorthand/expanded-object
handling) and (b) threads the SAME `resolve_runtime_constructor_identifier`
/ `resolve_runtime_constructor_array` gate through it.

**Why DEFER and not ADOPT-NOW:** implementing `defineModel` runtime-object
extraction from scratch is a materially larger scope addition than "gate
the existing extraction with the binding index" — the charter this block
implements did not ratify that addition.

## D3 — session-side `Local` resolution is fail-safe but not fail-complete for every genuinely local declaration shape

**Disposition: ADOPT-NOW the fail-closed behavior; DEFER full coverage.**

**Finding.** The same adversarial review flagged that
`collect_local_constructor_binding_keys` (`crates/verter_session/src/resolver_core/component_meta/mod.rs`)
feeds `RootBindingIndex`-proven `Local` keys directly into
`expand_macro_types_impl_with_expander`'s `BindingExpansionEntry` demand
list, bypassing `component_meta_binding_type_entries`'s own
`ShallowFileState::visible_value_binding` re-derivation (deliberately —
see that function's doc comment for why routing through it would be a
second, potentially-diverging binding-resolution engine for a question
`RootBindingIndex` already answered). Two consequences verified directly:

1. `ShallowFileState`'s shallow declaration index is TOP-LEVEL-statement
   scoped (per CLAUDE.md's Shallow File Processing Core Invariant), while
   `RootBindingIndex` sees every runtime-surviving binding including a
   hoisted nested `var`/Annex-B function-in-block. A constructor shadowed
   by such a binding is genuinely `Local` per the index, but the
   downstream `ctx.prepared_value_decl` lookup the expander closure
   ultimately calls may not find a top-level declaration under that name.
2. `ExpandedComponentTypes.bindings` is keyed by NAME ONLY (pre-existing —
   the same lane `defineExpose` already resolved through, not introduced
   by this block), so a same-name constructor shadow and an unrelated
   `defineExpose` binding from a DIFFERENT owner can both be admitted.

**What is implemented now.** Both consequences fail CLOSED, never wrong:
(1) an unresolvable `prepared_value_decl` lookup flows through the
existing `ResolvedTypeOutcome::{Absent, Failed}` discipline —
`constructor_binding_source_position` treats `Absent` as "no position"
(the caller's own unannotated fallback) and `Failed` as a typed failure,
never a fabricated type. (2) `constructor_binding_source_position`
explicitly detects a same-name collision in `ExpandedComponentTypes.bindings`
(more than one entry matching the shadowing declaration's name) and fails
closed (`SemanticSourceFailure::UnrepresentableRequiredMemberValue`)
rather than silently picking one. See
`constructor_local_ambiguous_cross_owner_name_collision_fails_closed` and
`constructor_array_mixing_local_with_anything_else_fails_closed` in
`crates/verter_semantic/src/analysis/component_meta_tests.rs`.

**Owner:** this evidence directory (CM1) for the fail-closed mitigation
(adopted); `verter_session`'s shallow-declaration-index owner for full
coverage of nested-hoisted-var `Local` shadows, and the `ExpandedField`
owner for owner-aware (not name-only) binding-lane keying, if either gap
proves to matter in practice.

**Resolution gate:** none required to land this block — the fail-closed
behavior is the correct terminal state for a case not yet fully
resolvable, not a placeholder. Revisit only if real-world SFCs surface a
hoisted-nested-var or cross-owner-same-name constructor shadow that
should resolve but currently degrades to unannotated/failed.

**Why ADOPT-NOW the mitigation:** the alternative (skip the ambiguity
check, pick the first name match) risks silently publishing a WRONG
type — strictly worse than the current honest failure.

**Why DEFER full coverage:** extending the shallow declaration index to
cover non-top-level hoisted declarations, and re-keying the shared
`ExpandedField`/`ExpandedComponentTypes.bindings` lane by
`(owner, name)` instead of name alone, are both changes to shared
machinery well beyond gating an existing extraction with this index.
