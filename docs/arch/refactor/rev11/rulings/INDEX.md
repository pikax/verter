# Rulings index

One row per ruling document migrated from the session scratchpad under RULING 2 of
[`ORCHESTRATION-AUTHORITY-MODEL`](ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md). Each document carries a
typed YAML frontmatter header (`ruling_id`, `type`, `date`, `date_source`, `binds`, `source_file`,
`summary`, `supersedes`, `superseded_by`, `contradicts`, `notes`) prepended to the verbatim original
text — body content was not rewritten, only the frontmatter and a mechanical `<MACHINE_ROOT>` path
substitution were applied. `supersedes`/`superseded_by` are per-CLAIM, not per-document: a ruling can
supersede one claim of another while the rest of that document remains binding — see each document's
own frontmatter for the exact claim text.

**Not yet built by this migration:** the effective-state generator and authority registry described in
RULING 1/3 of `ORCHESTRATION-AUTHORITY-MODEL` — this index is hand-curated, not a generated fail-closed
model. Do not treat `superseded_by = —` as proof a ruling is uncontested; it means no OTHER migrated
ruling's own text names it as superseded. Ledger `digest` binding is a separate step owned by the
program orchestrator (RULING 1), not performed here.

## Maintainer directives (25)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`HARDEN-ORCHESTRATION`](MAINTAINER-DIRECTIVE-HARDEN-ORCHESTRATION.md) | maintainer-directive | 2026-08-20 | program-wide orchestration machinery | — |
| [`CSS-CLEAN-CUTOVER`](MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md) | maintainer-directive | 2026-08-20 | Track J (J1-J4), CSS/style pipeline architecture | — |
| [`NO-LIGHTNINGCSS`](MAINTAINER-RULING-NO-LIGHTNINGCSS.md) | maintainer-directive | 2026-08-17 | Track J / J1, BCSS0 (superseded within this document), CSS/style pipeline | CSS-CLEAN-CUTOVER |
| [`BETA4-REGRESSION-INTAKE`](MAINTAINER-DIRECTIVE-BETA4-REGRESSION-INTAKE.md) | maintainer-directive | 2026-08-20 | program-wide (release boundary), BV2, CM1 | — |
| [`PARSER-CRATE-OWNERSHIP-INTENT`](MAINTAINER-INTENT-PARSER-CRATE.md) | maintainer-directive | 2026-08-18 | verter_parser crate ownership (cross-cutting, not a single block) | — |
| [`AMD-005-BV1-BS1-AUTHORISED`](MAINTAINER-RULING-AMD-005-BV1-BS1-AUTHORISED.md) | maintainer-directive | 2026-08-20 | BV1, BS1, AMD-005 | — |
| [`AMD-009`](MAINTAINER-RULING-AMD-009.md) | maintainer-directive | 2026-08-16 | B2, B3, AMD-005, AMD-010, JS-1 | — |
| [`BF3-SECTION7-RATIFICATION`](MAINTAINER-RULING-BF3-SECTION7.md) | maintainer-directive | 2026-08-16 | BF3, AMD-009, AMD-010, BA0, BS0, BCSS0, BRT0 | — |
| [`AT2-NAMED-ACT`](MAINTAINER-ACT-AT2.md) | maintainer-directive | 2026-08-17 | BF3, BA0, AT-2 finding row | — |
| [`AT2-ACT-CLARIFICATION`](MAINTAINER-ACT-AT2-CLARIFICATION.md) | maintainer-directive | 2026-08-17 | BF3, BA0, AT-2 finding row | — |
| [`BUGS-AND-TYPES`](MAINTAINER-RULING-BUGS-AND-TYPES.md) | maintainer-directive | 2026-08-17 | program-wide (every remaining block, not only BF3), AT-2 (applied here as the prompting case) | — |
| [`CODEX-NEVER-ORCHESTRATES`](MAINTAINER-RULING-CODEX-NEVER-ORCHESTRATES.md) | maintainer-directive | 2026-08-18 | program-wide dispatch discipline | DISPATCH-ROSTER |
| [`COMMENT-CLEANUP-PASS`](MAINTAINER-RULING-COMMENT-CLEANUP-PASS.md) | maintainer-directive | 2026-08-19 | program-wide, per-block landing process | — |
| [`CONCURRENCY-CEILING-AND-ROSTER`](MAINTAINER-RULING-CONCURRENCY-CEILING-AND-ROSTER.md) | maintainer-directive | 2026-08-20 | program-wide concurrency policy | — |
| [`DISPATCH-ROSTER`](MAINTAINER-RULING-DISPATCH-ROSTER.md) | maintainer-directive | 2026-08-18 | program-wide dispatch discipline | — |
| [`GATE-SCOPE`](MAINTAINER-RULING-GATE-SCOPE.md) | maintainer-directive | 2026-08-17 | program-wide gate discipline | GATE-UNRESTRICTED |
| [`GATE-UNRESTRICTED`](MAINTAINER-RULING-GATE-UNRESTRICTED.md) | maintainer-directive | 2026-08-17 | program-wide gate execution parameters | — |
| [`GREEN-BRANCH-AND-TRIAGE`](MAINTAINER-RULING-GREEN-BRANCH-AND-TRIAGE.md) | maintainer-directive | 2026-08-20 | program-wide gate-failure triage discipline | — |
| [`LANDING-IS-ORCHESTRATOR-ONLY`](MAINTAINER-RULING-LANDING-IS-ORCHESTRATOR-ONLY.md) | maintainer-directive | 2026-08-20 | program-wide landing/orchestration protocol | — |
| [`NO-BUILD-INVOKING-TESTS`](MAINTAINER-RULING-NO-BUILD-INVOKING-TESTS.md) | maintainer-directive | 2026-08-20 | Rust test suite composition | — |
| [`AUTO-ACCEPT`](MAINTAINER-RULING-AUTO-ACCEPT.md) | maintainer-directive | 2026-08-19 | program-wide acceptance protocol | — |
| [`BS1-COMPLETION-AUTHORITY`](MAINTAINER-RULING-BS1-COMPLETION-AUTHORITY.md) | maintainer-directive | 2026-08-20 | BS1 | — |
| [`BS1-COMPLETION-CORRECTION`](MAINTAINER-RULING-BS1-COMPLETION-CORRECTION.md) | maintainer-directive | 2026-08-20 | BS1 | — |
| [`PARALLEL-REVIEW-SEATS`](MAINTAINER-RULING-PARALLEL-REVIEW-SEATS.md) | maintainer-directive | 2026-08-18 | program-wide review-seat protocol | — |
| [`REVIEW-BUDGET-BY-ARTIFACT-CLASS`](MAINTAINER-RULING-REVIEW-BUDGET.md) | maintainer-directive | 2026-08-17 | program-wide review protocol | — |

## Architecture rulings (12)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`C1-FOUR-FORKS`](ARCH-RULING-C1-FOUR-FORKS.md) | architecture-ruling | 2026-08-20 | C1 | — |
| [`C1-THREE-GAPS-ADDENDUM`](ARCH-ADDENDUM-C1-THREE-GAPS.md) | architecture-ruling | 2026-08-20 | C1 | — |
| [`D1-SIX-FORKS`](ARCH-RULING-D1-SIX-FORKS.md) | architecture-ruling | 2026-08-20 | D1 | C1-D1-FLOW-FILE-RECONCILIATION |
| [`C1-D1-FLOW-FILE-RECONCILIATION`](ARCH-RULING-C1-D1-FLOW-FILE-RECONCILIATION.md) | architecture-ruling | 2026-08-20 | C1, D1 | — |
| [`C2-FIVE-FORKS`](ARCH-RULING-C2-FIVE-FORKS.md) | architecture-ruling | 2026-08-20 | C2 | — |
| [`CM1-FINDINGS-BC`](ARCH-RULING-CM1-FINDINGS-BC.md) | architecture-ruling | 2026-08-20 | CM1, BV1, BS1, C1, BV2, B5, C2, C3 | — |
| [`BV2-FINDING-A-REPAIR-AND-PLACEMENT`](ARCH-RULING-BV2-FINDING-A-REPAIR-AND-PLACEMENT.md) | architecture-ruling | 2026-08-20 | BV2, BV1, BS1, B5, B6 | — |
| [`CONCURRENCY-OPERATING-MODEL`](ARCH-RULING-CONCURRENCY-OPERATING-MODEL.md) | architecture-ruling | 2026-08-20 | program-wide (ledger/concurrency operating model, not a single block) | CONCURRENCY-CEILING-AND-ROSTER |
| [`ORCHESTRATION-AUTHORITY-MODEL`](ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md) | architecture-ruling | 2026-08-20 | program-wide (authority/ratification model, rulings custody, effective-state generator, mutation testing, block-checkpoint model) | — |
| [`B2-B3-SCOPE-AND-CONCURRENCY`](B2-scope-and-concurrency-ruling-codex-1.md) | architecture-ruling | 2026-08-16 | B2, B3, B4 | — |
| [`B3-SCOPE`](B3-scope-ruling-codex-1.md) | architecture-ruling | 2026-08-16 | B3 | AMD-009 |
| [`BF3-PARALLELISM`](parallelism-ruling-codex.md) | architecture-ruling | 2026-08-16 | BF3, J1 | — |

## Attestations (2)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`BS1-ADVERSARIAL-EXACT-CANDIDATE-ATTESTATION`](ATTESTATION-BS1-ADVERSARIAL-EXACT-CANDIDATE.md) | attestation | unknown | BS1 | BS1-RESTACK-BYTE-IDENTITY |
| [`BS1-RESTACK-BYTE-IDENTITY`](EVIDENCE-BS1-RESTACK-BYTE-IDENTITY.md) | attestation | unknown | BS1 | — |

## Dispositions (3)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`B4-C1-SERIALIZE`](DISPOSITION-B4-C1-SERIALIZE.md) | disposition | unknown | B4, C1 | — |
| [`BS1-SERIALIZE-BEHIND-BV1`](DISPOSITION-BS1-SERIALIZE.md) | disposition | unknown | BS1, BV1 | — |
| [`TYPECHECK-POC-TO-H-TRAIN`](DISPOSITION-TYPECHECK-POC-TO-H-TRAIN.md) | disposition | 2026-08-18 | H2, H3 (future Track H blocks) | — |

