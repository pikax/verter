# E1 scope disposition: the graph protocol consumer closure landed above its planned ceilings

- Status: accepted
- Date: 2026-09-05
- Concerns: `charters/rev11-public-typeinfo/E1.md` budget section (800
  production LOC / 8 files / 2 related packages; mandatory rescope at
  1,500 / 12 / 3)
- Node: E1 — TypeExpr component-meta graph protocol consumer closure
  (train rev11.public-typeinfo)
- What this record is: the missing explicit disposition for a
  scope-deviating candidate. It records the measured drift, the
  coherence explanation, and one test-home clarification. It does not
  amend the charter header or any DAG machine field — those stay as
  planned references, per the sizing policy that budgets are planning
  inputs, not acceptance gates, and that material drift is something to
  EXPLAIN, never a reason to split a coherent implementation or pad a
  weaker one.

## Measured drift

The implemented boundary is one coherent producer→wire→consumer closure
for the resolve-symbol graph operation:

| surface                            | production LOC (fix round) |
| ---------------------------------- | -------------------------- |
| `verter_protocol` bounded encoder  | ~1,032                     |
| `verter_session` executor + raise  | ~723 + 38 raise glue       |
| `@verter/typeinfo` DTO consumer    | ~604 + 20 index glue       |
| `verter_protocol` envelope aliases | ~17                        |
| total                              | ~2.4k                      |

That is roughly three times the 800-LOC target, past the 1,500
mandatory-rescope trigger, and across three packages/crates
(`verter_protocol`, `verter_session`, `@verter/typeinfo`) against a cap
of two — the three are not unrelated: they ARE the protocol's three
tiers (wire encoder, host executor, TS consumer), which is why the
mutation boundary naming all three was coherent even though the package
count exceeded the planning number.

## Why the boundary is not splittable at this size

The independently acceptable outcome is a CLOSED consumer loop: a
bounded terminal-`TypeExpr`→`SemanticTypeGraph` encoder whose budgets
are enforced by a validating executor, answered to a typed TS decode.
Each tier alone is unpublishable:

- the encoder without the executor is dead code (no validated entry
  reaches it, and the unbounded-export deletion contract means nothing
  else may call it);
- the executor without the TS consumer re-opens general `TypeExpr`
  transit (the displaced route's consumers have nowhere to go);
- the consumer without the producer is a decoder for graphs nobody
  emits.

Splitting the closure to fit an 800-LOC planning line would land three
half-authorities, each of which the charter's own deletions section
forbids. The drift is therefore explained, not cured by splitting.

## Test-home clarification

The charter declares test homes `crates/verter_protocol/tests` and
`packages/typeinfo/tests`. The executor suite lives at
`crates/verter_session/tests/cases/g_type/typeinfo_graph_query_executor.rs`
because it drives `VerterHost` end-to-end (fixture upsert → envelope →
audit record), which only an integration test in the owning crate can
do. `crates/verter_session` is a declared production surface of this
node; the test-home list is a documentation gap, recorded here rather
than papered over by relocating the suite away from the code it
discriminates.

## What this record cannot do

Same as the TCM0R amendment: no artifact inside the tree distinguishes
a disposition a maintainer accepted from a document asserting one was
accepted. The numbers and the coherence argument are properties of
committed bytes a reader re-checks without trusting this paragraph; the
acceptance is the maintainer's.
