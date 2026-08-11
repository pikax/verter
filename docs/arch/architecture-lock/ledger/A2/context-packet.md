# Verter Revision 11 Worker Context Packet

**Packet digest:** (sha256 of this file at rest — see `digests.txt`)
**Created from program-state digest:** see `<EVIDENCE>/program-state.toml` at A1 acceptance
**Role:** Implementor
**Block / charter:** A2 — Strengthen the U6 public cold/warm differential harness (`docs/arch/refactor/rev11/charters/A2.md`, digest prefix `522787b1c6f90166`)
**Stack window / StackSnapshotId / layer_id / acceptance block:** none (pre-A6 foundational block)
**Writable worktree / branch:** `<REPO>-wt-a2` / `block/a2-u6-harness`
**Maintainer:** Carlos
**Orchestrator:** rev11 program orchestrator (parent session)

# 1. Exact identities

- authority package digest: per `<EVIDENCE>/program-state.toml`
- A6 Implementation Lock digest or `PRE-A6`: PRE-A6
- entry checkout SHA/tree: `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83`
- implementation baseline SHA/tree or `UNSET`: UNSET (A6 accepts later)
- block base SHA/tree: `13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83`
- current candidate SHA/tree: `80a7d9c328842f1457e866fb8588687e9f1d3118` / tree `eaffd3997f140c2c881179e8089ef6bd05b9bc8d` (single squashed commit parented on the base; round-2 fix candidate replacing `2744b4081c3fabf797e50be44af79b5877483861` after the adversarial-mandate FAIL — comparator negative controls, checker cross-validation, anti-recurrence floor)
- charter digest: sha256 prefix `522787b1c6f90166` (`charters/A2.md`)
- relevant predecessor accepted SHAs/trees/evidence digests: A0/A1 records in `<EVIDENCE>/A0`, `A1`

# 2. Assigned objective

Make known U6 flow-return semantic and admission defects mechanically discriminating: a
recursive graph-node expectation, a public cold/warm boundary companion through
`get_flow_return_type_with_audit` (invoked twice; exact projected JSON; typed
degradation; cache-replay state), repair of the five non-discriminating corpus rows as
exact pins or recorded expected-versus-actual gap rows, negative controls for every
expectation form, the crossed capture-write matrix with the position-independence
assertion, and oracle/profile stamps on every row's evidence. No G1–G11 semantic fixes.

# 3. Current source facts

- current authorities/readers/writers: the corpus harness
  `crates/verter_session/src/u6_flow_shape_corpus_tests.rs` (+ include!'d rows file);
  the audited public entry `crates/verter_session/src/host_flow_return_audit.rs`;
  graph vocabulary `crates/verter_session/src/semantic_query.rs` (`SemanticNodeData`);
  JSON projection `crates/verter_session/src/typeinfo/raise.rs`
  (`project_node_to_type_expr_json_bytes`).
- exact files/symbols/contracts inspected: the above, plus
  `docs/arch/u6-flow-return-gaps-and-target.md` (§1 G-table, §7 spec),
  `tests/cases/g_type/flow_return_audit_contract.rs` (boundary usage pattern),
  `FlowReturnResult` (`semantic_query/flow_return_result.rs`).
- current behavior/capability status: measured in `red-green-record.md` and
  `matrix-coverage.md`; five G-classes reproduced live (G4/G5 position dependence,
  G6/G7 stale captures, G9 unapplied guard, N25 dead-contributor survival —
  wrong-and-warm; try/labeled/if/switch IIFE-write refusals — honest cold).
- known open PR/branch conflicts and disposition: none; base worktree untouched.

# 4. Allowed write set

- files/modules/generated outputs allowed: `crates/verter_session/src/u6_flow_*`,
  `docs/arch/u6-flow-return-gaps-and-target.md` (§7 status), external evidence dir.
- dependency/lockfile/protocol changes allowed: none (none made).
- branch/history operations allowed: WIP commits squashed to ONE on
  `block/a2-u6-harness`; no push/merge/GitHub.

# 5. Forbidden changes

- architecture/ADR/gate weakening: none made — no production code touched.
- scope widening or unrelated cleanup: none.
- compatibility shim, shadow path, runtime switch, alternate authority: none —
  everything drives the ONE audited public entry / shared dispatch.
- ambient I/O, secret/permission changes, unowned worktree mutation: none; the main
  checkout `<REPO>` remained clean at `13cedd6fc…`.
- self-approval or review-result fabrication: none — STATE stays BLOCKED pending the
  three-mandate recheck.

# 6. Required end state and deletions

- surviving owner/path/API: the strengthened harness in
  `u6_flow_expect_tests.rs` + extended `Row` columns; the five rows repaired in place.
- old declarations to delete: none owed — the five rows' weak pins were REPLACED in
  place (no dual assertion path); no evidence-only scaffolding was left in the tree
  (probe fixtures live only in the external evidence dir).
- public/protocol/compatibility consequences: none (test-only + doc status note).
- exact one-path/atomicity invariant: single squashed commit; the expect/boundary lane
  drives ONLY `get_flow_return_type_with_audit` + the shared dispatch graph read.

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|
| `cargo test -p verter_session --lib u6_flow_shape_corpus` (base) | 15 tests | green baseline | `command-proofs/00-baseline-corpus-green.txt` |
| RED lane (checker-derived pins, parent semantics) | 5 rows × expect+boundary | FAILS (discrimination proof) | `command-proofs/01-red-lane-checker-derived-pins.txt` |
| `U6_CORPUS_DUMP=1 … matrix_cells_hold_their_pins` | 20 cell programs measured | measurements recorded | `command-proofs/02-matrix-cell-dump-measurements.txt` |
| `cargo test -p verter_session --lib u6_flow_expect_tests::expectation_controls` | 8 controls | green | `command-proofs/03-negative-controls-green.txt` |
| `cargo test -p verter_session --lib u6_flow` (post-repin) | 28 tests | green | `command-proofs/04-green-all-u6-flow-after-repins.txt` |
| `cargo test -p verter_session --lib` (full) | 5655 tests | green, 0 failed | `command-proofs/05-full-verter-session-lib-green.txt` |
| `cargo test -p verter_session --lib u6_flow` (superseded candidate `2744b4081`) | 28 tests | green | `command-proofs/06-green-candidate-2744b4081-u6-flow.txt` |
| first-candidate gate run (FAIL attribution) | 3 surfaces | FAIL only on lib_rs_stays_under_line_ceiling (candidate-introduced, repaired) | `command-proofs/07a-gate-first-candidate-054445dde-FAIL-*.txt` |
| `node scripts/gate.mjs` (superseded candidate `2744b4081`) | 3 surfaces, full workspace | PASS | `command-proofs/07-final-gate-candidate-2744b4081-*.txt` |
| pinned tsgo over 25 probes | 25 programs, 50 diagnostics | recorded checker answers | `command-proofs/tsgo-probes/` |
| `cargo clippy -p verter_session --lib -- -D warnings` base-vs-candidate error diff | 6 pre-existing errors both | IDENTICAL (delta zero) | recorded in `A2-exact-candidate-record.md` |
| ROUND 2: comparator mutation matrix (28 mutations, plant-proved, restored) | 28 × 40-test runs | 28/28 caught by designated control | `command-proofs/20-comparator-mutation-matrix/` |
| ROUND 2: `cargo nextest run -p verter_session -E 'test(u6_flow)'` (superseded candidate `eb441c9bf`) | 40 tests | green | `command-proofs/21-green-candidate-eb441c9bf-u6-flow.txt` |
| ROUND 2: `U6_CORPUS_DUMP=1` headline tests | 2 tests | FAIL LOUDLY (dump is never evidence) | `command-proofs/22-dump-mode-fails-loud.txt` |
| ROUND 2: `node scripts/gate.mjs` (superseded candidate `eb441c9bf`) | 3 surfaces, full workspace | PASS | `command-proofs/23-final-gate-candidate-eb441c9bf-*.txt` |

# 8. Review scope and output

- mandatory changed surface: the 4 tracked files in the current candidate `80a7d9c32` (tree `eaffd3997`). Rows above marked "superseded candidate" are per-round history retained for provenance; the authoritative verification for this candidate is `canonical-verification.md`.
- required dependency/owner closure: none beyond `verter_session` test tree.
- causal blocker rule: any red traced to the candidate blocks; pre-existing reds are
  recorded, not adopted.
- output format: `contracts/agent-orchestration.md` §9 record (`A2-exact-candidate-record.md`).

# 9. Stop/rescope conditions

- a strengthened row unfixable without semantic change → recorded as KnownOwed gap
  (N25) instead of stopping; no row required a semantic change to become discriminating.
- gate failure traced to the candidate → fix, re-squash, re-run (did not occur /
  see final record).

# 10. Handoff result

`A2-exact-candidate-record.md` beside this packet; raw evidence under
`command-proofs/`; digests in `digests.txt`.
