# Solid 2 and htmx 4 roadmap targets

## Context

The maintainer selected Solid 2 for the existing Solid scope, explicitly excluding Solid 1 for now, and selected htmx 4 wherever htmx already appears in the Tama roadmap.

## Intent contract

- Solid 2 is the sole selected Solid major. Solid 1 support, compatibility, and migration are outside current scope; Solid 1 may appear only as an unsupported-major control, never as a fallback or an additional supported profile.
- htmx 4 is the selected htmx major in the existing portfolio and HTML/attribute-overlay proof scopes. Older htmx support is not implied.
- Each experiment or future profile must pin one exact release, package roles, oracle, and corpus within its selected major. Prerelease evidence records the exact prerelease and maturity; a family name or `latest` is not semantic identity. Unsupported majors cannot activate the selected profile.
- The selection preserves the existing distinction between private architectural proof and advertised production tooling. Compiler disposition remains `NotApplicable` for both portfolio entries.

## Changes

- Retain node `SLDP` and name it "Solid 2 counterexample over identical TSX geometry" in both the DAG and charter. Its release-specific semantic inventory includes async memos, split effects, batching, stores, and control flow alongside components, props, and signals.
- Update the React/MDX counterfixtures, downstream predecessor descriptions, and identity acceptance to name Solid 2 consistently.
- Update the `CLI3` portfolio and the `HWC2`/`ALPP` HTML counterfixtures to htmx 4. Attribute inheritance, configuration overrides, events, and request/target/trigger/swap semantics remain overlay-owned.
- Reconcile the local issue-content catalog with the changed charters. SolidStart remains deferred and requires a compatible exact project-profile release after the Solid 2 vertical.
- htmx already has portfolio and proof scope but no dedicated implementation node. This amendment adds no nodes or dependency edges and changes no implementation-ledger state or issue mapping.

## Legacy deletions

Replace unqualified Solid and htmx target wording in the affected live planning surfaces. Remove the Solid portfolio's resource-centric wording in favor of release-qualified Solid 2 semantics. No production code, parser, compiler, or existing implementation is deleted by this scope amendment.

## Verification

- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict` validates DAG/charter and catalog consistency.
- `node roadmap/0.1.0-tama/tools/programctl.mjs explain SLDP` exposes the updated Solid 2 node while preserving its predecessor-derived readiness. Work-packet generation remains unavailable until its ancestors are implemented.
- `pnpm docs:build` checks the documentation build.
- Inspect the affected diff and remaining Solid/htmx references for unqualified targets or accidental Solid 1 support. This is an authority/documentation amendment; runtime behavior and Rust test boundaries are unchanged.
