# TCM0 integration — round 2 review — ADVERSARIAL

**Block:** TCM0 integration (docs-only)
**Mandate:** ADVERSARIAL
**Baseline SHA:** 64234ab14
**Candidate SHA:** da31a892d
**Seat:** Codex `gpt-5.6-sol`, reasoning effort `xhigh`, sandbox read-only

VERDICT: BLOCKING

## Per-criterion evidence

1. **Scope 1 — Exact package lock: BLOCKING.** Package digest, integrity, provenance, and binary presence are recorded in [package-lock-and-semantic-api.md](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:13), with structural protocol evidence at [line 59](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:59). However, it explicitly lacks the byte-exact method spelling [at lines 90–96](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90), and the evidence tree contains no exact mapper manifest, configured/inferred-project behavior, or declaration/build/watch/incremental results required by [TCM0 lines 44–50](docs/arch/refactor/rev11/charters/TCM0.md:44).

2. **Scope 2 — Semantic API certification: BLOCKING.** Direct session initialization was measured at 34 ms/1037 ms [at lines 161–174](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:161); stale post-disposal behavior was reproduced [at lines 185–242](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:185); cancellation absence was established [at lines 253–260](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:253). But `API.fromLSPConnection`/local-pipe startup was not probed [at lines 167–174](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:167), and bulk symbol/type/reference, completion, diagnostic, cancellation, and failure behavior is mainly an availability inventory, not executed certification. This does not satisfy TCM0’s literal requirement to reproduce both known defect classes.

3. **Scope 3 — Feature-ownership ledger: BLOCKING.** The ledger genuinely enumerates 44 trait methods in 31 rows [at lines 3–16](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:3), matching an independent enumeration of `TypeProvider`. But [OPEN-GAPS lines 12–34](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12) admits the steering’s broader capabilities—auto-imports, implementation, formatting, call hierarchy, code lens, folding, selection ranges, document symbols, component surfaces, template typing, and background analysis—were not verified row by row. Rows #25–26 also remain ownerless candidates [at lines 95–96](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).

4. **Scope 4 — Diagnostic ownership matrix: SATISFIED.** The required diagnostic classes have explicit owner, attribution, suppression, precedence, and dedup columns [at lines 34–44](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:34), followed by a deterministic ordered rule set [at lines 46–66](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:46).

5. **Scope 5 — Projection-class contract: SATISFIED.** The five-class set and per-class masks are explicit [at lines 43–84](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:43); the terminal class × relation × region × owner × capability policy is stated [at lines 86–104](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:86); omitted masks are expressly forbidden [at lines 22–27](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:22).

6. **Scope 6 — External-source decision table: SATISFIED.** All named inline, external, supplemental, imported-asset, and multi-unit shapes receive an explicit model in [external-source-decision-table.md lines 13–25](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:13), with the selection rule at [lines 27–36](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:27).

7. **Scope 7 — Topology benchmarks: BLOCKING.** Candidate topologies and metrics are specified, but the artifact calls itself “plan, not results” and explicitly states no benchmark was run [at lines 67–72](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:67). [OPEN-GAPS lines 36–51](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:36) confirms there are no comparative measurements and no evidence-based topology selection.

8. **Scope 8 — Cache/lifecycle contract: BLOCKING.** The prepared-artifact ABI conflation is genuinely fixed: TypeScript package identity is excluded from the prepared key and placed in derived serialization [cache-lifecycle-contracts.md lines 63–93](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:63). But lines 87–93 permit encoder, policy, wire-contract, and position-encoding axes, while [lines 95–96](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:95) forbid “anything not already reachable from the prepared-artifact identity.” Those statements are mutually incompatible.

9. **Scope 9 — Deletion closure: BLOCKING.** The 19 steering categories are cross-checked [at lines 36–56](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:36), but items 17–18 are explicitly deferred to execution-time discovery [at lines 54–61](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:54), contrary to “Name every mechanism… Not deferred to TCM4.” Rows #25–26 are also expressly not dispositioned [at lines 64–73](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:64).

10. **Scope 10 — Performance baselines: BLOCKING.** Only 34 ms, 1037 ms, and a defect-signature 0 ms measurement exist [performance-baselines.md lines 8–18](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:8). The file explicitly excludes comparative numbers [at lines 46–51](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:46), while [OPEN-GAPS lines 53–66](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:53) admits the required equivalent-work table is unpopulated.

### Acceptance clauses

1. **No “semantic mechanism TBD”: BLOCKING.** The 44 trait methods have proposed mechanisms, but the wider steering capability inventory remains unverified, and rows #25–26 have no legal owner.
2. **No “retain provider temporarily”: SATISFIED.** TCM4 mandates atomic activation/deletion with no coexistence [TCM4 lines 13–17](docs/arch/refactor/rev11/charters/TCM4.md:13) and no legacy fallback [lines 36–43](docs/arch/refactor/rev11/charters/TCM4.md:36).
3. **No unclassified `TypeProvider` method: BLOCKING.** Rows #25–26 remain `CANDIDATE — governance ruling required`, outside the four legal owner classes.
4. **No feature claimed by two owners: SATISFIED narrowly.** The 13 combined rows are explicitly superseded by disjoint, single-owner `a`/`b` subrows [feature ledger lines 138–209](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:138).
5. **No intentional removal without governance approval: BLOCKING.** The required rows #25–26 ruling has not landed; [OPEN-GAPS lines 106–125](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106) explicitly makes it a TCM0 acceptance gate.

## Targeted adversarial checks

- **Rows #25–26 cycle:** The new correction and both `OPEN-GAPS` sections correctly say TCM0 obtains the ruling and TCM3 later cites it; this is non-circular against `TCM3.predecessors = ["TCM0","TCM1"]` [program-dag.toml lines 417–421](docs/arch/refactor/rev11/program-dag.toml:417). However, a residual sentence still says “the consult is TCM3’s exit criterion” [feature ledger lines 119–121](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:119). The core reasoning is repaired, but the full tree is not internally clean.
- **`OPEN-GAPS` headings:** PASS. Neither rows #25–26 heading uses “blocked”; they use “gates” [lines 106 and 127](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106).
- **Digests:** PASS. Independent `shasum -a 256` matched the registry for TCM0–TCM4, the steering, package-certification ruling, amendment, and DAG. No stale TCM digest was found.
- **Round-2 reports:** FAIL. `git ls-tree da31a892d` contains no `evidence/TCM0/reviews/` files. The worktree has only an untracked [README.md](docs/arch/refactor/rev11/evidence/TCM0/reviews/README.md:1), which claims three reports are “committed here in full” [at lines 8–15](docs/arch/refactor/rev11/evidence/TCM0/reviews/README.md:8); all three linked `round2-*.md` files are absent even from the worktree.

## Mandated validator reproduction

Exact output:

```text
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

Exit code: **1**. This matches the commit-message narration that three pre-existing violations remain; it is not a green validation.

Finding: Literal TCM0 scope and acceptance remain incomplete.  
Severity: BLOCKING  
Candidate cause: The integration records TCM0-owned work as open gaps or later-block delegations instead of satisfying the charter’s literal completion bar.  
Authority/charter requirement violated: TCM0 Scope 1–3, 7, 9, 10 and Acceptance; primary steering sections “Exact upstream/package lock,” “Semantic API certification,” “Feature replacement ledger,” “Process topology benchmarks,” “Deletion closure,” and “Performance baselines.”  
Affected behavior/invariant: TCM0 cannot be accepted and therefore TCM1–TCM4 cannot legally dispatch.  
Evidence/reproduction: Missing exact manifest/configured/inferred/build/watch/declaration evidence; untested attach-hang path; incomplete capability ledger; no topology results/selection; deferred deletion inventory; incomplete performance table; pending rows #25–26 ruling. These omissions are admitted by `OPEN-GAPS.md` and the evidence files cited above.  
Minimum correction condition: Produce and persist the missing package/API probes, complete the full steering capability ledger, measure and select both topologies, enumerate the exact deletion closure, lock the full pre-implementation baseline table, and land the rows #25–26 maintainer ruling.

Finding: Residual text still frames the rows #25–26 consult as TCM3-scoped.  
Severity: BLOCKING  
Candidate cause: The final cycle-fix commit rewrote the later correction section but did not remove/update the earlier summary sentence.  
Authority/charter requirement violated: TCM0 must obtain its own acceptance ruling; TCM3 cannot prepare a gate before its TCM0 predecessor accepts.  
Affected behavior/invariant: The tree retains two conflicting ownership descriptions for the same governance act.  
Evidence/reproduction: [feature-ownership-ledger.md lines 119–121](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:119) versus [lines 222–242](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:222).  
Minimum correction condition: Remove or rewrite the stale sentence so every live reference says TCM0 obtains the ruling and TCM3-EC-G1 only cites the already-recorded ruling.

Finding: Claimed round-2 review records do not exist in the candidate.  
Severity: BLOCKING  
Candidate cause: The review directory was neither completed nor committed.  
Authority/charter requirement violated: Evidence custody requires review reports at stable paths; summaries cannot replace raw evidence [agent-orchestration.md line 162](docs/arch/refactor/rev11/contracts/agent-orchestration.md:162).  
Affected behavior/invariant: There is no auditable conformance, architecture, or adversarial verdict bound to `da31a892d`.  
Evidence/reproduction: `git cat-file -e da31a892d:<each review path>` exits 128 for the README and all three reports; the only local file is untracked and links to nonexistent files.  
Minimum correction condition: Commit substantive `round2-conformance.md`, `round2-architecture.md`, and `round2-adversarial.md` reports bound to the exact candidate SHA, plus an accurate index.

Finding: The derived-serialization key contract contradicts itself.  
Severity: BLOCKING  
Candidate cause: An overbroad “must not include” sentence was added after enumerating valid terminal key dimensions.  
Authority/charter requirement violated: Steering §10’s separation between prepared-artifact identity and derived terminal encoder/policy/wire/encoding identity.  
Affected behavior/invariant: Implementers cannot determine whether the four terminal axes are legal key inputs.  
Evidence/reproduction: [cache-lifecycle-contracts.md lines 87–96](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:87).  
Minimum correction condition: Narrow the prohibition to additional source/semantic computation dependencies while explicitly preserving the four permitted terminal key axes.

Finding: The mandated live program-state validator fails.  
Severity: BLOCKING  
Candidate cause: Three accepted pre-existing blocks still have empty required context-packet digests; this range did not introduce them but touched the same program ledger without restoring validity.  
Authority/charter requirement violated: Accepted-block evidence binding and live program-state validation.  
Affected behavior/invariant: The repository cannot produce a green authoritative program-state validation.  
Evidence/reproduction: Exit code 1 and exact output above; the empty fields are visible for [BV2](docs/arch/architecture-lock/ledger/program-state.toml:664), [B5](docs/arch/architecture-lock/ledger/program-state.toml:707), and [CM1](docs/arch/architecture-lock/ledger/program-state.toml:760).  
Minimum correction condition: Bind each accepted block to an authentic qualifying context packet, or land a maintainer-ratified ledger/schema disposition that the validator recognizes without fabricating evidence.
tokens used
265 588
VERDICT: BLOCKING

## Per-criterion evidence

1. **Scope 1 — Exact package lock: BLOCKING.** Package digest, integrity, provenance, and binary presence are recorded in [package-lock-and-semantic-api.md](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:13), with structural protocol evidence at [line 59](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:59). However, it explicitly lacks the byte-exact method spelling [at lines 90–96](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90), and the evidence tree contains no exact mapper manifest, configured/inferred-project behavior, or declaration/build/watch/incremental results required by [TCM0 lines 44–50](docs/arch/refactor/rev11/charters/TCM0.md:44).

2. **Scope 2 — Semantic API certification: BLOCKING.** Direct session initialization was measured at 34 ms/1037 ms [at lines 161–174](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:161); stale post-disposal behavior was reproduced [at lines 185–242](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:185); cancellation absence was established [at lines 253–260](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:253). But `API.fromLSPConnection`/local-pipe startup was not probed [at lines 167–174](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:167), and bulk symbol/type/reference, completion, diagnostic, cancellation, and failure behavior is mainly an availability inventory, not executed certification. This does not satisfy TCM0’s literal requirement to reproduce both known defect classes.

3. **Scope 3 — Feature-ownership ledger: BLOCKING.** The ledger genuinely enumerates 44 trait methods in 31 rows [at lines 3–16](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:3), matching an independent enumeration of `TypeProvider`. But [OPEN-GAPS lines 12–34](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12) admits the steering’s broader capabilities—auto-imports, implementation, formatting, call hierarchy, code lens, folding, selection ranges, document symbols, component surfaces, template typing, and background analysis—were not verified row by row. Rows #25–26 also remain ownerless candidates [at lines 95–96](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).

4. **Scope 4 — Diagnostic ownership matrix: SATISFIED.** The required diagnostic classes have explicit owner, attribution, suppression, precedence, and dedup columns [at lines 34–44](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:34), followed by a deterministic ordered rule set [at lines 46–66](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:46).

5. **Scope 5 — Projection-class contract: SATISFIED.** The five-class set and per-class masks are explicit [at lines 43–84](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:43); the terminal class × relation × region × owner × capability policy is stated [at lines 86–104](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:86); omitted masks are expressly forbidden [at lines 22–27](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:22).

6. **Scope 6 — External-source decision table: SATISFIED.** All named inline, external, supplemental, imported-asset, and multi-unit shapes receive an explicit model in [external-source-decision-table.md lines 13–25](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:13), with the selection rule at [lines 27–36](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:27).

7. **Scope 7 — Topology benchmarks: BLOCKING.** Candidate topologies and metrics are specified, but the artifact calls itself “plan, not results” and explicitly states no benchmark was run [at lines 67–72](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:67). [OPEN-GAPS lines 36–51](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:36) confirms there are no comparative measurements and no evidence-based topology selection.

8. **Scope 8 — Cache/lifecycle contract: BLOCKING.** The prepared-artifact ABI conflation is genuinely fixed: TypeScript package identity is excluded from the prepared key and placed in derived serialization [cache-lifecycle-contracts.md lines 63–93](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:63). But lines 87–93 permit encoder, policy, wire-contract, and position-encoding axes, while [lines 95–96](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:95) forbid “anything not already reachable from the prepared-artifact identity.” Those statements are mutually incompatible.

9. **Scope 9 — Deletion closure: BLOCKING.** The 19 steering categories are cross-checked [at lines 36–56](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:36), but items 17–18 are explicitly deferred to execution-time discovery [at lines 54–61](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:54), contrary to “Name every mechanism… Not deferred to TCM4.” Rows #25–26 are also expressly not dispositioned [at lines 64–73](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:64).

10. **Scope 10 — Performance baselines: BLOCKING.** Only 34 ms, 1037 ms, and a defect-signature 0 ms measurement exist [performance-baselines.md lines 8–18](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:8). The file explicitly excludes comparative numbers [at lines 46–51](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:46), while [OPEN-GAPS lines 53–66](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:53) admits the required equivalent-work table is unpopulated.

### Acceptance clauses

1. **No “semantic mechanism TBD”: BLOCKING.** The 44 trait methods have proposed mechanisms, but the wider steering capability inventory remains unverified, and rows #25–26 have no legal owner.
2. **No “retain provider temporarily”: SATISFIED.** TCM4 mandates atomic activation/deletion with no coexistence [TCM4 lines 13–17](docs/arch/refactor/rev11/charters/TCM4.md:13) and no legacy fallback [lines 36–43](docs/arch/refactor/rev11/charters/TCM4.md:36).
3. **No unclassified `TypeProvider` method: BLOCKING.** Rows #25–26 remain `CANDIDATE — governance ruling required`, outside the four legal owner classes.
4. **No feature claimed by two owners: SATISFIED narrowly.** The 13 combined rows are explicitly superseded by disjoint, single-owner `a`/`b` subrows [feature ledger lines 138–209](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:138).
5. **No intentional removal without governance approval: BLOCKING.** The required rows #25–26 ruling has not landed; [OPEN-GAPS lines 106–125](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106) explicitly makes it a TCM0 acceptance gate.

## Targeted adversarial checks

- **Rows #25–26 cycle:** The new correction and both `OPEN-GAPS` sections correctly say TCM0 obtains the ruling and TCM3 later cites it; this is non-circular against `TCM3.predecessors = ["TCM0","TCM1"]` [program-dag.toml lines 417–421](docs/arch/refactor/rev11/program-dag.toml:417). However, a residual sentence still says “the consult is TCM3’s exit criterion” [feature ledger lines 119–121](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:119). The core reasoning is repaired, but the full tree is not internally clean.
- **`OPEN-GAPS` headings:** PASS. Neither rows #25–26 heading uses “blocked”; they use “gates” [lines 106 and 127](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106).
- **Digests:** PASS. Independent `shasum -a 256` matched the registry for TCM0–TCM4, the steering, package-certification ruling, amendment, and DAG. No stale TCM digest was found.
- **Round-2 reports:** FAIL. `git ls-tree da31a892d` contains no `evidence/TCM0/reviews/` files. The worktree has only an untracked [README.md](docs/arch/refactor/rev11/evidence/TCM0/reviews/README.md:1), which claims three reports are “committed here in full” [at lines 8–15](docs/arch/refactor/rev11/evidence/TCM0/reviews/README.md:8); all three linked `round2-*.md` files are absent even from the worktree.

## Mandated validator reproduction

Exact output:

```text
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

Exit code: **1**. This matches the commit-message narration that three pre-existing violations remain; it is not a green validation.

Finding: Literal TCM0 scope and acceptance remain incomplete.  
Severity: BLOCKING  
Candidate cause: The integration records TCM0-owned work as open gaps or later-block delegations instead of satisfying the charter’s literal completion bar.  
Authority/charter requirement violated: TCM0 Scope 1–3, 7, 9, 10 and Acceptance; primary steering sections “Exact upstream/package lock,” “Semantic API certification,” “Feature replacement ledger,” “Process topology benchmarks,” “Deletion closure,” and “Performance baselines.”  
Affected behavior/invariant: TCM0 cannot be accepted and therefore TCM1–TCM4 cannot legally dispatch.  
Evidence/reproduction: Missing exact manifest/configured/inferred/build/watch/declaration evidence; untested attach-hang path; incomplete capability ledger; no topology results/selection; deferred deletion inventory; incomplete performance table; pending rows #25–26 ruling. These omissions are admitted by `OPEN-GAPS.md` and the evidence files cited above.  
Minimum correction condition: Produce and persist the missing package/API probes, complete the full steering capability ledger, measure and select both topologies, enumerate the exact deletion closure, lock the full pre-implementation baseline table, and land the rows #25–26 maintainer ruling.

Finding: Residual text still frames the rows #25–26 consult as TCM3-scoped.  
Severity: BLOCKING  
Candidate cause: The final cycle-fix commit rewrote the later correction section but did not remove/update the earlier summary sentence.  
Authority/charter requirement violated: TCM0 must obtain its own acceptance ruling; TCM3 cannot prepare a gate before its TCM0 predecessor accepts.  
Affected behavior/invariant: The tree retains two conflicting ownership descriptions for the same governance act.  
Evidence/reproduction: [feature-ownership-ledger.md lines 119–121](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:119) versus [lines 222–242](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:222).  
Minimum correction condition: Remove or rewrite the stale sentence so every live reference says TCM0 obtains the ruling and TCM3-EC-G1 only cites the already-recorded ruling.

Finding: Claimed round-2 review records do not exist in the candidate.  
Severity: BLOCKING  
Candidate cause: The review directory was neither completed nor committed.  
Authority/charter requirement violated: Evidence custody requires review reports at stable paths; summaries cannot replace raw evidence [agent-orchestration.md line 162](docs/arch/refactor/rev11/contracts/agent-orchestration.md:162).  
Affected behavior/invariant: There is no auditable conformance, architecture, or adversarial verdict bound to `da31a892d`.  
Evidence/reproduction: `git cat-file -e da31a892d:<each review path>` exits 128 for the README and all three reports; the only local file is untracked and links to nonexistent files.  
Minimum correction condition: Commit substantive `round2-conformance.md`, `round2-architecture.md`, and `round2-adversarial.md` reports bound to the exact candidate SHA, plus an accurate index.

Finding: The derived-serialization key contract contradicts itself.  
Severity: BLOCKING  
Candidate cause: An overbroad “must not include” sentence was added after enumerating valid terminal key dimensions.  
Authority/charter requirement violated: Steering §10’s separation between prepared-artifact identity and derived terminal encoder/policy/wire/encoding identity.  
Affected behavior/invariant: Implementers cannot determine whether the four terminal axes are legal key inputs.  
Evidence/reproduction: [cache-lifecycle-contracts.md lines 87–96](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:87).  
Minimum correction condition: Narrow the prohibition to additional source/semantic computation dependencies while explicitly preserving the four permitted terminal key axes.

Finding: The mandated live program-state validator fails.  
Severity: BLOCKING  
Candidate cause: Three accepted pre-existing blocks still have empty required context-packet digests; this range did not introduce them but touched the same program ledger without restoring validity.  
Authority/charter requirement violated: Accepted-block evidence binding and live program-state validation.  
Affected behavior/invariant: The repository cannot produce a green authoritative program-state validation.  
Evidence/reproduction: Exit code 1 and exact output above; the empty fields are visible for [BV2](docs/arch/architecture-lock/ledger/program-state.toml:664), [B5](docs/arch/architecture-lock/ledger/program-state.toml:707), and [CM1](docs/arch/architecture-lock/ledger/program-state.toml:760).  
Minimum correction condition: Bind each accepted block to an authentic qualifying context packet, or land a maintainer-ratified ledger/schema disposition that the validator recognizes without fabricating evidence.
