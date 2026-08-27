# Architecture ruling — C1 final round-2 performance disposition

Status: **RATIFIED — exact-subject performance acceptance authorized.** C1 remains `IN_PROGRESS` until
registration, post-registration freeze, and every retained gate closes.

## Dispatch record

- Lane: `c1-final-round2-performance-architecture-disposition`
- Authority role: fresh, neutral Revision 11 architecture authority
- Execution mode: read-only
- Reviewed subject: `e0d6732a26ce3bb4a3a458ae8c2c484fd42fdc7a`
- Reviewed tree: `f45176eb1fc7f517a9d2efd6c3a1d2801e8b4172`
- Dispatch-prompt SHA-256: `f4810b898f47e6c938e09315d7070425a20ac73c338a4e69b19f202795a48527`
- External raw-output SHA-256: `0456148169cbc6e64c2f2bb2741254d9ac30caa56406c8aa7b1cf7358b9ff4f7`

The raw authority output remains external as a digest-bound artifact. This portable registration contains no
machine-local artifact path.

## Bound identities

- Production subject: `e0d6732a26ce3bb4a3a458ae8c2c484fd42fdc7a`
- Production tree: `f45176eb1fc7f517a9d2efd6c3a1d2801e8b4172`
- Evidence-only child: `0be9f2e867c634926c646b52fba75e9e7b60bc59`
- Evidence-only tree: `7ca20dde813ba73c03ba252c7802fd59fc68d4fe`
- Comparison base: `d1f3d50a948597f036868543b9bb21acacd730ff`
- Comparison tree: `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`
- Round-2 fix evidence SHA-256: `8c23395e0ec66dcbc192e9cafdcbe596ef41093d39f3f9de21f7b0e0b4ef2291`
- Performance evidence SHA-256: `5ecdf61545954f4c44d90a5e716ec0d882728d07b7624b831992b36fd34cf292`
- External 66-entry raw-manifest SHA-256: `91cb61887d97c6baba21cbb1cf9ba2e98a7b0ee640313e6354799ca54890b91b`
- Harness: Git blob `efa9ea54a14772ecd87511d6bb07017aa33940ba`, SHA-256
  `5e06d35dda284a8ef049bf0dd3dc39974b904729f740da58c650ec59e806f632`

The evidence-only child is a direct child of the production subject and adds only
`final-round2-performance.md`.

## Literal results

| Gate | Result |
|---|---|
| Relative wall | `88.440 → 98.805 ms`, `+11.719810041% > 3%`: **FAIL** |
| Absolute wall | `98.805 ms <= 100 ms`: **PASS** |
| Allocation count | `477,383 → 595,309`, `+24.702597286%`: **FAIL** |
| Allocation bytes | `104,078,206 → 137,007,048`, `+31.638556491%`: **FAIL** |
| Control drift | `1.009874327% <= 3%`: **PASS** |
| Absolute RSS | `77,512,704 B <= 268,435,456 B`: **PASS** |
| Frozen-relative RSS | `+3.556966181% <= 4.952%`: **PASS** |
| Completeness | `10/10` invocations, `30/30` samples, `40/40` warmup: **PASS** |
| Work/output | All configured work counters, zero counters, admissions, and component digest `7161214711717846280`: **PASS** |

The failures remain failures; no threshold or evidence is relabelled.

## Exact disposition

1. Issue `C1-A6-WALL-REL-003` for this base/production-subject pair only. It covers only the locked
   `wall_ns` median relative limit and the literal `+11.719810041%` failure. It supersedes
   `C1-A6-WALL-REL-002` only as current C1 coverage and supersedes `post_result_exception_allowed = false`
   only for this exact ID and subject.
2. Issue `C1-APM002-ALLOC-REL-002` for exactly the allocation count and byte comparisons above. It covers
   no retention, leak, RSS, absolute-memory, or future-subject result.
3. Do not issue `C1-A6-CONTROL-STABILITY-002`. The current session passes control stability; the
   maintainer's control-waiver authority is unnecessary here. `C1-A6-CONTROL-STABILITY-001` remains
   historical and exact to its older session.
4. Preserve all older dispositions and their identities unchanged. None is restamped onto this subject.
5. Order no further C1 optimization. The round-2 changes are mandatory correctness, ownership, retry,
   confinement, and stale-output fixes; they strengthen whole-restart/output-discard semantics, introduce
   no continuation or second resolver, and preserve equivalent measured work and output. They do not
   invalidate the architectural basis for deferral.

## Retained non-waivable gates

Still blocking are correctness and result equivalence; ordered `LoadSet`, wave, observation, replay, and
witness semantics; whole-restart and output-discard; APM-001 budgets and non-admission; absolute wall;
absolute and relative RSS; every configured work and zero counter; component digest; single resolver
authority; bounded ownership and retention; code quality, TDD, and discrimination mutations; C-train scope;
canonical gate and health checks; three reviews on one exact frozen candidate; independent verifier; landing
equivalence; atomic landing; and maintainer acceptance.

## Successors and exit

`C-TRAIN-END-PERFORMANCE-CONSOLIDATION-001` remains the single non-rollable comparative-performance
successor. Owner: C4/C-train-close authority through one maintainer-designated end-of-train tranche, with no
new DAG block. The existing record must be extended in place to include these exact wall and allocation
limitations, without creating a duplicate.

Before C4/C-train acceptance, one frozen full-train subject must run the applicable locked cells, close the
carried relative-wall and APM-002 allocation count/byte regressions under equivalent work and output, and keep
every absolute, correctness, counter, digest, RSS, code-quality, and scope gate green. It cannot roll beyond
C-train close.

`C2-AC-C1-A6-CONTINUATION-001` remains a separate existing semantic-continuation obligation; this ruling
neither satisfies nor cancels it and authorizes no continuation work inside C1.

## Inheritance and operative acts

The evidence-only child `0be9f2e867c634926c646b52fba75e9e7b60bc59` may inherit this disposition
through an explicit content-equivalence bridge. It must never be described as the measured production
subject. A later registration-only descendant may likewise inherit only if production, harness, corpus,
toolchain, configuration, and raw evidence remain unchanged.

Minimal acts:

1. Record this as a new digest-bound ruling; do not edit the registered older final-subject ruling.
2. Append its document ID, `C1-A6-WALL-REL-003`, and `C1-APM002-ALLOC-REL-002` to the sole C1 authorization.
   Bind the base, production subject, evidence child, evidence digests, 66-entry manifest, literal results,
   and successors.
3. Update C1 program state in place: production identity
   `e0d6732a26ce3bb4a3a458ae8c2c484fd42fdc7a`/`f45176eb1fc7f517a9d2efd6c3a1d2801e8b4172`,
   status `IN_PROGRESS`, reviews `PENDING`, reviewed-SHA fields empty, acceptance unset.
4. Extend the existing C4 successor note once with the current limitations; create no second successor.
5. Validate live program state, authority registration, performance-gate configuration, and source policy.
6. Freeze one post-registration SHA/tree descended only through evidence/authority acts. All three reviews,
   the verifier, canonical gate, and landing-equivalence proof must bind that exact freeze.
7. No further performance measurement is required absent a material change.

```text
LANE: c1-final-round2-performance-architecture-disposition
REVIEWED_SHA: e0d6732a26ce3bb4a3a458ae8c2c484fd42fdc7a
REVIEWED_TREE: f45176eb1fc7f517a9d2efd6c3a1d2801e8b4172
VERDICT: AUTHORIZE_EXACT_SUBJECT_PERFORMANCE_ACCEPTANCE; REGISTRATION_AND_POST_REGISTRATION_FREEZE_REQUIRED; C1_REMAINS_IN_PROGRESS_PENDING_RETAINED_GATES
WAIVERS/DEFERRED: C1-A6-WALL-REL-003=+11.719810041% FAIL WAIVED FOR EXACT SUBJECT; C1-APM002-ALLOC-REL-002=count +24.702597286% AND bytes +31.638556491% FAIL WAIVED/DEFERRED FOR EXACT SUBJECT; C1-A6-CONTROL-STABILITY-002=NOT_ISSUED_CURRENT_SESSION_PASS_1.009874327%; C-TRAIN-END-PERFORMANCE-CONSOLIDATION-001=DEFERRED_NON_ROLLABLE
RETAINED_GATES: CORRECTNESS; DIGEST; ORDERED_LOADSET/WAVE/OBSERVATION/REPLAY/WITNESS; APM001_BUDGETS_AND_NON_ADMISSION; WHOLE_RESTART_AND_OUTPUT_DISCARD; ABSOLUTE_WALL_100MS; ABSOLUTE_RSS_268435456B; RELATIVE_RSS_4.952%; ALL_CONFIGURED_WORK_AND_ZERO_COUNTERS; COMPONENT_DIGEST; SINGLE_AUTHORITY; BOUNDED_RETENTION; CODE_QUALITY_TDD_MUTATIONS; C_TRAIN_SCOPE; CANONICAL_GATE; THREE_EXACT_CANDIDATE_REVIEWS; INDEPENDENT_VERIFIER; LANDING_EQUIVALENCE; ATOMIC_LANDING; MAINTAINER_ACCEPTANCE
SUCCESSOR: C-TRAIN-END-PERFORMANCE-CONSOLIDATION-001 owner=C4/C-train-close-authority exit=one frozen full-train subject closes carried relative-wall and allocation count/byte regressions under equivalent work/output with all retained gates green before C4/C-train acceptance and cannot roll; C2-AC-C1-A6-CONTINUATION-001=UNCHANGED_SEPARATE_SEMANTIC_OBLIGATION
OPERATIVE_ACTS: RECORD_NEW_DIGEST_BOUND_RULING; APPEND_TO_SOLE_C1_AUTHORIZATION_WITHOUT_RESTAMPING_HISTORY; BIND_BASE_PRODUCTION_SUBJECT_EVIDENCE_CHILD_REPORT_AND_66_ENTRY_MANIFEST; UPDATE_C1_LIVE_IDENTITIES_KEEP_IN_PROGRESS_AND_REVIEWS_PENDING; EXTEND_EXISTING_C4_SUCCESSOR_ONCE; VALIDATE_LIVE_STATE_AUTHORITY_PERFORMANCE_CONFIG_AND_SOURCE_POLICY; FREEZE_POST_REGISTRATION_DESCENDANT; RUN_THREE_REVIEWS_VERIFIER_CANONICAL_GATE_AND_LANDING_EQUIVALENCE
RATIONALE: The round-2 production changes are required correctness and architectural-confinement fixes, strengthen restart/discard behavior, and preserve equal work and output. Absolute wall/RSS, relative RSS, counters, digest, control stability, and completeness pass. The remaining wall and allocation regressions are literal comparative failures after bounded safe attempts; exact-subject acceptance plus the existing non-rollable C-train-close successor is the narrowest lawful disposition.
```
