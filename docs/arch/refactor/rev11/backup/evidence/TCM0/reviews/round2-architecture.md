# TCM0 integration — round 2 review — ARCHITECTURE

**Block:** TCM0 integration (docs-only)
**Mandate:** ARCHITECTURE
**Baseline SHA:** 64234ab14
**Candidate SHA:** da31a892d
**Seat:** Codex `gpt-5.6-sol`, reasoning effort `xhigh`, sandbox read-only

VERDICT: BLOCKING

The round-1 rows #25–26 sequencing defect and forbidden “blocked” headings are fixed, but TCM0 still fails multiple literal Scope and Acceptance obligations. Several failures are explicitly admitted by the candidate’s own `OPEN-GAPS.md`.

## Per-criterion evidence

1. **Scope 1 — Exact package lock: BLOCKING.** Package identity is well evidenced: SHA-1, SHA-256, integrity, `gitHead`, and native-binary provenance are recorded in [package-lock-and-semantic-api.md:7](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:7). But the artifact admits that no byte-exact mapper wire trace was obtained at [line 90](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90), and provides no exact manifest, configured/inferred-project, build/watch/incremental, declaration, or declaration-map shape evidence required by [TCM0.md:44](docs/arch/refactor/rev11/charters/TCM0.md:44).

2. **Scope 2 — Semantic API certification: BLOCKING.** Normal initialization was measured at 34 ms plus 1037 ms, and stale post-dispose `Program` behavior was reproduced at [package-lock-and-semantic-api.md:161](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:161) and [line 185](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:185). Cancellation absence was established at line 253. However, the required `API.fromLSPConnection` hang probe was not run—explicitly admitted at lines 167–174—and the ruling moves it to TCM3 at [MAINTAINER-RULING…:30](docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md:30). That conflicts with TCM0 Scope 2 and the steering’s TCM0-owned probe requirement at [MAINTAINER-STEERING…:772](docs/arch/refactor/rev11/rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:772). No executed bulk-symbol/type/reference, checker, completion, diagnostic, or failure-behavior probe results are cited.

3. **Scope 3 — Feature-ownership ledger: BLOCKING.** I independently counted 44 methods in `TypeProvider`; the ledger represents those in 31 parent rows and documents its grouping at [feature-ownership-ledger.md:3](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:3). But its caller column is explicitly only “representative” at line 69, not every call site. `OPEN-GAPS.md` also admits that the steering’s wider capability list has not been checked row-by-row at [OPEN-GAPS.md:12](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12). Rows #25–26 remain ownerless `CANDIDATE` entries at lines 95–96.

4. **Scope 4 — Diagnostic ownership matrix: SATISFIED.** The matrix covers compiler, mapper parse/config, directives, framework asymmetry, generated regions, duplicates, and external units at [diagnostic-ownership-matrix.md:34](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:34). Deterministic precedence, attribution, suppression, and deduplication are stated at lines 46–66, including honest generated attribution.

5. **Scope 5 — Projection-class contract: BLOCKING.** Five classes and the required `DefinitionAnchor` tuple are present at [projection-class-contract.md:43](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:43). But the supposedly terminal mask policy is not total: `AuthoredTransformed` specifies some always-included and conditional exclusions while leaving many of the 20 `SpanMapFeature` bits undecided at lines 56–63. Lines 86–104 describe the five-axis AND in prose without a complete deterministic mapping for every axis combination.

6. **Scope 6 — External-source decision table: BLOCKING.** All named source families appear in [external-source-decision-table.md:13](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:13), but several rows carry multiple models rather than the charter’s required exactly-one model: row #2 is content-mapped plus Verter-owned, row #5 is content-mapped plus unsupported, and row #9 is content-mapped plus Verter-owned. Those sub-surfaces must be partitioned into individually owned entries.

7. **Scope 7 — Topology benchmarks: BLOCKING.** The artifact identifies candidates and a sound harness, but declares itself “plan, not results” and says no benchmark was run at [topology-benchmark-plan.md:1](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:1) and lines 67–72. `OPEN-GAPS.md` confirms no non-dominated topology has been selected at [line 36](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:36).

8. **Scope 8 — Cache/lifecycle contracts: BLOCKING despite the round-1 ABI fix.** The prepared-key ABI conflation is corrected: Verter compiler ABI belongs in the prepared key, while TypeScript package/wire identity belongs in the derived key at [cache-lifecycle-contracts.md:63](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:63). However, the derived-key contract is internally contradictory: lines 87–93 permit encoder, policy, wire-contract, and encoding identities, while lines 95–96 prohibit anything not reachable from the prepared-artifact identity. Both statements cannot govern one key.

9. **Scope 9 — Deletion closure: BLOCKING.** Six currently located mechanisms and the steering’s 19 categories are enumerated at [deletion-closure.md:6](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:6) and line 28. But items 17–18 are explicitly deferred to TCM4 execution-time discovery at lines 54–62, contrary to “Not deferred to TCM4.” Rows #25–26 also remain undispositioned at lines 64–73.

10. **Scope 10 — Performance baselines: BLOCKING.** Three package probe measurements and five qualitative/hard bounds are recorded at [performance-baselines.md:8](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:8). The file explicitly says comparative topology numbers are not locked at lines 46–51; `OPEN-GAPS.md` confirms the full equivalent-work threshold table is absent at [line 53](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:53).

### Acceptance prohibitions

1. **“Semantic mechanism TBD”: SATISFIED textually.** The inventoried features use the closed owner vocabulary; no such disposition remains.

2. **“Retain provider temporarily”: SATISFIED textually.** The old provider is governed by terminal green-before-delete/deletion-after-green rules, not retained as a fallback.

3. **Unclassified `TypeProvider` method: BLOCKING.** Rows #25–26 lack one of the four legal owners.

4. **Feature claimed by two owners: NOT PROVEN globally.** The 13 previously combined rows are correctly superseded by disjoint single-owner sub-rows at [feature-ownership-ledger.md:138](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:138). That round-1 fix is real. But the wider capability inventory is incomplete, so the invariant cannot yet be established for every feature.

5. **Intentional removal without approval: BLOCKING.** Rows #25–26 await a ruling. Separately, row #31 uses `DisabledByExplicitApprovedContract` while stating governance approval is unnecessary because the method is dead; that contradicts the steering’s rule that this owner is used only for explicitly approved removal at [MAINTAINER-STEERING…:350](docs/arch/refactor/rev11/rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:350).

## Required focused checks

- **Rows #25–26 sequencing:** fixed. [feature-ownership-ledger.md:211](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:211) and [OPEN-GAPS.md:106](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106) now state that TCM0 obtains the ruling. `OPEN-GAPS.md`’s TCM3 owner at line 138 owns only the downstream citation/TCM4-deletion consequence. This is non-circular against `TCM3.predecessors = ["TCM0","TCM1"]` and `TCM4.predecessors = ["TCM0","TCM1","TCM2","TCM3"]` at [program-dag.toml:417](docs/arch/refactor/rev11/program-dag.toml:417).

- **Forbidden headings:** fixed. The headings at `OPEN-GAPS.md` lines 106 and 127 say “gates”; neither uses “blocked” as a disposition.

- **Round-1 taxonomy/#5a/split fixes:** verified at `feature-ownership-ledger.md` lines 41–65, 155, and 185–209.

- **Digest binding:** current SHA-256 values for TCM0–TCM4, the steering, and the certification ruling exactly match `authority-registry.toml` lines 474–515.

- **Change scope:** no `crates/`, `packages/`, or `scripts/` file changed; `git diff --check 64234ab14..da31a892d` exits 0.

## Mandatory validator result

Exact output:

```text
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

Exit code: `1`. These three records are outside the candidate’s TCM-only state hunk, but the mandatory live validation is not green.

## Blocking findings

Finding: Exact package/semantic certification is asserted before TCM0’s required evidence exists.  
Severity: BLOCKING  
Candidate cause: The integration delegates exact wire closure to TCM2 and the API-session hang probe to TCM3, then binds a ruling that certifies the package while saying no correctness requirement was waived.  
Authority/charter requirement violated: TCM0 Scope 1–2; steering §§1 and “Semantic API certification.”  
Affected behavior/invariant: One exact, verified current TypeScript contract and reliable production certification.  
Evidence/reproduction: `package-lock-and-semantic-api.md` lines 90–96 and 167–174; certification ruling lines 30–35; steering lines 772–801.  
Minimum correction condition: TCM0-owned evidence must record the exact wire/manifest/project/build contract and execute the required hang/correctness probes, or a later ratified ruling must explicitly amend those specific steering prerequisites instead of simultaneously claiming they remain unwaived.

Finding: The feature-ownership ledger is not complete or fully governed.  
Severity: BLOCKING  
Candidate cause: The ledger inventories only the 44 trait methods, uses representative callers, leaves wider steering capabilities unverified, and leaves rows #25–26 without a legal owner.  
Authority/charter requirement violated: TCM0 Scope 3 and Acceptance items 3–5.  
Affected behavior/invariant: Every capability has one ratified owner and no capability disappears.  
Evidence/reproduction: `feature-ownership-ledger.md` lines 3–16, 69, 95–101; `OPEN-GAPS.md` lines 12–34 and 106–125.  
Minimum correction condition: Complete the row-by-row steering capability/caller/background-consumer inventory, assign one legal owner per entry, obtain the rows #25–26 ruling, and either obtain explicit approval for row #31 or stop using the approved-removal owner category there.

Finding: Projection masks and external-source ownership are not terminal deterministic contracts.  
Severity: BLOCKING  
Candidate cause: The mask contract leaves feature bits conditional/unspecified, while the external-source table combines multiple ownership models in single rows.  
Authority/charter requirement violated: TCM0 Scope 5–6.  
Affected behavior/invariant: Every emitted span receives a deterministic explicit mask and every source shape has one fail-closed ownership model.  
Evidence/reproduction: `projection-class-contract.md` lines 56–63 and 86–104; `external-source-decision-table.md` rows #2, #5, and #9.  
Minimum correction condition: Provide a total mask function covering every feature bit and axis combination, and split mixed source rows into single-model entries with explicit diagnostic ownership.

Finding: Required topology selection and performance locks do not exist.  
Severity: BLOCKING  
Candidate cause: Plans and qualitative bounds were recorded in place of comparative measurements and the full pre-implementation threshold table.  
Authority/charter requirement violated: TCM0 Scope 7 and 10.  
Affected behavior/invariant: Downstream topology and acceptance thresholds are selected before implementation results can bias them.  
Evidence/reproduction: `topology-benchmark-plan.md` lines 1–6 and 67–72; `performance-baselines.md` lines 46–51; `OPEN-GAPS.md` lines 36–66.  
Minimum correction condition: Run the named topology matrix, select/document the non-dominated topology, and lock the complete equivalent-work numeric threshold table before TCM1–TCM4 implementation results are considered.

Finding: The derived-serialization cache key contract remains self-contradictory.  
Severity: BLOCKING  
Candidate cause: The permitted terminal key dimensions and the subsequent “must not include anything not reachable from prepared identity” rule were written as mutually exclusive requirements.  
Authority/charter requirement violated: TCM0 Scope 8; one coherent cache identity/invalidation law per host process.  
Affected behavior/invariant: Stable cache identity and recompilation independence from terminal policy/encoding.  
Evidence/reproduction: `cache-lifecycle-contracts.md` lines 87–96.  
Minimum correction condition: State one unambiguous derived-key law that permits the enumerated terminal identities while forbidding independent semantic/compiler recomputation inputs.

Finding: Deletion closure is explicitly deferred despite TCM0 owning exact closure.  
Severity: BLOCKING  
Candidate cause: Categories 17–18 and rows #25–26 are left for later execution-time discovery or governance.  
Authority/charter requirement violated: TCM0 Scope 9 and steering “Deletion closure.”  
Affected behavior/invariant: TCM4 executes an authoritative manifest rather than re-deriving deletion scope.  
Evidence/reproduction: `deletion-closure.md` lines 54–73.  
Minimum correction condition: Close the exact current mechanism/survivor inventory in TCM0 and record the rows #25–26 ruling before TCM0 acceptance.

Finding: The mandatory live program-state validator fails.  
Severity: BLOCKING at the program gate; not introduced by this TCM range.  
Candidate cause: Pre-existing empty `context_packet_digest` fields for accepted BV2, B5, and CM1.  
Authority/charter requirement violated: Machine-valid live program ledger requirement.  
Affected behavior/invariant: The repository cannot claim a clean validated program state.  
Evidence/reproduction: Exact command exits `1` with the three violations reproduced above.  
Minimum correction condition: Supply correctly bound context-packet digests or land a ratified ledger correction that makes the live validator exit `0`.
tokens used
268 222
VERDICT: BLOCKING

The round-1 rows #25–26 sequencing defect and forbidden “blocked” headings are fixed, but TCM0 still fails multiple literal Scope and Acceptance obligations. Several failures are explicitly admitted by the candidate’s own `OPEN-GAPS.md`.

## Per-criterion evidence

1. **Scope 1 — Exact package lock: BLOCKING.** Package identity is well evidenced: SHA-1, SHA-256, integrity, `gitHead`, and native-binary provenance are recorded in [package-lock-and-semantic-api.md:7](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:7). But the artifact admits that no byte-exact mapper wire trace was obtained at [line 90](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90), and provides no exact manifest, configured/inferred-project, build/watch/incremental, declaration, or declaration-map shape evidence required by [TCM0.md:44](docs/arch/refactor/rev11/charters/TCM0.md:44).

2. **Scope 2 — Semantic API certification: BLOCKING.** Normal initialization was measured at 34 ms plus 1037 ms, and stale post-dispose `Program` behavior was reproduced at [package-lock-and-semantic-api.md:161](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:161) and [line 185](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:185). Cancellation absence was established at line 253. However, the required `API.fromLSPConnection` hang probe was not run—explicitly admitted at lines 167–174—and the ruling moves it to TCM3 at [MAINTAINER-RULING…:30](docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md:30). That conflicts with TCM0 Scope 2 and the steering’s TCM0-owned probe requirement at [MAINTAINER-STEERING…:772](docs/arch/refactor/rev11/rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:772). No executed bulk-symbol/type/reference, checker, completion, diagnostic, or failure-behavior probe results are cited.

3. **Scope 3 — Feature-ownership ledger: BLOCKING.** I independently counted 44 methods in `TypeProvider`; the ledger represents those in 31 parent rows and documents its grouping at [feature-ownership-ledger.md:3](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:3). But its caller column is explicitly only “representative” at line 69, not every call site. `OPEN-GAPS.md` also admits that the steering’s wider capability list has not been checked row-by-row at [OPEN-GAPS.md:12](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12). Rows #25–26 remain ownerless `CANDIDATE` entries at lines 95–96.

4. **Scope 4 — Diagnostic ownership matrix: SATISFIED.** The matrix covers compiler, mapper parse/config, directives, framework asymmetry, generated regions, duplicates, and external units at [diagnostic-ownership-matrix.md:34](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:34). Deterministic precedence, attribution, suppression, and deduplication are stated at lines 46–66, including honest generated attribution.

5. **Scope 5 — Projection-class contract: BLOCKING.** Five classes and the required `DefinitionAnchor` tuple are present at [projection-class-contract.md:43](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:43). But the supposedly terminal mask policy is not total: `AuthoredTransformed` specifies some always-included and conditional exclusions while leaving many of the 20 `SpanMapFeature` bits undecided at lines 56–63. Lines 86–104 describe the five-axis AND in prose without a complete deterministic mapping for every axis combination.

6. **Scope 6 — External-source decision table: BLOCKING.** All named source families appear in [external-source-decision-table.md:13](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:13), but several rows carry multiple models rather than the charter’s required exactly-one model: row #2 is content-mapped plus Verter-owned, row #5 is content-mapped plus unsupported, and row #9 is content-mapped plus Verter-owned. Those sub-surfaces must be partitioned into individually owned entries.

7. **Scope 7 — Topology benchmarks: BLOCKING.** The artifact identifies candidates and a sound harness, but declares itself “plan, not results” and says no benchmark was run at [topology-benchmark-plan.md:1](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:1) and lines 67–72. `OPEN-GAPS.md` confirms no non-dominated topology has been selected at [line 36](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:36).

8. **Scope 8 — Cache/lifecycle contracts: BLOCKING despite the round-1 ABI fix.** The prepared-key ABI conflation is corrected: Verter compiler ABI belongs in the prepared key, while TypeScript package/wire identity belongs in the derived key at [cache-lifecycle-contracts.md:63](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:63). However, the derived-key contract is internally contradictory: lines 87–93 permit encoder, policy, wire-contract, and encoding identities, while lines 95–96 prohibit anything not reachable from the prepared-artifact identity. Both statements cannot govern one key.

9. **Scope 9 — Deletion closure: BLOCKING.** Six currently located mechanisms and the steering’s 19 categories are enumerated at [deletion-closure.md:6](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:6) and line 28. But items 17–18 are explicitly deferred to TCM4 execution-time discovery at lines 54–62, contrary to “Not deferred to TCM4.” Rows #25–26 also remain undispositioned at lines 64–73.

10. **Scope 10 — Performance baselines: BLOCKING.** Three package probe measurements and five qualitative/hard bounds are recorded at [performance-baselines.md:8](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:8). The file explicitly says comparative topology numbers are not locked at lines 46–51; `OPEN-GAPS.md` confirms the full equivalent-work threshold table is absent at [line 53](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:53).

### Acceptance prohibitions

1. **“Semantic mechanism TBD”: SATISFIED textually.** The inventoried features use the closed owner vocabulary; no such disposition remains.

2. **“Retain provider temporarily”: SATISFIED textually.** The old provider is governed by terminal green-before-delete/deletion-after-green rules, not retained as a fallback.

3. **Unclassified `TypeProvider` method: BLOCKING.** Rows #25–26 lack one of the four legal owners.

4. **Feature claimed by two owners: NOT PROVEN globally.** The 13 previously combined rows are correctly superseded by disjoint single-owner sub-rows at [feature-ownership-ledger.md:138](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:138). That round-1 fix is real. But the wider capability inventory is incomplete, so the invariant cannot yet be established for every feature.

5. **Intentional removal without approval: BLOCKING.** Rows #25–26 await a ruling. Separately, row #31 uses `DisabledByExplicitApprovedContract` while stating governance approval is unnecessary because the method is dead; that contradicts the steering’s rule that this owner is used only for explicitly approved removal at [MAINTAINER-STEERING…:350](docs/arch/refactor/rev11/rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:350).

## Required focused checks

- **Rows #25–26 sequencing:** fixed. [feature-ownership-ledger.md:211](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:211) and [OPEN-GAPS.md:106](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106) now state that TCM0 obtains the ruling. `OPEN-GAPS.md`’s TCM3 owner at line 138 owns only the downstream citation/TCM4-deletion consequence. This is non-circular against `TCM3.predecessors = ["TCM0","TCM1"]` and `TCM4.predecessors = ["TCM0","TCM1","TCM2","TCM3"]` at [program-dag.toml:417](docs/arch/refactor/rev11/program-dag.toml:417).

- **Forbidden headings:** fixed. The headings at `OPEN-GAPS.md` lines 106 and 127 say “gates”; neither uses “blocked” as a disposition.

- **Round-1 taxonomy/#5a/split fixes:** verified at `feature-ownership-ledger.md` lines 41–65, 155, and 185–209.

- **Digest binding:** current SHA-256 values for TCM0–TCM4, the steering, and the certification ruling exactly match `authority-registry.toml` lines 474–515.

- **Change scope:** no `crates/`, `packages/`, or `scripts/` file changed; `git diff --check 64234ab14..da31a892d` exits 0.

## Mandatory validator result

Exact output:

```text
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

Exit code: `1`. These three records are outside the candidate’s TCM-only state hunk, but the mandatory live validation is not green.

## Blocking findings

Finding: Exact package/semantic certification is asserted before TCM0’s required evidence exists.  
Severity: BLOCKING  
Candidate cause: The integration delegates exact wire closure to TCM2 and the API-session hang probe to TCM3, then binds a ruling that certifies the package while saying no correctness requirement was waived.  
Authority/charter requirement violated: TCM0 Scope 1–2; steering §§1 and “Semantic API certification.”  
Affected behavior/invariant: One exact, verified current TypeScript contract and reliable production certification.  
Evidence/reproduction: `package-lock-and-semantic-api.md` lines 90–96 and 167–174; certification ruling lines 30–35; steering lines 772–801.  
Minimum correction condition: TCM0-owned evidence must record the exact wire/manifest/project/build contract and execute the required hang/correctness probes, or a later ratified ruling must explicitly amend those specific steering prerequisites instead of simultaneously claiming they remain unwaived.

Finding: The feature-ownership ledger is not complete or fully governed.  
Severity: BLOCKING  
Candidate cause: The ledger inventories only the 44 trait methods, uses representative callers, leaves wider steering capabilities unverified, and leaves rows #25–26 without a legal owner.  
Authority/charter requirement violated: TCM0 Scope 3 and Acceptance items 3–5.  
Affected behavior/invariant: Every capability has one ratified owner and no capability disappears.  
Evidence/reproduction: `feature-ownership-ledger.md` lines 3–16, 69, 95–101; `OPEN-GAPS.md` lines 12–34 and 106–125.  
Minimum correction condition: Complete the row-by-row steering capability/caller/background-consumer inventory, assign one legal owner per entry, obtain the rows #25–26 ruling, and either obtain explicit approval for row #31 or stop using the approved-removal owner category there.

Finding: Projection masks and external-source ownership are not terminal deterministic contracts.  
Severity: BLOCKING  
Candidate cause: The mask contract leaves feature bits conditional/unspecified, while the external-source table combines multiple ownership models in single rows.  
Authority/charter requirement violated: TCM0 Scope 5–6.  
Affected behavior/invariant: Every emitted span receives a deterministic explicit mask and every source shape has one fail-closed ownership model.  
Evidence/reproduction: `projection-class-contract.md` lines 56–63 and 86–104; `external-source-decision-table.md` rows #2, #5, and #9.  
Minimum correction condition: Provide a total mask function covering every feature bit and axis combination, and split mixed source rows into single-model entries with explicit diagnostic ownership.

Finding: Required topology selection and performance locks do not exist.  
Severity: BLOCKING  
Candidate cause: Plans and qualitative bounds were recorded in place of comparative measurements and the full pre-implementation threshold table.  
Authority/charter requirement violated: TCM0 Scope 7 and 10.  
Affected behavior/invariant: Downstream topology and acceptance thresholds are selected before implementation results can bias them.  
Evidence/reproduction: `topology-benchmark-plan.md` lines 1–6 and 67–72; `performance-baselines.md` lines 46–51; `OPEN-GAPS.md` lines 36–66.  
Minimum correction condition: Run the named topology matrix, select/document the non-dominated topology, and lock the complete equivalent-work numeric threshold table before TCM1–TCM4 implementation results are considered.

Finding: The derived-serialization cache key contract remains self-contradictory.  
Severity: BLOCKING  
Candidate cause: The permitted terminal key dimensions and the subsequent “must not include anything not reachable from prepared identity” rule were written as mutually exclusive requirements.  
Authority/charter requirement violated: TCM0 Scope 8; one coherent cache identity/invalidation law per host process.  
Affected behavior/invariant: Stable cache identity and recompilation independence from terminal policy/encoding.  
Evidence/reproduction: `cache-lifecycle-contracts.md` lines 87–96.  
Minimum correction condition: State one unambiguous derived-key law that permits the enumerated terminal identities while forbidding independent semantic/compiler recomputation inputs.

Finding: Deletion closure is explicitly deferred despite TCM0 owning exact closure.  
Severity: BLOCKING  
Candidate cause: Categories 17–18 and rows #25–26 are left for later execution-time discovery or governance.  
Authority/charter requirement violated: TCM0 Scope 9 and steering “Deletion closure.”  
Affected behavior/invariant: TCM4 executes an authoritative manifest rather than re-deriving deletion scope.  
Evidence/reproduction: `deletion-closure.md` lines 54–73.  
Minimum correction condition: Close the exact current mechanism/survivor inventory in TCM0 and record the rows #25–26 ruling before TCM0 acceptance.

Finding: The mandatory live program-state validator fails.  
Severity: BLOCKING at the program gate; not introduced by this TCM range.  
Candidate cause: Pre-existing empty `context_packet_digest` fields for accepted BV2, B5, and CM1.  
Authority/charter requirement violated: Machine-valid live program ledger requirement.  
Affected behavior/invariant: The repository cannot claim a clean validated program state.  
Evidence/reproduction: Exact command exits `1` with the three violations reproduced above.  
Minimum correction condition: Supply correctly bound context-packet digests or land a ratified ledger correction that makes the live validator exit `0`.
