# AMD-013 — Validator machinery for the concurrent-implementation ceiling

**Status:** RATIFIED WITH CORRECTIONS, 2026-08-21. The codex architect ratified
this amendment subject to two corrections (§11); both were applied in this
revision before landing. Two further post-landing corrections — round 6 (the
integration-trunk equality-pin self-reference defect the landing commit
itself exposed) and round 7 (the SAME defect one level down, in the
per-block `implementation_candidate_sha` pin) — are recorded under §11's
"Round 6 correction" and "Round 7 correction" subsections; the amendment
stays RATIFIED.

**Prepared against:** local `feat/concurrent-blocks` commit
`cd15a31b7a3c087dcca67f105434a823e49c55f1`, tree
`8ce5645112d5a8306d2131734f74ff82d2dfb6be`.
Every `file:line` citation below was read directly on that tree.

**Revision history:**

- v1 (commit `91375a6252967da90bdd2a2dd39c5b71ce83e009`) — REJECTED. Its
  certification model, disjointness rehearsal, and `candidate_sha` overload
  did not survive review.
- v2 (commit `2c0d462550a2274c8d79ac17e693e80e9ec07aa6`, reworked at
  `405a6bfd5580299ab2e87a371829c8020eeb4a3d`) — REJECTED a second time. Six
  findings: (A) the cumulative rehearsal modeled a MERGE commit
  (`git merge-tree` + two-parent `git commit-tree`), but this program lands
  by rebase-or-squash-then-fast-forward, never a merge — the rehearsal
  proved the wrong operation. (B) the numeric ceiling counted only
  `IN_PROGRESS`, so 5 IN_PROGRESS + 1 ACCEPTANCE_RECOMMENDED (6 concurrently
  active blocks) was silently legal against a ruling that says "up to 5
  concurrent blocks/trains". (C) the rehearsal sampled an ambient
  `git rev-parse HEAD` instead of a ledger-pinned, revalidated trunk
  identity. (D) `base_sha` was ancestry-checked and then ignored — the
  rehearsal's own merge-base was whatever git's ancestry search found, not
  the declared base. (E) `landing_order` was cross-validated against DAG
  predecessor order but not stack-layer order, and
  `implementation_candidate_sha` was never proven to be the block's actual
  current WIP tip. (F) no mutation covered wrong-HEAD trunk, declared-base-
  vs-auto-merge-base semantics, or the five-plus-one ceiling loophole. v3
  (commit `405a6bfd5580299ab2e87a371829c8020eeb4a3d`, answered again at
  `463c52fe425ac08d6a6657129df630007a2123ae`) answered all six in place.
- Round 3 review (commit `3e94742d42853ddb6d4634ce7740b669c9249ea9`) — the
  design was ACCEPTED (the replay model correctly models a squash-then-fast-
  forward landing; the ceiling conservatively satisfies the ruling). Three
  implementation defects survived, none warranting a full rework: (G) the
  trunk-pin's scoping to `active.length > 1` was justified by comparing the
  pin against checkout `HEAD` — but `HEAD` names only where THIS worktree
  happens to be checked out, which routinely differs from the ledger-
  declared trunk branch (that draft's own tree was a live example: checkout
  `HEAD` was `feat/concurrent-blocks`, several commits ahead of the `main`
  the ledger correctly pins). Comparing against `HEAD` conflated "worktree is
  elsewhere" with "trunk went stale"; conditionally skipping the check
  papered over comparing against the wrong ref rather than exempting a
  genuine staleness case. The pin now resolves and revalidates against the
  LIVE TIP OF `refs/heads/<repository.branch>` — the ledger's own explicit,
  named trunk ref — and runs UNCONDITIONALLY, on every live-mode validation,
  because the correct oracle does not drift with checkout position. (H)
  `implementation_candidate_sha` was a declared-but-unverifiable trust
  boundary (§8): nothing tied it to the block's actual live WIP tip. A new
  per-block field, `implementation_ref`, names a real, resolvable git ref;
  the rehearsal now REQUIRES that ref's live resolved tip to equal the pin
  exactly, closing the boundary. (I) three discriminating mutations/tests
  were missing and are now added — see §9.
- Round 4 review (this revision) — a NARROW rejection: the replay model,
  the ceiling, the trunk rehearsal pin, `repository.branch` validation, and
  all three round-3 mutations were reconfirmed sound. Four implementation
  defects, none warranting a full rework:
  - **FIX 1 — `implementation_ref` was never proven to be an actual ref.**
    `REF_NAME_RE` (a shape check) and `git rev-parse --verify` both accept a
    raw 40-char object id and the literal `HEAD` pseudoref — either
    trivially "resolves" (to itself, or to wherever this worktree happens to
    be checked out) no matter how stale `implementation_candidate_sha`
    really is, so the stale-pin false-green round 3 was asked to close was
    still reachable: declare `implementation_ref` equal to
    `implementation_candidate_sha` itself and the binding "verifies" by
    construction. `checkImplementationRefBinding` now explicitly rejects a
    raw object id or the literal `HEAD` BEFORE any git call, then resolves
    the ref via `git rev-parse --symbolic-full-name` and REQUIRES the result
    to start with `refs/heads/` — a real branch, never a tag or any other
    rev-parse-able spec.
  - **FIX 2 — the binding was scoped away in the state the ledger is
    normally in.** `verifyConcurrentLandingSafety` returned immediately when
    fewer than two blocks were concurrently active, so the ordinary,
    overwhelmingly common single-`IN_PROGRESS` ledger (the live ledger's own
    current shape) validated NEITHER `implementation_ref` nor
    `implementation_candidate_sha` at all — the same conditional-scoping
    mistake Finding G made for the trunk pin. `verifyImplementationRefFields`
    now runs `checkImplementationRefBinding` over EVERY `IN_PROGRESS` block,
    UNCONDITIONALLY, before `verifyConcurrentLandingSafety` is even called;
    that function reuses the precomputed result rather than re-checking (and
    duplicating messages) when `active.length > 1`. Both fields are now
    MANDATORY for every `IN_PROGRESS` block in live mode, not merely
    validated-if-present — see §9 for what this reveals against the real
    live ledger.
  - **FIX 3 — the checkout-`HEAD` oracle survived in a second place.**
    `verifyLiveGitIdentities`'s `accepted_sha` reachability/landing check
    (the A5 dangling-commit check) independently called
    `git rev-parse HEAD` to establish "the repository tip" — the exact same
    defect class as Finding G, at a different call site the round-3 fix
    never touched. It now consumes the SAME `resolvePinnedTrunk` result
    `verifyConcurrentLandingSafety` uses (resolved once, in `main()`, before
    either function runs) — `refs/heads/<repository.branch>`'s live tip,
    never checkout `HEAD`. A repo-wide sweep for any other `git rev-parse
    HEAD` / bare `"HEAD"` oracle found no third instance.
  - **FIX 4 — two missing discriminators, now added (§9):** a raw object id
    and the literal `HEAD` value for `implementation_ref` are each proven to
    fail (plus a real, tip-matching TAG, proving the check is "a real
    branch," not merely "resolves"); and a SOLE `IN_PROGRESS` block (no
    other block concurrently active) is proven to still be checked, so a
    future re-introduction of the FIX 2 scoping gate reddens. Both
    discriminators are additionally EMPIRICALLY proven by mutation (not
    merely asserted): a scratch copy of the validator with each fix's own
    guard neutralized is run against the same fixture and observed to
    WRONGLY pass, then the real, unmutated validator is run against the
    identical fixture and observed to correctly fail.
  §11 remains empty; a fix round is not a ratification decision.
- Round 5 review (this revision) — round 4's FIX 3 correctly stopped
  `resolvePinnedTrunk` (and `verifyLiveGitIdentities`'s `accepted_sha`
  reachability check) from sampling checkout `HEAD`, but it re-pointed both
  at `repository.branch`/`repository.head_sha` — and that pair is the
  IMMUTABLE A0 entry-lock checkout identity (`contracts/baseline-lock.md`
  §2; `A0` notes; `baseline-lock.md:5` distinguishes the entry checkout from
  later implementation lineage), never the moving operational trunk every
  landing/rehearsal actually replays against. That is exactly why round 4's
  own §9 found 21 "unreachable" `accepted_sha` violations against the live
  ledger: every post-entry `ACCEPTED` block lands on this program's
  long-lived integration branch (`program/architecture-lock`), which
  advances with every acceptance, while `repository.branch`/`head_sha`
  correctly never move past the one A0 checkout — the corrected oracle was
  aimed at the wrong ref, not merely a stale one. **Exposure check on the
  live ledger, run before any fix**, established this was a false-accept
  surface, not a false-reject one: of the 21 blocks round 4 flagged,
  21/21 are reachable from `refs/heads/program/architecture-lock`'s live
  tip, 0/21 are unreachable from it, and every accepted block's `base_sha`
  is a real ancestor of its `accepted_sha` — no block is falsely landed;
  the defect was solely that the oracle round 4 corrected `accepted_sha`
  reachability onto was the wrong ref, not that any acceptance was invalid.
  - **FIX 5 — add fields, do not redefine.** `repository.branch`/`head_sha`/
    `head_tree` keep their exact prior meaning (the immutable A0 entry-lock
    checkout) and are cross-checked, live, against the top-level
    `entry_checkout_sha`/`entry_checkout_tree` A0 already records
    (`verifyEntryLockIdentity`, new) — a check that did not exist as a
    dedicated function before (it was previously folded into, and
    conflated with, the trunk-pin resolution this round separates out). Two
    new `[repository]` fields, `integration_branch` and
    `integration_head_sha`, are the PINNED, MUTABLE operational-trunk
    identity: `resolvePinnedTrunk` (the fixed-landing-order rehearsal's own
    trunk pin) and `verifyLiveGitIdentities`'s `accepted_sha`
    landed/reachability check (FIX 3's own call site) now consume these,
    never `repository.branch`/`head_sha`, never checkout `HEAD`, and never
    the entry-lock pair alone. No existing field's documented meaning
    changes; this is an additive split, not a redefinition — the same shape
    §5 already uses for `implementation_candidate_sha` alongside
    `candidate_sha`.
  - **FIX 6 — the live ledger's own gap, filled, not papered over.** Making
    `verifyImplementationRefFields` unconditional (round 4, FIX 2) exposed
    that `BV2` — the live ledger's sole `IN_PROGRESS` block, predating this
    amendment's schema entirely — carried no `landing_order`/
    `implementation_candidate_sha`/`implementation_ref`. Per this round's
    own brief ("fill it correctly rather than relaxing the check"): `BV2`'s
    real, live WIP worktree (`git worktree list`: `refs/heads/block/bv2`,
    tip `855405daf5519c2b32e0d7a9e2f0b56978e76ac2`, whose top commit
    "eliminate VDOM/SSR root-prefix duplicate-ownership panic" matches
    `BV2`'s own charter/notes) is now bound as `implementation_ref`/
    `implementation_candidate_sha`; `landing_order = 1` (BV2 is the sole
    concurrently active block). The live ledger now validates clean —
    `OK ... validated 64 blocks ... 0 violations` — down from round 4's 22
    (1 FIX-2 gap + 21 FIX-3 wrong-oracle false positives).
  - Mutation-kill proof (empirical, not merely asserted): a scratch copy of
    `scripts/validate-program-state.mjs` with `resolvePinnedTrunk` reverted
    to read `repository.branch`/`repository.head_sha` (the exact round-4
    defect this round closes) is run against the CORRECTED live ledger and
    reproduces exactly the 21 false-positive violations round 4's own §9
    recorded; the real, unmutated validator run against the identical
    ledger reports 0 violations. Test suites extended: two new discriminator
    tests for `integration_branch`/`integration_head_sha` malformed
    (mirroring the pre-existing `repository.branch`/`head_sha` tests, now
    retargeted at `resolvePinnedTrunk`'s real field source) plus four new
    `verifyEntryLockIdentity` tests (branch malformed, `head_sha`/
    `head_tree` each malformed and each drifted from `entry_checkout_sha`/
    `entry_checkout_tree`) — 169/169 in `validate-mutation-suite.test.mjs`
    (was 164/164), 70/70 in `validate-program-state.test.mjs`, 30/30 in
    `validate-stack-window.test.mjs`, including the mutation suite's own
    completeness assertion that every derived check is tripped by at least
    one mutation.
  §11 remains empty; a fix round is not a ratification decision.
- Ratification (this revision) — the codex architect RATIFIED WITH
  CORRECTIONS, two of them, neither warranting a further review round:
  - **Correction 1 — the entry lock was immutable by convention, not by
    check.** `verifyEntryLockIdentity` (round 5) cross-checked
    `repository.branch`/`head_sha`/`head_tree` only against the top-level
    `entry_checkout_sha`/`entry_checkout_tree` — five fields that are all
    equally mutable ledger fields on the SAME in-memory ledger, so a single
    coordinated edit rewriting all five consistently passed cleanly (64/0)
    despite moving the "immutable" entry lock to a different checkout
    entirely. A new function, `verifyEntryLockRecordBinding`, binds those
    same five fields to the DAG root's digest-bound `entry-lock.toml` RECORD
    (content-hash-verified against `entry_lock_digest`, mirroring the
    existing `evidence_digest` content binding) — a separate file the
    in-memory ledger edit cannot also rewrite. A dedicated coordinated-
    mutation discriminator test proves this: rewriting
    `branch`/`head_sha`/`head_tree`/`entry_checkout_sha`/`entry_checkout_tree`
    in lockstep to a different, but still internally self-consistent,
    checkout still fails, because it no longer matches the separately-hashed
    record. Empirically proven by mutation (not merely asserted): a scratch
    copy of `scripts/validate-program-state.mjs` with
    `verifyEntryLockRecordBinding` neutralized, run against the exact
    coordinated-mutation fixture, WRONGLY PASSES (`OK ... 0` violations); the
    real, unmutated validator, run against the identical fixture, correctly
    FAILS with 4 violations (one per drifted field). See §3 item 2 and §9 for
    the design and coverage detail. Applying this against the REAL live
    ledger's own `A0/entry-lock.toml` (a hand-authored evidence artifact,
    not a fixture) surfaced one more real, pre-existing gap: it contains a
    multi-line array with embedded commas inside a quoted string element
    (`open_architecture_changes`), which `scripts/lib/rev11-toml.mjs`'s
    minimal reader — by explicit prior design — could not parse at all
    (`unterminated array (multi-line arrays unsupported)`). This is fixed in
    the same shared reader both validators use (never a second, divergent
    parser for this one artifact): multi-line arrays now join across raw
    lines the same way `scripts/validate-performance-gates.mjs`'s own reader
    already does (bracket-depth tracking, one trailing comma before the
    closing bracket permitted), and array-element splitting is now
    quote-aware (splits on commas outside quoted strings only) so a string
    element's own embedded commas no longer fragment it. Both are strict,
    generically-correct TOML-dialect extensions, not workarounds scoped to
    this one file.
  - **Correction 2 — the amendment text and template still described the
    superseded oracle.** §3 item 2's own body text, one §8 bullet, and the
    program-state template (lines ~118–146) still named
    `repository.branch`/`head_sha` as the trunk-pin oracle and described the
    `implementation_ref`/`implementation_candidate_sha` binding as scoped to
    `active.length > 1` — both the exact shape round 4 (FIX 2) and round 5
    (FIX 5) closed, left un-updated in the amendment's own normative prose
    after those fixes landed. §3 item 2, the affected §8 bullet, and the
    template are corrected to name `integration_branch`/`integration_head_sha`
    as the trunk-pin oracle, to state that `implementation_ref`/
    `implementation_candidate_sha` are required unconditionally for every
    `IN_PROGRESS` block, and to cross-reference `verifyEntryLockRecordBinding`
    (Correction 1) alongside `verifyEntryLockIdentity`. The round-3/round-4
    revision-history bullets that narrate what those rounds did AT THE TIME
    are left as historical record, not rewritten.
  Both corrections applied; no further review round follows (document review
  on this program is capped, and this amendment has had its round).
- Round 6 correction (post-landing, this revision) — the ratified equality
  pin (`resolvePinnedTrunk`: `repository.integration_head_sha` must EQUAL the
  live tip of `refs/heads/<repository.integration_branch>`) failed live
  validation the moment the landing commit (`2e878b6a5`) itself advanced the
  branch past the pin it recorded — a self-reference intrinsic to a ledger
  committed to the branch it pins, unfixable by any amount of resyncing.
  Corrected to ANCESTRY: the pin must be a real ancestor of the live tip
  (`git merge-base --is-ancestor`), not equal to it; `accepted_sha`
  reachability resolves against the live tip directly, never the pin. See §3
  item 2, §6, §9's round-6 update, and §11's "Round 6 correction" subsection
  for the full argument, verification, and disposition. Not a new review
  round (document review on this program is capped, and applying a
  ratification-blind-spot correction is not reopening the design) — see §11.

**Amends on ratification:** two files —
[`../../../../../scripts/validate-program-state.mjs`](../../../../../scripts/validate-program-state.mjs)
(the single-`IN_PROGRESS` gate, its bound `current_block` check, and the
disjointness rehearsal) and
[`../templates/program-state.template.toml`](../templates/program-state.template.toml)
(the `Status`/`current_block` header comments describing that gate, plus
three new per-block fields — see §3, and, as of round 5, two new
`[repository]` fields — see the round-5 revision-history entry above). It
adds no DAG edge, adds no block, retires no block, and does not itself apply
any ledger state TRANSITION (no block's `status`/review verdicts/
`maintainer_decision` change) — but as of round 5 it is no longer true that
the live ledger
([`../../../architecture-lock/ledger/program-state.toml`](../../../architecture-lock/ledger/program-state.toml))
is unedited: making the round-4 FIX-2/FIX-3 checks unconditional surfaced
real, pre-existing gaps in the live ledger's OWN data (a missing
`implementation_candidate_sha`/`implementation_ref`/`landing_order` on
`BV2`; the entry-lock-vs-integration-trunk field split this round adds),
and this round fills them — see §9. Every other row's `status`/decisions are
untouched.

## 1. Why this exists

Two rulings authorised concurrent block execution but left it mechanically
unusable:

[`MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER`](../rulings/MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md)
authorises up to 5 concurrent blocks/trains on `claude-max`, conditional on a
mechanically testable no-merge-conflict-risk property (`git merge-tree
--write-tree`), and states plainly:

> The ratified validator still fails closed at ONE `IN_PROGRESS` block
> (`scripts/validate-program-state.mjs:794-805`), and its own comment says a
> parallel regime "must relax this check under review, not ad hoc". So raising
> the ceiling requires a REVIEWED validator change plus the
> fence/rehearsal/review-binding machinery — the ruling authorises the
> destination, it does not itself relax the check.

[`ARCH-RULING-CONCURRENCY-OPERATING-MODEL`](../rulings/ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md)
rules that the model to adopt is "allow up to five disjoint blocks in
IMPLEMENTATION and targeted testing; SERIALISE final certification", and that
this needs `IN_PROGRESS` — which today "conflates 'being implemented' with
'being certified'" — separated into an implementation notion (concurrent, cap
5) and a certification notion.

Neither ruling is self-executing: both are policy authorisations over a
validator that, unamended, still fails closed the moment a second block enters
`IN_PROGRESS`. This is that reviewed validator change.

## 2. The state model

`scripts/validate-program-state.mjs`'s block-status enum is unchanged (no new
status is introduced). The existing `IN_PROGRESS`/`REVIEW`/
`ACCEPTANCE_RECOMMENDED` statuses split into **three** concurrency classes,
not two — the prior draft's two-way split (`IN_PROGRESS` vs.
`REVIEW`+`ACCEPTANCE_RECOMMENDED` together) directly contradicted
[`contracts/stacked-prs.md`](../contracts/stacked-prs.md), which describes a
`LANDABLE` stack where several upper layers sit green in `REVIEW`
simultaneously while only the bottom layer is `ACCEPTANCE_RECOMMENDED`
(`contracts/stacked-prs.md:100` — "`LAND_READY` means all mergeable layers are
green on the named immutable snapshot and the one currently eligible landing
block is `ACCEPTANCE_RECOMMENDED` ... Green upper `LANDABLE` layers remain
`REVIEW`, not accepted in advance"). A validator that fails the moment a
second block is `REVIEW` cannot accept that shape at all.

- **Implementation** (`IN_PROGRESS`), **review iteration** (`REVIEW`), and
  **final certification** (`ACCEPTANCE_RECOMMENDED`) remain three distinct
  concurrency CLASSES for the per-status rules below (`current_block`
  binding, the `ACCEPTANCE_RECOMMENDED`-first ordering in §3) — that part of
  v2 was not disputed and is unchanged.
- **The numeric ceiling itself is a SINGLE program-wide cap over the whole
  active set, not per-class (v3, Finding B).** v2 capped only
  `IN_PROGRESS.length` at `MAX_CONCURRENT_IMPLEMENTATION = 5`, so 5
  `IN_PROGRESS` blocks plus 1 `ACCEPTANCE_RECOMMENDED` block — six
  concurrently active blocks — was silently legal, against a ruling whose own
  words are "up to 5 concurrent blocks/trains", not "up to 5 IN_PROGRESS plus
  1 more". v3 caps `|IN_PROGRESS ∪ REVIEW ∪ ACCEPTANCE_RECOMMENDED| ≤ 5`
  program-wide, whatever the status mix. `contracts/stacked-prs.md` §4's
  per-stack open-layer limit (default 4, A6-lockable 2–6) and §3.3's
  ownership-disjointness rule bound how many blocks may sit in `REVIEW`
  WITHIN one stack or across independent windows, but the review that
  rejected v2 explicitly held that neither is a PROGRAM-WIDE numeric cap
  equivalent to the ruling's flat ceiling — so `REVIEW` blocks are not exempt
  from this cap either. `contracts/stacked-prs.md:100` ("Green upper
  `LANDABLE` layers remain `REVIEW`, not accepted in advance") establishes
  that `REVIEW` is not capped at ONE the way `ACCEPTANCE_RECOMMENDED` is —
  not that it is exempt from the shared five-block ceiling.
- `ACCEPTANCE_RECOMMENDED` additionally stays capped at exactly 1 block,
  program-wide, at a time — the ruling's "serialise final certification"
  subject: "restack the next block once, freeze it once, run ONE full gate,
  obtain ONE impact-bounded mandate re-attestation" describes the
  `ACCEPTANCE_RECOMMENDED` transition specifically, not `REVIEW`'s iterative
  revise/re-review cycle. This is a SEPARATE, additional constraint layered
  on top of the shared active-set ceiling, not a substitute for it.
- **Named, unresolved tension (not silently swept — see §8):** this is a
  block-counting proxy for "concurrent claude-max trains", and a single
  stack window legally holding up to six open `REVIEW` layers (A6,
  `contracts/stacked-prs.md` §4) could, on its own, approach or exceed this
  ceiling even though it may represent one orchestrated train, not six. The
  ledger has no field naming train/orchestrator identity directly (only
  `stack_id`, which groups blocks within one stack but says nothing about
  cross-stack train identity), so this amendment counts blocks, not trains,
  as the conservative, mechanically-checkable proxy.

`current_block` names the sole `ACCEPTANCE_RECOMMENDED` block when one exists
— the "currently eligible landing block" in `contracts/stacked-prs.md`'s own
words, and what `ARCH-RULING-CONCURRENCY-OPERATING-MODEL`'s "current_block ...
names the certifying block" actually refers to once certifying is scoped to
final certification rather than to `REVIEW`. With nothing
`ACCEPTANCE_RECOMMENDED`, `current_block` instead names any concurrently
ACTIVE block (`IN_PROGRESS` or `REVIEW`) — the single-active-block serial case
(still legal; every cap here is a ceiling, not a floor) satisfies this
trivially. This is the same shape the live ledger exercises today (one
`IN_PROGRESS` block, `BV2`, `current_block = "BV2"`, nothing
`ACCEPTANCE_RECOMMENDED`).

## 3. Fixed-landing-order cumulative rehearsal

**v2 rehearsed the wrong operation (Finding A).** v2's cumulative walk used
`git merge-tree --write-tree` followed by a TWO-parent `git commit-tree` —
modeling a MERGE COMMIT. This program does not land by merging: every landing
this program performs is a rebase-or-squash onto trunk followed by a
fast-forward (`contracts/stacked-prs.md` §9 names only "Bottom-up" and
"Atomic final only" as legal landing modes, and its own `accepted_sha`/`tree`
commentary lists "a reviewed rebase, squash, merge commit, or merge-queue base
advance" as the shapes `accepted_sha` may take relative to `candidate_sha` —
never a landing-time two-parent merge of the candidate against trunk). A
two-parent synthetic commit creates candidate ancestry no squash/rebase/
cherry-pick landing ever produces, and — the more serious defect — it let
each step's merge-base be whatever git's own commit-graph search found
between that synthetic two-parent commit and the next candidate, NOT the
block's own declared `base_sha` (Finding D, below): a wrong or stale declared
base was ancestor-checked and then silently ignored by the rehearsal itself.

v3 replaces the whole mechanism with a REPLAY, the operation this program
actually performs: for every concurrently ACTIVE block (`IN_PROGRESS` ∪
`REVIEW` ∪ `ACCEPTANCE_RECOMMENDED`), live mode only (needs real git
objects):

1. **A fixed landing order is a recorded, checked ledger fact, not an
   assumption** (unchanged from v2 — not a disputed part of it). Three
   per-block fields:
   - `landing_order` (integer, default `0`) — required to be a positive,
     pairwise-distinct integer across every concurrently active block
     whenever more than one is active. `MAINTAINER-RULING-CONCURRENCY-
     CEILING-AND-ROSTER.md` and `ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md`
     both use the phrase "a fixed/declared landing order" as something that
     exists to be rehearsed against. The sole `ACCEPTANCE_RECOMMENDED`
     block, when one exists, must hold the MINIMUM `landing_order` among the
     active set — it is, definitionally, "the currently eligible landing
     block" (`contracts/stacked-prs.md`). Where one active block is a DAG
     predecessor of another concurrently active block, the predecessor's
     `landing_order` must be lower. **v3 additionally cross-validates
     `landing_order` against same-stack `stack_layer` (Finding E):** two
     concurrently active blocks sharing a non-empty `stack_id`, both
     carrying an integer `stack_layer`, must have the lower `stack_layer`
     land first — `contracts/stacked-prs.md` §9's bottom-up-lands-first
     rule, now bound to something checkable rather than left as prose a
     stack's own private sublayers (which need not be DAG predecessors of
     one another) could otherwise violate undetected.
   - `implementation_candidate_sha` (string, default `""`) — a rehearsal
     identity SEPARATE from `candidate_sha` for `IN_PROGRESS` blocks. See §5.
   - `implementation_ref` (string, default `""`) — a git ref (a branch name
     or a full `refs/...` ref) naming the `IN_PROGRESS` block's live WIP
     branch, closing Finding E's second bullet (round 3, Finding H). See §5.
2. **The trunk is a ledger-declared, revalidated PIN against the CONFIGURED
   INTEGRATION-TRUNK REF, not an ambient sample, not checkout HEAD, and not
   the immutable A0 entry lock (Finding C, corrected again at round 3,
   Finding G; re-targeted at round 5, FIX 5).** v2 called `git rev-parse HEAD`
   inside the rehearsal itself — two validator runs minutes apart could
   rehearse against two different trunks with nothing in the ledger
   recording which one. v3's first answer reused `[repository].head_sha` as
   the pin but still validated it against checkout `HEAD` — which routinely
   differs from trunk in an ordinary worktree (a feature-branch checkout, a
   review worktree) for reasons that have nothing to do with trunk staleness,
   and scoping the check to `active.length > 1` was compensating for that
   wrong oracle, not a genuine exemption. Round 3 corrected `resolvePinnedTrunk`
   to instead validate `head_sha` against the LIVE TIP OF THE EXPLICIT,
   LEDGER-NAMED REF `refs/heads/<repository.branch>` — but round 5 (FIX 5)
   found that `repository.branch`/`head_sha` is the IMMUTABLE A0 entry-lock
   checkout (`contracts/baseline-lock.md` §2), never the moving operational
   trunk every landing/rehearsal actually replays against; pointing the
   rehearsal at it was aimed at the wrong ref, not merely a stale one (see the
   round-5 revision-history entry above for the full correction and its
   exposure check). The CURRENT, live `resolvePinnedTrunk` instead validates
   the SEPARATE, MUTABLE `repository.integration_head_sha` against the LIVE
   TIP OF THE CONFIGURED INTEGRATION-TRUNK REF —
   `refs/heads/<repository.integration_branch>` (`git rev-parse --verify`) —
   requiring both `repository.integration_branch` to be a well-formed branch
   name and `repository.integration_head_sha` to be a well-formed git object
   id that is a real ANCESTOR of that ref's live resolved tip (`git
   merge-base --is-ancestor`; self-ancestry — the pin equalling the live tip
   exactly — also satisfies this). This is a round-6 correction (see the
   revision-history entry below): the ledger this pin lives in is committed
   TO the branch it pins, so the ORIGINAL equality requirement made the pin
   stale the instant the committing commit landed, and no amount of resyncing
   could converge. `repository.branch`/`head_sha`/`head_tree` keep their
   entry-lock meaning, unchanged, and are validated separately by
   `verifyEntryLockIdentity` (cross-checked against the top-level
   `entry_checkout_sha`/`entry_checkout_tree`) and, as of this ratification's
   correction 1, `verifyEntryLockRecordBinding` (additionally content-bound to
   the DAG root's digest-bound `entry-lock.toml` record, so a coordinated
   rewrite of all five entry-lock fields together — self-consistent, but no
   longer matching that separately-hashed record — is caught rather than
   passing on internal consistency alone). A pin that is NOT an ancestor at
   all (a rewritten or foreign commit) is a violation, never a silent resync,
   and the rehearsal does not run at all until the ledger is resynced onto a
   real ancestor; a pin that lags behind the live tip but IS a genuine
   ancestor is valid rehearsal input — the pin's job is a deterministic
   replay base, not a freshness claim, and reachability checks (below) always
   resolve against the trunk's LIVE tip regardless of how far the pin lags.
   Because the oracle is the correct one, this check runs UNCONDITIONALLY on
   every live-mode validation — not scoped to `active.length > 1` — since
   there is no remaining reason to skip it on an ordinary single-active-block
   run (§9 reverifies the live ledger passes this unconditionally).
3. **The rehearsal replays each block's OWN delta, not a merge (Finding
   A/D).** Starting from the pinned trunk, the walk proceeds in
   `landing_order`; at each step:
   - `base_sha` is verified to be an ancestor of that block's rehearsal
     candidate (`git merge-base --is-ancestor`) — unchanged from v2, but now
     UNCONDITIONAL: `base_sha` is REQUIRED and well-formed for every
     concurrently active block, not merely checked when one happens to be
     present (v2 left it optional/decorative for `IN_PROGRESS`).
   - `git merge-tree --write-tree --merge-base=<base_sha> <cumulative>
     <candidate>` (git ≥ 2.38) replays the block's OWN `base_sha..candidate`
     delta onto the cumulative result of every prior block — the exact
     three-way-merge shape `git rebase --onto`/`git cherry-pick` perform
     (base = the commit's own original parent), not a merge-base git's own
     ancestry search happens to find between two unrelated trees. The
     declared `base_sha` IS the delta basis now, not a value that is merely
     ancestor-checked and then ignored.
   - A clean step synthesises a real, unreferenced, worktree-untouched
     SINGLE-PARENT commit via `git commit-tree <tree> -p <cumulative>` —
     modeling the single-parent commit a rebase/squash landing actually
     produces, never a two-parent merge commit — so the next step's replay
     sees genuine linear ancestry. A conflict (exit 1) or any other
     rehearsal failure stops the walk at that step; nothing past an
     unrehearsable step is vouched for.
4. **Missing rehearsal identity, missing implementation ref binding, missing
   base, invalid order, or an unresolved trunk pin is fail-closed, not
   silently skipped.** A concurrently active block with a malformed
   rehearsal candidate or `base_sha`, an `IN_PROGRESS` block whose
   `implementation_ref` is malformed, unresolvable, or resolves to a commit
   OTHER than the declared `implementation_candidate_sha` (round 3, Finding
   H), a non-positive or duplicate `landing_order`, a misordered
   `ACCEPTANCE_RECOMMENDED` block, an order that violates a DAG predecessor
   edge or same-stack layer order between two active blocks, or a trunk pin
   that failed to resolve or revalidate, is its own violation, and the git
   walk does not run at all for that active set — a partially trustworthy
   input proves nothing about the untrustworthy part.

**Trust boundary CLOSED (Finding E, second bullet — round 3, Finding H;
hardened round 4, FIX 1; re-scoped round 4, FIX 2).**
v3 left `implementation_candidate_sha` a declaration the validator trusted
but could not verify: the rehearsal proved it was a real, `base_sha`-
descended commit, but not that it IS the block's actual current WIP tip.
`implementation_ref` closes this: the rehearsal resolves that ref and
REQUIRES its live tip to equal `implementation_candidate_sha` exactly. A
stale pin — the ref has moved on, in either direction, since the ledger was
last written — is a violation, not a silently trusted declaration. This does
not (and cannot) prove the ref itself is the "right" branch for the block —
that binding is still a declaration — but it does prove the PIN is not stale
relative to whatever ref it claims to be, closing the specific gap Finding E
named.

Round 4 (FIX 1) found that "resolves via `git rev-parse --verify`" was not
by itself sufficient: a raw 40-char object id and the literal `HEAD`
pseudoref both resolve cleanly (the former to itself, the latter to
wherever this worktree happens to be checked out), so a declared
`implementation_ref` equal to `implementation_candidate_sha` itself
satisfied the old check trivially, regardless of how stale the real pin
was. `checkImplementationRefBinding` now explicitly rejects a raw object id
or the literal `HEAD` before any git call, then requires `git rev-parse
--symbolic-full-name` to resolve to a `refs/heads/...` branch specifically
— a real ref, never a tag or any other rev-parse-able spec.

Round 4 (FIX 2) additionally found that this whole binding was reachable
only through `verifyConcurrentLandingSafety`'s own `active.length > 1` gate
— so the ordinary, single-`IN_PROGRESS`-block ledger (this program's own
current shape) validated NEITHER field at all. `verifyImplementationRefFields`
now runs `checkImplementationRefBinding` over every `IN_PROGRESS` block
UNCONDITIONALLY, before the rehearsal gate is even evaluated; both fields
are MANDATORY for every `IN_PROGRESS` block in live mode now, whether or not
another block is concurrently active — see §9 for what running this against
the real live ledger reveals.

## 4. Serialised-certification cap, restated

Re-emphasising because it is easy to conflate with §3: there are now THREE
independent cardinality checks, none a substitute for another:

1. the shared active-set ceiling (§2, Finding B) —
   `|IN_PROGRESS ∪ REVIEW ∪ ACCEPTANCE_RECOMMENDED| ≤ 5`, program-wide,
   unconditional;
2. the single-`ACCEPTANCE_RECOMMENDED`-at-a-time cap (§2), a cardinality
   check on ledger status, unconditional, layered on top of (1) — it does
   not relax or replace the shared ceiling, it further restricts the
   certification slot within it;
3. the fixed-landing-order rehearsal (§3), a SEPARATE, mechanically-testable
   ordering/replay property that runs over the same active set whenever more
   than one block is concurrently active.

All three are required; none substitutes for another.

## 5. `candidate_sha` keeps its documented meaning

`contracts/stacked-prs.md:140` documents `candidate_sha`/`tree` as "the exact
cumulative candidate reviewers inspected". The prior draft let an
`IN_PROGRESS` block advance `candidate_sha` freely as an "unbound WIP
pointer" before any review occurred — silently overloading a field whose
documented meaning is tied to review, not to implementation-in-progress.

This amendment does not touch that documented meaning. Instead:

- `REVIEW`/`ACCEPTANCE_RECOMMENDED` blocks are rehearsed against
  `candidate_sha` — the SAME identity a `PASS` mandate is bound to
  (`REVIEWED_SHA_FIELDS`' existing stale-verdict check already fences it: an
  advance to `candidate_sha` without a fresh review is already a violation).
  So for every block this rehearsal treats as "under review or about to
  land", the rehearsal candidate genuinely IS the exact reviewed candidate.
- `IN_PROGRESS` blocks are rehearsed against the new, separate
  `implementation_candidate_sha` — an explicitly WIP-scoped field with no
  documented "exact reviewed candidate" claim to preserve. It is a NEW ledger
  field precisely to avoid amending `candidate_sha`'s existing meaning
  program-wide (a normative-contract-level change) to accommodate one
  validator amendment. Round 3 (Finding H) additionally binds this field to
  `implementation_ref`, a real resolvable git ref whose live tip the
  rehearsal requires to equal it exactly (§3 item 1, §8) — closing the trust
  boundary this section's own overload analysis does not touch.

This closes the prior draft's overload without touching
`contracts/stacked-prs.md`.

## 6. Preserved invariants

Every invariant the pre-amendment validator enforced is unchanged in kind,
only re-scoped from "the one `IN_PROGRESS` block" to "every concurrently
active/certifying block":

- Sequencing (direct predecessors `ACCEPTED` before a block begins, including
  the stacked-work and `PRIVATE_CHECKPOINT` exceptions) — untouched; it
  already iterates every begun block, not just the sole `IN_PROGRESS` one.
- Review-verdict-to-candidate binding (a `PASS` mandate's reviewed SHA must
  equal the row's current `candidate_sha`) — untouched; §5 leans on this
  directly.
- Authorization records required to leave `LOCKED` — untouched.
- `accepted_sha` reachable from trunk, `base_sha` its ancestor — the CHECK
  ITSELF is untouched in kind (it stays `ACCEPTED`-only; §3's separate
  `base_sha` ancestor check is scoped to the active-set rehearsal, is now
  UNCONDITIONAL there rather than "checked only when present" (Finding D),
  and does not extend the `ACCEPTED`-only check) — but what "reachable from
  trunk" resolves AGAINST has been corrected three times since the
  pre-amendment validator, which sampled checkout `HEAD`: round 4 (FIX 3)
  re-pointed it at the ledger-pinned trunk, round 5 (FIX 5) re-targeted that
  pin from the immutable A0 entry-lock pair (`repository.branch`/`head_sha`)
  onto the mutable operational-trunk pair, `repository.integration_branch`/
  `integration_head_sha`, and round 6 (see the revision-history entry below)
  split what each of the two consumers resolves against: `resolvePinnedTrunk`
  now validates the pin by ANCESTRY (a real ancestor of the live tip, not
  equal to it), while `verifyLiveGitIdentities`'s `accepted_sha` reachability
  check resolves against the trunk's LIVE tip directly, never the pin — a
  block landed after the pin was last recorded is not wrongly rejected merely
  because the pin has since lagged (see `resolvePinnedTrunk`, consumed by
  both `verifyConcurrentLandingSafety` and `verifyLiveGitIdentities`).
- The amendment-authority gate, entry-lock binding, evidence-digest binding,
  block-authorization registry — all untouched; none of them key off
  cardinality of `IN_PROGRESS`/`REVIEW`/`ACCEPTANCE_RECOMMENDED`.

## 7. What this does NOT do

- It does not open any new block, change any block's status, or edit the live
  ledger.
- It does not add, remove, or rename any block-status or review-result enum
  value.
- It does not change sequencing, review-verdict-binding, authorization, or
  entry-lock semantics.
- It does not change `contracts/stacked-prs.md`'s documented meaning of
  `candidate_sha` (§5).
- It does not resolve the N > 5 roster question or the restack-cascade
  throughput question (§8) — both stay open, tracked by their originating
  rulings.

## 8. What this leaves undecided (named, not hidden)

- **The N > 5 / grok-implementer roster is not modelled.** Unchanged from the
  prior draft — see the alternatives in §10.
- **The restack-cascade / `landing_equivalence_digest`-carries-forward
  question is open.** `MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER` poses
  it as "under evaluation", not decided. This amendment does not touch
  `landing_equivalence_digest` and does not let a review verdict carry across
  a restack — a restack that changes `candidate_sha` still requires a fresh
  `PASS` bound to the new SHA (§6). The single-`ACCEPTANCE_RECOMMENDED` cap
  (§2/§4) means only one block is ever mid-restack-and-final-certification at
  a time, so the cascade-cost question is orthogonal to whether concurrency
  is *legal*.
- **Semantic (not just path) disjointness is only partially covered.**
  `ARCH-RULING-CONCURRENCY-OPERATING-MODEL` names "shared registries, APIs,
  generated artifacts, build configuration, resource budgets, integration
  tests" as needing disjointness beyond file paths. The §3 rehearsal catches
  textual conflicts (including two blocks editing the same generated-artifact
  file) at every step of the fixed landing order, but it cannot catch two
  non-conflicting diffs that are nonetheless semantically incompatible. That
  class of defect is unchanged from today's serial regime's own review
  discipline and is not a gap this amendment introduces.
- **Stack windows (`contracts/stacked-prs.md`, AMD-001) are a separate
  mechanism, but §3's ordering now explicitly interoperates with `stack_layer`
  where both apply.** A stacked pair (predecessor `IN_PROGRESS`, successor
  `REVIEW` over it via the contingent-stacked-work exception) is exactly the
  canonical case §3's DAG-predecessor-order rule targets: the predecessor's
  `landing_order` must precede its dependent's. `stack_layer` (AMD-001) and
  `landing_order` (this amendment) answer different questions — relative
  position WITHIN one stack vs. a total order across every concurrently
  active block program-wide — and neither is derived from the other.
- **`landing_order` is program-state-only.** It is not modelled in
  `stack-window.template.toml` or cross-validated by
  `tools/validate_stack_window.py` — a future amendment may tie the two
  together if that composite validation becomes load-bearing; today they are
  independently enforced.
- **Active-block-count is a coarser proxy than train/orchestrator identity
  (Finding B, named in §2).** The shared five-block ceiling counts ledger
  ROWS, not claude-max orchestrator sessions. A single stack window legally
  holding up to six open `REVIEW` layers (A6, `contracts/stacked-prs.md` §4)
  could, on its own, approach or exceed this ceiling even if one orchestrator
  manages the whole stack as a single train. Modelling true train identity
  (e.g. grouping by `stack_id`, with a standalone non-empty-`stack_id`-less
  block counting as its own train) would answer this more precisely, but no
  ruling asks for that refinement yet and it is a materially larger design
  than the ceiling fix this amendment ships — recorded as a known limitation
  of the current block-counting proxy, not resolved here.
- **`implementation_candidate_sha` staleness (Finding E, second bullet) is
  CLOSED (round 3, Finding H), with one narrower residual named below.**
  v3 left this a trusted, unverifiable declaration. `implementation_ref`
  closes it: the rehearsal resolves that ref's live tip and REQUIRES it to
  equal `implementation_candidate_sha` exactly, so a pin that has gone stale
  relative to the ref it names is now a detected violation, not a silent
  trust. The residual: this proves the PIN matches the NAMED ref's live tip —
  it does not and cannot prove `implementation_ref` itself names the RIGHT
  branch for the block (a block could declare an unrelated but real,
  live-matching ref). That binding — "this ref is genuinely this block's WIP
  branch" — remains a declaration outside what any ledger this validator
  reads can independently observe; only review/authoring discipline around
  the ledger prevents that narrower substitution today.
- **What "reachable from trunk" means for `accepted_sha` — raised as an open
  question at round 4 (FIX 3), CLOSED at round 5 (FIX 3/FIX 5; see §9's
  live-ledger result for the full account).** Round 4's `verifyLiveGitIdentities`
  `accepted_sha` reachability check pointed at the SAME `refs/heads/<repository.branch>`
  pin the fixed-landing-order rehearsal used at the time, closing the
  checkout-`HEAD` defect Finding G named at a second call site — but that
  revealed every currently-`ACCEPTED` block in the live ledger except the
  entry block's own chain as "unreachable" from `refs/heads/main`, which read
  as an open ledger/process question ("does `accepted_sha` reachability mean
  upstream trunk, or this program's own development history?"). It was not a
  real ambiguity: `repository.branch` (`main`) is the immutable A0 entry-lock
  checkout, never the ref anything lands on, and this program lands onto its
  own long-lived integration branch, `program/architecture-lock`, completely
  independent of whether/when that branch is later merged onto `main`. Round 5
  re-targets both `resolvePinnedTrunk` and this same `accepted_sha`
  reachability check at `repository.integration_branch`/`integration_head_sha`
  — `refs/heads/program/architecture-lock`'s live tip — and the live ledger
  now validates with 0 violations (down from round 4's 22; see §9). No
  maintainer ruling on "which trunk" was needed: the round-4 oracle was aimed
  at the wrong ref, not at a genuinely undecided definition.

## 9. Verification on ratification

```
node --test scripts/validate-program-state.test.mjs
node --test scripts/validate-mutation-suite.test.mjs
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live
```

On the prepared-against tree: the first two commands pass in full — 70/70 in
`validate-program-state.test.mjs` and, as of round 5, 169/169 in
`validate-mutation-suite.test.mjs` (was 164/164 through round 4; round 5
adds two `integration_branch`/`integration_head_sha` malformed
discriminators plus four `verifyEntryLockIdentity` discriminators — see the
round-5 revision-history entry above), 239/239 run together, including the
mutation suite's completeness assertion that every `v(...)` call site
derived from `scripts/validate-program-state.mjs` was tripped by at least
one mutation. Every check v3 replaced, every net-new check the review's six
findings required, every net-new check round 3's three findings required,
and every net-new check round 4's four findings required, is covered: the
active-set (not `IN_PROGRESS`-only)
ceiling message and its five-plus-one discriminator fixture (Finding B); the
trunk-pin malformed/mismatched/git-failure branches, including a dedicated
fixture proving the rehearsal itself refuses to proceed on an unresolved pin
rather than silently skipping (Finding C); the now-mandatory `base_sha` check
and its own discriminator fixture (missing base with a present candidate,
isolated from the candidate-missing check) (Finding D); the same-stack
`stack_layer`-vs-`landing_order` cross-check, isolated from the
DAG-predecessor-order check via true DAG-sibling fixtures (Finding E, first
bullet); a dedicated adversarial fixture, in its own dedicated git
repository, that CONFLICTS under the new `--merge-base=<base_sha>` replay but
would have PASSED under the v2 auto-derived-merge-base mechanics, directly
discriminating the fixed behavior from the rejected one rather than merely
asserting the new code path runs (Finding A/D, Finding F); the malformed
`repository.branch` check (round 3, Finding G); the malformed/unresolvable/
mismatched `implementation_ref` checks, each isolated to its own fixture
(round 3, Finding H); and three round-3 discriminating fixtures, each
EMPIRICALLY proven — by literally applying the rejected mutation to a scratch
copy of `scripts/validate-program-state.mjs`, running the exact discriminator
fixture against it, observing red, then restoring the file and observing
green — not merely asserted to discriminate (round 3, Finding I):
  - a trunk-ref-vs-checkout-HEAD fixture (checkout `HEAD` on a feature branch
    while the ledger correctly pins `refs/heads/main`'s own live tip) that
    PASSES under the corrected branch-ref oracle and was hand-verified to
    FAIL under a reintroduced `git rev-parse HEAD` (a mirror fixture in the
    other direction — checkout `HEAD` matching a now-stale pin while `main`
    has genuinely advanced — additionally proves the corrected oracle still
    catches real staleness, though that fixture does not itself discriminate
    against the HEAD-vs-branch defect, since both the correct and the
    rejected code reject it, for different reasons);
  - a dedicated `git` shim that intercepts every real `commit-tree`
    invocation and fails loudly the instant it observes anything other than
    exactly one `-p` flag; the real, unmutated rehearsal passes cleanly
    through it, and hand-verified reintroducing a second `-p` (the exact
    v2 two-parent-merge-commit footgun) trips the shim and turns the test
    red;
  - the `implementation_ref`-resolves-but-mismatches-the-pin fixture already
    listed under Finding H above IS this discriminator: a live, resolvable
    ref (`concurrent-b`) whose current tip does not equal the declared
    `implementation_candidate_sha` (`concurrent-a`), hand-verified to fail
    to be caught with the implementation_ref block removed.

Round 4 adds: a raw-40-char-object-id `implementation_ref` and the literal
`HEAD` value are each rejected (FIX 1), isolated from a real, tip-matching
TAG `implementation_ref` — resolves, tip matches, and is STILL rejected
because it is not a branch (FIX 1's `refs/heads/` requirement, distinct from
the raw-OID/HEAD rejection). A SOLE `IN_PROGRESS` block (no other block
concurrently active) still has `implementation_ref`/
`implementation_candidate_sha` checked (FIX 2); a REVIEW block's own
`candidate_sha` missing in a concurrent set is isolated from the
`IN_PROGRESS`-specific check above it (the two checks are now genuinely
distinct call sites, not one shared message). Two round-4 discriminators are
additionally EMPIRICALLY proven by mutation, the same method as round 3's
three: a scratch copy of `scripts/validate-program-state.mjs`, written
alongside the real file so its own `./lib/...` relative imports still
resolve, with FIX 1's raw-OID/HEAD guard (respectively FIX 2's unconditional
call) neutralized, is run against the exact fixture the corresponding new
test uses and observed to WRONGLY PASS; the real, unmutated file is then run
against the identical fixture and observed to correctly fail. Both are
additionally hand-verified directly against the tracked source (not only the
scratch copy the automated test builds), applying each rejected mutation,
confirming red, restoring, and confirming green — reported in full in the
implementer's own report for this round.

The third command FAILS against the CURRENT live ledger — 22 violations, not
a clean pass — a materially different outcome from round 3's "64 blocks, 0
violations," verified directly on this tree, not merely asserted, and worth
recording precisely because it is a real, previously-hidden condition FIX 2
and FIX 3 surface rather than a validator regression:
- **1 violation is FIX 2's own, expected consequence.** `BV2`, the ledger's
  sole `IN_PROGRESS` block, predates this amendment's schema entirely (no
  `landing_order`/`implementation_candidate_sha`/`implementation_ref`
  fields) — round 3's own text called this "unaffected by the trunk-pin fix"
  because the fixed-landing-order rehearsal exempted a lone active block
  outright. FIX 2 closes exactly that exemption for the two new fields: `BV2`
  is IN_PROGRESS with an empty `implementation_candidate_sha`, so it is now
  correctly flagged. This is the real ledger gap FIX 2 was asked to prove
  rather than paper over with a re-added conditional (per the implementer's
  own brief) — it is NOT remediated here; the maintainer's next transition
  on `BV2` needs to populate both fields.
- **21 violations are FIX 3's own, previously-masked discovery.** Every
  OTHER `ACCEPTED` block in the ledger (`A0`…`A6`, `B1`…`B4`, `BF1`…`BF3`,
  `BV0`/`BV0A`/`BV1`, `BS0`/`BS1`, `BA0`, `BRT0`) reports its `accepted_sha`
  as "not reachable from the configured trunk ref's tip" —
  `refs/heads/main`'s actual live tip on this machine. Confirmed by hand,
  not merely inferred: (a) re-running round 3's OWN unmodified code (`git
  show HEAD:scripts/validate-program-state.mjs`) against the identical
  ledger, from the identical worktree, still prints `OK … validated 64
  blocks … 0 violations` — proving the 21 violations are net-new to this
  round's fix, not a pre-existing defect this round happened to also touch;
  (b) `git merge-base --is-ancestor <accepted_sha> refs/heads/main` fails
  (exit 1) for these commits, while `git merge-base --is-ancestor
  <accepted_sha> HEAD` (checkout `HEAD`, `feat/concurrent-blocks`) succeeds —
  every one of these "accepted" commits is real and reachable, but ONLY from
  this program's own long-lived feature branch, never from the `main` the
  ledger's own `repository.branch` field names as trunk. Round 3's checkout-
  `HEAD` oracle passed all 64 blocks cleanly for exactly the same reason
  Finding G named for the trunk-pin check: this validator has, since this
  program began, always been run from a worktree checked out ON
  `feat/concurrent-blocks` itself, so "the repository tip" (checkout `HEAD`)
  was trivially every one of this branch's own sequential commits — the
  check was never actually exercising "has this landed on trunk," only
  "does this exist somewhere in whatever branch I happen to be sitting on,"
  which is true by construction for a program that commits its own history
  sequentially onto one feature branch. FIX 3 is the corrected oracle
  working exactly as intended: it is not this validator's job to decide
  whether the maintainer intends `accepted_sha` reachability to mean
  "reachable from upstream `main`" or "reachable from this program's own
  development branch" — that is a ledger/process definition the maintainer
  owns, not a defect this validator change introduces or an ambiguity it
  is scoped to resolve. This is recorded here, plainly, as a REAL, newly-
  discovered gap for the maintainer's ratification decision, not silently
  worked around by reverting FIX 3 or by re-scoping the check.

**Round 5 update — the above is superseded, not deleted (kept as the exact
historical record of what round 4 found and why).** The maintainer's
ratification decision this passage names was never required, because the
"gap" was FIX 3 pointing the corrected oracle at the wrong ref, not a real
question about what `accepted_sha` reachability should mean. `main` genuinely
is this program's entry-lock trunk and genuinely has not merged this
program's history — that was never in dispute — but `refs/heads/main` was
never the ref FIX 3's `resolvePinnedTrunk`/`accepted_sha`-reachability
checks needed: this program lands onto its own long-lived integration branch
(`program/architecture-lock`), which every accepted block actually lands on,
completely independent of whether/when that branch is later merged onto
`main`. Round 5 (see the revision-history entry above, FIX 5/FIX 6) adds
`repository.integration_branch`/`integration_head_sha` as that ref's pin,
re-targets `resolvePinnedTrunk` and the `accepted_sha` reachability check at
it, and fills `BV2`'s three missing fields against its real live WIP
worktree. Re-running the same third command on the current tree now reports:

```
OK: program-state.toml (docs/arch/architecture-lock/ledger/program-state.toml) — validated 64 blocks (non-zero work asserted) against docs/arch/refactor/rev11/program-dag.toml in mode live
```

0 violations — down from round 4's 22 (1 FIX-2 gap, now filled; 21 FIX-3
wrong-oracle false positives, now correctly resolved against
`program/architecture-lock` instead of `main`). All 21 of the previously-
flagged `ACCEPTED` blocks are, and always were, genuinely landed — on the
program's own integration branch, which is what `accepted_sha` reachability
was always meant to prove.

Two git-mechanics findings surfaced only while building the live rehearsal,
recorded here since they are not obvious from the git-merge-tree manual page
summary and would silently break a future edit to this code:
- `git merge-tree --write-tree --quiet` (git 2.55) suppresses the toplevel
  tree-OID output on a clean merge, not merely conflict diagnostics — "allows
  merge-tree ... to avoid writing most objects created by merges." The
  rehearsal needs that OID for `git commit-tree`, so it never passes
  `--quiet`.
- `git merge-tree --merge-base=<tree-ish>` (git ≥ 2.38) accepts any tree-ish,
  including one that is NOT the git-computed merge-base of the two branches
  being compared — this is exactly what makes it usable as a replay-basis
  pin rather than an ancestry oracle, and it is proven discriminating (not
  merely asserted) by the dedicated adversarial fixture in §9's coverage
  list above, confirmed by hand against a throwaway repository outside the
  test suite: the SAME commit pair conflicts with `--merge-base=<declared
  root>` and passes cleanly with no `--merge-base` flag at all (git's own
  ancestry search resolving to the candidate's real, more recent parent).
  (v2 also recorded a `git commit-tree <tree> -p X -p X` two-identical-
  parents footgun for its two-parent merge commit; v3's single-parent
  `git commit-tree <tree> -p <cumulative>` has only one `-p`, so that
  specific failure mode no longer applies — moot, not silently dropped.)

**Round 6 update (post-landing correction) — the above is superseded, not
deleted (kept as the exact historical record of what round 5 found and
fixed).** This amendment landed as commit `2e878b6a5` and its ledger update
IMMEDIATELY failed live validation on the very next run — a self-reference
round 5 did not anticipate: `program-state.toml` is committed TO
`program/architecture-lock`, the branch its own `integration_head_sha` pins,
so landing the commit containing the pin necessarily advances the branch's
live tip PAST the pin the moment the commit exists. Requiring the pin to
EQUAL the live tip (round 5's `resolvePinnedTrunk`) made this specific ledger
permanently unable to pass its own validator — no amount of resyncing
converges, because every resync commit is itself a further advance.

The fix narrows what the pin is asked to prove. Equality was never load-
bearing for the rehearsal's actual job — a deterministic, reproducible replay
base for the fixed-landing-order walk (§3 item 3) — ancestry is: replaying
onto any real ancestor of the live trunk reproduces the same result
regardless of how many further commits trunk has gained. `resolvePinnedTrunk`
now requires `repository.integration_head_sha` to be a real ancestor of (or
equal to — self-ancestry holds) the live tip of
`refs/heads/<repository.integration_branch>` (`git merge-base --is-ancestor`,
not `!==` on the two resolved SHAs); a pin that is NOT an ancestor at all — a
rewritten or foreign commit, the case that actually indicates the pin is
untrustworthy — still fails closed. No staleness bound is added: freshness
beyond ancestry was never this pin's job, and an arbitrarily-lagging-but-valid
pin cannot mask a missing landing, because `verifyLiveGitIdentities`'s
`accepted_sha` reachability check is split out to resolve against the trunk's
LIVE tip directly (never the pin) — a block landed after the pin was last
recorded is reachable from the live tip even though it is not reachable from
a lagging pin's own history.

Verified against the CURRENT tree (not the prepared-against tree, since this
is a post-landing correction on a still-advancing trunk): the third command
above passes —

```
OK: program-state.toml (docs/arch/architecture-lock/ledger/program-state.toml) — validated 64 blocks (non-zero work asserted) against docs/arch/refactor/rev11/program-dag.toml in mode live
```

— on a tree where `integration_head_sha` (`ad0f15ed0…`) is several commits
behind the live `program/architecture-lock` tip, the exact shape that failed
under round 5's equality check. Two mutation-kill proofs (red under the
mutation, green restored, both against the real tracked files, not a scratch
copy): (1) `integration_head_sha` set to a real but non-ancestor commit (an
orphan `git commit-tree` object, unreachable from trunk by construction)
fails closed with the new "is not an ancestor of the live tip" violation;
restoring the real pin passes. (2) `resolvePinnedTrunk` reverted in place to
the round-5 equality check (`liveTip !== pin`), run against the CURRENT real
ledger (not a fixture), fails with round 5's exact "does not match the live
tip" violation — proving the equality check is genuinely broken on the tree
that exists today, not merely theoretically; restoring the ancestry check
passes. Test suites: 70/70 in `validate-program-state.test.mjs`, 176/176 in
`validate-mutation-suite.test.mjs` (retargeted trunk-pin fixtures at the
ancestry relation — a lagging-but-ancestral pin now PASSES where round 5
required it to fail; a non-ancestor pin still fails closed; two merge-base-
breaking tests isolated with a SHA-scoped shim, since `resolvePinnedTrunk`'s
own unconditional merge-base call now runs before either of their original
call sites), 30/30 in `validate-stack-window.test.mjs`, 276/276 run together,
including the mutation suite's completeness assertion.

## 10. Alternatives considered

1. **A new `IMPLEMENTATION`/`CERTIFYING` status pair, replacing `IN_PROGRESS`/
   `REVIEW`** — rejected: this would be a breaking schema change to a closed
   enum with wide fan-out, for no behavioural gain over re-partitioning the
   EXISTING statuses by concurrency class.
2. **Amend `contracts/stacked-prs.md`'s documented `candidate_sha` meaning to
   explicitly cover WIP mid-implementation identity** — rejected in favour of
   the new `implementation_candidate_sha` field (§5): `stacked-prs.md` is a
   normative delivery contract read program-wide, not scoped to this
   amendment's concern; broadening its documented meaning to accommodate one
   validator's rehearsal need is a disproportionate, higher-authority change
   for a problem an additive field solves cleanly.
3. **Treat `REVIEW` as part of the same single-certifying-block cap as
   `ACCEPTANCE_RECOMMENDED`** — this was the prior (rejected) draft's model.
   Rejected outright: it directly contradicts `contracts/stacked-prs.md:100`'s
   documented `LANDABLE` shape (multiple green `REVIEW` layers, one
   `ACCEPTANCE_RECOMMENDED` layer) and was the core of the review rejection.
4. **Derive a fixed landing order implicitly from DAG topology alone (no new
   `landing_order` field)** — rejected: most concurrently active blocks are
   mutually independent in the DAG (that is what "disjoint blocks" means),
   so topology alone under-determines their relative order; both rulings use
   "declared"/"fixed" order language, implying an actual declaration, not an
   inference. `landing_order` makes the declaration explicit and checkable
   (including consistency against the DAG edges that DO exist — §3).
5. **Model N > 5 (the grok-implementer roster) as a ledger field
   (`implementer_model` per block) with its own cap** — not adopted now: no
   ruling asks the ledger to record this; unchanged from the prior draft's
   reasoning.
6. **Rehearse the full concurrent set with one `git merge-tree --stdin` batch
   call instead of one cumulative walk** — not adopted: `--stdin` batches
   independent pairwise merges, which is precisely the pairwise shape §3
   replaces; it does not express "rehearse against the cumulative result of
   every prior step," which is the actual property needed.
7. **Keep v2's two-parent merge-commit rehearsal, only add `--merge-base`
   to its `git merge-tree` call** (v3, considered and rejected) — rejected:
   the two-parent commit still synthesises candidate ancestry no
   squash/rebase/cherry-pick landing ever produces, and — more importantly —
   the SECOND step onward would still compute merge-base by ancestry search
   against a two-parent synthetic commit rather than replaying cleanly from
   a linear cumulative history; the single-parent commit is required
   end-to-end, not merely at the first step, for `--merge-base` to mean what
   it is meant to mean at every step.
8. **Require `head_sha` to equal live checkout `HEAD` on every live-mode run,
   unconditionally** (v3's first answer, considered and rejected AS
   PROPOSED) — v3 rejected this on evidence that turned out to be measuring
   the wrong thing: the claim was that the CURRENT live ledger's own
   `repository.head_sha` is already several commits behind live `HEAD`, and
   an unconditional freshness requirement would break it outright. That
   evidence was gathered by comparing against checkout `HEAD`, which was
   itself the defect (round 3, Finding G) — the live ledger's pin was never
   actually stale relative to its declared trunk (`main`); only checkout
   `HEAD` (this worktree, on a feature branch) differed. Round 3 adopts an
   UNCONDITIONAL check after all, but against `refs/heads/<repository.branch>`
   rather than `HEAD` — the alternative this bullet rejected was the right
   shape aimed at the wrong ref, not a wrong shape.
9. **Model true train/orchestrator identity (e.g. group by `stack_id`) for
   the ceiling instead of counting raw active blocks** — not adopted now:
   this would answer the tension named in §2/§8 more precisely, but no
   ruling asks for it, and it is a materially larger design surface (a
   `stack_id`-less block is its own train; cross-stack train identity is
   undefined today) than the six-finding fix this draft closes. Recorded as
   a named limitation (§8), not silently accepted as correct.
10. **Bind `implementation_candidate_sha` to a git NOTE or a commit trailer
    on the commit itself, instead of a new ledger field** (round 3,
    considered and rejected) — rejected: a note/trailer requires the WIP
    commit itself to carry self-referential provenance, which is one more
    thing an implementer can forget to update and this validator cannot
    independently discover without first resolving SOME ref to find the
    commit to inspect — begging the question. A ledger-declared
    `implementation_ref`, resolved and pinned-checked the same way
    `repository.integration_branch`/`integration_head_sha` already are
    (round 5: originally `repository.branch`/`head_sha`, before those were
    identified as the immutable entry lock rather than the trunk pin — see
    the round-5 revision-history entry above), reuses the SAME pattern §3
    step 2 already established rather than inventing a second mechanism.

## 11. Ratification

**RATIFIED WITH CORRECTIONS, 2026-08-21.**

The codex architect reviewed round 5 (commit `cd15a31b7a3c087dcca67f105434a823e49c55f1`)
and ratified the design — oracle routing (integration ref →
`integration_head_sha` → reachability and rehearsal), no executable
checkout-`HEAD` oracle remaining, raw-OID/`HEAD` rejection before git calls,
`refs/heads/…` enforcement, unconditional per-block binding, the live
64/0 result, and the exact WIP SHA pin design (resyncing after each commit
preserves a deterministic rehearsal snapshot; an advisory SHA would weaken
that guarantee) — subject to two corrections, neither warranting a further
review round:

1. **The entry lock was mechanically immutable by convention, not by check**
   (see the revision-history entry above and §3 item 2). Closed by
   `verifyEntryLockRecordBinding`, content-binding `repository.branch`/
   `head_sha`/`head_tree`/`entry_checkout_sha`/`entry_checkout_tree` to the
   DAG root's digest-bound `entry-lock.toml` record — a separate file a
   coordinated in-memory ledger edit cannot also rewrite. Proven to kill by
   mutation: neutralizing the new check lets the coordinated five-field
   rewrite fixture wrongly pass; the real validator correctly fails it.
   Applying this against the REAL live ledger's own `A0/entry-lock.toml`
   surfaced a genuine pre-existing gap in the shared minimal TOML reader
   (`scripts/lib/rev11-toml.mjs` could not parse the file's own multi-line
   `open_architecture_changes` array, whose string element contains embedded
   commas) — fixed in that shared reader (multi-line-array join, mirroring
   `validate-performance-gates.mjs`'s own reader; quote-aware comma
   splitting), not worked around locally.
2. **The amendment text and template still described the superseded
   `repository.branch`/`head_sha` oracle and the pre-FIX-2 conditional
   `implementation_ref` binding** (see the revision-history entry above).
   §3 item 2, the affected §8 bullet, and
   `docs/arch/refactor/rev11/templates/program-state.template.toml` (lines
   ~118–146) are corrected to name `integration_branch`/`integration_head_sha`
   as the trunk-pin oracle and to state the `implementation_ref`/
   `implementation_candidate_sha` binding as unconditional, matching the code
   both before and after this correction.

Both corrections are applied in this revision. Verification (re-run after
applying both corrections):

- `node --test scripts/validate-program-state.test.mjs` — 70/70.
- `node --test scripts/validate-mutation-suite.test.mjs` — 172/172 (was
  169/169 before this ratification round; three new tests cover
  `verifyEntryLockRecordBinding`'s three violation sites, including the
  coordinated-mutation discriminator), including the completeness assertion
  that every derived check is tripped by at least one mutation.
- `node --test scripts/validate-stack-window.test.mjs` — 30/30.
- `node --test scripts/validate-performance-gates.test.mjs` — 26/26 (the
  shared reader's sibling — confirms the `rev11-toml.mjs` reader fix did not
  need to, and did not, touch `validate-performance-gates.mjs`'s own separate
  reader).
- `node scripts/validate-program-state.mjs --dag docs/arch/refactor/rev11/program-dag.toml --state docs/arch/architecture-lock/ledger/program-state.toml --mode live`
  — `OK ... validated 64 blocks (non-zero work asserted) ... 0 violations`,
  matching round 5's own result (Correction 1 is a live no-op against the
  real ledger's field VALUES — the live ledger has never carried a
  coordinated rewrite — but exercising it for the first time against the
  real `A0/entry-lock.toml` is what surfaced, and required fixing, the
  multi-line-array gap named above; without that reader fix this command
  FAILED with 1 violation, `... is not valid TOML: unterminated array
  (multi-line arrays unsupported)`, on the identical unmodified ledger).

This amendment now has execution authority. The two amended files —
`scripts/validate-program-state.mjs` and
`docs/arch/refactor/rev11/templates/program-state.template.toml` — are
landed as described in "Amends on ratification" below, together with this
revision's own additions: `verifyEntryLockRecordBinding` plus its three-test
coverage in `scripts/validate-mutation-suite.test.mjs`, and the necessary
multi-line-array/quote-aware-split fix in `scripts/lib/rev11-toml.mjs`.

### Round 6 correction (post-landing, 2026-08-21)

This amendment landed as commit `2e878b6a5` and its own ledger update FAILED
live validation on the very next run: `repository.integration_head_sha`
`does not match the live tip` — the ratified equality pin, applied to a
ledger that is itself committed to the branch it pins, was stale the instant
the landing commit existed. This was not foreseeable at ratification time
without running the validator against the post-landing tree — a check
ratification does not perform — so it is applied here as a correction rather
than reopened as a new design question; the amendment stays RATIFIED.

**Disposition: ADOPT-NOW.** The relation `resolvePinnedTrunk` checks changes
from equality to ancestry (`git merge-base --is-ancestor`,
`repository.integration_head_sha` a real ancestor of the live tip of
`refs/heads/<repository.integration_branch>`, not `!==` on the two resolved
SHAs); `verifyLiveGitIdentities`'s `accepted_sha` reachability check is split
to resolve against the trunk's live tip directly, never the (now possibly-
lagging) pin. No staleness bound is added — see §3 item 2 and §9's round-6
update for the full argument and verification detail. Both mutation-kill
proofs (a non-ancestor pin; the equality check reverted in place) are run
against the real tracked files and the real live ledger, not scratch copies
or fixtures alone. Test suites: 70/70 `validate-program-state.test.mjs`,
276/276 combined with `validate-mutation-suite.test.mjs` and
`validate-stack-window.test.mjs`.

### Round 7 correction (2026-08-21)

Round 6 fixed the ledger's OWN self-reference (`repository.integration_head_sha`,
committed to the branch it pins) by switching that pin from equality to
ancestry. `checkImplementationRefBinding`'s `implementation_candidate_sha`
pin — bound to `implementation_ref`'s live tip, closed under round 3/4 —
still checked EQUALITY, and BV2 (an actively-implementing block whose WIP
branch commits every few minutes) exposed the same defect one level down:
live validation of the real ledger against the real repository FAILED
(`block BV2 implementation_ref ... resolves to <live tip>, but the declared
implementation_candidate_sha is <pin> — the pin does not match the live
ref's current tip`); the prior run, minutes earlier, had reported 0
violations — a green snapshot caught between two of BV2's commits, not a
genuinely stable pass — and every subsequent BV2 commit invalidates the
pin again the same way. Not an intermittent staleness gap: a continuously
failing relation for any block under active implementation.

**Disposition: ADOPT-NOW.** The same reasoning round 6 applied to the trunk
pin applies here, unchanged: `checkImplementationRefBinding` now requires
`implementation_candidate_sha` to be a real ANCESTOR of `implementation_ref`'s
live resolved tip (`git merge-base --is-ancestor`), not `!==` on the two
resolved SHAs. This still proves `implementation_ref` is the real branch
carrying this identity — a foreign or rewritten SHA is not in that branch's
history and still fails closed — while ordinary forward progress on the WIP
branch no longer invalidates a pin recorded against an earlier commit on the
same line of history. No staleness bound is added, for the same reason round
6 added none: an arbitrarily stale-but-ancestor pin only means the
fixed-landing-order rehearsal (`verifyConcurrentLandingSafety`) replays a
smaller slice of the block's total pending delta until the next resync — it
cannot mask a real conflict in content that WAS rehearsed, it only leaves
content produced after the pin temporarily unrehearsed. The ref-only
alternative (drop the SHA check entirely for IN_PROGRESS, verify the exact
SHA only at REVIEW/ACCEPTANCE_RECOMMENDED) was considered and rejected: it
would stop verifying anything about `implementation_candidate_sha` for the
entire IN_PROGRESS lifetime — including a foreign/rewritten pin — strictly
weaker than the ancestor relation for no added benefit; the fixed-landing-
order rehearsal also consumes this pin while the block is still IN_PROGRESS
(round 4, FIX 2), so a check that only starts at certification would leave
the rehearsal's own input unverified.

**What the field now guarantees.** `implementation_candidate_sha` is a real
commit, genuinely reachable by walking `implementation_ref`'s history
backward from its live tip — i.e., a real point the WIP branch has actually
passed through — never a foreign, rewritten, or fabricated identity. It does
NOT guarantee currency: the pin may lag the branch's actual current tip by
any number of commits, and the validator does not bound how far. Freshness
("is this pin recent") is deliberately out of scope, same as round 6's trunk
pin — see that correction's own rationale, which applies here unchanged.

Verification (re-run after applying):

- `node --test scripts/validate-program-state.test.mjs` — 70/70.
- `node --test scripts/validate-mutation-suite.test.mjs` — 178/178 (176
  before this correction, +2 net: one existing test retargeted in place
  from the equality-mismatch case to the foreign-SHA-fails-closed case, plus
  two new tests — the genuine-ancestor-lag-passes case and the
  `checkImplementationRefBinding` merge-base subprocess-failure
  discriminator).
- `node --test scripts/validate-stack-window.test.mjs` — 30/30.
- Live validation against the real ledger and the real, currently-advancing
  `block/bv2` branch: `OK ... validated 64 blocks (non-zero work asserted)
  ... 0 violations` on three separate runs (11:28:17, 11:33:37, 11:36:21
  local time) spanning ~8 minutes — `block/bv2`'s live tip did not itself
  advance again in that window, but the pinned `implementation_candidate_sha`
  (recorded 10:20:11, one commit behind the branch's 11:18:29 tip) stayed a
  valid ancestor throughout, which is the exact lagging-pin shape this
  correction exists for; it would have failed all three runs under the
  pre-round-7 equality check (see the mutation-kill proof below).
- Mutation-kill, against the real tracked `scripts/validate-program-state.mjs`
  and the real live ledger (not a scratch copy): (a) BV2's
  `implementation_candidate_sha` replaced with a foreign commit not on
  `block/bv2` — FAILS with `is not an ancestor of implementation_ref`; (b)
  the ancestry check reverted in place to the pre-round-7 equality check —
  FAILS against the live ledger with the exact "the pin does not match the
  live ref's current tip" symptom that motivated this correction.
