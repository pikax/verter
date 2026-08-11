Verdict: **APPROVE WITH REQUIRED ATOMIC AMENDMENT.** Insert `A2C` between `A2` and `A3`. Do not land A3 with a reduced exit criterion. The strongest counter-argument is that an early completion model could become a second flow authority; the design below prevents that by making it a content-free `FunctionBodySkeleton` fact family that D6 must later consume—not an evaluator, graph, or closure implementation.

## 1. Block identity and DAG

Identity:

- **ID:** `A2C`
- **Name:** `Abrupt-completion facts for G10 safety discrimination`
- **Class:** `foundational-safety`
- **Position:** directly after `A2`, directly before `A3`
- **Total blocks after amendment:** 51; no existing ID is renumbered.

In [program-dag.toml](<REPO>/docs/arch/refactor/rev11/program-dag.toml:22), insert after the `A2` stanza:

```toml
[[block]]
id = "A2C"
name = "Abrupt-completion facts for G10 safety discrimination"
class = "foundational-safety"
predecessors = ["A2"]
```

Replace the A3 predecessor:

```toml
predecessors = ["A2"]
```

with:

```toml
predecessors = ["A2C"]
```

The resulting lineage is:

```text
A0 → A1 → A2 → A2C → A3 → A4 → A5 → A6
```

This remains:

- acyclic: the new edges point strictly from `A2` to `A2C` to `A3`;
- single-rooted: `A0` remains the sole block with no predecessors;
- fully reachable: every previous `A3` descendant transitively receives `A2C`;
- unchanged elsewhere: **no other block’s predecessor list changes**. In particular, `A4` and `D1` remain direct successors of `A3`.

## 2. Exact `charters/A2C.md`

Create `docs/arch/refactor/rev11/charters/A2C.md` with exactly:

```markdown
# A2C — Abrupt-completion facts for G10 safety discrimination

**Status:** PREPARED; begins only after A2 acceptance and ratification of AMD-002.  
**Class:** Foundational safety.  
**Predecessors:** A2.  
**Gate 0 lineage SHA:** `UNSET`; record the exact accepted A2-based candidate for this evidence block.

## Objective

Provide one content-free, exact-or-typed-unknown completion model sufficient for A3 to distinguish G10 from checker-correct clean results, without changing public semantic results or implementing the later sole flow solver.

## In scope

- canonical completion kinds `Normal`, `Return`, `Throw`, `Break(target)`, and `Continue(target)`;
- compositional completion-set transformation for blocks, `if`, labels, `switch`, loops, `try`, `catch`, and `finally`, limited to syntax-complete facts and typed unknown where final loop/flow semantics are required;
- structural authored-return membership and exact endpoint-`undefined` disposition;
- compact arena-free completion facts stored on `FunctionBodySkeleton`, computed once during skeleton construction and reusable without a query-time AST rewalk;
- an exact statement/suffix fact permitting A3 to identify the G10 abrupt-completion hazard without another syntax allowlist;
- discriminating fact-level tests for G10, labeled/switch/catch siblings, and the named checker-correct controls X68, X80, and X88.

## Out of scope

- public result retraction or any other semantic behavior change; A3 owns that behavior;
- closure reads/writes, capture summaries, escape/freshness analysis, or position-independent effect transfer; D5 owns those mechanisms;
- closure-escape, loop-summary, or `try`/`finally`-override graph edges, loop fixed points, slot transfer, or flow-state joins; D6 and `U6.LOOP_CLOSURE` own those mechanisms;
- proof-carrying complete-result construction, final obligation discharge, or warm-admission closure; D8 owns those mechanisms;
- a second syntax-shaped evaluator, a second control graph, cache admission, compatibility repair, or speculative services.

## Required evidence

Exact completion-set tests and pinned-checker discrimination for G10 plus labeled/switch/catch siblings; X68/X80/X88 remain exact clean controls; missing or unsupported facts produce typed unknown and never a guessed exact fact; construction is deterministic and linear, facts are `NoTypeExpr`, retained size is measured, and no query-time AST rewalk occurs; mutation recipes independently break label routing, throw-to-catch routing, and `finally` override and make the named tests fail.

## Abort/rescope

Stop if G10 discrimination requires value typing, capture/effect transfer, loop fixed-point state, graph-edge ownership, a second flow representation, or a public semantic change. Stop if a completion fact cannot be exact and fail-closed without guessing. Amend the charter rather than absorbing D5, D6, D8, or `U6.LOOP_CLOSURE`.

## Review

Exact-SHA conformance, architecture, and adversarial/performance mandates apply according to `governance.md`. `A2C` is accepted only when its evidence is attached to one unchanged candidate/evidence SHA and proves both exact G10 discrimination and non-interference with public results.
```

## 3. Required completion-model facts and placements

The current helper at [flow_slice_content.rs:1716](<REPO>/crates/verter_session/src/flow_slice_content.rs:1716) recognizes only:

- `return`;
- a block containing any recursively recognized statement;
- `if` with both arms recursively recognized.

It cannot represent:

- distinct abrupt channels;
- labeled target routing;
- switch-break absorption and fallthrough;
- throw-to-catch routing;
- `finally` replacement of pending completions;
- the distinction between structural return membership and endpoint `undefined`;
- whether an unsupported conclusion is exact or guessed.

Its boolean answer irreversibly conflates “does not guarantee return,” “completes normally,” “throws,” “breaks to a target,” and “completion is unmodelled.” Adding more match arms would remain a second hand-maintained suffix classifier and is rejected.

### Required types

Create `crates/verter_semantic/src/analysis/flow/completion.rs` beginning at line 1, containing these public content-free concepts:

```rust
pub enum CompletionTarget {
    Unlabeled,
    Label(FlowNameId),
}

pub enum CompletionKind {
    Normal,
    Return,
    Throw,
    Break(CompletionTarget),
    Continue(CompletionTarget),
}

pub struct CompletionSet {
    // Canonically ordered and deduplicated CompletionKind values.
}

pub enum EndpointDisposition {
    ExcludesImplicitUndefined,
    IncludesImplicitUndefined,
}

pub enum CompletionPrecision {
    Exact,
    Unknown(CompletionUnknownReason),
}

pub struct CompletionSummary {
    pub completions: CompletionSet,
    pub endpoint: EndpointDisposition,
    pub precision: CompletionPrecision,
}

pub struct StatementCompletionFact {
    pub span: FrameSpan,
    pub summary: CompletionSummary,
    pub contains_authored_return: bool,
    pub suffix_contains_authored_return: bool,
    pub authored_return_ordinal_start: u32,
    pub authored_return_ordinal_end: u32,
}

pub struct FunctionCompletionFacts {
    pub body: CompletionSummary,
    pub statements: Arc<[StatementCompletionFact]>,
}
```

All types must derive or satisfy `NoTypeExpr`. `CompletionSet` must have canonical ordering and deduplication; target-bearing values must not depend on allocation order, absolute offsets, or hash-map iteration.

### Exact placements

In [analysis/flow/mod.rs](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:41):

```rust
pub mod completion;
```

Re-export the fact types immediately after the module declarations.

In `FunctionBodySkeleton`, after `return_sites` at [mod.rs:568](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:568), add:

```rust
/// Canonical content-free completion facts for this function.
pub completion: FunctionCompletionFacts,
```

In its `impl`, after `return_site` around [mod.rs:617](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:617), add a span-keyed lookup:

```rust
pub fn statement_completion(
    &self,
    span: FrameSpan,
) -> Option<&StatementCompletionFact>;
```

The statement facts must be sorted canonically by `FrameSpan`; lookup may be binary-search-based. A missing fact is typed unknown to A3, never legacy fallback.

Populate the facts in the same skeleton construction rooted at [mod.rs:880](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:880). Refactor the statement-list processing at [mod.rs:1638](<REPO>/crates/verter_semantic/src/analysis/flow/mod.rs:1638) so completion composition occurs once per statement list. Do not introduce a query-time traversal from `flow_slice_content.rs`.

### Exact detector inputs

At the A3 decision point in `FlowSliceContentLowerer::lower_region`, presently beginning at [flow_slice_content.rs:3039](<REPO>/crates/verter_session/src/flow_slice_content.rs:3039), the detector must receive:

1. The exact completion set of the current statement.
2. Whether that statement structurally contains authored returns.
3. Whether its suffix structurally contains authored returns.
4. The ordinal interval of authored returns that must remain represented.
5. The function-body endpoint `undefined` disposition.
6. An exact/unknown discriminator.

A3 may emit `FlowGap::AbruptCompletion` when those accepted facts prove the legacy lowering cannot represent the required structural-return/endpoint combination. `Unknown` must also fail closed; it may not be converted to a clean result.

In A3, delete `statement_guarantees_current_function_return` and replace both callers at baseline lines 3050–3052 with `StatementCompletionFact`. Do not add a replacement `guarantees_*` boolean.

## 4. Ownership boundary

`A2C` owns only:

- the syntax-compositional completion vocabulary;
- canonical completion-set composition;
- authored-return membership;
- endpoint-`undefined` disposition;
- exact/typed-unknown skeleton facts;
- fact-level discrimination evidence.

It does **not** own:

- D5: capture identities, direct/transitive reads or writes, escape/freshness summaries, invalidating writes, or position-independent effect application;
- D6 / `U6.LOOP_CLOSURE`: graph edge emission, `try`/`finally` override edges, loop summaries, loop fixed points, break/continue state joins, or closure escape;
- D8: obligation-ledger closure, `CompleteFlowResult`, final admission proof, and structural prevention of wrong-and-warm publication.

D6 must consume A2C’s fact types as its structural completion input. It may enrich them with graph/state obligations but must not create a second completion classifier.

## 5. Plan knock-on edits

### `program.md`

At [program.md:15](<REPO>/docs/arch/refactor/rev11/program.md:15), replace the sentence with:

```markdown
Before `A6`, only Gate 0 work is legal. `A2C` may add only content-free completion facts and must not change public semantic results. `A3` may change behavior solely to retract a known wrong-complete result to a typed non-admissible outcome. Neither block may choose a disputed final flow owner.
```

After the A2 section ending at current line 83, insert:

```markdown
## A2C — Abrupt-completion facts for G10 safety discrimination

**Predecessors:** `A2`.

Add one content-free, exact-or-typed-unknown completion model to `FunctionBodySkeleton`: canonical completion sets for normal, return, throw, labeled/unlabeled break and continue; compositional label/switch/loop/try/catch/finally transformation; structural authored-return membership; and endpoint-`undefined` disposition. It changes no public result, adds no graph edge or flow-state solver, and supplies the sole facts A3 may use to discriminate G10.

**Exit:** G10 and its labeled/switch/catch siblings are distinguished from X68/X80/X88 by accepted skeleton facts; missing knowledge is typed unknown; public cold/warm results are unchanged; no query-time AST rewalk, second completion classifier, capture summary, effect transfer, graph edge, or fixed point is introduced.
```

Replace A3’s predecessor with:

```markdown
**Predecessors:** `A2C`.
```

Replace A3’s exit text with:

```markdown
**Exit:** every A2-catalogued known wrong-and-warm result, including G10 as discriminated by accepted A2C completion facts, returns typed `Partial`, `FlowGap`, or `NoValue` and is refused warm admission; authored `any` and the named checker-correct clean controls remain complete and warm.
```

Replace D6’s body at [program.md:263](<REPO>/docs/arch/refactor/rev11/program.md:263) with:

```markdown
Consume A2C's content-free completion facts as the sole structural completion classifier; integrate them into the sole graph solver with deterministic selected-frontier loop convergence, loop-summary and `try`/`finally`-override edges, and state routing for labels, switch, loops, try/catch/finally, return, throw, break, and continue. Do not rebuild or fork the completion classifier.
```

### `charters/A3.md`

Replace lines 3–5 with:

```markdown
**Status:** PREPARED; non-G10 implementation exists but is not landable until A2C is accepted and the candidate is restacked.  
**Class:** Foundational safety.  
**Predecessors:** A2C.  
```

Replace the Objective with:

```markdown
Replace every A2-catalogued fabricated complete result, including G10 as discriminated solely by accepted A2C completion facts, with a typed non-admissible outcome without selecting the final flow owner.
```

Add under “In scope”:

```markdown
- consumption of accepted `A2C` completion facts to classify G10; A3 may not add or reinterpret completion rules;
```

Replace “Required evidence” with:

```markdown
Cold/warm public tests for every A2-catalogued gap including G10; exact G10 `FlowGap::AbruptCompletion`; no semantic-any fallback; no warm admission; authored-`any` controls and X68/X80/X88 remain complete and warm; mutation recipes prove removing the A2C fact consumption restores the G10 wrong-and-warm result.
```

Insert before “Abort/rescope”:

```markdown
## Exit criterion

Every A2-catalogued known wrong-and-warm result, including G10 as discriminated by accepted A2C completion facts, returns typed `Partial`, `FlowGap`, or `NoValue` and is refused warm admission; authored `any` and the named checker-correct clean controls remain complete and warm.
```

### Program-state template and live ledger

In [program-state.template.toml](<REPO>/docs/arch/refactor/rev11/templates/program-state.template.toml:115), insert a complete block row identical to A3’s current row except:

```toml
id = "A2C"
status = "LOCKED"
```

Insert it before A3. The external live `program-state.toml` must receive the same row, and its DAG/package digests must be recomputed. The validator requires the state block-ID set to equal the DAG exactly.

### Ownership documents

In `docs/arch/u6-flow-return-gaps-and-target.md`, replace the G10 ownership row with:

```markdown
| G10 | `A2C` for Gate 0 discrimination; `D6` / `U6.LOOP_CLOSURE` for final semantics | accepted content-free completion facts for A3; later `try`/`finally`-override graph edges and completion-state routing |
```

After §4.3, add:

```markdown
Revision 11 staging: `A2C` pulls forward only the content-free completion fact algebra needed for Gate 0 G10 discrimination. `D6` / `U6.LOOP_CLOSURE` consumes that same algebra and remains the owner of graph edges, loop fixed points, state routing, and final clean semantics. No second classifier is permitted.
```

In `docs/arch/native-flow-return.md` §`U6.LOOP_CLOSURE`, add to “Changes”:

```markdown
- Consume the accepted `A2C` completion fact types as the sole structural classifier. This block adds graph edges, fixed points, and state transfer; it does not reconstruct completion meaning from syntax.
```

No change is required to:

- A4’s direct predecessor;
- D1’s direct predecessor;
- `verification.md`’s “post-A3” baseline wording;
- A4’s “post-A3 lineage” wording;
- D5 or D8 predecessor lists.

## 6. A3 disposition

**Keep the non-G10 work unlanded as A3. Do not reduce A3’s exit criterion and do not split it into another semantic block.**

Permitted interim state:

- preserve or commit the work on the local A3 branch;
- treat it as contingent upper-layer work only;
- remove the provisional `statement_guarantees_current_function_return`-based G10 detector;
- after A2C is accepted, rebase/restack A3, consume the accepted facts, add G10, and rerun all exact-SHA evidence.

Landing A3 without G10 would knowingly accept a catalogued wrong-and-warm result and contradict A3’s safety objective. Splitting the non-G10 work would add program identity and evidence churn without creating an independent architecture boundary.

## 7. Amendment/provenance integrity

This amendment must be registered as:

```text
AMD-002 — A2C completion-model predecessor for A3
```

Add `R-9` to `evidence/maintainer-rulings.md` recording the ratified insertion, and register AMD-002 in both that file and `README.md`.

Because this decision explicitly edits reconstructed authority files, the current statements in [README.md:56](<REPO>/docs/arch/refactor/rev11/README.md:56) and [PROVENANCE.md:24](<REPO>/docs/arch/refactor/rev11/PROVENANCE.md:24) otherwise become false. Replace the README policy paragraph with:

```markdown
Amendments normally record deltas without editing the verbatim-reconstructed authority files. AMD-002 is the maintainer-ratified exception because predecessor authority must be materialized in the machine-readable DAG and exact-state template. The published consolidated and release artifacts remain immutable historical originals; for execution, AMD-002 and the amended live split files supersede their A2-to-A3 lineage.
```

Update `PROVENANCE.md` to state that the 67/67 byte-verbatim attestation applies to the originally reconstructed bytes recoverable from the pinned consolidated master, while the named live split files were subsequently amended by AMD-002. Recompute the aggregate digest after all changes.

Do **not** edit:

- the pinned consolidated master;
- release artifacts;
- `_EXTRACTION_INDEX.md`;
- historical readiness-review prose.

AMD-002 must explicitly supersede their old `A0 → A1 → A2 → A3` lineage wherever encountered.

## 8. Risks and stale evidence

1. **Second completion authority.** Blocking unless completion lives on `FunctionBodySkeleton`, with D6 consuming the same types. A session-local recursive classifier is unacceptable.

2. **False exactness.** Unsupported loop/recovered-syntax cases must produce typed unknown. Defaulting to `Normal`, empty, or “does not guarantee return” recreates G10.

3. **Performance regression.** Do not rewalk OXC syntax per demand. Measure skeleton-build CPU and retained bytes; completion construction must be linear and deterministic.

4. **A3 false refusals.** X68, X80, X88 and labeled/switch/catch controls are mandatory negative controls. A broad “contains try/label/throw” detector is insufficient.

5. **Stale A3 candidate.** Its current base, charter digest, context packet, evidence digest, review, and mutation receipts become invalid after A2C acceptance. Restack and rerun.

6. **Program-state failure.** The validator rejects a DAG/state block-set mismatch. Update both the template and external ledger atomically with the DAG.

7. **Digest invalidation.** Recompute DAG, program, charter, context-packet, package-tree, and any stack-snapshot digests. Any A4/A6 preparation based on the previous DAG is stale.

8. **Historical-document ambiguity.** The consolidated master and readiness reviews retain the old lineage. AMD-002 must state explicit execution precedence.

9. **Downstream ownership drift.** D6 must be amended to consume A2C rather than rebuild it; D5 and D8 must remain unchanged.

10. **Cache identity.** If completion facts affect a cached lowered slice, prove that its key includes the whole-body content identity as currently contracted, or fold the selected completion facts into the slice identity. No cross-content warm reuse may bypass the new discriminator.

11. **Evidence dilution.** A2C proves fact correctness and non-interference; A3 proves public degradation and non-admission. Do not use A2C’s fact tests as substitutes for A3 cold/warm public evidence.

12. **Provenance falsehood.** Editing the live authority without the README/PROVENANCE amendment is an anti-rogue integrity defect and blocks acceptance.

__DONE__
