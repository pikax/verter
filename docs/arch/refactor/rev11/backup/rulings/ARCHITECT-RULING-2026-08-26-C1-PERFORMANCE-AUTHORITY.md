# C1 performance authority

## Dispatch record

- Input ID: `C1-PERFORMANCE-AUTHORITY-2026-08-26-01`
- Model: `gpt-5.6-sol`
- Reasoning effort: `xhigh`
- Transport: Codex CLI `codex exec`, read-only sandbox
- Exit status: `0`
- Reviewed candidate: `7ddbba827e15b9698850a7e01c21a9e41638aec3`
- Reviewed tree: `bb6dcd1908b3b81c5350ed777e1051b12cdc3a62`
- Integration base: `d1f3d50a948597f036868543b9bb21acacd730ff`
- Prompt SHA-256: `c032d04269b625dda393124c4f5720cdcf87a4ee4bb4d923503edc0eae8d0ca5`
- Raw output SHA-256: `458da29abb693cd6336e8da9efdf46edf6438cc2f6ba5b245bf03a1f749caed3`

The ruling below is the last `# Architecture ruling` final-answer block in the raw output, reproduced exactly. Earlier prompt, template, and trace receipts in the raw output are not part of this ruling.

# Architecture ruling

Disposition **2** is correct: preserve C1’s restart/discard semantics and authorize a narrow, one-candidate acceptance waiver for only the relative wall-time failure. Do not implement semantic continuation in C1.

## Binding invariants applied

1. **Recorded-authority invariant.** Revision 11 charters, contracts, and rulings control; an apparent impossibility requires a recorded deviation, never local substitution. A decision is not operative until registered. `CLAUDE.md:3-11`; `docs/arch/refactor/rev11/orchestration/delivery.md:782-807`.

2. **Single-authority invariant.** There is one module-resolution engine and one optimized production path; C1 cannot add a parallel resolver or transition path. `CLAUDE.md:25-40`; `CLAUDE.md:48-59`; `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md:458-483`.

3. **C1/C2 ownership invariant.** C1 owns one immutable snapshot and one typed outcome per kernel attempt. C2 owns repeated attempts and the snapshot-extend/load/commit/retry state machine. `docs/arch/refactor/rev11/charters/C1.md:221-252`; `docs/arch/refactor/rev11/charters/C1.md:379-389`.

4. **Whole-restart invariant.** After loading or committing observations, orchestration captures a new snapshot and restarts the whole operation from step 1; it may not splice observation-dependent state into an old attempt. `docs/arch/refactor/rev11/contracts/input-loading.md:34-45`.

5. **Attempt-output invariant.** `AttemptOutput` is fresh per attempt. `NeedInputs` and `Terminal` discard it; only `Complete` transfers output. `crates/verter_semantic/src/resolver_core/attempt_output.rs:1-10`; `crates/verter_semantic/src/resolver_core/attempt_output.rs:114-119`; `crates/verter_semantic/src/resolver_core/attempt_outcome.rs:441-447`.

6. **F18/F24 witness invariant.** At the first priority block only the `LoadSet` survives; every non-complete path discards frontier output. Final replay must contain exactly the consumed ordered witness. `crates/verter_semantic/src/resolver_core/priority_frontier.rs:28-49`; `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md:695-715`.

7. **Typed-validation invariant.** Observation-dependent reuse requires typed identity and validation covering every meaning-affecting dimension; an optimization that can alter the semantic answer is semantic policy, not a local performance heuristic. `.claude/skills/type-resolution/SKILL.md:302-321`; `.claude/skills/type-resolution/SKILL.md:323-350`.

8. **Conjunctive performance invariant.** The A6 metrics are conjunctive. The relative wall gate is median `wall_ns <= +3%`; the absolute gate remains 100 ms. Thresholds cannot be reweighted after measurement. `performance-gates.toml:120-137`; `performance-gates.toml:187-208`; `docs/arch/refactor/rev11/charters/C1.md:170-191`.

9. **Exact-evidence invariant.** Final evidence and all three foundational reviews bind one frozen candidate SHA/tree. A change invalidates attachment; verdicts may not be restamped. `docs/arch/refactor/rev11/charters/C1.md:406-415`; `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:33-43`; `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:81-87`.

10. **Successor-deferral invariant.** A deferred scope finding requires a recorded ruling, durable owner block, acceptance ID/test, resolution gate, and ruling reference. `CLAUDE.md:582-589`; `docs/arch/refactor/rev11/orchestration/review.md:219-221`.

## 1. Correct disposition

Issue waiver `C1-A6-WALL-REL-001`.

The conflict is architectural, not an ordinary missed micro-optimization:

- The driver reconstructs the attempt view and reruns `run` after each `NeedInputs`. `crates/verter_workspace/src/resolver.rs:282-364`.
- `ResolveFrame` retains only request-local pure geometry/memo state and clears it on basis change. `crates/verter_semantic/src/resolver_core/resolve_frame.rs:253-332`.
- `PriorityFrontierState` contains observation-dependent `AttemptOutput` and a blocked set whose present lifetime is one attempt. `crates/verter_semantic/src/resolver_core/priority_frontier.rs:56-75`.
- Exact accounting is 724 legacy resolves versus 2,172 candidate attempts and 19,548 ordered candidate operations. `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md:101-141`.
- The remaining lawful cleanup recovered 416,326,964 instructions, but 3,273,612,760 still must be removed. `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md:47-67`.
- Further meaningful improvement requires skipping observation-dependent semantic work, which violates the whole-restart, output-discard, F18, and witness contracts. `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md:168-183`.

Therefore:

- Option 1 would expand C1 into C2-owned orchestration and amend correctness contracts to satisfy a performance gate.
- Option 3 would preserve the conflict without improving architecture and would contradict the already-ruled atomic landing boundary. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:7-9`.
- The waiver preserves the correct semantic contract and records its measured cost honestly.

The A6 metric remains a recorded **FAIL**. The waiver is an acceptance rescope, not a claim that the cell passed.

## 2. Charter and successor ownership

The waiver does **not** expand C1’s production charter. C1 retains its existing one-snapshot/one-outcome boundary.

Implementing continuation now would expand C1 because it would retain observation-dependent state across distinct attempts and snapshots, crossing the boundary at `C1.md:245-252`.

The durable successor owner is existing planned block **C2 — Staged compile transaction and sealed facade**:

- C2 owns bounded load/retry orchestration. `docs/arch/refactor/rev11/program.md:229-235`.
- C1 explicitly assigns repeated attempts and snapshot extension/retry to C2. `docs/arch/refactor/rev11/charters/C1.md:245-252`.
- C2 exists in the authoritative DAG and directly follows C1. `docs/arch/refactor/rev11/program-dag.toml:171-181`.

C2 is presently `LOCKED` with no ratified charter digest. `docs/arch/architecture-lock/ledger/program-state.toml:936-959`. The required maintainer act is therefore to place obligation `C2-AC-C1-A6-CONTINUATION-001` into C2’s eventual charter before that charter is ratified or C2 is dispatched. This does not create a new owner.

## 3. Exact waiver scope

### Identity

The registered act must distinguish, without restamping:

| Role | Identity |
|---|---|
| Integration base | `d1f3d50a948597f036868543b9bb21acacd730ff`, tree `2e7cf8637ec5c52b0fa04572d99672b052f1f85f` |
| Valid wall-measurement subject | `1a4e41d5c604f7cf2e36933ca09bbd8c5ff6ea8e`, tree `3cfc2f81b4b451519c3074ddfd165c6367048a5c` |
| Current production-code subject | `0c22953821f57eedd32b812b1478a449a976f964`, tree `8edde35fb0a18cce5fe229b87ca991c9f95bff20` |
| Dispatched ruling/evidence candidate | `7ddbba827e15b9698850a7e01c21a9e41638aec3`, tree `bb6dcd1908b3b81c5350ed777e1051b12cdc3a62` |

The valid wall report itself identifies `1a4e41d5…`, not `7ddb…`. `docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md:7-16`; `docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md:28-40`. The later instruction evidence identifies production commit `0c229538…` and states that formal ABBA timing was not rerun. `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md:20-45`.

The waiver must therefore bind this evidence chain; it must not call the earlier wall session a `7ddb…` measurement.

### Evidence digests

The authority artifact must pin these exact candidate-tree SHA-256 values:

| Evidence | SHA-256 |
|---|---|
| `wall-diagnostic.md` | `63c632006b5f5df404876389f48c7b1e7858919388f736f52c3fa149ab44ebb9` |
| `pure-reuse-feasibility.md` | `737244cc8d18ff5ad0c5d84717c02f42d065a2ad3a22feb06559eba32b4d2ee6` |
| `frontier-resume-architecture-consult.md` | `b336e2509d64ce769e48928780ed6551a8a72b685997643f5bd6715a52d25520` |
| `unblock-architecture-consult.md` | `7531f5811957eb5cc0fb0a71f0a43502c24e22ed37a3d1f99e1fe382110df7a8` |
| `residual-244-diagnostic.md` | `3e28f8b2bd15c954c2342015732d92edc0ace214f60e2a6b743a8a01bb7e90ea` |
| `receipt.md` | `8c33ba2d16a47205d7c24f052e042d7d21fa1088d14f0e8acdfa58754354015a` |

The accepted wall raw-manifest digest is `306745db699e0b7244d40176d3885bc30bad650bfcad040ebfa2687a8920289a`. `docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md:183-201`.

### What is waived

Only:

```text
cell: A6_META_COMPILE_40_COLD_RUST
metric: wall_ns
statistic: median
comparison: no_regression_percent_max
limit: 3.0
waiver id: C1-A6-WALL-REL-001
subject: the exact C1 production content identified above
```

This waives the blocker created by the measured `+11.264925%` result. It does not change the 3% threshold in `performance-gates.toml`. `performance-gates.toml:202-208`.

The existing cell declares `post_result_exception_allowed = false`; consequently this architecture ruling is not self-operative. The registered delegated-maintainer act must explicitly supersede that field for this waiver ID and subject only. The field remains binding for every other candidate and cell. `performance-gates.toml:340-345`; `docs/arch/refactor/rev11/governance.md:12-21`.

### What remains binding

All of the following remain blocking:

- `wall_ns <= 100,000,000` ns. `performance-gates.toml:192-208`.
- Absolute and relative RSS gates. `performance-gates.toml:210-233`.
- Every enabled counter, zero-counter assertion, and output digest, including normalization `<=11,313`, dispatch `4,216`, cold builds `1,063`, admissions `1,063`, and component-meta digest `7161214711717846280`. `performance-gates.toml:155-185`; `performance-gates.toml:277-330`; `docs/arch/refactor/rev11/evidence/C1/a6/wall-diagnostic.md:269-278`.
- Result, ordered `LoadSet`, wave, consumed-observation, replay, and witness correctness.
- Full C1 acceptance mapping, mutation/revert controls, canonical gate, three independent reviews, landing-equivalence proof, and atomic landing sequence. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:45-87`.

Any production, harness, corpus, toolchain, performance-configuration, or semantic-evidence change voids the waiver. Authority/ledger-only registration changes may cross the identity boundary only with a content-equivalence proof showing every production, harness, and performance-config blob unchanged. Acts pin named content and digests rather than self-referential whole trees. `docs/arch/refactor/rev11/orchestration/delivery.md:802-807`.

### May C1 proceed?

Yes. Once the act is registered, C1 may proceed through final-candidate completion and review. It is not automatically ready:

- The repository receipt still contains an unstamped final-candidate placeholder. `docs/arch/refactor/rev11/evidence/C1/a6/receipt.md:1-4`.
- Step 6 still requires a fresh exact-candidate A6 receipt. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:81`.
- That receipt must report the relative result honestly as `FAIL — covered by C1-A6-WALL-REL-001`, never `PASS`.
- An absolute-wall, RSS, counter, digest, correctness, review, or landing failure remains blocking.

## 4. Continuation contract

Continuation is **not required now** and is unauthorized inside C1.

Before C2 may implement it, `C2-AC-C1-A6-CONTINUATION-001` must ratify at minimum:

1. A typed request-local continuation identity covering the operation/request, originating and current snapshot identities, exact `ResolutionBasis`, frontier/candidate ordering identity, and every consumed observation/fact version.
2. Revalidation of every skipped observation-derived result against the current immutable snapshot. Basis, ordering, configuration, or consumed-fact mismatch invalidates the continuation and forces a whole-operation restart.
3. `AttemptOutput` ownership rules: retained prefix output remains sealed and unpublished; invalidation, terminal failure, cancellation, or no-progress discards it; successful completion merges and publishes each ordered witness exactly once.
4. Explicit amendments to F18 rules 3 and 9 and to F24 replay/witness obligations.
5. A private request-local state-machine API only—no cross-request cache, reusable warm state, new retention authority, or public continuation DTO.
6. Tests for changed/appeared/disappeared observations, basis change, invalidated prefix, terminal after resume, duplicate/missing replay, independent requests, clean-preloaded equivalence, and mutations that omit each revalidation.
7. Fresh A6 evidence showing restoration of the original relative gate while every absolute, RSS, counter, digest, and correctness lock remains green.

These are the minimum domains already established by the feasibility ruling. `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md:212-225`. Implementation before that ratification would be an unapproved semantic optimization under the Typed-validation invariant.

## 5. Request-local cleanup

Commit `0c22953821f57eedd32b812b1478a449a976f964` should remain in C1.

It is semantics-preserving, request-local, and measured to remove 416,326,964 instructions without changing wave count, candidate count, observation order, result, or witness semantics. `docs/arch/refactor/rev11/evidence/C1/a6/pure-reuse-feasibility.md:185-210`. Reverting it would restore known waste and would not restore the 3% gate.

## 6. Operative repository acts

No candidate-branch registry edit is authorized; registry and ledger writes are trunk-owned. `docs/arch/refactor/rev11/orchestration/roles.md:28-41`; `docs/arch/refactor/rev11/orchestration/roles.md:51-54`.

The exact sequence is:

1. Record this ruling at:

   `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-PERFORMANCE-AUTHORITY.md`

   It must include the dispatch ID, identities, evidence digests, exact waiver scope, remaining gates, C2 obligation, and this receipt.

2. Register the following digest-bound documents in `docs/arch/architecture-lock/ledger/authority-registry.toml`:

   - `RULING-C1-THREE-GAPS-ADDENDUM`, path `ARCH-ADDENDUM-C1-THREE-GAPS.md`, SHA-256 `fbbdc70a075877bc2985dc4dfde326609c7062982b804573d16cd70838a2ed37`.
   - `RULING-2026-08-26-C1-LANDING-PATH`, SHA-256 `71ce7601448d75012855a28bb0de1036dc6dff0a755148277b1350c28c9690d7`.
   - `RULING-2026-08-26-C1-PERFORMANCE-AUTHORITY`, SHA-256 computed from the exact registered ruling bytes.

   The Stage-2 document already exists at `authority-registry.toml:708-712`; do not duplicate it.

3. Extend the single existing C1 `[[authorization]]` in place:

   - Preserve its existing document IDs and scope bytes.
   - Append the addendum, Stage-2, landing-path, and performance-authority document IDs.
   - Append, without rewriting the existing scope, keys for waiver ID, base, measured wall subject, production subject/tree, dispatched candidate/tree, evidence-table digest, and successor obligation.
   - Do not create a second C1 authorization. The registry requires exactly one authorization per active block. `docs/arch/architecture-lock/ledger/authority-registry.toml:12-32`; `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md:427-454`.

4. Update `program-state.toml` trunk-side:

   - Move C1 from `LOCKED` to `IN_PROGRESS`.
   - Populate its registered charter digest, base, current candidate/tree, and context/evidence identities.
   - Record the historical-authority deviation, `C1-A6-WALL-REL-001`, the literal relative failure, every retained gate, and the fact that no evidence was restamped.
   - Add `CODEX-DEFER C2-AC-C1-A6-CONTINUATION-001` to C2’s notes, with owner `C2`, resolution gate before C2 acceptance and no later than Revision 11 plan close, the required tests above, and this ruling reference.
   - Leave all review fields pending until three reviews name the frozen final candidate exactly.

5. Add a final waiver-application receipt under `docs/arch/refactor/rev11/evidence/C1/a6/` after the exact final A6 run. It must bind the final SHA/tree, raw output digests, literal relative failure, waiver ID, all passing retained conjuncts, and production-content equivalence to the waived subject. Do not overwrite the historical wall report’s subject.

6. Run the live validator with explicit paths and require a clean result:

```text
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --authority docs/arch/architecture-lock/ledger/authority-registry.toml \
  --mode live
```

7. Resume the landing ruling at Step 6, then complete Steps 7–9 unchanged. `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:81-87`.

===VERTER-RECEIPT-BEGIN===
LANE: c1-performance-architecture-authority
RESULT: PASS
REVIEWED: 7ddbba827e15b9698850a7e01c21a9e41638aec3
FINDINGS: none
===VERTER-RECEIPT-END===
