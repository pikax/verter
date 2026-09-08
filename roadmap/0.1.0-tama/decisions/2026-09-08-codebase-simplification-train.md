# Add a codebase simplification train

Date: 2026-09-08. Authority: the maintainer explicitly requested adding the audited simplification train to the TAMA DAG and updating pending dependents, while protecting seven named in-flight PRs. Implementation dispatch belongs to the TAMA controller.

## Source-grounded reason and bounded scope

The codebase assessment identified repeated provider source-index construction, duplicated IPC request and close bookkeeping, SSR property render/reparse passes, and surviving semantic dispatcher/memo responsibility debt. The test audit found 24,286 lines in session architecture_guards.rs and 17,270 in output_projector_residual_guards.rs, both mixed populations. Four whole scanner/governance retirement candidates total 2,646 physical lines. These are inspection sizes, not deletion quotas or measured CI/runtime gains.

Representative source findings:
- scheduler dag_arch_guards.rs counts only the three known enum variants while claiming to reject a fourth; it also pins deleted types and exact method text.
- component-meta operator-terminal-no-graphnode-leak.spec.ts misses the interpolated placeholder form described in its own regression history; actual native-eval output assertions remain useful.
- critical_rules_have_guards.rs is a 1,962-line documentation/test-name registry with its own discovery tests.
- playground wasmNoLiveTsgo.spec.ts scans itself using split-up banned tokens and walks again to assert a file-count floor.
- codec.rs::LineIndex::new scans/copies source, and tsgo endpoint conversion rebuilds it per position.
- background_drain.rs and workspace_scanner.rs contain equivalent stale-close operations with copied rationale.
- template/code_gen/ssr/mod.rs::merge_duplicate_event_handlers reparses rendered property strings.

The committed charters and contracts/codebase-simplification.md contain all instructions needed to implement the train; ignored audit notes are not an authority dependency. Source anchors are starting points to bind against the post-predecessor checkout.

## Added nodes

| Node | Independently acceptable outcome | Direct predecessors |
|---|---|---|
| SIMP1 | Retire scheduler source-shape guards | A6 |
| SIMP2 | Retire playground engine tombstones | A6 |
| SIMP3 | Remove recursive test governance | SIMP1, D3C, E1, SG0 |
| SIMP4 | Retire historical source/campaign guards | SIMP3, TCM1, CCA1O4D |
| SIMP5 | Retire closed semantic migration analyzers | SIMP4, E2 |
| SIMP6 | Replace provider shape scans with useful retained proof | SIMP3 |
| SIMP7 | Reuse exact-source coordinate conversion state | A6 |
| SIMP8 | Simplify pending-request transport bookkeeping | SIMP6, SIMP7 |
| SIMP9 | Unify stale-close lifecycle operation | SIMP6 |
| SIMP10 | Remove SSR property render/reparse work | TCM1, CCA1O4D |
| SIMP11 | Keep one integration-layout metadata check | SIMP3, TCM1 |
| SIMP12 | Simplify component-meta contract tests | SIMP3, E1 |
| SIMP13 | Simplify surviving memo coordination after cache convergence | SIMP5, G4 |
| SIMP14 | Narrow semantic dispatcher responsibilities | SIMP13 |
| SIMP15 | Verify combined code/test/comment outcomes | SIMP2, SIMP4, SIMP8, SIMP9, SIMP10, SIMP11, SIMP12, SIMP14 |

All rows start pending, dispatchable and non-optional in rev11.simplification, with the Rev11 foundation milestone. SIMP1/SIMP2/SIMP7 are initially READY; that means eligible, not started or reserved.

## Protected active work

| PR | Current local owner | Protection |
|---|---|---|
| #472 | D3R | Unchanged node/charter/ledger state; shared semantic/policy cleanup follows D3C. |
| #507 | D3P | Unchanged; transitive D3C boundary preserves the atomic stack. |
| #508 | D3C | Unchanged; no new prerequisites or acceptance burden. |
| #498 | E1 | Unchanged; source-policy/compat cleanup follows E1. |
| #501 | TCM1 | Unchanged; compiler/mapping/CI cleanup follows TCM1. |
| #510 | CCA1O4D | Unchanged; historical compiler-boundary and SSR cleanup follow it; native API files excluded. |
| #511 | No matching node in current 411-node authority | Its unplugin files and shared lockfile are excluded throughout; no fake DAG/implementation row is introduced. |

PR #511's branch-embedded identity and description refer to work that has no current TAMA node. Its actual six-file scope was inspected. A conventional PR title is not a valid basis for inventing local implementation identity.

The separate draft #98 is likewise not adopted by this amendment. Its Svelte runtime, guard/integration and dependency-closure surfaces remain outside this train.

## Pending dependent amendments

- G1 additionally follows SIMP5, so future query-contract work uses the useful semantic proof baseline.
- H2 additionally follows SIMP8/SIMP9, so ProviderHub migration consumes simplified existing transport/close owners.
- K3 additionally follows SIMP14, so host decomposition starts after dispatcher responsibilities are narrowed.
- CCA1T2V additionally follows SIMP10, so Vue compatibility deletion characterizes the equivalent simplified emitter.
- L1 additionally follows SIMP15, so final soak includes the simplification result.
- L4 additionally follows SIMP15 explicitly, keeping the required train visible in final closure.

The DAG and each pending charter header/body change together. Cross-train consumer contracts are declared in catalogs/contract-dependencies.toml. Existing acceptance remains intact. No implemented or in-flight charter is rewritten.

## Operation

The train uses ordinary independent-node candidates, risk-scaled fresh reviews and cumulative train checkpoints/final review. No new controller state, lease, receipt, source scanner, retirement registry or automatic dispatch is added. Size fields remain advisory. New node and parent issue prose is authored locally for normal controlled synchronization; this amendment does not refresh existing issue or PR prose.

Code/tests are changed only by later node implementations. Verification of this amendment covers strict DAG validation, frontier and packet behavior, unchanged existing implementation/issue mappings, declared conflict-domain coverage, human issue synchronization in check mode, the implementation-ledger tests and the docs build. Fresh dependency, architecture and testing reviews passed after correcting the affected TypeScript gate selections and explicitly binding retained type assertions.

The full conflict-ownership projection check is currently blocked by a pre-existing invalid `crates/verter_formatter/src/service/**` production-surface entry in `charters/expansion-formatter/FMT3C.md`. That unrelated charter is unchanged. The new train's concrete surfaces are checked independently against their declared domains; the projection check is not reported as passing.

## Retain DAG-removal evidence

The mapper closure instrument retains an explicit negative control over the canonical DAG validator. Its authority proof pins the validated node and edge summary so removal of a node or predecessor edge cannot silently shrink the program, while its mutation control proves that a broken predecessor edge is rejected together with the resulting charter mismatch.

The control, proof, mutation and their exclusive pins remain part of the mapper closure unless an equivalent node-and-edge removal mutation test replaces them. The canonical `validate-program-dag.mjs --strict` command remains in the Tama Roadmap CI job and continues checking graph validity, charter parity, catalogs and ledger membership. Valid roadmap growth must refresh the mapper closure transcript consistently. Remaining mapper contract evidence and in-flight implementation ownership are unchanged.
