---
ruling_id: "BETA4-REGRESSION-INTAKE"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["program-wide (release boundary)", "BV2", "CM1"]
source_file: "MAINTAINER-DIRECTIVE-BETA4-REGRESSION-INTAKE.md"
summary: "RATIFIED program-level regression intake after an independent benchmark run (pikax/vue-benchmarks) found beta.4-vs-beta.3 regressions on Windows/Node/rustc release builds. Verifies and classifies Findings A (panic! escalation in template/code_gen/types.rs), B (UnraisableSource in meta_resolve/output.rs), and C (runtime prop constructor lowering) as correctness discoveries and beta.4 release blockers. Dispatches two bounded read-only root-cause investigations (Finding A; Findings B+C) under strict standing constraints (no panic swallowing, no invented types, no benchmark/fixture special-casing) — governance intake (DAG edges, charters) follows root cause rather than preceding it."
supersedes: []
superseded_by: []
contradicts: []
notes: "This directive's Finding A investigation is what ARCH-RULING-BV2-FINDING-A-REPAIR-AND-PLACEMENT.md answers (creating block BV2); its Findings B/C investigation is what ARCH-RULING-CM1-FINDINGS-BC.md answers (creating block CM1, and finding the benchmark's UnraisableSource hypothesis in this directive does NOT match either reproduced defect — see CM1-FINDINGS-BC's CONTRADICTIONS field)."
---

# Maintainer directive — beta.4 regression intake and release boundary

**Status: RATIFIED, 2026-08-20.** Program-level regression intake ordered by the maintainer after an
independent agent ran `pikax/vue-benchmarks` @ `c89e6c34fa3b34fae5cc3e91aac4e8018fc5905c` against
Verter `6ab1000bd` versus published `@verter/*@0.0.1-beta.3` on Windows 11 / Node 26.5.0 / rustc 1.97.1,
release builds, same machine, ~4h apart.

## §1 state capture — COMPLETE, recorded here

| item | value |
|---|---|
| `origin/program/architecture-lock` | `6ab1000bd6542101e663d388b0ba20f1485d1e5c` |
| tree | `b53db4ba21b553d310483393be38409dee4f4ac0` |
| local trunk | IDENTICAL to origin |
| directive's tested commit | **is the current tip** — the regressions are in landed, published-adjacent code |
| ledger | 20 ACCEPTED, 1 IN_PROGRESS (BS1), 1 READY (C1), 38 LOCKED, 2 SUPERSEDED |

### BS1 identity — PROTECTED, not modified
- reviewed candidate `9786e756b`, branch tip `a48d92e82`, base `f46de1b6a` (3 commits behind trunk)
- **deliberately NOT rebased**: the adversarial attestation is bound to `9786e756b`
- tags `protect/bs1-reviewed-candidate` and `protect/bs1-branch-tip` pin both
- BS1's ledger row is EMPTY (no candidate_sha, all reviews PENDING) because BS1 never reached
  acceptance. "BS1 has advanced beyond the ledger" is TRUE by design, not by drift.

## Code pointers — ALL VERIFIED PRESENT at `6ab1000bd`

- **Finding A** — `panic!` escalation confirmed at `template/code_gen/types.rs:712`, draining
  `segmented_overwrites` through `try_overwrite_segmented`. Both `ReplacedContentSplit` producers exist:
  `code_transform/segmented.rs:70` and `:107` — matching the directive's "anchored replacement already
  present" vs "no sole containing `Original` chunk".
- **Finding B** — `UnraisableSource` at `meta_resolve/output.rs:490`; the exact message *"the source has
  no live graph representation under the request view"* at `:563`; the `exposed[].type` lane at `:376`.
- **Finding C** — runtime prop constructor lowering lives in
  `verter_semantic/src/analysis/component_meta.rs` and `analysis/macros.rs`.

The directive's evidence is accurate in every particular checked.

## Classification (§4.1)

Findings A, B and C are **correctness discoveries and beta.4 release blockers** — regressions against
published beta.3 on valid source inputs and claimed product modes.

## Sequencing decision

Per §9 ("perform a bounded root-cause investigation before final block assignment"), governance intake —
BV2/CM1 identifiers, DAG edges, charters — FOLLOWS root cause rather than preceding it. Two bounded
read-only investigations were dispatched at `6ab1000bd`, each carrying the directive's hard prohibitions
verbatim and each forbidden from implementing a repair:

- Finding A — reproduce in-repo, instrument the edit plan, identify which `ReplacedContentSplit` path
  fires, report repair classes without choosing by "what stops the panic".
- Findings B and C — reproduce in-repo (the benchmark repo is NOT available locally and the fix must not
  depend on it), answer every root-cause question in §9.1, and rule specifically on whether each is
  fixable under the current architecture (Path A → CM1) or genuinely requires C1's request-view rework
  (Path B → amended C1).

## Standing constraints carried into both dispatches

Finding A: no restoration of the removed whole-block overwrite fallback (it destroyed provenance and
could emit silently wrong mappings); no panic swallowing; no converting the invariant violation into an
apparent success; no global disabling of comment removal or static-class optimization; no benchmark
marker / fixture-name / allowlist special-casing; no lowering of source-map correctness.

Findings B/C: the strict output error is PREFERABLE to an invented type — no `unknown` substitution, no
omitting exposed entries, no swallowing, no stale graph state, no bypassing request-view isolation, no
retaining a representation beyond its lifetime. No `String -> string` mapping at the output seam before
tracing semantic ownership.

## Not yet done (tracked)

DAG/charter amendments (BV2, CM1-or-amended-C1), discovery-ledger rows, the beta.4 gate record, the
performance intake (host lint at 4.49× is the priority outlier), and the benchmark known-failure
revalidation. All follow the root-cause results.
