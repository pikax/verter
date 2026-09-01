<!-- unified-charter-v2
id=SM2
name=Import-meta glob membership and incremental invalidation
predecessors=SM1,IDX0
phase=expansion
train=expansion.source-modules
product=source_modules
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.source-modules:static source-module facts, provenance, read sets, and membership authority
conflict_domains=source_module_facts,semantic_cache_store
resource_class=rust-mixed
gate_profile=canonical
review_profile=concurrency-3
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
charter=charters/expansion-source-modules/SM2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SM2 — Import-meta glob membership and incremental invalidation

## Independently acceptable outcome and owners

Own static `import.meta.glob` pattern normalization, bounded membership, option facts, generated key/module relations, and precise add/remove/rename invalidation. Current enumeration is host/consumer-specific; final authority is `GlobMembershipSet` backed by IDX0 candidates but resolved under SM rules.

## Surfaces, APIs, and predecessor contracts

Expected surfaces are workspace/session source-module services and JS capture adapters. APIs: `GlobPattern`, `GlobOptions`, `GlobMembershipId`, `GlobMembershipSet`, `GlobReadSet`, `GlobDelta`. `SM1` supplies source-module environment/provenance; `IDX0` supplies bounded file candidates and never decides semantic membership.

## Binding architecture and subblocks

1. Parse only admitted literal string/array patterns and typed options, preserving authored key spelling.
2. Evaluate membership against project roots, exclusions, symlink/casing policy, and source-module environment.
3. Publish deterministic ordered relations and precise file/config read sets.
4. Apply add/remove/rename/edit/revert deltas atomically with cancellation and budget outcomes.

Membership cache identity includes pattern/options/importer/project/source-module environment and discovery generation. Partial enumeration is never a negative fact. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate glob-aware navigation, completion, checker, and build consumers; delete their enumerators only after exact parity. Forbid unbounded workspace scans, eager file reads, regex-only glob parsing, OS-order publication, and negative caching after cancellation/budget exhaustion.

- **SM2-AC1:** positive/negative/multi-pattern/eager/query/import/key fixtures match captured Vite behavior.
- **SM2-AC2:** planted omitted exclusion, importer, or discovery generation fails.
- **SM2-AC3:** add/remove/rename/edit/revert sequences equal fresh membership.
- **SM2-AC4:** warm unchanged membership is zero filesystem work; broad patterns obey budgets and memory plateaus.
- Abort if implementation requires bundler output generation or changes IDX0 into semantic authority.
- Verify workspace/source-module state-machine and TS capture suites, canonical gate, and `concurrency-3`.

SM3 consumes the membership authority. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
