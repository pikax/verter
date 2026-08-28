# C1 A6 unblock architecture consult

## Prompt metadata

- Task: `/root/c1_a6_unblock_architect`
- Sender: `/root`
- Date: `2026-08-26` (`Europe/Lisbon`)
- Repository: `/Users/carlosrodrigues/Documents/dev/verter-c1`
- Requested and observed candidate: `6eb7b834ceea2e474b4de1ddc7c2eec945f9e914`
- Observed worktree before this report: clean
- Mode: fresh read-only architecture/diagnostic; no production mutation and no heavy suite; this report is the only authorized write
- Question scope: C1 A6 normalization only; unrelated C1 findings are excluded

Exact task prompt:

> You are a fresh, read-only C1 unblock architect/diagnostic. Modify nothing and run no heavy test suite. Work against /Users/carlosrodrigues/Documents/dev/verter-c1 at exact HEAD 6eb7b834ceea2e474b4de1ddc7c2eec945f9e914. Read CLAUDE.md, .claude/skills/type-resolution/SKILL.md, .claude/skills/type-cache-architecture/SKILL.md, .claude/skills/rust-performance/SKILL.md, C1 charter/addendum/Stage-2/landing ruling, orchestration delivery/review, and the compact receipt .feedback/c1-recovery-implementer-receipt.md plus C1 A6 evidence.
>
> Question only: Is there a production-ready, ratified C1-local way to reduce A6 normalize_canonical_id.calls from 17,349 to the locked <=11,313 without cross-request unbounded caching, a new cache/retention authority, metric reweighting, semantic/output/load-set/witness changes, or charter expansion? Inspect the current request-local ResolveFrame implementation and the pre-cutover/base resolver behavior to identify exact remaining call sites and an ownership-correct correction. Distinguish duplicated normalization from legitimate first-use normalization. If a bounded existing owner cache can lawfully be used, cite the exact owner/retention/key mechanism already ratified; do not handwave. If no legal path exists, prove why and state which existing authority/lock must change rather than proposing implementation.
>
> Also classify whether the blocked count is a correctness/architecture defect inside C1 or a locked A6 baseline/threshold mismatch, using concrete evidence. Do not review unrelated C1 items.
>
> Persist the exact prompt metadata and complete report at /Users/carlosrodrigues/Documents/dev/verter-c1/docs/arch/refactor/rev11/evidence/C1/a6/unblock-architecture-consult.md. End with a compact receipt: RESULT FIX_PATH|NO_PATH|INCONCLUSIVE; exact candidate SHA; named files/functions; permitted change boundary; acceptance/mutation checks required. Return that compact receipt only.

## Materials inspected

The consult read the requested project references in full: `CLAUDE.md`, the type-resolution, type-cache-architecture, and Rust-performance skills; `charters/C1.md`; `rulings/ARCH-ADDENDUM-C1-THREE-GAPS.md`; the C1 Stage-2 cutover and landing-path rulings; orchestration delivery/review; `.feedback/c1-recovery-implementer-receipt.md`; and all six files in `evidence/C1/a6/`. It also inspected the current production driver and resolver call graph and the base resolver at `d1f3d50a948597f036868543b9bb21acacd730ff`.

No production file was changed. One focused existing test was run, not a suite:

```text
cargo nextest run -p verter_semantic one_resolution_frame_reuses_normalized_geometry_across_input_waves --no-capture
```

It passed. Its counter discrimination was one-shot retry waves `[8, 8, 8, 10]` versus one retained frame `[8, 0, 0, 2]`. This proves that the recovery removed retry-prefix re-execution, but also proves that first materialization/late consumers still perform normalization after the initial seed.

The exact committed attributed binary (`sha256 3603084a134ee9669e96ad8c2179517b972a4e6f1f7ab0d3e28a2d57b31b1df0`) was invoked with smaller corpus sizes only. Its normalization count is exactly linear after the first component:

| `--files` | calls |
|---:|---:|
| 0 | 13 |
| 1 | 540 |
| 2 | 971 |
| 3 | 1,402 |

Thus the recorded 40-file result is `13 + 527 + 39*431 = 17,349`; this is deterministic work, not measurement noise.

## Ruling

**RESULT: FIX_PATH.**

There is a C1-local, already-authorized correction. It does not require cross-request reuse, a new store, a retention policy, a metric change, or a semantic/load/witness change. The correction is to preserve canonical provenance through the existing request-local `ResolveFrame::memo` and through workspace replay of the frame's already-canonical observation keys. The current code discards that provenance and calls the canonicalizer again at several exact boundaries.

This is not a ruling that the proposed edit is already measured green. The locked A6 run on the exact corrected SHA remains a conjunctive acceptance condition. It is a ruling that a lawful production path remains and that the previous receipt's statement that request-local correction is “exhausted” is false.

## Locked evidence and classification

| observation | base | current recovery candidate |
|---|---:|---:|
| `workspace.normalize_canonical_id.calls` | 11,313 | 17,349 |
| normalized bytes | 175,101 | 314,481 |
| `session.semantic_dispatch.calls` | 4,216 | 4,216 |
| `session.semantic_cold_build.calls` | 1,063 | 1,063 |
| `session.cache_admit_cacheable.calls` | 1,063 | 1,063 |
| component-meta digest | `7161214711717846280` | `7161214711717846280` |

The gap is exactly 6,036 calls and 139,380 bytes. The excess input averages 23.092 bytes per call, compared with 15.477 bytes per base call. That is consistent with repeated full canonical paths/prefixes, not merely required normalization of short raw path fragments.

This is a **C1 performance/ownership architecture defect**, not a semantic correctness failure and not a baseline/threshold mismatch:

1. Result digest and the three locked semantic/build/admission call counts are equal.
2. The count is deterministic and linear on the exact candidate binary.
3. C1 explicitly requires that identity construction add no normalization pass and that locked A6 not regress (`C1.md` A6 section; Stage-2 `S2-R6`). `S2-F6` forbids reweighting or reinterpretation.
4. The locked base is the actual pre-cutover resolver and performs the equivalent live resolution. Its 11,313 is therefore a real construction-work ceiling, not a statistical timing assumption.
5. The new staged implementation introduced canonical values that cross more typed boundaries. Re-canonicalizing those values is implementation overhead, not indispensable semantic work.

## Exact remaining sites

### Existing request owner and lifecycle

`crates/verter_workspace/src/resolver.rs::{resolve_tracked,resolve_for_project_tracked}` creates one `ResolveFrame` before `drive_attempt`; all production attempts use `ResolveFrame::attempt`. The frame and its `ResolutionStringMemo` are dropped when that top-level resolution returns. `ResolveFrame::attempt` calls `ResolutionStringMemo::clear` when `ResolutionBasis` changes. This is the exact owner and retention boundary already present in production.

Its existing keys are:

- `normalized`: exact input string → canonical string;
- `joined`: exact base → exact relative part → canonical result;
- `parents`: exact path → canonical parent;
- `probe_candidates`: `(apply_source_sibling, prefers_declarations)` → canonical base → ordered canonical candidates;
- `package_paths`: canonical package directory → target → optional capture → canonical result.

The C1 memo architecture consult expressly permits request/`ResolveFrame`-local pure derivation reuse. No new owner or map is needed.

### Duplicated normalization

The following are canonical-provenance losses, not legitimate first use:

1. `ResolutionStringMemo::normalize` stores `raw → canonical` but not `canonical → canonical`. A later consumer of the returned value misses the same request-local map.
2. `ResolutionStringMemo::join` returns `join_paths`' canonical result but records it only under the compound join key, not in `normalized`.
3. `ResolutionStringMemo::parent` returns the normalized parent but records it only in `parents`.
4. `ResolutionStringMemo::probe_candidates` derives candidates by suffix replacement/append or fixed `index` append from a canonical base. Those outputs remain canonical, but none is registered as such.
5. `ResolveFrame::attempt` calls `seed_precomputed_geometry` before the three lazy `OnceCell`s materialize. `project_geometry`, `node_modules_directories`, and `imports_directories` build canonical geometry after that seed and never seed the new values. This ordering was introduced by `5b3b58dd9fb6587e5a3a6bd64db55515a0294c91`; before that lazy cut, the precomputed cells existed when seeding ran.
6. `build_project_geometry::mapping` calls `normalize_canonical_id` on a candidate produced by `join_paths` or `apply_tsconfig_target`, both of which already return canonical output.
7. `ResolutionStringMemo::join` delegates wholesale to `join_paths`. For a normal relative join, `join_paths` re-normalizes both base and relative part on every distinct compound join. The memo already owns exact normalization of those components; it can use `normalize` for each component, retain the required first collapse of the combined path, and register the canonical result. This preserves the byte result while avoiding repeated component work.
8. `crates/verter_workspace/src/resolver.rs::apply_attempt_output` replays `ConsumedResolutionObservationKey` values documented and constructed as canonical IDs into `TransactionReader::record_snapshot_*`. `crates/verter_workspace/src/resolution_currency.rs::{ResolutionTransaction::observe_path,observe_realpath,observe_manifest,TransactionReader::record_snapshot_recovery_scope}` then canonicalize those canonical values again. The base live reader had to normalize raw reader ingress; the staged replay has stronger provenance and should use a private canonical replay entry point. The consumed-observation vector, its order, and every recorded fact remain unchanged.

Items 1–7 belong to the current request-local derivation owner. Item 8 belongs to the existing workspace transaction/fact-capture owner. Neither is a reusable cross-request cache.

### Legitimate first use that must remain

The correction must retain normalization for:

- raw request importer/specifier ingress and raw absolute specifiers;
- raw project roots, alias replacements, base URLs, tsconfig targets, manifest targets, and filesystem/realpath results at their first untrusted boundary;
- a relative fragment at its first use in a frame;
- the first collapse of a newly combined path containing possible `.` or `..` segments;
- the first use in every independent `ResolveFrame` and after every basis change;
- raw non-frame workspace transaction ingress.

The ordered priority frontier's bounded sibling evaluation is also legitimate: its load set and consumed witness are locked and cannot be reduced to obtain the counter. Only re-normalization of values whose canonical provenance is already established may be removed.

## Ownership-correct correction

The permitted implementation is narrow:

1. Add a private “remember canonical” operation to the existing `ResolutionStringMemo`; do not add a field or a second map.
2. Make `normalize` register both spellings when they differ, and make `join`, `parent`, `probe_candidates`, and `package_path` register their canonical outputs.
3. Make frame-local joining use the existing component-normalization memo while retaining the exact `join_paths` collapse semantics.
4. Seed each lazy geometry immediately when its `OnceCell` is first built; do not depend on the earlier `seed_precomputed_geometry` call. Remove `build_project_geometry`'s post-join normalization.
5. Add private workspace transaction methods for replaying an already-canonical path/realpath/manifest/recovery-scope value. Route only `apply_attempt_output` through them. Keep the raw `WorkspaceRead` methods on the normalizing path.

The likely production files are limited to:

- `crates/verter_semantic/src/resolver_core/resolve_frame.rs`;
- `crates/verter_workspace/src/resolver.rs`;
- `crates/verter_workspace/src/resolution_currency.rs`.

Focused tests may be added beside the existing resolver/frame and resolution-currency tests. There is no authorization to alter `priority_frontier`, input loading, result formation, cache keys, cache admission, retention, or public DTOs.

## Why no existing cross-request cache is cited

No existing bounded cache has a ratified key/value contract for arbitrary canonical-string derivations. In particular, the workspace lazy resolution cache is keyed by the complete resolution query dimensions (importer, specifier, phase, kind, population) and retains complete resolution candidates under its existing cap; it is not a string-normalization interner. Reusing it across unrelated query keys would change its authority and key semantics.

The rejected `Arc<ResolutionSharedMemo>` had five unbounded maps in `verter_semantic` and remains forbidden. `U3.CACHE_FACT_MODEL` / `verter_session::bounded_query_retention` is the authority for a future reusable bounded cache, but no such cache is needed for this correction.

## Acceptance and mutation requirements

The implementation is not accepted by a local counter test alone. It requires:

1. RED/GREEN focused call-count tests for raw→canonical→consumer reuse, canonical component joins, parent reuse, probe candidates, all three lazy geometry families, manifest targets, and basis-change clearing.
2. A two-frame negative control proving that independent frames each perform first-use normalization.
3. A raw transaction-ingress control proving it still normalizes, plus a canonical replay control proving identical ordered facts/observations without a second normalization.
4. Revert controls for each optimization family; arbitrary planted calls are insufficient.
5. Exact preservation of result, ordered `LoadSet`s, `NeedInputs` waves, consumed-observation order, fact witness, and provider projection.
6. The existing 24 converted cases, seven production-driver cases, and the frame discrimination test.
7. The exact frozen A6 protocol on the corrected SHA: `normalize_canonical_id.calls <= 11,313`; semantic dispatch `4,216`; cold build `1,063`; cache admit `1,063`; component-meta digest `7161214711717846280`; and all locked wall/RSS/counter conjuncts.
8. A structural mutation check that no static/core/shared normalization map, cross-request memo, cache authority, retention policy, metric edit, load-set edit, witness edit, or output edit was introduced.

If all canonical-provenance duplicates above are removed and the exact A6 cell still exceeds 11,313, the implementation must stop. At that point the conflict would be between the locked Stage-2 `NeedInputs`/witness work and A6 `S2-R6`; only the maintainer/A6 lock authority could amend the A6 implementation lock (or the architecture authority could amend the load/witness contract) under the blind recalibration rules. Current evidence does not justify either change while concrete duplicate work remains.

## Compact receipt

RESULT FIX_PATH; CANDIDATE `6eb7b834ceea2e474b4de1ddc7c2eec945f9e914`; FILES/FUNCTIONS `crates/verter_semantic/src/resolver_core/resolve_frame.rs::{ResolutionStringMemo::{normalize,seed_normalized,join,parent,probe_candidates,package_path},ResolveFrame::{attempt,seed_precomputed_geometry,project_geometry,node_modules_directories,imports_directories},build_project_geometry}`, `crates/verter_workspace/src/resolver.rs::{apply_attempt_output,resolve_tracked,resolve_for_project_tracked,drive_attempt}`, `crates/verter_workspace/src/resolution_currency.rs::{TransactionReader::record_snapshot_*,ResolutionTransaction::{observe_path,observe_realpath,observe_manifest,observe_recovery_chain}}`; PERMITTED CHANGE BOUNDARY extend existing request-local `ResolveFrame` canonical-provenance registration and existing workspace transaction canonical replay only—no new cache/map/owner, cross-request retention, public API, semantics, output, load set, observation order, witness, or metric change; CHECKS required RED/GREEN plus revert controls for canonical reuse/lazy geometry/replay/basis/two-frame isolation, existing 24 conversion + 7 driver + frame discrimination, structural no-cache/no-contract mutation, and exact frozen A6 `<=11,313` with `4,216/1,063/1,063`, digest `7161214711717846280`, and every conjunctive wall/RSS/counter lock preserved.
