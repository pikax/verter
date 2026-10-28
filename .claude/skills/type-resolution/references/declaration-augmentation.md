## Declaration Augmentation (CRITICAL)

Ambient declaration augmentation (`declare module "X" { ... }` / `declare global { ... }`) is a RETAINED, addressable scoped inventory — never fingerprint-only facts and never file-scope pollution.

**Typed inventory (the single source of truth).** The binder retains every augmentation-block inner declaration on `EvalEnv`, keyed by `(AugmentationScopeKind {Global, Module(specifier)}, name)`:

- `EvalEnv.augmentation_scopes` → ordered `TypeDeclGroup` (interfaces / type aliases), served by `ShallowFileState::augmentation_type_decl(scope, name)` as a demand-lowered `Arc<LoweredTypeDecl>` (the eager header inventory lists the scoped names; bodies lower through the memo on first touch).
- `EvalEnv.augmentation_value_scopes` → ordered `ValueDeclGroup` (`const`/`let`/`var`, `function`, `class`, `enum`), served by `ShallowFileState::augmentation_value_decl(scope, name)` as a demand-lowered `Arc<LoweredValueDecl>`. An enum a block declares, directly or in a namespace nested in the block (`declare global { namespace N { enum E {} } }` is `N.E`), is a block of the enum its scope augments: dual-space headers plus its member names and positions in `DeclHeaderIndex::augmentation_enum_headers`, and its prepared value (`prepare_augmentation_value`) carries `enum_members`. `enum_type::enum_blocks` gathers those blocks through `collect_augmentation_contributions` (`AugmentationContribution::EnumBlocks`) — see `global-names.md`.

Inner declarations NEVER enter file-scope `type_symbols`/`value_symbols`. Parse-domain `ModuleAugmentationFact`s are DERIVED from this typed inventory (`fact_emission::collect_augmentations`) — one fact per `(scope, name)` carrying the augmented name, its symbol space, and a HEADER-level shape fingerprint (kind + syntactic member headers + contributor count): publish hashes no bodies (honoring the zero-body-lowering publish invariant), so the fingerprint is invariant under a member's VALUE-type edit. Body sensitivity for every augmentation consumer therefore rides the per-contributor `FileWholeHash` facts, which BOTH the semantic augmentation stitch (`project_semantic_dispatch::collect_augmentation_contributions`) AND the compile-tier augmentation rail (`compile_fact_emission::observe_augmentation_fingerprints`) record alongside the header-level `ModuleAugmentationIndexShape` fingerprint — a member-body edit moves the augmenter's whole-hash and invalidates the consumer on both rails. There is NO raw-source byte-scan (Build Philosophy: no stage rescans raw source to rediscover what shallow processing captured — guard `shallow_walk_invariant::fact_emission_reads_only_shallow_state_never_raw_source`). There is no `ScopeId.kind`/`semantic_query::ScopeKind` — the live addressing is `AugmentationScopeKind` alone.

**Cross-file augmentation merge is the SAME `MergedDecl` peer-merge path as same-file merging — NOT a second merge engine.** When a declaration `(canonical, name)` is instantiated, `stitch_module_augmentations` (in `project_semantic_dispatch::build`) finds every augmenter file via `FileArtifactStore::ensure_augmentation_index_populated`, fetches each augmenter's RETAINED inner declaration from the memo-owned augmentation inventory (`ShallowFileState::augmentation_type_decl(scope, name)` / `augmentation_value_decl(scope, name)`; typed-IR only — never a source/byte scan in the resolver), lowers it in the augmenter's own file context through `prepare_augmentation_type_decl` + `lower_decl_body_with_provenance`, and folds base ∪ augmenter contributions into ONE `SemanticNodeData::MergedDecl` carrier (flattening the base body if it is already a `MergedDecl`). The shared contributor splitter also traverses nested `MergedDecl` groups from other files, iteratively and in declaration order: a same-file merge contributing to a cross-file merge retains every own member, including the ES5 members of the globally merged `String` interface. Augmenter order is the stable `(canonical, parse_stable_hash)` key — discovery-order-independent. Relative-augmenter discovery loads the base's `reverse_deps_for` before the index scan. `collect_augmentation_contributions` reads every contributor's served source, artifact answer and contributing blocks first, then lowers all of their type bodies as ONE batch through `OwnedLowering::batch_type_bodies` (one decl-lowering rendezvous per occupied worker shard; each statement lowers once for its facts and its body — see `declaration-headers.md`) before folding them one by one in declaration order through each contributor's own `LowerLocator`. A type-body fold is context-free up to its projection, so it is planned once per dispatch transaction (`AugmentationPlan`, kept in `CheckerDispatchTransaction::augmentation` keyed by target, name, arguments, requesting file and excluded contributor): a declaration instantiated under several projection contexts in one request (`M[keyof M]` reads `M` published for the index and in structural transit for the access) replays the fold's recorded reads, file self-roots, refusal and canonical evidence into the live scopes — the flow-result table's reuse rule — and only projects the planned bodies under the new context. A fold is kept only when complete and every shape fetch answered a value. A fold that finds neither an augmenter nor another file-scope declaration for the target decides that before it reads the arguments or the projection context, so the transaction keeps that absence keyed without the arguments (`AugmentationAbsenceKey`): every later instantiation of the declaration in the request — `Paths<…>` applied a thousand times — replays the absent read's facts, refusal and self-roots instead of indexing every reverse dependency again. Relative specifiers the augmentation-index population resolved in the same read are not resolved again for the per-fact target match (`OwnedLowering::augmentation_index_with_resolutions`), and the candidate-augmenter indexing pass runs inside the recorded fold.

**Overlay-aware index.** `AugmentationTargetKey.population: AugmentationPopulation {Base, Session(u64)}`. A `Base` scan reads `is_base()` artifacts only (base parse-env hash + current parser version); a `Session` scan reads the session's overlay artifacts (matched by the session overlay discriminator) UNIONED with base, keyed by the overlay-set fingerprint. **`Session(u64)` is the overlay-set fingerprint, NOT a raw session id** — the producer (the body stitch) and the route-surface validator (`HostStoreView::validates_route_surface_domain`) derive `(population, overlay_discriminator)` through the SINGLE shared `session_view::augmentation_population_for_view`, so a session view can never be cached as, or validated against, a base-only set. Overlay augmenters NEVER poison the base index and NEVER cross sessions. There is NO base-only `assert!(view.compat_token().session.is_none(), …)` on the augmentation-index surface — a session view is accepted under `Session` population.

**Cross-file FACTS** ride the stitch's own rail: the cold stitch observes one `FactKey::ModuleAugmentationIndexShape` (the augmenter-set fingerprint — the SOLE `RouteSurface` fact) plus one `FileWholeHash` per contributing file (base ∪ augmenters), and records `self_root_canonicals = {base} ∪ {augmenters}`. A content edit to ANY contributor misses the warm read; torn/partial routes through `ReturnOnly`, never warmed. Query keys stay content-free (R6).

**Guards**: `session_overlay_augmenter_isolated_from_base_index`, `session_overlay_augmentation_isolated_from_base_meta` (the e2e overlay-isolation oracle).

**Shared collection and finite reads.** `GlobalContributorPopulation` owns
project-filtered global views and augmentation collection orders. Each view is
materialized once for its immutable contributor population and workspace
membership revision. The collection order contains `AugmentationCandidate`
slots into the shared augmenter/contributor arrays, never copied lists per
declaring base. It preserves configured `files` precedence, canonical/hash
ties, and the interface/namespace co-declaration rule. Configured-file ranks
are indexed once on the owning workspace snapshot. Canonical/hash-ordered
source arrays merge linearly, and four stable byte-radix passes apply configured
ranks without comparison sorting. The work metric includes merging, rank
visits, bucket offsets and entry moves. Global interface bases apply defaults
in their declaring scope before reusing the first declaration's shared
instantiation; the application retains all supplied positional arguments even
when the first declaration has fewer parameters. Peers bind any remaining
defaults in their own contributor contexts. Wide signature completion indexes
the observed root hashes in one fact pass and removes precise roots through
that same index, preserving strict validation of every contributor. The probe's
work metric includes root-fact visits and lookups. Content and source-environment
facts continue to fence every merged result.

Finite indexed access over an object resolves its shared subject once through
`normalize_node_for_structural_fact_demand`,
builds one `SurfaceKeyIndex`, and applies the path walker's known-member and
terminal rules to the keys. It retains optional-property widening, accessors,
visible method overloads, key coercion and projection-mode semantics. Each
terminal has independent cycle tracking, so repeated references to one
declaration expand as they do in separate indexed reads. An absent
key or unsupported subject uses the ordinary indexed-access path. This avoids
one full merged-surface construction and dependency closure per member;
`M[keyof M]` and a subsequent member projection have linear member visits and
graph edges without changing operational limits.

Deferred evaluation can read a declaration's current augmentation even when
the declaring file's own bytes are unchanged. Its memo therefore stores the
evaluation's fact signature alongside its result and cost receipt, validates
contributing file roots on every hit, and replays those facts into the caller.
All observed whole-file facts are strict roots: the memo key has no single
file owner, and an untracked contributor must reject the cached answer.
Each continuation frame traces its own reads; completing a child folds its
signature into its parent. The metadata and the shared signature storage use the existing
aggregate retention account. A newly added consumer must observe augmenter
edits and additions even when its declaration subject was already evaluated.

**Ambient namespace body context.** A module-scoped namespace body keeps its
source-ordered `augmentation_namespace_blocks` ordinal as
`NodeLocalScopeId::AugmentationNamespace` in `NodeScopeId::File.local_scope`,
pinned by the canonical file, exact owner and whole-file hash. Lexical table
indices use the distinct `NodeLocalScopeId::Lexical(LocalScopeId)` variant;
an equal ordinal never selects the ambient namespace inventory. Scope hashing
and stable-key encoding preserve this discriminator.
Module-root declaration bodies retain their `augmentation_blocks` ordinal as
`NodeLocalScopeId::AugmentationModule`, with the same generation and owner
fences. A namespace alias that reaches a module-root alias therefore keeps the
exact module inventory when that alias demands its own sibling dependencies.
Both locator-shape lowering and contribution projection use that scope. Lexical
references consult the exact module inventory, walking enclosing namespace
prefixes before the module root, before file/import/global lookup. They stay
scoped carriers until the shared ambient contribution folder reads the demanded
type, including supplied generic arguments. No sibling map or second resolver
is stored on a carrier. Contribution parameters use the shared positional/default
binder; their defaults are read through `AugmentationBody` locators in the
contributor scope, so earlier parameters can feed later defaults.
Body projection preserves scoped member references as back-edges, lowering their
arguments without unfolding sibling bodies. Alias roots and conditional operands
resolve siblings through the contribution folder rather than retaining a bare
reference as their answer. The consumer's reference demand
uses the same ambient folder, so recursive interfaces remain projectable without
adding a separate recursive resolver or a second cycle table.
The bound locators used to reconstruct declaration binder identities must carry
that same augmentation scope: embedded constraints/defaults are part of the
binder node, so a file-scope retry can break even explicit-argument substitution.
A single contribution retains its actual type (including an alias reference or
scalar); only multiple contributors require peer surface merging.

Namespace `typeof` roots bind values in that same inventory before file/global
lookup, including nested namespace roots whose members have qualified value
headers. The shared value builder consumes those headers before projecting any
remaining property path. Their qualified name and augmentation binder identity
reach the shared value builder, which prepares the exact module-scoped declaration
and reads its authored value position through an `AugmentationBody` locator even when another
module declares the same namespace and value names. Enum member types
follow the shared ambient import value route before enum projection. An enum
member referenced by a sibling alias first binds its owning enum in the lexical
namespace inventory, before file/global lookup, then uses the same enum projector.
Ambient value import roots carry the module's structural binder identity and
test declaration presence in that exact inventory; another module or file-level
namesake never makes that presence ambiguous. Enum preparation shares the exact
augmentation-value reader with `typeof`. Enum literals retain the declaring
module specifier alongside their declaration identity through interning, stable
keys, union printing, publication proof and widening, so subsequent enum demands
cannot fall back to a file-global enum of the same qualified name.
Function value signature jobs carry that same known augmentation into their
authored body locators, including ambient default exports. Their return and
parameter demands never reconstruct a file-scope locator for a scoped root.
A scoped namespace root is composed from its namespace inventory, not prepared
as an ordinary value declaration; each member retains the module binder.
Namespace classes retain both their instance type and constructor value through
the ordinary class header and body lowerers, including explicit exports and
generic defaults.
