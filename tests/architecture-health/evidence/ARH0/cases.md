# ARH0 evidence cases

## ARH0-ratification (accept)

Clean products validate: inventory totals equal row sums, all module/consumer/candidate paths exist on the candidate tree, every owner resolves to a repo-authority train (charters dir) or DAG node id or an explicitly annotated planAuthorityGap id, and each implemented capability pin appears verbatim in its pinned source file.

## ARH0-authority (reject)

- `stale-gap-annotation`: a planAuthorityGap id that has appeared in repo authority since recording.

## ARH0-inventory (reject)

- `totals-mismatch`: population totals drifted from row sums (dirty twin bumps productionLoc by 1).
- `missing-module`: a module row whose directory does not exist.

## ARH0-ownership (reject)

- `unknown-owner`: train without charters dir and without gap annotation (dirty twin invents `expansion.nope` for `crates/verter_parser`).
- `duplicate-module`, `owner-without-responsibility`: structural discipline of the map.
- `inventory-module-unowned`: an inventory crate/package in neither `owners` nor `debt-register.candidatePath` (silent absorption; dirty twin drops the `crates/verter_parser` owner row).
- `module-double-disposition`: a module with both an owner row and a debt row (dirty twin adds a debt row for the owned `crates/verter_parser`).

## ARH0-god-evidence (reject) — ARH0-AC2 discriminator

- `god-without-responsibility-evidence`: a god-module-candidate row reduced to one responsibility and size-only evidence, or to touches-only coupling (a touch count is churn, not coupling; the dirty twin strips `fanIn` from the live `flow_slice_content.rs` row). A large file alone never declares a god module; coupling evidence is a shared-commit count or fan-in.
- `split-module-reclassified-without-new-evidence`: the retired Phase 11 target `meta_resolve.rs` pushed back into godModuleCandidates with only pre-split history. Previously split modules need fresh measured multi-responsibility evidence.

## ARH0-capability (reject)

- `version-not-pinned-in-source`: fabricated `5.99.0` Svelte pin that does not appear in `package.json`.
- `implemented-without-consumers`, `planned-without-uncertainty`, `missing-consumer`, `missing-version-source`: no support claim without named consumers; required-planned rows must state uncertainty and have no live consumers.

## ARH0-debt (reject)

- `unknown-disposition-owner`: disposition owner `SIMP99` outside authority and gap list.
- `debt-without-disposition`, `missing-candidate-path`: every debt row carries a concrete path and a disposition.
