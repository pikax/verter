# Revision 11 package — extraction index

**Source:** `<DOWNLOADS>/verter-architecture-lock-master-plan-v11.md` (8947 lines, 402905 bytes), where `<DOWNLOADS>` is the operator's local downloads directory; the same file is landed at `consolidated/verter-architecture-lock-master-plan-v11.md`.

**Delimiter convention:** every package file is introduced by a line `# Consolidated source: `<relative-path>``, preceded by a `---` horizontal rule; the file body runs verbatim from the next non-blank line until the `---` preceding the next such heading.

**Fidelity note:** markdown bodies are byte-verbatim apart from trailing blank-line padding introduced by the consolidator. The five non-markdown package files (`program-dag.toml` and the four `templates/*.toml`) are wrapped in a ```toml fence in the reading copy; that fence was stripped on reconstruction so each file is valid TOML.

## Files written (67)

| Relative path | Lines |
|---|---|
| `OPUS-START-HERE.md` | 83 |
| `ORCHESTRATOR.md` | 157 |
| `package-README.md` (the package's canonical `README.md`, landed under this name; the landed `README.md` is a separate repository-local index, not a package file) | 205 |
| `agents/opus-bootstrap.md` | 21 |
| `architecture.md` | 1262 |
| `baseline/9af553dd.md` | 294 |
| `charters/A0.md` | 34 |
| `charters/A1.md` | 34 |
| `charters/A2.md` | 34 |
| `charters/A3.md` | 34 |
| `charters/A4.md` | 34 |
| `charters/A5.md` | 34 |
| `charters/A6.md` | 34 |
| `charters/B1.template.md` | 36 |
| `charters/J1.template.md` | 27 |
| `contracts/agent-orchestration.md` | 164 |
| `contracts/architecture-falsification.md` | 122 |
| `contracts/baseline-lock.md` | 97 |
| `contracts/capability-matrix.md` | 31 |
| `contracts/compile-transaction.md` | 95 |
| `contracts/current-tree-reconciliation.md` | 56 |
| `contracts/deterministic-ordering.md` | 74 |
| `contracts/flow-completeness.md` | 104 |
| `contracts/identity-encoding.md` | 60 |
| `contracts/input-loading.md` | 92 |
| `contracts/mapping-products.md` | 57 |
| `contracts/package-publication.md` | 74 |
| `contracts/parse-ownership.md` | 85 |
| `contracts/result-contract-and-flight.md` | 132 |
| `contracts/semantic-profile.md` | 58 |
| `contracts/stacked-prs.md` | 172 |
| `decisions/ADR-001-semantic-authority-and-derived-projections.md` | 28 |
| `decisions/ADR-002-compatibility-domains.md` | 28 |
| `decisions/ADR-003-sealed-compile-semantic-facade.md` | 23 |
| `decisions/ADR-004-typescript-semantic-profiles.md` | 42 |
| `decisions/ADR-005-operation-dtos-and-optional-graph-export.md` | 23 |
| `decisions/ADR-006-demand-selected-flow-domains.md` | 23 |
| `decisions/ADR-007-direct-core-before-managed-runtime.md` | 23 |
| `decisions/ADR-008-deterministic-artifacts-and-persistence.md` | 19 |
| `decisions/ADR-009-shared-frontends-and-parse-owner-domains.md` | 36 |
| `decisions/ADR-010-compositional-products-and-mapping-taxonomy.md` | 41 |
| `decisions/ADR-011-staged-compile-attempt-and-input-loading.md` | 37 |
| `decisions/ADR-012-stable-identifiers-and-canonical-ordering.md` | 29 |
| `decisions/ADR-013-result-contracts-and-flight-owned-production.md` | 38 |
| `decisions/ADR-014-atomic-flow-cutover-and-obligation-proof.md` | 30 |
| `decisions/ADR-015-binding-dependency-direction.md` | 45 |
| `decisions/ADR-016-implementation-lock-and-performance-gates.md` | 36 |
| `decisions/ADR-017-stack-aware-review-and-landing.md` | 33 |
| `decisions/ADR-018-opus-adapter-and-orchestrator-state.md` | 31 |
| `decisions/ADR-019-reproducible-authority-package.md` | 28 |
| `decisions/ADR-020-constitutional-invariants-and-falsifiable-tactics.md` | 29 |
| `governance.md` | 378 |
| `implementation-readiness-review-v10.md` | 178 |
| `implementation-readiness-review-v9.md` | 126 |
| `program-dag.toml` | 309 |
| `program.md` | 462 |
| `templates/architecture-premise-ledger.template.md` | 28 |
| `templates/block-charter.md` | 89 |
| `templates/context-packet.md` | 79 |
| `templates/implementation-lock-record.md` | 129 |
| `templates/landing-equivalence.template.toml` | 54 |
| `templates/performance-gates.template.toml` | 115 |
| `templates/pr-description.md` | 78 |
| `templates/program-state.template.toml` | 1100 |
| `templates/review-report.md` | 46 |
| `templates/stack-window.template.toml` | 75 |
| `verification.md` | 864 |

## Referenced package files NOT reconstructed (14 entries)

**Reason (applies to all):** the consolidated master is generated from `consolidation-order.txt` and concatenates only the `.md`/`.toml` authority documents. Generated metadata (`MANIFEST.json`, `VALIDATION.json`), the consolidation-order file itself, the Python tool sources under `tools/`, and the optional non-normative Claude Code role adapters under `agents/claude-code/` are never concatenated into the reading copy, so their content does not exist in the source and cannot be reconstructed from it. They are named in the package README's authority map (`package-README.md`, section 5 — the landed repository-local `README.md` index has no such section) and in `contracts/package-publication.md`.

- `MANIFEST.json`
- `VALIDATION.json`
- `consolidation-order.txt`
- `tools/validate_package.py`
- `tools/validate_performance_gates.py`
- `tools/validate_program_state.py`
- `tools/validate_stack_window.py`
- `tools/validate_landing_equivalence.py`
- `tools/selftest_orchestration.py`
- `tools/build_release.py`
- `tools/build_consolidated.py`
- `tools/build_deterministic_zip.py`
- `agents/claude-code/README.md`
- `agents/claude-code/*.md (individual role adapter files; names are not enumerated anywhere in the consolidated copy)`

The package manifest is stated to contain 85 files; 67 were reconstructed and 14 known-referenced entries could not be (the `agents/claude-code/*.md` row covers an unenumerated set, which accounts for the residual difference).
