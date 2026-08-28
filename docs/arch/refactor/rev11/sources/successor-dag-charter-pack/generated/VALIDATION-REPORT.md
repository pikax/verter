# Proposal package validation report

## Automated checks passed

- All TOML files parse with Python `tomllib`.
- Proposed canonical DAG copies equal the generator working copies.
- Node IDs are globally unique across the four DAG modules.
- Node counts: 27 static successor nodes, 30 generated Native Checker feature slices, one generated family convergence node (`NCKF0`), 58 total.
- Every DAG node has a charter at its declared path.
- Charter metadata matches node ID, name, train, product, kind, predecessors, conditional predecessors, and charter path.
- Every static charter contains the complete implementation-grade section set and at least five detailed subblocks.
- Every generated slice charter contains scope, architecture, subblocks, acceptance, forbidden design, budgets, and verification.
- The required Native Checker manifest slice set exactly equals the generated `NCF-*` DAG node set.
- `NCKF0.predecessors` exactly equals every manifest row marked required.
- `NCK8` depends on `NCKF0` and has no external “all slices complete” assertion.
- EPR2/EPR3 are optional and require explicit maintainer authorizations.
- Deliberate architecture splits are present: NCK6/NCK7/NCK8, LSO4/LSO5/LSO8, and EPR4/EPR5.
- Known stale proposal phrases and placeholders are absent from final DAG/charter/support artifacts.

## Manual/live-authority checks still required

This validation proves internal package consistency only. The canonical amendment workflow must still:

- replace proposal commit/tree pins with the live branch basis;
- verify every predecessor against current accepted receipts and release/activation state;
- generate exact legacy source line atoms and SHA-256 text digests;
- generate the complete legacy path disposition catalog from the current Git tree rather than relying on the seed entries;
- verify current conflict-domain path/symbol ownership and acquire leases;
- reconcile the proposed `security-3` profile against the live review catalog;
- register external requirements and module/root indexes under the live schemas;
- run canonical authority, cycle, reachability, optional-predecessor, source-coverage, gate, RED/GREEN, and independent review workflows.
