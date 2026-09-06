# D3P production sizing investigation (rev11.flow)

- Status: recorded (implementer investigation under `contracts/sizing.md`)
- Date: 2026-09-06
- Subject: `charters/rev11-flow/D3P.md` `max_production_loc = 800`, `rescope_loc = 1500`,
  `max_production_files = 8`, `rescope_files = 12`
- Scope: D3P only; no charter field, budget, or ledger row changes

## What was measured

Production files D3P owns, measured against the branch base:

| file | added raw lines | code lines (blank/`//`-only excluded) |
| --- | --- | --- |
| `crates/verter_session/src/project_semantic_dispatch/flow_products.rs` | 1484 | 981 |
| `crates/verter_session/src/project_semantic_dispatch/flow_solve.rs` | 24 | ~20 |
| `crates/verter_session/src/project_semantic_dispatch/mod.rs` | 22 | ~18 |
| `crates/verter_semantic/src/analysis/flow/flow_graph.rs` | 9 | ~7 |
| **total** | **1539** | **~1026** |

Four production files against `max_production_files = 8`; two crates against
`max_related_packages = 2`. The file and package axes are not in question.

The LOC axis straddles the numeric signal depending on the counting convention:
~1026 under a code-line count (inside `rescope_loc = 1500`, above the 800 planning
reference); 1539 under a raw added-line count (across `rescope_loc` by 39 lines).
`contracts/sizing.md` fixes no convention, so this record investigates rather than
assuming the favourable one.

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
   asks for.** 503 of the 1484 lines are blank or comment; the module header alone
   states five load-bearing ownership boundaries (product-state algebra versus semantic
   type algebra, the one domain registry, the one store with a module-private write
   surface, the structurally-unwarmable degraded arm, and stable binding identity).
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
