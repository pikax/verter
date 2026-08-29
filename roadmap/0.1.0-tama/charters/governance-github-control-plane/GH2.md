<!-- unified-charter-v2
id=GH2
name=Occasional issue-sync command and local mapping
predecessors=GH1
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:explicit issue creation/update and local node-to-issue mapping
conflict_domains=github_projection_state
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
charter=charters/governance-github-control-plane/GH2.md
size=M
max_production_loc=1200
max_production_files=12
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# GH2 — Occasional issue-sync command and local mapping

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.


## Independently acceptable outcome

Deliver `githubctl sync-issues` for occasional explicit one-way synchronization from local authority to GitHub. For named trains or node sets, it creates missing issues with opt-in mappings and deterministically reconciles the versioned label catalog and managed issue labels. Existing issue prose changes only through an explicit content-refresh flag. It reports and skips protected mappings. It never imports GitHub edits, runs continuously, or publishes DAG metadata; PR creation remains GH3.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `roadmap/0.1.0-tama/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`.
- Named boundaries: `GitHubIssueMapping`, `IssueCreateOrUpdatePlan`, `IssueSyncLabels`, `sync-issues --check`, `sync-issues --apply`, and local mapping lookup.

## Exact predecessor contracts

- **GH1:** implemented ledger row for “GitHub adapter and deterministic fixtures”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- Read local authority to enumerate an explicitly requested train or node set. Only when creating a missing issue or explicitly refreshing content, read the selected node's live charter and current source context and synthesize—not extract—the human issue standard in `contracts/github-control-plane.md`: a short action/outcome title plus standalone `Problem`, `Expected outcome`, and three-to-six-bullet `Acceptance` sections followed by the model line. Reject copied charter headings/prose, program/DAG wording, source-plan conditions, predecessor/readiness fields, effort/budgets, owner-transition slogans, generic forbidden-design lists, unexplained deletion inventories, abort conditions, gates/review instructions, verification commands, and implementation sequence.
- In normal check mode, report missing mappings, deterministic managed-label drift, protected mappings, current mappings, and repository catalog drift. Do not render or compare existing issue prose. Check mode never mutates GitHub or local authority.
- In apply mode, create each requested missing issue, capture the returned number, write its mapping with `sync_to_github = true`, and apply deterministic labels. For existing opt-in mappings, create or correct catalog definitions and add/remove only sync-owned issue labels. Never mutate a protected mapping. The operator commits any mapping patch.
- Classify labels only from the checked-in catalog and current node fields. Every opt-in issue has one area label, one problem label, optional framework scope, and AI-origin provenance. Preserve unrelated labels, maintainer guards, and AI-inspection verdict labels. Never ask a model to classify labels and never replace a whole issue-label set.
- Newly rendered opt-in issue bodies target 120–250 words and never exceed 350 words excluding the final `Model: <model name>` line. Content-refresh check mode rejects missing required sections, prohibited charter headings, an over-limit body, or a first paragraph that fails to state a concrete problem and impact. Normal label-only sync does not evaluate existing prose. Protected issue bodies remain untouched.
- `--refresh-content` is the sole existing-issue prose refresh and requires a model. It preserves the opt-in issue number, comments, and discussion history while replacing ordinary title/body only when changed. Normal mapped label reconciliation requires no model and leaves prose untouched. Neither path touches a protected issue, discovers identity from prose, or adds DAG metadata.
- Given a GitHub issue number, resolve the node only by searching the unique local `gh_issue` value. This is local reverse lookup, not reverse synchronization; no GitHub field mutates DAG/charter/ledger authority. Mapping presence never marks implementation complete.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs` and future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject hidden identity markers, managed body regions, blocked-by projection, generated lifecycle labels, effort metadata, any GitHub-to-DAG synchronization, continuous synchronization, title identity, and issue-closure authority.
- Do not create PRs, import CI evidence, inspect non-DAG issues, or schedule releases in this block.

## Budgets and mandatory rescope

- Target ceiling: 1,200 production LOC, 12 production files, 3 related packages.
- Rescope if synchronization requires a second mapping database or generic bidirectional reconciliation.
- Correctness budget: zero duplicate local mapping, duplicate issue number, accidental implementation row, protected-issue write, lost discussion, wrong-issue update, or unreported partial create/update.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on a duplicate local node/issue mapping, missing ancestor ledger row for the sync block itself, wrong repository, an issue creation whose returned number cannot be written locally, or an update whose mapped issue cannot be read unambiguously.
- On partial external failure, stop and report exactly which issues were created or updated and which mappings were written; do not guess identity by title or body.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Carry-forward obligation liveness comes from implemented-ledger rows, never mutable GitHub issue closure, labels, or milestone movement.
