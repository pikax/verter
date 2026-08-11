# A0 Exact-Candidate Record — Revision 11 documentation/tooling landing

This is the **exact-candidate record** for block A0, in the
`contracts/agent-orchestration.md` §9 bounded format. It lives EXTERNALLY (not in the
candidate tree) precisely so that recording candidate identity and review verdicts
does not mutate the candidate: every prior round stored this record IN the tree, so
each fix minted a new candidate and invalidated the reviews that had just run. The
committed counterpart, `docs/arch/refactor/rev11/evidence/A0-summary.md`, is a
stable, identity-free description only. This file's SHA-256 is recorded as
`block.A0.evidence_digest` in `../program-state.toml` (input rule documented at that
field).

```text
BLOCK: A0
STATE: ACCEPTED
       (matches block.A0.status in ../program-state.toml — the ledger is the status
       authority and this record uses the same vocabulary. The three-mandate
       recheck ran against this exact candidate SHA/tree and all three mandates
       returned PASS — see round 7 under REVIEWS. Verdicts are recorded HERE and
       in the external ledger — outside the candidate tree — so the recheck binds
       to this exact SHA/tree without a fixpoint. Maintainer acceptance was
       recorded under delegation — see MAINTAINER_DECISION below.)
BASE: 9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0 / tree 3cf111cf5665586b7d8fdfd520f01cfee3bf8108
CANDIDATE: b7ea2dc88bda86473de81de3438b7f88ef30adc7 / tree 47645406a9246e600af995c62608b709347e13a4
           (single squashed commit on branch docs/rev11-architecture-plan, parented
           on BASE; the three-mandate recheck ran against this candidate — 3/3 PASS)
ACCEPTED_TARGET: b7ea2dc88bda86473de81de3438b7f88ef30adc7 / tree 47645406a9246e600af995c62608b709347e13a4
                 (EQUAL to the candidate identity — the landing is a fast-forward
                 with no rebase, so no landing-equivalence artifact is required;
                 the validator's divergence gate confirms this: accepted ==
                 candidate, landing_equivalence_digest legitimately empty)
LANDING_EQUIVALENCE: none (single-commit documentation/tooling landing; no stack)
CHARTER_DIGEST: 68c2140d3be29de0b8737771aa80d30c17be7cf55aa249a7cfaa3b47f384cd21
                (SHA-256 of the landed charters/A0.md — the digest of a
                reconstruction, not of a canonical package file; ruling R-2)
CONTEXT_PACKET_DIGEST: d70534bd934f042c752439838dca6ec11076c2f1219199df763aa548ec07b75f
                       (SHA-256 of A0/context-packet.md in this evidence root; also
                       recorded as block.A0.context_packet_digest in the ledger. The
                       packet records the literal SHA-256 of the ledger state it was
                       built from. Re-digested after the acceptance-round correction
                       of the packet's stale §7 expected-work counts to the real
                       round-6 values: node --test 26 tests, stub experiment 26
                       tests, falsification battery 15 mutated ledgers.)
STACK: none
CHANGES: adds docs/arch/refactor/rev11/ — the 67-file reconstructed authority
         package (plan, contracts, charters, ADRs, templates, baseline), the
         consolidated canonical master, the three release artifacts, PROVENANCE.md
         (aggregate digest 80dc835a080f258d85101a15c580e19a685c0984e1cce6f90666c162b9a4817e
         over 70 files, algorithm and input set stated in the file), the README.md
         index, _EXTRACTION_INDEX.md, evidence/ (A0-summary.md — identity-free,
         A0-preflight-blocked.md — historical, maintainer-rulings.md — the eight
         rulings R-1…R-8 plus the amendment registry), and amendments/AMD-001
         (stack-window-validator prerequisite: the program-state validator's
         fail-closed rejection of begun successors of a PRIVATE_CHECKPOINT
         predecessor vs the canonical D1 -> D2 atomic path; A6 must deliver the
         Node stack-window validator, composite cross-validation, CI wiring, and a
         discriminating D1/D2 transition test before any post-A6 stacked delivery;
         the refusal is superseded, never deleted; and — clause 4 — the A6 context
         packet and Implementation Lock evidence must NAME AMD-001 and bind its
         file SHA-256, so the prerequisite is mechanically traceable). Adds the
         Node program-state
         validator scripts/validate-program-state.mjs (strict TOML reader with loud
         unbalanced-quote and leading-zero-integer failure; sequencing invariant
         with READY and PRIVATE_CHECKPOINT as begun statuses and an
         ESTABLISHED-only stacked-work exception — snapshot digest shared by every
         unaccepted predecessor, same non-whitespace stack_id, begun predecessor
         status, strictly lower predecessor stack_layer; status-dependent gates
         for REVIEW/ACCEPTANCE_RECOMMENDED/ACCEPTED/PRIVATE_CHECKPOINT — identity
         fields, mandate PASS with NOT_REQUIRED permitted only for
         architecture_review on subsystem-class DAG blocks, maintainer acceptance,
         accepted_sha/tree, landing_equivalence_digest required when the accepted
         identity diverges from the candidate identity; the DAG's single root
         block — derived structurally from predecessors = [], never a hardcoded
         block name — additionally requires a well-formed entry_lock_digest at
         REVIEW/ACCEPTANCE_RECOMMENDED/ACCEPTED, composing with the
         zero/multi-root violation rather than crashing; PRIVATE_CHECKPOINT
         additionally class-bound to foundational-private-checkpoint DAG blocks,
         identity/evidence-bound with all three mandates PASS, and deliberately
         NOT requiring accepted_sha/tree or maintainer acceptance — a checkpoint
         never lands independently; live-mode requirements status = "ACTIVE" and
         non-empty program_dag_digest, verified against the DAG file; fail-closed
         rejection of unmodelled PRIVATE_CHECKPOINT-predecessor (see AMD-001) and
         opened-conditional-predecessor paths; recorded-intent comments for the
         BLOCKED/RESCOPE_REQUIRED non-begun choice and the single-IN_PROGRESS vs
         max_active_workers/stacked-prs tension, with debt rows in
         evidence/A0-summary.md — content-unverified evidence digests plus the
         missing maintainer_decision backstop on PRIVATE_CHECKPOINT, and the
         un-pinned foundational-private-checkpoint class census) and
         its node --test suite (26 tests, including positive proven-checkpoint,
         legal same-snapshot stacking, subsystem NOT_REQUIRED, and bound
         entry-lock fixtures; every
         test fails against a process.exit(0) stub). Wires the suite into
         package.json test:scripts (the authoritative runner, executed by the CI
         js-build-test job) and names scripts/validate-program-state.mjs +
         scripts/validate-program-state.test.mjs in the CI `js` change-detection
         path filter in .github/workflows/ci.yml, so a change touching ONLY the
         validator or its tests still triggers js-build-test (without the filter
         entries such a change would evaluate every filter group false, both
         candidate jobs would skip, and ci-success counts skips as pass). The
         path-filter entries are the single authorized .github edit (ruling R-7 as
         amended). Adds an eight-line single-file vitest exclude (a seven-line
         comment plus the one entry) for the node:test suite (root vitest
         collection fails on node:test files; a broad scripts/** exclude would
         change collection for the 10 pre-existing test/spec files under
         scripts/ — 5 vitest, 5 node:test).
DELETIONS: none required; none performed.
EVIDENCE: committed — evidence/A0-preflight-blocked.md (historical entry inspection
          at the BASE SHA), evidence/maintainer-rulings.md (the eight maintainer
          rulings R-1…R-8 + amendment registry), PROVENANCE.md (per-artifact
          digests + the recomputable aggregate digest). External (this evidence
          root) — program-state.toml (live ledger, ruling R-6), A0/entry-lock.toml
          (the contracts/baseline-lock.md §2 entry-lock record: A0-knowable fields
          filled — repository, entry SHA/tree, short SHA, dirty, untracked,
          submodules, open architecture changes, lockfile digests, toolchain;
          implementation_baseline_* and [verification] left to A6, stated inline;
          SHA-256 recorded as block.A0.entry_lock_digest), A0/context-packet.md,
          A0/branch-screen.md (baseline-lock §3 screen: 572 unmerged local
          branches, method + honest not-individually-screened statement),
          A0/command-proofs/ (raw output of every §7 required command, one file
          each, plus index.md recording the baseline-lock §4 seven fields per
          command), A0/historical/ (the pre-candidate records, banner-marked
          HISTORICAL).
          Non-vacuous counts: 67/67 authority files fidelity-verified verbatim (in
          earlier rounds; the round-5 and round-6 tracked edits touch only
          PROVENANCE.md, amendments/AMD-001, evidence/A0-summary.md, and the two
          scripts/ files — all outside the verbatim set, so the 67/67 attestation
          is unaffected);
          guards 5 + 12 + 6 tests executed, all pass;
          validator suite 26/26 pass; stub experiment 26/26 FAIL against the stub;
          template validation 50 blocks OK; live validation 50 blocks OK (run
          after this round's ledger updates); falsification battery 15/15 REJECT;
          tracked-tree machine-path grep: zero real machine paths and zero
          occurrences of the external evidence-root name in the landed tree.
REVIEWS: round 1 — four legs against pre-fix candidate
         d08d8334370527e45c240969d8cd85a2219cf8f5 / tree 27ede4664d3c2677199744091b72926f90775d7b:
         Opus conformance FAIL -> fixed; Opus adversarial NOT PROVEN -> fixed;
         codex architecture BLOCKING -> in-scope fixed, scope-deviating items
         dispositioned by maintainer (maintainer-rulings.md); grok fidelity PASS.
         round 2 — two legs against squashed candidate
         149346ea16d08971980206341e8965066511db4f / tree f962f1b73de9811789a69c46bd969568347aad4d:
         Opus adversarial BLOCKING -> fixed; codex architecture BLOCKING -> fixed.
         round 3 — three mandates against candidate
         058b0d18a88b781c8c1573c6f3d38d09505eb642 / tree 51e257aaed76f47012877d92b7e726bc7baa95a0:
         ALL THREE FAILED (validator bypass/gating defects, in-tree exact-candidate
         record contradiction, unwired test suite, evidence-hygiene findings) ->
         findings fixed in candidate 3be3976af88d112ce7ac3cda0aa1fa42d5239409 /
         tree dbbe09bc0de727e34cd86f673d2cea3bf21500aa.
         round 4 — two legs (Opus adversarial, codex architecture) plus
         conformance converged on that candidate with a reproduced-falsification
         defect list: validator semantics (READY not a begun status; NOT_REQUIRED
         accepted on foundational-class mandates; stacked exception ignoring the
         cited snapshot/predecessor status; blank program_dag_digest disabling the
         DAG binding; live mode not requiring ACTIVE; ACCEPTED divergence without
         landing equivalence; TOML leading-zero/whitespace-stack_id laxity), CI
         gating (a validator-only change evaluated every paths-filter group false,
         so no candidate job ran the suite — the previous fix had relocated, not
         fixed, this defect), the D1/D2 fail-closed trap (no block owned
         delivering the stack-window model the validator's refusal presumes —
         now AMD-001), and evidence corrections (entry-lock record missing;
         STATE/status vocabulary mismatch; bypass (c) violation count overstated)
         -> ALL fixed in candidate
         b541e8c59a48dca9205551b79dead2ac9cebee18 / 71503f7f785507c5294ea2490d44e9af0a675735.
         round 5 — three legs (conformance, architecture, adversarial) against
         b541e8c59a48dca9205551b79dead2ac9cebee18: the adversarial and
         architecture legs independently reproduced ONE remaining blocker —
         PRIVATE_CHECKPOINT was absent from the validator's begun-status and
         evidence-bound sets, so a live ledger with D1 (or L4) in
         PRIVATE_CHECKPOINT over LOCKED predecessors validated green (exit 0,
         "validated 50 blocks") — plus small corrections (stale external context
         packet still authorizing the reverted CI guard-step edit and requiring
         "8 tests"; the "five-line" vitest-exclude miscount — it is 8 lines;
         AMD-001 lacking a mechanical-traceability clause; unrecorded validator
         limits). One conformance finding was itself WRONG and was NOT applied:
         the claim that the two `js` path-filter lines in ci.yml are a no-op is
         false under dorny/paths-filter semantics (the default `some` quantifier
         needs at least one non-negated pattern match; a validator-only change
         matches none of the `js` group's positive patterns at base), so the
         filter lines are load-bearing and their evidence sentences stand
         unchanged. -> blocker + corrections fixed in THIS candidate
         (b83150f4d46dc2b491d9fb65a10c42d47e42bfd9 / 1191b03c1318d1c6344c27981bc08ce4f68b9c58):
         PRIVATE_CHECKPOINT added to BEGUN_STATUSES (not to the stacked-exception
         set), class-bound to foundational-private-checkpoint DAG blocks,
         identity/evidence-bound with all three mandates PASS (no accepted
         identity or maintainer acceptance required), with four new checkpoint
         tests plus five promoted falsification cases in the committed suite.
         round 6 — the architecture leg against b83150f4d46dc2b491d9fb65a10c42d47e42bfd9
         held ONE blocker (entry_lock_digest presence/shape-checked only: no
         status gate required it, so a ledger could carry the entry block
         through REVIEW -> ACCEPTANCE_RECOMMENDED -> ACCEPTED with the field
         absent/emptied — emptying it, deleting the line, and a
         wrong-but-well-formed digest all passed) plus small items: a hollow
         sole-defence coverage gap (0 of 22 tests caught neutralising the
         fail-closed PRIVATE_CHECKPOINT-predecessor check once
         PRIVATE_CHECKPOINT joined BEGUN_STATUSES — the stack-establishment
         path then accepts a checkpoint predecessor as begun, exactly the
         AMD-001 D1/D2 interaction), a stale AMD-001 begun-status list, an
         acceptance-specific diagnostic on non-acceptance stacked statuses, the
         entry-lock remote recorded with a .git suffix the configured remote
         does not carry, and a mis-described falsification case 5.2. -> ALL
         fixed in THIS candidate
         (b7ea2dc88bda86473de81de3438b7f88ef30adc7 / 47645406a9246e600af995c62608b709347e13a4):
         the validator requires a well-formed entry_lock_digest on the DAG's
         single root block (derived structurally, composing with the
         zero/multi-root violation) at REVIEW/ACCEPTANCE_RECOMMENDED/ACCEPTED;
         four new tests (root ACCEPTED with emptied AND line-deleted digest
         rejected; root REVIEW rejected empty and passing bound; stackless and
         perfect-stack REVIEW successors over a PRIVATE_CHECKPOINT predecessor
         rejected with the fail-closed message — mutation-verified: neutralising
         the check fails exactly those two tests, the perfect-stack state
         otherwise validating green); the stale shape-only debt row replaced by
         the two remaining-debt rows; AMD-001 begun-status list synchronised;
         the ineligible-status stacked diagnostic corrected; the entry-lock
         remote made literal (re-digested, ledger updated); case 5.2's
         description corrected to lead with the sequencing violation.
         round 7 — the three-mandate recheck against THIS exact candidate
         (b7ea2dc88bda86473de81de3438b7f88ef30adc7 / 47645406a9246e600af995c62608b709347e13a4):
         conformance mandate PASS; architecture mandate PASS; adversarial
         mandate PASS. All remaining findings were classified DEBT (carried in
         the DEBT section below and in the ledger notes); no blocker remained.
         Ledger mandate fields set to PASS; A0 advanced to
         ACCEPTANCE_RECOMMENDED, then ACCEPTED on the recorded maintainer
         decision (made by the architecture authority under explicit delegation
         from the designated maintainer, Carlos Rodrigues / GitHub pikax, and
         returned PASS with no blockers).
DISCOVERIES: D-1 (main persistently red at the entry SHA) and D-2 (evidence_root
             template/contract inconsistency) remain recorded in
             evidence/A0-preflight-blocked.md; dispositions unchanged.
NEXT_LEGAL_BLOCKS: A1 (program-dag.toml: A1.predecessors = ["A0"], now satisfied by
                   A0 acceptance). A1 set READY in the ledger — the template's
                   not-started-but-legal value; no A1 work has started.
MAINTAINER_DECISION_REQUIRED: satisfied — maintainer_decision = ACCEPTED recorded in
                              the ledger. The designated maintainer (Carlos
                              Rodrigues, GitHub pikax) delegated the acceptance
                              decision to the architecture authority, which
                              returned PASS with no blockers; the decision is
                              recorded under that delegation.
```

## Verification (this candidate)

Raw outputs and the per-command seven-field index (exact command, working directory,
environment/features, exit code, executed count, skipped/ignored count, exact
binaries/packages/fixtures, raw-output digest) are in `command-proofs/`. Summary:

- `tracked_paths_no_machine_roots` — 5 tests run: 5 passed.
- `tracked_paths_are_portable` — 12 tests run: 12 passed.
- `analysis_config_paths_never_committed` — 6 tests run: 6 passed.
- `node --test scripts/validate-program-state.test.mjs` — 26 pass / 0 fail (the
  round-5 twenty-two plus four round-6 tests: root-block entry-lock digest
  emptied/line-deleted at ACCEPTED rejected; root REVIEW rejected empty and
  passing with a bound digest; stackless and otherwise-perfect-stack REVIEW
  successors over a PRIVATE_CHECKPOINT predecessor rejected with the
  fail-closed stack-window message — the stacked variant is the discriminating
  mutation cover for the sole-defence check).
- validator `--mode template` — OK, 50 blocks.
- validator `--mode live` against the ledger — OK, 50 blocks (run after the ledger
  updates of this round; raw output in `command-proofs/`).
- stub experiment — suite copied beside a `process.exit(0)` validator stub: 26/26
  tests FAIL, proving no test can pass against an always-green validator.
- falsification battery (`command-proofs/08-falsification-rejections.txt`) —
  plant-verified mutations of the live ledger, 15/15 REJECT: (1.1) stackless
  `READY` successor of an unaccepted predecessor ->
  "block A1 is READY but direct predecessor(s) not ACCEPTED: [A0] (no stack_id...)";
  (1.2) foundational-class ACCEPTED on three NOT_REQUIRED mandates -> three
  "NOT_REQUIRED ... DAG class \"foundational\" does not permit it" violations;
  (1.3a) predecessor citing a DIFFERENT stack_snapshot_digest -> "not the same
  well-formed snapshot digest"; (1.3b) ABORTED predecessor and (1.3c) LOCKED
  predecessor inside a claimed stack -> "a predecessor that has not begun (or has
  terminated) cannot be a lower layer of the same validated stack snapshot";
  (1.3d) predecessor at an EQUAL stack_layer -> "stack_layer 1 is not below block
  A1 stack_layer 1"; (1.4) blank `program_dag_digest` in live mode -> "silently
  disables the ledger-to-DAG binding"; (1.5) live status "PAUSED" -> "requires
  the live ledger to carry status = \"ACTIVE\""; (1.6) ACCEPTED with a diverged
  accepted identity and no landing_equivalence_digest -> "a differing accepted
  identity is legal only with a repository-validated landing-equivalence
  artifact"; and the round-5 blocker reproductions, previously ALL exit 0:
  (5.1) D1 PRIVATE_CHECKPOINT (fully proven) over LOCKED predecessors ->
  "block D1 is PRIVATE_CHECKPOINT but direct predecessor(s) not ACCEPTED:
  [A3, B1, B2, C1]"; (5.2) D1 PRIVATE_CHECKPOINT unproven -> ten violations
  led by the sequencing violation ("block D1 is PRIVATE_CHECKPOINT but direct
  predecessor(s) not ACCEPTED: [A3, B1, B2, C1]"), the identity/mandate
  violations following ("state block D1 is PRIVATE_CHECKPOINT but candidate_sha
  is not a non-empty 40-char lowercase git object id" among them); (5.3) L4
  PRIVATE_CHECKPOINT ->
  "state block L4 is PRIVATE_CHECKPOINT but its DAG class is
  \"foundational-final\" — the PRIVATE_CHECKPOINT status is permitted only for a
  block whose DAG class is \"foundational-private-checkpoint\"".
  Round-6 additions: (6.1a) fully-accepted root A0 with entry_lock_digest
  EMPTIED and (6.1b) with the entry_lock_digest line DELETED -> "state block A0
  is ACCEPTED but entry_lock_digest \"\" is not a non-empty 64-char lowercase
  SHA-256 — A0 is the DAG's entry (root) block and its entry-lock record
  (contracts/baseline-lock.md §2; the entry charter's first required-evidence
  item) must be digest-bound before review, acceptance recommendation, or
  acceptance"; (6.2) D2 REVIEW with otherwise-perfect stack fields (same
  stack_id, identical well-formed snapshot digest, strictly lower checkpoint
  layer) over a proven D1 PRIVATE_CHECKPOINT -> "block D2 is REVIEW with
  predecessor D1 in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor
  satisfies sequencing only inside a validated stack window for the final
  acceptance block (contracts/stacked-prs.md), which this validator does not
  model — fail closed".
- historical round-3 bypass re-tests remain rejecting: (a) bare `stack_id`
  stacked-work exception -> "contingent stacked-work exception is REJECTED";
  (b) ACCEPTED with PENDING mandates/empty accepted identity -> six violations;
  (c) ACCEPTANCE_RECOMMENDED with a BLOCKING adversarial mandate -> ONE violation
  (the adversarial_review mandate line; the earlier claim of "three violations"
  was wrong — the other two fields PASS and the identity fields are populated, so
  exactly one check fires); (d) `""#REQUIRED_..."` quote-comment -> loud
  unparseable-TOML failure. A fifth probe: substituting 64 `a`s for
  `program_dag_digest` -> digest-mismatch violation.

## Three-mandate recheck verdicts (round 7 — the acceptance basis)

All three Foundational mandates were rechecked against candidate
`b7ea2dc88bda86473de81de3438b7f88ef30adc7` / tree
`47645406a9246e600af995c62608b709347e13a4`:

- conformance mandate — **PASS** (`conformance_review = "PASS"` in the ledger).
- architecture mandate — **PASS** (`architecture_review = "PASS"` in the ledger).
- adversarial mandate — **PASS** (`adversarial_review = "PASS"` in the ledger).

All remaining findings were classified **DEBT** (below); none was a blocker.

## DEBT — open items carried forward (for the inheriting agent)

1. Evidence digests are presence/shape-checked only: a wrong-but-well-formed
   digest passes (only `program_dag_digest` is content-recomputed against the
   file it names). Durable fix: an artifact-binding manifest (block, digest
   field, custody root, relative path) with live hashing of the referenced
   bytes. Owner: A6, before post-Gate-0 cutovers.
2. No cardinality pin for the `foundational-private-checkpoint` DAG class; a
   second DAG row carrying that class would authorise another checkpoint.
   Owner: A6/AMD-001, before the D1/D2 path is reachable.
3. The entry-lock gate covers `REVIEW`/`ACCEPTANCE_RECOMMENDED`/`ACCEPTED` but
   not `PRIVATE_CHECKPOINT`, the fourth evidence-bound status.
4. Two in-tree debt rows in `evidence/A0-summary.md` are slightly imprecise:
   one says a wrong `stack_snapshot_digest` "passes every gate" (a value
   diverging across the stack's rows is in fact caught by the cross-row
   equality check — only a uniformly wrong value passes); the other calls
   `PRIVATE_CHECKPOINT` "the one evidence-bound status with no
   `maintainer_decision` backstop" when `REVIEW` and `ACCEPTANCE_RECOMMENDED`
   also lack one. Both are tracked files, so correcting them would mint a new
   candidate; carried as debt into A1.
5. The entry-lock test matrix covers `REVIEW` and `ACCEPTED` but not
   `ACCEPTANCE_RECOMMENDED`, and has no renamed-root fixture.
