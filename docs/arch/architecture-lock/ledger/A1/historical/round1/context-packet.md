# Verter Revision 11 Worker Context Packet

**Packet digest:** (SHA-256 of this file; recorded in the ledger by the orchestrator — a file cannot contain its own digest)
**Created from program-state digest:** `a1248ccce85cffb611c7006af6043c18f46e74b86725bde55d4fd83b99f1dbcb` (SHA-256 of `<EVIDENCE_ROOT>/program-state.toml` at packet creation, A1 `IN_PROGRESS`, A0 `ACCEPTED`)
**Role:** Implementor
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
- current candidate SHA/tree or `UNSET`: `UNSET` in this packet — candidate identity is recorded in the external ledger after the squash, never inside packet/record files (the A0 fixpoint lesson)
- charter digest: `b92ef37570b804d170aac6877cd41299e236a7dcb237a6c1d50e76a7748f6d4c` (SHA-256 of `docs/arch/refactor/rev11/charters/A1.md` at the block base)
- relevant predecessor accepted SHAs/trees/evidence digests: A0 ACCEPTED at `b7ea2dc88bda86473de81de3438b7f88ef30adc7` / `47645406a9246e600af995c62608b709347e13a4`; A0 evidence digest recorded as `block.A0.evidence_digest` in the ledger

# 2. Assigned objective

Prove that the canonical Rust, TypeScript, NAPI, WASM, corpus, provider and conformance
commands of the repository execute their intended targets and non-zero work
(`program.md` §A1); complete the affected capability-matrix rows with only what the
executed evidence establishes; preserve raw evidence externally. A1 proves
NON-VACUITY, not greenness: `main` is persistently CI-red for pre-existing reasons, a
failing command is a valid record when its intended-target execution and non-zero work
are proven, and nothing is fixed to make anything pass.

# 3. Current source facts

- current authorities/readers/writers: `scripts/gate.mjs` is the canonical Rust gate
  (three surfaces, self-attesting counts); `.github/workflows/ci.yml` defines the CI
  command surface; `.github/workflows/corpus-gate.yml` defines the external-corpus
  gate (honest-skip without a corpus); root `package.json` defines the JS/TS, native,
  WASM, provider and conformance selectors.
- exact files/symbols/contracts already inspected: `charters/A1.md`,
  `verification.md` §2, `contracts/baseline-lock.md` §4,
  `contracts/capability-matrix.md`, `contracts/agent-orchestration.md` §9,
  `CLAUDE.md` (Running Tests / End-of-change Checks / Verification Must Prove
  Execution), `package.json` scripts, `ci.yml`, `corpus-gate.yml`,
  `packages/dx-harness/vitest.{editor-neutral-lsp,corpus-gate}.config.ts`.
- current behavior/capability status: capability matrix rows all seeded `VERIFY`;
  entry-lock records CI persistently red at the entry SHA (53 failed / 20 success).
- known open PR/branch conflicts and disposition: PR #98 dispositioned ABANDON at A0;
  no competing writer on `block/a1-command-truth`.

# 4. Allowed write set

- files/modules/generated outputs allowed:
  `docs/arch/refactor/rev11/contracts/capability-matrix.md` (the block's tracked
  source change); external evidence under `<EVIDENCE_ROOT>/A1/` (never a tracked
  file).
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
- self-approval or review-result fabrication: forbidden; this record stays
  `BLOCKED` pending the three-mandate recheck.

# 6. Required end state and deletions

- surviving owner/path/API: unchanged production tree; the only tracked change is
  the capability-matrix completion.
- old declarations/implementations/caches/tasks/metrics/flags/docs to delete: none
  (evidence-only block); evidence-only scaffolding (sentinel copy, scratch dirs)
  deleted or left outside the tree before acceptance.
- public/protocol/compatibility consequences: none.
- exact one-path/atomicity invariant: one squashed commit on the block branch,
  parented on the block base; `git status --porcelain` empty.

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|
| `node scripts/gate.mjs --timeout 420m` | 3 surfaces; >20k tests discovered+executed | self-attested per-surface counts; failures recorded, not fixed | `command-proofs/01-gate-mjs.txt` |
| `cargo clippy --workspace --all-targets -- -D warnings` | full workspace lint compile | exit + lint findings recorded as-is | `command-proofs/02-cargo-clippy.txt` |
| `cargo check --workspace --release` | full release-profile check | exit recorded | `command-proofs/03-cargo-check-release.txt` |
| `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | wasm32 lint of verter_wasm | exit recorded | `command-proofs/04-cargo-clippy-wasm.txt` |
| `cargo fmt --all --check` | all workspace sources | exit recorded; non-vacuity via isolated negative control | `command-proofs/05-cargo-fmt-check.txt` |
| `cargo test --workspace --doc` | workspace doctests | discovered/executed counts | `command-proofs/06-cargo-test-doc.txt` |
| `pnpm install --frozen-lockfile` | full dependency materialization | exit 0; lockfile in sync | `command-proofs/07-pnpm-install-frozen-lockfile.txt` |
| `pnpm test` | all workspace package test scripts | per-package vitest counts; failures recorded | `command-proofs/08-pnpm-test.txt` |
| `pnpm run test:scripts` | 4 script suites | counts (14 vitest + 5 + 7 + 26 node --test) | `command-proofs/09-pnpm-test-scripts.txt` |
| `pnpm run build:native` | napi release build + .node artifact | artifact exists, non-trivial size | `command-proofs/10-build-native.txt` |
| `pnpm run build:wasm` | wasm release build + wasm artifact | artifact exists, non-trivial size | `command-proofs/11-build-wasm.txt` |
| `pnpm run gen:vue-goldens:check` | 286 committed oracle artifacts | in-sync verdict against pinned Vue RC | `command-proofs/12-vue-goldens-check.txt` |
| `node scripts/gen-svelte-goldens.mjs --check` | 1066 goldens vs svelte@5.56.3 | in-sync verdict | `command-proofs/13-svelte-goldens-check.txt` |
| `node scripts/gen-svelte-goldens.mjs --conformance --check` | 1218 goldens | in-sync verdict | `command-proofs/14-svelte-conformance-goldens-check.txt` |
| `gen:vue-macro-oracle:check` + `test:vue-macro-oracle` | 11 cases + 4 tests | in-sync + pass counts | `command-proofs/15-vue-macro-oracle.txt` |
| `pnpm --filter @verter/dx-harness test:corpus-gate` | explicit skip WITHOUT corpus env | honest-skip reason captured (SKIP, not pass) | `command-proofs/16-corpus-gate-skip.txt` |
| `node scripts/gen-svelte-name-parity-corpus.mjs --check` | 80 rows | in-sync verdict | `command-proofs/17-svelte-name-parity-check.txt` |
| provider matrix `pnpm run test:lsp:neutral` | tsserver + tsgo + shared-tsgo routes | route-attributed executions | `command-proofs/18-provider-matrix-lsp-neutral.txt` |
| `cargo run -p verter_svelte_conformance -- check` | conformance corpus vs manifest | verdict recorded | `command-proofs/19-svelte-conformance-check.txt` |
| `cargo test -p verter_compiler --features svelte-oracle` | live Svelte oracle harness | counts recorded | `command-proofs/20-svelte-oracle-live.txt` |
| Sentinel battery (gate / vitest / conformance) | plants proven applied; canonical selectors fail; restores prove recovery | discriminating outcomes | `../sentinel-verification.md` |

# 8. Review scope and output

- mandatory changed surface: `contracts/capability-matrix.md` diff + the full A1
  evidence bundle.
- required dependency/owner closure: none (no production change).
- causal blocker rule: a required command proven vacuous, or a sentinel that cannot
  be proven applied, is a blocker.
- output format: `contracts/agent-orchestration.md` §9 bounded record
  (`A1-exact-candidate-record.md`, STATE `BLOCKED` pending the three-mandate
  recheck).

# 9. Stop/rescope conditions

- a canonical command executes zero intended work and the vacuity cannot be recorded
  as the finding itself;
- the checkout differs from the block base;
- a sentinel plant cannot be proven present/unique/new (a green planted run is a
  failure to prove, never a pass);
- any required write outside the allowed write set.

# 10. Handoff result

The §9 bounded record at `../A1/A1-exact-candidate-record.md` with raw evidence
paths/digests and no unsupported success claim; candidate identity recorded in the
external ledger only.
