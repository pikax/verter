# AMD-009 — BF3 audit and immediate correction blocks

**Status:** **RATIFIED** by the designated maintainer's 2026-08-16
[`product ruling`](../evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md),
as recorded in §8. This ratification does not accept BF3 or unlock B2/B3.
**Prepared against:** program checkout
`75bf4f722d2a0f1c99efae8d15d2eb811f16b168`, tree
`54817b786bb326de5b9a823d6c5536480d1cd916`; BF3 worktree tip
`885961a76f13e7022a6cb3bb5aa31558fc57da64`, tree
`c63e6d45606fd08fb086246d865b28150362de51`.
**Amends only on ratification:** [`../program-dag.toml`](../program-dag.toml), the
live program ledger through its returning orchestrator, the
[`BF3.md`](../charters/BF3.md) charter, and the conflicting BF3 text identified in
§2; introduces [`BS0.md`](../charters/BS0.md),
[`BA0.md`](../charters/BA0.md), [`BCSS0.md`](../charters/BCSS0.md), and
[`BRT0.md`](../charters/BRT0.md).

## 1. Binding direction and product boundary

BF3 is a pre-B2/B3 conformance-exhaustion and correction-dispatch audit. It builds
no production guard, typed refusal, artifact-withholding path, whole-cell
retraction, known-divergence list, or runtime tracking mechanism for a compiler
defect. A supported request that produces wrong output receives a discriminating
regression and a correction in its owning layer. Safety is the DAG lock: downstream
work does not dispatch while an immediate correction predecessor remains
unaccepted.

A typed production refusal is permissible only for a real, independently specified
capability boundary decided from the typed request before compilation. The existing
Svelte `ServerGenerate` refusal is the example. A refusal must never be selected by
fixture identity, a known compiler defect, an oracle mismatch, syntax chosen because
it currently miscompiles, or a version-specific known-divergence list. Such a
capability refusal is tested as contract behavior and carries no BF3 removal ID.

The settled per-finding classifications and owners are the ratified
[`dispositions.md`](../evidence/BF3/dispositions.md). This amendment does not
re-class, rename, invent, or reopen any row, including RA-1 and RA-2.

## 2. Explicit supersession of the retraction mechanism

On ratification, this section expressly supersedes the following text. The old text
may remain in historical evidence, but it has no continuing implementation
authority where this section conflicts with it.

1. **BF3 title and objective.** “Known-wrong successful-cell safety retraction” and
   its retraction objective are replaced by “Conformance exhaustion and correction
   dispatch audit” and the audit objective ratified through the rewritten charter.
2. **BF3 procedure steps 3–5.** Detect-before-publication, typed non-success with
   artifact withholding, and whole-cell retraction are replaced by root-cause
   classification, an independently discriminating regression, and dispatch to the
   named immediate correction owner. BF3 does not change production publication.
3. **BF3 procedure step 7.** Guard-deletion ownership and a removal acceptance are
   replaced by the correction block and its permanent acceptance/test ID. There is
   no guard and no removal ID.
4. **BF3's retained-retraction paragraph.** The sentence retaining the old procedure
   for Svelte and non-Vue-runtime successful cells is superseded. Those cells are
   audited under one cross-framework rule: genuine defects are corrected by their
   root-cause owners.
5. **BF3's “no broad correction” abort logic.** A broad repair is not grounds to
   retract a cell. BF3 instead stops and a rescope names the appropriate correction
   block; the repair proceeds only under that owner's ratified charter.
6. **BF3's required-exit guard/removal clause.** “Every failure has a guard/whole-cell
   retraction ... and removal ID” is replaced by exact evidence, a discriminating
   regression, root-cause classification, a named owner, and a correction
   acceptance/test ID. No removal ID exists.
7. **AMD-005 §5 and §12, plus the conflicting §15.1 recorded-ratification
   wording.** Section 5's allocation of BF3 to retract reachable successful cells
   proven wrong and §12's requirement to return typed non-success, withhold every
   product, and retract the entire cell are superseded for retained Svelte and
   other non-Vue-runtime successful cells. The §15.1 recorded-ratification wording
   accepting AMD-005's charters and amendment body is likewise superseded only
   insofar as it would otherwise continue that BF3 authority in those domains. The
   superseded AMD-005 text remains historical evidence; its Vue and oracle body is
   untouched and otherwise remains in force.
8. **AMD-006 §4.** Its retention of BF3's original retraction procedure and
   whole-cell mechanics for Svelte and non-Vue-runtime cells is superseded.
9. **AMD-006 §8.1.** Its `RETROACTIVE-NO-FORWARD-ONLY` ruling is superseded. That
   ruling explains why an implementer could not silently deviate from ratified text;
   it no longer authorizes the architecturally rejected mechanism in any framework
   domain.
10. **The BF3 ledger note.** The returning program orchestrator, and no author of this
   package, replaces it with the following text only after ratification:

   > BF3 is a pre-B2/B3 conformance-exhaustion and correction-dispatch audit under
   > ratified AMD-009. It adds no production guard, typed refusal,
   > artifact-withholding, retraction, or runtime tracking mechanism. Inventory
   > exhaustion requires actual results; every genuine failure has evidence, an
   > independently discriminating regression, root-cause classification, a named
   > immediate correction owner, and a correction acceptance/test ID. BF3 may close
   > only after AMD-009 ratification and creation of mandatory B2/B3 predecessor
   > edges for BA0, BS0, BCSS0, and BRT0. B2/B3 remain locked until BV0, BF3, BA0,
   > BS0, BCSS0, and BRT0 are accepted. The existing Svelte ServerGenerate refusal
   > is a contract-defined pre-compilation capability boundary and receives no BF3
   > removal ID.
11. **The DAG edges.** The former `{BV0, BF3} -> {B2, B3}` region is superseded by
    the region in §4, which inserts all four immediate correction owners.
12. **The scope document's `BF3-RET-*` scheme.** The production-record scheme in
    [`bf3-safety-retraction-scope.md`](../evidence/framework-conformance/bf3-safety-retraction-scope.md)
    is superseded. Stable per-finding dispositions and correction acceptance/test
    IDs replace retraction records; no `BF3-RET-*` table or production consumer may
    be created.

This supersession is narrow. AMD-005's Vue domain, oracle rules, and other unaffected
body remain in force; only its conflicting BF3 authority in §5 and §12 and the
corresponding effect of the §15.1 recorded ratification are replaced for retained
Svelte and non-Vue-runtime successful cells. AMD-006's Vue correction direction,
BV0 ownership, and BV1 preservation requirement likewise remain in force; only its
conflicting retention and ratification-ruling text identified above is replaced.

## 3. BF3 audit charter

On ratification, the rewritten [`BF3.md`](../charters/BF3.md) is ratified verbatim.
BF3 must run the shipped-path Svelte authority over the exact six
`svelte@5.56.8` client cells, exhaust the remaining reachable-success product and
route inventory, prove the oracle axes with mutation controls, separate production
defects from harness and route artifacts, add an exact regression for every genuine
failure, and dispatch each genuine failure to its named correction owner.

Inventory exhaustion requires actual results; `UNPROVEN` is a blocking observation,
not an exhausted cell. Every genuine failure records its exact
request/route/profile/products/domain evidence, discriminating regression,
root-cause classification, correction owner, and acceptance/test ID. Route-parity
tests, harness mutation controls, and owner regressions replace the former cold-path
and guard tests.

`FC-ATOMIC-001` remains non-vacuous for successful results and genuine
contract-defined refusals. A success publishes all and only its requested product
set; a refusal publishes none. The empty set of BF3-created refusals does not satisfy
that exit.

BF3 may close as an audit only after this amendment is ratified and the four
correction blocks exist as mandatory predecessors of B2 and B3. Audit closure does
not accept a correction block and does not unlock downstream work.

## 4. Amended DAG

The amended region is:

```text
BF3 -> {BA0, BS0, BCSS0, BRT0}
{BV0, BF3, BA0, BS0, BCSS0, BRT0} -> {B2, B3}
```

The machine-readable proposal in [`../program-dag.toml`](../program-dag.toml)
renames and reclasses BF3 and adds:

```toml
[[block]]
id = "BF3"
name = "Conformance exhaustion and correction dispatch audit"
class = "foundational"
predecessors = ["BF2"]

[[block]]
id = "BA0"
name = "Immediate request and result atomicity"
class = "foundational-atomic"
predecessors = ["BF3"]

[[block]]
id = "BS0"
name = "Immediate Svelte correction"
class = "subsystem"
predecessors = ["BF3"]

[[block]]
id = "BCSS0"
name = "Standalone CSS source-map product correction"
class = "subsystem"
predecessors = ["BF3"]

[[block]]
id = "BRT0"
name = "Immediate route and transport parity"
class = "subsystem"
predecessors = ["BF3"]
```

The B2 and B3 predecessor rows both become:

```toml
predecessors = ["BV0", "BF3", "BA0", "BS0", "BCSS0", "BRT0"]
```

B2 and B3 remain locked until all six predecessors are accepted. This proposal does
not accept BF3, accept any correction owner, or authorize downstream dispatch.

## 5. Immediate correction charters

On ratification, the four new charters are ratified verbatim:

- [`BS0.md`](../charters/BS0.md) owns SV-1, SV-2, SV-3, and the distinct
  session-projector item SV-4. It corrects the named Svelte defects before B2/B3
  rather than waiting for post-B4 BS1.
- [`BA0.md`](../charters/BA0.md) owns distinct AT-1 and AT-2. It establishes
  independent product-request/result identities and all-or-nothing combined
  requests without taking B3/B4's final authority.
- [`BCSS0.md`](../charters/BCSS0.md) owns CSS-1 at
  `verter_compiler::css` and the standalone NAPI product boundary.
- [`BRT0.md`](../charters/BRT0.md) owns RT-1 and TR-1 and provisionally carries
  BND-1/BND-2 as `AWAITING CONFIRMATION`, exactly as dispositioned.

Each block's acceptance is a mandatory predecessor of B2 and B3. None may add a
production retraction path, fixture-specific branch, generated-output string scan,
or second semantic authority.

## 6. Exclusions

This package creates charters and proposed DAG ownership only. It implements no
compiler, session, route, transport, CSS, NAPI, WASM, or bundler correction. It
changes no public result contract and deletes no production code.

The stale root `svelte@5.56.3` pin and associated corpus migration are excluded. The
consult identified that work as a distinct conformance-infrastructure train, but no
such train is authorized by this amendment. The migration is not folded into BF3,
BS0, or post-B4 BS1, and this package adds no pin-migration block.

## 7. Exact ratification action

After the exact package commit and tree receive the required independent reviews,
the designated maintainer records:

> Ratify AMD-009 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`, tree
> `<bundle-tree-oid>`; supersede BF3's safety-retraction title, objective,
> procedures, exits, abort logic, ledger note, and `BF3-RET-*` production-record
> scheme; supersede AMD-005 §5 and §12 and the conflicting AMD-005 §15.1
> recorded-ratification wording insofar as they authorize typed non-success,
> artifact withholding, or whole-cell retraction for retained Svelte and
> non-Vue-runtime successful cells; supersede AMD-006 §4's retained
> Svelte/non-Vue-runtime retraction mechanism and §8.1's
> RETROACTIVE-NO-FORWARD-ONLY ruling; ratify BF3 as a conformance-exhaustion and
> correction-dispatch audit; create BA0, BS0, BCSS0, and BRT0 as immediate
> correction owners after BF3; require all four, together with BV0 and BF3, as
> mandatory predecessors of B2 and B3; authorize no production defect-recognition
> refusal or retraction path; keep B2 and B3 locked until every predecessor is
> accepted; and leave the separate svelte@5.56.3 pin migration unauthorized by
> this package.

Silence, merge, or a commit in this worktree was not ratification. Before the
maintainer ruling, the preparer could not ratify this amendment, accept BF3 or any
correction block, edit the live program ledger, or unlock B2/B3. Any challenged or
changed byte requires fresh reviewed identities and the designated maintainer's
explicit acceptance.

## 8. Recorded ratification

On 2026-08-16, the designated maintainer, Carlos Rodrigues / pikax, issued the
binding [`product ruling`](../evidence/BF3/maintainer-product-ruling-no-error-on-bad-output.md).
The maintainer accepted AMD-009 through that ruling, not by quoting the packet's
accept line in chat. The ruling ratifies AMD-009's product boundary and retraction
supersession on the exact scope of the reviewed package. For traceability, the
packet's accept line is recorded verbatim below and bound to reviewed package
commit `9e457ca781d3684e562d6eaea24c401e2d9849a7`, the last AMD-009 content
commit before the packet:

> Ratify AMD-009 for worktree commit `9e457ca781d3684e562d6eaea24c401e2d9849a7` on exactly the scope and terms of AMD-009 §7: supersede BF3's safety-retraction title, objective, procedures, exits, abort logic, ledger note, and `BF3-RET-*` production-record scheme; supersede AMD-005 §5 and §12 and the conflicting AMD-005 §15.1 recorded-ratification wording, and AMD-006 §4 and §8.1, only as §7 states; ratify BF3 as a conformance-exhaustion and correction-dispatch audit; create BA0, BS0, BCSS0, and BRT0 after BF3 and require all four, together with BV0 and BF3, as mandatory predecessors of B2 and B3; authorize no production defect-recognition refusal or retraction path; keep B2 and B3 locked until every predecessor is accepted; and leave the separate `svelte@5.56.3` pin migration unauthorized.

No program-branch landing SHA is recorded because it is not yet known. This
ratifies AMD-009 only: it does **not** accept BF3 or any correction block, does
**not** unlock B2/B3, and does not itself mutate the live program ledger.
