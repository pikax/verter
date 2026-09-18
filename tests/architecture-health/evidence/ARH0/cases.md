# ARH0 evidence cases

The sole owning interface is `node tests/architecture-health/ARH0/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH0/arh0.test.mjs` (dirty twins), wired into `test:scripts`. The program DAG is database-owned by the TAMA controller, so nothing here reads a DAG copy from this tree: owner ids are checked structurally (dotted lowercase train ids, uppercase node ids) and rows whose path fields bind TAMA-database DAG records instead of repo tree paths declare `provenance: "tama-dag"` with a null path.

## ARH0-ratification (accept)

Clean products validate: inventory totals equal row sums, all module/consumer/candidate paths that name repo tree paths resolve inside the repository tree on the candidate tree (tama-dag rows bind database records instead), and each implemented capability's version equals the exact value of its named source property.

## ARH0-ratification (reject)

- `manifest-case-drift`: the manifest records a case the verifier does not implement, drops one it does, or lists a case without disposition reject and the `clean products` accept twin (dirty twins add `ARH0-phantom` and drop `ARH0-god-evidence`). The manifest cannot claim checks that do not run.
- `manifest-product-drift`: a carried product schema missing from the manifest's products list, or a recorded product no file carries.
- `manifest-command-drift`: the manifest verify/test command is not the canonical command string derived from the verifier's own location (dirty twins point verify at an absent file and at `scripts/affected-tests.mjs` — an existing script that is not the ARH0 verifier).

## ARH0-inventory (reject)

- `totals-mismatch`: population totals drifted from row sums (dirty twin bumps productionLoc by 1).
- `missing-module`: a module row whose directory does not exist.

## ARH0-ownership (reject)

- `malformed-owner`: owner id outside the structural discipline — a train id without a dot, or a lowercase node id (dirty twins invent `expansion` and `simp99` for `crates/verter_parser`). Resolving ids against the real DAG is the TAMA controller's job; this tree holds no copy.
- `duplicate-module`, `owner-without-responsibility`: structural discipline of the map.
- `inventory-module-unowned`: an inventory crate/package in neither `owners` nor `debt-register.candidatePath` (silent absorption; dirty twin drops the `crates/verter_parser` owner row).
- `workspace-package-unowned`: a literal `pnpm-workspace.yaml` package entry outside the inventoried `packages/` globs (e.g. `docs`) in neither `owners` nor `debt-register.candidatePath`; the join reads the live workspace definition, parsed with full support for comment lines, inline comments and single- or double-quoted entries so every list member reaches the join (fixtures cover a comment line inside the list, a single-quoted entry and inline-commented entries; dirty twin drops the `docs` owner row).
- `module-double-disposition`: a module with both an owner row and a debt row (dirty twin adds a debt row for the owned `crates/verter_parser`).

## ARH0-god-evidence (reject) — ARH0-AC2 discriminator

- `god-without-responsibility-evidence`: a god-module-candidate row reduced to one responsibility and size-only evidence, or to touches-only coupling (a touch count is churn, not coupling; the dirty twin strips `fanIn` from the live `flow_slice_content.rs` row), or to a zero/negative coupling count — measured evidence of no coupling (dirty twins set `fanIn: 0` and `sharedCommits: -3`). A large file alone never declares a god module; coupling evidence is a positive fan-in or shared-commit count.
- `split-module-reclassified-without-new-evidence`: the retired Phase 11 target `meta_resolve.rs` pushed back into godModuleCandidates with only pre-split history. Previously split modules need fresh measured multi-responsibility evidence.

## ARH0-capability (reject)

- `version-not-pinned-in-source`: the named source property does not pin the recorded version — a fabricated `5.99.0` Svelte pin, or another dependency's version that merely occurs in the same file (dirty twin records the Vue pin `3.6.0-rc.5` for Svelte, which a whole-file text search would accept). Implemented rows record an exact manifest pin (JSON dotted path, e.g. `devDependencies.svelte`) or a `pnpm-lock.yaml` resolved version (`importers.<importer>.<section>.<dependency>`); Cargo.toml pins bind a `property = "value"` declaration or a `section.property` inside its `[section]` table. Peer-resolution suffixes on lockfile versions are annotations, not part of the version.
- `version-without-property`: an implemented row that does not name the property its version is bound to.
- `build-identity-not-in-source`: executable names and compilation targets are recorded as `buildIdentity` (kind, value, source), never as the version; the identity value must appear in its named source (dirty twin renames the `protoc-gen-es` executable).
- `implemented-without-consumers`, `planned-without-uncertainty`, `missing-consumer`: no support claim without named consumers; required-planned rows must state uncertainty and have no live consumers.
- `missing-version-source`: a versionSource that is not a repo tree path and not marked `provenance: "tama-dag"` (the solid-2/htmx-4 target rows bind the TAMA-database target decision; an implemented pin may never use the tama-dag binding). Path fields must resolve inside the repository tree: a traversal value such as `..` normalizes to an existing directory outside the tree and is rejected (dirty twin sets `versionSource: ".."`).

## ARH0-debt (reject)

- `malformed-disposition-owner`: disposition owner outside the structural id discipline (dirty twin uses `simp99`).
- `debt-without-disposition`: every debt row carries a disposition.
- `missing-candidate-path`: a candidatePath that is not a repo tree path and not marked `provenance: "tama-dag"` (the rev11 retirement-block row binds the TAMA-database simplification contract).
