# TCM0 integration — round 2 review — CONFORMANCE

**Block:** TCM0 integration (docs-only)
**Mandate:** CONFORMANCE
**Baseline SHA:** 64234ab14
**Candidate SHA:** da31a892d
**Seat:** Codex `gpt-5.6-sol`, reasoning effort `xhigh`, sandbox read-only

VERDICT: BLOCKING

## Per-criterion evidence

### TCM0 Scope

1. **Exact package lock — BLOCKING.** Package digest and provenance are recorded in [package-lock-and-semantic-api.md:13](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:13), and trust plus mapping fields are recorded at [line 98](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:98). However, the evidence explicitly says the exact wire method names were not captured at [line 90](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90). It also does not record the exact mapper manifest, configured/inferred-project behavior, or declaration/build/watch/incremental behavior required by [TCM0.md:44](docs/arch/refactor/rev11/charters/TCM0.md:44).

2. **Semantic API certification — BLOCKING.** Normal initialization and stale-handle behavior were measured at [package-lock-and-semantic-api.md:161](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:161) and [line 185](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:185); cancellation absence is recorded at [line 253](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:253). But the required `API.fromLSPConnection` hang probe was explicitly not run at [line 167](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:167). The file lists bulk symbol/type/reference, completion, and diagnostic methods, but supplies no executed correctness measurements for them. The four named probe scripts at lines 288–290 are absent from the repository.

3. **Feature-ownership ledger — BLOCKING.** The ledger does enumerate 44 trait methods in 31 rows, and I independently counted exactly 44 methods in `TypeProvider` at `traits.rs:130-512`. But [OPEN-GAPS.md:12](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12) acknowledges that steering capabilities outside those 44 methods have not been classified row-by-row. Rows #25–26 also still have no legal primary owner at [feature-ownership-ledger.md:95](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).

4. **Diagnostic ownership matrix — SATISFIED as a TCM0 architecture lock.** The required classes and ownership are tabulated at [diagnostic-ownership-matrix.md:34](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:34), with deterministic precedence/dedup rules at [line 46](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:46). Generated-only diagnostics are explicitly required to remain visible with honest attribution.

5. **Projection-class contract — BLOCKING / NOT PROVEN.** Five classes are named at [projection-class-contract.md:43](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:43), and the intended factors are listed at [line 86](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:86). But this is not a total derivation: several masks are described conditionally, and [feature-ownership-ledger.md:57](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:57) defers per-row class assignment to TCM1/TCM2. TCM0 therefore has not yet supplied an implementation-deterministic class × relation × region × owner × capability policy.

6. **External-source decision table — SATISFIED.** Every charter-named shape, or explicitly distinguished sub-surface of that shape, is dispositioned in [external-source-decision-table.md:13](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:13), with the governing decision rule at [line 27](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:27).

7. **Topology benchmarks — BLOCKING.** [topology-benchmark-plan.md:1](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:1) labels itself “plan, not results,” and [line 67](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:67) confirms no benchmark was run. Consequently, no non-dominated topology has been selected. This is also acknowledged at [OPEN-GAPS.md:36](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:36).

8. **Cache and lifecycle contracts — SATISFIED as a contract.** The per-process cache ownership is defined at [cache-lifecycle-contracts.md:27](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:27), prepared-key inclusion/exclusion at [line 49](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:49), derived serialization key at [line 82](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:82), and invalidation law at [line 98](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:98). The round-1 ABI conflation is genuinely fixed: TypeScript package/wire identity is now derived-key-only; Verter’s own compiler ABI remains in the prepared key.

9. **Deletion closure — BLOCKING.** The 19 steering categories are cross-checked at [deletion-closure.md:28](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:28), but items 17–18 explicitly defer discovery to TCM4 at [line 54](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:54), with that deferral acknowledged again at [line 58](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:58). That contradicts Scope 9’s literal “Name every mechanism … Not deferred to TCM4.”

10. **Performance baselines — BLOCKING.** Three probe measurements and five qualitative/hard requirements are recorded at [performance-baselines.md:8](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:8). But [line 46](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:46) explicitly says comparative topology numbers are absent, and [OPEN-GAPS.md:53](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:53) acknowledges that the required equivalent-work numeric table is not populated.

### TCM0 Acceptance prohibitions

A1. **No “semantic mechanism TBD” — SATISFIED.** TCM3 names a narrow snapshot-bound `TypeSemanticOracle` and exhaustive fallback order in [TCM3.md:46](docs/arch/refactor/rev11/charters/TCM3.md:46).

A2. **No “retain provider temporarily” — SATISFIED.** TCM2–TCM4 consistently forbid a fallback/dual route; see [TCM4.md:30](docs/arch/refactor/rev11/charters/TCM4.md:30).

A3. **No unclassified `TypeProvider` method — BLOCKING.** Rows #25–26 remain `CANDIDATE — governance ruling required`, rather than one of the four legal owners, at [feature-ownership-ledger.md:95](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).

A4. **No feature claimed by two owners — SATISFIED by the round-1 correction.** The combined rows are expressly superseded by disjoint, single-owner sub-rows at [feature-ownership-ledger.md:138](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:138), with structural discriminants explained at [line 185](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:185).

A5. **No intentional removal without governance approval — BLOCKING.** No rows #25–26 maintainer ruling exists. The gap is explicitly acknowledged at [OPEN-GAPS.md:106](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106).

## Specific round-1 checks

- **Rows #25–26 ownership/cycle:** substantively fixed. TCM0 owns obtaining the ruling; TCM3-EC-G1 only cites that already-obtained ruling downstream. [feature-ownership-ledger.md:222](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:222) agrees with both gap sections. This is non-circular because [program-dag.toml:399](docs/arch/refactor/rev11/program-dag.toml:399) makes TCM3 depend on TCM0 and TCM1. The section heading’s claim that the rows are “closed” remains misleading: only their gates are assigned; the rows remain open candidates.
- **OPEN-GAPS headings:** fixed. Neither relevant `##` heading uses “blocked”; a direct heading search returned no match.
- **Cache ABI, ledger taxonomy, #5a, and split-row fixes:** present in the candidate and consistent with the corrected prose.
- **Remaining round-1 gaps:** not fixed—only recorded. Ledger scope, topology results, performance thresholds, and rows #25–26 governance still gate TCM0 acceptance.

## Validator, digests, and framework-contract preservation

Exact validator output:

```text
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

Exit code: `1`. All three empty digests are also present at baseline `64234ab14`, so the candidate narration is accurate and this is not a candidate-introduced regression.

I recomputed every newly/currently pinned TCM digest, not merely two. All seven match `authority-registry.toml`: TCM0–TCM4 charters, TCM steering, and package-certification ruling. The steering’s “verbatim” claim also passed an independent comparison: removing the 24-line header and comparing against `<external steering source outside this repository>` returned `cmp_exit=0`.

Framework contracts are byte-preserved: the `crates`, `packages`, and `scripts` Git tree hashes are identical between baseline and candidate, and the range contains zero non-`docs/` paths. Thus the accepted Vue/Svelte outputs, published code surfaces, and CSS behavior are unchanged by this integration.

## Blocking findings

Finding: TCM0’s exact package and semantic-API certification is incomplete.  
Severity: BLOCKING  
Candidate cause: Required package/API probes were delegated to TCM2/TCM3, while TCM0’s literal charter assigns them to TCM0; the probe harness/transcript was not committed.  
Authority/charter requirement violated: TCM0 Scope 1–2; steering “Exact upstream/package lock” and “Semantic API certification.”  
Affected behavior/invariant: Exact codec fidelity, project-mode behavior, semantic-session correctness, and safe package certification are not proven.  
Evidence/reproduction: Exact wire spelling and attach-session probe are explicitly open at [package-lock-and-semantic-api.md:90](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90) and [line 167](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:167); all four named probe scripts are absent.  
Minimum correction condition: Record the exact request/response and manifest contract, configured/inferred/build/watch/declaration behavior, execute the attach-hang and full semantic correctness probes against the exact candidate, and commit reproducible evidence.

Finding: The feature-ownership ledger and rows #25–26 disposition are incomplete.  
Severity: BLOCKING  
Candidate cause: Inventory stopped at the 44 trait methods, and the maintainer ruling was converted into a non-circular gate but was not actually obtained.  
Authority/charter requirement violated: TCM0 Scope 3 and Acceptance prohibitions A3/A5.  
Affected behavior/invariant: Some capabilities have no proven owner; rows #25–26 cannot legally be retained or deleted.  
Evidence/reproduction: [OPEN-GAPS.md:12](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12), [feature-ownership-ledger.md:95](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).  
Minimum correction condition: Classify every steering-listed capability row-by-row and land a digest-bound maintainer ruling assigning rows #25–26 either approved-disabled/deletion or `VerterWithTypeSemanticOracle` ownership.

Finding: The terminal projection-mask policy is not fully determined.  
Severity: BLOCKING  
Candidate cause: TCM0 ratified class names and examples but deferred per-row classification and omitted an exhaustive derivation.  
Authority/charter requirement violated: TCM0 Scope 5.  
Affected behavior/invariant: TCM2 cannot deterministically compute an explicit legal mask for every wire span without making new policy decisions.  
Evidence/reproduction: [projection-class-contract.md:86](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:86), [feature-ownership-ledger.md:57](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:57).  
Minimum correction condition: Supply an exhaustive table or closed algorithm for every relevant class × relation × region × owner × certified-capability combination, including explicit zero/disabled masks.

Finding: Required topology results and locked performance baselines do not exist.  
Severity: BLOCKING  
Candidate cause: A benchmark plan and qualitative requirements were recorded in place of completed comparisons and numeric gates.  
Authority/charter requirement violated: TCM0 Scope 7 and 10.  
Affected behavior/invariant: No evidence-based topology selection exists, and later implementation results could influence thresholds.  
Evidence/reproduction: [topology-benchmark-plan.md:67](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:67), [performance-baselines.md:46](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:46).  
Minimum correction condition: Run all required projection/semantic topology cells, select the non-dominated topology from recorded measurements, and lock the complete equivalent-work numeric gate table before TCM1–TCM4 implementation results are observed.

Finding: Deletion closure explicitly defers part of its inventory to TCM4.  
Severity: BLOCKING  
Candidate cause: Items 17–18 were left as execution-time discovery categories.  
Authority/charter requirement violated: TCM0 Scope 9: “Name every mechanism … Not deferred to TCM4.”  
Affected behavior/invariant: TCM4 lacks an exact, pre-ratified deletion manifest and must re-derive part of it during execution.  
Evidence/reproduction: [deletion-closure.md:54](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:54).  
Minimum correction condition: Enumerate the current old-route-only APIs/DTOs and provide a concrete historical-codec absence/deletion inventory before TCM0 acceptance.

Finding: TCM1’s claimed “full” string-surface inventory is internally inconsistent and contains stale citations.  
Severity: BLOCKING  
Candidate cause: A lower-bound inventory was converted into an exact 13-field acceptance universe without reconciling its own list against source.  
Authority/charter requirement violated: TCM1 owned scope 2 and exit criterion 3a; integration claim that the rewritten charters are implementation-ready.  
Affected behavior/invariant: TCM1 can satisfy “all thirteen” while leaving string-encoded mapping fields unmigrated.  
Evidence/reproduction: [mapping-products-string-surface.md:58](docs/arch/refactor/rev11/evidence/TCM0/mapping-products-string-surface.md:58) enumerates eleven compiler-side names plus four protocol fields, while [TCM1.md:69](docs/arch/refactor/rev11/charters/TCM1.md:69) calls the total thirteen. The cited `VerterTsxBlock.source_map` is at [compile/types.rs:495](crates/verter_compiler/src/compile/types.rs:495), not line 497; several recorded struct names also do not exist verbatim.  
Minimum correction condition: Regenerate a complete field inventory from the candidate source, correct all names/locations/counts, make the TCM1 exit universe match that inventory, and recompute affected charter digests.
tokens used
252 699
VERDICT: BLOCKING

## Per-criterion evidence

### TCM0 Scope

1. **Exact package lock — BLOCKING.** Package digest and provenance are recorded in [package-lock-and-semantic-api.md:13](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:13), and trust plus mapping fields are recorded at [line 98](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:98). However, the evidence explicitly says the exact wire method names were not captured at [line 90](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90). It also does not record the exact mapper manifest, configured/inferred-project behavior, or declaration/build/watch/incremental behavior required by [TCM0.md:44](docs/arch/refactor/rev11/charters/TCM0.md:44).

2. **Semantic API certification — BLOCKING.** Normal initialization and stale-handle behavior were measured at [package-lock-and-semantic-api.md:161](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:161) and [line 185](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:185); cancellation absence is recorded at [line 253](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:253). But the required `API.fromLSPConnection` hang probe was explicitly not run at [line 167](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:167). The file lists bulk symbol/type/reference, completion, and diagnostic methods, but supplies no executed correctness measurements for them. The four named probe scripts at lines 288–290 are absent from the repository.

3. **Feature-ownership ledger — BLOCKING.** The ledger does enumerate 44 trait methods in 31 rows, and I independently counted exactly 44 methods in `TypeProvider` at `traits.rs:130-512`. But [OPEN-GAPS.md:12](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12) acknowledges that steering capabilities outside those 44 methods have not been classified row-by-row. Rows #25–26 also still have no legal primary owner at [feature-ownership-ledger.md:95](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).

4. **Diagnostic ownership matrix — SATISFIED as a TCM0 architecture lock.** The required classes and ownership are tabulated at [diagnostic-ownership-matrix.md:34](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:34), with deterministic precedence/dedup rules at [line 46](docs/arch/refactor/rev11/evidence/TCM0/diagnostic-ownership-matrix.md:46). Generated-only diagnostics are explicitly required to remain visible with honest attribution.

5. **Projection-class contract — BLOCKING / NOT PROVEN.** Five classes are named at [projection-class-contract.md:43](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:43), and the intended factors are listed at [line 86](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:86). But this is not a total derivation: several masks are described conditionally, and [feature-ownership-ledger.md:57](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:57) defers per-row class assignment to TCM1/TCM2. TCM0 therefore has not yet supplied an implementation-deterministic class × relation × region × owner × capability policy.

6. **External-source decision table — SATISFIED.** Every charter-named shape, or explicitly distinguished sub-surface of that shape, is dispositioned in [external-source-decision-table.md:13](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:13), with the governing decision rule at [line 27](docs/arch/refactor/rev11/evidence/TCM0/external-source-decision-table.md:27).

7. **Topology benchmarks — BLOCKING.** [topology-benchmark-plan.md:1](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:1) labels itself “plan, not results,” and [line 67](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:67) confirms no benchmark was run. Consequently, no non-dominated topology has been selected. This is also acknowledged at [OPEN-GAPS.md:36](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:36).

8. **Cache and lifecycle contracts — SATISFIED as a contract.** The per-process cache ownership is defined at [cache-lifecycle-contracts.md:27](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:27), prepared-key inclusion/exclusion at [line 49](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:49), derived serialization key at [line 82](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:82), and invalidation law at [line 98](docs/arch/refactor/rev11/evidence/TCM0/cache-lifecycle-contracts.md:98). The round-1 ABI conflation is genuinely fixed: TypeScript package/wire identity is now derived-key-only; Verter’s own compiler ABI remains in the prepared key.

9. **Deletion closure — BLOCKING.** The 19 steering categories are cross-checked at [deletion-closure.md:28](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:28), but items 17–18 explicitly defer discovery to TCM4 at [line 54](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:54), with that deferral acknowledged again at [line 58](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:58). That contradicts Scope 9’s literal “Name every mechanism … Not deferred to TCM4.”

10. **Performance baselines — BLOCKING.** Three probe measurements and five qualitative/hard requirements are recorded at [performance-baselines.md:8](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:8). But [line 46](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:46) explicitly says comparative topology numbers are absent, and [OPEN-GAPS.md:53](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:53) acknowledges that the required equivalent-work numeric table is not populated.

### TCM0 Acceptance prohibitions

A1. **No “semantic mechanism TBD” — SATISFIED.** TCM3 names a narrow snapshot-bound `TypeSemanticOracle` and exhaustive fallback order in [TCM3.md:46](docs/arch/refactor/rev11/charters/TCM3.md:46).

A2. **No “retain provider temporarily” — SATISFIED.** TCM2–TCM4 consistently forbid a fallback/dual route; see [TCM4.md:30](docs/arch/refactor/rev11/charters/TCM4.md:30).

A3. **No unclassified `TypeProvider` method — BLOCKING.** Rows #25–26 remain `CANDIDATE — governance ruling required`, rather than one of the four legal owners, at [feature-ownership-ledger.md:95](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).

A4. **No feature claimed by two owners — SATISFIED by the round-1 correction.** The combined rows are expressly superseded by disjoint, single-owner sub-rows at [feature-ownership-ledger.md:138](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:138), with structural discriminants explained at [line 185](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:185).

A5. **No intentional removal without governance approval — BLOCKING.** No rows #25–26 maintainer ruling exists. The gap is explicitly acknowledged at [OPEN-GAPS.md:106](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:106).

## Specific round-1 checks

- **Rows #25–26 ownership/cycle:** substantively fixed. TCM0 owns obtaining the ruling; TCM3-EC-G1 only cites that already-obtained ruling downstream. [feature-ownership-ledger.md:222](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:222) agrees with both gap sections. This is non-circular because [program-dag.toml:399](docs/arch/refactor/rev11/program-dag.toml:399) makes TCM3 depend on TCM0 and TCM1. The section heading’s claim that the rows are “closed” remains misleading: only their gates are assigned; the rows remain open candidates.
- **OPEN-GAPS headings:** fixed. Neither relevant `##` heading uses “blocked”; a direct heading search returned no match.
- **Cache ABI, ledger taxonomy, #5a, and split-row fixes:** present in the candidate and consistent with the corrected prose.
- **Remaining round-1 gaps:** not fixed—only recorded. Ledger scope, topology results, performance thresholds, and rows #25–26 governance still gate TCM0 acceptance.

## Validator, digests, and framework-contract preservation

Exact validator output:

```text
VIOLATION: state block BV2 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block B5 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
VIOLATION: state block CM1 is ACCEPTED but context_packet_digest is not a non-empty 64-char lowercase SHA-256: ""
FAIL: 3 violation(s) in docs/arch/architecture-lock/ledger/program-state.toml against docs/arch/refactor/rev11/program-dag.toml (mode live)
```

Exit code: `1`. All three empty digests are also present at baseline `64234ab14`, so the candidate narration is accurate and this is not a candidate-introduced regression.

I recomputed every newly/currently pinned TCM digest, not merely two. All seven match `authority-registry.toml`: TCM0–TCM4 charters, TCM steering, and package-certification ruling. The steering’s “verbatim” claim also passed an independent comparison: removing the 24-line header and comparing against `<external steering source outside this repository>` returned `cmp_exit=0`.

Framework contracts are byte-preserved: the `crates`, `packages`, and `scripts` Git tree hashes are identical between baseline and candidate, and the range contains zero non-`docs/` paths. Thus the accepted Vue/Svelte outputs, published code surfaces, and CSS behavior are unchanged by this integration.

## Blocking findings

Finding: TCM0’s exact package and semantic-API certification is incomplete.  
Severity: BLOCKING  
Candidate cause: Required package/API probes were delegated to TCM2/TCM3, while TCM0’s literal charter assigns them to TCM0; the probe harness/transcript was not committed.  
Authority/charter requirement violated: TCM0 Scope 1–2; steering “Exact upstream/package lock” and “Semantic API certification.”  
Affected behavior/invariant: Exact codec fidelity, project-mode behavior, semantic-session correctness, and safe package certification are not proven.  
Evidence/reproduction: Exact wire spelling and attach-session probe are explicitly open at [package-lock-and-semantic-api.md:90](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:90) and [line 167](docs/arch/refactor/rev11/evidence/TCM0/package-lock-and-semantic-api.md:167); all four named probe scripts are absent.  
Minimum correction condition: Record the exact request/response and manifest contract, configured/inferred/build/watch/declaration behavior, execute the attach-hang and full semantic correctness probes against the exact candidate, and commit reproducible evidence.

Finding: The feature-ownership ledger and rows #25–26 disposition are incomplete.  
Severity: BLOCKING  
Candidate cause: Inventory stopped at the 44 trait methods, and the maintainer ruling was converted into a non-circular gate but was not actually obtained.  
Authority/charter requirement violated: TCM0 Scope 3 and Acceptance prohibitions A3/A5.  
Affected behavior/invariant: Some capabilities have no proven owner; rows #25–26 cannot legally be retained or deleted.  
Evidence/reproduction: [OPEN-GAPS.md:12](docs/arch/refactor/rev11/evidence/TCM0/OPEN-GAPS.md:12), [feature-ownership-ledger.md:95](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:95).  
Minimum correction condition: Classify every steering-listed capability row-by-row and land a digest-bound maintainer ruling assigning rows #25–26 either approved-disabled/deletion or `VerterWithTypeSemanticOracle` ownership.

Finding: The terminal projection-mask policy is not fully determined.  
Severity: BLOCKING  
Candidate cause: TCM0 ratified class names and examples but deferred per-row classification and omitted an exhaustive derivation.  
Authority/charter requirement violated: TCM0 Scope 5.  
Affected behavior/invariant: TCM2 cannot deterministically compute an explicit legal mask for every wire span without making new policy decisions.  
Evidence/reproduction: [projection-class-contract.md:86](docs/arch/refactor/rev11/evidence/TCM0/projection-class-contract.md:86), [feature-ownership-ledger.md:57](docs/arch/refactor/rev11/evidence/TCM0/feature-ownership-ledger.md:57).  
Minimum correction condition: Supply an exhaustive table or closed algorithm for every relevant class × relation × region × owner × certified-capability combination, including explicit zero/disabled masks.

Finding: Required topology results and locked performance baselines do not exist.  
Severity: BLOCKING  
Candidate cause: A benchmark plan and qualitative requirements were recorded in place of completed comparisons and numeric gates.  
Authority/charter requirement violated: TCM0 Scope 7 and 10.  
Affected behavior/invariant: No evidence-based topology selection exists, and later implementation results could influence thresholds.  
Evidence/reproduction: [topology-benchmark-plan.md:67](docs/arch/refactor/rev11/evidence/TCM0/topology-benchmark-plan.md:67), [performance-baselines.md:46](docs/arch/refactor/rev11/evidence/TCM0/performance-baselines.md:46).  
Minimum correction condition: Run all required projection/semantic topology cells, select the non-dominated topology from recorded measurements, and lock the complete equivalent-work numeric gate table before TCM1–TCM4 implementation results are observed.

Finding: Deletion closure explicitly defers part of its inventory to TCM4.  
Severity: BLOCKING  
Candidate cause: Items 17–18 were left as execution-time discovery categories.  
Authority/charter requirement violated: TCM0 Scope 9: “Name every mechanism … Not deferred to TCM4.”  
Affected behavior/invariant: TCM4 lacks an exact, pre-ratified deletion manifest and must re-derive part of it during execution.  
Evidence/reproduction: [deletion-closure.md:54](docs/arch/refactor/rev11/evidence/TCM0/deletion-closure.md:54).  
Minimum correction condition: Enumerate the current old-route-only APIs/DTOs and provide a concrete historical-codec absence/deletion inventory before TCM0 acceptance.

Finding: TCM1’s claimed “full” string-surface inventory is internally inconsistent and contains stale citations.  
Severity: BLOCKING  
Candidate cause: A lower-bound inventory was converted into an exact 13-field acceptance universe without reconciling its own list against source.  
Authority/charter requirement violated: TCM1 owned scope 2 and exit criterion 3a; integration claim that the rewritten charters are implementation-ready.  
Affected behavior/invariant: TCM1 can satisfy “all thirteen” while leaving string-encoded mapping fields unmigrated.  
Evidence/reproduction: [mapping-products-string-surface.md:58](docs/arch/refactor/rev11/evidence/TCM0/mapping-products-string-surface.md:58) enumerates eleven compiler-side names plus four protocol fields, while [TCM1.md:69](docs/arch/refactor/rev11/charters/TCM1.md:69) calls the total thirteen. The cited `VerterTsxBlock.source_map` is at [compile/types.rs:495](crates/verter_compiler/src/compile/types.rs:495), not line 497; several recorded struct names also do not exist verbatim.  
Minimum correction condition: Regenerate a complete field inventory from the candidate source, correct all names/locations/counts, make the TCM1 exit universe match that inventory, and recompute affected charter digests.
