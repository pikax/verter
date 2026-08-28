# Tracked debt — FC-B4-001: `generated_chunk` call-site migration to `assembly::compose`

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition"; codex ruling on the two
B4 findings).

## What happened

B4 built `crates/verter_compiler/src/assembly/{fragment,compose,publish}.rs` as the typed,
framework-neutral, evidence-backed replacement for `framework_common::generated_chunk`'s
`compose_generated_chunk` (EM-003). `Fragment::validate()` requires a fragment's own `code`
to independently parse under its declared `SyntacticContract` before
`compose::splice_into_hole` will accept it.

The 3 live production call sites of `generated_chunk::compose_generated_chunk`, all in
`crates/verter_compiler/src/framework_common/vue_bridge.rs`, were NOT migrated onto
`assembly::splice_into_hole` in this pass. `generated_chunk.rs` itself, and all 3 call sites,
are unchanged.

## Ruling reference

Codex ruling on the B4 findings (Finding 2), quoted verbatim: *"Migration is demonstrably
unsafe under the current contracts... The runtime fragment at the ~1083 site is deliberately
inline and begins with a function-body-only `return`... The IDE producer emits JSX children
and wraps only multiple roots; a single text child becomes `{"..."}`, not a guaranteed
standalone expression... The ~428 route is presently precluded by its `want_ide` projection
guards... Follow-up owner: B4 / `verter_compiler::assembly`, jointly with the Vue chunk
producers... Resolution condition: close only after each producer declares and tests its
actual contextual grammar — likely new function-body and JSX-children contracts, or producer
normalization into existing standalone contracts — then migrate each reachable site with
parse-validation and assembled-output parity coverage."*

## Owner

B4 / `verter_compiler::assembly`, jointly with the Vue chunk producers (BV1) — per the ruling
reference above.

## Acceptance ID

No existing acceptance id in `capability-matrix.tsv` (or elsewhere in the framework-
conformance evidence tree) covers this call-site migration — it is an internal assembly-
engine adoption, not a framework-option/capability row. Minting **`FC-B4-001`** as this debt
row's own acceptance id.

## Call sites — current status

1. **`vue_bridge.rs` ~428** (block-content IDE composition, inside `compile_bundle`'s
   `opts.want_ide` branch). Per the ruling, this route is **presently precluded by its
   `want_ide` projection guards** — i.e. not currently reachable under today's production
   projection configuration. Recorded precisely as *unreachable today*, not as "migrated" or
   "safe by default" — a future change to those guards could make it reachable again, at
   which point it needs the same grammar-contract work as the other two sites before
   migrating.
2. **`vue_bridge.rs` ~1083** (inline-template runtime composition, `supplied_inline_template`
   branch). Reachable in production. Per the ruling, the spliced runtime fragment is
   **deliberately inline and begins with a function-body-only `return`** — not a standalone
   `Expression`/`StatementList`/`Declaration`/`CompleteModule` under
   `assembly::SyntacticContract` as currently defined. Unmigrated.
3. **`vue_bridge.rs` ~1155** (block-content-supplied IDE composition, `opts.want_ide` branch
   inside the template-recompile path). Reachable in production. Per the ruling, **the IDE
   producer emits JSX children and wraps only multiple roots; a single text child becomes
   `{"..."}`, not a guaranteed standalone expression** — same class of contract gap as site 2,
   different producer. Unmigrated.

All 3 sites continue to call `generated_chunk::compose_generated_chunk` exactly as before
B4's change; `generated_chunk.rs` is not deleted.

## Resolution gate

Concrete, per the ruling text — close only after, for EACH of the 3 sites above:

1. Its producing Vue chunk producer (BV1) declares the fragment's actual contextual grammar —
   e.g. a new `assembly::SyntacticContract::FunctionBody` variant for the ~1083 inline-return
   shape, and/or a new `JsxChildren` (or equivalent) variant for the ~1155 IDE-children shape
   — OR the producer is changed to normalize its output into one of the EXISTING standalone
   contracts (`Expression`/`StatementList`/`Declaration`/`CompleteModule`) before handing it
   to assembly.
2. `Fragment::validate()` (or an equivalent per-contract check) is proven against that
   declared grammar with a discriminating parse-validation test — positive and negative, per
   the pattern already established in
   `crates/verter_compiler/tests/cases/assembly/fragment_parse_contract.rs`.
3. Assembled-output parity coverage exists proving the migrated call site produces
   byte-identical (or behaviorally-equivalent per this program's Compiled-Output Conformance
   rule) output to the pre-migration `generated_chunk`-based path, for real Vue fixtures
   exercising that call site.
4. Each site is migrated individually once 1-3 hold for it — partial closure (some sites
   migrated, not all) keeps this row open with an updated table, not a premature close. Site
   1 additionally needs its `want_ide` guard to actually become reachable before migration is
   even meaningful to attempt; until then it stays recorded as unreachable, not closed.

No code changes accompany this record — `generated_chunk.rs` and its 3 call sites are
unchanged from before this finding.
