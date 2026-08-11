# Verter Revision 11 Worker Context Packet

**Packet digest:** not inlined (a self-digest is a fixpoint); the SHA-256 of this file
is recorded as `block.A0.context_packet_digest` in the external program ledger.
**Created from program-state digest:** SHA-256
`afbfcf8a76d85f1831645f8b3921fddc3abaf9ce67e17d01d812e47b3322dc6e` — the literal
digest of `program-state.toml` in this evidence root as it stood at packet-creation
time (block A0 `IN_PROGRESS`, candidate identity resolved, all other blocks
`LOCKED`). The ledger is the mutable authority and is written AFTER this packet is
minted (it receives this packet's digest and the evidence digest), so the live
ledger's current digest legitimately differs from the value above; the value above
identifies the exact ledger state this packet was built from. The ledger is
validated with `scripts/validate-program-state.mjs --mode live` after every change.
**Role:** Implementor (executed directly by the orchestrator under
`ORCHESTRATOR.md` §6 — "Use no subagent when A0 can be completed directly"; the three
review mandates remain distinct and external to this packet).
**Block / charter:** A0 — "Adopt Revision 11 and freeze the exact checkout"
(`charters/A0.md`).
**Stack window / StackSnapshotId / layer_id / acceptance block:** none (no stack;
`ORCHESTRATOR.md` §7 — no program-wide stack created).
**Writable worktree / branch:** a dedicated git worktree on branch
`docs/rev11-architecture-plan`, parented on the entry checkout SHA; the primary
checkout of `pikax/verter` stays byte-clean at the entry SHA.
**Maintainer:** Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub `pikax`) — ruling R-1.
**Orchestrator:** Claude Opus 5 (`claude-opus-5`) main session, Claude Code 2.1.222.

Provenance note: this packet was minted during the A0 fix rounds to satisfy
`charters/A0.md` ("Required evidence ... A0 context/evidence packet") and
`ORCHESTRATOR.md` §5 ("A0 context/evidence packet and exact review state"); it
records the context under which the A0 landing candidate was and is being produced.

# 1. Exact identities

- authority package digest: NONE — package validation waived (ruling R-2); the claimed
  canonical 85-file package digest
  `af11392f5f9eeea75cbd82def85adadfee41b3c8032b5248c09e96aba13123a7` remains UNVERIFIED.
- A6 Implementation Lock digest or `PRE-A6`: `PRE-A6`.
- entry checkout SHA/tree: `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` /
  `3cf111cf5665586b7d8fdfd520f01cfee3bf8108`.
- implementation baseline SHA/tree or `UNSET`: `UNSET` (accepted later by A6).
- block base SHA/tree: `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` /
  `3cf111cf5665586b7d8fdfd520f01cfee3bf8108`.
- current candidate SHA/tree: `b83150f4d46dc2b491d9fb65a10c42d47e42bfd9` /
  `1191b03c1318d1c6344c27981bc08ce4f68b9c58` (also recorded as
  `block.A0.candidate_sha` / `candidate_tree` in the ledger — the ledger is the
  authority if they ever diverge). Prior reviewed candidates and their verdicts are
  recorded in `A0/A0-exact-candidate-record.md`.
- charter digest: `68c2140d3be29de0b8737771aa80d30c17be7cf55aa249a7cfaa3b47f384cd21`
  (SHA-256 of the landed `charters/A0.md` — a reconstruction, not a canonical package
  file; ruling R-2).
- relevant predecessor accepted SHAs/trees/evidence digests: none — A0 has no
  predecessors (`program-dag.toml`).

# 2. Assigned objective

Produce the A0 entry lock and evidence: freeze the exact entry checkout SHA/tree with
clean/untracked/submodule/worktree proof; record package/DAG digests and what is and
is not attested; disposition every architecture-affecting open change (PR #98);
record maintainer/orchestrator identities, model/runtime identity, and delivery
permissions; initialize and validate the external program ledger; land the Revision
11 program package under `docs/arch/refactor/rev11/` plus the Node program-state
validator under `scripts/` (wired to real gates), as one squashed commit parented on
the entry SHA, so the program is resumable from the repository plus the external
ledger.

# 3. Current source facts

- current authorities/readers/writers: the repository at the entry SHA is the sole
  production authority; nothing under `docs/arch/refactor/rev11/` existed before this
  landing; the external evidence directory is orchestrator-written only.
- exact files/symbols/contracts already inspected: `program-dag.toml`,
  `governance.md`, `charters/A0.md`, `contracts/agent-orchestration.md`,
  `contracts/baseline-lock.md`, `ORCHESTRATOR.md`, the repository guard suites named
  in §7, and the entry-state facts recorded in `evidence/A0-preflight-blocked.md`.
- current behavior/capability status: main persistently red at the entry SHA
  (discovery D-1); no branch protection/merge queue; Python 3 absent; details in
  `evidence/A0-preflight-blocked.md`.
- known open PR/branch conflicts and disposition: PR #98 dispositioned ABANDON
  (ruling R-5, program-relationship only; no GitHub action); the wider
  unmerged-branch inventory is screened in `A0/branch-screen.md`
  (`contracts/baseline-lock.md` §3).

# 4. Allowed write set

- files/modules/generated outputs allowed: `docs/arch/refactor/rev11/**` (new tree),
  `scripts/validate-program-state.mjs`, `scripts/validate-program-state.test.mjs`,
  the root `vitest.config.ts` exclude list only as required to keep the node:test
  suite out of vitest collection, the `test:scripts` line in `package.json`, ONE
  narrowly-authorized CI edit — naming `scripts/validate-program-state.mjs` and
  `scripts/validate-program-state.test.mjs` in the `js` change-detection path
  filter of `.github/workflows/ci.yml`, so a change touching only the validator or
  its tests still triggers the `js-build-test` job that runs `test:scripts`
  (maintainer-authorized for that one purpose only; ruling R-7 as amended — an
  earlier draft authorized editing the CI `node --test` guard step instead; that
  edit was reverted and is NOT part of the landing) — and the external evidence
  directory.
- dependency/lockfile/protocol changes allowed: none.
- branch/history operations allowed: WIP commits on the worktree branch, then a squash
  to exactly one commit parented on the entry SHA. Nothing is pushed to `origin`.

# 5. Forbidden changes

- architecture/ADR/gate weakening: forbidden.
- scope widening or unrelated cleanup: forbidden.
- compatibility shim, shadow path, runtime switch, alternate authority: forbidden.
- ambient I/O, secret/permission changes, or unowned worktree mutation: forbidden;
  the primary checkout is never touched; `.github/` is touched ONLY by the single
  authorized CI-wiring edit named in §4 — nothing else under `.github/` is edited.
- self-approval or review-result fabrication: forbidden — acceptance is
  maintainer-only; review verdicts are recorded exactly.

# 6. Required end state and deletions

- surviving owner/path/API: the Revision 11 tree under `docs/arch/refactor/rev11/`
  plus `scripts/validate-program-state.mjs` (+ test, wired into `test:scripts` —
  run by the CI `js-build-test` job — with both files named in the CI `js`
  change-detection path filter); the live ledger stays external (ruling R-6).
- old declarations/implementations/caches/tasks/metrics/flags/docs to delete: none —
  documentation/tooling-only landing; no production source touched.
- public/protocol/compatibility consequences: none at runtime; the plan's Python
  `tools/validate_program_state.py` invocation is fulfilled by the Node
  reimplementation (rulings R-3/R-4).
- exact one-path/atomicity invariant: one squashed commit parented on
  `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0`; `git status --porcelain` empty after.

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|
| `cargo nextest run -p verter_session -E 'test(tracked_paths_no_machine_roots)'` | 5 tests | all pass | `A0/command-proofs/` |
| `cargo nextest run -p verter_session -E 'test(tracked_paths_are_portable)'` | 12 tests | all pass | `A0/command-proofs/` |
| `cargo nextest run -p verter_analysis_inputs -E 'test(analysis_config_paths_never_committed)'` | 6 tests | all pass | `A0/command-proofs/` |
| `node --test scripts/validate-program-state.test.mjs` | 26 tests | all pass | `A0/command-proofs/` |
| `node scripts/validate-program-state.mjs --dag ... --state templates/program-state.template.toml --mode template` | 50 blocks | OK | `A0/command-proofs/` |
| `node scripts/validate-program-state.mjs --dag ... --state <external ledger> --mode live` | 50 blocks | OK | `A0/command-proofs/` |
| stub experiment: suite run against a `process.exit(0)` validator stub | 26 tests | ALL FAIL | `A0/command-proofs/` |
| falsification battery: plant-verified mutations of the live ledger run `--mode live` | 15 mutated ledgers | ALL REJECT with targeted messages | `A0/command-proofs/` |
| machine-path grep over the tracked tree | full tree scanned | zero real machine paths in the landed tree | exact-candidate record |

# 8. Review scope and output

- mandatory changed surface: `docs/arch/refactor/rev11/**`, the two `scripts/` files,
  the `vitest.config.ts` exclude entry, the `package.json` `test:scripts` line, the
  single CI-wiring edit in `.github/workflows/ci.yml`.
- required dependency/owner closure: the three repository guards above; the plan's
  own governance/DAG/template texts as validator rule sources.
- causal blocker rule: `governance.md` §7.
- output format: `contracts/agent-orchestration.md` §9 block record — kept EXTERNAL
  at `A0/A0-exact-candidate-record.md` (the committed `evidence/A0-summary.md` is a
  stable, identity-free description and is NOT the §9 record).

# 9. Stop/rescope conditions

- the entry checkout SHA/tree differs from the lock;
- the landing would require touching production source, lockfiles, `.github/` beyond
  the single authorized CI-wiring edit, or pushing to `origin`;
- a guard fails for a cause not fixable inside the allowed write set;
- the live ledger fails validation for a cause that would require weakening the
  validator (fix the ledger or stop — never the validator);
- maintainer identity or acceptance path becomes ambiguous.

# 10. Handoff result

Return the `contracts/agent-orchestration.md` §9 block record; it is stored
EXTERNALLY as `A0/A0-exact-candidate-record.md` with raw evidence digests, and the
external ledger carries the exact candidate SHA/tree, this packet's digest
(`context_packet_digest`), and the record's digest (`evidence_digest`). No acceptance
is claimed: A0 acceptance is maintainer-only and pending the three-mandate recheck on
the exact final candidate.
