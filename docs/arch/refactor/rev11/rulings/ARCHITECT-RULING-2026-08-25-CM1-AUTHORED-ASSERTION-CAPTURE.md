---
ruling_id: "CM1-AUTHORED-ASSERTION-CAPTURE-2026-08-25"
type: "architecture-ruling"
date: "2026-08-25"
date_source: "stated"
binds: ["CM1"]
source_file: "ARCHITECT-RULING-2026-08-25-CM1-AUTHORED-ASSERTION-CAPTURE.md"
summary: "Ratifies a third deferred capture for CM1. The runtime-form value `mixed runtime + type-declared` is not satisfiable: both authored spellings, `X as PropType<T>` and `X as () => T`, are detected and stamped at the same payload position, but runtime object members begin as a closed Unknown leaf and the props normalizer selects that leaf before consulting the authored payload, so the declared type never reaches publication. Structural equivalence, established at the loss point rather than inferred from both rendering `unknown`. Recovering the discarded type would change published props[].type correctness, which the type waiver forbids any block from opening, so the value is excluded from the demanded matrix and carried as an ignored discriminating capture owned by the post-program type-correction work."
supersedes:
  - ruling: "CM1-RUNTIME-FORM-AXIS-2026-08-24"
    claim: "Its CONCLUSION only — that `mixed runtime + type-declared` is satisfiable and owed as an uncovered acceptance cell. Its DETECTION evidence is preserved and cited by this ruling: the analyzer does accept both `X as PropType<T>` and `X as () => T` as authored type positions. What is superseded is the inference from detection to publication, falsified by the fresh public-boundary execution that ruling itself required as its gate."
superseded_by: []
contradicts: []
notes: "Supersedes the prior ruling that had ordered an uncovered cell. That ruling's detection evidence was correct and its conclusion was not: detection does not prove publication, and its premise was falsified by the fresh public-boundary execution it had itself required as the gate. The cell was written to the ruling's required values, executed, and failed on both surfaces; the already-excluded PropType<T> capture reproduced the identical loss in the same run. Scoped to the macro route: the script-setup-local class capture is a bare constructor reference distinguished by declaration site, a different mechanism, and is deliberately not merged with these two."
---

# Architect ruling — authored runtime-prop assertion publication capture

**Status:** RATIFIED  
**Date:** 2026-08-25  
**Lane:** `architect-owed-cell-red`  
**Reviewed:** `cdca7f35a4edbe61b81b6d4043657b03fed9f22b`  
**Supersedes:** the prior `architect-runtime-form-axis` ruling that ordered an uncovered cell.

Transcribed verbatim from the seat's verdict. The receipt was validated
(`check-results.mjs`: ALL SOUND, PASS, no findings) before the ruling was acted on.

---

## Ruling

**RATIFIED — THIRD DEFERRED CAPTURE.** For the `defineProps({...})` macro route, the proposition is correct. `mixed runtime + type-declared` is excluded from CM1’s demanded matrix, not covered and not repaired here.

Both spellings are detected, stamped as the same `MacroPayloadPosition::Field`, and replayed through the same lowerer. Their authored type is then lost at one concrete publication site: runtime-object members begin as `Primitive(Unknown)`, and the props normalizer selects that closed leaf before considering the authored payload locator. The output sink consequently renders the selected unknown leaf directly. This is structural equivalence, not an inference from identical output. [macros.rs:3003](crates/verter_semantic/src/analysis/macros.rs:3003), [macros.rs:1209](crates/verter_semantic/src/analysis/macros.rs:1209), [macros.rs:1023](crates/verter_semantic/src/analysis/macros.rs:1023), [vue_exec/mod.rs:733](crates/verter_session/src/typeinfo/framework_surface/vue_exec/mod.rs:733), [normalize.rs:204](crates/verter_session/src/typeinfo/framework_surface/vue_exec/normalize.rs:204), [output_sink.rs:1202](crates/verter_session/src/meta_resolve/projectors/output_sink.rs:1202)

Fresh execution reproduced `UnknownPrimitive` in cold, warm, sequential, batch, and overlay native lanes; the unignored existing `PropType<T>` capture produced the same `UnknownPrimitive` on its macro arm. A fresh compat public-checker call produced exact sibling folds and `item: "unknown | undefined"`. Compat merely projects native semantic output and therefore introduces no second loss point. [runtime_constructor_matrix.rs:948](crates/verter_session/tests/cases/runtime_constructor_matrix.rs:948), [runtime_constructor_matrix.rs:1169](crates/verter_session/tests/cases/runtime_constructor_matrix.rs:1169), [CLAUDE.md:213](CLAUDE.md:213)

This is a publication-stage **type-correctness defect**. Stage does not decide ownership: CM1 owns runtime-constructor facts and folding; the exact sibling folds prove that half works. Restoring the discarded authored `T` changes the correctness of published `props[].type`, which the waiver forbids every program block from opening. It is the same macro-side defect class already demonstrated by `PropType<T>`, not the charter’s “third distinct defect class.” [CM1.md:110](docs/arch/refactor/rev11/charters/CM1.md:110), [CM1.md:225](docs/arch/refactor/rev11/charters/CM1.md:225), [type waiver:37](docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-BUGS-AND-TYPES.md:37)

Required axis wording:

`mixed runtime + type-declared — EXCLUDED: deferred authored runtime-assertion type-publication capture; not a demanded CM1 cell`

The evidence row must claim: runtime siblings fold exactly across all invocation/view modes; the authored field reproducibly publishes native `Primitive(Unknown)` and compat `unknown | undefined`; this is red deferred-capture evidence owned by the maintainer’s post-program type-correction work, **not green coverage**. Convert the live cell into a third discriminating `#[ignore]`d capture naming that owner.

This **supersedes the prior “UNCOVERED CELL OWED” ruling**. Its detection evidence was valid, but detection did not prove publication; its satisfiability premise was falsified by the fresh public-boundary execution that ruling itself required. [prior ruling:16](out-of-tree working artifact; transcribed in this directory), [prior ruling:37](out-of-tree working artifact; transcribed in this directory)

This ratifies the axis disposition only; CM1 acceptance remains the maintainer’s act.

===VERTER-RECEIPT-BEGIN===
LANE: architect-owed-cell-red
RESULT: PASS
REVIEWED: cdca7f35a4edbe61b81b6d4043657b03fed9f22b
FINDINGS: none
===VERTER-RECEIPT-END===
