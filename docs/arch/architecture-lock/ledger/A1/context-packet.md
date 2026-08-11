# Verter Revision 11 Worker Context Packet — A1 (round 2)

**Packet digest:** (SHA-256 of this file; recorded in the ledger by the orchestrator — a file cannot contain its own digest)
**Created from program-state digest:** `e5579710764c9e1d631969e9c7d7fd1c5d1b9bcdd35dd7d32b4e72f8396b28e6` (SHA-256 of `<EVIDENCE_ROOT>/program-state.toml` at packet regeneration: A1 `IN_PROGRESS` with the round-2 candidate identity recorded, A0 `ACCEPTED`)
**Role:** Implementor (round 2 — fix round for the three-mandate BLOCK findings)
**Block / charter:** A1 — Prove non-vacuous commands and capability truth (`docs/arch/refactor/rev11/charters/A1.md`)
**Stack window / StackSnapshotId / layer_id / acceptance block:** none (single-branch evidence block, no stack)
**Writable worktree / branch:** dedicated A1 worktree beside the main checkout, branch `block/a1-command-truth` (the primary checkout is never touched)
**Maintainer:** Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax)
**Orchestrator:** Claude Opus 5 main session (Claude Code)

# 1. Exact identities

- authority package digest: `""` (EMPTY — package validation WAIVED by maintainer ruling R-2, recorded in A0)
- A6 Implementation Lock digest or `PRE-A6`: `PRE-A6`
- entry checkout SHA/tree: `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` / `3cf111cf5665586b7d8fdfd520f01cfee3bf8108`
- implementation baseline SHA/tree or `UNSET`: `UNSET` (locked only at A6)
- block base SHA/tree: `b7ea2dc88bda86473de81de3438b7f88ef30adc7` / `47645406a9246e600af995c62608b709347e13a4`
- current candidate SHA/tree: `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83` / `a992bb87382e58d6ec846c7be37cbb941ee0b1b2` — the FINAL candidate: the single tracked change was committed FIRST and every command/sentinel run in this round's evidence ran against exactly this SHA/tree (round-2 ordering correction; the ledger carries the same identity)
- charter digest: `b92ef37570b804d170aac6877cd41299e236a7dcb237a6c1d50e76a7748f6d4c` (SHA-256 of `docs/arch/refactor/rev11/charters/A1.md` at the block base; unchanged at the candidate)
- relevant predecessor accepted SHAs/trees/evidence digests: A0 ACCEPTED at `b7ea2dc88bda86473de81de3438b7f88ef30adc7` / `47645406a9246e600af995c62608b709347e13a4`; A0 evidence digest recorded as `block.A0.evidence_digest` in the ledger

# 2. Assigned objective

Prove that the canonical Rust, TypeScript, NAPI, WASM, corpus, provider and conformance
commands of the repository execute their intended targets and non-zero work
(`program.md` §A1); complete the affected capability-matrix rows with only what the
executed evidence establishes; preserve raw evidence externally. A1 proves
NON-VACUITY, not greenness: `main` is persistently CI-red for pre-existing reasons, a
failing command is a valid record when its intended-target execution and non-zero work
are proven, and nothing is fixed to make anything pass.

Round-2 fix mandates additionally require: candidate-first run ordering (one unchanged
candidate/evidence SHA); a `VERIFY`-preserving capability matrix with reference-only
evidence; an armed corpus-gate execution with CLASSIFIED corpus handling; honest
sentinel accounting (canonical JS selector UNMET-with-cause); the nondeterminism
restatement replacing the refuted worktree-vs-clone claim; a uniform
discovered/executed/passed/failed/skipped convention; and the ignored-test inventory.

# 3. Current source facts

- current authorities/readers/writers: `scripts/gate.mjs` is the canonical Rust gate
  (three surfaces, self-attesting counts); `.github/workflows/ci.yml` defines the CI
  command surface; `.github/workflows/corpus-gate.yml` defines the external-corpus
  gate; root `package.json` defines the JS/TS, native, WASM, provider and conformance
  selectors; `packages/dx-harness/src/corpus-gate/config.ts` defines the env-driven
  corpus-gate contract (`VERTER_CORPUS_GATE_DIR`/`_LABEL`/`_FILE_DETAIL`/…).
- exact files/symbols/contracts already inspected: `charters/A1.md`,
  `verification.md` §2, `contracts/baseline-lock.md` §4,
  `contracts/capability-matrix.md`, `contracts/agent-orchestration.md` §9,
  `CLAUDE.md` (Running Tests / End-of-change Checks / Verification Must Prove
  Execution), `package.json` scripts, `ci.yml`, `corpus-gate.yml`,
  `packages/dx-harness/vitest.{editor-neutral-lsp,corpus-gate}.config.ts`,
  `packages/dx-harness/src/corpus-gate/{config,spawn,index,receipt,types}.ts`.
- current behavior/capability status: capability matrix rows all `VERIFY` (the
  candidate's only tracked change adds a references-only evidence subsection and
  keeps every `Status` cell exactly `VERIFY`); entry-lock records CI persistently
  red at the entry SHA (53 failed / 20 success).
- known open PR/branch conflicts and disposition: PR #98 dispositioned ABANDON at A0;
  no competing writer on `block/a1-command-truth`.

# 4. Allowed write set

- files/modules/generated outputs allowed:
  `docs/arch/refactor/rev11/contracts/capability-matrix.md` (the block's tracked
  source change — committed BEFORE the evidence runs); external evidence under
  `<EVIDENCE_ROOT>/A1/` (never a tracked file).
- dependency/lockfile/protocol changes allowed: none.
- branch/history operations allowed: WIP commits on `block/a1-command-truth`, then
  squash to ONE commit parented on the block base; no push, no merge, no GitHub
  action.

# 5. Forbidden changes

- architecture/ADR/gate weakening: forbidden.
- scope widening or unrelated cleanup: forbidden — no source fix of any red command;
  A1 records truth, it does not repair it.
- compatibility shim, shadow path, runtime switch, alternate authority: forbidden.
- ambient I/O, secret/permission changes, or unowned worktree mutation: forbidden;
  the primary checkout stays untouched and clean; sentinel plants run only in an
  isolated non-candidate copy.
- tracked changes AFTER the evidence runs: forbidden — a run-forced doc edit
  restarts the block from the commit step.
- corpus identity leakage: the classified corpus name must appear in NO tracked
  file and NO evidence file; only the anonymous label and content fingerprint.
- self-approval or review-result fabrication: forbidden; the exact-candidate
  record stays `BLOCKED` pending the three-mandate recheck.

# 6. Required end state and deletions

- surviving owner/path/API: unchanged production tree; the only tracked change is
  the capability-matrix completion (references-only, `VERIFY` preserved).
- old declarations/implementations/caches/tasks/metrics/flags/docs to delete: none
  (evidence-only block); evidence-only scaffolding created by the block (the
  isolated sentinel clone) DELETED before acceptance and verified gone.
- public/protocol/compatibility consequences: none.
- exact one-path/atomicity invariant: one squashed commit on the block branch,
  parented on the block base; `git status --porcelain` empty.

# 7. Required commands and proof

The full required command set, per-row counts under the uniform
discovered/executed/passed/failed/skipped convention, provenance, and digests
live in `command-proofs/index.md` (rows 00b, 01, 01b, 01c, 01d, 01e, 02–06, 07,
08, 08b, 08c, 09, 10, 11, 11b, 12–20, 18b, 21). The sentinel battery (A gate,
B vitest canonical+no-bail, C conformance, D corpus-gate, E release-check
negative control, fmt negative control) with plant-applied proofs lives in
`../sentinel-verification.md`. Highlights of what must hold:

| Command/evidence | Required result |
|---|---|
| `node scripts/gate.mjs --timeout 420m` | three surfaces, self-attested counts, failures recorded not fixed; sentinel A discriminates |
| targeted `-E 'test(typeinfo_proto_ts_freshness)'` + `-p verter_css_syntax` receipts | direct proof replacing gate-coverage inference |
| ignored-test inventories from the gate's own archives | enumeration EQUALS the gate's skip counts |
| canonical JS selector (`pnpm test`) + `--no-bail` supplement | bail-fast finding recorded; canonical sentinel honestly UNMET-with-cause; no-bail discriminates |
| armed corpus gate (`test:corpus-gate`) | non-zero execution against the classified corpus (label + fingerprint only); sentinel D discriminates |
| provider matrix (`test:lsp:neutral`) | 274/274 attempted, receipt `sourceSha` = the FINAL candidate SHA |
| conformance selectors (12–15, 17, 19, 20) | non-vacuous with counts (row 19: verdict-attested, count independent) |
| trybuild re-runs (row 21) | own data points for the nondeterminism restatement |

# 8. Review scope and output

- mandatory changed surface: `contracts/capability-matrix.md` diff + the full A1
  round-2 evidence bundle.
- required dependency/owner closure: none (no production change).
- causal blocker rule: a required command proven vacuous, a sentinel that cannot
  be proven applied, a run-ordering violation (any command evidence not bound to
  the final candidate), or a corpus-name leak is a blocker.
- output format: `contracts/agent-orchestration.md` §9 bounded record
  (`A1-exact-candidate-record.md`, STATE `BLOCKED` pending the three-mandate
  recheck).

# 9. Stop/rescope conditions

- a canonical command executes zero intended work and the vacuity cannot be recorded
  as the finding itself;
- the checkout differs from the recorded candidate;
- a sentinel plant cannot be proven present/unique/new (a green planted run is a
  failure to prove, never a pass);
- any required write outside the allowed write set;
- any tracked change becoming necessary after the runs (restart from the commit
  step instead).

# 10. Handoff result

The §9 bounded record at `../A1/A1-exact-candidate-record.md` with raw evidence
paths/digests and no unsupported success claim; candidate identity recorded in the
external ledger AND carried in every regenerated identity-bearing evidence file.
