# D3P production sizing investigation (rev11.flow)

- Status: recorded (implementer investigation under `contracts/sizing.md`)
- Date: 2026-09-06
- Subject: `charters/rev11-flow/D3P.md` `max_production_loc = 800`, `rescope_loc = 1500`,
  `max_production_files = 8`, `rescope_files = 12`
- Scope: D3P only; no charter field, budget, or ledger row changes

## How the footprint was derived

From the COMPLETE candidate diff rather than from the module inventory, so the
count cannot omit a file the patch touched. `<branch-base>` is this candidate's
own base — the tip of the stacked predecessor it builds on, not the merge base
with the default branch, which would fold the predecessors' footprints into
this node's.

The table has two columns and therefore two commands, and BOTH are stated. An
earlier revision named only the first, so its code-line column could not be
reproduced at all and both columns had drifted from the tree by the time they
were read: a record whose numbers cannot be re-derived from the commands it
prints is a transcription, not a derivation.

Raw added lines, per file:

```
git diff --numstat <branch-base>..HEAD -- 'crates/**/src/**'
```

Code lines — the same added lines with blank and `//`-only lines excluded:

```
git diff <branch-base>..HEAD -- <file> \
  | grep '^+' | grep -v '^+++' | sed 's/^+//' \
  | grep -vE '^[[:space:]]*$' | grep -vE '^[[:space:]]*//' | wc -l
```

An earlier revision of this record enumerated the module inventory instead and
therefore missed every file the candidate touched outside it. That revision's
file-axis conclusion was drawn from incomplete evidence; the table below is the
whole diff, and the mutation-boundary section states what happened to the files
the earlier count omitted.

## What was measured

Every source file the candidate changes, measured against the branch base:

| file | added raw lines | code lines (blank/`//`-only excluded) |
| --- | --- | --- |
| `crates/verter_session/src/project_semantic_dispatch/flow_products.rs` | 1550 | 1001 |
| `crates/verter_session/src/project_semantic_dispatch/flow_solve.rs` | 24 | 8 |
| `crates/verter_semantic/src/analysis/flow/flow_graph.rs` | 9 | 4 |
| `crates/verter_session/src/for_tests.rs` | 8 | 4 |
| `crates/verter_session/src/project_semantic_dispatch/mod.rs` | 5 | 1 |
| `crates/verter_session/src/flow_gap_retraction_tests.rs` | 3 | 0 |
| **total** | **1599** | **1018** |

Six files against `max_production_files = 8` and `rescope_files = 12`; two crates
against `max_related_packages = 2`. Two of the six are not production paths at
all: `for_tests.rs` is the crate's test-support re-export surface, and
`flow_gap_retraction_tests.rs` is a test module whose only change is comment
text. The file and package axes are inside every declared line either way.

The LOC axis straddles the numeric signal depending on the counting convention:
1018 under a code-line count (inside `rescope_loc = 1500`, above the 800 planning
reference); 1599 under a raw added-line count (across `rescope_loc` by 99 lines).
`contracts/sizing.md` fixes no convention, so this record investigates rather than
assuming the favourable one.

## Mutation boundary

The charter's mutation boundary admits the named production surfaces only, and
excludes sibling ownership. Measured over the same complete diff, every changed
source path is inside it, with one recorded exception:

- `flow_products.rs`, `flow_solve.rs`, `mod.rs` — charter-named production files.
- `for_tests.rs` — the crate's existing test-support surface, re-exporting the
  new module for the charter's own discriminating test legs.
- `flow_gap_retraction_tests.rs` — comment text only, inside the acquired
  `flowslice` conflict domain; no executable line changes.
- `flow_graph.rs` — the one cross-crate line, a read-only graph accessor,
  recorded separately in
  `2026-09-06-d3p-flow-graph-node-enumeration-surface.md`.

An earlier revision of the candidate also carried changes to the nominal
relation, member lowering, carrier, semantic-query, component-meta audit and
component-meta registry surfaces. Those are a sibling authority's files, none of
them is required by the product lattice, and they have been restored to their
pre-candidate content so that the sibling's reviewed surface is unchanged by
this candidate. The correctness work they contained is not carried here; it needs
its own scope and its own review.

## Finding: the patch is one reviewable outcome

1. **One authority, one file.** The overshoot is concentrated in a single new module
   that implements exactly the one charter outcome: the product key/value/store types,
   the one transfer route, the one join route, and the one worklist that drives them to
   a fixed point. There is no second authority hiding in it — no second domain enum, no
   second store, no private semantic reducer, no public product query. Splitting the
   store from the transfer, or the transfer from the join, would produce fragments that
   are not independently acceptable: none of them answers a question on its own, and each
   would land dark with no consumer at all rather than dark with a driven test surface.

2. **A third of the module is the ownership contract the charter's evidence policy
   asks for.** 549 of the 1550 lines are blank or comment; the module header alone
   states five load-bearing ownership boundaries (product-state algebra versus semantic
   type algebra, the one domain registry, the one store with a module-private write
   surface, the structurally-unwarmable degraded arm, and binding identity that is both
   stable and injective).
   Deleting that prose would move the candidate under any convention's line while making
   it harder to review, which is the outcome `contracts/sizing.md` explicitly forbids
   ("do not split or pad a coherent change merely to hit a number").

3. **The mandatory-rescope triggers do not fire.** The candidate spans two related
   crates, not three unrelated ones, and combines no public/wire, unsafe, concurrency,
   or lifetime work with another major concern. The one line of cross-crate surface is a
   read-only graph accessor recorded separately in
   `2026-09-06-d3p-flow-graph-node-enumeration-surface.md`.

4. **No hidden independently landable work was exposed.** Every item in the module is
   reachable from the product suites; nothing in it is a general-purpose helper that
   another node would want, and nothing in it belongs to the evaluator cutover, which
   this candidate deliberately does not perform.

## Decision

The scope is coherent and the candidate lands as one outcome. The budget fields are not
amended: the planning reference stays 800 and the signal stays 1500, so a future flow
block inherits the same comparison rather than a ratchet. This record is the
investigation the signal calls for; it is not an authorisation to exceed the reference
again without one.
