# A0 Summary — What This Landing Contains and How It Is Governed

This is a **stable, identity-free description** of the A0 landing: what the
`docs/arch/refactor/rev11/` directory and its companion tooling contain, and where the
program's live state and exact-candidate evidence live. It deliberately records **no
candidate identity and no review outcome** — a committed file that names its own
commit or depends on a review verdict invalidates itself on every fix round. The
exact-candidate record (the `contracts/agent-orchestration.md` §9 bounded record,
with the final SHA/tree, the three mandate verdicts, and the evidence digests) lives
EXTERNALLY at `<EXTERNAL_EVIDENCE_ROOT>/A0/A0-exact-candidate-record.md` and is
addressed by digest in the external ledger's `block.A0.evidence_digest`. Live program
state is the external ledger `<EXTERNAL_EVIDENCE_ROOT>/program-state.toml` (maintainer
ruling R-6 — see [`maintainer-rulings.md`](maintainer-rulings.md)); the
`contracts/baseline-lock.md` §2 entry-lock record lives at
`<EXTERNAL_EVIDENCE_ROOT>/A0/entry-lock.toml` (A0-knowable fields filled; the
`implementation_baseline_*` and `[verification]` fields are A6-owned and deliberately
unfilled), addressed by digest in the ledger's `block.A0.entry_lock_digest`. This
directory alone is not sufficient to resume the program.

This summary supersedes the historical pre-candidate record
[`A0-preflight-blocked.md`](A0-preflight-blocked.md) as the current committed A0
description.

## What the A0 landing contains

- `docs/arch/refactor/rev11/` — the 67-file reconstructed authority package (plan,
  contracts, charters, ADRs, templates, baseline), the consolidated canonical master,
  the three release artifacts, `PROVENANCE.md` (per-artifact digests plus the
  recomputable aggregate digest, algorithm and input set stated there), the
  `README.md` index, `_EXTRACTION_INDEX.md`, and the `evidence/` records (this
  summary, the historical preflight record, and the eight maintainer rulings
  R-1…R-8 in [`maintainer-rulings.md`](maintainer-rulings.md)).
- `docs/arch/refactor/rev11/amendments/` — registered plan amendments (currently
  `AMD-001`, the stack-window-validator prerequisite for the canonical
  `D1 PRIVATE_CHECKPOINT -> D2` path); amendments record plan deltas WITHOUT editing
  the verbatim-reconstructed authority files (see `PROVENANCE.md`).
- `scripts/validate-program-state.mjs` — the Node program-state validator
  (maintainer ruling R-4: the plan's Python validators are reimplemented in Node).
  It validates the DAG/state pair in `template` and `live` modes, enforces the
  governance sequencing invariant (with `READY` as a begun status) including the
  establishment conditions of the contingent stacked-work exception (bound snapshot
  digest shared by every unaccepted predecessor, same stack, begun predecessor,
  strictly lower layer), the status-dependent identity/review gates for
  `REVIEW`/`ACCEPTANCE_RECOMMENDED`/`ACCEPTED` (including the DAG-class gate on
  `NOT_REQUIRED` mandates, the landing-equivalence requirement for a diverged
  accepted identity, and the entry-lock binding — the DAG's single root block,
  derived structurally from `predecessors = []`, must carry a well-formed
  `entry_lock_digest` at each of those three statuses; a zero/multi-root DAG is
  its own reported violation and the gate composes with it), the
  `PRIVATE_CHECKPOINT` gates (a begun status for
  sequencing; permitted only on a `foundational-private-checkpoint`-class DAG
  block; identity/evidence-bound with all three mandates `PASS`; no accepted
  identity or maintainer acceptance required — a checkpoint never lands
  independently), the live-mode `status = "ACTIVE"` and non-empty
  `program_dag_digest` requirements with `program_dag_digest` verification, and
  fails closed on paths it does not model (`PRIVATE_CHECKPOINT` predecessors —
  see `AMD-001` — and opened conditional predecessors).
- `scripts/validate-program-state.test.mjs` — its `node --test` suite: positive
  template/live fixtures plus discriminating negative controls. Every test fails
  against a validator stubbed to `process.exit(0)`.
- Gate wiring — `pnpm run test:scripts` in `package.json` is the authoritative
  runner (it runs `node --test scripts/validate-program-state.test.mjs`); CI
  executes it in the `js-build-test` job, and the validator plus its suite are
  named in the CI `js` change-detection path filter so a change touching ONLY the
  validator or its tests still triggers that job (without the filter entries, a
  validator-only PR would skip `js-build-test` and `ci-success` counts skips as
  pass). The path-filter entries are the single `.github/` edit of the landing,
  maintainer-authorized (see ruling R-7).
- `vitest.config.ts` — an eight-line addition to the root exclude list (a
  seven-line comment plus the one entry) naming the
  single file `scripts/validate-program-state.test.mjs`, because root vitest
  collection FAILS on a node:test file ("No test suite found in file"). It is
  scoped to the one file on purpose: a broad `scripts/**` exclude would change root
  collection for the 10 pre-existing test/spec files under `scripts/` (5 vitest
  suites, plus 5 node:test files that fail root collection the same way), whereas
  the single-file exclude leaves all of them collected exactly as before.

No production source and no runtime behavior is touched.

## Recorded validator limits (debt)

Deliberate, documented limits of `scripts/validate-program-state.mjs` — each is
recorded at its check site in the source and here as debt, not silently absent:

- **Single-`IN_PROGRESS` tension.** The validator enforces the strict
  `ORCHESTRATOR.md:15` reading (one bounded block at a time), while the ledger's
  `[orchestration]` table declares `max_active_workers = 3` and
  `contracts/stacked-prs.md:39` allows an upper stack layer to be `IN_PROGRESS`
  over an unaccepted lower one. Fail-closed by choice; a future stacked/parallel
  regime must relax the check under review alongside the A6-owned stack-window
  model (`AMD-001`), not ad hoc.
- **`BLOCKED`/`RESCOPE_REQUIRED` are not begun statuses.** They bypass the
  sequencing gate by recorded intent (they are paused states reached FROM begun
  work; the exit back into any begun status re-enters the full gate). A block
  minted directly into either status without ever legally beginning is not
  caught — accepted limit, recorded at the `BEGUN_STATUSES` site.
- **Evidence-bound digests are presence/shape-checked only.** Apart from
  `program_dag_digest` (recomputed against the DAG file on every run), NO digest
  field is content-verified: a well-formed but WRONG `evidence_digest` (or
  `charter_digest`, `context_packet_digest`, `entry_lock_digest`,
  `stack_snapshot_digest`, `landing_equivalence_digest`) passes every gate —
  the validator proves a binding was recorded, not that it binds the right
  bytes. And `PRIVATE_CHECKPOINT` is the one evidence-bound status with no
  `maintainer_decision` backstop (deliberately — a checkpoint never lands
  independently), so a forged checkpoint row is caught only by the class gate
  and the shape/mandate gates, never by a recorded maintainer decision.
- **The `PRIVATE_CHECKPOINT` class gate trusts a self-asserted DAG column with
  no count pin.** The gate admits the status for any block whose DAG row says
  `class = "foundational-private-checkpoint"`; the validator does not pin how
  many rows may carry that class, so a second DAG row claiming it would
  legalise a second private checkpoint without tripping any check (the DAG
  digest binding only detects that the DAG changed if the ledger's
  `program_dag_digest` is left stale, not that the class census grew).

## Provenance and attestation

`PROVENANCE.md` states exactly what is and is not attested: the consolidated master
and the release artifacts are digest-pinned; package validation is WAIVED (ruling
R-2 — the Revision 11 ZIP was never available); the split tree is a verbatim
reconstruction from the digest-verified consolidated master, not the canonical
85-file package.

## Which repository guards can see this content

Honest guard coverage for a documentation-plus-tooling landing — these are the
guards whose scan surface includes this content (their per-run results belong to the
external command proofs, not to this file):

- `tracked_paths_are_portable` (`crates/verter_session/tests/cases/tracked_paths_are_portable.rs`)
  — enumerates `git ls-files` and enforces portable path shapes.
- `tracked_paths_no_machine_roots` (`crates/verter_session/tests/cases/tracked_paths_no_machine_roots.rs`)
  — fixed-marker scan of tracked file bytes for machine/user/session absolute-path
  roots. Its `MACHINE_MARKERS` list is a fixed 64-marker list which does NOT cover
  the Downloads/AppData class of paths.
- `analysis_config_paths_never_committed`
  (`crates/verter_analysis_inputs/tests/cases/analysis_config_paths_never_committed.rs`
  — note: it lives in `verter_analysis_inputs`, not `verter_session`).
- `node --test scripts/validate-program-state.test.mjs` — the validator's own suite.

Guards that CANNOT see this content and are therefore **not** claimed as coverage:

- `external_corpus_paths_not_present_outside_gated_tests` — scans only `.rs` test
  files, not Markdown.
- `no_phase_archaeology_in_production_code` — scans `crates/*/src/**` only.
- the CRITICAL-rule meta-guard (`every_critical_rule_in_docs_has_registered_guard`) —
  reads only `CLAUDE.md` and `.claude/skills/*/SKILL.md`.
