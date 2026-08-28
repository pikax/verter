# Architecture challenge reattestation 2 — AMD-005 framework compiler conformance rescope

## Exact candidate binding

- Previous candidate commit: `ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`
- Previous candidate tree: `1ff1f83d8e994b6f1169b0b209c9f557c23f4728`
- Reattested candidate commit: `7442bb9060b7faa0720e528d3f96ee1df1abff95`
- Reattested candidate tree: `69502487b55f87eb7c0c009876865b64397da660`
- Branch: `work/framework-conformance-rescope`
- Read-only implementation checkout spot-checked at commit:
  `b3249d13d07806a14a4307954dfcc459cf7301ac`

This is an impact-bounded reattestation of the two blocking findings in
[`architecture-challenge.md`](architecture-challenge.md), not a fresh review of the
full package. The reviewed delta is exactly `git diff
ce1d0e4688af1b5bd548b6b68286632cc0f7ede8
7442bb9060b7faa0720e528d3f96ee1df1abff95`.

## Reattestation

1. **BV1→C3 defaults-object boundary — genuinely resolved.**
   `charters/C3.md:12-20` now restricts `TypedSubject.position` to exactly
   `TypeArgument`, so every C3 subject is a canonical type-argument locator. The
   demand vocabulary at `charters/C3.md:22-28` has exactly one variant,
   `MacroPayload`; the former `PropsWithDefaults`, second defaults subject, effective
   outer-macro index, and two-subject demand identity/order are gone. For a typed
   `defineProps` wrapped by `withDefaults`, only the inner props type argument is
   demanded. `charters/C3.md:26-36` explicitly leaves the wrapper association,
   defaults object, eligibility, application, and merge in BV1 and forbids their
   lowering to `TypeExpr`, entry into a C3 demand, or C3-owned merge.

   This is executable on the retained architecture. BV1 already owns source-local
   macro behavior and exposes only imported project-data demands
   (`charters/BV1.md:22-27`), while the compiler contract keeps runtime object/array
   facts and `withDefaults` syntax/merge parser/compiler-owned
   (`.claude/skills/compiler-codegen/SKILL.md:217-244`). The canonical locator path
   raises Vue `TypeArgument` payloads through the macro hot-mirror/project dispatch
   (`crates/verter_session/src/project_semantic_dispatch/semantic_source.rs:470-521`),
   whereas `ObjectArgument` remains deliberately unroutable
   (`crates/verter_session/src/decl_body_memo/locator_deref.rs:89-98` and `:225-230`).
   The revised protocol no longer asks C3 to traverse that unroutable arm and creates
   no second object-expression-to-`TypeExpr` substrate.

2. **Emitter/mapping disposition completeness — genuinely resolved for every owner
   named by the finding.** The ledger adds four distinct, non-overlapping rows at
   `emitter-mapping-dispositions.tsv:42-45`, each with an allowed disposition, named
   acceptance owners, and an owner-specific final state:

   - EM-041 covers `svelte/runtime/expr_rewrite/*` as `Converge`, owned by `BS1+B4`.
     In the read-only main checkout, `expr_rewrite/mod.rs:1-17` identifies the
     emission-grade, scope-aware rewriter; `:46-63` carries expression-local mappings;
     and `:66-135` constructs `CodeTransform` output and source ranges. The row
     correctly keeps Svelte rewrite semantics in BS1 and moves final source-space
     composition authority to B4.
   - EM-042 covers `svelte/runtime/client_event.rs` as `Converge`, owned by `BS1+B4`.
     The file identifies its separate emission responsibility at `:1-14`, emits mapped
     event/delegated calls at `:28-83`, and emits the delegate epilogue at `:110-119`.
     Its disposition preserves Svelte event topology under BS1 and routes fragments
     and maps through B4.
   - EM-043 covers `svelte/runtime/client_effect.rs` as `Converge`, owned by `BS1+B4`.
     The file identifies the shared effect emitter at `:1-8`, retains mapped
     dependencies at `:40-47`, and emits mapped effect bodies/runtime calls at
     `:90-104` and `:121-173`. Its split between BS1 effect semantics and B4 final
     placement/map/publication ownership matches the code.
   - EM-044 covers `compile/helpers.rs::empty_sfc_script_block` as `Converge`, owned by
     `BV1+B4`. The helper is a live synthetic Vue runtime-module emitter at `:13-52`,
     including the `defineComponent` shell and exported component. The row keeps empty
     component semantics/topology in BV1 and gives B4 final fragment assembly and
     atomic publication, so this path is no longer hidden behind EM-008's
     `compile/mod.rs` entry.

   The ledger now has 44 unique six-column rows with non-empty IDs, dispositions, and
   acceptance owners. EM-041 through EM-044 do not overlap the exact path sets in
   EM-026, EM-039, or EM-008.

## Bounded sanity pass

The remaining changed charter, amendment, README, evidence, historical review, and
validator bytes introduce no new blocking architecture issue. BF1's change only
records the accepted B1 predecessor; the amendment/README/evidence updates consistently
rebase that state and preserve the existing framework ownership, atomic-publication,
and unchanged DAG architecture. The validator's explicit pre-review/post-review modes
check attachment identity without treating a closed blocking verdict as acceptance.
Historical reports remain bound to their named older objects and do not redefine the
revised normative C3 protocol.

Focused checks passed:

- `git diff --check` for the exact previous-to-new candidate delta;
- `node --check` for `validate-package.mjs`;
- package validation in post-review mode with 22,718 non-zero assertions;
- both program-state validators against all 56 blocks; and
- the independent 44-row emitter-ledger structural check above.

## Verdict

PASS — all findings resolved, no new blocking issue introduced, bound to commit `7442bb9060b7faa0720e528d3f96ee1df1abff95`.
