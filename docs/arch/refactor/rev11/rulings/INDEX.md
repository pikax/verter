# Rulings index

One row per document in this directory. The original corpus was migrated from the session scratchpad
under RULING 2 of [`ORCHESTRATION-AUTHORITY-MODEL`](ARCH-RULING-ORCHESTRATION-AUTHORITY-MODEL.md);
documents added after that migration are listed here too. Each document carries a typed YAML
frontmatter header. Nine keys are on every document — `ruling_id`, `type`, `date`, `date_source`,
`binds`, `summary`, `supersedes`, `superseded_by`, `contradicts`; `source_file` and `notes` are usual
but not universal, omitted by two documents. The corpus is the authority for this schema, not the
reverse — read the keys off the documents rather than assuming this list. For the migrated documents
the header was prepended to the verbatim original text — body content was not rewritten, only the
frontmatter and a mechanical `<MACHINE_ROOT>` path substitution were applied.
`supersedes`/`superseded_by` are per-CLAIM, not per-document: a ruling can supersede one claim of
another while the rest of that document remains binding — see each document's own frontmatter for the
exact claim text.

**Built since this migration:** the effective-state generator and authority registry described in RULING
1/3 of `ORCHESTRATION-AUTHORITY-MODEL` now exist, at `scripts/effective-state.mjs` and
`docs/arch/architecture-lock/ledger/authority-registry.toml`. This index is derived from neither: it
remains hand-curated, not a generated fail-closed model. Do not treat `superseded_by = —` as proof a
ruling is uncontested; it means no OTHER migrated ruling's own text names it as superseded. Ledger
`digest` binding is a separate step owned by the program orchestrator (RULING 1), not performed here.

## Maintainer directives (43)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`STEERING-TCM-CONTENT-MAPPERS`](MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md) | maintainer-directive | 2026-08-22 | TCM0, TCM1, TCM2, TCM3, TCM4 | — |
| [`TCM-PACKAGE-CERTIFICATION-SETTLED`](MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md) | maintainer-ruling | 2026-08-23 | TCM0, TCM1, TCM2, TCM3, TCM4 | — |
| [`COALESCER-CLOSURE-IS-NAMED-DISPOSITION`](MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md) | maintainer-ruling | 2026-08-23 | K3, G2, H2, TCM4 | — |
| [`CSS-WORK-REACHES-J1`](MAINTAINER-RULING-2026-08-23-CSS-WORK-REACHES-J1.md) | maintainer-ruling | 2026-08-23 | J1 | — |
| [`CODE-OVER-LEDGER`](MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md) | maintainer-ruling | 2026-08-22 | BV2, B5, CM1, scripts/validate-program-state.mjs, ledger bookkeeping protocol | — |
| [`BV2-B5-J1-RATIFICATION`](MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md) | maintainer-ruling | 2026-08-22 | BV2, B5, J1, review-mandate protocol | — |
| [`LSP-CSS-READERS-CONSUME-SEMANTICS`](MAINTAINER-ADDENDUM-LSP-CSS-READERS-CONSUME-SEMANTICS.md) | maintainer-ruling | 2026-08-21 | J1 | — |
| [`SEMANTIC-CSS-EXTRACTION-CONSUMERS`](MAINTAINER-ADDENDUM-SEMANTIC-CSS-EXTRACTION-CONSUMERS.md) | maintainer-ruling | 2026-08-21 | J1 | — |
| [`BUILD-LANE-SEPARATION`](MAINTAINER-DIRECTIVE-BUILD-LANE-SEPARATION.md) | maintainer-directive | 2026-08-21 | build architecture, release pipeline, developer workflow | — |
| [`GATE-BLOCK-DEFERS-VERIFICATION`](MAINTAINER-DIRECTIVE-GATE-BLOCK-DEFERS-VERIFICATION.md) | maintainer-directive | 2026-08-21 | gate architecture, verification infrastructure | — |
| [`GATE-PERFORMANCE-BLOCK`](MAINTAINER-DIRECTIVE-GATE-PERFORMANCE-BLOCK.md) | maintainer-directive | 2026-08-21 | gate architecture, verification infrastructure | — |
| [`ONE-BUILD-ONE-RUN`](MAINTAINER-DIRECTIVE-ONE-BUILD-ONE-RUN.md) | maintainer-directive | 2026-08-21 | gate architecture, verification infrastructure | — |
| [`SINGLE-TEST-UNIVERSE`](MAINTAINER-DIRECTIVE-SINGLE-TEST-UNIVERSE.md) | maintainer-directive | 2026-08-21 | gate architecture, verification infrastructure | — |
| [`J-TRAIN-SCOPE-IS-PARSING-ONLY`](MAINTAINER-RULING-J-TRAIN-SCOPE-IS-PARSING-ONLY.md) | maintainer-ruling | 2026-08-21 | J1, J2, J3, J4, CSS/style pipeline architecture | — |
| [`NO-COMPAT-OR-LEGACY-CODE`](MAINTAINER-RULING-NO-COMPAT-OR-LEGACY-CODE.md) | maintainer-ruling | 2026-08-21 | all blocks, all crates, all packages | — |
| [`ONE-CSS-PARSER-PARSE-ONCE`](MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE.md) | maintainer-ruling | 2026-08-21 | J1, J2, J3, J4, CSS/style pipeline architecture | — |
| [`VUE-DOUBLE-PIN-DISPOSITION`](MAINTAINER-RULING-VUE-DOUBLE-PIN-DISPOSITION.md) | maintainer-ruling | 2026-08-21 | Vue oracle pinning, conformance infrastructure | — |
| [`PRE-ENFORCEMENT-ACCEPTANCES`](MAINTAINER-RULING-PRE-ENFORCEMENT-ACCEPTANCES.md) | maintainer-ruling | 2026-08-20 | BF1, BF2, B2, B3, B4 | — |
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

## Architecture rulings (23)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`LSP-DURABLE-FENCE-OWNERSHIP-2026-08-24`](ARCHITECT-RULING-2026-08-24-LSP-DURABLE-FENCE-OWNERSHIP.md) | architecture-ruling | 2026-08-24 | H2, H3, K3 | — |
| [`C1-CHARTER-RATIFICATION-2026-08-24`](ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFICATION.md) | architecture-ruling | 2026-08-24 | C1 | — |
| [`C1-CHARTER-RATIFIABILITY-2026-08-24`](ARCHITECT-RULING-2026-08-24-C1-CHARTER-RATIFIABILITY.md) | architecture-ruling | 2026-08-24 | C1 | — |
| [`TCM0-DECISIONS-2026-08-24`](ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md) | architecture-ruling | 2026-08-24 | TCM0, TCM2, TCM3, TCM4 | — |
| [`SIX-WAY-B6-CM1-2026-08-24`](ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md) | architecture-ruling | 2026-08-24 | B6, BF1, CM1, governance.md, performance-gates.toml | — |
| [`CM1-CONTROL-AXIS-AMENDMENT-2026-08-24`](ARCHITECT-RULING-2026-08-24-CM1-CONTROL-AXIS-AMENDMENT.md) | architecture-ruling | 2026-08-24 | CM1 | — |
| [`B6-ROUTE-OVERHEAD-CELL-LOCK-2026-08-23`](ARCHITECT-RULING-2026-08-23-B6-ROUTE-OVERHEAD-CELL-LOCK.md) | architecture-ruling | 2026-08-23 | B6, BF1, performance-gates.toml | — |
| [`TIMING-ARCHITECTURE-2026-08-23`](ARCHITECT-RULING-2026-08-23-TIMING-ARCHITECTURE.md) | architecture-ruling | 2026-08-23 | program-wide timing architecture (production and tests) | — |
| [`CSS-ALLOCATION-OWNERSHIP-2026-08-23`](ARCHITECT-RULING-2026-08-23-CSS-ALLOCATION-OWNERSHIP.md) | architecture-ruling | 2026-08-23 | J1 | — |
| [`CSS-FRAMEWORK-CONSTRUCT-VALIDITY`](ARCH-RULING-CSS-FRAMEWORK-CONSTRUCT-VALIDITY.md) | architecture-ruling | 2026-08-21 | J1, J4, CSS/style pipeline architecture | — |
| [`J-TRAIN-FIVE-FORKS`](ARCH-RULING-J-TRAIN-FIVE-FORKS.md) | architecture-ruling | 2026-08-20 | J1, J2, J3, J4 | — |
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

## Procedures (1)

| ID | Type | Date | Binds | Superseded by |
|---|---|---|---|---|
| [`CONTEXT-PACKET-DISPATCH-PROCEDURE`](context-packet-dispatch-procedure.md) | procedure | 2026-08-22 | — | — |
