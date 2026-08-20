---
ruling_id: "D1-SIX-FORKS"
type: "architecture-ruling"
date: "2026-08-20"
date_source: "file-mtime (no in-document date)"
binds: ["D1"]
source_file: "ARCH-RULING-D1-SIX-FORKS.md"
summary: "Codex xhigh consult rules on all six open forks in the D1 (private sole-flow-solver foundation) charter draft: capability-matrix ratification timing gates D2 not D1; C1/D1 file-relocation boundary (Fork 2, see supersedes below); D1's obligation ledger must extend the existing ObligationRuntime, never a peer ledger; A6 effective-flow row enumerability; binding-slot identity mechanism; AMD-001's four artifacts verified delivered, discharging the PRIVATE_CHECKPOINT prerequisite. No fork requires a program/ADR/DAG amendment (Fork 4 calls for a capability-matrix revision, explicitly 'program AMD: no')."
supersedes: []
superseded_by:
  - ruling: "C1-D1-FLOW-FILE-RECONCILIATION"
    claim: "Fork 2's disposition 'C1 moves flow_return.rs, flow_return_callee.rs, and dispatch_txn.rs; NOT flow_slice_content.rs' — the 'NOT flow_slice_content.rs' claim is superseded (flow_slice_content.rs MOVES whole to verter_semantic), and the whole-file MOVE claims for flow_return.rs and dispatch_txn.rs are superseded (both instead SPLIT). Fork 2's whole-file MOVE for flow_return_callee.rs is RETAINED, as is Fork 3's 'extend the same ObligationRuntime; no peer ledger' ruling."
contradicts: []
notes: "This document's own text frames Fork 2 purely as a correction of the unratified D1-CHARTER-DRAFT.md, and never itself asserts disagreement with C1.md or ARCH-RULING-C1-FOUR-FORKS.md — the conflict with C1's actual convergence map was caught externally, by a human, and is what ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md cites as the C1/D1 contradiction benchmark case. 12,360-line raw transcript; closing verdict block duplicated verbatim (~12299-12328 and ~12331-12360). Notes a sandbox limitation: a local test invocation was blocked by mkdtemp denial, so Fork 6 relies on landed source/commit evidence rather than a fresh green run."
---

Reading prompt from stdin...
OpenAI Codex v0.148.0
--------
[1mworkdir:[0m <MACHINE_ROOT>/verter
[1mmodel:[0m gpt-5.6-sol
[1mprovider:[0m openai
[1mapproval:[0m never
[1msandbox:[0m read-only
[1mreasoning effort:[0m xhigh
[1mreasoning summaries:[0m none
[1msession id:[0m 01a02001-2214-7d70-be4d-2b9bb5dd96d4
--------
[36muser[0m
Architecture challenge, read-only, on <MACHINE_ROOT>/verter
(branch program/architecture-lock, tip 6ab1000bd).

Block D1 ("Private sole-solver foundation", program.md:225-233) has a drafted charter at
<SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md whose author honestly flagged SIX open design forks rather than guessing.
Your job is to RULE on each — decisively, from the code and the ratified documents, not from preference.
Read the charter first, then answer.

Context that binds you: D1 builds a private, hermetic, non-production flow solver; it may receive
checkpoint review approval but MUST NOT merge or release independently from D2 (program.md:231). Scope is
~14,500 lines of evaluator to functionally match plus ~20,000 lines of pinned characterization tests.

## The six forks

1. **Capability-matrix ratification timing (A6 lock record U-1).** Is the matrix ratified enough to bind
   D1's exit criteria today, or does D1 start against an unratified target?
2. **Does C1's relocation of `project_semantic_dispatch` into `verter_semantic` include
   `flow_return.rs` / `dispatch_txn.rs`?** C1's ruling excluded FLOW semantics from its own scope, so
   the boundary between what C1 moves and what D1 owns must be exact. Cite the files.
3. **Is D1's obligation ledger the SAME mechanism as `dispatch_txn.rs`'s existing `ObligationRuntime`,
   or a second one?** A second obligation authority is the "two engines diverge" failure this codebase
   forbids. Read both and rule.
4. **Which A6 capability-matrix rows count as "effective-flow capability rows"?** program.md:231 makes
   satisfying every Supported/Stable effective-flow row a precondition for D2. If nobody can enumerate
   the rows, the precondition is unenforceable. Enumerate them or say why it cannot be done.
5. **Is "stable binding slots" (program.md:229) the existing `SkeletonBindingId` mechanism or something
   new?** Reusing an existing identity vs minting a parallel one is a real fork.
6. **Are AMD-001's artifacts a hard gate on D1 STARTING, or only on D2?** NOTE: the stack-window validator
   and composite cross-validation that AMD-001 required have now been DELIVERED and landed (commit
   `f46de1b6a`, `scripts/validate-stack-window.mjs` + `scripts/lib/stack-window-lib.mjs` + the
   discriminating D1/D2 transition test). Verify that yourself and rule on whether the prerequisite is
   discharged.

## Rules for your answer

- The program forbids name-keyed source-tree scanners as LANDED enforcement — structural confinement only.
  Do not propose one.
- Breadth, breakage, migration size and effort are NEVER reasons to weaken a design. If the correct answer
  is a large breaking change, say so.
- Where the charter draft is factually WRONG about the code, say so plainly with the file:line that
  disproves it.
- For EACH fork state explicitly whether your ruling changes an accepted ADR, a DAG edge, or a program
  outcome — that determines whether a formal amendment is required, and the maintainer needs it stated.
- Concise. file:line throughout. End with six verdict lines, one per fork.

[35m[3mcodex[0m[0m
I’ll treat this as an architecture ruling, not an implementation review. I’m starting with the charter and the canonical architecture/program references, then I’ll trace each fork into the exact code and landed validator artifacts.
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "sed -n '1,260p' <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 15778ms:[0m
# D1 — Private sole-flow-solver foundation

**Status:** DRAFT pending maintainer ratification. **Class:** Foundational
(`governance.md` §2.3 names "flow/effective typing" explicitly as a Foundational
example area; `program-dag.toml` / `program.md:225-233`). **Predecessors:** `A3`, `B1`,
`B2` — all three `ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml:157-176,245-264,465-484`)
— and `C1`, currently `status = "READY"` but its own charter is **DRAFT, unratified**
(`docs/arch/architecture-lock/ledger/program-state.toml:619-638`; `charters/C1.md:3`).
D1 cannot be `BLOCK_READY` while a predecessor's charter is unratified; see Sequencing.

## Context — why this is far bigger than program.md's nine lines

`program.md:225-233` gives D1 one paragraph: build canonical graph adapters, derived
projections, a domain registry, a demand plan, binding slots, relation-authority
integration, an obligation ledger, a private finalizer, parse reacquisition, and typed
gaps — all behind a hermetic non-production boundary — so that it "preserves every
ratified supported capability" before `D2`'s cutover. Read literally that sounds like a
scaffolding exercise. **It is not**, for the same reason this program has repeatedly
found blocks larger than their summary: the paragraph describes architecture.md §9's
full target shape (§9.1-9.9), and the code it must functionally match today is real,
large, and already characterized by ~20,000 lines of pinned regression tests.

**The thing being replaced is not small.** The current effective-flow ("`FlowReturn`")
answer is produced by a private, syntax-shaped, name-keyed evaluator:

- `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` — 8,416 lines
- `crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs` — 733 lines
- `crates/verter_session/src/flow_slice_content.rs` — 5,407 lines

— roughly 14,500 production lines — pinned by roughly 20,000 lines of characterization
tests (`flow_return_coverage_tests.rs` 3,154; `flow_return_lexical_tests.rs` 6,683;
`flow_return_tests.rs` 4,531; `flow_return_positional_tests.rs` 463;
`flow_return_root_gate_tests.rs` 680; `flow_return_frame_seal_tests.rs` 409;
`flow_gap_retraction_tests.rs` 672; `u6_flow_shape_corpus_tests.rs` 4,084). D1 must
reproduce, through a wholly different graph-native mechanism, whatever slice of that
14,500-line surface the (currently unratified — see Open Questions) A6 capability
matrix calls Supported/Stable, and must answer everything else with a typed gap at
least as precise as the eleven (`G1`-`G11`) gap classes `A3` already shipped
(`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md` §5). That is not "scaffolding
behind a flag"; it is standing up a second, competing, complete-enough analysis engine
under the same hermetic roof.

**The target is already normatively specified, in more detail than program.md itself.**
Four documents bind D1 more precisely than the block's own paragraph:
[`contracts/flow-completeness.md`](../contracts/flow-completeness.md) (the "Flow Domain
Closure, Obligation Ledger, and Completeness Contract" — normative), `architecture.md`
§9 ("Effective typing and flow", §9.1-9.9), `decisions/ADR-001-semantic-authority-and-derived-projections.md`,
and `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md`. This charter treats
those four as the binding shape and program.md's paragraph as their one-line summary,
consistent with how `C1.md`'s Context section treated `program.md:191-197` — effort and
breadth are not reasons to shrink a charter (`CLAUDE.md` → Planning).

**What D1 does NOT do**, stated up front because it is easy to over-read the paragraph:
it does not delete `flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` (that
is `D2`'s one atomic cutover, per `ADR-014` decision 2); it does not touch any product
entry point; it does not implement nominal-identity extension (`D3`), the full
narrowing/structural-return algebra (`D4`), closure/capture join semantics (`D5`), the
completion graph that closes debt `FR-D8`/`G10` (`D6` — still explicitly open per ruling
R-11), `this`/sequence/call-context surfaces (`D7`), or the final U6 admission proof
(`D8`). D1 builds the foundation those six blocks extend.

## Sequencing

Per `docs/arch/architecture-lock/ledger/program-state.toml`: `A3`, `B1`, `B2` are
`ACCEPTED`; `C1` is `status = "READY"` (dependency-eligible) but has no accepted
candidate, and its own charter (`charters/C1.md:3`) is explicitly "DRAFT — authored for
maintainer review; no AMD ratifies it yet." The ledger's `current_block` at this
charter's authoring time is `BS1` (`program-state.toml:22`), unrelated to Track D. **D1
dispatch follows both whatever is currently in flight and C1's acceptance** — the same
sequencing discipline `C1.md` itself applied to `BV1` ("C1 dispatch follows BV1's
landing... This charter does not propose running C1 in parallel").

Two further preconditions are recorded in already-accepted evidence and are **not**
discharged by this charter, only named so they are not silently skipped:

1. **AMD-001 / ruling R-12.** The Node stack-window validator, composite
   program-state cross-validation, its CI wiring, and a discriminating `D1`/`D2`
   transition test are due "unconditionally before `D1` enters `PRIVATE_CHECKPOINT`"
   (`evidence/maintainer-rulings.md` R-12, `AMD-001-stack-window-validator-prerequisite.md`).
   `A6`'s own lock record records these as **not yet delivered**
   (`evidence/A6/implementation-lock-record.md` §11 row U-9). The composite validator
   must accept `D2` "only when `D1` is the declared private checkpoint in the same
   validated `ATOMIC_REVIEW` snapshot with `D2` as its acceptance block" — that
   machinery does not exist in the ledger tooling today.
2. **A6 lock record U-1 — capability matrix ratification.** `contracts/capability-matrix.md`'s
   eight seed rows are **all `Status = VERIFY`**
   (`evidence/A6/implementation-lock-record.md` §3, §11 row U-1). D1's own governing
   text ("it covers every effective-flow capability row declared Supported/Stable by
   the `A6` matrix" — `contracts/flow-completeness.md` §6; `program.md:231`) is,
   by the lock record's own admission, **currently vacuous**: "with none declared, the
   obligation is vacuous until the matrix is ratified... the matrix must be ratified
   before that block begins." See Open Questions §1.

## Intent Contract

**Actor / problem.** Effective-flow answers (function return-type inference feeding
`defineProps`/`defineEmits`/component-meta and any other consumer of
`SemanticQueryKey::FlowReturn`) are produced today by a private evaluator that resolves
local values through its own `read_local(&mut self, name: &str)` map
(`flow_return.rs:4353`) and its own ad hoc reaching-definition bookkeeping — not by
walking the typed-edge `FunctionFlowGraph` dependence structure
(`crates/verter_semantic/src/analysis/flow/flow_graph.rs:155`,
`build_function_flow_graph` at `:243`) that already exists beside it, built for demand
SLICING only (`peeker.rs`, `hashing.rs`, `lower.rs`). `A3` (accepted) proved specific
rows of this evaluator wrong-and-warm and retracted them behind typed `FlowGap` markers,
but was explicitly forbidden to replace the evaluator: "adding `FlowSolveOutcome`, proof
tokens, or an obligation ledger in A3 is prohibited scope expansion... belongs to
D1/D2's final typestate cutover" (`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md`
§12.1, and its Verdict paragraph). `D3`-`D8` each need a graph-native binding/relation
authority to extend — nominal identity, narrowing, closure, loops, completion — without
re-deriving name-keyed state per block. That authority does not exist. D1 is the block
that builds it, privately, so `D2` can perform ADR-014's one atomic cutover without ever
exposing two selectable production semantics engines at once.

**Required observable outcomes** (all inside the private hermetic boundary; none are
reachable in a release build):

- Every value/effect fact the new solver produces is derived by walking
  `FunctionFlowGraph` edges (`FlowEdgeKind::ValueDef` / `PathWrite` / `EvalEffect` /
  `ControlRegion`) keyed by stable `SkeletonBindingId`/`FlowNodeId` slots — never a
  `&str`-keyed lookup into a locally built map, and never a re-walk of the OXC AST for a
  fact the graph already encodes.
- A `FlowDemandPlan` (`architecture.md` §9.3) is computed once per cold logical demand
  by taking the transitive closure of named obligations through one closed static domain
  registry; naming an unregistered domain produces a typed unsupported obligation at
  plan time, never a silent skip and never a partial plan the solver evaluates anyway.
- A private obligation ledger (`contracts/flow-completeness.md` §4 — modelled, not
  necessarily named, on `ObligationState { Pending, Running, Discharged(evidence),
  Gap(FlowGap), Failed(reason) }`) discharges every required/expanded obligation before
  a private finalizer can construct a `CompleteFlowResult`-shaped value; that
  constructor is unreachable from outside its owning module (no public `new`, no
  `Default` that yields a "complete" discriminant, no test-only escape left reachable
  outside `#[cfg(test)]`).
- Every `G1`-`G11` case `A3` already typed as a gap
  (`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md` §5's per-gap table) is answered by
  the new solver with an equal-or-more-precise typed gap — never silently "solved" where
  production still honestly refuses.
- Authored/legitimately-inferred semantic `any` stays a distinct value from "Verter has
  no model" (`contracts/flow-completeness.md` §5: "An authored `any` is a semantic
  value. 'Verter has no model' is a gap and cannot become `any`"), mirroring the closed
  split `A3` already installed (`SliceExpr::SemanticAny` vs. `SliceExpr::Gap(FlowGap)`,
  `flow_slice_content.rs`) or D1's own graph-native equivalent of it.
- Relation/inference decisions — nominal comparability, union/intersection membership,
  coinductive SCC discharge — route exclusively through the existing shared authority:
  `ProjectSemanticDispatch::execute`/`execute_relate` plus the `ObligationRuntime`/
  `ObligationReentryStack` tagged-identity space
  (`crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:879,396`,
  physically relocating into `verter_semantic` under `C1` per its convergence map) —
  never a second copy of that machinery.
- The entire private tree is unreachable from any product entry point (LSP, NAPI, WASM,
  MCP, unplugin) **in a release build** — absent from the compiled artifact, not merely
  undocumented or unlinked-by-convention.

**Forbidden observable outcomes:**

- A second name-keyed value-resolution map inside the new solver (any
  `FxHashMap<&str, _>`- or `read_local`-shaped local-binding lookup that competes with
  graph-edge traversal).
- A second relation classifier: a new `RelationDomainRuntime`/`InferenceSession`/
  `SessionAdmissionLedger`-shaped type, or any `SemanticNodeId` comparison that does not
  route through `execute_relate`.
- A second `FunctionFlowGraph`-shaped structure, a second CFG, or a derived projection
  (dominators / loop forest / def-use overlay) that stores a semantic fact instead of
  being deterministically reconstructible from `(graph identity, profile)` alone — the
  exact prohibition `ADR-001` states.
- Any product/runtime-selectable flag, config value, or code path — in any crate — that
  can reach the new solver before `D2`'s cutover lands.
- A `CompleteFlowResult`-shaped value constructed anywhere outside its private
  finalizer.
- Porting `D3`-`D6`'s future algorithms (full nominal-identity comparability, full
  narrowing/closure/loop/completion semantics) into the private tree early merely to
  make a parity test pass. D1 owns re-deriving, graph-natively, whatever slice of
  today's behavior is ratified Supported/Stable (or already a typed `A3` gap); it does
  not pre-implement work `program.md` assigns to later blocks by name.

**Authority/fallback order.** New-solver value/effect resolution → `FunctionFlowGraph`
edges (sole structural authority per `ADR-001`, `architecture.md` §9.1) → stable binding
slots. Relation/inference decisions → `ProjectSemanticDispatch::execute`/`execute_relate`
(unchanged; `C1`-preserved, not `C1`-converged in the sense of new semantics). Anything
the closed domain registry does not recognize → typed unsupported obligation, never
silent success. There is **no** fallback from the new private solver to the old
production evaluator or vice versa inside D1's scope — they are two independent
implementations that happen to coexist pre-`D2`, never composed.

**Acceptance IDs.**

| ID | Requirement | Planned test/gate |
|---|---|---|
| D1-AC-1 | Value/effect resolution is graph-edge-native, never name-keyed | Type-level: the new solver's hot evaluation path takes no `&str`-keyed lookup type in its signature (reviewed at the module boundary, not grep-detected); behavioral: a private-boundary corpus reusing `A3`'s pinned `FIXED_CELLS`/G1-G11/154-row fixtures asserts the same observable result class (`Complete` / `Partial+FlowGap` / `NoValue`) as production for every fixture the new solver claims to cover |
| D1-AC-2 | Private hermetic boundary — unreachable from any release build | `#[cfg(any(test, feature = "test-support"))]`-gated module reusing the crate's EXISTING `test-support` feature (`crates/verter_semantic/Cargo.toml:22`, already documented as "never on the default/production closure"); a Cargo-metadata closure test proves no production `[dependencies]` edge (LSP/NAPI/WASM/MCP/unplugin binaries and their production dependency graphs) enables `test-support`, mirroring the existing dependency-firewall pattern `C1`/`B1` already use |
| D1-AC-3 | Obligation ledger + private finalizer | Compile-fail (`trybuild`) proof that the `CompleteFlowResult`-shaped type has no publicly reachable constructor; a mutation test that removes one discharge call makes the finalizer return the `ReturnOnly`/gap shape instead of `Complete` (`contracts/flow-completeness.md` §7: "mutation test dropping one obligation cannot yield `Complete`") |
| D1-AC-4 | Closed static domain registry; unregistered domain fails typed, not silently | Every dispatch site over the domain enum is an exhaustive `match` with no `_` wildcard (compile-time proof: adding a domain variant without handling it is `E0004`); a fixture that requests an intentionally-unregistered domain asserts a typed unsupported-obligation result, never a partial evaluation |
| D1-AC-5 | No second relation/inference authority | The new solver module holds no `pub(crate)`-or-broader path to construct `RelationDomainRuntime`/`InferenceSession`/`SessionAdmissionLedger` directly (those stay `pub(crate)` to their defining module; the new solver only calls the public dispatch entry) — a `trybuild` compile-fail fixture proves a type outside that defining module cannot construct them, mirroring `C1-AC-7`'s method |
| D1-AC-6 | Deterministic derived projections carry no semantic fact | Any dominator/loop-forest/def-use overlay the new solver builds is rebuilt twice from the same `(graph identity, profile)` and asserted byte-identical; a fixture proves it is never given its own persisted cache identity or treated as a second graph |
| D1-AC-7 | Typed-gap parity with `A3`'s already-shipped G1-G11 taxonomy | For every G1-G11 fixture in `u6_flow_a3_retraction_tests.rs`/successor, the private solver's result class matches production's (both `Partial+FlowGap` of at least as precise a reason, or both `NoValue`) — never a silent "solved" answer where production still typed-refuses |
| D1-AC-8 | Ratified capability-row coverage | For whatever row set the maintainer ratifies Supported/Stable before D1's candidate is dispatched (see Open Questions §1), every corpus fixture tagged to that row returns the same observable result class production gives today, via the new mechanism; every other row stays a typed gap. **Until ratification, this acceptance ID has an empty covered set and D1-AC-7 is the operative coverage floor** |
| D1-AC-9 | Exact parse reacquisition — at most one reborrow per cold demand | The new solver reborrows the retained parse snapshot through the same lease-only pattern the existing flow substrate already documents (`crates/verter_semantic/src/analysis/flow/mod.rs:1-18`: "borrows no OXC node... performs NO type lowering"; `decl_body_memo.rs`'s scheduler-retained-snapshot pattern), never a second ad hoc re-parse; a counter-instrumented fixture asserts exactly one reborrow per cold function-body demand regardless of how many domains that demand touches |
| D1-AC-10 | No product entry point reaches the private tree | A source/dependency-graph test (the kind `contracts/flow-completeness.md` §7 requires of `D2`, run here as a D1-scoped negative control) proves zero non-test, non-`test-support` call graph edge from `verter_lsp`/`verter_napi`/`verter_wasm`/`verter_mcp`/`unplugin` reaches the new module |

**Cold/warm/allocation/fan-out/latency bounds.** D1's private tree is, by its own
required outcome, unreachable from any product entry point — it therefore touches **no
existing locked performance-gate cell**, and this charter does not add one.
`A6_META_COMPILE_40_COLD_RUST` (the sole currently-locked cell) must show **zero**
measured delta from a D1 candidate, because nothing it exercises can compile into a
release artifact's hot path; that null result is itself part of D1-AC-2's proof, not a
separate performance obligation. D1 may **not** request a new locked cell for
production-facing flow performance — that is `D2`'s (and later `L2`'s) obligation, once
the solver is actually reachable. The one in-charter resource requirement is
`contracts/flow-completeness.md`'s own "no-flow path allocates no graph/plan" and
"exact parse reacquisition" properties (D1-AC-9 above), proven by the private-boundary
test suite itself, not by a gated SLO cell.

## Current-tree convergence map

| current authority | source | disposition under D1 | disposition under D2 |
|---|---|---|---|
| Demand-slicing graph substrate (already graph-shaped) | `crates/verter_semantic/src/analysis/flow/{mod.rs,flow_graph.rs,peeker.rs,hashing.rs,lower.rs,flow_ir.rs,frame_span.rs,value_descent.rs}` | Preserve + extend. D1 builds its VALUE/effect resolution on top of `FunctionFlowGraph`'s existing edge kinds; adds no second graph (`ADR-001`) | Becomes the sole structural authority for production, unchanged in kind |
| Syntax-shaped VALUE evaluator | `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` (8,416 lines), `flow_return_callee.rs` (733 lines) | Untouched in production. D1 builds a graph-native replacement privately, alongside it, answering the same demand shape | Deleted wholesale, in the same accepted candidate that routes public operations to the new solver |
| Content lowering (owned control-flow tree over the demanded slice) | `crates/verter_session/src/flow_slice_content.rs` (5,407 lines) | Untouched in production; D1's private tree builds its own graph-native content-lowering equivalent | Deleted, same D2 candidate |
| Obligation/coinductive-SCC substrate | `crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs` (`ObligationRuntime` `:879`, `ObligationReentryStack` `:396`, `ObligationFrameDomain::FlowReturn` `:318-325`, `FlowReturnFrameState` `:298`, `FlowReturnDomainRuntime` `:1868`) | Preserve. D1 integrates against this (see Open Questions §3 on how); does not duplicate the reentry/SCC mechanism | The `FlowReturn`-domain-specific arms become dead code once the new solver's obligation identity supersedes them; `D2` deletes them as part of "duplicate control/completion/relation/effect paths" per `program.md:243` |
| Typed gap taxonomy | `crates/verter_session/src/semantic_query.rs` (`FlowGap` enum, ~`:1547`) | Preserve/extend as the coverage floor (D1-AC-7); D1 may need additional domain-specific gap variants (e.g. covering `G10`/`AbruptCompletion` once `D6` lands) but must not weaken any existing variant's meaning | Final home decided at `D2`'s cutover — may fold into the new solver's own typed-gap vocabulary or stay as-is; not a D1 decision |
| Characterization corpus | `u6_flow_shape_corpus_tests.rs`, `u6_flow_expect_tests.rs`, `flow_return_*_tests.rs`, `flow_gap_retraction_tests.rs` (~20,000 lines total) | D1 REUSES these fixtures as its own private-boundary oracle (same expected outcomes, new implementation under test) — it does not re-author a parallel fixture set | `D2` owns final consolidation: fixtures that only exercised the deleted evaluator's internals are retired; fixtures that assert observable behavior are re-targeted at the sole solver |

## Legacy deletions

**D1 deletes nothing in production.** This is a deliberate, explicit statement, not an
omission: `ADR-014` decision 2 and `contracts/flow-completeness.md` §6 both make the old
evaluator's deletion `D2`'s ONE atomic act ("In one public cutover... delete the old
syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and
migration guards"). A D1 candidate that deletes or even behaviorally alters
`flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` — or any of their
production call sites — is out of scope and should be treated as a design violation, not
merely an aggressive interpretation. A reviewer should expect **zero diff** in those
three files (and in `dispatch_txn.rs`'s existing `FlowReturn`-domain arms) from a
conforming D1 candidate.

What D1 must still not leave behind, inside its own scope:

- Any private-boundary scaffolding that is not part of the final D1 exit surface
  (ordinary hygiene — draft types, abandoned domain stubs).
- Any temporary shim that duplicates existing obligation/relation machinery instead of
  calling it (the forbidden-outcomes list above), even if scoped
  `#[cfg(any(test, feature = "test-support"))]` — the private boundary makes it
  unreachable in production, but a duplicate authority inside the private tree still
  violates `program.md:229`'s "contains no name-keyed or flow-private relation
  authority."
- Block-identifier-named evidence scaffolding, per this program's own hygiene rule
  (`charters/A3.md` "In scope": "deletion or renaming of block-named evidence
  scaffolding before landing").

**Named for continuity — `D2`'s eventual deletion list, sourced from this research, not
executed here:** `flow_return.rs`, `flow_return_callee.rs`, `flow_slice_content.rs` in
full; `dispatch_txn.rs`'s `ObligationFrameDomain::FlowReturn`, `FlowReturnFrameState`,
`FlowReturnPendingState`/`FlowReturnPendingOutcome`, `CompletedFlowReturnMember`,
`FlowReturnDomainRuntime`; and, per `A3-SPEC.md`'s own forward note, `semantic_query.rs`'s
`FlowReturnStep`/`FlowReturnFailure`/`FlowReturnResult` shapes "to the extent D1's final
typestate cutover supersedes them" ("The legacy name `FlowReturnStep::Complete` is
misleading for degraded payloads, but renaming or replacing that outcome type belongs to
D1/D2's final typestate cutover" — `A3-SPEC.md` §1.4). D1's own charter binds none of
this list as its exit obligation; it is recorded here so `D2`'s charter does not have to

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'ls -l <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md && wc -l <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md && /bin/cat <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md'[0m in <MACHINE_ROOT>/verter
[32m succeeded in 19364ms:[0m
-rw-r--r--@ 1 carlosrodrigues  wheel  39881 Aug 20 11:53 <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md
     478 <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md
# D1 — Private sole-flow-solver foundation

**Status:** DRAFT pending maintainer ratification. **Class:** Foundational
(`governance.md` §2.3 names "flow/effective typing" explicitly as a Foundational
example area; `program-dag.toml` / `program.md:225-233`). **Predecessors:** `A3`, `B1`,
`B2` — all three `ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml:157-176,245-264,465-484`)
— and `C1`, currently `status = "READY"` but its own charter is **DRAFT, unratified**
(`docs/arch/architecture-lock/ledger/program-state.toml:619-638`; `charters/C1.md:3`).
D1 cannot be `BLOCK_READY` while a predecessor's charter is unratified; see Sequencing.

## Context — why this is far bigger than program.md's nine lines

`program.md:225-233` gives D1 one paragraph: build canonical graph adapters, derived
projections, a domain registry, a demand plan, binding slots, relation-authority
integration, an obligation ledger, a private finalizer, parse reacquisition, and typed
gaps — all behind a hermetic non-production boundary — so that it "preserves every
ratified supported capability" before `D2`'s cutover. Read literally that sounds like a
scaffolding exercise. **It is not**, for the same reason this program has repeatedly
found blocks larger than their summary: the paragraph describes architecture.md §9's
full target shape (§9.1-9.9), and the code it must functionally match today is real,
large, and already characterized by ~20,000 lines of pinned regression tests.

**The thing being replaced is not small.** The current effective-flow ("`FlowReturn`")
answer is produced by a private, syntax-shaped, name-keyed evaluator:

- `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` — 8,416 lines
- `crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs` — 733 lines
- `crates/verter_session/src/flow_slice_content.rs` — 5,407 lines

— roughly 14,500 production lines — pinned by roughly 20,000 lines of characterization
tests (`flow_return_coverage_tests.rs` 3,154; `flow_return_lexical_tests.rs` 6,683;
`flow_return_tests.rs` 4,531; `flow_return_positional_tests.rs` 463;
`flow_return_root_gate_tests.rs` 680; `flow_return_frame_seal_tests.rs` 409;
`flow_gap_retraction_tests.rs` 672; `u6_flow_shape_corpus_tests.rs` 4,084). D1 must
reproduce, through a wholly different graph-native mechanism, whatever slice of that
14,500-line surface the (currently unratified — see Open Questions) A6 capability
matrix calls Supported/Stable, and must answer everything else with a typed gap at
least as precise as the eleven (`G1`-`G11`) gap classes `A3` already shipped
(`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md` §5). That is not "scaffolding
behind a flag"; it is standing up a second, competing, complete-enough analysis engine
under the same hermetic roof.

**The target is already normatively specified, in more detail than program.md itself.**
Four documents bind D1 more precisely than the block's own paragraph:
[`contracts/flow-completeness.md`](../contracts/flow-completeness.md) (the "Flow Domain
Closure, Obligation Ledger, and Completeness Contract" — normative), `architecture.md`
§9 ("Effective typing and flow", §9.1-9.9), `decisions/ADR-001-semantic-authority-and-derived-projections.md`,
and `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md`. This charter treats
those four as the binding shape and program.md's paragraph as their one-line summary,
consistent with how `C1.md`'s Context section treated `program.md:191-197` — effort and
breadth are not reasons to shrink a charter (`CLAUDE.md` → Planning).

**What D1 does NOT do**, stated up front because it is easy to over-read the paragraph:
it does not delete `flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` (that
is `D2`'s one atomic cutover, per `ADR-014` decision 2); it does not touch any product
entry point; it does not implement nominal-identity extension (`D3`), the full
narrowing/structural-return algebra (`D4`), closure/capture join semantics (`D5`), the
completion graph that closes debt `FR-D8`/`G10` (`D6` — still explicitly open per ruling
R-11), `this`/sequence/call-context surfaces (`D7`), or the final U6 admission proof
(`D8`). D1 builds the foundation those six blocks extend.

## Sequencing

Per `docs/arch/architecture-lock/ledger/program-state.toml`: `A3`, `B1`, `B2` are
`ACCEPTED`; `C1` is `status = "READY"` (dependency-eligible) but has no accepted
candidate, and its own charter (`charters/C1.md:3`) is explicitly "DRAFT — authored for
maintainer review; no AMD ratifies it yet." The ledger's `current_block` at this
charter's authoring time is `BS1` (`program-state.toml:22`), unrelated to Track D. **D1
dispatch follows both whatever is currently in flight and C1's acceptance** — the same
sequencing discipline `C1.md` itself applied to `BV1` ("C1 dispatch follows BV1's
landing... This charter does not propose running C1 in parallel").

Two further preconditions are recorded in already-accepted evidence and are **not**
discharged by this charter, only named so they are not silently skipped:

1. **AMD-001 / ruling R-12.** The Node stack-window validator, composite
   program-state cross-validation, its CI wiring, and a discriminating `D1`/`D2`
   transition test are due "unconditionally before `D1` enters `PRIVATE_CHECKPOINT`"
   (`evidence/maintainer-rulings.md` R-12, `AMD-001-stack-window-validator-prerequisite.md`).
   `A6`'s own lock record records these as **not yet delivered**
   (`evidence/A6/implementation-lock-record.md` §11 row U-9). The composite validator
   must accept `D2` "only when `D1` is the declared private checkpoint in the same
   validated `ATOMIC_REVIEW` snapshot with `D2` as its acceptance block" — that
   machinery does not exist in the ledger tooling today.
2. **A6 lock record U-1 — capability matrix ratification.** `contracts/capability-matrix.md`'s
   eight seed rows are **all `Status = VERIFY`**
   (`evidence/A6/implementation-lock-record.md` §3, §11 row U-1). D1's own governing
   text ("it covers every effective-flow capability row declared Supported/Stable by
   the `A6` matrix" — `contracts/flow-completeness.md` §6; `program.md:231`) is,
   by the lock record's own admission, **currently vacuous**: "with none declared, the
   obligation is vacuous until the matrix is ratified... the matrix must be ratified
   before that block begins." See Open Questions §1.

## Intent Contract

**Actor / problem.** Effective-flow answers (function return-type inference feeding
`defineProps`/`defineEmits`/component-meta and any other consumer of
`SemanticQueryKey::FlowReturn`) are produced today by a private evaluator that resolves
local values through its own `read_local(&mut self, name: &str)` map
(`flow_return.rs:4353`) and its own ad hoc reaching-definition bookkeeping — not by
walking the typed-edge `FunctionFlowGraph` dependence structure
(`crates/verter_semantic/src/analysis/flow/flow_graph.rs:155`,
`build_function_flow_graph` at `:243`) that already exists beside it, built for demand
SLICING only (`peeker.rs`, `hashing.rs`, `lower.rs`). `A3` (accepted) proved specific
rows of this evaluator wrong-and-warm and retracted them behind typed `FlowGap` markers,
but was explicitly forbidden to replace the evaluator: "adding `FlowSolveOutcome`, proof
tokens, or an obligation ledger in A3 is prohibited scope expansion... belongs to
D1/D2's final typestate cutover" (`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md`
§12.1, and its Verdict paragraph). `D3`-`D8` each need a graph-native binding/relation
authority to extend — nominal identity, narrowing, closure, loops, completion — without
re-deriving name-keyed state per block. That authority does not exist. D1 is the block
that builds it, privately, so `D2` can perform ADR-014's one atomic cutover without ever
exposing two selectable production semantics engines at once.

**Required observable outcomes** (all inside the private hermetic boundary; none are
reachable in a release build):

- Every value/effect fact the new solver produces is derived by walking
  `FunctionFlowGraph` edges (`FlowEdgeKind::ValueDef` / `PathWrite` / `EvalEffect` /
  `ControlRegion`) keyed by stable `SkeletonBindingId`/`FlowNodeId` slots — never a
  `&str`-keyed lookup into a locally built map, and never a re-walk of the OXC AST for a
  fact the graph already encodes.
- A `FlowDemandPlan` (`architecture.md` §9.3) is computed once per cold logical demand
  by taking the transitive closure of named obligations through one closed static domain
  registry; naming an unregistered domain produces a typed unsupported obligation at
  plan time, never a silent skip and never a partial plan the solver evaluates anyway.
- A private obligation ledger (`contracts/flow-completeness.md` §4 — modelled, not
  necessarily named, on `ObligationState { Pending, Running, Discharged(evidence),
  Gap(FlowGap), Failed(reason) }`) discharges every required/expanded obligation before
  a private finalizer can construct a `CompleteFlowResult`-shaped value; that
  constructor is unreachable from outside its owning module (no public `new`, no
  `Default` that yields a "complete" discriminant, no test-only escape left reachable
  outside `#[cfg(test)]`).
- Every `G1`-`G11` case `A3` already typed as a gap
  (`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md` §5's per-gap table) is answered by
  the new solver with an equal-or-more-precise typed gap — never silently "solved" where
  production still honestly refuses.
- Authored/legitimately-inferred semantic `any` stays a distinct value from "Verter has
  no model" (`contracts/flow-completeness.md` §5: "An authored `any` is a semantic
  value. 'Verter has no model' is a gap and cannot become `any`"), mirroring the closed
  split `A3` already installed (`SliceExpr::SemanticAny` vs. `SliceExpr::Gap(FlowGap)`,
  `flow_slice_content.rs`) or D1's own graph-native equivalent of it.
- Relation/inference decisions — nominal comparability, union/intersection membership,
  coinductive SCC discharge — route exclusively through the existing shared authority:
  `ProjectSemanticDispatch::execute`/`execute_relate` plus the `ObligationRuntime`/
  `ObligationReentryStack` tagged-identity space
  (`crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:879,396`,
  physically relocating into `verter_semantic` under `C1` per its convergence map) —
  never a second copy of that machinery.
- The entire private tree is unreachable from any product entry point (LSP, NAPI, WASM,
  MCP, unplugin) **in a release build** — absent from the compiled artifact, not merely
  undocumented or unlinked-by-convention.

**Forbidden observable outcomes:**

- A second name-keyed value-resolution map inside the new solver (any
  `FxHashMap<&str, _>`- or `read_local`-shaped local-binding lookup that competes with
  graph-edge traversal).
- A second relation classifier: a new `RelationDomainRuntime`/`InferenceSession`/
  `SessionAdmissionLedger`-shaped type, or any `SemanticNodeId` comparison that does not
  route through `execute_relate`.
- A second `FunctionFlowGraph`-shaped structure, a second CFG, or a derived projection
  (dominators / loop forest / def-use overlay) that stores a semantic fact instead of
  being deterministically reconstructible from `(graph identity, profile)` alone — the
  exact prohibition `ADR-001` states.
- Any product/runtime-selectable flag, config value, or code path — in any crate — that
  can reach the new solver before `D2`'s cutover lands.
- A `CompleteFlowResult`-shaped value constructed anywhere outside its private
  finalizer.
- Porting `D3`-`D6`'s future algorithms (full nominal-identity comparability, full
  narrowing/closure/loop/completion semantics) into the private tree early merely to
  make a parity test pass. D1 owns re-deriving, graph-natively, whatever slice of
  today's behavior is ratified Supported/Stable (or already a typed `A3` gap); it does
  not pre-implement work `program.md` assigns to later blocks by name.

**Authority/fallback order.** New-solver value/effect resolution → `FunctionFlowGraph`
edges (sole structural authority per `ADR-001`, `architecture.md` §9.1) → stable binding
slots. Relation/inference decisions → `ProjectSemanticDispatch::execute`/`execute_relate`
(unchanged; `C1`-preserved, not `C1`-converged in the sense of new semantics). Anything
the closed domain registry does not recognize → typed unsupported obligation, never
silent success. There is **no** fallback from the new private solver to the old
production evaluator or vice versa inside D1's scope — they are two independent
implementations that happen to coexist pre-`D2`, never composed.

**Acceptance IDs.**

| ID | Requirement | Planned test/gate |
|---|---|---|
| D1-AC-1 | Value/effect resolution is graph-edge-native, never name-keyed | Type-level: the new solver's hot evaluation path takes no `&str`-keyed lookup type in its signature (reviewed at the module boundary, not grep-detected); behavioral: a private-boundary corpus reusing `A3`'s pinned `FIXED_CELLS`/G1-G11/154-row fixtures asserts the same observable result class (`Complete` / `Partial+FlowGap` / `NoValue`) as production for every fixture the new solver claims to cover |
| D1-AC-2 | Private hermetic boundary — unreachable from any release build | `#[cfg(any(test, feature = "test-support"))]`-gated module reusing the crate's EXISTING `test-support` feature (`crates/verter_semantic/Cargo.toml:22`, already documented as "never on the default/production closure"); a Cargo-metadata closure test proves no production `[dependencies]` edge (LSP/NAPI/WASM/MCP/unplugin binaries and their production dependency graphs) enables `test-support`, mirroring the existing dependency-firewall pattern `C1`/`B1` already use |
| D1-AC-3 | Obligation ledger + private finalizer | Compile-fail (`trybuild`) proof that the `CompleteFlowResult`-shaped type has no publicly reachable constructor; a mutation test that removes one discharge call makes the finalizer return the `ReturnOnly`/gap shape instead of `Complete` (`contracts/flow-completeness.md` §7: "mutation test dropping one obligation cannot yield `Complete`") |
| D1-AC-4 | Closed static domain registry; unregistered domain fails typed, not silently | Every dispatch site over the domain enum is an exhaustive `match` with no `_` wildcard (compile-time proof: adding a domain variant without handling it is `E0004`); a fixture that requests an intentionally-unregistered domain asserts a typed unsupported-obligation result, never a partial evaluation |
| D1-AC-5 | No second relation/inference authority | The new solver module holds no `pub(crate)`-or-broader path to construct `RelationDomainRuntime`/`InferenceSession`/`SessionAdmissionLedger` directly (those stay `pub(crate)` to their defining module; the new solver only calls the public dispatch entry) — a `trybuild` compile-fail fixture proves a type outside that defining module cannot construct them, mirroring `C1-AC-7`'s method |
| D1-AC-6 | Deterministic derived projections carry no semantic fact | Any dominator/loop-forest/def-use overlay the new solver builds is rebuilt twice from the same `(graph identity, profile)` and asserted byte-identical; a fixture proves it is never given its own persisted cache identity or treated as a second graph |
| D1-AC-7 | Typed-gap parity with `A3`'s already-shipped G1-G11 taxonomy | For every G1-G11 fixture in `u6_flow_a3_retraction_tests.rs`/successor, the private solver's result class matches production's (both `Partial+FlowGap` of at least as precise a reason, or both `NoValue`) — never a silent "solved" answer where production still typed-refuses |
| D1-AC-8 | Ratified capability-row coverage | For whatever row set the maintainer ratifies Supported/Stable before D1's candidate is dispatched (see Open Questions §1), every corpus fixture tagged to that row returns the same observable result class production gives today, via the new mechanism; every other row stays a typed gap. **Until ratification, this acceptance ID has an empty covered set and D1-AC-7 is the operative coverage floor** |
| D1-AC-9 | Exact parse reacquisition — at most one reborrow per cold demand | The new solver reborrows the retained parse snapshot through the same lease-only pattern the existing flow substrate already documents (`crates/verter_semantic/src/analysis/flow/mod.rs:1-18`: "borrows no OXC node... performs NO type lowering"; `decl_body_memo.rs`'s scheduler-retained-snapshot pattern), never a second ad hoc re-parse; a counter-instrumented fixture asserts exactly one reborrow per cold function-body demand regardless of how many domains that demand touches |
| D1-AC-10 | No product entry point reaches the private tree | A source/dependency-graph test (the kind `contracts/flow-completeness.md` §7 requires of `D2`, run here as a D1-scoped negative control) proves zero non-test, non-`test-support` call graph edge from `verter_lsp`/`verter_napi`/`verter_wasm`/`verter_mcp`/`unplugin` reaches the new module |

**Cold/warm/allocation/fan-out/latency bounds.** D1's private tree is, by its own
required outcome, unreachable from any product entry point — it therefore touches **no
existing locked performance-gate cell**, and this charter does not add one.
`A6_META_COMPILE_40_COLD_RUST` (the sole currently-locked cell) must show **zero**
measured delta from a D1 candidate, because nothing it exercises can compile into a
release artifact's hot path; that null result is itself part of D1-AC-2's proof, not a
separate performance obligation. D1 may **not** request a new locked cell for
production-facing flow performance — that is `D2`'s (and later `L2`'s) obligation, once
the solver is actually reachable. The one in-charter resource requirement is
`contracts/flow-completeness.md`'s own "no-flow path allocates no graph/plan" and
"exact parse reacquisition" properties (D1-AC-9 above), proven by the private-boundary
test suite itself, not by a gated SLO cell.

## Current-tree convergence map

| current authority | source | disposition under D1 | disposition under D2 |
|---|---|---|---|
| Demand-slicing graph substrate (already graph-shaped) | `crates/verter_semantic/src/analysis/flow/{mod.rs,flow_graph.rs,peeker.rs,hashing.rs,lower.rs,flow_ir.rs,frame_span.rs,value_descent.rs}` | Preserve + extend. D1 builds its VALUE/effect resolution on top of `FunctionFlowGraph`'s existing edge kinds; adds no second graph (`ADR-001`) | Becomes the sole structural authority for production, unchanged in kind |
| Syntax-shaped VALUE evaluator | `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` (8,416 lines), `flow_return_callee.rs` (733 lines) | Untouched in production. D1 builds a graph-native replacement privately, alongside it, answering the same demand shape | Deleted wholesale, in the same accepted candidate that routes public operations to the new solver |
| Content lowering (owned control-flow tree over the demanded slice) | `crates/verter_session/src/flow_slice_content.rs` (5,407 lines) | Untouched in production; D1's private tree builds its own graph-native content-lowering equivalent | Deleted, same D2 candidate |
| Obligation/coinductive-SCC substrate | `crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs` (`ObligationRuntime` `:879`, `ObligationReentryStack` `:396`, `ObligationFrameDomain::FlowReturn` `:318-325`, `FlowReturnFrameState` `:298`, `FlowReturnDomainRuntime` `:1868`) | Preserve. D1 integrates against this (see Open Questions §3 on how); does not duplicate the reentry/SCC mechanism | The `FlowReturn`-domain-specific arms become dead code once the new solver's obligation identity supersedes them; `D2` deletes them as part of "duplicate control/completion/relation/effect paths" per `program.md:243` |
| Typed gap taxonomy | `crates/verter_session/src/semantic_query.rs` (`FlowGap` enum, ~`:1547`) | Preserve/extend as the coverage floor (D1-AC-7); D1 may need additional domain-specific gap variants (e.g. covering `G10`/`AbruptCompletion` once `D6` lands) but must not weaken any existing variant's meaning | Final home decided at `D2`'s cutover — may fold into the new solver's own typed-gap vocabulary or stay as-is; not a D1 decision |
| Characterization corpus | `u6_flow_shape_corpus_tests.rs`, `u6_flow_expect_tests.rs`, `flow_return_*_tests.rs`, `flow_gap_retraction_tests.rs` (~20,000 lines total) | D1 REUSES these fixtures as its own private-boundary oracle (same expected outcomes, new implementation under test) — it does not re-author a parallel fixture set | `D2` owns final consolidation: fixtures that only exercised the deleted evaluator's internals are retired; fixtures that assert observable behavior are re-targeted at the sole solver |

## Legacy deletions

**D1 deletes nothing in production.** This is a deliberate, explicit statement, not an
omission: `ADR-014` decision 2 and `contracts/flow-completeness.md` §6 both make the old
evaluator's deletion `D2`'s ONE atomic act ("In one public cutover... delete the old
syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and
migration guards"). A D1 candidate that deletes or even behaviorally alters
`flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` — or any of their
production call sites — is out of scope and should be treated as a design violation, not
merely an aggressive interpretation. A reviewer should expect **zero diff** in those
three files (and in `dispatch_txn.rs`'s existing `FlowReturn`-domain arms) from a
conforming D1 candidate.

What D1 must still not leave behind, inside its own scope:

- Any private-boundary scaffolding that is not part of the final D1 exit surface
  (ordinary hygiene — draft types, abandoned domain stubs).
- Any temporary shim that duplicates existing obligation/relation machinery instead of
  calling it (the forbidden-outcomes list above), even if scoped
  `#[cfg(any(test, feature = "test-support"))]` — the private boundary makes it
  unreachable in production, but a duplicate authority inside the private tree still
  violates `program.md:229`'s "contains no name-keyed or flow-private relation
  authority."
- Block-identifier-named evidence scaffolding, per this program's own hygiene rule
  (`charters/A3.md` "In scope": "deletion or renaming of block-named evidence
  scaffolding before landing").

**Named for continuity — `D2`'s eventual deletion list, sourced from this research, not
executed here:** `flow_return.rs`, `flow_return_callee.rs`, `flow_slice_content.rs` in
full; `dispatch_txn.rs`'s `ObligationFrameDomain::FlowReturn`, `FlowReturnFrameState`,
`FlowReturnPendingState`/`FlowReturnPendingOutcome`, `CompletedFlowReturnMember`,
`FlowReturnDomainRuntime`; and, per `A3-SPEC.md`'s own forward note, `semantic_query.rs`'s
`FlowReturnStep`/`FlowReturnFailure`/`FlowReturnResult` shapes "to the extent D1's final
typestate cutover supersedes them" ("The legacy name `FlowReturnStep::Complete` is
misleading for degraded payloads, but renaming or replacing that outcome type belongs to
D1/D2's final typestate cutover" — `A3-SPEC.md` §1.4). D1's own charter binds none of
this list as its exit obligation; it is recorded here so `D2`'s charter does not have to
re-derive it from scratch, and so a D1 reviewer can confirm D1 did not quietly start
executing it early.

## Structural confinement

Every invariant above is enforced structurally where the codebase already has the
mechanism, and named honestly as a convention where it does not:

- **Hermetic boundary — genuinely structural.** `#[cfg(any(test, feature =
  "test-support"))]`, reusing the `test-support` feature `verter_semantic` (and
  `verter_session`) already declare (`crates/verter_semantic/Cargo.toml:12-22`,
  already documented: "never on the default/production closure, so release builds
  compile... without" the gated code). A module gated this way does not exist in a
  release compilation unit at all — not merely undocumented or unlinked by convention.
  The existing Cargo-metadata dependency-firewall guard pattern (`C1.md`'s "crate
  dependency firewall is the primary proof, not a marker" — `crates/verter_identity/tests/cases/workspace_dependency_layers.rs`)
  extends naturally to prove no production `[dependencies]` edge enables
  `test-support` transitively; this is the SAME proof shape already landed for a
  different invariant, not a new mechanism.
- **Private finalizer — genuinely structural, existing pattern.** A `CompleteFlowResult`-shaped
  type with a private constructor, buildable only inside its own module by its own
  finalizer function, is the same shape this codebase already uses elsewhere
  (`MaterializeStructureDb`'s sink-private constructors, the `RequestBoundResolverContext`
  sealed-marker pattern `C1.md` cites). A `trybuild` compile-fail fixture is the
  existing proof idiom (`C1-AC-7`).
- **Closed-enum domain registry and typed gaps — genuinely structural, WITH a caveat.**
  An exhaustive `match` with no `_` arm over the domain enum turns "someone adds a
  domain and forgets a call site" into `E0004` at compile time — real, and consistent
  with this program's existing "EXHAUSTIVE match, no wildcard" rule
  (`framework-adapters` skill: `ExecutorResolveCtx`). The caveat: this protects against
  an **omitted** domain, not a domain handled **wrong** — a match arm that silently
  returns a plausible-looking default instead of the correct discharge still compiles
  and still passes an exhaustiveness check. That half is a test-coverage obligation
  (D1-AC-1/D1-AC-7's behavioral parity corpus), not a structural one; this charter does
  not claim otherwise.
- **No-second-relation-authority — structural via VISIBILITY, not via "nothing calls
  it."** Per this task's own falsification warning: an "exhaustive test double" or "no
  current call site" claim is not proof — a contributor adding a method next year could
  satisfy either trivially. The actual mechanism here is narrower and load-bearing:
  `RelationDomainRuntime`/`InferenceSession`/`SessionAdmissionLedger` stay `pub(crate)`
  to their OWN defining module (today `project_semantic_dispatch`; post-`C1`, whichever
  module inside `verter_semantic` inherits that scope). D1's new solver module must be
  placed OUTSIDE that visibility boundary, so it physically cannot name a constructor
  for those types — the same enforcement `HostStoreView`'s immutability already gets
  from holding no `&VerterHost`/`Arc<Mutex<_>>` field (`C1.md`'s "already structural, not
  a convention" bullet). A contributor who wants to bypass this would have to WIDEN the
  visibility modifier on the shared type — a one-line diff any reviewer sees, not a
  silent addition.
- **Graph-edge-native value resolution — NOT fully structural; named as such.** Nothing
  in Rust's type system stops a contributor from adding a `&str`-keyed `HashMap` inside
  the new solver next to graph-edge traversal; there is no marker type that makes
  "resolves via graph edges" a compile-time property the way `NoTypeExpr` makes
  "content-free" one. This is a **design/review discipline**, proven by the behavioral
  parity corpus (D1-AC-1) and caught at review, not by the compiler. Calling it
  structural would repeat the exact failure mode this task warned against (a trait
  default method silently satisfied); this charter does not make that claim.

## Boundary against predecessors and successors

**Against `C1`.** D1 depends on `C1`'s converged `ProjectSemanticDispatch`/relation
authority being physically resident where D1 will build against it. D1 does not
re-litigate `C1`'s crate-placement ruling (`ARCH-RULING-C1-FOUR-FORKS.md`) and treats its
outcome as given, whichever module boundary it lands with — see Open Questions §2 for
the one genuine gap C1's own charter left unaddressed (whether `flow_return.rs` et al.
physically relocate under `C1` at all).

**Against `A3`.** D1 preserves `A3`'s typed-gap taxonomy (`FlowGap` and its eleven
classes) and the 154-row checker-correct clean/warm preservation cohort as its own
private-boundary oracle. D1 must not weaken any `A3` guarantee — in particular, D1's
private tree answering a case `A3` retracted must retract it too, never "solve" it back
to a guessed value without that becoming a maintainer-ratified capability decision
(`flow-completeness.md` §6: "unless a separate reviewed breaking product decision
changes that row").

**Against `D2`.** D1 explicitly does not perform the atomic cutover, does not delete
`flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs`, does not touch any
product entry point, and is not independently mergeable or releasable
(`contracts/flow-completeness.md` §6: "`D1` is a review checkpoint, not an independently
mergeable/releasable production block").

**Against `D3`-`D8`.** D1 does not implement `D3`'s full nominal-identity/unique-symbol
comparability extension, `D4`'s full narrowing/structural-return algebra, `D5`'s full
closure/capture join semantics, `D6`'s completion graph (debt `FR-D8`/`G10` stays open —
ruling R-11), `D7`'s `this`/sequence/call-context surfaces, or `D8`'s final U6 admission
proof. D1 builds `architecture.md` §9.1-9.4 for real (structural authority, derived
projections, demand plan, binding/relation authority) plus exactly enough of §9.5-9.9 to
cover whatever the A6 matrix ends up ratifying (today: nothing — see Open Questions §1);
everything else is a typed gap those later blocks close.

## Required exit

Per `program.md:233`, elaborated against the research above: the minimum private
replacement (a) preserves every `A6`-ratified Supported/Stable effective-flow capability
row — today an empty set, so this reduces to D1-AC-7's G1-G11 parity floor until
ratification — (b) safely answers its covered cases via graph-native resolution over
`FunctionFlowGraph`, never a re-derived name-keyed map, (c) fails every unsupported case
through a typed gap at least as precise as `A3`'s existing taxonomy, and (d) does so with
zero product entry point able to reach it and zero second production authority
(relation, graph, or obligation) constructed anywhere in the private tree. `D1` may
receive checkpoint review approval under this exit; it does not merge or release
independently of `D2`.

## Review

Foundational: three mandates, three independent contexts, one candidate SHA and tree
(`governance.md` §2.3, mirroring `B1.md`/`C1.md`).

| mandate | scope |
|---|---|
| conformance | charter, diff, and the private-boundary proof — specifically whether the new module is genuinely absent from every release compilation unit (D1-AC-2), and whether the G1-G11/154-row parity corpus actually passes against the NEW solver, not merely re-asserts production's existing pins |
| architecture | diff plus the authority package — specifically whether value/effect resolution is genuinely graph-edge-native (not a relabelled name-keyed map), whether the obligation ledger is a genuinely distinct mechanism from `dispatch_txn.rs`'s existing `ObligationRuntime` and how the two compose (Open Questions §3), whether any derived projection smuggles a semantic fact (`ADR-001`), and whether the visibility fence against `RelationDomainRuntime`/`InferenceSession` construction actually holds |
| adversarial performance/memory | diff plus `A6_META_COMPILE_40_COLD_RUST` raw output — the required result is **zero measured delta**, since nothing D1 adds can compile into a release artifact; this mandate's real job is proving that null result plus checking the private test suite's own resource use against the ~20,000-line reused corpus (no leak, no unbounded growth, no timeout risk added to the canonical gate) |

## Abort/rescope

Stop for: a discovered fourth production consumer of the old evaluator this research did
not find (would change what "unreachable from product entry points" must guarantee);
`C1` landing with a convergence map that does **not** relocate
`project_semantic_dispatch`'s `flow_return*.rs`/`dispatch_txn.rs` as Open Questions §2
assumes one way or the other (changes which crate D1's private module lives in, and
whether D1 can meaningfully start before that is settled); the A6 capability matrix
remaining entirely `VERIFY` at D1 dispatch time with no maintainer ruling on whether that
blocks dispatch (Open Questions §1); `AMD-001`'s stack-window validator/CI-wiring/`D1`-`D2`
transition test still undelivered when this charter's work would need D1's ledger status
to advance to `PRIVATE_CHECKPOINT` (ruling R-12's explicit precondition); or a discovery
that `contracts/flow-completeness.md` §4's `ObligationState` ledger and `dispatch_txn.rs`'s
existing `ObligationRuntime` cannot coexist without one absorbing the other's job in a
way that either duplicates the coinductive-cycle mechanism (forbidden by
`program.md:229`) or overloads it with an unrelated bookkeeping concern (Open Questions
§3). A discovery at this bar reopens the relevant ruling, not a quiet local substitution.

## Open questions requiring maintainer/architecture ruling

This charter does not guess at the following; each is a genuine fork the code and the
existing normative documents leave open, and a wrong guess here has already cost this
program rework on other blocks (`C1`'s four forks, `B4`'s emitter-disposition ledger).

1. **Capability-matrix ratification timing (A6 lock record U-1).** The matrix is
   entirely `VERIFY`; D1's own governing contract makes its per-row coverage obligation
   literally vacuous until ratification, and the lock record itself calls this "a real
   gap, not a pass" whose resolution point is "before the atomic flow-cutover block
   begins." Is D1 dispatchable in a meaningful sense before that ratification — landing
   with D1-AC-7 (G1-G11 parity) as its only real coverage floor and D1-AC-8 vacuously
   satisfied — or does U-1 in fact block D1 from being `BLOCK_READY` at all until the
   maintainer ratifies real Supported/Stable rows? Recorded as U-1's owner is "product/conformance
   review with the maintainer"; this charter cannot self-ratify that decision.

2. **Does `C1`'s relocation of "the `project_semantic_dispatch` module" include
   `flow_return.rs`/`flow_return_callee.rs`/`dispatch_txn.rs`, or are they a carve-out
   staying in `verter_session`?** `C1.md`'s Context §1 says "the private `resolver_store`
   module, and the `pub(crate)` `project_semantic_dispatch` module — all three currently
   living inside `verter_session`... physically relocate into the existing
   `verter_semantic` crate," which by file-tree scope includes every file under
   `project_semantic_dispatch/`, including the 8,416-line `flow_return.rs` D1/D2 exist to
   replace. But `C1.md`'s convergence map and Legacy Deletions sections never mention
   `flow_return.rs`, `flow_return_callee.rs`, or `dispatch_txn.rs` by name anywhere, and
   `C1.md`'s own boundary section disclaims flow ownership ("C1 does not own flow
   semantics (`D1`-`D8`)"). Ownership disclaimer and physical file relocation are not the
   same thing — C1 could relocate the code while explicitly not touching its semantics,
   the same way it relocates `execute_relate` unchanged. If flow_return.rs et al. DO move
   under C1, D1's private module lives in `verter_semantic` (a compiler-tier crate)
   alongside ~14,500 lines of pre-cutover production code it is explicitly forbidden to
   touch; if they do NOT move (an implicit carve-out `C1.md` never states), D1's private
   module more naturally lives in `verter_session` next to the code it is replacing, and
   the crate D1 builds in changes. This determines which crate hosts D1's work and
   should be settled — ideally inside `C1`'s own charter revision — before D1 dispatch.

3. **How does D1's obligation ledger (`contracts/flow-completeness.md` §4's generic
   `ObligationState`) relate to `dispatch_txn.rs`'s EXISTING `ObligationRuntime`/
   `ObligationPendingLedger`/`ObligationFrameDomain::FlowReturn` coinductive SCC
   substrate?** Two readings are both defensible from the text and neither is provable
   from the code alone: (a) they are the SAME mechanism — D1 extends
   `ObligationFrameDomain` with new flow-specific domain variants and reuses the
   existing reentry stack/SCC discharge, so "integration with the shared relation
   authority" (`program.md:229`) literally means "shares this struct"; or (b) they are
   genuinely SEPARATE layers — D1's ledger tracks per-`FlowDemandPlan` REQUIRED-DOMAIN
   discharge (a simple state machine per obligation, per `flow-completeness.md`'s own
   pseudocode), nested inside or alongside one `ObligationFrame`'s existing `FlowReturn`
   arm, which stays scoped to cross-engine SCC/re-entry bookkeeping only. Picking (a)
   without ratification risks conflating two genuinely different concerns (cross-engine
   coinductive cycles vs. within-one-plan domain completeness) into one type; picking
   (b) without ratification risks building a second obligation-tracking mechanism next
   to an existing one, which is exactly what `program.md:229`'s "no... flow-private
   relation authority" forbids if drawn at the wrong boundary. Needs an architecture
   ruling before implementation, not an implementer's local choice.

4. **Which A6 capability-matrix rows actually count as "effective-flow capability
   rows"?** The seed matrix's eight rows
   (`evidence/A6/implementation-lock-record.md` §3) are framework/product-scoped (Vue
   runtime compile, Vue IDE companion, Vue imported macro runtime projection, Svelte
   native runtime compile, TypeInfo `TypeAtPosition`, TypeInfo graph export, LSP external
   TypeScript provider, CSS parse/format/index/transform) — none names "effective-flow"
   or `FlowReturn` at the granularity `program.md:231`'s "every effective-flow
   capability row" requires. Does D1 need the maintainer to ADD flow-specific rows to
   the matrix (a matrix schema change, beyond simply flipping existing rows'
   `Status`) before its coverage obligation is even well-defined, or does an existing
   row (e.g. "TypeInfo `TypeAtPosition`") implicitly carry flow sub-obligations that
   ratification would need to state explicitly?

5. **Is "stable binding slots" (`program.md:229`) the existing `SkeletonBindingId`/
   `FlowNodeId` pair as-is, or a new cross-domain "slot" abstraction D1 must define on
   top of them?** `D3`'s own charter text says it will "complete stable binding slots"
   (`program.md:253`), which implies they are not expected to be complete even after
   D1. Without a ruling on where the D1/D3 boundary sits for this specific mechanism, an
   implementer risks either under-building (leaving D3 with no real foundation to
   "complete") or over-building (doing D3's job early, which this charter's Boundary
   section above forbids). This is the narrowest of the five open questions but the one
   most likely to cause silent scope creep in either direction during implementation.

6. **Are AMD-001's undelivered artifacts (ruling R-12) a hard gate on D1 STARTING
   implementation, or only on D1's ledger status advancing to `PRIVATE_CHECKPOINT` once
   the implementation work is already done?** The ruling's own wording
   ("unconditionally before `D1` enters `PRIVATE_CHECKPOINT`") reads as the latter, which
   would let implementation proceed now with the ledger-mechanics prerequisite satisfied
   later, in parallel. But R-12 also generically warns "AMD-001 §3's unchanged
   fail-closed refusal already prevents the unsupported `D1`/`D2` transition
   meanwhile" — worth an explicit maintainer confirmation before dispatch rather than an
   implementer inferring it from ruling prose under time pressure.

[35m[3mcodex[0m[0m
The draft’s core ambiguity is real, but one premise is already stale: it still describes AMD-001’s validator package as undelivered. I’m reading the omitted middle sections now before testing that and the other five premises against the ratified evidence.
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md | sed -n '1,170p'; nl -ba <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md | sed -n '171,340p'; nl -ba <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md | sed -n '341,520p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 11013ms:[0m
     1	# D1 — Private sole-flow-solver foundation
     2	
     3	**Status:** DRAFT pending maintainer ratification. **Class:** Foundational
     4	(`governance.md` §2.3 names "flow/effective typing" explicitly as a Foundational
     5	example area; `program-dag.toml` / `program.md:225-233`). **Predecessors:** `A3`, `B1`,
     6	`B2` — all three `ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml:157-176,245-264,465-484`)
     7	— and `C1`, currently `status = "READY"` but its own charter is **DRAFT, unratified**
     8	(`docs/arch/architecture-lock/ledger/program-state.toml:619-638`; `charters/C1.md:3`).
     9	D1 cannot be `BLOCK_READY` while a predecessor's charter is unratified; see Sequencing.
    10	
    11	## Context — why this is far bigger than program.md's nine lines
    12	
    13	`program.md:225-233` gives D1 one paragraph: build canonical graph adapters, derived
    14	projections, a domain registry, a demand plan, binding slots, relation-authority
    15	integration, an obligation ledger, a private finalizer, parse reacquisition, and typed
    16	gaps — all behind a hermetic non-production boundary — so that it "preserves every
    17	ratified supported capability" before `D2`'s cutover. Read literally that sounds like a
    18	scaffolding exercise. **It is not**, for the same reason this program has repeatedly
    19	found blocks larger than their summary: the paragraph describes architecture.md §9's
    20	full target shape (§9.1-9.9), and the code it must functionally match today is real,
    21	large, and already characterized by ~20,000 lines of pinned regression tests.
    22	
    23	**The thing being replaced is not small.** The current effective-flow ("`FlowReturn`")
    24	answer is produced by a private, syntax-shaped, name-keyed evaluator:
    25	
    26	- `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` — 8,416 lines
    27	- `crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs` — 733 lines
    28	- `crates/verter_session/src/flow_slice_content.rs` — 5,407 lines
    29	
    30	— roughly 14,500 production lines — pinned by roughly 20,000 lines of characterization
    31	tests (`flow_return_coverage_tests.rs` 3,154; `flow_return_lexical_tests.rs` 6,683;
    32	`flow_return_tests.rs` 4,531; `flow_return_positional_tests.rs` 463;
    33	`flow_return_root_gate_tests.rs` 680; `flow_return_frame_seal_tests.rs` 409;
    34	`flow_gap_retraction_tests.rs` 672; `u6_flow_shape_corpus_tests.rs` 4,084). D1 must
    35	reproduce, through a wholly different graph-native mechanism, whatever slice of that
    36	14,500-line surface the (currently unratified — see Open Questions) A6 capability
    37	matrix calls Supported/Stable, and must answer everything else with a typed gap at
    38	least as precise as the eleven (`G1`-`G11`) gap classes `A3` already shipped
    39	(`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md` §5). That is not "scaffolding
    40	behind a flag"; it is standing up a second, competing, complete-enough analysis engine
    41	under the same hermetic roof.
    42	
    43	**The target is already normatively specified, in more detail than program.md itself.**
    44	Four documents bind D1 more precisely than the block's own paragraph:
    45	[`contracts/flow-completeness.md`](../contracts/flow-completeness.md) (the "Flow Domain
    46	Closure, Obligation Ledger, and Completeness Contract" — normative), `architecture.md`
    47	§9 ("Effective typing and flow", §9.1-9.9), `decisions/ADR-001-semantic-authority-and-derived-projections.md`,
    48	and `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md`. This charter treats
    49	those four as the binding shape and program.md's paragraph as their one-line summary,
    50	consistent with how `C1.md`'s Context section treated `program.md:191-197` — effort and
    51	breadth are not reasons to shrink a charter (`CLAUDE.md` → Planning).
    52	
    53	**What D1 does NOT do**, stated up front because it is easy to over-read the paragraph:
    54	it does not delete `flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` (that
    55	is `D2`'s one atomic cutover, per `ADR-014` decision 2); it does not touch any product
    56	entry point; it does not implement nominal-identity extension (`D3`), the full
    57	narrowing/structural-return algebra (`D4`), closure/capture join semantics (`D5`), the
    58	completion graph that closes debt `FR-D8`/`G10` (`D6` — still explicitly open per ruling
    59	R-11), `this`/sequence/call-context surfaces (`D7`), or the final U6 admission proof
    60	(`D8`). D1 builds the foundation those six blocks extend.
    61	
    62	## Sequencing
    63	
    64	Per `docs/arch/architecture-lock/ledger/program-state.toml`: `A3`, `B1`, `B2` are
    65	`ACCEPTED`; `C1` is `status = "READY"` (dependency-eligible) but has no accepted
    66	candidate, and its own charter (`charters/C1.md:3`) is explicitly "DRAFT — authored for
    67	maintainer review; no AMD ratifies it yet." The ledger's `current_block` at this
    68	charter's authoring time is `BS1` (`program-state.toml:22`), unrelated to Track D. **D1
    69	dispatch follows both whatever is currently in flight and C1's acceptance** — the same
    70	sequencing discipline `C1.md` itself applied to `BV1` ("C1 dispatch follows BV1's
    71	landing... This charter does not propose running C1 in parallel").
    72	
    73	Two further preconditions are recorded in already-accepted evidence and are **not**
    74	discharged by this charter, only named so they are not silently skipped:
    75	
    76	1. **AMD-001 / ruling R-12.** The Node stack-window validator, composite
    77	   program-state cross-validation, its CI wiring, and a discriminating `D1`/`D2`
    78	   transition test are due "unconditionally before `D1` enters `PRIVATE_CHECKPOINT`"
    79	   (`evidence/maintainer-rulings.md` R-12, `AMD-001-stack-window-validator-prerequisite.md`).
    80	   `A6`'s own lock record records these as **not yet delivered**
    81	   (`evidence/A6/implementation-lock-record.md` §11 row U-9). The composite validator
    82	   must accept `D2` "only when `D1` is the declared private checkpoint in the same
    83	   validated `ATOMIC_REVIEW` snapshot with `D2` as its acceptance block" — that
    84	   machinery does not exist in the ledger tooling today.
    85	2. **A6 lock record U-1 — capability matrix ratification.** `contracts/capability-matrix.md`'s
    86	   eight seed rows are **all `Status = VERIFY`**
    87	   (`evidence/A6/implementation-lock-record.md` §3, §11 row U-1). D1's own governing
    88	   text ("it covers every effective-flow capability row declared Supported/Stable by
    89	   the `A6` matrix" — `contracts/flow-completeness.md` §6; `program.md:231`) is,
    90	   by the lock record's own admission, **currently vacuous**: "with none declared, the
    91	   obligation is vacuous until the matrix is ratified... the matrix must be ratified
    92	   before that block begins." See Open Questions §1.
    93	
    94	## Intent Contract
    95	
    96	**Actor / problem.** Effective-flow answers (function return-type inference feeding
    97	`defineProps`/`defineEmits`/component-meta and any other consumer of
    98	`SemanticQueryKey::FlowReturn`) are produced today by a private evaluator that resolves
    99	local values through its own `read_local(&mut self, name: &str)` map
   100	(`flow_return.rs:4353`) and its own ad hoc reaching-definition bookkeeping — not by
   101	walking the typed-edge `FunctionFlowGraph` dependence structure
   102	(`crates/verter_semantic/src/analysis/flow/flow_graph.rs:155`,
   103	`build_function_flow_graph` at `:243`) that already exists beside it, built for demand
   104	SLICING only (`peeker.rs`, `hashing.rs`, `lower.rs`). `A3` (accepted) proved specific
   105	rows of this evaluator wrong-and-warm and retracted them behind typed `FlowGap` markers,
   106	but was explicitly forbidden to replace the evaluator: "adding `FlowSolveOutcome`, proof
   107	tokens, or an obligation ledger in A3 is prohibited scope expansion... belongs to
   108	D1/D2's final typestate cutover" (`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md`
   109	§12.1, and its Verdict paragraph). `D3`-`D8` each need a graph-native binding/relation
   110	authority to extend — nominal identity, narrowing, closure, loops, completion — without
   111	re-deriving name-keyed state per block. That authority does not exist. D1 is the block
   112	that builds it, privately, so `D2` can perform ADR-014's one atomic cutover without ever
   113	exposing two selectable production semantics engines at once.
   114	
   115	**Required observable outcomes** (all inside the private hermetic boundary; none are
   116	reachable in a release build):
   117	
   118	- Every value/effect fact the new solver produces is derived by walking
   119	  `FunctionFlowGraph` edges (`FlowEdgeKind::ValueDef` / `PathWrite` / `EvalEffect` /
   120	  `ControlRegion`) keyed by stable `SkeletonBindingId`/`FlowNodeId` slots — never a
   121	  `&str`-keyed lookup into a locally built map, and never a re-walk of the OXC AST for a
   122	  fact the graph already encodes.
   123	- A `FlowDemandPlan` (`architecture.md` §9.3) is computed once per cold logical demand
   124	  by taking the transitive closure of named obligations through one closed static domain
   125	  registry; naming an unregistered domain produces a typed unsupported obligation at
   126	  plan time, never a silent skip and never a partial plan the solver evaluates anyway.
   127	- A private obligation ledger (`contracts/flow-completeness.md` §4 — modelled, not
   128	  necessarily named, on `ObligationState { Pending, Running, Discharged(evidence),
   129	  Gap(FlowGap), Failed(reason) }`) discharges every required/expanded obligation before
   130	  a private finalizer can construct a `CompleteFlowResult`-shaped value; that
   131	  constructor is unreachable from outside its owning module (no public `new`, no
   132	  `Default` that yields a "complete" discriminant, no test-only escape left reachable
   133	  outside `#[cfg(test)]`).
   134	- Every `G1`-`G11` case `A3` already typed as a gap
   135	  (`docs/arch/architecture-lock/ledger/A3/A3-SPEC.md` §5's per-gap table) is answered by
   136	  the new solver with an equal-or-more-precise typed gap — never silently "solved" where
   137	  production still honestly refuses.
   138	- Authored/legitimately-inferred semantic `any` stays a distinct value from "Verter has
   139	  no model" (`contracts/flow-completeness.md` §5: "An authored `any` is a semantic
   140	  value. 'Verter has no model' is a gap and cannot become `any`"), mirroring the closed
   141	  split `A3` already installed (`SliceExpr::SemanticAny` vs. `SliceExpr::Gap(FlowGap)`,
   142	  `flow_slice_content.rs`) or D1's own graph-native equivalent of it.
   143	- Relation/inference decisions — nominal comparability, union/intersection membership,
   144	  coinductive SCC discharge — route exclusively through the existing shared authority:
   145	  `ProjectSemanticDispatch::execute`/`execute_relate` plus the `ObligationRuntime`/
   146	  `ObligationReentryStack` tagged-identity space
   147	  (`crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:879,396`,
   148	  physically relocating into `verter_semantic` under `C1` per its convergence map) —
   149	  never a second copy of that machinery.
   150	- The entire private tree is unreachable from any product entry point (LSP, NAPI, WASM,
   151	  MCP, unplugin) **in a release build** — absent from the compiled artifact, not merely
   152	  undocumented or unlinked-by-convention.
   153	
   154	**Forbidden observable outcomes:**
   155	
   156	- A second name-keyed value-resolution map inside the new solver (any
   157	  `FxHashMap<&str, _>`- or `read_local`-shaped local-binding lookup that competes with
   158	  graph-edge traversal).
   159	- A second relation classifier: a new `RelationDomainRuntime`/`InferenceSession`/
   160	  `SessionAdmissionLedger`-shaped type, or any `SemanticNodeId` comparison that does not
   161	  route through `execute_relate`.
   162	- A second `FunctionFlowGraph`-shaped structure, a second CFG, or a derived projection
   163	  (dominators / loop forest / def-use overlay) that stores a semantic fact instead of
   164	  being deterministically reconstructible from `(graph identity, profile)` alone — the
   165	  exact prohibition `ADR-001` states.
   166	- Any product/runtime-selectable flag, config value, or code path — in any crate — that
   167	  can reach the new solver before `D2`'s cutover lands.
   168	- A `CompleteFlowResult`-shaped value constructed anywhere outside its private
   169	  finalizer.
   170	- Porting `D3`-`D6`'s future algorithms (full nominal-identity comparability, full
   171	  narrowing/closure/loop/completion semantics) into the private tree early merely to
   172	  make a parity test pass. D1 owns re-deriving, graph-natively, whatever slice of
   173	  today's behavior is ratified Supported/Stable (or already a typed `A3` gap); it does
   174	  not pre-implement work `program.md` assigns to later blocks by name.
   175	
   176	**Authority/fallback order.** New-solver value/effect resolution → `FunctionFlowGraph`
   177	edges (sole structural authority per `ADR-001`, `architecture.md` §9.1) → stable binding
   178	slots. Relation/inference decisions → `ProjectSemanticDispatch::execute`/`execute_relate`
   179	(unchanged; `C1`-preserved, not `C1`-converged in the sense of new semantics). Anything
   180	the closed domain registry does not recognize → typed unsupported obligation, never
   181	silent success. There is **no** fallback from the new private solver to the old
   182	production evaluator or vice versa inside D1's scope — they are two independent
   183	implementations that happen to coexist pre-`D2`, never composed.
   184	
   185	**Acceptance IDs.**
   186	
   187	| ID | Requirement | Planned test/gate |
   188	|---|---|---|
   189	| D1-AC-1 | Value/effect resolution is graph-edge-native, never name-keyed | Type-level: the new solver's hot evaluation path takes no `&str`-keyed lookup type in its signature (reviewed at the module boundary, not grep-detected); behavioral: a private-boundary corpus reusing `A3`'s pinned `FIXED_CELLS`/G1-G11/154-row fixtures asserts the same observable result class (`Complete` / `Partial+FlowGap` / `NoValue`) as production for every fixture the new solver claims to cover |
   190	| D1-AC-2 | Private hermetic boundary — unreachable from any release build | `#[cfg(any(test, feature = "test-support"))]`-gated module reusing the crate's EXISTING `test-support` feature (`crates/verter_semantic/Cargo.toml:22`, already documented as "never on the default/production closure"); a Cargo-metadata closure test proves no production `[dependencies]` edge (LSP/NAPI/WASM/MCP/unplugin binaries and their production dependency graphs) enables `test-support`, mirroring the existing dependency-firewall pattern `C1`/`B1` already use |
   191	| D1-AC-3 | Obligation ledger + private finalizer | Compile-fail (`trybuild`) proof that the `CompleteFlowResult`-shaped type has no publicly reachable constructor; a mutation test that removes one discharge call makes the finalizer return the `ReturnOnly`/gap shape instead of `Complete` (`contracts/flow-completeness.md` §7: "mutation test dropping one obligation cannot yield `Complete`") |
   192	| D1-AC-4 | Closed static domain registry; unregistered domain fails typed, not silently | Every dispatch site over the domain enum is an exhaustive `match` with no `_` wildcard (compile-time proof: adding a domain variant without handling it is `E0004`); a fixture that requests an intentionally-unregistered domain asserts a typed unsupported-obligation result, never a partial evaluation |
   193	| D1-AC-5 | No second relation/inference authority | The new solver module holds no `pub(crate)`-or-broader path to construct `RelationDomainRuntime`/`InferenceSession`/`SessionAdmissionLedger` directly (those stay `pub(crate)` to their defining module; the new solver only calls the public dispatch entry) — a `trybuild` compile-fail fixture proves a type outside that defining module cannot construct them, mirroring `C1-AC-7`'s method |
   194	| D1-AC-6 | Deterministic derived projections carry no semantic fact | Any dominator/loop-forest/def-use overlay the new solver builds is rebuilt twice from the same `(graph identity, profile)` and asserted byte-identical; a fixture proves it is never given its own persisted cache identity or treated as a second graph |
   195	| D1-AC-7 | Typed-gap parity with `A3`'s already-shipped G1-G11 taxonomy | For every G1-G11 fixture in `u6_flow_a3_retraction_tests.rs`/successor, the private solver's result class matches production's (both `Partial+FlowGap` of at least as precise a reason, or both `NoValue`) — never a silent "solved" answer where production still typed-refuses |
   196	| D1-AC-8 | Ratified capability-row coverage | For whatever row set the maintainer ratifies Supported/Stable before D1's candidate is dispatched (see Open Questions §1), every corpus fixture tagged to that row returns the same observable result class production gives today, via the new mechanism; every other row stays a typed gap. **Until ratification, this acceptance ID has an empty covered set and D1-AC-7 is the operative coverage floor** |
   197	| D1-AC-9 | Exact parse reacquisition — at most one reborrow per cold demand | The new solver reborrows the retained parse snapshot through the same lease-only pattern the existing flow substrate already documents (`crates/verter_semantic/src/analysis/flow/mod.rs:1-18`: "borrows no OXC node... performs NO type lowering"; `decl_body_memo.rs`'s scheduler-retained-snapshot pattern), never a second ad hoc re-parse; a counter-instrumented fixture asserts exactly one reborrow per cold function-body demand regardless of how many domains that demand touches |
   198	| D1-AC-10 | No product entry point reaches the private tree | A source/dependency-graph test (the kind `contracts/flow-completeness.md` §7 requires of `D2`, run here as a D1-scoped negative control) proves zero non-test, non-`test-support` call graph edge from `verter_lsp`/`verter_napi`/`verter_wasm`/`verter_mcp`/`unplugin` reaches the new module |
   199	
   200	**Cold/warm/allocation/fan-out/latency bounds.** D1's private tree is, by its own
   201	required outcome, unreachable from any product entry point — it therefore touches **no
   202	existing locked performance-gate cell**, and this charter does not add one.
   203	`A6_META_COMPILE_40_COLD_RUST` (the sole currently-locked cell) must show **zero**
   204	measured delta from a D1 candidate, because nothing it exercises can compile into a
   205	release artifact's hot path; that null result is itself part of D1-AC-2's proof, not a
   206	separate performance obligation. D1 may **not** request a new locked cell for
   207	production-facing flow performance — that is `D2`'s (and later `L2`'s) obligation, once
   208	the solver is actually reachable. The one in-charter resource requirement is
   209	`contracts/flow-completeness.md`'s own "no-flow path allocates no graph/plan" and
   210	"exact parse reacquisition" properties (D1-AC-9 above), proven by the private-boundary
   211	test suite itself, not by a gated SLO cell.
   212	
   213	## Current-tree convergence map
   214	
   215	| current authority | source | disposition under D1 | disposition under D2 |
   216	|---|---|---|---|
   217	| Demand-slicing graph substrate (already graph-shaped) | `crates/verter_semantic/src/analysis/flow/{mod.rs,flow_graph.rs,peeker.rs,hashing.rs,lower.rs,flow_ir.rs,frame_span.rs,value_descent.rs}` | Preserve + extend. D1 builds its VALUE/effect resolution on top of `FunctionFlowGraph`'s existing edge kinds; adds no second graph (`ADR-001`) | Becomes the sole structural authority for production, unchanged in kind |
   218	| Syntax-shaped VALUE evaluator | `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` (8,416 lines), `flow_return_callee.rs` (733 lines) | Untouched in production. D1 builds a graph-native replacement privately, alongside it, answering the same demand shape | Deleted wholesale, in the same accepted candidate that routes public operations to the new solver |
   219	| Content lowering (owned control-flow tree over the demanded slice) | `crates/verter_session/src/flow_slice_content.rs` (5,407 lines) | Untouched in production; D1's private tree builds its own graph-native content-lowering equivalent | Deleted, same D2 candidate |
   220	| Obligation/coinductive-SCC substrate | `crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs` (`ObligationRuntime` `:879`, `ObligationReentryStack` `:396`, `ObligationFrameDomain::FlowReturn` `:318-325`, `FlowReturnFrameState` `:298`, `FlowReturnDomainRuntime` `:1868`) | Preserve. D1 integrates against this (see Open Questions §3 on how); does not duplicate the reentry/SCC mechanism | The `FlowReturn`-domain-specific arms become dead code once the new solver's obligation identity supersedes them; `D2` deletes them as part of "duplicate control/completion/relation/effect paths" per `program.md:243` |
   221	| Typed gap taxonomy | `crates/verter_session/src/semantic_query.rs` (`FlowGap` enum, ~`:1547`) | Preserve/extend as the coverage floor (D1-AC-7); D1 may need additional domain-specific gap variants (e.g. covering `G10`/`AbruptCompletion` once `D6` lands) but must not weaken any existing variant's meaning | Final home decided at `D2`'s cutover — may fold into the new solver's own typed-gap vocabulary or stay as-is; not a D1 decision |
   222	| Characterization corpus | `u6_flow_shape_corpus_tests.rs`, `u6_flow_expect_tests.rs`, `flow_return_*_tests.rs`, `flow_gap_retraction_tests.rs` (~20,000 lines total) | D1 REUSES these fixtures as its own private-boundary oracle (same expected outcomes, new implementation under test) — it does not re-author a parallel fixture set | `D2` owns final consolidation: fixtures that only exercised the deleted evaluator's internals are retired; fixtures that assert observable behavior are re-targeted at the sole solver |
   223	
   224	## Legacy deletions
   225	
   226	**D1 deletes nothing in production.** This is a deliberate, explicit statement, not an
   227	omission: `ADR-014` decision 2 and `contracts/flow-completeness.md` §6 both make the old
   228	evaluator's deletion `D2`'s ONE atomic act ("In one public cutover... delete the old
   229	syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and
   230	migration guards"). A D1 candidate that deletes or even behaviorally alters
   231	`flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` — or any of their
   232	production call sites — is out of scope and should be treated as a design violation, not
   233	merely an aggressive interpretation. A reviewer should expect **zero diff** in those
   234	three files (and in `dispatch_txn.rs`'s existing `FlowReturn`-domain arms) from a
   235	conforming D1 candidate.
   236	
   237	What D1 must still not leave behind, inside its own scope:
   238	
   239	- Any private-boundary scaffolding that is not part of the final D1 exit surface
   240	  (ordinary hygiene — draft types, abandoned domain stubs).
   241	- Any temporary shim that duplicates existing obligation/relation machinery instead of
   242	  calling it (the forbidden-outcomes list above), even if scoped
   243	  `#[cfg(any(test, feature = "test-support"))]` — the private boundary makes it
   244	  unreachable in production, but a duplicate authority inside the private tree still
   245	  violates `program.md:229`'s "contains no name-keyed or flow-private relation
   246	  authority."
   247	- Block-identifier-named evidence scaffolding, per this program's own hygiene rule
   248	  (`charters/A3.md` "In scope": "deletion or renaming of block-named evidence
   249	  scaffolding before landing").
   250	
   251	**Named for continuity — `D2`'s eventual deletion list, sourced from this research, not
   252	executed here:** `flow_return.rs`, `flow_return_callee.rs`, `flow_slice_content.rs` in
   253	full; `dispatch_txn.rs`'s `ObligationFrameDomain::FlowReturn`, `FlowReturnFrameState`,
   254	`FlowReturnPendingState`/`FlowReturnPendingOutcome`, `CompletedFlowReturnMember`,
   255	`FlowReturnDomainRuntime`; and, per `A3-SPEC.md`'s own forward note, `semantic_query.rs`'s
   256	`FlowReturnStep`/`FlowReturnFailure`/`FlowReturnResult` shapes "to the extent D1's final
   257	typestate cutover supersedes them" ("The legacy name `FlowReturnStep::Complete` is
   258	misleading for degraded payloads, but renaming or replacing that outcome type belongs to
   259	D1/D2's final typestate cutover" — `A3-SPEC.md` §1.4). D1's own charter binds none of
   260	this list as its exit obligation; it is recorded here so `D2`'s charter does not have to
   261	re-derive it from scratch, and so a D1 reviewer can confirm D1 did not quietly start
   262	executing it early.
   263	
   264	## Structural confinement
   265	
   266	Every invariant above is enforced structurally where the codebase already has the
   267	mechanism, and named honestly as a convention where it does not:
   268	
   269	- **Hermetic boundary — genuinely structural.** `#[cfg(any(test, feature =
   270	  "test-support"))]`, reusing the `test-support` feature `verter_semantic` (and
   271	  `verter_session`) already declare (`crates/verter_semantic/Cargo.toml:12-22`,
   272	  already documented: "never on the default/production closure, so release builds
   273	  compile... without" the gated code). A module gated this way does not exist in a
   274	  release compilation unit at all — not merely undocumented or unlinked by convention.
   275	  The existing Cargo-metadata dependency-firewall guard pattern (`C1.md`'s "crate
   276	  dependency firewall is the primary proof, not a marker" — `crates/verter_identity/tests/cases/workspace_dependency_layers.rs`)
   277	  extends naturally to prove no production `[dependencies]` edge enables
   278	  `test-support` transitively; this is the SAME proof shape already landed for a
   279	  different invariant, not a new mechanism.
   280	- **Private finalizer — genuinely structural, existing pattern.** A `CompleteFlowResult`-shaped
   281	  type with a private constructor, buildable only inside its own module by its own
   282	  finalizer function, is the same shape this codebase already uses elsewhere
   283	  (`MaterializeStructureDb`'s sink-private constructors, the `RequestBoundResolverContext`
   284	  sealed-marker pattern `C1.md` cites). A `trybuild` compile-fail fixture is the
   285	  existing proof idiom (`C1-AC-7`).
   286	- **Closed-enum domain registry and typed gaps — genuinely structural, WITH a caveat.**
   287	  An exhaustive `match` with no `_` arm over the domain enum turns "someone adds a
   288	  domain and forgets a call site" into `E0004` at compile time — real, and consistent
   289	  with this program's existing "EXHAUSTIVE match, no wildcard" rule
   290	  (`framework-adapters` skill: `ExecutorResolveCtx`). The caveat: this protects against
   291	  an **omitted** domain, not a domain handled **wrong** — a match arm that silently
   292	  returns a plausible-looking default instead of the correct discharge still compiles
   293	  and still passes an exhaustiveness check. That half is a test-coverage obligation
   294	  (D1-AC-1/D1-AC-7's behavioral parity corpus), not a structural one; this charter does
   295	  not claim otherwise.
   296	- **No-second-relation-authority — structural via VISIBILITY, not via "nothing calls
   297	  it."** Per this task's own falsification warning: an "exhaustive test double" or "no
   298	  current call site" claim is not proof — a contributor adding a method next year could
   299	  satisfy either trivially. The actual mechanism here is narrower and load-bearing:
   300	  `RelationDomainRuntime`/`InferenceSession`/`SessionAdmissionLedger` stay `pub(crate)`
   301	  to their OWN defining module (today `project_semantic_dispatch`; post-`C1`, whichever
   302	  module inside `verter_semantic` inherits that scope). D1's new solver module must be
   303	  placed OUTSIDE that visibility boundary, so it physically cannot name a constructor
   304	  for those types — the same enforcement `HostStoreView`'s immutability already gets
   305	  from holding no `&VerterHost`/`Arc<Mutex<_>>` field (`C1.md`'s "already structural, not
   306	  a convention" bullet). A contributor who wants to bypass this would have to WIDEN the
   307	  visibility modifier on the shared type — a one-line diff any reviewer sees, not a
   308	  silent addition.
   309	- **Graph-edge-native value resolution — NOT fully structural; named as such.** Nothing
   310	  in Rust's type system stops a contributor from adding a `&str`-keyed `HashMap` inside
   311	  the new solver next to graph-edge traversal; there is no marker type that makes
   312	  "resolves via graph edges" a compile-time property the way `NoTypeExpr` makes
   313	  "content-free" one. This is a **design/review discipline**, proven by the behavioral
   314	  parity corpus (D1-AC-1) and caught at review, not by the compiler. Calling it
   315	  structural would repeat the exact failure mode this task warned against (a trait
   316	  default method silently satisfied); this charter does not make that claim.
   317	
   318	## Boundary against predecessors and successors
   319	
   320	**Against `C1`.** D1 depends on `C1`'s converged `ProjectSemanticDispatch`/relation
   321	authority being physically resident where D1 will build against it. D1 does not
   322	re-litigate `C1`'s crate-placement ruling (`ARCH-RULING-C1-FOUR-FORKS.md`) and treats its
   323	outcome as given, whichever module boundary it lands with — see Open Questions §2 for
   324	the one genuine gap C1's own charter left unaddressed (whether `flow_return.rs` et al.
   325	physically relocate under `C1` at all).
   326	
   327	**Against `A3`.** D1 preserves `A3`'s typed-gap taxonomy (`FlowGap` and its eleven
   328	classes) and the 154-row checker-correct clean/warm preservation cohort as its own
   329	private-boundary oracle. D1 must not weaken any `A3` guarantee — in particular, D1's
   330	private tree answering a case `A3` retracted must retract it too, never "solve" it back
   331	to a guessed value without that becoming a maintainer-ratified capability decision
   332	(`flow-completeness.md` §6: "unless a separate reviewed breaking product decision
   333	changes that row").
   334	
   335	**Against `D2`.** D1 explicitly does not perform the atomic cutover, does not delete
   336	`flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs`, does not touch any
   337	product entry point, and is not independently mergeable or releasable
   338	(`contracts/flow-completeness.md` §6: "`D1` is a review checkpoint, not an independently
   339	mergeable/releasable production block").
   340	
   341	**Against `D3`-`D8`.** D1 does not implement `D3`'s full nominal-identity/unique-symbol
   342	comparability extension, `D4`'s full narrowing/structural-return algebra, `D5`'s full
   343	closure/capture join semantics, `D6`'s completion graph (debt `FR-D8`/`G10` stays open —
   344	ruling R-11), `D7`'s `this`/sequence/call-context surfaces, or `D8`'s final U6 admission
   345	proof. D1 builds `architecture.md` §9.1-9.4 for real (structural authority, derived
   346	projections, demand plan, binding/relation authority) plus exactly enough of §9.5-9.9 to
   347	cover whatever the A6 matrix ends up ratifying (today: nothing — see Open Questions §1);
   348	everything else is a typed gap those later blocks close.
   349	
   350	## Required exit
   351	
   352	Per `program.md:233`, elaborated against the research above: the minimum private
   353	replacement (a) preserves every `A6`-ratified Supported/Stable effective-flow capability
   354	row — today an empty set, so this reduces to D1-AC-7's G1-G11 parity floor until
   355	ratification — (b) safely answers its covered cases via graph-native resolution over
   356	`FunctionFlowGraph`, never a re-derived name-keyed map, (c) fails every unsupported case
   357	through a typed gap at least as precise as `A3`'s existing taxonomy, and (d) does so with
   358	zero product entry point able to reach it and zero second production authority
   359	(relation, graph, or obligation) constructed anywhere in the private tree. `D1` may
   360	receive checkpoint review approval under this exit; it does not merge or release
   361	independently of `D2`.
   362	
   363	## Review
   364	
   365	Foundational: three mandates, three independent contexts, one candidate SHA and tree
   366	(`governance.md` §2.3, mirroring `B1.md`/`C1.md`).
   367	
   368	| mandate | scope |
   369	|---|---|
   370	| conformance | charter, diff, and the private-boundary proof — specifically whether the new module is genuinely absent from every release compilation unit (D1-AC-2), and whether the G1-G11/154-row parity corpus actually passes against the NEW solver, not merely re-asserts production's existing pins |
   371	| architecture | diff plus the authority package — specifically whether value/effect resolution is genuinely graph-edge-native (not a relabelled name-keyed map), whether the obligation ledger is a genuinely distinct mechanism from `dispatch_txn.rs`'s existing `ObligationRuntime` and how the two compose (Open Questions §3), whether any derived projection smuggles a semantic fact (`ADR-001`), and whether the visibility fence against `RelationDomainRuntime`/`InferenceSession` construction actually holds |
   372	| adversarial performance/memory | diff plus `A6_META_COMPILE_40_COLD_RUST` raw output — the required result is **zero measured delta**, since nothing D1 adds can compile into a release artifact; this mandate's real job is proving that null result plus checking the private test suite's own resource use against the ~20,000-line reused corpus (no leak, no unbounded growth, no timeout risk added to the canonical gate) |
   373	
   374	## Abort/rescope
   375	
   376	Stop for: a discovered fourth production consumer of the old evaluator this research did
   377	not find (would change what "unreachable from product entry points" must guarantee);
   378	`C1` landing with a convergence map that does **not** relocate
   379	`project_semantic_dispatch`'s `flow_return*.rs`/`dispatch_txn.rs` as Open Questions §2
   380	assumes one way or the other (changes which crate D1's private module lives in, and
   381	whether D1 can meaningfully start before that is settled); the A6 capability matrix
   382	remaining entirely `VERIFY` at D1 dispatch time with no maintainer ruling on whether that
   383	blocks dispatch (Open Questions §1); `AMD-001`'s stack-window validator/CI-wiring/`D1`-`D2`
   384	transition test still undelivered when this charter's work would need D1's ledger status
   385	to advance to `PRIVATE_CHECKPOINT` (ruling R-12's explicit precondition); or a discovery
   386	that `contracts/flow-completeness.md` §4's `ObligationState` ledger and `dispatch_txn.rs`'s
   387	existing `ObligationRuntime` cannot coexist without one absorbing the other's job in a
   388	way that either duplicates the coinductive-cycle mechanism (forbidden by
   389	`program.md:229`) or overloads it with an unrelated bookkeeping concern (Open Questions
   390	§3). A discovery at this bar reopens the relevant ruling, not a quiet local substitution.
   391	
   392	## Open questions requiring maintainer/architecture ruling
   393	
   394	This charter does not guess at the following; each is a genuine fork the code and the
   395	existing normative documents leave open, and a wrong guess here has already cost this
   396	program rework on other blocks (`C1`'s four forks, `B4`'s emitter-disposition ledger).
   397	
   398	1. **Capability-matrix ratification timing (A6 lock record U-1).** The matrix is
   399	   entirely `VERIFY`; D1's own governing contract makes its per-row coverage obligation
   400	   literally vacuous until ratification, and the lock record itself calls this "a real
   401	   gap, not a pass" whose resolution point is "before the atomic flow-cutover block
   402	   begins." Is D1 dispatchable in a meaningful sense before that ratification — landing
   403	   with D1-AC-7 (G1-G11 parity) as its only real coverage floor and D1-AC-8 vacuously
   404	   satisfied — or does U-1 in fact block D1 from being `BLOCK_READY` at all until the
   405	   maintainer ratifies real Supported/Stable rows? Recorded as U-1's owner is "product/conformance
   406	   review with the maintainer"; this charter cannot self-ratify that decision.
   407	
   408	2. **Does `C1`'s relocation of "the `project_semantic_dispatch` module" include
   409	   `flow_return.rs`/`flow_return_callee.rs`/`dispatch_txn.rs`, or are they a carve-out
   410	   staying in `verter_session`?** `C1.md`'s Context §1 says "the private `resolver_store`
   411	   module, and the `pub(crate)` `project_semantic_dispatch` module — all three currently
   412	   living inside `verter_session`... physically relocate into the existing
   413	   `verter_semantic` crate," which by file-tree scope includes every file under
   414	   `project_semantic_dispatch/`, including the 8,416-line `flow_return.rs` D1/D2 exist to
   415	   replace. But `C1.md`'s convergence map and Legacy Deletions sections never mention
   416	   `flow_return.rs`, `flow_return_callee.rs`, or `dispatch_txn.rs` by name anywhere, and
   417	   `C1.md`'s own boundary section disclaims flow ownership ("C1 does not own flow
   418	   semantics (`D1`-`D8`)"). Ownership disclaimer and physical file relocation are not the
   419	   same thing — C1 could relocate the code while explicitly not touching its semantics,
   420	   the same way it relocates `execute_relate` unchanged. If flow_return.rs et al. DO move
   421	   under C1, D1's private module lives in `verter_semantic` (a compiler-tier crate)
   422	   alongside ~14,500 lines of pre-cutover production code it is explicitly forbidden to
   423	   touch; if they do NOT move (an implicit carve-out `C1.md` never states), D1's private
   424	   module more naturally lives in `verter_session` next to the code it is replacing, and
   425	   the crate D1 builds in changes. This determines which crate hosts D1's work and
   426	   should be settled — ideally inside `C1`'s own charter revision — before D1 dispatch.
   427	
   428	3. **How does D1's obligation ledger (`contracts/flow-completeness.md` §4's generic
   429	   `ObligationState`) relate to `dispatch_txn.rs`'s EXISTING `ObligationRuntime`/
   430	   `ObligationPendingLedger`/`ObligationFrameDomain::FlowReturn` coinductive SCC
   431	   substrate?** Two readings are both defensible from the text and neither is provable
   432	   from the code alone: (a) they are the SAME mechanism — D1 extends
   433	   `ObligationFrameDomain` with new flow-specific domain variants and reuses the
   434	   existing reentry stack/SCC discharge, so "integration with the shared relation
   435	   authority" (`program.md:229`) literally means "shares this struct"; or (b) they are
   436	   genuinely SEPARATE layers — D1's ledger tracks per-`FlowDemandPlan` REQUIRED-DOMAIN
   437	   discharge (a simple state machine per obligation, per `flow-completeness.md`'s own
   438	   pseudocode), nested inside or alongside one `ObligationFrame`'s existing `FlowReturn`
   439	   arm, which stays scoped to cross-engine SCC/re-entry bookkeeping only. Picking (a)
   440	   without ratification risks conflating two genuinely different concerns (cross-engine
   441	   coinductive cycles vs. within-one-plan domain completeness) into one type; picking
   442	   (b) without ratification risks building a second obligation-tracking mechanism next
   443	   to an existing one, which is exactly what `program.md:229`'s "no... flow-private
   444	   relation authority" forbids if drawn at the wrong boundary. Needs an architecture
   445	   ruling before implementation, not an implementer's local choice.
   446	
   447	4. **Which A6 capability-matrix rows actually count as "effective-flow capability
   448	   rows"?** The seed matrix's eight rows
   449	   (`evidence/A6/implementation-lock-record.md` §3) are framework/product-scoped (Vue
   450	   runtime compile, Vue IDE companion, Vue imported macro runtime projection, Svelte
   451	   native runtime compile, TypeInfo `TypeAtPosition`, TypeInfo graph export, LSP external
   452	   TypeScript provider, CSS parse/format/index/transform) — none names "effective-flow"
   453	   or `FlowReturn` at the granularity `program.md:231`'s "every effective-flow
   454	   capability row" requires. Does D1 need the maintainer to ADD flow-specific rows to
   455	   the matrix (a matrix schema change, beyond simply flipping existing rows'
   456	   `Status`) before its coverage obligation is even well-defined, or does an existing
   457	   row (e.g. "TypeInfo `TypeAtPosition`") implicitly carry flow sub-obligations that
   458	   ratification would need to state explicitly?
   459	
   460	5. **Is "stable binding slots" (`program.md:229`) the existing `SkeletonBindingId`/
   461	   `FlowNodeId` pair as-is, or a new cross-domain "slot" abstraction D1 must define on
   462	   top of them?** `D3`'s own charter text says it will "complete stable binding slots"
   463	   (`program.md:253`), which implies they are not expected to be complete even after
   464	   D1. Without a ruling on where the D1/D3 boundary sits for this specific mechanism, an
   465	   implementer risks either under-building (leaving D3 with no real foundation to
   466	   "complete") or over-building (doing D3's job early, which this charter's Boundary
   467	   section above forbids). This is the narrowest of the five open questions but the one
   468	   most likely to cause silent scope creep in either direction during implementation.
   469	
   470	6. **Are AMD-001's undelivered artifacts (ruling R-12) a hard gate on D1 STARTING
   471	   implementation, or only on D1's ledger status advancing to `PRIVATE_CHECKPOINT` once
   472	   the implementation work is already done?** The ruling's own wording
   473	   ("unconditionally before `D1` enters `PRIVATE_CHECKPOINT`") reads as the latter, which
   474	   would let implementation proceed now with the ledger-mechanics prerequisite satisfied
   475	   later, in parallel. But R-12 also generically warns "AMD-001 §3's unchanged
   476	   fail-closed refusal already prevents the unsupported `D1`/`D2` transition
   477	   meanwhile" — worth an explicit maintainer confirmation before dispatch rather than an
   478	   implementer inferring it from ruling prose under time pressure.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/D1-CHARTER-DRAFT.md | sed -n '205,265p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 7760ms:[0m
   205	release artifact's hot path; that null result is itself part of D1-AC-2's proof, not a
   206	separate performance obligation. D1 may **not** request a new locked cell for
   207	production-facing flow performance — that is `D2`'s (and later `L2`'s) obligation, once
   208	the solver is actually reachable. The one in-charter resource requirement is
   209	`contracts/flow-completeness.md`'s own "no-flow path allocates no graph/plan" and
   210	"exact parse reacquisition" properties (D1-AC-9 above), proven by the private-boundary
   211	test suite itself, not by a gated SLO cell.
   212	
   213	## Current-tree convergence map
   214	
   215	| current authority | source | disposition under D1 | disposition under D2 |
   216	|---|---|---|---|
   217	| Demand-slicing graph substrate (already graph-shaped) | `crates/verter_semantic/src/analysis/flow/{mod.rs,flow_graph.rs,peeker.rs,hashing.rs,lower.rs,flow_ir.rs,frame_span.rs,value_descent.rs}` | Preserve + extend. D1 builds its VALUE/effect resolution on top of `FunctionFlowGraph`'s existing edge kinds; adds no second graph (`ADR-001`) | Becomes the sole structural authority for production, unchanged in kind |
   218	| Syntax-shaped VALUE evaluator | `crates/verter_session/src/project_semantic_dispatch/flow_return.rs` (8,416 lines), `flow_return_callee.rs` (733 lines) | Untouched in production. D1 builds a graph-native replacement privately, alongside it, answering the same demand shape | Deleted wholesale, in the same accepted candidate that routes public operations to the new solver |
   219	| Content lowering (owned control-flow tree over the demanded slice) | `crates/verter_session/src/flow_slice_content.rs` (5,407 lines) | Untouched in production; D1's private tree builds its own graph-native content-lowering equivalent | Deleted, same D2 candidate |
   220	| Obligation/coinductive-SCC substrate | `crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs` (`ObligationRuntime` `:879`, `ObligationReentryStack` `:396`, `ObligationFrameDomain::FlowReturn` `:318-325`, `FlowReturnFrameState` `:298`, `FlowReturnDomainRuntime` `:1868`) | Preserve. D1 integrates against this (see Open Questions §3 on how); does not duplicate the reentry/SCC mechanism | The `FlowReturn`-domain-specific arms become dead code once the new solver's obligation identity supersedes them; `D2` deletes them as part of "duplicate control/completion/relation/effect paths" per `program.md:243` |
   221	| Typed gap taxonomy | `crates/verter_session/src/semantic_query.rs` (`FlowGap` enum, ~`:1547`) | Preserve/extend as the coverage floor (D1-AC-7); D1 may need additional domain-specific gap variants (e.g. covering `G10`/`AbruptCompletion` once `D6` lands) but must not weaken any existing variant's meaning | Final home decided at `D2`'s cutover — may fold into the new solver's own typed-gap vocabulary or stay as-is; not a D1 decision |
   222	| Characterization corpus | `u6_flow_shape_corpus_tests.rs`, `u6_flow_expect_tests.rs`, `flow_return_*_tests.rs`, `flow_gap_retraction_tests.rs` (~20,000 lines total) | D1 REUSES these fixtures as its own private-boundary oracle (same expected outcomes, new implementation under test) — it does not re-author a parallel fixture set | `D2` owns final consolidation: fixtures that only exercised the deleted evaluator's internals are retired; fixtures that assert observable behavior are re-targeted at the sole solver |
   223	
   224	## Legacy deletions
   225	
   226	**D1 deletes nothing in production.** This is a deliberate, explicit statement, not an
   227	omission: `ADR-014` decision 2 and `contracts/flow-completeness.md` §6 both make the old
   228	evaluator's deletion `D2`'s ONE atomic act ("In one public cutover... delete the old
   229	syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and
   230	migration guards"). A D1 candidate that deletes or even behaviorally alters
   231	`flow_return.rs`/`flow_return_callee.rs`/`flow_slice_content.rs` — or any of their
   232	production call sites — is out of scope and should be treated as a design violation, not
   233	merely an aggressive interpretation. A reviewer should expect **zero diff** in those
   234	three files (and in `dispatch_txn.rs`'s existing `FlowReturn`-domain arms) from a
   235	conforming D1 candidate.
   236	
   237	What D1 must still not leave behind, inside its own scope:
   238	
   239	- Any private-boundary scaffolding that is not part of the final D1 exit surface
   240	  (ordinary hygiene — draft types, abandoned domain stubs).
   241	- Any temporary shim that duplicates existing obligation/relation machinery instead of
   242	  calling it (the forbidden-outcomes list above), even if scoped
   243	  `#[cfg(any(test, feature = "test-support"))]` — the private boundary makes it
   244	  unreachable in production, but a duplicate authority inside the private tree still
   245	  violates `program.md:229`'s "contains no name-keyed or flow-private relation
   246	  authority."
   247	- Block-identifier-named evidence scaffolding, per this program's own hygiene rule
   248	  (`charters/A3.md` "In scope": "deletion or renaming of block-named evidence
   249	  scaffolding before landing").
   250	
   251	**Named for continuity — `D2`'s eventual deletion list, sourced from this research, not
   252	executed here:** `flow_return.rs`, `flow_return_callee.rs`, `flow_slice_content.rs` in
   253	full; `dispatch_txn.rs`'s `ObligationFrameDomain::FlowReturn`, `FlowReturnFrameState`,
   254	`FlowReturnPendingState`/`FlowReturnPendingOutcome`, `CompletedFlowReturnMember`,
   255	`FlowReturnDomainRuntime`; and, per `A3-SPEC.md`'s own forward note, `semantic_query.rs`'s
   256	`FlowReturnStep`/`FlowReturnFailure`/`FlowReturnResult` shapes "to the extent D1's final
   257	typestate cutover supersedes them" ("The legacy name `FlowReturnStep::Complete` is
   258	misleading for degraded payloads, but renaming or replacing that outcome type belongs to
   259	D1/D2's final typestate cutover" — `A3-SPEC.md` §1.4). D1's own charter binds none of
   260	this list as its exit obligation; it is recorded here so `D2`'s charter does not have to
   261	re-derive it from scratch, and so a D1 reviewer can confirm D1 did not quietly start
   262	executing it early.
   263	
   264	## Structural confinement
   265	

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "wc -l CLAUDE.md .claude/skills/architecture/SKILL.md && sed -n '1,260p' CLAUDE.md && sed -n '261,520p' CLAUDE.md && sed -n '1,260p' .claude/skills/architecture/SKILL.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 11441ms:[0m
     659 CLAUDE.md
     230 .claude/skills/architecture/SKILL.md
     889 total
# Verter

> **SUPERSEDED WHERE IT CONFLICTS — an architecture program is in flight.**
>
> The ratified Revision 11 architecture program lives at [`docs/arch/refactor/rev11/`](docs/arch/refactor/rev11/); its normative entry point is [`ORCHESTRATOR.md`](docs/arch/refactor/rev11/ORCHESTRATOR.md).
>
> **Precedence:** where this file and the Revision 11 plan disagree, **the plan wins**. The plan was designed to replace parts of this document, and the maintainer has ratified that precedence — see ruling R-3 in [`evidence/maintainer-rulings.md`](docs/arch/refactor/rev11/evidence/maintainer-rulings.md), which quotes both sides of the known conflicts.
>
> **How to read this file while the program runs:** treat it as an accurate description of how the code behaves **today**, and as authority for day-to-day operational matters — build and test commands, commit conventions, testing requirements, review discipline. Do **not** treat its architecture sections as authority over the program's target design; several describe structures the plan intends to change or remove. A rule here is not grounds to refuse a change the plan mandates.
>
> **If you are implementing a program block:** the plan's charters, contracts and ownership tables bind you. Follow them to the letter. Where implementing the plan appears impossible, record a deviation for maintainer ratification rather than substituting a local decision — an unrecorded deviation is far more expensive to unwind than a delay.
>
> This banner is removed when the program completes and the target architecture is described here directly.

Verter = a Vue compiler + Language Server Protocol (LSP) implementation. Converts Vue Single File Components (SFCs) to valid TSX (TypeScript type-checks them) and compiles templates to optimized render functions. Unlike Volar, Verter generates real valid TSX, not virtual files.

Hybrid Rust + TypeScript monorepo: Rust crates own carrier parsing, runtime and IDE code generation, the shared semantic session, and the LSP server (`verter_lsp` binary, stdio); TypeScript packages provide editor integration, TypeScript-provider adapters, protocol bindings, and bundler orchestration.

## Architecture

Detailed module reference, key files, and implementation specifics live in domain skills: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`, `/architecture`.

### Shared Optimized Codebase (CRITICAL)

Verter is one shared optimized codebase, not separate semantic implementations per consumer.

- Improvements land in the lowest reusable owner crate that can correctly serve all consumers.
- `verter_session` + shared workspace/VFS integration are the authority for host-backed loading, invalidation, dependency tracking, cache reuse.
- `verter_semantic` + `verter_compiler` own reusable semantics, lowering, codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack + type-resolution orchestration.
- `verter_audit` is the leaf observability substrate (depends only on `verter_span`, no back-edge; lower crates emit through `current_observer()` (TLS) without knowing whether a `HostAuditRuntime` is installed); the concrete host runtime lives in `verter_session` — full ownership inventory in `/audit-infrastructure`.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` remains the thin native/WASM adapter layer.
- Consumer packages (`@verter/component-meta`, LSP, MCP, unplugin, playground) consume the shared substrate, not their own semantic forks.

Architectural consequence:

- A perf/correctness fix found in one surface is implemented in the shared owner layer whenever the behavior is reusable.
- Consumer-local wrappers stay thin and do not bypass shared parsing, analysis, resolution, or cache ownership.

**Exactly one type-resolution engine.** `SemanticQueryKey` → `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`) — the SOLE query-time type resolver. OXC is the syntax/lowering front-end ONLY (declaration bodies lower to `TypeExpr` lazily on first semantic demand through the scheduler-retained parse snapshot — the `DeclBodyMemo` owned by `IndexedReady`); OXC must never resolve types at query time. Any second query-time resolution path — a parallel `resolve_type` engine, a per-surface walker, a re-parse-and-resolve, an OXC element/frontier resolver — is a rule violation: delete it, route through the shared resolver. Two engines diverge; divergence is the bug/hang class.

**Audit infrastructure:** Rust-first deterministic per-request observability for every audited `VerterHost` entry-point (component-meta, type-resolution, compile, analyze, workspace ops, LSP handlers, MCP tools, bundler batches). TS bindings in `packages/types/audit.generated.ts`; opt-in via `HostConfig::audit_enabled + footprint_capture`. See `/audit-infrastructure` and [`docs/audit-footprint/`](docs/audit-footprint/).

Guards: `verter_audit_no_upward_deps`, `audit_substrate_isolation`, `audit_observer_single_accessor`. The former single-engine `no_new_*` shrinking-ledger scanners retired with the deleted parser-owned `type_surface` engine; the single-engine invariant is now held structurally (the compiler consumes only the dependency-neutral macro DTO) plus the Vue macro boundary suite registered under "Vue Macro Semantic Boundary" in `CRITICAL_RULE_GUARDS`.

### Build Philosophy (CRITICAL)

Same end-state philosophy as `binary-exploring-lamport.md`. Core rules:

1. Read, parse, shallow-process, cache each canonical file once per content hash through one shared host path.
2. Store the full shallow symbol inventory up front, then process only requested items on demand.
3. Same-file closure stays local to the owning file.
4. Cross-file deepening happens in one place only, one import level at a time.
5. The builder/solver reads only from cached lookup state; it does not reopen file loading or routing.
6. The design is demand-driven and query-scoped.
7. The final implementation lands as one clean cutover, not a merged dual-path transition.
8. Component-meta, LSP, MCP, and other host-backed consumers share the same file-ready/read/parse/shallow-process lifecycle.

These are architecture rules, not optimization hints. On conflict, fix the owner layer or delete the legacy path rather than preserve a second read/parse/resolution flow.

Guards: `no_thread_local_oxc_caches`, `no_direct_oxc_parser_calls_outside_scheduler_path`, `recursion_budget_invariant_across_module_boundary`.

### Shallow File Processing Core Invariant (CRITICAL)

The shallow file process is a core architectural invariant and must be preserved. When a canonical file is processed, the host stores its shallow symbol inventory once; that inventory is the authoritative index later stages query.

Shallow state must classify and retain at minimum: imports; exports and reexports; type declarations; interfaces; enums; classes; variables/constants; functions/method signatures; `typeof`-relevant value declarations; local symbol dependency edges; cross-file dependency edges.

Design rule: processing a file means collecting and indexing its symbols, not eagerly evaluating them; later stages look up the indexed items they need and process only those on demand; no stage rescans the raw file to rediscover symbols shallow processing already captured. Performance: very high performance comes from targeted demand after broad shallow indexing, not repeated partial reparsing.

Core invariants (full architectural-target detail: `/type-resolution` → IndexedReady Target Contract + Cache Population Target Contract):

- Canonical post-parse artifact = `IndexedReady`: a shallow declaration INDEX plus body locators, NOT a body store. Eagerly it carries canonical imports/exports, top-level symbol names/kinds, declaration spans, source-order contributor grouping, type-parameter names, syntactic member headers, and augmentation inventory — all safe for host-owned `Send + Sync` caches. Declaration BODIES lower only on first semantic demand through the shared lazy body service (the content-addressed `DeclBodyMemo` + scheduler-side `DeclLoweringService` retained-parse workers); publishing an artifact lowers ZERO declaration bodies. Component-meta and later analysis layers both build from it; symbol expansion populates and reuses the same shared resolver caches — no separate expansion paths.
- Parse each live file version once; the lazy lowering service RETAINS the parse snapshot on its worker shard (keyed `(canonical, whole_hash, parse_env_hash)`) so body demands reuse it instead of re-parsing per touch. Transient OXC parse arenas stay per-file/per-version and never leak into host-owned shared caches — jobs borrow the retained AST on the worker and return owned typed IR.
- The declaration-body **hot READ path is handle-native at the one migrated graph-backed site** (`lower_decl_body_to_node`) through `decl_body_hot_ref`; prepared declarations and `LoweredValueDecl` are fact+locator `NoTypeExpr`, and `LoweredTypeDecl.body` is content-free. The two former stored type-parameter `TypeExpr` pockets are CLOSED (the type-parameter-bound confinement block): `LoweredTypeDecl` is wholly `NoTypeExpr` — the stored full `type_parameters: Vec<TypeParam>` is gone; the `narrow_type_parameters` mirror (name + ordinal + content-free bound locators) is the sole stored authority, consumed by the prepared-decl builder and the external frontier (which content-free re-anchors the mirror's bound slots to the frontier symbol, preserving `export default`), while the locator/binder deref uses the mirror for ordinal/name/bound-presence authority and re-borrows bound CONTENT + the full sibling frame lease-only from the retained snapshot (`transient_type_parts`). `TypeParamBinding` is likewise shrunk to the content-free `(name, ordinal)` fact pair (`NoTypeExpr`): its `<script setup generic="…">` bounds are re-borrowed at query time through ONE artifact-local transient producer over the pinned `IndexedReady` and lowered by ONE dispatch helper shared by both content readers, with a missing/stale re-borrow failing as a typed cache-suppressed miss, never a bound-free fabricated binder. The residual-reader ledger is not a terminal-zero mechanism: `ProducerLowering` is permanent transient ingress, the five `GraphFreeDto` rows remain named until the separate producer-boundary-confinement cutover, and a hot consumer must never take `HotTypeRef → TypeExpr → semantic decision`.
- Navigation stays narrower than expansion: walking `A['c']['full']['bar']` navigates intermediate hops and expands only the terminal requested projection unless limited normalization is required to continue.
- Generic substitutions are semantic meaning: navigation/expansion operate on instantiated types; cache keys include the relevant substitutions/type arguments.
- Navigators stay non-owning (choose the next hop, non-owning normalization only); reusable semantic work enters through the shared query API, not a private drill-down path. The shared semantic layer is keyed by semantic query identity and stores immutable semantic data or ids — never borrowed AST pointers or retained parser arenas.
- Completion fence: top-level live-host results record touched dependency facts, revalidate before publish, retry at most 3 times on mid-flight changes; never warm shared caches with torn provisional results; cancelled, superseded, interrupted, budget-exceeded, or partial results are never promoted warm.
- Waiters on in-flight work block cooperatively, never busy-spin; same-path recursion never self-awaits.
- Cache population is path-independent (same result from different entry points → same shared entry); broader successful results may backfill only the narrower entries they actually satisfied; narrower results must not pretend broader work is cached.
- Final payload caches hand out immutable `Arc` values; any backend preserving concurrency, size bounds, validation semantics is fine.

Guards: `audit_publishes_member_edge_with_published_field_provenance_at_macro_boundaries`, `macro_impacting_constructs_fail_lowering_not_silent_skip`, `indexed_ready_publish_lowers_zero_decl_bodies`, `resolve_unrelated_symbol_lowers_only_demanded_decl`, `lazy_decl_body_singleflight_lowers_once`, `no_indexed_ready_eval_env_or_type_decl_body_storage`, `emit_parse_facts_never_hashes_decl_bodies`.

**Project-global cache (final state):** `VerterHost` owns a single `ProjectTypeStore` accessed via `.project_type_store()` — the sole shared cache graph: `FileArtifactStore`, `AnalysisReadyDb`, the rehomed `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `MaterializeStructureDb`, `SemanticGraphStore` (which also owns the Vue macro resolution artifacts — the former `ResolvedNamedTypesDb`), `ShapeCacheDb`, `FlowSliceStores` (the flow-return substrate's per-function graph/slice artifact stores), and the `IntrinsicRegistry`. `IndexedReady` is the single canonical post-parse artifact (the former `ModuleFactsDb` is retired). Validated cache writes record a `ReadSetSignature.facts` fact signature (the path-precise fact-tracer observation set) — the sole cache-validity rail, revalidated against the live `StoreView` on every warm hit. The `StoreViewValidationToken` is the complete reuse/validity oracle; the singleflight LANE identity is the narrower `external_supersession_fingerprint` (reuse-oracle = full token; lane-identity = external fingerprint). See `/host-session` (store-view token dimensions, token-advance rules, lane identity, singleflight, `RequestStoreView`/`CanonicalCompletionOverlay`, handle-backed dims), `/component-meta` (`get_component_meta` final-result flow, `resolve_owner_direct_import`, `materialize_component_meta_structure`, the `ShapeCacheDb` per-member route, `reduce_published_field_types` / sink-private `reduce_field_value_node`), `/type-cache-architecture` (admission, retired split stores), and `/type-resolution` (`execute_cooperative` dedup, `SemanticNodeData::VueMacroElements` hot path, `IntrinsicRegistry::lookup`).

### Canonical Dependency Cache Rule (CRITICAL)

Host-backed type/import resolution treats the canonical file ID as the cache identity. Load and parse each dependency at most once per canonical ID per workspace content generation. Cache the parsed state, the shallow declaration index plus lazy declaration-body memo, symbol/export tables, and prepared declarations together. Later lookups hit cached maps — never rewalk the AST. VFS is the authority for file-change invalidation. Concurrent cold requests to the same file collapse onto one materialization path. Changes land as one clean cutover, no dual-path shims.

Guards: `host_upsert_performs_no_reverse_dependent_eviction`, `host_upsert_reverse_dep_eviction_scanner_discriminates`, `import_route_writer_guard`.

See `/type-resolution` skill for the full rule set (invalidation semantics, route caches, prepared declarations, cross-owner reuse, negative caching, the concrete performance contract).

### Cache Architecture (CRITICAL)

The fact-based cache architecture splits cache keys across five orthogonal env-hash dimensions (`parse_env_hash`, `resolve_env_hash`, `type_env_hash`, `lib_env_hash`, `project_identity`). Each cache layer keys only on the dimensions it actually depends on (R21 scoping rule — a single bundled `project_config_hash` is forbidden). `lib_env_hash` enters a key only when the value depends on lib data: `ResolvedImportFacts` does NOT include it; `RouteDb`, typed-IR resolve, `MaterializeStructureDb`, `SemanticGraphStore`, `ComponentMetaResultDb` DO.

Two cache families: **content-addressed artifact caches** (`FileArtifactStore`, `ResolvedImportFacts`, typed-IR resolve, `MemberSemanticFactStore`, `MemberDisplayFactStore`, `ModuleAugmentationIndex`) carry `content_hash` or `parse_stable_hash` in the key; **query-identity caches** (`RouteDb`, `MaterializeStructureDb`, `SemanticGraphStore` query nodes, `ComponentMetaResultDb`) exclude version hashes from the key — concurrent variants coexist as candidates in one slot, with version rooting on the cached value (the structural + semantic-graph caches — `MaterializeStructureDb`, `SemanticGraphStore` memo, `ShapeCacheDb` — root via `ReadSetSignature.facts` + `self_root_canonicals`; `RouteDb` via its value-side `ValidatedFactCache` fact signature; `ComponentMetaResultDb` via the owner whole-hash candidate discriminant + `ReadSetSignature.facts`). Cache keys never include `fact_dep_signature`. The `MaterializeStructureDb` subject is the content-free `MaterializationCacheKey` (a `ResolvedDeclSlotIdentity` slot + projection/policy/mode axes + `resolve_env_hash`), NOT a graph-instance `SemanticNodeId` — the per-thread recursion identity `MaterializeRuntimeKey` is a separate, non-cache key; a root-less anonymous subject keys no slot (uncached). The materialization cycle gate is the sealed `SemanticQueryKey::ClassifyMaterializationCycleGate` family (`MaterializationCycleGateKey`: `ResolvedDeclSlotIdentity` root slot + `parse_env_hash` + `resolve_env_hash`, fixed `StructuralTransit`/`Skeleton`/empty-args axes) — the SOLE authority for "does this declaration transitively reach a cycle through a complex helper surface"; only its `Decided` outcome admits (`LegacyFallback` always suppresses), and consumers branch on the carried `Stop | Continue` verdict from both arms.

Family-memo slots (`SemanticQueryKey::Instantiate.base` / `ResolveMacroPayload.owner`, mirrored on `FamilyKey`) are the env-bearing, content-free `ResolvedDeclSlotIdentity` (R6 — content/version hashes and the versioned `DeclIdentity` are forbidden in any derived-`Hash` query-identity key; the live whole-hash is re-sourced at value-compute time, never carried in the key). A warm hit requires TWO independent gates (§3.4): `cached_satisfies` over a RECORDED materialised `(path, point)` the candidate's compute actually produced — never the candidate's nominal slot/mode, never enum rank — AND per-candidate `ReadSetSignature.validate_with_self_roots` against the caller's live view. Backfill clones only recorded materialised points, directionally gated (the `Shallow → Navigate` clone is lattice-unsound). `validated_at_generation` is recency metadata only, never a validity oracle. Family-slot retention is bounded PER FAMILY (`U3.ADAPTIVE_FAMILY_RETENTION`): each `FamilyKey` declares an exhaustive `candidate_cap()` (floor 4; the inference/substitution-heavy live families `Instantiate` / `TypeOf` / `Conditional` / `MappedType` hold 8); at the cap a publish evicts a candidate INVALID against the publishing caller's stable store view FIRST (snapshot/validate/reacquire OUTSIDE the `entries` mutex, `admission_seq` identity recheck under it), then the least-recently validated-hit candidate (a valid hit promotes to the back); a new cacheable candidate is ALWAYS admitted after local eviction, and same-discriminant re-publish replaces in place as freshest. The process-wide candidate-memory ceiling + typed non-admission are deferred full-`U3.CACHE_FACT_MODEL` work, NOT live. See `/type-cache-architecture` for the full key/context composition (`InstantiateContext`/`MacroPayloadContext` per-key contexts, per-family `candidate_cap()` candidate semantics, non-file-base rooting).

`FileArtifactStore` is the authoritative per-file storage layer, keyed by `(canonical, content_hash, parse_env_hash, parse_key, build_toolchain_fingerprint, file_language_id)` — `parse_key` is the stable exact source-bytes/language/compatibility-domain/syntax-profile identity (there is no separate parser-version counter dimension), and `file_language_id` is the file's `FileLanguage` row (the per-file classification dimension of artifact identity, so a framework-capability flip misses exactly the affected files' artifact slots without touching the global `parse_env_hash`). The overlay-aware `augmentation_index` (module-augmentation inverse lookup) lives on the same store. See `/type-cache-architecture` for the full key composition, `file_language_id` producer wiring, `AugmentationTargetKey`/`AugmentationPopulation` semantics, and the `parse_stable_hash` definition.

Cache runtime hard rules — three always in force: cache correctness is read-side authoritative; `ReturnOnly` (overflow, budget exhaustion, cancellation, generation supersession, incomplete self-rooting, unresolved provenance) never publishes entries, reverse-index metadata, or persistent artifacts; overlay/session results never populate base-only or persistent caches. Full 20-rule list: `/type-cache-architecture` → Cache Runtime Hard Rules.

Guards: `cache_satisfaction_is_materialized_point_not_nominal_demand`, `cache_satisfaction_requires_path_exact_not_prefix`, `backfill_writes_only_recorded_materialized_points`, `no_off_store_host_caches`, the `r6_*` cluster, plus the four migrated-query-identity-key guards in `tests/cases/g_cache/r6_r21_query_identity_keys.rs` (`component_meta_result_key_*`, `route_name_key_*`/`barrel_surface_key_*`, `materialization_cycle_gate_key_*`, `materialization_cache_key_*`) — full list in `CRITICAL_RULE_GUARDS`.

See `/type-cache-architecture` skill for the full rule set (R1–R31, two-fact `MemberPresence`/`Member` model, multi-candidate substrate, signature-overflow contract, module augmentation completeness, heuristic-cache-semantics prevention, exact policy identity) and `docs/arch/fact-based-cache.md` for the per-field audit table + per-cache-layer key composition.

### Macro Type Traversal Rule (CRITICAL)

When resolving cross-file macro types (`defineProps<T>()`, `defineEmits<T>()`, component-meta deep expansion, etc.), only follow the import graph reachable from the requested type's declaration graph. There is one shared cross-file type resolver with five query modes: `Identity`, `Navigate`, `Shallow`, `Expanded`, `Skeleton` (see `/type-resolution` → Query Mode Contract).

**Macro resolution is one shared path, not a per-macro engine.** Every macro (`defineProps` / `defineEmits` / `defineOptions` / `defineSlots` / `withDefaults`) and every imported `.vue` component surface resolves through exactly TWO steps: (1) resolve ONE type via the shared typed-IR five-mode dispatch — the generic-parameter type (`define*<T>()`) OR the object-argument type (`define*({ ... })`); `withDefaults` resolves the props payload type plus the defaults-object type and merges; `.vue`-component imports resolve the synthesized `$props` / `$emit` / `$slots` / expose surface recursively through the same dispatch (the hardest case — apply EXTRA caution: it is exactly where rule violations cause the worst hangs); no macro-specific engine, no per-surface walker, no eager element resolver. (2) Normalise per kind — a thin transform, NOT a resolver (per-kind field rules: `/type-resolution` → Macro Type Traversal Rule). A macro/import that resolves through anything else, or flattens a full surface eagerly before the consumer demands it, is a rule violation — collapse it into `shared_resolve(type) + normalise`.

`Skeleton` is the BFS / generic-helper traversal mode: unbound type parameters stay `TypeParam` shells so Conditional branches do not collapse to `never`. Path projection is path-precise: intermediate hops run in `Navigate`, the terminal hop runs in the caller's mode; non-contributing intersection arms are ignored (not rewritten to `never`); open conditionals distribute the remaining path into both branches; closed conditionals reduce immediately. Do not walk unrelated imports. Do not treat plain imports as implicit exports. Cache discovered symbol mappings and barrel hops.

**TS-first resolution priority:** TypeScript types always take priority over JavaScript files. Use `effective_target()`: `.d.ts` > `.d.cts` > `.d.mts` > `.ts` > `.tsx` > `.js` > `.jsx` > `.cjs` > `.mjs`.

**Owned resolution is bounded by `workspace_root`:** `node_modules` and package `#imports` ancestor walks stop at `IdeProjectConfig.workspace_root`.

Guards: `root_conditional_still_distributes`, `no_macro_string_heuristics_in_resolver_core`, `no_text_based_macro_surface_projection_helpers`, `no_role_inference_from_name_suffix`, `no_pick_or_omit_string_prefix_check` — full list in `CRITICAL_RULE_GUARDS` (the retired `no_new_*` shrinking-ledger scanners left with the deleted `type_surface` engine; the compile-boundary half of the invariant is guarded under "Vue Macro Semantic Boundary").

See `/type-resolution` skill for the full traversal rules and resolver mode details.

### Declaration Merging (CRITICAL)

Same-name declaration merge is produced ONLY by `verter_semantic::type_eval` ordered declaration groups: `EvalEnv` appends contributors in source/binder order (`add_type`/`add_value` push onto an ordered `TypeDeclGroup`/`ValueDeclGroup` — no last-wins `FxHashMap<String, TypeDeclInfo>`/`…ValueDeclInfo>` map, no overwrite `insert` for mergeable kinds). Same-name `interface` declarations lower to the explicit `TypeDeclBody::Merged` carrier (on the memo-owned `LoweredTypeDecl.body` read through `ShallowFileState::type_decl(name)` → `PreparedTypeDecl.merged_contributors`), interned as a distinct `SemanticNodeData::MergedDecl { contributors }` node.

A merged declaration MUST reach the project-semantic reducer as that distinct carrier — a bare `TypeExpr::Intersection` / `SemanticNodeData::Intersection` is FORBIDDEN as the merged-decl representation, because the intersection reducer applies **heritage-shadow** member precedence and cannot accumulate method overload groups. The `MergedDecl` peer-merge reducer (`reduce_merged_decl_with_graph` + `merge_declaration_surfaces`): (a) same-name methods/call-signatures ACCUMULATE into one ordered overload group across contributors in source order; (b) conflicting non-method properties take deterministic first-contributor precedence (never `never`); (c) distinct members union.

Functions accumulate into an ordered `Vec<FunctionSignature>` (`ValueDeclGroup::merged_signatures`), each carrying `has_implementation_body`; overload visibility is a PROJECTION-time rule (`build_typeof`): a lone signature is visible (even if bodied), a multi-signature group surfaces every bodiless overload in source order and hides the trailing implementation. Same-file merged values version-root on the owner's single `FileWholeHash` self-root under a content-free query-identity key (R6). `verter_session` MUST NOT synthesise the merge as `raw_body = TypeExpr::intersection(...)`. Cross-file ambient augmentation (`declare module`/`declare global`) reuses this same `MergedDecl` peer-merge path — see Declaration Augmentation (CRITICAL).

Guards: `eval_env_type_symbols_are_grouped_not_last_wins_map`, `eval_env_add_decl_appends_not_overwrites`, `no_intersection_merge_synthesis_in_verter_session`, `merged_decl_lowers_to_distinct_carrier_not_intersection`, `declaration_merge_facts`.

See `/type-resolution` skill for the carrier chain, the peer-merge reducer, and the architecture guards.

### Declaration Augmentation (CRITICAL)

Ambient declaration augmentation (`declare module "X" { ... }` / `declare global { ... }`) is a RETAINED, addressable scoped inventory — never fingerprint-only facts, never file-scope pollution. `EvalEnv.augmentation_scopes` / `EvalEnv.augmentation_value_scopes` key `(AugmentationScopeKind {Global, Module(specifier)}, name)` → ordered `TypeDeclGroup`/`ValueDeclGroup`, mirrored on `ShallowFileState`; inner decls NEVER enter file-scope `type_symbols`/`value_symbols`. Parse-domain `ModuleAugmentationFact`s are DERIVED from this typed inventory (`fact_emission::collect_augmentations`) — NO raw-source byte-scan.

Cross-file augmentation merge is the SAME `MergedDecl` peer-merge path as same-file merging — NOT a second merge engine: `stitch_module_augmentations` finds every augmenter via `FileArtifactStore::ensure_augmentation_index_populated`, lowers each augmenter's RETAINED inner body in its own file context, and folds base ∪ augmenter contributions into ONE `SemanticNodeData::MergedDecl` carrier; augmenter order is the stable `(canonical, parse_stable_hash)` key — discovery-order-independent.

Facts rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` fingerprint plus one `FileWholeHash` per contributing file and records `self_root_canonicals = {base} ∪ {augmenters}` — a content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`. Query keys stay content-free (R6). The index is OVERLAY-AWARE (`AugmentationPopulation {Base, Session(overlay-set fingerprint)}`): overlay augmenters NEVER poison the base index and NEVER cross sessions, and there is NO base-only session assert on the augmentation-index surface — a session view is accepted under `Session` scope.

Guards: `session_overlay_augmenter_isolated_from_base_index`, `session_overlay_augmentation_isolated_from_base_meta`.

See `/type-resolution` skill for the stitch chain and the overlay-aware index, and `/type-cache-architecture` for the content-addressed vs query-identity augmentation key split.

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has two separate template codegen paths; modifying one does NOT affect the other: **VDOM/Vapor** (`template/code_gen/vdom/`) for runtime render functions, and **IDE** (`ide/template/`) for valid JSX/TSX used by LSP/TSGO type checking. The LSP uses the IDE path via `CompileTarget::IDE`.

Guards: `compile_audit_sourcemap`.

See `/compiler-codegen` skill for full codegen pipeline, backends, and CompileTarget details.

### Carrier IDE TS Surface Principle

North star for the IDE/LSP experience: for every carrier with an IDE projection (`.vue`, `.svelte`), the script block (`<script>`, `<script setup>`, Svelte module/instance scripts) AND the supported template/markup expressions are **ONE** generated TypeScript/JavaScript/JSX surface — interpolations (`{{ }}`, Svelte `{expr}`), directive/attribute expression values (`v-if`/`v-for`/`v-bind`/`:`/`v-on`/`@`/`v-model`/`v-slot` and dynamic args; Svelte `bind:`/`on:`/`class:`/`style:`/`use:`, `{#if}`/`{#each}`/`{#await}`/snippets, `{@render}`/`{@html}`/`{@const}`, rune calls) all lower into it. That surface is obtained through the IDE path (`CompileTarget::IDE`/`TSX`), synced to the active TypeProvider, with provider positions/ranges/edits mapped back through the document's `ProviderPositionMapper`.

**The bar:** for any supported mapped TS/JS expression position, every provider-backed IDE feature — diagnostics, hover, definition/type-definition, references, rename, completion/resolve, signature help, document highlights, semantic tokens, inlay hints, and generic code actions whose edits map exactly — should behave like the equivalent standalone `.ts`/`.js`/`.jsx` program, with results mapped back to the carrier source. A binding represented in BOTH script and template is discoverable and renamable from either side (rename spans script + template; find-all-references finds both). This holds for **both Vue and Svelte** over the shared LSP path.

Fail-closed boundary: unmapped synthetic helper code, framework tokens with no TS correlate, unsupported/experimental projection regions, and provider edits whose full ranges cannot be mapped must fail closed or return framework-native results — never mis-mapped. Source actions (organize-imports, fix-all, formatting) require explicit per-action support and tests; they are NOT implied by this principle. This is a **principle, not yet a `(CRITICAL)` guarded rule** — it is promoted to CRITICAL once real-provider cross-region Vue/Svelte regression tests guard it.

See `/compiler-codegen` → "Carrier IDE TS Surface Principle" for the full normative text (every covered expression form), and `/host-session` / `/position-encoding` for provider sync and position/range/edit mapping.

### Compiled-Output Conformance (CRITICAL)

Official-framework compiler conformance is behavioral plus structural/helper-topology parity, not raw-byte identity. For Vue VDOM/Vapor, Svelte `svelte/internal/*`, SSR/client, and future runtime backends, compare emitted output by observable behavior plus parsed/token-normalized structure: imports, helper families, helper call sequence where order is semantic, memoization/reactivity/effect topology, DOM/hydration template topology, class/style/attribute normalization, prop/property routing, event delegation, and diagnostic/reject ordering.

Cosmetic JS carrier formatting is not a finding: indentation, line breaks, non-semantic comments, intra-expression whitespace outside literals, and behavior-preserving redundant parentheses may differ from the official compiler. Directive, pragma, license/preserve, source-map/sourceURL, TS-directive, JSDoc, and other tool-consumed or framework-significant comments remain in contract. Generated local identifier spellings are waived only when the backend oracle implements scope-aware alpha-equivalence for private, non-observable bindings; otherwise identifiers are structural. Literal payload bytes, static HTML/CSS/SSR strings, public/exported or source-authored names, sourcemap mappings, diagnostic text/codes/order, and any framework-defined observable format remain in contract.

Do not build or route production compiled-output emission through JS printers, re-printers, redundant-paren canonicalizers, or any machinery whose role includes mimicking the official compiler's cosmetic JS carrier formatting. Direct-emission helpers may emit syntax-required tokens, including required parentheses for valid JavaScript expression/statement shape, but they must be scoped to semantic/syntactic correctness and covered by behavioral/structural tests rather than official cosmetic byte parity. Emit correct code directly and make conformance oracles structural for cosmetic categories: a cosmetic-only diff passes; a behavioral or structural divergence fails.

The positive structural-discriminator guard currently covers Svelte client only (Vue VDOM/Vapor and SSR/client positive oracles are tracked follow-ups); the re-printer guard is cross-backend negative coverage. See `/compiler-codegen` for the tracked guard gap.

Guards: `svelte_structural_conformance_discriminates_cosmetic_from_behavioral_diffs`, `no_compiled_output_cosmetic_reprinter_path`.

### Fallthrough / Root Inheritance (CRITICAL)

The shared Rust pipeline owns all fallthrough and root inheritance semantics. `verter_semantic::analysis` extracts root reachability facts only. `verter_session` owns the single inheritance resolver, recursion, conditional branch composition, generic propagation, caching, and final metadata projection.

Key rules: `inheritAttrs: false` → no inherited surface. Single native root → intrinsic attrs minus declared props/events. Single component root → recursive propagation. Conditional branches → exact union. Cycles → unresolved branches. `class`/`style` are never consumed.

Guards: `fallthrough_recomputes_from_runtime_subnodes_after_top_level_node_clear`, `fallthrough_runtime_reuse_survives_host_cache_clear`, `fallthrough_reuses_root_follow_after_branch_union_node_clear`.

See `/component-meta` skill for the full semantic rules, public contract, authority chain, and key files.

### Component-Meta Shallow-By-Default Rule (CRITICAL)

Types and properties are ALWAYS published shallow at the projector surface UNLESS the consumer explicitly walks the path. This is the single architectural invariant the projector pipeline (`meta_resolve::projectors::reduce_published_field_types` + sink-private `reduce_field_value_node`) enforces.

Concrete contract:

- Plain alias references (`type Foo = ...`) — published prop type stays `TypeExpr::Ref { name: "Foo" }`. Consumers re-resolve `Foo` through the registry on demand. The projector does NOT eagerly inline the alias body.
- `Pick<Foo, "bar">` — materialises ONLY the `bar` member of Foo. Other Foo properties stay shallow (path-precise). Built-in utility types (`Pick`, `Omit`, `Required`, `Partial`) behave identically to a userland implementation referencing the same keys.
- **Carrier-preserving decl-body lowering.** Under `Shallow` (as under `Navigate` / `Skeleton`), decl-body lowering interns `DeclRef` / `InstantiationRef` carriers for member-value type references — including ALL builtin utilities — and never executes `ResolveDecl` / `Instantiate` eagerly; eager lowering-time execution is `Expanded` / `Identity` only; materialisation enters exclusively through the demand points (PathWalker hops, the shallow-surface synthesiser's carrier unwrap, closed object-filter surface reads, the relation/conditional oracle). Eager Shallow member-value lowering was the `Table.vue` storm: 94.3% of all budget charges were `Instantiate(StructuralTransit:Shallow)` recursion across the transitive TanStack decl graph.
- **Open key domain ⇒ shallow carrier (L1) — route/mode-independent.** TWO families stay shallow carriers at EVERY entrance, in every mode, and open-OR-UNKNOWN (including traversal-budget exhaustion) preserves the carrier instead of falling through into Expanded materialisation: (1) an object-filter utility (`Pick`/`Omit`) whose enumeration domain is OPEN or undecidable (`Pick<PropsBase<T>, …>` over the SFC's open `generic="T"` stays `Pick<…>`); (2) a mapped type `{ [K in S]: V }` whose produced surface still depends on an unbound OUTER generic (a CLOSED-key/open-VALUE mapped enumerates its keys path-precisely with shallow values). Closed sources still materialise the requested keys path-precisely. A carrier-stopped `Pick` at a SURFACE-enumeration demand (heritage arm / macro props-slots surface) still publishes its CLOSED output-key selection from the source's enumerable arms via the shallow walker's `Pick`-carrier enumeration — the source is never whole-materialised and `Omit` (source-dependent-open output keys) stays a carrier; zero-member surface collapse was the nuxt-ui ContentSearch/DropdownMenuContent bug. Typed-IR only, no string matching. The carrier-stop is the PRIMARY defense for the open-generic class; the per-request projection budget (`request_budget.rs`) is an ARMED-by-default runaway fuse (`projection_op_budget == 0` ⇒ effective cap 2000) whose trip returns `BudgetExceeded` as a genuine partial — refused warm admission, the no-poison invariant. Publication demand is `Navigate`-only on the projector/registry macro surfaces: a full `get_component_meta` records ZERO `Published(Expanded)` projection contexts; `Table.vue` and `ChatMessages.vue` are COMPLETE corpus members with un-ignored green trackers (`table_resolves_complete_and_warm`, `chat_messages_resolves_complete_without_false_partial`, `chat_messages_resolves_without_timeout`). The FULL authoritative spec — entrances, owner predicates, the per-argument position-sensitive key-domain rule, the tri-state conditional oracle, per-utility output-key semantics, mapped family composition, OPEN/CLOSED definitions, memoization, invalidation, the `TypeOf` demand rails, and the four named current scoped exceptions — lives in `/type-resolution` → Open-Key-Domain Carrier-Stop (L1).
- `Omit<Foo, "bar">` — keeps `bar` shallow (excluded from the surface) and materialises the others.
- `Foo['a']['b']` — path-precise: only the `a` and `b` hops load; other Foo keys never enter the published surface.
- True recursive types (`type Self = Pick<Self>`) — NOT supported. The published surface stays the bare `Ref { name: "Self" }`.
- Imported alias names (workspace-owned OR package-backed) — stay shallow regardless of where they live.

The projector pipeline is the sole post-projection authority — no eager per-field materialisation runs at publication time.

Guards: `decl_body_lowering_keeps_member_value_refs_as_carriers`, `publication_routes_never_demand_expanded`, `chatmessages_resolvable_barrel_publishes_open_pick_as_shallow_carrier`, `closed_pick_sources_still_materialize_path_precisely`, `projection_budget_counts_instantiate_and_conditional`, `cycle_guard_roots_at_utility_source_type_argument` — full list in `CRITICAL_RULE_GUARDS`.

See `/component-meta` skill for the publication-surface rules and the locked-down negative tests in `crates/verter_session/src/meta_tests.rs`, and `/type-resolution` for the authoritative L1 spec.

### Component-Meta Native Vs Compat (CRITICAL)

The native component-meta payload is the semantic authority. `@verter/component-meta/compat` is a projection layer for `vue-component-meta` interoperability, not a second semantic pipeline.

Core rules: Fix metadata in the native layer first. Rust owns resolution, declaration routing, graph construction. One async native request per query. JS may transform structure but must not recover meaning. JS must not become a second resolver or expander. Cache-owned type recovery only — no AST/source fallbacks.

Guards: `no_napi_direct_verter_compiler_emitters`, `compat_one_napi_call_audit`.

See `/component-meta` skill for the full policy, resolver rules, and cache contracts.

### Typed-IR-Only Resolver Rule (CRITICAL)

The native component-meta / typeinfo type resolver — analyzer → projector → registry → policy → materialiser — drives semantic decisions exclusively from the typed IR (`verter_semantic::analysis::type_expr::TypeExpr` on Rust, `TypeDescriptor` from `@verter/type-ir` on TS). Forbidden inside that pipeline:

- Source slicing, regex against type text, hand-rolled type-text splitters (`split_top_level_*`, `find_top_level_char`, `extract_pick_slot_bindings`, `extract_string_literal_name`, `splitTopLevelTypeOperator`), `starts_with("Pick<")` shape sniffing, and the synthesise-then-reparse pattern (`format!(...).parse_type_annotation(...)`). Walk the typed IR instead.
- `parse_type_annotation` anywhere except JSDoc tag-type payloads — the single explicit text exception: `{Type}` payloads inside JSDoc tags are inherently text, parsed via the dedicated JSDoc path only.
- Parsing back raw / display strings (`Analyzed*Field.type_annotation`, `ExpandedField.raw_type`, `ResolvedLocalType.expanded`, `PropMeta.rawType`) — display-only passthroughs. The JS compat layer (`@verter/component-meta/compat`) reads `prop.type` (`TypeDescriptor`) for every semantic decision; `prop.rawType` must not feed any `looksLike*`, `extract*`, `normalize*`, `split*`, `strip*`, `prefer*`, `shouldPrefer*`, or `repairOpaque*` branch.
- Substring path classification (`"/node_modules/"`, `"\\node_modules\\"`) — use `ResolverContext::workspace_is_package_backed(canonical_id)`. That predicate is the single structural authority for workspace-ownership classification, and it is what the live decision sites call directly (`component_meta_materialize.rs`, `framework/script_facts.rs`, `host_manage/jsdoc_resolve.rs`, `meta_resolve/graph_predicates.rs`, `meta_resolve/materialize/field_types.rs`, `meta_resolve/projectors/output_sink.rs`, `project_semantic_dispatch/raise.rs`/`walk.rs`, and others). Workspace-owned is its complement — there is no separate `workspace_is_workspace_owned` predicate.
- Name-suffix role inference (`name.ends_with("Props")` / `"Emits"` / `"Events"` / `"Model"` / `"Slots"`). Type-role classification is structural, not nominal: a type is a prop/emit/model/slot type because a Vue SFC macro (`defineProps`, `defineEmits`, `defineModel`, `defineSlots`, `withDefaults`) consumes it — read from `AnalyzedMacro.kind` / `parsed_type_argument` / `type_references` on the analyzer snapshot.

OXC is a syntax/lowering front-end only and never resolves types at query time. Macro/JSDoc producer fields still lower at their producer boundary via `lower_ts_type(ts_type, source)` (the producers emit content-free locators — `Analyzed*Field` carries `payload: Option<MacroPayloadLocator>`, `ResolvedLocalType` carries `shape: ResolvedLocalShape`; only display-only text rides alongside, surviving all caches); top-level declaration bodies lower LAZILY through the scheduler-retained parse snapshot (`DeclBodyMemo` → `DeclLoweringService`) and return owned typed IR before dispatch/reducers ever see them — no raw-string reparsing, no OXC resolver path. For the hot read surface the `decl_body_hot_ref` accessor mints a `HotTypeRef` handle over the `Instantiate` query result (`build_instantiate`'s post-processed node, produced via the resolving-lowerer body-shape helper `lower_decl_body_with_provenance`); the handle is NOT a re-lowering — bodies still lower to typed IR, and `DeclBodyMemo` records are fact+locator content-free end to end (the former memoized `LoweredTypeDecl.type_parameters: Vec<TypeParam>` pocket is CLOSED — the narrow mirror is the stored authority; bound content re-borrows lease-only). If a new requirement appears to need text manipulation inside the resolver, fix the producer (lower the right OXC node, store the right typed field, extend `@verter/type-ir` with a missing variant) rather than reparsing or pattern-matching on text.

Guards: `no_macro_string_heuristics_in_resolver_core`, `no_format_then_reparse`, `no_role_inference_from_name_suffix`, `no_node_modules_substring_outside_workspace_api`, `no_pick_or_omit_string_prefix_check`, `lazy_decl_lowering_uses_scheduler_snapshot_not_reparse`, plus the rest of the typed-IR guard cluster — full list in `CRITICAL_RULE_GUARDS`.

See `/component-meta` and `/type-resolution` skills for the typed schema contract, the producer-side lowering points, and the architecture-guard list.

### CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.) — never string replacements, regex transforms, or manual splicing on the output of `build_string()` or content produced by a `CodeTransform`. `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten); modifying the string after the transform desyncs byte offsets → LSP position mismatches (hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** `ct.prepend_left(pos, ".ts")` — chunk list and source map stay consistent. **Wrong:** `content.replace(".vue'", ".vue.ts'")` on the built string — the source map still reflects pre-replace byte offsets.

Guards: `compile_audit_sourcemap`.

### Typeinfo Wire Contract (CRITICAL)

The typeinfo graph wire surface (`crates/verter_protocol/proto/verter/v1/typeinfo.proto`, its generated Rust and TS bindings, and the audit envelope on top) is a closed contract. Four invariants:

1. **Closed-enum discipline.** `GraphTypeNode.kind`, `StructuredTypeExpression.kind`, `TypeInfoGraphRequest.payload`, `TypeInfoRequestError.kind` are closed `oneof` taxonomies. Adding a variant bumps `SemanticTypeGraph.schema_version`; removing one requires `reserved` directives at the enclosing message scope (proto3 forbids `reserved` inside an `oneof` block).
2. **Wire-compat: field numbers never reused.** A retired variant's tag goes into the message's `reserved` list with its name (off-tree clients keep round-tripping the slot as an unknown field); new variants take the next free tag, never a recycled one.
3. **Audit envelope additions are purely additive.** Every new typeinfo audit field (`structured_event`, `kind_payload`, `RequestKind::TypeInfoGraph`) lands as a new arm or a default-zero field, never a replacement.
4. **Request validation runs before semantic execution.** `validate_type_info_graph_request` rejects malformed envelopes through a typed `TypeInfoRequestError`; the schema-version gate is closed-set (`SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS`); per-variant structured-expression validation is exhaustive over the `oneof` taxonomy.

Guards: `typeinfo_graph_taxonomy` (`crates/verter_session/tests/cases/g_block/typeinfo_graph_taxonomy.rs` — proto/TS oneof parity), `typeinfo_proto_ts_freshness` (`crates/verter_protocol/tests/cases/typeinfo_proto_ts_freshness.rs::typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output` — regenerates the TS bindings via the workspace `buf` and `oxfmt` binaries and byte-compares), `request_kind_payload_parity` (`crates/verter_audit/tests/cases/request_kind_payload_parity.rs`), `typeinfo_request_validation` (`crates/verter_session/tests/cases/g_type/typeinfo_request_validation.rs` — closed-set schema-version + exhaustive structured-expression coverage), `typeinfo_wire_surface_guards`, `typeinfo_graph_contract_guards`, `typeinfo_request_contract_guards`, `typeinfo_audit_contract_guards`.

### Cross-Platform Portability (CRITICAL)

The codebase MUST build, test, and materialize on macOS, Windows, AND Linux. Platform-assuming code is a defect, not a nit.

Guard-enforced — `tracked_paths_are_portable` (`crates/verter_session/tests/cases/tracked_paths_are_portable.rs`) enumerates `git ls-files -z` and enforces: valid UTF-8; no NTFS-illegal characters (`< > : " | ? * \` plus control chars); no trailing dot or space; no reserved device basenames (`CON`/`PRN`/`AUX`/`NUL`/`COM1`–`COM9`/`LPT1`–`LPT9`, with or without extension, plus `CONIN$`/`CONOUT$` — the `$`-suffixed forms only); no case-insensitive path collisions (lowercase-fold approximation of NTFS/APFS folding, not the exact filesystem fold tables); ≤200-byte relative paths.

Review-enforced (the guard does not cover these):

- Sanitize generated on-disk names (e.g. `blake3:<hash>` → `blake3-<hash>`) — logical identifiers are unconstrained; only the on-disk boundary is. The guard only sees tracked paths, so it catches a generated name once committed, not at generation time.
- Build paths with `Path`/`PathBuf`/`Path::join` — never string concatenation with hardcoded `/` or `\`.
- Byte-equality comparisons over checked-out text normalize line endings (CRLF ↔ LF) or compare as text — never raw bytes embedding EOL.
- OS-specific binaries (`tsgo`, `.exe` suffixes) are discovered platform-aware, never via a hardcoded per-OS name.
- Temp and cwd paths come from std abstractions, not literal paths.

Guards: `tracked_paths_are_portable`.

### Anti-Binary-Growth Integration-Test Layout (CRITICAL)

Each crate exposes AT MOST one `tests/main.rs` integration-test binary; extra cases live under `tests/cases/` and are wired through `main.rs`. A second top-level `tests/*.rs` auto-becomes its own test binary and re-balloons the gate, so it is forbidden unless EXACTLY allowlisted. The only sanctioned exceptions are genuine "needs a separate test process" cases (process-global state that must be isolated): `verter_session` `allocator_canaries` (a counting `#[global_allocator]`) and `verter_lsp` `lsp_audit_trace_out_env_var` (a process-global env mutation). The allowlist (`scripts/integration-test-layout-allowlist.json`) is the single source of truth shared by both guards, is EXACT (package + target + repo-relative `src_path`, no globs/prefixes), and is STALE-FAILING — an allowlisted target that no longer exists in `cargo metadata` (or whose `src_path` moved) FAILS the guard.

Dual guard: the fast-fail CI Node check `scripts/check-integration-test-layout.mjs` (runs before the Rust gate) and the in-gate Rust mirror (`crates/verter_session/tests/cases/integration_test_layout_guard.rs`), both reading the same allowlist.

Guards: `integration_test_layout_is_consolidated`, `layout_checker_discriminates_stray_and_stale`, `allowlist_is_the_known_standalone_targets`.

### Framework Adapter Substrate (CRITICAL)

Multi-framework component support is ONE shared adapter substrate, not a per-framework semantic fork. `verter_session::framework` owns the `FrameworkAdapterRegistry` (built once at `VerterHost` construction), the per-adapter `FrameworkAdapterDescriptor` (identity, supported surface kinds, carrier language, the `VirtualFileNaming` column), the facts/carrier-only `FrameworkAdapterCtx`, the `ComponentDefaultSynth` seam, and the two-pass script-fact seam. Vue is the REFERENCE adapter — re-housed as a true plan/normalize adapter (`VueFrameworkAdapter` + the relocated `vue_exec` resolution delegates), NOT a privileged hardcoded path.

Closed-contract rules:

- **One audited wire entry, validation-first.** `VerterHost::resolve_framework_surface_with_audit(TypeInfoGraphRequest)` is the SOLE entry for the `GRAPH_OPERATION_FRAMEWORK_SURFACES` operation. It runs `validate_type_info_graph_request` FIRST (op/payload-arm match, schema echo, the nested framework-surface validator) — a malformed envelope returns the typed wire `error` arm BEFORE any registry lookup or semantic dispatch. A bare-inner-request entry is forbidden. The operation rides the EXISTING typeinfo graph envelope, and its current `FrameworkSurfacePayload`/embedded-`SemanticTypeGraph` shape is PROVISIONAL — an interim wire pinned today, NOT a permanent "no schema change" guarantee. The hard gate `S5.B11/B12 → U8` was landed ahead of order, so U8 still OWES the retag of `FrameworkSurfacePayload.graph` to a `TypeInfoGraphPayload` carrier, the `SemanticTypeGraph.schema_version` bump, and reserving the old field per the Typeinfo Wire Contract (CRITICAL) above; until U8 lands this wire stays pinned but is not final. Guard `framework_surface_wire_executor_validates_first`.
- **Registry dispatch, no privileged framework branch.** The executor interns `selector.framework_adapter_id`, looks up the registry (unknown id ⇒ typed `MalformedPayload`, NO new error variant), and dispatches to the adapter. Every wire `FrameworkTag` maps to a registered adapter OR an explicit `TagDisposition` row (`DeferredVertical` / `OutOfScope`); a tag's existence is NOT a support guarantee — support is asserted only by a registered adapter and surfaced per-request via `FrameworkSurfaceKindStatus`. Guard `framework_registry_complete` (+ the `framework_surface_executor` integration suite).
- **Closed plan/resolve/result vocabulary.** The adapter PLANS demands (`plan_surfaces` ⇒ closed 4-variant `PlannedDemand` — `MacroPayload` / `PathProjection` / `ShallowSurface` plus the Svelte arm `SvelteSurface`; no `Custom`/`Raw` arm, no source text / OXC handles / raw `SemanticQueryKey`s) and NORMALIZES resolved data (`normalize`); it holds NO resolve entry point. The executor resolves each `PlannedDemand` through the module-private `ExecutorResolveCtx` (EXHAUSTIVE match, no wildcard) THROUGH the one shared type-resolution engine — it plans, dispatches, and encodes; it is never a second resolver. Per-kind status maps DIRECTLY onto `SUPPORTED`/`PARTIAL`/`UNSUPPORTED` via the typed `ResolvedOutcome` (a supported-empty kind stays distinct from an unsupported kind). The first `SemanticTypeGraph` encoder (`graph_export`) is a pure ZERO-DISPATCH shallow projection of resolved data — named refs mint `GraphSymbolNode` + `GraphReference{symbol_id}`, structural unencodables degrade to `GraphOpaque`, never a fabricated ref and never a re-resolution.
- **Facts/carrier-only adapter ctx.** `FrameworkAdapterCtx` exposes EXACTLY two ops — `carrier_for::<T>` (the adapter's typed parse carrier, `None` for a carrier-less adapter — never a forged token) and `script_facts_for::<T>` (resolved script-fact evidence on demand: exact, partial, unavailable, or not applicable). It never resolves types, indexes a file, runs OXC, calls `ProjectSemanticDispatch`, or reads a `StoreView`. Guard `framework_adapter_ctx_closed_surface`.
- **Two-pass script-fact seam.** The syntax-capture half (`verter_semantic::analysis::framework_facts`) captures candidates from the live OXC program — SYNTAX-ONLY (may touch OXC + `lower_ts_type`, MUST NOT resolve imports or read capability bits; guard `script_fact_capture_is_syntax_only`). The session mints an exact candidate inventory, including exact-empty, only when its capture parse completes without syntax diagnostics; a recovered parse yields positive-only candidate observations, returns partial evidence, and never warms either script-fact store. Cache absence means “not computed.” The resolved-validation half (`framework/script_facts`) drives provider `validate` on demand over neutral resolved-import + capability data and preserves exact (including exact-empty), partial, unavailable, and not-applicable evidence. Resolved-fact publication accepts only producer-minted exact evidence under the fact-rail + strict-same-generation gate with `SignatureAdmission::Cacheable`-only publication (overflow ⇒ `ReturnOnly`, no warm); partial/unavailable results remain cold. Partial payloads have no whole-payload accessor: Svelte consumers can visit explicitly named conservative positive observations, while a resolution-only partial can separately expose its producer-proven exact syntax facet. Svelte facts keep syntax-owned facts (including sealed `ExactSveltePropsCalls`) separate from resolution-owned `Snippet`/dispatcher provenance, so resolution failure cannot erase exact script geometry. An EMPTY active-provider set is byte-identical zero-cost (Vue does NOT move onto the seam). The `ActiveProviderIndex` is the shared gate authority. Guard `script_fact_providers_zero_cost_on_miss`. The framework-surface result caches (`FrameworkSurfaceStore` / `FrameworkScriptCaches`) are fact-validated today but live on the framework registry rows, NOT the single `ProjectTypeStore` — they are PROVISIONAL off-store caches to be consolidated onto `ProjectTypeStore` (and given true singleflight) at U10.
- **Parse-domain component-default synth.** `ComponentDefaultSynth` synthesises a component's default-export value symbol from PARSE-DOMAIN inputs only (macros + syntax-capture candidates); it never names the resolved-validation fact types. Registry-dispatched at the shallow-analysis injection points by the file's resolved language. Guard `component_default_synth_parse_domain_only`.
- **Generated virtual-file naming is descriptor-owned.** The `VirtualFileNaming` column is the single authority for an adapter's IDE / API / testing-API / sidecar suffixes; the committed TS mirror (`packages/language-shared/src/virtual-file-naming.generated.ts`) is rendered from it and byte-pinned. Guard `virtual_file_naming_ts_freshness`.
- **No re-export shim for relocated Vue resolution.** The Vue resolution bodies relocated to `framework_surface::vue_exec`; `typeinfo/adapters/vue/{public_type,surface,store}.rs` are DELETED with no re-export shim or alias under `adapters::vue`, and `VueShallowMetadataStore` / `VueMacroDtoKey` are retired. Guards `vue_relocation_no_shim` + `retired_symbols_absent_from_production_source`.

See the `/framework-adapters` skill for the substrate's module map, the descriptor/registry/ctx/executor contracts, the script-fact seam, and Vue as the reference adapter.

### Carrier Geometry From Registered Facts (MANDATORY)

The carrier parser is the only production owner allowed to discover carrier structure from raw source. Downstream production code must not infer carrier geometry, section dialect, or block identity from text: for example, by searching for tags or delimiters, applying regex dialect heuristics, or recounting source order. Consume registered `RegisteredFileStructure`/`CarrierBlockInventory` projections and parser-minted facts, refs, or tokens; any retained compatibility ordinal is projected from that inventory, never reconstructed from text.

Local edit-time lexing may classify a bounded token or recovery window, but it must not become an alternate structure parser; any geometry-sensitive window is bounded by parser/inventory facts, including parser-unowned gaps bounded by registered parse boundaries. Tests may scan fixture text for setup, decoys, and assertions. This rule is review-enforced alongside the existing structural boundaries; do not add scanner-detection CI jobs or name-keyed source-tree guard scanners.

### Project-Bound External-TS Contract (CRITICAL)

Production external-TypeScript results for carrier sources are project-bound. The result-producing backend path is `ExternalTsProjectResolver` → `CarrierRegistry` → `EngineBackend`: `EngineBackend::ensure_project` is reached only from a resolved `ProjectBinding`, and `publish_snapshot`, `query`, and `diagnostics` require the resulting `BoundProject` witness. No production external-TS result path may infer a project from a bare path, open a carrier into a config-less/inferred project, or fall back to an inferred backend. Path-shaped transport notifications may exist below this contract, but they cannot construct external-TS results or bypass `BoundProject`.

Ownership is TypeScript-correct. A carrier source (`.vue`, `.svelte`, or any adapter extension) is owned by a configured project only through the default include, a no-extension directory/bare-star glob, or a glob/`files` entry that explicitly covers that extension. An extension-specific `*.ts` glob does not own it. TypeScript include has no brace expansion: multi-extension coverage is separate entries, never `*.{vue,svelte}`.

**Owner selection is tsgo-faithful, single-winner, and never terminally ambiguous when a configured project owns the carrier.** A carrier claimed by MULTIPLE configured projects is NOT a terminal state: `WorkspaceSnapshot::default_configured_owner_for_file` models tsgo `ProjectCollection.GetDefaultProject` + `findDefaultConfiguredProject` (`microsoft/typescript-go` `internal/project/projectcollection.go`) — start from the nearest ancestor solution (the nearest LITERAL `tsconfig.json`/`jsconfig.json`, not the nearest project root), BFS its `references` in DECLARED array order, take the FIRST project that directly includes the carrier, climb to the next ancestor solution unless `disableSolutionSearching`, and finally fall back to the lexicographically-least `tsconfig_path` among the configured claimants (tsgo `firstConfiguredProject`, a name-least ordering DISTINCT from the reference BFS order). The winner is chosen only from ordered structures (the `projects` Vec, `references` Vec, an ordered visited set) — never `HashSet` iteration; reference cycles resolve via the visited set, never a terminal ambiguity. The resolved owner flows through the SAME `binding_for` → `BoundProject` witness as the unique-owner arm. This selection is provider-neutral: the ONE `WorkspaceSnapshot` decision the tsserver, managed-tsgo, and shared-tsgo carrier routes all consume identically.

**Bounded divergence (recorded per tsgo faithfulness).** Verter's `ConfiguredMembership` is include/`files` only and carries NO `IsSourceFromProjectReference` (program-level project-reference-redirect) data, so for a carrier every include/`files` hit is treated as DIRECT and tsgo's `multipleDirectInclusions` is effectively always true — the reference BFS decides. The residual solution-graph pruning in `configured_owner_resolution_for_file` (which can pre-select a leaf over an ancestor that both includes and `references` it) is preserved on the `Unique` arm and is NOT unified with the BFS.

The remaining TERMINAL no-serve states are ONLY: `NoProject` (no configured project's include/`files` covers the carrier's extension at all) and the disk-layout carrier-path conflicts (a real user file at a generated companion path; a same-stem rune module beside the source). A multi-claimant configured overlap is NO LONGER terminal. `NotReady` stays the transient bootstrap retry state. Terminal `NoProject` / carrier-path conflicts surface a user-visible `verter(project)` warning on `did_open` AND `did_change` (wired through the debounced coordinator publish path, not only the request-only full-diagnostics path). Verter-native non-external-TS features may still answer. `SyntheticScratch` is a separate, explicitly labelled scratch lane for non-cross-file features only. It never supplies configured-project semantics, batch typecheck, cross-file results, or project-cache warming.

**Rename fail-closed for a resolved multi-claimant carrier.** A multi-claimant carrier serves every per-file feature (hover, definition, completion, references, diagnostics) from its single resolved owner, but a provider rename runs only within that one project. A symbol that ESCAPES the owner project (exported + imported by a sibling configured project) would rename partially and leave the symbol dangling in the siblings; cheap escape detection needs the cross-project rename fan-out (a future block). Until then rename FAILS CLOSED (a clear error, no `WorkspaceEdit`) for a resolved multi-claimant carrier — never a silent partial cross-project rename — while every other feature serves normally. A uniquely-owned carrier renames normally.

Generated companion names are descriptor-owned and live in the user namespace. They are collision-free against different adapter source extensions in the normal case, but not resolution-unambiguous or reserved. A real user file at the exact `{name}.vue.tsx` / `{name}.svelte.tsx` companion path, or a same-stem Svelte rune module beside a component, is a detected resolution conflict: Verter marks the source ambiguous and fails closed, never overlay-shadows a real user file and never surfaces a silently wrong edge.

This rule becomes live for a backend only when that backend's real project-bound path lands; the inferred fallback for that backend is deleted in the same change.

Guards: `provider_op_requires_resolved_project`, `carrier_ownership_extension_rules`, `carrier_never_shadows_real_user_file`, `same_stem_svelte_component_rune_fails_closed`, `no_fallback_to_inferred_anywhere`.

See the `/host-session` skill for the contract's three-layer structure (`ProjectResolver`/`CarrierRegistry`/`EngineBackend`), the `BoundProject` witness type-state, and the carrier-publish path.

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → lsp → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:lsp            # Build Rust LSP binary (debug)
pnpm run build:lsp:release    # Build Rust LSP binary (release, optimized)
pnpm run build:mcp            # Build MCP server binary (debug)
pnpm run build:mcp:release    # Build MCP server binary (release, optimized)
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then LSP binary (shares compiled Rust deps with native, avoids recompilation), then WASM (needed by playground), then all TS packages.

See `/build-and-profiling` skill for build dependency chains, rebuild sequences, and profiling setup.

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm clean                    # Remove build artifacts
```

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust — CANONICAL agent gate
node scripts/gate.mjs                         # THE Rust gate. Builds the test universe ONCE via `cargo nextest archive` (single compile, no second-command recompile), then runs surfaces 1 and 2 from the same artifacts: SURFACE 1 = nextest run (per-test process isolation), SURFACE 2 = the verter_session libtest binaries executed directly (in-process / multi-test-per-process). SURFACE 3 then builds a SECOND `--workspace` archive with `--cargo-profile no-debug-assertions` — the shipped `cfg(debug_assertions)` state (debug assertions and overflow checks OFF, dev codegen otherwise) — and RUNS `package(verter_session) + package(verter_scheduler)` from it. That second archive is a second whole-workspace compile (a different profile is a different unit hash, so nothing is shared with the dev archive); it is the only surface that executes tests with `debug_assertions` off, and it is NOT an optimised build. Before the archive build it runs a freshness-tooling preflight: it ensures the workspace `buf` + `oxfmt` binaries are present (auto-running `pnpm install --frozen-lockfile` inside the mutex/timeout/stall machinery when the `node_modules/.bin` shims are missing), then VERDICT-GATES the `cases::typeinfo_proto_ts_freshness::*` byte-pin tolerance on the outcome — tooling present/installed ⇒ tolerance OFF, so a freshness failure is a HARD gate failure (exit 1), NOT PASS-WITH-TOLERATED; a deterministic install failure (e.g. frozen-lockfile mismatch) ⇒ a LOUD setup failure (exit 127), never silently tolerated (when an install is attempted — both `node_modules/.bin/{buf,oxfmt}` shims already present ⇒ the preflight returns already-present and no install runs); when pnpm is not resolvable AND `buf` is not resolvable the Rust byte-pin pair SKIPS gracefully and PASSES, so the gate reports an ORDINARY PASS (no FAIL line) — the verdict-gated tolerance flips ON there only as a LATENT safety net that would surface PASS-WITH-TOLERATED solely in the unusual case the pair produced a tolerated FAIL despite `buf` being absent. `oxfmt` absence NEVER grants tolerance — with `buf` present, a missing `oxfmt` is a LOUD setup failure (exit 127), not a degraded run. Run it with `node_modules` present (the normal path) so the byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD FAIL (a real stale-binding regression to regenerate + commit) — PASS-WITH-TOLERATED is NEVER the regression signal on a normal machine, and on a buf-less runner the pair yields an ordinary PASS via the skip, not PASS-WITH-TOLERATED. See docs/arch/gate-performance.md.

# The TWO UNDERLYING SURFACES gate.mjs runs — runnable directly (no Node, or debugging one surface in isolation):
cargo nextest run --workspace                # SURFACE 1 — every workspace test target INCLUDING the ~25 verter_session integration binaries, per-test process isolation
cargo test -p verter_session --tests         # SURFACE 2 — shared-process (in-process) surface for the verter_session integration suite
cargo test --workspace --doc                 # Rust doctests only; run when rustdoc examples changed or explicitly requested
cargo test --package verter_compiler test_name   # Specific Rust test
# NOTE: bare `cargo test --workspace --tests` SILENTLY SKIPS the verter_session integration suite (~4404 tests) because `session_metrics` feature unification drops those binaries from the workspace test set — it MUST NOT be the sole Rust gate; run `node scripts/gate.mjs` (which runs surfaces 1-2 from one archive plus the shipped-cfg SURFACE 3 from a second) or the two-surface pair above directly — note the pair covers neither the shipped-`cfg(debug_assertions)` surface nor its compile.
cargo test --package verter_compiler 2>&1 | tail -60  # Full suite with truncated output
```

**Oversize-source advisory:** `scripts/gate.mjs` scans production Rust sources and warns for each
non-exempt file above 1,500 lines as `path (N lines)`. File size is informational and never affects the
gate verdict.

**Resource ceilings — MEMORY-CEILING-MANDATORY.** Every gate/`--prepare` run is bounded by three flags,
each host-derived by default and overridable: `--build-jobs <N>` (cargo build parallelism, default
`min(4, cpuCount)`), `--test-threads <N>` (nextest/libtest thread count, gate-only, default
`min(4, cpuCount)`), `--memory-limit <SIZE>` (active child-tree RSS ceiling, e.g. `8GiB`/`12288MiB`,
default `max(512MiB, 50% of physical RAM)`). Policy: every gate run — agent or CI — goes through this
bounded wrapper with an explicit or defaulted `--memory-limit`; there is no sanctioned unbounded
invocation. A breach kills the active child tree fast and exits 123 (`ABORTED — memory ceiling`) — this
means NO gate verdict was produced. The same rule covers TIMEOUT (124), STALL (125), and any run whose
summary did not complete: an aborted or incomplete run is NOT PROVEN, never a PASS, and must never be
recorded as one.

**Build-prerequisite preflight (fail-closed, the gate's FIRST step).** Parts of the Rust suite load
artifacts cargo does not build: the real-provider suites spawn the pinned tsserver with `--globalPlugins
@verter/typescript-plugin --pluginProbeLocations packages/vue-vscode/node_modules`, a pnpm symlink to
`packages/typescript-plugin` whose `main` is `dist/index.js` — a `tsc -b` OUTPUT that `pnpm install` does
NOT produce. With the symlink present but the dist absent, tsserver loads no plugin and ~64 `*_tsserver`
tests fail with `TS2307: Cannot find module './Comp.vue'`, indistinguishable from a compiler regression.
So before the freshness preflight, before cargo, and before any test, `gate.mjs` **loads** that plugin
entry in a child process (`require()` of the probe directory, exactly what tsserver resolves) and on any
load failure FAILS CLOSED (exit 127, marker `BUILD-PREREQUISITE MISSING`) naming the probe target, the
load error, the producing packages, and the producer command. The oracle is a real load, not a list of
files to stat: the entry eagerly requires its emitted helpers and `@verter/language-shared`'s entry
re-exports a dozen emitted siblings, so a stat list mirrors the emit graph and drifts — both `index.js`
present with one helper missing passes every stat and still throws inside tsserver. The probe runs under
**tsserver's** environment, not the gate's — it strips the `CHILD_PROCESS_ENV_DENYLIST` the tsserver
launcher strips, read out of `crates/verter_type_runtime/src/tsserver/ipc.rs` so the two cannot drift,
because otherwise a `NODE_OPTIONS` preload can forge a status-0 load — and its timeout is a hard `SIGKILL`
bounded by the gate's own remaining deadline, since `spawnSync`'s default SIGTERM is trappable and a
trapping child both hangs the gate (with the single-flight mutex held) and returns a false positive.
Failure classes are typed (`reason`), so only `module-not-found` may ever be read as "never built". It
proves the closure **resolves**, not that it is **fresh**; a stale-but-loadable dist is a separate,
deliberately out-of-scope problem. It never builds the artifacts (the verdict must not depend on a mutation the gate performed) and
never skips the affected tests (with no install at all those tests SKIP and the gate goes green while
proving nothing). Produce them with `pnpm --filter @verter/language-shared --filter
@verter/typescript-plugin build` — deliberately NOT `pnpm build` and NOT `--filter
@verter/typescript-plugin...` (the trailing ellipsis pulls in `@verter/native`'s `napi build --release`).
`--prepare` is exempt; it runs no test. `(GB9)` in `scripts/gate-selftest.mjs` proves the discrimination in
six directions against the real production CLI on a synthetic miniature of the package graph.

### End-of-change Checks

Run after **every** change. Verter's crates are highly interconnected — a change in one crate frequently breaks tests in dependent crates. Always run the full workspace suite:

```bash
node scripts/gate.mjs 2>&1 | tee /tmp/test-output.txt   # CANONICAL Rust gate — runs THREE surfaces: SURFACE 1 (nextest process-isolation) + SURFACE 2 (direct in-process verter_session) from ONE dev-profile archive with zero second-compile, then SURFACE 3 from a SECOND `--workspace` archive built with `--cargo-profile no-debug-assertions` (the shipped `cfg(debug_assertions)` state), which RUNS `package(verter_session) + package(verter_scheduler)`. SURFACE 3 is the ONLY thing in the repo that executes tests with `debug_assertions` off — the class where a state mutation written inside a `debug_assert!` argument is a silent no-op in every shipped build. It is not an optimised build (dev codegen, no LTO) and it runs only that filterset. Run with `node_modules` present so the freshness-tooling preflight is a no-op and the `cases::typeinfo_proto_ts_freshness::*` byte-pin runs GENUINELY: with the tooling present a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), NOT tolerated. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS — the verdict-gated tolerance flips ON there only as a latent safety net (PASS-WITH-TOLERATED appears solely if the pair somehow emitted a tolerated FAIL despite `buf` being absent, which the skip does not). `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure); a deterministic install failure (frozen-lockfile mismatch) fails loud as setup (exit 127) when an install is attempted (both shims already present ⇒ no install runs).
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --release   # Compiles the REAL release profile (opt-level 3 + fat LTO), which the gate does not: gate SURFACE 3 uses the cheap `no-debug-assertions` profile, and surfaces 1-2 are debug. `debug_assert!` gates on `cfg!` — a RUNTIME constant — so its body still name-resolves in release: a `#[cfg(debug_assertions)]` helper called inside one is an E0425 in every release build (napi and wasm artifacts included) while compiling clean in debug. This is a CHECK — it RUNS NO TESTS, so it CANNOT observe the runtime half of the same class (a state mutation written inside a `debug_assert!` argument, which compiles fine and silently never executes in a shipped build). That half is covered only by gate SURFACE 3. Do not read a green `cargo check --workspace --release` as coverage of `debug_assert!` behaviour.
cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings   # Host clippy cannot see target-gated code. The wasm32 artifact is what the playground and `@verter/wasm` consumers run. The `wasm32-wasip1`/`wasip2` clippy jobs cover the SEPARATE lapce/zed manifests, not this one.
cargo fmt --all --check
pnpm install --frozen-lockfile   # Verify lockfile is in sync (CI uses this); also what the gate's preflight runs to make the freshness byte-pin run genuinely
```

Confirm `cargo clippy --version` reports the `rust-toolchain.toml`-pinned version before
trusting any of the three lint/check results — a clippy run on a different toolchain is not
evidence about the one CI uses.

- Corpus audit-test regenerator (run after audit-record schema or fixture changes; idempotent): `node scripts/gen-corpus-audit-tests.mjs`

For TypeScript changes, also run `pnpm test`. Do not skip workspace-wide testing even for "small" changes.

**Agent test policy:** `node scripts/gate.mjs` is the default Rust gate — it builds the test universe once and runs the two debug surfaces (`cargo nextest run --workspace` process-isolation + the in-process `verter_session` libtest binaries, the same direct surface as `cargo test -p verter_session --tests`) from the same archive with no second-command recompile, then builds a second `--workspace` archive under the `no-debug-assertions` profile and runs `package(verter_session) + package(verter_scheduler)` from it (SURFACE 3, the shipped-`cfg(debug_assertions)` surface). It runs the `verter_session` binaries under the workspace-unified `session_metrics` feature set (ON), intentionally replacing the old package-scoped default-feature (`session_metrics` OFF) rebuild rather than reproducing its feature config — that ON config is what the shipped LSP uses and what removes the second compile; no test target the old pair compiled is dropped. A contributor without Node, or debugging one surface in isolation, runs `cargo nextest run --workspace` then `cargo test -p verter_session --tests` directly. The `cases::typeinfo_proto_ts_freshness::*` buf/oxfmt byte-pin is the only tolerated failure, and its tolerance is now VERDICT-GATED on the gate's freshness-tooling preflight: the gate ensures `buf`/`oxfmt` are present (auto `pnpm install --frozen-lockfile` when the `node_modules/.bin` shims are missing) so with `node_modules` present that pair runs GENUINELY — and with the tooling present, tolerance is OFF, so a freshness failure is a HARD gate failure (exit 1, a real stale-binding regression to regenerate + commit), NOT surfaced as PASS-WITH-TOLERATED. On a buf-less runner (pnpm not resolvable AND `buf` not resolvable) the Rust byte-pin SKIPS and PASSES, so the gate reports an ordinary PASS (no FAIL line); the verdict-gated tolerance flips ON there only as a latent safety net, surfacing PASS-WITH-TOLERATED solely in the unusual case the pair emitted a tolerated FAIL despite `buf` being absent. PASS-WITH-TOLERATED is never the regression signal on a normal `node_modules`-present machine, and never the normal buf-less verdict either; `oxfmt` absence never grants tolerance (with `buf` present a missing `oxfmt` is a LOUD setup failure); a deterministic install failure (frozen-lockfile mismatch) fails loud as setup (exit 127) when an install is attempted (both shims already present ⇒ no install runs). Run the gate with `node_modules` present. Do not run bare `cargo test --workspace` (no `--tests`) by default: it pulls in doctests and example builds without improving the normal verification loop (and the silent-skip trap is stated once in Running Tests above). Run doctests (`cargo test --workspace --doc`) only when rustdoc examples changed or the user explicitly asks.

### Documentation Updates

After adding, changing, or removing features, update the **owning** documentation:

- **Domain skills** (`.claude/skills/`) — update the skill that owns the affected module or API
- **`CLAUDE.md`** — only if summaries or skill pointers change
- **`AGENTS.md`** — if skill routing or shared sources change
- **`docs/`** — API docs, guide pages, contributing guides
- **Inline doc comments** — Public API rustdoc (`///`) and JSDoc (`/** */`) on changed signatures

Skip for purely internal refactors that don't change public behavior, module paths, or APIs.

### Testing Requirements

**MANDATORY: TDD must be followed for EVERY code change. Non-negotiable.**

1. Write failing tests FIRST — verify they fail before implementing
2. Implement minimum code to pass
3. Run tests, verify green
4. Refactor while keeping tests green

Coverage: new features need tests, bug fixes need regression tests, refactors must keep existing tests passing.

**Always include negative assertions**: verify both what SHOULD and should NOT be present. Codegen tests must check removed syntax is absent. Type tests must include `@ts-expect-error` guards against `any`/`never`.

**Public-boundary acceptance**: for every changed user-visible IDE, API, or compiler outcome, each affected acceptance ID has an automated public-boundary test asserting the required result AND the relevant forbidden or fail-closed result. Provider-selection, status, unit, and architecture tests supplement but do not substitute for that boundary test. A substrate block may inherit a parent boundary test only by recording the acceptance-ID mapping and executing that test in its gate. Enforcement is judgment — reviewers assess the actual invocation path and assertions, not the filename; §1a proves discrimination; confirm reruns the mapped test.

**Architecture guards for critical rules**: every new `CRITICAL` architecture rule lands with a static architecture guard or a discriminating regression test in the same change (subject to the landed-scanner bar below — a "static guard" is never a new name-keyed file scanner); if a guard cannot be automated yet, the rule text names the planned guard/test and the gap is tracked in the owning skill/doc. The R6 meta-guard at `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs` (`every_critical_rule_in_docs_has_registered_guard`) walks `CLAUDE.md` plus every `.claude/skills/*/SKILL.md` and asserts every `(CRITICAL)` heading has a `CRITICAL_RULE_GUARDS` registry row with at least one named guard — a prose-only `(CRITICAL)` section fails the gate.

**Landed guards are structural, never name-keyed file scanners (forward-only)**: a heuristic file-scanner guard/test that keys on a specific tool, function, or identifier name (any spelled source name/path/token — type, module, import/path-segment, and string identities included; `syn`/AST-based scanning included) is a transient plan artifact — WIP-only (scratch branches, squashed out before landing), never a full-fledged landed guard. LANDED enforcement of an invariant is structural — compiler/type-system/tool-based (privacy/visibility/`E0603`, type-state, sealed traits, marker-trait derives, a real used tool or function) — never a name/text/grep scanner over the source tree. This strengthens Structural-Confinement-First (`.claude/skills/mom-cto-orchestration/reference/PROTOCOL.md` → Structural-Confinement-First → Landed-scanner bar): even a residual scanner that rule would permit (justified, recorded, supplement to a structural primary) does not land — keep it WIP, replace it structurally, or accept the residue uncovered by any landed scanner. Review/governance-enforced by design, NOT guard-enforced — a guard that detects "name-keyed scanner guards" would itself be a name-scanner. Forward-only: pre-existing landed scanners are grandfathered as a class — by temporal status (already landed at rule adoption), not by list membership — and retained as-is; the explicitly disclosed high-risk example (illustrative, not an exhaustive inventory) is the hot-materialize syntactic tripwire (`hot_materialize_syntactic_tripwire_residual_backstop` + its `HOT_TERMINAL_SINKS`/`HOT_DECIDE_TAINTED_GATE_IDENTS`/`HOT_EXTRACTING_GATE_IDENTS`/`HOT_MAT_DIRECT_IDENTS` name-lists in `crates/verter_session/tests/cases/output_projector_residual_guards.rs`), retained as-is with no removal planned or required — its STRUCTURAL rail (the `NoTypeExpr` marker + the sealed `OutputProjector` capabilities) remains the durable primary.

**Rust test file organization**: When inline `#[cfg(test)]` exceeds ~400 lines, extract to a sibling `*_tests.rs` file.

### Verification Must Prove Execution (MANDATORY)

A required gate passes only on fresh, input-bound evidence that: every applicable required job was eligible and ran; the intended tree-derived surface was owned and independently discovered; selectors matched non-zero work; required source, build, and fixture prerequisites matched the tested tree; executed work was non-zero; unexpected prerequisite skips were zero; child deadlines were strictly below their parent killer; and a terminal summary completed. **Exit status 0 alone, a self-declared test universe, or a missing required-job result is FAIL.** Every tracked test or guard has exactly one declared primary gate; a hand-maintained filename list may not define the primary universe unless generated from independent discovery and parity-checked.

Attestation alone is insufficient — a receipt faithfully attests whatever incomplete universe the runner defines for itself. The durable design needs all three: fresh execution attestation; independently tree-derived inventory/discovery parity; and per-surface negative-control mutation through the exact canonical entry point. A single global canary cannot detect an omitted unrelated spec.

**The negative control must itself be proven to have applied.** A plant that fails to apply reports a pass: `perl`/`sed`/`grep` exit 0 on a non-match, so a mutation's exit code is never proof it landed, and a verification search hitting a PRE-EXISTING occurrence of the planted string is a false positive. Prove the mutation is present, unique, and new in the source before trusting the run; a green planted run means the plant failed until proven otherwise. A discrimination check that cannot distinguish "the plant did not apply" from "the code is correct" is not a discrimination check.

Planned guard: `gate_contract_integrity` — one registered suite exercising the canonical entry point against independent inventory plus per-surface negative controls covering missing summary, disabled or missing job, invalid timeout nesting, zero selection, stale or missing build, missing fixture or unexpected skip, omitted or unowned test, and a mutation that silently fails to apply. Until that guard, its attesting driver, and the required-job aggregator land, this rule is held only by §1a and confirm judgment.

**This rule currently fails its own test, and says so.** It ships `(MANDATORY)` — precisely the tier the R6 meta-guard (`every_critical_rule_in_docs_has_registered_guard`) does not check, because that guard scans `(CRITICAL)` headings only. A rule whose thesis is "a gate that cannot prove it ran is a failure" is therefore, today, a gate that cannot prove it ran. `(CRITICAL)` is not available as a shortcut: an unguarded `(CRITICAL)` heading FAILS the meta-guard. So the gap is named rather than hidden — the deferral, its owner (the gate-integrity block), its resolution gate (that block's landing), and the live in-tree instances are recorded in [`docs/arch/gate-integrity-ledger.md`](docs/arch/gate-integrity-ledger.md). Promotion to `(CRITICAL)` with its own `CRITICAL_RULE_GUARDS` row, in the same change that lands the guard, is an ACCEPTANCE CRITERION of that block (ledger row GI-4). It is never folded into `Stub Prevention` — a related but distinct invariant whose guards do not enforce these semantics.

### Testing-Hermeticity (MANDATORY)

Unit tests must only depend on locally-vendored fixtures. They must compile and run without any third-party repository (e.g., `nuxt-ui`, `element-plus`) checked out alongside this repository. Tests that need external corpora must be feature-gated (e.g., `#[cfg(feature = "external-corpus")]`) and excluded from the default canonical run (`node scripts/gate.mjs`, i.e. its two underlying surfaces `cargo nextest run --workspace` + `cargo test -p verter_session --tests`).

A test that references `.integration-tests/repos/<third-party>/...` from a non-gated test file is a violation. The architecture guard `external_corpus_paths_not_present_outside_gated_tests` enforces this.

### No phase archaeology in production code (MANDATORY)

Source comments must not reference plan phases (`phase 5d`, `phase 11`, `post-cutover`, `pre-Phase`), cutover stages (`d-cutover`, `cutover`), deletion history (`deleted in 5g`, `retired in`), or any project-management vocabulary. Once a plan is over, the code reads as final-state.

Durable architecture insights belong in `.claude/skills/*` or `docs/arch/`, not in source comments. Test files named after retired phases must be renamed to describe the invariant they characterize, not the phase that produced them.

The architecture guard `no_phase_archaeology_in_production_code` enforces this on `crates/*/src/**`.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, and server cleanup.

### VS Code Extension Testing (MANDATORY)

Changes to the VS Code extension or the LSP server MUST be verified with automated tests, NOT manual testing. Unit tests (Vitest) for pure logic, E2E tests (Mocha) for LSP integration features.

See `/testing` and `/e2e-vscode-testing` skills for commands, fixture design, and helpers API.

## Agent Implementation Rules

### Codebase Navigation

Use semantic code-navigation tools (Serena or equivalent MCP: symbol overviews, symbol/reference lookup, rename/refactor ops) before broad source reads. Read full source files only when symbolic context is insufficient or the file is small enough that a full read is clearly the most direct path.

### Planning

Prefer architecturally correct, long-term solutions; evaluate by correctness and durability, not implementation speed. Time constraints, implementation size, migration breadth, anticipated breaking changes, or "a lot of work" are not valid reasons to weaken the design, preserve a compromised path, or diverge from the approved plan — if the correct implementation is larger or breaking, plan for it explicitly or raise it before execution; never silently ship an architectural deviation. Do not provide time estimates unless explicitly asked, and never use estimated effort/duration/perceived time cost as a factor for doing, not doing, or partially doing planned work.

Plans must include these sections:
1. **Context** — why this change is being made
---
name: architecture
description: "Verter codebase architecture: high-level module map, TypeScript packages, plugin system, CSS analysis, MCP server, static analysis types"
---

# Verter Architecture Reference

For domain-specific detail, see: `/type-resolution`, `/type-cache-architecture`, `/component-meta`, `/compiler-codegen`, `/host-session`.

## Shared Substrate Principle

Verter is one shared optimized codebase. Consumers reuse lower-level crates instead of separate semantic pipelines.

- Put reusable parsing, analysis, type-resolution, caching, and import-following behavior in the shared owner crate.
- `verter_language` is the zero-dependency leaf routing authority: `FileLanguage`, `FrameworkAdapterId`, `LanguageId`, `CapabilityId`, and the pure static `LanguageRegistry` (`classify_static(path)` — never reads project config). Host-gated classification (static registry × `ProjectCapabilitySnapshot`) is owned by `verter_session::framework::HostLanguageClassifier`; scheduler/workspace consumers reach it only through session-implemented trait objects (`SourceLoader::classify` / `WorkspaceAccess::classify_file`). The crate is a `verter_span`-only leaf (its design allowance — spans for the parse-artifact regions; strings stay crate-interned) and keeps a crate-local id-intern table: no lower crate exposes a reusable interning facility, and the id set is bounded by registered languages. It also owns the framework-neutral parse payload: `FrameworkParseArtifact` (typed `FrameworkParseCommon` — `ScriptRegion { span, source_type, kind }` / template / style regions, external links, `LanguageDiagnostic`s — plus a PRIVATE erased `Arc<dyn CarrierParse>`), with the raw downcast confined to each adapter's own bridge module (no capability token — a foreign artifact's erased payload is a DIFFERENT concrete `CarrierParse` type, so the `Any` downcast already fails structurally for it); the session's blessed accessors are `FrameworkAdapterCtx::carrier_for::<T>` (routed through each adapter's registered-projector opener, e.g. `open_vue_carrier`) and the Vue adapter's `vue_parse()`; the concrete `VueParseCarrier` + Vue producer live in `verter_compiler::framework_common::vue_bridge`.
- `verter_session` is the shared host/session/cache boundary for host-backed consumers.
- `verter_semantic` and `verter_compiler` own reusable semantics, lowering, and codegen.
- `verter_session::resolver_core` owns the host-backed resolver stack and type-resolution orchestration. Resolver-path methods receive `ctx: &dyn ResolverContext` (sealed super-trait at `resolver_core/resolver_context.rs`) — only `VerterHost` implements it, enforced by the `no_concrete_verter_host_in_seal_scope` arch-guard.
- `verter_protocol` owns transport-facing schema DTOs; `verter_ffi` stays a thin native/WASM adapter layer.
- Consumer packages and apps stay adapter-oriented: thin wrappers, public API shaping, transport glue, UX-specific behavior.

Bug or slowdown in one surface → fix in shared substrate so other consumers benefit.

## TypeScript Packages

| Package | Purpose | Entry Point |
| ------- | ------- | ----------- |
| **`@verter/types`** | TypeScript utility types (`PatchHidden`, `ExtractHidden`, `EmitsToProps`, etc.). Has `/string` export with `$V_` prefixed types for LSP injection | `src/index.ts` |
| **`@verter/language-shared`** | Shared custom protocol types between VS Code client and Rust LSP binary | `src/index.ts` |
| **`@verter/typescript-plugin`** | TypeScript plugin resolving `.vue` imports in TS/JS files. Intercepts module resolution to return transformed TSX | `src/index.ts` |
| **`verter-vscode`** | VS Code extension. Launches Rust `verter-lsp` binary over stdio, bundles TS plugin, handles extension activation | `src/extension.ts` |
| **`@verter/unplugin`** | Universal bundler plugin (Vite, Rollup, webpack, esbuild, rspack, Rolldown, Farm). Compiles `.vue` files via `@verter/native`. Supports `preCompile` for build-start cache warming | `src/index.ts` |

## Unplugin Configuration (`packages/unplugin/`)

`@verter/unplugin` provides a `VerterPluginOptions` interface:

| Option | Type | Default | Description |
| ------ | ---- | ------- | ----------- |
| `componentId` | `(filename, source, isProd) => string` | hash-based | Custom component ID generator |
| `include` | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include |
| `preCompile` | `boolean` | `false` | Pre-compile all `.vue` files during `buildStart`. Scans project root, upserts files into host cache (including type dependencies for macros), and compiles them. When `transform()` later receives same content, host returns cached result instantly. `node_modules` excluded from scanning. |
| `crossFileOptimize` | `boolean` | `false` | Cross-file prop constness optimization. Requires `preCompile: true`. After pre-compilation, analyzes render tree to determine which props are always passed constant values, skipping dynamic tracking in compiled output. |
| `template` | `object` | — | Template compiler options (compat with `@vitejs/plugin-vue`) |

**`preCompile` architecture:** During `buildStart()`, scans project root for `.vue` files (excluding `node_modules` and dot-directories). For each file: upserts into host, resolves external `src` attributes and macro type dependencies (e.g., `import type { Props } from './types'` used in `defineProps<Props>()`), then triggers compilation. When another plugin modifies the file before `transform()`, host detects content change via internal hashing and recompiles. Third-party `.vue` files in `node_modules` compile on-demand during `transform()` — no pre-compilation overhead.

**Macro type resolution invariant:** cross-file macro type resolution must only follow imports reachable from the requested type's local declaration graph. Unrelated imports in the same file are out of scope; plain imports are not implicit re-exports.

## CSS Analysis & Selector Matching (`crates/verter_semantic/src/analysis/`)

`verter_css_syntax` is the shared lossless token/event authority for CSS, SCSS, indented Sass, Less, and Stylus. `StyleSyntaxIrSink` and `LosslessCstSink` are peers over the same parser event stream. Semantic style analysis projects only complete, static selector nodes into selectors, classes, IDs, custom properties, and at-rules; interpolation, recovery, and evaluation-dependent selectors fail closed. Each `AnalyzedCssClass` carries `selector_index` (exact class → comma-part selector join) and each `AnalyzedSelector` carries `rule_body_span` (brace- or indentation-delimited body span). Vue's planner separately consumes trusted IR for authored-dialect `v-bind()` and post-preprocess plain-CSS module hashing/scoping; Svelte consumes the IR as a trust gate for its distinct plain-CSS matcher/scoper. Svelte's carrier/CSS parser remains the compatibility owner until exact Svelte 5.56.3 error-code, offset, and read-past-close parity is proven. Style `v-bind()` usage is discovered through the same dialect-aware planner IR, then OXC-derived `expr_roots`/`roots_complete` remain the liveness facts consumed by `mark_bindings_used_in_style` and compile-input assembly.

`StyleSyntaxIr` retains positioned containment and balanced values without evaluating or compiling preprocessors. Imports, modules, plugins, guards, mixin/function arguments, and control expressions remain opaque-but-positioned.

Stylesheet parser mode is deterministic by dialect and structural tokens. CSS always uses brace grammar. Sass and Stylus use the layout-capable grammar, which also recognizes explicit braced blocks. SCSS and Less use brace grammar whenever the lexer emits any plain `LeftBrace`; only a brace-free source with an actual deeper-indented line pair uses layout grammar. Closing-brace indentation and other incidental formatting never select the parser. Selector trust folds every component descendant and functional-pseudo selector list; class/ID collection descends those lists and gates each component independently, so complete literal class components may still publish from an otherwise evaluation-dependent selector such as `&.active` or `:global(.a .#{$x})`. A textually certain `:deep`/`:global`/`:slotted` kind publishes independently of argument trust, while every class inside its argument remains subject to the same per-component gate. Ambiguous optional-syntax statements remain locally typed and diagnosed without recovering intact ancestor rules. A declaration may own a retained `StyleBlock` (for example, an indented Sass nested-property namespace); the IR sink never discards such a block.

**Module structure:**

```
style.rs              # Semantic style projection types and specificity computation
style_syntax.rs       # Five-dialect syntax-to-semantic projection
selector_match.rs     # Three-valued selector matching against template elements
template.rs           # Template element analysis, dynamic class extraction, :style CSS var extraction
```

**Key types:**

| Type | Location | Purpose |
| ---- | -------- | ------- |
| `StructuredSelector` | `style.rs` | Parsed CSS selector (compounds + combinators) |
| `CompoundSelector` | `style.rs` | Single compound: element, classes, id, attributes, pseudo-classes |
| `SelectorCombinator` | `style.rs` | Descendant / Child / NextSibling / LaterSibling |
| `MatchResult` | `selector_match.rs` | Three-valued: `Matches`, `MaybeMatches`, `NoMatch` |
| `DomQueryCallSite` | `types.rs` | DOM query call with parsed selector and spans |
| `StyleBlockAnalysis` | `style.rs` | Per-`<style>` block analysis with nested `CssAnalysis` |
| `AnalyzedCustomProperty` | `style.rs` | CSS custom property with name/value spans, var references, selector index |
| `CssVarReference` | `style.rs` | `var()` call with name, span, optional fallback (recursive) |
| `AnalyzedVarUsage` | `style.rs` | Regular CSS property using `var()` with property name and selector index |
| `CssVarManipulation` | `types.rs` | Script-side CSS variable manipulation via DOM APIs |
| `DynamicStyleVar` | `template.rs` | CSS variable set via `:style` binding in template |
| `StaticStyleVar` | `template.rs` | CSS variable set via static `style` attribute in template |
| `CssVarFlow` | `project_index.rs` | Cross-component CSS variable flow (definitions + usages + manipulations) |

**CSS Variable Analysis (three-block tracking):**

- **Style**: the balanced component-value IR projects `AnalyzedCustomProperty` (definitions with values/spans) and `AnalyzedVarUsage` (var() references). `extract_var_references()` handles nested var() fallbacks without evaluating values.
- **Template**: `extract_dynamic_style_vars()` extracts CSS vars from `:style="{ '--color': val }"`. `extract_static_style_vars()` extracts from `style="--color: red"`.
- **Script**: `try_extract_css_var_manipulation()` detects `el.style.setProperty('--x', val)`, `getPropertyValue('--x')`, `removeProperty('--x')`.
- **Cross-component**: `ProjectIndex.css_var_flow(name)` and `VerterHost.css_var_flow(name)` return `CssVarFlow` with all files defining/referencing/manipulating a variable.

**Selector matching algorithm** (`match_selector()`):

1. Match rightmost compound against target element
2. Walk left through combinators: `Child` checks `parent_index`, `Descendant` walks ancestor chain
3. Dynamic `:class` or component types → `MaybeMatches` (can't determine statically)
4. `:not()` inverts, `:is()`/`:where()` takes best match across alternatives

**Position encoding for CSS spans**: `CssAnalysis` spans (classes, IDs, selectors) are **SFC-absolute byte offsets**. `CssSource` is constructed with the style content origin, so syntax and projection spans are absolute from creation. Consumers use spans directly without adding any offset. `StyleBlockAnalysis.content_offset` is retained for documentation and slice operations.

## Analysis MCP Server (`verter_mcp`)

`verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP for AI agents. `VerterMcpServer` wraps `VerterHost` (with `AnalysisScope::LSP`), `Linter`, and `ActionEngine`. Tools auto-load via `ensure_loaded()`; template analysis triggers `ensure_template_analysis()` transparently. Cross-file tools iterate all loaded files (no `ProjectIndex` exposed from host). Scoring engine computes composite 0-100 quality scores from a11y, lint, template complexity, API surface, CSS health, and reactivity dimensions.

Both binaries (`verter-mcp`, and the LSP-decoupling twin `verter-mcp-server`) run the one shared entry body `verter_mcp::run::run`. The HTTP transport binds before its initial scan and announces the real bound port as a one-line stdout readiness record (`verter_mcp::readiness`, mirrored by `packages/vue-vscode/src/mcpServer.ts`); `--client-pid` arms the same `ClientProcessGuard` containment as the LSP. The VS Code extension spawns this standalone binary per LSP start attempt (`createMcpServerLifecycle`: awaited replacement, bounded crash respawn, provider-registration teardown on death) — see `docs/contributing/lsp-mcp-decoupling.md`.

## verter_semantic::analysis — Static Analysis Types

`verter_semantic::analysis` is the shared static-analysis surface consumed by `verter_session`, diagnostics, and tooling. Compilation crate owns lowering and codegen; `verter_session` projects compiler and workspace state into these semantic snapshots.

### AnalysisScope

Bitflags (`u32`) controlling which analysis passes run during file upsert.

**Script (bits 0-7)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `IMPORTS` | 0 | Import declarations |
| `BINDINGS` | 1 | Variable/function/class declarations |
| `REACTIVITY` | 2 | Ref/reactive/computed classification |
| `MACROS` | 3 | defineProps/Emits/Model/Slots/Expose |
| `MACRO_TYPE_DEPS` | 4 | Cross-file type references in macros |
| `VUE_API_USAGE` | 5 | Track provide/inject/lifecycle/watcher calls |
| `EXPORT_SIGNATURES` | 6 | Per-export hashes for smart invalidation |
| `FUNC_RETURNS` | 7 | Analyze function return reactivity (for composables) |

**Template (bits 8-15)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `TPL_COMPONENTS` | 8 | Component usages + prop expressions |
| `TPL_BINDINGS` | 9 | Which script bindings are used in template |
| `TPL_SLOTS` | 10 | Slot definitions + usages |
| `TPL_REFS` | 11 | Template ref attributes |
| `TPL_EVENTS` | 12 | Event handler bindings |
| `TPL_CONSTNESS` | 13 | Prop constness classification |

**Style (bits 16-19)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `STYLE_CSS` | 16 | Full CSS analysis (selectors, classes, IDs) |
| `STYLE_VBIND` | 17 | v-bind() in styles |
| `STYLE_SCOPED` | 18 | Scoped/module metadata |
| `STYLE_PSEUDOS` | 19 | :deep/:global/:slotted |

**Cross-file (bits 24-26)**

| Flag | Bit | Description |
| ---- | --- | ----------- |
| `CROSS_RENDER_TREE` | 24 | Build render tree from template analysis |
| `CROSS_PROVIDE` | 25 | Provide/inject chain validation |
| `CROSS_PROP_CONST` | 26 | Prop constness optimization |

**Presets:**

| Preset | Flags | Use Case |
| ------ | ----- | -------- |
| `BUILD` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES, STYLE_VBIND, STYLE_SCOPED | Minimal overhead for compilation + smart invalidation |
| `BUILD_OPTIMIZED` | BUILD + REACTIVITY, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_CONSTNESS, CROSS_RENDER_TREE, CROSS_PROVIDE, CROSS_PROP_CONST | Build with cross-file optimization |
| `LSP` | All flags | Full analysis for completions, hover, diagnostics |
| `LINTER` | IMPORTS, BINDINGS, REACTIVITY, MACROS, VUE_API_USAGE, TPL_COMPONENTS, TPL_BINDINGS, TPL_SLOTS, TPL_REFS, TPL_EVENTS | Script + template for lint rules |
| `ESSENTIAL` | IMPORTS, BINDINGS, MACROS, MACRO_TYPE_DEPS, EXPORT_SIGNATURES | Script-only (legacy compat) |

### ScriptAnalysisSnapshot

Primary output of `build_script_analysis()`. Produced by a single OXC parse + AST walk.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `imports` | `Vec<AnalyzedImport>` | All import declarations with source, bindings, spans |
| `bindings` | `Vec<AnalyzedBinding>` | Top-level variable/function/class declarations |
| `macros` | `Vec<AnalyzedMacro>` | Vue macro calls (defineProps, defineEmits, etc.) |
| `macro_type_deps` | `Vec<MacroTypeDep>` | Cross-file type references used by macros, tiered by structural position (`usage: MacroTypeDepUsage` — `Surface` = argument root / intersection-union arms / extends heritage / alias chains, missing ⇒ error; `Member` = top-level member annotation, missing ⇒ warning + `null` degrade). References nested deeper are never collected (runtime codegen does not need them) |
| `flags` | `AnalysisFlags` | Bitwise flags for O(1) queries |
| `exported_functions` | `Vec<AnalyzedExportedFunction>` | Non-SFC exported functions (composable analysis). Carries `name` / `is_default` / `params` / `is_async` / `composable` only. It carries NO return-type field: the declared return type's reactive-wrapper identity is a resolution decision answered at demand time from the lowered typed IR plus a package-backed route proof (`/type-resolution` → Reactive-wrapper demand), never from annotation text on this DTO |

**ReactivityKind**: None | Ref | Computed | Reactive | MaybeRef | Mutable

### TemplateAnalysisSnapshot

Populated after compilation by converting `RawTemplateData` from `verter_compiler`.

| Field | Type | Description |
| ----- | ---- | ----------- |
| `components` | `Vec<TemplateComponentUsage>` | Components used in template with props and slots |
| `binding_occurrences` | `Vec<TemplateBindingOccurrence>` | Script bindings referenced in template with spans |
| `defined_slots` | `Vec<DefinedSlot>` | `<slot>` elements defined in template |
| `template_refs` | `Vec<TemplateRef>` | `ref="foo"` attributes |
| `event_handlers` | `Vec<TemplateEventHandler>` | `@click`, `@input`, etc. |
| `elements` | `Vec<TemplateElement>` | Full element tree for linter traversal |
| `if_chains` | `Vec<IfChain>` | v-if/v-else-if chains for duplicate detection |
| `prop_definitions` | `Vec<AnalyzedPropDefinition>` | Props from defineProps |
| `emit_definitions` | `Vec<AnalyzedEmitDefinition>` | Emits from defineEmits |
| `comment_directives` | `Vec<CommentDirective>` | `@verter:disable`, `@verter:todo`, etc. |

### ProjectIndex

Aggregates file-level usage into project-wide indexes:

- **provide_index**: provide key → files that call `provide(key)`
- **inject_index**: inject key → files that call `inject(key)`
- **component_graph**: file → components it uses (forward edges)
- **component_reverse_index**: component name → files that use it
- **class_index**: CSS class name → files that define it
- **v_bind_css_index**: v-bind CSS expression → files that use it
- **custom_property_index**: CSS custom property → files that define it

### Data Flow

```
Vue SFC Source
    |
    v
verter_compiler::compile()
    |-- ScriptAnalysisSnapshot (from OXC parse during compilation)
    |-- RawTemplateData (spans, binding refs, component tags)
    |-- CssParsed* (v-bind spans, pseudo spans)
    |
    v
verter_session (conversion layer)
    |-- RawTemplateData --> TemplateAnalysisSnapshot
    |-- CssParsed*      --> StyleBlockAnalysis
    |-- Resolves import paths, populates resolved_canonical_id
    |-- Updates ProjectIndex with file usage
    |
    v
Consumers (LSP, build, linter) query snapshots + ProjectIndex
```

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg --files docs/arch/refactor/rev11 | sort | sed -n '1,260p'"[0m in <MACHINE_ROOT>/verter
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "pwd; rg --files docs/arch/refactor/rev11 | sort | sed -n '1,260p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 16024ms:[0m
docs/arch/refactor/rev11/OPUS-START-HERE.md
docs/arch/refactor/rev11/ORCHESTRATOR.md
docs/arch/refactor/rev11/PROVENANCE.md
docs/arch/refactor/rev11/README.md
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md
docs/arch/refactor/rev11/agents/opus-bootstrap.md
docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md
docs/arch/refactor/rev11/amendments/AMD-002-a2c-completion-predecessor.md
docs/arch/refactor/rev11/amendments/AMD-003-a2c-completion-graph-authority.md
docs/arch/refactor/rev11/amendments/AMD-004-defer-completion-to-d6.md
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md
docs/arch/refactor/rev11/architecture.md
docs/arch/refactor/rev11/baseline/9af553dd.md
docs/arch/refactor/rev11/charters/A0.md
docs/arch/refactor/rev11/charters/A1.md
docs/arch/refactor/rev11/charters/A2.md
docs/arch/refactor/rev11/charters/A2C.md
docs/arch/refactor/rev11/charters/A3.md
docs/arch/refactor/rev11/charters/A4.md
docs/arch/refactor/rev11/charters/A5.md
docs/arch/refactor/rev11/charters/A6.md
docs/arch/refactor/rev11/charters/B1.md
docs/arch/refactor/rev11/charters/B1.template.md
docs/arch/refactor/rev11/charters/B2.md
docs/arch/refactor/rev11/charters/B3.md
docs/arch/refactor/rev11/charters/B4.md
docs/arch/refactor/rev11/charters/B5.md
docs/arch/refactor/rev11/charters/B6.md
docs/arch/refactor/rev11/charters/BA0.md
docs/arch/refactor/rev11/charters/BCSS0.md
docs/arch/refactor/rev11/charters/BF1.md
docs/arch/refactor/rev11/charters/BF2.md
docs/arch/refactor/rev11/charters/BF3.md
docs/arch/refactor/rev11/charters/BRT0.md
docs/arch/refactor/rev11/charters/BS0.md
docs/arch/refactor/rev11/charters/BS1.md
docs/arch/refactor/rev11/charters/BV0.md
docs/arch/refactor/rev11/charters/BV0A.md
docs/arch/refactor/rev11/charters/BV1.md
docs/arch/refactor/rev11/charters/C1.md
docs/arch/refactor/rev11/charters/C3.md
docs/arch/refactor/rev11/charters/C4.md
docs/arch/refactor/rev11/charters/J1.template.md
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md
docs/arch/refactor/rev11/contracts/agent-orchestration.md
docs/arch/refactor/rev11/contracts/architecture-falsification.md
docs/arch/refactor/rev11/contracts/baseline-lock.md
docs/arch/refactor/rev11/contracts/capability-matrix.md
docs/arch/refactor/rev11/contracts/compile-transaction.md
docs/arch/refactor/rev11/contracts/conformance-goldens.md
docs/arch/refactor/rev11/contracts/conformance-normalizer.md
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md
docs/arch/refactor/rev11/contracts/deterministic-ordering.md
docs/arch/refactor/rev11/contracts/flow-completeness.md
docs/arch/refactor/rev11/contracts/fragment-assembly.md
docs/arch/refactor/rev11/contracts/framework-compiler-boundary.md
docs/arch/refactor/rev11/contracts/identity-encoding.md
docs/arch/refactor/rev11/contracts/input-loading.md
docs/arch/refactor/rev11/contracts/language-tools-exclusion.md
docs/arch/refactor/rev11/contracts/mapping-products.md
docs/arch/refactor/rev11/contracts/official-core-oracles.md
docs/arch/refactor/rev11/contracts/package-publication.md
docs/arch/refactor/rev11/contracts/parse-ownership.md
docs/arch/refactor/rev11/contracts/result-contract-and-flight.md
docs/arch/refactor/rev11/contracts/semantic-profile.md
docs/arch/refactor/rev11/contracts/ssr-hydration.md
docs/arch/refactor/rev11/contracts/stacked-prs.md
docs/arch/refactor/rev11/contracts/third-party-exclusion.md
docs/arch/refactor/rev11/contracts/typescript-product-conformance.md
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md
docs/arch/refactor/rev11/decisions/ADR-002-compatibility-domains.md
docs/arch/refactor/rev11/decisions/ADR-003-sealed-compile-semantic-facade.md
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md
docs/arch/refactor/rev11/decisions/ADR-005-operation-dtos-and-optional-graph-export.md
docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md
docs/arch/refactor/rev11/decisions/ADR-007-direct-core-before-managed-runtime.md
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md
docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md
docs/arch/refactor/rev11/decisions/ADR-011-staged-compile-attempt-and-input-loading.md
docs/arch/refactor/rev11/decisions/ADR-012-stable-identifiers-and-canonical-ordering.md
docs/arch/refactor/rev11/decisions/ADR-013-result-contracts-and-flight-owned-production.md
docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md
docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md
docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md
docs/arch/refactor/rev11/decisions/ADR-018-opus-adapter-and-orchestrator-state.md
docs/arch/refactor/rev11/decisions/ADR-019-reproducible-authority-package.md
docs/arch/refactor/rev11/decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md
docs/arch/refactor/rev11/evidence/A0-preflight-blocked.md
docs/arch/refactor/rev11/evidence/A0-summary.md
docs/arch/refactor/rev11/evidence/A4-summary.md
docs/arch/refactor/rev11/evidence/A4/baseline-40-components.tsv
docs/arch/refactor/rev11/evidence/A4/context-packet.md
docs/arch/refactor/rev11/evidence/A4/disabled-overhead.md
docs/arch/refactor/rev11/evidence/A5-summary.md
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md
docs/arch/refactor/rev11/evidence/A5/consumer-protocol-map.md
docs/arch/refactor/rev11/evidence/A5/context-packet.md
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md
docs/arch/refactor/rev11/evidence/A5/instrumentation-reconciliation.md
docs/arch/refactor/rev11/evidence/A5/loop5-counter-census.tsv
docs/arch/refactor/rev11/evidence/A5/open-changes.md
docs/arch/refactor/rev11/evidence/A5/option-classification.md
docs/arch/refactor/rev11/evidence/A5/option-classification.tsv
docs/arch/refactor/rev11/evidence/A5/owner-rows.md
docs/arch/refactor/rev11/evidence/A5/program-operations-policy.md
docs/arch/refactor/rev11/evidence/A6-summary.md
docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md
docs/arch/refactor/rev11/evidence/A6/B1-context-packet.md
docs/arch/refactor/rev11/evidence/A6/baseline-counters.tsv
docs/arch/refactor/rev11/evidence/A6/baseline-measurement.md
docs/arch/refactor/rev11/evidence/A6/command-proofs-native.md
docs/arch/refactor/rev11/evidence/A6/command-proofs.md
docs/arch/refactor/rev11/evidence/A6/context-packet.md
docs/arch/refactor/rev11/evidence/A6/counter-reproduction.md
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md
docs/arch/refactor/rev11/evidence/A6/landing-equivalence-A5-A6.md
docs/arch/refactor/rev11/evidence/A6/stack-window-policy.toml
docs/arch/refactor/rev11/evidence/AMD-010/maintainer-ruling.md
docs/arch/refactor/rev11/evidence/B1/command-proofs/01-gate.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/02-clippy-workspace.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/03-check-release.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/04-clippy-wasm.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/05-fmt-check.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/06-pnpm-install.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/07-pnpm-test.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/08-audit-arms.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/09-discrimination-proof.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/10-performance-cell.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/11-validate-performance-gates.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/12-real-capped-gate.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/13-flake-disposition-hover.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/14-flake-disposition-compile-fail-smoke.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/15-tracked-paths-recheck.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/16-real-gate-disposition.md
docs/arch/refactor/rev11/evidence/B1/landing-equivalence.md
docs/arch/refactor/rev11/evidence/B1/pnpm-test-disposition.md
docs/arch/refactor/rev11/evidence/B2/context-packet.md
docs/arch/refactor/rev11/evidence/B2/landing-record.md
docs/arch/refactor/rev11/evidence/B3/context-packet.md
docs/arch/refactor/rev11/evidence/B3/debt-FC-OPTIONS-002-option-attempt-decode-unwired.md
docs/arch/refactor/rev11/evidence/B3/debt-FC-SVELTE-001-svelte-output-liveness.md
docs/arch/refactor/rev11/evidence/B3/landing-record.md
docs/arch/refactor/rev11/evidence/B4/context-packet.md
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-001-generated-chunk-callsite-migration.md
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-002-vue-main-module-composer-cutover.md
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-003-scaffold-text-import-fact-drift.md
docs/arch/refactor/rev11/evidence/B4/finding-frozen-w13-superseded-by-render-export-fact.md
docs/arch/refactor/rev11/evidence/B4/landing-record.md
docs/arch/refactor/rev11/evidence/BA0/context-packet.md
docs/arch/refactor/rev11/evidence/BA0/landing-record.md
docs/arch/refactor/rev11/evidence/BF1/BF1-summary.md
docs/arch/refactor/rev11/evidence/BF1/context-packet.md
docs/arch/refactor/rev11/evidence/BF2/BF2-reopen3-summary.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-final-candidate/README.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-final-candidate/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-final-candidate/session-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/session-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/README.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/session1-nonconforming-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/session2-nonconforming-raw.txt
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/context-packet.md
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary.md
docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md
docs/arch/refactor/rev11/evidence/BF2/reopen3-context-packet.md
docs/arch/refactor/rev11/evidence/BF2/reopen3-implementation-report.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/context-packet.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/landing-record.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/mapping-oracle-scoping-consult.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/optimization-vs-conformance-investigation.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-adversarial.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md
docs/arch/refactor/rev11/evidence/BF3/adjudication-prompt.md
docs/arch/refactor/rev11/evidence/BF3/adjudication-ruling.md
docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md
docs/arch/refactor/rev11/evidence/BF3/amd009-unratified-package.md
docs/arch/refactor/rev11/evidence/BF3/architecture-mandate-review.md
docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md
docs/arch/refactor/rev11/evidence/BF3/at2-disposition-prompt.md
docs/arch/refactor/rev11/evidence/BF3/at2-disposition-ruling.md
docs/arch/refactor/rev11/evidence/BF3/charter-drift-review.md
docs/arch/refactor/rev11/evidence/BF3/context-packet.md
docs/arch/refactor/rev11/evidence/BF3/disposition-prompt.md
docs/arch/refactor/rev11/evidence/BF3/disposition-ruling.md
docs/arch/refactor/rev11/evidence/BF3/dispositions.md
docs/arch/refactor/rev11/evidence/BF3/exhaustion-closure-reviews.md
docs/arch/refactor/rev11/evidence/BF3/landing-record.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-scope-clarification.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-ruling-section7-ratification.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-standing-ruling-bugs-and-types.md
docs/arch/refactor/rev11/evidence/BF3/reattestation.md
docs/arch/refactor/rev11/evidence/BF3/reviews/README.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-adversarial.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-architecture.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-confirm-1.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-confirm-2.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-conformance.md
docs/arch/refactor/rev11/evidence/BF3/scope-consult-prompt.md
docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md
docs/arch/refactor/rev11/evidence/BF3/scope-memo.md
docs/arch/refactor/rev11/evidence/BF3/test-invocations.md
docs/arch/refactor/rev11/evidence/BRT0/context-packet.md
docs/arch/refactor/rev11/evidence/BRT0/landing-record.md
docs/arch/refactor/rev11/evidence/BS0/context-packet.md
docs/arch/refactor/rev11/evidence/BS0/landing-record.md
docs/arch/refactor/rev11/evidence/BV0/context-packet.md
docs/arch/refactor/rev11/evidence/BV0/landing-record.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round10-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round3-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round6-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round7-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round8-review.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round9-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-rounds4-5-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/circularity-consult.md
docs/arch/refactor/rev11/evidence/BV0A/context-packet.md
docs/arch/refactor/rev11/evidence/BV0A/debt-layer1-gate-authority.md
docs/arch/refactor/rev11/evidence/BV0A/historical-baseline-provenance.md
docs/arch/refactor/rev11/evidence/BV0A/landing-record.md
docs/arch/refactor/rev11/evidence/BV0A/layer1-d8-adoption.md
docs/arch/refactor/rev11/evidence/BV0A/layer1-freeze-adoption.md
docs/arch/refactor/rev11/evidence/BV0A/layer2-readiness-record.md
docs/arch/refactor/rev11/evidence/BV0A/mutation-evidence-fields-and-staging.md
docs/arch/refactor/rev11/evidence/BV0A/mutation-replay-recipes.md
docs/arch/refactor/rev11/evidence/BV1/context-packet.md
docs/arch/refactor/rev11/evidence/BV1/landing-record.md
docs/arch/refactor/rev11/evidence/framework-conformance/B2-parse-facet-svelte.md
docs/arch/refactor/rev11/evidence/framework-conformance/B2-parse-facet-vue.md
docs/arch/refactor/rev11/evidence/framework-conformance/README.md
docs/arch/refactor/rev11/evidence/framework-conformance/bf3-safety-retraction-scope.md
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/deny-network.sb
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/run-session.sh
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/session-raw.txt
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-oracle-manifest-generate/README.md
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-oracle-manifest-generate/deny-network.sb
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-oracle-manifest-generate/run-session.sh

[32m succeeded in 26614ms:[0m
<MACHINE_ROOT>/verter
docs/arch/refactor/rev11/OPUS-START-HERE.md
docs/arch/refactor/rev11/ORCHESTRATOR.md
docs/arch/refactor/rev11/PROVENANCE.md
docs/arch/refactor/rev11/README.md
docs/arch/refactor/rev11/_EXTRACTION_INDEX.md
docs/arch/refactor/rev11/agents/opus-bootstrap.md
docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md
docs/arch/refactor/rev11/amendments/AMD-002-a2c-completion-predecessor.md
docs/arch/refactor/rev11/amendments/AMD-003-a2c-completion-graph-authority.md
docs/arch/refactor/rev11/amendments/AMD-004-defer-completion-to-d6.md
docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md
docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md
docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md
docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md
docs/arch/refactor/rev11/amendments/AMD-009-bf3-audit-and-immediate-correction-blocks.md
docs/arch/refactor/rev11/amendments/AMD-010-b3-route-conversion-ownership-and-b2-parse-facet-exit.md
docs/arch/refactor/rev11/architecture.md
docs/arch/refactor/rev11/baseline/9af553dd.md
docs/arch/refactor/rev11/charters/A0.md
docs/arch/refactor/rev11/charters/A1.md
docs/arch/refactor/rev11/charters/A2.md
docs/arch/refactor/rev11/charters/A2C.md
docs/arch/refactor/rev11/charters/A3.md
docs/arch/refactor/rev11/charters/A4.md
docs/arch/refactor/rev11/charters/A5.md
docs/arch/refactor/rev11/charters/A6.md
docs/arch/refactor/rev11/charters/B1.md
docs/arch/refactor/rev11/charters/B1.template.md
docs/arch/refactor/rev11/charters/B2.md
docs/arch/refactor/rev11/charters/B3.md
docs/arch/refactor/rev11/charters/B4.md
docs/arch/refactor/rev11/charters/B5.md
docs/arch/refactor/rev11/charters/B6.md
docs/arch/refactor/rev11/charters/BA0.md
docs/arch/refactor/rev11/charters/BCSS0.md
docs/arch/refactor/rev11/charters/BF1.md
docs/arch/refactor/rev11/charters/BF2.md
docs/arch/refactor/rev11/charters/BF3.md
docs/arch/refactor/rev11/charters/BRT0.md
docs/arch/refactor/rev11/charters/BS0.md
docs/arch/refactor/rev11/charters/BS1.md
docs/arch/refactor/rev11/charters/BV0.md
docs/arch/refactor/rev11/charters/BV0A.md
docs/arch/refactor/rev11/charters/BV1.md
docs/arch/refactor/rev11/charters/C1.md
docs/arch/refactor/rev11/charters/C3.md
docs/arch/refactor/rev11/charters/C4.md
docs/arch/refactor/rev11/charters/J1.template.md
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md
docs/arch/refactor/rev11/contracts/agent-orchestration.md
docs/arch/refactor/rev11/contracts/architecture-falsification.md
docs/arch/refactor/rev11/contracts/baseline-lock.md
docs/arch/refactor/rev11/contracts/capability-matrix.md
docs/arch/refactor/rev11/contracts/compile-transaction.md
docs/arch/refactor/rev11/contracts/conformance-goldens.md
docs/arch/refactor/rev11/contracts/conformance-normalizer.md
docs/arch/refactor/rev11/contracts/current-tree-reconciliation.md
docs/arch/refactor/rev11/contracts/deterministic-ordering.md
docs/arch/refactor/rev11/contracts/flow-completeness.md
docs/arch/refactor/rev11/contracts/fragment-assembly.md
docs/arch/refactor/rev11/contracts/framework-compiler-boundary.md
docs/arch/refactor/rev11/contracts/identity-encoding.md
docs/arch/refactor/rev11/contracts/input-loading.md
docs/arch/refactor/rev11/contracts/language-tools-exclusion.md
docs/arch/refactor/rev11/contracts/mapping-products.md
docs/arch/refactor/rev11/contracts/official-core-oracles.md
docs/arch/refactor/rev11/contracts/package-publication.md
docs/arch/refactor/rev11/contracts/parse-ownership.md
docs/arch/refactor/rev11/contracts/result-contract-and-flight.md
docs/arch/refactor/rev11/contracts/semantic-profile.md
docs/arch/refactor/rev11/contracts/ssr-hydration.md
docs/arch/refactor/rev11/contracts/stacked-prs.md
docs/arch/refactor/rev11/contracts/third-party-exclusion.md
docs/arch/refactor/rev11/contracts/typescript-product-conformance.md
docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md
docs/arch/refactor/rev11/decisions/ADR-002-compatibility-domains.md
docs/arch/refactor/rev11/decisions/ADR-003-sealed-compile-semantic-facade.md
docs/arch/refactor/rev11/decisions/ADR-004-typescript-semantic-profiles.md
docs/arch/refactor/rev11/decisions/ADR-005-operation-dtos-and-optional-graph-export.md
docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md
docs/arch/refactor/rev11/decisions/ADR-007-direct-core-before-managed-runtime.md
docs/arch/refactor/rev11/decisions/ADR-008-deterministic-artifacts-and-persistence.md
docs/arch/refactor/rev11/decisions/ADR-009-shared-frontends-and-parse-owner-domains.md
docs/arch/refactor/rev11/decisions/ADR-010-compositional-products-and-mapping-taxonomy.md
docs/arch/refactor/rev11/decisions/ADR-011-staged-compile-attempt-and-input-loading.md
docs/arch/refactor/rev11/decisions/ADR-012-stable-identifiers-and-canonical-ordering.md
docs/arch/refactor/rev11/decisions/ADR-013-result-contracts-and-flight-owned-production.md
docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md
docs/arch/refactor/rev11/decisions/ADR-015-binding-dependency-direction.md
docs/arch/refactor/rev11/decisions/ADR-016-implementation-lock-and-performance-gates.md
docs/arch/refactor/rev11/decisions/ADR-017-stack-aware-review-and-landing.md
docs/arch/refactor/rev11/decisions/ADR-018-opus-adapter-and-orchestrator-state.md
docs/arch/refactor/rev11/decisions/ADR-019-reproducible-authority-package.md
docs/arch/refactor/rev11/decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md
docs/arch/refactor/rev11/evidence/A0-preflight-blocked.md
docs/arch/refactor/rev11/evidence/A0-summary.md
docs/arch/refactor/rev11/evidence/A4-summary.md
docs/arch/refactor/rev11/evidence/A4/baseline-40-components.tsv
docs/arch/refactor/rev11/evidence/A4/context-packet.md
docs/arch/refactor/rev11/evidence/A4/disabled-overhead.md
docs/arch/refactor/rev11/evidence/A5-summary.md
docs/arch/refactor/rev11/evidence/A5/compatibility-domains.md
docs/arch/refactor/rev11/evidence/A5/consumer-protocol-map.md
docs/arch/refactor/rev11/evidence/A5/context-packet.md
docs/arch/refactor/rev11/evidence/A5/dependency-direction.md
docs/arch/refactor/rev11/evidence/A5/instrumentation-reconciliation.md
docs/arch/refactor/rev11/evidence/A5/loop5-counter-census.tsv
docs/arch/refactor/rev11/evidence/A5/open-changes.md
docs/arch/refactor/rev11/evidence/A5/option-classification.md
docs/arch/refactor/rev11/evidence/A5/option-classification.tsv
docs/arch/refactor/rev11/evidence/A5/owner-rows.md
docs/arch/refactor/rev11/evidence/A5/program-operations-policy.md
docs/arch/refactor/rev11/evidence/A6-summary.md
docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md
docs/arch/refactor/rev11/evidence/A6/B1-context-packet.md
docs/arch/refactor/rev11/evidence/A6/baseline-counters.tsv
docs/arch/refactor/rev11/evidence/A6/baseline-measurement.md
docs/arch/refactor/rev11/evidence/A6/command-proofs-native.md
docs/arch/refactor/rev11/evidence/A6/command-proofs.md
docs/arch/refactor/rev11/evidence/A6/context-packet.md
docs/arch/refactor/rev11/evidence/A6/counter-reproduction.md
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md
docs/arch/refactor/rev11/evidence/A6/landing-equivalence-A5-A6.md
docs/arch/refactor/rev11/evidence/A6/stack-window-policy.toml
docs/arch/refactor/rev11/evidence/AMD-010/maintainer-ruling.md
docs/arch/refactor/rev11/evidence/B1/command-proofs/01-gate.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/02-clippy-workspace.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/03-check-release.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/04-clippy-wasm.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/05-fmt-check.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/06-pnpm-install.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/07-pnpm-test.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/08-audit-arms.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/09-discrimination-proof.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/10-performance-cell.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/11-validate-performance-gates.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/12-real-capped-gate.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/13-flake-disposition-hover.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/14-flake-disposition-compile-fail-smoke.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/15-tracked-paths-recheck.txt
docs/arch/refactor/rev11/evidence/B1/command-proofs/16-real-gate-disposition.md
docs/arch/refactor/rev11/evidence/B1/landing-equivalence.md
docs/arch/refactor/rev11/evidence/B1/pnpm-test-disposition.md
docs/arch/refactor/rev11/evidence/B2/context-packet.md
docs/arch/refactor/rev11/evidence/B2/landing-record.md
docs/arch/refactor/rev11/evidence/B3/context-packet.md
docs/arch/refactor/rev11/evidence/B3/debt-FC-OPTIONS-002-option-attempt-decode-unwired.md
docs/arch/refactor/rev11/evidence/B3/debt-FC-SVELTE-001-svelte-output-liveness.md
docs/arch/refactor/rev11/evidence/B3/landing-record.md
docs/arch/refactor/rev11/evidence/B4/context-packet.md
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-001-generated-chunk-callsite-migration.md
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-002-vue-main-module-composer-cutover.md
docs/arch/refactor/rev11/evidence/B4/debt-FC-B4-003-scaffold-text-import-fact-drift.md
docs/arch/refactor/rev11/evidence/B4/finding-frozen-w13-superseded-by-render-export-fact.md
docs/arch/refactor/rev11/evidence/B4/landing-record.md
docs/arch/refactor/rev11/evidence/BA0/context-packet.md
docs/arch/refactor/rev11/evidence/BA0/landing-record.md
docs/arch/refactor/rev11/evidence/BF1/BF1-summary.md
docs/arch/refactor/rev11/evidence/BF1/context-packet.md
docs/arch/refactor/rev11/evidence/BF2/BF2-reopen3-summary.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-final-candidate/README.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-final-candidate/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-final-candidate/session-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/README.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen2-fix-final/session-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/README.md
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/run-session.sh
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/session1-nonconforming-raw.txt
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/session2-nonconforming-raw.txt
docs/arch/refactor/rev11/evidence/BF2/context-packet-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/context-packet.md
docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen1.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary-reopen2-final.md
docs/arch/refactor/rev11/evidence/BF2/evidence-summary.md
docs/arch/refactor/rev11/evidence/BF2/manifest-classification-accounting.md
docs/arch/refactor/rev11/evidence/BF2/reopen3-context-packet.md
docs/arch/refactor/rev11/evidence/BF2/reopen3-implementation-report.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/context-packet.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/landing-record.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/mapping-oracle-scoping-consult.md
docs/arch/refactor/rev11/evidence/BF2/reopen4/optimization-vs-conformance-investigation.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-perf-gate-consult-codex-xhigh.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-adversarial.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-architecture.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen1-round2-conformance.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-adversarial.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-architecture.md
docs/arch/refactor/rev11/evidence/BF2/reviews/reopen2-round7-conformance.md
docs/arch/refactor/rev11/evidence/BF2/second-reopen-ruling.md
docs/arch/refactor/rev11/evidence/BF3/adjudication-prompt.md
docs/arch/refactor/rev11/evidence/BF3/adjudication-ruling.md
docs/arch/refactor/rev11/evidence/BF3/amd009-ratification-packet.md
docs/arch/refactor/rev11/evidence/BF3/amd009-unratified-package.md
docs/arch/refactor/rev11/evidence/BF3/architecture-mandate-review.md
docs/arch/refactor/rev11/evidence/BF3/at2-deviation-memo.md
docs/arch/refactor/rev11/evidence/BF3/at2-disposition-prompt.md
docs/arch/refactor/rev11/evidence/BF3/at2-disposition-ruling.md
docs/arch/refactor/rev11/evidence/BF3/charter-drift-review.md
docs/arch/refactor/rev11/evidence/BF3/context-packet.md
docs/arch/refactor/rev11/evidence/BF3/disposition-prompt.md
docs/arch/refactor/rev11/evidence/BF3/disposition-ruling.md
docs/arch/refactor/rev11/evidence/BF3/dispositions.md
docs/arch/refactor/rev11/evidence/BF3/exhaustion-closure-reviews.md
docs/arch/refactor/rev11/evidence/BF3/landing-record.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-amendment.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-act-at2-scope-clarification.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-ruling-section7-ratification.md
docs/arch/refactor/rev11/evidence/BF3/maintainer-standing-ruling-bugs-and-types.md
docs/arch/refactor/rev11/evidence/BF3/reattestation.md
docs/arch/refactor/rev11/evidence/BF3/reviews/README.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-adversarial.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-architecture.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-confirm-1.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-confirm-2.md
docs/arch/refactor/rev11/evidence/BF3/reviews/exhaustion-conformance.md
docs/arch/refactor/rev11/evidence/BF3/scope-consult-prompt.md
docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md
docs/arch/refactor/rev11/evidence/BF3/scope-memo.md
docs/arch/refactor/rev11/evidence/BF3/test-invocations.md
docs/arch/refactor/rev11/evidence/BRT0/context-packet.md
docs/arch/refactor/rev11/evidence/BRT0/landing-record.md
docs/arch/refactor/rev11/evidence/BS0/context-packet.md
docs/arch/refactor/rev11/evidence/BS0/landing-record.md
docs/arch/refactor/rev11/evidence/BV0/context-packet.md
docs/arch/refactor/rev11/evidence/BV0/landing-record.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round10-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round3-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round6-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round7-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round8-review.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-round9-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/amd-008-rounds4-5-reviews.md
docs/arch/refactor/rev11/evidence/BV0A/circularity-consult.md
docs/arch/refactor/rev11/evidence/BV0A/context-packet.md
docs/arch/refactor/rev11/evidence/BV0A/debt-layer1-gate-authority.md
docs/arch/refactor/rev11/evidence/BV0A/historical-baseline-provenance.md
docs/arch/refactor/rev11/evidence/BV0A/landing-record.md
docs/arch/refactor/rev11/evidence/BV0A/layer1-d8-adoption.md
docs/arch/refactor/rev11/evidence/BV0A/layer1-freeze-adoption.md
docs/arch/refactor/rev11/evidence/BV0A/layer2-readiness-record.md
docs/arch/refactor/rev11/evidence/BV0A/mutation-evidence-fields-and-staging.md
docs/arch/refactor/rev11/evidence/BV0A/mutation-replay-recipes.md
docs/arch/refactor/rev11/evidence/BV1/context-packet.md
docs/arch/refactor/rev11/evidence/BV1/landing-record.md
docs/arch/refactor/rev11/evidence/framework-conformance/B2-parse-facet-svelte.md
docs/arch/refactor/rev11/evidence/framework-conformance/B2-parse-facet-vue.md
docs/arch/refactor/rev11/evidence/framework-conformance/README.md
docs/arch/refactor/rev11/evidence/framework-conformance/bf3-safety-retraction-scope.md
docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/README.md
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/deny-network.sb
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/run-session.sh
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-official-compiler-invocation-golden-generate/session-raw.txt
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-oracle-manifest-generate/README.md
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-oracle-manifest-generate/deny-network.sb
docs/arch/refactor/rev11/evidence/framework-conformance/command-proofs/bf2-oracle-manifest-generate/run-session.sh

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'git branch --show-current; git rev-parse HEAD; git status --short; git log -1 --oneline --decorate; git show --stat --oneline --decorate --no-renames f46de1b6a -- scripts/validate-stack-window.mjs scripts/lib/stack-window-lib.mjs'[0m in <MACHINE_ROOT>/verter
[32m succeeded in 23847ms:[0m
program/architecture-lock
6ab1000bd6542101e663d388b0ba20f1485d1e5c
6ab1000bd (HEAD -> program/architecture-lock, origin/program/architecture-lock, feat/gate-failure-triage) fix(ci): gate the Svelte/Vue oracle-cache prerequisite and land bf2-authoritative in the canonical archive
f46de1b6a feat(ci): add stack-window validator and supersede the checkpoint fail-closed refusal
 scripts/lib/stack-window-lib.mjs  | 442 ++++++++++++++++++++++++++++++++++++++
 scripts/validate-stack-window.mjs | 146 +++++++++++++
 2 files changed, 588 insertions(+)

  [36m→[0m Read charter and governing Revision 11 program/ADR/contracts/evidence
  [2m•[0m [2mTrace C1 relocation, obligation runtime, and stable binding identities in code[0m
  [2m•[0m [2mVerify A6 capability rows and AMD-001 validator/test delivery[0m
  [2m•[0m [2mIssue six rulings with amendment impact and exact citations[0m
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg --files docs/arch/refactor/rev11 | rg '("'^|/)(program|program-dag|program-state|maintainer-rulings|ARCH-RULING-C1)'"' | sort; rg -n \"ARCH-RULING-C1|C1-FOUR|U-1|effective-flow capability|stable binding slots|Private sole-solver\" docs/arch/refactor/rev11"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 19145ms:[0m
docs/arch/refactor/rev11/evidence/A5/program-operations-policy.md
docs/arch/refactor/rev11/evidence/framework-conformance/program-state-transition.md
docs/arch/refactor/rev11/evidence/maintainer-rulings.md
docs/arch/refactor/rev11/program-dag.toml
docs/arch/refactor/rev11/program.md
docs/arch/refactor/rev11/templates/program-state.template.toml
docs/arch/refactor/rev11/program-dag.toml:185:name = "Private sole-solver foundation checkpoint"
docs/arch/refactor/rev11/verification.md:271:- every `A6`-ratified Supported/Stable effective-flow capability remains conformant, or an explicit reviewed breaking capability decision is present.
docs/arch/refactor/rev11/program.md:225:## D1 — Private sole-solver foundation
docs/arch/refactor/rev11/program.md:229:On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.
docs/arch/refactor/rev11/program.md:231:No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.
docs/arch/refactor/rev11/program.md:244:- preserve every `A6`-ratified Supported/Stable effective-flow capability and its public failure contract;
docs/arch/refactor/rev11/program.md:253:Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.
docs/arch/refactor/rev11/architecture.md:790:4. preserve every `A6`-ratified Supported/Stable effective-flow capability unless a separate reviewed breaking product decision changes it;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1293:4. preserve every `A6`-ratified Supported/Stable effective-flow capability unless a separate reviewed breaking product decision changes it;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3231:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4299:## D1 — Private sole-solver foundation
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4303:On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4305:No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4318:- preserve every `A6`-ratified Supported/Stable effective-flow capability and its public failure contract;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4327:Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4664:name = "Private sole-solver foundation checkpoint"
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:5522:- every `A6`-ratified Supported/Stable effective-flow capability remains conformant, or an explicit reviewed breaking capability decision is present.
docs/arch/refactor/rev11/charters/C1.md:7:**Rulings applied.** This charter was rewritten to conform to `ARCH-RULING-C1-FOUR-FORKS.md` (the
docs/arch/refactor/rev11/charters/C1.md:416:Binding ruling: `ARCH-RULING-C1-FOUR-FORKS.md` (Codex xhigh architecture challenge against the prior
docs/arch/refactor/rev11/contracts/flow-completeness.md:81:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
docs/arch/refactor/rev11/evidence/A6-summary.md:169:| the two feature arms as locked per-block commands, CI job deferred post-program | lock record §2, with the deferral as U-10 |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md:3051:docs/arch/refactor/rev11/program.md:231:No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md:3161:docs/arch/refactor/rev11/contracts/flow-completeness.md:81:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:6249:docs/arch/refactor/rev11/program-dag.toml-155-name = "Private sole-solver foundation checkpoint"
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7124:docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md-872-| **U-10** CI wiring for the instrumentation feature arms | No GitHub Actions job runs for any block of this program, so a CI job added now would not execute; the arms are locked as required per-block commands instead | post-program | after the program lands on `main`; requires a ruling extending the one narrow CI-wiring authorization |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7125:docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md-873-| **U-11** The new gate validator is not added to the CI change-detection path filter | Same reason as U-10, plus: the existing authorization for a `.github/` edit was granted for one named purpose only, and extending it needs its own ruling | post-program | with U-10 |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7126:docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md-874-| **U-12** `PROVENANCE.md`'s published aggregate digest is stale with respect to the tree it describes | The rescope commit `fb863297a…` edited `amendments/AMD-001-…md`, which is inside the aggregate's input set, without republishing the aggregate; the published value `ff49cdd…` now reproduces only at the pre-rescope tree. Non-blocking here because this record does not consume the published value as authority — §1 recomputes both aggregates from the git object store and records the recomputed pair, and it reproduces `ff49cdd…` at the pre-rescope tree as the method control. **Not corrected by this candidate:** `PROVENANCE.md` is an authority file of the integration lineage, not a block artifact, and a lock candidate silently rewriting an authority digest is exactly the move this program's evidence discipline forbids | orchestrator, on the integration lineage | at, or before, the transition that accepts this record — republish the aggregate as `a061d97534f2b96f96a92eae24569f8500c696a4b82d827efbd1a52deb78a173` (74 files) at the accepted tip, or record why the published value is pinned to a superseded tree |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7216:docs/arch/refactor/rev11/evidence/A6-summary.md-169-| the two feature arms as locked per-block commands, CI job deferred post-program | lock record §2, with the deferral as U-10 |
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:165:candidate, and this record does not edit `PROVENANCE.md` to hide it — see §11 note U-12.
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:333:ratified before that block begins.** Recorded as §11 row U-1 with its owner and gate. Nothing before
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:850:exceeding it rather than quietly filed under it** (U-1, U-4). Each is a deferral of *program
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:858:longer a deviation this lock carries. U-12 is likewise informational — it discloses a staleness in
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:863:| **U-1** Capability matrix is entirely `VERIFY`; no maturity, default or compatibility promise is ratified | Fail-closed: an unratified row is not approved for architecture claims or default changes, so nothing can rely on it by accident. **Exceeds the §11 bar** — it is a compatibility decision, deferred, not a private choice | product/conformance review with the maintainer | **before the atomic flow-cutover block begins.** That block's charter requires it to satisfy every Supported/Stable row in this matrix; with none declared, the obligation is vacuous until the matrix is ratified |
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:872:| **U-10** CI wiring for the instrumentation feature arms | No GitHub Actions job runs for any block of this program, so a CI job added now would not execute; the arms are locked as required per-block commands instead | post-program | after the program lands on `main`; requires a ruling extending the one narrow CI-wiring authorization |
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:873:| **U-11** The new gate validator is not added to the CI change-detection path filter | Same reason as U-10, plus: the existing authorization for a `.github/` edit was granted for one named purpose only, and extending it needs its own ruling | post-program | with U-10 |
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:874:| **U-12** `PROVENANCE.md`'s published aggregate digest is stale with respect to the tree it describes | The rescope commit `fb863297a…` edited `amendments/AMD-001-…md`, which is inside the aggregate's input set, without republishing the aggregate; the published value `ff49cdd…` now reproduces only at the pre-rescope tree. Non-blocking here because this record does not consume the published value as authority — §1 recomputes both aggregates from the git object store and records the recomputed pair, and it reproduces `ff49cdd…` at the pre-rescope tree as the method control. **Not corrected by this candidate:** `PROVENANCE.md` is an authority file of the integration lineage, not a block artifact, and a lock candidate silently rewriting an authority digest is exactly the move this program's evidence discipline forbids | orchestrator, on the integration lineage | at, or before, the transition that accepts this record — republish the aggregate as `a061d97534f2b96f96a92eae24569f8500c696a4b82d827efbd1a52deb78a173` (74 files) at the accepted tip, or record why the published value is pinned to a superseded tree |
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:885:      complete (§5); the **capability matrix is unratified** and recorded as U-1, fail-closed
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:898:      U-1 and U-4 exceed the §11 bar and are named as such. U-9 no longer does — ruling R-12
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:906:ticked complete while U-1 sits unratified would be exactly the failure mode this program's

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/ORCHESTRATOR.md | sed -n '1,170p'; nl -ba docs/arch/refactor/rev11/program.md | sed -n '210,270p'; nl -ba docs/arch/refactor/rev11/program-dag.toml | sed -n '170,210p'; rg --files docs/arch/architecture-lock docs/arch/refactor/rev11 | rg 'program-state\\.toml"'$'"'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 19840ms:[0m
     1	# Verter Revision 11 — Orchestrator Entry Point
     2	
     3	**Use this file as the first package instruction for the orchestrator.**  
     4	**Canonical input:** the extracted Revision 11 split package plus an actual local checkout of `pikax/verter`.  
     5	**Initial authorization:** execute `A0` only.
     6	
     7	# 1. Your role
     8	
     9	You are the implementation orchestrator, not the maintainer and not an architecture authority.
    10	
    11	Your job is to:
    12	
    13	- validate the release package and actual checkout;
    14	- maintain exact program, branch, stack, candidate, evidence, and review state;
    15	- execute only the next legal bounded block;
    16	- delegate substantial independent work through immutable context packets;
    17	- stop and report when source or evidence contradicts the plan;
    18	- recommend acceptance only from reproducible evidence.
    19	
    20	You must not silently change architecture, weaken a gate, widen a charter, self-approve, or treat your own summary as independent review.
    21	
    22	# 2. Opus adapter identity
    23	
    24	The supplied Opus bootstrap requests fixed model ID `claude-opus-5`. Record the actual model, provider, and any fallback/substitution in `program-state.toml` before repository mutation.
    25	
    26	If the actual runtime differs from the requested runtime, you may complete non-mutating package/repository inspection, but return `A0 BLOCKED` before representing the result as an Opus-governed Foundational outcome unless the designated maintainer records an explicit exception. Model brand does not replace role independence or evidence.
    27	
    28	# 3. Read before acting
    29	
    30	Read, in this order:
    31	
    32	1. `README.md`
    33	2. `contracts/agent-orchestration.md`
    34	3. `contracts/stacked-prs.md`
    35	4. `contracts/package-publication.md`
    36	5. `contracts/architecture-falsification.md`
    37	6. `program-dag.toml`
    38	7. `governance.md`
    39	8. `charters/A0.md`
    40	9. `contracts/baseline-lock.md`
    41	10. `contracts/current-tree-reconciliation.md`
    42	11. the A0-relevant sections of `verification.md`
    43	
    44	Do not load the entire package into every worker context. Give each worker only its bounded context packet and directly relevant authority/evidence.
    45	
    46	# 4. First commands
    47	
    48	From the extracted package:
    49	
    50	```bash
    51	python3 tools/validate_package.py
    52	python3 tools/selftest_orchestration.py
    53	claude --version
    54	python3 tools/validate_program_state.py \
    55	  --dag program-dag.toml \
    56	  --state templates/program-state.template.toml \
    57	  --mode template
    58	python3 tools/validate_stack_window.py \
    59	  --dag program-dag.toml \
    60	  --stack templates/stack-window.template.toml \
    61	  --mode template
    62	python3 tools/validate_landing_equivalence.py \
    63	  --proof templates/landing-equivalence.template.toml \
    64	  --mode template
    65	```
    66	
    67	From the actual Verter checkout, collect without mutation:
    68	
    69	```bash
    70	git rev-parse --show-toplevel
    71	git remote -v
    72	git branch --show-current
    73	git rev-parse HEAD
    74	git rev-parse HEAD^{tree}
    75	git status --porcelain=v1 --untracked-files=all
    76	git worktree list --porcelain
    77	git submodule status --recursive || true
    78	git log -1 --decorate=full --stat
    79	```
    80	
    81	Enumerate local/remote architecture-affecting branches and open/queued pull requests using the repository's available GitHub tooling. Record unavailable permissions or APIs rather than assuming a clean state.
    82	
    83	Initialize a repository-local ignored or external evidence directory and copy `templates/program-state.template.toml` to `program-state.toml`. Set top-level `status = "ACTIVE"`, resolve every A0-required field, then run:
    84	
    85	```bash
    86	python3 <REV11_PACKAGE>/tools/validate_program_state.py \
    87	  --dag <REV11_PACKAGE>/program-dag.toml \
    88	  --state <REPO_EVIDENCE>/program-state.toml \
    89	  --mode live
    90	```
    91	
    92	# 5. Execute only A0
    93	
    94	A0 produces:
    95	
    96	- exact entry checkout SHA/tree and dirty/untracked/submodule/worktree state;
    97	- remote, branch, target-lineage, and toolchain identity;
    98	- Revision 11 package, release, and DAG digests;
    99	- open PR/branch/queued-change disposition, including changes touching compiler, TypeInfo, flow, source ownership, CSS, framework, LSP/provider, protocol, cache, persistence, or lifetime boundaries;
   100	- available command, CI, branch protection, merge queue, stack tooling, signing, and permission facts;
   101	- designated maintainer and orchestrator identities;
   102	- requested/actual model, orchestrator runtime/version, provider, and fallback state;
   103	- initial validated program state with only legal next work exposed;
   104	- A0 context/evidence packet and exact review state.
   105	
   106	Do not start `A1` or any production cutover until A0 has been accepted under `governance.md`.
   107	
   108	# 6. Delegation
   109	
   110	Use no subagent when A0 can be completed directly with a small number of repository/tool calls.
   111	
   112	When delegation is genuinely useful:
   113	
   114	- default to no more than three active worker contexts;
   115	- assign one role and one immutable context packet per worker;
   116	- use separate writable worktrees/branches;
   117	- do not ask workers to “review the entire architecture”;
   118	- do not use a subagent merely to echo or generically double-check your own conclusion;
   119	- keep conformance, architecture, and adversarial mandates distinct where required.
   120	
   121	# 7. Stacked PR policy
   122	
   123	Stacked PRs are allowed only under `contracts/stacked-prs.md`. Every `ACTIVE` or later stack snapshot is validated both against its pre-stack program-state basis and the current program-state ledger.
   124	
   125	Do not create a program-wide stack. During A0, record whether GitHub native stacks, ordinary dependent PRs, merge queues, and signed rebases are available. A6 selects the operational tooling and locks a bounded stack-window policy. The default maximum is four open review layers; the permitted A6 range is two through six.
   126	
   127	Private atomic layers, especially D1, are never independently mergeable to trunk.
   128	
   129	# 8. Stop instead of improvising
   130	
   131	Stop and report `BLOCKED` or `RESCOPE_REQUIRED` when:
   132	
   133	- release checksum or package validation fails;
   134	- the checkout is dirty or contains unowned work;
   135	- target trunk differs from the expected line and the difference is not dispositioned;
   136	- an open PR/branch competes for an authority;
   137	- required repository/GitHub/CI facts cannot be obtained;
   138	- source disproves an architectural assumption;
   139	- A0 would require production ownership changes;
   140	- the maintainer identity or acceptance path is absent;
   141	- actual model/runtime differs from the Opus adapter without an accepted exception.
   142	
   143	Do not fix these by hiding changes, deleting someone else's work, weakening checks, or assuming intent.
   144	
   145	# 9. Return format
   146	
   147	Lead with exactly one:
   148	
   149	```text
   150	A0 ACCEPTANCE RECOMMENDED
   151	A0 BLOCKED
   152	A0 RESCOPE REQUIRED
   153	```
   154	
   155	Then return the bounded record required by `contracts/agent-orchestration.md`, including raw evidence paths/digests, requested/actual model, exact SHA/tree, unresolved decisions, and next legal blocks derived from validated state.
   156	
   157	Do not claim the architecture is implemented or performance-proven. A0 establishes a trustworthy entry state only.
   210	
   211	Batch exact demands, deduplicate shared roots, stop at broad constructor terminals, follow imports/generics/unions/mapped/indexed forms only until the closed answer, and return typed degradation/dependencies/profile.
   212	
   213	**Exit:** codegen receives only facts it consumes.
   214	
   215	## C4 — Zero-work, batch-shape, and regime-equivalence proof
   216	
   217	**Predecessors:** `B5`, `C3`.
   218	
   219	Prove no-demand zero initialization, Svelte zero Vue/native projection, aggregate-project-once batch shape, and equal direct/prepared/managed semantics.
   220	
   221	**Exit:** optional semantics are physically absent when not requested.
   222	
   223	# 7. Track D — Atomic sole effective-flow solver
   224	
   225	## D1 — Private sole-solver foundation
   226	
   227	**Predecessors:** `A3`, `B1`, `B2`, `C1`.
   228	
   229	On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.
   230	
   231	No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.
   232	
   233	**Exit:** the minimum replacement preserves every ratified supported capability, safely answers its covered cases, and fails unsupported cases without a second production authority.
   234	
   235	## D2 — Atomic public flow cutover and legacy deletion
   236	
   237	**Predecessors:** `D1`.
   238	
   239	In one accepted candidate:
   240	
   241	- route every effective-flow product operation to the new solver;
   242	- delete the syntax-shaped evaluator, name-keyed state, duplicate control/completion/relation/effect paths, caches, tasks, metrics, flags, guards, and migration comments;
   243	- keep unsupported mechanisms as typed non-admissible gaps;
   244	- preserve every `A6`-ratified Supported/Stable effective-flow capability and its public failure contract;
   245	- prove one production solver by source, dependency, runtime, and cache-admission tests.
   246	
   247	**Exit:** one graph authority and one production solver. This block is indivisible.
   248	
   249	## D3 — Shared relation authority and binding/product domains
   250	
   251	**Predecessors:** `D2`, `C1`.
   252	
   253	Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.
   254	
   255	## D4 — Narrowing and structural returns
   256	
   257	**Predecessors:** `D3`.
   258	
   259	Implement supported narrowing/invalidation/predicate/substitution mechanisms. Every authored return contributes structurally; endpoint completion alone controls fallthrough `undefined`.
   260	
   261	## D5 — Closure, capture, freshness, and position-independent effects
   262	
   263	**Predecessors:** `D3`.
   264	
   265	Retain direct/transitive reads and writes, escape summaries, creation-time frontier/freshness, profile-compatible invalidation, and one effect mechanism across expression positions.
   266	
   267	## D6 — Loop fixed points and completion graph
   268	
   269	**Predecessors:** `D3`.
   270	
   170	
   171	[[block]]
   172	id = "C3"
   173	name = "Closed Vue runtime macro projection"
   174	class = "foundational"
   175	predecessors = ["C2"]
   176	
   177	[[block]]
   178	id = "C4"
   179	name = "Direct prepared batch and project-aware equivalence proof"
   180	class = "foundational"
   181	predecessors = ["B6", "C3"]
   182	
   183	[[block]]
   184	id = "D1"
   185	name = "Private sole-solver foundation checkpoint"
   186	class = "foundational-private-checkpoint"
   187	predecessors = ["A3", "B1", "B2", "C1"]
   188	
   189	[[block]]
   190	id = "D2"
   191	name = "Atomic public flow cutover and legacy deletion"
   192	class = "foundational-atomic"
   193	predecessors = ["D1"]
   194	
   195	[[block]]
   196	id = "D3"
   197	name = "Extend shared relation authority and binding product domains"
   198	class = "foundational"
   199	predecessors = ["D2", "C1"]
   200	
   201	[[block]]
   202	id = "D4"
   203	name = "Narrowing and structural returns"
   204	class = "foundational"
   205	predecessors = ["D3"]
   206	
   207	[[block]]
   208	id = "D5"
   209	name = "Closure capture freshness effects"
   210	class = "foundational"
docs/arch/architecture-lock/ledger/program-state.toml

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "find docs/arch/refactor/rev11 <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter -name 'ARCH-RULING-C1-FOUR-FORKS.md' -print 2>/dev/null; nl -ba docs/arch/refactor/rev11/charters/C1.md | sed -n '1,180p'; nl -ba docs/arch/refactor/rev11/charters/C1.md | sed -n '180,360p'; nl -ba docs/arch/refactor/rev11/charters/C1.md | sed -n '360,500p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 10712ms:[0m
<SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/rulings/ARCH-RULING-C1-FOUR-FORKS.md
     1	# C1 — Converge `ModuleResolverCore` and non-flow `TypeInfoCore`
     2	
     3	**Status:** DRAFT — authored for maintainer review; no AMD ratifies it yet. **Class:** Foundational
     4	(`program-dag.toml:162`). **Predecessors:** `A6`, `B1`, `B2` (`program-dag.toml:163`) — all three
     5	`ACCEPTED` (`docs/arch/architecture-lock/ledger/program-state.toml:216,237,447`).
     6	
     7	**Rulings applied.** This charter was rewritten to conform to `ARCH-RULING-C1-FOUR-FORKS.md` (the
     8	architecture challenge against the prior draft's four open forks). All four of that draft's proposed
     9	positions were **REJECTED**. See "Rulings applied" below for the four verdicts and their consequences; do
    10	not relitigate them here.
    11	
    12	## Context — why this is bigger than program.md's five lines
    13	
    14	`program.md:191-197` gives C1 five lines: converge two logical owners, preserve resolution semantics,
    15	use immutable observation views, return batched `NeedInputs`, exclude flow. Read literally that sounds
    16	like an in-place refactor. **It is not.** The four rulings resolve every genuine fork in the direction
    17	that costs the most, and the charter says so plainly rather than understating it:
    18	
    19	1. **This is a real crate extraction, now, not a deferred one.** `resolver_core` (`pub mod`, 59 files —
    20	   including `component_meta/` (6 files) and `component_meta_query_engine/` (14 files): verified, neither
    21	   holds a `&VerterHost`/`Arc<VerterHost>` field or parameter in production code, only doc-comment mentions
    22	   and `#[cfg(test)]` `VerterHost::new` fixture construction identical to the pattern already accepted for
    23	   `resolver_core`'s own top-level `*_tests.rs` files, so both subdirectories move with the rest of the
    24	   wildcard, not a carve-out — see the convergence map below),
    25	   the private `resolver_store` module, and the `pub(crate)` `project_semantic_dispatch` module —
    26	   all three currently living inside `verter_session` (`crates/verter_session/src/lib.rs:332,341-344`)
    27	   — physically relocate into the existing `verter_semantic` crate (layer 3). So does
    28	   `verter_workspace::resolver::ProjectResolver` (`crates/verter_workspace/src/resolver.rs`, 2122
    29	   lines), the TypeScript-style module resolver wrongly homed in the scheduler/tsgo-dependent
    30	   `verter_workspace` crate today. This is a multi-crate structural move touching `verter_session`'s
    31	   module tree, `verter_workspace`'s module tree, `verter_semantic`'s Cargo.toml dependency edges, and
    32	   the landed dependency-layers closure guard — not a `pub(crate)` facade behind which nothing moves.
    33	2. **The `NeedInputs`/`AttemptOutcome` cutover is full-coverage, not a first cut.** Every non-flow
    34	   `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt must support the
    35	   batched outcome — not just the two `ensure_loaded`/`ensure_indexed_ready_serve` call sites the prior
    36	   draft scoped it to. TypeInfo projection already reaches `ensure_indexed_ready_serve` today
    37	   (`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`), so "module/import resolution only"
    38	   was never even a clean existing boundary to cut at.
    39	
    40	Effort and breadth are not reasons to shrink this (`CLAUDE.md` → Planning: "Time constraints,
    41	implementation size, migration breadth... are not valid reasons to weaken the design"). The charter
    42	states the true scope instead of preserving the five-line framing that would mislead whoever picks this
    43	up. C1 converges the current host-backed module/type-resolution stack — `verter_session::resolver_core`,
    44	`verter_session::resolver_store`, `ProjectSemanticDispatch`, and `verter_workspace::resolver` — into the
    45	two target logical owners named in `architecture.md` §8.1: a `ModuleResolverCore` that owns
    46	TypeScript-style module/path/package resolution, and a non-flow `TypeInfoCore` that owns authored-node
    47	traversal, binder/name/reference semantics, substitution, relation/inference, effective typing,
    48	recursion/work budgets, query-specific projection, and exactness/completeness propagation. Neither
    49	target type exists in code today; both names describe roles the current tree already plays under
    50	different names, in the wrong crate, with the wrong I/O coupling. C1 does not invent new resolution
    51	semantics — every module/name/type/relation answer the converged kernel gives must be identical to
    52	today's answer for every existing lifecycle. It changes who owns the code (crate, not just module), how
    53	many duplicate implementations of the same lifecycle-adapter shape exist, whether the kernel can perform
    54	I/O directly, and where the crate boundary sits.
    55	
    56	C1 does not own flow semantics (`D1`–`D8`), Vue/Svelte macro projection (`C3`), the staged
    57	prepare/plan/project/emit compile transaction or its anti-replay tokens (`C2`), or component-meta
    58	publication policy (`/component-meta`). It extends nothing about `execute_relate`'s relation semantics or
    59	`ProjectSemanticDispatch`'s five query modes — those are `Preserve`, not `Converge`, dispositions, even
    60	though (per the full-coverage ruling) the *plumbing* around every call into them now must support
    61	`AttemptOutcome`. It owns convergence of context/lifecycle plumbing, dependency direction, physical crate
    62	placement, and full-coverage batched/resumable load outcomes so a future I/O-free lifecycle can consume
    63	the same kernel a blocking lifecycle consumes today.
    64	
    65	## Sequencing
    66	
    67	C1's predecessors (`A6`, `B1`, `B2`) are all `ACCEPTED`; C1 is dependency-eligible now
    68	(`docs/arch/architecture-lock/ledger/program-state.toml:594`, `status = "READY"`). The program executes
    69	one block at a time, and the ledger's `current_block` is `BV1`, `status = "IN_PROGRESS"`
    70	(`program-state.toml:22,510-513`). **C1 dispatch follows BV1's landing.** This charter does not propose
    71	running C1 in parallel with BV1 or with any other in-flight block.
    72	
    73	## Intent Contract
    74	
    75	**Actor / problem.** Every current and future first-party consumer of Verter's type/module resolution —
    76	the host-backed LSP session, the session-overlay (unsaved-edit) path, and the future project-aware direct
    77	compiler (`C2`'s sealed `CompileTypeInfo`) — must resolve a symbol, import, or type to the *same* answer.
    78	Today that guarantee is held by convention (three lifecycle adapters hand-implement the same sealed
    79	`ResolverContext` trait with large amounts of copy-pasted delegation), by omission (no lifecycle exists
    80	yet that cannot block a thread on I/O), and by the wrong crate holding the kernel at all (a session-tier
    81	crate that already depends on the compiler, so nothing compiler-tier can embed it without a cycle) — not
    82	by a structural authority that forces convergence and permits a new I/O-free lifecycle to be added
    83	without duplicating resolution logic a fourth time.
    84	
    85	**Required observable outcomes.**
    86	- A module/name/type/relation query issued through `HostResolverContext`, `SessionResolverContext`, or
    87	  (once added) an I/O-free project-capable context returns bit-identical `SemanticNodeId`/resolved-surface
    88	  results for the same source content, project configuration, and query, modulo the outcome shape
    89	  (blocking-complete vs. `NeedInputs`).
    90	- A resolver-tier operation that needs a not-yet-loaded dependency and runs under an I/O-free environment
    91	  returns `AttemptOutcome::NeedInputs(LoadSet)` (per `contracts/input-loading.md` §2, §4) instead of
    92	  panicking, blocking, or fabricating a partial answer — for **every** non-flow `ModuleResolverCore`/
    93	  `TypeInfoCore` operation reachable from a C2 projection attempt, not a subset.
    94	- `verter_semantic`'s production dependency closure contains neither `verter_workspace`,
    95	  `verter_session`, `verter_scheduler`, nor `verter_tsgo_api` on any target — this closes debt row A5-DD1
    96	  by *deleting* the exception, not widening it (`crates/verter_identity/tests/cases/
    97	  workspace_dependency_layers.rs:118-127`, the `ratified_upward_exceptions()` row keyed
    98	  `"verter_semantic"`).
    99	- `resolver_core`, `resolver_store`'s immutable observation types, `project_semantic_dispatch`, and
   100	  `ModuleResolverCore` (the relocated `verter_workspace::resolver::ProjectResolver`) physically live in
   101	  `verter_semantic`. `verter_compiler` can depend on the converged kernel directly — it already does
   102	  (`crates/verter_compiler/Cargo.toml:59`, `verter_semantic = { path = "../verter_semantic" }`) — with no
   103	  new edge and no cycle.
   104	- A new capability-limited, immutable **observation interface**, defined inside `verter_semantic`, does
   105	  **not** extend today's `ResolverContext` and cannot name, hold, or return `VerterHost` or the
   106	  scheduler — not because a marker says so, but because `verter_semantic`'s crate dependency closure
   107	  makes those types structurally unnameable from inside it.
   108	- The sealed `ResolverContext` trait (relocated to `verter_semantic`) has exactly as many
   109	  production-reachable implementors as there are genuinely distinct blocking lifecycles; a lifecycle that
   110	  cannot resolve (e.g. a bare host with no request/session binding) is a compile-time impossibility for
   111	  resolver-tier call sites, not a documented runtime `panic!`.
   112	
   113	**Forbidden observable outcomes.**
   114	- A second implementation of `SemanticQueryApi::execute`, `execute_relate`, `shallow_lower_type_expr`, a
   115	  second struct owning a `RelationMemo` or the semantic node map (the existing Authority-uniqueness
   116	  contract, `.claude/skills/type-resolution/SKILL.md:790-796` — C1 preserves this unchanged across the
   117	  crate move).
   118	- A converged kernel that reads live, un-snapshotted host state at validation time (defeats "immutable
   119	  observation views").
   120	- A lifecycle-specific answer: the same query returning a different resolved type, a different route, or
   121	  a different completeness verdict depending on which `ResolverContext`/observation-interface implementor
   122	  served it, for identical inputs.
   123	- A blocking wait introduced on the path a future I/O-free lifecycle uses to reach `NeedInputs` — the
   124	  point of the new outcome is that that path never calls `wait_or_drive`/`Condvar::wait`.
   125	- A marker type presented as *the* proof of I/O freedom. Per the ruling on Fork 3, a marker may
   126	  *classify* the observation interface; the proof is that the interface's home crate cannot name the
   127	  host/scheduler types at all. A subtrait of `ResolverContext` is never an acceptable substitute — it
   128	  inherits `ensure_loaded`/`ensure_indexed_ready_serve`/the host escape hatch
   129	  (`resolver_context.rs:780-782,261-266,700-717`) exactly as the rejected draft position did.
   130	- Any resolver-tier operation reachable from a C2 projection attempt that is left blocking-only, with no
   131	  `NeedInputs`-capable path — that is exactly the coverage gap Fork 4's ruling forecloses.
   132	
   133	**Authority/fallback order.** `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`
   134	remains the sole query-time resolver (`project_semantic_dispatch/mod.rs:2206-2214`, the shared choke
   135	point) — unchanged by the crate move. `ResolverContext`/observation-interface implementors are lifecycle
   136	*adapters* over that one authority, never alternate authorities. A resolver-tier operation that cannot
   137	prove a current, coherent view fails closed to a typed non-current miss (`StoreViewRead`'s
   138	`CurrentHostStoreView`/`ColdSeedHostStoreView` split, `.claude/skills/host-session/SKILL.md:678-696`) —
   139	C1 preserves this fail-closed order and must not weaken it while adding the `NeedInputs` outcome
   140	alongside it.
   141	
   142	**Acceptance IDs.**
   143	
   144	| ID | Requirement | Planned test/gate |
   145	|---|---|---|
   146	| C1-AC-1 | Same query, same content, different lifecycle ⇒ same resolved answer | New characterization suite driving one fixed corpus of queries through `HostResolverContext` and `SessionResolverContext` (and the new I/O-free observation interface once added), asserting structural equality of `SemanticNodeId` surfaces, sibling to `resolver_core/*_tests.rs` (relocated) |
   147	| C1-AC-2 | `verter_semantic`'s production closure excludes `verter_workspace`/`verter_session`/`verter_scheduler`/`verter_tsgo_api` on every target, with the A5-DD1 exception row **deleted**, not widened | `crates/verter_identity/tests/cases/workspace_dependency_layers.rs::workspace_production_closures_never_cross_upward_except_the_recorded_exception` — remove `"verter_semantic"` from `ratified_upward_exceptions()`; any surviving upward edge now hard-fails instead of matching an exception |
   148	| C1-AC-3 | Authority-uniqueness contract holds after the crate move | Existing `project_semantic_dispatch_invariants_tests.rs` + the five-row Authority-uniqueness contract stay green, unmodified in substance, relocated alongside the module they test |
   149	| C1-AC-4 | A resolver-tier call site cannot reach resolution without a request-bound context | `RequestBoundResolverContext` sealed marker (relocated to `verter_semantic`) becomes the *only* production-constructible path once the bare `impl ResolverContext for VerterHost` production rail is deleted (see Legacy Deletions) — `VerterHost` stays in `verter_session` and implements the now-foreign trait for a local type, which the orphan rules permit |
   150	| C1-AC-5 | `AttemptOutcome::{Complete, NeedInputs(LoadSet), Terminal}` covers **every** non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt — full coverage, not one load point | Discharged structurally, not by sampling. Per C1-AC-7 and the Authority/fallback order clause, the observation interface is the *only* surface an I/O-free (C2-shaped) caller can reach a non-flow `ModuleResolverCore`/`TypeInfoCore` operation through — `ResolverContext` itself can call `ensure_loaded`/`wait_or_drive`, so it is not usable I/O-free. "Every reachable operation" therefore reduces to "every method on this one finite, closed trait." The trait is defined so every method returns `AttemptOutcome<T>` at the type level (never a bare `T`, `Result<T, _>`, or a call that can block) — a non-conforming method is a compile error at authoring time, not a gap a test could miss. Gate: one exhaustive test double (`impl <ObservationInterface> for TestDouble`) that must implement every trait method to compile; a method added later that does not return `AttemptOutcome<T>` fails to compile at the double, not at a sampled runtime assertion. If a future operation cannot be expressed this way, that is itself a Fork-4-reopening discovery (see Abort/rescope), not a reason to fall back to sampling. |
   151	| C1-AC-6 | Duplicated lifecycle-adapter boilerplate collapses | Diff proof: the ~10 near-identical delegation methods listed under Legacy Deletions shrink to one shared implementation; `HostResolverContext`/`SessionResolverContext` (staying in `verter_session`, implementing the relocated trait) retain only the ~9 genuinely session-specific overrides |
   152	| C1-AC-7 | The observation interface does not extend `ResolverContext` and cannot be built holding a host/scheduler reference | Type-level: the interface is defined in `verter_semantic` with no `VerterHost`/scheduler type nameable in scope (proven by C1-AC-2's closure guard, not a separate scanner); a `trybuild`/compile-fail fixture asserting a `&VerterHost`-holding type does not satisfy the interface's bound |
   153	| C1-AC-8 | Route extraction takes an owned immutable snapshot, not a live `&dyn WorkspaceRead` | `analysis/routes.rs`'s extractors (`detect_routing_framework_from_json`, `extract_programmatic_routes`, and the four other call sites) take `&RouteAnalysisInputs`; zero `WorkspaceRead`-typed parameters remain anywhere in `verter_semantic`; existing route-extraction unit tests re-target the new snapshot type with unchanged assertions |
   154	| C1-AC-9 | `ModuleResolverCore` (the relocated `ProjectResolver`) has no direct scheduler/tsgo I/O left uncoverted | Any synchronous I/O call inside the relocated 2122-line resolver either does not exist in the moved code (pure computation) or is converted to the same `AttemptOutcome`/`LoadSet` pattern as `resolver_core`'s load points — audited as part of C1-AC-5's coverage sweep, not a separate carve-out |
   155	
   156	**Cold/warm/allocation/fan-out/latency bounds.** C1 is bound to the existing locked cell
   157	`A6_META_COMPILE_40_COLD_RUST` (`performance-gates.toml:125`), not a new cell: that cell already measures
   158	`session.semantic_dispatch.calls` (`performance-gates.toml:321`) and `session.semantic_cold_build.calls`
   159	(`performance-gates.toml:296`) — the two counters convergence work directly touches, since every
   160	`ResolverContext` call funnels to `dispatch()` (`host_resolver_context.rs:494-500`,
   161	`session_resolver_context.rs:674-680`) and every cold miss reaches the chokepoint at
   162	`project_semantic_dispatch/mod.rs:2214`. C1 must not move either counter adversely — the crate move must
   163	not itself introduce cross-crate call overhead beyond ordinary function-call cost (no new serialization,
   164	no new heap round-trip at the crate seam). Existing fan-out fuses (`resolver_core/fuses.rs:10-34`:
   165	`wildcard_route_fanout=500`, `imported_root_fanout=200`, `registry_deepening_fanout=300`,
   166	`member_surface_recursion_depth=10`, `projection_op_count=2000`, `union_member_explosion=100`) are budget
   167	*policy*, not owned by C1 — convergence must reuse them unchanged and must not introduce a second fuse
   168	table. Warm-hit validation stays the existing O(signature-length) `validate_fact_signature` loop
   169	(`resolver_core/mod.rs:409-428`) with zero new heap allocation per fact — a converged `ResolverContext`
   170	construction path must not add a clone, extra `Arc` construction, or normalization pass beyond what
   171	`HostResolverContext::new`/`SessionResolverContext::new` already do per request
   172	(`host_resolver_context.rs:96-149`, `session_resolver_context.rs:103-150`). The full-coverage
   173	`AttemptOutcome` threading (C1-AC-5) must not add allocation to the existing blocking lifecycles' hot
   174	path — a blocking `ResolverContext` call still resolves in one pass with no extra `LoadSet`
   175	materialization when nothing is missing. C1 may add a dedicated micro-cell only via the ADR-016
   176	new-lock-record path (mirroring `B1.md:169-171`); it may not reweight or reinterpret
   177	`A6_META_COMPILE_40_COLD_RUST` after measurement.
   178	
   179	## Current-tree convergence map
   180	
   180	
   181	| current authority | source | disposition | final owner |
   182	|---|---|---|---|
   183	| `resolver_core` / `ProjectSemanticDispatch` orchestration | `resolver_core/**` (59 files, incl. `component_meta/` (6) + `component_meta_query_engine/` (14), `verter_session`), `project_semantic_dispatch/mod.rs:309` (`verter_session`, `pub(crate)`) | Preserve resolver semantics; **physically relocate** into `verter_semantic` — **except** the three files named in the "Sealed lifecycle adapters" row below, which hold `&VerterHost` and cannot cross | `verter_semantic` (except the named adapter carve-outs) |
   184	| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | **Physically relocate** — this is the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | `verter_semantic` |
   185	| Re-export shim + two real functions | `crates/verter_semantic/src/analysis/project_resolver.rs` (94 lines, not a pure shim: `:1-30` re-exports `verter_workspace::resolver::*`/`verter_workspace::types::*`; `:32-90` defines `collect_resolvable_module_reference_specifiers`/`resolve_known_module_reference_dependencies`, real analysis-dependent logic with production callers at `verter_napi/src/lib.rs:2095,2117` and `verter_wasm/src/lib.rs:640,667`) | Delete only the `:1-30` re-export half — its target no longer exists at that path once `ModuleResolverCore` relocates into `verter_semantic` alongside it. The `:32-90` functions stay at this same module path unchanged; their napi/wasm callers keep calling `verter_semantic::analysis::project_resolver::*` with no repointing | re-export half deleted; functions retained in place |
   186	| Sealed lifecycle adapters | `resolver_core/resolver_context.rs:161` (trait), `:817-1343` (`impl ResolverContext for VerterHost`, plus `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed` impls), `resolver_core/host_resolver_context.rs:189`, `resolver_core/session_resolver_context.rs:183` | Trait + `sealed` module + collapsed shared boilerplate relocate with the kernel — `resolver_context.rs` is **split**, not moved whole. The two concrete adapter structs (`HostResolverContext`, `SessionResolverContext`) and the bare-host production rail (the `:817-1343` impl block) stay/are-deleted in `verter_session` (they hold `&VerterHost`, which cannot cross into `verter_semantic`) | trait + `sealed` module: `verter_semantic`; adapters + `VerterHost` impl: `verter_session` |
   187	| Immutable observation view | `crates/verter_session/src/resolver_store.rs:1462-1525` (`HostStoreView`), `:427-558` (`StoreViewValidationToken`) | Relocate the immutable, `Arc`-backed value types; leave the host-lifecycle-bound `StoreViewManager`/cache-retention machinery in `verter_session` | value types: `verter_semantic`; manager: `verter_session` |
   188	| Blocking cross-file load-on-demand | `host_lifecycle.rs:953` (`ensure_loaded`), `:1012` (`wait_or_drive`), `resolver_context.rs:913-914`, `host_resolver_context.rs:281-288`, `bare_name_resolve.rs:188-190` | Stays in `verter_session` (it needs `VerterHost`/the scheduler); the kernel-side call sites it's invoked from gain the full-coverage `AttemptOutcome` alternative instead | `verter_session` (blocking path) + `verter_semantic` (typed outcome) |
   189	| `verter_semantic → verter_workspace` edge (A5-DD1) | `crates/verter_semantic/Cargo.toml:27`, consumers at `analysis/project_resolver.rs`, `analysis/routes.rs:196,251,661,672,869,1120` (`&dyn WorkspaceRead`), `facts/registry.rs:3` | **Delete the Cargo.toml edge outright** — the module resolver that motivated it relocates into `verter_semantic` itself; `WorkspaceRead` stays up (Fork 2), fact vocabulary moves down | edge deleted |
   190	
   191	**Duplicated lifecycle-adapter boilerplate to collapse** (research-verified, not hypothesis, unaffected
   192	by the crate move other than the trait's new home): `is_request_bound` (`host_resolver_context.rs:193-195`
   193	vs `session_resolver_context.rs:187-189`), `request_completion_overlay` (`:221-224` vs `:234-237`),
   194	`store_view` (`:321-324` vs `:508-511`), `aggregate_basis_seed` (`:326-329` vs `:513-516`), `dispatch`
   195	(`:494-500` vs `:674-680`), `resolve_imported_type_root`/`_with_facts` (`:343-372` vs `:530-559`),
   196	`resolve_type_declaration_for_dep` (`:431-450` vs `:616-634`), plus the constructor trio's near-verbatim
   197	doc/rationale duplication (`:96-149` vs `:103-150`). Each pair is the same delegation shape over a
   198	different receiver (`self.inner` vs `ResolverContext::method(self.inner, ..)`) — a single shared default
   199	or a common inner-delegate helper removes the duplication without touching the genuinely session-specific
   200	overrides (`authoritative_current_content_hash`, `observe_materialize_scope`, `indexed_for_current_content`,
   201	`artifact_key_for_current_content`, `resolve_type_dependency_canonical`, `shallow_file_state`,
   202	`active_session_view`, `complete_canonical`/`complete_canonical_with_session_view` — these have no
   203	host-side analog and stay distinct; `session_resolver_context.rs:304-332,357-403,421-470,591-608,280-288,
   204	712-715,171-180`). These structs stay in `verter_session` after the crate move (see convergence map
   205	above); only the trait they implement relocates.
   206	
   207	## Batched `NeedInputs` contract — full coverage (Fork 4)
   208	
   209	C1 owns the **kernel-level** attempt outcome: a resolver-core operation run under an I/O-free environment
   210	returns `AttemptOutcome::{Complete(T), NeedInputs(LoadSet), Terminal(AttemptFailure)}` per
   211	`contracts/input-loading.md` §2, §4, instead of calling `ensure_loaded`/`ensure_indexed_ready_serve`
   212	synchronously. `LoadSet` is normalized/sorted/deduplicated; `NeedInputs` on an empty delta with no basis
   213	change is the typed `InputResolutionNoProgress` failure (§4.3-4.5 of that contract), never a silent retry
   214	loop. This capability does not exist anywhere in the tree today (`grep -rn "NeedInputs\|LoadSet" crates/`
   215	— zero hits) and is new work, not a refactor of an existing batching mechanism.
   216	
   217	**Coverage is full, per the Fork 4 ruling — not "module/import resolution only."** Every non-flow
   218	`ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt must support the
   219	outcome: module/import resolution, decl-body lowering, relation/inference queries driven through
   220	`execute_relate`'s plumbing (the algorithm itself stays `Preserve`; only the calling convention around it
   221	changes), and the member/JSDoc-hydration path that already reaches `ensure_indexed_ready_serve`
   222	(`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`) today. `contracts/input-loading.md:5-19`
   223	is unqualified across compiler/resolver/TypeInfo kernels, and ADR-011:19-28 requires each attempt to
   224	report the maximal sound missing-observation set — a partial cut would leave C2 unable to plan its
   225	complete semantic demand closure before projection (`contracts/compile-transaction.md:18,44-53`) for
   226	whichever operations were left out. Internally, `ensure_loaded`/`ensure_indexed_ready_serve` may still
   227	become the two central observation-or-demand choke points every covered operation funnels through — that
   228	is an implementation strategy, not a scope carve-out. Acceptance is never "one real load point exercised
   229	by a test harness"; it is every reachable operation covered.
   230	
   231	**C1 does not own** the outer staged transaction. `contracts/input-loading.md` §5: "A direct/project
   232	`CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns
   233	`NeedInputs`; the caller may rebuild/extend the environment and retry." That caller — the
   234	prepare/plan/project/emit loop, `CompilePlanToken`/`ProjectionPlanToken` anti-replay, and the
   235	load/commit/retry orchestration across *multiple* kernel attempts — is `C2` (`program.md:242-247`, ADR-011).
   236	C1's obligation ends at: every resolver-tier operation reachable from a projection attempt, one immutable
   237	snapshot, one typed outcome per attempt. C2's obligation starts at: drive repeated C1 attempts, own the
   238	snapshot-extend/retry loop, own the anti-replay tokens.
   239	
   240	The existing blocking behavior is **not deleted**: the host-backed LSP session and the session-overlay
   241	lifecycle are permitted, documented lifecycles that legitimately block a cooperating thread
   242	(`decl_body_memo.rs:21`, `store_view_manager_tests.rs:2399` — "in-flight work block cooperatively, never
   243	busy-spin"). C1 adds the alternative outcome, across the full operation surface, so a *new* I/O-free
   244	lifecycle can reuse the same resolution logic without being forced to block; it does not retrofit every
   245	existing call site to stop blocking — `HostResolverContext`/`SessionResolverContext` keep blocking by
   246	design.
   247	
   248	## Legacy deletions
   249	
   250	- **`resolver_core/**` minus three named carve-outs, `resolver_store`'s immutable value types, and
   251	  `project_semantic_dispatch`** physically move out of `crates/verter_session/src/` into
   252	  `crates/verter_semantic/src/`. This includes `component_meta/` and `component_meta_query_engine/` in
   253	  full (verified dependency-neutral — see Context §1). The carve-outs, which stay in `verter_session`
   254	  because they hold `&VerterHost` (per the "Sealed lifecycle adapters" convergence-map row): 
   255	  `resolver_core/host_resolver_context.rs`, `resolver_core/session_resolver_context.rs`, and the `impl
   256	  ResolverContext for VerterHost` block plus its `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed`
   257	  impls inside `resolver_core/resolver_context.rs` (`:817-1343` — deleted rather than kept if the bare-host
   258	  rail itself is deleted, see the bare-host bullet below). `resolver_context.rs`'s trait definition and
   259	  `sealed` module (everything outside `:817-1343`) relocate with the kernel — the file is **split across
   260	  two crates**, not moved whole. The `verter_session::lib.rs` module declarations for the relocating pieces
   261	  (`mod project_semantic_dispatch` at line 332, `pub mod resolver_core`/`mod resolver_store` at lines
   262	  341-344) are deleted and replaced with narrower declarations covering only the three staying carve-out
   263	  files; `verter_semantic::lib.rs` gains the equivalent declarations for everything else. This is a
   264	  relocation, not a rewrite, for the moved code — its behavior is unchanged; its crate is not.
   265	- **`crates/verter_workspace/src/resolver.rs`** (`ProjectResolver`, 2122 lines) relocates into
   266	  `verter_semantic` as the `ModuleResolverCore` target. `verter_workspace`'s module declaration for it is
   267	  deleted; any I/O the resolver performs directly is converted to the `AttemptOutcome`/`LoadSet` pattern
   268	  as part of C1-AC-9, not left as a silent exception to full coverage.
   269	- **The bare `impl ResolverContext for VerterHost`** (`resolver_context.rs:817`) production-reachable
   270	  method bodies — confirmed to be `panic!("Architectural violation...")` in production builds today
   271	  (`resolver_context.rs:826-841,853-873,882-902,950-996,1021-1039,1054-1072,1081-1098,1107-1124,1171-1188`),
   272	  live only under `#[cfg(any(test, feature = "test-support"))]`. `VerterHost` stays defined in
   273	  `verter_session`; `ResolverContext` becomes a foreign trait from `verter_session`'s perspective, which
   274	  Rust's orphan rules permit implementing for a local type. If no production call site needs the
   275	  bare-host rail once convergence lands (verify at implementation time — every currently-known production
   276	  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
   277	  `RequestBoundResolverContext` become the sole production-constructible rail. This turns "resolve
   278	  without a request-bound context" from a runtime panic into a compile error.
   279	- **`crates/verter_semantic/src/analysis/project_resolver.rs`** (94 lines, not a pure shim) — only the
   280	  `:1-30` re-export half (`pub use verter_workspace::resolver::{...}` / `verter_workspace::types::{...}`)
   281	  is deleted; its target no longer exists at that path once `ModuleResolverCore` relocates *into*
   282	  `verter_semantic` alongside it. The `:32-90` functions (`collect_resolvable_module_reference_specifiers`,
   283	  `resolve_known_module_reference_dependencies`) are real analysis-dependent logic, not shim, and stay at
   284	  this module path unchanged — their production callers (`verter_napi/src/lib.rs:2095,2117`,
   285	  `verter_wasm/src/lib.rs:640,667`) keep calling `verter_semantic::analysis::project_resolver::*` with no
   286	  repointing needed, since the module is not deleted, only its now-redundant re-export half.
   287	- **`crates/verter_semantic/src/facts/registry.rs:3`** (`pub use verter_workspace::fact_registry::*;`) —
   288	  the fact-key vocabulary (`FactKey`, `FactDomain`, `Fact`, etc., currently
   289	  `verter_workspace/src/fact_registry.rs`) is dependency-neutral value data, exactly the kind of type the
   290	  ruling's boundary assigns to `verter_semantic` ("dependency-neutral semantic store/value types"). It
   291	  moves into `verter_semantic` directly; the re-export is deleted, not left as a permanent alias.
   292	- **`crates/verter_semantic/src/analysis/routes.rs:196,251,661,672,869,1120`** (`workspace: &dyn
   293	  verter_workspace::WorkspaceRead`) — per Fork 2's ruling, `WorkspaceRead` does **not** relocate downward
   294	  (it also exposes live authoritative import resolution and dependency-graph authority, `traits.rs:214-
   295	  280,347-508`, which must stay a live capability, not a captured observation). Instead: the six call
   296	  sites are rewritten to take an owned immutable `RouteAnalysisInputs` snapshot; the orchestration that
   297	  currently calls `read_file`/`file_exists`/`is_dir`/`read_dir` to answer route-extraction questions moves
   298	  *upward* into `verter_workspace`/`verter_session`, which builds the snapshot and passes it down. The
   299	  existing pure extractors (`detect_routing_framework_from_json`, `extract_programmatic_routes`,
   300	  `routes.rs:208-233,266-289`) are unchanged in substance — only their input type changes.
   301	- **`crates/verter_identity/tests/cases/workspace_dependency_layers.rs:118-127`** —
   302	  `ratified_upward_exceptions()`'s `"verter_semantic"` row is deleted (not widened, not target-scoped
   303	  differently). The `"verter_diagnostics"` row is untouched — out of scope. This is the mechanical proof
   304	  that A5-DD1 closes for real: after this row is gone, *any* remaining `verter_semantic → verter_workspace`
   305	  edge fails the existing closure test, with no new guard authored.
   306	- **Any singleflight/condvar/mutex blocking-wait code path** that a converged, full-coverage
   307	  `AttemptOutcome::NeedInputs` caller would otherwise still be forced through — audit
   308	  `SingleflightGroup::run`/`run_retaining` (`resolver_core/mod.rs:2116-2214,2595-2639`),
   309	  `route_db_singleflight.rs:70-146`, and `prepared_decl.rs:35-38`'s `build_gate: parking_lot::Mutex<()>`.
   310	  Because coverage is now full (Fork 4), this audit is broader than the prior draft's narrower scope: every
   311	  blocking primitive reachable from a covered operation either stays confined to the blocking lifecycles
   312	  (`HostResolverContext`/`SessionResolverContext`, which still legitimately block) or gains a non-blocking
   313	  peek-and-decline path feeding `NeedInputs` instead of parking the calling thread.
   314	
   315	## Structural confinement
   316	
   317	Every invariant above is enforced structurally, matching the pattern the codebase already uses in this
   318	exact area — C1 extends that pattern, it does not invent scanner-based enforcement:
   319	
   320	- **The crate dependency firewall is the primary proof, not a marker.** Per the Fork 1/3 ruling, "prove
   321	  extractability without extracting" is not mechanically checkable — the existing structural graph guard
   322	  cannot see intra-crate direction (`docs/arch/refactor/rev11/evidence/A5/dependency-direction.md:
   323	  182-189`). Once the kernel physically lives in `verter_semantic`, the **existing, landed**
   324	  Cargo-metadata closure guard (`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`) is
   325	  sufficient and requires no new guard: `verter_semantic` simply cannot name `VerterHost`, the scheduler,
   326	  or `verter_tsgo_api` types, because its production dependency closure does not reach them. That is what
   327	  makes the new observation interface's non-blocking, host-free guarantee real — not a marker trait
   328	  layered on top of a context that could still physically hold a host reference.
   329	- **Sealed-trait lifecycle closure.** `ResolverContext: sealed::Sealed` (relocated with the trait) already
   330	  makes "some fourth, unregistered lifecycle adapter" a compile error. C1 preserves the seal and, if the
   331	  bare-host rail is deleted (see above), tightens `RequestBoundResolverContext` from "narrower than
   332	  `ResolverContext`" to "identical to it" — a resolver-tier function taking `&dyn ResolverContext` becomes
   333	  provably request-bound by construction, not by convention. The new observation interface is a *separate*
   334	  sealed trait, not a subtrait of `ResolverContext` — per the ruling, a subtrait inherits
   335	  `ensure_loaded`/the host escape hatch and proves nothing.
   336	- **`StoreView`/`HostStoreView` immutability is already structural**, not a convention: `HostStoreView`
   337	  has no `&VerterHost`/`Arc<Mutex<..>>` field, only `Arc`-shared by-value snapshot data; a future violation
   338	  (a live host reference smuggled into the struct) is a visible field addition a reviewer catches at the
   339	  type definition, not a runtime behavior to detect. This is one of the value types that relocates into
   340	  `verter_semantic` unchanged.
   341	- **The Authority-uniqueness contract** (`.claude/skills/type-resolution/SKILL.md:790-796`) is enforced
   342	  today partly by a `syn`-AST content scanner (`crates/verter_session/tests/cases/architecture_guards.rs:
   343	  3384` `mod resolver_context_seal`) and partly by grandfathered string-content assertions. Per `CLAUDE.md`'s
   344	  forward-only landed-scanner rule, these are **grandfathered, not a template**. Because the scanned file
   345	  physically moves to `verter_semantic`, this grandfathered scanner's target path must be updated to keep
   346	  scanning the relocated file — that is "extending the existing grandfathered scanner's checked
   347	  assertions" (permitted) tracking a genuine regression risk from the move, not authoring a new
   348	  standalone scanner (forbidden). C1 must not add a new name-keyed source scanner for any new invariant it
   349	  introduces (the deleted bare-host rail, the collapsed duplication, the new `AttemptOutcome`, the new
   350	  observation interface) — new confinement here is type-level and crate-boundary-level only.
   351	
   352	## Boundary against flow (D-track) and against C2
   353	
   354	**Flow exclusion.** C1 converges the relation/inference authority (`execute_relate`,
   355	`project_semantic_dispatch/relation.rs`) exactly as it stands today — `Preserve`, not `Converge`. It adds
   356	no flow-specific relation classifier, no binding-slot integration, no nominal-identity/unique-symbol
   357	comparability extension. Those are `D3`'s stated job: "Extend the already-shared relation authority with
   358	nominal identities including unique symbols and tri-state comparability" (`program.md:249-253`). C1's only
   359	obligation to the flow track is that the relation authority it converges (now physically in
   360	`verter_semantic`) remains the single authority `D1` integrates against behind its private non-production
   360	`verter_semantic`) remains the single authority `D1` integrates against behind its private non-production
   361	test boundary (`program.md:229`) and that `D3` later extends without needing a second relation/name
   362	authority (`program.md:253`). The full-coverage `AttemptOutcome` threading touches the *calling
   363	convention* around `execute_relate` (per Fork 4) but never the relation algorithm itself.
   364	
   365	**C2 boundary.** C2 owns the staged compile transaction: `prepare → plan → project → emit`,
   366	`CompilePlanToken`/`ProjectionPlanToken` anti-replay, `CompileFactsBatch`, the concrete sealed
   367	`CompileTypeInfo`, and the outer load/commit/retry loop over C1's `NeedInputs` outcome (ADR-011,
   368	`program.md:242-247`). C1 must not build any of that — no plan token, no fact batch, no staged
   369	prepare/plan/project/emit state machine. C1's deliverable to C2 is now concretely: a converged kernel
   370	physically resident in `verter_semantic` that (a) gives identical answers across lifecycles, (b) can be
   371	driven from an I/O-free observation interface and returns `NeedInputs` instead of blocking across its
   372	**full** operation surface, and (c) is constructible without pulling in `verter_scheduler`/
   373	`verter_tsgo_api`/LSP-facing machinery *by construction of the crate graph*, so C2's compiler-facing
   374	facade embeds `verter_semantic` the same way `verter_compiler` already does today
   375	(`crates/verter_compiler/Cargo.toml:59`) with no cycle risk.
   376	
   377	## Required exit
   378	
   379	Per `program.md:196`: **lifecycle differs; module/name/type/relation meaning does not.** Concretely: the
   380	converged kernel (`ModuleResolverCore`, non-flow `TypeInfoCore`, `ProjectSemanticDispatch`, relation/node
   381	algorithms, dependency-neutral value types, `AttemptOutcome`/`LoadSet`) lives in `verter_semantic`; the
   382	three (or more, once the I/O-free observation interface exists) lifecycle implementors give bit-identical
   383	resolved answers for identical inputs; the duplicated boilerplate between `HostResolverContext` and
   384	`SessionResolverContext` is gone; `verter_semantic`'s production closure excludes `verter_workspace`,
   385	`verter_session`, `verter_scheduler`, and `verter_tsgo_api` on every target with the A5-DD1 exception row
   386	deleted, not recorded as still-permitted; `AttemptOutcome::NeedInputs(LoadSet)` exists and covers every
   387	non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt, exercised
   388	by an I/O-free test harness that never touches the scheduler; the Authority-uniqueness contract's five
   389	rows still hold; no `TypeExpr`-general transit, flow semantics, or C2-owned staged-transaction machinery
   390	was introduced.
   391	
   392	## Review
   393	
   394	Foundational: three mandates, three independent contexts, one candidate SHA and tree
   395	(`governance.md` §1, mirroring `B1.md:173-186`).
   396	
   397	| mandate | scope |
   398	|---|---|
   399	| conformance | charter, diff, and the deletion set — including whether every listed relocation actually happened (not a re-export left behind), whether the bare-host `ResolverContext` rail was actually deleted (or, if retained, why a production call site still needs it), and whether A5-DD1 closed by exception-row deletion rather than a subset-checked allowance |
   400	| architecture | diff plus the crate-boundary outcome — specifically whether `verter_semantic`'s new contents satisfy ADR-015's inward dependency direction with zero exception, whether the observation interface genuinely cannot reach `VerterHost`/the scheduler (not just "does not currently"), and whether any duplication-collapse accidentally created a new second authority instead of removing one |
   401	| adversarial performance/memory | diff plus `A6_META_COMPILE_40_COLD_RUST` raw output — specifically whether `session.semantic_dispatch.calls`/`session.semantic_cold_build.calls` regressed, whether the crate-seam move added call/serialization overhead, and whether the full-coverage `AttemptOutcome` path adds allocation on the existing blocking lifecycles' hot path |
   402	
   403	## Abort/rescope
   404	
   405	Stop for: a discovered fourth production lifecycle this charter did not account for; evidence that
   406	`verter_workspace::resolver::ProjectResolver` is not, in fact, cleanly separable from `verter_workspace`'s
   407	scheduler-integrated file-loading responsibilities in a way full-coverage `AttemptOutcome` conversion
   408	cannot resolve (i.e. A5-DD1 cannot close without also restructuring `F1`'s future committed-input-authority
   409	scope); a discovered second query-time resolution path this research did not find; or a locked-cell
   410	regression on `A6_META_COMPILE_40_COLD_RUST` that convergence cannot explain and correct within scope. A
   411	discovery at this bar reopens the ruling itself (a second architecture challenge), not a quiet local
   412	substitution for one of the four already-decided positions.
   413	
   414	## Rulings applied
   415	
   416	Binding ruling: `ARCH-RULING-C1-FOUR-FORKS.md` (Codex xhigh architecture challenge against the prior
   417	charter draft, tip `339c06989`). All four proposed positions in that draft were rejected:
   418	
   419	1. **Fork 1 (crate placement) — VIOLATES.** "Prove extractability without extracting" is not mechanically
   420	   enforceable (the structural graph guard cannot see intra-crate direction); a `pub(crate)` facade is
   421	   unusable by C2, and a public one would need `verter_compiler → verter_session` while session already
   422	   depends on compiler — a cycle. Ruling: **EXTRACT-NOW into the existing `verter_semantic` crate**, not a
   423	   new crate.
   424	2. **Fork 2 (`WorkspaceRead`'s home) — VIOLATES.** Relocating the whole trait downward would drag live
   425	   authoritative import resolution and dependency-graph authority into the kernel — resolution semantics,
   426	   not a captured observation. Ruling: move the workspace-capture entry points **upward**; pass an owned
   427	   immutable `RouteAnalysisInputs` snapshot **downward** into the existing pure extractors. This does not
   428	   foreclose `F1`'s future committed-input-authority design; relocating the trait would have.
   429	3. **Fork 3 (non-blocking guarantee) — VIOLATES.** The proposed `IoFreeResolverContext` marker, as a
   430	   subtrait of `ResolverContext`, inherits `ensure_loaded`/the host escape hatch — the draft's claim that
   431	   such a bound "does not expose" it was factually wrong. Ruling: extraction and I/O confinement are **one
   432	   decision**. Define a capability-limited observation interface that does not extend `ResolverContext`
   433	   and cannot return a host; the crate firewall (from ruling 1) is what makes the host unnameable, not the
   434	   marker by itself.
   435	4. **Fork 4 (coverage scope) — VIOLATES.** "Module/import resolution only" was never a clean cut — TypeInfo
   436	   projection already reaches `ensure_indexed_ready_serve`. `contracts/input-loading.md` is unqualified,
   437	   and ADR-011 requires the maximal sound missing-observation set per attempt. Ruling:
   438	   **FULL-COVERAGE-REQUIRED** for every non-flow `ModuleResolverCore`/`TypeInfoCore` operation reachable
   439	   from a C2 projection attempt; `ensure_loaded`/`ensure_indexed_ready_serve` may become internal choke
   440	   points, but acceptance is never "one real load point."
   441	
   442	**New open questions created by these rulings: none.** All four forks resolved to a specific, actionable
   443	position with a named target (crate, boundary lines, interface shape, coverage scope); nothing here
   444	requires a further architecture challenge before implementation. Two items are implementation-time
   445	judgment calls *within* the ruling's already-decided boundary, not further forks: (a) the exact intra-crate
   446	module layout for the relocated `resolver_core`/`resolver_store`/`project_semantic_dispatch` trees inside
   447	`verter_semantic`, and (b) the precise split of `resolver_store` between the immutable value types that
   448	move down and the `StoreViewManager`/cache-retention machinery that stays in `verter_session` — both are
   449	already fully constrained by the ruling's boundary list (verter_semantic gets "immutable observation
   450	contracts"; verter_session keeps "committed-store implementations... cache-retention policy") and do not
   451	need a ruling to resolve.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/charters/C1.md | sed -n '176,246p'; wc -l <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/rulings/ARCH-RULING-C1-FOUR-FORKS.md; nl -ba <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/rulings/ARCH-RULING-C1-FOUR-FORKS.md | sed -n '1,260p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 16909ms:[0m
   176	new-lock-record path (mirroring `B1.md:169-171`); it may not reweight or reinterpret
   177	`A6_META_COMPILE_40_COLD_RUST` after measurement.
   178	
   179	## Current-tree convergence map
   180	
   181	| current authority | source | disposition | final owner |
   182	|---|---|---|---|
   183	| `resolver_core` / `ProjectSemanticDispatch` orchestration | `resolver_core/**` (59 files, incl. `component_meta/` (6) + `component_meta_query_engine/` (14), `verter_session`), `project_semantic_dispatch/mod.rs:309` (`verter_session`, `pub(crate)`) | Preserve resolver semantics; **physically relocate** into `verter_semantic` — **except** the three files named in the "Sealed lifecycle adapters" row below, which hold `&VerterHost` and cannot cross | `verter_semantic` (except the named adapter carve-outs) |
   184	| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | **Physically relocate** — this is the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | `verter_semantic` |
   185	| Re-export shim + two real functions | `crates/verter_semantic/src/analysis/project_resolver.rs` (94 lines, not a pure shim: `:1-30` re-exports `verter_workspace::resolver::*`/`verter_workspace::types::*`; `:32-90` defines `collect_resolvable_module_reference_specifiers`/`resolve_known_module_reference_dependencies`, real analysis-dependent logic with production callers at `verter_napi/src/lib.rs:2095,2117` and `verter_wasm/src/lib.rs:640,667`) | Delete only the `:1-30` re-export half — its target no longer exists at that path once `ModuleResolverCore` relocates into `verter_semantic` alongside it. The `:32-90` functions stay at this same module path unchanged; their napi/wasm callers keep calling `verter_semantic::analysis::project_resolver::*` with no repointing | re-export half deleted; functions retained in place |
   186	| Sealed lifecycle adapters | `resolver_core/resolver_context.rs:161` (trait), `:817-1343` (`impl ResolverContext for VerterHost`, plus `VerterHost`-specific `sealed::Sealed`/`RequestBoundSealed` impls), `resolver_core/host_resolver_context.rs:189`, `resolver_core/session_resolver_context.rs:183` | Trait + `sealed` module + collapsed shared boilerplate relocate with the kernel — `resolver_context.rs` is **split**, not moved whole. The two concrete adapter structs (`HostResolverContext`, `SessionResolverContext`) and the bare-host production rail (the `:817-1343` impl block) stay/are-deleted in `verter_session` (they hold `&VerterHost`, which cannot cross into `verter_semantic`) | trait + `sealed` module: `verter_semantic`; adapters + `VerterHost` impl: `verter_session` |
   187	| Immutable observation view | `crates/verter_session/src/resolver_store.rs:1462-1525` (`HostStoreView`), `:427-558` (`StoreViewValidationToken`) | Relocate the immutable, `Arc`-backed value types; leave the host-lifecycle-bound `StoreViewManager`/cache-retention machinery in `verter_session` | value types: `verter_semantic`; manager: `verter_session` |
   188	| Blocking cross-file load-on-demand | `host_lifecycle.rs:953` (`ensure_loaded`), `:1012` (`wait_or_drive`), `resolver_context.rs:913-914`, `host_resolver_context.rs:281-288`, `bare_name_resolve.rs:188-190` | Stays in `verter_session` (it needs `VerterHost`/the scheduler); the kernel-side call sites it's invoked from gain the full-coverage `AttemptOutcome` alternative instead | `verter_session` (blocking path) + `verter_semantic` (typed outcome) |
   189	| `verter_semantic → verter_workspace` edge (A5-DD1) | `crates/verter_semantic/Cargo.toml:27`, consumers at `analysis/project_resolver.rs`, `analysis/routes.rs:196,251,661,672,869,1120` (`&dyn WorkspaceRead`), `facts/registry.rs:3` | **Delete the Cargo.toml edge outright** — the module resolver that motivated it relocates into `verter_semantic` itself; `WorkspaceRead` stays up (Fork 2), fact vocabulary moves down | edge deleted |
   190	
   191	**Duplicated lifecycle-adapter boilerplate to collapse** (research-verified, not hypothesis, unaffected
   192	by the crate move other than the trait's new home): `is_request_bound` (`host_resolver_context.rs:193-195`
   193	vs `session_resolver_context.rs:187-189`), `request_completion_overlay` (`:221-224` vs `:234-237`),
   194	`store_view` (`:321-324` vs `:508-511`), `aggregate_basis_seed` (`:326-329` vs `:513-516`), `dispatch`
   195	(`:494-500` vs `:674-680`), `resolve_imported_type_root`/`_with_facts` (`:343-372` vs `:530-559`),
   196	`resolve_type_declaration_for_dep` (`:431-450` vs `:616-634`), plus the constructor trio's near-verbatim
   197	doc/rationale duplication (`:96-149` vs `:103-150`). Each pair is the same delegation shape over a
   198	different receiver (`self.inner` vs `ResolverContext::method(self.inner, ..)`) — a single shared default
   199	or a common inner-delegate helper removes the duplication without touching the genuinely session-specific
   200	overrides (`authoritative_current_content_hash`, `observe_materialize_scope`, `indexed_for_current_content`,
   201	`artifact_key_for_current_content`, `resolve_type_dependency_canonical`, `shallow_file_state`,
   202	`active_session_view`, `complete_canonical`/`complete_canonical_with_session_view` — these have no
   203	host-side analog and stay distinct; `session_resolver_context.rs:304-332,357-403,421-470,591-608,280-288,
   204	712-715,171-180`). These structs stay in `verter_session` after the crate move (see convergence map
   205	above); only the trait they implement relocates.
   206	
   207	## Batched `NeedInputs` contract — full coverage (Fork 4)
   208	
   209	C1 owns the **kernel-level** attempt outcome: a resolver-core operation run under an I/O-free environment
   210	returns `AttemptOutcome::{Complete(T), NeedInputs(LoadSet), Terminal(AttemptFailure)}` per
   211	`contracts/input-loading.md` §2, §4, instead of calling `ensure_loaded`/`ensure_indexed_ready_serve`
   212	synchronously. `LoadSet` is normalized/sorted/deduplicated; `NeedInputs` on an empty delta with no basis
   213	change is the typed `InputResolutionNoProgress` failure (§4.3-4.5 of that contract), never a silent retry
   214	loop. This capability does not exist anywhere in the tree today (`grep -rn "NeedInputs\|LoadSet" crates/`
   215	— zero hits) and is new work, not a refactor of an existing batching mechanism.
   216	
   217	**Coverage is full, per the Fork 4 ruling — not "module/import resolution only."** Every non-flow
   218	`ModuleResolverCore`/`TypeInfoCore` operation reachable from a C2 projection attempt must support the
   219	outcome: module/import resolution, decl-body lowering, relation/inference queries driven through
   220	`execute_relate`'s plumbing (the algorithm itself stays `Preserve`; only the calling convention around it
   221	changes), and the member/JSDoc-hydration path that already reaches `ensure_indexed_ready_serve`
   222	(`crates/verter_session/src/typeinfo/shallow_surface.rs:179-193`) today. `contracts/input-loading.md:5-19`
   223	is unqualified across compiler/resolver/TypeInfo kernels, and ADR-011:19-28 requires each attempt to
   224	report the maximal sound missing-observation set — a partial cut would leave C2 unable to plan its
   225	complete semantic demand closure before projection (`contracts/compile-transaction.md:18,44-53`) for
   226	whichever operations were left out. Internally, `ensure_loaded`/`ensure_indexed_ready_serve` may still
   227	become the two central observation-or-demand choke points every covered operation funnels through — that
   228	is an implementation strategy, not a scope carve-out. Acceptance is never "one real load point exercised
   229	by a test harness"; it is every reachable operation covered.
   230	
   231	**C1 does not own** the outer staged transaction. `contracts/input-loading.md` §5: "A direct/project
   232	`CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns
   233	`NeedInputs`; the caller may rebuild/extend the environment and retry." That caller — the
   234	prepare/plan/project/emit loop, `CompilePlanToken`/`ProjectionPlanToken` anti-replay, and the
   235	load/commit/retry orchestration across *multiple* kernel attempts — is `C2` (`program.md:242-247`, ADR-011).
   236	C1's obligation ends at: every resolver-tier operation reachable from a projection attempt, one immutable
   237	snapshot, one typed outcome per attempt. C2's obligation starts at: drive repeated C1 attempts, own the
   238	snapshot-extend/retry loop, own the anti-replay tokens.
   239	
   240	The existing blocking behavior is **not deleted**: the host-backed LSP session and the session-overlay
   241	lifecycle are permitted, documented lifecycles that legitimately block a cooperating thread
   242	(`decl_body_memo.rs:21`, `store_view_manager_tests.rs:2399` — "in-flight work block cooperatively, never
   243	busy-spin"). C1 adds the alternative outcome, across the full operation surface, so a *new* I/O-free
   244	lifecycle can reuse the same resolution logic without being forced to block; it does not retrofit every
   245	existing call site to stop blocking — `HostResolverContext`/`SessionResolverContext` keep blocking by
   246	design.
   14472 <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/rulings/ARCH-RULING-C1-FOUR-FORKS.md
     1	Reading prompt from stdin...
     2	OpenAI Codex v0.148.0
     3	--------
     4	workdir: <MACHINE_ROOT>/verter
     5	model: gpt-5.6-sol
     6	provider: openai
     7	approval: never
     8	sandbox: read-only
     9	reasoning effort: xhigh
    10	reasoning summaries: none
    11	session id: 01a01cbf-a9a5-73b2-aced-080ee25e38c3
    12	--------
    13	user
    14	Architecture challenge, read-only, on <MACHINE_ROOT>/verter (branch
    15	program/architecture-lock, tip 339c06989). This is a pre-implementation challenge for Revision 11 block
    16	C1 ("Converge `ModuleResolverCore` and non-flow `TypeInfoCore`", program.md:191-197), which is the
    17	heaviest fan-in node left in the DAG — C2, D3, D7 and two others depend on it.
    18	
    19	A charter draft exists at
    20	<SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/C1-CHARTER-DRAFT.md
    21	Read it first. Its author honestly flagged four unresolved design forks rather than guessing. Your job is
    22	to KILL OR CONFIRM a specific proposed position on each — falsification, not fresh design. For each,
    23	answer VIOLATES (name the invariant and cite file:line) or DOES-NOT-VIOLATE, then give the ruling.
    24	
    25	FORK 1 — crate placement. `resolver_core`/`resolver_store` are private modules inside `verter_session`
    26	(lib.rs:341-344). architecture.md §18.1 puts the semantic kernel/module resolver BELOW compiler and far
    27	below managed engine/session; today's placement inverts that.
    28	  PROPOSED POSITION: C1 does NOT physically extract a new crate. It converges the kernel IN PLACE behind
    29	  an explicit, sealed member boundary (a single `pub(crate)` facade module enumerating exactly what the
    30	  kernel exposes), and its acceptance requires PROVING extraction is possible — i.e. zero references from
    31	  kernel modules to `VerterHost`, the scheduler, or anything above the boundary, enforced structurally.
    32	  Physical extraction is staged to a later block.
    33	  Falsify: does deferring physical extraction let C2 (which must embed a concrete `CompileTypeInfo` facade)
    34	  land on a structure it will have to unwind? Is "prove extractability without extracting" mechanically
    35	  checkable, or is it the kind of promise that rots? If the honest answer is that C1 must extract now,
    36	  say EXTRACT-NOW and name the crate and its exact member boundary.
    37	
    38	FORK 2 — A5-DD1's harder half. `verter_semantic/src/analysis/routes.rs` takes
    39	`&dyn verter_workspace::WorkspaceRead` (6 call sites) — a real functional dependency, not a re-export.
    40	  PROPOSED POSITION: relocate the `WorkspaceRead` TRAIT (not its implementations) down to a neutral layer
    41	  both crates already depend on, leaving route extraction where it is.
    42	  Falsify against the alternative (move route extraction up to where `WorkspaceRead` lives) and against
    43	  F1's not-yet-written committed-input-authority design. Which is correct, and does either foreclose F1?
    44	
    45	FORK 3 — non-blocking guarantee. C1 must return batched `NeedInputs` instead of blocking on I/O.
    46	  PROPOSED POSITION: enforce structurally with a sealed `IoFreeResolverContext` marker (mirroring the
    47	  existing `RequestBoundResolverContext`), so a resolver-tier function generic over it cannot reach
    48	  `ensure_loaded` or the scheduler even by accident.
    49	  Falsify: is the marker actually unforgeable given how contexts are constructed today? Does it work if
    50	  FORK 1 is decided as "converge in place" (the I/O-free context would still live inside `verter_session`
    51	  and could physically hold a `&VerterHost`)? If the marker only works AFTER extraction, say so — that
    52	  makes FORK 1 and FORK 3 one decision, not two.
    53	
    54	FORK 4 — coverage scope. program.md:196 says "return batched `NeedInputs`" without qualifying which
    55	operations.
    56	  PROPOSED POSITION: first cut covers only the load points reached through
    57	  `ensure_loaded`/`ensure_indexed_ready_serve`; full type-query coverage deferred.
    58	  Falsify: does C2's staged compile transaction (prepare/plan/project/emit with anti-replay plan/fact
    59	  tokens, program.md:199-205) actually require full coverage on day one? If a partial cut leaves C2 unable
    60	  to meet its own exit criterion, say FULL-COVERAGE-REQUIRED.
    61	
    62	Rules for your answer:
    63	- The program forbids name-keyed source-tree scanners as LANDED enforcement; structural confinement only
    64	  (CLAUDE.md, "Landed guards are structural"). Do not propose a scanner.
    65	- Breadth, breakage, migration size and effort are NOT valid reasons to weaken a design (CLAUDE.md,
    66	  "Planning"). If the correct answer is a large breaking change, say so.
    67	- Where the charter draft's research is simply WRONG about the code, say that plainly and cite the file.
    68	- Be concise. file:line throughout. End with four verdict lines, one per fork.
    69	
    70	codex
    71	I’ll treat this as an architecture falsification review, not a redesign. I’ll read the draft and the canonical architecture/type-resolution/session references, then trace the cited APIs and C2/F1 dependencies to ground each verdict in exact lines.
    72	exec
    73	/bin/zsh -lc "pwd && git branch --show-current && git rev-parse HEAD && wc -l CLAUDE.md .claude/skills/architecture/SKILL.md .claude/skills/type-resolution/SKILL.md .claude/skills/host-session/SKILL.md <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/C1-CHARTER-DRAFT.md && sed -n '1,260p' <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/C1-CHARTER-DRAFT.md" in <MACHINE_ROOT>/verter
    74	 succeeded in 0ms:
    75	<MACHINE_ROOT>/verter
    76	program/architecture-lock
    77	339c06989cee9e47a5afdac8e4ae8a9d2ad97805
    78	     659 CLAUDE.md
    79	     230 .claude/skills/architecture/SKILL.md
    80	    1342 .claude/skills/type-resolution/SKILL.md
    81	     738 .claude/skills/host-session/SKILL.md
    82	     314 <SESSION_SCRATCH>/-Users-carlosrodrigues-Documents-dev-verter/cad766b2-94b4-4208-a25d-0fed78cc40e6/scratchpad/C1-CHARTER-DRAFT.md
    83	    3283 total
    84	# C1 — Converge `ModuleResolverCore` and non-flow `TypeInfoCore`
    85	
    86	**Status:** DRAFT — authored for maintainer review; no AMD ratifies it yet. **Class:** Foundational
    87	(`program-dag.toml:162`). **Predecessors:** `A6`, `B1`, `B2` (`program-dag.toml:163`).
    88	
    89	C1 converges the current host-backed module/type-resolution stack — `verter_session::resolver_core`,
    90	`verter_session::resolver_store`, `ProjectSemanticDispatch`, and the TypeScript-style resolver living in
    91	`verter_workspace::resolver` — into the two target logical owners named in `architecture.md` §8.1: a
    92	`ModuleResolverCore` that owns TypeScript-style module/path/package resolution, and a non-flow
    93	`TypeInfoCore` that owns authored-node traversal, binder/name/reference semantics, substitution,
    94	relation/inference, effective typing, recursion/work budgets, query-specific projection, and
    95	exactness/completeness propagation. Neither target type exists in code today; both names describe roles
    96	the current tree already plays under different names and in the wrong crate. C1 does not invent new
    97	resolution semantics — every module/name/type/relation answer the converged kernel gives must be
    98	identical to today's answer for every existing lifecycle. It changes who owns the code, how many
    99	duplicate implementations of the same lifecycle-adapter shape exist, whether the kernel can perform I/O
   100	directly, and where the crate boundary sits.
   101	
   102	C1 does not own flow semantics (`D1`–`D8`), Vue/Svelte macro projection (`C3`), the staged
   103	prepare/plan/project/emit compile transaction or its anti-replay tokens (`C2`), or component-meta
   104	publication policy (`/component-meta`). It extends nothing about `execute_relate`'s relation semantics or
   105	`ProjectSemanticDispatch`'s five query modes — those are `Preserve`, not `Converge`, dispositions. It owns
   106	convergence of context/lifecycle plumbing, dependency direction, and the addition of a batched,
   107	resumable load-outcome so a future I/O-free lifecycle can consume the same kernel a blocking lifecycle
   108	consumes today.
   109	
   110	## Intent Contract
   111	
   112	**Actor / problem.** Every current and future first-party consumer of Verter's type/module resolution —
   113	the host-backed LSP session, the session-overlay (unsaved-edit) path, and the future project-aware direct
   114	compiler (`C2`'s sealed `CompileTypeInfo`) — must resolve a symbol, import, or type to the *same* answer.
   115	Today that guarantee is held by convention (three lifecycle adapters hand-implement the same sealed
   116	`ResolverContext` trait with large amounts of copy-pasted delegation) and by omission (no lifecycle exists
   117	yet that cannot block a thread on I/O), not by a structural authority that forces convergence and permits
   118	a new I/O-free lifecycle to be added without duplicating resolution logic a fourth time.
   119	
   120	**Required observable outcomes.**
   121	- A module/name/type/relation query issued through `HostResolverContext`, `SessionResolverContext`, or
   122	  (once added) an I/O-free project-capable context returns bit-identical `SemanticNodeId`/resolved-surface
   123	  results for the same source content, project configuration, and query, modulo the outcome shape
   124	  (blocking-complete vs. `NeedInputs`).
   125	- A resolver-tier operation that needs a not-yet-loaded dependency and runs under an I/O-free environment
   126	  returns `AttemptOutcome::NeedInputs(LoadSet)` (per `contracts/input-loading.md` §2, §4) instead of
   127	  panicking, blocking, or fabricating a partial answer.
   128	- `verter_semantic`'s production dependency closure contains neither `verter_scheduler` nor
   129	  `verter_tsgo_api` on any target (closes debt row A5-DD1, `evidence/A5/dependency-direction.md:158-176`).
   130	- The sealed `ResolverContext` trait (`resolver_core/resolver_context.rs:161`) has exactly as many
   131	  production-reachable implementors as there are genuinely distinct lifecycles; a lifecycle that cannot
   132	  resolve (e.g. a bare host with no request/session binding) is a compile-time impossibility for
   133	  resolver-tier call sites, not a documented runtime `panic!`.
   134	
   135	**Forbidden observable outcomes.**
   136	- A second implementation of `SemanticQueryApi::execute`, `execute_relate`, `shallow_lower_type_expr`, a
   137	  second struct owning a `RelationMemo` or the semantic node map (the existing Authority-uniqueness
   138	  contract, `.claude/skills/type-resolution/SKILL.md:826-832` — C1 preserves this unchanged).
   139	- A converged kernel that reads live, un-snapshotted host state at validation time (defeats "immutable
   140	  observation views").
   141	- A lifecycle-specific answer: the same query returning a different resolved type, a different route, or
   142	  a different completeness verdict depending on which `ResolverContext` implementor served it, for
   143	  identical inputs.
   144	- A blocking wait introduced on the path a future I/O-free lifecycle uses to reach `NeedInputs` — the
   145	  point of the new outcome is that that path never calls `wait_or_drive`/`Condvar::wait`.
   146	
   147	**Authority/fallback order.** `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`
   148	remains the sole query-time resolver (`project_semantic_dispatch/mod.rs:2206-2214`, the shared choke
   149	point). `ResolverContext` implementors are lifecycle *adapters* over that one authority, never alternate
   150	authorities. A resolver-tier operation that cannot prove a current, coherent view fails closed to a typed
   151	non-current miss (`StoreViewRead`'s `CurrentHostStoreView`/`ColdSeedHostStoreView` split,
   152	`.claude/skills/host-session/SKILL.md:678-696`) — C1 preserves this fail-closed order and must not weaken
   153	it while adding the `NeedInputs` outcome alongside it.
   154	
   155	**Acceptance IDs.**
   156	
   157	| ID | Requirement | Planned test/gate |
   158	|---|---|---|
   159	| C1-AC-1 | Same query, same content, different lifecycle ⇒ same resolved answer | New characterization suite driving one fixed corpus of queries through `HostResolverContext` and `SessionResolverContext` (and the new I/O-free context once added), asserting structural equality of `SemanticNodeId` surfaces, sibling to `resolver_core/*_tests.rs` |
   160	| C1-AC-2 | `verter_semantic` closure excludes `verter_scheduler`/`verter_tsgo_api` on every target | The equality-pinned exception B1 lands names `C1` as removal gate (`B1.md:90-109`); C1 tightens B1's closure test from exception-permitted to hard-fail for this pair |
   161	| C1-AC-3 | Authority-uniqueness contract holds after convergence | Existing `project_semantic_dispatch_invariants_tests.rs` + the five-row Authority-uniqueness contract stay green, unmodified in substance |
   162	| C1-AC-4 | A resolver-tier call site cannot reach resolution without a request-bound context | `RequestBoundResolverContext` sealed marker (`resolver_context.rs:780-783`) becomes the *only* production-constructible path if the bare `impl ResolverContext for VerterHost` production rail is deleted (see Legacy Deletions) |
   163	| C1-AC-5 | `NeedInputs(LoadSet)` is real for at least one resolver-tier load point | New typed `AttemptOutcome` wraps the `ensure_loaded`/`ensure_indexed_ready_serve` mid-resolution call sites; existing blocking behavior stays available for the host/session lifecycles, and the new outcome is exercised by an I/O-free test harness that never touches the scheduler |
   164	| C1-AC-6 | Duplicated lifecycle-adapter boilerplate collapses | Diff proof: the ~10 near-identical delegation methods listed under Changes shrink to one shared implementation; `HostResolverContext`/`SessionResolverContext` retain only the ~9 genuinely session-specific overrides |
   165	
   166	**Cold/warm/allocation/fan-out/latency bounds.** C1 is bound to the existing locked cell
   167	`A6_META_COMPILE_40_COLD_RUST` (`performance-gates.toml:125`), not a new cell: that cell already measures
   168	`session.semantic_dispatch.calls` (`performance-gates.toml:321`) and `session.semantic_cold_build.calls`
   169	(`performance-gates.toml:296`) — the two counters convergence work directly touches, since every
   170	`ResolverContext` call funnels to `dispatch()` (`host_resolver_context.rs:494-500`,
   171	`session_resolver_context.rs:674-680`) and every cold miss reaches the chokepoint at
   172	`project_semantic_dispatch/mod.rs:2214`. C1 must not move either counter adversely. Existing fan-out fuses
   173	(`resolver_core/fuses.rs:10-34`: `wildcard_route_fanout=500`, `imported_root_fanout=200`,
   174	`registry_deepening_fanout=300`, `member_surface_recursion_depth=10`, `projection_op_count=2000`,
   175	`union_member_explosion=100`) are budget *policy*, not owned by C1 — convergence must reuse them unchanged
   176	and must not introduce a second fuse table. Warm-hit validation stays the existing O(signature-length)
   177	`validate_fact_signature` loop (`resolver_core/mod.rs:409-428`) with zero new heap allocation per fact — a
   178	converged `ResolverContext` construction path must not add a clone, extra `Arc` construction, or
   179	normalization pass beyond what `HostResolverContext::new`/`SessionResolverContext::new` already do per
   180	request (`host_resolver_context.rs:96-149`, `session_resolver_context.rs:103-150`). C1 may add a
   181	dedicated micro-cell only via the ADR-016 new-lock-record path (mirroring `B1.md:169-171`); it may not
   182	reweight or reinterpret `A6_META_COMPILE_40_COLD_RUST` after measurement.
   183	
   184	## Current-tree convergence map
   185	
   186	| current authority | source | disposition | final owner |
   187	|---|---|---|---|
   188	| `resolver_core` / `ProjectSemanticDispatch` orchestration | `resolver_core/**` (37 files), `project_semantic_dispatch/mod.rs:309` | Preserve resolver semantics; Converge context/lifecycle plumbing and crate placement | C1 (`evidence/A5/owner-rows.md` Row 3) |
   189	| TypeScript-style module/path/package resolution | `crates/verter_workspace/src/resolver.rs:122` (`ProjectResolver`, 2122 lines) | Converge — physically the `ModuleResolverCore` target, wrongly homed in the scheduler/tsgo-dependent `verter_workspace` crate | C1 |
   190	| Re-export compatibility shim | `crates/verter_semantic/src/analysis/project_resolver.rs:1-30` | Delete once consumers repoint to the true `ModuleResolverCore` home | C1 |
   191	| Sealed lifecycle adapters | `resolver_core/resolver_context.rs:161` (trait), `:817` (`VerterHost`), `resolver_core/host_resolver_context.rs:189`, `resolver_core/session_resolver_context.rs:183` | Converge — collapse shared boilerplate, delete the bare-host production rail | C1 |
   192	| Immutable observation view | `crates/verter_session/src/resolver_store.rs:1462-1525` (`HostStoreView`), `:427-558` (`StoreViewValidationToken`) | Preserve — already `Arc`-backed and immutable; no live host reference is a field | C1 (no behavior change) |
   193	| Blocking cross-file load-on-demand | `host_lifecycle.rs:953` (`ensure_loaded`), `:1012` (`wait_or_drive`), `resolver_context.rs:913-914`, `host_resolver_context.rs:281-288`, `bare_name_resolve.rs:188-190` | Preserve for host/session lifecycles; add the non-blocking `NeedInputs` alternative | C1 |
   194	| `verter_semantic → verter_workspace` edge (A5-DD1) | `crates/verter_semantic/Cargo.toml:27`, consumers at `analysis/project_resolver.rs`, `analysis/routes.rs:196,251,661,672,869,1120` (`&dyn WorkspaceRead`), `facts/registry.rs:3` | Delete the edge | C1 (named removal gate in `B1.md:90-109`) |
   195	
   196	**Duplicated lifecycle-adapter boilerplate to collapse** (research-verified, not hypothesis): `is_request_bound`
   197	(`host_resolver_context.rs:193-195` vs `session_resolver_context.rs:187-189`), `request_completion_overlay`
   198	(`:221-224` vs `:234-237`), `store_view` (`:321-324` vs `:508-511`), `aggregate_basis_seed` (`:326-329` vs
   199	`:513-516`), `dispatch` (`:494-500` vs `:674-680`), `resolve_imported_type_root`/`_with_facts`
   200	(`:343-372` vs `:530-559`), `resolve_type_declaration_for_dep` (`:431-450` vs `:616-634`), plus the
   201	constructor trio's near-verbatim doc/rationale duplication (`:96-149` vs `:103-150`). Each pair is the
   202	same delegation shape over a different receiver (`self.inner` vs `ResolverContext::method(self.inner,
   203	..)`) — a single shared default or a common inner-delegate helper removes the duplication without
   204	touching the genuinely session-specific overrides (`authoritative_current_content_hash`,
   205	`observe_materialize_scope`, `indexed_for_current_content`, `artifact_key_for_current_content`,
   206	`resolve_type_dependency_canonical`, `shallow_file_state`, `active_session_view`,
   207	`complete_canonical`/`complete_canonical_with_session_view` — these have no host-side analog and stay
   208	distinct; `session_resolver_context.rs:304-332,357-403,421-470,591-608,280-288,712-715,171-180`).
   209	
   210	## Batched `NeedInputs` contract
   211	
   212	C1 owns the **kernel-level** attempt outcome: a resolver-core operation run under an I/O-free environment
   213	returns `AttemptOutcome::{Complete(T), NeedInputs(LoadSet), Terminal(AttemptFailure)}` per
   214	`contracts/input-loading.md` §2, §4, instead of calling `ensure_loaded`/`ensure_indexed_ready_serve`
   215	synchronously. `LoadSet` is normalized/sorted/deduplicated; `NeedInputs` on an empty delta with no basis
   216	change is the typed `InputResolutionNoProgress` failure (§4.3-4.5 of that contract), never a silent retry
   217	loop. This capability does not exist anywhere in the tree today (`grep -rn "NeedInputs\|LoadSet"
   218	crates/` — zero hits) and is new work, not a refactor of an existing batching mechanism.
   219	
   220	**C1 does not own** the outer staged transaction. `contracts/input-loading.md` §5: "A direct/project
   221	`CompileTypeInfo` over an immutable caller environment does not own commits or I/O. It returns
   222	`NeedInputs`; the caller may rebuild/extend the environment and retry." That caller — the
   223	prepare/plan/project/emit loop, `CompilePlanToken`/`ProjectionPlanToken` anti-replay, and the
   224	load/commit/retry orchestration across *multiple* kernel attempts — is `C2` (`program.md:242-247`, ADR-011).
   225	C1's obligation ends at: one resolver-tier operation, one immutable snapshot, one typed outcome. C2's
   226	obligation starts at: drive repeated C1 attempts, own the snapshot-extend/retry loop, own the anti-replay
   227	tokens.
   228	
   229	The existing blocking behavior is **not deleted**: the host-backed LSP session and the session-overlay
   230	lifecycle are permitted, documented lifecycles that legitimately block a cooperating thread
   231	(`decl_body_memo.rs:21`, `store_view_manager_tests.rs:2399` — "in-flight work block cooperatively, never
   232	busy-spin"). C1 adds the alternative outcome so a *new* I/O-free lifecycle can reuse the same resolution
   233	logic without being forced to block; it does not retrofit every existing call site to stop blocking.
   234	
   235	## Legacy deletions
   236	
   237	- The bare `impl ResolverContext for VerterHost` (`resolver_context.rs:817`) production-reachable method
   238	  bodies — confirmed to be `panic!("Architectural violation...")` in production builds today
   239	  (`resolver_context.rs:826-841,853-873,882-902,950-996,1021-1039,1054-1072,1081-1098,1107-1124,1171-1188`),
   240	  live only under `#[cfg(any(test, feature = "test-support"))]`. If no production call site needs the
   241	  bare-host rail once convergence lands (verify at implementation time — every currently-known production
   242	  entry already binds `HostResolverContext`/`SessionResolverContext`), delete the impl entirely and let
   243	  `RequestBoundResolverContext` (the sealed marker already excluding `VerterHost`,
   244	  `resolver_context.rs:780-783,788-803`) become the sole production-constructible rail. This turns
   245	  "resolve without a request-bound context" from a runtime panic into a compile error.
   246	- `crates/verter_semantic/src/analysis/project_resolver.rs` (30-line pure re-export shim of
   247	  `verter_workspace::resolver::*` and `verter_workspace::types::*`) — repoint its four documented
   248	  consumers (`verter_session`, `verter_lsp`, `verter_napi`, `verter_wasm`, per the file's own header
   249	  comment at line 12) directly at the converged `ModuleResolverCore` home and delete the shim. Do not
   250	  leave it as a permanent alias.
   251	- `crates/verter_semantic/src/facts/registry.rs:3` (`pub use verter_workspace::fact_registry::*;`) —
   252	  either the fact-key vocabulary (`FactKey`, `FactDomain`, `Fact`, etc., `verter_workspace/src/fact_registry.rs`)
   253	  moves to a dependency-neutral home both `verter_semantic` and `verter_workspace` can reach without an
   254	  edge between them, or this re-export is proven not to require the edge (e.g. the types are
   255	  already dependency-neutral and only their current physical file location is wrong). Resolve one way;
   256	  do not leave the edge and a `#[allow]`.
   257	- `crates/verter_semantic/src/analysis/routes.rs:196,251,661,672,869,1120` (`workspace: &dyn
   258	  verter_workspace::WorkspaceRead`) — the one genuinely *functional* (non-re-export) use of
   259	  `verter_workspace` inside `verter_semantic`. Closing A5-DD1 requires either relocating `WorkspaceRead`
   260	  to a neutral layer both crates depend on downward, or moving `routes.rs`'s route-extraction logic to

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'rg -n "''^(mod|pub'"\\(crate\\) mod|pub mod) (dispatch_txn|flow_return|flow_return_callee)|flow_return_callee|dispatch_txn\" crates/verter_session/src/project_semantic_dispatch/mod.rs crates/verter_session/src/project_semantic_dispatch/flow_return.rs; nl -ba crates/verter_session/src/project_semantic_dispatch/mod.rs | sed -n '1,90p'; ls -1 crates/verter_session/src/project_semantic_dispatch | sed -n '1,120p'; git ls-tree -r --name-only HEAD crates/verter_session/src/project_semantic_dispatch | rg '(flow_return|dispatch_txn)'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 10763ms:[0m
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:23:use super::dispatch_txn::{
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:27:use super::flow_return_callee::{
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:458:                        .dispatch_txn
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:462:                            super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:473:                        .dispatch_txn
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:477:                            super::dispatch_txn::ReturnObligationIdentity::ResolveCall(*target),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:737:    pub(super) fn flow_return_callee_for_typeof_arg(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:794:                && self.flow_return_callee_for_typeof_arg(args[0]).is_some();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:800:                && self.flow_return_callee_for_typeof_arg(args[0]).is_some();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:818:        let identity = self.flow_return_callee_for_typeof_arg(callee_arg)?;
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:946:            let mut txn = self.dispatch_txn.borrow_mut();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:967:        if self.dispatch_txn.borrow().obligations.decides_root() {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:988:                self.dispatch_txn
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1050:            let mut txn = self.dispatch_txn.borrow_mut();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1132:                self.dispatch_txn.borrow_mut().flow.last_root_failure = Some(failure);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1146:        let wants_inline_flight = !self.dispatch_txn.borrow().obligations.decides_root();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1150:        let mut txn = self.dispatch_txn.borrow_mut();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1156:            .and_then(super::dispatch_txn::ObligationFrame::flow_return_mut)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1206:        holds: Vec<super::dispatch_txn::ReturnObligationIdentity>,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1211:                super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1214:                super::dispatch_txn::ReturnObligationIdentity::FlowReturn(_) => None,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1258:        let popped = self.dispatch_txn.borrow_mut().reentry_mut().pop();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1280:                    super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1283:                    super::dispatch_txn::ReturnObligationIdentity::FlowReturn(_) => None,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1315:            let mut txn = self.dispatch_txn.borrow_mut();
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1336:            super::dispatch_txn::ResolveCallPendingState,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1339:            .dispatch_txn
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1399:        let replay_substitution: super::dispatch_txn::ProvisionalSubstitution = relation_members
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1403:                    super::dispatch_txn::ObligationIdentity::Relate {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1407:                    super::dispatch_txn::ProvisionalVerdict::Relate(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1415:            prefix_entries.push(super::dispatch_txn::FlowDischargeEntry {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1475:                    super::dispatch_txn::PendingVerdict::Unknown
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1476:                        | super::dispatch_txn::PendingVerdict::BudgetExceeded(_)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1576:                    self.dispatch_txn.borrow_mut().flow.completed_members.push(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1606:        entries: &mut [super::dispatch_txn::FlowDischargeEntry],
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2252:                params.push(super::flow_return_callee::unmodeled_position_marker(self));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2535:                                    super::dispatch_txn::RelationStep::Assignable { .. }
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2539:                                    super::dispatch_txn::RelationStep::NotAssignable
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:3230:        super::flow_return_callee::unmodeled_position_marker(self.dispatch)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4664:                super::dispatch_txn::RelationStep::Assignable { .. } => survivors.push(*arm),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4665:                super::dispatch_txn::RelationStep::NotAssignable => {}
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4666:                super::dispatch_txn::RelationStep::Unknown
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4667:                | super::dispatch_txn::RelationStep::BudgetExceeded(_)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4668:                | super::dispatch_txn::RelationStep::Assumed(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4834:            super::dispatch_txn::RelationStep::Assignable { .. } => Some(true),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4835:            super::dispatch_txn::RelationStep::NotAssignable => Some(false),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4836:            super::dispatch_txn::RelationStep::Unknown
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4837:            | super::dispatch_txn::RelationStep::BudgetExceeded(_)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4838:            | super::dispatch_txn::RelationStep::Assumed(_) => None,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:6729:                                let marker = super::flow_return_callee::unmodeled_position_marker(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:6762:                                super::flow_return_callee::unmodeled_position_marker(self.dispatch);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8127:                            .dispatch_txn
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8148:                                    .dispatch_txn
crates/verter_session/src/project_semantic_dispatch/mod.rs:109:pub(crate) mod dispatch_txn;
crates/verter_session/src/project_semantic_dispatch/mod.rs:110:pub(crate) mod flow_return;
crates/verter_session/src/project_semantic_dispatch/mod.rs:111:pub(crate) mod flow_return_callee;
crates/verter_session/src/project_semantic_dispatch/mod.rs:113:pub(crate) mod flow_return_coverage_tests;
crates/verter_session/src/project_semantic_dispatch/mod.rs:115:pub(crate) mod flow_return_frame_seal_tests;
crates/verter_session/src/project_semantic_dispatch/mod.rs:117:pub(crate) mod flow_return_lexical_tests;
crates/verter_session/src/project_semantic_dispatch/mod.rs:119:pub(crate) mod flow_return_positional_tests;
crates/verter_session/src/project_semantic_dispatch/mod.rs:121:pub(crate) mod flow_return_root_gate_tests;
crates/verter_session/src/project_semantic_dispatch/mod.rs:123:pub(crate) mod flow_return_tests;
crates/verter_session/src/project_semantic_dispatch/mod.rs:388:    pub(super) dispatch_txn: std::cell::RefCell<dispatch_txn::CheckerDispatchTransaction>,
crates/verter_session/src/project_semantic_dispatch/mod.rs:526:            dispatch_txn: std::cell::RefCell::new(
crates/verter_session/src/project_semantic_dispatch/mod.rs:527:                dispatch_txn::CheckerDispatchTransaction::default(),
     1	//! Project-global [`SemanticQueryApi`] dispatcher.
     2	//!
     3	//! Binds [`SemanticQueryKey`] variants onto the shared
     4	//! [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore) memo
     5	//! and routes them to the existing resolver/solver entry points. This is the
     6	//! single dispatch site every reusable type-resolution operation flows
     7	//! through, regardless of which higher-level request initiated it.
     8	//!
     9	//! ## Scope of this landing
    10	//!
    11	//! Every [`SemanticQueryKey`] variant dispatches through this module:
    12	//!
    13	//! - `ResolveDecl` — returns a structural node sourced from the shallow state
    14	//!   (an `Object` surface when the declaration carries member signatures,
    15	//!   otherwise a `Primitive(Never)` anchor that memoizes the declaration's
    16	//!   scope/name identity).
    17	//! - `Instantiate` — identity-preserving alias anchored to the base node.
    18	//!   Memoizing the key is the dedup guarantee; the richer instantiation
    19	//!   shape is produced on demand by the solver once the caller walks
    20	//!   into it. Mode-free per the lazy-materialisation rule.
    21	//! - `ProjectPath` — path-precise projection rooted at `base` walking each
    22	//!   [`PathSegment`]. The empty-path form is the canonical shape of
    23	//!   "expand the whole surface" and supersedes the retired `Expand`
    24	//!   variant. `ProjectMember { base, member, mode }` and
    25	//!   `IndexedAccess { base, index, mode }` admission-canonicalise to the
    26	//!   length-1 `ProjectPath` form **before** memo hashing so sugar and
    27	//!   canonical share one warm entry and one in-flight wait graph.
    28	//! - `NormalizeUnion` / `NormalizeIntersection` — structural dedup over the
    29	//!   supplied members with stable ordering.
    30	//! - `KeyOf` / `MappedType` / `Conditional` — navigation operations that
    31	//!   walk the base node's shared-graph payload. Paths that do not reach a
    32	//!   concrete node fall through to a recorded `Opaque(Miss)` entry — this
    33	//!   is distinct from a dispatch miss because the warm entry observes the
    34	//!   base node identity and therefore dedups repeated asks.
    35	//! - `TypeOf` — mirrors `ResolveDecl` but routes through the shallow
    36	//!   value-symbol space.
    37	//!
    38	//! Every variant observes a dep-signature fragment (at minimum the project
    39	//! generation; additional file hashes flow in from the base nodes' origin
    40	//! scopes where the dispatcher can observe them). Dep-signature propagation
    41	//! is what the publish-side completion-fence revalidation relies on for
    42	//! transitive final-result validation.
    43	//!
    44	//! ## Design rules
    45	//!
    46	//! - Navigators stay non-owning: new semantic nodes must enter through
    47	//!   [`SemanticQueryApi::execute`], not through ad-hoc helpers on the
    48	//!   dispatcher.
    49	//! - Errors, partial results, and recursion sentinels never promote to warm
    50	//!   memo entries — the underlying [`SemanticGraphStore`] enforces this
    51	//!   invariant at publish time.
    52	//! - Dep-signatures returned from warm hits must fold into the caller's
    53	//!   dependency-fact set so the publish-side completion-fence
    54	//!   revalidation stays transitive.
    55	
    56	use std::sync::Arc;
    57	
    58	use verter_semantic::analysis::type_solver::host::{
    59	    BareRefOrigin, ResolvedRootIdentity, UtilitySource,
    60	};
    61	
    62	use crate::resolver_core::prepared_decl::PreparedTypeDeclResolution;
    63	use crate::resolver_core::{BudgetDomain, BudgetExceededFailure, ResolverContext};
    64	use crate::semantic_query::{
    65	    BranchSelection, CacheRead, DeclIdentity, DepSignature, DepVersion, IndexKey, LiteralValue,
    66	    NodeScopeId, OriginEdgeKind, OriginMeta, PathSegment, PrimitiveKind, ProjectionMode,
    67	    PropertyKey, QueryError, QueryResult, ResolveDeclKey, ResultProvenance, ScopeId,
    68	    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    69	    SemanticQueryValue, SemanticQueryValueTag, SignatureRef,
    70	};
    71	use crate::semantic_query_memo::SemanticGraphStore;
    72	use verter_type_expr::PrimitiveName;
    73	
    74	// Module tree. The sub-modules are `pub(crate)` so external callers see only
    75	// the `ProjectSemanticDispatch` struct / trait impl, while each module owns one
    76	// concern of the dispatcher and shares private accessors through `impl` blocks:
    77	//   - `build`     — the `build_*` builders for every query variant that
    78	//                   produces a new `SemanticNodeId` (`build_resolve_decl`,
    79	//                   `build_typeof`, `build_instantiate`, `build_class_surface`,
    80	//                   `build_resolve_macro_payload`, …).
    81	//   - `lower` / `raise` — `TypeExpr` ⇄ `SemanticNodeId` structural conversion.
    82	//   - `walk`      — path-walking + the non-recursive shallow-mode terminal
    83	//                   surface synthesiser.
    84	//   - `enumerate` — `keyof` member-name enumeration helpers.
    85	//   - `evaluate`  — the deferred-shell fix-point evaluation loop.
    86	//   - `substitute`— generic type-parameter substitution into the graph.
    87	//   - `relation`  — the authoritative semantic-node assignability engine.
    88	// `mod.rs` retains the dispatch entry points and shared dispatcher state.
    89	pub(crate) mod absorb;
    90	mod apparent_type;
absorb.rs
apparent_type.rs
apparent_type_tests.rs
broad_runtime.rs
broad_runtime_tests.rs
build.rs
call_resolve.rs
call_resolve_tests.rs
carrier.rs
carrier_head_resolution_tests.rs
carrier_materialize_tests.rs
carrier_reduction_tests.rs
carrier_tests.rs
closedness_evaluator_tests.rs
cycle_gate.rs
cycle_gate_tests.rs
dispatch_txn.rs
dispatch_txn_tests.rs
enumerate.rs
evaluate.rs
flow_return.rs
flow_return_callee.rs
flow_return_coverage_tests.rs
flow_return_frame_seal_tests.rs
flow_return_lexical_tests.rs
flow_return_positional_tests.rs
flow_return_root_gate_tests.rs
flow_return_tests.rs
locator_shape.rs
locator_shape_binder.rs
locator_view.rs
locator_view_worklist
locator_view_worklist.rs
lower.rs
mapped_key_domain_carrier_tests.rs
mod.rs
object_spread_program_lowering.rs
object_spread_projection_eval.rs
object_spread_projection_eval_tests.rs
output_materialization.rs
output_materialization_guards.rs
projection_stack_safety_tests.rs
prototype_call.rs
prototype_call_tests.rs
query_error_disposition.rs
query_error_disposition_tests.rs
raise
raise.rs
raise_sentinel.rs
raised_shape_tests.rs
reactive_wrapper.rs
relation.rs
relation_excess.rs
relation_predicates.rs
return_equation.rs
return_equation_tests.rs
semantic_source.rs
semantic_source_compose.rs
semantic_source_leaf_facts.rs
semantic_source_tests.rs
substitute.rs
symbol_identity.rs
template_class_facts.rs
tests.rs
walk.rs
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs
crates/verter_session/src/project_semantic_dispatch/dispatch_txn_tests.rs
crates/verter_session/src/project_semantic_dispatch/flow_return.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/contracts/flow-completeness.md; nl -ba docs/arch/refactor/rev11/architecture.md | sed -n '680,830p'; nl -ba docs/arch/refactor/rev11/decisions/ADR-001-semantic-authority-and-derived-projections.md; nl -ba docs/arch/refactor/rev11/decisions/ADR-006-demand-selected-flow-domains.md; nl -ba docs/arch/refactor/rev11/decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 28516ms:[0m
     1	# Flow Domain Closure, Obligation Ledger, and Completeness Contract
     2	
     3	**Status:** Normative effective-flow solver and warm-admission contract.
     4	
     5	# 1. Sole structural authority
     6	
     7	`FunctionProgramIndex -> FunctionBodySkeleton -> FunctionFlowGraph` is the sole structural flow authority. Deterministic derived projections such as dominators, loop forests, SCCs, def-use indexes, capture summaries, and execution schedules may accelerate the graph but cannot introduce semantic facts.
     8	
     9	# 2. Closed operation/domain registry
    10	
    11	Every flow-bearing operation declares a static closed registry:
    12	
    13	```text
    14	operation/result contract
    15	-> required product domains
    16	-> required graph edge/fact families
    17	-> expansion rules
    18	-> fixed-point requirements
    19	-> accepted gaps/failures
    20	-> completeness finalizer
    21	```
    22	
    23	Representative domains include reaching values/types, narrowing, completion, closure capture/freshness/invalidation, effects, definite assignment, and coverage. An unknown domain/fact family is a typed unsupported obligation, not an ignored enum variant.
    24	
    25	# 3. Demand plan
    26	
    27	A deterministic `FlowDemandPlan` records:
    28	
    29	- graph/body identity;
    30	- source/input and semantic profile basis;
    31	- result contract;
    32	- selected subject/program point;
    33	- required domains;
    34	- initial and expanded obligation IDs;
    35	- deterministic work order/tie breakers;
    36	- convergence and resource policy.
    37	
    38	Planning occurs once per cold logical demand by default. Replanning or lowering a second carrier requires a predeclared measured exception.
    39	
    40	# 4. Obligation ledger
    41	
    42	Each required obligation has a private typed state, for example:
    43	
    44	```rust
    45	enum ObligationState {
    46	    Pending,
    47	    Running,
    48	    Discharged(DischargeEvidence),
    49	    Gap(FlowGap),
    50	    Failed(FlowFailure),
    51	}
    52	```
    53	
    54	Expansion may add obligations only through registered deterministic rules. The ledger records origin, required domain, graph node/edge basis, dependencies, and discharge evidence. No public caller can mark an obligation discharged.
    55	
    56	# 5. Complete-result construction
    57	
    58	```rust
    59	enum FlowSolveOutcome {
    60	    Complete(CompleteFlowResult),
    61	    Partial(PartialFlowResult),
    62	    NoValue(FlowFailure),
    63	}
    64	```
    65	
    66	`CompleteFlowResult` has a private constructor. The finalizer succeeds only when:
    67	
    68	- graph, demand, profile, input basis, and result contract match;
    69	- the closed domain registry is known;
    70	- every required/expanded obligation is discharged;
    71	- every required fixed point converged deterministically;
    72	- every semantic suboperation is complete under the same contract;
    73	- no gap, stale basis, cancellation, budget exhaustion, panic, or internal failure occurred.
    74	
    75	An authored `any` is a semantic value. “Verter has no model” is a gap and cannot become `any`.
    76	
    77	Only proof-bearing complete results may be warm-admitted. Partial values may be returned to explicitly tolerant consumers but remain return-only unless a separate operation contract proves partial admission safe.
    78	
    79	# 6. Atomic production cutover
    80	
    81	`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
    82	
    83	`D2` is one indivisible accepted cutover:
    84	
    85	1. route every public effective-flow operation to the new solver;
    86	2. delete the old syntax-shaped evaluator and its state, caches, tasks, flags, compatibility shims, and migration guards;
    87	3. return typed gaps for mechanisms not yet implemented;
    88	4. preserve every ratified Supported/Stable capability and failure contract;
    89	5. prove no second selectable evaluator remains.
    90	
    91	Later blocks expand only the sole solver.
    92	
    93	# 7. Required tests
    94	
    95	- compile-fail/private-constructor proof;
    96	- mutation test dropping one obligation cannot yield `Complete`;
    97	- unknown edge/domain produces a gap;
    98	- plan/order randomized but observable result deterministic;
    99	- no-flow path allocates no graph/plan;
   100	- structural authored returns independent from endpoint completion;
   101	- closure effects independent of expression position;
   102	- loop/completion convergence and budget failure;
   103	- partial replay never appears as warm complete;
   104	- source search, dependency graph, and runtime tests prove the legacy evaluator is absent after `D2`.
   680	- borrowed exact authored nodes;
   681	- compact exact authored chunks only across a proven boundary;
   682	- request/generation-local semantic values;
   683	- operation-specific DTOs;
   684	- rendered type text only when requested;
   685	- opaque cohort/session-bound continuation handles;
   686	- optional bounded graph export for actual graph consumers.
   687	
   688	## 8.3 Optional graph export
   689	
   690	A semantic graph snapshot is an optional advanced product operation, not the mandatory base API. It exists only for consumers that need graph traversal, transfer, audit, or continuation beyond operation-specific DTOs.
   691	
   692	Graph export has:
   693	
   694	- its own compatibility domain;
   695	- explicit roots and snapshot-local IDs;
   696	- size/node/depth budgets;
   697	- exactness and diagnostics;
   698	- stable serialization tests;
   699	- no promise that internal storage matches wire layout.
   700	
   701	Opaque internal handles encode or validate cohort/generation identity. A raw node index never escapes its cohort. Stale use fails deterministically rather than aliasing a new node.
   702	
   703	# 9. Effective typing and flow
   704	
   705	## 9.1 Structural authority
   706	
   707	The reusable structural path is:
   708	
   709	```text
   710	FunctionProgramIndex
   711	-> FunctionBodySkeleton
   712	-> FunctionFlowGraph
   713	```
   714	
   715	`FunctionFlowGraph` is the one flow dependence authority. It contains stable binding/reference/effect/control/return/call/capture/escape/loop/completion facts and exact authored locators. Another CFG/control tree is prohibited.
   716	
   717	Graph construction performs no general type lowering or query-specific semantic evaluation.
   718	
   719	## 9.2 Deterministic derived projections
   720	
   721	Dominator indexes, loop forests, SCC indexes, reverse adjacency, def-use overlays, capture summaries, and compact schedules are permitted only as deterministic projections of one exact graph/profile identity. They cannot introduce semantic facts, are request-local by default, and have independent weight/eviction when retained.
   722	
   723	## 9.3 Demand plan and closed domain registry
   724	
   725	A product operation names semantic obligations. The solver computes the transitive closure through one static domain registry before evaluation. Candidate domains include declared/reaching type, narrowing, relation, completion, definite assignment, capture freshness/invalidation, effect, and coverage.
   726	
   727	A `FlowDemandPlan` selects only nodes/edges/domains needed by the result. A fixed point runs only on selected obligations that require convergence. A missing registered prerequisite makes the plan invalid and cannot produce a complete result.
   728	
   729	## 9.4 Binding and relation authority
   730	
   731	Flow state is keyed by stable binding/slot identity, never by string names. Declaration authority exists for annotated and inferred bindings. Joins, substitution, and nominal `unique symbol` overlap/comparability use the shared semantic relation/inference implementation; flow owns no private relation classifier.
   732	
   733	## 9.5 Structural returns and completion
   734	
   735	For return inference:
   736	
   737	```text
   738	result = union(effective_type(return_expr_i, program_point_i)
   739	               for every authored return statement i)
   740	       + (endpoint_reachable ? undefined : nothing)
   741	```
   742	
   743	Authored return membership is structural. Reachability does not delete a return contributor. Endpoint reachability is a separate completion question.
   744	
   745	Completion uses one algebra:
   746	
   747	```rust
   748	enum CompletionKind {
   749	    Normal,
   750	    Return,
   751	    Throw,
   752	    Break(LabelId),
   753	    Continue(LabelId),
   754	}
   755	```
   756	
   757	Blocks, conditions, loops, labels, `switch`, `try`, `catch`, and `finally` transform/join completion sets. Suffix booleans, “guarantees return,” and pending-break flags are prohibited.
   758	
   759	## 9.6 Closure and effects
   760	
   761	The structural producer retains nested capture reads and writes. Summaries include direct/transitive reads, writes, escapes, creation point, freshness, and invalidating-write information.
   762	
   763	Effects use one expression-position-independent mechanism. No IIFE/capture hook is specialized to statement, initializer, condition, argument, return, sequence, literal, or another parent position. Compatibility results match the selected semantic profile; extra analysis may decide whether answering is safe but may not publish a different compatibility answer.
   764	
   765	## 9.7 Obligation ledger and proof-carrying completeness
   766	
   767	Each solve owns a private obligation ledger. Every selected graph/domain/fixed-point/semantic operation finishes as one of:
   768	
   769	```text
   770	Discharged
   771	CoverageGap
   772	Unsupported
   773	Cancelled
   774	BudgetExceeded
   775	Stale
   776	InternalFailure
   777	```
   778	
   779	Only a private finalizer can construct `CompleteFlowResult`, and only when every required obligation is `Discharged` under the exact graph, demand, profile, input basis, and result contract. Rich audit evidence is optional/request-scoped; the production proof token is compact.
   780	
   781	Partial/gap results may be returned but remain non-admissible unless a separate query contract proves partial admission. Authored `any` is distinct from unmodelled semantics.
   782	
   783	## 9.8 Atomic sole-solver cutover
   784	
   785	Flow migration has one public cutover:
   786	
   787	1. build the minimum graph/domain/obligation solver behind a private non-production test boundary, already using stable binding identities and the shared relation/inference authority for every capability that will remain enabled at cutover;
   788	2. route public effective-flow operations exclusively to it and delete the syntax-shaped evaluator, name maps, position-specific hooks, private relation logic, completion booleans, and related caches/tasks/flags in the same accepted merge;
   789	3. return typed gaps for mechanisms not yet supported;
   790	4. preserve every `A6`-ratified Supported/Stable effective-flow capability unless a separate reviewed breaking product decision changes it;
   791	5. expand semantics only inside the sole surviving solver, never by reintroducing a private relation/name authority.
   792	
   793	No runtime flag or selectable shadow evaluator survives. Temporary reduction from guessed complete output to honest partial is accepted only for rows not ratified Supported/Stable, or through a separate reviewed breaking product decision.
   794	
   795	## 9.9 No-flow fast path
   796	
   797	`TypeAtPosition` locates subject, owner, anchor, and base/contextual type once. If the index proves no relevant flow owner/effect, it returns immediately with zero graph/plan/flow allocation, no second source traversal, and no cache entry whose only meaning is “no flow.”
   798	
   799	# 10. QueryRuntime, flights, and CPU execution
   800	
   801	## 10.1 Separation of concerns
   802	
   803	```text
   804	API demand
   805	-> QueryRuntime
   806	   -> validated hit: return inline
   807	   -> compatible running FlightCell: join
   808	   -> new cold FlightCell
   809	      -> cheap dependent work: inline
   810	      -> coarse migratable work: CpuExecutor
   811	      -> owner-affine work: owner call/inline
   812	```
   813	
   814	`QueryRuntime` owns semantic query identity, recursion/cycles, fact capture, exactness/completeness, cancellation, flights, admission, validation, and retention hooks. `CpuExecutor` owns only prerequisite-ready migratable CPU execution.
   815	
   816	## 10.2 Query identity, flight identity, and result contract
   817	
   818	`QueryIdentity<Q>` contains semantic arguments, only the semantic/output/presentation/serialization profiles observable at that typed query boundary, and `ResultContractId`. It is independent of a particular snapshot and is the bounded candidate-cache lookup identity. Terminal presentation or serialization uses a separate query/materialization identity when the underlying typed result is unchanged.
   819	
   820	`SemanticFlightKey<Q> = (QueryIdentity<Q>, InputBasisId)`. The exact input basis scopes in-flight semantic production; it is not folded into cross-snapshot cache candidate lookup. A cached candidate carries its production `InputBasisId`, complete positive/negative read facts, proof, and compatibility basis and is usable only after value-side validation against the requester’s current view. A store may keep a bounded, measured number of candidates per query identity. Candidate insertion order, newest-snapshot preference, or global revision never becomes semantic authority.
   821	
   822	`ResultContractId` includes semantics, operation/product, required exactness, capability/unsupported policy, explicitly requested approximation mode, and only the output/mapping/diagnostic/serialization contract observable at that typed query boundary.
   823	
   824	Waiter-local `ExecutionPolicy` contains deadline, cancellation, priority, and ordinary work/time/memory budget. A larger budget may let a producer continue; it does not change result identity. Budget exhaustion yields partial/failure and never a weaker complete result.
   825	
   826	Value-side validation is correctness authority. Reverse dependencies are optional cleanup/observability/prefetch aids.
   827	
   828	## 10.3 Flight classes and compatibility
   829	
   830	Two default flight scopes exist:
     1	# ADR-001 — One Semantic Authority, Justified Derived Projections
     2	
     3	**Status:** Accepted  
     4	**Decision owner:** TypeInfo/effective-flow architecture
     5	
     6	## Context
     7	
     8	`FunctionFlowGraph` must be the one structural flow authority, but efficient solving may need dominators, loop forests, SCCs, def-use overlays, capture summaries, reverse adjacency, or compact execution schedules. Forbidding every graph-derived structure would either bloat the canonical graph or repeat topology work. Allowing independent query-specific CFGs recreates the current dual-authority defect.
     9	
    10	## Decision
    11	
    12	`FunctionFlowGraph` is the canonical structural authority. A derived structure is allowed only when it:
    13	
    14	- is keyed by one exact graph/body identity and any interpretation-affecting semantic profile;
    15	- is deterministic and fully reproducible from the graph and accepted kernel rules;
    16	- cannot add facts, define transfer/join semantics, mark coverage complete, or publish a semantic result;
    17	- is request-local by default and independently weighted/evicted if retention is proven valuable;
    18	- can be replaced without changing observable semantics.
    19	
    20	## Consequences
    21	
    22	The solver may use efficient layouts without creating a second semantic or control authority. Review checks authority and construction rights, not superficial data-structure count.
    23	
    24	## Rejected alternatives
    25	
    26	- exactly one physical graph-like object;
    27	- independent syntax-shaped/query-specific CFGs;
    28	- projections that own relation, completion, capture, or coverage decisions.
     1	# ADR-006 — Flow Uses Demand-Selected Abstract Domains
     2	
     3	**Status:** Accepted
     4	
     5	## Context
     6	
     7	One binding-based solver is required, but one always-maximal state would make every operation pay for narrowing, completion, definite assignment, freshness, capture/effects, and coverage.
     8	
     9	## Decision
    10	
    11	Use one solver framework and one authoritative transfer/join implementation per closed flow domain. `FlowDemandPlan` activates the transitive prerequisite closure required by the result contract. Fixed points run only over the selected obligation frontier.
    12	
    13	Structural authored-return collection is independent of endpoint completion. A private obligation ledger and finalizer construct complete results; a query cannot omit required domains manually.
    14	
    15	## Consequences
    16	
    17	Narrow queries stay narrow without introducing a second evaluator. Domains are independently testable while completeness remains global to the requested result.
    18	
    19	## Rejected alternatives
    20	
    21	- unconditional whole-function maximal lattice;
    22	- independent per-query semantic evaluators;
    23	- caller-selected domain masks without closure validation.
     1	# ADR-014 — Flow Replaces the Legacy Evaluator in One Atomic Public Cutover
     2	
     3	**Status:** Accepted  
     4	**Decision owner:** flow migration and cache admission  
     5	**Reopen only if:** a release obligation explicitly requires two semantics paths, which would require a new architecture decision and bounded compatibility window.
     6	
     7	## Context
     8	
     9	Building graph/domain features across many accepted merges while the old syntax-shaped evaluator remains selectable creates two production authorities. Deleting the old path only after full parity pressures implementors to copy the legacy model into the replacement.
    10	
    11	## Decision
    12	
    13	1. Build the minimum new graph/domain/obligation/coverage foundation behind a private non-production test boundary on the bounded cutover branch; it may be reviewed as a checkpoint but never merged or released independently.
    14	2. In one public cutover, route all effective-flow operations to the new solver and delete the old evaluator and its state/caches/tasks/flags.
    15	3. Unsupported mechanisms return typed non-admissible gaps; temporary reduction from guessed success to honest partial is allowed only for rows not ratified Supported/Stable, or through a separate reviewed breaking product decision.
    16	4. Later semantic blocks only expand the sole solver.
    17	5. A private obligation ledger and finalizer make complete-result construction impossible unless all required closed-domain obligations are discharged.
    18	
    19	No runtime flag, compatibility shim, shadow evaluator, or compare-in-production path survives the accepted cutover.
    20	
    21	## Consequences
    22	
    23	- one production semantic authority at all times after `D2`;
    24	- completeness proof is architectural rather than a convention;
    25	- parity can expand without preserving guessed legacy behavior.
    26	
    27	## Rejected alternatives
    28	
    29	- **Long-running dual evaluators:** divergent semantics and cache risk.
    30	- **Wait for full parity before deletion:** encourages porting the second authority intact.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'rg -n "''^(pub'"\\(crate\\)|pub\\(super\\)|pub|struct|enum|impl|type).*?(Obligation|FlowReturn|FlowDischarge|RelationDomain|PendingLedger|CheckerDispatch|SessionAdmission|InferenceSession)|Obligation(Runtime|PendingLedger|State)|FlowDischarge|finaliz|discharg|mark.*complete\" crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '1,280p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 14439ms:[0m
8://! ├── ObligationRuntime          (tagged identities, generic frames/
14://! │   └── ObligationPendingLedger
17://!                                 redischarge/fixation state)
35://!   domains discharges through the same storage, so the per-engine cycle
44://! - [`ObligationPendingLedger`] — popped-but-unpublished SCC members
169:    /// undischargeable SCC edge). No public value-domain form; ReturnOnly.
215:pub(crate) enum ObligationIdentity {
232:impl ObligationIdentity {
298:pub(crate) struct FlowReturnFrameState {
318:pub(crate) enum ObligationFrameDomain {
329:/// discharge needs (assumption edges + lowlink + drain watermark) and its
332:pub(crate) struct ObligationFrame {
346:    /// The `ObligationPendingLedger` pending length at this frame's PUSH —
358:impl ObligationFrame {
396:pub(crate) struct ObligationReentryStack {
401:impl ObligationReentryStack {
566:/// is produced at the batched-publish instant by the discharge against
569:pub(crate) struct PendingObligation {
579:    /// The member's provisional discharged verdict at pop.
594:pub(crate) enum FlowReturnPendingOutcome {
606:        /// component discharge (its value is the join of its hold
619:impl FlowReturnPendingOutcome {
631:pub(crate) struct FlowReturnPendingState {
637:    /// callees and direct self-calls) — the SCC close discharges an
781:pub(crate) enum PendingObligationDomain {
795:pub(crate) struct ObligationPendingLedger {
799:impl ObligationPendingLedger {
833:/// installs for its members' re-discharge (design §2.3 step 4 — the
840:    /// A converged call result used while relation members re-discharge.
844:/// The ONE tagged provisional substitution table: SCC members re-discharge
846:pub(crate) type ProvisionalSubstitution = FxHashMap<ObligationIdentity, ProvisionalVerdict>;
879:pub(crate) struct ObligationRuntime {
881:    pending: ObligationPendingLedger,
885:impl ObligationRuntime {
894:    pub(crate) fn pending(&self) -> &ObligationPendingLedger {
898:    pub(crate) fn pending_mut(&mut self) -> &mut ObligationPendingLedger {
947:    /// Install one SCC re-discharge context (the tagged substitution table)
948:    /// and return the complete previous context so a nested re-discharge
970:pub(crate) enum InferenceSessionState {
1105:pub(crate) struct InferenceSessionSetup {
1110:impl InferenceSessionSetup {
1179:pub(crate) struct InferenceSession {
1201:impl InferenceSession {
1705:/// provisional: redischarge may publish only when its polarity and complete
1709:pub(crate) fn redischarge_is_stable(
1711:    redischarge: &PendingVerdict,
1713:    match (provisional, redischarge) {
1719:                bindings: redischarge,
1721:        ) => provisional == redischarge,
1730:/// close re-discharges against the converged state and publishes ONLY a
1733:pub(crate) struct SessionAdmissionLedger {
1737:impl SessionAdmissionLedger {
1770:/// payloads, and relation redischarge/fixation state. The SCC topology it
1771:/// runs on lives in the generic [`ObligationRuntime`].
1773:pub(crate) struct RelationDomainRuntime {
1782:    /// Virtual root occurrence used while an SCC member re-discharges after
1786:    pub(crate) redischarge_occurrence: Option<(usize, InferenceOccurrence)>,
1820:/// Saved transient state for a nested SCC re-discharge. Persistent relation
1822:/// substitution rails used by the enclosing re-discharge.
1823:pub(crate) struct SavedRedischargeContext {
1836:pub(crate) struct CompletedFlowReturnMember {
1868:pub(crate) struct FlowReturnDomainRuntime {
1894:pub(crate) struct CheckerDispatchTransaction {
1897:    pub(crate) obligations: ObligationRuntime,
1911:pub(super) struct FlowDischargeEntry {
1914:    /// The member's outcome (updated in place by the discharge).
1928:pub(crate) enum ReturnObligationIdentity {
1937:    // discharge through the callee-clause fixed point and never enter the
1968:impl CheckerDispatchTransaction {
2142:    /// Install one SCC re-discharge context and return the complete previous
2143:    /// context so a nested re-discharge can restore its caller exactly.
2144:    pub(crate) fn replace_redischarge_context(
2148:    ) -> SavedRedischargeContext {
2153:            .redischarge_occurrence
2155:        SavedRedischargeContext {
2161:    pub(crate) fn restore_redischarge_context(&mut self, saved: SavedRedischargeContext) {
2163:        self.relation.redischarge_occurrence = saved.occurrence;

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '1,120p'; nl -ba crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '140,430p'; nl -ba crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '540,985p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 15561ms:[0m
     1	//! `CheckerDispatchTransaction` — the transient per-obligation-root
     2	//! cold-compute frame of the ONE resolver (design `docs/arch/u2-relation-infer-design.md`
     3	//! §2.1), laid out as ONE tagged obligation runtime plus per-domain
     4	//! runtimes:
     5	//!
     6	//! ```text
     7	//! CheckerDispatchTransaction
     8	//! ├── ObligationRuntime          (tagged identities, generic frames/
     9	//! │   │                            backedges/lowlinks, the generic pending
    10	//! │   │                            ledger + watermarks, the tagged
    11	//! │   │                            provisional substitution table)
    12	//! │   ├── ObligationIdentity::{Relate, FlowReturn, ResolveCall}
    13	//! │   ├── ObligationReentryStack (frames + tagged index)
    14	//! │   └── ObligationPendingLedger
    15	//! └── RelationDomainRuntime      (inference sessions, relation
    16	//!                                 provisional payloads, relation
    17	//!                                 redischarge/fixation state)
    18	//! ```
    19	//!
    20	//! The persistent relation cache lives in the family memo's `Relate` family,
    21	//! keyed by the full §2.7 identity; EVERYTHING in this module is TRANSIENT
    22	//! per-`CheckerDispatchTransaction` state and is NEVER a cache key, NEVER
    23	//! thread-local, NEVER process-wide. The transaction rides the dispatch
    24	//! ([`crate::project_semantic_dispatch::ProjectSemanticDispatch`]) as a
    25	//! `RefCell`, exactly like the dispatch's other cold-compute cycle guards
    26	//! (`instantiate_active`, `carrier_normalizing`, `build_local_taint`).
    27	//!
    28	//! Shapes:
    29	//!
    30	//! - [`ObligationReentryStack`] — the ONE shared re-entry / cycle-id space.
    31	//!   Each node is keyed by its full normalized tagged identity (a `Relate`
    32	//!   node by the full §2.7 key plus its transient inference occurrence).
    33	//! - Assumption-edge recording plus the lowlink (min open-target) tracking
    34	//!   lives on the GENERIC frame — a coinductive SCC whose members span
    35	//!   domains discharges through the same storage, so the per-engine cycle
    36	//!   spaces cannot diverge.
    37	//! - [`InferenceSession`] / [`SessionAdmissionLedger`] — the in-flight
    38	//!   relation inference substrate: a binding-producing relation opens a
    39	//!   session whose SETUP is fully determined by the infer pattern it serves
    40	//!   (see [`InferenceSession`]), so the content-free [`InferenceContextKey`]
    41	//!   fingerprint is well-defined at session OPEN — the transient `SessionId`
    42	//!   stand-in of design §2.2 is not needed for this subset (the setup never
    43	//!   mutates mid-flight; fixation is a single deterministic pass).
    44	//! - [`ObligationPendingLedger`] — popped-but-unpublished SCC members
    45	//!   awaiting their SCC root's close (PROVISIONAL verdicts — caller-return
    46	//!   values + deferral metadata, NEVER the published payload).
    47	//!
    48	//! Execution model (single-threaded per transaction): frames nest strictly,
    49	//! so assumption edges ALWAYS point from a deeper frame to an ancestor on
    50	//! the current stack. The SCC of the frame being popped is therefore the
    51	//! contiguous stack suffix from the minimum open-assumption target — the
    52	//! Tarjan lowlink specialised to a path graph (design §2.3 step 1 "Tarjan
    53	//! over the assumption edges"). Discharge (§2.3 step 3): a member decided
    54	//! with all non-assumptive obligations positive closes POSITIVE
    55	//! (`Assignable` + `CoinductiveCycle`); a member with a negative
    56	//! non-assumptive obligation publishes `NotAssignable` (final); any
    57	//! `Unknown` / budget edge anywhere in the component routes the WHOLE SCC
    58	//! through `ReturnOnly` (nothing publishes).
    59	
    60	use std::sync::Arc;
    61	
    62	use rustc_hash::{FxHashMap, FxHashSet};
    63	
    64	use crate::semantic_query::{
    65	    CanonicalTypeSubstitution, ConstParamPolicy, ContextualInferenceMode, FlowReturnFailure,
    66	    FlowReturnKey, FlowReturnResult, IndexSignature, InferBinding, InferableParamSetId,
    67	    InferenceCandidatePriority, InferenceContextKey, InferencePassKind, NoInferMask,
    68	    RecursionOrBudgetCap, RelateMemoKey, RelationPayload, ResolveCallFailure, ResolveCallKey,
    69	    ResolvedCallResult, SemanticNodeId, SignatureCandidateOrigin, SurfaceMember, TupleElement,
    70	    VariancePhase, VariancePolicy,
    71	};
    72	use crate::semantic_query_memo::InlineRelationFlight;
    73	
    74	/// Transient per-transaction session token. Content-free; NEVER enters a
    75	/// published key, a `ReadSetSignature.facts` observation, or any fact
    76	/// signature.
    77	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    78	pub(crate) struct SessionId(pub(crate) u64);
    79	
    80	/// Transient inference occurrence carried by one in-flight relation frame.
    81	/// It affects session-local candidate deposits, but never the persistent
    82	/// `RelateMemoKey` or a published payload.
    83	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    84	pub(crate) struct InferenceOccurrence {
    85	    pub(crate) priority: InferenceCandidatePriority,
    86	    pub(crate) variance: VariancePhase,
    87	}
    88	
    89	impl InferenceOccurrence {
    90	    pub(crate) const ARGUMENT_COVARIANT: Self = Self {
    91	        priority: InferenceCandidatePriority::Argument,
    92	        variance: VariancePhase::Covariant,
    93	    };
    94	}
    95	
    96	/// Normalized typed strict-family configuration threaded into the
    97	/// transaction (RI-10): the reducer BRANCHES on it, and it folds into the
    98	/// relation key's `type_env_hash` so a strict-on judgement can never
    99	/// warm-hit a strict-off request (design obligation 3 — behavioral branch
   100	/// AND key isolation, never hash-only).
   101	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   102	pub(crate) struct StrictFamilyConfig {
   103	    /// `strictNullChecks`: when OFF, `null` / `undefined` are assignable to
   104	    /// every target except `never`.
   105	    pub(crate) strict_null_checks: bool,
   106	    /// `strictFunctionTypes`: when ON, function-type parameters relate
   107	    /// strictly contravariantly; when OFF they relate bivariantly (the
   108	    /// non-strict function rule — either direction suffices).
   109	    pub(crate) strict_function_types: bool,
   110	    /// `exactOptionalPropertyTypes`: when ON, an authored optional write
   111	    /// keeps its explicit `undefined` in the present value (Disabled drops
   112	    /// it). Not part of the `strict` family — threaded along the same host
   113	    /// knob so every consumer reads one configuration.
   114	    pub(crate) exact_optional_property_types: bool,
   115	}
   116	
   117	impl StrictFamilyConfig {
   118	    /// The default regime — matches the pre-activation engine's behavior
   119	    /// (null/undefined isolated; contravariant function parameters).
   120	    pub(crate) const TS_STRICT: Self = Self {
   140	    /// identity fold for any non-default configuration so strict-on and
   141	    /// strict-off relations occupy distinct slots.
   142	    pub(crate) fn mix_into_type_env_hash(self, base: [u8; 16]) -> [u8; 16] {
   143	        if self == Self::TS_STRICT {
   144	            return base;
   145	        }
   146	        let mut out = base;
   147	        // Domain-separate marker byte plus the two config bits, mixed into
   148	        // every lane so any base hash stays collision-free per config.
   149	        let marker: u8 =
   150	            0x5C ^ (self.strict_null_checks as u8) ^ ((self.strict_function_types as u8) << 1);
   151	        for (i, b) in out.iter_mut().enumerate() {
   152	            *b = b.wrapping_add(marker.rotate_left(i as u32));
   153	        }
   154	        out
   155	    }
   156	}
   157	
   158	/// The engine-internal step result of one [`super::relation`] authority
   159	/// dispatch (`execute_relate`). NEVER cached as-is; the admission boundary
   160	/// maps it onto the public payload + the admission table.
   161	#[derive(Debug, Clone)]
   162	pub(crate) enum RelationStep {
   163	    /// The source relates to the target (with the inference bindings a
   164	    /// binding-producing judgement fixed at session close).
   165	    Assignable { bindings: Arc<[InferBinding]> },
   166	    /// The source provably does NOT relate to the target.
   167	    NotAssignable,
   168	    /// The judgement could not be decided (deferred / opaque / an
   169	    /// undischargeable SCC edge). No public value-domain form; ReturnOnly.
   170	    Unknown,
   171	    /// A budget cap stopped the relate. PUBLIC-but-never-warm: expressible
   172	    /// on the payload, ReturnOnly at the admission gate.
   173	    BudgetExceeded(RecursionOrBudgetCap),
   174	    /// The scoped coinductive assumption sentinel: the queried full
   175	    /// identity is already on the reentry stack, so the relation is
   176	    /// ASSUMED to hold for this SCC and the caller's frame recorded the
   177	    /// assumption edge. NEVER warm-admitted, NEVER the published proof.
   178	    Assumed(RelationAssumptionEvidence),
   179	}
   180	
   181	/// Exact transient dependency evidence carried by a coinductive relation
   182	/// assumption. The suffix starts at the intercepted ancestor and ends at the
   183	/// current demander; it is never admitted into a memo value.
   184	#[derive(Debug, Clone)]
   185	pub(crate) struct RelationAssumptionEvidence {
   186	    closure: Arc<[ObligationIdentity]>,
   187	}
   188	
   189	impl RelationAssumptionEvidence {
   190	    pub(crate) fn reaches_flow_function(
   191	        &self,
   192	        function: &crate::semantic_query::FlowFunctionSlotIdentity,
   193	    ) -> bool {
   194	        self.closure.iter().any(|identity| {
   195	            matches!(identity, ObligationIdentity::FlowReturn(key) if &key.function == function)
   196	        })
   197	    }
   198	
   199	    #[cfg(test)]
   200	    pub(crate) fn empty_for_tests() -> Self {
   201	        Self {
   202	            closure: Arc::from([]),
   203	        }
   204	    }
   205	}
   206	
   207	// ---------------------------------------------------------------------------
   208	// Tagged obligation identity + generic frame/pending machinery
   209	// ---------------------------------------------------------------------------
   210	
   211	/// The tagged full identity of one in-flight obligation on the shared
   212	/// reentry stack. Reentry identity IS this value exactly: a `Relate`
   213	/// obligation is the full §2.7 key plus its transient inference occurrence.
   214	#[derive(Debug, Clone, PartialEq, Eq, Hash)]
   215	pub(crate) enum ObligationIdentity {
   216	    /// A relation judgement in flight.
   217	    Relate {
   218	        /// The full §2.7 identity.
   219	        key: RelateMemoKey,
   220	        /// The transient occurrence axis (session-local orientation).
   221	        occurrence: InferenceOccurrence,
   222	    },
   223	    /// A whole-function `FlowReturn` evaluation in flight. Reentry
   224	    /// identity IS the `FlowReturnKey` exactly.
   225	    FlowReturn(FlowReturnKey),
   226	    /// A call-resolution execution. Its transparent generic-runtime frame
   227	    /// owns no pending/publication payload, but makes every relation opened by
   228	    /// the executor structurally inline.
   229	    ResolveCall(ResolveCallKey),
   230	}
   231	
   232	impl ObligationIdentity {
   233	    /// The relation identity parts, when this obligation is a relation.
   234	    pub(crate) fn as_relate(&self) -> Option<(&RelateMemoKey, InferenceOccurrence)> {
   235	        match self {
   236	            Self::Relate { key, occurrence } => Some((key, *occurrence)),
   237	            Self::FlowReturn(_) | Self::ResolveCall(_) => None,
   238	        }
   239	    }
   240	
   241	    /// The flow-return key, when this obligation is a flow evaluation.
   242	    pub(crate) fn as_flow_return(&self) -> Option<&FlowReturnKey> {
   243	        match self {
   244	            Self::Relate { .. } => None,
   245	            Self::FlowReturn(key) => Some(key),
   246	            Self::ResolveCall(_) => None,
   247	        }
   248	    }
   249	
   250	    /// The call-resolution key, when this obligation is a call.
   251	    pub(crate) fn as_resolve_call(&self) -> Option<&ResolveCallKey> {
   252	        match self {
   253	            Self::Relate { .. } | Self::FlowReturn(_) => None,
   254	            Self::ResolveCall(key) => Some(key),
   255	        }
   256	    }
   257	
   258	    /// The relation identity parts. Panics when the obligation is not a
   259	    /// relation — callers on a relation-only code path uphold that the
   260	    /// frames they pop are relation frames.
   261	    pub(crate) fn expect_relate(&self) -> (&RelateMemoKey, InferenceOccurrence) {
   262	        self.as_relate()
   263	            .expect("relation code path popped a non-relation obligation frame")
   264	    }
   265	}
   266	
   267	/// The relation-domain payload of one in-flight frame.
   268	#[derive(Debug)]
   269	pub(crate) struct RelationFrameState {
   270	    /// This frame deposited inference candidates into the active session
   271	    /// (a session-local delta — admission row 7: ReturnOnly, never
   272	    /// published).
   273	    pub(crate) session_delta: bool,
   274	    /// The session this frame OPENED (it is the binding root), if any.
   275	    pub(crate) opened_session: Option<SessionId>,
   276	    /// Store-owned family admission claimed for a non-binding inline
   277	    /// relation. It follows the member through SCC deferral and is either
   278	    /// completed by the root's batched publish or explicitly aborted.
   279	    pub(crate) inline_flight: Option<InlineRelationFlight>,
   280	}
   281	
   282	impl RelationFrameState {
   283	    fn new() -> Self {
   284	        Self {
   285	            session_delta: false,
   286	            opened_session: None,
   287	            inline_flight: None,
   288	        }
   289	    }
   290	}
   291	
   292	/// The flow-return-domain payload of one in-flight frame. The ordered
   293	/// return-site contributor map is evaluated inside the frame's compute
   294	/// and decided at pop: a recursive same-slot edge records as a
   295	/// coinductive hold (never a contributor, never a failure), so the
   296	/// outcome is final when the frame closes.
   297	#[derive(Debug, Default)]
   298	pub(crate) struct FlowReturnFrameState {
   299	    /// Store-owned family admission claimed for a non-root inline flow
   300	    /// evaluation. It follows the member through SCC deferral and is
   301	    /// either completed by the root's batched publish or explicitly
   302	    /// aborted.
   303	    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
   304	    /// Tagged return dependencies discovered by indexed call evaluation
   305	    /// while this flow frame is active.
   306	    pub(crate) holds: Vec<ReturnObligationIdentity>,
   307	}
   308	
   309	/// The call-resolution-domain payload of one in-flight frame.
   310	#[derive(Debug, Default)]
   311	pub(crate) struct ResolveCallFrameState {
   312	    /// Store-owned family admission claimed for a non-root inline call.
   313	    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineResolveCallFlight>,
   314	}
   315	
   316	/// The domain payload of one in-flight frame.
   317	#[derive(Debug)]
   318	pub(crate) enum ObligationFrameDomain {
   319	    /// Relation frame state.
   320	    Relate(RelationFrameState),
   321	    /// Flow-return frame state.
   322	    FlowReturn(FlowReturnFrameState),
   323	    /// Call-resolution frame state.
   324	    ResolveCall(ResolveCallFrameState),
   325	}
   326	
   327	/// One in-flight obligation frame on the reentry stack — a tagged
   328	/// full identity plus the GENERIC coinductive bookkeeping its SCC
   329	/// discharge needs (assumption edges + lowlink + drain watermark) and its
   330	/// domain payload.
   331	#[derive(Debug)]
   332	pub(crate) struct ObligationFrame {
   333	    /// The tagged full identity this frame computes.
   334	    pub(crate) identity: ObligationIdentity,
   335	    /// Assumption edges recorded by this frame's subtree: stack indices of
   336	    /// the frames this subtree ASSUMED hold (back-edges).
   337	    pub(crate) assumption_targets: Vec<usize>,
   338	    /// The Tarjan lowlink: the minimum stack index any open assumption in
   339	    /// this frame's subtree targets. `Some(own)` or `None` at pop ⇒ this
   340	    /// frame is its SCC's root.
   341	    pub(crate) min_open_target: Option<usize>,
   342	    /// This frame's reducer consumed a budget edge — the typed cap that
   343	    /// stopped the obligation. Poisons the whole SCC (ReturnOnly); the ROOT
   344	    /// surfaces the public `BudgetExceeded` payload.
   345	    pub(crate) budget_cap: Option<RecursionOrBudgetCap>,
   346	    /// The `ObligationPendingLedger` pending length at this frame's PUSH —
   347	    /// the drain watermark. Everything deposited at `pending[watermark..]`
   348	    /// was deposited by THIS frame's subtree (frames nest strictly), so an
   349	    /// SCC-root close drains exactly its own suffix. Stack indices
   350	    /// recycle after pops; this watermark does not, so a sibling frame
   351	    /// that reuses a popped member's stack index can never steal that
   352	    /// member from a still-open outer SCC.
   353	    pub(crate) pending_watermark: usize,
   354	    /// The domain payload.
   355	    pub(crate) domain: ObligationFrameDomain,
   356	}
   357	
   358	impl ObligationFrame {
   359	    /// The relation frame state, when this is a relation frame.
   360	    pub(crate) fn relation(&self) -> Option<&RelationFrameState> {
   361	        match &self.domain {
   362	            ObligationFrameDomain::Relate(state) => Some(state),
   363	            ObligationFrameDomain::FlowReturn(_) | ObligationFrameDomain::ResolveCall(_) => None,
   364	        }
   365	    }
   366	
   367	    /// The relation frame state mutably, when this is a relation frame.
   368	    pub(crate) fn relation_mut(&mut self) -> Option<&mut RelationFrameState> {
   369	        match &mut self.domain {
   370	            ObligationFrameDomain::Relate(state) => Some(state),
   371	            ObligationFrameDomain::FlowReturn(_) | ObligationFrameDomain::ResolveCall(_) => None,
   372	        }
   373	    }
   374	
   375	    /// The flow-return frame state mutably, when this is a flow frame.
   376	    pub(crate) fn flow_return_mut(&mut self) -> Option<&mut FlowReturnFrameState> {
   377	        match &mut self.domain {
   378	            ObligationFrameDomain::Relate(_) => None,
   379	            ObligationFrameDomain::FlowReturn(state) => Some(state),
   380	            ObligationFrameDomain::ResolveCall(_) => None,
   381	        }
   382	    }
   383	
   384	    /// The call-resolution frame state mutably, when this is a call frame.
   385	    pub(crate) fn resolve_call_mut(&mut self) -> Option<&mut ResolveCallFrameState> {
   386	        match &mut self.domain {
   387	            ObligationFrameDomain::Relate(_) | ObligationFrameDomain::FlowReturn(_) => None,
   388	            ObligationFrameDomain::ResolveCall(state) => Some(state),
   389	        }
   390	    }
   391	}
   392	
   393	/// The ONE shared re-entry / cycle-id space (design §2.1). Heap-backed,
   394	/// per-`CheckerDispatchTransaction`, keyed by tagged full identity.
   395	#[derive(Debug, Default)]
   396	pub(crate) struct ObligationReentryStack {
   397	    frames: Vec<ObligationFrame>,
   398	    index: FxHashMap<ObligationIdentity, usize>,
   399	}
   400	
   401	impl ObligationReentryStack {
   402	    /// The stack index of `identity` when that tagged obligation is already
   403	    /// in flight on THIS transaction.
   404	    pub(crate) fn find(&self, identity: &ObligationIdentity) -> Option<usize> {
   405	        self.index.get(identity).copied()
   406	    }
   407	
   408	    pub(crate) fn is_empty(&self) -> bool {
   409	        self.frames.is_empty()
   410	    }
   411	
   412	    pub(crate) fn depth(&self) -> usize {
   413	        self.frames.len()
   414	    }
   415	
   416	    /// Push a fresh RELATION frame for `(key, occurrence)` with the pending
   417	    /// ledger's current length as its drain watermark; returns its stack
   418	    /// index.
   419	    pub(crate) fn push_relate(
   420	        &mut self,
   421	        key: RelateMemoKey,
   422	        occurrence: InferenceOccurrence,
   423	        pending_watermark: usize,
   424	    ) -> usize {
   425	        let identity = ObligationIdentity::Relate { key, occurrence };
   426	        let idx = self.frames.len();
   427	        self.frames.push(ObligationFrame {
   428	            identity: identity.clone(),
   429	            assumption_targets: Vec::new(),
   430	            min_open_target: None,
   540	    /// own callee gate records those).
   541	    pub(crate) fn record_nearest_flow_hold(&mut self, hold: ReturnObligationIdentity) -> bool {
   542	        if matches!(hold, ReturnObligationIdentity::FlowReturn(_)) {
   543	            return false;
   544	        }
   545	        let Some(state) = self
   546	            .frames
   547	            .iter_mut()
   548	            .rev()
   549	            .find_map(|frame| match &mut frame.domain {
   550	                ObligationFrameDomain::FlowReturn(state) => Some(state),
   551	                ObligationFrameDomain::Relate(_) | ObligationFrameDomain::ResolveCall(_) => None,
   552	            })
   553	        else {
   554	            return false;
   555	        };
   556	        if !state.holds.contains(&hold) {
   557	            state.holds.push(hold);
   558	        }
   559	        true
   560	    }
   561	}
   562	
   563	/// A popped SCC member awaiting its SCC root's close — the PROVISIONAL
   564	/// deferral record (design §2.3 step 4): a caller-return value plus
   565	/// deferral metadata, NEVER the published payload. The published payload
   566	/// is produced at the batched-publish instant by the discharge against
   567	/// converged state.
   568	#[derive(Debug)]
   569	pub(crate) struct PendingObligation {
   570	    /// The member's tagged full identity.
   571	    pub(crate) identity: ObligationIdentity,
   572	    /// The domain deferral payload.
   573	    pub(crate) domain: PendingObligationDomain,
   574	}
   575	
   576	/// The relation-domain deferral payload of a popped member.
   577	#[derive(Debug)]
   578	pub(crate) struct RelationPendingState {
   579	    /// The member's provisional discharged verdict at pop.
   580	    pub(crate) verdict: PendingVerdict,
   581	    /// Session-local delta (row 7) — never publishes.
   582	    pub(crate) session_delta: bool,
   583	    /// The member opened session `Some(..)` (a binding member).
   584	    pub(crate) opened_session: Option<SessionId>,
   585	    /// Store-owned admission for this inline non-binding member.
   586	    pub(crate) inline_flight: Option<InlineRelationFlight>,
   587	}
   588	
   589	/// The decided outcome of a popped flow-return member. Decided at pop:
   590	/// a same-slot recursive backedge is a coinductive hold, so the
   591	/// contributor set is complete when the frame closes — the seed check
   592	/// runs once, at pop.
   593	#[derive(Debug, Clone)]
   594	pub(crate) enum FlowReturnPendingOutcome {
   595	    /// Complete evaluation (the admitted shape).
   596	    Complete(FlowReturnResult),
   597	    /// Typed failure — `ReturnOnly`, never admitted.
   598	    NoValue {
   599	        /// The typed no-value failure.
   600	        failure: FlowReturnFailure,
   601	        /// The degradation the FAILED evaluation had already observed
   602	        /// before it failed.
   603	        ///
   604	        /// This field is not optional decoration: a hold-only
   605	        /// [`FlowReturnFailure::EmptyCycle`] member is RESURRECTED by the
   606	        /// component discharge (its value is the join of its hold
   607	        /// targets'), so a degradation observed on the way to the empty
   608	        /// cycle must ride the failure into the fixed point. Dropping it
   609	        /// launders a degraded evaluation into a clean, WARM-admissible
   610	        /// result — and, because only the non-root member takes the
   611	        /// resurrection path, it does so in exactly one of the two demand
   612	        /// orders. Naming the field at every construction site is what
   613	        /// makes "a Degraded outcome without its degradation"
   614	        /// unrepresentable.
   615	        degradation: Option<crate::semantic_query::FlowReturnDegradation>,
   616	    },
   617	}
   618	
   619	impl FlowReturnPendingOutcome {
   620	    /// The outcome's OWN degradation, whichever arm carries it.
   621	    pub(crate) fn degradation(&self) -> Option<crate::semantic_query::FlowReturnDegradation> {
   622	        match self {
   623	            Self::Complete(result) => result.degradation(),
   624	            Self::NoValue { degradation, .. } => *degradation,
   625	        }
   626	    }
   627	}
   628	
   629	/// The flow-return-domain deferral payload of a popped member.
   630	#[derive(Debug)]
   631	pub(crate) struct FlowReturnPendingState {
   632	    /// The member's decided outcome at pop.
   633	    pub(crate) outcome: FlowReturnPendingOutcome,
   634	    /// Store-owned admission for this inline member.
   635	    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
   636	    /// The coinductive hold targets the member's evaluation met (in-flight
   637	    /// callees and direct self-calls) — the SCC close discharges an
   638	    /// empty-cycle member on its targets' admitted returns.
   639	    pub(super) holds: Vec<super::flow_return_callee::HeldCallee>,
   640	    /// The member's own file roots — the published component's self-roots
   641	    /// are the UNION of every drained member's roots, so a cross-file edit
   642	    /// invalidates the whole component.
   643	    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
   644	    /// The materialised point set the member's compute ACTUALLY produced
   645	    /// (§3.4 — recorded by the compute, never re-derived from the nominal
   646	    /// key at publish).
   647	    pub(crate) materialized: crate::semantic_query::demand::MaterializedSet,
   648	    /// Whether every one of the member's OWN return contributors was a
   649	    /// FRESH literal (and no bare-return / fallthrough arm joined). The
   650	    /// component-wide literal-widening decision is made after the
   651	    /// equation fixed point converges, so the bit must survive the pop.
   652	    pub(crate) fresh_seed: bool,
   653	}
   654	
   655	/// The winning candidate's signature while the shared return equation is
   656	/// still running.
   657	#[derive(Debug, Clone, PartialEq, Eq)]
   658	pub(crate) enum SelectedSignature {
   659	    /// A general signature node.
   660	    General(SemanticNodeId),
   661	    /// The sealed index-composed carrier. Its general signature is minted
   662	    /// only once the equation resolves the call's return, so a deferred
   663	    /// return is never observable as a failed one.
   664	    Deferred(Box<crate::semantic_query::DeferredCallable>),
   665	}
   666	
   667	/// Stable winner metadata retained while the shared return equation resolves
   668	/// the call's return node.
   669	#[derive(Debug, Clone, PartialEq, Eq)]
   670	pub(crate) enum ResolveCallSelection {
   671	    Selected {
   672	        selected: SignatureCandidateOrigin,
   673	        selected_signature: SelectedSignature,
   674	        substitution: CanonicalTypeSubstitution,
   675	        /// The FRESH primitive-literal return candidates this winner may
   676	        /// close on: a naked declared return of an unconstrained parameter
   677	        /// fixed to a bare-literal argument. Consulted at close — a final
   678	        /// return equal to one of these is a fresh literal the caller's
   679	        /// return position widens.
   680	        fresh_literal_returns: Vec<SemanticNodeId>,
   681	    },
   682	    /// A UNION callee's per-arm winners: one first-applicable signature in
   683	    /// EVERY callable arm; the close unions the arm returns.
   684	    UnionSelected {
   685	        arms: Vec<ResolveCallUnionArmSelection>,
   686	    },
   687	    DynamicAny,
   688	}
   689	
   690	/// One union-callee arm's staged winner.
   691	#[derive(Debug, Clone, PartialEq, Eq)]
   692	pub(crate) struct ResolveCallUnionArmSelection {
   693	    pub(crate) selected: SignatureCandidateOrigin,
   694	    /// The arm winner's signature node (the sealed deferred carrier node
   695	    /// when the arm's return deferred — the general signature of a lone
   696	    /// winner is minted at close, but a union close has no per-arm return
   697	    /// to mint with, so the carrier node itself is the arm's signature).
   698	    pub(crate) selected_signature: SemanticNodeId,
   699	    pub(crate) substitution: CanonicalTypeSubstitution,
   700	}
   701	
   702	impl ResolveCallSelection {
   703	    /// The winner's fresh primitive-literal return candidates (empty for a
   704	    /// dynamic-`any` selection).
   705	    pub(crate) fn fresh_literal_returns(&self) -> &[SemanticNodeId] {
   706	        match self {
   707	            Self::Selected {
   708	                fresh_literal_returns,
   709	                ..
   710	            } => fresh_literal_returns,
   711	            Self::UnionSelected { .. } | Self::DynamicAny => &[],
   712	        }
   713	    }
   714	
   715	    pub(crate) fn with_return_type(
   716	        &self,
   717	        dispatch: &super::ProjectSemanticDispatch<'_>,
   718	        return_type: SemanticNodeId,
   719	    ) -> ResolvedCallResult {
   720	        match self {
   721	            Self::Selected {
   722	                selected,
   723	                selected_signature,
   724	                substitution,
   725	                fresh_literal_returns,
   726	            } => ResolvedCallResult::Selected {
   727	                selected: selected.clone(),
   728	                selected_signature: match selected_signature {
   729	                    SelectedSignature::General(node) => *node,
   730	                    SelectedSignature::Deferred(callable) => dispatch
   731	                        .graph()
   732	                        .intern_node(callable.clone().into_general_signature(return_type)),
   733	                },
   734	                substitution: substitution.clone(),
   735	                return_type,
   736	                fresh_literal_return: fresh_literal_returns.contains(&return_type),
   737	            },
   738	            Self::UnionSelected { arms } => ResolvedCallResult::UnionSelected {
   739	                selections: Arc::from(
   740	                    arms.iter()
   741	                        .map(|arm| crate::semantic_query::ResolvedUnionArm {
   742	                            selected: arm.selected.clone(),
   743	                            selected_signature: arm.selected_signature,
   744	                            substitution: arm.substitution.clone(),
   745	                        })
   746	                        .collect::<Vec<_>>()
   747	                        .into_boxed_slice(),
   748	                ),
   749	                return_type,
   750	            },
   751	            Self::DynamicAny => ResolvedCallResult::DynamicAny { return_type },
   752	        }
   753	    }
   754	}
   755	
   756	/// The call-resolution-domain deferral payload of a popped member.
   757	#[derive(Debug, Clone)]
   758	pub(crate) struct ResolveCallPendingState {
   759	    /// The fixed winning occurrence/substitution, without a pre-equation
   760	    /// return node.
   761	    pub(crate) selection: ResolveCallSelection,
   762	    /// Concrete return seeds owned by the call (declared return or dynamic
   763	    /// `any`).
   764	    pub(crate) concrete_seeds: Vec<SemanticNodeId>,
   765	    /// Tagged return dependencies (a body-derived winner holds FlowReturn).
   766	    pub(crate) holds: Vec<ReturnObligationIdentity>,
   767	    /// Candidate session staged by this winner, committed only after the
   768	    /// mixed component is stable.
   769	    pub(crate) staged_session: Option<SessionId>,
   770	    /// Relation-only assumptions require a fresh applicability replay at the
   771	    /// component root before the return equation runs.
   772	    pub(crate) replay_applicability: bool,
   773	    /// Store-owned admission for this inline member.
   774	    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineResolveCallFlight>,
   775	    /// The call site's own file roots.
   776	    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
   777	}
   778	
   779	/// The domain deferral payload of a popped member.
   780	#[derive(Debug)]
   781	pub(crate) enum PendingObligationDomain {
   782	    /// Relation deferral state.
   783	    Relate(RelationPendingState),
   784	    /// Flow-return deferral state.
   785	    FlowReturn(FlowReturnPendingState),
   786	    /// ResolveCall deferral state (boxed: the union-selection payload
   787	    /// makes this by far the largest domain).
   788	    ResolveCall(Box<ResolveCallPendingState>),
   789	}
   790	
   791	/// The per-`CheckerDispatchTransaction` pending ledger (design §2.3 step 4
   792	/// R-a): accumulates popped-but-unpublished TAGGED members; the SCC root's
   793	/// close computes each member's published outcome and routes the batch.
   794	#[derive(Debug, Default)]
   795	pub(crate) struct ObligationPendingLedger {
   796	    pending: Vec<PendingObligation>,
   797	}
   798	
   799	impl ObligationPendingLedger {
   800	    pub(crate) fn deposit(&mut self, member: PendingObligation) {
   801	        self.pending.push(member);
   802	    }
   803	
   804	    /// The current pending length — recorded as a frame's drain watermark
   805	    /// at push.
   806	    pub(crate) fn pending_len(&self) -> usize {
   807	        self.pending.len()
   808	    }
   809	
   810	    /// Whether `identity` is an OPEN member of the enclosing component —
   811	    /// deposited here and not yet drained by an SCC close. A hold edge is
   812	    /// only meaningful against such a member; anything else has already
   813	    /// converged to a value.
   814	    pub(crate) fn contains(&self, identity: &ObligationIdentity) -> bool {
   815	        self.pending
   816	            .iter()
   817	            .any(|member| &member.identity == identity)
   818	    }
   819	
   820	    /// Drain every member deposited at or after `watermark` — exactly the
   821	    /// closing frame's own subtree deposits (frames nest strictly and
   822	    /// deposits append, so the suffix from the frame's push-time watermark
   823	    /// IS its SCC membership; stack indices recycle and MUST NOT identify
   824	    /// membership). The drained members are in pop order: deepest-popped
   825	    /// first.
   826	    pub(crate) fn drain_scc(&mut self, watermark: usize) -> Vec<PendingObligation> {
   827	        let split = watermark.min(self.pending.len());
   828	        self.pending.split_off(split)
   829	    }
   830	}
   831	
   832	/// One entry of the tagged provisional substitution table an SCC close
   833	/// installs for its members' re-discharge (design §2.3 step 4 — the
   834	/// converged verdicts a re-running member consults instead of re-entering
   835	/// the SCC).
   836	#[derive(Debug, Clone)]
   837	pub(crate) enum ProvisionalVerdict {
   838	    /// A relation step verdict.
   839	    Relate(RelationStep),
   840	    /// A converged call result used while relation members re-discharge.
   841	    ResolveCall(ResolvedCallResult),
   842	}
   843	
   844	/// The ONE tagged provisional substitution table: SCC members re-discharge
   845	/// deepest-first/root-last against it across domains.
   846	pub(crate) type ProvisionalSubstitution = FxHashMap<ObligationIdentity, ProvisionalVerdict>;
   847	
   848	/// Read a RELATION verdict from the tagged table.
   849	pub(crate) fn provisional_relate_step<'a>(
   850	    substitution: &'a ProvisionalSubstitution,
   851	    key: &RelateMemoKey,
   852	    occurrence: InferenceOccurrence,
   853	) -> Option<&'a RelationStep> {
   854	    match substitution.get(&ObligationIdentity::Relate {
   855	        key: key.clone(),
   856	        occurrence,
   857	    }) {
   858	        Some(ProvisionalVerdict::Relate(step)) => Some(step),
   859	        Some(ProvisionalVerdict::ResolveCall(_)) | None => None,
   860	    }
   861	}
   862	
   863	/// Read a RESOLVE-CALL result from the tagged table.
   864	pub(crate) fn provisional_resolve_call_result<'a>(
   865	    substitution: &'a ProvisionalSubstitution,
   866	    key: &ResolveCallKey,
   867	) -> Option<&'a ResolvedCallResult> {
   868	    match substitution.get(&ObligationIdentity::ResolveCall(key.clone())) {
   869	        Some(ProvisionalVerdict::ResolveCall(result)) => Some(result),
   870	        _ => None,
   871	    }
   872	}
   873	
   874	/// The generic obligation runtime: tagged identities, generic frames /
   875	/// backedges / lowlinks, the generic pending ledger + watermarks, and the
   876	/// tagged provisional substitution table. Domain runtimes own their
   877	/// verdict algebra; this runtime owns the SCC topology.
   878	#[derive(Debug, Default)]
   879	pub(crate) struct ObligationRuntime {
   880	    stack: ObligationReentryStack,
   881	    pending: ObligationPendingLedger,
   882	    substitution: ProvisionalSubstitution,
   883	}
   884	
   885	impl ObligationRuntime {
   886	    pub(crate) fn stack(&self) -> &ObligationReentryStack {
   887	        &self.stack
   888	    }
   889	
   890	    pub(crate) fn stack_mut(&mut self) -> &mut ObligationReentryStack {
   891	        &mut self.stack
   892	    }
   893	
   894	    pub(crate) fn pending(&self) -> &ObligationPendingLedger {
   895	        &self.pending
   896	    }
   897	
   898	    pub(crate) fn pending_mut(&mut self) -> &mut ObligationPendingLedger {
   899	        &mut self.pending
   900	    }
   901	
   902	    pub(crate) fn substitution(&self) -> &ProvisionalSubstitution {
   903	        &self.substitution
   904	    }
   905	
   906	    /// Whether the next obligation push is a ROOT push (the generic stack
   907	    /// is empty). Root versus inline is decided HERE, at the generic
   908	    /// transaction — a nested obligation of any domain under an open frame
   909	    /// is inline because the generic root owns its eventual drain.
   910	    pub(crate) fn decides_root(&self) -> bool {
   911	        self.stack.is_empty()
   912	    }
   913	
   914	    /// Record an assumption edge `top → target` (the coinductive "assume
   915	    /// it holds" step, design §2.2): the caller's accumulator is marked
   916	    /// `OpenAssumption(target)` — transient, NEVER written to a published
   917	    /// `ReadSetSignature.facts`.
   918	    pub(crate) fn record_assumption(&mut self, target: usize) {
   919	        if let Some(frame) = self.stack.top_mut() {
   920	            frame.assumption_targets.push(target);
   921	            frame.min_open_target = Some(
   922	                frame
   923	                    .min_open_target
   924	                    .map_or(target, |current| current.min(target)),
   925	            );
   926	        }
   927	    }
   928	
   929	    /// Fold a popped child's still-open lowlink into the (new) top frame:
   930	    /// an assumption the child recorded against a frame BELOW it stays
   931	    /// open against the parent after the child pops. This folds through
   932	    /// EVERY generic frame, including non-relation frames between two
   933	    /// relation frames.
   934	    pub(crate) fn propagate_lowlink(&mut self, child_min_open: Option<usize>) {
   935	        let Some(child_min_open) = child_min_open else {
   936	            return;
   937	        };
   938	        if let Some(frame) = self.stack.top_mut() {
   939	            frame.min_open_target = Some(
   940	                frame
   941	                    .min_open_target
   942	                    .map_or(child_min_open, |current| current.min(child_min_open)),
   943	            );
   944	        }
   945	    }
   946	
   947	    /// Install one SCC re-discharge context (the tagged substitution table)
   948	    /// and return the complete previous context so a nested re-discharge
   949	    /// can restore its caller exactly. The relation occurrence rail rides
   950	    /// the relation domain runtime; this installs the tagged table only.
   951	    pub(crate) fn replace_substitution(
   952	        &mut self,
   953	        substitution: ProvisionalSubstitution,
   954	    ) -> ProvisionalSubstitution {
   955	        std::mem::replace(&mut self.substitution, substitution)
   956	    }
   957	
   958	    /// Restore a previously saved substitution table.
   959	    pub(crate) fn restore_substitution(&mut self, saved: ProvisionalSubstitution) {
   960	        self.substitution = saved;
   961	    }
   962	}
   963	
   964	// ---------------------------------------------------------------------------
   965	// Relation domain runtime
   966	// ---------------------------------------------------------------------------
   967	
   968	/// Lifecycle of an in-flight inference session.
   969	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
   970	pub(crate) enum InferenceSessionState {
   971	    /// Still collecting candidates — NOT converged (ReturnOnly).
   972	    Collecting,
   973	    /// Fixation completed deterministically. The binding snapshot is
   974	    /// immutable and the session is inactive for deposits, but it has not
   975	    /// crossed the atomic publication boundary.
   976	    StagedDeterministic,
   977	    /// The staged snapshot crossed its stability gate. The ONLY state that
   978	    /// admits when its ledger is atomically drained.
   979	    CommittedDeterministic,
   980	    /// Cancel / budget-exceeded / superseded / non-deterministic — the
   981	    /// deferred batch releases WITHOUT publish (ReturnOnly).
   982	    Abandoned,
   983	}
   984	
   985	/// A rollback point over an [`InferenceSession`]'s per-parameter candidate

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '390,575p'; nl -ba crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '1680,1925p'; nl -ba crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs | sed -n '1920,2205p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 17381ms:[0m
   390	    }
   391	}
   392	
   393	/// The ONE shared re-entry / cycle-id space (design §2.1). Heap-backed,
   394	/// per-`CheckerDispatchTransaction`, keyed by tagged full identity.
   395	#[derive(Debug, Default)]
   396	pub(crate) struct ObligationReentryStack {
   397	    frames: Vec<ObligationFrame>,
   398	    index: FxHashMap<ObligationIdentity, usize>,
   399	}
   400	
   401	impl ObligationReentryStack {
   402	    /// The stack index of `identity` when that tagged obligation is already
   403	    /// in flight on THIS transaction.
   404	    pub(crate) fn find(&self, identity: &ObligationIdentity) -> Option<usize> {
   405	        self.index.get(identity).copied()
   406	    }
   407	
   408	    pub(crate) fn is_empty(&self) -> bool {
   409	        self.frames.is_empty()
   410	    }
   411	
   412	    pub(crate) fn depth(&self) -> usize {
   413	        self.frames.len()
   414	    }
   415	
   416	    /// Push a fresh RELATION frame for `(key, occurrence)` with the pending
   417	    /// ledger's current length as its drain watermark; returns its stack
   418	    /// index.
   419	    pub(crate) fn push_relate(
   420	        &mut self,
   421	        key: RelateMemoKey,
   422	        occurrence: InferenceOccurrence,
   423	        pending_watermark: usize,
   424	    ) -> usize {
   425	        let identity = ObligationIdentity::Relate { key, occurrence };
   426	        let idx = self.frames.len();
   427	        self.frames.push(ObligationFrame {
   428	            identity: identity.clone(),
   429	            assumption_targets: Vec::new(),
   430	            min_open_target: None,
   431	            budget_cap: None,
   432	            pending_watermark,
   433	            domain: ObligationFrameDomain::Relate(RelationFrameState::new()),
   434	        });
   435	        self.index.insert(identity, idx);
   436	        idx
   437	    }
   438	
   439	    /// Push a fresh FLOW-RETURN frame for `key` with the pending ledger's
   440	    /// current length as its drain watermark; returns its stack index.
   441	    pub(crate) fn push_flow_return(
   442	        &mut self,
   443	        key: FlowReturnKey,
   444	        pending_watermark: usize,
   445	    ) -> usize {
   446	        let identity = ObligationIdentity::FlowReturn(key);
   447	        let idx = self.frames.len();
   448	        self.frames.push(ObligationFrame {
   449	            identity: identity.clone(),
   450	            assumption_targets: Vec::new(),
   451	            min_open_target: None,
   452	            budget_cap: None,
   453	            pending_watermark,
   454	            domain: ObligationFrameDomain::FlowReturn(FlowReturnFrameState::default()),
   455	        });
   456	        self.index.insert(identity, idx);
   457	        idx
   458	    }
   459	
   460	    /// Push a transparent RESOLVE-CALL executor frame. The frame exists before
   461	    /// candidate sessions or argument relations are opened, so the generic
   462	    /// transaction — not a relation-domain special case — classifies those
   463	    /// relations as inline.
   464	    pub(crate) fn push_resolve_call(
   465	        &mut self,
   466	        key: ResolveCallKey,
   467	        pending_watermark: usize,
   468	    ) -> usize {
   469	        let identity = ObligationIdentity::ResolveCall(key);
   470	        let idx = self.frames.len();
   471	        self.frames.push(ObligationFrame {
   472	            identity: identity.clone(),
   473	            assumption_targets: Vec::new(),
   474	            min_open_target: None,
   475	            budget_cap: None,
   476	            pending_watermark,
   477	            domain: ObligationFrameDomain::ResolveCall(ResolveCallFrameState::default()),
   478	        });
   479	        self.index.insert(identity, idx);
   480	        idx
   481	    }
   482	
   483	    /// Pop the top frame. Callers uphold strict LIFO nesting (the
   484	    /// transaction's execution model).
   485	    pub(crate) fn pop(&mut self) -> ObligationFrame {
   486	        let frame = self.frames.pop().expect("reentry stack underflow");
   487	        self.index.remove(&frame.identity);
   488	        frame
   489	    }
   490	
   491	    pub(crate) fn top_mut(&mut self) -> Option<&mut ObligationFrame> {
   492	        self.frames.last_mut()
   493	    }
   494	
   495	    /// Record a budget edge on the frame at `idx` (poisons its SCC).
   496	    pub(crate) fn note_budget_edge(&mut self, idx: usize, cap: RecursionOrBudgetCap) {
   497	        if let Some(frame) = self.frames.get_mut(idx) {
   498	            if frame.budget_cap.is_none() {
   499	                frame.budget_cap = Some(cap);
   500	            }
   501	        }
   502	    }
   503	
   504	    /// The frame at `idx`, when in range.
   505	    pub(crate) fn frame(&self, idx: usize) -> Option<&ObligationFrame> {
   506	        self.frames.get(idx)
   507	    }
   508	
   509	    pub(crate) fn assumption_evidence(&self, target: usize) -> RelationAssumptionEvidence {
   510	        let closure = self.frames[target..]
   511	            .iter()
   512	            .map(|frame| frame.identity.clone())
   513	            .collect::<Vec<_>>();
   514	        RelationAssumptionEvidence {
   515	            closure: Arc::from(closure.into_boxed_slice()),
   516	        }
   517	    }
   518	
   519	    /// The frame at `idx` mutably, when in range.
   520	    pub(crate) fn frame_mut_for_update(&mut self, idx: usize) -> Option<&mut ObligationFrame> {
   521	        self.frames.get_mut(idx)
   522	    }
   523	
   524	    /// The nearest open RELATION frame's identity parts, walking from the
   525	    /// top of the GENERIC stack down. Relation subkeys inherit their axes
   526	    /// from the nearest open `Relate` ancestor — never the untyped top of
   527	    /// a mixed stack (a non-relation frame between two relation frames
   528	    /// carries no relation axes to inherit).
   529	    pub(crate) fn nearest_relate(&self) -> Option<(&RelateMemoKey, InferenceOccurrence)> {
   530	        self.frames
   531	            .iter()
   532	            .rev()
   533	            .find_map(|frame| frame.identity.as_relate())
   534	    }
   535	
   536	    /// Attach a resolved-call hold to the nearest active flow frame.
   537	    /// Returns false when the caller is not executing inside FlowReturn,
   538	    /// or when the identity is a bare flow return (a flow hold without
   539	    /// its instantiation clause cannot be transferred — the evaluator's
   540	    /// own callee gate records those).
   541	    pub(crate) fn record_nearest_flow_hold(&mut self, hold: ReturnObligationIdentity) -> bool {
   542	        if matches!(hold, ReturnObligationIdentity::FlowReturn(_)) {
   543	            return false;
   544	        }
   545	        let Some(state) = self
   546	            .frames
   547	            .iter_mut()
   548	            .rev()
   549	            .find_map(|frame| match &mut frame.domain {
   550	                ObligationFrameDomain::FlowReturn(state) => Some(state),
   551	                ObligationFrameDomain::Relate(_) | ObligationFrameDomain::ResolveCall(_) => None,
   552	            })
   553	        else {
   554	            return false;
   555	        };
   556	        if !state.holds.contains(&hold) {
   557	            state.holds.push(hold);
   558	        }
   559	        true
   560	    }
   561	}
   562	
   563	/// A popped SCC member awaiting its SCC root's close — the PROVISIONAL
   564	/// deferral record (design §2.3 step 4): a caller-return value plus
   565	/// deferral metadata, NEVER the published payload. The published payload
   566	/// is produced at the batched-publish instant by the discharge against
   567	/// converged state.
   568	#[derive(Debug)]
   569	pub(crate) struct PendingObligation {
   570	    /// The member's tagged full identity.
   571	    pub(crate) identity: ObligationIdentity,
   572	    /// The domain deferral payload.
   573	    pub(crate) domain: PendingObligationDomain,
   574	}
   575	
  1680	    let contravariant: Vec<SemanticNodeId> = chosen
  1681	        .iter()
  1682	        .filter(|candidate| candidate.variance == VariancePhase::Contravariant)
  1683	        .map(|candidate| candidate.node)
  1684	        .collect();
  1685	    if contravariant.is_empty() {
  1686	        (
  1687	            chosen.iter().map(|candidate| candidate.node).collect(),
  1688	            VariancePhase::Covariant,
  1689	        )
  1690	    } else {
  1691	        (contravariant, VariancePhase::Contravariant)
  1692	    }
  1693	}
  1694	
  1695	/// The decided provisional verdict of a popped member.
  1696	#[derive(Debug, Clone)]
  1697	pub(crate) enum PendingVerdict {
  1698	    Assignable { bindings: Arc<[InferBinding]> },
  1699	    NotAssignable,
  1700	    Unknown,
  1701	    BudgetExceeded(RecursionOrBudgetCap),
  1702	}
  1703	
  1704	/// Session-close publication stability gate. The SCC-close snapshot is
  1705	/// provisional: redischarge may publish only when its polarity and complete
  1706	/// fixed-binding snapshot are unchanged. Proof shape is then deterministic
  1707	/// from that verdict plus the unchanged SCC key set; `Unknown`/budget states
  1708	/// are never stable publication candidates.
  1709	pub(crate) fn redischarge_is_stable(
  1710	    provisional: &PendingVerdict,
  1711	    redischarge: &PendingVerdict,
  1712	) -> bool {
  1713	    match (provisional, redischarge) {
  1714	        (
  1715	            PendingVerdict::Assignable {
  1716	                bindings: provisional,
  1717	            },
  1718	            PendingVerdict::Assignable {
  1719	                bindings: redischarge,
  1720	            },
  1721	        ) => provisional == redischarge,
  1722	        (PendingVerdict::NotAssignable, PendingVerdict::NotAssignable) => true,
  1723	        _ => false,
  1724	    }
  1725	}
  1726	
  1727	/// The per-session deferred-admission ledger (design §2.3 step 4 /
  1728	/// §3.3): binding members of a not-yet-closed SCC record here at
  1729	/// SCC-close; the drain at the relevant session's `CommittedDeterministic`
  1730	/// close re-discharges against the converged state and publishes ONLY a
  1731	/// stable determined outcome (flip / abandonment publishes nothing).
  1732	#[derive(Debug, Default)]
  1733	pub(crate) struct SessionAdmissionLedger {
  1734	    deferred: std::collections::BTreeMap<SessionId, Vec<RelateMemoKey>>,
  1735	}
  1736	
  1737	impl SessionAdmissionLedger {
  1738	    /// Record `key` as deferred on session `session`'s close.
  1739	    pub(crate) fn defer(&mut self, session: SessionId, key: RelateMemoKey) {
  1740	        self.deferred.entry(session).or_default().push(key);
  1741	    }
  1742	
  1743	    /// Validate a deferred member without consuming the ledger. Mixed
  1744	    /// return components perform every fallible check before committing
  1745	    /// call sessions, then drain in the no-semantic-work publication tail.
  1746	    pub(crate) fn contains(&self, session: SessionId, key: &RelateMemoKey) -> bool {
  1747	        self.deferred
  1748	            .get(&session)
  1749	            .is_some_and(|keys| keys.iter().any(|candidate| candidate == key))
  1750	    }
  1751	
  1752	    /// Drain every key deferred on `session` (at that session's close).
  1753	    pub(crate) fn drain(&mut self, session: SessionId) -> Vec<RelateMemoKey> {
  1754	        self.deferred.remove(&session).unwrap_or_default()
  1755	    }
  1756	}
  1757	
  1758	/// A member whose SCC closed cleanly, queued for the batched publish the
  1759	/// relation ROOT performs after its family-memo publish lands (the member
  1760	/// entries ride the ROOT's SCC-union carrier — design §2.3 step 3: the
  1761	/// published fact set is never the bare per-member set).
  1762	#[derive(Debug)]
  1763	pub(crate) struct CompletedSccMember {
  1764	    pub(crate) key: RelateMemoKey,
  1765	    pub(crate) payload: RelationPayload,
  1766	    pub(crate) inline_flight: Option<InlineRelationFlight>,
  1767	}
  1768	
  1769	/// The relation domain runtime: inference sessions, relation provisional
  1770	/// payloads, and relation redischarge/fixation state. The SCC topology it
  1771	/// runs on lives in the generic [`ObligationRuntime`].
  1772	#[derive(Debug, Default)]
  1773	pub(crate) struct RelationDomainRuntime {
  1774	    /// The active inference-session stack.
  1775	    pub(crate) sessions: Vec<InferenceSession>,
  1776	    /// Per-session deferred-admission ledger.
  1777	    pub(crate) session_admission: SessionAdmissionLedger,
  1778	    /// SCC-closed members queued for the root's batched publish drain.
  1779	    pub(crate) completed_members: Vec<CompletedSccMember>,
  1780	    /// The normalized strict-family configuration in force (RI-10).
  1781	    pub(crate) strict: Option<StrictFamilyConfig>,
  1782	    /// Virtual root occurrence used while an SCC member re-discharges after
  1783	    /// its real frame has been popped. The recorded stack depth lets nested
  1784	    /// frames take over normally while preserving the popped member's
  1785	    /// orientation at the virtual root.
  1786	    pub(crate) redischarge_occurrence: Option<(usize, InferenceOccurrence)>,
  1787	    /// Per-target-node memo of the `infer`-pattern detection (a pure
  1788	    /// function of the pattern; avoids rescanning per ask).
  1789	    pub(crate) pattern_cache: FxHashMap<SemanticNodeId, Option<super::relation::InferPatternInfo>>,
  1790	    /// Nestable call-applicability final-check barriers. Sessions below the
  1791	    /// newest length watermark are inactive; a genuinely nested call may push
  1792	    /// a fresh session above it and infer normally.
  1793	    binding_disabled_session_barriers: Vec<usize>,
  1794	    /// Literal interpretation for the current call argument relation. Empty
  1795	    /// outside call-owned collection, so ordinary relation inference is
  1796	    /// unchanged.
  1797	    call_argument_literal_modes: Vec<CallArgumentLiteralPolicy>,
  1798	    /// Monotonic count of ACCEPTED session deposits (ordinary, reverse
  1799	    /// aggregate, and projection), bumped at each acceptance site. The
  1800	    /// call executor charges its `inference_deposits` fuse from deltas of
  1801	    /// this counter, so the fuse's unit is the accepted deposit itself —
  1802	    /// never one unit per top-level argument.
  1803	    pub(crate) accepted_inference_deposits: u64,
  1804	    next_session_id: u64,
  1805	}
  1806	
  1807	/// One in-flight call-argument relation's literal policy: the argument's
  1808	/// authored literal mode plus the parameter positions its declared TARGET
  1809	/// exposes at TOP LEVEL (the naked type-parameter set — the parameter
  1810	/// itself or a union / intersection arm). A deposit into a top-level
  1811	/// position preserves a primitive-literal candidate (TypeScript's naked
  1812	/// inference, constrained or not); a nested deposit widens under the
  1813	/// parameter's const policy.
  1814	#[derive(Debug)]
  1815	struct CallArgumentLiteralPolicy {
  1816	    literal_mode: Option<crate::semantic_query::ArgumentLiteralMode>,
  1817	    top_level_infer_targets: Vec<SemanticNodeId>,
  1818	}
  1819	
  1820	/// Saved transient state for a nested SCC re-discharge. Persistent relation
  1821	/// identity is unaffected; this restores only the virtual occurrence and
  1822	/// substitution rails used by the enclosing re-discharge.
  1823	pub(crate) struct SavedRedischargeContext {
  1824	    substitution: ProvisionalSubstitution,
  1825	    occurrence: Option<(usize, InferenceOccurrence)>,
  1826	}
  1827	
  1828	/// The per-obligation-root cold-compute frame (design §2.1 /
  1829	/// `native-typeinfo-parity.md` §4.2): ONE tagged obligation runtime plus
  1830	/// the per-domain runtimes. Transient; NEVER a cache key.
  1831	/// A flow member whose SCC closed cleanly, queued for the batched
  1832	/// publish the relation ROOT performs after its family-memo publish
  1833	/// lands (the member entries ride the ROOT's SCC-union carrier — the
  1834	/// published fact set is the UNION of all SCC members' observed facts).
  1835	#[derive(Debug)]
  1836	pub(crate) struct CompletedFlowReturnMember {
  1837	    pub(crate) key: FlowReturnKey,
  1838	    pub(crate) result: FlowReturnResult,
  1839	    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
  1840	    /// The member's own file roots (the SCC-union carrier's self-roots
  1841	    /// include them even when the ROOT is a relation obligation).
  1842	    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
  1843	    /// The materialised point set the member's compute ACTUALLY produced
  1844	    /// (§3.4) — carried to the fenced member publish.
  1845	    pub(crate) materialized: crate::semantic_query::demand::MaterializedSet,
  1846	}
  1847	
  1848	/// A call member whose mixed component closed cleanly, queued for the
  1849	/// root's completed-member drain — fenced backfill behind the root's
  1850	/// committing admission, never a second commit boundary.
  1851	#[derive(Debug)]
  1852	pub(crate) struct CompletedResolveCallMember {
  1853	    pub(crate) key: ResolveCallKey,
  1854	    /// The admitted result. A rootless winner cannot be represented here,
  1855	    /// so it never reaches the shared cache.
  1856	    pub(crate) result: crate::semantic_query::AdmissibleCallResult,
  1857	    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineResolveCallFlight>,
  1858	    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
  1859	}
  1860	
  1861	/// The flow-return domain runtime: the completed flow members queued
  1862	/// for the relation root's batched publish. Contributor maps ride the
  1863	/// in-flight frames and the tagged pending ledger (a popped member's
  1864	/// decided outcome is final at pop — same-slot recursive edges are
  1865	/// coinductive holds, never unresolved failures), so the domain owns
  1866	/// no parallel contributor ledger.
  1867	#[derive(Debug, Default)]
  1868	pub(crate) struct FlowReturnDomainRuntime {
  1869	    /// SCC-closed flow members queued for the root's batched publish
  1870	    /// drain.
  1871	    pub(crate) completed_members: Vec<CompletedFlowReturnMember>,
  1872	    /// The typed failure of the in-flight machinery ROOT's close — the
  1873	    /// caller-return payload channel: the family memo admits only COMPLETE
  1874	    /// values, so a degraded root's typed failure rides the transaction to
  1875	    /// the demanding caller (never admitted, never a cache value).
  1876	    pub(crate) last_root_failure: Option<crate::semantic_query::FlowReturnFailure>,
  1877	}
  1878	
  1879	/// The call-resolution domain runtime.
  1880	#[derive(Debug, Default)]
  1881	pub(crate) struct ResolveCallDomainRuntime {
  1882	    /// Mixed-component members awaiting the root carrier's drain (fenced
  1883	    /// backfill behind the root's committing admission).
  1884	    pub(crate) completed_members: Vec<CompletedResolveCallMember>,
  1885	    /// Typed failure channel for a machinery-root call whose family value is
  1886	    /// suppressed.
  1887	    pub(crate) last_root_failure: Option<ResolveCallFailure>,
  1888	}
  1889	
  1890	/// The per-obligation-root cold-compute frame (design §2.1 /
  1891	/// `native-typeinfo-parity.md` §4.2): ONE tagged obligation runtime plus
  1892	/// the per-domain runtimes. Transient; NEVER a cache key.
  1893	#[derive(Debug, Default)]
  1894	pub(crate) struct CheckerDispatchTransaction {
  1895	    /// The generic obligation runtime (tagged identities, frames, pending
  1896	    /// ledger, watermarks, the tagged provisional substitution table).
  1897	    pub(crate) obligations: ObligationRuntime,
  1898	    /// The relation domain runtime.
  1899	    pub(crate) relation: RelationDomainRuntime,
  1900	    /// The flow-return domain runtime.
  1901	    pub(crate) flow: FlowReturnDomainRuntime,
  1902	    /// The call-resolution domain runtime.
  1903	    pub(crate) call: ResolveCallDomainRuntime,
  1904	}
  1905	
  1906	/// One entry of a tagged flow component awaiting its equation fixed
  1907	/// point: the member's current outcome (a Complete outcome IS its
  1908	/// concrete seed join; a hold-only EmptyCycle has no seed) and the
  1909	/// coinductive hold targets its evaluation met.
  1910	#[derive(Debug, Clone)]
  1911	pub(super) struct FlowDischargeEntry {
  1912	    /// The member's flow identity.
  1913	    pub(super) key: crate::semantic_query::FlowReturnKey,
  1914	    /// The member's outcome (updated in place by the discharge).
  1915	    pub(super) outcome: FlowReturnPendingOutcome,
  1916	    /// The member's coinductive hold targets, each carrying the
  1917	    /// instantiation obligation the fixed point owes its callee.
  1918	    pub(super) holds: Vec<super::flow_return_callee::HeldCallee>,
  1919	    /// Whether the member's own contributors were all FRESH literals —
  1920	    /// the post-convergence literal-widening input.
  1921	    pub(super) fresh_seed: bool,
  1922	}
  1923	
  1924	/// Identity of a member in the shared return equation. This is deliberately
  1925	/// separate from [`ObligationIdentity`]: relations share SCC topology but do
  1920	    /// the post-convergence literal-widening input.
  1921	    pub(super) fresh_seed: bool,
  1922	}
  1923	
  1924	/// Identity of a member in the shared return equation. This is deliberately
  1925	/// separate from [`ObligationIdentity`]: relations share SCC topology but do
  1926	/// not inhabit the return lattice.
  1927	#[derive(Debug, Clone, PartialEq, Eq, Hash)]
  1928	pub(crate) enum ReturnObligationIdentity {
  1929	    FlowReturn(FlowReturnKey),
  1930	    ResolveCall(ResolveCallKey),
  1931	}
  1932	
  1933	/// Domain-specific metadata retained beside the shared return lattice.
  1934	#[derive(Debug, Clone, PartialEq, Eq)]
  1935	pub(crate) enum ReturnDomainMetadata {
  1936	    // Exercised by the solver's own contract tests: production flow members
  1937	    // discharge through the callee-clause fixed point and never enter the
  1938	    // call equation.
  1939	    #[allow(dead_code)]
  1940	    FlowReturn {
  1941	        can_fall_through: bool,
  1942	    },
  1943	    ResolveCall,
  1944	}
  1945	
  1946	/// One member of the multi-domain return equation.
  1947	#[derive(Debug, Clone, PartialEq, Eq)]
  1948	pub(crate) struct ReturnEquationMember {
  1949	    /// The member's FRESH primitive-literal return candidates (ResolveCall
  1950	    /// members only; a flow member's own position already widened its
  1951	    /// fresh leaves). A FLOW-domain consumer widens a contributed leaf
  1952	    /// equal to one of these.
  1953	    pub(crate) fresh_literal_returns: Vec<SemanticNodeId>,
  1954	    pub(crate) identity: ReturnObligationIdentity,
  1955	    pub(crate) concrete_seeds: Vec<SemanticNodeId>,
  1956	    pub(crate) holds: Vec<ReturnObligationIdentity>,
  1957	    pub(crate) domain: ReturnDomainMetadata,
  1958	}
  1959	
  1960	/// Failure of the shared equation. Both cases poison the whole mixed
  1961	/// component and admit nothing.
  1962	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
  1963	pub(crate) enum ReturnEquationFailure {
  1964	    EmptyCycle,
  1965	    UnresolvedOutsideHold,
  1966	}
  1967	
  1968	impl CheckerDispatchTransaction {
  1969	    pub(crate) fn reentry(&self) -> &ObligationReentryStack {
  1970	        self.obligations.stack()
  1971	    }
  1972	
  1973	    pub(crate) fn reentry_mut(&mut self) -> &mut ObligationReentryStack {
  1974	        self.obligations.stack_mut()
  1975	    }
  1976	
  1977	    pub(crate) fn alloc_session_id(&mut self) -> SessionId {
  1978	        self.relation.next_session_id += 1;
  1979	        SessionId(self.relation.next_session_id)
  1980	    }
  1981	
  1982	    /// Push a fresh collecting session unconditionally. Call-owned candidate
  1983	    /// execution uses this path even when an outer collector exists; relation
  1984	    /// roots retain their separate admission predicate.
  1985	    pub(crate) fn push_collecting_session(
  1986	        &mut self,
  1987	        setup: InferenceSessionSetup,
  1988	        reverse_projection: Option<ReverseProjectionState>,
  1989	    ) -> SessionId {
  1990	        let id = self.alloc_session_id();
  1991	        self.relation
  1992	            .sessions
  1993	            .push(InferenceSession::new(id, setup, reverse_projection));
  1994	        id
  1995	    }
  1996	
  1997	    /// The active (innermost `Collecting`) session, if any.
  1998	    pub(crate) fn active_session_mut(&mut self) -> Option<&mut InferenceSession> {
  1999	        let start = self
  2000	            .relation
  2001	            .binding_disabled_session_barriers
  2002	            .last()
  2003	            .copied()
  2004	            .unwrap_or(0);
  2005	        self.relation
  2006	            .sessions
  2007	            .get_mut(start..)?
  2008	            .iter_mut()
  2009	            .rev()
  2010	            .find(|s| s.state == InferenceSessionState::Collecting)
  2011	    }
  2012	
  2013	    pub(crate) fn active_session(&self) -> Option<&InferenceSession> {
  2014	        let start = self
  2015	            .relation
  2016	            .binding_disabled_session_barriers
  2017	            .last()
  2018	            .copied()
  2019	            .unwrap_or(0);
  2020	        self.relation
  2021	            .sessions
  2022	            .get(start..)?
  2023	            .iter()
  2024	            .rev()
  2025	            .find(|s| s.state == InferenceSessionState::Collecting)
  2026	    }
  2027	
  2028	    pub(crate) fn binding_is_disabled(&self) -> bool {
  2029	        !self.relation.binding_disabled_session_barriers.is_empty()
  2030	    }
  2031	
  2032	    pub(crate) fn begin_binding_disabled(&mut self) {
  2033	        self.relation
  2034	            .binding_disabled_session_barriers
  2035	            .push(self.relation.sessions.len());
  2036	    }
  2037	
  2038	    pub(crate) fn end_binding_disabled(&mut self) {
  2039	        self.relation
  2040	            .binding_disabled_session_barriers
  2041	            .pop()
  2042	            .expect("binding-disabled barrier underflow");
  2043	    }
  2044	
  2045	    pub(crate) fn begin_call_argument(
  2046	        &mut self,
  2047	        literal_mode: Option<crate::semantic_query::ArgumentLiteralMode>,
  2048	        top_level_infer_targets: Vec<SemanticNodeId>,
  2049	    ) {
  2050	        self.relation
  2051	            .call_argument_literal_modes
  2052	            .push(CallArgumentLiteralPolicy {
  2053	                literal_mode,
  2054	                top_level_infer_targets,
  2055	            });
  2056	    }
  2057	
  2058	    pub(crate) fn end_call_argument(&mut self) {
  2059	        self.relation
  2060	            .call_argument_literal_modes
  2061	            .pop()
  2062	            .expect("call-argument literal-mode stack underflow");
  2063	    }
  2064	
  2065	    pub(crate) fn call_argument_literal_mode(
  2066	        &self,
  2067	    ) -> Option<crate::semantic_query::ArgumentLiteralMode> {
  2068	        self.relation
  2069	            .call_argument_literal_modes
  2070	            .last()
  2071	            .and_then(|policy| policy.literal_mode)
  2072	    }
  2073	
  2074	    /// Whether the CURRENT call argument's declared TARGET exposes
  2075	    /// `param_node` at top level (a naked type-parameter position — the
  2076	    /// parameter itself, or a union / intersection arm of it). A deposit
  2077	    /// into a top-level position preserves a primitive-literal candidate;
  2078	    /// a nested deposit widens it.
  2079	    pub(crate) fn call_argument_target_is_top_level(&self, param_node: SemanticNodeId) -> bool {
  2080	        self.relation
  2081	            .call_argument_literal_modes
  2082	            .last()
  2083	            .is_some_and(|policy| policy.top_level_infer_targets.contains(&param_node))
  2084	    }
  2085	
  2086	    /// The session the frame at `idx` opened, if any.
  2087	    pub(crate) fn frame_opened_session(&self, idx: usize) -> Option<SessionId> {
  2088	        self.reentry()
  2089	            .frame(idx)
  2090	            .and_then(|frame| frame.relation())
  2091	            .and_then(|state| state.opened_session)
  2092	    }
  2093	
  2094	    /// Mark the frame at `idx` as having opened session `session`.
  2095	    pub(crate) fn note_opened_session(&mut self, idx: usize, session: SessionId) {
  2096	        if let Some(state) = self
  2097	            .reentry_mut()
  2098	            .frame_mut_for_update(idx)
  2099	            .and_then(ObligationFrame::relation_mut)
  2100	        {
  2101	            state.opened_session = Some(session);
  2102	        }
  2103	    }
  2104	
  2105	    pub(crate) fn note_inline_flight(&mut self, idx: usize, flight: Option<InlineRelationFlight>) {
  2106	        if let Some(state) = self
  2107	            .reentry_mut()
  2108	            .frame_mut_for_update(idx)
  2109	            .and_then(ObligationFrame::relation_mut)
  2110	        {
  2111	            state.inline_flight = flight;
  2112	        }
  2113	    }
  2114	
  2115	    pub(crate) fn note_session_delta_range(&mut self, start: usize, end: usize) {
  2116	        for idx in start..end {
  2117	            if let Some(state) = self
  2118	                .reentry_mut()
  2119	                .frame_mut_for_update(idx)
  2120	                .and_then(ObligationFrame::relation_mut)
  2121	            {
  2122	                state.session_delta = true;
  2123	            }
  2124	        }
  2125	    }
  2126	
  2127	    /// Mark every active non-owner frame when an accepted candidate write
  2128	    /// mutates an outer session.
  2129	    pub(crate) fn note_candidate_write(&mut self, active_id: Option<SessionId>) {
  2130	        let depth = self.reentry().depth();
  2131	        if depth == 0 {
  2132	            return;
  2133	        }
  2134	        let owner = (0..depth).rev().find(|index| {
  2135	            self.frame_opened_session(*index)
  2136	                .is_some_and(|opened| Some(opened) == active_id)
  2137	        });
  2138	        let first_non_owner = owner.map_or(0, |index| index + 1);
  2139	        self.note_session_delta_range(first_non_owner, depth);
  2140	    }
  2141	
  2142	    /// Install one SCC re-discharge context and return the complete previous
  2143	    /// context so a nested re-discharge can restore its caller exactly.
  2144	    pub(crate) fn replace_redischarge_context(
  2145	        &mut self,
  2146	        substitution: ProvisionalSubstitution,
  2147	        occurrence: InferenceOccurrence,
  2148	    ) -> SavedRedischargeContext {
  2149	        let previous_substitution = self.obligations.replace_substitution(substitution);
  2150	        let depth = self.reentry().depth();
  2151	        let previous_occurrence = self
  2152	            .relation
  2153	            .redischarge_occurrence
  2154	            .replace((depth, occurrence));
  2155	        SavedRedischargeContext {
  2156	            substitution: previous_substitution,
  2157	            occurrence: previous_occurrence,
  2158	        }
  2159	    }
  2160	
  2161	    pub(crate) fn restore_redischarge_context(&mut self, saved: SavedRedischargeContext) {
  2162	        self.obligations.restore_substitution(saved.substitution);
  2163	        self.relation.redischarge_occurrence = saved.occurrence;
  2164	    }
  2165	}
  2166	
  2167	#[cfg(test)]
  2168	#[path = "dispatch_txn_tests.rs"]
  2169	mod dispatch_txn_tests;

[35m[3mcodex[0m[0m
The C1 boundary is now clear: its “flow exclusion” excludes semantic changes, not files. The accepted C1 ruling moves the whole `project_semantic_dispatch` module; the source tree itself declares both `dispatch_txn` and `flow_return` as children of that module. I’m now resolving the harder point: whether A6 actually contains any enforceable flow rows, rather than inferring flow coverage from broad product labels.
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/contracts/capability-matrix.md; nl -ba docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md | sed -n '280,350p'; nl -ba docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md | sed -n '840,920p'; nl -ba docs/arch/refactor/rev11/evidence/A6-summary.md | sed -n '130,190p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 25665ms:[0m
     1	# Framework and Product Capability Matrix
     2	
     3	**Status:** Normative product truth after baseline lock.  
     4	**Current state:** seed rows as ratified at the A0-accepted base; every `Status` cell is exactly `VERIFY` — A1 ratifies no maturity, default, or compatibility cell, and every non-`VERIFY` seed cell (the Svelte experimental maturity/promise, the graph-export maturity/default/promise, the LSP default/promise, and the seeded degradation/zero-work cells) is the Revision 11 plan's own seed, carried unaltered from the A0-accepted base under the authority of that acceptance. A1 contributes execution evidence only: §2.1 maps each row to its canonical selectors and to the externally retained A1 evidence bundle; per-run counts, verdicts, receipts, and recorded pre-existing failures live exclusively in that bundle (`A1/command-proofs/index.md`, `A1/sentinel-verification.md` under the program's external evidence root) and are never inlined here, so this file is byte-stable across evidence re-runs. A3 updates any fail-closed behavior, and A5/A6 finalize the exact post-safety matrix. Affected product blocks cannot start until completed.
     5	
     6	# 1. Row schema
     7	
     8	| Framework/product | Operation | Route/backend | Maturity | Default | Semantic profile(s) | Oracle/conformance corpus | Exact unsupported/degradation behavior | Zero-work negative proof | Compatibility promise | Status |
     9	|---|---|---|---|---|---|---|---|---|---|---|
    10	
    11	# 2. Seed rows
    12	
    13	| Framework/product | Operation | Route/backend | Maturity | Default | Semantic profile(s) | Oracle/conformance corpus | Exact unsupported/degradation behavior | Zero-work negative proof | Compatibility promise | Status |
    14	|---|---|---|---|---|---|---|---|---|---|---|
    15	| Vue | runtime compile | direct Rust | VERIFY | VERIFY | VERIFY | official Vue fixtures + Verter corpus | VERIFY | no IDE/public/native enrichment work | VERIFY | VERIFY |
    16	| Vue | IDE companion | managed/provider | VERIFY | VERIFY | provider-specific | provider + mapping corpus | typed route/capability failure | no runtime constructor projection unless demanded | VERIFY | VERIFY |
    17	| Vue | imported macro runtime projection | CompileTypeInfo | VERIFY | VERIFY | supported normalized profiles | official/compiler-sfc differential | typed degradation/unresolved input | unrelated object members not traversed | VERIFY | VERIFY |
    18	| Svelte | native runtime compile | direct Rust | Experimental (verify current pin) | VERIFY | syntax/toolchain profile | pinned Svelte compiler corpus | typed unsupported/experimental behavior | zero Vue/native compile projection | experimental | VERIFY |
    19	| TypeInfo | `TypeAtPosition` | native | VERIFY | VERIFY | normalized TS profiles | selected TS oracle | typed partial/gap/no-value | no-flow allocates no graph/plan | VERIFY | VERIFY |
    20	| TypeInfo | graph export | public/wire | advanced explicit | off unless requested | profile stamped | protocol/round-trip corpus | size/depth/unsupported failure | simple DTO operations serialize no graph | named compatibility domain | VERIFY |
    21	| LSP | external TypeScript provider | project binding | VERIFY | `auto`/explicit per product | provider profile | capability matrix | actionable incompatible route; no race/fallback | disabled native enrichment is zero-work | provider epoch/profile stamped | VERIFY |
    22	| CSS | parse/format/index/transform | native/external by dialect | VERIFY | VERIFY | dialect profile | dialect/framework corpus | typed unsupported/recovery-incomplete | identical bytes parsed once per residence | VERIFY | VERIFY |
    23	
    24	## 2.1 A1 execution evidence (references only)
    25	
    26	A1 proves ONLY that each row's canonical selectors execute their intended targets
    27	with non-zero work. It ratifies nothing in the table above. Every reference below
    28	names an entry in the external A1 evidence bundle; the bundle — not this file —
    29	carries the counts, exit codes, receipts, digests, and recorded pre-existing
    30	failures. Evidence rows are `A1/command-proofs/index.md` row numbers; sentinels
    31	are `A1/sentinel-verification.md` entries.
    32	
    33	- **Vue / runtime compile:** the canonical Rust gate (row 01, including the
    34	  in-gate Verter corpus suites and the gate-internal Vue macro-oracle checks);
    35	  the committed official-Vue golden corpus check (row 12) — the golden corpus
    36	  spans three backend trees (`vdom`, `vapor`, `vdom-inline`); the
    37	  official-compiler macro-runtime oracle (row 15). Sentinels A (gate) and C
    38	  (goldens) discriminate these selectors.
    39	- **Vue / IDE companion:** the editor-neutral provider-matrix lane over real
    40	  tsserver, managed tsgo, and relay shared-tsgo routes (row 18, machine receipt
    41	  row 18r); the external-corpus gate lane (row 16, receipt row 16r) executed
    42	  against a classified corpus identified in the bundle only by an anonymous
    43	  label and a content fingerprint. Sentinel D discriminates the corpus-gate
    44	  selector.
    45	- **Vue / imported macro runtime projection:** the official/compiler-sfc
    46	  macro-runtime differential oracle (row 15), re-executed inside the canonical
    47	  gate (row 01).
    48	- **Svelte / native runtime compile:** the pinned-compiler golden checks
    49	  (rows 13, 14), the name-parity corpus check (row 17), the conformance-corpus
    50	  reconciliation (row 19 — the reconciliation binary emits a verdict, not a
    51	  count; the bundle records the independently counted fixture inventory), and
    52	  the live feature-gated oracle harness (row 20). The pin itself is a tree fact
    53	  recorded in `A1/environment.md`.
    54	- **TypeInfo / `TypeAtPosition`:** native suites inside the canonical gate
    55	  (row 01); the JS `@verter/typeinfo` package suite inside the workspace JS run
    56	  (rows 08, 08c).
    57	- **TypeInfo / graph export:** the wire/taxonomy guard suites inside the
    58	  canonical gate (row 01) and the targeted `typeinfo_proto_ts_freshness`
    59	  byte-pin receipt (row 01b — a direct targeted execution, not an inference
    60	  from unfiltered gate coverage).
    61	- **LSP / external TypeScript provider:** the provider-matrix lane (row 18);
    62	  its machine receipt (row 18r) records a `sourceSha` field captured from the
    63	  checkout at run time, and A1's evidence binding requires that field to equal
    64	  the A1 candidate SHA recorded in the external program ledger; the
    65	  external-corpus gate lane (row 16). Sentinel D discriminates the corpus-gate
    66	  selector.
    67	- **CSS / parse/format/index/transform:** the targeted CSS-syntax package
    68	  receipt (row 01c — a direct targeted execution, not an inference from
    69	  unfiltered gate coverage) plus the Svelte conformance golden corpus whose
    70	  committed payloads pin CSS output bytes (row 14). No dedicated standalone
    71	  CSS selector exists in the repo beyond these.
    72	
    73	# 3. Rules
    74	
    75	- A missing/`VERIFY` row means the capability is not approved for architecture claims or default changes.
    76	- Maturity is operation-specific; framework citizenship does not imply equal maturity.
    77	- Changing a default or compatibility promise requires product/conformance review.
    78	- Experimental behavior cannot be silently used as a stable oracle for another surface.
    79	- Every enabled row links exact tests and benchmark cells.
    80	- Unsupported and partial behavior is part of the public contract, not an implementation accident.
    81	
    82	# 4. Proposed AMD-005 framework detail
    83	
    84	AMD-005 is not ratified. Its proposed exact framework/profile/route expansion is
    85	machine-readable at
    86	[`../evidence/framework-conformance/capability-matrix.tsv`](../evidence/framework-conformance/capability-matrix.tsv).
    87	Until ratification and BF1 acceptance, those rows do not replace the `VERIFY` seed
    88	truth above. On acceptance they govern Vue RC.3 and Svelte 5.56.8 compiler products;
    89	the seed rows remain historical lineage.
   280	  failing. Same empty-diff argument: the candidate changes zero bytes of either input those tests
   281	  read.
   282	
   283	**The canonical gate does not return PASS on this tree, and did not return PASS on the baseline
   284	either.** This record does not claim a green gate. Confirming rows 03 and 14 against a baseline
   285	checkout is an orchestrator action at landing.
   286	
   287	**Locked per-block commands (decision A5-G1, ratified).** The instrumentation feature arms are
   288	compiled and run by no automated gate: the disabled arm does not type-check the enabled arm's amount
   289	expressions, and the trybuild seal that proves the counter-reader path is absent — the negative
   290	control for the no-semantic-authority claim — is never executed by the canonical gate. The obvious
   291	remedy is structurally unavailable: a feature arm cannot ride the existing archive variants without
   292	changing feature unification for all three surfaces, a third variant is a third whole-workspace
   293	compile, and a correct addition needs a matching arm in the 7,170-line gate self-test. So these three
   294	become **required per-block commands**, whose captured output is preserved as command proofs:
   295	
   296	```sh
   297	cargo check --workspace --all-targets --features verter_audit/attribution
   298	cargo test -p verter_audit --features attribution
   299	cargo test -p verter_audit --features compile-fail
   300	```
   301	
   302	Stated honestly, and locked here so it does not silently lapse: **this is weaker than a gate.** It
   303	depends on the orchestrator running the set and the reviewer checking the proof. That weakness is
   304	inherent to a program in which CI cannot run at all. The CI job is proposed for **after** the
   305	program and requires a ruling extending R-7; no `.github/` change is made now.
   306	
   307	# 3. Capability and maturity matrix
   308	
   309	`contracts/capability-matrix.md` carries eight seed rows, and **every `Status` cell is `VERIFY`**.
   310	This lock **ratifies no maturity, default, or compatibility promise**, and says so rather than
   311	manufacturing one: ratifying a Supported/Stable cell requires product/conformance review and
   312	oracle evidence that no block through this one produced.
   313	
   314	| Framework/surface | Operation/product | Complete/Partial/Unsupported | Profile | Oracle/evidence | Public contract |
   315	|---|---|---|---|---|---|
   316	| Vue | runtime compile | `VERIFY` | `VERIFY` | official Vue fixtures + Verter corpus; execution evidence via the canonical gate and the committed golden corpus | `VERIFY` |
   317	| Vue | IDE companion | `VERIFY` | provider-specific | provider + mapping corpus; the editor-neutral provider-matrix lane | `VERIFY` |
   318	| Vue | imported macro runtime projection | `VERIFY` | supported normalized profiles | official/compiler-sfc differential oracle | `VERIFY` |
   319	| Svelte | native runtime compile | Experimental (pin verified as a tree fact) | syntax/toolchain profile | pinned Svelte compiler corpus | experimental |
   320	| TypeInfo | `TypeAtPosition` | `VERIFY` | normalized TS profiles | selected TS oracle; native suites in the canonical gate | `VERIFY` |
   321	| TypeInfo | graph export | advanced explicit; off unless requested | profile stamped | protocol/round-trip corpus; the byte-pin freshness receipt | named compatibility domain |
   322	| LSP | external TypeScript provider | `VERIFY` | provider profile | capability matrix; the provider-matrix lane | provider epoch/profile stamped |
   323	| CSS | parse/format/index/transform | `VERIFY` | dialect profile | dialect/framework corpus; the CSS-syntax package receipt | `VERIFY` |
   324	
   325	The non-`VERIFY` cells above are the plan's own seed values, carried unaltered from the accepted
   326	base; they are not ratifications made here.
   327	
   328	**Consequence, and it is deliberately fail-closed.** `contracts/capability-matrix.md` §3: "A
   329	missing/`VERIFY` row means the capability is not approved for architecture claims or default
   330	changes." `program.md` requires the atomic flow-cutover block to satisfy every effective-flow
   331	capability row declared Supported/Stable **in the A6 matrix**; with no row so declared, that
   332	obligation is currently satisfied vacuously. That is a real gap, not a pass: **the matrix must be
   333	ratified before that block begins.** Recorded as §11 row U-1 with its owner and gate. Nothing before
   334	the first unlocked block depends on it, which is why it is deferred rather than guessed.
   335	
   336	# 4. Identity and profile lock
   337	
   338	**Canonical digest schema / domain epoch.** The tree carries no single canonical digest schema; it
   339	carries seven real compatibility domains and a separate family of disposable-cache invalidation
   340	namespaces, enumerated in §5. The Revision 11 canonical-digest schema is created by the first
   341	unlocked block, not lifted from an existing owner.
   342	
   343	**Source/unit/syntax/parse/placement identities.** Current owners are enumerated per row in
   344	[`../A5/owner-rows.md`](../A5/owner-rows.md) — all sixteen seed `VERIFY` rows source-verified, plus
   345	two the seed table omitted. Two corrections in that inventory bind design and are restated because
   346	missing either changes the work rather than the schedule:
   347	
   348	- **`ProviderHub` does not exist.** It is a Revision 11 *target* name that the seed reconciliation
   349	  table listed under *current* authorities; `grep -rn "ProviderHub" crates/*/src` returns nothing.
   350	  The real current owners are `SyncCoordinatorHandle` (`verter_lsp`), the `TypeProvider` trait
   840	
   841	No later block is unlocked merely by being listed here. Contingent stacked draft or review work is
   842	legal only under the validated stack contract; no successor may become acceptance-recommended until
   843	every predecessor is formally satisfied and the candidate is restacked and revalidated.
   844	
   845	# 11. Unresolved items
   846	
   847	The template requires every item here to be a private implementation choice that cannot change
   848	semantics, identity, lifetime, cache validity, mapping interpretation, compatibility, dependency
   849	direction, or pass/fail gates. **Two of the rows below do not meet that bar, and are recorded as
   850	exceeding it rather than quietly filed under it** (U-1, U-4). Each is a deferral of *program
   851	scope*, not a private choice, and each names the gate that must resolve it. They are stated here
   852	because the alternative — inventing the content — would freeze a guess as authority, and because the
   853	gate for each falls after the work this lock unlocks.
   854	
   855	U-9 was a third such row while it stood as a recorded deviation against an amendment that named this
   856	block as deliverer. Ruling R-12 rescoped that duty off this block before acceptance, so U-9 is now
   857	**informational**: it records that the rescope happened and where the obligation went, and it is no
   858	longer a deviation this lock carries. U-12 is likewise informational — it discloses a staleness in
   859	an authority file that this record deliberately does not edit.
   860	
   861	| Item | Why non-blocking | Owner | Resolution point |
   862	|---|---|---|---|
   863	| **U-1** Capability matrix is entirely `VERIFY`; no maturity, default or compatibility promise is ratified | Fail-closed: an unratified row is not approved for architecture claims or default changes, so nothing can rely on it by accident. **Exceeds the §11 bar** — it is a compatibility decision, deferred, not a private choice | product/conformance review with the maintainer | **before the atomic flow-cutover block begins.** That block's charter requires it to satisfy every Supported/Stable row in this matrix; with none declared, the obligation is vacuous until the matrix is ratified |
   864	| **U-2** Stable-ID collision/equality policy and the output/presentation/serialization/execution profile schemas are stated as constraints, not as schemas | The types they govern do not exist yet; the constraints they must satisfy are locked in §4 and in the bound charter | `B1` | `B1`'s accepted candidate |
   865	| **U-3** `type_env_hash` and `lib_env_hash` have no production input | Nothing varies, so nothing collides; this is a missing ingress, not a live cache defect | `B1` owns modelling the dimension honestly; the block that threads real values owns the blast radius | when real configuration values are first threaded through — that change alters cache identity for every existing project at that moment and must be landed as such |
   866	| **U-4** `provider_protocol_version = 12` may duplicate a compatibility domain owned elsewhere | Recorded **NOT PROVEN**, not assumed either way; no block through this one reads or writes it. **Exceeds the §11 bar** — it is a compatibility question | the protocol/provider convergence block | that block's accepted candidate |
   867	| **U-5** Current IDE/build parser front-ends and direct/managed/FFI routes are recorded thin | The enumerations exist in the owner-rows inventory; only the blocks that consume them as a closure need them assembled, and none is unlocked | the shared-front-end, direct-compiler and managed-runtime blocks | each block's own scoping, from the existing inventory |
   868	| **U-6** No owner memory budget, allocator slack, quiescence protocol or soak cell is locked | Whole-process memory is gated by the locked cell's peak-RSS metrics; the owner memory model does not exist yet, and a ~70 ms batch cell cannot carry a soak gate | the managed-runtime and lifecycle blocks | extending this lock with those cells, before the first block whose acceptance depends on them |
   869	| **U-7** Exact structural completion and its discrimination remain open debt | The safety retraction already refuses the wrong-complete results; the remaining gap is unsupported behavior, not incorrect published behavior | the completion-graph block | that block's accepted candidate; no second classifier may be created to work around it |
   870	| **U-8** `J1` not unlocked; no CSS benchmark cell locked | CSS work is not selected and nothing unlocked depends on the CSS inventory | maintainer | maintainer selects CSS work → `J1`'s charter is bound and a CSS cell added by a new lock digest |
   871	| **U-9** *(informational — no longer a deviation against this lock)* All four of **`AMD-001`** §1's deliverables — the Node stack-window validator, composite program-state cross-validation, that validator's CI wiring, and the discriminating checkpoint/acceptance transition test — are **not delivered by this lock** (§9 enumerates them and binds the amendment's post-rescope digest) | The maintainer ruled **AMEND-AMD-001-TIMING** ([`maintainer-rulings.md` R-12](../maintainer-rulings.md)) **before this candidate's acceptance**: §1 is amended in place so the four artifacts bind to whichever accepted candidate immediately precedes the first opened stack window, and unconditionally to the one before `D1` enters `PRIVATE_CHECKPOINT` — **not to this block by name**. So this row records no open deviation and no unratified choice; the amendment text and the delivery reality now agree. The [`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md) is retained as the historical record that a `DEFER` recommendation was made and was superseded by a *different* ruling. Substantively unchanged: no window is open, the unlocked block is sequential and single-layer, and the program-state validator's fail-closed refusal is untouched here, so the unmodelled path stays closed | a later block, under the amended §1 — **not this one**; the orchestrator carries the duty forward to the candidate the amended timing names | **before the first snapshot with more than one open layer is minted, and unconditionally before the private-checkpoint block begins.** The amendment's refusal is superseded by delivering the validator, never by deleting it |
   872	| **U-10** CI wiring for the instrumentation feature arms | No GitHub Actions job runs for any block of this program, so a CI job added now would not execute; the arms are locked as required per-block commands instead | post-program | after the program lands on `main`; requires a ruling extending the one narrow CI-wiring authorization |
   873	| **U-11** The new gate validator is not added to the CI change-detection path filter | Same reason as U-10, plus: the existing authorization for a `.github/` edit was granted for one named purpose only, and extending it needs its own ruling | post-program | with U-10 |
   874	| **U-12** `PROVENANCE.md`'s published aggregate digest is stale with respect to the tree it describes | The rescope commit `fb863297a…` edited `amendments/AMD-001-…md`, which is inside the aggregate's input set, without republishing the aggregate; the published value `ff49cdd…` now reproduces only at the pre-rescope tree. Non-blocking here because this record does not consume the published value as authority — §1 recomputes both aggregates from the git object store and records the recomputed pair, and it reproduces `ff49cdd…` at the pre-rescope tree as the method control. **Not corrected by this candidate:** `PROVENANCE.md` is an authority file of the integration lineage, not a block artifact, and a lock candidate silently rewriting an authority digest is exactly the move this program's evidence discipline forbids | orchestrator, on the integration lineage | at, or before, the transition that accepts this record — republish the aggregate as `a061d97534f2b96f96a92eae24569f8500c696a4b82d827efbd1a52deb78a173` (74 files) at the accepted tip, or record why the published value is pinned to a superseded tree |
   875	
   876	# 12. Acceptance checklist
   877	
   878	- [x] exact entry checkout, exact implementation baseline, tree OID, record digest rule, and
   879	      authority digest recorded — §1
   880	- [~] all canonical commands non-vacuous — §2: every command executed non-zero intended work, and
   881	      the selectors are proven non-vacuous. But **the canonical gate returns FAIL**, on a defect that
   882	      pre-exists this baseline plus two load-sensitive tests that pass in isolation. Recorded as a
   883	      classified failure, not as a pass
   884	- [~] capability/protocol/consumer inventory complete — protocol and consumer inventories are
   885	      complete (§5); the **capability matrix is unratified** and recorded as U-1, fail-closed
   886	- [~] identity/profile/compatibility decisions accepted — compatibility decisions accepted (§5);
   887	      identity/profile **schemas** are the unlocked block's deliverable, with their constraints
   888	      locked here (§4, U-2)
   889	- [x] performance gate file contains no placeholders/zero-required fields — machine-checked by
   890	      `scripts/validate-performance-gates.mjs`, with a twenty-control discriminating suite
   891	- [x] raw baseline and noise measurements retained — §7
   892	- [x] semantic safety retraction complete for its declared scope — §8, with the remaining structural
   893	      gap recorded as U-7 rather than claimed closed
   894	- [x] maintainer/orchestrator identities and program-state/evidence custody accepted — §1, §9
   895	- [x] worktree/branch/CI/merge/stack/restack policy accepted — §9
   896	- [x] first foundational charter, context packet and stack placement accepted — §9, §10
   897	- [~] no unresolved public/semantic/identity/lifetime/cache/compatibility/gate issue — **not clean.**
   898	      U-1 and U-4 exceed the §11 bar and are named as such. U-9 no longer does — ruling R-12
   899	      rescoped the amendment obligation off this block before acceptance, leaving that row
   900	      informational. No *gate* issue is open: the gate file is complete and locked
   901	- [ ] exact SHA/tree architecture and adversarial evidence accepted — pending the three review
   902	      mandates against one unchanged candidate SHA and tree
   903	- [x] no agent may self-accept, weaken gates, or merge private atomic layers independently — §9
   904	
   905	The three `[~]` rows and the two `[ ]` rows are the honest state of this record at draft. A checklist
   906	ticked complete while U-1 sits unratified would be exactly the failure mode this program's
   907	verification rule exists to prevent.
   130	- **The registered amendment's four artifacts are not delivered — and, after a maintainer ruling,
   131	  that is now correct rather than a gap.** `AMD-001` originally named this block as their deliverer:
   132	  a Node stack-window validator, composite program-state cross-validation, that validator's CI
   133	  wiring, and a discriminating checkpoint/acceptance transition test. None is delivered. The deferral
   134	  was **not this block's to grant**, so it was written up as a `governance.md` §10 deviation memo
   135	  recommending `DEFER`. The maintainer ruled **AMEND-AMD-001-TIMING** instead: §1 is amended in place
   136	  so the four artifacts bind to whichever accepted candidate immediately precedes the first opened
   137	  stack window, and unconditionally to the one before the private-checkpoint block begins — not to
   138	  this block by name. So the amendment text and the delivery reality now agree; the memo is retained
   139	  as the historical record of a recommendation the ruling superseded. §§2-4 stand unchanged: the
   140	  amendment is named by identifier and bound by its **post-amendment** digest in the lock record and
   141	  in the context packet's second addendum — the half of it a lock block can actually discharge — and
   142	  its rule that the program-state validator's fail-closed refusal may be superseded but never deleted
   143	  is honoured: that refusal is untouched here, which is what keeps the unmodelled path closed rather
   144	  than open.
   145	
   146	The acceptance checklist in the lock record is ticked accordingly — three rows partial, two pending.
   147	A checklist ticked complete while those sit open would be exactly the failure the program's
   148	verification rule exists to prevent.
   149	
   150	### The ledger gains an integration-lineage field
   151	
   152	The ledger's `[repository]` table records the *entry checkout*, while accepted blocks land on a
   153	separate integration branch, and no field distinguishes the two. A resuming agent reading that table
   154	alone would land onto the default branch and silently drop every accepted block. The lock records the
   155	lineage explicitly and adds it to the ledger schema.
   156	
   157	One consequence is recorded before the first landing rather than discovered at it: the lineage must
   158	not be fast-forwarded into the default branch while the ledger-import commit is in its history,
   159	because the transport copy's removal obligation includes git history.
   160	
   161	## Ratified decisions carried into the lock
   162	
   163	Five decisions the previous block raised for ratification are incorporated as accepted, each into the
   164	section that owns it, and each is locatable rather than merely claimed:
   165	
   166	| decision | where it lands |
   167	|---|---|
   168	| the instrumentation converge-then-delete disposition, with its counter owner, its watchdog owner and its hard backstop named | lock record §7, as a debt row whose ruling reference is ratified with the lock; the record also states that this block performs none of that migration |
   169	| the two feature arms as locked per-block commands, CI job deferred post-program | lock record §2, with the deferral as U-10 |
   170	| the semantic-kernel upward edge as an equality-pinned exception, with its removal gate and its target condition | lock record §6 |
   171	| the unlanded local-branch population abandoned as a class — no branch deleted, no GitHub action | lock record §1; the ruling itself is registered as R-12 in [`maintainer-rulings.md`](maintainer-rulings.md) |
   172	| the bounded-stack policy above | lock record §9 and [`A6/stack-window-policy.toml`](A6/stack-window-policy.toml) |
   173	
   174	## Verification, including what failed
   175	
   176	The canonical gate and the end-of-change checks were executed at this candidate; exit codes, executed
   177	counts and raw-output digests are in [`A6/command-proofs.md`](A6/command-proofs.md) and
   178	[`A6/command-proofs-native.md`](A6/command-proofs-native.md). The three instrumentation-arm commands
   179	this lock makes mandatory for every later block were run here first, so the requirement ships with a
   180	proof that it can be met.
   181	
   182	**The canonical gate returns FAIL, and running it is how that was discovered.** Five reported
   183	failures, three distinct tests:
   184	
   185	- **`tracked_paths_no_machine_roots` — a genuine tracked-tree defect that pre-exists this baseline.**
   186	  Two already-accepted blocks' context packets embed an absolute machine path. Both blocks skipped
   187	  the canonical gate on the reasoning that they changed no production source; the guard scans tracked
   188	  *bytes*, not production source, so that reasoning had a hole in it. Proven against the baseline
   189	  commit with `git grep`, not inferred. This block's own packet was a third instance and is fixed —
   190	  verified discriminating, since re-running the guard alone now reports two violations rather than

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"SemanticQueryKey::FlowReturn|FlowReturnKey|FlowReturnResult|FlowReturnStep|flow_return\\(\" crates packages | sed -n '1,320p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 37740ms:[0m
crates/verter_session/src/loop5_instrumentation.rs:388:        SemanticQueryKey::FlowReturn(_) => 25,
crates/verter_session/src/host_resolve_type_audit.rs:506:        | SemanticQueryKey::FlowReturn(_)
crates/verter_session/src/cache_runtime/flow_slice_node_tests.rs:915:    let key = crate::semantic_query::FlowReturnKey {
crates/verter_session/src/cache_runtime/flow_slice_node_tests.rs:928:    match dispatch.execute(crate::semantic_query::SemanticQueryKey::FlowReturn(
crates/verter_session/src/host_flow_return_audit.rs:8://! `SemanticQueryKey::FlowReturn` demand routed through the shared
crates/verter_session/src/host_flow_return_audit.rs:14://! `FlowReturnResult::degradation` — rides the [`AuditedResult`] `Ok`
crates/verter_session/src/host_flow_return_audit.rs:38:use crate::semantic_query::{FlowReturnFailure, FlowReturnResult, ReturnProjectionDemand};
crates/verter_session/src/host_flow_return_audit.rs:43:/// an error: it rides the `Ok` arm as a [`FlowReturnResult`] with
crates/verter_session/src/host_flow_return_audit.rs:86:    ) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
crates/verter_session/src/host_flow_return_audit.rs:128:        let outcome: Result<Arc<FlowReturnResult>, FlowReturnError> =
crates/verter_session/src/host_flow_return_audit.rs:141:                        match dispatch.execute_flow_return(key) {
crates/verter_session/src/host_flow_return_audit.rs:142:                            crate::semantic_query::FlowReturnStep::Complete(result) => {
crates/verter_session/src/host_flow_return_audit.rs:145:                            crate::semantic_query::FlowReturnStep::NoValue(failure) => {
crates/verter_session/src/host_flow_return_audit.rs:152:                            crate::semantic_query::FlowReturnStep::Hold(_) => {
crates/verter_session/src/host_flow_return_audit.rs:262:    outcome: Result<Arc<FlowReturnResult>, FlowReturnError>,
crates/verter_session/src/host_flow_return_audit.rs:264:) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
crates/verter_session/src/u6_flow_shape_corpus_rows_tests.rs:308:    Row { id: "CC06_contextual_arrow_param", script: "function apply<T>(cb: (v: \"a\" | \"b\") => T) { return cb(\"a\") }\nfunction makeProps() { return { mode: apply((v) => v) } }", probe: "ReturnType<typeof makeProps>", checker: "{ mode: \"a\" | \"b\"; }", owner: Owner::U6CallResolve, flow: Flow::Result { function: "makeProps", node: NodeShape::Object, members: &[("mode", NodeShape::Primitive)], degradation: Degr::None, candidates: 1 }, verdict: Verdict::KnownOwed { owed_absent: &[], note: "Context-sensitive callback arguments are never contextually typed: the arrow's un-annotated parameter lowers to `any`, the argument is withheld from the first inference pass, and no second pass re-types it under the fixed substitution — so `T` infers no candidate from the callback's return and defaults to `unknown` where the checker computes `\"a\" | \"b\"`. The repair is the inference pass's contextual half: mint the callback's contextual input at the call seam (the FlowReturnKey input axis — every production construction is the canonical empty point today), let the evaluator bind un-annotated parameters from it (non-empty inputs fail closed as UnmodeledDemandPoint today), and deposit the re-typed return as the covariant candidate." }, ..Row::BLANK },
crates/verter_session/src/flow_gap_retraction_tests.rs:11:    FlowGap, FlowInputContext, FlowReturnDegradation, FlowReturnFailure, FlowReturnKey,
crates/verter_session/src/flow_gap_retraction_tests.rs:53:    let key = FlowReturnKey {
crates/verter_session/src/flow_gap_retraction_tests.rs:68:        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:30:    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:187:fn key_for(dispatch: &ProjectSemanticDispatch<'_>, canonical: &str, name: &str) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:188:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:215:        let result = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:224:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:3://! One `SemanticQueryKey::FlowReturn` producer through
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:34:    FlowReturnDegradation, FlowReturnFailure, FlowReturnKey, FlowReturnResult, FlowReturnStep,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:47:    Flow(FlowReturnResult),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:238:        FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:279:    key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:292:    Provisional(FlowReturnStep),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:301:    /// `FlowReturnKey` construction routes through here.
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:348:    /// The ONE `FlowReturnKey` construction: every body-derived
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:359:    ) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:377:    ) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:378:        FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:403:    ) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:655:    /// constructs and executes the [`FlowReturnKey`] through
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:674:                let node = match self.execute_flow_return(self.flow_return_key_for(identity)) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:678:                    FlowReturnStep::Complete(result) => FunctionReturnNode::Flow(result),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:679:                    FlowReturnStep::NoValue(failure) => FunctionReturnNode::NoValue(failure),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:682:                    FlowReturnStep::Hold(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:721:    /// `SemanticQueryKey::FlowReturn` with the single-member
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:829:        match self.execute_flow_return(key) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:830:            FlowReturnStep::Complete(result) if result.degradation().is_none() => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:934:    pub(crate) fn execute_flow_return(&self, key: FlowReturnKey) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:957:                return FlowReturnStep::Hold(Box::new(key));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:962:            return FlowReturnStep::Complete(result);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:977:    fn execute_flow_return_root(&self, key: FlowReturnKey) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:978:        let read = self.execute_flow_return_cold_build(SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:981:                FlowReturnStep::Complete((*result).clone())
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:987:            _ => FlowReturnStep::NoValue(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:999:    /// [`SemanticQueryKey::FlowReturn`] reaches the family cold build.
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1027:            matches!(key, SemanticQueryKey::FlowReturn(_)),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1030:        let SemanticQueryKey::FlowReturn(root_key) = key.clone() else {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1046:        root_key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1058:            crate::semantic_query_memo::SccRootWitness::flow_return(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1076:    fn execute_flow_return_inline(&self, key: FlowReturnKey) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1078:            return FlowReturnStep::NoValue(FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1083:        let evaluated = self.evaluate_flow_return(&key);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1095:    pub(super) fn build_flow_return(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1097:        key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1102:        let evaluated = self.evaluate_flow_return(key);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1145:    fn flow_frame_open(&self, key: &FlowReturnKey) -> usize {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1152:        let idx = txn.reentry_mut().push_flow_return(key.clone(), watermark);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1164:    fn flow_frame_close(&self, idx: usize, evaluated: FlowEvaluationOutcome) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1168:                FlowRootClose::Complete(result, _, _) => FlowReturnStep::Complete(result),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1169:                FlowRootClose::NoValue(failure) => FlowReturnStep::NoValue(failure),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1180:        key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1181:        result: FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1182:    ) -> FlowReturnResult {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1207:    ) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1264:            .as_flow_return()
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1308:                FlowReturnPendingOutcome::Complete(result) => FlowReturnStep::Complete(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1312:                    FlowReturnStep::NoValue(*failure)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1369:                        .as_flow_return()
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1585:                    FlowFramePop::Provisional(FlowReturnStep::Complete(result))
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1609:        let index: rustc_hash::FxHashMap<&FlowReturnKey, usize> = entries
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1614:        let mut current: Vec<Option<FlowReturnResult>> = entries
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1705:                let next = FlowReturnResult::new(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1921:    fn evaluate_flow_return(&self, key: &FlowReturnKey) -> FlowEvaluationOutcome {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2425:    ) -> Result<(FlowReturnResult, bool), FlowReturnFailure> {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2491:                    FlowReturnResult::new(graph, return_type, can_fall_through, degradation),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2548:            FlowReturnResult::new(graph, return_type, can_fall_through, degradation),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2935:    self_slot: Option<&'b FlowReturnKey>,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8132:                        match self.dispatch.execute_flow_return(key.clone()) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8133:                            FlowReturnStep::Complete(result) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8169:                            FlowReturnStep::Hold(key) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8174:                            FlowReturnStep::NoValue(FlowReturnFailure::EmptyCycle) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8187:                            FlowReturnStep::NoValue(_) => Positional::Unmodeled,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8327:                match self.dispatch.execute_flow_return(self_slot.clone()) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8328:                    FlowReturnStep::Hold(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8335:                    FlowReturnStep::Complete(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8338:                    FlowReturnStep::NoValue(_) => Positional::Unmodeled,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:11:    FlowReturnKey, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:315:    let key = FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:336:) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:337:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:354:    key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:356:    dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:362:    key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:417:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:425:fn flow_is_miss(dispatch: &ProjectSemanticDispatch<'_>, key: FlowReturnKey) -> bool {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:508:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:513:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:909:/// The sealed helper's Flow arm constructs the IDENTICAL `FlowReturnKey` the
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:927:            "the sealed helper constructs the identical FlowReturnKey"
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1196:        let key = FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1213:            .entry_self_root_canonicals_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1965:    key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1966:) -> crate::semantic_query::FlowReturnResult {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1977:fn key_hash(key: &FlowReturnKey) -> u64 {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1988:/// distinct identities (the family key embeds the full `FlowReturnKey`,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2094:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2135:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2147:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2167:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2186:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(clean_key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2227:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2264:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2311:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2355:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2398:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2434:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2478:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2518:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2545:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2623:            .entry_satisfied_projection_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2651:        let step = dispatch.execute_flow_return(root);
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2654:            crate::semantic_query::FlowReturnStep::Complete(_)
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2664:            .entry_satisfied_projection_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2688:) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2932:        let mut key = FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3019:/// — the `SemanticQueryKey::FlowReturn` arm dispatches through
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3022:/// store serves the identical published `FlowReturnResult`, and the
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3027:/// construction confinement on `FlowReturnResult` (its production
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3089:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3109:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3342:fn scc_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3343:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3370:        let result = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3373:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3384:    crate::semantic_query::FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3584:            let key = SemanticQueryKey::FlowReturn(Box::new(scc_key(dispatch, second)));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3633:                    &SemanticQueryKey::FlowReturn(Box::new(key.clone())),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3690:            let root_query = SemanticQueryKey::FlowReturn(Box::new(root_key.clone()));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3700:                let query = SemanticQueryKey::FlowReturn(Box::new(key.clone()));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3740:                &crate::semantic_query_memo::SccRootWitness::flow_return(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3811:        let key = FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:4165:            SemanticQueryKey::FlowReturn(Box::new(dispatch.flow_return_key_for(&identity)));
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:66:    FlowReturnKey, FlowReturnResult, IndexSignature, InferBinding, InferableParamSetId,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:224:    /// identity IS the `FlowReturnKey` exactly.
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:225:    FlowReturn(FlowReturnKey),
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:242:    pub(crate) fn as_flow_return(&self) -> Option<&FlowReturnKey> {
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:441:    pub(crate) fn push_flow_return(
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:443:        key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:596:    Complete(FlowReturnResult),
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1837:    pub(crate) key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1838:    pub(crate) result: FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1913:    pub(super) key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1929:    FlowReturn(FlowReturnKey),
crates/verter_session/src/project_semantic_dispatch/dispatch_txn_tests.rs:196:fn flow_return_key() -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/dispatch_txn_tests.rs:197:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/dispatch_txn_tests.rs:717:    let flow_key = |name: &str| FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/dispatch_txn_tests.rs:744:    stack.push_flow_return(flow_key("nested"), 0);
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:384:                        .as_flow_return()
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:534:                crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:1993:                    match self.execute_flow_return(flow_key.clone()) {
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2001:                        crate::semantic_query::FlowReturnStep::Complete(result)
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2027:                        crate::semantic_query::FlowReturnStep::Complete(_)
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2028:                        | crate::semantic_query::FlowReturnStep::Hold(_) => {
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2031:                        crate::semantic_query::FlowReturnStep::NoValue(
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2039:                        crate::semantic_query::FlowReturnStep::NoValue(
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2045:                        crate::semantic_query::FlowReturnStep::NoValue(_) => {
crates/verter_session/src/project_semantic_dispatch/mod.rs:2192:        if matches!(key, SemanticQueryKey::FlowReturn(_)) {
crates/verter_session/src/project_semantic_dispatch/mod.rs:2475:            if let SemanticQueryKey::FlowReturn(key) = &key_for_build {
crates/verter_session/src/project_semantic_dispatch/mod.rs:2476:                return self.build_flow_return(key);
crates/verter_session/src/project_semantic_dispatch/mod.rs:2676:                SemanticQueryKey::FlowReturn(_) => unreachable!(
crates/verter_session/src/project_semantic_dispatch/relation.rs:314:    pub(super) key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/relation.rs:1186:                        .as_flow_return()
crates/verter_session/src/project_semantic_dispatch/relation.rs:2019:                crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/relation.rs:2074:            crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:15:    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:108:fn key_for(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:109:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:133:        let result = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:142:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:13:use crate::semantic_query::{FlowReturnKey, SemanticNodeData, SemanticNodeId};
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:25:        flow_overrides: &FxHashMap<FlowReturnKey, SemanticNodeId>,
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:159:        flow_overrides: &FxHashMap<FlowReturnKey, SemanticNodeId>,
crates/verter_session/src/project_semantic_dispatch/raise.rs:296:        SemanticQueryKey::FlowReturn(_) => "FlowReturn",
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:15:    FlowNarrowingKey, FlowReturnFailure, FlowReturnResult, FlowReturnStep,
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:146:        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:165:            FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:173:        FlowReturnStep::Complete(ref result) if result.return_type() == number
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:199:        .push_flow_return(flow_key, 0);
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:281:        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:306:        FlowReturnStep::NoValue(FlowReturnFailure::EmptyCycle)
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:401:        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:426:        FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:436:        FlowReturnStep::NoValue(FlowReturnFailure::Budget(_))
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:474:        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:507:            FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:515:        FlowReturnStep::NoValue(_)
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:548:        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:581:            FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
crates/verter_session/src/project_semantic_dispatch/return_equation_tests.rs:589:        FlowReturnStep::NoValue(_)
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:29:    FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:253:fn r6_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:254:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:278:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:290:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:309:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:314:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:329:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:439:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:473:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:576:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:606:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:45:    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:567:) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:568:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:585:fn key_of(dispatch: &ProjectSemanticDispatch<'_>, canonical: &str, name: &str) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:621:    key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:623:    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:633:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:731:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:736:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:815:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:823:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:1167:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:1197:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:2680:/// Two threads racing ONE `FlowReturnKey` agree on the answer, and the
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:2956:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:23:    FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1828:fn r5_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1836:) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1837:    FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1867:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1876:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1926:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1931:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3081:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3089:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3114:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3122:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3462:                dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3467:                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3582:        let _ = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3705:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(r5_key(
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6175:        let key = FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6191:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6200:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6504:        let key = FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6520:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6529:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/call_resolve_tests.rs:179:        txn.reentry_mut().push_flow_return(flow_key, 0);
crates/verter_session/src/project_semantic_dispatch/call_resolve_tests.rs:1867:    let key = crate::semantic_query::FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/call_resolve_tests.rs:1883:    }) = SemanticQueryApi::execute(&dispatch, SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:89:    ClauseSpelling, FlowReturnKey, PrimitiveKind, QueryError, SemanticNodeData, SemanticNodeId,
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:660:        key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:680:    pub(super) fn foreign(key: FlowReturnKey, clause: CalleeClause) -> Self {
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:693:    pub(super) fn own_frame(key: FlowReturnKey) -> Self {
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:708:    pub(super) fn flow_key(&self) -> Option<&FlowReturnKey> {
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry_lib.rs:119:    FlowReturnKeyCoversEnvDimensions,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry_lib.rs:120:    FlowReturnKeyCoversInputContextAndProjectionDemand,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry_lib.rs:270:    GuardId::FlowReturnKeyCoversEnvDimensions,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry_lib.rs:271:    GuardId::FlowReturnKeyCoversInputContextAndProjectionDemand,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry_lib.rs:344:    GuardId::FlowReturnKeyCoversEnvDimensions,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry_lib.rs:345:    GuardId::FlowReturnKeyCoversInputContextAndProjectionDemand,
crates/verter_session/tests/cases/manifest_data/typeinfo_parity_blocks.rs:33:    BlockContractRow { block_id: TypeInfoParityBlockId::U6FlowReturnSubstrate, owning_u_block: UBlock::U6, organ: ArchitectureOrgan::FlowCallSolver, status: BlockLandingStatus::LandingUnverified, prereqs: &[TypeInfoParityBlockId::U2QueryValueDomain, TypeInfoParityBlockId::U2RelationInfer, TypeInfoParityBlockId::U2IndexedAccess], mechanism_id: MechanismId::ReturnPathPeekerTwoFrontier, consumed_mechanisms: &[MechanismId::QueryValueDomainFoundation, MechanismId::RelateCoinductiveScc, MechanismId::IndexedAccessUnionDistribution], required_guards: &[GuardId::TypeinfoParityBlockDagIsAcyclicAndConsumedKeysAndMechanismsArePrereqs, GuardId::FunctionFlowGraphBuiltOncePerFunctionSkeleton, GuardId::FlowSliceIsGraphReachabilityNotProceduralWalk, GuardId::FlowGraphEffectEdgesStayLivePastValueWrites, GuardId::FlowGraphBuildIsShallowInternedNoLoweringLazyRegions, GuardId::FlowReturnRoutesThroughProjectSemanticDispatch, GuardId::FlowSliceLoweredBodyDoesNotComputeSliceHash, GuardId::FlowSliceKeysOnBodySensitiveHashNotParseStableHash, GuardId::FlowReturnKeyCoversEnvDimensions, GuardId::FlowReturnKeyCoversInputContextAndProjectionDemand, GuardId::FlowSolverNeverSlicesSourceText, GuardId::NoFlowSlotInPublishedTypeSurface, GuardId::FlowSliceBudgetExceededAdmitsNothing, GuardId::ProgramAnalysisFactDomainValidatesFlowSlice, GuardId::FlowSliceIrDetachesFromOxcArena, GuardId::SubstitutionEnvCanonicalHashIsOrderIndependent, GuardId::NoDepthSentinelOnFlowReturnPath], verification_labels: &["cargo test -p verter_session --test main cases::typeinfo_ignored_test_manifest", "cargo nextest run --workspace", "cargo test -p verter_session --tests", "cargo clippy --workspace -- -D warnings", "cargo fmt --all --check", "pnpm test", "pnpm install --frozen-lockfile"] },
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:119:    FlowReturnKeyCoversEnvDimensions,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:120:    FlowReturnKeyCoversInputContextAndProjectionDemand,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:270:    GuardId::FlowReturnKeyCoversEnvDimensions,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:271:    GuardId::FlowReturnKeyCoversInputContextAndProjectionDemand,
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:455:    GuardSpec { id: GuardId::FlowReturnKeyCoversEnvDimensions, owner: TypeInfoParityBlockId::U6FlowReturnSubstrate, disposition: GuardDisposition::Live { target: GuardTarget::SessionLib } },
crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:456:    GuardSpec { id: GuardId::FlowReturnKeyCoversInputContextAndProjectionDemand, owner: TypeInfoParityBlockId::U6FlowReturnSubstrate, disposition: GuardDisposition::Live { target: GuardTarget::SessionLib } },
crates/verter_session/src/semantic_query/flow_return_result.rs:1://! [`FlowReturnResult`] — the SUCCESS carrier of a `FlowReturn` query, and
crates/verter_session/src/semantic_query/flow_return_result.rs:6://! [`FlowReturnResult::new`] is the only way to set them anywhere in the
crates/verter_session/src/semantic_query/flow_return_result.rs:31://! [`FlowReturnResult::with_return_type`], re-derives.
crates/verter_session/src/semantic_query/flow_return_result.rs:50:pub struct FlowReturnResult {
crates/verter_session/src/semantic_query/flow_return_result.rs:71:    /// only way to obtain a `FlowReturnResult` is [`Self::new`], which
crates/verter_session/src/semantic_query/flow_return_result.rs:77:impl FlowReturnResult {
crates/verter_session/src/semantic_query/query_key_spec.rs:997:        // FlowReturn(key: FlowReturnKey) — the whole-function return
crates/verter_session/src/semantic_query/query_key_spec.rs:1007:        // (`FlowInputContext`) are KEY-EMBEDDED fields of `FlowReturnKey`
crates/verter_session/src/typeinfo_guard_bindings_tests.rs:51:    lib_guard!(FlowReturnKeyCoversEnvDimensions => crate::project_semantic_dispatch::flow_return_tests::flow_return_keys_do_not_warm_hit_across_env_axes),
crates/verter_session/src/typeinfo_guard_bindings_tests.rs:52:    lib_guard!(FlowReturnKeyCoversInputContextAndProjectionDemand => crate::project_semantic_dispatch::flow_return_tests::flow_return_key_covers_input_context_and_projection_demand),
crates/verter_session/src/semantic_query_memo/family.rs:521:    /// the full [`crate::semantic_query::FlowReturnKey`] — function slot
crates/verter_session/src/semantic_query_memo/family.rs:532:        key: Box<crate::semantic_query::FlowReturnKey>,
crates/verter_session/src/semantic_query_memo/family.rs:1876:        // key: the full `FlowReturnKey` IS the family identity (function
crates/verter_session/src/semantic_query_memo/family.rs:1879:        SemanticQueryKey::FlowReturn(key) => {
crates/verter_session/src/semantic_query_memo/family.rs:1916:        SemanticQueryKey::FlowReturn(key) => key.demand.point.projection.path.clone(),
crates/verter_session/src/semantic_query_memo/family.rs:1931:        SemanticQueryKey::FlowReturn(key) => Some(key.demand.point.clone()),
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:18:    FlowReturnKey, FlowReturnPolicy, FlowReturnResult, PrimitiveKind, RelateMemoKey,
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:56:fn distinct_flow_keys(count: usize) -> Vec<FlowReturnKey> {
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:58:        .map(|index| FlowReturnKey {
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:99:    keys: &[FlowReturnKey],
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:112:    keys: &[FlowReturnKey],
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:129:                result: FlowReturnResult::new(store, return_type, false, *degradation),
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:201:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:395:/// A `FlowReturnResult` carrying `degradation: Some(_)` is a DEGRADED
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:404:/// This shape is live, not hypothetical: `FlowReturnResult.degradation`
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:514:                SemanticQueryValue::FlowReturn(Arc::new(FlowReturnResult::new(
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:596:            store.slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/semantic_query_memo/tests.rs:10613:        SemanticQueryKey::FlowReturn(Box::new(crate::semantic_query::FlowReturnKey {
crates/verter_session/src/semantic_query_memo/scc_publish.rs:81:    pub(crate) fn flow_return(
crates/verter_session/src/semantic_query_memo/scc_publish.rs:82:        key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/semantic_query_memo/scc_publish.rs:116:    pub(crate) key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/semantic_query_memo/scc_publish.rs:118:    pub(crate) result: crate::semantic_query::FlowReturnResult,
crates/verter_session/src/semantic_query_memo/scc_publish.rs:281:                matches!(member.flight.prepared.key(), SemanticQueryKey::FlowReturn(k) if **k == member.key),
crates/verter_session/src/semantic_query.rs:115:pub use flow_return_result::FlowReturnResult;
crates/verter_session/src/semantic_query.rs:1435:/// Env a [`SemanticQueryKey::FlowReturn`] value depends on: the full
crates/verter_session/src/semantic_query.rs:1460:/// The demand axis of a [`FlowReturnKey`]: the flow-typed
crates/verter_session/src/semantic_query.rs:1492:/// The input axis of a [`FlowReturnKey`]: the contextual callback input
crates/verter_session/src/semantic_query.rs:1531:pub struct FlowReturnKey {
crates/verter_session/src/semantic_query.rs:1559:/// Typed degradation on a usable but incomplete [`FlowReturnResult`].
crates/verter_session/src/semantic_query.rs:1609:    /// node by [`FlowReturnResult::new`], never recorded per-arm.
crates/verter_session/src/semantic_query.rs:1707:pub(crate) enum FlowReturnStep {
crates/verter_session/src/semantic_query.rs:1710:    Complete(FlowReturnResult),
crates/verter_session/src/semantic_query.rs:1714:    Hold(Box<FlowReturnKey>),

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n --glob '*.rs' \"SemanticQueryKey::FlowReturn\" crates/verter_session crates/verter_semantic crates/verter_compiler crates/verter_lsp crates/verter_napi crates/verter_wasm crates/verter_mcp | sed -n '1,240p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 91704ms:[0m
crates/verter_session/src/loop5_instrumentation.rs:388:        SemanticQueryKey::FlowReturn(_) => 25,
crates/verter_session/src/host_resolve_type_audit.rs:506:        | SemanticQueryKey::FlowReturn(_)
crates/verter_session/src/cache_runtime/flow_slice_node_tests.rs:928:    match dispatch.execute(crate::semantic_query::SemanticQueryKey::FlowReturn(
crates/verter_session/src/host_flow_return_audit.rs:8://! `SemanticQueryKey::FlowReturn` demand routed through the shared
crates/verter_session/src/semantic_query_memo/family.rs:1879:        SemanticQueryKey::FlowReturn(key) => {
crates/verter_session/src/semantic_query_memo/family.rs:1916:        SemanticQueryKey::FlowReturn(key) => key.demand.point.projection.path.clone(),
crates/verter_session/src/semantic_query_memo/family.rs:1931:        SemanticQueryKey::FlowReturn(key) => Some(key.demand.point.clone()),
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:201:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/semantic_query_memo/scc_publish_tests.rs:596:            store.slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/flow_gap_retraction_tests.rs:68:        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/semantic_query_memo/tests.rs:10613:        SemanticQueryKey::FlowReturn(Box::new(crate::semantic_query::FlowReturnKey {
crates/verter_session/src/semantic_query_memo/scc_publish.rs:281:                matches!(member.flight.prepared.key(), SemanticQueryKey::FlowReturn(k) if **k == member.key),
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:215:        let result = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
crates/verter_session/src/project_semantic_dispatch/flow_return_positional_tests.rs:224:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/mod.rs:2192:        if matches!(key, SemanticQueryKey::FlowReturn(_)) {
crates/verter_session/src/project_semantic_dispatch/mod.rs:2475:            if let SemanticQueryKey::FlowReturn(key) = &key_for_build {
crates/verter_session/src/project_semantic_dispatch/mod.rs:2676:                SemanticQueryKey::FlowReturn(_) => unreachable!(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:3://! One `SemanticQueryKey::FlowReturn` producer through
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:721:    /// `SemanticQueryKey::FlowReturn` with the single-member
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:978:        let read = self.execute_flow_return_cold_build(SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:999:    /// [`SemanticQueryKey::FlowReturn`] reaches the family cold build.
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1027:            matches!(key, SemanticQueryKey::FlowReturn(_)),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1030:        let SemanticQueryKey::FlowReturn(root_key) = key.clone() else {
crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:54:            PreparedKeyHandle::prepare(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:356:    dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:417:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:508:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:513:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1213:            .entry_self_root_canonicals_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2094:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2135:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2147:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2167:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2186:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(clean_key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2227:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2264:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2311:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2355:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2398:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2434:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2478:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2518:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2545:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2623:            .entry_satisfied_projection_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:2664:            .entry_satisfied_projection_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3019:/// — the `SemanticQueryKey::FlowReturn` arm dispatches through
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3089:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3109:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3370:        let result = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3373:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3584:            let key = SemanticQueryKey::FlowReturn(Box::new(scc_key(dispatch, second)));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3633:                    &SemanticQueryKey::FlowReturn(Box::new(key.clone())),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3690:            let root_query = SemanticQueryKey::FlowReturn(Box::new(root_key.clone()));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:3700:                let query = SemanticQueryKey::FlowReturn(Box::new(key.clone()));
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:4165:            SemanticQueryKey::FlowReturn(Box::new(dispatch.flow_return_key_for(&identity)));
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:133:        let result = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
crates/verter_session/src/project_semantic_dispatch/flow_return_frame_seal_tests.rs:142:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/raise.rs:296:        SemanticQueryKey::FlowReturn(_) => "FlowReturn",
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:278:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:290:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:309:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:314:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:329:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:439:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:473:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:576:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_root_gate_tests.rs:606:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:831:                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:1798:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:623:    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:633:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:731:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:736:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:815:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:823:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:1167:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:1197:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:2956:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1867:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1876:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1926:            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:1931:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3081:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3089:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3114:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3122:                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3462:                dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3467:                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3582:        let _ = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:3705:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(r5_key(
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6191:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6200:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6520:        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
crates/verter_session/src/project_semantic_dispatch/flow_return_lexical_tests.rs:6529:            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/call_resolve_tests.rs:1883:    }) = SemanticQueryApi::execute(&dispatch, SemanticQueryKey::FlowReturn(Box::new(key)))
crates/verter_session/src/semantic_query.rs:1435:/// Env a [`SemanticQueryKey::FlowReturn`] value depends on: the full
crates/verter_session/src/semantic_query.rs:7628:            SemanticQueryKey::FlowReturn(_) => SemanticQueryKeyTag::FlowReturn,

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n --glob '*.rs' \"FlowReturn(Result|Failure|Key|Step)|flow_return_(for|key|memo|result)|execute_flow_return\" crates/verter_session/src crates/verter_compiler/src crates/verter_semantic/src crates/verter_lsp/src crates/verter_napi/src crates/verter_wasm/src | rg -v '(/tests?/|_tests\\.rs:)' | sed -n '1,320p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 31307ms:[0m
crates/verter_session/src/flow_return_audit.rs:11://! hit in `execute_flow_return` returns before any of them — and the
crates/verter_session/src/host_flow_return_audit.rs:14://! `FlowReturnResult::degradation` — rides the [`AuditedResult`] `Ok`
crates/verter_session/src/host_flow_return_audit.rs:16://! [`crate::semantic_query::FlowReturnFailure`] class) rides the `Err`
crates/verter_session/src/host_flow_return_audit.rs:38:use crate::semantic_query::{FlowReturnFailure, FlowReturnResult, ReturnProjectionDemand};
crates/verter_session/src/host_flow_return_audit.rs:43:/// an error: it rides the `Ok` arm as a [`FlowReturnResult`] with
crates/verter_session/src/host_flow_return_audit.rs:48:    /// [`FlowReturnFailure`] class (missing function, unsupported
crates/verter_session/src/host_flow_return_audit.rs:51:    Failure(FlowReturnFailure),
crates/verter_session/src/host_flow_return_audit.rs:86:    ) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
crates/verter_session/src/host_flow_return_audit.rs:128:        let outcome: Result<Arc<FlowReturnResult>, FlowReturnError> =
crates/verter_session/src/host_flow_return_audit.rs:140:                        let key = dispatch.flow_return_key_with_demand(function, demand.clone());
crates/verter_session/src/host_flow_return_audit.rs:141:                        match dispatch.execute_flow_return(key) {
crates/verter_session/src/host_flow_return_audit.rs:142:                            crate::semantic_query::FlowReturnStep::Complete(result) => {
crates/verter_session/src/host_flow_return_audit.rs:145:                            crate::semantic_query::FlowReturnStep::NoValue(failure) => {
crates/verter_session/src/host_flow_return_audit.rs:152:                            crate::semantic_query::FlowReturnStep::Hold(_) => {
crates/verter_session/src/host_flow_return_audit.rs:153:                                Err(FlowReturnError::Failure(FlowReturnFailure::Unresolved))
crates/verter_session/src/host_flow_return_audit.rs:262:    outcome: Result<Arc<FlowReturnResult>, FlowReturnError>,
crates/verter_session/src/host_flow_return_audit.rs:264:) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:18://! shape is a typed `FlowReturnFailure` through `ReturnOnly` (never
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:34:    FlowReturnDegradation, FlowReturnFailure, FlowReturnKey, FlowReturnResult, FlowReturnStep,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:47:    Flow(FlowReturnResult),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:52:    /// `FlowReturnFailure` through `ReturnOnly` (never admitted) — the
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:59:    NoValue(FlowReturnFailure),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:102:/// The partial class EVERY typed NO-VALUE [`FlowReturnFailure`] carries:
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:134:/// the typed `FlowReturnFailure` itself is what the flow-return consumers
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:238:        FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:243:    NoValue(FlowReturnFailure),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:264:/// A hold-only [`FlowReturnFailure::EmptyCycle`] is the one failure the
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:278:    failure: FlowReturnFailure,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:279:    key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:282:    if matches!(failure, FlowReturnFailure::EmptyCycle) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:292:    Provisional(FlowReturnStep),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:301:    /// `FlowReturnKey` construction routes through here.
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:348:    /// The ONE `FlowReturnKey` construction: every body-derived
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:356:    pub(crate) fn flow_return_key_for(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:359:    ) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:363:        self.flow_return_key_with_demand(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:369:    /// Demand-parameterised half of [`Self::flow_return_key_for`].
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:373:    pub(crate) fn flow_return_key_with_demand(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:377:    ) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:378:        FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:398:    pub(crate) fn flow_return_key_for_instantiation(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:403:    ) -> FlowReturnKey {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:404:        let mut key = self.flow_return_key_with_demand(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:655:    /// constructs and executes the [`FlowReturnKey`] through
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:656:    /// [`Self::flow_return_key_for`] (never the `None → miss_node` arm);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:674:                let node = match self.execute_flow_return(self.flow_return_key_for(identity)) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:678:                    FlowReturnStep::Complete(result) => FunctionReturnNode::Flow(result),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:679:                    FlowReturnStep::NoValue(failure) => FunctionReturnNode::NoValue(failure),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:682:                    FlowReturnStep::Hold(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:683:                        FunctionReturnNode::NoValue(FlowReturnFailure::Unresolved)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:828:        let key = self.flow_return_key_with_demand(&identity, demand);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:829:        match self.execute_flow_return(key) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:830:            FlowReturnStep::Complete(result) if result.degradation().is_none() => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:934:    pub(crate) fn execute_flow_return(&self, key: FlowReturnKey) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:957:                return FlowReturnStep::Hold(Box::new(key));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:961:        if let Some(result) = self.graph().get_flow_return_result(self.ctx, &key) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:962:            return FlowReturnStep::Complete(result);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:968:            self.execute_flow_return_root(key)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:970:            self.execute_flow_return_inline(key)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:977:    fn execute_flow_return_root(&self, key: FlowReturnKey) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:978:        let read = self.execute_flow_return_cold_build(SemanticQueryKey::FlowReturn(Box::new(key)));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:981:                FlowReturnStep::Complete((*result).clone())
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:987:            _ => FlowReturnStep::NoValue(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:993:                    .unwrap_or(FlowReturnFailure::Unresolved),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1006:    /// here — the typed producer entry (`execute_flow_return_root`) AND
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1022:    pub(super) fn execute_flow_return_cold_build(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1046:        root_key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1076:    fn execute_flow_return_inline(&self, key: FlowReturnKey) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1078:            return FlowReturnStep::NoValue(FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1097:        key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1145:    fn flow_frame_open(&self, key: &FlowReturnKey) -> usize {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1164:    fn flow_frame_close(&self, idx: usize, evaluated: FlowEvaluationOutcome) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1168:                FlowRootClose::Complete(result, _, _) => FlowReturnStep::Complete(result),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1169:                FlowRootClose::NoValue(failure) => FlowReturnStep::NoValue(failure),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1180:        key: &FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1181:        result: FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1182:    ) -> FlowReturnResult {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1207:    ) -> FlowReturnStep {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1291:                failure: FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1308:                FlowReturnPendingOutcome::Complete(result) => FlowReturnStep::Complete(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1312:                    FlowReturnStep::NoValue(*failure)
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1445:                    FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1453:                                FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1457:                            _ => FlowReturnFailure::Unresolved,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1490:                        FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1494:                        FlowReturnFailure::Unresolved
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1547:            return FlowFramePop::RootClose(FlowRootClose::NoValue(FlowReturnFailure::Unresolved));
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1585:                    FlowFramePop::Provisional(FlowReturnStep::Complete(result))
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1609:        let index: rustc_hash::FxHashMap<&FlowReturnKey, usize> = entries
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1614:        let mut current: Vec<Option<FlowReturnResult>> = entries
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1705:                let next = FlowReturnResult::new(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1747:                        failure: FlowReturnFailure::EmptyCycle,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1921:    fn evaluate_flow_return(&self, key: &FlowReturnKey) -> FlowEvaluationOutcome {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1928:            |failure: FlowReturnFailure,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1943:        // family hit in `execute_flow_return` returns before any frame
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1956:            return degraded(FlowReturnFailure::UnmodeledDemandPoint, Vec::new());
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1964:                    return degraded(FlowReturnFailure::UnmodeledDemandPoint, Vec::new());
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1972:            return degraded(FlowReturnFailure::Missing, Vec::new());
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:1990:            return degraded(FlowReturnFailure::Missing, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2014:            return degraded(FlowReturnFailure::Unresolved, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2042:                    return degraded(FlowReturnFailure::Unresolved, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2057:                        FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2081:                            return degraded(FlowReturnFailure::Unresolved, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2105:            return degraded(FlowReturnFailure::Unresolved, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2112:            return degraded(FlowReturnFailure::Missing, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2117:            return degraded(FlowReturnFailure::Budget(reason), self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2123:            return degraded(FlowReturnFailure::UnmodeledDemandPoint, self_roots);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2340:            Some(_) => Err(FlowReturnFailure::Budget(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2356:                    fresh_seed: matches!(failure, FlowReturnFailure::EmptyCycle),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2382:                    fresh_seed: matches!(failure, FlowReturnFailure::EmptyCycle),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2425:    ) -> Result<(FlowReturnResult, bool), FlowReturnFailure> {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2491:                    FlowReturnResult::new(graph, return_type, can_fall_through, degradation),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2515:                return Err(FlowReturnFailure::EmptyCycle);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2548:            FlowReturnResult::new(graph, return_type, can_fall_through, degradation),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:2935:    self_slot: Option<&'b FlowReturnKey>,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:3167:/// frame-level `Err`: with `Result<_, FlowReturnFailure>` as the
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:3174:/// [`FlowReturnFailure`] is UNSPELLABLE, not merely unspelled: there is no
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:3176:/// `Result<_, FlowReturnFailure>` does not typecheck against a
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:4300:        FlowReturnFailure,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:5602:    ) -> Result<Option<SemanticNodeId>, FlowReturnFailure> {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:5605:            None => return Err(FlowReturnFailure::UnmodeledDemandPoint),
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:5608:            return Err(FlowReturnFailure::UnmodeledDemandPoint);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:5638:            return Err(FlowReturnFailure::UnmodeledDemandPoint);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:5736:    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:6803:                        Err(FlowReturnFailure::Unsupported(match kind {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8124:                        let key = self.dispatch.flow_return_key_for(identity);
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8132:                        match self.dispatch.execute_flow_return(key.clone()) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8133:                            FlowReturnStep::Complete(result) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8169:                            FlowReturnStep::Hold(key) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8174:                            FlowReturnStep::NoValue(FlowReturnFailure::EmptyCycle) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8187:                            FlowReturnStep::NoValue(_) => Positional::Unmodeled,
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8327:                match self.dispatch.execute_flow_return(self_slot.clone()) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8328:                    FlowReturnStep::Hold(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8335:                    FlowReturnStep::Complete(_) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8338:                    FlowReturnStep::NoValue(_) => Positional::Unmodeled,
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:89:    ClauseSpelling, FlowReturnKey, PrimitiveKind, QueryError, SemanticNodeData, SemanticNodeId,
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:660:        key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:680:    pub(super) fn foreign(key: FlowReturnKey, clause: CalleeClause) -> Self {
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:693:    pub(super) fn own_frame(key: FlowReturnKey) -> Self {
crates/verter_session/src/project_semantic_dispatch/flow_return_callee.rs:708:    pub(super) fn flow_key(&self) -> Option<&FlowReturnKey> {
crates/verter_session/src/project_semantic_dispatch/mod.rs:2193:            return self.execute_flow_return_cold_build(key);
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:13:use crate::semantic_query::{FlowReturnKey, SemanticNodeData, SemanticNodeId};
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:25:        flow_overrides: &FxHashMap<FlowReturnKey, SemanticNodeId>,
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:159:        flow_overrides: &FxHashMap<FlowReturnKey, SemanticNodeId>,
crates/verter_session/src/project_semantic_dispatch/return_equation.rs:190:                    .get_flow_return_result(self.ctx, key)
crates/verter_session/src/project_semantic_dispatch/relation.rs:314:    pub(super) key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/relation.rs:2019:                crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/relation.rs:2074:            crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:65:    CanonicalTypeSubstitution, ConstParamPolicy, ContextualInferenceMode, FlowReturnFailure,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:66:    FlowReturnKey, FlowReturnResult, IndexSignature, InferBinding, InferableParamSetId,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:224:    /// identity IS the `FlowReturnKey` exactly.
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:225:    FlowReturn(FlowReturnKey),
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:242:    pub(crate) fn as_flow_return(&self) -> Option<&FlowReturnKey> {
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:443:        key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:596:    Complete(FlowReturnResult),
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:600:        failure: FlowReturnFailure,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:605:        /// [`FlowReturnFailure::EmptyCycle`] member is RESURRECTED by the
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1837:    pub(crate) key: FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1838:    pub(crate) result: FlowReturnResult,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1876:    pub(crate) last_root_failure: Option<crate::semantic_query::FlowReturnFailure>,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1913:    pub(super) key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1929:    FlowReturn(FlowReturnKey),
crates/verter_session/src/semantic_query/flow_return_result.rs:1://! [`FlowReturnResult`] — the SUCCESS carrier of a `FlowReturn` query, and
crates/verter_session/src/semantic_query/flow_return_result.rs:6://! [`FlowReturnResult::new`] is the only way to set them anywhere in the
crates/verter_session/src/semantic_query/flow_return_result.rs:31://! [`FlowReturnResult::with_return_type`], re-derives.
crates/verter_session/src/semantic_query/flow_return_result.rs:45:/// NO-VALUE results are typed `FlowReturnFailure`s through
crates/verter_session/src/semantic_query/flow_return_result.rs:50:pub struct FlowReturnResult {
crates/verter_session/src/semantic_query/flow_return_result.rs:71:    /// only way to obtain a `FlowReturnResult` is [`Self::new`], which
crates/verter_session/src/semantic_query/flow_return_result.rs:77:impl FlowReturnResult {
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:524:                        crate::semantic_query::FlowReturnFailure::Budget(_) => {
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:534:                crate::semantic_query::FlowReturnKey,
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:1451:            ) => Some(self.flow_return_key_for(identity).function),
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:1982:                    let flow_key = self.flow_return_key_for_instantiation(
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:1993:                    match self.execute_flow_return(flow_key.clone()) {
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2001:                        crate::semantic_query::FlowReturnStep::Complete(result)
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2027:                        crate::semantic_query::FlowReturnStep::Complete(_)
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2028:                        | crate::semantic_query::FlowReturnStep::Hold(_) => {
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2031:                        crate::semantic_query::FlowReturnStep::NoValue(
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2032:                            crate::semantic_query::FlowReturnFailure::EmptyCycle,
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2039:                        crate::semantic_query::FlowReturnStep::NoValue(
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2040:                            crate::semantic_query::FlowReturnFailure::Budget(_),
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:2045:                        crate::semantic_query::FlowReturnStep::NoValue(_) => {
crates/verter_session/src/semantic_query/query_key_spec.rs:997:        // FlowReturn(key: FlowReturnKey) — the whole-function return
crates/verter_session/src/semantic_query/query_key_spec.rs:1007:        // (`FlowInputContext`) are KEY-EMBEDDED fields of `FlowReturnKey`
crates/verter_session/src/semantic_query/query_key_spec.rs:1024:            cross_context_guard: "flow_return_keys_do_not_warm_hit_across_env_axes",
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:790:                    crate::semantic_query::FlowReturnFailure::Budget(reason),
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:798:                    crate::semantic_query::FlowReturnFailure::CallResolution(_),
crates/verter_session/src/semantic_query.rs:114:mod flow_return_result;
crates/verter_session/src/semantic_query.rs:115:pub use flow_return_result::FlowReturnResult;
crates/verter_session/src/semantic_query.rs:1460:/// The demand axis of a [`FlowReturnKey`]: the flow-typed
crates/verter_session/src/semantic_query.rs:1492:/// The input axis of a [`FlowReturnKey`]: the contextual callback input
crates/verter_session/src/semantic_query.rs:1531:pub struct FlowReturnKey {
crates/verter_session/src/semantic_query.rs:1559:/// Typed degradation on a usable but incomplete [`FlowReturnResult`].
crates/verter_session/src/semantic_query.rs:1560:/// Not a failure substitute (`FlowReturnFailure` is no-value). First
crates/verter_session/src/semantic_query.rs:1609:    /// node by [`FlowReturnResult::new`], never recorded per-arm.
crates/verter_session/src/semantic_query.rs:1644:/// `FlowReturnFailure`.
crates/verter_session/src/semantic_query.rs:1648:/// `Result<_, FlowReturnFailure>` as the return type of the positional
crates/verter_session/src/semantic_query.rs:1658:pub enum FlowReturnFailure {
crates/verter_session/src/semantic_query.rs:1707:pub(crate) enum FlowReturnStep {
crates/verter_session/src/semantic_query.rs:1710:    Complete(FlowReturnResult),
crates/verter_session/src/semantic_query.rs:1714:    Hold(Box<FlowReturnKey>),
crates/verter_session/src/semantic_query.rs:1717:    /// [`FlowReturnResult::degradation`] value is, and sharing that word
crates/verter_session/src/semantic_query.rs:1720:    NoValue(FlowReturnFailure),
crates/verter_session/src/semantic_query.rs:4279:    /// typed `FlowReturnFailure` (an unmodelled control construct, a
crates/verter_session/src/semantic_query.rs:5147:    /// `FlowReturnFailure` through `ReturnOnly` and never admits.
crates/verter_session/src/semantic_query.rs:5148:    FlowReturn(Arc<FlowReturnResult>),
crates/verter_session/src/semantic_query.rs:7384:    /// The payload is the full [`FlowReturnKey`] identity: the function
crates/verter_session/src/semantic_query.rs:7397:    FlowReturn(Box<FlowReturnKey>),
crates/verter_session/src/semantic_query.rs:9573:                SemanticQueryValue::FlowReturn(Arc::new(FlowReturnResult::new(
crates/verter_session/src/semantic_query_memo/family.rs:521:    /// the full [`crate::semantic_query::FlowReturnKey`] — function slot
crates/verter_session/src/semantic_query_memo/family.rs:532:        key: Box<crate::semantic_query::FlowReturnKey>,
crates/verter_session/src/semantic_query_memo/family.rs:1876:        // key: the full `FlowReturnKey` IS the family identity (function
crates/verter_session/src/semantic_query_memo/tests.rs:10613:        SemanticQueryKey::FlowReturn(Box::new(crate::semantic_query::FlowReturnKey {
crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:7://! results ONLY: a typed `FlowReturnFailure` (`Unsupported` / `Missing` /
crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:51:        key: &crate::semantic_query::FlowReturnKey,
crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:114:    pub(crate) fn get_flow_return_result(
crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:117:        key: &crate::semantic_query::FlowReturnKey,
crates/verter_session/src/semantic_query_memo/flow_return_memo.rs:118:    ) -> Option<crate::semantic_query::FlowReturnResult> {
crates/verter_session/src/semantic_query_memo/scc_publish.rs:82:        key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/semantic_query_memo/scc_publish.rs:116:    pub(crate) key: crate::semantic_query::FlowReturnKey,
crates/verter_session/src/semantic_query_memo/scc_publish.rs:118:    pub(crate) result: crate::semantic_query::FlowReturnResult,
crates/verter_session/src/semantic_query_memo/mod.rs:50:mod flow_return_memo;
crates/verter_session/src/semantic_query_memo/mod.rs:65:pub(crate) use flow_return_memo::InlineFlowReturnFlight;

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_session/src/host_flow_return_audit.rs | sed -n '1,190p'; rg -n \"flow_return_audited|flow_return\\(\" crates/verter_session/src/lib.rs crates/verter_session/src/host.rs crates/verter_session/src/typeinfo crates/verter_napi/src crates/verter_wasm/src crates/verter_lsp/src packages --glob '*.{rs,ts}' | sed -n '1,260p'; nl -ba crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs | sed -n '740,825p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 20177ms:[0m
     1	#![deny(missing_docs)]
     2	//! `VerterHost::get_flow_return_type_with_audit` — the single public
     3	//! audited entry-point for whole-function flow-return inference.
     4	//!
     5	//! Wires the standard audit lifecycle (registration constructed BEFORE
     6	//! the TLS observer install, producer body run under the matching
     7	//! guard, per-request counters snapshotted at finalize) around one
     8	//! `SemanticQueryKey::FlowReturn` demand routed through the shared
     9	//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch`] —
    10	//! never a second resolver.
    11	//!
    12	//! Outcome mapping (the locked design's split result/carrier contract,
    13	//! C1): a COMPLETE evaluation — including a DEGRADED SUCCESS carrying
    14	//! `FlowReturnResult::degradation` — rides the [`AuditedResult`] `Ok`
    15	//! arm; a genuine NO-VALUE outcome (the typed
    16	//! [`crate::semantic_query::FlowReturnFailure`] class) rides the `Err`
    17	//! arm as [`FlowReturnError::Failure`]. Both arms carry the audit
    18	//! record.
    19	//!
    20	//! Cold-vs-warm audit contract: a warm family hit emits NO
    21	//! `FlowReturnStarted` structured event and its record reports
    22	//! `from_cache = true` with `cold_computes == 0`; the cold-path
    23	//! emission helpers construct no event payload without an installed
    24	//! accumulator (see [`crate::flow_return_audit`]).
    25	
    26	use std::sync::atomic::Ordering;
    27	use std::sync::Arc;
    28	
    29	use verter_audit::{
    30	    AuditedResult, FlowReturnInferencePayload, RequestAuditRecord, RequestKind, RequestKindPayload,
    31	    RequestMemoryAudit, RequestStoreAudit, RequestTimingAudit, WaitAudit,
    32	};
    33	
    34	use crate::host_audit_runtime::AuditRequestRegistration;
    35	use crate::instant::Instant;
    36	use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    37	use crate::request_context::{RequestContext, RequestContextGuard};
    38	use crate::semantic_query::{FlowReturnFailure, FlowReturnResult, ReturnProjectionDemand};
    39	use crate::VerterHost;
    40	
    41	/// Typed `Err` arm of the flow-return [`AuditedResult`] carrier —
    42	/// genuine NO-VALUE outcomes only. A degraded-but-usable result is NOT
    43	/// an error: it rides the `Ok` arm as a [`FlowReturnResult`] with
    44	/// `degradation: Some(_)` (the split result/carrier contract).
    45	#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    46	pub enum FlowReturnError {
    47	    /// The evaluation produced no value — the typed
    48	    /// [`FlowReturnFailure`] class (missing function, unsupported
    49	    /// control surface, torn state, empty recursive cycle, unmodeled
    50	    /// demand point, budget exhaustion).
    51	    Failure(FlowReturnFailure),
    52	    /// The host could not pin a proven-current store view within the
    53	    /// bounded retry window; the query was not resolved against
    54	    /// superseded state.
    55	    UnstableState {
    56	        /// Number of retry attempts made before giving up.
    57	        attempts: u8,
    58	    },
    59	}
    60	
    61	impl VerterHost {
    62	    /// Resolve one whole-function flow return through the shared
    63	    /// dispatch and return the result — or a typed
    64	    /// [`FlowReturnError`] — alongside the per-request
    65	    /// [`RequestAuditRecord`], packaged in one [`AuditedResult`].
    66	    ///
    67	    /// `function` is the content-free served-function identity (the
    68	    /// declaration anchor plus part/overload ordinal); `demand` is the
    69	    /// return-projection point. Production's canonical point is
    70	    /// [`ReturnProjectionDemand::whole_return`]; any narrower point is
    71	    /// accepted as key data and currently fails CLOSED with the typed
    72	    /// `UnmodeledDemandPoint` failure (never a silently widened
    73	    /// whole-return result).
    74	    ///
    75	    /// The carrier's `audit` field is always populated: an active
    76	    /// registration carries the full `FlowReturnInference` payload
    77	    /// ([`verter_audit::AuditCaptureState::ActiveStored`]); a filtered
    78	    /// or disabled registration carries the cheap default-filled
    79	    /// record ([`verter_audit::AuditCaptureState::FilteredNoop`] /
    80	    /// [`verter_audit::AuditCaptureState::AuditDisabled`]).
    81	    #[must_use]
    82	    pub fn get_flow_return_type_with_audit(
    83	        &self,
    84	        function: &verter_type_expr::facts::FlowFunctionReturnIdentity,
    85	        demand: ReturnProjectionDemand,
    86	    ) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
    87	        let canonical_id: &str = function.anchor.canonical_id.as_ref();
    88	        let function_symbol: &str = function.anchor.symbol.as_ref();
    89	
    90	        // Stamp a fresh request id and bookkeeping for the harness'
    91	        // multi-request guard. Mirrors the other audited entry-points.
    92	        let request_id = self.next_request_id();
    93	        crate::request_context::increment_requests_created();
    94	
    95	        // Construct a per-request context. The footprint-attachment
    96	        // pipeline plants the per-request accumulator (and workspace
    97	        // VFS audit sink) so flow-return requests attach a mined
    98	        // footprint when `footprint_capture=true`.
    99	        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
   100	        let timing_capture = self.config.audit_timing_capture && self.config.audit_enabled;
   101	        let footprint_scope = crate::typeinfo::footprint_attach::TypeinfoFootprintScope::install(
   102	            self,
   103	            request_id,
   104	            footprint_capture,
   105	        );
   106	        let ctx = RequestContext::with_kind_and_timing(
   107	            request_id,
   108	            Arc::<str>::from(canonical_id),
   109	            RequestKind::FlowReturnInference,
   110	            footprint_capture,
   111	            timing_capture,
   112	            footprint_scope.accumulator(),
   113	        );
   114	
   115	        // BEFORE installing the TLS guard: construct the registration.
   116	        let registration = Arc::new(AuditRequestRegistration::new(self, Arc::clone(&ctx)));
   117	        debug_assert!(
   118	            ctx.audit_registration.get().is_none(),
   119	            "freshly-constructed RequestContext must have no audit_registration",
   120	        );
   121	        let _ = ctx.install_audit_registration(Arc::clone(&registration));
   122	
   123	        // Resolve against a PROVEN-CURRENT snapshot (this entry-point
   124	        // returns the value with no outer publish fence). On sustained
   125	        // churn surface the typed `UnstableState` error rather than
   126	        // answering from superseded state.
   127	        let request_start = Instant::now();
   128	        let outcome: Result<Arc<FlowReturnResult>, FlowReturnError> =
   129	            match crate::typeinfo::current_store_view_for_query(self) {
   130	                Some(current_view) => {
   131	                    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
   132	                    let host_ctx = crate::resolver_core::HostResolverContext::from_current(
   133	                        self,
   134	                        &current_view,
   135	                        overlay,
   136	                    );
   137	                    let host_ctx_ref: &dyn crate::resolver_core::resolver_context::ResolverContext =
   138	                        &host_ctx;
   139	                    let run = |dispatch: &ProjectSemanticDispatch<'_>| {
   140	                        let key = dispatch.flow_return_key_with_demand(function, demand.clone());
   141	                        match dispatch.execute_flow_return(key) {
   142	                            crate::semantic_query::FlowReturnStep::Complete(result) => {
   143	                                Ok(Arc::new(result))
   144	                            }
   145	                            crate::semantic_query::FlowReturnStep::NoValue(failure) => {
   146	                                Err(FlowReturnError::Failure(failure))
   147	                            }
   148	                            // A hold cannot surface at a fresh top-level
   149	                            // transaction (no in-flight frame exists to
   150	                            // re-enter); treat a torn surfacing as
   151	                            // undecided, never a fabricated value.
   152	                            crate::semantic_query::FlowReturnStep::Hold(_) => {
   153	                                Err(FlowReturnError::Failure(FlowReturnFailure::Unresolved))
   154	                            }
   155	                        }
   156	                    };
   157	                    match registration.as_ref() {
   158	                        AuditRequestRegistration::Active(_) => {
   159	                            let _ctx_guard = RequestContextGuard::install(Arc::clone(&ctx));
   160	                            let dispatch = ProjectSemanticDispatch::new(host_ctx_ref);
   161	                            run(&dispatch)
   162	                        }
   163	                        AuditRequestRegistration::Noop => {
   164	                            let _noop_guard = verter_audit::install_noop_observer();
   165	                            let dispatch = ProjectSemanticDispatch::new(host_ctx_ref);
   166	                            run(&dispatch)
   167	                        }
   168	                    }
   169	                }
   170	                None => Err(FlowReturnError::UnstableState {
   171	                    attempts: crate::typeinfo::TYPEINFO_CURRENT_VIEW_RETRY_ATTEMPTS as u8,
   172	                }),
   173	            };
   174	        let total_ms = request_start.elapsed().as_secs_f64() * 1000.0;
   175	
   176	        // Filtered kinds: return the cheap default-filled record. The
   177	        // query still ran; no payload was collected and nothing is
   178	        // published.
   179	        if matches!(registration.as_ref(), AuditRequestRegistration::Noop) {
   180	            let state = if self.config.audit_enabled {
   181	                verter_audit::AuditCaptureState::FilteredNoop
   182	            } else {
   183	                verter_audit::AuditCaptureState::AuditDisabled
   184	            };
   185	            let record =
   186	                noop_flow_return_record(request_id, canonical_id, ctx.parent_request_id, state);
   187	            return audited_from_outcome(outcome, record);
   188	        }
   189	
   190	        // Build the audit record — only the `Active` arm reaches here.
rg: crates/verter_session/src/host.rs: No such file or directory (os error 2)
   740	        // for its splice text. Every degraded shape (typed failure, empty
   741	        // cycle, absent identity) fails closed here.
   742	        if candidate.position == TscInferredClassTypePosition::Return {
   743	            let Some(mut identity) = candidate.flow_return else {
   744	                return Err(ClassInferenceFailure::Unsupported(
   745	                    UnsupportedReason::SemanticConstruct,
   746	                ));
   747	            };
   748	            identity.anchor.canonical_id = std::sync::Arc::from(owner_canonical);
   749	            identity.anchor.owner = owner;
   750	            let (type_text, typeof_paths) = match dispatch.execute_function_return_source(
   751	                &verter_type_expr::facts::FunctionReturnSource::Flow(*identity),
   752	                owner_canonical,
   753	            ) {
   754	                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::Flow(result) => {
   755	                    // A DEGRADED SUCCESS never splices display text: this
   756	                    // projection is fail-closed (every degraded shape
   757	                    // refuses), and a modeled-`any` substitution is a
   758	                    // degraded shape even though it carries a usable value.
   759	                    if result.degradation().is_some() {
   760	                        return Err(ClassInferenceFailure::Unsupported(
   761	                            UnsupportedReason::SemanticConstruct,
   762	                        ));
   763	                    }
   764	                    let Some((safe, typeof_paths)) =
   765	                        crate::project_semantic_dispatch::raise::node_declaration_facts_with_dispatch(
   766	                            dispatch,
   767	                            result.return_type(),
   768	                        )
   769	                    else {
   770	                        return Err(ClassInferenceFailure::Unsupported(
   771	                            UnsupportedReason::SemanticConstruct,
   772	                        ));
   773	                    };
   774	                    if !safe {
   775	                        return Err(ClassInferenceFailure::Unsupported(
   776	                            UnsupportedReason::SemanticConstruct,
   777	                        ));
   778	                    }
   779	                    let Some(type_text) = crate::typeinfo::raise::render_node_display_with_ctx(
   780	                        ctx,
   781	                        result.return_type(),
   782	                    ) else {
   783	                        return Err(ClassInferenceFailure::Unsupported(
   784	                            UnsupportedReason::SemanticConstruct,
   785	                        ));
   786	                    };
   787	                    (type_text, typeof_paths)
   788	                }
   789	                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::NoValue(
   790	                    crate::semantic_query::FlowReturnFailure::Budget(reason),
   791	                ) => {
   792	                    return Err(ClassInferenceFailure::InferenceUnavailable(reason));
   793	                }
   794	                // A call in the body did not resolve: the member is typed
   795	                // UNSUPPORTED and suppresses admission — never widened
   796	                // back to `any`.
   797	                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::NoValue(
   798	                    crate::semantic_query::FlowReturnFailure::CallResolution(_),
   799	                ) => {
   800	                    return Err(ClassInferenceFailure::Unsupported(
   801	                        UnsupportedReason::SemanticConstruct,
   802	                    ));
   803	                }
   804	                _ => {
   805	                    return Err(ClassInferenceFailure::Unsupported(
   806	                        UnsupportedReason::SemanticConstruct,
   807	                    ));
   808	                }
   809	            };
   810	            value_dependencies.extend(typeof_paths);
   811	            let occurrence = occurrences
   812	                .entry((
   813	                    candidate.name.clone(),
   814	                    candidate.is_static,
   815	                    candidate.position,
   816	                ))
   817	                .or_insert(0_u32);
   818	            inferred.push(TscInferredClassMember {
   819	                name: candidate.name,
   820	                occurrence: *occurrence,
   821	                is_static: candidate.is_static,
   822	                position: candidate.position,
   823	                type_text: TscSpliceText::new(type_text),
   824	            });
   825	            *occurrence = occurrence.saturating_add(1);

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n --glob '*.rs' \"FunctionReturnSource::Flow|execute_function_return_source|get_flow_return_type_with_audit\" crates | sed -n '1,240p'; rg -n --glob '*.rs' \"flow_return\" crates/verter_session/src/typeinfo crates/verter_session/src/resolver_core/component_meta crates/verter_session/src/resolver_core/component_meta_query_engine crates/verter_compiler/src | sed -n '1,320p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 10805ms:[0m
crates/verter_audit/src/record.rs:459:    /// (`get_flow_return_type_with_audit`) opens a record with this
crates/verter_type_expr/src/facts.rs:4480:            FunctionReturnSource::Flow(identity) => anchor_scope_relative(&identity.anchor),
crates/verter_session/src/meta_tests.rs:2192:/// (`ProjectSemanticDispatch::execute_function_return_source`) is what
crates/verter_semantic/src/analysis/type_eval_build.rs:972:            FunctionReturnSource::Flow(FlowFunctionReturnIdentity {
crates/verter_semantic/src/analysis/function_program.rs:1402:            let source = FunctionReturnSource::Flow(FlowFunctionReturnIdentity {
crates/verter_semantic/src/analysis/function_program.rs:3555:                Some(verter_type_expr::facts::FunctionReturnSource::Flow(identity)),
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1383:    let FunctionReturnSource::Flow(identity) = &fact.return_source else {
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1413:        matches!(stored.return_source, FunctionReturnSource::Flow(_)),
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1436:        matches!(fact.return_source, FunctionReturnSource::Flow(_)),
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1462:            matches!(fact.return_source, FunctionReturnSource::Flow(_)),
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1499:    let FunctionReturnSource::Flow(identity) = &fact.return_source else {
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1536:        matches!(fact.return_source, FunctionReturnSource::Flow(_)),
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1549:        matches!(fact.return_source, FunctionReturnSource::Flow(_)),
crates/verter_semantic/src/analysis/type_eval_build_tests.rs:1572:    let FunctionReturnSource::Flow(identity) = &static_fact.return_source else {
crates/verter_semantic/src/analysis/type_eval.rs:1063:        if let verter_type_expr::facts::FunctionReturnSource::Flow(identity) =
crates/verter_session/src/flow_gap_retraction_tests.rs:72:    let carrier = host.get_flow_return_type_with_audit(
crates/verter_session/src/host_flow_return_audit.rs:2://! `VerterHost::get_flow_return_type_with_audit` — the single public
crates/verter_session/src/host_flow_return_audit.rs:82:    pub fn get_flow_return_type_with_audit(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:40:/// ([`ProjectSemanticDispatch::execute_function_return_source`]).
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:201:    /// ([`ProjectSemanticDispatch::execute_function_return_source`] calls
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:603:                    verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:608:                match self.execute_function_return_source(source, canonical) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:626:                        match self.execute_function_return_source(source, point.canonical_id.as_ref()) {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:658:    pub(crate) fn execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:673:            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:720:    /// (`FunctionReturnSource::Flow`) — dispatch
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:733:    /// (`FunctionReturnSource::Flow`), resolved through the prepared
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:756:        let verter_type_expr::facts::FunctionReturnSource::Flow(identity) =
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8109:                    verter_type_expr::facts::FunctionReturnSource::Flow(
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8123:                    verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
crates/verter_session/src/project_semantic_dispatch/flow_return.rs:8192:                        .execute_function_return_source(source, self.canonical)
crates/verter_session/src/u6_flow_expect_tests.rs:11://! 3. Public boundary via `get_flow_return_type_with_audit`, twice.
crates/verter_session/src/u6_flow_expect_tests.rs:66:     rail = body-derived FlowReturn via VerterHost::get_flow_return_type_with_audit";
crates/verter_session/src/u6_flow_expect_tests.rs:433:    /// `get_flow_return_type_with_audit` twice:
crates/verter_session/src/u6_flow_expect_tests.rs:449:    /// `get_flow_return_type_with_audit` twice; both calls refuse. Call 1
crates/verter_session/src/u6_flow_expect_tests.rs:545:        host.get_flow_return_type_with_audit(&ident, ReturnProjectionDemand::whole_return());
crates/verter_session/src/u6_flow_expect_tests.rs:589:        host.get_flow_return_type_with_audit(&ident, ReturnProjectionDemand::whole_return());
crates/verter_session/src/u6_flow_expect_tests.rs:707:    let carrier = host.get_flow_return_type_with_audit(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:393:    match dispatch.execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:394:        &verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:929:        let source = verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone());
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:931:            dispatch.execute_function_return_source(&source, CANONICAL)
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:970:            dispatch.execute_function_return_source(&authored, CANONICAL)
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:987:            dispatch.execute_function_return_source(&jsdoc, CANONICAL)
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1062:            dispatch.execute_function_return_source(&declared, UNION_CANONICAL)
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1077:        let plain = verter_type_expr::facts::FunctionReturnSource::Flow(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1085:            dispatch.execute_function_return_source(&plain, UNION_CANONICAL)
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1106:        let loop_source = verter_type_expr::facts::FunctionReturnSource::Flow(return_identity(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1114:        match dispatch.execute_function_return_source(&loop_source, CANONICAL) {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1124:            dispatch.execute_function_return_source(&absent, CANONICAL),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1258:            .execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1259:                &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1504:        match dispatch.execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1505:            &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1622:        let node = dispatch.execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:1623:            &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:4158:        let verter_type_expr::facts::FunctionReturnSource::Flow(identity) = &mut source else {
crates/verter_session/src/project_semantic_dispatch/flow_return_tests.rs:4166:        match dispatch.execute_function_return_source(&source, canonical) {
crates/verter_session/src/project_semantic_dispatch/lower.rs:2234:                                        verter_type_expr::facts::FunctionReturnSource::Flow(
crates/verter_session/src/project_semantic_dispatch/lower.rs:2238:                                let return_type = match self.execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/lower.rs:2239:                                    &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
crates/verter_session/src/semantic_query/flow_return_result.rs:21://!    (`ProjectSemanticDispatch::execute_function_return_source`) — the
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:1450:                verter_type_expr::facts::FunctionReturnSource::Flow(identity),
crates/verter_session/src/project_semantic_dispatch/call_resolve.rs:1981:                verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
crates/verter_session/src/project_semantic_dispatch/locator_shape.rs:1280:                            verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
crates/verter_session/src/project_semantic_dispatch/locator_shape.rs:1282:                        let return_type = match self.execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/locator_shape.rs:1283:                            &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
crates/verter_session/src/project_semantic_dispatch/semantic_source_compose.rs:735:            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
crates/verter_session/src/project_semantic_dispatch/semantic_source_compose.rs:737:                    verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
crates/verter_session/src/project_semantic_dispatch/semantic_source_compose.rs:742:                        self.execute_function_return_source(
crates/verter_session/src/project_semantic_dispatch/semantic_source_compose.rs:743:                            &verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:2395:/// warm-admitted, so `execute_function_return_source` never folded the
crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs:2972:/// `execute_function_return_source` never folds the cache-read rails, so
crates/verter_session/src/project_semantic_dispatch/call_resolve_tests.rs:169:        SignatureReturnCarrier::Function(verter_type_expr::facts::FunctionReturnSource::Flow(
crates/verter_session/src/project_semantic_dispatch/call_resolve_tests.rs:878:        SignatureReturnCarrier::Function(verter_type_expr::facts::FunctionReturnSource::Flow(
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:750:            let (type_text, typeof_paths) = match dispatch.execute_function_return_source(
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:751:                &verter_type_expr::facts::FunctionReturnSource::Flow(*identity),
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:2://! (`VerterHost::get_flow_return_type_with_audit`) — the
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:112:    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "makeThing"), whole());
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:155:    let cold = host.get_flow_return_type_with_audit(&ident, whole());
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:160:    let warm = host.get_flow_return_type_with_audit(&ident, whole());
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:195:        host.get_flow_return_type_with_audit(&identity(canonical, "tooManyReturns"), whole());
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:221:    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "recurse"), whole());
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:253:    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "makeThing"), narrower);
crates/verter_session/tests/cases/g_type/flow_return_audit_contract.rs:282:    let carrier = host.get_flow_return_type_with_audit(&identity(CANONICAL, "makeThing"), whole());
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:473:    /// Public cold/warm companion: `get_flow_return_type_with_audit`
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:761:/// consumer entry ([`ProjectSemanticDispatch::execute_function_return_source`]),
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:809:        verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:816:        verter_type_expr::facts::FunctionReturnSource::Flow(identity) => Some(identity.clone()),
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:819:    match dispatch.execute_function_return_source(&source, &canonical) {
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:1874:            let first = host.get_flow_return_type_with_audit(
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:1937:            let second = host.get_flow_return_type_with_audit(
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:2384:    /// `get_flow_return_type_with_audit` TWICE on a fresh host: the
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:2476:                            "get_flow_return_type_with_audit, invoked twice: call 1 must be \
crates/verter_session/src/u6_flow_shape_corpus_tests.rs:2491:                            "get_flow_return_type_with_audit, invoked twice: both calls must \
crates/verter_session/tests/cases/architecture_guards.rs:5706:        // `VerterHost::get_flow_return_type_with_audit` (the single
crates/verter_session/tests/cases/architecture_guards.rs:12783:        // `get_flow_return_type_with_audit` (verter_session) drives
crates/verter_session/tests/cases/architecture_guards.rs:12792:            "get_flow_return_type_with_audit",
crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs:2://! `VerterHost::get_flow_return_type_with_audit`.
crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs:76:fn get_flow_return_type_with_audit_propagates_observer_through_dispatch() {
crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs:88:            .get_flow_return_type_with_audit(
crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs:108:        "get_flow_return_type_with_audit must publish a FlowReturnInference record when \
crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs:127:fn get_flow_return_type_with_audit_observer_absent_outside_harness_window() {
crates/verter_session/tests/cases/g_misc0/flow_return_audit_tls_propagation.rs:140:            .get_flow_return_type_with_audit(
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:13://! - `flow_return_substrate_*` rows are the producer-routing contracts:
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:22:const SUBSTRATE: &str = "/fixtures/flow_return_substrate.ts";
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:26:fn flow_return_dispatch_bit() -> u32 {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:43:fn flow_return_dispatched(record: &verter_audit::RequestAuditRecord) -> bool {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:46:            payload.semantic_query_dispatch_mask & (1 << flow_return_dispatch_bit()) != 0
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:52:fn assert_flow_return_dispatched(record: &verter_audit::RequestAuditRecord, alias: &str) {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:54:        flow_return_dispatched(record),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:267:fn flow_return_substrate_serves_symbolic_call_return_complete() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:274:    assert_flow_return_dispatched(&record, "SubCallReturn");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:291:fn flow_return_substrate_fails_closed_on_a_this_call() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:296:    assert_flow_return_dispatched(&record, "SubThisCallRun");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:300:fn flow_return_substrate_serves_return_free_loop_transparent() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:307:    assert_flow_return_dispatched(&record, "SubCallAfterLoop");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:311:fn flow_return_substrate_keeps_return_bearing_loop_degraded() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:316:    assert_flow_return_dispatched(&record, "SubLoopReturn");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:320:fn flow_return_substrate_serves_switch_arm_join() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:334:    assert_flow_return_dispatched(&record, "SubSwitchReturn");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:338:fn flow_return_substrate_serves_try_return_widened() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:343:    assert_flow_return_dispatched(&record, "SubTryReturn");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:347:fn flow_return_substrate_empty_cycle_admits_nothing_and_never_produces_never() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:356:    assert_flow_return_dispatched(&record, "SubEmptyRecursion");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:360:fn flow_return_substrate_base_plus_recursion_admits_widened_number() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:365:    assert_flow_return_dispatched(&record, "SubBaseRecursion");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:369:fn flow_return_substrate_signature_raise_and_return_type_share_one_producer() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:388:    assert_flow_return_dispatched(&typeof_record, "typeof subShared");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:395:    assert_flow_return_dispatched(&alias_record, "SubCallerA");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:399:fn flow_return_substrate_value_environment_cannot_enter_type_substitution() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:407:    assert_flow_return_dispatched(&a_record, "SubCallerA");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:408:    assert_flow_return_dispatched(&b_record, "SubCallerB");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:416:fn flow_return_substrate_mixed_scc_records_flow_frame_inside_relation() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:421:    assert_flow_return_dispatched(&record, "SubMixedAssign");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:433:fn flow_return_substrate_mutual_recursion_discharges_on_the_concrete_seed() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:463:fn flow_return_substrate_empty_mutual_cycle_admits_nothing() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:492:fn flow_return_substrate_returned_object_method_return_evaluates() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:517:fn flow_return_substrate_nested_block_arrow_return_evaluates() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:545:fn flow_return_substrate_export_default_object_method_return_evaluates() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:556:export type DefaultMethodReturn = ReturnType<(typeof import("/fixtures/flow_return_substrate.ts")["default"])["m"]>;
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:577:fn flow_return_substrate_mixed_component_invalidates_on_nested_flow_member_edit() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:651:fn flow_return_substrate_three_member_cycle_discharges_on_the_seed() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:679:fn flow_return_substrate_forward_dependency_cycle_is_order_independent() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:709:fn flow_return_substrate_class_field_function_initializer_evaluates() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:776:fn flow_return_substrate_multi_seed_cycle_publishes_the_union_msa_first() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:789:fn flow_return_substrate_multi_seed_cycle_publishes_the_union_msb_first() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:826:fn flow_return_substrate_multi_seed_cycle_under_a_relation_root() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs:839:fn flow_return_substrate_multi_seed_cycle_under_a_relation_root_after_a_flow_drain() {
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:488:        flow_return: Option<Box<verter_type_expr::facts::FlowFunctionReturnIdentity>>,
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:522:                                    flow_return: None,
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:545:                                        flow_return: None,
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:570:                                        flow_return: method.function.flow_return.clone(),
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:645:                            flow_return: None,
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:668:                                flow_return: None,
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:691:                                flow_return: method.function.flow_return.clone(),
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:715:                                flow_return: None,
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:743:            let Some(mut identity) = candidate.flow_return else {
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:754:                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::Flow(result) => {
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:789:                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::NoValue(
crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:797:                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::NoValue(
crates/verter_session/src/typeinfo/vue_macro_codegen/runtime.rs:780:pub(super) fn flow_return_degradation_observed() -> bool {
crates/verter_session/src/typeinfo/vue_macro_codegen/runtime.rs:867:    /// `a_no_surface_flow_return_refuses_even_when_a_sibling_arm_contributes`.
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:45:pub(super) const FLOW_RETURN_CATALOG: &str = include_str!("fixtures/flow_return_catalog.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:46:pub(super) const FLOW_RETURN_SUBSTRATE: &str = include_str!("fixtures/flow_return_substrate.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:48:    include_str!("fixtures/flow_return_edge_catalog.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:49:pub(super) const FLOW_RETURN_EDGE_CROSS: &str = include_str!("fixtures/flow_return_edge_cross.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:50:pub(super) const FLOW_RETURN_PATH_OWNER: &str = include_str!("fixtures/flow_return_path_owner.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:52:    include_str!("fixtures/flow_return_path_barrel.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:54:    include_str!("fixtures/flow_return_path_selected.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:56:    include_str!("fixtures/flow_return_path_alternate.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:58:    include_str!("fixtures/flow_return_path_unused.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:60:    include_str!("fixtures/flow_return_parity_catalog.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:62:    include_str!("fixtures/flow_return_parity_aug_owner.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:64:    include_str!("fixtures/flow_return_parity_aug_barrel.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:66:    include_str!("fixtures/flow_return_parity_aug_base.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:68:    include_str!("fixtures/flow_return_parity_aug_patch.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:70:    include_str!("fixtures/flow_return_parity_aug_unused.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:72:    include_str!("fixtures/flow_return_cross_types.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:74:    include_str!("fixtures/flow_return_cross_factory.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:76:    include_str!("fixtures/flow_return_cross_guards.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:78:    include_str!("fixtures/flow_return_cross_source.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:80:    include_str!("fixtures/flow_return_cross_index.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:81:pub(super) const FLOW_RETURN_CROSS_MAIN: &str = include_str!("fixtures/flow_return_cross_main.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:83:    include_str!("fixtures/flow_return_cross_package_main.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:85:    include_str!("fixtures/flow_return_package_declarations.ts");
crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:87:    include_str!("fixtures/flow_return_edge_package_declarations.ts");
crates/verter_session/src/typeinfo/vue_macro_codegen.rs:1377:        if classify_constructors && props.is_empty() && flow_return_degradation_observed() {
crates/verter_session/src/typeinfo/vue_macro_codegen.rs:1445:        if renders_runtime_options && emits.is_empty() && flow_return_degradation_observed() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:28:fn upsert_flow_return_fixture(host: &VerterHost) {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:36:fn upsert_flow_return_cross_fixture(host: &VerterHost) {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:37:    upsert_flow_return_cross_fixture_at(host, "/fixtures");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:40:fn upsert_flow_return_cross_fixture_at(host: &VerterHost, dir: &str) {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:43:        &format!("{dir}/flow_return_cross_types.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:48:        &format!("{dir}/flow_return_cross_factory.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:53:        &format!("{dir}/flow_return_cross_guards.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:58:        &format!("{dir}/flow_return_cross_source.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:63:        &format!("{dir}/flow_return_cross_index.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:68:        &format!("{dir}/flow_return_cross_main.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:73:fn upsert_flow_return_cross_package_fixture_at(host: &VerterHost, dir: &str) {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:76:        &format!("{dir}/flow_return_cross_package_main.ts"),
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:86:    upsert_flow_return_fixture(&host);
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:103:    upsert_flow_return_fixture(&host);
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:158:    upsert_flow_return_cross_fixture(&host);
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:161:        "/fixtures/flow_return_cross_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:181:        "/fixtures/flow_return_cross_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:227:    upsert_flow_return_cross_package_fixture_at(&host, "/workspace/src");
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:229:        "/workspace/src/flow_return_cross_package_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:248:        "/workspace/src/flow_return_cross_package_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:270:        "/workspace/src/flow_return_cross_package_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:433:fn flow_return_xf_fixture_routes_are_hermetic_and_resolvable() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:435:    upsert_flow_return_cross_fixture(&host);
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:438:        "/fixtures/flow_return_cross_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:439:        "./flow_return_cross_types",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:444:        Some("/fixtures/flow_return_cross_types.ts")
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:448:        "/fixtures/flow_return_cross_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:449:        "./flow_return_cross_factory",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:454:        Some("/fixtures/flow_return_cross_factory.ts")
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:458:        "/fixtures/flow_return_cross_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:459:        "./flow_return_cross_guards",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:464:        Some("/fixtures/flow_return_cross_guards.ts")
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:468:        "/fixtures/flow_return_cross_main.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:469:        "./flow_return_cross_index",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:474:        Some("/fixtures/flow_return_cross_index.ts")
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:478:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:479:        "./flow_return_cross_source",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:484:        Some("/fixtures/flow_return_cross_source.ts")
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:488:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:489:        "./flow_return_cross_guards",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:494:        Some("/fixtures/flow_return_cross_guards.ts")
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:499:fn flow_return_bl03_multi_return_union_is_collected() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:506:fn flow_return_bl05_explicit_never_annotation_wins() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:513:fn flow_return_bl14_unreachable_branch_reference_behavior_is_collected() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:520:fn flow_return_ob05_satisfies_preserves_value_shape() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:529:fn flow_return_cf13_labeled_break_current_return_collection() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:536:    flow_return_bl01_widens_primitive_literal_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:542:    flow_return_bl02_widens_object_return_properties_selectively,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:552:    flow_return_bl04_adds_implicit_undefined_for_fallthrough,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:562:    flow_return_bl06_ignores_throw_branch_and_widens_surviving_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:569:    flow_return_bl07_wraps_async_return_in_promise,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:579:    flow_return_bl08_constructs_generator_protocol_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:591:    flow_return_bl09_preserves_readonly_tuple_const_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:605:    flow_return_bl10_widens_mutable_array_element_union,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:616:    flow_return_bl11_constructs_async_generator_protocol_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:628:    flow_return_bl12_models_bare_return_as_void,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:634:fn flow_return_bl13_throw_only_body_is_never() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:642:    flow_return_bl15_models_divergent_loop_as_void,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:649:    flow_return_lr01_resolves_parameter_identifier_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:656:    flow_return_lr02_resolves_const_local_identifier_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:663:    flow_return_lr03_tracks_let_alias_narrowing,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:673:    flow_return_lr04_applies_reassignment_before_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:680:    flow_return_lr05_widens_mutated_let_literal,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:687:    flow_return_lr06_tracks_destructuring_alias_flow,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:702:    flow_return_lr07_projects_nested_parameter_member_path,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:708:    flow_return_lr08_propagates_optional_chain_undefined,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:718:    flow_return_lr09_applies_non_null_assertion,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:729:    flow_return_lr10_invalidates_captured_local_after_unknown_call,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:742:    flow_return_lr11_models_var_hoist_and_maybe_assignment,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:749:    flow_return_lr12_tracks_member_assignment_fact,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:756:    flow_return_cn01_tracks_typeof_positive_and_negative_branches,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:766:    flow_return_cn02_filters_truthy_and_falsy_literals,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:773:    flow_return_cn03_applies_nullish_equality_narrowing,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:780:    flow_return_cn04_applies_strict_undefined_narrowing,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:787:    flow_return_cn05_applies_literal_equality_narrowing,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:794:    flow_return_cn06_switch_discriminant_joins_case_returns,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:804:    flow_return_cn07_in_operator_narrows_object_union,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:814:    flow_return_cn08_models_array_is_array_predicate,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:821:    flow_return_cn09_models_instanceof_class_narrowing,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:828:    flow_return_cn10_composes_nested_boolean_guard_facts,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:839:    flow_return_cn11_applies_negative_typeof_guard,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:850:    flow_return_cn12_eliminates_exhaustive_never_tail,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:857:    flow_return_cn13_narrows_optional_property_truthiness,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:864:    flow_return_cn14_correlates_equality_between_variables,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:876:// covered in flow_return_path_contracts FP05/FP07 (where the contributing
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:879:    flow_return_cn15_narrows_nested_discriminant_paths,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:893:// See FP05 in flow_return_path_contracts for a per-branch correlated test.
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:895:    flow_return_cn16_preserves_destructured_discriminant_correlation,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:905:    flow_return_pa01_applies_local_type_predicate_signature,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:915:    flow_return_pa02_applies_asserts_is_signature,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:922:    flow_return_pa03_applies_asserts_condition_truthiness,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:929:    flow_return_pa04_instantiates_generic_predicate,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:936:    flow_return_pa05_applies_importable_predicate_signature,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:943:    flow_return_pa06_sequences_chained_predicates,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:953:    flow_return_pa07_refines_property_shape_from_predicate,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:963:    flow_return_pa08_applies_assertion_effect,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:970:    flow_return_pa09_uses_declared_predicate_without_body,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:980:    flow_return_cg01_expands_local_function_call_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:989:    flow_return_cg02_infers_generic_identity_call_argument,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:999:    flow_return_cg03_instantiates_generic_wrapper_return_annotation,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1009:    flow_return_cg04_selects_matching_overload_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1016:    flow_return_cg05_infers_rest_parameter_literal_union,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1025:    flow_return_cg06_uses_default_parameter_initializer_type,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1032:    flow_return_cg07_infers_generic_from_callback_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1039:    flow_return_cg08_contextually_types_callback_parameter,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1046:    flow_return_cg09_uses_constraint_for_member_return_widening,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1053:fn flow_return_cg10_terminates_recursive_return_inference() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1058:    flow_return_cg11_lowers_constructor_call_to_instance_type,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1065:    flow_return_ho01_infers_computed_style_callback_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1075:    flow_return_ho02_applies_filter_predicate_overload,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1082:    flow_return_ho03_maps_callback_return_union_to_array_element,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1093:    flow_return_ho04_infers_reduce_accumulator_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1100:    flow_return_ho05_models_flat_map_callback_flattening,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1107:    flow_return_ho06_infers_custom_generic_callback_helper,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1114:    flow_return_ho07_composes_nested_callback_predicates,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1124:    flow_return_ho08_narrows_discriminant_inside_map_callback,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1136:fn flow_return_ho09_keeps_unknown_declared_callback_result_opaque() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1143:    flow_return_ho10_returns_closure_with_captured_substitution,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1159:    flow_return_ho11_wraps_promise_all_async_callback_result,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1173:// flow_return_path_contracts (FP01 etc.) where the branch types diverge.
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1175:    flow_return_ob01_tracks_member_path_flow_facts,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1185:    flow_return_ob02_materializes_spread_override_order,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1205:    flow_return_ob03_synthesizes_optional_property_for_conditional_spread,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1217:    flow_return_ob04_preserves_deep_const_assertion_readonly_literals,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1231:    flow_return_ob06_projects_indexed_access_through_constraint,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1238:    flow_return_ob07_evaluates_computed_literal_object_key,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1248:    flow_return_ob08_preserves_readonly_parameter_shape,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1259:    flow_return_ob09_instantiates_keyof_driven_array_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1266:    flow_return_ob10_instantiates_mapped_return_annotation,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1277:    flow_return_ob11_reduces_conditional_return_annotation,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1306:    flow_return_ob12_keeps_unique_symbol_computed_key_shape,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1332:    flow_return_ob13_infers_getter_property_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1343:    flow_return_cf01_joins_nested_if_flow_returns,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1354:    flow_return_cf02_types_logical_and_short_circuit_expression,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1364:    flow_return_cf03_filters_falsy_left_side_of_logical_or,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1371:    flow_return_cf04_removes_nullish_constituents_for_coalescing,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1378:    flow_return_cf05_narrows_continuation_after_early_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1385:    flow_return_cf06_joins_try_and_catch_returns,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1392:    flow_return_cf07_models_loop_break_return_paths,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1402:    flow_return_cf08_joins_continue_and_accumulator_assignment,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1412:    flow_return_cf09_solves_loop_mutation_fixed_point,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1419:    flow_return_cf10_models_finally_reference_behavior,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1431:    flow_return_cf11_invalidates_closure_captured_flow_facts,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1438:    flow_return_cf12_invalidates_object_member_facts_after_unknown_call,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1448:    flow_return_cf14_models_switch_fallthrough_case_facts,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1455:    flow_return_cf15_separates_definite_assignment_diagnostics_from_return_type,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1462:    flow_return_cf16_terminates_budgeted_complex_loop_flow,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1473:    flow_return_xf01_uses_imported_type_for_parameter_member_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1475:    &["/fixtures/flow_return_cross_types.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1477:        "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1478:        "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1479:        "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1480:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1488:fn flow_return_xf02_expands_imported_value_function_return() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1495:        &["/fixtures/flow_return_cross_factory.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1497:            "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1498:            "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1499:            "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1500:            "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1506:    flow_return_xf03_applies_imported_predicate_flow_fact,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1508:    &["/fixtures/flow_return_cross_guards.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1510:        "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1511:        "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1512:        "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1513:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1521:fn flow_return_xf04_expands_barrel_imported_value_function_return() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1528:        &["/fixtures/flow_return_cross_source.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1530:            "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1531:            "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1532:            "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1539:fn flow_return_xf04_records_barrel_route_before_selected_leaf() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1547:            "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1548:            "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1551:            "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1552:            "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1553:            "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1560:fn flow_return_xf05_resolves_namespace_import_value_call() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1567:        &["/fixtures/flow_return_cross_factory.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1569:            "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1570:            "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1571:            "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1572:            "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1579:fn flow_return_xf06_keeps_value_type_namespace_separate() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1586:        &["/fixtures/flow_return_cross_factory.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1588:            "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1589:            "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1590:            "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1591:            "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1597:    flow_return_xf07_preserves_predicate_signature_through_reexport_alias,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1600:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1601:        "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1604:        "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1605:        "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1606:        "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1613:    flow_return_xf08_applies_imported_assertion_signature,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1615:    &["/fixtures/flow_return_cross_guards.ts"],
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1617:        "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1618:        "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1619:        "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1620:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1628:fn flow_return_xf09_terminates_cross_file_recursive_returns() {
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1637:            "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1638:            "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1639:            "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1640:            "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1641:            "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1647:    flow_return_xf10_uses_external_declaration_signature_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1654:    flow_return_xf11_applies_ambient_global_predicate,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1658:        "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1659:        "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1660:        "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1661:        "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1662:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1669:    flow_return_pa08_barrel_assertion_import_preserves_effect,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1672:        "/fixtures/flow_return_cross_index.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1673:        "/fixtures/flow_return_cross_guards.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1676:        "/fixtures/flow_return_cross_types.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1677:        "/fixtures/flow_return_cross_factory.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1678:        "/fixtures/flow_return_cross_source.ts",
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1685:    flow_return_vv01_infers_computed_callback_union_value,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1721:    flow_return_vv02_pins_ref_literal_widening_policy,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1740:    flow_return_vv03_infers_unref_union_helper_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1750:    flow_return_vv04_narrows_reactive_member_truthiness,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1757:    flow_return_vv05_expands_props_factory_returntype_with_widening,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1776:    flow_return_vv06_projects_default_factory_callback_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1807:    flow_return_vv07_materializes_composable_return_object,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1826:    flow_return_vv08_computed_from_discriminated_props,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1837:    flow_return_vv09_optional_slot_call_projects_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1857:    flow_return_vv10_template_ref_returntype_optional_method_call,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1872:    flow_return_vv11_resolves_emit_call_signature_return,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1879:    flow_return_vv12_contextually_types_model_transform_callbacks,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1886:    flow_return_vv13_applies_callback_mutation_policy_for_watch,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1893:    flow_return_vv14_instantiates_injection_assertion_helper,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1900:    flow_return_vv15_expands_barrel_imported_macro_factory,
crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_catalog.rs:1903:        "/fixtures/flow_return_cross_index.ts",

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"SkeletonBindingId|stable binding|binding slots|FlowNodeId|BindingSlot\" docs/arch/refactor/rev11 crates/verter_semantic/src crates/verter_type_expr/src crates/verter_session/src --glob '*.{md,rs}' | sed -n '1,360p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 4386ms:[0m
crates/verter_semantic/src/analysis/flow/lower.rs:24:use super::flow_graph::{FlowEdgeKind, FlowNodeId, FlowNodeKind, FunctionFlowGraph};
crates/verter_semantic/src/analysis/flow/lower.rs:32:    FunctionBodySkeleton, SkeletonBindingId, SkeletonCallee, SkeletonExprShape, SkeletonExprSiteId,
crates/verter_semantic/src/analysis/flow/lower.rs:50:    let mut selected_bindings: Vec<(SkeletonBindingId, bool)> = Vec::new();
crates/verter_semantic/src/analysis/flow/lower.rs:51:    let mut classify = |node: FlowNodeId, value: bool| match graph.node_kind(node) {
crates/verter_semantic/src/analysis/flow/peeker_tests.rs:11:    build_function_flow_graph, FlowEdgeKind, FlowNodeId, FlowNodeKind, FunctionFlowGraph,
crates/verter_semantic/src/analysis/flow/peeker_tests.rs:60:) -> FlowNodeId {
crates/verter_semantic/src/analysis/flow/peeker_tests.rs:82:) -> FlowNodeId {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:35:    FunctionBodySkeleton, SkeletonBindingId, SkeletonCallee, SkeletonExprShape, SkeletonExprSiteId,
crates/verter_semantic/src/analysis/flow/flow_graph.rs:56:pub struct FlowNodeId(u32);
crates/verter_semantic/src/analysis/flow/flow_graph.rs:58:impl FlowNodeId {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:76:    Binding(SkeletonBindingId),
crates/verter_semantic/src/analysis/flow/flow_graph.rs:141:    pub from: FlowNodeId,
crates/verter_semantic/src/analysis/flow/flow_graph.rs:143:    pub to: FlowNodeId,
crates/verter_semantic/src/analysis/flow/flow_graph.rs:184:    pub fn binding_node(&self, id: SkeletonBindingId) -> FlowNodeId {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:186:        FlowNodeId(id.index() as u32)
crates/verter_semantic/src/analysis/flow/flow_graph.rs:191:    pub fn expr_site_node(&self, id: SkeletonExprSiteId) -> FlowNodeId {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:193:        FlowNodeId(self.binding_count + id.index() as u32)
crates/verter_semantic/src/analysis/flow/flow_graph.rs:198:    pub fn return_site_node(&self, id: SkeletonReturnSiteId) -> FlowNodeId {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:200:        FlowNodeId(self.binding_count + self.expr_site_count + id.index() as u32)
crates/verter_semantic/src/analysis/flow/flow_graph.rs:205:    pub fn region_node(&self, id: SkeletonRegionId) -> FlowNodeId {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:207:        FlowNodeId(
crates/verter_semantic/src/analysis/flow/flow_graph.rs:214:    pub fn node_kind(&self, node: FlowNodeId) -> FlowNodeKind {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:217:            return FlowNodeKind::Binding(SkeletonBindingId::from_index(index));
crates/verter_semantic/src/analysis/flow/flow_graph.rs:232:    pub fn out_edges(&self, node: FlowNodeId) -> &[FlowEdge] {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:249:    let binding_node = |id: SkeletonBindingId| FlowNodeId(id.index() as u32);
crates/verter_semantic/src/analysis/flow/flow_graph.rs:250:    let site_node = |id: SkeletonExprSiteId| FlowNodeId(binding_count + id.index() as u32);
crates/verter_semantic/src/analysis/flow/flow_graph.rs:252:        |id: SkeletonReturnSiteId| FlowNodeId(binding_count + expr_site_count + id.index() as u32);
crates/verter_semantic/src/analysis/flow/flow_graph.rs:254:        FlowNodeId(binding_count + expr_site_count + return_site_count + id.index() as u32)
crates/verter_semantic/src/analysis/flow/flow_graph.rs:281:        |name: super::FlowNameId, region: SkeletonRegionId| -> Vec<SkeletonBindingId> {
crates/verter_semantic/src/analysis/flow/flow_graph.rs:285:    let mut edges: Vec<(FlowNodeId, FlowNodeId, FlowEdgeKind)> = Vec::new();
crates/verter_semantic/src/analysis/flow/flow_graph.rs:304:        let node = binding_node(SkeletonBindingId::from_index(index as u32));
crates/verter_semantic/src/analysis/flow/mod.rs:77:pub struct SkeletonBindingId(u32);
crates/verter_semantic/src/analysis/flow/mod.rs:79:impl SkeletonBindingId {
crates/verter_semantic/src/analysis/flow/mod.rs:591:    pub fn bindings_named(&self, name: FlowNameId) -> impl Iterator<Item = SkeletonBindingId> + '_ {
crates/verter_semantic/src/analysis/flow/mod.rs:596:            .filter_map(|(index, _)| u32::try_from(index).ok().map(SkeletonBindingId))
crates/verter_semantic/src/analysis/flow/mod.rs:607:    pub fn binding(&self, id: SkeletonBindingId) -> &SkeletonBinding {
crates/verter_semantic/src/analysis/flow/mod.rs:673:    ) -> Vec<SkeletonBindingId> {
crates/verter_semantic/src/analysis/flow/mod.rs:676:            let mut hits: Vec<SkeletonBindingId> = Vec::new();
crates/verter_semantic/src/analysis/flow/mod.rs:682:                    hits.push(SkeletonBindingId::from_index(index as u32));
crates/verter_semantic/src/analysis/flow/mod.rs:767:    fn hoisting_bindings_of_name(&self, name: FlowNameId) -> Vec<SkeletonBindingId> {
crates/verter_semantic/src/analysis/flow/mod.rs:781:            .map(|(index, _)| SkeletonBindingId::from_index(index as u32))
crates/verter_semantic/src/analysis/flow/hashing.rs:33:use super::flow_graph::{FlowEdgeKind, FlowNodeId, FunctionFlowGraph};
crates/verter_semantic/src/analysis/flow/hashing.rs:149:    let mut fold_edges = |nodes: &[FlowNodeId]| {
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:8:    build_function_body_skeleton, FunctionBodySkeleton, FunctionBodySource, SkeletonBindingId,
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:17:fn single_binding(skeleton: &FunctionBodySkeleton, name: &str) -> SkeletonBindingId {
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:31:    from: FlowNodeId,
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:43:    from: FlowNodeId,
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:44:    to: FlowNodeId,
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:53:fn out_path_writes(graph: &FunctionFlowGraph, from: FlowNodeId) -> Vec<&FlowEdge> {
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:329:    let bindings: Vec<SkeletonBindingId> = skeleton.bindings_named(x_name).collect();
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:444:    let bindings: Vec<SkeletonBindingId> = skeleton.bindings_named(x_name).collect();
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:536:        let node = FlowNodeId::from_index(index as u32);
docs/arch/refactor/rev11/baseline/9af553dd.md:122:- function programs/skeletons carry stable binding identities and structural inventories;
crates/verter_semantic/src/analysis/flow/peeker.rs:34:use super::flow_graph::{FlowEdge, FlowEdgeClass, FlowEdgeKind, FlowNodeId, FunctionFlowGraph};
crates/verter_semantic/src/analysis/flow/peeker.rs:197:        node: FlowNodeId,
crates/verter_semantic/src/analysis/flow/peeker.rs:205:        node: FlowNodeId,
crates/verter_semantic/src/analysis/flow/peeker.rs:271:        let mut value: Vec<FlowNodeId> = state.value_nodes.into_iter().collect();
crates/verter_semantic/src/analysis/flow/peeker.rs:273:        let mut effect_only: Vec<FlowNodeId> = state
crates/verter_semantic/src/analysis/flow/peeker.rs:295:        node: FlowNodeId,
crates/verter_semantic/src/analysis/flow/peeker.rs:391:        node: FlowNodeId,
crates/verter_semantic/src/analysis/flow/peeker.rs:414:    value_nodes: FxHashSet<FlowNodeId>,
crates/verter_semantic/src/analysis/flow/peeker.rs:415:    effect_nodes: FxHashSet<FlowNodeId>,
crates/verter_semantic/src/analysis/flow/peeker.rs:416:    selected: FxHashSet<FlowNodeId>,
crates/verter_semantic/src/analysis/flow/peeker.rs:417:    value_visited: FxHashSet<(FlowNodeId, u32)>,
crates/verter_semantic/src/analysis/flow/peeker.rs:418:    effect_visited: FxHashSet<FlowNodeId>,
crates/verter_semantic/src/analysis/flow/peeker.rs:425:    node: FlowNodeId,
crates/verter_semantic/src/analysis/flow/flow_ir.rs:16:use super::flow_graph::FlowNodeId;
crates/verter_semantic/src/analysis/flow/flow_ir.rs:19:    FrameSpan, SkeletonBindingId, SkeletonBindingKind, SkeletonExprSiteId, SkeletonWriteCertainty,
crates/verter_semantic/src/analysis/flow/flow_ir.rs:39:    pub value_nodes: Arc<[FlowNodeId]>,
crates/verter_semantic/src/analysis/flow/flow_ir.rs:42:    pub effect_only_nodes: Arc<[FlowNodeId]>,
crates/verter_semantic/src/analysis/flow/flow_ir.rs:48:    pub fn is_selected(&self, node: FlowNodeId) -> bool {
crates/verter_semantic/src/analysis/flow/flow_ir.rs:54:    pub fn is_value(&self, node: FlowNodeId) -> bool {
crates/verter_semantic/src/analysis/flow/flow_ir.rs:62:    pub fn is_effect_only(&self, node: FlowNodeId) -> bool {
crates/verter_semantic/src/analysis/flow/flow_ir.rs:148:    pub binding: SkeletonBindingId,
docs/arch/refactor/rev11/contracts/flow-completeness.md:81:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
crates/verter_session/src/host_semantic.rs:186:    /// Uses stable binding name lookup through the semantic snapshot.
crates/verter_session/src/flow_slice_content.rs:66:    ObjectEntryKind, SkeletonBindingId, SkeletonBindingKind, SkeletonPathSegment,
crates/verter_session/src/flow_slice_content.rs:2372:    nested_free_writes: FxHashSet<SkeletonBindingId>,
crates/verter_session/src/flow_slice_content.rs:2373:    active_guard_bindings: Vec<SkeletonBindingId>,
crates/verter_session/src/flow_slice_content.rs:2426:        binding: verter_semantic::analysis::flow::SkeletonBindingId,
crates/verter_session/src/flow_slice_content.rs:2437:        binding: verter_semantic::analysis::flow::SkeletonBindingId,
crates/verter_session/src/flow_slice_content.rs:2445:        binding: verter_semantic::analysis::flow::SkeletonBindingId,
crates/verter_session/src/flow_slice_content.rs:2712:    ) -> FxHashSet<SkeletonBindingId> {
crates/verter_session/src/flow_slice_content.rs:2792:    ) -> FxHashSet<SkeletonBindingId> {
crates/verter_session/src/flow_slice_content.rs:2822:        binding: SkeletonBindingId,
crates/verter_session/src/flow_slice_content.rs:2848:        binding: SkeletonBindingId,
crates/verter_session/src/flow_slice_content.rs:2865:    fn guard_bindings(&self, guard: &SliceGuard, at: oxc_span::Span) -> Vec<SkeletonBindingId> {
crates/verter_session/src/flow_slice_content.rs:3184:        bindings: &[verter_semantic::analysis::flow::SkeletonBindingId],
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round1-reopen-scoping-ruling.md:3161:docs/arch/refactor/rev11/contracts/flow-completeness.md:81:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
docs/arch/refactor/rev11/program.md:229:On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.
docs/arch/refactor/rev11/program.md:253:Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.
docs/arch/refactor/rev11/architecture.md:715:`FunctionFlowGraph` is the one flow dependence authority. It contains stable binding/reference/effect/control/return/call/capture/escape/loop/completion facts and exact authored locators. Another CFG/control tree is prohibited.
docs/arch/refactor/rev11/architecture.md:731:Flow state is keyed by stable binding/slot identity, never by string names. Declaration authority exists for annotated and inferred bindings. Joins, substitution, and nominal `unique symbol` overlap/comparability use the shared semantic relation/inference implementation; flow owns no private relation classifier.
docs/arch/refactor/rev11/architecture.md:787:1. build the minimum graph/domain/obligation solver behind a private non-production test boundary, already using stable binding identities and the shared relation/inference authority for every capability that will remain enabled at cutover;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1218:`FunctionFlowGraph` is the one flow dependence authority. It contains stable binding/reference/effect/control/return/call/capture/escape/loop/completion facts and exact authored locators. Another CFG/control tree is prohibited.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1234:Flow state is keyed by stable binding/slot identity, never by string names. Declaration authority exists for annotated and inferred bindings. Joins, substitution, and nominal `unique symbol` overlap/comparability use the shared semantic relation/inference implementation; flow owns no private relation classifier.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:1290:1. build the minimum graph/domain/obligation solver behind a private non-production test boundary, already using stable binding identities and the shared relation/inference authority for every capability that will remain enabled at cutover;
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:3231:`D1` may build the minimum graph/domain/ledger/finalizer foundation behind a private hermetic test boundary on the bounded `D2` integration branch. It is unreachable from product entry points. Before the public cutover, it covers every effective-flow capability row declared Supported/Stable by the `A6` matrix, unless a separate reviewed breaking product decision changes that row. It already uses stable binding identities and the shared relation/inference authority; it may not contain a temporary name-keyed or flow-private semantic authority. Experimental/unsupported rows may remain typed gaps. `D1` is a review checkpoint, not an independently mergeable/releasable production block.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4303:On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:4327:Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.
docs/arch/refactor/rev11/consolidated/verter-architecture-lock-master-plan-v11.md:8775:- function programs/skeletons carry stable binding identities and structural inventories;

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_semantic/src/analysis/flow/mod.rs | sed -n '1,130p'; nl -ba crates/verter_semantic/src/analysis/flow/mod.rs | sed -n '430,635p'; nl -ba crates/verter_semantic/src/analysis/flow/flow_graph.rs | sed -n '1,270p'; nl -ba docs/arch/refactor/rev11/baseline/9af553dd.md | sed -n '105,132p'; nl -ba docs/arch/refactor/rev11/decisions/ADR-012-stable-identifiers-and-canonical-ordering.md"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 4128ms:[0m
     1	//! Demand-sliced flow substrate: the arena-free [`FunctionBodySkeleton`] and
     2	//! the per-function [`flow_graph::FunctionFlowGraph`] built from it.
     3	//!
     4	//! The skeleton is a SHALLOW structural index over one authored function
     5	//! body: a statement / control-region skeleton, a return-site index, a
     6	//! lexical binding index, and an assignment / kill summary, plus per-site
     7	//! read / write / call footprints and object-literal property footprints.
     8	//! It is built once per function content version from the retained parse
     9	//! snapshot and never rebuilt per query or demand; it borrows no OXC node
    10	//! (`Send + Sync + 'static`) and performs NO type lowering — every stored
    11	//! leaf is an interned name, ordinal, span, or id, certified transitively
    12	//! `TypeExpr`-free by the `NoTypeExpr` marker on every carrier.
    13	//!
    14	//! The [`flow_graph::FunctionFlowGraph`] is the sparse typed-edge dependence
    15	//! structure the flow demand planner computes reachability over; it is built
    16	//! from the skeleton ALONE ([`flow_graph::build_function_flow_graph`] takes
    17	//! only `&FunctionBodySkeleton`), so a graph build can never re-walk the AST
    18	//! or observe a query demand.
    19	//!
    20	//! On top of the graph: [`peeker::ReturnPathPeeker`] plans a demand slice
    21	//! as graph reachability (the two-frontier rule as edge classes) into a
    22	//! [`flow_ir::ReturnSlicePlan`]; [`hashing::compute_flow_slice_hash`]
    23	//! folds exactly that selected subgraph into the opaque
    24	//! [`hashing::FlowSliceHash`]; and [`lower::lower_slice_plan`] lowers only
    25	//! the plan into the arena-free [`flow_ir::FlowSliceIR`].
    26	
    27	use std::sync::Arc;
    28	
    29	use oxc_ast::ast::{
    30	    ArrowFunctionExpression, AssignmentTarget, AssignmentTargetMaybeDefault,
    31	    AssignmentTargetProperty, BindingPattern, Expression, Function, ObjectExpression,
    32	    SimpleAssignmentTarget, Statement,
    33	};
    34	use oxc_ast_visit::{walk, Visit};
    35	use oxc_span::GetSpan;
    36	use rustc_hash::FxHashMap;
    37	use verter_no_typeexpr::NoTypeExpr;
    38	
    39	pub use frame_span::FrameSpan;
    40	
    41	pub mod flow_graph;
    42	pub mod flow_ir;
    43	pub mod frame_span;
    44	pub mod hashing;
    45	pub mod lower;
    46	pub mod peeker;
    47	pub mod value_descent;
    48	
    49	pub use value_descent::{
    50	    expression_contains_call, object_entry_descent, object_entry_key, static_property_key_text,
    51	    value_composes_unmodeled_call, value_descent, value_is_unmodeled_call, ObjectEntryDescent,
    52	    ObjectEntryKey, ObjectEntryKind, ValueDescent,
    53	};
    54	
    55	#[cfg(test)]
    56	#[path = "skeleton_tests.rs"]
    57	mod skeleton_tests;
    58	
    59	// ---------------------------------------------------------------------------
    60	// Ids
    61	// ---------------------------------------------------------------------------
    62	
    63	/// Interned identifier / property-key name within one skeleton.
    64	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
    65	pub struct FlowNameId(u32);
    66	
    67	impl FlowNameId {
    68	    /// Index into [`FunctionBodySkeleton::names`].
    69	    #[must_use]
    70	    pub fn index(self) -> usize {
    71	        self.0 as usize
    72	    }
    73	}
    74	
    75	/// One entry of the lexical binding index.
    76	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
    77	pub struct SkeletonBindingId(u32);
    78	
    79	impl SkeletonBindingId {
    80	    /// Index into [`FunctionBodySkeleton::bindings`].
    81	    #[must_use]
    82	    pub fn index(self) -> usize {
    83	        self.0 as usize
    84	    }
    85	
    86	    pub(crate) fn from_index(index: u32) -> Self {
    87	        Self(index)
    88	    }
    89	}
    90	
    91	/// One control region of the statement skeleton.
    92	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
    93	pub struct SkeletonRegionId(u32);
    94	
    95	impl SkeletonRegionId {
    96	    /// Index into [`FunctionBodySkeleton::regions`].
    97	    #[must_use]
    98	    pub fn index(self) -> usize {
    99	        self.0 as usize
   100	    }
   101	
   102	    pub(crate) fn from_index(index: u32) -> Self {
   103	        Self(index)
   104	    }
   105	}
   106	
   107	/// One tracked expression site.
   108	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   109	pub struct SkeletonExprSiteId(u32);
   110	
   111	impl SkeletonExprSiteId {
   112	    /// Index into [`FunctionBodySkeleton::expr_sites`].
   113	    #[must_use]
   114	    pub fn index(self) -> usize {
   115	        self.0 as usize
   116	    }
   117	
   118	    pub(crate) fn from_index(index: u32) -> Self {
   119	        Self(index)
   120	    }
   121	}
   122	
   123	/// One `return` site of the indexed function.
   124	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   125	pub struct SkeletonReturnSiteId(u32);
   126	
   127	impl SkeletonReturnSiteId {
   128	    /// Index into [`FunctionBodySkeleton::return_sites`].
   129	    #[must_use]
   130	    pub fn index(self) -> usize {
   430	    },
   431	    /// A branch JOIN (a conditional expression): every arm site provides
   432	    /// the WHOLE value of this site, so a demand for this site's value —
   433	    /// or for a projection under it — is a demand for each arm's, at the
   434	    /// same remaining path.
   435	    BranchJoin {
   436	        /// The arm sites, in authored order (consequent, alternate).
   437	        arms: Arc<[SkeletonExprSiteId]>,
   438	    },
   439	    /// Any other expression shape (footprint-only).
   440	    Other,
   441	}
   442	
   443	/// One tracked expression site: span, region membership, containment
   444	/// parent, shape, and the read / call footprint attributed to this site
   445	/// (child sites carry their own).
   446	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   447	pub struct SkeletonExprSite {
   448	    /// The expression's span.
   449	    pub span: FrameSpan,
   450	    /// The region the site evaluates in.
   451	    pub region: SkeletonRegionId,
   452	    /// The containing site (`None` for a root site owned by a statement,
   453	    /// declarator, return, or control input).
   454	    pub parent: Option<SkeletonExprSiteId>,
   455	    /// The recorded expression shape.
   456	    pub shape: SkeletonExprShape,
   457	    /// Identifier reads attributed to this site.
   458	    pub reads: Arc<[SkeletonRead]>,
   459	    /// Call / construct footprints attributed to this site.
   460	    pub calls: Arc<[SkeletonCall]>,
   461	}
   462	
   463	// ---------------------------------------------------------------------------
   464	// Writes (assignment / kill summary)
   465	// ---------------------------------------------------------------------------
   466	
   467	/// The root target of one write.
   468	#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
   469	pub enum SkeletonWriteTarget {
   470	    /// A named root (a local slot when the name binds in this frame,
   471	    /// otherwise a free name).
   472	    Named(FlowNameId),
   473	    /// An unresolvable target root (call result, `this`, computed root).
   474	    Opaque,
   475	}
   476	
   477	/// One segment of a write's projection path.
   478	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   479	pub enum SkeletonPathSegment {
   480	    /// A statically-known property key.
   481	    Static(FlowNameId),
   482	    /// A computed / unknown key.
   483	    Computed,
   484	}
   485	
   486	/// Whether a write definitely happens when its site evaluates.
   487	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   488	pub enum SkeletonWriteCertainty {
   489	    /// The write happens whenever the site evaluates.
   490	    Definite,
   491	    /// The write is conditional on the site's own evaluation (logical
   492	    /// assignment, iteration-provided values).
   493	    Optional,
   494	}
   495	
   496	/// One write of the assignment / kill summary, in source order.
   497	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   498	pub struct SkeletonWrite {
   499	    /// The write's root target.
   500	    pub target: SkeletonWriteTarget,
   501	    /// The projection path under the root (empty = whole-slot write).
   502	    pub path: Arc<[SkeletonPathSegment]>,
   503	    /// Whether the write definitely happens when the site evaluates.
   504	    pub certainty: SkeletonWriteCertainty,
   505	    /// The site providing the written value (`None` for self-referential
   506	    /// update writes like `x++`).
   507	    pub value: Option<SkeletonExprSiteId>,
   508	    /// The tracked site whose evaluation performs the write.
   509	    pub site: SkeletonExprSiteId,
   510	    /// The region the write evaluates in.
   511	    pub region: SkeletonRegionId,
   512	    /// The write expression's span.
   513	    pub span: FrameSpan,
   514	}
   515	
   516	// ---------------------------------------------------------------------------
   517	// Return sites
   518	// ---------------------------------------------------------------------------
   519	
   520	/// One `return` site of the indexed function, in source order.
   521	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   522	pub struct SkeletonReturnSite {
   523	    /// Source-order ordinal.
   524	    pub ordinal: u32,
   525	    /// The region the return site evaluates in.
   526	    pub region: SkeletonRegionId,
   527	    /// The returned expression's site (`None` for bare `return;`).
   528	    pub argument: Option<SkeletonExprSiteId>,
   529	    /// Whether the site is the implicit return of an expression-bodied
   530	    /// arrow.
   531	    pub implicit: bool,
   532	    /// The return statement's span.
   533	    pub span: FrameSpan,
   534	}
   535	
   536	// ---------------------------------------------------------------------------
   537	// The skeleton
   538	// ---------------------------------------------------------------------------
   539	
   540	/// The arena-free shallow skeleton of one authored function body: the
   541	/// statement / control-region skeleton, the return-site index, the lexical
   542	/// binding index, the assignment / kill summary, and per-site read / write /
   543	/// call / object-shape footprints.
   544	///
   545	/// Built once per function content version from the retained parse
   546	/// snapshot; never rebuilt per query or demand. Stores NO lowered type —
   547	/// every leaf is an interned name, ordinal, span, or id.
   548	///
   549	/// **Every span here is a [`FrameSpan`]**, relative to the function's own
   550	/// start ([`FunctionBodySource::anchor`]) — an absolute file offset cannot
   551	/// be stored here because it does not have the type. The skeleton is
   552	/// content-addressed and reused across every file content its key admits,
   553	/// and an absolute offset is not a property of that content: a blank line
   554	/// above the function moves all of them while changing nothing the key can
   555	/// see. Consumers rebase a live position through [`FrameSpan::rebase`]
   556	/// before comparing.
   557	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   558	pub struct FunctionBodySkeleton {
   559	    /// The interned name table.
   560	    pub names: Arc<[Arc<str>]>,
   561	    /// The control regions; index 0 is the function-body root.
   562	    pub regions: Arc<[SkeletonRegion]>,
   563	    /// The lexical binding index.
   564	    pub bindings: Arc<[SkeletonBinding]>,
   565	    /// The tracked expression sites (parents precede their children).
   566	    pub expr_sites: Arc<[SkeletonExprSite]>,
   567	    /// The return-site index, in source order.
   568	    pub return_sites: Arc<[SkeletonReturnSite]>,
   569	    /// The assignment / kill summary, in source order.
   570	    pub writes: Arc<[SkeletonWrite]>,
   571	}
   572	
   573	impl FunctionBodySkeleton {
   574	    /// The interned text of `name`.
   575	    #[must_use]
   576	    pub fn name(&self, name: FlowNameId) -> &str {
   577	        &self.names[name.index()]
   578	    }
   579	
   580	    /// The id of an interned name, when present.
   581	    #[must_use]
   582	    pub fn name_id(&self, text: &str) -> Option<FlowNameId> {
   583	        self.names
   584	            .iter()
   585	            .position(|candidate| candidate.as_ref() == text)
   586	            .and_then(|index| u32::try_from(index).ok())
   587	            .map(FlowNameId)
   588	    }
   589	
   590	    /// Every binding of `name` in this frame, in declaration order.
   591	    pub fn bindings_named(&self, name: FlowNameId) -> impl Iterator<Item = SkeletonBindingId> + '_ {
   592	        self.bindings
   593	            .iter()
   594	            .enumerate()
   595	            .filter(move |(_, binding)| binding.name == name)
   596	            .filter_map(|(index, _)| u32::try_from(index).ok().map(SkeletonBindingId))
   597	    }
   598	
   599	    /// The region record for `id`.
   600	    #[must_use]
   601	    pub fn region(&self, id: SkeletonRegionId) -> &SkeletonRegion {
   602	        &self.regions[id.index()]
   603	    }
   604	
   605	    /// The binding record for `id`.
   606	    #[must_use]
   607	    pub fn binding(&self, id: SkeletonBindingId) -> &SkeletonBinding {
   608	        &self.bindings[id.index()]
   609	    }
   610	
   611	    /// The expression-site record for `id`.
   612	    #[must_use]
   613	    pub fn expr_site(&self, id: SkeletonExprSiteId) -> &SkeletonExprSite {
   614	        &self.expr_sites[id.index()]
   615	    }
   616	
   617	    /// The return-site record for `id`.
   618	    #[must_use]
   619	    pub fn return_site(&self, id: SkeletonReturnSiteId) -> &SkeletonReturnSite {
   620	        &self.return_sites[id.index()]
   621	    }
   622	
   623	    /// The innermost control region whose span CONTAINS `span` — the
   624	    /// region an authored position evaluates in. Regions are
   625	    /// statement-scoped and properly nested, so the smallest containing
   626	    /// region is unique; a position outside every nested region (the
   627	    /// body's own top level) resolves to the function-body root.
   628	    #[must_use]
   629	    pub fn innermost_region_containing(&self, span: FrameSpan) -> SkeletonRegionId {
   630	        let mut best = SkeletonRegionId(0);
   631	        let mut best_width = u32::MAX;
   632	        for (index, region) in self.regions.iter().enumerate() {
   633	            if !region.span.contains(span) {
   634	                continue;
   635	            }
     1	//! The per-function [`FunctionFlowGraph`]: a sparse, arena-free typed-edge
     2	//! dependence structure built ONCE per function content version from its
     3	//! [`FunctionBodySkeleton`](super::FunctionBodySkeleton) — and from the
     4	//! skeleton ALONE. [`build_function_flow_graph`] takes only
     5	//! `&FunctionBodySkeleton`, so a graph build can never re-walk the AST,
     6	//! observe a query demand, lower a type, or produce a fact.
     7	//!
     8	//! Nodes are the function's bindings (value-definition hubs), expression
     9	//! sites, return sites, and control regions. Edges are TYPED — one class
    10	//! per dependence kind — and split into two families with different
    11	//! reachability stop conditions:
    12	//!
    13	//! - **Value-provider edges** ([`FlowEdgeKind::ValueDef`] +
    14	//!   [`FlowEdgeKind::PathWrite`]) compute which sources provide a demanded
    15	//!   value; a planner MAY stop following them at a definite-present write
    16	//!   for the demanded path head.
    17	//! - **Effect edges** ([`FlowEdgeKind::EvalEffect`] +
    18	//!   [`FlowEdgeKind::ControlRegion`]) stay live past a definite-present
    19	//!   write: a value-dead sibling (an overwritten duplicate key, a spread
    20	//!   source, a computed key) keeps its evaluation-effect edges, because
    21	//!   evaluation effects survive a definite write even though value
    22	//!   materialization does not.
    23	//!
    24	//! The edge vocabulary is open by construction: further dependence classes
    25	//! (narrowing predicates, closure escapes, loop summaries, `try`/`finally`
    26	//! overrides) extend [`FlowEdgeKind`] on this SAME graph — a second flow
    27	//! structure is forbidden.
    28	
    29	use std::sync::Arc;
    30	
    31	use rustc_hash::FxHashSet;
    32	use verter_no_typeexpr::NoTypeExpr;
    33	
    34	use super::{
    35	    FunctionBodySkeleton, SkeletonBindingId, SkeletonCallee, SkeletonExprShape, SkeletonExprSiteId,
    36	    SkeletonObjectEntry, SkeletonObjectKey, SkeletonPathSegment, SkeletonRegionId,
    37	    SkeletonReturnSiteId, SkeletonWriteCertainty, SkeletonWriteTarget,
    38	};
    39	
    40	#[cfg(test)]
    41	#[path = "flow_graph_tests.rs"]
    42	mod flow_graph_tests;
    43	
    44	/// The executable-region kind a flow graph covers. The graph is ONE region
    45	/// kind today; other executable region kinds (module top-level, static
    46	/// blocks, field / parameter initializers, decorator expressions) enter
    47	/// through this discriminant without reshaping the demand planner.
    48	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
    49	pub enum ExecutableRegionKind {
    50	    /// An authored function body.
    51	    Function,
    52	}
    53	
    54	/// One node of the flow graph.
    55	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
    56	pub struct FlowNodeId(u32);
    57	
    58	impl FlowNodeId {
    59	    /// The dense node index.
    60	    #[must_use]
    61	    pub fn index(self) -> usize {
    62	        self.0 as usize
    63	    }
    64	
    65	    #[cfg(test)]
    66	    pub(crate) fn from_index(index: u32) -> Self {
    67	        Self(index)
    68	    }
    69	}
    70	
    71	/// What one flow-graph node stands for.
    72	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    73	pub enum FlowNodeKind {
    74	    /// A lexical binding — the value-definition hub of one slot: its
    75	    /// out-edges enumerate the slot's definitions in source order.
    76	    Binding(SkeletonBindingId),
    77	    /// A tracked expression site.
    78	    ExprSite(SkeletonExprSiteId),
    79	    /// A `return` site.
    80	    ReturnSite(SkeletonReturnSiteId),
    81	    /// A control region.
    82	    Region(SkeletonRegionId),
    83	}
    84	
    85	/// The typed dependence class of one edge.
    86	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
    87	pub enum FlowEdgeKind {
    88	    /// The source node's value is provided by the target: a return site's
    89	    /// argument, a binding's initializer / whole-slot definite write
    90	    /// (reaching definition), or an expression site's read of a binding.
    91	    ValueDef,
    92	    /// A write targets a projection path on the source node's value: an
    93	    /// object-literal entry provisioning a key, a member write on a slot,
    94	    /// an optional / unknown write (spread, computed key, logical
    95	    /// assignment).
    96	    PathWrite {
    97	        /// The written projection path (empty = whole-slot, non-definite).
    98	        path: Arc<[SkeletonPathSegment]>,
    99	        /// Whether the write definitely happens when its site evaluates.
   100	        certainty: SkeletonWriteCertainty,
   101	    },
   102	    /// Evaluating the source affects the target: a site's contained write
   103	    /// / call into a binding, or a container's evaluation of an effectful
   104	    /// child site. Stays live past definite-present value writes.
   105	    EvalEffect,
   106	    /// The source node belongs to (or nests inside) the target region.
   107	    ControlRegion,
   108	}
   109	
   110	/// The edge-class family discriminant (the reachability stop-condition
   111	/// families).
   112	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   113	pub enum FlowEdgeClass {
   114	    /// Value-provider: may stop at a definite-present write.
   115	    ValueDef,
   116	    /// Value-provider: path-targeted writes.
   117	    PathWrite,
   118	    /// Effect: stays live past value writes.
   119	    EvalEffect,
   120	    /// Effect: control-region membership / nesting.
   121	    ControlRegion,
   122	}
   123	
   124	impl FlowEdgeKind {
   125	    /// The edge's class discriminant.
   126	    #[must_use]
   127	    pub fn class(&self) -> FlowEdgeClass {
   128	        match self {
   129	            FlowEdgeKind::ValueDef => FlowEdgeClass::ValueDef,
   130	            FlowEdgeKind::PathWrite { .. } => FlowEdgeClass::PathWrite,
   131	            FlowEdgeKind::EvalEffect => FlowEdgeClass::EvalEffect,
   132	            FlowEdgeKind::ControlRegion => FlowEdgeClass::ControlRegion,
   133	        }
   134	    }
   135	}
   136	
   137	/// One typed edge.
   138	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   139	pub struct FlowEdge {
   140	    /// The depending node.
   141	    pub from: FlowNodeId,
   142	    /// The provider / affected node.
   143	    pub to: FlowNodeId,
   144	    /// The typed dependence class.
   145	    pub kind: FlowEdgeKind,
   146	    /// Source-order ordinal among `from`'s out-edges of the same class.
   147	    pub ordinal: u32,
   148	}
   149	
   150	/// The sparse per-function dependence graph. Arena-free
   151	/// (`Send + Sync + 'static`), compact interned ids throughout, no stored
   152	/// lowered type (`NoTypeExpr`-certified); every type along an edge resolves
   153	/// on demand only when a demand slice traverses it.
   154	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   155	pub struct FunctionFlowGraph {
   156	    /// The executable-region kind this graph covers.
   157	    pub region_kind: ExecutableRegionKind,
   158	    binding_count: u32,
   159	    expr_site_count: u32,
   160	    return_site_count: u32,
   161	    region_count: u32,
   162	    /// Every edge, grouped by `from` node (CSR layout).
   163	    edges: Arc<[FlowEdge]>,
   164	    /// CSR offsets: node `n`'s out-edges are `edges[offsets[n]..offsets[n+1]]`.
   165	    offsets: Arc<[u32]>,
   166	}
   167	
   168	impl FunctionFlowGraph {
   169	    /// Total node count.
   170	    #[must_use]
   171	    pub fn node_count(&self) -> usize {
   172	        (self.binding_count + self.expr_site_count + self.return_site_count + self.region_count)
   173	            as usize
   174	    }
   175	
   176	    /// Every edge, grouped by `from` node.
   177	    #[must_use]
   178	    pub fn edges(&self) -> &[FlowEdge] {
   179	        &self.edges
   180	    }
   181	
   182	    /// The node of one binding.
   183	    #[must_use]
   184	    pub fn binding_node(&self, id: SkeletonBindingId) -> FlowNodeId {
   185	        debug_assert!((id.index() as u32) < self.binding_count);
   186	        FlowNodeId(id.index() as u32)
   187	    }
   188	
   189	    /// The node of one expression site.
   190	    #[must_use]
   191	    pub fn expr_site_node(&self, id: SkeletonExprSiteId) -> FlowNodeId {
   192	        debug_assert!((id.index() as u32) < self.expr_site_count);
   193	        FlowNodeId(self.binding_count + id.index() as u32)
   194	    }
   195	
   196	    /// The node of one return site.
   197	    #[must_use]
   198	    pub fn return_site_node(&self, id: SkeletonReturnSiteId) -> FlowNodeId {
   199	        debug_assert!((id.index() as u32) < self.return_site_count);
   200	        FlowNodeId(self.binding_count + self.expr_site_count + id.index() as u32)
   201	    }
   202	
   203	    /// The node of one control region.
   204	    #[must_use]
   205	    pub fn region_node(&self, id: SkeletonRegionId) -> FlowNodeId {
   206	        debug_assert!((id.index() as u32) < self.region_count);
   207	        FlowNodeId(
   208	            self.binding_count + self.expr_site_count + self.return_site_count + id.index() as u32,
   209	        )
   210	    }
   211	
   212	    /// What `node` stands for.
   213	    #[must_use]
   214	    pub fn node_kind(&self, node: FlowNodeId) -> FlowNodeKind {
   215	        let index = node.0;
   216	        if index < self.binding_count {
   217	            return FlowNodeKind::Binding(SkeletonBindingId::from_index(index));
   218	        }
   219	        let index = index - self.binding_count;
   220	        if index < self.expr_site_count {
   221	            return FlowNodeKind::ExprSite(SkeletonExprSiteId::from_index(index));
   222	        }
   223	        let index = index - self.expr_site_count;
   224	        if index < self.return_site_count {
   225	            return FlowNodeKind::ReturnSite(SkeletonReturnSiteId::from_index(index));
   226	        }
   227	        FlowNodeKind::Region(SkeletonRegionId::from_index(index - self.return_site_count))
   228	    }
   229	
   230	    /// The out-edges of `node`, source-ordered within each class.
   231	    #[must_use]
   232	    pub fn out_edges(&self, node: FlowNodeId) -> &[FlowEdge] {
   233	        let start = self.offsets[node.index()] as usize;
   234	        let end = self.offsets[node.index() + 1] as usize;
   235	        &self.edges[start..end]
   236	    }
   237	}
   238	
   239	/// Build the [`FunctionFlowGraph`] of one skeleton. Pure and deterministic
   240	/// over the skeleton alone: no AST, no demand, no type lowering, no
   241	/// resolution dispatch, no route lookup, no fact production.
   242	#[must_use]
   243	pub fn build_function_flow_graph(skeleton: &FunctionBodySkeleton) -> FunctionFlowGraph {
   244	    let binding_count = u32::try_from(skeleton.bindings.len()).unwrap_or(u32::MAX);
   245	    let expr_site_count = u32::try_from(skeleton.expr_sites.len()).unwrap_or(u32::MAX);
   246	    let return_site_count = u32::try_from(skeleton.return_sites.len()).unwrap_or(u32::MAX);
   247	    let region_count = u32::try_from(skeleton.regions.len()).unwrap_or(u32::MAX);
   248	
   249	    let binding_node = |id: SkeletonBindingId| FlowNodeId(id.index() as u32);
   250	    let site_node = |id: SkeletonExprSiteId| FlowNodeId(binding_count + id.index() as u32);
   251	    let return_node =
   252	        |id: SkeletonReturnSiteId| FlowNodeId(binding_count + expr_site_count + id.index() as u32);
   253	    let region_node = |id: SkeletonRegionId| {
   254	        FlowNodeId(binding_count + expr_site_count + return_site_count + id.index() as u32)
   255	    };
   256	
   257	    // Effectful closure per site: own write / call footprint, or an
   258	    // effectful child (children always follow their parents in the table).
   259	    let mut effectful = vec![false; skeleton.expr_sites.len()];
   260	    for write in skeleton.writes.iter() {
   261	        effectful[write.site.index()] = true;
   262	    }
   263	    for (index, site) in skeleton.expr_sites.iter().enumerate() {
   264	        if !site.calls.is_empty() {
   265	            effectful[index] = true;
   266	        }
   267	    }
   268	    for index in (0..skeleton.expr_sites.len()).rev() {
   269	        if effectful[index] {
   270	            if let Some(parent) = skeleton.expr_sites[index].parent {
   105	- a hash node runs the demand planner and hashes the selected subgraph;
   106	- a lowered node re-runs the planner and describes the replan as cheap and deterministic;
   107	- over-budget plans are return-only/non-admitted.
   108	
   109	The non-admission policy is strong. The replan remains a performance hypothesis and must be measured against carrying the first plan.
   110	
   111	## 3.5 Binding identities are already partially available
   112	
   113	Source:
   114	
   115	```text
   116	crates/verter_semantic/src/analysis/function_program.rs
   117	crates/verter_semantic/src/analysis/flow/flow_ir.rs
   118	```
   119	
   120	Observed shape:
   121	
   122	- function programs/skeletons carry stable binding identities and structural inventories;
   123	- capture resolution can use defining frame plus binding slot;
   124	- some flow carriers still include name data and optional slot resolution.
   125	
   126	This makes a binding-identity semantic cutover feasible without inventing a new source-wide identity authority.
   127	
   128	# 4. Cache/query source observations
   129	
   130	## 4.1 Same-key singleflight is already serious infrastructure
   131	
   132	Source:
     1	# ADR-012 — Stable Entity IDs, Session Handles, and Canonical Ordering Are Distinct
     2	
     3	**Status:** Accepted  
     4	**Decision owner:** public identity and deterministic serialization  
     5	**Reopen only if:** a public contract explicitly changes which identifiers are stable across regimes/sessions.
     6	
     7	## Context
     8	
     9	Deterministic cross-regime IDs and cohort-local continuation handles serve different purposes. Treating a raw semantic node or session handle as a stable public ID creates lifetime and equality errors. Parallel insertion order can leak into output unless ordering authority is explicit.
    10	
    11	## Decision
    12	
    13	- `StableEntityId` is deterministic from a documented canonical/content-relative basis and may be compared across declared portable regimes;
    14	- `SessionHandle` is opaque, owner/cohort-bound, generation-validated, and not compared across sessions;
    15	- graph export, when requested, uses deterministic graph-local canonical IDs under its serialization profile;
    16	- every observable collection has a total canonical order and deterministic tie-breaker;
    17	- allocation address, concurrent interner insertion, hash iteration, worker completion, cache history, and owner-shard assignment cannot affect observable ordering;
    18	- canonical serialization records its profile/domain and uses deterministic string/table/reference ordering.
    19	
    20	## Consequences
    21	
    22	- storage cohorts can reclaim without breaking public stable identity promises;
    23	- direct/prepared/managed/native/WASM equality is well-defined;
    24	- parallelism cannot leak nondeterminism.
    25	
    26	## Rejected alternatives
    27	
    28	- **Expose raw node IDs:** lifetime-bound and not stable.
    29	- **Sort only at some adapters:** permits internal nondeterminism to affect hashes, maps, and caches.

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"struct .*BindingId|enum .*BindingId|binding.*slot|BindingSlot|Stable.*Binding|binding identity|binding_identity|binding_id\" crates/verter_semantic/src/analysis/function_program.rs crates/verter_semantic/src/analysis/flow crates/verter_type_expr/src/facts.rs | sed -n '1,320p'; nl -ba crates/verter_semantic/src/analysis/flow/flow_ir.rs | sed -n '120,175p'; rg -n \"struct SkeletonBinding|enum SkeletonBindingKind\" crates/verter_semantic/src/analysis/flow/mod.rs && nl -ba crates/verter_semantic/src/analysis/flow/mod.rs | sed -n '170,270p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 12250ms:[0m
crates/verter_type_expr/src/facts.rs:3105:/// excluded; exact lexical owner and owner-qualified binding identity are the
crates/verter_semantic/src/analysis/function_program.rs:266:/// the binding plus the binding's stable source-order slot in that frame's
crates/verter_semantic/src/analysis/function_program.rs:268:/// `(defining_function, binding_slot)` pair is the identity — it separates
crates/verter_semantic/src/analysis/function_program.rs:275:pub struct FlowBindingIdentity {
crates/verter_semantic/src/analysis/function_program.rs:282:    /// The binding's source-order slot in that frame's binding inventory.
crates/verter_semantic/src/analysis/function_program.rs:283:    pub binding_slot: u32,
crates/verter_semantic/src/analysis/function_program.rs:292:pub struct CanonicalCaptureIdentity(pub Arc<[FlowBindingIdentity]>);
crates/verter_semantic/src/analysis/function_program.rs:804:/// order; identity is the `(defining frame, binding slot)` pair, so two
crates/verter_semantic/src/analysis/function_program.rs:856:                kind: frame_bindings[frame][slot as usize].kind,
crates/verter_semantic/src/analysis/function_program.rs:858:                binding_slot: slot,
crates/verter_semantic/src/analysis/function_program.rs:869:/// `(frame position, binding slot)` pair.
crates/verter_semantic/src/analysis/flow/lower.rs:5://! skeleton records: selected bindings become slots, selected expression
crates/verter_semantic/src/analysis/flow/skeleton_tests.rs:59:    let binding_id = skeleton
crates/verter_semantic/src/analysis/flow/skeleton_tests.rs:63:    skeleton.binding(binding_id)
crates/verter_semantic/src/analysis/flow/flow_graph_tests.rs:316:/// Lexical binding identity (defect D): a read resolves to the binding of
crates/verter_semantic/src/analysis/flow/flow_ir.rs:147:    /// The skeleton binding this slot lowers.
crates/verter_semantic/src/analysis/flow/flow_ir.rs:149:    /// The binding identifier's span — the DECLARATION-precise slot
crates/verter_semantic/src/analysis/flow/flow_graph.rs:74:    /// A lexical binding — the value-definition hub of one slot: its
crates/verter_semantic/src/analysis/flow/flow_graph.rs:89:    /// argument, a binding's initializer / whole-slot definite write
crates/verter_semantic/src/analysis/flow/mod.rs:77:pub struct SkeletonBindingId(u32);
   120	/// A lowered projection path.
   121	pub type FlowPath = Arc<[FlowPathSegment]>;
   122	
   123	// ---------------------------------------------------------------------------
   124	// Slots
   125	// ---------------------------------------------------------------------------
   126	
   127	/// One definition contributing to a slot, in source order.
   128	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   129	pub struct FlowDef {
   130	    /// The providing expression.
   131	    pub value: FlowExprId,
   132	    /// The written projection path under the slot (empty = whole-slot).
   133	    pub path: FlowPath,
   134	    /// Whether the write definitely happens when its site evaluates.
   135	    pub certainty: SkeletonWriteCertainty,
   136	}
   137	
   138	/// One lowered slot: a selected binding with the selected subset of its
   139	/// definitions. An effect-only slot (mutated but never read by the
   140	/// demanded path) carries no selected definitions.
   141	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   142	pub struct FlowSlot {
   143	    /// The binding name.
   144	    pub name: Arc<str>,
   145	    /// The binding kind.
   146	    pub kind: SkeletonBindingKind,
   147	    /// The skeleton binding this slot lowers.
   148	    pub binding: SkeletonBindingId,
   149	    /// The binding identifier's span — the DECLARATION-precise slot
   150	    /// identity the content lowering gates on (name identity would
   151	    /// re-conflate shadowing same-named bindings the plan kept
   152	    /// distinct). Never folded into the slice hash (the hash covers the
   153	    /// plan's selected subgraph and is span-free).
   154	    pub span: FrameSpan,
   155	    /// Whether the slot's value contributes to the demand (`false` =
   156	    /// effect-only: mutated, never value-read by the selected path).
   157	    pub value_selected: bool,
   158	    /// Selected definitions in source order.
   159	    pub defs: Arc<[FlowDef]>,
   160	}
   161	
   162	// ---------------------------------------------------------------------------
   163	// Expressions
   164	// ---------------------------------------------------------------------------
   165	
   166	/// Why an expression record is in the slice.
   167	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   168	pub enum FlowExprRole {
   169	    /// The expression's value contributes to the demanded projection.
   170	    Value,
   171	    /// Only the expression's evaluation effects contribute — its value
   172	    /// is never materialized.
   173	    EffectOnly,
   174	}
   175	
77:pub struct SkeletonBindingId(u32);
195:pub enum SkeletonBindingKind {
313:pub struct SkeletonBinding {
   170	/// One control region: kind, parent nesting, the controlling expression
   171	/// site (an `if` / loop condition or `switch` discriminant), whether the
   172	/// region's statement subtree returns from the indexed function, and the
   173	/// region's span.
   174	#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
   175	pub struct SkeletonRegion {
   176	    /// The region kind.
   177	    pub kind: SkeletonRegionKind,
   178	    /// The enclosing region (`None` for the function-body root).
   179	    pub parent: Option<SkeletonRegionId>,
   180	    /// The controlling expression site, when the region is predicated.
   181	    pub control_input: Option<SkeletonExprSiteId>,
   182	    /// Whether the region's subtree contains a `return` of the indexed
   183	    /// function (nested function bodies never contribute).
   184	    pub has_return: bool,
   185	    /// The region statement's span.
   186	    pub span: FrameSpan,
   187	}
   188	
   189	// ---------------------------------------------------------------------------
   190	// Bindings
   191	// ---------------------------------------------------------------------------
   192	
   193	/// The kind of one lexical binding.
   194	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   195	pub enum SkeletonBindingKind {
   196	    /// A formal parameter (destructured identifiers included).
   197	    Param,
   198	    /// A `const` / `using` / `await using` declarator (block-scoped).
   199	    Const,
   200	    /// A `let` declarator.
   201	    Let,
   202	    /// A `var` declarator — the only function-scoped declarator kind.
   203	    Var,
   204	    /// A nested function declaration's name.
   205	    NestedFunction,
   206	    /// A local class declaration's name.
   207	    Class,
   208	    /// A `catch` clause parameter.
   209	    CatchParam,
   210	    /// A local `enum` declaration's name.
   211	    Enum,
   212	    /// A local `namespace` / `module` declaration's name.
   213	    Namespace,
   214	    /// A local `import x = …` declaration's name.
   215	    ImportEquals,
   216	    /// A local `type X = …` alias declaration's name. TYPE space only.
   217	    TypeAlias,
   218	    /// A local `interface X { … }` declaration's name. TYPE space only.
   219	    Interface,
   220	}
   221	
   222	/// The name MEANING one lookup demands, mirroring the TypeScript
   223	/// `SymbolFlags` split that decides which declarations can answer it.
   224	///
   225	/// A BARE reference (`x as N`) demands `Type`; the HEAD of a QUALIFIED
   226	/// reference (`x as N.B`) demands `Namespace`
   227	/// (`SymbolFlags.Namespace = ValueModule | NamespaceModule | Enum` — a
   228	/// class is NOT in it). The two are genuinely different questions about
   229	/// the same name: a local `class N` shadows the bare reference but not
   230	/// the qualified head, and a local `namespace N` does the reverse.
   231	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
   232	pub enum NameMeaning {
   233	    /// A type reference's own name (`N`).
   234	    Type,
   235	    /// A qualified type reference's HEAD (`N` in `N.B`).
   236	    Namespace,
   237	}
   238	
   239	impl SkeletonBindingKind {
   240	    /// Whether this kind declares a VALUE.
   241	    ///
   242	    /// `type` / `interface` declare a type and NOTHING else, so a value
   243	    /// lookup must walk straight past them to the enclosing scope —
   244	    /// exactly as [`FunctionBodySkeleton::declares_meaning_in_scope`]
   245	    /// walks past a value-only kind in type space. The two spaces are
   246	    /// symmetric: whichever space a lookup asks about, a declaration
   247	    /// that does not occupy it is transparent.
   248	    #[must_use]
   249	    pub const fn declares_value(self) -> bool {
   250	        !matches!(self, Self::TypeAlias | Self::Interface)
   251	    }
   252	
   253	    /// Whether this kind declares `meaning`.
   254	    ///
   255	    /// Oracle-anchored against `tsc --strict`, one fixture per cell:
   256	    ///
   257	    /// | kind                    | `Type` | `Namespace` |
   258	    /// |-------------------------|--------|-------------|
   259	    /// | `class` (static or not) | yes    | no          |
   260	    /// | `enum` / `const enum`   | yes    | yes         |
   261	    /// | `namespace`             | no     | yes         |
   262	    /// | `type` / `interface`    | yes    | no          |
   263	    /// | `import x = …`          | yes\*  | yes\*       |
   264	    /// | value-only kinds        | no     | no          |
   265	    ///
   266	    /// \* An `import x = …` is MEANING-TRANSPARENT: it occupies exactly
   267	    /// the spaces its target occupies, and the target is not decidable
   268	    /// from the skeleton. It therefore answers both meanings — a
   269	    /// deliberate over-fire that can only FAIL CLOSED, never publish a
   270	    /// wrong answer. (An `import =` inside a function body is TS1232

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_semantic/src/analysis/function_program.rs | sed -n '245,310p'; nl -ba crates/verter_semantic/src/analysis/function_program.rs | sed -n '780,885p'; nl -ba crates/verter_semantic/src/analysis/type_eval.rs | sed -n '1040,1090p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 13359ms:[0m
   245	    FunctionReturn(FunctionReturnSource),
   246	    /// A call-bearing compound outside the indexed expression domain.
   247	    UnsupportedCall,
   248	}
   249	
   250	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   251	pub enum ProgramExpressionCallKind {
   252	    Call,
   253	    Construct,
   254	}
   255	
   256	/// One declaration/callback expression indexed by content-free program point.
   257	#[derive(Debug, Clone, PartialEq, Eq)]
   258	pub struct ProgramExpressionRecord {
   259	    pub point: ProgramExpressionIdentity,
   260	    pub span: verter_span::Span,
   261	    pub locator: FunctionBodyLocator,
   262	    pub source: ProgramExpressionSource,
   263	}
   264	
   265	/// One captured binding's content-free identity: the frame that DECLARES
   266	/// the binding plus the binding's stable source-order slot in that frame's
   267	/// full binding inventory, alongside the binding's name and kind. The
   268	/// `(defining_function, binding_slot)` pair is the identity — it separates
   269	/// two same-name binders in different frames AND two same-name binders in
   270	/// different lexical scopes of one frame, neither of which a name (or a
   271	/// per-capture-list ordinal) can distinguish. NEVER a node id, a type, a
   272	/// content hash, or a span — capture types rehydrate from indexed binding /
   273	/// reaching-definition facts under the final type substitution.
   274	#[derive(Debug, Clone, PartialEq, Eq, Hash)]
   275	pub struct FlowBindingIdentity {
   276	    /// The binding name.
   277	    pub name: Arc<str>,
   278	    /// The binding kind in the DEFINING frame.
   279	    pub kind: FunctionBindingKind,
   280	    /// The frame whose binding inventory declares this binding.
   281	    pub defining_function: FunctionProgramKey,
   282	    /// The binding's source-order slot in that frame's binding inventory.
   283	    pub binding_slot: u32,
   284	}
   285	
   286	/// The content-free capture environment of a nested function position:
   287	/// capture binding identities (and their deterministic source order)
   288	/// only. Until non-empty narrowing lands, a capture whose type cannot be
   289	/// reconstructed from the indexed binding / reaching-definition facts is
   290	/// a typed ReturnOnly, never guessed or separately keyed.
   291	#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
   292	pub struct CanonicalCaptureIdentity(pub Arc<[FlowBindingIdentity]>);
   293	
   294	/// The callee shape of one evaluation-effect call site.
   295	#[derive(Debug, Clone, PartialEq, Eq, Hash)]
   296	pub enum FunctionEffectCallee {
   297	    /// A bare identifier callee (`g()`).
   298	    Identifier(Arc<str>),
   299	    /// A static member path (`a.b.c()`).
   300	    StaticMember(Arc<[Arc<str>]>),
   301	    /// Any other callee shape (computed, call-result, `this`-rooted).
   302	    Other,
   303	}
   304	
   305	/// One evaluation-effect call site in the current function body.
   306	#[derive(Debug, Clone, PartialEq, Eq, Hash)]
   307	pub struct FunctionEffectRecord {
   308	    /// The call expression's span.
   309	    pub span: verter_span::Span,
   310	    /// The callee shape.
   780	                    target,
   781	                });
   782	            }
   783	        }
   784	        entry.direct_calls = Arc::from(direct.into_boxed_slice());
   785	    }
   786	}
   787	
   788	/// Whether `scope` lexically contains `site`.
   789	fn scope_contains(scope: verter_span::Span, site: verter_span::Span) -> bool {
   790	    scope.start <= site.start && site.end <= scope.end
   791	}
   792	
   793	/// Whether `inner` is strictly narrower than (or equally narrow as, but
   794	/// later than) `outer` — the innermost-wins tiebreak of lexical lookup.
   795	fn scope_is_at_least_as_inner(inner: verter_span::Span, outer: verter_span::Span) -> bool {
   796	    inner.start >= outer.start && inner.end <= outer.end
   797	}
   798	
   799	/// Compute every nested position's content-free capture identities: the
   800	/// referenced names that bind in an enclosing frame, resolved LEXICALLY —
   801	/// innermost enclosing frame first, and within a frame the innermost
   802	/// same-name binding whose scope contains the capturing position. Each
   803	/// distinct captured BINDING is recorded once, in first-reference source
   804	/// order; identity is the `(defining frame, binding slot)` pair, so two
   805	/// same-name binders never collapse. A name binding in NO enclosing frame
   806	/// is not a capture (a free/global reference).
   807	fn resolve_captures(entries: &mut [FunctionProgramEntry]) {
   808	    // Snapshot the frame bindings + parents up front (no borrow conflicts).
   809	    // The binding inventories are shared, not copied, and one key -> position
   810	    // index resolves the whole parent chain by lookup. Duplicate keys keep
   811	    // the FIRST position, matching source order.
   812	    let frame_bindings: Vec<Arc<[FunctionBindingRecord]>> = entries
   813	        .iter()
   814	        .map(|entry| Arc::clone(&entry.bindings))
   815	        .collect();
   816	    let frame_keys: Vec<FunctionProgramKey> =
   817	        entries.iter().map(|entry| entry.key.clone()).collect();
   818	    let parents: Vec<Option<FunctionProgramKey>> = entries
   819	        .iter()
   820	        .map(|entry| entry.lexical_parent.as_deref().cloned())
   821	        .collect();
   822	    let mut position_of: rustc_hash::FxHashMap<FunctionProgramKey, usize> =
   823	        rustc_hash::FxHashMap::with_capacity_and_hasher(entries.len(), Default::default());
   824	    for (position, entry) in entries.iter().enumerate() {
   825	        position_of.entry(entry.key.clone()).or_insert(position);
   826	    }
   827	    for index in 0..entries.len() {
   828	        let Some(parent) = parents[index].clone() else {
   829	            continue;
   830	        };
   831	        // The enclosing frame chain, innermost first.
   832	        let mut chain: Vec<usize> = Vec::new();
   833	        let mut current = Some(parent);
   834	        while let Some(key) = current {
   835	            let Some(position) = position_of.get(&key).copied() else {
   836	                break;
   837	            };
   838	            chain.push(position);
   839	            current = parents[position].clone();
   840	        }
   841	        let site = entries[index].span;
   842	        let mut captures: Vec<FlowBindingIdentity> = Vec::new();
   843	        let mut seen: Vec<(usize, u32)> = Vec::new();
   844	        for reference in entries[index].references.iter() {
   845	            let Some((frame, slot)) =
   846	                resolve_lexical_binding(&chain, &frame_bindings, &reference.name, site)
   847	            else {
   848	                continue;
   849	            };
   850	            if seen.contains(&(frame, slot)) {
   851	                continue;
   852	            }
   853	            seen.push((frame, slot));
   854	            captures.push(FlowBindingIdentity {
   855	                name: Arc::clone(&reference.name),
   856	                kind: frame_bindings[frame][slot as usize].kind,
   857	                defining_function: frame_keys[frame].clone(),
   858	                binding_slot: slot,
   859	            });
   860	        }
   861	        entries[index].captures = CanonicalCaptureIdentity(Arc::from(captures.into_boxed_slice()));
   862	    }
   863	}
   864	
   865	/// Resolve one referenced name against the enclosing frame chain: the
   866	/// first (innermost) frame that binds it in a scope containing `site`
   867	/// wins, and within that frame the innermost such binding wins (a later
   868	/// binding wins over an earlier one of the same scope). Returns the
   869	/// `(frame position, binding slot)` pair.
   870	fn resolve_lexical_binding(
   871	    chain: &[usize],
   872	    frame_bindings: &[Arc<[FunctionBindingRecord]>],
   873	    name: &Arc<str>,
   874	    site: verter_span::Span,
   875	) -> Option<(usize, u32)> {
   876	    for &frame in chain {
   877	        let bindings = &frame_bindings[frame];
   878	        let mut best: Option<(u32, verter_span::Span)> = None;
   879	        for (slot, binding) in bindings.iter().enumerate() {
   880	            if &binding.name != name || !scope_contains(binding.scope_span, site) {
   881	                continue;
   882	            }
   883	            let slot = u32::try_from(slot).unwrap_or(u32::MAX);
   884	            best = match best {
   885	                Some((_, best_scope))
  1040	    for (j, signature) in decl.signatures.iter_mut().enumerate() {
  1041	        let Some(ordinal) = checked_rebased_ordinal(base, j) else {
  1042	            return false;
  1043	        };
  1044	        // Generalized over the PATH position rather than the carrier type: the
  1045	        // authored return head's arguments are `TypeArgLocator`s, not
  1046	        // `TypeBodySlot`s, and they carry the same leading `ValueSignature`
  1047	        // step. A rebase typed to the slot alone would leave every merged
  1048	        // contributor's head addressing overload ordinal 0 — the wrong
  1049	        // overload.
  1050	        let repoint_path = |path: &mut Arc<[TypeBodyPathStep]>| {
  1051	            if let Some(TypeBodyPathStep::ValueSignature { .. }) = path.first() {
  1052	                let mut rebased: Vec<TypeBodyPathStep> = path.to_vec();
  1053	                rebased[0] = TypeBodyPathStep::ValueSignature { ordinal };
  1054	                *path = rebased.into();
  1055	            }
  1056	        };
  1057	        let repoint = |slot: &mut TypeBodySlot| repoint_path(&mut slot.path);
  1058	        if let verter_type_expr::facts::FunctionReturnSource::Declared(locator) =
  1059	            &mut signature.return_source
  1060	        {
  1061	            repoint(locator.slot_mut());
  1062	        }
  1063	        if let verter_type_expr::facts::FunctionReturnSource::Flow(identity) =
  1064	            &mut signature.return_source
  1065	        {
  1066	            // The served overload ordinal is the group-level ordinal — it
  1067	            // rebases exactly like the locator path's first step.
  1068	            identity.overload_ordinal = ordinal;
  1069	        }
  1070	        let mut parameters = signature.parameters.to_vec();
  1071	        for parameter in &mut parameters {
  1072	            // An unannotated / rest parameter carries no slot (`ty: None`) —
  1073	            // nothing to re-point.
  1074	            if let Some(ty) = parameter.ty.as_mut() {
  1075	                repoint(ty);
  1076	            }
  1077	        }
  1078	        signature.parameters = parameters.into();
  1079	        rebase_authored_reference_head(&mut signature.return_reference_head, repoint_path);
  1080	    }
  1081	    true
  1082	}
  1083	
  1084	/// Re-point the leading `ValueSignature` step of every argument locator on an
  1085	/// authored reference head. The anchor-free head arms (`NotReference` /
  1086	/// `Unavailable`) and macro-payload argument locators carry no
  1087	/// `ValueSignature`-rooted path and pass through untouched.
  1088	fn rebase_authored_reference_head(
  1089	    head: &mut AuthoredReferenceHeadFact,
  1090	    repoint_path: impl Fn(&mut Arc<[TypeBodyPathStep]>),

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "git show --stat --oneline --decorate --no-renames f46de1b6a; git show --name-status --format='' --no-renames f46de1b6a; nl -ba docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md; rg -n \"R-12|AMEND-AMD-001-TIMING|PRIVATE_CHECKPOINT|stack-window\" docs/arch/refactor/rev11/evidence/maintainer-rulings.md | sed -n '1,220p'; rg -n \"validate-stack-window|stack-window\" .github scripts package.json pnpm-workspace.yaml | sed -n '1,320p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 18972ms:[0m
f46de1b6a feat(ci): add stack-window validator and supersede the checkpoint fail-closed refusal
 .github/workflows/ci.yml                |   4 +
 package.json                            |   2 +-
 scripts/lib/rev11-toml.mjs              | 133 ++++++++++
 scripts/lib/stack-window-lib.mjs        | 442 ++++++++++++++++++++++++++++++++
 scripts/validate-program-state.mjs      | 165 +++---------
 scripts/validate-program-state.test.mjs | 248 +++++++++++++++++-
 scripts/validate-stack-window.mjs       | 146 +++++++++++
 scripts/validate-stack-window.test.mjs  | 387 ++++++++++++++++++++++++++++
 8 files changed, 1386 insertions(+), 141 deletions(-)
M	.github/workflows/ci.yml
M	package.json
A	scripts/lib/rev11-toml.mjs
A	scripts/lib/stack-window-lib.mjs
M	scripts/validate-program-state.mjs
M	scripts/validate-program-state.test.mjs
A	scripts/validate-stack-window.mjs
A	scripts/validate-stack-window.test.mjs
     1	# AMD-001 — Stack-Window Validator Is a Prerequisite for the D1/D2 Path
     2	
     3	**Status:** Registered amendment (repository-local; NOT part of the verbatim-reconstructed
     4	authority set — see [`../PROVENANCE.md`](../PROVENANCE.md)). **§1 amended** (timing clause
     5	only, §§2-4 unchanged) — see "Amendment to §1's timing (rescope)" below.
     6	**Registered in:** [`../README.md`](../README.md) (the `ORCHESTRATOR.md` §3 read-order
     7	item 1) and [`../evidence/maintainer-rulings.md`](../evidence/maintainer-rulings.md).
     8	**Amends the execution plan around:** [`../charters/A6.md`](../charters/A6.md),
     9	[`../program.md`](../program.md) §7 (D1), [`../program-dag.toml`](../program-dag.toml)
    10	(the `D1` block and `D2 -> D1` edge),
    11	[`../contracts/stacked-prs.md`](../contracts/stacked-prs.md) §3.2.
    12	The verbatim authority files themselves are NOT edited: they are byte-for-byte
    13	reconstructions from the digest-verified consolidated master, and editing them would
    14	void the 67/67 fidelity attestation. This amendment is the recorded delta.
    15	
    16	## The defect
    17	
    18	`scripts/validate-program-state.mjs` fails closed on every path it does not model: any
    19	block that has begun (`READY`/`IN_PROGRESS`/`REVIEW`/`ACCEPTANCE_RECOMMENDED`/
    20	`ACCEPTED`/`PRIVATE_CHECKPOINT`) while a direct predecessor is in `PRIVATE_CHECKPOINT`
    21	is REJECTED, because
    22	the conditions under which a `PRIVATE_CHECKPOINT` predecessor legally satisfies
    23	sequencing live in the stack-window model (`contracts/stacked-prs.md` §3.2), which the
    24	program-state validator does not implement.
    25	
    26	But `program-dag.toml` (`D2.predecessors = ["D1"]`, `D1.class =
    27	"foundational-private-checkpoint"`) and `program.md` §7 ("`D1` may receive checkpoint
    28	review approval but must not merge or release independently from `D2`") make
    29	`D1 PRIVATE_CHECKPOINT -> D2` the CANONICAL atomic path — `contracts/stacked-prs.md`
    30	§3.2 says so verbatim: "`D1`/`D2` is the canonical case: D1 is a private checkpoint; D2
    31	is the sole acceptance and landing unit."
    32	
    33	So the ledger's only validator categorically rejects a state the plan REQUIRES the
    34	program to pass through, and no block's charter owns delivering the model that would
    35	make that state validatable. Fail-closed is safe only when the unsupported path has a
    36	prerequisite that delivers its model; without one, fail-closed is a trap: when D2
    37	begins, the mandatory always-green validator (`governance.md` — it "must pass after
    38	every transition") goes red with no legal move to make it green except weakening the
    39	validator ad hoc, unreviewed, at the worst possible moment (mid-atomic-cutover).
    40	
    41	## The amendment
    42	
    43	1. **Prerequisite (as amended — see rescope below).** Before ANY post-A6 stacked delivery
    44	   is permitted — any stack window is opened, any block claims the contingent stacked-work
    45	   exception on a `PRIVATE_CHECKPOINT` predecessor, and in particular before `D1` may enter
    46	   `PRIVATE_CHECKPOINT` — the accepted candidate immediately preceding that event must
    47	   deliver:
    48	   - a **Node stack-window validator** (the `tools/validate_stack_window.py`
    49	     reimplementation under maintainer ruling R-4) validating stack-window records
    50	     against `contracts/stacked-prs.md`;
    51	   - **composite program-state cross-validation**: the stack-window validator and
    52	     `scripts/validate-program-state.mjs` run against each other's records (the
    53	     `--current-program-state` cross-check named in `contracts/stacked-prs.md`), so
    54	     the mutable ledger and the immutable snapshot cannot silently diverge;
    55	   - **CI wiring** for the new validator's test suite, in the same
    56	     `test:scripts`/path-filter pattern used for the program-state validator suite;
    57	   - a **discriminating D1/D2 transition test**: a fixture where `D1` is
    58	     `PRIVATE_CHECKPOINT` inside a validated `ATOMIC_REVIEW` window with `D2` as its
    59	     `acceptance_block_id` VALIDATES, and the same state without the window (or with a
    60	     mismatched snapshot, a non-D2 acceptance block, or a landed-independently D1)
    61	     REJECTS.
    62	2. **The acceptance rule for the composite validator.** The eventual composite
    63	   validation must accept a begun `D2` over a `PRIVATE_CHECKPOINT` `D1` ONLY when `D1`
    64	   is the declared private checkpoint in the same validated `ATOMIC_REVIEW` snapshot
    65	   whose `acceptance_block_id` is `D2` (`contracts/stacked-prs.md` §3.2 — a
    66	   `PRIVATE_CHECKPOINT` state "is valid only for the final acceptance block").
    67	3. **The refusal stays until then.** The program-state validator's fail-closed
    68	   rejection of begun successors of a `PRIVATE_CHECKPOINT` predecessor **must not
    69	   simply be deleted**: it is removed only by being SUPERSEDED by the composite
    70	   validation above, delivered and reviewed under whichever accepted candidate
    71	   discharges the amended §1 prerequisite. Deleting or bypassing the refusal
    72	   without that replacement recreates the unvalidated-path defect this amendment
    73	   records.
    74	4. **Mechanical traceability.** The `A6` context packet AND the `A6` Implementation
    75	   Lock evidence must each NAME this amendment by identifier (`AMD-001`) and bind
    76	   the SHA-256 (lowercase hex, over the raw bytes) of this file as it stands in
    77	   `A6`'s base tree — recompute with
    78	   `sha256sum docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md`.
    79	   (The digest is quoted THERE, never inlined here — a self-digest is a fixpoint.)
    80	   An `A6` candidate whose packet or lock evidence omits the name or the digest has
    81	   not carried this prerequisite; the reviews must treat that as a missing required
    82	   input, not prose to rediscover. This makes the prerequisite mechanically
    83	   traceable from `A6`'s own record rather than dependent on a reader re-finding
    84	   this file. This traceability duty is UNCHANGED by the §1 rescope below — A6 must
    85	   still name and bind this amendment even though A6 itself no longer delivers the
    86	   four artifacts.
    87	
    88	## Amendment to §1's timing (rescope, ratified after A6 architecture review)
    89	
    90	**Failed assumption:** §1's original text assumed A6 — an implementation-lock block whose
    91	charter explicitly excludes "later production ownership or API cutovers" and "speculative
    92	services" — was the correct forcing function for delivering machinery that only becomes
    93	load-bearing once a stack window actually opens.
    94	
    95	**Measured/source evidence:** A6's own accepted candidate unlocks exactly one successor,
    96	`B1`, at `stack_layer 0` / depth 1 with no stack window open (`stack-window-policy.toml`).
    97	No block between `A6` and `D1` opens a stack window under current program sequencing.
    98	`D1` is many blocks downstream. Meanwhile §3's fail-closed refusal in
    99	`scripts/validate-program-state.mjs` is untouched and continues to reject any begun
   100	successor of a `PRIVATE_CHECKPOINT` predecessor — the unmodelled `D1`/`D2` path stays
   101	CLOSED, not open, for the entire interval between A6's acceptance and whichever later
   102	candidate delivers §1's four artifacts.
   103	
   104	**Affected architecture/verification invariants:** §1 (timing only — WHO must deliver and
   105	WHEN). §§2-4 are unaffected: the composite validator's acceptance rule (§2), the
   106	must-not-bypass refusal (§3), and the mechanical-traceability duty (§4) all stand exactly
   107	as written, including `A6`'s continuing duty to name and bind this amendment.
   108	
   109	**Compatibility or consumer consequences:** none. No wire, cache, API, persisted artifact,
   110	or generated output depends on the undelivered artifacts today, because nothing downstream
   111	of A6 opens a stack window yet.
   112	
   113	**Alternatives considered:**
   114	1. Deliver in A6 anyway — rejected: freezes the validator's semantics against zero real
   115	   stack-window instances, which is the same "self-declared test universe" failure
   116	   `governance.md`'s Verification-Must-Prove-Execution rule names, under a regime where
   117	   `performance-gates.toml`-style immutability would make a later correction a
   118	   recalibration rather than an ordinary fix.
   119	2. Defer with no timing change (an open debt row) — rejected: leaves §1's literal text
   120	   pointing at `A6` while the actual delivery point drifts informally, which is exactly the
   121	   "TODO masquerading as a disposition" pattern `CLAUDE.md`'s Explicit Finding Disposition
   122	   rule forbids.
   123	3. **Amend §1's timing clause (ADOPTED)** — the prerequisite now binds to the event it
   124	   actually gates (the first post-A6 stack window opening, and unconditionally before `D1`
   125	   enters `PRIVATE_CHECKPOINT`) rather than to a specific block identity. Whichever accepted
   126	   candidate immediately precedes that event carries the delivery duty. `§3`'s refusal
   127	   remains the enforcement mechanism in the interim, exactly as before.
   128	4. Rescope AMD-001's ownership to a named future block — not adopted now because no
   129	   concrete block between `A6` and `D1` is yet chartered to own it; §1 as amended already
   130	   binds correctly to "whichever candidate is immediately pre-window" without requiring a
   131	   specific block name today. A future amendment MAY name a concrete owning block once one
   132	   is chartered.
   133	
   134	**Work that remains valid:** all of it. A6's mechanical-traceability discharge (naming
   135	AMD-001, binding its digest) stands. Nothing in A6's accepted candidate depended on the
   136	four artifacts existing.
   137	
   138	**Ruling:** ADOPTED. Recorded in `../evidence/maintainer-rulings.md`.
12:R-11 were made and recorded on **2026-08-11**. R-12 (rescoping AMD-001 §1's timing) was
163:## R-12 — Rescope AMD-001 §1's timing away from `A6`
170:Maintainer decision (verbatim): **AMEND-AMD-001-TIMING** (not DEFER, not DELIVER-NOW).
177:enters `PRIVATE_CHECKPOINT`; retain §§2-4 unchanged."
180:seven-field form, recorded inside `AMD-001-stack-window-validator-prerequisite.md` itself
185:`PRIVATE_CHECKPOINT`) carries the delivery duty.
201:dispatch prompt itself labels the decision "R-12"; it is registered here as **R-13**
202:because R-12 in this register is the later AMD-001 timing rescope above — the numbers
205:> R-12 (as labelled in the dispatch prompt): the 469-candidate unlanded local-branch
229:citations of this ruling (rendered there as "R-12", the dispatch prompt's own label) are
240:- **AMD-001** — [`../amendments/AMD-001-stack-window-validator-prerequisite.md`](../amendments/AMD-001-stack-window-validator-prerequisite.md):
242:  `PRIVATE_CHECKPOINT` predecessor collides with the plan's canonical
243:  `D1 PRIVATE_CHECKPOINT -> D2` atomic path; before any post-A6 stacked delivery,
245:  stack-window validator, composite program-state cross-validation, CI wiring, and
246:  a discriminating D1/D2 transition test (rescoped away from `A6` by R-12 — see
package.json:26:    "test:scripts": "vitest run scripts/sccache-env.test.mjs && node --test scripts/lib/publish-set.spec.mjs && node --test scripts/editor-contracts/plenary-outcome-guard.test.mjs && node --test scripts/validate-program-state.test.mjs && node --test scripts/validate-stack-window.test.mjs && node --test scripts/validate-performance-gates.test.mjs && node --test scripts/probe-bundler-route-isolation.test.mjs",
scripts/validate-program-state.test.mjs:907:test("private-checkpoint predecessor: a STACKLESS REVIEW successor over a PRIVATE_CHECKPOINT predecessor is rejected with the fail-closed stack-window message", () => {
scripts/validate-program-state.test.mjs:910:  // stack-window message, not merely a non-zero exit — the generic
scripts/validate-program-state.test.mjs:970:// case fails for its OWN reason — see scripts/validate-stack-window.test.mjs
scripts/validate-program-state.test.mjs:972:// end-to-end wiring through validate-program-state.mjs's --stack-window flag.
scripts/validate-program-state.test.mjs:1054:  const windowPath = write("stack-window-ok.toml", windowText);
scripts/validate-program-state.test.mjs:1075:  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
scripts/validate-program-state.test.mjs:1079:test("D1/D2 transition (AMD-001), negative (a): no --stack-window given REJECTS with the fail-closed message", () => {
scripts/validate-program-state.test.mjs:1100:  const r = run(dag, state, "live"); // no --stack-window
scripts/validate-program-state.test.mjs:1111:  const windowPath = write("stack-window-b.toml", windowText);
scripts/validate-program-state.test.mjs:1133:  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
scripts/validate-program-state.test.mjs:1135:  assert.match(r.err, /composite stack-window validation via --stack-window/);
scripts/validate-program-state.test.mjs:1136:  assert.match(r.err, /block A2 ledger stack_snapshot_digest .* does not match the SHA-256 of the validated stack-window file/);
scripts/validate-program-state.test.mjs:1142:  const windowPath = write("stack-window-c.toml", windowText);
scripts/validate-program-state.test.mjs:1163:  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
scripts/validate-program-state.test.mjs:1165:  assert.match(r.err, /composite stack-window validation via --stack-window/);
scripts/validate-program-state.test.mjs:1172:  const windowPath = write("stack-window-d.toml", windowText);
scripts/validate-program-state.test.mjs:1193:  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
scripts/validate-program-state.test.mjs:1195:  assert.match(r.err, /composite stack-window validation via --stack-window/);
scripts/validate-stack-window.test.mjs:1:// Tests for scripts/validate-stack-window.mjs (AMD-001 §1 — the Node
scripts/validate-stack-window.test.mjs:2:// stack-window validator) and scripts/lib/stack-window-lib.mjs (the shared
scripts/validate-stack-window.test.mjs:7://   node --test scripts/validate-stack-window.test.mjs
scripts/validate-stack-window.test.mjs:22:const VALIDATOR = join(dirname(fileURLToPath(import.meta.url)), "validate-stack-window.mjs");
scripts/validate-stack-window.test.mjs:27:  dir = mkdtempSync(join(tmpdir(), "validate-stack-window-"));
scripts/validate-stack-window.test.mjs:44:const DIGEST = createHash("sha256").update("stack-window-test-digest").digest("hex");
scripts/validate-stack-window.test.mjs:45:const DIGEST2 = createHash("sha256").update("stack-window-test-digest-2").digest("hex");
scripts/validate-stack-window.test.mjs:117:test("template mode accepts the real repository stack-window template", () => {
scripts/validate-stack-window.test.mjs:120:    join(REPO_ROOT, "docs/arch/refactor/rev11/templates/stack-window.template.toml"),
scripts/validate-stack-window.test.mjs:138:  assert.match(r.err, /cannot read stack-window file/);
scripts/validate-stack-window.test.mjs:358:  assert.match(r.err, /block D1 ledger stack_snapshot_digest .* does not match the SHA-256 of the validated stack-window file/);
.github/workflows/ci.yml:62:              - 'scripts/validate-stack-window.mjs'
.github/workflows/ci.yml:63:              - 'scripts/validate-stack-window.test.mjs'
.github/workflows/ci.yml:65:              - 'scripts/lib/stack-window-lib.mjs'
scripts/validate-stack-window.mjs:5:// resolved stack-window record (templates/stack-window.template.toml,
scripts/validate-stack-window.mjs:8://   node scripts/validate-stack-window.mjs \
scripts/validate-stack-window.mjs:9://     --window <stack-window.toml> --mode template|live \
scripts/validate-stack-window.mjs:24:// scripts/lib/stack-window-lib.mjs) to supersede its own PRIVATE_CHECKPOINT-
scripts/validate-stack-window.mjs:41:} from "./lib/stack-window-lib.mjs";
scripts/validate-stack-window.mjs:45:    `${msg}\nusage: node scripts/validate-stack-window.mjs --window <stack-window.toml> --mode template|live [--dag <program-dag.toml>] [--current-program-state <program-state.toml>]\n`,
scripts/validate-stack-window.mjs:94:  const { text: windowText, parsed: window } = loadToml(opts.window, "stack-window");
scripts/lib/rev11-toml.mjs:2:// stack-window validators. Extracted from scripts/validate-program-state.mjs
scripts/lib/rev11-toml.mjs:10:// templates/stack-window.template.toml): full-line comments, `[table]`,
scripts/lib/stack-window-lib.mjs:1:// Shared model for the rev11 stack-window contract (contracts/stacked-prs.md)
scripts/lib/stack-window-lib.mjs:2:// consumed by BOTH scripts/validate-stack-window.mjs (its own CLI) and
scripts/lib/stack-window-lib.mjs:51:// note (see templates/stack-window.template.toml) — string-typed, not
scripts/lib/stack-window-lib.mjs:282:// against the immutable snapshot (a validated stack-window file). Named
scripts/lib/stack-window-lib.mjs:304:        `${label} block ${blockId} ledger stack_snapshot_digest ${JSON.stringify(row.stack_snapshot_digest ?? "")} does not match the SHA-256 of the validated stack-window file (${snapshotDigest}) — the mutable ledger and the immutable snapshot have diverged`,
scripts/lib/stack-window-lib.mjs:373:    return { ok: false, problems: [`stack-window file ${windowPath} could not be read: ${err.message}`] };
scripts/lib/stack-window-lib.mjs:380:      return { ok: false, problems: [`stack-window file ${windowPath}: ${err.message}`] };
scripts/lib/stack-window-lib.mjs:401:      `stack-window ${windowPath} mode is ${JSON.stringify(window.mode ?? "")}, not ATOMIC_REVIEW — a PRIVATE_CHECKPOINT predecessor is legalized only inside the ATOMIC_REVIEW canonical case (contracts/stacked-prs.md 3.2)`,
scripts/lib/stack-window-lib.mjs:406:      `stack-window ${windowPath} acceptance_block_id is ${JSON.stringify(window.acceptance_block_id ?? "")}, not the successor block ${JSON.stringify(successorId)} (AMD-001 §2 — the exception is granted "ONLY when ... the same validated ATOMIC_REVIEW snapshot whose acceptance_block_id is D2")`,
scripts/lib/stack-window-lib.mjs:413:    problems.push(`stack-window ${windowPath} declares no layer for predecessor block ${JSON.stringify(predecessorId)}`);
scripts/lib/stack-window-lib.mjs:416:      `stack-window ${windowPath} layer for predecessor block ${JSON.stringify(predecessorId)} has kind ${JSON.stringify(predecessorLayer.kind ?? "")}, not NON_MERGEABLE_PRIVATE_LAYER — a checkpoint predecessor's layer must never be independently mergeable (contracts/stacked-prs.md 3.2, "no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor")`,
scripts/lib/stack-window-lib.mjs:435:          `stack-window ${windowPath} predecessor layer index ${predecessorLayer.index} is not below acceptance layer index ${acceptanceLayer.index} for block ${JSON.stringify(successorId)}`,
scripts/validate-program-state.mjs:19:import { evaluateCheckpointException } from "./lib/stack-window-lib.mjs";
scripts/validate-program-state.mjs:135:// validator does not model a stack-window relaxation, so it fails closed.
scripts/validate-program-state.mjs:356:    `${msg}\nusage: node scripts/validate-program-state.mjs --dag <program-dag.toml> --state <program-state.toml> --mode template|live [--stack-window <stack-window.toml>]\n`,
scripts/validate-program-state.mjs:366:    if (!["--dag", "--state", "--mode", "--stack-window"].includes(flag))
scripts/validate-program-state.mjs:637:    //     the composite stack-window cross-validation when the caller passes
scripts/validate-program-state.mjs:638:    //     --stack-window — evaluateCheckpointException (scripts/lib/
scripts/validate-program-state.mjs:639:    //     stack-window-lib.mjs) is the SOLE model of that exception, shared
scripts/validate-program-state.mjs:640:    //     with scripts/validate-stack-window.mjs, so this validator never
scripts/validate-program-state.mjs:642:    //     --stack-window given, the original fail-closed refusal stands
scripts/validate-program-state.mjs:646:      const stackWindowPath = opts["stack-window"];
scripts/validate-program-state.mjs:662:          `block ${id} is ${b.status} with predecessor ${p} in PRIVATE_CHECKPOINT — composite stack-window validation via --stack-window ${stackWindowPath} did not establish the checkpoint exception (AMD-001 §2): ${result.problems.join("; ")}`,

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/evidence/maintainer-rulings.md | sed -n '150,190p'; nl -ba .github/workflows/ci.yml | sed -n '45,80p'; nl -ba package.json | sed -n '18,31p'; nl -ba scripts/validate-program-state.test.mjs | sed -n '990,1210p'; nl -ba scripts/validate-stack-window.test.mjs | sed -n '300,385p'; nl -ba scripts/lib/stack-window-lib.mjs | sed -n '270,445p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 18431ms:[0m
   150	   approval, mutation result, or latency result transfers.
   151	
   152	## R-11 — Ratify completion rescope and reduced retraction exit
   153	
   154	Maintainer decision (verbatim): **ACCEPTED IN FULL**.
   155	
   156	The maintainer ratifies **AMD-004 — Defer structural completion to D6 and reduce A3**.
   157	A2C is terminally superseded as an executable predecessor while its reachable DAG and
   158	ledger row remain. A3 depends directly on A2 and retracts only non-G10 wrong-complete
   159	results. Exact structural completion and G10 discrimination remain open debt `FR-D8`,
   160	owned by D6 / `U6.LOOP_CLOSURE`, with the sole demanded `FunctionFlowGraph` as completion
   161	authority and no syntax-only fallback or second classifier.
   162	
   163	## R-12 — Rescope AMD-001 §1's timing away from `A6`
   164	
   165	**Source.** Raised as an `A6` architecture-review finding (AMD-001's traceability and
   166	undelivered-prerequisite obligations), then referred for architecture ruling rather than
   167	resolved by an implementer or track orchestrator, per governance's requirement that a
   168	governance-§10 deviation on a maintainer-authored amendment is decided by the maintainer.
   169	
   170	Maintainer decision (verbatim): **AMEND-AMD-001-TIMING** (not DEFER, not DELIVER-NOW).
   171	"AMD-001 §1 correctly requires the four artifacts before stacked delivery, but incorrectly
   172	makes `A6` their delivery point — `A6` is an implementation-lock block, excludes
   173	speculative later machinery, and its only unlocked successor `B1` is depth-1 with no stack
   174	window. AMD-001 §3's unchanged fail-closed refusal already prevents the unsupported
   175	`D1`/`D2` transition meanwhile. Action: amend AMD-001 §1 so the four artifacts remain
   176	mandatory before the first post-`A6` stack window opens, and unconditionally before `D1`
   177	enters `PRIVATE_CHECKPOINT`; retain §§2-4 unchanged."
   178	
   179	**Disposition.** ADOPTED. `AMD-001` §1 is amended in place (a governance-§10 rescope, full
   180	seven-field form, recorded inside `AMD-001-stack-window-validator-prerequisite.md` itself
   181	under "Amendment to §1's timing"); §§2-4 stand unchanged, including `A6`'s continuing
   182	mechanical-traceability duty to name `AMD-001` and bind its digest. `A6` delivers none of
   183	the four artifacts; whichever accepted candidate immediately precedes the first opened
   184	stack window (or, absent one sooner, the candidate immediately before `D1` enters
   185	`PRIVATE_CHECKPOINT`) carries the delivery duty.
   186	
   187	**Consequence for `A6`'s already-bound digest.** Amending `AMD-001` changes its SHA-256.
   188	`A6`'s Implementation Lock Record and context-packet addendum, which bound the
   189	PRE-amendment digest under §4's traceability duty, must rebind to the POST-amendment
   190	digest before `A6`'s candidate is accepted — tracked as open work on `A6`, not a
    45	              - 'rust-toolchain.toml'
    46	              - 'scripts/check-integration-test-layout.mjs'
    47	              - 'scripts/integration-test-layout-allowlist.json'
    48	              - '.github/workflows/ci.yml'
    49	            js:
    50	              - 'packages/**'
    51	              - '!packages/playground/**'
    52	              - 'package.json'
    53	              - 'pnpm-lock.yaml'
    54	              - 'pnpm-workspace.yaml'
    55	              - 'tsconfig*.json'
    56	              - 'scripts/gen-svelte-goldens.mjs'
    57	              - 'scripts/sccache-env.mjs'
    58	              - 'scripts/sccache-env.test.mjs'
    59	              - 'scripts/svelte-golden-lib.mjs'
    60	              - 'scripts/validate-program-state.mjs'
    61	              - 'scripts/validate-program-state.test.mjs'
    62	              - 'scripts/validate-stack-window.mjs'
    63	              - 'scripts/validate-stack-window.test.mjs'
    64	              - 'scripts/lib/rev11-toml.mjs'
    65	              - 'scripts/lib/stack-window-lib.mjs'
    66	              - 'crates/verter_compiler/tests/svelte_oracle_corpus/**'
    67	              - '.github/workflows/ci.yml'
    68	            wasm:
    69	              - 'crates/verter_compiler/**'
    70	              - 'crates/verter_parser/**'
    71	              - 'crates/verter_session/**'
    72	              - 'crates/verter_workspace/**'
    73	              - 'crates/verter_semantic/**'
    74	              - 'crates/verter_ffi/**'
    75	              - 'crates/verter_wasm/**'
    76	              - 'crates/verter_diagnostics/**'
    77	              - 'crates/verter_actions/**'
    78	              - 'crates/verter_span/**'
    79	              - 'packages/wasm/**'
    80	              - '.cargo/**'
    18	  },
    19	  "main": "index.js",
    20	  "scripts": {
    21	    "test": "pnpm -r --parallel run test",
    22	    "test:e2e": "pnpm --filter verter-vscode test:e2e:matrix",
    23	    "test:e2e:single": "pnpm --filter verter-vscode test:e2e",
    24	    "test:lsp:neutral": "cargo build -p verter_lsp -p verter_relay_shim && pnpm --filter @verter/language-shared build && pnpm --filter @verter/typescript-plugin build && pnpm --filter @verter/dx-harness test:editor-neutral-lsp",
    25	    "test:endurance": "pnpm --filter @verter/dx-harness run test:endurance",
    26	    "test:scripts": "vitest run scripts/sccache-env.test.mjs && node --test scripts/lib/publish-set.spec.mjs && node --test scripts/editor-contracts/plenary-outcome-guard.test.mjs && node --test scripts/validate-program-state.test.mjs && node --test scripts/validate-stack-window.test.mjs && node --test scripts/validate-performance-gates.test.mjs && node --test scripts/probe-bundler-route-isolation.test.mjs",
    27	    "bump": "node scripts/bump.mjs",
    28	    "fmt": "oxfmt",
    29	    "build": "pnpm run build:native && pnpm run build:lsp && pnpm run build:wasm && pnpm run build:ts",
    30	    "build:native": "pnpm --filter @verter/native build",
    31	    "build:lsp": "cargo build -p verter_lsp",
   990	max_open_layers = 2
   991	owner = "orchestrator"
   992	evidence_root = "docs/arch/refactor/rev11/evidence"
   993	shared_writer_surfaces = []
   994	integration_commands = []
   995	notes = ""
   996	
   997	[[layer]]
   998	index = 1
   999	layer_id = "A1"
  1000	block_id = "A1"
  1001	charter_digest = "${DIGEST}"
  1002	kind = "${a1Kind}"
  1003	branch = "a1-branch"
  1004	base_branch = "main"
  1005	worktree = "wt-a1"
  1006	worker = "w1"
  1007	pr_number = 0
  1008	pr_url = ""
  1009	base_sha = "${SHA}"
  1010	base_tree = "${SHA}"
  1011	head_sha = ""
  1012	head_tree = ""
  1013	patch_digest = ""
  1014	generated_digest = ""
  1015	evidence_digest = ""
  1016	ci_state = "PENDING"
  1017	review_state = "PENDING"
  1018	mergeable = ${a1Kind === "mergeable"}
  1019	notes = ""
  1020	
  1021	[[layer]]
  1022	index = 2
  1023	layer_id = "A2"
  1024	block_id = "A2"
  1025	charter_digest = "${DIGEST}"
  1026	kind = "mergeable"
  1027	branch = "a2-branch"
  1028	base_branch = "a1-branch"
  1029	worktree = "wt-a2"
  1030	worker = "w2"
  1031	pr_number = 0
  1032	pr_url = ""
  1033	base_sha = "${SHA}"
  1034	base_tree = "${SHA}"
  1035	head_sha = ""
  1036	head_tree = ""
  1037	patch_digest = ""
  1038	generated_digest = ""
  1039	evidence_digest = ""
  1040	ci_state = "PENDING"
  1041	review_state = "PENDING"
  1042	mergeable = true
  1043	notes = ""
  1044	`;
  1045	}
  1046	
  1047	function digestOf(text) {
  1048	  return createHash("sha256").update(text).digest("hex");
  1049	}
  1050	
  1051	test("D1/D2 transition (AMD-001): PRIVATE_CHECKPOINT A1 inside a validated ATOMIC_REVIEW window with A2 as acceptance_block_id VALIDATES", () => {
  1052	  const dag = write("dag-cp-d1d2-ok.toml", DAG_CP);
  1053	  const windowText = stackWindowText();
  1054	  const windowPath = write("stack-window-ok.toml", windowText);
  1055	  const snap = digestOf(windowText);
  1056	  const state = write(
  1057	    "state-d1d2-ok.toml",
  1058	    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
  1059	      acceptedBlock("A0") +
  1060	      "\n" +
  1061	      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
  1062	      "\n" +
  1063	      block("A2", "REVIEW", {
  1064	        stack_id: "S1",
  1065	        stack_snapshot_digest: snap,
  1066	        stack_layer: 2,
  1067	        base_sha: SHA,
  1068	        candidate_sha: SHA,
  1069	        candidate_tree: TREE,
  1070	        charter_digest: DIGEST,
  1071	        context_packet_digest: DIGEST,
  1072	        evidence_digest: DIGEST,
  1073	      }),
  1074	  );
  1075	  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  1076	  assert.equal(r.status, 0, `expected pass, got:\n${r.err}\n${r.out}`);
  1077	});
  1078	
  1079	test("D1/D2 transition (AMD-001), negative (a): no --stack-window given REJECTS with the fail-closed message", () => {
  1080	  const dag = write("dag-cp-d1d2-a.toml", DAG_CP);
  1081	  const state = write(
  1082	    "state-d1d2-a.toml",
  1083	    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
  1084	      acceptedBlock("A0") +
  1085	      "\n" +
  1086	      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: DIGEST, stack_layer: 1 }) +
  1087	      "\n" +
  1088	      block("A2", "REVIEW", {
  1089	        stack_id: "S1",
  1090	        stack_snapshot_digest: DIGEST,
  1091	        stack_layer: 2,
  1092	        base_sha: SHA,
  1093	        candidate_sha: SHA,
  1094	        candidate_tree: TREE,
  1095	        charter_digest: DIGEST,
  1096	        context_packet_digest: DIGEST,
  1097	        evidence_digest: DIGEST,
  1098	      }),
  1099	  );
  1100	  const r = run(dag, state, "live"); // no --stack-window
  1101	  assert.notEqual(r.status, 0);
  1102	  assert.match(
  1103	    r.err,
  1104	    /block A2 is REVIEW with predecessor A1 in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block \(contracts\/stacked-prs\.md\), which this validator does not model — fail closed/,
  1105	  );
  1106	});
  1107	
  1108	test("D1/D2 transition (AMD-001), negative (b): mismatched snapshot digest REJECTS", () => {
  1109	  const dag = write("dag-cp-d1d2-b.toml", DAG_CP);
  1110	  const windowText = stackWindowText();
  1111	  const windowPath = write("stack-window-b.toml", windowText);
  1112	  const snap = digestOf(windowText);
  1113	  const wrongSnap = digestOf("not the window contents");
  1114	  const state = write(
  1115	    "state-d1d2-b.toml",
  1116	    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
  1117	      acceptedBlock("A0") +
  1118	      "\n" +
  1119	      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
  1120	      "\n" +
  1121	      block("A2", "REVIEW", {
  1122	        stack_id: "S1",
  1123	        stack_snapshot_digest: wrongSnap, // MISMATCH
  1124	        stack_layer: 2,
  1125	        base_sha: SHA,
  1126	        candidate_sha: SHA,
  1127	        candidate_tree: TREE,
  1128	        charter_digest: DIGEST,
  1129	        context_packet_digest: DIGEST,
  1130	        evidence_digest: DIGEST,
  1131	      }),
  1132	  );
  1133	  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  1134	  assert.notEqual(r.status, 0);
  1135	  assert.match(r.err, /composite stack-window validation via --stack-window/);
  1136	  assert.match(r.err, /block A2 ledger stack_snapshot_digest .* does not match the SHA-256 of the validated stack-window file/);
  1137	});
  1138	
  1139	test("D1/D2 transition (AMD-001), negative (c): acceptance_block_id names a block OTHER than A2 REJECTS", () => {
  1140	  const dag = write("dag-cp-d1d2-c.toml", DAG_CP);
  1141	  const windowText = stackWindowText({ acceptanceId: "SOMETHING_ELSE" });
  1142	  const windowPath = write("stack-window-c.toml", windowText);
  1143	  const snap = digestOf(windowText);
  1144	  const state = write(
  1145	    "state-d1d2-c.toml",
  1146	    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
  1147	      acceptedBlock("A0") +
  1148	      "\n" +
  1149	      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
  1150	      "\n" +
  1151	      block("A2", "REVIEW", {
  1152	        stack_id: "S1",
  1153	        stack_snapshot_digest: snap,
  1154	        stack_layer: 2,
  1155	        base_sha: SHA,
  1156	        candidate_sha: SHA,
  1157	        candidate_tree: TREE,
  1158	        charter_digest: DIGEST,
  1159	        context_packet_digest: DIGEST,
  1160	        evidence_digest: DIGEST,
  1161	      }),
  1162	  );
  1163	  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  1164	  assert.notEqual(r.status, 0);
  1165	  assert.match(r.err, /composite stack-window validation via --stack-window/);
  1166	  assert.match(r.err, /acceptance_block_id is "SOMETHING_ELSE", not the successor block "A2"/);
  1167	});
  1168	
  1169	test("D1/D2 transition (AMD-001), negative (d): A1 landed independently (its layer is kind = mergeable, not NON_MERGEABLE_PRIVATE_LAYER) REJECTS", () => {
  1170	  const dag = write("dag-cp-d1d2-d.toml", DAG_CP);
  1171	  const windowText = stackWindowText({ a1Kind: "mergeable" });
  1172	  const windowPath = write("stack-window-d.toml", windowText);
  1173	  const snap = digestOf(windowText);
  1174	  const state = write(
  1175	    "state-d1d2-d.toml",
  1176	    header({ status: "ACTIVE", current: "A2", repoSha: SHA, dagDigest: DAG_CP_DIGEST }) +
  1177	      acceptedBlock("A0") +
  1178	      "\n" +
  1179	      checkpointBlock("A1", { stack_id: "S1", stack_snapshot_digest: snap, stack_layer: 1 }) +
  1180	      "\n" +
  1181	      block("A2", "REVIEW", {
  1182	        stack_id: "S1",
  1183	        stack_snapshot_digest: snap,
  1184	        stack_layer: 2,
  1185	        base_sha: SHA,
  1186	        candidate_sha: SHA,
  1187	        candidate_tree: TREE,
  1188	        charter_digest: DIGEST,
  1189	        context_packet_digest: DIGEST,
  1190	        evidence_digest: DIGEST,
  1191	      }),
  1192	  );
  1193	  const r = run(dag, state, "live", undefined, ["--stack-window", windowPath]);
  1194	  assert.notEqual(r.status, 0);
  1195	  assert.match(r.err, /composite stack-window validation via --stack-window/);
  1196	  assert.match(
  1197	    r.err,
  1198	    /layer for predecessor block "A1" has kind "mergeable", not NON_MERGEABLE_PRIVATE_LAYER — a checkpoint predecessor's layer must never be independently mergeable/,
  1199	  );
  1200	});
  1201	
  1202	function writeLandingRecord(root, id, body) {
  1203	  const blockDir = join(root, id);
  1204	  mkdirSync(blockDir, { recursive: true });
  1205	  const artifact = join(blockDir, "landing-record.md");
  1206	  writeFileSync(artifact, body);
  1207	  return artifact;
  1208	}
  1209	
  1210	test("live mode: a well-formed but WRONG program_dag_digest is a violation", () => {
   300	  );
   301	  const p = write("atomic-good-class.toml", atomicWindow());
   302	  const r = run(["--window", p, "--mode", "live", "--dag", dag]);
   303	  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
   304	});
   305	
   306	// -- --current-program-state composite cross-validation
   307	function ledger({ d1Snapshot, d2Snapshot, d1StackId = "S1", d2StackId = "S1", d1Layer = 1, d2Layer = 2, d1Status = "PRIVATE_CHECKPOINT" } = {}) {
   308	  const row = (id, status, stackId, snapshot, layerIdx) =>
   309	    `[[block]]\nid = "${id}"\nstatus = "${status}"\nstack_id = "${stackId}"\nstack_snapshot_digest = "${snapshot}"\nstack_layer = ${layerIdx}\n`;
   310	  return `schema = 1\nrevision = 11\n\n${row("D1", d1Status, d1StackId, d1Snapshot, d1Layer)}\n${row("D2", "REVIEW", d2StackId, d2Snapshot, d2Layer)}\n`;
   311	}
   312	
   313	function snapshotOf(windowPath) {
   314	  const text = readFileSync(windowPath, "utf8");
   315	  return createHash("sha256").update(text).digest("hex");
   316	}
   317	
   318	test("--current-program-state requires --mode live", () => {
   319	  const p = write("cps-template.toml", atomicWindow());
   320	  const state = write("cps-state-a.toml", ledger({ d1Snapshot: DIGEST, d2Snapshot: DIGEST }));
   321	  const r = run(["--window", p, "--mode", "template", "--current-program-state", state]);
   322	  assert.notEqual(r.status, 0);
   323	  assert.match(r.err, /--current-program-state was given but --mode is "template"/);
   324	});
   325	
   326	test("--current-program-state: skipped with an explanatory note when structural validation fails first", () => {
   327	  const p = write("cps-bad-structural.toml", atomicWindow({ acceptanceBlockId: "" }));
   328	  const state = write("cps-state-b.toml", ledger({ d1Snapshot: DIGEST, d2Snapshot: DIGEST }));
   329	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   330	  assert.notEqual(r.status, 0);
   331	  assert.match(r.err, /cross-validation skipped — .* failed its own structural validation first/);
   332	});
   333	
   334	test("--current-program-state: matching ledger passes", () => {
   335	  const p = write("cps-ok.toml", atomicWindow());
   336	  const digest = readFileSync(p, "utf8");
   337	  const snap = createHash("sha256").update(digest).digest("hex");
   338	  const state = write("cps-state-ok.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap }));
   339	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   340	  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
   341	});
   342	
   343	test("--current-program-state: stack_id mismatch is rejected", () => {
   344	  const p = write("cps-stackid.toml", atomicWindow());
   345	  const text = readFileSync(p, "utf8");
   346	  const snap = createHash("sha256").update(text).digest("hex");
   347	  const state = write("cps-state-stackid.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap, d1StackId: "OTHER" }));
   348	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   349	  assert.notEqual(r.status, 0);
   350	  assert.match(r.err, /block D1 ledger stack_id "OTHER" does not match window stack_id "S1"/);
   351	});
   352	
   353	test("--current-program-state: stack_snapshot_digest mismatch is rejected", () => {
   354	  const p = write("cps-snapshot.toml", atomicWindow());
   355	  const state = write("cps-state-snapshot.toml", ledger({ d1Snapshot: DIGEST, d2Snapshot: DIGEST2 }));
   356	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   357	  assert.notEqual(r.status, 0);
   358	  assert.match(r.err, /block D1 ledger stack_snapshot_digest .* does not match the SHA-256 of the validated stack-window file/);
   359	});
   360	
   361	test("--current-program-state: stack_layer mismatch is rejected", () => {
   362	  const p = write("cps-layer.toml", atomicWindow());
   363	  const text = readFileSync(p, "utf8");
   364	  const snap = createHash("sha256").update(text).digest("hex");
   365	  const state = write("cps-state-layer.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap, d1Layer: 5 }));
   366	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   367	  assert.notEqual(r.status, 0);
   368	  assert.match(r.err, /block D1 ledger stack_layer 5 does not match window layer index 1/);
   369	});
   370	
   371	test("--current-program-state: block missing from ledger is rejected", () => {
   372	  const p = write("cps-missing.toml", atomicWindow());
   373	  const state = write("cps-state-missing.toml", `schema = 1\nrevision = 11\n`);
   374	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   375	  assert.notEqual(r.status, 0);
   376	  assert.match(r.err, /names block_id "D1", which does not exist in the program-state ledger/);
   377	});
   378	
   379	test("--current-program-state: a checkpoint layer whose ledger row is not PRIVATE_CHECKPOINT is rejected", () => {
   380	  const p = write("cps-not-checkpoint.toml", atomicWindow());
   381	  const text = readFileSync(p, "utf8");
   382	  const snap = createHash("sha256").update(text).digest("hex");
   383	  const state = write("cps-state-not-checkpoint.toml", ledger({ d1Snapshot: snap, d2Snapshot: snap, d1Status: "ACCEPTED" }));
   384	  const r = run(["--window", p, "--mode", "live", "--current-program-state", state]);
   385	  assert.notEqual(r.status, 0);
   270	              `${label} layer ${JSON.stringify(layer.layer_id ?? "")} names block_id ${JSON.stringify(layer.block_id)}, whose DAG class is ${JSON.stringify(cls ?? "")} — a private ATOMIC_REVIEW layer must repeat the acceptance block's own id or name a block whose DAG class is "foundational-private-checkpoint" (contracts/stacked-prs.md 3.2)`,
   271	            );
   272	          }
   273	        }
   274	      }
   275	    }
   276	  }
   277	
   278	  return v;
   279	}
   280	
   281	// -- Composite cross-validation: the mutable ledger (program-state.toml)
   282	// against the immutable snapshot (a validated stack-window file). Named
   283	// `--current-program-state` in contracts/stacked-prs.md 2. `snapshotDigest`
   284	// is the SHA-256 of the fully resolved window file — the immutable
   285	// StackSnapshotId (contracts/stacked-prs.md 2).
   286	export function crossValidateAgainstProgramState({ window, label, snapshotDigest, stateById }) {
   287	  const v = [];
   288	  const layers = Array.isArray(window.layer) ? window.layer : [];
   289	  for (const layer of layers) {
   290	    const blockId = layer.block_id;
   291	    if (typeof blockId !== "string" || blockId === "") continue; // already reported by structural check
   292	    const row = stateById.get(blockId);
   293	    if (!row) {
   294	      v.push(`${label} layer ${JSON.stringify(layer.layer_id ?? "")} names block_id ${JSON.stringify(blockId)}, which does not exist in the program-state ledger`);
   295	      continue;
   296	    }
   297	    if (row.stack_id !== window.stack_id) {
   298	      v.push(
   299	        `${label} block ${blockId} ledger stack_id ${JSON.stringify(row.stack_id ?? "")} does not match window stack_id ${JSON.stringify(window.stack_id)} — the mutable ledger and the immutable snapshot have diverged`,
   300	      );
   301	    }
   302	    if (!(typeof row.stack_snapshot_digest === "string" && row.stack_snapshot_digest === snapshotDigest)) {
   303	      v.push(
   304	        `${label} block ${blockId} ledger stack_snapshot_digest ${JSON.stringify(row.stack_snapshot_digest ?? "")} does not match the SHA-256 of the validated stack-window file (${snapshotDigest}) — the mutable ledger and the immutable snapshot have diverged`,
   305	      );
   306	    }
   307	    if (row.stack_layer !== layer.index) {
   308	      v.push(
   309	        `${label} block ${blockId} ledger stack_layer ${JSON.stringify(row.stack_layer ?? "")} does not match window layer index ${JSON.stringify(layer.index ?? "")} — the mutable ledger and the immutable snapshot have diverged`,
   310	      );
   311	    }
   312	    // contracts/stacked-prs.md 3.2 — "an explicit program checkpoint such as
   313	    // D1 whose PRIVATE_CHECKPOINT state is valid only for the final
   314	    // acceptance block": a NON_MERGEABLE_PRIVATE_LAYER standing for a real
   315	    // program checkpoint (not an internal sublayer of the acceptance block
   316	    // itself) must be recorded in PRIVATE_CHECKPOINT — never landed/accepted
   317	    // independently.
   318	    if (
   319	      layer.kind === "NON_MERGEABLE_PRIVATE_LAYER" &&
   320	      blockId !== window.acceptance_block_id &&
   321	      row.status !== "PRIVATE_CHECKPOINT"
   322	    ) {
   323	      v.push(
   324	        `${label} block ${blockId} is a NON_MERGEABLE_PRIVATE_LAYER checkpoint but ledger status is ${JSON.stringify(row.status ?? "")}, not PRIVATE_CHECKPOINT — a checkpoint layer never lands independently (contracts/stacked-prs.md 3.2)`,
   325	      );
   326	    }
   327	  }
   328	  return v;
   329	}
   330	
   331	// Minimal program-state block index — id -> row. Full ledger structural
   332	// validation (duplicate ids, closed status enum, sequencing, ...) stays
   333	// validate-program-state.mjs's job; a cross-validating caller only needs the
   334	// id -> row lookup.
   335	export function buildStateById(state) {
   336	  const map = new Map();
   337	  const blocks = Array.isArray(state.block) ? state.block : [];
   338	  for (const b of blocks) {
   339	    if (typeof b.id !== "string" || b.id === "") continue;
   340	    map.set(b.id, b);
   341	  }
   342	  return map;
   343	}
   344	
   345	export function buildDagClassMap(dag) {
   346	  const map = new Map();
   347	  const blocks = Array.isArray(dag.block) ? dag.block : [];
   348	  for (const b of blocks) {
   349	    if (typeof b.id !== "string" || b.id === "") continue;
   350	    map.set(b.id, typeof b.class === "string" ? b.class : "");
   351	  }
   352	  return map;
   353	}
   354	
   355	// -- The AMD-001 §2 acceptance rule, as a composite check callable from
   356	// validate-program-state.mjs's PRIVATE_CHECKPOINT-predecessor sequencing
   357	// gate. This is the SOLE model of "does a stack window legalize this
   358	// PRIVATE_CHECKPOINT predecessor" — validate-program-state.mjs must not grow
   359	// a second, parallel notion of the same question.
   360	//
   361	// Returns { ok: true } when the exception is established, or
   362	// { ok: false, problems: string[] } — each problem names its OWN distinct
   363	// cause (missing/unreadable/unparseable window; failed structural
   364	// validation; wrong acceptance_block_id; failed cross-validation, which
   365	// covers a mismatched snapshot digest and a checkpoint layer whose ledger
   366	// row is not PRIVATE_CHECKPOINT).
   367	export function evaluateCheckpointException({ windowPath, predecessorId, successorId, stateById, dagById }) {
   368	  const problems = [];
   369	  let text;
   370	  try {
   371	    text = readFileSync(windowPath, "utf8");
   372	  } catch (err) {
   373	    return { ok: false, problems: [`stack-window file ${windowPath} could not be read: ${err.message}`] };
   374	  }
   375	  let window;
   376	  try {
   377	    window = parseToml(text, windowPath);
   378	  } catch (err) {
   379	    if (err instanceof TomlError) {
   380	      return { ok: false, problems: [`stack-window file ${windowPath}: ${err.message}`] };
   381	    }
   382	    throw err;
   383	  }
   384	
   385	  const dagClassMap = new Map();
   386	  if (dagById) {
   387	    for (const [id, b] of dagById) {
   388	      dagClassMap.set(id, typeof b?.class === "string" ? b.class : "");
   389	    }
   390	  }
   391	
   392	  const structural = validateStackWindowStructure(window, {
   393	    cliMode: "live",
   394	    dagClassMap,
   395	    label: windowPath,
   396	  });
   397	  problems.push(...structural);
   398	
   399	  if (window.mode !== "ATOMIC_REVIEW") {
   400	    problems.push(
   401	      `stack-window ${windowPath} mode is ${JSON.stringify(window.mode ?? "")}, not ATOMIC_REVIEW — a PRIVATE_CHECKPOINT predecessor is legalized only inside the ATOMIC_REVIEW canonical case (contracts/stacked-prs.md 3.2)`,
   402	    );
   403	  }
   404	  if (window.acceptance_block_id !== successorId) {
   405	    problems.push(
   406	      `stack-window ${windowPath} acceptance_block_id is ${JSON.stringify(window.acceptance_block_id ?? "")}, not the successor block ${JSON.stringify(successorId)} (AMD-001 §2 — the exception is granted "ONLY when ... the same validated ATOMIC_REVIEW snapshot whose acceptance_block_id is D2")`,
   407	    );
   408	  }
   409	
   410	  const layers = Array.isArray(window.layer) ? window.layer : [];
   411	  const predecessorLayer = layers.find((l) => l.block_id === predecessorId);
   412	  if (!predecessorLayer) {
   413	    problems.push(`stack-window ${windowPath} declares no layer for predecessor block ${JSON.stringify(predecessorId)}`);
   414	  } else if (predecessorLayer.kind !== "NON_MERGEABLE_PRIVATE_LAYER") {
   415	    problems.push(
   416	      `stack-window ${windowPath} layer for predecessor block ${JSON.stringify(predecessorId)} has kind ${JSON.stringify(predecessorLayer.kind ?? "")}, not NON_MERGEABLE_PRIVATE_LAYER — a checkpoint predecessor's layer must never be independently mergeable (contracts/stacked-prs.md 3.2, "no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor")`,
   417	    );
   418	  }
   419	
   420	  // Only cross-validate identity binding once the window is at least
   421	  // internally coherent enough to compute a meaningful snapshot digest and
   422	  // name a real predecessor layer — otherwise the cross-check would just
   423	  // restate the structural problems above under a different label.
   424	  if (structural.length === 0 && predecessorLayer) {
   425	    const snapshotDigest = createHash("sha256").update(text).digest("hex");
   426	    const cross = crossValidateAgainstProgramState({ window, label: windowPath, snapshotDigest, stateById });
   427	    problems.push(...cross);
   428	    if (
   429	      predecessorLayer.index !== undefined &&
   430	      window.acceptance_block_id === successorId
   431	    ) {
   432	      const acceptanceLayer = layers.find((l) => l.block_id === successorId && l.kind === "mergeable");
   433	      if (acceptanceLayer && !(predecessorLayer.index < acceptanceLayer.index)) {
   434	        problems.push(
   435	          `stack-window ${windowPath} predecessor layer index ${predecessorLayer.index} is not below acceptance layer index ${acceptanceLayer.index} for block ${JSON.stringify(successorId)}`,
   436	        );
   437	      }
   438	    }
   439	  }
   440	
   441	  return problems.length === 0 ? { ok: true } : { ok: false, problems };
   442	}

[35m[3mexec[0m[0m
[1m/bin/zsh -lc 'node --test scripts/validate-stack-window.test.mjs scripts/validate-program-state.test.mjs'[0m in <MACHINE_ROOT>/verter
[31m exited 1 in 14773ms:[0m
(node:60669) Warning: The 'NO_COLOR' env is ignored due to the 'FORCE_COLOR' env being set.
(Use `node --trace-warnings ...` to show where the warning was created)
TAP version 13
# (node:62416) Warning: The 'NO_COLOR' env is ignored due to the 'FORCE_COLOR' env being set.
# (Use `node --trace-warnings ...` to show where the warning was created)
# Subtest: template mode: template-shaped state with REQUIRED_ placeholders passes
not ok 1 - template mode: template-shaped state with REQUIRED_ placeholders passes
  ---
  duration_ms: 0.86575
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:304:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: fully-resolved state with a legal ACCEPTED predecessor and IN_PROGRESS block passes
not ok 2 - live mode: fully-resolved state with a legal ACCEPTED predecessor and IN_PROGRESS block passes
  ---
  duration_ms: 0.126416
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:337:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: sequencing invariant: IN_PROGRESS with an unaccepted predecessor is rejected
not ok 3 - sequencing invariant: IN_PROGRESS with an unaccepted predecessor is rejected
  ---
  duration_ms: 0.069583
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:353:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: stacked-work exception: a bare stack_id with no established stack is REJECTED
not ok 4 - stacked-work exception: a bare stack_id with no established stack is REJECTED
  ---
  duration_ms: 0.098459
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:371:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: status gate: ACCEPTED with PENDING mandates and no accepted identity is rejected
not ok 5 - status gate: ACCEPTED with PENDING mandates and no accepted identity is rejected
  ---
  duration_ms: 0.053916
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:401:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: strict TOML reader: unbalanced quoting is a loud parse failure, never a silent mis-read
not ok 6 - strict TOML reader: unbalanced quoting is a loud parse failure, never a silent mis-read
  ---
  duration_ms: 0.050083
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:422:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: block-set mismatch: state missing a DAG block and carrying an extra one is rejected with the symmetric difference
not ok 7 - block-set mismatch: state missing a DAG block and carrying an extra one is rejected with the symmetric difference
  ---
  duration_ms: 0.062541
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:459:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: zero blocks validated is a FAILURE, not a pass
not ok 8 - zero blocks validated is a FAILURE, not a pass
  ---
  duration_ms: 0.058
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:478:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: mandate class gate: NOT_REQUIRED on a foundational-class block is rejected
not ok 9 - mandate class gate: NOT_REQUIRED on a foundational-class block is rejected
  ---
  duration_ms: 0.379334
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:486:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: sequencing invariant: a stackless READY block with an unaccepted predecessor is rejected
not ok 10 - sequencing invariant: a stackless READY block with an unaccepted predecessor is rejected
  ---
  duration_ms: 0.171459
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:514:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: stacked-work exception: a predecessor citing a DIFFERENT stack snapshot digest is rejected
not ok 11 - stacked-work exception: a predecessor citing a DIFFERENT stack snapshot digest is rejected
  ---
  duration_ms: 0.058916
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:536:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: an empty program_dag_digest is a violation, not a silent skip
not ok 12 - live mode: an empty program_dag_digest is a violation, not a silent skip
  ---
  duration_ms: 0.052959
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:573:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: landing equivalence: ACCEPTED with a diverged accepted identity and no landing_equivalence_digest is rejected
not ok 13 - landing equivalence: ACCEPTED with a diverged accepted identity and no landing_equivalence_digest is rejected
  ---
  duration_ms: 0.04825
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:592:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: private checkpoint: a PROVEN checkpoint over accepted predecessors passes (no fail-closed false positive)
not ok 14 - private checkpoint: a PROVEN checkpoint over accepted predecessors passes (no fail-closed false positive)
  ---
  duration_ms: 0.060792
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:614:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: private checkpoint: a PREMATURE checkpoint (unaccepted predecessor) is rejected
not ok 15 - private checkpoint: a PREMATURE checkpoint (unaccepted predecessor) is rejected
  ---
  duration_ms: 0.063458
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:633:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: private checkpoint: an UNPROVEN checkpoint (PENDING mandates, empty identities) is rejected
not ok 16 - private checkpoint: an UNPROVEN checkpoint (PENDING mandates, empty identities) is rejected
  ---
  duration_ms: 0.050958
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:654:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: private checkpoint: the status on a WRONG-CLASS block is rejected
not ok 17 - private checkpoint: the status on a WRONG-CLASS block is rejected
  ---
  duration_ms: 0.051167
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:675:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: a non-ACTIVE top-level status is rejected
not ok 18 - live mode: a non-ACTIVE top-level status is rejected
  ---
  duration_ms: 0.120916
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:696:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: stacked-work exception: a fully-ESTABLISHED same-snapshot stack passes
not ok 19 - stacked-work exception: a fully-ESTABLISHED same-snapshot stack passes
  ---
  duration_ms: 0.170292
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:712:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: stacked-work exception: an equal (non-lower) predecessor stack_layer is rejected
not ok 20 - stacked-work exception: an equal (non-lower) predecessor stack_layer is rejected
  ---
  duration_ms: 0.07575
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:740:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: stacked-work exception: a TERMINATED (ABORTED) predecessor inside the claimed stack is rejected
not ok 21 - stacked-work exception: a TERMINATED (ABORTED) predecessor inside the claimed stack is rejected
  ---
  duration_ms: 0.050667
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:769:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: mandate class gate: architecture_review NOT_REQUIRED on a subsystem-class block passes
not ok 22 - mandate class gate: architecture_review NOT_REQUIRED on a subsystem-class block passes
  ---
  duration_ms: 0.059959
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:799:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: entry lock: the DAG root block ACCEPTED with an EMPTY or MISSING entry_lock_digest is rejected
not ok 23 - entry lock: the DAG root block ACCEPTED with an EMPTY or MISSING entry_lock_digest is rejected
  ---
  duration_ms: 0.044833
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:817:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: entry lock: the DAG root block in REVIEW requires the digest; a bound digest passes
not ok 24 - entry lock: the DAG root block in REVIEW requires the digest; a bound digest passes
  ---
  duration_ms: 0.226291
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:862:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: private-checkpoint predecessor: a STACKLESS REVIEW successor over a PRIVATE_CHECKPOINT predecessor is rejected with the fail-closed stack-window message
not ok 25 - private-checkpoint predecessor: a STACKLESS REVIEW successor over a PRIVATE_CHECKPOINT predecessor is rejected with the fail-closed stack-window message
  ---
  duration_ms: 0.062542
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:907:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: private-checkpoint predecessor: a REVIEW successor with OTHERWISE-PERFECT stack fields over a PRIVATE_CHECKPOINT predecessor is still rejected
not ok 26 - private-checkpoint predecessor: a REVIEW successor with OTHERWISE-PERFECT stack fields over a PRIVATE_CHECKPOINT predecessor is still rejected
  ---
  duration_ms: 0.029958
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:936:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: D1/D2 transition (AMD-001): PRIVATE_CHECKPOINT A1 inside a validated ATOMIC_REVIEW window with A2 as acceptance_block_id VALIDATES
not ok 27 - D1/D2 transition (AMD-001): PRIVATE_CHECKPOINT A1 inside a validated ATOMIC_REVIEW window with A2 as acceptance_block_id VALIDATES
  ---
  duration_ms: 0.026125
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1051:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: D1/D2 transition (AMD-001), negative (a): no --stack-window given REJECTS with the fail-closed message
not ok 28 - D1/D2 transition (AMD-001), negative (a): no --stack-window given REJECTS with the fail-closed message
  ---
  duration_ms: 0.022917
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1079:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: D1/D2 transition (AMD-001), negative (b): mismatched snapshot digest REJECTS
not ok 29 - D1/D2 transition (AMD-001), negative (b): mismatched snapshot digest REJECTS
  ---
  duration_ms: 0.022209
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1108:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: D1/D2 transition (AMD-001), negative (c): acceptance_block_id names a block OTHER than A2 REJECTS
not ok 30 - D1/D2 transition (AMD-001), negative (c): acceptance_block_id names a block OTHER than A2 REJECTS
  ---
  duration_ms: 0.021166
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1139:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: D1/D2 transition (AMD-001), negative (d): A1 landed independently (its layer is kind = mergeable, not NON_MERGEABLE_PRIVATE_LAYER) REJECTS
not ok 31 - D1/D2 transition (AMD-001), negative (d): A1 landed independently (its layer is kind = mergeable, not NON_MERGEABLE_PRIVATE_LAYER) REJECTS
  ---
  duration_ms: 0.097208
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1169:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: a well-formed but WRONG program_dag_digest is a violation
not ok 32 - live mode: a well-formed but WRONG program_dag_digest is a violation
  ---
  duration_ms: 0.02625
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1210:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: an unresolvable evidence_root cannot bind evidence_digest — fail closed
not ok 33 - live mode: an unresolvable evidence_root cannot bind evidence_digest — fail closed
  ---
  duration_ms: 0.132667
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1229:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: a well-formed evidence_digest that does not match the bound artifact is a violation
not ok 34 - live mode: a well-formed evidence_digest that does not match the bound artifact is a violation
  ---
  duration_ms: 0.073084
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1258:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: a bound evidence_digest with no artifact under evidence_root is a violation
not ok 35 - live mode: a bound evidence_digest with no artifact under evidence_root is a violation
  ---
  duration_ms: 0.045666
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1285:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: an evidence_digest that matches the bound landing-record artifact passes
not ok 36 - live mode: an evidence_digest that matches the bound landing-record artifact passes
  ---
  duration_ms: 0.038667
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1306:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: multiple evidence roots: a block's artifact resolves from the second declared root when the first root does not carry it
not ok 37 - multiple evidence roots: a block's artifact resolves from the second declared root when the first root does not carry it
  ---
  duration_ms: 0.037
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1326:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: multiple evidence roots: a mismatched digest against the resolved artifact still fails
not ok 38 - multiple evidence roots: a mismatched digest against the resolved artifact still fails
  ---
  duration_ms: 0.0385
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1348:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: multiple evidence roots: a block with no artifact under ANY declared root still fails
not ok 39 - multiple evidence roots: a block with no artifact under ANY declared root still fails
  ---
  duration_ms: 0.036625
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1377:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: multiple evidence roots: one unresolvable declared root still fails closed even when another root resolves the artifact
not ok 40 - multiple evidence roots: one unresolvable declared root still fails closed even when another root resolves the artifact
  ---
  duration_ms: 0.038666
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1399:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: root-level sibling <id>-summary.md (not nested under <id>/) resolves
not ok 41 - extended artifact convention: root-level sibling <id>-summary.md (not nested under <id>/) resolves
  ---
  duration_ms: 0.040291
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1461:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: root-level sibling <id>-summary.md with a mismatched digest still fails
not ok 42 - extended artifact convention: root-level sibling <id>-summary.md with a mismatched digest still fails
  ---
  duration_ms: 0.040541
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1480:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: <id>-summary.md resolves and its digest matches
not ok 43 - extended artifact convention: <id>-summary.md resolves and its digest matches
  ---
  duration_ms: 0.0365
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1509:3'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: <id>-summary.md with a mismatched digest still fails
not ok 44 - extended artifact convention: <id>-summary.md with a mismatched digest still fails
  ---
  duration_ms: 0.041125
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1529:3'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: landing-equivalence.md resolves and its digest matches
not ok 45 - extended artifact convention: landing-equivalence.md resolves and its digest matches
  ---
  duration_ms: 0.035625
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1509:3'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: landing-equivalence.md with a mismatched digest still fails
not ok 46 - extended artifact convention: landing-equivalence.md with a mismatched digest still fails
  ---
  duration_ms: 0.036084
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1529:3'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: one nested match (<root>/<id>/*/landing-record.md) resolves
not ok 47 - extended artifact convention: one nested match (<root>/<id>/*/landing-record.md) resolves
  ---
  duration_ms: 0.03575
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1557:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: extended artifact convention: MULTIPLE nested matches are ambiguous and fail closed
not ok 48 - extended artifact convention: MULTIPLE nested matches are ambiguous and fail closed
  ---
  duration_ms: 0.03525
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1582:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: git identity: a well-formed but NEVER-COMMITTED accepted_sha is rejected
not ok 49 - git identity: a well-formed but NEVER-COMMITTED accepted_sha is rejected
  ---
  duration_ms: 0.035333
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1625:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: git identity: a real but DANGLING accepted_sha (unreachable from the repository tip) is rejected — the A5 case
not ok 50 - git identity: a real but DANGLING accepted_sha (unreachable from the repository tip) is rejected — the A5 case
  ---
  duration_ms: 0.036
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1647:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: git identity: a candidate_tree that is the tree of a DIFFERENT commit is rejected
not ok 51 - git identity: a candidate_tree that is the tree of a DIFFERENT commit is rejected
  ---
  duration_ms: 0.036125
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1675:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: git identity: an ACCEPTED base_sha that is NOT an ancestor of accepted_sha is rejected
not ok 52 - git identity: an ACCEPTED base_sha that is NOT an ancestor of accepted_sha is rejected
  ---
  duration_ms: 0.03525
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1704:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: git identity: a genuine multi-commit ancestry chain (base strictly precedes accepted) passes
not ok 53 - git identity: a genuine multi-commit ancestry chain (base strictly precedes accepted) passes
  ---
  duration_ms: 0.036084
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1728:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode: NOT a git repository fails loudly, never a silent skip that greens the run
not ok 54 - live mode: NOT a git repository fails loudly, never a silent skip that greens the run
  ---
  duration_ms: 0.035541
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1743:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: template mode: passes with placeholder identities and NO git repository at all
not ok 55 - template mode: passes with placeholder identities and NO git repository at all
  ---
  duration_ms: 0.049041
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1763:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: READY block with an unratified enabling_amendment is a violation (BV1 shape)
not ok 56 - amendment authority gate: READY block with an unratified enabling_amendment is a violation (BV1 shape)
  ---
  duration_ms: 0.036542
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1799:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: the same block at LOCKED with an unratified enabling_amendment passes
not ok 57 - amendment authority gate: the same block at LOCKED with an unratified enabling_amendment passes
  ---
  duration_ms: 0.037375
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1818:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: ACCEPTED block with an unratified enabling_amendment is a violation
not ok 58 - amendment authority gate: ACCEPTED block with an unratified enabling_amendment is a violation
  ---
  duration_ms: 0.053666
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1833:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: a ratified enabling_amendment imposes no restriction at READY or ACCEPTED
not ok 59 - amendment authority gate: a ratified enabling_amendment imposes no restriction at READY or ACCEPTED
  ---
  duration_ms: 0.038542
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1852:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: enabling_amendment naming a file that does not exist is a violation, even at LOCKED
not ok 60 - amendment authority gate: enabling_amendment naming a file that does not exist is a violation, even at LOCKED
  ---
  duration_ms: 4.809542
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1879:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: an unparseable Status line is a violation, not a silent pass, even at LOCKED
not ok 61 - amendment authority gate: an unparseable Status line is a violation, not a silent pass, even at LOCKED
  ---
  duration_ms: 0.152167
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1898:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: an empty enabling_amendment is unaffected
not ok 62 - amendment authority gate: an empty enabling_amendment is unaffected
  ---
  duration_ms: 0.050125
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1917:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: amendment authority gate: an ambiguous enabling_amendment match (multiple files) is a violation
not ok 63 - amendment authority gate: an ambiguous enabling_amendment match (multiple files) is a violation
  ---
  duration_ms: 0.044958
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1932:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: a PASS mandate whose reviewed SHA differs from candidate_sha is a stale verdict — REJECTED
not ok 64 - review verdict binding: a PASS mandate whose reviewed SHA differs from candidate_sha is a stale verdict — REJECTED
  ---
  duration_ms: 0.039541
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1967:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: a PASS mandate with an EMPTY reviewed SHA is rejected
not ok 65 - review verdict binding: a PASS mandate with an EMPTY reviewed SHA is rejected
  ---
  duration_ms: 0.040666
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:1992:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: a PENDING mandate that wrongly carries a reviewed SHA is rejected
not ok 66 - review verdict binding: a PENDING mandate that wrongly carries a reviewed SHA is rejected
  ---
  duration_ms: 1.901458
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:2012:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: NOT_REQUIRED wrongly carrying a reviewed SHA is rejected
not ok 67 - review verdict binding: NOT_REQUIRED wrongly carrying a reviewed SHA is rejected
  ---
  duration_ms: 0.137459
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:2038:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: a reviewed SHA naming a non-existent commit is rejected (live git existence)
not ok 68 - review verdict binding: a reviewed SHA naming a non-existent commit is rejected (live git existence)
  ---
  duration_ms: 0.064875
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:2062:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: all three mandates correctly bound to a NON-DEFAULT candidate_sha passes
not ok 69 - review verdict binding: all three mandates correctly bound to a NON-DEFAULT candidate_sha passes
  ---
  duration_ms: 0.055916
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:2090:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: review verdict binding: applies in template mode too (structural, not git-dependent)
not ok 70 - review verdict binding: applies in template mode too (structural, not git-dependent)
  ---
  duration_ms: 0.04825
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:2118:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-program-state-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:45:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:44:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
not ok 71 - <MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs
  ---
  duration_ms: 0.3515
  location: '<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:106:1'
  failureType: 'hookFailed'
  error: 'The "path" argument must be of type string or an instance of Buffer or URL. Received undefined'
  code: 'ERR_INVALID_ARG_TYPE'
  name: 'TypeError'
  stack: |-
    rmSync (node:fs:1259:10)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-program-state.test.mjs:107:3)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.runHook (node:internal/test_runner/test:723:20)
    after (node:internal/test_runner/test:755:20)
    Test.run (node:internal/test_runner/test:804:13)
  ...
# (node:62417) Warning: The 'NO_COLOR' env is ignored due to the 'FORCE_COLOR' env being set.
# (Use `node --trace-warnings ...` to show where the warning was created)
# Subtest: template mode accepts the real repository stack-window template
not ok 72 - template mode accepts the real repository stack-window template
  ---
  duration_ms: 1.037584
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:117:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: usage failure: missing required flags exits 2
not ok 73 - usage failure: missing required flags exits 2
  ---
  duration_ms: 0.072167
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:129:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: unreadable window file exits 2
not ok 74 - unreadable window file exits 2
  ---
  duration_ms: 0.037666
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:135:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: unparseable TOML exits 1 with a VIOLATION line, not a silent pass
not ok 75 - unparseable TOML exits 1 with a VIOLATION line, not a silent pass
  ---
  duration_ms: 0.0275
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:141:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: unknown top-level mode value is rejected
not ok 76 - unknown top-level mode value is rejected
  ---
  duration_ms: 0.045625
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:150:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode rejects status = TEMPLATE
not ok 77 - live mode rejects status = TEMPLATE
  ---
  duration_ms: 0.023583
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:157:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: live mode rejects an unresolved digest field
not ok 78 - live mode rejects an unresolved digest field
  ---
  duration_ms: 0.099083
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:164:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: template mode accepts a REQUIRED_ placeholder digest
not ok 79 - template mode accepts a REQUIRED_ placeholder digest
  ---
  duration_ms: 0.070458
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:171:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: max_open_layers outside [2, 6] is rejected
not ok 80 - max_open_layers outside [2, 6] is rejected
  ---
  duration_ms: 0.511958
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:177:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: layer count exceeding the window's own max_open_layers is rejected
not ok 81 - layer count exceeding the window's own max_open_layers is rejected
  ---
  duration_ms: 0.108375
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:184:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: duplicate layer_id is rejected
not ok 82 - duplicate layer_id is rejected
  ---
  duration_ms: 0.032833
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:194:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: duplicate layer index is rejected
not ok 83 - duplicate layer index is rejected
  ---
  duration_ms: 0.030084
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:201:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: malformed layer charter_digest is rejected
not ok 84 - malformed layer charter_digest is rejected
  ---
  duration_ms: 0.02525
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:208:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: LANDABLE: non-empty acceptance_block_id is rejected
not ok 85 - LANDABLE: non-empty acceptance_block_id is rejected
  ---
  duration_ms: 0.034833
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:216:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: LANDABLE: duplicate block_id across layers is rejected
not ok 86 - LANDABLE: duplicate block_id across layers is rejected
  ---
  duration_ms: 0.027125
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:223:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: LANDABLE: two distinct block_ids passes
not ok 87 - LANDABLE: two distinct block_ids passes
  ---
  duration_ms: 0.088333
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:233:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: ATOMIC_REVIEW: empty acceptance_block_id is rejected
not ok 88 - ATOMIC_REVIEW: empty acceptance_block_id is rejected
  ---
  duration_ms: 0.027584
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:257:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: ATOMIC_REVIEW: zero mergeable layers is rejected
not ok 89 - ATOMIC_REVIEW: zero mergeable layers is rejected
  ---
  duration_ms: 0.023792
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:264:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: ATOMIC_REVIEW: two mergeable layers is rejected
not ok 90 - ATOMIC_REVIEW: two mergeable layers is rejected
  ---
  duration_ms: 0.070417
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:271:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: ATOMIC_REVIEW: a valid D1/D2 window passes structurally
not ok 91 - ATOMIC_REVIEW: a valid D1/D2 window passes structurally
  ---
  duration_ms: 0.024542
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:279:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: ATOMIC_REVIEW with --dag: a private layer naming a non-checkpoint block is rejected
not ok 92 - ATOMIC_REVIEW with --dag: a private layer naming a non-checkpoint block is rejected
  ---
  duration_ms: 0.028417
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:285:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: ATOMIC_REVIEW with --dag: a private layer naming a foundational-private-checkpoint block passes
not ok 93 - ATOMIC_REVIEW with --dag: a private layer naming a foundational-private-checkpoint block passes
  ---
  duration_ms: 0.021542
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:296:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state requires --mode live
not ok 94 - --current-program-state requires --mode live
  ---
  duration_ms: 0.020417
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:318:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: skipped with an explanatory note when structural validation fails first
not ok 95 - --current-program-state: skipped with an explanatory note when structural validation fails first
  ---
  duration_ms: 0.034875
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:326:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: matching ledger passes
not ok 96 - --current-program-state: matching ledger passes
  ---
  duration_ms: 0.028042
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:334:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: stack_id mismatch is rejected
not ok 97 - --current-program-state: stack_id mismatch is rejected
  ---
  duration_ms: 0.02075
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:343:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: stack_snapshot_digest mismatch is rejected
not ok 98 - --current-program-state: stack_snapshot_digest mismatch is rejected
  ---
  duration_ms: 0.021083
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:353:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: stack_layer mismatch is rejected
not ok 99 - --current-program-state: stack_layer mismatch is rejected
  ---
  duration_ms: 0.020333
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:361:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: block missing from ledger is rejected
not ok 100 - --current-program-state: block missing from ledger is rejected
  ---
  duration_ms: 0.019458
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:371:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
# Subtest: --current-program-state: a checkpoint layer whose ledger row is not PRIVATE_CHECKPOINT is rejected
not ok 101 - --current-program-state: a checkpoint layer whose ledger row is not PRIVATE_CHECKPOINT is rejected
  ---
  duration_ms: 0.01925
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:379:1'
  failureType: 'hookFailed'
  error: "EPERM: operation not permitted, mkdtemp '/var/folders/km/hqbb_7hd3k562lz54m0nf0940000gn/T/validate-stack-window-XXXXXX'"
  code: 'EPERM'
  stack: |-
    mkdtempSync (node:fs:3005:18)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:27:9)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.createHook (node:internal/test_runner/test:639:33)
    node:internal/test_runner/harness:297:12
    file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:26:1
    ModuleJob.run (node:internal/modules/esm/module_job:325:25)
  ...
not ok 102 - <MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs
  ---
  duration_ms: 0.395375
  location: '<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:29:1'
  failureType: 'hookFailed'
  error: 'The "path" argument must be of type string or an instance of Buffer or URL. Received undefined'
  code: 'ERR_INVALID_ARG_TYPE'
  name: 'TypeError'
  stack: |-
    rmSync (node:fs:1259:10)
    TestContext.<anonymous> (file://<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:30:3)
    TestHook.runInAsyncScope (node:async_hooks:206:9)
    TestHook.run (node:internal/test_runner/test:796:25)
    TestHook.run (node:internal/test_runner/test:1041:18)
    TestHook.run (node:internal/util:527:20)
    Test.runHook (node:internal/test_runner/test:723:20)
    after (node:internal/test_runner/test:755:20)
    Test.run (node:internal/test_runner/test:804:13)
  ...
1..102
# tests 102
# suites 0
# pass 0
# fail 102
# cancelled 0
# skipped 0
# todo 0
# duration_ms 417.226

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "git log --oneline --decorate --all --grep='stack-window validator' -8; rg -n \"f46de1b6a|AMD-001.*deliver|supersede.*fail-closed|validate-stack-window\" docs/arch/architecture-lock docs/arch/refactor/rev11/evidence | sed -n '1,300p'; nl -ba docs/arch/architecture-lock/ledger/program-state.toml | sed -n '1,55p'; rg -n '"'^id = "(D1|D2|C1)"|''^status =|stack_snapshot'"' docs/arch/architecture-lock/ledger/program-state.toml | tail -80"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 11414ms:[0m
355fb396d index on fix/flaky-tests-perf-program: f46de1b6a feat(ci): add stack-window validator and supersede the checkpoint fail-closed refusal
f46de1b6a feat(ci): add stack-window validator and supersede the checkpoint fail-closed refusal
fb863297a docs(arch): rescope the stack-window validator prerequisite's timing away from the implementation lock
docs/arch/architecture-lock/ledger/program-state.toml:275:notes = "Implementation Lock Record + locked performance-gates.toml (no placeholders) + bound B1 charter. Reviews initially BLOCKING (missing AMD-001 traceability, missing A5-L1 disposition, one gate-direction bug on session.cache_admit_cacheable); one bounded fix pass corrected all, reattested clean. Architecture ruling (Codex Sol xhigh, read-only): (1) AMD-001 §1's four-artifact stack-window-validator prerequisite is AMENDED (not delivered now, not deferred as open debt) so it binds to the first post-A6 opened stack window / unconditionally before D1 PRIVATE_CHECKPOINT, not to A6 itself — recorded as a governance §10 acceptance-contract rescope in evidence/A6/AMD-001-deviation-memo.md; (2) discovery D-1 (machine-path leak in A4/A5 context-packet.md) ruled FIX-NOW, applied, both blocks' context_packet_digest corrected. Maintainer ratified A5-L1/G1/DD1/R-12/S1 (loop5_instrumentation converge+delete owners G4/K3/L4; attribution/compile-fail locked as per-block commands, CI deferred post-program; verter_semantic->verter_workspace pinned exception, C1 removal gate; 469 unlanded local branches abandoned as a class; stack policy max_open_stack_layers=2/ATOMIC_REVIEW/LOCAL_BRANCH_CHAIN). LANDING EQUIVALENCE: accepted identity diverges from the reviewed candidate identity — a maintainer-directed cross-block housekeeping squash (collapsing A4-ledger/A5/A6 bookkeeping commits into one, excluding the real product commit 1ab403c01) replaced the branch tip after review; the delta is disclosed, evidenced, and tree-diffed in docs/arch/refactor/rev11/evidence/A6/landing-equivalence-A5-A6.md (landing_equivalence_digest binds that file). Reviewed candidate preserved from GC via tag program-history/A6-reviewed-candidate. B1 is BLOCK_READY (charter digest ac60d191221fc5e5938e0343091c6809648a482960ca7c1a49596e547d3e28e1); J1 stays not-unlocked, no CSS work selected. EVIDENCE RE-BINDING (evidence/A6-summary.md): original digest unrecoverable (matched no committed artifact); re-bound to this block real landing summary. BASE IDENTITY CORRECTED by orchestrator audit: base_sha was 9e053d014ca4f98124f431a8d39e1a688087bf49, a pre-squash commit that is not an ancestor of the accepted identity and whose tree appears nowhere in this branch's history, so the recorded base described a state that was never landed. Re-bound to 1ab403c0107801b080438fab30b887c0c8164ecb, the real landed parent of the accepted commit, which is verifiable and satisfies base-ancestor-of-accepted. Content is unaffected; this repairs a dangling reference left by a history squash."
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:171:"AMD-001 §1 correctly requires the four artifacts before stacked delivery, but incorrectly
docs/arch/refactor/rev11/evidence/maintainer-rulings.md:182:mechanical-traceability duty to name `AMD-001` and bind its digest. `A6` delivers none of
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:6959:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md-120-   pointing at `A6` while the actual delivery point drifts informally, which is exactly the
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7123:docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:871:| **U-9** *(informational — no longer a deviation against this lock)* All four of **`AMD-001`** §1's deliverables — the Node stack-window validator, composite program-state cross-validation, that validator's CI wiring, and the discriminating checkpoint/acceptance transition test — are **not delivered by this lock** (§9 enumerates them and binds the amendment's post-rescope digest) | The maintainer ruled **AMEND-AMD-001-TIMING** ([`maintainer-rulings.md` R-12](../maintainer-rulings.md)) **before this candidate's acceptance**: §1 is amended in place so the four artifacts bind to whichever accepted candidate immediately precedes the first opened stack window, and unconditionally to the one before `D1` enters `PRIVATE_CHECKPOINT` — **not to this block by name**. So this row records no open deviation and no unratified choice; the amendment text and the delivery reality now agree. The [`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md) is retained as the historical record that a `DEFER` recommendation was made and was superseded by a *different* ruling. Substantively unchanged: no window is open, the unlocked block is sequential and single-layer, and the program-state validator's fail-closed refusal is untouched here, so the unmodelled path stays closed | a later block, under the amended §1 — **not this one**; the orchestrator carries the duty forward to the candidate the amended timing names | **before the first snapshot with more than one open layer is minted, and unconditionally before the private-checkpoint block begins.** The amendment's refusal is superseded by delivering the validator, never by deleting it |
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7136:docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md-1-# A6 — Architecture deviation memo: AMD-001's four artifacts are not delivered
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7162:docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md-141-  prerequisite binds to the FIRST STACK WINDOW rather than to A6's acceptance — delivery required
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7167:docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md-161-1. **Disposition** — `ADOPT-NOW` (deliver the four artifacts, in this block or an immediate
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7173:docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md-167-   amendment that says A6 delivered something A6 did not.
docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md:7199:docs/arch/refactor/rev11/evidence/A6-summary.md:21:| [`A6/AMD-001-deviation-memo.md`](A6/AMD-001-deviation-memo.md) | the `governance.md` §10 deviation memo for the amendment's four undelivered artifacts — **RULED and superseded**: the maintainer adopted the rescope, not the memo's `DEFER`; retained as the historical record |
docs/arch/refactor/rev11/evidence/A6-summary.md:21:| [`A6/AMD-001-deviation-memo.md`](A6/AMD-001-deviation-memo.md) | the `governance.md` §10 deviation memo for the amendment's four undelivered artifacts — **RULED and superseded**: the maintainer adopted the rescope, not the memo's `DEFER`; retained as the historical record |
docs/arch/refactor/rev11/evidence/A6-summary.md:131:  that is now correct rather than a gap.** `AMD-001` originally named this block as their deliverer:
docs/arch/refactor/rev11/evidence/A5/program-operations-policy.md:221:is why AMD-001's prerequisite binds only "before any post-A6 stacked delivery", and why A0–A5 have
docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md:1:# A6 — Architecture deviation memo: AMD-001's four artifacts are not delivered
docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md:73:  AMD-001 §1 assumes A6 — an evidence-and-lock block whose deliverables are a record, a gate file,
docs/arch/refactor/rev11/evidence/A6/AMD-001-deviation-memo.md:102:  - AMD-001 §2 (the composite acceptance rule) — undelivered, and therefore unexercised.
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2962:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md-110-or generated output depends on the undelivered artifacts today, because nothing downstream
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2972:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md-120-   pointing at `A6` while the actual delivery point drifts informally, which is exactly the
docs/arch/refactor/rev11/evidence/BF2/command-proofs/oracle-manifest-cells-reopen3-unaffected/scoping-consult-out.txt:2978:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md-126-   candidate immediately precedes that event carries the delivery duty. `§3`'s refusal
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:765:**None of `AMD-001` §1's four deliverables is delivered by this candidate**, and the amendment's own
docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:871:| **U-9** *(informational — no longer a deviation against this lock)* All four of **`AMD-001`** §1's deliverables — the Node stack-window validator, composite program-state cross-validation, that validator's CI wiring, and the discriminating checkpoint/acceptance transition test — are **not delivered by this lock** (§9 enumerates them and binds the amendment's post-rescope digest) | The maintainer ruled **AMEND-AMD-001-TIMING** ([`maintainer-rulings.md` R-12](../maintainer-rulings.md)) **before this candidate's acceptance**: §1 is amended in place so the four artifacts bind to whichever accepted candidate immediately precedes the first opened stack window, and unconditionally to the one before `D1` enters `PRIVATE_CHECKPOINT` — **not to this block by name**. So this row records no open deviation and no unratified choice; the amendment text and the delivery reality now agree. The [`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md) is retained as the historical record that a `DEFER` recommendation was made and was superseded by a *different* ruling. Substantively unchanged: no window is open, the unlocked block is sequential and single-layer, and the program-state validator's fail-closed refusal is untouched here, so the unmodelled path stays closed | a later block, under the amended §1 — **not this one**; the orchestrator carries the duty forward to the candidate the amended timing names | **before the first snapshot with more than one open layer is minted, and unconditionally before the private-checkpoint block begins.** The amendment's refusal is superseded by delivering the validator, never by deleting it |
     1	# Verter Revision 11 program-state template.
     2	# A0 copies this into the repository evidence area, replaces REQUIRED_* values,
     3	# updates state only through the orchestrator, and validates it after every change.
     4	
     5	schema = 1
     6	revision = 11
     7	status = "ACTIVE"
     8	# authority_package_digest is EMPTY, not a digest: package validation was WAIVED by
     9	# explicit maintainer decision (ruling R-2) - the Revision 11 ZIP was never
    10	# available, so no package digest exists to record. The waiver itself is recorded
    11	# in block A0 notes below and in the repository PROVENANCE.md.
    12	authority_package_digest = ""
    13	release_report_digest = "027f4e6dca5798ad75066acba3bb560980f7e81103492c0fcbad75c9edc62f91"
    14	program_dag_digest = "214120cb972ac1cdd768298389e332cdde7814aca6d4f3154c0141fe5ce988da"
    15	entry_checkout_sha = "9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0"
    16	entry_checkout_tree = "3cf111cf5665586b7d8fdfd520f01cfee3bf8108"
    17	implementation_baseline_sha = ""
    18	implementation_baseline_tree = ""
    19	implementation_lock_digest = ""
    20	performance_gates_digest = ""
    21	architecture_premise_ledger_digest = ""
    22	current_block = "BS1"
    23	
    24	[repository]
    25	remote = "https://github.com/pikax/verter"
    26	branch = "main"
    27	head_sha = "9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0"
    28	head_tree = "3cf111cf5665586b7d8fdfd520f01cfee3bf8108"
    29	dirty = false
    30	untracked_count = 0
    31	
    32	[orchestration]
    33	maintainer = "Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax)"
    34	orchestrator = "Claude Opus 5 (claude-opus-5) main session"
    35	requested_model_id = "claude-opus-5"
    36	actual_model_id = "claude-opus-5"
    37	orchestrator_runtime = "Claude Code"
    38	orchestrator_runtime_version = "2.1.222"
    39	provider = "Anthropic"
    40	fallback_or_substitution = false
    41	model_exception_accepted = false
    42	max_active_workers = 3
    43	max_open_stack_layers = 4
    44	stack_tool = "UNDECIDED_UNTIL_A6"
    45	stack_mode_policy = "UNDECIDED_UNTIL_A6"
    46	evidence_roots = ["docs/arch/architecture-lock/ledger", "docs/arch/refactor/rev11/evidence"]
    47	
    48	# Status: LOCKED | READY | IN_PROGRESS | REVIEW | ACCEPTANCE_RECOMMENDED | ACCEPTED |
    49	#         BLOCKED | RESCOPE_REQUIRED | ABORTED | SUPERSEDED | PRIVATE_CHECKPOINT
    50	# Reviews: NOT_REQUIRED | PENDING | PASS | BLOCKING | NOT_PROVEN | INVALIDATED
    51	# candidate_sha/tree = exact reviewed cumulative candidate.
    52	# accepted_sha/tree = actual landed commit and full repository tree; they may differ
    53	# from the reviewed candidate after a base advance. landing_equivalence_digest binds
    54	# both base/candidate/accepted identities and exact candidate-delta/post-landing proof.
    55	# conformance_reviewed_sha/architecture_reviewed_sha/adversarial_reviewed_sha = the
665:stack_snapshot_digest = ""
679:status = "LOCKED"
690:stack_snapshot_digest = ""
703:id = "C1"
704:status = "READY"
715:stack_snapshot_digest = ""
729:status = "LOCKED"
740:stack_snapshot_digest = ""
754:status = "LOCKED"
765:stack_snapshot_digest = ""
779:status = "LOCKED"
790:stack_snapshot_digest = ""
803:id = "D1"
804:status = "LOCKED"
815:stack_snapshot_digest = ""
828:id = "D2"
829:status = "LOCKED"
840:stack_snapshot_digest = ""
854:status = "LOCKED"
865:stack_snapshot_digest = ""
879:status = "LOCKED"
890:stack_snapshot_digest = ""
904:status = "LOCKED"
915:stack_snapshot_digest = ""
929:status = "LOCKED"
940:stack_snapshot_digest = ""
954:status = "LOCKED"
965:stack_snapshot_digest = ""
979:status = "LOCKED"
990:stack_snapshot_digest = ""
1004:status = "LOCKED"
1015:stack_snapshot_digest = ""
1029:status = "LOCKED"
1040:stack_snapshot_digest = ""
1054:status = "LOCKED"
1065:stack_snapshot_digest = ""
1079:status = "LOCKED"
1090:stack_snapshot_digest = ""
1104:status = "LOCKED"
1115:stack_snapshot_digest = ""
1129:status = "LOCKED"
1140:stack_snapshot_digest = ""
1154:status = "LOCKED"
1165:stack_snapshot_digest = ""
1179:status = "LOCKED"
1190:stack_snapshot_digest = ""
1204:status = "LOCKED"
1215:stack_snapshot_digest = ""
1229:status = "LOCKED"
1240:stack_snapshot_digest = ""
1254:status = "LOCKED"
1265:stack_snapshot_digest = ""
1279:status = "LOCKED"
1290:stack_snapshot_digest = ""
1304:status = "LOCKED"
1315:stack_snapshot_digest = ""
1329:status = "LOCKED"
1340:stack_snapshot_digest = ""
1354:status = "LOCKED"
1365:stack_snapshot_digest = ""
1379:status = "LOCKED"
1390:stack_snapshot_digest = ""
1404:status = "LOCKED"
1415:stack_snapshot_digest = ""
1429:status = "LOCKED"
1440:stack_snapshot_digest = ""
1454:status = "LOCKED"
1465:stack_snapshot_digest = ""
1479:status = "LOCKED"
1490:stack_snapshot_digest = ""
1504:status = "LOCKED"
1515:stack_snapshot_digest = ""
1529:status = "LOCKED"
1540:stack_snapshot_digest = ""
1554:status = "LOCKED"
1565:stack_snapshot_digest = ""
1579:status = "LOCKED"
1590:stack_snapshot_digest = ""
1604:status = "LOCKED"
1615:stack_snapshot_digest = ""

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs | sed -n '1,72p'; nl -ba crates/verter_session/src/typeinfo/typeinfo_tests/flow_return_substrate.rs | sed -n '250,282p'; nl -ba crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs | sed -n '730,760p'; nl -ba crates/verter_session/src/host_flow_return_audit.rs | sed -n '1,18p'; nl -ba crates/verter_session/src/host_flow_return_audit.rs | sed -n '61,88p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 10063ms:[0m
     1	//! @ai-generated - FlowReturn substrate characterization rows.
     2	//!
     3	//! Two families of rows:
     4	//!
     5	//! - Un-ignored `flow_surface_*` / characterization rows pin the
     6	//!   observable FlowReturn surface: symbolic call returns resolve, the
     7	//!   `this`-call fallback yields `any`, return-free loops stay fall-through
     8	//!   transparent, return-bearing loops stay degraded, switch/try clause
     9	//!   returns join, an empty recursive cycle degrades without collapsing
    10	//!   to `never`, and complete unannotated functions never surface a
    11	//!   semantic miss.
    12	//!
    13	//! - `flow_return_substrate_*` rows are the producer-routing contracts:
    14	//!   each asserts the pinned surface AND that the demand was served by a
    15	//!   `FlowReturn` dispatch through `ProjectSemanticDispatch`.
    16	
    17	use super::support::*;
    18	use crate::VerterHost;
    19	use verter_audit::RequestKindPayload;
    20	use verter_type_expr::LiteralValue;
    21	
    22	const SUBSTRATE: &str = "/fixtures/flow_return_substrate.ts";
    23	
    24	/// Dispatch-mask bit of the `FlowReturn` family — its
    25	/// [`crate::semantic_query::SemanticQueryKeyTag::bit_index`].
    26	fn flow_return_dispatch_bit() -> u32 {
    27	    crate::semantic_query::SemanticQueryKeyTag::FlowReturn.bit_index()
    28	}
    29	
    30	fn upsert_substrate_fixture(host: &VerterHost) {
    31	    upsert_ts(host, SUBSTRATE, FLOW_RETURN_SUBSTRATE);
    32	}
    33	
    34	fn resolve_substrate_alias(
    35	    host: &VerterHost,
    36	    alias: &str,
    37	) -> (TypeExpr, verter_audit::RequestAuditRecord) {
    38	    resolve_expr(host, SUBSTRATE, alias, &[], ProjectionMode::Expanded)
    39	}
    40	
    41	/// Whether this request dispatched the `FlowReturn` family anywhere in its
    42	/// resolution (root or any nested subquery).
    43	fn flow_return_dispatched(record: &verter_audit::RequestAuditRecord) -> bool {
    44	    match &record.kind_payload {
    45	        RequestKindPayload::TypeResolution(payload) => {
    46	            payload.semantic_query_dispatch_mask & (1 << flow_return_dispatch_bit()) != 0
    47	        }
    48	        other => panic!("expected TypeResolution payload, got {other:?}"),
    49	    }
    50	}
    51	
    52	fn assert_flow_return_dispatched(record: &verter_audit::RequestAuditRecord, alias: &str) {
    53	    assert!(
    54	        flow_return_dispatched(record),
    55	        "{alias} must be served by a FlowReturn dispatch through ProjectSemanticDispatch"
    56	    );
    57	}
    58	
    59	/// The FRAME-level degraded surface: an evaluation with NO VALUE AT ALL
    60	/// (an unmodelled control surface, an empty recursive cycle) projects the
    61	/// typed miss.
    62	fn assert_semantic_miss(expr: &TypeExpr) {
    63	    match expr {
    64	        TypeExpr::Unknown(unknown) => assert_eq!(
    65	            unknown.raw(),
    66	            crate::semantic_query::compat_spelling::SEMANTIC_MISS
    67	        ),
    68	        other => panic!("expected the degraded semantic-miss surface, got {other:?}"),
    69	    }
    70	}
    71	
    72	/// The POSITIONAL degraded surface: a position the substrate has no model
   250	fn mixed_relation_function_return_component_stays_coinductive_assignable() {
   251	    // Relate(SubMixedA, SubMixedB) → the `next` return relation →
   252	    // SubMixedA.next's body-derived return → Relate(SubMixedB, SubMixedA) →
   253	    // assumption on the open relation. The component closes coinductive
   254	    // positive; this verdict must survive the obligation-runtime rewrite.
   255	    let host = make_host_with_footprint();
   256	    upsert_substrate_fixture(&host);
   257	    let (expr, record) = resolve_substrate_alias(&host, "SubMixedAssign");
   258	    assert_string_literal(&expr, "yes");
   259	    assert_query_mode(&record, ProjectionModeTag::Expanded);
   260	}
   261	
   262	// ---------------------------------------------------------------------------
   263	// Producer-routing contracts (RED → lifted as the substrate lands)
   264	// ---------------------------------------------------------------------------
   265	
   266	#[test]
   267	fn flow_return_substrate_serves_symbolic_call_return_complete() {
   268	    let host = make_host_with_footprint();
   269	    upsert_substrate_fixture(&host);
   270	    let (expr, record) = resolve_substrate_alias(&host, "SubCallReturn");
   271	    let props = object_props(&expr);
   272	    assert_eq!(prop_names(&props), vec!["ok"]);
   273	    assert_primitive(&props["ok"].ty, PrimitiveName::String);
   274	    assert_flow_return_dispatched(&record, "SubCallReturn");
   275	}
   276	
   277	/// The `FlowReturn` family owns the class-method demand even when the
   278	/// receiver is the in-flight class surface: the family is dispatched and
   279	/// the undecidable receiver surfaces as a typed miss, never admitted.
   280	///
   281	/// The class-surface-reentrant `this` call fails closed with the typed
   282	/// `unmodeledPosition` marker: `this` is not modeled as a receiver (the
   730	    let mut occurrences = std::collections::BTreeMap::new();
   731	    let mut inferred = Vec::with_capacity(candidates.len());
   732	    let mut value_dependencies = BTreeSet::new();
   733	    for candidate in candidates {
   734	        // A body-derived method return is demanded from the whole-function
   735	        // producer through the ONE sealed helper (the exact contributor /
   736	        // member / overload slot the extractor marked). Every decision —
   737	        // the declaration-safety gate and the `typeof`-root dependency walk
   738	        // — runs on NODE-DOMAIN facts before any materialization; the
   739	        // returned node materializes ONCE at the pure terminal display sink
   740	        // for its splice text. Every degraded shape (typed failure, empty
   741	        // cycle, absent identity) fails closed here.
   742	        if candidate.position == TscInferredClassTypePosition::Return {
   743	            let Some(mut identity) = candidate.flow_return else {
   744	                return Err(ClassInferenceFailure::Unsupported(
   745	                    UnsupportedReason::SemanticConstruct,
   746	                ));
   747	            };
   748	            identity.anchor.canonical_id = std::sync::Arc::from(owner_canonical);
   749	            identity.anchor.owner = owner;
   750	            let (type_text, typeof_paths) = match dispatch.execute_function_return_source(
   751	                &verter_type_expr::facts::FunctionReturnSource::Flow(*identity),
   752	                owner_canonical,
   753	            ) {
   754	                crate::project_semantic_dispatch::flow_return::FunctionReturnNode::Flow(result) => {
   755	                    // A DEGRADED SUCCESS never splices display text: this
   756	                    // projection is fail-closed (every degraded shape
   757	                    // refuses), and a modeled-`any` substitution is a
   758	                    // degraded shape even though it carries a usable value.
   759	                    if result.degradation().is_some() {
   760	                        return Err(ClassInferenceFailure::Unsupported(
     1	#![deny(missing_docs)]
     2	//! `VerterHost::get_flow_return_type_with_audit` — the single public
     3	//! audited entry-point for whole-function flow-return inference.
     4	//!
     5	//! Wires the standard audit lifecycle (registration constructed BEFORE
     6	//! the TLS observer install, producer body run under the matching
     7	//! guard, per-request counters snapshotted at finalize) around one
     8	//! `SemanticQueryKey::FlowReturn` demand routed through the shared
     9	//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch`] —
    10	//! never a second resolver.
    11	//!
    12	//! Outcome mapping (the locked design's split result/carrier contract,
    13	//! C1): a COMPLETE evaluation — including a DEGRADED SUCCESS carrying
    14	//! `FlowReturnResult::degradation` — rides the [`AuditedResult`] `Ok`
    15	//! arm; a genuine NO-VALUE outcome (the typed
    16	//! [`crate::semantic_query::FlowReturnFailure`] class) rides the `Err`
    17	//! arm as [`FlowReturnError::Failure`]. Both arms carry the audit
    18	//! record.
    61	impl VerterHost {
    62	    /// Resolve one whole-function flow return through the shared
    63	    /// dispatch and return the result — or a typed
    64	    /// [`FlowReturnError`] — alongside the per-request
    65	    /// [`RequestAuditRecord`], packaged in one [`AuditedResult`].
    66	    ///
    67	    /// `function` is the content-free served-function identity (the
    68	    /// declaration anchor plus part/overload ordinal); `demand` is the
    69	    /// return-projection point. Production's canonical point is
    70	    /// [`ReturnProjectionDemand::whole_return`]; any narrower point is
    71	    /// accepted as key data and currently fails CLOSED with the typed
    72	    /// `UnmodeledDemandPoint` failure (never a silently widened
    73	    /// whole-return result).
    74	    ///
    75	    /// The carrier's `audit` field is always populated: an active
    76	    /// registration carries the full `FlowReturnInference` payload
    77	    /// ([`verter_audit::AuditCaptureState::ActiveStored`]); a filtered
    78	    /// or disabled registration carries the cheap default-filled
    79	    /// record ([`verter_audit::AuditCaptureState::FilteredNoop`] /
    80	    /// [`verter_audit::AuditCaptureState::AuditDisabled`]).
    81	    #[must_use]
    82	    pub fn get_flow_return_type_with_audit(
    83	        &self,
    84	        function: &verter_type_expr::facts::FlowFunctionReturnIdentity,
    85	        demand: ReturnProjectionDemand,
    86	    ) -> AuditedResult<Arc<FlowReturnResult>, FlowReturnError> {
    87	        let canonical_id: &str = function.anchor.canonical_id.as_ref();
    88	        let function_symbol: &str = function.anchor.symbol.as_ref();

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"mod flow_slice_content|mod project_semantic_dispatch|pub mod resolver_core|mod resolver_store\" crates/verter_session/src/lib.rs; nl -ba crates/verter_session/src/lib.rs | sed -n '315,352p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 6280ms:[0m
189:pub(crate) mod flow_slice_content;
191:mod flow_slice_content_tests;
220:mod project_semantic_dispatch_invariants_tests;
332:pub(crate) mod project_semantic_dispatch;
341:pub mod resolver_core;
342:mod resolver_store;
344:mod resolver_store_tests;
   315	pub mod meta;
   316	#[cfg(all(test, not(target_arch = "wasm32")))]
   317	mod runtime_render_lane_tests;
   318	
   319	#[cfg(test)]
   320	mod artifact_root_retention_tests;
   321	pub mod meta_resolve;
   322	#[cfg(test)]
   323	mod negative_import_route_tests;
   324	pub mod owned_artifacts;
   325	pub mod owner_import_surface;
   326	#[cfg(test)]
   327	mod parity_tests;
   328	mod parse;
   329	mod parsed_eval_program;
   330	#[cfg(test)]
   331	mod project_global_cache_tests;
   332	pub(crate) mod project_semantic_dispatch;
   333	pub mod project_type_store;
   334	#[cfg(test)]
   335	mod project_type_store_tests;
   336	#[cfg(test)]
   337	mod public_api_batch_fixed_view_tests;
   338	pub mod query_host_port;
   339	mod request_budget;
   340	pub mod request_context;
   341	pub mod resolver_core;
   342	mod resolver_store;
   343	#[cfg(test)]
   344	mod resolver_store_tests;
   345	pub mod semantic_query;
   346	pub(crate) mod semantic_query_memo;
   347	pub(crate) mod session_runtime;
   348	pub mod session_view;
   349	mod shared;
   350	#[cfg(test)]
   351	mod source_root_retention_tests;
   352	#[cfg(test)]

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba crates/verter_session/src/lib.rs | sed -n '180,196p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 5439ms:[0m
   180	// implementation detail. The only externally-needed type is
   181	// `ReadSetSignature` — the return type of the public inspector
   182	// `compile_slot_fact_dep_signature` — selectively re-exported below.
   183	pub(crate) mod fact_signature_helpers;
   184	pub use crate::fact_signature_helpers::ReadSetSignature;
   185	#[cfg(test)]
   186	mod error_propagation_lattice_tests;
   187	pub mod external_ts;
   188	pub mod file_artifact_store;
   189	pub(crate) mod flow_slice_content;
   190	#[cfg(test)]
   191	mod flow_slice_content_tests;
   192	mod hash;
   193	pub(crate) mod instant;
   194	/// Session-side key identities for locator-backed body lowering
   195	/// (`LocatorLoweringKey` + the sealed R6 key-dimension witness +
   196	/// `SessionDemandIdentity`). Crate-private: the substrate is B1-internal;

[35m[3mcodex[0m[0m
AMD-001 is discharged in the tree at `f46de1b6a`: the commit adds the validator, shared composite model, both test suites, `test:scripts`, and CI path filters. Its discriminating cases cover valid checkpoint→acceptance plus missing window, mismatched snapshot, wrong acceptance block, and independently mergeable checkpoint. A local test invocation was prevented before execution by this read-only sandbox’s `mkdtemp` denial, so I’m relying on the landed source/commit evidence rather than claiming a fresh green run.
[3mThe governing documents and current code now resolve all six forks. Remaining work is to synthesize the rulings and state amendment impact precisely.[0m
  [32m✓[0m Read charter and governing Revision 11 program/ADR/contracts/evidence
  [32m✓[0m Trace C1 relocation, obligation runtime, and stable binding identities in code
  [32m✓[0m Verify A6 capability rows and AMD-001 validator/test delivery
  [36m→[0m Issue six rulings with amendment impact and exact citations
[35m[3mexec[0m[0m
[1m/bin/zsh -lc "rg -n \"D1.*D2|PRIVATE_CHECKPOINT.*final acceptance|canonical case\" docs/arch/refactor/rev11/contracts/stacked-prs.md; nl -ba docs/arch/refactor/rev11/contracts/stacked-prs.md | sed -n '90,145p'; nl -ba scripts/validate-stack-window.test.mjs | sed -n '250,305p'; nl -ba scripts/validate-program-state.mjs | sed -n '625,668p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 7330ms:[0m
53:- no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor, except an explicit program checkpoint such as D1 whose `PRIVATE_CHECKPOINT` state is valid only for the final acceptance block;
58:`D1`/`D2` is the canonical case: D1 is a private checkpoint; D2 is the sole acceptance and landing unit.
    90	Every mergeable layer runs:
    91	
    92	1. layer-specific tests/static checks;
    93	2. every charter check applicable to the cumulative tree from stack root through that layer;
    94	3. non-vacuous execution and generated-file cleanliness proof;
    95	4. relevant performance/memory/work gates;
    96	5. dependency, architecture, compatibility, and failure checks required by its block.
    97	
    98	The top `LANDABLE` layer additionally runs the declared stack-integration suite. The final `ATOMIC_REVIEW` layer runs full atomic-cutover proof, including one-production-path and deletion assertions.
    99	
   100	`LAND_READY` means all mergeable layers are green on the named immutable snapshot and the one currently eligible landing block is `ACCEPTANCE_RECOMMENDED`: the bottom layer for `LANDABLE`, or the final acceptance block for `ATOMIC_REVIEW`. Green upper `LANDABLE` layers remain `REVIEW`, not accepted in advance.
   101	
   102	Evidence from an older base, snapshot, toolchain, profile, or corpus is not silently reused.
   103	
   104	# 7. Lower-layer change and cascading restack
   105	
   106	When a lower layer changes:
   107	
   108	1. fix the layer where the defect belongs; never hide it in an upper workaround;
   109	2. restack bottom-to-top;
   110	3. record old/new base SHA/tree, canonical patch digest, range-diff, candidate tree, generated diff, evidence digest, and every manual conflict resolution;
   111	4. set `previous_stack_snapshot_digest` to the replaced snapshot and mint a new StackSnapshotId;
   112	5. mark all affected upper candidates/reviews `INVALIDATED`/revalidation-required;
   113	6. rerun required CI on every new cumulative tree;
   114	7. obtain impact-bounded reattestation from every required review mandate on the new exact candidate/snapshot.
   115	
   116	No approval transfers automatically. Tree/patch equivalence can make reattestation small, but the new exact identity must be named.
   117	
   118	# 8. Restructuring
   119	
   120	Inserting, dropping, folding, reordering, or unstacking a layer requires:
   121	
   122	- clean worktrees;
   123	- no affected layer queued/merging;
   124	- updated validated stack window and program state;
   125	- predecessor and mergeability revalidation;
   126	- regenerated PR descriptions/context packets where affected;
   127	- invalidation of changed cumulative candidates, CI, and reviews.
   128	
   129	A transport-only linear relation between true DAG siblings must not be created. Separate sibling stacks are required.
   130	
   131	# 9. Landing
   132	
   133	Legal modes:
   134	
   135	- **Bottom-up:** land only the lowest `LANDABLE` layer. Then invalidate/restack every remaining upper layer on the actual accepted base and issue a successor snapshot. If one layer remains, continue as an ordinary single PR.
   136	- **Atomic final only:** land only the final `ATOMIC_REVIEW` candidate; private layers never reach trunk independently.
   137	
   138	Before landing, record the reviewed base/candidate SHA/tree and the predicted landing base/target identity. Branch protection and required checks remain binding. A merge queue is preferred where available, but queue admission does not replace exact review.
   139	
   140	`candidate_sha/tree` remains the exact cumulative candidate reviewers inspected. `accepted_sha/tree` records the actual landed commit and full repository tree and may differ after a reviewed rebase, squash, merge commit, or merge-queue base advance. A validated `landing_equivalence_digest` proves that the canonical binary Git delta from reviewed base to reviewed candidate exactly equals the delta from accepted base to accepted commit, that generated-output digests match, that no manual conflict resolution occurred after review, and that required post-landing checks passed. If the delta differs, re-freeze and re-review; do not call it equivalent.
   141	
   142	A single accepted program block must not be co-batched with unrelated changes in the same landing delta. Foundational or atomic candidates receive a dedicated merge-group/queue position where the repository supports it.
   143	
   144	After landing:
   145	
   250	    .split("[[layer]]")[0]
   251	    .concat(
   252	      layer({ index: 1, layer_id: "D1", block_id: "D1", kind: d1Kind, mergeable: d1Kind === "mergeable", base_branch: "main", branch: "d1" }),
   253	      layer({ index: 2, layer_id: "D2", block_id: "D2", kind: d2Kind, mergeable: d2Kind === "mergeable", base_branch: "d1", branch: "d2" }),
   254	    );
   255	}
   256	
   257	test("ATOMIC_REVIEW: empty acceptance_block_id is rejected", () => {
   258	  const p = write("atomic-empty-acceptance.toml", atomicWindow({ acceptanceBlockId: "" }));
   259	  const r = run(["--window", p, "--mode", "live"]);
   260	  assert.notEqual(r.status, 0);
   261	  assert.match(r.err, /mode is ATOMIC_REVIEW but acceptance_block_id is empty/);
   262	});
   263	
   264	test("ATOMIC_REVIEW: zero mergeable layers is rejected", () => {
   265	  const p = write("atomic-zero-mergeable.toml", atomicWindow({ d2Kind: "NON_MERGEABLE_PRIVATE_LAYER" }));
   266	  const r = run(["--window", p, "--mode", "live"]);
   267	  assert.notEqual(r.status, 0);
   268	  assert.match(r.err, /found 0 mergeable layer\(s\)/);
   269	});
   270	
   271	test("ATOMIC_REVIEW: two mergeable layers is rejected", () => {
   272	  const p = write("atomic-two-mergeable.toml", atomicWindow({ d1Kind: "mergeable" }));
   273	  const r = run(["--window", p, "--mode", "live"]);
   274	  assert.notEqual(r.status, 0);
   275	  assert.match(r.err, /found 2 mergeable layer\(s\)/);
   276	  assert.match(r.err, /layer "D1".*kind is "mergeable", not NON_MERGEABLE_PRIVATE_LAYER/);
   277	});
   278	
   279	test("ATOMIC_REVIEW: a valid D1/D2 window passes structurally", () => {
   280	  const p = write("atomic-ok.toml", atomicWindow());
   281	  const r = run(["--window", p, "--mode", "live"]);
   282	  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
   283	});
   284	
   285	test("ATOMIC_REVIEW with --dag: a private layer naming a non-checkpoint block is rejected", () => {
   286	  const dag = write(
   287	    "dag.toml",
   288	    `schema = 1\nrevision = 11\n\n[[block]]\nid = "D1"\nclass = "foundational"\npredecessors = []\n\n[[block]]\nid = "D2"\nclass = "foundational"\npredecessors = ["D1"]\n`,
   289	  );
   290	  const p = write("atomic-bad-class.toml", atomicWindow());
   291	  const r = run(["--window", p, "--mode", "live", "--dag", dag]);
   292	  assert.notEqual(r.status, 0);
   293	  assert.match(r.err, /DAG class is "foundational" — a private ATOMIC_REVIEW layer must repeat the acceptance block's own id or name a block whose DAG class is "foundational-private-checkpoint"/);
   294	});
   295	
   296	test("ATOMIC_REVIEW with --dag: a private layer naming a foundational-private-checkpoint block passes", () => {
   297	  const dag = write(
   298	    "dag-ok.toml",
   299	    `schema = 1\nrevision = 11\n\n[[block]]\nid = "D1"\nclass = "foundational-private-checkpoint"\npredecessors = []\n\n[[block]]\nid = "D2"\nclass = "foundational"\npredecessors = ["D1"]\n`,
   300	  );
   301	  const p = write("atomic-good-class.toml", atomicWindow());
   302	  const r = run(["--window", p, "--mode", "live", "--dag", dag]);
   303	  assert.equal(r.status, 0, `expected pass, got:\n${r.err}`);
   304	});
   305	
   625	  // accepted, except contingent ... work ... in the same validated immutable
   626	  // stack snapshot. Such work cannot be acceptance-recommended or accepted
   627	  // until the predecessor lands."
   628	  for (const [id, b] of stateById) {
   629	    if (!BEGUN_STATUSES.has(b.status)) continue;
   630	    const dagBlock = dagById.get(id);
   631	    if (!dagBlock) continue; // already reported as extra
   632	
   633	    // (a) a PRIVATE_CHECKPOINT predecessor. contracts/stacked-prs.md:39,53 let a
   634	    //     PRIVATE_CHECKPOINT predecessor satisfy sequencing only inside a
   635	    //     validated stack window and only for the final acceptance block.
   636	    //     AMD-001 §3: this refusal is SUPERSEDED (never simply deleted) by
   637	    //     the composite stack-window cross-validation when the caller passes
   638	    //     --stack-window — evaluateCheckpointException (scripts/lib/
   639	    //     stack-window-lib.mjs) is the SOLE model of that exception, shared
   640	    //     with scripts/validate-stack-window.mjs, so this validator never
   641	    //     grows a second, parallel notion of the same question. With no
   642	    //     --stack-window given, the original fail-closed refusal stands
   643	    //     unchanged.
   644	    for (const p of dagBlock.predecessors ?? []) {
   645	      if (stateById.get(p)?.status !== "PRIVATE_CHECKPOINT") continue;
   646	      const stackWindowPath = opts["stack-window"];
   647	      if (!stackWindowPath) {
   648	        v(
   649	          `block ${id} is ${b.status} with predecessor ${p} in PRIVATE_CHECKPOINT — a PRIVATE_CHECKPOINT predecessor satisfies sequencing only inside a validated stack window for the final acceptance block (contracts/stacked-prs.md), which this validator does not model — fail closed`,
   650	        );
   651	        continue;
   652	      }
   653	      const result = evaluateCheckpointException({
   654	        windowPath: stackWindowPath,
   655	        predecessorId: p,
   656	        successorId: id,
   657	        stateById,
   658	        dagById,
   659	      });
   660	      if (!result.ok) {
   661	        v(
   662	          `block ${id} is ${b.status} with predecessor ${p} in PRIVATE_CHECKPOINT — composite stack-window validation via --stack-window ${stackWindowPath} did not establish the checkpoint exception (AMD-001 §2): ${result.problems.join("; ")}`,
   663	        );
   664	      }
   665	    }
   666	    // (b) an OPENED conditional predecessor (program-dag.toml:
   667	    //     conditional_predecessor_if_opened — "If opened, it becomes an
   668	    //     additional predecessor"). LOCKED = never opened (no dependency);

[35m[3mexec[0m[0m
[1m/bin/zsh -lc "nl -ba docs/arch/refactor/rev11/contracts/stacked-prs.md | sed -n '42,62p'"[0m in <MACHINE_ROOT>/verter
[32m succeeded in 4515ms:[0m
    42	
    43	## 3.2 `ATOMIC_REVIEW`
    44	
    45	Use when several review-sized diffs collectively form one indivisible clean cutover.
    46	
    47	Rules:
    48	
    49	- top-level `acceptance_block_id` names the sole program block that may become accepted/landed from this window;
    50	- all layers belong to that atomic acceptance unit and have unique `layer_id` values;
    51	- private layers may repeat the acceptance block's `block_id` as internal checkpoints, or name an explicit `foundational-private-checkpoint` predecessor such as `D1`;
    52	- intermediate layers target a private integration branch, remain draft, are marked `NON_MERGEABLE_PRIVATE_LAYER`, and are unreachable from production entry points;
    53	- no intermediate layer is released, merged to trunk, or recorded as an accepted program predecessor, except an explicit program checkpoint such as D1 whose `PRIVATE_CHECKPOINT` state is valid only for the final acceptance block;
    54	- exactly one final mergeable layer routes every consumer, deletes the displaced path/support machinery, and becomes the reviewed candidate;
    55	- the complete combined tip receives the block's required conformance, architecture, and adversarial/performance review;
    56	- landing preserves the exact reviewed candidate delta on the recorded landing base.
    57	
    58	`D1`/`D2` is the canonical case: D1 is a private checkpoint; D2 is the sole acceptance and landing unit.
    59	
    60	## 3.3 Parallel disjoint work
    61	
    62	Parallelism is represented by separate `LANDABLE` or `ATOMIC_REVIEW` windows, not one artificial stack. Each window declares shared owners/files/generated artifacts and integration tests. A newly discovered ownership overlap stops or serializes the affected work.

[3mAll six rulings are grounded in the current tip, governing documents, code boundaries, and the landed AMD-001 implementation.[0m
  [32m✓[0m Read charter and governing Revision 11 program/ADR/contracts/evidence
  [32m✓[0m Trace C1 relocation, obligation runtime, and stable binding identities in code
  [32m✓[0m Verify A6 capability rows and AMD-001 validator/test delivery
  [32m✓[0m Issue six rulings with amendment impact and exact citations
[35m[3mcodex[0m[0m
1. **Capability matrix: D1 starts against an unratified row target.** All eight `Status` cells remain `VERIFY`; such rows cannot support architecture/default claims ([capability-matrix.md:4](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:4), [capability-matrix.md:15](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:15), [capability-matrix.md:75](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:75)). Therefore D1’s substantive Supported/Stable row set is empty today; its structural requirements still bind. U-1 requires ratification before the **atomic flow-cutover block**, which is D2—not before D1 starts ([implementation-lock-record.md:328](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:328), [implementation-lock-record.md:863](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:863), [program.md:231](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program.md:231), [program.md:235](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program.md:235)). The draft’s implication that U-1 may block D1 `BLOCK_READY` is wrong.

   Impact — ADR: no; DAG: no; program outcome: no. No program amendment; product/conformance matrix ratification remains mandatory before D2.

2. **C1 moves `flow_return.rs`, `flow_return_callee.rs`, and `dispatch_txn.rs`.** C1 orders physical relocation of `project_semantic_dispatch` as a module ([C1.md:183](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:183), [C1.md:250](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:250)). Those files are declared children of that module ([mod.rs:109](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/mod.rs:109)). C1’s named carve-outs are three `resolver_core` lifecycle-adapter pieces, not flow files ([C1.md:253](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:253)). “C1 does not own flow semantics” means relocate unchanged, not leave behind ([C1.md:352](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:352)). Conversely, `flow_slice_content.rs` is a sibling session module and does not move under C1 ([lib.rs:189](<MACHINE_ROOT>/verter/crates/verter_session/src/lib.rs:189)). The draft’s claimed boundary gap is not real.

   Impact — ADR: no; DAG: no; program outcome: no. No AMD; make the three filenames explicit when ratifying C1’s charter.

3. **D1 must extend the existing `ObligationRuntime`; it must not create a peer obligation authority.** The code explicitly defines one tagged runtime shared across relation, flow-return, and call domains so cycle spaces cannot diverge ([dispatch_txn.rs:1](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1), [dispatch_txn.rs:30](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:30), [dispatch_txn.rs:211](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:211)). `ObligationRuntime` owns generic topology/pending/substitution state, while one transaction owns it plus typed domain payloads ([dispatch_txn.rs:874](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:874), [dispatch_txn.rs:1890](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1890)). D1 may add the contract’s plan-origin/domain/dependency/discharge-evidence representation, but subordinate to that one runtime—not as a separately constructed ledger ([flow-completeness.md:40](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/flow-completeness.md:40), [architecture.md:765](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/architecture.md:765)). Consequently, the draft’s demand for zero diff in `dispatch_txn.rs` is wrong; legacy behavior stays unchanged, but the shared mechanism must evolve.

   Impact — ADR: no; DAG: no; program outcome: no. No amendment.

4. **The effective-flow rows cannot presently be enumerated from A6.** The matrix has no flow-capability discriminator or explicit `FlowReturn` operation—only eight broad product rows ([capability-matrix.md:13](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:13)). Inferring only `TypeInfo / TypeAtPosition` would be unsound: that row visibly references flow ([capability-matrix.md:19](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:19)), but Vue macro projection also directly demands body-derived `FlowReturn` ([tsc_projection.rs:734](<MACHINE_ROOT>/verter/crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:734)), and the public whole-function flow-return operation has no matrix row at all ([host_flow_return_audit.rs:2](<MACHINE_ROOT>/verter/crates/verter_session/src/host_flow_return_audit.rs:2), [host_flow_return_audit.rs:82](<MACHINE_ROOT>/verter/crates/verter_session/src/host_flow_return_audit.rs:82)). Therefore the enforceable Supported/Stable effective-flow set is empty, while the underlying flow-bearing operation set is undefined. Add explicit flow rows or an explicit row-to-flow-subcapability mapping before ratification.

   Impact — ADR: no; DAG: no; program outcome: no—the existing outcome is being made enforceable. Formal product/conformance matrix revision and ratification: yes; program AMD: no.

5. **Stable binding slots are the existing identity mechanism, not a new abstraction.** Within one body, `SkeletonBindingId` is the lexical binding slot ([flow/mod.rs:75](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/mod.rs:75)); the canonical graph maps it deterministically to a binding node ([flow_graph.rs:71](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/flow_graph.rs:71), [flow_graph.rs:182](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/flow_graph.rs:182)). Cross-frame identity already exists as `(defining_function, binding_slot)` in `FlowBindingIdentity` ([function_program.rs:265](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/function_program.rs:265)). `FlowNodeId` is merely a dense graph-local node index, not the binding identity itself ([flow_graph.rs:54](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/flow_graph.rs:54)). Thus D1 keys state by graph/body identity plus `SkeletonBindingId`, using `FlowBindingIdentity` across frames; D3 completes coverage and domain use. Minting a parallel slot type is forbidden.

   Impact — ADR: no; DAG: no; program outcome: no. No amendment.

6. **AMD-001 never gated D1 implementation start; it gated the first stack window and D1 entering `PRIVATE_CHECKPOINT`, and that prerequisite is now discharged.** That is AMD-001’s exact timing ([AMD-001:43](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md:43), [maintainer-rulings.md:170](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/evidence/maintainer-rulings.md:170)). Commit `f46de1b6a` delivered all four artifacts: composite validation ([stack-window-lib.mjs:281](<MACHINE_ROOT>/verter/scripts/lib/stack-window-lib.mjs:281)), the D1/D2 structural transition tests ([validate-stack-window.test.mjs:257](<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:257)), script registration ([package.json:26](<MACHINE_ROOT>/verter/package.json:26)), and CI path wiring ([ci.yml:60](<MACHINE_ROOT>/verter/.github/workflows/ci.yml:60)). The draft’s “machinery does not exist” statement at `D1-CHARTER-DRAFT.md:80-84` is plainly stale. D2 still requires an actual validated `ATOMIC_REVIEW` snapshot with D1 private and D2 as sole acceptance block ([stacked-prs.md:49](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/stacked-prs.md:49)). Fresh test execution was sandbox-blocked at temporary-directory creation, so this is landed-source verification, not a new green-run claim.

   Impact — ADR: no; DAG: no; program outcome: no. AMD-001 already records the governing change; no further amendment is required.

Fork 1 — **UNRATIFIED TARGET; D1 MAY START, D2 MAY NOT START BEFORE MATRIX RATIFICATION.**  
Fork 2 — **C1 MOVES `flow_return.rs`, `flow_return_callee.rs`, AND `dispatch_txn.rs`; NOT `flow_slice_content.rs`.**  
Fork 3 — **SAME OBLIGATION AUTHORITY: EXTEND `ObligationRuntime`; NO PEER LEDGER.**  
Fork 4 — **NOT ENUMERABLE FROM CURRENT A6; ADD EXPLICIT FLOW ROWS/MAPPING.**  
Fork 5 — **REUSE EXISTING BINDING-SLOT IDENTITIES; DO NOT MINT A PARALLEL ABSTRACTION.**  
Fork 6 — **NOT A D1-START GATE; PRE-CHECKPOINT PREREQUISITE IS DELIVERED AND DISCHARGED.**
[2mtokens used[0m
256 897
1. **Capability matrix: D1 starts against an unratified row target.** All eight `Status` cells remain `VERIFY`; such rows cannot support architecture/default claims ([capability-matrix.md:4](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:4), [capability-matrix.md:15](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:15), [capability-matrix.md:75](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:75)). Therefore D1’s substantive Supported/Stable row set is empty today; its structural requirements still bind. U-1 requires ratification before the **atomic flow-cutover block**, which is D2—not before D1 starts ([implementation-lock-record.md:328](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:328), [implementation-lock-record.md:863](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md:863), [program.md:231](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program.md:231), [program.md:235](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/program.md:235)). The draft’s implication that U-1 may block D1 `BLOCK_READY` is wrong.

   Impact — ADR: no; DAG: no; program outcome: no. No program amendment; product/conformance matrix ratification remains mandatory before D2.

2. **C1 moves `flow_return.rs`, `flow_return_callee.rs`, and `dispatch_txn.rs`.** C1 orders physical relocation of `project_semantic_dispatch` as a module ([C1.md:183](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:183), [C1.md:250](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:250)). Those files are declared children of that module ([mod.rs:109](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/mod.rs:109)). C1’s named carve-outs are three `resolver_core` lifecycle-adapter pieces, not flow files ([C1.md:253](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:253)). “C1 does not own flow semantics” means relocate unchanged, not leave behind ([C1.md:352](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/charters/C1.md:352)). Conversely, `flow_slice_content.rs` is a sibling session module and does not move under C1 ([lib.rs:189](<MACHINE_ROOT>/verter/crates/verter_session/src/lib.rs:189)). The draft’s claimed boundary gap is not real.

   Impact — ADR: no; DAG: no; program outcome: no. No AMD; make the three filenames explicit when ratifying C1’s charter.

3. **D1 must extend the existing `ObligationRuntime`; it must not create a peer obligation authority.** The code explicitly defines one tagged runtime shared across relation, flow-return, and call domains so cycle spaces cannot diverge ([dispatch_txn.rs:1](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1), [dispatch_txn.rs:30](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:30), [dispatch_txn.rs:211](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:211)). `ObligationRuntime` owns generic topology/pending/substitution state, while one transaction owns it plus typed domain payloads ([dispatch_txn.rs:874](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:874), [dispatch_txn.rs:1890](<MACHINE_ROOT>/verter/crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:1890)). D1 may add the contract’s plan-origin/domain/dependency/discharge-evidence representation, but subordinate to that one runtime—not as a separately constructed ledger ([flow-completeness.md:40](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/flow-completeness.md:40), [architecture.md:765](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/architecture.md:765)). Consequently, the draft’s demand for zero diff in `dispatch_txn.rs` is wrong; legacy behavior stays unchanged, but the shared mechanism must evolve.

   Impact — ADR: no; DAG: no; program outcome: no. No amendment.

4. **The effective-flow rows cannot presently be enumerated from A6.** The matrix has no flow-capability discriminator or explicit `FlowReturn` operation—only eight broad product rows ([capability-matrix.md:13](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:13)). Inferring only `TypeInfo / TypeAtPosition` would be unsound: that row visibly references flow ([capability-matrix.md:19](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/capability-matrix.md:19)), but Vue macro projection also directly demands body-derived `FlowReturn` ([tsc_projection.rs:734](<MACHINE_ROOT>/verter/crates/verter_session/src/typeinfo/vue_macro_codegen/tsc_projection.rs:734)), and the public whole-function flow-return operation has no matrix row at all ([host_flow_return_audit.rs:2](<MACHINE_ROOT>/verter/crates/verter_session/src/host_flow_return_audit.rs:2), [host_flow_return_audit.rs:82](<MACHINE_ROOT>/verter/crates/verter_session/src/host_flow_return_audit.rs:82)). Therefore the enforceable Supported/Stable effective-flow set is empty, while the underlying flow-bearing operation set is undefined. Add explicit flow rows or an explicit row-to-flow-subcapability mapping before ratification.

   Impact — ADR: no; DAG: no; program outcome: no—the existing outcome is being made enforceable. Formal product/conformance matrix revision and ratification: yes; program AMD: no.

5. **Stable binding slots are the existing identity mechanism, not a new abstraction.** Within one body, `SkeletonBindingId` is the lexical binding slot ([flow/mod.rs:75](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/mod.rs:75)); the canonical graph maps it deterministically to a binding node ([flow_graph.rs:71](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/flow_graph.rs:71), [flow_graph.rs:182](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/flow_graph.rs:182)). Cross-frame identity already exists as `(defining_function, binding_slot)` in `FlowBindingIdentity` ([function_program.rs:265](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/function_program.rs:265)). `FlowNodeId` is merely a dense graph-local node index, not the binding identity itself ([flow_graph.rs:54](<MACHINE_ROOT>/verter/crates/verter_semantic/src/analysis/flow/flow_graph.rs:54)). Thus D1 keys state by graph/body identity plus `SkeletonBindingId`, using `FlowBindingIdentity` across frames; D3 completes coverage and domain use. Minting a parallel slot type is forbidden.

   Impact — ADR: no; DAG: no; program outcome: no. No amendment.

6. **AMD-001 never gated D1 implementation start; it gated the first stack window and D1 entering `PRIVATE_CHECKPOINT`, and that prerequisite is now discharged.** That is AMD-001’s exact timing ([AMD-001:43](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md:43), [maintainer-rulings.md:170](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/evidence/maintainer-rulings.md:170)). Commit `f46de1b6a` delivered all four artifacts: composite validation ([stack-window-lib.mjs:281](<MACHINE_ROOT>/verter/scripts/lib/stack-window-lib.mjs:281)), the D1/D2 structural transition tests ([validate-stack-window.test.mjs:257](<MACHINE_ROOT>/verter/scripts/validate-stack-window.test.mjs:257)), script registration ([package.json:26](<MACHINE_ROOT>/verter/package.json:26)), and CI path wiring ([ci.yml:60](<MACHINE_ROOT>/verter/.github/workflows/ci.yml:60)). The draft’s “machinery does not exist” statement at `D1-CHARTER-DRAFT.md:80-84` is plainly stale. D2 still requires an actual validated `ATOMIC_REVIEW` snapshot with D1 private and D2 as sole acceptance block ([stacked-prs.md:49](<MACHINE_ROOT>/verter/docs/arch/refactor/rev11/contracts/stacked-prs.md:49)). Fresh test execution was sandbox-blocked at temporary-directory creation, so this is landed-source verification, not a new green-run claim.

   Impact — ADR: no; DAG: no; program outcome: no. AMD-001 already records the governing change; no further amendment is required.

Fork 1 — **UNRATIFIED TARGET; D1 MAY START, D2 MAY NOT START BEFORE MATRIX RATIFICATION.**  
Fork 2 — **C1 MOVES `flow_return.rs`, `flow_return_callee.rs`, AND `dispatch_txn.rs`; NOT `flow_slice_content.rs`.**  
Fork 3 — **SAME OBLIGATION AUTHORITY: EXTEND `ObligationRuntime`; NO PEER LEDGER.**  
Fork 4 — **NOT ENUMERABLE FROM CURRENT A6; ADD EXPLICIT FLOW ROWS/MAPPING.**  
Fork 5 — **REUSE EXISTING BINDING-SLOT IDENTITIES; DO NOT MINT A PARALLEL ABSTRACTION.**  
Fork 6 — **NOT A D1-START GATE; PRE-CHECKPOINT PREREQUISITE IS DELIVERED AND DISCHARGED.**
