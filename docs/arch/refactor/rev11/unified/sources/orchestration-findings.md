# Rev11 orchestration / rescoping findings for Codex PRO

Use the following as architectural input when revisiting the Rev11 plan, DAG, charters, and orchestration model.

The goal is **not to weaken Rev11, C1, J1, or any other ambitious architectural work**. The goal is to preserve the architecture while fixing the execution shape that turned some nominal “blocks” into multi-day trains with unnecessarily large acceptance surfaces, excessive governance churn, poor parallelism, and avoidable model/token cost.

One important correction to earlier discussion: **keep** **`program/architecture-lock`** **as the canonical integration/control branch. Do not redesign the system so that** **`architecture-lock`** **consumes** **`refactor/product-branch`****.** Independent trains/blocks may execute on their own branches and merge into `program/architecture-lock`. If a clean code-only/product branch is retained, it should be downstream/derived from accepted architecture-lock work, not the authority that architecture-lock follows.

---

## 1. Primary diagnosis: Rev11 sometimes confuses an architectural outcome with an executable DAG block

The main problem discovered through C1 and J1 is not that the architecture is too ambitious.

The problem is that Rev11 sometimes treats:

> “one architectural outcome”

as equivalent to:

> “one independently dispatched, reviewed, accepted and landed DAG node.”

Those are not necessarily equivalent.

A block should ideally represent the **smallest independently acceptable architectural mutation**.

A train is a logical grouping of several such mutations that collectively achieve a larger architectural outcome.

The current pattern sometimes produces:

```text
GIANT BLOCK
  architecture investigation
  + new primitives
  + multiple consumer migrations
  + multiple ownership relocations
  + compatibility transition
  + old-path deletion
  + final convergence proof
```

even when several of those phases are independently implementable and independently reviewable.

Internally dividing this giant block into “subblocks” or “slices” does not fully solve the problem if the DAG still sees only one node.

The DAG then still gets:

```text
C1
 │
 │ days of work
 │
 ▼
ACCEPTED
```

rather than:

```text
C1A ──┐
C1B ──┼── C1X
C1C ──┘
```

The latter permits partial convergence, independent review, parallel work, shorter context lifetimes, smaller recovery surfaces and potentially earlier downstream unlocks.

---

# 2. C1 is effectively a train disguised as a block

C1 is probably the clearest example of **scope discovery dramatically outgrowing the original DAG node**.

The original plan made C1 sound roughly like:

```text
converge ModuleResolverCore + non-flow TypeInfoCore
preserve semantics
introduce immutable observations
support NeedInputs
```

Architecture review then correctly discovered that doing this properly required substantially more:

- actual crate extraction into `verter_semantic`;
- movement of a large resolver subsystem;
- relocation of `ProjectResolver`;
- dependency-layer changes;
- a proper semantic dependency firewall;
- immutable observation interfaces;
- full `AttemptOutcome::{Complete, NeedInputs, Terminal}` coverage rather than a narrow patch;
- lifecycle adapter convergence;
- duplicated ownership removal;
- authority-uniqueness proofs;
- lifecycle equivalence tests;
- compatibility with the future I/O-free compiler path.

This is excellent architecture.

But once those rulings expanded the work to that extent, **C1 should probably have been converted from one block into a train**.

Conceptually:

```text
C1A  semantic crate/dependency boundary
C1B  resolver authority relocation
C1C  immutable observation contract
C1D  AttemptOutcome / NeedInputs closure
C1E  lifecycle adapter convergence
C1F  old-owner deletion
C1X  cross-lifecycle equivalence + terminal
```

The exact decomposition should be determined from current source ownership rather than mechanically adopting these names.

The important point is that **C1's architectural destination should not be weakened**.

Its destination is extremely valuable:

```text
                     verter_semantic
                           │
             ┌─────────────┴─────────────┐
             │                           │
      ModuleResolverCore            TypeInfoCore
             │                           │
             └─────────────┬─────────────┘
                           │
                    semantic authority
                           │
             ┌─────────────┼──────────────┐
             │             │              │
          compiler        LSP       session/managed
```

This is foundational for Verter becoming a shared engine for compiler, LSP, CLI, TypeInfo and future framework/language support.

### C1 conclusion

- Architectural value: **extremely high**
- Scope quality as an architectural goal: **high**
- Scope quality as one DAG block: **poor**
- Do not undo the architectural rulings.
- Do change how similarly expanding blocks are represented in the DAG.

---

# 3. J1 is also a train disguised as a block

J1 has a similarly strong architectural destination:

```text
CSS-family source
       │
       ▼
  StyleSyntaxIr
       │
 ┌─────┼──────────┬──────────┐
 │     │          │          │
compiler LSP   formatter    lint/actions
```

The governing principle is excellent:

> One authoritative CSS-family parser parses the bytes once; consumers use its result instead of independently re-deriving CSS structure.

The problem is that J1 contains a broad population of migration targets:

- canonical `StyleSyntaxIr`;
- deletion of Lightning CSS parsing authority;
- Svelte's separate CSS parser;
- Svelte's additional rejection parser;
- VDOM static-style scanning;
- SSR static-style scanning;
- semantic style scanning;
- unused-CSS scanning;
- document-color scanning;
- CSS completion-context scanning;
- Vue CSS processing boundaries;
- parser-result extensions needed by those consumers;
- framework-neutral vs framework-specific ownership reconciliation;
- dependency deletion;
- negative proofs that alternate parsing/scanning no longer exists.

That is not one implementation surface.

A healthier conceptual structure would have been closer to:

```text
J1L     CSS authority/inventory lock
  │
  ├── J1P    canonical parser capability completion
  ├── J1S    Svelte parser migration
  ├── J1C    compiler consumer migrations
  ├── J1A    semantic/action migrations
  └── J1LSP  LSP consumer migrations
         │
         ▼
J1X     alternate-parser deletion + parse-once terminal
```

Again, the exact partition should come from source-level analysis.

### J1 conclusion

J1's **architecture is very good**.

Its execution decomposition was poor.

The lesson should not be:

> avoid broad convergence.

It should be:

> represent broad convergence as a train with an atomic terminal rather than forcing the entire convergence through one acceptance node.

---

# 4. C1 and J1 were expensive, but the engineering work is largely justified

Do not interpret their long execution times as proof that the architectural goals were wrong.

Most of the difficult engineering is paying down real architectural debt.

C1 establishes:

- correct semantic ownership;
- clean dependency direction;
- lifecycle-independent semantic answers;
- resumable I/O-free semantic operations;
- shared semantic machinery suitable for compiler/LSP/CLI;
- elimination of lifecycle-specific resolver duplication.

J1 establishes:

- one CSS-family syntax authority;
- no competing private grammars;
- reuse of parsed structure;
- reduced semantic drift;
- reduced repeated source scanning;
- a foundation for compiler, LSP, formatter and lint to consume common style facts.

The expensive part that should be eliminated is **administrative amplification**, not architectural ambition.

---

# 5. Lower-effort models probably amplified churn, but did not make C1/J1 fundamentally large

This distinction is important.

A weaker model may turn:

```text
large coherent migration
```

into:

```text
partial migration
→ reviewer finds missed consumer
→ repair
→ reviewer discovers duplicate fallback
→ repair
→ test doesn't discriminate architecture
→ repair
→ temporary compatibility path survives
→ cleanup
→ recovery agent
```

So lower-capability implementers/planners can materially increase:

- repair cycles;
- missed migration surfaces;
- accidental fallback paths;
- duplicate authorities;
- temporary adapters;
- weak tests;
- stale documentation;
- context churn;
- partial landings;
- recovery effort.

However, they **did not create the fundamental scope**.

C1 was already made large by architecture rulings choosing the stronger structural solution.

J1 was already broad because the parse-once invariant legitimately applies to many existing CSS readers.

A substantially stronger implementation model would most likely have produced:

```text
same architectural destination
same broad required migration
fewer false starts
fewer repair cycles
less temporary code
shorter wall-clock execution
```

not a tiny C1 or J1.

---

# 6. Remaining hidden trains that should be audited before dispatch

The following blocks should be treated as **high-risk hidden trains** unless source-level prescoping proves otherwise.

## H2 — highest concern

Current effective risk: **10/10**

H2 currently combines many mechanisms:

- ProviderHub bindings;
- provider applied-generation authority;
- request admission;
- deadlines;
- cancellation;
- IDE repair lifecycle;
- resync replacement;
- tsserver carrier refresh;
- shared-tsgo synchronization;
- declaration overlay lifecycle;
- lazy transport/provider establishment;
- provider spawning/cold start;
- project bootstrap;
- membership recovery;
- retry/backoff removal;
- old coalescer deletion.

These are several independently complicated concurrency/lifecycle problems.

Do **not** dispatch H2 in its present monolithic conceptual form without a decomposition review.

Potential shape:

```text
H2A  provider binding/receipt contract
H2B  admission + deadline + cancellation
H2C  establishment/cold-start lifecycle
H2D  tsserver/shared-tsgo convergence
H2E  overlay lifecycle
H2F  recovery/retry convergence
H2G  remaining provider/coalescer migration
H2X  final old-lifecycle deletion + terminal
```

Exact boundaries must come from code analysis.

---

## H3 — another likely train

Risk: **\~9/10**

H3 appears to combine:

- foreground LSP settlement;
- background quiet-window policy;
- synchronization coordination;
- import publication;
- semantic scheduling;
- typing cooldown removal;
- provider-context migration;
- hover provenance;
- global readiness protocol replacement;
- sync-complete protocol replacement;
- Rust/LSP/client/DX-harness changes;
- stale-safe publication.

It has a legitimate final invariant—atomic/stale-safe publication—but many preparatory migrations could be separate nodes before the final cutover.

---

## G2 — likely hidden train

Risk: **\~8.5/10**

The `FlightCell` primitive itself is reasonably bounded.

The problem is that the same block appears to also migrate/delete several existing independent dedupe/coalescing mechanisms.

Prefer:

```text
G2A FlightCell primitive
G2B ImportSync migration
G2C external-TS flight migration
G2D publication-store migration
G2E component-meta migration
G2X duplicate-flight terminal
```

if source analysis confirms those migration surfaces are independently acceptable.

---

## E2 — likely hidden train

Risk: **\~8/10**

E2 migrates multiple consumer populations away from old TypeExpr/intermediate representations toward different replacement forms:

- borrowed authored nodes;
- compact chunks;
- semantic values;
- operation DTOs.

Different consumer families may naturally be independently migratable.

Prefer a lock + consumer migration branches + final deletion terminal rather than one giant consumer sweep.

---

## K3 — should become a small terminal if possible

Risk if unchanged: **\~8/10**

K3 is a broad convergence/deletion inventory.

The preferable design is **not** to turn K3 into another implementation train.

Instead, upstream owners should migrate/delete their own mechanisms.

Then K3 becomes approximately:

```text
verify residual inventory
verify final host shape
delete tiny remaining residue
terminal
```

If K3 still needs substantial migrations after all predecessors land, then those migrations should become explicit upstream/subtrain nodes.

---

## G4 / G5

Risk: **\~7/10 each**

Both have the pattern:

```text
inventory a broad population
→ classify
→ migrate/converge
→ delete old mechanisms
→ prove terminal
```

They should receive explicit pre-dispatch decomposition review.

---

## B4

Risk: **\~6–7/10**

B4 is broad but has a more coherent final invariant.

It may benefit from:

```text
contract
→ producer implementation
→ consumer migration
→ blanking deletion terminal
```

but is less obviously pathological than C1/J1/H2.

---

# 7. C1 and J1 are outliers within their own trains

Do not assume all remaining `C*` and `J*` nodes are equally enormous.

Approximate current assessment:

| BlockRelative scopeHidden-train risk |              |               |
| ------------------------------------ | ------------ | ------------- |
| C1                                   | 10/10        | 10/10         |
| C2                                   | \~7/10       | moderate      |
| C3                                   | \~5–6/10     | low-moderate  |
| C4                                   | \~3–4/10     | low           |
| J1                                   | 9.5–10/10    | 10/10         |
| J2                                   | \~4–5/10     | low           |
| J3                                   | \~5.5–6.5/10 | moderate      |
| J4                                   | \~6–7/10     | moderate/high |

C2 is substantial but conceptually cohesive around the staged compile transaction and sealed semantic facade.

C3 is a bounded semantic capability.

C4 is predominantly proof/closure.

J2 is narrowly concerned with exact style identity/reuse.

J3 is a moderate performance/materialization convergence.

J4 deserves a careful decomposition audit because dialect contracts, preprocessors, formatting, recovery and mapping could expand significantly once the source inventory is complete.

---

# 8. D and TCM demonstrate healthier decomposition

Use existing successful structures as precedent.

The D train recognizes that preparation can be decomposed even when the final authority switch must be atomic.

Conceptually:

```text
private foundation
       ↓
atomic public cutover
       ↓
independent follow-up work
```

TCM similarly separates:

```text
architecture lock
     ↓
independent planes
     ↓
atomic activation/deletion
```

This is much healthier than:

```text
everything from design through migration through deletion
inside one giant block
```

C1/J1-like future work should use the D/TCM pattern.

---

# 9. Formal rule for determining whether something is a block or a train

Adopt a mechanical prescope test.

A candidate node should be split when it contains **two or more independently acceptable mutation surfaces**.

Questions to ask before dispatch:

1. Can part A land without part B while preserving all currently admitted behavior?
2. Can A be independently reviewed against an exact contract?
3. Does completing A unlock useful downstream work?
4. Does A touch a different ownership domain from B?
5. Does A have distinct failure/recovery semantics?
6. Does A require a different reviewer specialization?
7. Could A and B execute safely in parallel?
8. Would rebasing or repairing B unnecessarily invalidate a correct A?
9. Is the only reason they are together “they belong to the same architectural outcome”?
10. Is the real atomic requirement only the final deletion/cutover?

If several answers indicate independence, create separate DAG nodes.

The preferred pattern is:

```text
contract / architecture lock
          ↓
     preparation
    ↙    ↓    ↘
   A     B     C
    \    |    /
    atomic cutover
          ↓
       terminal
```

---

# 10. Atomic cutover does not justify a giant block

This should become an explicit orchestration principle.

Sometimes the final authority transition really is atomic:

```text
OldAuthority → NewAuthority
```

There must never be an accepted intermediate state with two competing authorities.

That does **not** mean all preparatory implementation must happen inside the same DAG node.

The correct structure is:

```text
new primitives
consumer migration preparation
representation work
tests
compatibility preparation
       │
       ▼
final atomic cutover
       │
       ▼
old authority deleted
```

Preparatory nodes may be independently acceptable while the final cutover remains indivisible.

---

# 11. One canonical DAG, but not one giant release train

Keep one canonical authority graph.

However:

> **One DAG must not mean one serialized train.**

The graph should represent many logical subtrains:

```text
Rev11 core
compiler bridge
compiler common
Vue compiler
Svelte compiler
CSS/style
TypeInfo
formatter
lint
CLI
future verticals
...
```

A train should be metadata/grouping, not an acceptance boundary.

Useful node metadata:

```toml
id = "..."
train = "compiler.vue"
phase = "rev11" # or successor
product = "vue_compiler"
kind = "implementation"
conflict_domains = ["vue_semantics", "vue_compiler"]
resource_class = "rust-heavy"
release_gating = "none"
```

The DAG should encode **correctness dependencies**.

It should not encode machine availability or scheduling convenience as fake dependency edges.

---

# 12. Physically modular DAG, logically one DAG

“One DAG” does not need to mean one enormous TOML file that every train edits.

Prefer a modular physical representation if necessary:

```text
dag/root.toml
dag/rev11.toml
dag/compiler.toml
dag/css.toml
dag/typeinfo.toml
dag/expansion.toml
...
```

with a deterministic validator/generator producing the canonical combined graph.

This reduces central-file merge conflicts while preserving one logical authority graph.

---

# 13. Schedule the full READY frontier

The orchestrator should stop thinking primarily in terms of:

```text
what is the next block?
```

and instead compute:

```text
READY = all DAG nodes whose authority predecessors are accepted
```

Then schedule across the complete READY frontier according to:

- machine availability;
- conflict domains;
- model requirements;
- resource class;
- critical-path importance;
- fairness/age;
- expected integration conflict.

Machine constraints should not become DAG edges.

Example:

```text
READY:
  C3
  J2
  G2B
  formatter-lock
  compiler-contract

Machines:
  M1 rust-heavy
  M2 rust-heavy
  M3 docs/architecture
```

The scheduler assigns leases.

The DAG remains unchanged.

---

# 14. Introduce conflict domains rather than over-serializing

Blocks should declare the subsystems whose simultaneous mutation is unsafe.

For example:

```toml
conflict_domains = [
  "resolver_core",
  "semantic_authority"
]
```

Two READY blocks with disjoint conflict domains can proceed concurrently.

Two blocks that both modify `semantic_authority` may need serialization even if there is no conceptual DAG dependency.

This distinction prevents the DAG from becoming polluted with false ordering edges.

---

# 15. Separate static authority, historical evidence, runtime state and derived state

The current orchestration model carries too much mutable information in central state.

Move toward:

```text
AUTHORITATIVE STATIC STATE
    DAG
    charters
    architecture decisions

AUTHORITATIVE HISTORICAL STATE
    immutable acceptance receipts

OPERATIONAL / EPHEMERAL STATE
    leases
    active machines
    worktree/ref
    heartbeat
    current implementation slice

DERIVED STATE
    generated status/program view
```

Core rule:

> **Derived state must not become another authority.**

---

# 16. Replace central mutable ledger churn with immutable receipts

Do not continuously rewrite a giant `program-state.toml` with information that Git or immutable evidence can already prove.

An accepted block could have a small receipt approximately like:

```toml
schema = 2
block = "J2"
control_basis = "..."
candidate = "..."
accepted_integration_commit = "..."
charter = "docs/.../J2.md"
predecessors = ["J1"]

reviews = [
  "evidence/J2/conformance.receipt",
  "evidence/J2/architecture.receipt",
  "evidence/J2/adversarial.receipt",
]

gate = "evidence/J2/gate.receipt"
decision = "accepted"
```

Do not store redundant facts merely because they can be stored.

Derive where possible:

- candidate tree from candidate SHA;
- accepted tree from integration SHA;
- ancestry from Git;
- charter content from `control_basis + path`;
- DAG content from `control_basis`;
- review identity from immutable review receipts;
- code/tree equivalence mechanically.

Persist only facts that cannot be reconstructed safely.

---

# 17. Runtime leases should not mutate governance

An active agent should obtain operational state such as:

```text
block
branch/ref
control basis
machine
lease epoch
heartbeat
expiry
```

That should not require governance commits every time an agent:

- starts;
- stops;
- changes implementation slice;
- clears context;
- moves between machines;
- resumes.

This state is ephemeral.

Only acceptance creates permanent historical evidence.

---

# 18. WIP commits should be cheap; acceptance identity should be strict

During implementation:

```text
commit
rebase
fix
rebase
review locally
continue
```

should be normal.

Do not incur expensive authority/ledger churn for every WIP identity change.

At:

```text
READY FOR ACCEPTANCE
candidate = exact SHA
```

freeze the candidate.

From that point:

- reviewers review exactly that candidate;
- modifications invalidate relevant verdicts;
- do not rebase the frozen candidate;
- if changes are required, generate a new candidate and re-review affected evidence.

This preserves strong exact-candidate guarantees without making ordinary development prohibitively expensive.

---

# 19. Keep `program/architecture-lock` as canonical integration

This is the corrected branch policy.

Recommended topology:

```text
                    canonical authority
                program/architecture-lock
                         ↑
                    accepted merges
                  ↗      ↑      ↖
              block/C2 block/J2 block/H2A
```

Independent train branches may exist:

```text
train/compiler
train/css
train/typeinfo
...
```

but their accepted units ultimately merge into `program/architecture-lock`.

Do **not** make `architecture-lock` consume a code/product branch as its upstream authority.

If a clean `refactor/product-branch` remains useful, treat it as something like:

```text
program/architecture-lock
         │
         │ accepted code projection / cherry-pick / generated sync
         ▼
refactor/product-branch
```

It is a clean derivative/product history, not the canonical program authority.

The exact mechanics of maintaining that derivative branch should be planned separately so it does not reintroduce SHA/ledger busywork.

---

# 20. Prefer exact candidate preservation through merge commits

Once a candidate has completed final review, preserve it.

If architecture-lock advances while another candidate is under review:

### No conflict

Merge the frozen candidate as an exact parent of a new integration commit.

Do not rewrite the reviewed candidate purely to retain artificial linear history.

### Conflict

Do not let the landing orchestrator creatively resolve significant conflicts.

Return the block to implementation:

```text
update basis
resolve conflict
produce new candidate
re-run affected validation
```

This preserves the meaning of exact-candidate review.

---

# 21. Distinguish candidate identity from integration identity

An acceptance receipt should distinguish:

```text
candidate_sha
integration_sha
control/receipt_sha
```

These are different concepts.

`candidate_sha`:

> exact implementation reviewed.

`integration_sha`:

> commit on `program/architecture-lock` containing that candidate in the cumulative accepted tree.

`receipt/control_sha`:

> optional tiny subsequent control-state/receipt commit.

The invariant can require:

```text
candidate_sha is ancestor/parent of integration_sha
```

rather than pretending all three identities must be identical.

---

# 22. Integration needs its own semantic safety check

A conflict-free Git merge does not guarantee semantic compatibility.

Therefore after combining independently accepted candidates, run an integration gate appropriate to the touched conflict domains.

It does not necessarily need to rerun every expensive block-specific test.

Think:

```text
block-specific acceptance gate
        +
cross-block integration gate
```

The latter checks what could have changed because of concurrent integration.

---

# 23. Avoid landing-time ledger work becoming the critical path

The landing path should be short:

```text
candidate accepted
      ↓
integration compatibility check
      ↓
merge into architecture-lock
      ↓
tiny immutable receipt
      ↓
READY frontier recomputed
```

Avoid:

```text
rewrite several central docs
recalculate hand-maintained SHAs
change duplicated state tables
repair generated-but-manually-edited summaries
rerun document ratification
```

for ordinary accepted blocks.

---

# 24. Charters should lock architecture, not become mini implementations

J1's eleven ratification rounds show another failure mode: the document itself can consume too much of the project.

Charters need enough specificity to distinguish:

- correct implementation;
- forbidden fallback;
- authority ownership;
- acceptance criteria;
- deletion responsibility;
- abort/rescope conditions.

But they should avoid duplicated prose and redundant restatement of the same facts.

Prefer one machine-readable/source-of-truth inventory with generated views rather than multiple sections manually restating the same classifications.

---

# 25. Tests must discriminate the architecture, not merely produce green output

One lesson from J1 is especially important.

Bad acceptance test:

```text
canonical parser was called
```

because this still passes:

```text
canonical parser called
result ignored
private scanner produces output
```

Good test/structural gate:

```text
canonical parser called exactly as expected
AND output derives from returned representation
AND alternate scanning implementation is structurally absent
```

The same applies throughout Rev11.

Tests should answer:

> Would the forbidden architecture also pass this test?

If yes, the test is not an architectural acceptance proof.

---

# 26. RED/GREEN testing remains valuable, but should be used selectively

Keep RED/GREEN where it proves the test genuinely detects the intended failure.

It is particularly useful for:

- architecture guards;
- regression fixes;
- negative capability tests;
- stale-publication tests;
- authority uniqueness;
- dependency-firewall compile failures;
- deterministic failure cases.

Do not blindly require RED/GREEN for:

- pure documentation;
- trivial generated tables;
- mechanical formatting;
- tests where a meaningful planted failure cannot be constructed.

The rule should be evidence-driven rather than ritualistic.

---

# 27. Model effort should be allocated by architectural risk

Do not use maximum reasoning effort everywhere.

Use the expensive models where mistakes multiply downstream cost.

### Highest reasoning tier

Use GPT-5.6 PRO/Ultra-class architecture reasoning for:

- block/train prescoping;
- architecture locks;
- hidden-train detection;
- ownership changes;
- cross-crate dependency moves;
- semantic authority changes;
- concurrency/lifecycle design;
- atomic cutovers;
- large deletion closures;
- amendment impact analysis;
- final architecture review of foundational blocks.

### Strong implementation models

Use strong implementers for:

- C1/J1/H2/H3-type foundational migrations;
- concurrency/state machinery;
- semantic/resolver changes;
- high-performance parser/compiler internals;
- broad migration terminals.

### Medium/cheaper models

These can handle well-specified:

- mechanical consumer migrations;
- repetitive API call-site changes;
- generated bindings;
- deterministic fixture additions;
- isolated cleanup;
- documentation synchronization;
- narrow RED/GREEN adversarial checks.

The prerequisite is that the architecture and exact mutation boundary are already locked.

Cheap models should not be expected to discover the architecture while implementing it.

---

# 28. Pre-scope every serious block before dispatch

The already-added architect prescope step should be strengthened into an explicit **block-or-train decision gate**.

Before dispatch, the architect should produce:

```text
1. mutation surfaces
2. current owners
3. final owners
4. migration populations
5. true atomic cutovers
6. independently acceptable slices
7. conflict domains
8. downstream unlock opportunities
9. deletion closure
10. model/effort recommendation
```

Then explicitly conclude:

```text
BLOCK
```

or:

```text
TRAIN
  A
  B
  C
  X
```

A charter should not proceed until this classification has been reviewed.

This would probably have caught C1/J1 before they became multi-day monoliths.

---

# 29. Architecture discovery may legitimately enlarge scope — but that should trigger DAG amendment

C1 demonstrates this perfectly.

Initial plan:

```text
small-ish convergence
```

Architecture review:

```text
this actually requires crate extraction
+ full NeedInputs closure
+ lifecycle convergence
+ dependency firewall
```

The correct response should have been:

```text
scope grew materially
→ stop
→ amend DAG
→ split C1 into train
→ resume
```

not:

```text
scope grew materially
→ make C1 charter enormous
→ still call it one block
```

Introduce a threshold where architectural discovery automatically triggers **rescope review**.

---

# 30. A block becoming larger is not itself a failure

Some work is legitimately large.

Do not optimize for number of lines or elapsed hours.

The important questions are:

```text
Is there one coherent authority mutation?
Is there one genuine acceptance boundary?
Would splitting create invalid intermediate architecture?
```

A large but cohesive atomic cutover may remain one block.

D2 is the kind of thing that should remain atomic.

Large validation/soak terminals can also remain blocks.

The anti-pattern is **multiple independently acceptable ownership/migration surfaces bundled together merely because they support one broad architectural objective**.

---

# 31. Convergence nodes should generally be cheap

A convergence node should preferably:

```text
consume previously accepted mutations
verify system-level invariants
perform tiny remaining deletion
close terminal
```

It should not unexpectedly become:

```text
implement another 30% of the train
```

If substantial implementation remains, upstream ownership/decomposition was wrong.

Apply this particularly to K3 and future compiler/tooling convergence nodes.

---

# 32. Accepted history should be immutable; corrections should be new facts

If an accepted block later needs to be reverted or superseded:

Do not rewrite its historical receipt.

Create:

```text
accepted receipt A
        ↓
superseding/revert receipt B
```

History remains auditable.

Likewise, later charter/DAG changes do not retroactively redefine what an earlier block accepted because the receipt binds its exact `control_basis`.

---

# 33. Amendments should compute impact closure mechanically

When architecture changes:

```text
A → B → C → D
```

and A's accepted basis is invalidated, the system should mechanically determine which downstream evidence is potentially stale.

Do not rely on humans manually updating dozens of ledger fields.

The DAG and receipts should make this computable.

---

# 34. The Compiler proposal already demonstrates better execution decomposition

The new compiler architecture proposal is structurally healthier than C1/J1.

It separates common work into nodes such as:

```text
CMP0 request/policy/identity
CMP1 demand + semantic admission
CMP2 data-oriented structure
CMP3 target planning
CMP4 emission/artifacts
CMP5 convergence
```

and then creates independent Vue and Svelte compiler trains.

That should be treated as a useful template:

> ambitious architecture can be decomposed without weakening it.

The proposed bounded bridge around:

```text
C1
 ↓
CCA0
 ↓
CCA1
 ↓
CCA2
 ↓
C2
```

is also preferable to injecting the entire future compiler architecture into C2.

Keep C2 bounded.

---

# 35. The successor/expansion plan has also learned this lesson

The newer expansion design explicitly moved away from one enormous all-verticals program and toward independently promotable product/vertical terminals.

That principle should also apply inside Rev11:

```text
one authority graph
many independently schedulable trains
few genuine convergence barriers
```

Do not make unrelated products wait for global completion merely because they share one program.

---

# 36. Suggested risk audit of remaining Rev11 nodes

Before resuming broad dispatch, explicitly audit at least:

```text
H2  CRITICAL hidden-train audit
H3  CRITICAL hidden-train audit
G2  HIGH
E2  HIGH
K3  HIGH — preferably shrink into terminal
G4  MEDIUM-HIGH
G5  MEDIUM-HIGH
J4  MEDIUM-HIGH
B4  MEDIUM
C2  confirm cohesive
```

C3/C4/J2/J3 appear much less concerning from their current framing.

---

# 37. Desired post-C1/J1 transition

Once C1 and J1 are safely closed, this is a good point to change orchestration because they have exposed the failure modes clearly.

Recommended sequence:

```text
1. Finish C1/J1 under current accepted authority.
2. Do not destabilize their recovery by changing governance mid-flight.

3. Introduce orchestration-v2 rules.

4. Audit every remaining large/unstarted node for block-vs-train scope.

5. Amend the canonical DAG where hidden trains are found.

6. Integrate Compiler/Expansion work into the same logical DAG
   with proper phase/train/product metadata.

7. Keep program/architecture-lock as canonical integration/control branch.

8. Allow independent train/block branches and multiple machines.

9. Replace heavy mutable ledger state with:
      static DAG/charters
      immutable receipts
      ephemeral leases
      generated status

10. Schedule the whole READY frontier.

11. Freeze exact identity only at acceptance.

12. Preserve reviewed candidates through integration.

13. Keep a short serialized landing lane into architecture-lock.

14. Continue maximizing architectural quality while reducing
    orchestration/token/context overhead.
```

---

# 38. Core principles Codex PRO should preserve

These are probably the most important sentences to carry into the formal redesign:

> **A DAG node represents the smallest independently acceptable architectural mutation. A train groups nodes that collectively achieve a larger architectural outcome.**

> **Atomic cutover does not imply atomic preparation. Prepare independently; converge atomically.**

> **Architecture discovery that materially expands a block should trigger DAG rescoping, not merely a larger charter.**

> **The DAG represents correctness dependencies. Resource contention and machine availability belong to the scheduler, not to dependency edges.**

> **The orchestrator schedules the entire READY frontier, not a single “next block.”**

> **The DAG is authority. Immutable receipts are history. Git commits are implementation identity. Leases are runtime state. Generated state must not become another authority.**

> **Exact candidate identity matters at acceptance, not during every WIP iteration.**

> **Convergence nodes should validate and close previously implemented architecture, not become surprise implementation trains.**

> **Use the strongest models where architectural mistakes have multiplicative cost; use cheaper models for bounded mechanical work after architecture has been locked.**

> **C1 and J1 are lessons in execution decomposition, not arguments for weaker architecture.**

> **Keep** **`program/architecture-lock`** **as the canonical integration/control branch. Independent branches merge into it; do not invert this authority relationship.**

---

## Final assessment to carry into the PRO planning pass

C1 and J1 are both genuinely valuable and point toward excellent long-term architecture.

Their main failure was **execution granularity**.

C1 became a train after architecture discovery expanded it, but the DAG was not amended accordingly.

J1 was fundamentally a broad convergence train whose many independent consumer migrations were represented under a single acceptance node.

The same mistake is currently most likely to recur in **H2, H3, G2, E2 and potentially K3/G4/G5**.

The solution is not smaller ambition.

It is:

```text
better prescoping
+ explicit trains
+ smaller acceptance nodes
+ atomic convergence terminals
+ one canonical DAG
+ full READY-frontier scheduling
+ conflict-domain scheduling
+ multiple machines
+ immutable acceptance receipts
+ ephemeral runtime leases
+ generated state
+ less SHA/ledger busywork
+ stronger models at architectural choke points
+ cheaper models for bounded mechanical work
```

That should be the basis on which Codex PRO revises the orchestration architecture and then asks the higher-level planning pass to produce the final DAG/charters.