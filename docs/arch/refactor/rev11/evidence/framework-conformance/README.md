# AMD-005 framework-conformance evidence index

This is a package-preparation record, not compiler acceptance. Every `blocked`,
`VERIFY`, or proposed row remains work for its owning block. The package was prepared
without modifying production compiler code or the protected primary checkout.

| artifact | purpose |
|---|---|
| `current-state.md` | freshly resolved repository, branch, worktree, and program facts |
| `package-checklist.md` | one-to-one routing for every required deliverable |
| `version-domain.md` + `oracles/*/{package-lock.json,closure.tsv}` | immutable source/package domains and full exact closures |
| `product-inventory.md` | current and target product/route ownership |
| `vue-options.tsv`, `svelte-options.tsv` | complete official semantic option classifications |
| `capability-matrix.tsv` | proposed route/profile/capability/maturity lock |
| `vue-official-cases.tsv`, `svelte-official-cases.tsv` | source-identity seed case ledgers |
| `generate-official-case-manifests.mjs` | deterministic manifest extractor |
| `emitter-mapping-dispositions.tsv` | one proposed disposition per current owner |
| `bf3-safety-retraction-scope.md` | bounded reachable-success probe scope |
| `performance-impact.md` | pre-candidate performance cells and lease rules |
| `program-state-transition.md` | 51-to-56 state amendment and exposure rules |
| `reviews/README.md` | exact paths and briefs for independent challenges |

The official-case manifests are seeds. BF2 must runner-enumerate dynamic Vue cases,
resolve all blocked rows, and attach profile/product evidence. They cannot be treated
as an accepted conformance pack.

Generation from the exact clean upstream trees produced 2,003 Vue test declarations
across the five compiler packages and 3,457 Svelte sample/suite rows. The committed
manifest SHA-256 values are respectively
`76cbe75f5dbee5b6014ab44ec4b5e58ff77a65839fafdc40d7328dda30f456ba` and
`09eccfbe2be9a97b3f5f412d30109d346773917afe69dc74b1e59e75dcd3a42e`.
