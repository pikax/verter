<!-- unified-charter-v2
id=CCA2D
name=Unqualified style boundary deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA2DV,CCA2DS
owner=compiler.compiler-bridge:terminal unqualified-style input and DTO deletion
conflict_domains=style_semantics,compiler_execution
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA2D.md
max_production_loc=500
max_production_files=10
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2D — Unqualified style boundary deletion

## Independently acceptable outcome and owners

Delete the unqualified style transport — `RuntimeStyleBlock`, the `styles` lists on `RuntimeCompileOutput` and `DirectCompileOutput`, and the bridge's compatibility population of them — so every carrier publishes every `<style>` output as one stage-qualified `QualifiedRuntimeStyle`. Final style handoff ownership is CCA2D0's typed continuation. Reverting restores the unqualified transport and the two defects named under the amendment.

J4 owns deletion of style-owner-local unqualified preprocessor output. This node owns the compiler bridge transport, `RuntimeStyleBlock`, the bridge compatibility population, and — by the amendment below — the unselected Vue carrier route that still depended on them.

## Amendment (ratified scope)

The original charter assumed CCA2DV's contract held. It did not: CCA2DV moved only the *fully host-selected* Vue style route onto the qualified boundary. The plain Vue carrier route still published `RuntimeStyleBlock`, and `apply_selected_runtime_styles` kept a `selects_style_fully` dual-publication fallback. Deleting the transport therefore requires finishing that migration here, rather than reopening CCA2DV, because the completion and the deletion are the same change: the unqualified list is removed in the same edit that stops populating it.

Two defects fall inside that change and are owned here:

- The Vue main-module prelude counted the unqualified list, which a full host style selection had emptied, so a component whose every `<style>` block had host-selected content published no style import.
- The route identity digest did not hash a qualified style's dialect, producer (kind, named identity, version, config fingerprint) or refusal flag, so routes could publish byte-identical CSS under different provenance and still compare equal.

## Exact population and boundary

- `crates/verter_compiler/src/framework_common/carrier_compiler.rs`, `framework_common/mod.rs` — delete `RuntimeStyleBlock`, `RuntimeCompileOutput::styles` and the export.
- `crates/verter_compiler/src/compile/types.rs`, `compile/mod.rs` — `VerterStyleBlock` carries the Vue cascade's own `QualifiedStyleResult` instead of discarding it at `into_code()`; `None` exactly when authored bytes are in a `lang` naming no admitted dialect.
- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — hand that value to the qualified list without re-deriving any stage.
- `crates/verter_compiler/src/standalone.rs` — delete `DirectCompileOutput::styles` and the `selects_style_fully` fallback; the route identity digest hashes the whole qualified identity.
- `crates/verter_compiler/src/assembly/vue_module.rs` — the prelude counts published styles.
- `crates/verter_session/src/compile.rs`, `host_compile_audit.rs`, `host_resolve/virtual_file_pipeline.rs` — read the one published list.
- Focused fixture/test construction is retargeted to qualified style artifacts using durable style identity/stage wording.

Do not alter `StyleStage`, `QualifiedStyleResult`, the qualified continuation/artifact, or CSS preprocessing/semantics. Never publish a qualified value whose stage, dialect or producer is not a fact of its bytes: where no truthful value exists, publish none.

## Exact predecessor contracts and binding laws

- **CCA2DV:** Vue consumers on the host-selected route use the qualified continuation. The unselected carrier route's migration is completed by this node (see Amendment).
- **CCA2DS:** all Svelte style consumers use the qualified continuation; Svelte-side unqualified fields/adapters are absent.
- Every legacy declaration has zero production consumer after this change. No compatibility fallback or dual DTO may be retained.
- A Vue style block whose authored bytes are in a `lang` the rewrite cannot name has no qualified value. Its compile already carries that refusal as an error and publishes no styles: the style list is an index space the host's imports and virtual files are keyed by, so it is never published with a hole.

## Acceptance, performance, aborts, and verification

- **CCA2D-AC1:** repository-wide structural/type evidence finds no unqualified style input, `RuntimeStyleBlock`, export, constructor, helper, or fallback.
- **CCA2D-AC2:** Vue/Svelte CSS bytes, qualified maps, diagnostics, provenance, stage/basis, source order, scoped/modules/global behavior remain equivalent, except exactly: (a) an error-bearing Vue compile refusing an unnameable style dialect publishes no styles; (b) a fully host-selected Vue component imports its styles again; (c) a block that authored no bytes in a `lang` naming no admitted dialect reports the base CSS dialect for those zero bytes (a host-selected block's placeholder, replaced by the selection). A host-supplied result for an unnameable authored dialect still publishes its preprocessed result under the named producer.
- **CCA2D-AC3:** fresh/incremental/cancellation/complete-only evidence from both migrations remains green; the direct, prepared-first, prepared-repeat and batch routes agree under the strengthened digest.
- **CCA2D-AC4:** deletion adds no work; one qualified continuation remains per applicable style and absent/inapplicable style stays zero-work.
- **CCA2D-AC5:** the route identity digest changes when only a qualified style's refusal, producer kind, producer identity, version, config fingerprint, dialect, result stage or consumed stage changes; its style-count plant still collides when the count is removed from the hasher.

Ceiling: 500 production LOC, 10 production files, 2 crates. Abort on a CSS semantic/preprocessor change, a `StyleStage`/`QualifiedStyleResult`/continuation mutation, a fabricated qualification, or an eleventh production file. Run structural scans plus Vue/Svelte style/preprocessor/host/map suites and `targeted-domain`. CCA2F consumes the legacy-free qualified boundary.
