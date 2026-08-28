# CM1 acceptance repair — 2026-08-22

**[SCOPE CORRECTION, 2026-08-23, `block/ledger-subordinate-to-code`]:** every "Fixed on this
branch"/"this repair branch" claim below (§"Genuine review — dispatched, verdict `BLOCK`" and
§"Fixes applied on this repair branch") describes `crates/` work that is **NOT present on
`block/ledger-subordinate-to-code`** — this ledger block's own branch carries `docs/`/`scripts/`
changes only (see its commit history). That `crates/` work is duplicated, independent work now
owned by `block/binding-index-owner-and-eval`, not yet landed or reviewed. This record is kept
as-is below because it accurately documents review findings and disposition reasoning that
still hold; read every "this branch"/"this repair branch" reference below as the now-dropped
historical repair work, not as code currently on this ledger block's branch. See
`program-state.toml`'s CM1 row FOLLOW-ON note for the current, authoritative ownership record.

CM1's ledger row was marked `ACCEPTED` (`docs/arch/architecture-lock/ledger/
2026-08-22-cm1-closure-record.md`) on the strength of a review that was never actually
dispatched: all three review mandates recorded `PASS` at `reviewed_sha = 13eafb2ab` without
any independent reviewer ever examining that candidate. This record documents that: a genuine
review was run, it returned `BLOCK`, its findings were dispositioned (some fixed on this
branch, some recorded as open debt), and CM1's ledger review-mandate fields are corrected to
match what the evidence actually supports.

**Disposition history (read this before the rest of the record) — CORRECTED, round 3:** the
corrective code in this repair originally shipped with a ledger status reversion (`ACCEPTED` →
`REVIEW`, `364db87de`/`4f4ab6d39`). An intermediate draft of
`MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md` §3 claimed to supersede that STATUS
reversion and recorded CM1 staying `ACCEPTED` via a `REVIEW_MANDATE_MAINTAINER_OVERRIDE_
GRANDFATHER` in `scripts/validate-program-state.mjs`. On review, that was wrong: the
maintainer's verbatim ruling is conditional ("**if** the code is correct and landed"; §4 of
the ruling explicitly keeps review mandates untouched), and no maintainer ruling actually
authorized bypassing a genuine `BLOCK` review verdict — that clause was written by this block,
not dictated by the maintainer. The override grandfather has been REMOVED from the validator;
CM1's status stays `REVIEW`, exactly as `364db87de`/`4f4ab6d39` left it, and the ordinary
ACCEPTED-gate PASS requirement applies to CM1 like any other block. CM1's `conformance_review`/
`architecture_review`/`adversarial_review` fields remain `BLOCKING` (the true verdict) with
empty `*_reviewed_sha`, and CM1 cannot reach `ACCEPTED` until it earns a genuine `PASS` on all
three — which requires closing the open gaps listed below first.

## What was found wrong with the closure record (three P1s)

1. **No review verdict was ever issued against the recorded SHA.** The single-review-lane
   waiver (`MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §4) genuinely applies to CM1 — that part
   of the closure record is correct. But the waiver replaces three lanes with one; it does not
   waive the requirement that the one lane's `PASS` be a real verdict against the exact
   candidate. The closure record's own text admits Finding B has no tree-identical reviewed
   candidate and Finding C's proof rests on a different lineage (`47287d9dd`) that happens to
   share `13eafb2ab`'s tree — yet both mandates recorded `PASS` at `13eafb2ab` regardless.
2. **The `context_packet_digest` gap is not maintainer-waived.** The field is unconditionally
   required by the validator (`scripts/validate-program-state.mjs`) for every `EVIDENCE_BOUND`
   status. `MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md` §1 now waives it, narrowly, for
   exactly `{BV2, B5, CM1}`.
3. **The closure overstated scope and a behavioural claim.** D1 (nullable constructor-array
   element) is a required acceptance-matrix cell with no maintainer-ratified rescope. D3's
   claim that "all three [gaps] fail closed today" was false for the `Local` + `Absent`
   sub-case — it silently fell through to the caller's unannotated default instead.

## Genuine review — dispatched, verdict BLOCK

One independent codex review (`gpt-5.6-sol`, `model_reasoning_effort=xhigh`, read-only sandbox)
was dispatched against the exact cumulative candidate `13eafb2ab` (tree
`9f848e6cbb90f22df3ed9bac023b983163275302`), covering both Finding B (`0e5177931`) and Finding
C (`13eafb2ab`), per the single-lane waiver.

**Verdict: `BLOCK`.** Confirmed findings, most severe first:

- **D3's "fails closed" claim is false**: a `Local`-resolved constructor binding whose
  underlying declaration resolves `ResolvedTypeOutcome::Absent` returned `None` from
  `constructor_binding_source_position` (`component_meta.rs`, pre-fix), and the caller silently
  fell back to the unannotated default instead of failing closed. **Fixed on the historical
  repair branch (see the scope correction above — not present on `block/ledger-subordinate-
  to-code`; now owned by `block/binding-index-owner-and-eval`).**
- **A correctness bug, not previously disclosed: locally-bound `eval` misclassification.**
  `root_binding_index.rs`'s sloppy-eval collector classified a call by NAME alone
  (`callee.name == "eval"`) without checking whether the reference actually resolves to the
  unbound global `eval` versus a local declaration. **Fixed on the historical repair branch
  (see the scope correction above) — in two stages**
  (see "A second, independent review found the first fix unsound" below): the FIRST fix
  attempt was itself a rule violation, later caught by a second independent review pass.
- **The compat surface still encodes the pre-repair regression value.**
  `packages/component-meta/test/compat-gaps.test.ts` ("runtime defineProps object syntax
  preserves defaults without raw display authority") asserts `type: "unknown | undefined"` for
  `StringPropDefault.vue`'s `defineProps({ hello: { type: String, default: "Hello" } })` — the
  exact shape Finding C's fix targets. **Not fixed on this branch** — a real compat-surface gap
  requiring its own investigation, recorded as open debt below.
- **No producer-owned typed constructor-identity enum.** The charter (item 4, `CM1.md:110`)
  requires "one producer-owned typed enum," not a display-text parse or output-seam mapping.
  Constructor identity is still a raw `Arc<str>` spelling (`ConstructorBindingEntry.spelling`)
  resolved by string match at the fold site (`component_meta.rs`'s `primitive_of`), not a typed
  enum minted at the producer. Both Options and macro paths do share exactly ONE fold site, so
  the charter's *forbidden* outcome ("two independently-maintained string switches") is
  avoided, but the charter's *positive* requirement (a producer-owned typed enum) is not met.
  **Not fixed on this branch** — a larger change than this repair's bounded scope justifies as
  a drive-by; recorded as open debt below.
- **The acceptance matrix is not proven cell-by-cell for Finding C.** Most Finding-C cells
  (constructor arrays, required/optional/default combinations, native/compat agreement, all
  five invocation shapes) are asserted only at the analyzer/binding-classification level
  (`root_binding_index_tests.rs`), not through a public `get_component_meta`/compat end-to-end
  assertion of the actual published type. Finding B's cells are far better covered end-to-end
  (`exposed_binding_regression.rs`). **Not fixed on this branch** — recorded as open debt below.
- **The `Present → UnraisableSource` hard-error fixture question is more nuanced than a blanket
  "not satisfied."** `exposed_binding_regression.rs`'s discriminating tests for this failure
  mode exist and are real, but assert by `.expect(...)`-panic-message style rather than an
  exact-variant assertion (`matches!(err, ComponentMetaOutputFailure::UnraisableSource)`) — a
  precision gap, not an absence. **Not fixed on this branch** — recorded as open debt below.

## Fixes applied on the historical repair branch (not retained on this ledger block's branch — see the scope correction above)

1. **D3 — fail closed instead of silently falling through** (`component_meta.rs`,
   `constructor_binding_source_position`): the `ResolvedTypeOutcome::Absent` arm for a
   `Local`-resolved, uniquely-matched constructor binding now returns
   `SourcePosition::Failed(SemanticSourceFailure::UnrepresentableRequiredMemberValue)` instead
   of `None`ing through to the caller's unannotated default. New discriminating test:
   `constructor_local_absent_resolution_fails_closed`
   (`crates/verter_semantic/src/analysis/component_meta_tests.rs`).
2. **D1 — nullable constructor-array element, ADOPT-NOW per codex ruling**: `null` as a
   constructor-array element now resolves `ConstructorBindingOutcome::Global` (a literal
   keyword can never be locally shadowed) and folds to `PrimitiveName::Null` at the shared fold
   site, publishing `string | null` for `defineProps({ label: [String, null] })` instead of
   failing closed as `Indeterminate`. Verified against `@vue/runtime-core`'s own vendored
   runtime (`runtime-core.esm-bundler.js`): `getType(ctor)` returns `"null"` for
   `ctor === null`, and `assertType` for that expected type checks `value === null` — a real,
   documented nullable-constructor convention, not a guess. Tests:
   `nullable_constructor_array_element_resolves_global` (`root_binding_index_tests.rs`), new
   `constructor_array_nullable_publishes_primitive_union_with_null`
   (`component_meta_tests.rs`).
3. **The locally-bound `eval` misclassification.**

## A second, independent review found the first fix unsound

The first fix attempt (`root_binding_index.rs`) skipped sloppy-eval-scope poisoning whenever
the eval callee reference's `symbol_id().is_some()` — i.e. whenever the reference resolved to
ANY bound symbol. That is unsound: binding presence does not prove a non-intrinsic value.
`const eval = globalThis.eval; eval(...)` is still direct eval despite `eval` being a local
binding — the spec's direct-eval test is a value-identity check (is the resolved value
`SameValue` as the intrinsic `%eval%`), not an "is this name bound" check. A second,
independent review pass (same review lane, re-dispatched against the corrected candidate)
caught this and required the reconciliation invariant below, which the sibling
`verter-bindfix` block (fixing the same file for an unrelated reason) had already implemented
correctly:

> a fresh, unmutated, unredeclared local function named `eval` must not poison scope;
> unresolved, aliasable, mutated, or redeclared `eval` must remain indeterminate.

**Corrected fix:** the poisoning skip is now gated on
`SymbolFlags::Function && !scoping.symbol_is_mutated(symbol_id) && scoping
.symbol_redeclarations(symbol_id).is_empty()` — a function declaration's OWN value is
provably a fresh, non-intrinsic function object, but only as long as no other
declaration/assignment could still reach the binding with a different value. Every other
local binding kind, any mutated function binding, and any redeclared one falls through and is
still recorded — fail closed rather than guess. New discriminating tests:
`var_bound_eval_stays_indeterminate_not_provably_non_intrinsic`,
`eval_reassigned_after_function_declaration_stays_indeterminate`,
`eval_redeclared_via_initializer_stays_indeterminate`,
`aliased_global_eval_binding_stays_indeterminate_not_provably_non_intrinsic` (the concrete
`const eval = globalThis.eval` alias case named in the reconciliation invariant above, added
after round-3 review found the earlier `var eval = something` test did not actually cover it)
(`root_binding_index_tests.rs`), alongside the original
`locally_bound_eval_is_not_direct_eval_and_never_poisons_the_scope`.

**Verification scope is package-level, not the full workspace gate.** The commands originally
recorded here (`cargo nextest -p verter_semantic`, `cargo nextest -p verter_session --lib
component_meta/root_binding`, `cargo nextest -p verter_session --test main --
exposed_binding_regression`) all omit nextest's required `run` subcommand and exit 2 without
running anything — the pass counts against them were false. Re-run at `09a5b69f7` with valid
commands:

- `cargo nextest run -p verter_semantic` — **1679 passed**, 0 failed (not 1678; the prior count
  predated `aliased_global_eval_binding_stays_indeterminate_not_provably_non_intrinsic`, added at
  `root_binding_index_tests.rs:379`, and could not have covered it even if genuine). This run
  includes that test.
- `cargo nextest run -p verter_session -E '(test(component_meta) or test(root_binding)) and
  kind(lib)'` — 621 passed, 0 failed (matches the originally-claimed count).
- `cargo nextest run -p verter_session -E 'test(exposed_binding_regression)'` — 15 passed, 0
  failed (matches the originally-claimed count).
- `node --test scripts/validate-program-state.test.mjs` — 74 passed, 0 failed.
- `cargo fmt`/`clippy -p verter_semantic` clean.

Scope actually tested: `verter_semantic` plus the two selected `verter_session` filters above,
and the Node validator suite. This is NOT a full-workspace `node scripts/gate.mjs` run — this
repair does not run the canonical gate itself; that is the program orchestrator's responsibility
at landing, per this program's division of labor (implementers run targeted/affected-crate
tests, the program orchestrator runs the full gate before landing).

**Discrimination proof for the two tests this correction turns on** (fresh, run against
`09a5b69f7`, plant verified present/unique via `git diff` before trusting each red result, tree
restored to a clean `git diff`/`git status` after each):

- `aliased_global_eval_binding_stays_indeterminate_not_provably_non_intrinsic`
  (`root_binding_index_tests.rs`): with the predicate at `root_binding_index.rs` reverted to the
  old, unsound `reference.symbol_id().is_some()` form, `cargo nextest run -p verter_semantic -E
  'test(aliased_global_eval_binding_stays_indeterminate_not_provably_non_intrinsic)'` **FAILS**
  (`assertion left == right failed / left: [Global] / right: [Indeterminate]`). Restoring the
  corrected predicate (`SymbolFlags::Function && !symbol_is_mutated && symbol_redeclarations
  .is_empty()`), the same command **PASSES** (1 passed).
- "review mandates have no maintainer-override grandfather" (`validate-program-state.test.mjs:
  2416`): with a planted `if (id === "CM1") continue;` re-added inside the review-mandate loop in
  `scripts/validate-program-state.mjs`, `node --test --test-name-pattern="review mandates have no
  maintainer-override grandfather" scripts/validate-program-state.test.mjs` **FAILS** (CM1's three
  expected `BLOCKING` violation lines are silently absent; only XX9's fire). Removing the plant,
  the same command **PASSES** (1 passed, 0 failed).

## D1 disposition — codex ADOPT-NOW ruling

A dedicated codex consult (same model/effort) was dispatched to rule on D1's disposition per
CLAUDE.md's explicit-finding-disposition rule. Ruling: **`ADOPT-NOW`**, not `DEFER` —
nullable constructor arrays are already explicitly required charter scope (`FC-CM-001`), so
verifying Vue's real semantics is necessary acceptance evidence, not a scope deviation; the
original DEFER's own resolution gate ("before this block's final review") had already expired
with no DAG-bound follow-up.

## What remains open (NOT fixed by this repair, and why)

These are **not** silently absorbed as "fixed" — they require their own implementation track,
and CM1's ledger notes name them explicitly rather than claiming CM1's code is unqualifiedly
correct:

- The compat-surface regression (`compat-gaps.test.ts` still asserting the pre-repair
  `"unknown | undefined"` value for a shape Finding C's fix should cover).
- The producer-owned typed constructor-identity enum (charter item 4's positive requirement;
  currently a shared string-spelling fold, not a typed enum minted at the producer).
- Full acceptance-matrix, end-to-end (public `get_component_meta`/compat) coverage for Finding
  C — most cells are proven only at the binding-classification level today.
- An exact-variant assertion for the `Present → UnraisableSource` hard-error fixture (a real
  fixture already exists and is discriminating for the underlying defect; it does not yet pin
  the specific error variant).

**Disposition: route to a CM1 corrective follow-up track** — the owner and gate for these four
items are: a fresh implementer dispatch scoped to exactly these four items (owner), reviewed
under the normal three-mandate protocol and required to reach genuine `PASS` on all three
before CM1 can be marked `ACCEPTED` (gate) — the single-lane waiver was specific to the
original CM1/BV2/B5/J1 batch and is not re-invoked here. CM1 stays `REVIEW`, NOT `ACCEPTED`,
until that follow-up track closes these four items and earns a real `PASS`; there is no
maintainer override of the `BLOCK` verdict in force (see disposition history above, corrected).

## Ledger correction

CM1's `program-state.toml` row is corrected:
- `status`: `REVIEW` — no maintainer override of the review-mandate gate exists (corrected;
  see disposition history above). `MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md`'s
  operative scope is the `context_packet_digest` identity-bookkeeping gap only (§1); review
  mandates are explicitly untouched (§4).
- `conformance_review`/`architecture_review`/`adversarial_review`: `PASS` → `BLOCKING` (single
  lane, per the waiver — all three fields record the one lane's true verdict).
  `*_reviewed_sha` fields are cleared to `""` — the validator's universal review-verdict-
  identity-binding check requires a non-`PASS` mandate to carry no reviewed SHA at all; the
  fact that the review genuinely examined `13eafb2ab` is recorded here, in prose, not in the
  now-empty fields.
- `maintainer_decision`: `PENDING` — no maintainer acceptance has been recorded.
- `accepted_sha`/`accepted_tree`: cleared to `""` — CM1 is not accepted.

**Validator state (`--mode live`), disclosed not silenced:** one violation remains and is
the intended end state of this ledger block: the fixed-landing-order rehearsal reports a
real concurrent-landing conflict for CM1 (landing_order 3) against the pinned trunk
(`contracts/stacked-prs.md`, `MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md`).
The already-landed shortcut (MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md §2) only
fires when a block's candidate is an ancestor of the ledger's pinned
`repository.integration_head_sha`; that pin predates CM1's real landing (`13eafb2ab`),
so the shortcut does not fire and the replay stands. This block does not claim to
make that rehearsal accurate and does not silence the exit 1. J1 — the other
concurrently-active block, still `IN_PROGRESS` — must rebase onto current trunk for
the independently confirmed real conflict. Advancing the pin to the live tip was
tried and rejected: it would hide CM1's instance without making J1 landable, and is
the dropped live-tip shortcut. Same precedent as `364db87de`/`4f4ab6d39` on leaving
the violation disclosed rather than routing around it by pin manipulation.
- `notes`: corrected to name the true review verdict and the disposition history plainly.

## Independent verification performed in this repair (not taken on faith)

- D3's code claim was independently traced in `component_meta.rs` before the review was even
  dispatched (matches the review's later, independent confirmation).
- The locally-bound-`eval` bug was independently traced in `root_binding_index.rs` after the
  review flagged it; the FIRST fix was itself independently found unsound by a second review
  pass, tracing the exact aliasing counter-example before the corrected fix landed.
- The compat-surface regression claim was independently confirmed by reading the actual checked-
  in fixture (`StringPropDefault.vue`) and test assertion (`compat-gaps.test.ts`) at `13eafb2ab`.
- Vue's actual nullable-constructor runtime semantics were independently verified against the
  vendored `@vue/runtime-core` source, not asserted from memory or the review's say-so.
