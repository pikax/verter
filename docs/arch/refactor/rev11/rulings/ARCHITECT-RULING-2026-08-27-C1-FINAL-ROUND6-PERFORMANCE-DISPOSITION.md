---
ruling_id: "C1-FINAL-ROUND6-PERFORMANCE-DISPOSITION-2026-08-27"
type: "architecture-ruling"
date: "2026-08-27"
date_source: "stated"
binds: ["C1", "C4"]
source_file: "ARCHITECT-RULING-2026-08-27-C1-FINAL-ROUND6-PERFORMANCE-DISPOSITION.md"
summary: "Authorizes four non-transferable quantitative dispositions for exact C1 production subject 2820cf2eb790caffdb69f59bc20402d7d0a6647b while retaining every literal failure and locked threshold; carries relative wall, absolute wall and APM-002 allocation closure to the sole non-rollable C4/C-train-close obligation; changes no code, scope, DAG or CompileArtifactSet surface."
supersedes: []
superseded_by: []
contradicts: []
notes: "Direct exact-subject user waivers cover final-session control drift below 10%, relative wall and absolute wall; allocation count/bytes are accepted for C1 only and deferred to end of C train. Correctness, resource correctness, RSS, counters, admissions, digest and code quality remain non-waivable."
---

# Architecture ruling — C1 final round-6 quantitative performance disposition

Status: **RATIFIED — exact-subject quantitative acceptance authorized.** C1 remains `IN_PROGRESS`;
this act clears only the quantitative performance blockers named below and does not accept or land the
block.

## Bound identities

- Production subject: `2820cf2eb790caffdb69f59bc20402d7d0a6647b`
- Production tree: `ef8efbec06c8e87d1d6d72d9ea8e69fa624f515b`
- Evidence-only child: `713651edd3c9ab629ea5c68380238fb4bafa6711`
- Evidence-only tree: `1e112ba5218803e6166c2f4065732179fa339ebe`
- Comparison base: `d1f3d50a948597f036868543b9bb21acacd730ff`
- Comparison tree: `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`
- Evidence report: `docs/arch/refactor/rev11/evidence/C1/a6/final-round6-performance.md`
- Evidence report SHA-256: `a6083c355239afa6216352f5f78bc36b8c3df9ad2fc3845a32b79e76283b0136`
- External 149-entry raw-manifest SHA-256:
  `11c016f9a80ea8c8ffc16d564e69a4eb52b8cc426deb4e60b9b18cd9eff86892`
- Harness Git blob: `efa9ea54a14772ecd87511d6bb07017aa33940ba`
- Harness SHA-256: `5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632`

The evidence-only child is a direct child of the production subject and adds only the report above. It
is not the measured production subject.

## Literal results

| Gate | Round-6 result |
|---|---|
| Control stability, final session | `+3.096636412% > 3%`: **FAIL**; below the user-authorized `<10%` exact-session ceiling |
| Relative wall | `95.710 -> 111.895 ms`, `+16.910458677% > 3%`: **FAIL** |
| Absolute wall | `111.895 ms > 100 ms`: **FAIL** |
| Allocation count | `477365 -> 604706`, `+26.675814105%`: **FAIL** |
| Allocation bytes | `104030662 -> 137711352`, `+32.375733608%`: **FAIL** |
| Absolute RSS | `76709888 B <= 268435456 B`: **PASS** |
| Frozen-relative RSS | `+2.484404071% <= +4.952%`: **PASS** |
| Work, admission, zero counters and digest | **PASS**; component digest `7161214711717846280` |

Session 1 independently corroborates the wall and RSS direction but carries no additional acceptance
effect. No result is relabelled and no threshold is changed.

## Exact disposition

1. Issue `C1-A6-CONTROL-STABILITY-003` for the complete final round-6 session and exact subject above.
   It covers only the observed `+3.096636412%` control-drift failure under the user's exact-C1 `<10%`
   validity waiver. It does not change the locked `3%` control threshold or authorize a future session.
   `C1-A6-CONTROL-STABILITY-002` remains the historical not-issued ID for round 2.
2. Issue `C1-A6-WALL-REL-004` for only the `+16.910458677%` relative-wall failure above. It supersedes
   `C1-A6-WALL-REL-003` only as current C1 coverage; every older result and disposition remains historical
   and unchanged.
3. Issue `C1-A6-WALL-ABS-001` for only the `111.895 ms > 100 ms` absolute-wall failure above. This is a
   non-transferable direct-user exception for this exact C1 subject, not a prospective permission to waive
   absolute latency.
4. Issue `C1-APM002-ALLOC-REL-003` for exactly the allocation-count and allocation-byte comparisons above.
   It accepts those comparative failures for this C1 landing only and carries their closure to the existing
   non-rollable `C-TRAIN-END-PERFORMANCE-CONSOLIDATION-001` obligation. It covers no retention, leak, RSS,
   absolute-memory, correctness, or future-subject result.
5. Order no further C1 optimization or measurement. The exact production subject already exhausted the
   bounded safe-pass policy. Comparative optimization belongs to the existing end-of-C-train tranche.

All four IDs are invalid for any different production tree, harness, corpus, toolchain, configuration,
raw evidence, or result. The locked thresholds remain unchanged and literal failures remain failures.

## Retained gates and review consequence

Correctness and result equivalence, resource correctness, bounded retention, absolute and relative RSS,
configured work and zero counters, admissions, digest, one semantic/resolution meaning, distinct host and
session lifecycle adapters, one resolver authority, code quality, C1 scope, the canonical gate, exact-review
identity, landing equivalence, atomic landing and maintainer acceptance remain non-waivable.

Three independent reviews of the exact production subject reported PASS with zero P0/P1, including the
single-meaning, distinct-lifecycle-adapter and no-alternate-core predicates. They support this quantitative
disposition but do not populate the ledger's final review fields: the evidence/registration descendant is a
different SHA/tree, so C1 remains `IN_PROGRESS` with reviews `PENDING` until the landing authority binds the
final post-registration freeze under the normal equivalence rules.

## End-of-C-train obligation

Extend `C-TRAIN-END-PERFORMANCE-CONSOLIDATION-001` in place; do not create another obligation or DAG block.
Owner remains C4/C-train-close authority through one maintainer-designated tranche. Before C4/C-train
acceptance, one frozen full-train subject must:

- run the applicable unchanged locked cells in a session satisfying the original `3%` control fence;
- restore relative wall to `<=3%` and absolute wall to `<=100 ms`;
- close the carried APM-002 allocation count/byte regression under equivalent work and output; and
- keep correctness, counters, admissions, digest, absolute/relative RSS, resource correctness, code quality
  and scope green.

The obligation cannot roll beyond C-train close. `C2-AC-C1-A6-CONTINUATION-001` remains a separate semantic
obligation; this ruling neither satisfies it nor authorizes Compiler V2, C2/C4 implementation,
`CompileArtifactSet`, scope, code, or DAG changes.

## Operative acts

Digest-register this ruling and append it to the sole C1 authorization. Bind the exact identities, evidence
digest, manifest digest, literal results and exception IDs above. Update C1's live production/evidence
identities while keeping reviews and acceptance unset. Extend the sole C4 state record in place. No other
artifact changes are authorized.
