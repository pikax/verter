# CM1 closure record — 2026-08-22

CM1's ledger row was `IN_PROGRESS` with all three review mandates `PENDING`
while its work was already on trunk. This record closes that gap: it
establishes CM1's real delivered scope from git evidence, resolves the two
honesty problems the closure required, and reports the resulting ledger and
validator state. It supersedes no ratified maintainer ruling — it applies one
already granted (`MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §4, which names
CM1 by name) to CM1's row for the first time.

## What CM1 actually delivered

CM1's charter (`docs/arch/refactor/rev11/charters/CM1.md`, digest
`1f18b0663d...`) owns seven items across two findings plus a hard-error
fixture. Reading the two commits the maintainer-facing task named
(`0e5177931`, `13eafb2ab`) with `git show --stat`/`git show` rather than
trusting their one-line summaries:

- **`0e5177931`** (22 files, +1473/-97) — far larger than its own short
  summary suggests. It touches `eval_env.rs` (142 lines: the exhaustive
  `PreparedValueDecl` admission gate — `prepared_value_decl_has_demandable_type`
  checks annotation, signatures, object shape, enum members, and class kind,
  matching the charter's Structural Confinement text almost verbatim),
  `locator_deref.rs` (80 lines: the empty-path dereference's signature
  fallback — the fix for the `Present -> UnraisableSource` hard-error
  fixture), `prepared_decl.rs`/`resolver_core/prepared_decl*.rs` (the
  call-initializer owner/preparation handoff), `component_meta.rs` (aliased
  binding resolution by referenced identity, `Failed`-vs-`Absent`
  preservation), a 970-line `exposed_binding_regression.rs` (the renamed
  `findbc_regression.rs`, covering plain function, ref/computed, mixed
  full-API, imported-type-position, enum, aliased, and non-identifier-value
  bindings; cold/warm, `Promise.all`-equivalent concurrent, batch, and
  overlay/session invocation; and two dedicated `UnraisableSource`
  non-silent-failure tests), and a compat-surface TS test
  (`exposed-binding-compat.test.ts`). This is Finding B (items 1-3) and the
  hard-error fixture (item 5), substantially complete.
- **`13eafb2ab`** (41 files, +3136/-360 — corrected from an earlier draft's
  miscounted +2874/-360, verified with `git show --shortstat`) — Finding C.
  Deletes the structurally-unsound AST shadow scanner (confirmed dropped:
  `0e5177931`'s own message states `defineProps` runtime-constructor
  behaviour is "byte-identical to before this change") and replaces it with
  `RootBindingIndex`, an `oxc_semantic`-binder-backed owner-aware resolver
  (`Global`/`Local`/`Indeterminate`, no silent fourth state) shared by both
  the Options-API and macro constructor-extraction paths through one call
  site (`eval_env.rs`'s `constructor_binding_keys` wiring). The commit
  message says "eighteen tests"; `root_binding_index_tests.rs` actually
  contains 22 `#[test]` functions (`grep -c` verified), plus more in
  `component_meta_tests.rs` — the author's own count was an
  underestimate, not independently re-verified before this closure's first
  draft repeated it. This is Finding C (item 4), **with three explicitly
  disclosed deferrals** — see "Disclosed scope residuals" below.

**What I did not find independent evidence closes**: item 2's residual
divergence (the explicitly-annotated call-initializer edge case the charter
flags as "unresolved" at authoring time) is addressed by the same
`prepared_decl.rs`/handoff changes in `0e5177931`, but I have not re-derived
that specific edge case from the diff line-by-line — I am relying on the
regression suite's `expose_admits_explicitly_typed_call_initializer_binding`
test as the discriminating proof, not a manual re-trace. This is recorded as
the honest limit of a ledger-orchestrator's review (no test run, no code
review beyond reading diffs), not asserted as independently re-verified.

## Disclosed scope residuals (found on review, not by the first draft)

`evidence/CM1/binding-index-deferrals.md` records three findings dispositioned
under CLAUDE.md's explicit-finding-disposition rule, each real and each
already fail-closed rather than silently wrong:

- **D1 — nullable constructor-array element** (`defineProps({ label: [String,
  null] })`). The charter's acceptance matrix names "nullable" as a required
  runtime-prop form. This is DEFERRED, not implemented: a `null` array
  element resolves to `ConstructorBindingOutcome::Indeterminate` and fails
  the position closed (`UnrepresentableRequiredMemberValue`) rather than
  guessing Vue's nullable-constructor semantics or fabricating a type. The
  disposition's own reasoning (never publish a guessed type; the underlying
  question is real scope, not something to reject) is consistent with the
  charter's own forbidden-outcomes list, but it means this one matrix cell is
  not closed as originally specified.
- **D2 — `defineModel` runtime-constructor gating.** Not gated through
  `RootBindingIndex` at all; `extract_define_model_type` has never extracted
  the runtime-argument form (`defineModel({ type: String })`) on trunk,
  independent of this block, so there is nothing for CM1 to gate. Out of
  CM1's charter scope (which discusses `defineProps` shorthand/expanded
  runtime forms, not `defineModel`), recorded here because a reviewer found
  it and it belongs on the record.
- **D3 — session-side `Local` resolution is fail-safe, not fail-complete**
  for a hoisted-nested-var shadow or a cross-owner same-name collision in the
  name-only-keyed `ExpandedComponentTypes.bindings` lane. Both known gaps
  fail closed (verified by
  `constructor_local_ambiguous_cross_owner_name_collision_fails_closed` and
  `constructor_array_mixing_local_with_anything_else_fails_closed`), never
  silently wrong.

None of the three is silently dropped or hidden — each has a named owner, a
resolution gate, and a reason DEFER was chosen over ADOPT-NOW/REJECT, per
CLAUDE.md's process. But this means an earlier draft of this record's claim
that Finding C is complete was overstated by omission: Finding C is complete
**for the identified/non-deferred cases**, with three named residuals still
open. I am accepting CM1 with these disclosed, not silently absorbing them
into "complete."

## Honesty decision (a): `context_packet_digest`

**No context packet exists, and none is fabricated.** CM1 was dispatched
directly off the maintainer's beta.4 regression-intake directive
(`authority-registry.toml`, `[[authorization]] block = "CM1"`,
ratified 2026-08-20) — the same dispatch shape as BV2. Neither ever had a
`context-packet.md` produced before implementation started.

One wrinkle CM1 has that BV2/B5 do not: a file literally named
`context-packet.md` existed transiently on `block/cm1`
(`e9cdfea93 docs(core): add CM1 implementation evidence packet`,
2026-08-21T08:37) at `evidence/CM1/context-packet.md`. I checked whether this
could honestly fill the field. Two independent reasons it cannot:

1. It never landed. `0e5177931`'s squash of `block/cm1` does not include it
   (`git show 0e5177931 --stat` — not present); it is unreachable from
   `program/architecture-lock` today (`git cat-file -e HEAD:.../CM1/context-
   packet.md` fails).
2. Even if it had landed, its own commit message says "implementation
   evidence packet," not a dispatch input — it was authored eleven hours
   into implementation, describing work already done, not a pre-dispatch
   context binding the implementer to a scope before writing code. Filling
   `context_packet_digest` with its digest would misrepresent an
   implementation-time artifact as an input artifact, which is the exact
   dishonesty the field's convention (A0/A2/A3's real `context-packet.md`
   files) exists to prevent.

So the field is recorded empty, with an inline comment stating why, in the
same form BV2's and B5's rows already use. **This closes the loop the task
asked for**: BV2 and B5 already carry this exact pattern (BV2's row
comment, added by an earlier session, and B5's row comment, which already
cites BV2's). CM1's row now makes it three consistent instances of one
documented, maintainer-visible, deliberately-open gap rather than three
independent ad hoc decisions. I did not edit BV2's or B5's rows — they
already conform; the pattern check confirmed rather than required a change.

## Honesty decision (b): the rehearsal conflict

Diagnosed by hand-reproducing the exact `git merge-tree` call the validator
issued (documented in full, with commands and output, in
`evidence/CM1/landing-equivalence.md`). Findings:

- The validator's violation message cites `contracts/stacked-prs.md` /
  `MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md` as the rehearsal's
  *governing authority* (a static string in the check's own source), not the
  actual conflicting file. The real conflict is in
  `docs/arch/architecture-lock/ledger/authority-registry.toml`.
- It is a pure adjacency collision: `block/cm1`'s stale base
  (`53d6c3157`, 2026-08-21) predates the trunk commit that appended CM1's
  own dispatch-authorization rows to that file (71 lines, already present on
  `block/cm1` too via its own snapshot) and the cumulative side independently
  appends BV2's/B5's acceptance rows (36 lines) at the same end-of-file
  point. Neither side disagrees about CM1's content — `git diff` between the
  two candidate identities and their shared base shows two unrelated
  additions git cannot auto-interleave.
- **The fix is re-identifying CM1, not suppressing the check.** The recorded
  `implementation_candidate_sha` never represented what landed (see below),
  so the rehearsal was replaying a fictional delta against a real cumulative
  state. `base_sha` is now `eadec2dc0` (the real trunk parent immediately
  before CM1's first landed commit) and `candidate_sha`/`accepted_sha` are
  now `13eafb2ab` (the real final landed state) — a genuine, already-merged,
  zero-op delta. Separately, and independently sufficient on its own: CM1
  leaving the `IN_PROGRESS ∪ REVIEW ∪ ACCEPTANCE_RECOMMENDED` active set by
  becoming `ACCEPTED` removes it from the rehearsal's scope entirely.

I did not adjust `landing_order` or record a rehearsal exemption — the root
cause was a wrong identity, not a legitimately-unrehearsable structural
case, so re-identifying it is the true fix, not a workaround.

## Candidate identity: two disjoint lineages, not one

`implementation_candidate_sha` (`47e85159...`, `block/cm1`'s tip) is
retained on the row as a historical WIP-dispatch record (same convention as
BV2's row) but is **not** CM1's landed identity. Verified directly:

- `47e8515` is not an ancestor of `0e5177931` (squash, not rebase/fast-forward).
- `13eafb2ab` is not on `block/cm1` at all (neither ancestor nor descendant of
  its tip) — it was built and reviewed on a separate lineage
  (`bf61e676b`..`47287d9dd`, including three rounds of codex xhigh adversarial
  design review recorded in `evidence/CM1/binding-index-design*.md`, plus a
  post-implementation adversarial pass, `2f0379039`, that found and fixed
  three real bugs) and squashed directly onto trunk with parent `120eede71`
  (a J1 commit).

`candidate_sha`/`candidate_tree`/`accepted_sha`/`accepted_tree` are therefore
all set to `13eafb2ab` — the one real commit at which CM1's full delivered
surface exists on trunk. There is no candidate/accepted divergence to prove
(nothing landed after `13eafb2ab` that touches CM1's files), so unlike BV2
there is no post-squash regression-fix story to tell.

## Review mandates

All three (`conformance_review`, `architecture_review`, `adversarial_review`)
are set `PASS`, reviewed SHA `13eafb2ab`, under the single-independent-review-
lane waiver `MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §4 already grants "for
BV2, B5, J1 and CM1 only." That ruling extended the waiver's eligibility to
CM1 by name but never applied it — §§1-3 dispose of BV2/B5/J1 individually;
there is no §4a for CM1. This record is that application, not a new grant of
authority. The evidence for a real, iterated-to-clean single lane: the
`block/cm1` lineage's own history shows successive review-driven fix commits
that correctly identified and removed an unsound mechanism rather than
patching around it; the `RootBindingIndex` lineage shows three explicit codex
xhigh design-review rounds (v1 "not ready", v2 "not ready", v3 accepted) and
one adversarial implementation pass that found and fixed three real bugs. I
did not run the gate or any test — this is ledger orchestration, not
verification — so this PASS rests on reading the review trail and the
regression suite's coverage, not on independently re-executing anything.

**Precision of `reviewed_sha = 13eafb2ab` — asymmetric, disclosed.** For
Finding C, this is exact: `47287d9dd` (the actual tip of the reviewed
`RootBindingIndex` lineage, after `2f0379039`'s adversarial fixes) has a tree
byte-identical to `13eafb2ab`'s (`9f848e6c...`, verified) — the same
squash-preserves-content guarantee B5's candidate/accepted split rests on.
For Finding B, there is no equivalent hash proof: `0e5177931`'s tree matches
neither `block/cm1`'s tip nor its post-revert state (all three trees
verified distinct in landing-equivalence.md). The ledger schema binds one
`reviewed_sha` per block, so `13eafb2ab` is recorded for both; that binding
is exact for Finding C and is the closest honest anchor available for
Finding B, not a claim that `13eafb2ab` is the literal object a reviewer
looked at for the expose-binding/admission-gate work — reviewed on
`block/cm1` through iterative, commit-history-evidenced rounds, not against
a single tree-identical final SHA.

## Violation delta

Before (baseline, `node scripts/validate-program-state.mjs --mode live`):

```
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: block CM1 (landing_order 3) does not land cleanly onto the cumulative result of every prior block in the fixed landing order — ...
FAIL: 3 violation(s)
```

After:

```
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s)
```

Count unchanged (3 → 3); **kind changed**: the structural, DAG-blocking
rehearsal violation is gone, replaced by a third instance of the same
already-known, already-precedented, non-blocking documentation gap the
maintainer has already accepted twice. This is not a wash — the rehearsal
violation was actively preventing C1 from becoming dependency-eligible; the
context-packet gap is a recorded, honest absence of an artifact that never
existed, blocking nothing.

## C1 / D1

C1's predecessors (`A6`, `B1`, `B2`, `CM1`) are now all `ACCEPTED`. C1 is
dependency-eligible again (the DAG gate this closure exists to clear). C1
itself remains `LOCKED` in the ledger pending its own charter ratification
and dispatch — that is separate, later work, out of this closure's scope.
D1's predecessors include C1, so D1 stays blocked behind C1's own dispatch
regardless of this closure.

## Open gaps, left open

- `context_packet_digest` empty for BV2, B5, and now CM1 — recorded, not
  hidden, per the maintainer-ratified pattern.
- The explicitly-annotated call-initializer residual (Finding B item 2) is
  covered by a named regression test but not independently re-derived from
  the diff by this closure.
- D1/D2/D3 (nullable constructor-array elements, `defineModel` runtime-form
  gating, hoisted-nested-var/cross-owner `Local` resolution completeness) —
  see "Disclosed scope residuals" above. All three fail closed today; none
  is silently wrong; none has a resolution gate bound to a specific
  follow-up block in the DAG.
- The `reviewed_sha = 13eafb2ab` binding on all three review mandates is
  tree-exact for Finding C and narrative-only (no matching tree) for Finding
  B — see "Precision of `reviewed_sha`" above.
- No gate or test run was performed as part of this closure (ledger
  orchestration scope only, per this task's own constraints).

## Independent review

One codex review round (read-only, unprimed, question-shaped prompt) was
dispatched against this record before the fixes above were applied. Real
findings applied: the diff-stat and test-count numbers were corrected against
`git show --shortstat`/`grep -c`; the D1/D2/D3 scope residuals (found by
review, not by the first draft) are now disclosed above instead of Finding C
being characterized as unqualified "complete"; the `reviewed_sha` precision
gap between Finding B and Finding C is now made explicit in both this record
and `landing-equivalence.md`. Not applied as a status change: the reviewer's
observation that becoming `ACCEPTED` both re-identifies the candidate and
removes CM1 from the rehearsal's active set is accurate and already stated
as two independently-sufficient reasons in this record's rehearsal section —
recorded as a confirmation, not a contradiction requiring a fix.
