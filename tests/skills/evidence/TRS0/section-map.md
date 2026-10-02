# Type-resolution reference section map

Original source: `.claude/skills/type-resolution/SKILL.md`, captured before this split.
The front matter and `# Type Resolution` title remain in the entry point.
Ranges include their trailing blank lines. Read each destination's mapped chunks
in table order to reconstruct the original body; repeated destinations append
chunks in source order. No heading levels or relative links change.

Whole subordinate sections and existing paragraph groups are separated only at
existing boundaries. Each original byte belongs to exactly one row. Sections
with several future consumers stay with the earlier owner; later owners put
superseding notes in their own file for L4 documentation closure.

| Order | Original lines | Section or paragraph group | Reference file | Owner |
| --- | --- | --- | --- | --- |
| 1 | 8–20 | Project-Global Cache Authority (post-rewrite) | `cache-authority.md` | TRS0 |
| 2 | 21–21 | SemanticExecution continuation runtime | `continuation-runtime.md` | SKR-RUNTIME |
| 3 | 22–53 | Project-Global Cache Authority (post-rewrite) (continued) | `cache-authority.md` | TRS0 |
| 4 | 54–212 | Canonical Dependency Cache Rule | `canonical-dependency-cache.md` | TRS0 |
| 5 | 213–239 | IndexedReady Target Contract | `declaration-headers.md` | SKR-P-HEADERS |
| 6 | 240–364 | Semantic Query Identity Target Contract | `continuation-runtime.md` | SKR-RUNTIME |
| 7 | 365–387 | Semantic Heuristic Prevention (CRITICAL) | `semantic-heuristic-prevention.md` | TRS0 |
| 8 | 388–398 | Typed Degradation And Completeness Contract (CRITICAL) | `typed-degradation.md` | TRS0 |
| 9 | 399–423 | Cache Population Target Contract | `limits-and-retention.md` | SKR-LIMITS |
| 10 | 424–469 | Query Mode Contract | `query-contracts.md` | TRS0 |
| 11 | 470–496 | Reverse-homomorphic mapped recovery and conditional-infer identity | `inference-owner.md` | SKR-INFER-1 |
| 12 | 497–504 | Query Mode Contract (continued) | `query-contracts.md` | TRS0 |
| 13 | 505–506 | Projection request fuse | `request-budget.md` | SKR-FUSE |
| 14 | 507–525 | Query Mode Contract (continued) | `query-contracts.md` | TRS0 |
| 15 | 526–550 | Path-Precise Navigation And Projection Contract | `conditional-decisions.md` | SKR-COND |
| 16 | 551–574 | Navigator Boundary Contract | `navigator-boundary.md` | TRS0 |
| 17 | 575–596 | Generic Navigation And Expansion Contract | `generic-navigation-and-expansion.md` | TRS0 |
| 18 | 597–623 | Derivation / Origin Layer Contract | `semantic-observe.md` | SKR-OBS |
| 19 | 624–710 | Worked Examples | `worked-examples.md` | TRS0 |
| 20 | 711–760 | Shallow File State and Frontier Engine | `shallow-file-state-and-frontier.md` | TRS0 |
| 21 | 761–790 | Semantic Dispatch (current authority) | `relation-ownership.md` | SKR-OWN-1 |
| 22 | 791–792 | Class surfaces and overload sets | `class-flow.md` | SKR-CLASS-1 |
| 23 | 793–840 | Semantic Dispatch (current authority) (continued) | `relation-ownership.md` | SKR-OWN-1 |
| 24 | 841–850 | Semantic complexity caps and retained safety fuses | `request-budget.md` | SKR-FUSE |
| 25 | 851–878 | Semantic Dispatch (current authority) (continued) | `relation-ownership.md` | SKR-OWN-1 |
| 26 | 879–902 | Retired solver surface and retained carriers | `retained-carriers.md` | TRS0 |
| 27 | 903–916 | Declaration symbol inventories | `declaration-headers.md` | SKR-P-HEADERS |
| 28 | 917–940 | Declaration Merging (CRITICAL) | `declaration-merging.md` | TRS0 |
| 29 | 941–959 | Declaration Augmentation (CRITICAL) | `declaration-augmentation.md` | TRS0 |
| 30 | 960–980 | Global Names (one resolution path) | `global-names.md` | TRS0 |
| 31 | 981–1015 | Cross-File Type Resolution (Compiler Integration) | `cross-file-compiler-integration.md` | TRS0 |
| 32 | 1016–1047 | Macro Type Traversal Rule | `macro-type-traversal.md` | TRS0 |
| 33 | 1048–1094 | Typed-IR-Only Resolver Rule (CRITICAL) | `typed-ir-resolver.md` | TRS0 |
| 34 | 1095–1192 | PARSELOWER Carrier Contracts (handle-migration foundation) | `carrier-contracts.md` | TRS0 |
| 35 | 1193–1196 | Frontier Engine Tests | `test-and-guard-layout.md` | SKR-ENGINE-TESTS |
| 36 | 1197–1453 | Flow-Return Substrate (U6) | `flow-return-substrate.md` | TRS0 |
| 37 | 1454–1469 | Tagged-Component Publication: Root Admission Commits, Members Backfill | `tagged-component-publication.md` | TRS0 |
| 38 | 1470–1486 | Template class fact demand | `template-class-facts.md` | TRS0 |
| 39 | 1487–1591 | Reactive-wrapper demand (shared vocabulary) | `reactive-wrapper-demand.md` | TRS0 |
| 40 | 1592–1601 | Signature discovery (`SignaturesOfType` / `ReadSignatureResult`) | `signature-discovery.md` | TRS0 |

Owner overlap: CLASS-3 supersedes CLASS-1 class/heritage text in `class-flow-solve.md`; INFER-1 supersedes OWN-1 relation/inference text in `inference-owner.md`; RUNTIME supersedes earlier relation/inference execution text in `continuation-runtime.md`; PARALLEL supersedes RUNTIME cooperative execution text in `parallel-execution.md`; OBS supersedes OWN-1 explanation-policy text in `semantic-observe.md`; FUSE supersedes earlier owner budget references in `request-budget.md`; LIMITS supersedes earlier owner numerical envelopes in `limits-and-retention.md`. RET-F freshness-history mechanics have no standalone current section and are pre-registered as a stub. Other owner files with no assigned current section are likewise stubs.

`lossless-move.diff` records the unified diff between the original section bodies
and the reconstruction above. It is empty because the move makes no byte changes.
The verification runs externally to the repository; this evidence adds no test,
validator, link checker or CI gate. Existing one-level references in the shared
compiler-codegen skill and the agent reference-loading instructions support
on-demand loading without changing skill discovery.

All thirteen receiving charters already contain the 2026-10-02 skill-doc amendment
naming their reference and forbidding edits to the entry point or another owner's
file. TRS0 is already a direct predecessor of each receiver in the controller-owned
plan; no repository charter or plan file is introduced by this split.
