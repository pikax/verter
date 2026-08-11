# Worker Context Packet

**Packet digest:** recorded in the ledger as `A3.context_packet_digest`  
**Created from program-state digest:** the ledger state at the block's start, with the completion predecessor SUPERSEDED and this block's predecessor set reduced to `["A2"]`  
**Role:** Implementor, then Fix implementor across four review rounds  
**Block / charter:** `charters/A3.md` as amended  
**Stack window / StackSnapshotId / layer_id / acceptance block:** none; single-layer, no stack tooling in use  
**Writable worktree / branch:** `<REPO>-wt-a3`, `block/a3-retraction`  
**Maintainer:** Carlos Rodrigues (GitHub: pikax)  
**Orchestrator:** main session

# 1. Exact identities

- authority package digest: EMPTY — package validation waived by maintainer ruling
- Implementation Lock digest: `PRE-A6`
- entry checkout SHA/tree: `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` / `3cf111cf5665586b7d8fdfd520f01cfee3bf8108`
- implementation baseline SHA/tree: `UNSET`
- block base SHA/tree: `20acec177a030576d680954a87ad355b23ce30cd` / tree of that commit
- candidate SHA/tree: `c1aef669d9c1505e69faf0e327a9c1a5069c5798` / `a2fd9db82c6c2ca49f0bfb1cddc15860290b0a66`
- charter digest: `33f92398b8d2ddd574dc5deba5ffd905fad264a93f1291189a2e4756b8082d8c`
- predecessor accepted identity: `A2` accepted at `d6eefef76c515949a7b7f760bbdf4596a5eef77c`, tree `eaffd3997f140c2c881179e8089ef6bd05b9bc8d`

# 2. Assigned objective

Every catalogued known wrong-and-warm flow-return result except G10 returns a typed degraded
usable result or typed `NoValue` and is refused warm admission, while authored `any`, the
154-row preservation cohort, and rows X05/X68/X80/X88 remain checker-correct, clean and warm.
No syntax-only G10 detector exists and no second completion classifier is introduced.

# 3. Current source facts

- authority: `verter_session::project_semantic_dispatch::flow_return` owns result assembly,
  degradation recording and admission; `flow_slice_content` owns slice lowering.
- inspected: the flow-return result-assembly path, the guard-narrowing detectors, optional-chain
  lowering, the corpus expectation harness and the preservation cohort lock.
- behaviour at base: known wrong-complete results were published complete and admitted warm.
- open branch conflicts: the prior non-G10 retraction work existed on a branch parented on the
  pre-rewrite predecessor; it was restacked onto this block's base rather than re-authored.

# 4. Allowed write set

- `crates/verter_session/src/**` for the retraction path, its lowering inputs and their tests.
- `docs/arch/u6-flow-return-gaps-and-target.md` for the recorded debt row only.
- Evidence under `<EVIDENCE>\A3\`.
- Branch operations confined to `block/a3-retraction`; commits performed outside the worker.

# 5. Forbidden changes

- No structural completion work, no completion topology, events, edges or endpoint accessor.
- No syntax-only G10 detector; no second graph or completion classifier.
- No false refusal of a checker-correct clean/warm result.
- No weakening, deletion or `#[ignore]` of a test to reach green; no fabricated review result.
- No planning vocabulary in source, test names, fixture paths or commit messages.

# 6. Required end state and deletions

- surviving owner: the existing typed degradation and non-admission rails; no new authority.
- deletions: the prior test module name carrying block vocabulary, replaced by a final-state name.
- public consequences: results previously published complete-and-warm for unmodelled shapes are
  now typed partial and cold; no public type or protocol surface changed.
- invariant: one path — a result is either proven and admitted, or typed partial and refused.

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---:|---|---|
| `node scripts/gate.mjs` | 3 surfaces, 24103 + 3 suites + 8536 | PASS | `scratchpad/a3-gate-land2.txt` |
| `cargo nextest run -p verter_session -E 'test(u6) or test(flow)'` | 524 | all pass | fix-round reports |
| `cargo clippy -p verter_session -p verter_semantic --all-targets -- -D warnings` | 2 packages | clean | fix-round reports |
| `cargo fmt --all --check` | workspace | clean | fix-round reports |
| mutation recipes | 15+ reversible plants | each RED when planted, byte-restored | `A3/mutation-evidence.md` |

# 8. Review scope and output

- mandatory changed surface: the full candidate diff against the block base.
- dependency closure: slice lowering, result assembly, admission, corpus expectations.
- causal blocker rule: a finding blocks when it admits a wrong-complete result or refuses a
  checker-correct one; coverage gaps that predate the candidate are debt, not blockers.
- output: verdict plus findings with file, line and required change, each marked blocking or debt.

# 9. Stop/rescope conditions

- A charter requirement that cannot be met without a design decision.
- A fix that would require deleting a detector to resolve a false refusal.
- A finding whose correct resolution lies outside this block's reduced scope.

# 10. Handoff result

Recorded in `A3/A3-exact-candidate-record.md`, with the gate verdict, the four review rounds and
their opposite-polarity defects, the final semantics, the preservation result, and debt `FR-D9`.
