# GitHub control-plane contract

This contract governs the post-ORC0 GitHub operational projection for Rev11. It authorizes no live GitHub mutation by itself. GH0 may begin only after ORC0 has landed, the activation state is ACTIVE, and the current accepted ORC0 activation receipt validates against the current authority. Every GH, FB, and REL dispatch and acceptance repeats that activation-currency check; a rebase, mutable label, issue state, or operator assertion is not a substitute.

## Authority hierarchy

- static Rev11 DAG, charters, and contracts are architecture authority;
- immutable `programctl` receipts are lifecycle and correctness authority;
- round-bound leases are active-work ownership;
- GitHub Issues, Project 3, pull requests, checks, and milestones are an operational projection and human-readable history;
- GitHub CI is an executor whose exact result must be imported into and bound by the Rev11 evidence model;
- milestones and human-authored GitHub fields remain maintainer-owned.

GitHub closure, labels, Project status, milestone placement, PR state, check conclusions, or merge buttons cannot satisfy a DAG predecessor. Only a current immutable Rev11 receipt can do that. The adapter is field-owned and directional; it is not a generic bidirectional synchronizer.

The configured repository is `pikax/verter`. The configured long-lived project is owner `pikax`, project number `3`. Configuration changes are authority amendments, not opportunistic runtime discovery.

## ORC0 activation boundary

GH0 has the exact direct predecessor ORC0. All other GH, FB, and REL nodes retain the exact source-defined predecessor lists and transitively follow GH0. Every node also declares `activation_gate = "ORC0"`.

Before any check or apply operation, the implementation must prove all of the following:

1. the authority state is ACTIVE;
2. `activation_node` is ORC0;
3. `orc0_receipt` names the current accepted immutable receipt;
4. the receipt is current for the loaded authority and activation transition;
5. every node-specific predecessor receipt is current;
6. no candidate, rebase, or generated projection has silently changed the receipt basis.

Failure is a typed refusal with zero GitHub mutation.

## Field ownership matrix

### DAG-owned

- node ID and train identity;
- charter-derived managed metadata;
- train-parent and node-parent relationships;
- exact DAG predecessor relationships;
- projected DAG lifecycle state;
- accepted and landed status derived from immutable receipts.

### GitHub-maintainer-owned

- milestone assignment;
- issue and PR text outside managed regions;
- labels outside the owned `dag:*` and `ai:*` subsets;
- `ai:ignore`;
- `dag:promotion-approved` and the underlying promotion authorization;
- explicit release-cut authorization.

### GitHub/runtime-owned

- issue and PR database IDs, numbers, URLs, and repository identity;
- PR draft/ready state;
- CI run and check identity;
- merge result and resulting commit identity.

### AI-owned

- exactly one AI triage result label;
- `.feedback/issues/<issue-number>.md` operational reports;
- the managed PR summary and review-cycle presentation.

### Derived and non-authoritative

- GitHub Project Status;
- project views and grouping;
- convenience progress and roadmap indicators.

Every field has exactly one owner. Reconciliation may change only fields owned by the applicable actor. Human-owned content outside managed regions survives byte-for-byte. Label updates are set operations restricted to the owned namespace; replacing the complete label set is forbidden.

## Stable identity and managed regions

Issue identity is resolved only through durable markers such as:

```html
<!-- verter:dag-train=compiler.compiler-bridge -->
<!-- verter:dag-node=CCA1 -->
```

Titles, Project row order, milestone names, and labels are never identity. Duplicate claims, one issue claiming multiple nodes, wrong-repository claims, incompatible parent mappings, and ambiguous markers fail closed.

Generated issue and PR content is confined to explicit markers:

```html
<!-- verter:managed:start -->
generated content
<!-- verter:managed:end -->
```

Missing, nested, duplicated, or reversed boundaries are dangerous drift. The adapter refuses mutation rather than guessing. Content outside the single valid managed region is preserved exactly.

## Reconciliation and write discipline

`githubctl` is a replaceable external-control-plane adapter above the `programctl` correctness kernel. Its GitHub implementation consumes structured `gh` JSON or API responses and never scrapes terminal prose or web UI text.

Every mutating operation must:

- offer check/dry-run and apply modes;
- print a structured mutation plan before apply when practical;
- use stable database IDs or durable markers;
- be idempotent, including after partial network failure;
- preserve every field outside its ownership;
- fail closed on ambiguity, stale authority, stale PR head, stale integration base, or insufficient permissions;
- never persist tokens or credentials;
- leave a second identical apply with zero mutations.

At startup, doctor validates authentication, repository access, project scope, Project 3 identity and fields, existing labels, and required capabilities. Permission failure occurs before mutation. Tests use deterministic fixtures or a fake adapter; live GitHub is never the ordinary test substrate.

## DAG issue projection

One train maps to one parent issue. One DAG node maps to one issue or sub-issue. DAG predecessors map to GitHub blocked-by relationships. The node and parent markers are the identity authority.

Repairable projection drift is restricted to DAG-owned managed fields. Maintainer-owned differences are preserved. Dangerous drift refuses mutation. A manually closed DAG issue without a current landed receipt is dangerous drift; it never changes DAG readiness. Issue auto-closing is configurable and defaults off. A `dag:landed` label comes only from verified landed evidence.

## PR-backed lifecycle and immutable evidence

The normal operational identity is DAG node, issue number, and PR number. The immutable correctness identity remains the exact PR head SHA/tree, base SHA/tree, integration SHA/tree, authority digest, charter digest, and evidence digests.

Dispatch creates or binds a draft PR after admission. Creating a PR never mutates static DAG authority. Candidate finalization freezes the current PR revision; a later head change makes the prior candidate, reviews, CI, and acceptance basis stale.

The managed PR description contains Problem, Scope, Validation, bounded review-cycle summaries, and a final Implementer section. Raw authoritative logs stay in the Rev11 evidence system. Human text outside managed regions remains exact.

## Finding retention and disposition

The following invariant is binding on every acceptance in this program: GH0–GH6, FB0–FB2, and REL0–REL2. It applies to contract, adapter, projection, PR, CI/landing, feedback, promotion, release-planning, release-execution, convergence, and cutover candidates alike:

> No finding is lost across acceptance. P0/P1 remain non-dispositionable blockers. Any actionable lower-severity finding that survives acceptance must be either explicitly accepted as non-actionable risk under an owning policy, or materialized as a uniquely fingerprinted carry-forward obligation bound to an immutable receipt and durable GitHub issue. A carried finding remains live until a later immutable resolution receipt supersedes it. Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation. Repeated carry-forward requires escalating authorization and is never implicit.

P0/P1 remain non-dispositionable blockers. A lower-severity finding has exactly two acceptable terminal decisions for a candidate:

1. an owning policy explicitly classifies the exact fingerprint as non-actionable risk and the immutable acceptance receipt binds that policy decision; or
2. an immutable carry-forward receipt binds the finding fingerprint, severity, node/candidate/review basis, owner, class-wide sweep, obligation text, durable GitHub repository and issue database identity, issue number/URL, authorization tier, and prior carry-forward receipt when repeated.

A carried obligation remains OPEN independently of the mutable issue. Only a later immutable resolution receipt naming the exact obligation fingerprint and superseded carry-forward receipt may resolve it. Closing, deleting, transferring, relabeling, or moving the issue or milestone yields projection drift; it cannot alter obligation liveness.

The first carry-forward requires the authority named by the owning disposition policy. Each repeat increments a monotonic carry-forward generation and requires an explicitly higher authorization tier. Missing, equal, lower, inferred, label-only, or implicit authorization refuses acceptance.

The eventual machine contract must distinguish `non_actionable_risk`, `carry_forward_open`, and `resolved` without collapsing them into GitHub issue state. It must validate immutable receipt chains, unique fingerprints, durable issue database identity, and supersession. The existing pre-control-plane one-time disposition format is migration input, not permission to lose or silently close a finding.

## CI, squash landing, and landed proof

GitHub CI is the top-level integration-gate executor, not the correctness authority. Required CI emits or exposes evidence bound to DAG node, PR identity, exact PR head, exact integration base, exact tested integration tree, executed commands, unexpected skips, and terminal result. That evidence is imported through existing Rev11 gate primitives.

The merge invariant is `landed_tree == validated_integration_tree`. Candidate SHA equality is neither required nor sufficient for squash landing.

Only the program orchestrator may squash-merge DAG block PRs. Before merge it verifies expected PR, expected head, current review and CI evidence, and current integration-base policy using expected-head protection. Any movement invalidates and reruns evidence. After merge it fetches the resulting main commit, proves the exact landed tree, emits the generic landed receipt, and only then advances DAG lifecycle state.

Normal block history is one DAG node, one GitHub issue, one implementation PR, and one squash commit. Exceptions require an explicit charter reason. Use `Refs #N`; automatic close keywords are not correctness authority.

## Feedback intake and promotion

AI result labels are mutually exclusive: `ai:unchecked`, `ai:confirmed`, `ai:rejected`, `ai:fixed`, and `ai:needs-human`. `ai:ignore` and `dag:promotion-approved` are maintainer-owned. AI never removes `ai:ignore` and never creates promotion authorization.

Non-DAG inspection may update only the AI result label and local operational report. It does not close, reopen, comment, rewrite, move a milestone, or create DAG authority. If `ai:ignore` is present, inspection is a no-op.

Promotion requires maintainer authorization plus the normal post-ORC0 amendment and ratification mechanism. A mutable label alone is insufficient. The accepted amendment records source-issue provenance and reuses the existing issue without overwriting human content.

## Release control

Milestones are maintainer-owned scheduling metadata. They may prioritize only among nodes already READY according to `programctl`; they never bypass predecessors. One long-lived Project 3 supplies execution, frontier, triage, review, train, milestone, and roadmap views.

Release readiness for DAG-managed work derives from immutable accepted/landed receipts, not issue closure. Missing work is reported exactly and is never silently waived. Only the maintainer moves, defers, or waives milestone content.

Release rehearsal reuses `.github/workflows/release-check.yml` and its exact invocation of `.github/workflows/release.yml` with dry-run enabled. Release landing reuses `.github/workflows/release-tag.yml` and `.github/workflows/release.yml`. The squash subject remains exactly `release: v<version>` where required by the tag matcher. Prereleases preserve the existing alpha/beta/rc behavior. Milestone closure is never automatic without explicit maintainer policy.

## Required fail-closed proof

The GH/FB/REL implementation fixtures must discriminate at least:

- empty, correct, partial, interrupted, and repeated projection;
- duplicate or conflicting markers and parent/topology mappings;
- human body, milestone, and unrelated-label preservation;
- manual issue closure without landed proof;
- ignored and conflicting-label feedback cases;
- authorized and unauthorized promotion;
- PR-head and integration-base movement after freeze;
- stale CI and squash-tree mismatch;
- open, repeated, and resolved carry-forward chains plus mutable-issue tampering;
- blocked and ready milestone release plans;
- beta release version, tag, and existing workflow path.

Every critical authority negative uses a planted RED/GREEN control through the canonical entry point. Live smoke testing, if separately authorized, is bounded and reversible and never creates throwaway noise without maintainer cleanup authority.

## Source

- `source:github-control-plane-program.md:L1`
