# A5 — Evidence custody, program-state workflow, worktree/branch/CI/merge rules, stack window, review contexts

Decides the six operational deliverables A5 owes: evidence paths, program-state custody,
worktree/branch naming, CI/merge constraints, review contexts, and bounded stack-window policy.
`A6` **freezes** what A5 decides (`program.md`: A6 freezes "accepted program-state, context-packet,
evidence-custody, worktree, branch, CI, merge, and bounded stack-window policy").

Each section states the *current practice* observed in the tree, then the *decision* — because
several of these already have de-facto conventions established by A0–A4, and A5's job is to
ratify or correct them, not to invent replacements.

---

## 1. Evidence custody

### Current practice (A0–A4)

Three tiers, all in use:

1. **In-tree, identity-free summary** — `docs/arch/refactor/rev11/evidence/<BLOCK>-summary.md`,
   plus a `<BLOCK>/` directory of data artifacts (A4: `A4/baseline-40-components.tsv`,
   `A4/disabled-overhead.md`). A0-summary states the rule that makes this tier work: it records
   **no candidate identity and no review outcome**, because "a committed file that names its own
   commit or depends on a review verdict invalidates itself on every fix round".
2. **Exact-candidate record** — the SHA/tree, the three mandate verdicts, the evidence digests,
   and the raw command proofs. Lives in the ledger tree, addressed by digest from
   `program-state.toml`'s `block.<ID>.evidence_digest`.
3. **The live ledger** — `program-state.toml` and its per-block bundles. Ruling **R-6** keeps the
   authoritative copy **external** to any checkout; commit `49850029c` imported a **transport
   copy** to `docs/arch/architecture-lock/ledger/` so a second machine can resume, with absolute
   paths normalised to placeholders and `ORIGINAL-DIGESTS.tsv` recording every file's
   pre-normalisation digest.

### Decision E-1 — ratify the three tiers as written

The split is correct and A5 adopts it unchanged. The identity-free rule for tier 1 is the load-bearing
part: it is what lets a summary be committed on the candidate branch without invalidating itself
when a fix round produces a new candidate.

### Decision E-2 — the transport copy is transport, and its removal obligation is real

`docs/arch/architecture-lock/ledger/README.md` records the obligation: the directory is removed
from the repository **and from git history** at plan close, and the cheapest way to honour that is
to keep the importing commit off any long-lived branch. A5 ratifies this and adds one operational
consequence the README implies but does not state as a rule:

> **The program's integration lineage (`program/architecture-lock`) must not be fast-forwarded
> into `main` while commit `49850029c` is in its history.** Landing the program is therefore a
> history-rewriting operation (drop that commit) or a squash that excludes the directory — decided
> at plan close, but decided *before* the first landing to `main`, not discovered at it.

### Decision E-3 — the in-tree ledger copy is stale, and staleness is expected

The in-tree copy records `current_block = "A3"` and an `A4` row at `status = "LOCKED"`, while A4
is landed at `147258e0b`. That is not a defect: the README states this copy has no merge story and
one machine is the writer at a time. A5 records it so a reader does not mistake the transport copy
for the ledger. The authoritative state is external.

## 2. Program-state custody

### Current practice

`governance.md` §5: `program-state.toml` is the durable execution ledger, the **orchestrator is its
sole writer**, and the maintainer accepts transitions requiring authority. `governance.md` names
`tools/validate_program_state.py`; ruling **R-4** reimplemented it as
`scripts/validate-program-state.mjs` (Node, with a `node --test` suite), because `CLAUDE.md`'s
dependency policy forbids committed Python and R-3 resolved the conflict in the plan's favour
*by reimplementation*, so no Python is committed.

### Decision P-1 — ratify sole-writer custody; the implementer never writes the ledger

Confirmed as the standing rule. This block wrote no ledger row, and no block implementer should:
an implementer that records its own `ACCEPTED` row is self-accepting, which `governance.md` §1.2
forbids for the orchestrator and a fortiori for an implementer.

### Decision P-2 — the validator runs on the transport copy, with the path recorded

```sh
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live
```

is the in-tree invocation (from the ledger README). The external ledger is validated with the same
command against its own path. Both must pass; a divergence between them is a synchronisation
failure to reconcile by hand, not a validator finding.

### Decision P-3 — record the integration lineage in the ledger

The ledger's `[repository]` table records `branch = "main"`, `head_sha = 9af553dd…` — the **A0
entry checkout**. But A1–A4 landed on `program/architecture-lock`, which is now 15 commits ahead of
`main`. The ledger has no field distinguishing "entry checkout" from "current integration branch",
so the integration lineage is recorded nowhere machine-readable.

**Decision:** A6 adds an explicit integration-lineage field to the ledger schema (or an
`[integration]` table) naming the branch onto which accepted blocks land. Until then the
orchestrator records it in prose in each block's exact-candidate record. This is a real gap: a
resuming agent reading `[repository]` alone would land onto `main` and silently drop A1–A4.

## 3. Worktree and branch naming

### Current practice

Observed in this program: `work/a<N>-<slug>` for the implementer branch (`work/a5-inventories`),
`program/architecture-lock` for the integration lineage, and a per-block worktree at a sibling
path (`…/verter-a5`). A4's review used a separate detached checkout (`…/a4-adv-control`).

### Decision W-1 — ratify, and extend to the review and fix roles

| purpose | convention |
|---|---|
| integration lineage | `program/architecture-lock` (single, fixed) |
| block implementation | `work/<block-id-lowercase>-<slug>` |
| block review checkout | `review/<block-id-lowercase>-<mandate>` where mandate ∈ `conformance` \| `architecture` \| `adversarial` |
| fix round on an existing candidate | the same `work/…` branch; a fix produces a new candidate, not a new branch |
| worktree path | a sibling directory of the program root, never nested inside it |

The "sibling, never nested" rule is not cosmetic: seven of the ten live worktrees are nested under
`…/verter/.claude/worktrees/`, i.e. inside the program root's own ignored tree, which makes them
invisible to a `git status` in the parent and easy to leave behind (see
[`open-changes.md`](open-changes.md) §5).

### Decision W-2 — one writable worktree per worker, enforced by assignment not by hope

`governance.md` §5 already requires it. The operational addition A5 makes: **a fresh worktree runs
`pnpm install --frozen-lockfile` before any JS/TS test or workspace-importing Node script**, because
`node_modules/` is gitignored and its absence makes JS/TS tests fail in a way that reads as a
regression. This is a known cost of the worktree model and it belongs in the context packet, not in
each implementer's memory.

## 4. CI and merge constraints

### The governing fact

**GitHub Actions never runs for this program.** `.github/workflows/ci.yml` triggers on
`push: branches: main` and on `pull_request`. Ruling **R-8** states that all Revision 11 work stays
local: nothing is pushed to `origin`, no PR is opened, landing is a local fast-forward, and
`origin/main` is frozen.

Everything else in this section follows from that.

### Decision C-1 — the local canonical gate is the only executed automated verification

`node scripts/gate.mjs` plus the end-of-change checks in `CLAUDE.md` are the program's whole
automated surface. Per-block command evidence (the A1 model: captured stdout/stderr preserved as
numbered command proofs) is how execution is proven, because no CI receipt exists.

This is weaker than CI and must be stated as such rather than papered over. `CLAUDE.md`'s
*Verification Must Prove Execution (MANDATORY)* rule applies with full force precisely because
there is no independent runner: exit status 0 alone is not evidence, and the command proof must
show the intended target was selected and did non-zero work.

### Decision C-2 — CI wiring is a post-program obligation, not a program mechanism

Any block tempted to "add a CI job" for coverage is adding a job that will not run until the
program lands on `main`. Two consequences, both live:

- A4's deferred gate-coverage debt cannot be discharged by a CI job during the program. Settled in
  [`instrumentation-reconciliation.md`](instrumentation-reconciliation.md) §3 by making the two
  feature arms **required per-block commands locked by A6**, with the CI job proposed for after.
- Ruling **R-7** authorised exactly one narrow `.github/` edit for a different purpose. Any further
  `.github/` change needs its own maintainer ruling.

### Decision C-3 — merge constraint: one block per landing delta, fast-forward, no co-batching

`governance.md` §9: "A single program block is not co-batched with unrelated changes in the same
landing delta." Landing is a local fast-forward of `program/architecture-lock` onto the accepted
candidate. Because the reviewed candidate SHA and the accepted landing SHA may differ when the base
advances, a diverged accepted identity requires the landing-equivalence artifact (the ledger's
`landing_equivalence_digest`), which the program-state validator already gates.

## 5. Bounded stack-window policy

### Constraints, from source

- `ORCHESTRATOR.md` §7: "The default maximum is four open review layers; the permitted A6 range is
  **two through six**." A6 selects the operational tooling and locks the policy.
- The ledger currently carries the default: `max_open_stack_layers = 4`,
  `stack_tool = "UNDECIDED_UNTIL_A6"`, `stack_mode_policy = "UNDECIDED_UNTIL_A6"`.
- R-8 removes PR stacks entirely as a mechanism. A "stack" here is a chain of local branches, not
  a chain of pull requests.
- **AMD-001** makes the stack-window validator a *prerequisite*: before any post-A6 stacked
  delivery — any window opened, any block claiming the contingent stacked-work exception, and in
  particular before `D1` may enter `PRIVATE_CHECKPOINT` — `A6` must deliver the Node stack-window
  validator, composite program-state cross-validation, CI wiring, and a discriminating D1/D2
  transition test.
- `contracts/stacked-prs.md` §3.2: `D1`/`D2` is *the* canonical `ATOMIC_REVIEW` case.

### Decision S-1 — `max_open_stack_layers = 2`, `stack_mode_policy = ATOMIC_REVIEW`, `stack_tool = LOCAL_BRANCH_CHAIN`

Narrow the ledger's inherited default of 4 to the **minimum of the permitted range**, and name the
mode and tool that R-8 actually leaves available.

Rationale, in order of weight:

1. **The program requires exactly one stack, and it is depth 2.** The only stacked delivery the
   plan mandates is `D1` (private checkpoint) → `D2` (sole acceptance and landing unit). A window
   of 2 admits it exactly and admits nothing else.
2. **AMD-001 makes window width a cost.** A6 must build a validator that models the
   contingent-predecessor rules. Modelling a 2-layer window correctly is a bounded job; modelling
   a 4-to-6-layer window is a larger one for capability the program has no use for, and AMD-001
   exists precisely because an unmodelled path becomes a trap at the worst moment.
3. **A0's recorded validator limit points the same way.** `scripts/validate-program-state.mjs`
   enforces the strict single-`IN_PROGRESS` reading, which already conflicts with the ledger's
   `max_active_workers = 3`. Widening the stack widens that conflict; narrowing it shrinks the
   conflict to the one case AMD-001 requires be modelled anyway.
4. **`stack_tool`**: GitHub native stacks, merge queues, and dependent PRs are all unavailable
   under R-8. The available tool is a local branch chain with an explicit stack-window record;
   naming it as such prevents A6 from selecting tooling that cannot run.

The `LANDABLE` mode (§3.1) stays permitted but unused: no two program blocks are currently planned
to be in review simultaneously, and `ORCHESTRATOR.md` §6's "default to no more than three active
worker contexts" is about *workers*, not about open review layers.

### Decision S-2 — the default operating mode is depth 1 (no stack)

A window of 2 is a *ceiling*, not a target. Blocks land sequentially, each on the accepted tip of
`program/architecture-lock`, and a stack window is opened only for `D1`/`D2` or for a case the
maintainer explicitly ratifies. Sequential operation needs no stack-window record at all — which
is why AMD-001's prerequisite binds only "before any post-A6 stacked delivery", and why A0–A5 have
correctly carried `stack_id = ""`.

## 6. Review contexts

### Constraints, from source

`governance.md` §1.6: Foundational work has three distinct mandates — **conformance**,
**architecture**, **adversarial performance/memory**. §13: independence requires a clean or
intentionally bounded context, a distinct mandate, exact baseline and candidate SHA, direct access
to diff/source/tests/benchmarks/raw outputs, an explicit scope cone and causal-blocker rule,
permission to challenge plan assumptions, permission to return `NOT PROVEN`, and no reliance solely
on the implementor summary. §1.6 again: "one context must not scope, implement, and provide the
only substantive approval for the same non-local block". §13: "Multiple automated/model instances
with identical prompt/context and no independent inspection are not automatically independent."

### Decision R-1 — three contexts, three mandates, one candidate identity

| mandate | context requirement | verdict lands in |
|---|---|---|
| conformance | clean context; reads the charter, the diff, and the deletion set | `block.<ID>.conformance_review` |
| architecture | clean context; reads the diff plus the authority package; may challenge the charter | `block.<ID>.architecture_review` |
| adversarial performance/memory | clean context; reads the diff plus the raw command/benchmark outputs | `block.<ID>.adversarial_review` |

Each returns exactly one of `PASS` / `BLOCKING FINDINGS` / `NOT PROVEN` / `NON-BLOCKING
DISCOVERIES` (§8), bound to one exact candidate SHA **and tree**.

### Decision R-2 — the four hard exclusions

Stated as rules because each is a specific way a review can look independent and not be:

1. **The implementer context fills no mandate**, on its own block, in any round.
2. **The orchestrator's synthesis is not a mandate.** §1.2 is explicit; a summary of three reports
   is not a fourth review, and a summary of *zero* reports is not a first one.
3. **A reviewer that applies a fix does not re-approve its own patch** (§8). The re-check after a
   fix round is impact-bounded (§9) but must come from a context that did not author the fix.
4. **Three instances of the same prompt are one context, not three** (§13). Distinctness is
   established by *mandate and inspection*, not by process count. Where the same tool serves two
   mandates on one block, the two must be given different scope cones and different evidence, and
   the orchestrator records that they were.

### Decision R-3 — for an evidence-only block, the mandates are re-pointed, not waived

A5 changes no production source, so "adversarial performance/memory" has no candidate runtime to
attack. The mandate is not waived — that would let an inventory block claim Foundational review
while receiving two-thirds of it. It is re-pointed at the evidence:

- **conformance** — is every `Required evidence` item in the charter delivered, and does each cite
  real source rather than restating `CLAUDE.md`?
- **architecture** — are the dispositions and the locked strategies correct, and does any decision
  create a second owner or foreclose a later block's legitimate choice?
- **adversarial** — are the *claims* falsifiable and false-negative-resistant? Concretely: re-run
  the counter census and the closure walk; check that a claim marked NOT PROVEN is genuinely
  unproven rather than unexamined; check that the "abandon as a class" branch disposition holds
  for a sampled member.

This re-pointing is itself a decision a reviewer may reject. It is stated openly so that rejecting
it is possible.
