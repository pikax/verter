<!-- unified-charter-v2
id=REL3
name=Clean-room published-package installation and public entrypoint smoke
predecessors=REL2
phase=governance
train=governance.release-control
product=release_control
kind=implementation
semantic_role=delivery
class=successor
owner=governance.release-control:clean-room published-artifact installation and public entrypoint evidence
conflict_domains=release_orchestration
resource_class=ts-heavy
gate_profile=canonical
review_profile=security-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/governance-release-control/REL3.md
size=S
max_production_loc=350
max_production_files=5
max_related_packages=2
rescope_loc=750
rescope_files=8
rescope_unrelated_packages=3
-->

# REL3 — Clean-room published-package installation and public entrypoint smoke

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Every package the release publishes installs from its own packed tarball into an empty consumer project and answers one minimal public call through its documented entrypoint, with no workspace resolution, no consumer-side patch and no source-tree fallback. The evidence is produced by the existing release rehearsal, before publication.

The gap this closes was observed in the published 0.0.1-beta.5 artifacts: `@verter/proto` emitted ESM imports without `.js` sibling extensions, so Node could not load `@verter/component-meta` at all, and both benchmark repositories had to add a local pnpm patch merely to make the release load. Every Rust and workspace check was green; the defect existed only in the published artifact. That specific emission has since been corrected in the tree — this node owns making its class impossible to publish again.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, the existing release rehearsal entrypoint, `.github/workflows`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundaries: the published package set, each package's documented public entrypoint and export conditions, and the rehearsal's evidence record.

## Exact predecessor contracts

- **REL2:** implemented ledger row for "Release PR, tag and publication integration"; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional pull-request number are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Derive the published package set from the existing publication configuration rather than a hand-maintained list, so a newly published package is covered without editing this check.
- Pack every published package, install the packed tarballs into a scratch ESM consumer outside the workspace, and import each package by every entrypoint its `exports` declares. Resolve nothing through the workspace, a lockfile override, a patch or a relative source path.
- Exercise one minimal public call per package, chosen to load the package's real module graph rather than only its type declarations.
- Run the check in the existing release rehearsal, so a failure blocks the release before the tag rather than after. Its result is recorded in the rehearsal evidence that REL1 already produces.
- Cover both module systems the packages declare. A package that declares only ESM is imported as ESM; a package that declares both is exercised through both conditions.
- Do not add a second publication implementation, change prerelease classification, or alter what is published. This node observes the artifact; it does not produce it.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **REL3-AC1 — sole-owner outcome:** the clean-room check is the sole authority for published-artifact loadability, and a workspace-resolved or patched success cannot satisfy it. Prove the scratch consumer cannot reach the repository: a deliberate source-tree fallback must fail the check.
- **REL3-AC2 — positive contract:** every published package and every declared entrypoint is covered, derived from the publication configuration. A package added to that configuration and not covered fails the check rather than passing silently.
- **REL3-AC3 — incremental equivalence:** the check runs against freshly packed tarballs on each rehearsal; a cached tarball, a previously installed consumer directory or a prior rehearsal's result cannot be reused as evidence.
- **REL3-AC4 — bounded work:** the negative controls are executed and demonstrated to apply: a generated sibling import without its `.js` extension, a missing `exports` condition, and a package whose declared entrypoint file is absent from the tarball. Each must fail for its own reason.
- Test homes: `scripts/githubctl/tests`, plus fixture tarballs owned by this node. A live registry is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject: a consumer-side patch used to make a published package load, a workspace-resolved import counted as clean-room evidence, a hand-maintained package list, and a rehearsal that reports PASS with the check skipped.
- Do not add a release-blocking rule outside the existing rehearsal, and do not change the publishing implementation to make the check pass.

## Budgets and mandatory rescope

- Target ceiling: 350 production LOC, 5 production files, 2 related packages.
- Rescope if the existing rehearsal cannot host the check or a second publication authority appears.
- Correctness budget: zero unloadable published package, zero uncovered published entrypoint, zero patched or workspace-resolved success reported as clean-room evidence, and zero skipped check reported as PASS.
- Performance budget: this check runs once per rehearsal and owns no hot path; record a terse not-applicable rationale rather than creating counters or a soak.

## Abort conditions

- Abort on a missing predecessor row, or if the publication configuration does not actually enumerate the published set, in which case that enumeration is the prerequisite and is named rather than reimplemented here.
- Abort rather than opportunistically rewriting the existing release or publication architecture.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. Execute the clean-room check against the current packed artifacts and record the per-package, per-entrypoint result.
3. Execute each negative control and demonstrate that the mutation applied before its failing run is treated as evidence.
4. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
5. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance` and `supply-chain-platform` PASS reports. P0/P1 block; lower findings follow the owning review policy. L4 consumes this node's implemented row as required release evidence; that consumption adds no readiness rule here.
