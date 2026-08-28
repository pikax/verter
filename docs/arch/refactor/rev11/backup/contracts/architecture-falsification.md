# Architecture Premise and Falsification Contract

**Status:** Normative.  
**Owner:** architecture authority, A6 lock, and deviation governance.

# 1. Purpose

The program optimizes for the best final system, not for loyalty to a document. It must distinguish rules that define the desired product from implementation tactics that are selected by evidence.

No implementor or orchestrator may claim “best possible” merely because a design is detailed. The accepted claim is that the target is the strongest defensible architecture under the current evidence and that every evidence-sensitive premise has a falsification path.

# 2. Decision classes

## 2.1 Constitutional invariant

A constitutional invariant defines correctness, authority, product behavior, trust, or lifetime boundaries. Examples include:

- one semantic authority for one operation;
- unsupported work cannot publish as complete;
- direct compilation does not depend on managed/session/provider ownership;
- exact authoritative inputs and profiles determine observable semantics;
- shared published artifacts are immutable;
- retained state is bounded and reclaimable;
- a clean cutover leaves one production path;
- public/wire/persistent compatibility has an explicit owner;
- required mapping correctness is not weakened to save optional source-map work.

Changing one requires an accepted ADR that explicitly supersedes the affected rule, updates `architecture.md`, identifies downstream blocks/evidence invalidated, and re-ratifies affected charters. A local benchmark win is insufficient.

## 2.2 Repository fact or product obligation

A repository fact describes the current tree, current consumers, current compatibility obligations, current toolchain, or current product maturity. It must be established by A0–A6 evidence and may change when the checkout changes.

Examples include:

- which parser currently owns an operation;
- whether an external protocol has released consumers;
- which Svelte operations are supported;
- which open PRs alter architecture-affecting boundaries;
- which commands actually execute non-zero tests;
- current competitor versions and benchmark corpus.

A false or changed fact stops affected work and refreshes the baseline/lock. It does not justify a compatibility shim or second owner.

## 2.3 Evidence-selected tactic

A tactic is a private or operational choice whose alternatives preserve every constitutional invariant and public contract. Examples include:

- Rayon versus another mature bounded executor;
- exact inline/fork threshold and chunk size;
- cache admission score and store shard count;
- arena pooling limits;
- compact index layout;
- bounded PR stack depth within the stack contract;
- whether a derived projection is retained after measured reuse;
- a specific data structure with equivalent semantics and lifetime.

A tactic may change inside a ratified charter when the charter permits it and all locked gates remain unchanged. It must not be promoted into a public abstraction without a real consumer/compatibility reason.

## 2.4 Deferred research choice

A deferred research choice is intentionally unapproved until simpler architecture is measured and found insufficient. Examples include mutable within-unit parsing, a custom executor, specialized lock-free containers, allocator replacement, PGO, or SIMD.

It cannot enter a candidate under “future proofing.” It requires its own measured block and accepted premise update.

# 3. Premise ledger

A6 accepts one premise ledger. Every architecture-significant or performance-significant premise records:

```text
Premise ID:
Decision class: constitutional | repository-fact | tactic | deferred
Statement:
Evidence/source:
Confidence and uncertainty:
Falsification test or trigger:
Allowed alternatives:
Blocks and gates affected:
Action when falsified: refresh | rescope | ADR amendment | abort
Owner:
```

The ledger must include at minimum:

- current owner/consumer and compatibility assumptions;
- direct-versus-managed cost premises;
- parse reuse and retention premises;
- query/flight/cache premises;
- flow coverage and oracle premises;
- CSS reuse premises;
- provider/capability premises;
- executor/task-grain premises;
- memory plateau and allocator-slack premises;
- PR stack/reviewer-capacity premises.

# 4. Falsification behavior

When evidence contradicts a premise:

1. stop affected implementation before adding a workaround;
2. record the exact evidence and affected class;
3. preserve work whose correctness is independent of the premise;
4. refresh a repository fact, change an allowed tactic, or open an ADR amendment as required;
5. invalidate downstream charters, gates, or approvals whose basis changed;
6. resume only from a new exact authority/lock state.

Disallowed responses include hiding the contradiction behind a flag, alternate semantics path, unbounded cache, broader trait, silent fallback, reduced test, or post-result gate relaxation.

# 5. Architecture challenge

For every Foundational block, the independent architecture challenger must attempt to disprove:

- the current owner inventory;
- the claimed minimal prerequisite closure;
- the necessity of every new owner/cache/queue/trait/version;
- lifetime and thread-affinity assumptions;
- failure and cancellation completeness;
- performance comparison equivalence;
- deletion closure;
- the classification of each contested premise.

A challenge passes when no blocking contradiction remains, not when the challenger merely prefers the proposal.
