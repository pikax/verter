# AMD-006 — Vue known-defect correction

**Status:** PROPOSED — NOT RATIFIED. This candidate has no execution authority.
**Prepared against:** local `program/architecture-lock` commit
`2493c0056b55e58f28f8df89756bd3a3ffbeed4e`, tree
`3b4a8634856ab675d81e38cb46dca89e01fe32df`.
**Amends on ratification:** [`../program-dag.toml`](../program-dag.toml), the live
program ledger, and the [`BF3.md`](../charters/BF3.md) and
[`BV1.md`](../charters/BV1.md) charters; introduces
[`../charters/BV0.md`](../charters/BV0.md).

## 1. Binding maintainer direction and product boundary

For Vue VDOM, Vapor, and SSR findings produced by BF2/BF3 conformance probes,
production compilation continues returning success and generated Vue output. BF3
must not add a typed non-success, publication guard, artifact-withholding path,
known-divergence allowlist, temporary tracker, or backlog mechanism for those
findings.

Confirmed defects are corrected in the compiler. This direction supersedes the BF3
worktree disposition recorded as "track, don't retract"; that record must not land
and creates no ongoing program authority.

The first two named, bounded corrections are the missing `__expose()` emission in
every non-inline `<script setup>` cell and the dropped VDOM `<slot>` fallback
render-caching / `CACHED` patch-flag optimization. The full 36-cell BF2 Vue seed
matrix contains additional genuine defects beyond those two. BV0 must correct the
named defects first and then close the complete bounded seed domain under its
charter; this amendment corrects, rather than suspends, the conformance-golden rule
that an enabled successful cell carries no semantic known-divergence.

## 2. Amended DAG

The amended region is:

```text
B1 -> BF1 -> BF2 -> {BV0, BF3}
{BV0, BF3} -> {B2, B3}
{B2, B3} -> B4
B4 -> {BV1, BS1}
{BV1, BS1} -> B5 -> B6
```

This same commit adds the following machine-readable row to
[`../program-dag.toml`](../program-dag.toml):

```toml
[[block]]
id = "BV0"
name = "Immediate Vue known-defect correction"
class = "subsystem"
predecessors = ["BF2"]
```

The B2 and B3 predecessor rows both become:

```toml
predecessors = ["BV0", "BF3"]
```

All tracked program-state shapes and validators add the same `BV0` identity. BV0 and
BF3 may overlap only after an exact writable-ownership proof demonstrates disjoint
Vue and Svelte production files, tests, generated artifacts, manifests, and
lockfiles. Without that proof they serialize.

## 3. BV0 charter

On ratification, the full [`BV0` charter](../charters/BV0.md) is ratified verbatim.
BV0 immediately corrects the genuine Vue VDOM, Vapor, SSR, assembly, and mapping
defects exposed by the exact 36-cell BF2 Vue seed matrix while preserving every
public route's successful result contract. It owns source-root-cause corrections and
independent controls within that bounded domain, but must stop with
`RESCOPE_REQUIRED` rather than introduce B3/B4 authority, change a ratified public
contract, or substitute any guard, tracker, waiver, fixture-specific branch, or
silent deferral.

## 4. BF3 charter amendment

BF3's Vue VDOM/Vapor/SSR runtime-render rows are removed from its retraction and
tracking scope and assigned to BV0 correction. BF3 retains the original procedure
for in-scope Svelte and non-Vue-runtime reachable-success cells.

BF3 must probe BF2's exact `svelte@5.56.8` client cells. Results against
`svelte@5.56.3` do not satisfy that exit. Svelte server's existing typed
`ServerGenerate` refusal is recorded as an already non-successful cell and receives
no new production mechanism.

BF3 cannot accept until the exact Svelte client inventory and remaining in-scope
product/route inventory are exhausted. B2/B3 additionally wait for BV0 acceptance.
The corresponding [`BF3.md`](../charters/BF3.md) edit narrows its Objective, Required
procedure per successful cell, and owned scope away from Vue VDOM/Vapor/SSR; adds the
exact-version client probe and existing server-refusal requirements; and amends its
Required exits with the exhausted-inventory and BV0-acceptance waits. Its whole-cell
retraction mechanics remain intact for the domain it still owns.

## 5. BV1 preservation amendment

BV1 remains after B4 and retains its complete existing charter. Its required exits
additionally prove that every BV0 correction survives the final B2–B4 substrate and
that the exact BV0 seed pack remains green. BV1 may replace a BV0 implementation only
with an accepted equivalent correction; it may not reintroduce a corrected defect or
convert one into a refusal or tracked divergence. This preservation requirement is
materialized in [`BV1.md`](../charters/BV1.md).

## 6. Worktree disposition

The Vue candidate-production commit may be carried into BV0 only as part of a
non-vacuous conformance gate. The "track, don't retract" deviation record is
superseded and excluded from landing. No replacement tracking artifact is created.
That superseded record lived in an isolated implementation worktree, which is
excluded from this package and from landing.

## 7. Deviation memo

The failed assumption, measured evidence, affected invariants, consequences, and
recommended amendment backing this package are recorded in the
[`Vue known-defect correction deviation memo`](../evidence/vue-known-defect-correction/deviation-memo.md).
That record applies the repository's architecture-deviation format and binds the
scope change to correction rather than retraction, tracking, or silent divergence.

## 8. Exact ratification action

After the amendment package, new charter, DAG, state-shape updates, and independent
architecture/conformance/governance reviews bind one exact candidate commit and tree,
the designated maintainer records:

> Ratify AMD-006 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`,
> tree `<bundle-tree-oid>`; confirm that Vue VDOM/Vapor/SSR production
> compilation remains successful with no BF3 retraction or temporary tracking;
> authorize BV0 as the immediate correction owner for the exact BF2 Vue seed
> domain; narrow BF3 to its remaining Svelte and non-Vue-runtime inventory;
> amend the DAG so B2 and B3 require both BV0 and BF3; require BV1 to preserve
> every BV0 correction on the final substrate; and authorize no B2/B3 dispatch
> until both BV0 and BF3 are accepted.

On ratification this amendment supersedes only the conflicting BF3 Vue-retraction
scope in AMD-005 §5 and §12 and the BF3 "track, don't retract" worktree disposition.
It does not touch AMD-005's compatibility-domain locks, oracle/exclusion rules,
capability matrix, or performance-lock process; all remain in force unchanged.
