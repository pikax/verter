---
ruling_id: "COALESCER-CLOSURE-IS-NAMED-DISPOSITION"
type: "maintainer-ruling"
date: "2026-08-23"
date_source: "stated"
binds: ["K3", "G2", "H2", "TCM4"]
source_file: "MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md"
summary: "K3 closes when every NAMED baseline row in its same-key coalescer inventory is dispositioned — absent, or converged with its recorded final owner. A clean cutover of the named inventory is enough; the obligation to PROVE ABSENCE of unnamed coalescers by re-running a documented search is removed, and an unnamed cell is no longer a close failure. The search recorded in charters/K3.md survives as evidence of how the inventory was built, never as an acceptance gate. This does NOT license creating a second generic coordinator: that design prohibition stands unchanged (TCM4 §5). The durable confinement rail is an unforgeable publication/commit capability over authoritative effects, owned by G2/H2 and not implemented."
supersedes:
  - document: "charters/K3.md"
    claim: "K3 close is inventory-closed; a named deletion row that is absent is not enough, an enumeration hit with no disposition is a blocking defect, and re-running the enumeration method is an acceptance condition."
  - document: "architecture.md"
    claim: "Same-key coalescers are inventoried by re-runnable enumeration; an unnamed same-key coalescer is a K3 close failure."
  - document: "program.md"
    claim: "Same-key coalescers are inventoried by the re-runnable enumeration in charters/K3.md; an unnamed cell is a close failure. K3 close re-runs the enumeration method. An unnamed same-key coalescer on the activated path fails TCM4 close; re-run the K3 enumeration method on the activated tree."
  - document: "charters/TCM4.md"
    claim: "An unnamed same-key coalescer on the activated path is a TCM4 close failure; re-running the enumeration method producing no unclassified hit is exit-criterion evidence."
superseded_by: []
contradicts: []
notes: "Resolves a live authority contradiction: CLAUDE.md forbids a landed name/token/text/grep/AST scanner as an enforcement rail, while architecture.md and program.md required exactly such a search and made an unnamed cell a close failure. Labelling the receipt interim did not resolve it, because re-running it remained an acceptance condition. The design prohibition and the search obligation are deliberately separated by this ruling — only the second is removed."
---

# MAINTAINER RULING — coalescer closure is NAMED disposition

**Status:** RATIFIED by the maintainer, 2026-08-23.
**Scope:** the closure condition of the same-key coalescer inventory in
[`charters/K3.md`](../charters/K3.md), and the statements in
[`architecture.md`](../architecture.md), [`program.md`](../program.md) and
[`charters/TCM4.md`](../charters/TCM4.md) that restated it.

## The ruling

> K3 closes when everything named is dispositioned. A clean cutover is enough. We do not keep testing
> for unnamed coalescers.

Operationally:

1. **Closure is disposition of the NAMED population.** Every row in K3's inventory must be absent, or
   converged with its recorded final owner. That is the whole bar.
2. **No proof of absence is required.** K3 makes no claim that no unnamed behavioural coalescer exists
   anywhere in the tree, and needs none. An unnamed cell is **not** a close failure.
3. **Re-running the search is not an acceptance condition** — for K3 or for TCM4 on its activated tree.
   The documented search in `charters/K3.md` is retained as evidence of how the inventory was built.
4. **Independent adversarial searches remain welcome** and may add rows at any time; a new hit is
   classified in the inventory as usual. A clean search is never acceptance evidence.

## What this does NOT license

Creating a second generic coordinator, a duplicate `FlightCell`, or an unnamed same-key coalescer on any
path remains **forbidden as a design rule** — `charters/TCM4.md` §5 is unchanged, as are the equivalent
prohibitions in G2's and H2's charters. This ruling removes an obligation to *prove absence by search*.
It does not weaken the prohibition on *creating* the thing.

## Why — the reasoning, recorded so this is not relitigated

**Four consecutive adversarial passes each found a live coalescer the commands could not see.** Each pass
widened the command set on the previous round's miss; the next pass then found something the widened set
still missed. The cells were real and are rowed in the inventory: two cold-start validation gates in the
tsgo API clients, a relay initialize-witness `watch`, an editor debounce, a plugin refresh fold, a
per-project bootstrap reservation, the scheduler's demand-merge index and waiter groups, and its file-node
map — the last being the strongest join in the inventory, whose own source comment states that a losing
creator blocks on the shard lock and reuses the winner's value.

**The architecture consult found the limit is structural, not effort.** No finite vocabulary can prove
absence of this behaviour unless the permitted construction syntax is first restricted structurally. It
demonstrated the newest command still missing `or_insert_with(|| expr)` (`verter_scheduler`
`scheduler.rs:2304`, `:3496`) and `or_default()` (`verter_lsp` `tsgo/carrier_sync.rs:153`) — further
spellings of cells the inventory already rows by other means. The number of lexical forms for one
behaviour is not bounded, so a command set chasing them cannot terminate.

**The receipt was functionally a landed scanner guard.** `CLAUDE.md` requires landed enforcement of an
invariant to be structural — compiler, type system, visibility, sealed traits, a real tool — and never a
name/token/text/grep/AST scanner over the source tree, admitting name-keyed scanners only as transient
WIP. Because re-running the search was an acceptance condition, the receipt was such a guard in
substance. Labelling it "interim" did not resolve the contradiction; only removing the gate does.

**The claim was falsifiable, just never established.** "No unnamed coalescer remains" is refuted by a
single counterexample, and four rounds produced counterexamples. Calling it unfalsifiable was wrong. It
is falsifiable and unestablished, which is precisely why it cannot serve as a closure condition.

## The durable target

The real confinement rail is an **unforgeable publication/commit capability governing authoritative
effects**: coalescing becomes structurally impossible off-rail because only a capability holder can
publish a result, rather than because the syntax for building a cell was forbidden. That is
implementation work on **G2's and H2's** own surfaces, not a contracts block's to carry. It is recorded
here so the option is not lost. **It is not implemented, and no block's closure waits on it.**

A rail proposed during this work — deny the raw primitives at the crate boundary plus one sealed
constructor — was assessed **insufficient** and should not be re-proposed: Cargo dependencies are
crate-wide and cannot be scoped to the surfaces that need them; `std`'s map, cell and atomic types cannot
be denied at all; TypeScript's `Map`, `Promise` and timers are ambient in the language; and a sealed
trait constrains who may *implement* it, not who may compose the same behaviour directly out of
primitives.
