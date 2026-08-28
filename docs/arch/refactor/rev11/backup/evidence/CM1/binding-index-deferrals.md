# Deferred findings — owner-aware root value-binding index

Per CLAUDE.md's Fix Quality / explicit finding disposition rule: every
scope-deviating correctness finding is dispositioned before related work
continues.

## D1 — nullable constructor-array element (`defineProps({ label: [String, null] })`) — RESOLVED

**Disposition: ADOPT-NOW.** (Originally DEFERRED; resolved before this
block's final review, per the resolution gate this entry itself named.)

**Original finding.** Whether a `null` entry in a runtime-constructor array
means "this prop may also be `null`" (Vue's own documented
nullable-constructor convention) was a real Vue-runtime-semantics question
this block had not verified against Vue's own compiler/runtime behavior.

**Verification.** Confirmed directly against the vendored
`@vue/runtime-core` source (`getType`/`assertType` in
`packages/runtime-core/src/componentProps.ts`, inspected at
`node_modules/.pnpm/@vue+runtime-core@*/node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.js`):

```js
function getType(ctor) {
  if (ctor === null) return "null";
  ...
}
function assertType(value, type) {
  const expectedType = getType(type);
  if (expectedType === "null") valid = value === null;
  ...
}
```

`[String, null]` therefore means "this prop accepts a `String`-typed value
OR the literal value `null`" — the ordinary nullable-constructor idiom, not
an unconfirmed guess.

**What is implemented now.** A `null` array element is a LITERAL, never an
identifier — it cannot be locally shadowed, so `resolve_runtime_constructor_array`
never gates it through `RootBindingIndex` at all; it always resolves
`ConstructorBindingOutcome::Global` with spelling `"null"`.
`constructor_binding_source_position`'s primitive fold
(`crates/verter_semantic/src/analysis/component_meta.rs`) maps that
spelling to `PrimitiveName::Null`, so `[String, null]` publishes the closed
`LeafUnion([Primitive(String), Primitive(Null)])` fact — `string | null`.
See `nullable_constructor_array_element_resolves_global_null` in
`crates/verter_semantic/src/analysis/root_binding_index_tests.rs` and
`constructor_array_null_element_folds_to_primitive_null_union` in
`crates/verter_semantic/src/analysis/component_meta_tests.rs`.

**Owner:** this evidence directory (CM1).

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

**Disposition: ADOPT-NOW the fail-closed behavior AND owner-aware binding-lane
keying; DEFER only the remaining nested-hoisted-var shallow-index gap.**

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
2. `ExpandedComponentTypes.bindings` was keyed by NAME ONLY, so a same-name
   constructor shadow and an unrelated `defineExpose` binding from a
   DIFFERENT owner could both be admitted and only be told apart by an
   honest ambiguity failure.

**What is implemented now.** (1) An unresolvable `prepared_value_decl`
lookup still flows through the existing `ResolvedTypeOutcome::{Absent,
Failed}` discipline — `Failed` fails closed as a typed failure, and a
PROVEN `Local` binding whose evaluated authority came back `Absent` ALSO
fails closed (`SemanticSourceFailure::UnrepresentableRequiredMemberValue`),
never falling through to the caller's own unannotated/display-text route
(see `constructor_local_absent_evaluated_authority_fails_closed` in
`crates/verter_semantic/src/analysis/component_meta_tests.rs`). (2)
`ExpandedField` now carries `owner: TopLevelOwnerId` (populated from
`BindingExpansionEntry::owner`/`DeclBindingKey::owner` at every producer),
and `constructor_binding_source_position` matches `ExpandedComponentTypes.
bindings` by `(owner, name)` instead of name alone — a genuine cross-owner
same-name pair now DISAMBIGUATES correctly instead of failing closed (see
`constructor_local_cross_owner_same_name_disambiguates_by_owner`); only a
same-owner same-name collision (never expected in practice) still fails
closed (see `constructor_local_ambiguous_same_owner_name_collision_fails_closed`).
Both in `crates/verter_semantic/src/analysis/component_meta_tests.rs`.

**Owner:** this evidence directory (CM1) for the fail-closed mitigation and
owner-aware keying (both adopted); `verter_session`'s shallow-declaration-
index owner for full coverage of nested-hoisted-var `Local` shadows (still
open).

**Resolution gate:** none required to land this block for the two adopted
items. The remaining nested-hoisted-var shallow-index gap is the correct
terminal state for a case not yet fully resolvable, not a placeholder —
revisit only if a real-world SFC surfaces a hoisted-nested-var constructor
shadow that should resolve but currently degrades to unannotated/failed.

**Why ADOPT-NOW the mitigation and the owner-aware keying:** the
alternative (skip the ambiguity check, pick the first name match) risks
silently publishing a WRONG type — strictly worse than the current honest
failure; and owner-aware keying was a small, contained field addition
(`ExpandedField.owner`) already threaded from producers that had the owner
in hand, not the "materially larger scope" the original DEFER assessed.

**Why DEFER the remaining gap:** extending the shallow declaration index to
cover non-top-level hoisted declarations is a change to shared machinery
well beyond gating an existing extraction with this index.
