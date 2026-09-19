# DOC0 evidence index

Selected case IDs: `DOC0-AC1`, `DOC0-AC2`, `DOC0-AC3`, `DOC0-AC4`, `DOC0-AC5`, `DOC0-ratification`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node tests/documentation/DOC0/verify.mjs`
- `node --test tests/documentation/DOC0/doc0.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
- the remaining `docs-domain` final profile commands (see `catalogs/gate-profiles.toml`)

Deletion population this node: empty. No doc route, generator or build pipeline is displaced or retired by DOC0; retirement obligations are assigned to the successor implementations (DOC3, DOC9) through `contracts/web-product-expansion-v1.md` §13.

DOC0.1 grounding: the inventory was captured from the live candidate — all 49 docs markdown files, the VitePress nav/sidebar (`docs/.vitepress/config.ts`, `srcExclude: ["arch/**"]`), all 25 package manifests (16 public at exact versions, 9 internal), the capability catalog (24 surfaces, owner L0), the lint registry (187 registered rules), the docs-domain gate profile, the generated page and its freshness guard, and the two example estates. Pages built but unreachable from the nav (guide/experimental, guide/release-state, 8 contributing pages, migration, plans, audit-footprint, evidence, generated) are recorded with `nav: unnavigated` and assigned to DOC9's qualification; `docs/arch/**` is recorded as excluded from the site build by the live config. The harness joins the IA doc types to the inventory rows after glob expansion (sole per-asset owner/type authority) and re-reads published user-page and user-audience repository-document bytes so pinned roadmap-only mentions (the unpublished VS Code Marketplace listing) stay labelled with a `roadmapClaims` row and never appear as a working install step.

AC3 is recorded as not applicable with a precise rationale (no state, query, map or runtime boundary is touched at 0 production LOC); the runtime evidence obligations are bound downstream to DOC1 and DOC3. AC5 introduces no performance budget, so none is measured; the empty deletion population carries its rationale in the ownership map.

Conflict-domain note: the charter originally materialized on the formatter document-algebra lease id `doc` in `catalogs/conflict-domains.toml`. The architecture-3 review split the documentation estate into its own `documentation_estate` domain (`docs/`, `examples/`, `tests/documentation/`, `contracts/web-product-expansion-v1.md`); the charter records the split, the DAG node leases the new domain, and the `domain-collision-recorded` and `missing-owner` checks enforce it. The formatter `doc` lease is unchanged.

This file does not claim the cases executed. Copying the charter here is not evidence.
