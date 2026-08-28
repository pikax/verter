# C1 landing subject and equivalence scope

## Reviewed subject

- Base: `d1f3d50a948597f036868543b9bb21acacd730ff`, tree
  `2e7cf8637ec5c52b0fa04572d99672b052f1f85f`.
- Production: `2820cf2eb790caffdb69f59bc20402d7d0a6647b`, tree
  `ef8efbec06c8e87d1d6d72d9ea8e69fa624f515b`.
- Evidence child: `713651edd3c9ab629ea5c68380238fb4bafa6711`, tree
  `1e112ba5218803e6166c2f4065732179fa339ebe`.
- Final reviewed candidate: `c46c60c52f33784356a9f1d7fade31627486e874`, tree
  `031c84419aaa1bc851c24e31add987c9ad678ba8`.

All three are a direct linear chain. `713651edd…` adds only
`a6/final-round6-performance.md`. `c46c60c5…` adds only the registered round-6 performance ruling and
the authority-registry/program-state registration. The candidate therefore preserves the production,
test, harness, corpus, toolchain, configuration, and architecture-contract bytes of `2820cf2e…`.

## Candidate path and identity scope

`changed-paths.tsv` is the exact `git diff -M40% --name-status <base>..<candidate>` result.
`identity-map.md` binds moved definitions and records that both possible `project_membership.rs`
paths are nonexistent; `ProjectMembership` remains workspace-owned in the existing `membership.rs`.

The final review receipts name `c46c60c5…` exactly. They do not attach to the later commit that stores
this bundle.

## Landing-equivalence rule

The reviewed canonical patch is the binary/full-index/no-renames Git delta from the base above to
`c46c60c5…`. A squash may inherit those reviews only when its accepted-base→accepted delta is exactly
equal, generated-output digests match, and no manual conflict resolution occurs.

The evidence-assembly commit containing this file is a post-review carrier. It is not silently folded
into the reviewed patch. If it is to be included in C1's landing delta, the exact new tip requires
impact-bounded reattestation by every required mandate and becomes the new frozen candidate. If it is
registered trunk-side outside the C1 squash, `c46c60c5…` remains the reviewed candidate. Any production,
test, contract, charter, DAG, harness, corpus, toolchain, configuration, or threshold change voids the
bridge and requires a fresh freeze/review.
