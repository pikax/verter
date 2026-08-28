from chartergen import write_train


def sb(title, outcome, architecture, changes, proof, sid=None):
    item = {
        'title': title,
        'outcome': outcome,
        'architecture': architecture,
        'changes': changes,
        'proof': proof,
    }
    if sid:
        item['id'] = sid
    return item


specs = {
'NCK0': {
    'outcome': 'Ratify the native semantic checker constitution: one diagnostic authority over the existing resolver, a typed diagnostic result model, a family and feature-slice certification law, and an atomic provider-to-native cutover protocol. This block changes authority and contracts only; it does not implement checker execution.',
    'current_owner': 'fragmented parser diagnostics, framework-specific checks, lint registration, provider diagnostics, LSP merge logic, and legacy Native Checker prose',
    'final_owner': 'the native checker product constitution, with semantic facts owned by their existing resolver and diagnostic evaluation owned by expansion.native-checker',
    'role': 'NCK0 prevents the checker from becoming a second type system. It defines the ownership boundary between semantic fact production, diagnostic evaluation, framework contributions, external oracle certification, lint, publication, and fixes. Every later NCK and generated NCF node must be mechanically derivable from this constitution.',
    'surfaces': [
        '`docs/arch/refactor/rev11/sources/legacy-arch-reconciliation.md` and generated authority catalogs',
        '`crates/verter_identity/src` for stable diagnostic, family, rule, and certification identities',
        '`crates/verter_protocol` for the future public diagnostic batch contract owned with PUB0',
        '`crates/verter_diagnostics`, `crates/verter_actions`, `crates/verter_semantic`, and `crates/verter_session` as future implementation owners',
        '`crates/verter_type_runtime` and `crates/verter_lsp` only for certified observation and cutover boundaries, never native semantic computation',
    ],
    'apis': [
        '`DiagnosticOrigin`, `DiagnosticFamilyId`, `DiagnosticFeatureSliceId`, and `DiagnosticRuleId`',
        '`DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`',
        '`DiagnosticCertification` and immutable family certification receipts',
        '`DiagnosticBasis`, `DiagnosticCompleteness`, and typed operational outcomes',
        '`CorrectionOverlayEntry` as test and certification data, not a runtime compatibility mode',
        '`DiagnosticDedupKey` and the law that one family/profile/slice has one publishing authority',
    ],
    'predecessor_contracts': {
        'UAK1': 'consume the universal-tooling constitution and product split so the checker is a successor product rather than an amendment hidden inside Rev11 finalization.',
        'D8': 'consume complete shared flow, call, contextual, and relation result admission; incomplete flow facts may not be relabeled as checker results.',
        'E4': 'consume reclaimable semantic storage and scoped interning so checker results cannot retain the whole project graph.',
        'G2': 'consume same-key singleflight ownership and ReturnOnly admission laws for checker query families.',
        'TCM4': 'consume the certified external TypeScript observation plane and its exact input basis; the provider is an oracle/owner for uncertified families, never a callback from native queries.',
        'UAO0': 'consume the universal activation, TypeInfo, index, and performance contract lock.',
        'UAP0': 'consume the capability, diagnostics/actions, formatter, coexistence, and public contract lock.',
    },
    'principles': [
        'Exactly one resolver remains authoritative for symbols, types, relation, calls, overloads, contextual typing, and flow. The checker evaluates diagnostic rules over those facts and may not recompute them.',
        'Diagnostic classes remain distinct: parser/recovery, native semantic, framework semantic, lint, external provider, and project/configuration diagnostics share a public shape but not authority or suppression rules.',
        'Certification and cutover occur at `profile + diagnostic family + semantic feature slice`; never at a vague project-wide percentage or one global boolean.',
        'External TypeScript is the oracle and fallback owner for uncertified families. Native checker execution never invokes tsserver or tsgo to decide a native result.',
        'The resolver has one correctness behavior. A reviewed correction overlay records conceded TypeScript bugs for certification; no user-facing compat mode or cache-key spec dimension exists.',
        'Every diagnostic carries authored provenance, exact input basis, completeness, and optional proof/fix references. Identity-less side effects are forbidden.',
        'Shadow observation is non-publishing. Promotion to CertifiedNative atomically suppresses the external family before native publication becomes visible.',
        'No monolithic CheckProgram cache entry is allowed. Program checks are coordinators over scoped region, file, and project-rule queries.',
    ],
    'subblocks': [
        sb('Diagnostic ownership matrix', 'Every diagnostic class, family, and surface has a named owner and no overlapping publication authority.', [
            'Define the authority matrix across parser, semantic checker, framework adapters, lint, external provider, and configuration/project services.',
            'Define which owner may create proof references, suppressions, related locations, and fixes.',
            'Require a stable family and feature-slice identity for every diagnostic capable of cutover.',
        ], [
            'Add the matrix and machine-readable catalog schema to Rev11 authority.',
            'Map existing diagnostics and legacy Native Checker clauses to the new classes.',
            'Reject an uncategorized diagnostic at registration and publication boundaries.',
        ], [
            'A planted duplicate owner or uncategorized diagnostic must fail the catalog validator.',
            'The generated ownership table must be byte-deterministic and complete against registered diagnostics.',
        ]),
        sb('Typed result and operational outcome law', 'Diagnostic results cannot collapse cancellation, stale state, missing inputs, or unsupported capability into empty success.', [
            'Specify complete, NeedInputs, unsupported, cancelled, stale, and superseded outcomes.',
            'Specify that partial diagnostic batches are ReturnOnly and never warm-admitted as complete.',
            'Separate result completeness from an empty diagnostic vector.',
        ], [
            'Amend PUB0 result vocabulary and LRA0 diagnostic provenance requirements.',
            'Reserve the native checker query result domain without adding live query keys yet.',
        ], [
            'Mutation tests must prove empty-complete differs from NeedInputs, cancelled, stale, and unsupported.',
            'Serialization round trips must preserve basis and completeness exactly.',
        ]),
        sb('Family and feature-slice taxonomy', 'The checker can be implemented and certified in bounded slices rather than one train-sized parity claim.', [
            'Define required diagnostic families and a stable feature-slice namespace.',
            'Permit a family to contain many independently generated NCF nodes.',
            'Define terminal criteria as manifest completeness, not a hand-maintained percentage.',
        ], [
            'Bind the family manifest schema and generated-node policy.',
            'Define split and merge rules for slices without renumbering published identities.',
        ], [
            'A missing required slice or duplicate slice identity must fail generation.',
            'A manifest reorder must not change generated node identity or evidence keys.',
        ]),
        sb('Certification and correction-overlay constitution', 'Native parity can be certified against TypeScript without placing TypeScript on the runtime query path or implementing bug-for-bug modes.', [
            'Separate recomputable oracle snapshots from review-gated correction overlays.',
            'Require issue/evidence, semantic rationale, affected slices, and expiry review for each correction.',
            'Disallow production access to oracle values except static explanatory issue metadata explicitly approved by PUB0.',
        ], [
            'Amend TCM3 certification inputs and source atoms.',
            'Define deterministic canonicalization of provider diagnostics before comparison.',
        ], [
            'Planting a runtime provider callback, compat-mode query field, or unreviewed overlay must fail a critical guard.',
            'Recomputing an unchanged oracle corpus must produce byte-identical snapshots.',
        ]),
        sb('Atomic authority transition law', 'A family can move from external ownership to native ownership without duplicates, gaps, or stale mixed publication.', [
            'Define External, ObserveNative, CertifiedNative, and Disabled transitions.',
            'Bind transitions to exact profile, provider epoch, native implementation receipt, and certification receipt.',
            'Require latest-basis publication and cancellation of superseded observation work.',
        ], [
            'Amend COX0/LRA0/PUB0 transition and publication contracts.',
            'Define rollback only to the previous certified authority receipt, never to an implicit fallback.',
        ], [
            'State-machine tests must reject illegal transitions and mixed-epoch batches.',
            'A planted double-publication path must fail before user-visible output.',
        ]),
        sb('Critical guard and source-transfer index', 'The constitution is mechanically tied to durable source atoms and named guards before legacy docs are deleted.', [
            'Name guards for one resolver, no runtime oracle callback, no compat mode, exact authority, typed outcomes, and no monolithic program cache.',
            'Bind legacy Native Checker requirements to exact NCK targets and digests.',
        ], [
            'Register requirement atoms in `legacy-arch-reconciliation.md`.',
            'Add the future guard names to the authority catalog; implementation nodes activate them with code.',
        ], [
            'The legacy disposition validator must refuse deletion if any atom lacks a target charter.',
            'A renamed or removed guard without an amendment must fail authority validation.',
        ]),
    ],
    'laws': [
        'Diagnostic identity is independent of message wording and source position; it is rooted in family, rule, semantic subject, authored anchor identity, profile, and exact input basis.',
        'Severity and presentation are policy fields; they do not change semantic diagnostic identity or cache identity unless the rule itself branches on policy.',
        'A certified family result must name the facts and environment dimensions it read. Provider epoch enters only observation/cutover identity, never native semantic computation.',
        'Diagnostic ordering is deterministic: primary authored location, family ID, rule ID, semantic subject identity, then stable tie-breaker.',
        'Fixes are references to authored edit intents owned by LRA0/LSO7, not opaque text edits embedded in semantic facts.',
    ],
    'migration': [
        'Land this constitution and source atoms before deleting `docs/arch/native-checker.md`.',
        'Do not activate native query keys or publish native semantic diagnostics in NCK0.',
        'Classify every existing diagnostic producer and record unknown cases as blocking migration debt, not inferred ownership.',
        'Update successor DAG and existing contract charters in one amendment so no interim authority contradiction exists.',
    ],
    'deletions': [
        'Delete the legacy Native Checker prose only after all durable clauses are digest-bound to NCK0-NCK7 and generated-family authority.',
        'Delete any proposed checker-specific resolver or TypeScript compatibility-mode design from live authority.',
        'Delete ambiguous claims that a green coverage ledger alone proves TypeScript semantic parity.',
    ],
    'forbidden': [
        'A checker-private type walker, relation engine, overload resolver, flow engine, symbol table, or module resolver.',
        'Runtime tsserver/tsgo calls from a native Check query.',
        'One global native-checker enabled boolean used as a substitute for family/slice authority.',
        'Diagnostics stored as GraphTypeNode arms or identity-less side products.',
        'A monolithic whole-program cache entry or eager workspace check on an interactive leaf request.',
        'Permanent duplicate native and provider diagnostics hidden by message-text deduplication.',
    ],
    'acceptance': [
        '**NCK0-AC-AUTHORITY:** generated ownership and transition tables cover every registered diagnostic origin and reject overlap.',
        '**NCK0-AC-ONE-ENGINE:** static guard text and architecture tests reject any checker semantic resolver surface.',
        '**NCK0-AC-CERTIFICATION:** correction-overlay and oracle rules are exact and contain no runtime compatibility path.',
        '**NCK0-AC-LEGACY:** every durable clause from `native-checker.md` has a digest-bound disposition.',
    ],
    'performance': [
        'The constitution must declare zero hidden work for Disabled and External-only native paths.',
        'Certification cost is test/offline work and must not enter runtime latency budgets.',
    ],
    'abort': [
        'Abort if any diagnostic family cannot name a sole semantic fact owner and a sole publishing owner.',
        'Abort if certification requires generated TypeScript text to become semantic truth rather than oracle input.',
    ],
    'verification': [
        '`programctl validate-authority --module expansion-native-checker` and source-coverage validation.',
        'Schema tests for diagnostic family, authority state, result outcome, and correction overlay catalogs.',
        'Negative mutations for duplicate owner, runtime provider callback, compat-mode field, and unclassified legacy clause.',
    ],
    'consumers': [
        'Unlocks NCK1 and all later native checker implementation.',
        'Provides the diagnostic authority contract consumed by CLI2, LSO8, COX0, LRA0, and PUB0 amendments.',
        'Defines the promotion law used by generated NCF family nodes.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` blob `3e96bf48ec481e97b9fd3067041e21099d194944`.',
        '`docs/arch/native-typeinfo-parity.md` and the D/E/G/TCM authority it was partially absorbed into.',
        '`docs/arch/ts-compat-two-mode-model.md` durable single-spec/correction-overlay decision.',
    ],
},
'NCK1': {
    'outcome': 'Specify the framework-neutral executable-region graph and typed semantic-contribution ingress that the checker will consume, while preserving current semantic ownership and preventing adapters or indexes from becoming resolvers.',
    'current_owner': 'function-only flow structures, parser-specific body identities, framework-specific template analysis, and informal ProgramAnalysisContributor seams',
    'final_owner': 'one validated ExecutableRegionGraph identity model and one typed SemanticContributionBatch ingress consumed by the existing semantic graph and checker',
    'role': 'NCK1 generalizes the function flow substrate into executable regions without rebuilding flow or inventing a framework checker. It defines stable region identity, sparse region topology, contribution provenance, validation, and the boundary between discovery/indexing and authoritative resolution.',
    'surfaces': [
        '`crates/verter_identity/src` and `crates/verter_language/src` for region/profile identities',
        '`crates/verter_parser` and framework parser outputs for region discovery descriptors',
        '`crates/verter_semantic` for region graph and typed contribution contracts',
        '`crates/verter_session` for validated contribution ingestion and project-scoped snapshots',
        '`crates/verter_protocol` only where PUB0 exposes region/provenance diagnostics, not internal graph internals',
    ],
    'apis': [
        '`ExecutableRegionId`, `ExecutableRegionKind`, `ExecutableRegionGraph`, and `ExecutableRegionSnapshot`',
        '`RegionStableHash`, `RegionRevision`, and explicit parent/owner source identities',
        '`SemanticContributionBatch` and closed typed `SemanticContribution` arms',
        '`ContributionProvenance`, `ContributionReadSet`, `ContributionValidation`',
        '`FrameworkRegionDescriptor` and `SemanticContributor` capability contract',
        '`ComponentContract` as a framework-neutral semantic contribution, not a checker-specific side table',
    ],
    'predecessor_contracts': {
        'NCK0': 'consume the diagnostic ownership and no-second-resolver constitution.',
        'UAI0': 'consume exact identity, carrier, parser, and coordinate contracts.',
        'PAR0': 'consume parser ownership and lineage; region discovery may not create a second parser.',
        'IDX0': 'consume atomic semantic contributions and bounded workspace indexes while preserving the rule that indexes are not semantic authority.',
    },
    'principles': [
        'A function is one ExecutableRegionKind, not the definition of an executable region.',
        'Region discovery is syntax/lowering work; type and diagnostic meaning are resolved later by the one semantic engine.',
        'Region nodes are compact and structural. Types, diagnostics, effects, and target-specific presentation live in side tables or query results.',
        'Stable identity is content-derived from semantic body structure and source lineage, cosmetic-insensitive where safe, and never a raw source offset alone.',
        'Contributors emit typed facts and demands. They cannot receive ProjectSemanticDispatch, raw resolver internals, or a callback that resolves types privately.',
        'Every contribution carries profile, source, environment, provenance, dependency read set, and validation status.',
        'IDX0 may index contribution identities and candidates but may not answer checker semantics.',
    ],
    'subblocks': [
        sb('Region identity and taxonomy', 'Every executable body kind has a stable, non-colliding identity and explicit owner.', [
            'Define ModuleTopLevel, Function, StaticBlock, FieldInitializer, ParameterInitializer, DecoratorExpression, TopLevelAwait, and FrameworkRegion kinds.',
            'Separate region identity from transient arena index and source offset.',
            'Define parent region, declaration owner, source unit, profile, and body stable hash components.',
        ], [
            'Add contract types and schema/catalog rows; no live builders in this contract block.',
            'Map existing FunctionFlowGraph identities to the Function region compatibility proof, then retire function-only naming in later implementation.',
        ], [
            'Collision/property tests over reordered declarations, cosmetic edits, and same-offset different source units.',
            'Exact identity changes on semantically relevant body edits and remains stable on approved cosmetic edits.',
        ]),
        sb('Sparse executable-region graph shape', 'The graph represents control and dependency structure without embedding types or per-target state.', [
            'Define compact node/edge tables, entry/exit anchors, child regions, captures, declaration dependencies, and source anchors.',
            'Reuse D8 flow slices for function control flow rather than creating a second CFG.',
            'Permit demand-sliced materialization; whole-region construction is not required for leaf queries.',
        ], [
            'Specify logical graph and future compact storage layout.',
            'Define which edge classes are parser/lowering facts versus semantic facts.',
        ], [
            'Taxonomy guard rejects type nodes, diagnostics, provider handles, or per-feature Vec/String payloads in structural nodes.',
            'A function-region projection must reproduce the existing accepted flow identity and topology facts.',
        ]),
        sb('Typed contribution vocabulary', 'Framework and language contributors can add declarations, bindings, contexts, relations, regions, and component contracts without source synthesis.', [
            'Define a closed initial enum with versioned extension law.',
            'Keep semantic values typed: InjectedDeclaration, ExecutableRegion, Binding, NarrowingFact, ContextualType, RelationDemand, ComponentContract, and DiagnosticRuleDescriptor.',
            'Distinguish contributed facts from declarative demands that the executor must resolve.',
        ], [
            'Add schema/source atoms and exact ownership for every contribution arm.',
            'Forbid fake AST, generated TSX, source text, or mutable type-node injection as semantic truth.',
        ], [
            'Round-trip and exhaustive-match guards for the contribution taxonomy.',
            'Negative compile/static tests prove contributors cannot access private resolver APIs.',
        ]),
        sb('Provenance, read sets, and validation', 'No contributed fact can be admitted without exact source and environment basis.', [
            'Define contribution batch basis, profile epoch, source revision, coordinate encoding, and dependency facts.',
            'Validate self-roots and reject stale, partial, cancelled, cyclic, or foreign-profile contributions.',
            'Specify ReturnOnly behavior for budget-exhausted contribution production.',
        ], [
            'Align validation with FactDomain::ProgramAnalysis and ReadSetSignature laws.',
            'Define deterministic batch ordering and digesting.',
        ], [
            'Mutation tests for stale source, wrong profile, missing read set, and forged complete status.',
            'Incremental contribution snapshot equals a fresh rebuild on the same inputs.',
        ]),
        sb('Contributor capability boundary', 'A contributor is a declarative producer, never a resolver or checker.', [
            'Define capability-scoped input views: indexed syntax facts, carrier metadata, resolved catalog identity, and validated existing facts.',
            'Do not expose raw project store mutation, provider handles, or semantic dispatch.',
            'Define zero-work behavior for profiles without the capability.',
        ], [
            'Amend universal catalog/profile contracts to register contributor capabilities.',
            'Specify separate discovery, indexing, and contribution stages.',
        ], [
            'Static API surface test rejects forbidden resolver/session methods in contributor context.',
            'Disabled profile performs zero contribution construction and zero index writes.',
        ]),
        sb('Migration and compatibility proof', 'Existing function flow and framework facts map into the new contract without dual authorities.', [
            'Characterize existing FunctionFlowGraph and current Vue/Svelte template facts.',
            'Define one-way migration into region/contribution snapshots.',
            'Do not retain legacy and new same-role stores after cutover.',
        ], [
            'Create migration manifest by owner and store.',
            'Assign implementation to NCK2/NCK5 rather than mutating production here.',
        ], [
            'Byte/semantic characterization fixtures prove accepted function behavior survives.',
            'A planted write to a displaced same-role store fails the ownership guard.',
        ]),
    ],
    'laws': [
        'Region IDs are project/profile/source qualified and never alias across files, framework profiles, or parser epochs.',
        'Region graph admission is independent of diagnostic demand; a graph may exist without checking and a check may demand only a slice.',
        'Contribution batches are immutable, sorted, validated, and atomically replaced by basis.',
        'A candidate index may point to contribution identity but cannot store final relation/call/checker verdicts.',
        'FrameworkRegion kinds remain open through profile registration; core code does not branch on Vue or Svelte names.',
    ],
    'migration': [
        'Reserve types and ownership first; NCK2 and NCK5 implement builders and ingestion.',
        'Map existing function-region identity with an explicit compatibility test, then remove legacy naming only when the final consumer moves.',
        'Admit no framework contribution until its profile epoch and validation contract are available.',
    ],
    'deletions': [
        'Delete the function-is-the-only-region assumption from live architecture.',
        'Delete any adapter proposal that receives raw semantic dispatch or synthesizes TSX as semantic truth.',
        'Delete duplicate ProgramAnalysis contribution stores after atomic migration.',
    ],
    'forbidden': [
        'Source-offset-only region IDs, mutable region nodes, or per-node owned collections in hot structural storage.',
        'A framework-specific core enum branch or checker engine.',
        'Index-backed final semantic verdicts.',
        'Unvalidated injected narrowing/contextual/relation facts.',
        'Whole-workspace region graph construction as a prerequisite for an interactive leaf query.',
    ],
    'acceptance': [
        '**NCK1-AC-REGION:** region taxonomy and identity are exact, collision-tested, and preserve FunctionFlowGraph compatibility.',
        '**NCK1-AC-CONTRIBUTION:** every contribution arm has sole ownership, provenance, validation, and no text/fake-AST path.',
        '**NCK1-AC-BOUNDARY:** contributor contexts expose no resolver or provider capability.',
    ],
    'verification': [
        'Authority/schema tests for region and contribution taxonomies.',
        'Property tests for stable region identity and deterministic contribution digests.',
        'Static API-surface negative tests for contributor access to resolver/session internals.',
    ],
    'consumers': [
        'Unlocks NCK2 diagnostic queries and NCK5 framework ingress.',
        'Provides region/target identity input to LSO2 without coupling language-service operations to checker internals.',
        'Provides a future common substrate for compiler and lint consumers that explicitly demand executable regions.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` executable-region and ProgramAnalysisContributor sections.',
        '`docs/arch/native-flow-return.md` function flow substrate requirements transferred to D8.',
        '`docs/arch/multi-framework-adapters-plan.md` durable typed-contribution and one-resolver rules.',
    ],
},
'NCK2': {
    'outcome': 'Implement the native checker query/result substrate: scoped Check query keys, typed DiagnosticBatch results, exact contexts and read sets, same-key production, reclaimable stores, aggregation, and public result conversion. This block implements no broad TypeScript diagnostic catalogue.',
    'current_owner': 'reserved or absent Check query names, ad hoc diagnostic vectors, LSP-owned aggregation, and provider-oriented result assumptions',
    'final_owner': 'ProjectSemanticDispatch and SemanticGraphStore-backed scoped diagnostic query families with typed outcomes and bounded storage',
    'role': 'NCK2 makes diagnostics first-class semantic query values while preserving the one resolver. It supplies the execution, caching, cancellation, and aggregation substrate that NCK3 rules and generated NCF slices use.',
    'surfaces': [
        '`crates/verter_session/src/semantic_query` for keys, contexts, specs, dispatch, and query-family admission',
        '`crates/verter_semantic` for diagnostic query values, bases, stores, and proof references',
        '`crates/verter_diagnostics` for immutable Diagnostic and DiagnosticBatch core types',
        '`crates/verter_protocol` and public adapters jointly with PUB0',
        '`crates/verter_session/tests` for spec-table generation, cache, cancellation, and incremental proofs',
    ],
    'apis': [
        '`SemanticQueryKey::{CheckRegion, CheckFile, CheckProjectRule, CheckExpression}`',
        '`SemanticQueryValue::DiagnosticBatch` and `SemanticQueryValueTag::DiagnosticBatch`',
        '`CheckRegionContext`, `CheckFileContext`, `CheckProjectRuleContext`, `CheckExpressionContext`',
        '`DiagnosticBatch`, `DiagnosticBatchBasis`, `DiagnosticBatchOutcome`, `DiagnosticAggregate`',
        '`DiagnosticQueryStore`, per-family retention policy, and same-key FlightCell ownership',
        '`DiagnosticQuerySpec` generated alongside the enum/spec table',
    ],
    'predecessor_contracts': {
        'NCK1': 'consume exact region identity and typed contribution ingress.',
        'G2': 'consume FlightCell-owned same-key production, cancellation, and admission laws.',
        'H3': 'consume exact-basis foreground settlement and stale-safe background publication semantics.',
        'PUB0': 'consume versioned typed public outcomes and capability truth.',
    },
    'principles': [
        'CheckRegion is the primary semantic unit; CheckFile aggregates exact region and file-rule results; CheckProjectRule owns only genuinely project-scoped rules.',
        'CheckExpression is demand-sliced and interactive; it may not trigger whole-file checking unless the declared rule requirements demand it.',
        'There is no CheckProgram memo family. Workspace checking is an application coordinator over files/project rules with cancellation and progress.',
        'Query keys contain content-free identity and exact environment dimensions; result values are version-rooted by recorded facts.',
        'Only complete, current results admit. Cancelled, superseded, budget-exceeded, partial, or NeedInputs results are ReturnOnly.',
        'Storage is per project/profile/family, reclaimable, and bounded. Diagnostic payloads do not retain semantic arenas unnecessarily.',
    ],
    'subblocks': [
        sb('Query taxonomy and spec-table integration', 'Every live Check key has one generated spec row and dispatch arm.', [
            'Add scoped keys and tags together with value-domain, admission, allowed demand, environment dimensions, and cross-context guard.',
            'Keep project rules separate from file/region execution.',
            'Reserve future finer keys only through an amendment; do not add speculative dead variants.',
        ], [
            'Update enum, spec generator, artifact, tag set, and dispatch exhaustiveness in one change.',
            'Register critical guards for enum/spec/dispatch equality.',
        ], [
            'Enum-spec-dispatch triangulation fails on a planted missing row or dispatch arm.',
            'No Check key resolves to TypeNode or GraphTypeNode.',
        ]),
        sb('Exact query contexts and identities', 'Cross-project, profile, environment, and source revisions cannot warm-hit incorrectly.', [
            'Define per-key contexts with only semantically relevant parse, resolve, type, lib, and project dimensions.',
            'Include region/file/project-rule identity and diagnostic family/slice identity.',
            'Keep content hashes on value/read-set rooting, not as substitutes for semantic identity.',
        ], [
            'Implement family key construction and minimal-dimension characterization tests.',
            'Add cross-context no-warm-hit guards.',
        ], [
            'Mutation matrix changes each identity axis independently and proves correct hit/miss behavior.',
            'Benched minimality rejects unnecessary dimensions that cause false misses.',
        ]),
        sb('DiagnosticBatch core value domain', 'A query result carries immutable diagnostics, basis, completeness, read set, and operational outcome.', [
            'Define compact diagnostic records with stable IDs, primary/related authored anchors, proof refs, and fix-intent refs.',
            'Separate semantic diagnostic identity from localized message rendering.',
            'Use compact interned strings/IDs and avoid retaining full semantic nodes.',
        ], [
            'Implement core types and deterministic canonical ordering.',
            'Add public DTO conversion behind PUB0 schema/version gates.',
        ], [
            'Layout/size tests and deterministic serialization tests.',
            'Empty-complete, NeedInputs, cancelled, stale, and unsupported remain distinguishable.',
        ]),
        sb('Same-key production and admission', 'Concurrent identical checks compute once and only complete current results enter warm storage.', [
            'Use the existing FlightCell/singleflight family runtime.',
            'Bind cancellation, deadline, budget, supersession, and provider-independent semantic basis.',
            'Record exact facts read by the rule executor.',
        ], [
            'Implement producer ownership, waiter behavior, admission probe, and ReturnOnly paths.',
            'Instrument compute, wait, cancellation, and admission counters.',
        ], [
            'Concurrency tests prove one producer and deterministic waiter results.',
            'Poison tests prove a cancelled/partial producer cannot populate the cache.',
        ]),
        sb('Bounded diagnostic storage and reclamation', 'Repeated checks do not create unbounded per-file, per-family, or per-revision retention.', [
            'Define per-family retention and generation replacement.',
            'Store compact values detached from temporary semantic arenas.',
            'Evict superseded profile/source generations and release proof references safely.',
        ], [
            'Add DiagnosticQueryStore under the existing project semantic store ownership.',
            'Add memory counters and explicit teardown/epoch transition behavior.',
        ], [
            'Long-churn tests plateau RSS/retained bytes.',
            'A deleted/closed project releases all checker storage and contributor snapshots.',
        ]),
        sb('File/project aggregation and public conversion', 'Aggregators compose scoped results without becoming another semantic engine or cache authority.', [
            'CheckFile joins exact region and file-rule batches; workspace coordination stays above query storage.',
            'Deduplicate only by stable diagnostic identity and authority, never message text.',
            'Propagate the least complete outcome and exact contributing bases.',
        ], [
            'Implement deterministic aggregation helpers and public DTO conversion.',
            'Leave publication arbitration to NCK6.',
        ], [
            'Fresh versus incremental aggregate equality across reordered region completion.',
            'A mixed stale/current input is rejected rather than published as current.',
        ]),
    ],
    'laws': [
        'A Check key may read semantic facts but may not mutate or create a second semantic node authority.',
        'DiagnosticBatch basis includes project/profile/source/region or rule identity plus canonical fact read-set signature.',
        'Message localization and editor formatting occur after semantic identity/cache computation.',
        'Aggregation is deterministic and non-cache-authoritative unless represented by its own exact scoped query key.',
        'CheckExpression has strict budgets and cannot admit a result that omitted a required rule input.',
    ],
    'migration': [
        'Introduce query values dormant until NCK3 registers rule executors; no user-visible native diagnostic publication in this block.',
        'Migrate ad hoc semantic diagnostic vectors only where exact identity and basis can be preserved; leave parser/lint/provider classes with their owners.',
        'Delete duplicate checker cache prototypes in the same candidate that routes their final consumer.',
    ],
    'deletions': [
        'Delete identity-less semantic diagnostic side channels displaced by DiagnosticBatch.',
        'Delete any ad hoc whole-file or whole-project checker cache introduced during prototyping.',
        'Delete duplicate per-feature same-key coordination for checker queries.',
    ],
    'forbidden': [
        'CheckProgram as a monolithic memoized key.',
        'Diagnostic data embedded in TypeNode/GraphTypeNode or public TypeInfo graph nodes.',
        'Message text or source range as the sole diagnostic identity.',
        'Caching cancelled, partial, stale, budget-exceeded, or NeedInputs outcomes as complete.',
        'A query context that bundles an opaque project_config_hash instead of exact dimensions.',
    ],
    'acceptance': [
        '**NCK2-AC-SPEC:** Check enum, tags, generated spec table, dispatch, and value tags are exactly equal.',
        '**NCK2-AC-ADMISSION:** only complete current batches warm-admit under same-key production.',
        '**NCK2-AC-MEMORY:** repeated revisions and project teardown prove bounded retention.',
        '**NCK2-AC-NO-PROGRAM-CACHE:** static guard rejects monolithic CheckProgram memo authority.',
    ],
    'performance': [
        'Warm identical CheckRegion requests perform zero parse, index walk, semantic recomputation, provider work, and diagnostic allocation beyond result sharing.',
        'CheckFile aggregation is linear in returned scoped results, not project size.',
    ],
    'verification': [
        '`cargo nextest run -p verter_semantic -p verter_session -p verter_diagnostics -p verter_protocol`.',
        'Query-key spec generation/diff and cross-context mutation matrix.',
        'Concurrent same-key, cancellation poison, incremental/fresh, and long-churn memory tests.',
    ],
    'consumers': [
        'Unlocks NCK3 rule execution and NCK4 certification harness integration.',
        'Provides typed result contracts consumed by NCK6, CLI2, LSO8, and public surfaces.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` checker query layer and typed result requirements.',
        '`docs/arch/fact-based-cache.md` query identity/admission laws transferred through G1/G2/E4.',
        'Live `SemanticQueryKeySpec` table and D8 complete-result contract.',
    ],
},
'NCK3': {
    'outcome': 'Implement the shared-proof diagnostic rule kernel that plans fact demands, reads authoritative relation/call/flow/contextual/declaration facts, emits stable diagnostics and fix intents, and proves that no rule re-resolves semantic meaning. Only representative canary rules land here; catalogue parity belongs to generated NCF nodes.',
    'current_owner': 'scattered hard-coded checks, provider diagnostic messages, framework-specific validation, and prospective checker walkers',
    'final_owner': 'a static, typed, demand-declared diagnostic rule kernel over shared semantic proofs',
    'role': 'NCK3 is the semantic checker engine in the narrow sense: not a resolver, but a rule planner/evaluator over existing facts. It establishes one reusable execution contract so every generated family slice implements semantic rules without forking infrastructure.',
    'surfaces': [
        '`crates/verter_diagnostics` for rule descriptors, emission, suppression identity, and compact diagnostic construction',
        '`crates/verter_semantic` for read-only fact/proof views and typed rule demands',
        '`crates/verter_session` for query dispatch integration, rule planning, and exact read-set capture',
        '`crates/verter_actions` for typed fix intents, not direct edits',
        '`crates/verter_session/tests` and semantic fixtures for no-second-resolver guards and canary rules',
    ],
    'apis': [
        '`DiagnosticRuleDescriptor`, `DiagnosticRulePlan`, and static `DiagnosticRuleRegistry`',
        '`FactRequirement`, `RuleApplicability`, `RuleBudget`, and `RuleExecutionContext`',
        '`DiagnosticFactView` exposing typed relation/call/flow/contextual/declaration results only',
        '`ProofRef`, `DiagnosticEmitter`, `SuppressionKey`, and `FixIntentRef`',
        '`RuleExecutionReceipt` with facts read, work counters, and completeness',
    ],
    'predecessor_contracts': {
        'NCK2': 'consume scoped diagnostic queries, typed batches, same-key admission, and bounded stores.',
        'D8': 'consume complete authoritative flow/call/contextual results and completion algebra.',
        'TCM4': 'consume certified external observation for canary differential evidence only.',
        'LRA0': 'consume profile-scoped rule/action registration, provenance, suppression, and authored fix safety contracts.',
    },
    'principles': [
        'Rules declare fact requirements before execution. Applicability and demand planning must permit zero work for irrelevant rules.',
        'The fact view exposes final typed outcomes and proof references, not mutable semantic stores or resolver callbacks.',
        'A negative relation or failed call applicability becomes a diagnostic through a rule; the rule does not rerun relation or overload matching.',
        'Control-flow diagnostics consume existing reachability, completion, return, capture, and narrowing facts.',
        'Rule registration is static/catalog-driven. Executing arbitrary third-party code inside the semantic engine is out of scope.',
        'Suppressions are keyed by stable rule/subject/provenance identity and cannot hide diagnostics from unrelated authorities.',
        'Fixes are semantic intents that LSO7 later materializes against authored current source.',
    ],
    'subblocks': [
        sb('Static rule descriptor and registry', 'Every rule has exact family/slice identity, applicability, fact requirements, severity class, fix capability, and owner.', [
            'Define a generated/static registry keyed by DiagnosticRuleId.',
            'Separate semantic rules from lint and framework-owned rule descriptors while allowing one public shape.',
            'Declare profile/language/region applicability without framework switches in core.',
        ], [
            'Implement descriptor types and registry generation hooks.',
            'Bind every rule to NCK4 manifest rows and LRA0 action policy.',
        ], [
            'Registry completeness and duplicate-ID mutation tests.',
            'An inapplicable rule records zero fact reads and zero allocations.',
        ]),
        sb('Demand planning and applicability', 'The kernel requests only facts required by applicable rules and never whole-checks by default.', [
            'Compile applicable rule requirements into a deterministic RulePlan.',
            'Coalesce identical fact demands while preserving rule attribution.',
            'Propagate budget, cancellation, and NeedInputs before evaluation.',
        ], [
            'Implement planner and demand counters.',
            'Add plan dumps for tests/evidence, not production semantic authority.',
        ], [
            'Permutation tests produce byte-identical plans.',
            'A leaf CheckExpression proves unrelated file/project rules perform zero work.',
        ]),
        sb('Read-only shared fact and proof view', 'Rules can inspect authoritative facts without access to private resolver algorithms or mutable stores.', [
            'Expose typed read methods for relation, resolve-call, overload, flow, contextual, declaration, and project-index facts.',
            'Record every read into the Check query read set.',
            'Return typed incomplete/NeedInputs rather than synthesizing fallback facts.',
        ], [
            'Implement capability-limited view wrappers.',
            'Add compile/static guards banning resolver entry points from rule modules.',
        ], [
            'A planted direct resolver call or store mutation fails static architecture tests.',
            'Read-set mutation invalidates the right cache entry and no broader family.',
        ]),
        sb('Diagnostic emission, proof, and dedup', 'Rule output is stable, authored, evidence-linked, and deterministic.', [
            'Construct semantic diagnostic identity before localized message rendering.',
            'Attach primary and related authored anchors plus optional ProofRef.',
            'Deduplicate by stable identity/authority, not message text.',
        ], [
            'Implement DiagnosticEmitter and canonical sorting.',
            'Create proof retention/refcount policy compatible with NCK2 reclamation.',
        ], [
            'Equivalent reordered fact delivery yields byte-identical batches.',
            'Two distinct semantic subjects with identical messages never collapse.',
        ]),
        sb('Suppression and fix-intent boundary', 'Suppressions and fixes preserve owner, profile, source basis, and safety class.', [
            'Model suppression directives separately from diagnostics and lint configuration.',
            'Emit fix intents containing semantic target and transformation class, never generated-coordinate TextEdits.',
            'Classify safe, suggested, and unsafe intents under LRA0.',
        ], [
            'Implement typed refs and validation hooks; LSO7 remains the edit materializer.',
            'Add duplicate/suppression provenance guards.',
        ], [
            'Stale or foreign-profile suppression fails closed.',
            'No fix intent can be converted without an exact authored basis.',
        ]),
        sb('Representative canary rules and one-engine guards', 'The kernel proves its architecture on a small cross-family set without absorbing the parity train.', [
            'Canaries: assignment relation failure, failed call applicability, missing return/unreachable region, and duplicate declaration project rule.',
            'Each canary must consume an existing authoritative fact and carry an oracle fixture.',
            'No additional family breadth is accepted in NCK3.',
        ], [
            'Implement canaries and named guards.',
            'Record remaining catalogue work only in the NCK4 manifest.',
        ], [
            'Mutation of the underlying shared fact changes the diagnostic; mutation of a duplicate checker algorithm is impossible because none exists.',
            'Canary differential and incremental/fresh tests pass across native and provider observation.',
        ]),
    ],
    'laws': [
        'A rule result is complete only when all declared required facts are complete on the same basis.',
        'Rule execution order does not affect diagnostic identity, ordering, or read-set signature.',
        'Rules cannot mutate semantic facts or write index state.',
        'Framework-owned rules enter through NCK5 descriptors/contributions but run on the same kernel.',
        'Proof references are opaque stable handles with lifecycle tied to the batch/store generation.',
    ],
    'migration': [
        'Move only representative checks whose fact authority and exact replacement can be proven.',
        'Leave lint rules in LRA0/LNT ownership and provider semantic families external until their generated NCF slice is certified.',
        'Delete a displaced hard-coded rule only in the same candidate that routes its complete demand and output through the kernel.',
    ],
    'deletions': [
        'Delete canary-equivalent ad hoc semantic checks and duplicate rule registries.',
        'Delete direct TextEdit construction from semantic checker rules.',
        'Delete any checker-private relation/call/flow helper introduced during implementation.',
    ],
    'forbidden': [
        'Rules parsing source text, regexing type text, synthesizing/reparsing TypeScript, or walking types to reproduce resolver decisions.',
        'Dynamic third-party rule code in the trusted semantic process.',
        'Message-text dedup or range-only suppression.',
        'Rules emitting LSP coordinates or provider handles.',
        'Expanding NCK3 into the full diagnostic catalogue.',
    ],
    'acceptance': [
        '**NCK3-AC-FACTS:** every canary diagnostic is traceable to declared authoritative facts and exact read-set entries.',
        '**NCK3-AC-NO-RESOLVER:** static architecture guard rejects resolver calls and duplicate semantic algorithms in rule modules.',
        '**NCK3-AC-ZERO-WORK:** inapplicable rules execute no fact demand, provider call, or allocation.',
        '**NCK3-AC-CANARIES:** four cross-family canaries pass oracle, incremental, cancellation, and proof tests.',
    ],
    'performance': [
        'Rule planning cost is proportional to applicable registered rules for the selected profile/slice, with catalog indexing preventing global scans.',
        'Repeated warm canary checks allocate only the returned batch representation or share it according to NCK2 policy.',
    ],
    'verification': [
        '`cargo nextest run -p verter_diagnostics -p verter_actions -p verter_semantic -p verter_session`.',
        'Static one-engine guards and rule-registry generation tests.',
        'Canary differential, zero-work, incremental/fresh, cancellation, and proof-lifecycle tests.',
    ],
    'consumers': [
        'Unlocks NCK4 manifest/oracle generation and NCK5 framework rule ingress.',
        'Supplies the sole diagnostic rule execution contract for every generated NCF slice.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` diagnostics-from-facts and named guard sections.',
        'D8 flow/call authority and LRA0 rule/action contract.',
    ],
},
'NCK4': {
    'outcome': 'Implement the machine-readable diagnostic-family manifest, hermetic TypeScript oracle corpus, deterministic diagnostic canonicalizer, review-gated correction overlays, generated NCF DAG/charter production, and evidence receipts. This block creates the parity production system; it does not implement all family slices itself.',
    'current_owner': 'free-form parity prose, scattered ignored tests, manually curated provider expectations, and no checker-family DAG generator',
    'final_owner': 'one source-digest-bound manifest and generator that creates bounded, independently acceptable native checker family slices',
    'role': 'NCK4 converts the multi-person-year checker catalogue into explicit program work. It prevents parity claims from being hidden in a monolithic block and makes certification reproducible, reviewable, and tied to exact TypeScript engine identity.',
    'surfaces': [
        '`docs/arch/refactor/rev11/catalogs` for diagnostic family and correction-overlay schemas',
        '`docs/arch/refactor/rev11/generated` and authority DAG/charters for generated NCF nodes',
        '`crates/verter_session/tests`, `crates/verter_diagnostics/tests`, and hermetic conformance corpora',
        '`crates/verter_type_runtime` or dedicated test harness code for oracle observation only',
        '`tools` or a dedicated Rust generator binary; tests never write generated authority artifacts',
    ],
    'apis': [
        '`DiagnosticFamilyManifest`, `DiagnosticFamilyRow`, `DiagnosticFeatureSliceRow`',
        '`DiagnosticOracleCase`, `OracleEngineIdentity`, `OracleSnapshot`, `DiagnosticCanonicalizer`',
        '`CorrectionOverlay`, `CorrectionOverlayEntry`, and review/expiry metadata',
        '`GeneratedCheckerNodeSpec`, `DiagnosticFamilyReceipt`, and `FamilyPromotionEvidence`',
        '`gen-native-checker-dag` as the sole writer of generated NCF DAG/charter/index artifacts',
    ],
    'predecessor_contracts': {
        'NCK3': 'consume the exact rule kernel and canary execution/evidence format.',
        'TCM4': 'consume certified TypeScript engine binding, input basis, mapping, and observation identity.',
        'VIM1': 'consume deterministic manifest compilation and conformance generation patterns.',
    },
    'principles': [
        'Manifest rows, not prose section headings, define required checker scope and terminal completeness.',
        'One generated NCF node owns one bounded semantic feature slice, exact rule population, exact deletion population, oracle corpus, and certification receipt.',
        'Oracle execution is hermetic and test-only. Production native queries have no access to provider observation.',
        'Diagnostic comparison canonicalizes codes, semantic family, subject, authored locations, related locations, severity, and stable message parameters; raw localized strings are not primary equality.',
        'Correction overlays are sparse, review-gated exceptions for clear TypeScript bugs and cannot become a second runtime behavior.',
        'The generator is the sole writer; tests render in memory and diff committed outputs.',
        'Generated node identity remains stable under manifest reordering and changes only when its semantic slice identity changes.',
    ],
    'subblocks': [
        sb('Manifest schema and family partition', 'The full required diagnostic catalogue is partitioned into stable, bounded slices with no unowned rows.', [
            'Define family, slice, rule population, applicability, prerequisites, oracle cases, deletion owner, and performance counters.',
            'Require explicit required/optional status and terminal coverage.',
            'Allow later versioned additions without renumbering existing slice identity.',
        ], [
            'Implement schema parser/validator and canonical renderer.',
            'Populate initial required families and representative rows.',
        ], [
            'Coverage bijection and duplicate/missing mutation tests.',
            'Reordering input produces identical canonical manifest and generated IDs.',
        ]),
        sb('Hermetic oracle corpus and engine identity', 'Every certified row is reproducible against an exact TypeScript/tsgo engine and exact project inputs.', [
            'Pin engine artifact/version/platform, libs, compiler options, module graph, source encoding, and expected observation surface.',
            'Keep third-party corpora optional and external; required certification fixtures are vendored/hermetic.',
            'Separate syntax/provider failures from semantic diagnostic observations.',
        ], [
            'Implement oracle runner and fixture format.',
            'Capture deterministic snapshots only through an explicit recompute command.',
        ], [
            'Fresh recompute on the same engine/input is byte-identical.',
            'Engine/options/lib mutation changes the oracle identity and invalidates affected receipts.',
        ]),
        sb('Diagnostic canonicalization and comparison', 'Native/provider outputs compare semantically rather than by unstable localized text or generated coordinates.', [
            'Normalize provider codes, categories, message arguments, authored locations, related info, and family mapping.',
            'Map generated/provider coordinates through exact TCM basis and drop unverifiable observations from certification rather than guessing.',
            'Represent missing, extra, mismatched, and non-comparable outcomes explicitly.',
        ], [
            'Implement canonicalizer and structured diff output.',
            'Add cross-platform/locale stability fixtures.',
        ], [
            'Locale and ordering mutations preserve semantic canonical result.',
            'Synthetic/unmappable provider locations cannot be silently accepted as parity.',
        ]),
        sb('Correction overlay and divergence registry', 'Approved TypeScript bugs are represented as sparse data with explicit evidence, never runtime modes.', [
            'Require issue reference or equivalent evidence, affected rows, TS oracle value, Verter correct value, rationale, reviewer receipts, and review date.',
            'Default every non-overlay row to exact TypeScript parity.',
            'Provide expiry/revalidation when TypeScript fixes the bug.',
        ], [
            'Implement overlay schema, validator, and co-presence metadata rules.',
            'Compile only static issue metadata into production when explicitly authorized; oracle values remain test data.',
        ], [
            'Unreviewed, orphaned, broad wildcard, or stale overlay entries fail validation.',
            'Removing an overlay after an upstream fix restores ordinary parity comparison.',
        ]),
        sb('Generated NCF DAG and charter writer', 'Each semantic feature slice becomes a real bounded DAG node with a detailed charter and exact predecessors.', [
            'Derive node ID, name, owner, conflict domains, budgets, source atoms, rule population, oracle fixtures, deletions, and acceptance IDs.',
            'Generate a detailed family charter from row-specific architecture templates; do not emit generic one-line charters.',
            'Require amendment review before generated authority enters the live DAG.',
        ], [
            'Implement `gen-native-checker-dag` and generated output directories.',
            'Add in-memory render/diff tests and cycle/reachability validation.',
        ], [
            'Tests never write generated files.',
            'A row exceeding limits or containing multiple independently acceptable outcomes fails generation and requests manual rescope.',
        ]),
        sb('Certification receipts and promotion evidence', 'A family slice can be promoted only from immutable implementation, oracle, performance, and review evidence.', [
            'Bind candidate tree, implementation receipt, manifest row digest, oracle engine/input, diff result, correction overlays, incremental/fresh proof, and work counters.',
            'Separate observation success from authority promotion.',
            'Make NCK6 consume receipts rather than rerun hidden certification logic.',
        ], [
            'Implement receipt schema and validator.',
            'Generate human-readable evidence summaries from structured data.',
        ], [
            'Changing any input invalidates the receipt.',
            'A clean observation without exact candidate or manifest digest cannot promote authority.',
        ]),
    ],
    'laws': [
        'The family manifest is the exact scope authority; generated reports are derivative and never hand-edited.',
        'Oracle snapshots and correction overlays are test/evidence artifacts, not production semantic dependencies.',
        'Every generated NCF node owns an exact rule set and legacy deletion set; overlapping ownership is invalid.',
        'Certification receipts are immutable and content-addressed.',
        'A non-comparable provider observation is not a pass and cannot be hidden as an ignored test.',
    ],
    'migration': [
        'Import durable parity rows from legacy TypeInfo/checker docs and existing ignored tests into the manifest with explicit status.',
        'Do not mechanically convert every old test into required checker scope without classifying its semantic family and authority.',
        'Generate NCF nodes through an amendment and keep them locked until predecessors and implementation receipts exist.',
    ],
    'deletions': [
        'Delete free-form checker parity ledgers and generator-by-test patterns displaced by the manifest/generator.',
        'Delete wildcard ignored-test acceptance and manually stamped parity percentages.',
        'Delete runtime compatibility-mode scaffolding if any exists.',
    ],
    'forbidden': [
        'One NCK4 implementation claiming the full TypeScript diagnostic catalogue.',
        'Tests mutating checked-in manifests, DAGs, charters, or snapshots.',
        'Localized message text as the sole parity comparator.',
        'Oracle execution in production or network-dependent required certification tests.',
        'Correction overlays without row-exact scope and independent review.',
    ],
    'acceptance': [
        '**NCK4-AC-BIJECTION:** required manifest rows, generated NCF nodes, charters, and terminal coverage are exact bijections.',
        '**NCK4-AC-ORACLE:** hermetic recomputation is deterministic and engine/input identity is exact.',
        '**NCK4-AC-GENERATOR:** dedicated generator is sole writer; tests only assert in-memory equality.',
        '**NCK4-AC-OVERLAY:** sparse correction overlays satisfy evidence, scope, review, and expiry laws.',
    ],
    'performance': [
        'Certification harness performance is measured separately from runtime; generated slice charters still require runtime equivalent-work counters.',
        'Manifest parsing/generation is deterministic and bounded by row count with no repository-wide semantic scan.',
    ],
    'verification': [
        '`cargo nextest run` for manifest, canonicalizer, oracle harness, overlay, receipt, and generator crates/tests.',
        'Run explicit oracle recompute in hermetic mode and compare committed snapshots.',
        'Run generator in check mode plus planted missing/duplicate/oversized/cycle mutations.',
    ],
    'consumers': [
        'Generates the NCF implementation backlog and evidence contract.',
        'Supplies certification receipts consumed by NCK6 authority promotion and NCK7 terminal completeness.',
        'Provides checker rows consumed by LSO8 and CLI conformance when native diagnostics are enabled.',
    ],
    'sources': [
        '`docs/arch/native-typeinfo-parity.md` parity/oracle discipline, corrected so coverage is not semantic parity.',
        '`docs/arch/native-checker.md` separate checker manifest requirement.',
        '`docs/arch/ts-compat-two-mode-model.md` correction-overlay and one-spec rules.',
    ],
},
'NCK5': {
    'outcome': 'Implement validated framework semantic and diagnostic contribution ingress, executable template regions, component-contract facts, profile isolation, and Vue/Svelte canaries over the same checker kernel. Core remains framework-neutral and generated TypeScript remains an interoperability surface, not semantic truth.',
    'current_owner': 'framework-specific generated TSX checks, component-meta adapters, template diagnostics, and incomplete typed contribution seams',
    'final_owner': 'profile-registered typed framework contributions admitted into the same ProgramAnalysisGraph, relation/call/flow facts, and NCK rule kernel',
    'role': 'NCK5 proves that framework templates are equal contributors to one checker rather than separate generated-code checkers. It defines the adapter boundary for regions, bindings, contexts, component contracts, and framework-owned rules while preserving exact profile isolation.',
    'surfaces': [
        '`crates/verter_language/src` and universal catalog/profile registration',
        '`crates/verter_compiler/src/framework` only for typed lowering outputs already owned by framework frontends',
        '`crates/verter_semantic` and `crates/verter_session` for validated contribution snapshots and checker ingress',
        '`crates/verter_vue_conformance` and `crates/verter_svelte_conformance` for canary fixtures',
        '`crates/verter_protocol`/TypeInfo only where component contracts are public through existing universal contracts',
    ],
    'apis': [
        '`FrameworkDiagnosticContributor` or catalog capability equivalent',
        '`FrameworkRegionContribution`, `TemplateScopeContribution`, and `ComponentContract`',
        '`InjectedBinding`, `InjectedNarrowingFact`, `InjectedContextualType`, and `InjectedRelationDemand`',
        '`ProfileSemanticEpoch`, `FrameworkContributionSnapshot`, and `ContributionValidationReceipt`',
        '`FrameworkDiagnosticRuleDescriptor` for intrinsic framework semantics only',
    ],
    'predecessor_contracts': {
        'NCK1': 'consume executable-region and typed-contribution contracts.',
        'NCK3': 'consume the shared diagnostic rule kernel and no-second-resolver boundary.',
        'TIF1': 'consume TypeInfo-first component metadata and component-surface authority.',
        'IDX0': 'consume atomic semantic contribution/index updates and bounded candidate discovery.',
        'VIM1': 'consume deterministic vertical conformance generation.',
    },
    'principles': [
        'Framework adapters lower syntax into typed contributions; they do not resolve types, run private relation/call algorithms, or call a framework checker.',
        'Template control flow and handlers become ExecutableRegionKind::FrameworkRegion with exact authored coordinates and profile identity.',
        'Generated TSX may remain an external-provider interoperability carrier but cannot be the native semantic fact source.',
        'Component contracts are framework-neutral: inputs/props, outputs/events, slots/children/content, exposed instance, refs, directives/actions, and lifecycle bindings.',
        'Framework-owned diagnostics are limited to intrinsic framework semantics. TypeScript-semantic diagnostics use common NCF rules over shared facts.',
        'Contributions are admitted atomically per profile/source basis and invalidated by exact read sets.',
        'No core branch on framework name is permitted; capability/catalog dispatch selects contributors.',
    ],
    'subblocks': [
        sb('Profile contributor registration', 'Framework semantic contribution capabilities are immutable catalog entries selected by exact profile.', [
            'Register contributor, region kinds, component contract capabilities, and intrinsic diagnostic rules.',
            'Separate file discovery/indexing from contribution execution.',
            'Define Disabled/WorkspaceOnly/Full zero-work behavior with COX0.',
        ], [
            'Extend catalog descriptors and generated registration.',
            'Replace central framework switches in checker ingress.',
        ], [
            'Two profiles for the same file kind do not collide.',
            'Disabled and non-applicable profiles execute no contribution work.',
        ]),
        sb('Template executable-region lowering', 'Vue and Svelte template bodies/branches/handlers are represented as authored framework regions.', [
            'Lower lexical scopes, branches, loops, event handlers, slot/snippet bodies, and expression anchors into compact region descriptors.',
            'Reuse native flow/relation/call facts through declarative demands.',
            'Keep framework AST nodes and source maps in frontend ownership.',
        ], [
            'Implement region contribution builders for canary subsets.',
            'Add exact source/UTF encoding and profile provenance.',
        ], [
            'Template branch narrowing and handler call canaries match fresh/incremental behavior.',
            'No generated TSX text is read by native region execution.',
        ]),
        sb('Binding, contextual, and relation contributions', 'Framework scopes contribute typed facts/demands without mutating semantic nodes.', [
            'Contribute template bindings, contextual targets, narrowing facts, event payload expectations, directive effects, and relation demands.',
            'Validate canonical symbol/provenance and reject unresolved fake facts.',
            'Let the executor resolve declarative demands through the one semantic dispatch.',
        ], [
            'Implement contribution conversion and validation.',
            'Capture exact read sets and profile/source epochs.',
        ], [
            'Forged canonical symbol, stale source, or foreign-profile contribution is rejected.',
            'Equivalent TS and template semantic facts converge to the same relation/call outcomes.',
        ]),
        sb('Framework-neutral component contract', 'Component surfaces from Vue and Svelte lower into one typed contract used by checker, TypeInfo, and language-service operations.', [
            'Define inputs, outputs, slots/content, exposed instance, refs, directives/actions, models/bindings, and lifecycle values.',
            'Separate contract identity from framework presentation names.',
            'Reuse TIF1 component authority and avoid a duplicate component metadata store.',
        ], [
            'Implement adapter normalization into existing universal component contracts or amend them atomically.',
            'Migrate canary component checks to common relation rules.',
        ], [
            'Vue/Svelte equivalent contracts produce common query behavior while preserving framework provenance.',
            'A duplicate component authority guard rejects same-role stores.',
        ]),
        sb('Intrinsic framework diagnostic rules', 'Only genuinely framework-specific semantics register framework-owned rules.', [
            'Examples: invalid directive/action usage, slot/snippet contract shape, framework binding constraints, component registration rules.',
            'Common assignment/call/flow errors remain common NCF families.',
            'Framework rules declare exact contributed fact requirements and fix intents.',
        ], [
            'Implement a small Vue and Svelte canary set.',
            'Classify remaining legacy framework diagnostics in VIM/NCF manifests.',
        ], [
            'A rule requiring a framework-name branch in core fails architecture review.',
            'Canaries perform zero work on the other framework profile.',
        ]),
        sb('Atomic contribution snapshot and isolation proof', 'Updates, cancellation, and profile changes cannot leak stale framework facts or diagnostics.', [
            'Stage contribution batches and atomically swap only complete validated snapshots.',
            'Bind checker query reads to profile semantic epoch and exact source basis.',
            'Cancel superseded contribution/check work and refuse warm publication.',
        ], [
            'Implement project-scoped snapshot storage and teardown.',
            'Instrument contribution count, validation, reuse, and retained bytes.',
        ], [
            'Rapid edit/profile-switch tests publish no mixed-epoch diagnostics.',
            'Long churn plateaus memory and fresh equals incremental across both frameworks.',
        ]),
    ],
    'laws': [
        'A framework contribution is data plus provenance; it cannot own final TypeScript semantic truth.',
        'ComponentContract identity includes framework/profile and canonical component identity but remains queryable through common operations.',
        'Framework region coordinates are authored source coordinates with tagged encoding; generated coordinate identity is not accepted as primary.',
        'Only complete validated contribution snapshots admit and become visible to Check queries.',
        'Common and framework-owned diagnostics cannot share the same family/slice authority.',
    ],
    'migration': [
        'Start with Vue/Svelte canary regions and component contracts; broader vertical work belongs to generated NCF/VIM rows.',
        'Keep provider-generated TSX functioning during observation but prevent it from feeding native semantic facts.',
        'Delete old framework checker paths only after exact native/provider behavior and authored mapping are proven.',
    ],
    'deletions': [
        'Delete framework-specific native resolver/checker paths displaced by typed contributions.',
        'Delete duplicate framework component metadata authority after TIF1 contract migration.',
        'Delete generated-TSX-as-native-truth assumptions from live architecture.',
    ],
    'forbidden': [
        'Core `if framework == vue/svelte` branches.',
        'Adapters receiving raw ProjectSemanticDispatch or mutable semantic stores.',
        'Generated TypeScript/TSX text, regex, or source slicing as native semantic facts.',
        'Framework-specific copies of relation, call, flow, or diagnostic query stores.',
        'Cross-profile contribution or cache identity aliasing.',
    ],
    'acceptance': [
        '**NCK5-AC-NEUTRAL:** core checker modules contain no framework-name dispatch and contributors expose no resolver.',
        '**NCK5-AC-REGIONS:** Vue/Svelte canary template regions carry exact authored identity, scopes, and read sets.',
        '**NCK5-AC-CONTRACT:** common ComponentContract queries match vertical fixtures with provenance preserved.',
        '**NCK5-AC-ISOLATION:** profile changes, cancellation, and rapid edits publish no stale or mixed contributions.',
    ],
    'performance': [
        'One parse and one shallow framework pass per content hash; no checker-triggered rescan.',
        'Contribution work is demand-selected and zero for non-participating capabilities.',
    ],
    'verification': [
        '`cargo nextest run -p verter_language -p verter_semantic -p verter_session -p verter_vue_conformance -p verter_svelte_conformance`.',
        'Static no-framework-switch/no-resolver adapter guards.',
        'Vue/Svelte canary differential, profile-isolation, rapid-edit, and memory-plateau tests.',
    ],
    'consumers': [
        'Unlocks NCK6 native publication for framework semantic families.',
        'Provides framework-authored targets/facts consumed by LSO2/LSO8 and future NCF slices.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` framework-agnostic end-state sections.',
        '`docs/arch/multi-framework-adapters-plan.md` typed contribution and one-resolver invariants.',
        'TIF1, IDX0, VIM1, and architecture-proof vertical contracts.',
    ],
},
'NCK6': {
    'outcome': 'Implement family/slice-scoped external-versus-native authority arbitration, shadow observation, exact-basis deduplication, atomic LSP/CLI publication cutover, dynamic profile/provider transitions, and rollback to prior certified receipts. This block promotes only NCF slices with valid certification receipts.',
    'current_owner': 'provider-first LSP merge paths, command-local diagnostic composition, implicit duplicate suppression, and no family-scoped native promotion state',
    'final_owner': 'one NativeCheckerService and immutable authority table composing parser, native, framework, lint, provider, and project diagnostics by exact family/profile basis',
    'role': 'NCK6 turns implemented/certified checker slices into user-visible authority without duplicate or missing diagnostics. It is the only cutover owner; NCF implementation nodes cannot self-promote.',
    'surfaces': [
        '`crates/verter_session` for NativeCheckerService, authority snapshots, and composed diagnostic plans',
        '`crates/verter_lsp` for publication consumption only, replacing provider-specific arbitration',
        '`crates/verter_type_runtime` for provider observation tagged by exact provider/input basis',
        '`crates/verter_diagnostics` and `crates/verter_actions` for dedup/suppression/fix provenance',
        'CLI application services consumed by `verter typecheck` and watch mode',
    ],
    'apis': [
        '`DiagnosticAuthorityTable`, `DiagnosticAuthorityEntry`, and immutable authority epochs',
        '`NativeCheckerService`, `DiagnosticCheckPlan`, and `DiagnosticPublicationPlan`',
        '`ProviderObservationBatch` tagged by provider binding/input basis',
        '`DiagnosticDedupKey`, `AuthorityTransitionReceipt`, and `FamilyRollbackReceipt`',
        '`CheckerCapabilitySnapshot` for CLI/LSP/public capability truth',
    ],
    'predecessor_contracts': {
        'NCK4': 'consume exact family/slice manifest rows and immutable certification receipts.',
        'NCK5': 'consume validated framework contribution and intrinsic framework rule ingress.',
        'H2': 'consume project-scoped ProviderHub binding, provider epoch, deadlines, and applied receipts.',
        'H3': 'consume exact-basis foreground settlement and latest-wins background publication.',
        'CLI2': 'consume the application-service typecheck command contract; do not create a checker-specific CLI engine.',
        'COX0': 'consume per-profile editor participation and dynamic capability transitions.',
        'PUB0': 'consume typed public outcomes and honest capability reporting.',
    },
    'principles': [
        'Only NCK6 writes live diagnostic authority state. Implementation and certification nodes produce receipts but cannot publish.',
        'ObserveNative computes and compares native results without user publication and without suppressing the external owner.',
        'CertifiedNative transition atomically suppresses the external family/slice and enables native publication on one authority epoch.',
        'Deduplication uses stable diagnostic identity plus authority; it is a safety net, not a substitute for sole ownership.',
        'Parser, lint, framework-intrinsic, project/config, and provider/native semantic classes compose explicitly.',
        'Provider unavailability yields truthful outcomes for external-owned slices; it cannot silently relabel uncertified native work as complete.',
        'Rollback names a prior certified authority receipt and exact reason; no implicit fallback or mixed epoch.',
    ],
    'subblocks': [
        sb('Immutable authority table and transition validator', 'Every profile/family/slice has one current state and exact certification/provider dependencies.', [
            'Build authority snapshots from catalog, NCF receipts, provider binding, profile mode, and explicit maintainer policy.',
            'Validate legal transitions and rollback targets.',
            'Version authority epochs independently from source revisions.',
        ], [
            'Implement table construction and transition receipt validation.',
            'Expose read-only capability snapshots to consumers.',
        ], [
            'State-machine mutation matrix rejects duplicate/missing authority and stale receipts.',
            'Snapshot generation is deterministic from identical inputs.',
        ]),
        sb('Shadow observation and structured comparison', 'ObserveNative produces bounded non-publishing evidence without affecting interactive correctness.', [
            'Schedule observation under explicit budgets and lower priority.',
            'Compare canonical native/provider batches on exact equivalent basis.',
            'Record mismatches, non-comparable rows, cancellation, and provider failure separately.',
        ], [
            'Implement observation coordinator and counters.',
            'Never cache provider observations as native semantic results.',
        ], [
            'Observation disabled performs zero native comparison work.',
            'Cancellation/supersession produces no promotion evidence or publication.',
        ]),
        sb('Composed diagnostic plan', 'One service selects parser, native, framework, lint, provider, and project sources without duplicate authority.', [
            'Build a typed plan per project/profile/request basis.',
            'Select provider families only where External; native where CertifiedNative; both only in non-publishing ObserveNative evidence.',
            'Propagate completeness and NeedInputs across owners.',
        ], [
            'Implement NativeCheckerService planning and deterministic aggregation.',
            'Replace command/LSP-local source selection.',
        ], [
            'Matrix tests cover every authority state and diagnostic class combination.',
            'No source is queried when its families are not selected.',
        ]),
        sb('LSP publication cutover', 'LSP publishes latest exact-basis composed diagnostics with no provider-specific merge authority.', [
            'Use H3 publication epoch and stale rejection.',
            'Clear withdrawn families on capability/profile/authority transitions.',
            'Preserve authored anchors, related locations, codes, provenance, and fix refs.',
        ], [
            'Route LSP diagnostic publication through NativeCheckerService output.',
            'Delete old provider/native semantic suppression branches as their final family moves.',
        ], [
            'Rapid edit/provider/profile transition tests never publish mixed/stale batches.',
            'Planted old merge path is rejected by the sole-owner guard.',
        ]),
        sb('CLI and watch-mode cutover', '`verter typecheck` and watch consume the same composed service and exact authority state.', [
            'Return complete/NeedInputs/unsupported/cancelled outcomes and zero writes.',
            'Coordinate workspace files/project rules without a monolithic CheckProgram cache.',
            'Stream deterministic progress/results while preserving final aggregate basis.',
        ], [
            'Replace command-local diagnostic composition with application-service calls.',
            'Add machine-readable and human reporters outside semantic core.',
        ], [
            'CLI/LSP differential fixtures observe equivalent semantic diagnostics on the same profile/basis.',
            'Watch incremental result equals fresh and performs bounded changed-work only.',
        ]),
        sb('Promotion and rollback execution', 'A certified slice transitions atomically and can roll back only to a validated prior authority receipt.', [
            'Verify implementation, certification, performance, and review receipts at transition.',
            'Invalidate/clear displaced cached/public state and increment authority epoch.',
            'Record explicit rollback cause and prevent silent automatic semantic fallback.',
        ], [
            'Implement promotion/rollback commands in canonical governance tooling.',
            'Add audit events and state receipts.',
        ], [
            'Negative tests for stale candidate, stale oracle, missing performance proof, mixed profile, and invalid rollback.',
            'Promotion cannot occur through code/config changes alone without an authority receipt.',
        ]),
    ],
    'laws': [
        'Authority state is immutable per epoch and selected before diagnostic work starts.',
        'A batch cannot combine semantic diagnostics from different authority epochs as one complete result.',
        'Provider observation identity includes provider binding and exact generated/source mapping basis; native identity does not.',
        'Capability advertisement reports only families/surfaces that can produce complete results under current profile/policy.',
        'Family withdrawal clears prior publication and cancels pending work before the new epoch becomes current.',
    ],
    'migration': [
        'Begin with NCK3 canary families and certified generated slices only.',
        'Keep external ownership for all remaining families; no broad native-checker switch.',
        'Route LSP/CLI through the composed service before deleting old merge/arbitration code.',
        'Record every family migration and deletion in generated receipts.',
    ],
    'deletions': [
        'Delete provider-baked diagnostic arbitration and message-text dedup for migrated families.',
        'Delete command-local project/typecheck diagnostic composition.',
        'Delete broad native-checker enable flags that bypass family authority.',
        'Delete old semantic family publication routes after exact cutover.',
    ],
    'forbidden': [
        'Self-promotion by an NCF implementation node.',
        'Publishing ObserveNative results or using observation to fill provider gaps.',
        'Automatic silent rollback/fallback without an exact receipt.',
        'Provider and native same-family publication hidden by dedup.',
        'LSP- or CLI-specific semantic source selection logic.',
    ],
    'acceptance': [
        '**NCK6-AC-AUTHORITY:** every active profile/family/slice has one selected publisher and legal transition history.',
        '**NCK6-AC-SHADOW:** ObserveNative is non-publishing, bounded, cancellable, and basis-equivalent.',
        '**NCK6-AC-SURFACES:** CLI and LSP consume one composed service and agree on semantic results.',
        '**NCK6-AC-PROMOTION:** stale/missing receipts cannot promote or roll back authority.',
    ],
    'performance': [
        'CertifiedNative paths perform zero external provider diagnostic work for migrated families.',
        'External paths perform zero native rule work unless ObserveNative is explicitly enabled.',
        'Authority planning is O(selected families) using catalog indexes, not O(all workspace files).',
    ],
    'verification': [
        '`cargo nextest run -p verter_session -p verter_lsp -p verter_type_runtime -p verter_diagnostics` plus CLI application-service tests.',
        'Authority-state, promotion/rollback, rapid-edit, provider-swap, and profile-transition mutation matrices.',
        'CLI/LSP differential and watch incremental/fresh performance tests.',
    ],
    'consumers': [
        'Unlocks NCK7 terminal and optional NCK-aware LSO8 conformance.',
        'Makes certified native checker families available to CLI2, LSP, MCP/public consumers through PUB0.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` runtime-independent parity and sequencing.',
        'H2/H3 provider/publication contracts, CLI2 composed typecheck contract, and COX0 participation rules.',
    ],
},
'NCK7': {
    'outcome': 'Prove the native checker product terminal: all required manifest slices are implemented or explicitly product-excluded, every promoted family has immutable certification and performance evidence, displaced diagnostic authorities are deleted, cross-surface results converge, memory/latency remain bounded, and legacy checker architecture files are removed.',
    'current_owner': 'mixed certified and external diagnostic families, residual legacy routes, and pending checker manifest work',
    'final_owner': 'the completed family-scoped native checker product with explicit residual external ownership and no displaced authority',
    'role': 'NCK7 is a convergence and deletion gate, not a final feature implementation bucket. It accepts only bounded validation, exact residual classification, deletion, and product receipt work. Missing semantic families reopen generated NCF nodes rather than expanding NCK7.',
    'surfaces': [
        '`docs/arch/refactor/rev11/authority`, catalogs, generated manifests, receipts, and legacy disposition',
        '`crates/verter_session`, `crates/verter_semantic`, `crates/verter_diagnostics`, `crates/verter_lsp`, and CLI only for bounded final cutover/deletion',
        '`crates/verter_bench` and performance evidence for checker latency, work, allocation, and RSS',
        'repository-wide architecture guards for deleted diagnostic authority paths',
    ],
    'apis': [
        '`NativeCheckerProductReceipt` and exact required/residual family inventory',
        '`LegacyDiagnosticAuthorityDeletionManifest`',
        '`CheckerSurfaceEquivalenceReceipt` across CLI/LSP/public consumers',
        '`CheckerPerformanceReceipt` and long-churn memory evidence',
    ],
    'predecessor_contracts': {
        'NCK6': 'consume live family authority, publication cutover, and immutable transition receipts.',
        'PER0': 'consume cache/backend identity, cancellation, budget, zero-work, and performance contract lock.',
        'UAO0': 'consume activation/TypeInfo/index/performance convergence.',
        'UAP0': 'consume capability/diagnostic/action/public convergence.',
        'BR0': 'consume successor product promotion governance and repair-freeze authorization.',
    },
    'principles': [
        'Terminal completeness is manifest-derived. NCK7 cannot declare success by sampling or percentage.',
        'External residual families are allowed only when explicitly classified as product exclusions or future requirements with honest capability reporting.',
        'No semantic algorithm work is hidden in the terminal. Any missing rule/family opens or amends an NCF node.',
        'Every displaced route/store/guard/doc is deleted or explicitly retained with sole ownership and rationale.',
        'Cross-surface equivalence compares semantic diagnostic identity/basis, not editor formatting.',
        'Performance acceptance uses equivalent work, first/warm check latency, cancellation waste, allocations, and long-churn RSS.',
    ],
    'subblocks': [
        sb('Manifest completeness and residual classification', 'Every required family slice has an accepted implementation/certification/promotion receipt or an explicit product exclusion.', [
            'Compute completeness from the canonical manifest and authority table.',
            'Reject wildcard deferrals and unowned residual rows.',
            'Record future external-owned scope separately from completed native product claims.',
        ], [
            'Generate terminal completeness report and machine receipt.',
            'Open amendments for any missing independently acceptable work before proceeding.',
        ], [
            'Planted missing/duplicate/unpromoted required slice blocks terminal.',
            'Report is reproducible from authority inputs.',
        ]),
        sb('Displaced authority and store deletion', 'No migrated family has an old producer, cache, merge path, or fallback capable of publishing.', [
            'Sweep semantic, session, LSP, provider, framework, and command paths by registered family owners.',
            'Delete old stores and compatibility branches after final consumers move.',
            'Retain external provider machinery only for explicitly external families and other language-service capabilities.',
        ], [
            'Apply exact deletion manifest and negative guards.',
            'Remove stale docs/tests/config flags tied to deleted authority.',
        ], [
            'Planting any deleted route fails architecture tests.',
            'No migrated family produces provider diagnostic work in runtime counters.',
        ]),
        sb('Cross-surface semantic equivalence', 'CLI, LSP, MCP, NAPI/WASM/public surfaces observe equivalent native semantic diagnostics and truthful outcomes.', [
            'Compare diagnostic identity, basis, completeness, provenance, and related/fix refs.',
            'Allow presentation-specific formatting only after core equivalence.',
            'Verify unavailable inputs yield NeedInputs rather than empty success.',
        ], [
            'Generate surface matrix fixtures and receipts.',
            'Fix only bounded adapter discrepancies; semantic gaps reopen NCF work.',
        ], [
            'Differential matrix passes for all available surfaces/profiles.',
            'A surface-specific semantic DTO or dropped provenance blocks terminal.',
        ]),
        sb('Performance, cancellation, and memory terminal', 'The checker is production-bounded under cold, warm, incremental, churn, cancellation, and parallel load.', [
            'Measure equivalent fact/rule/query work, allocations, retained bytes, latency distributions, and provider avoidance.',
            'Test repeated edits, project open/close, profile transitions, and cancelled workspace checks.',
            'Require no unbounded result/proof/contribution retention.',
        ], [
            'Capture checker performance receipt under PER0 methodology.',
            'Reopen the owning implementation node for unexplained regressions; do not micro-optimize blindly in NCK7.',
        ], [
            'Long-churn memory plateaus and project teardown releases storage.',
            'Warm certified families perform zero provider diagnostic work.',
        ]),
        sb('Legacy architecture reconciliation and deletion', 'All durable legacy checker/type-parity clauses are in Rev11 authority and obsolete files are removed.', [
            'Validate exact blob-SHA disposition for every legacy path.',
            'Ensure no live authority references deleted files.',
            'Keep product/user docs outside `docs/arch` where appropriate.',
        ], [
            'Delete classified legacy files in the same accepted amendment.',
            'Enable permanent guard forbidding new docs/arch files outside Rev11.',
        ], [
            'Repository tree contains no unclassified live legacy architecture.',
            'Source-atom coverage remains complete after deletion.',
        ]),
        sb('Native checker product receipt and promotion', 'The product is promoted with exact scope, residuals, evidence, and no hidden claim of full TypeScript replacement beyond certified families.', [
            'Bind manifest digest, authority snapshot, surface/performance/deletion receipts, and review verdicts.',
            'State remaining external families and runtime provider uses honestly.',
            'Separate checker completion from full language-service/provider retirement.',
        ], [
            'Emit immutable product receipt and update capability/maturity matrices.',
            'Do not delete TypeScript provider capabilities still owned by LSO/EPR or external residual families.',
        ], [
            'Receipt invalidates on any authority/source/evidence change.',
            'Public capability claims match the exact certified scope.',
        ]),
    ],
    'laws': [
        'NCK7 may not add a new diagnostic algorithm, rule family, or semantic fact authority.',
        'Residual external ownership is explicit and capability-visible; it is not a failure if product scope says so.',
        'A product receipt names exact manifest and authority epochs and is immutable.',
        'Deleting provider diagnostic paths does not imply deleting provider completion/navigation capabilities.',
    ],
    'migration': [
        'Run terminal only after required NCF nodes and NCK6 promotions are accepted.',
        'Perform bounded cleanup/deletion in one landing-frozen candidate with complete negative guards.',
        'If final sweeps discover semantic gaps, stop and open owning NCF/NCK amendments.',
    ],
    'deletions': [
        'Delete all displaced checker diagnostic producers, stores, merge paths, flags, and legacy docs named in the terminal manifest.',
        'Delete stale parity ledgers and ignored-test mechanisms replaced by NCK4 authority.',
        'Delete any claim that NCK7 retires the entire TypeScript engine unless separate LSO/EPR/provider retirement authority exists.',
    ],
    'forbidden': [
        'Adding missing semantic features in the terminal block.',
        'Treating sampled parity, green coverage, or message counts as full certification.',
        'Deleting provider capabilities still owned outside diagnostic families.',
        'Accepting unexplained performance or memory regressions as cleanup noise.',
    ],
    'acceptance': [
        '**NCK7-AC-MANIFEST:** all required slices have exact accepted implementation, certification, and promotion receipts.',
        '**NCK7-AC-DELETION:** every displaced diagnostic route/store/doc is absent and structurally rejected.',
        '**NCK7-AC-SURFACES:** semantic diagnostic results and outcomes are equivalent across supported public surfaces.',
        '**NCK7-AC-TERMINAL-PERF:** cold/warm/incremental/cancel/churn work, allocation, latency, and RSS satisfy PER0 evidence.',
        '**NCK7-AC-HONESTY:** residual external ownership and non-checker provider uses are explicitly documented.',
    ],
    'performance': [
        'Terminal performance thresholds must be replacement/equivalent-work thresholds ratified by PER0, not arbitrary zero-regression assertions when capability work differs.',
    ],
    'verification': [
        'Full native checker manifest/authority/source/deletion validation.',
        'Canonical cross-surface, provider-avoidance, incremental/fresh, cancellation, and long-churn test matrix.',
        'Configured architecture-3 review and product promotion receipt validation on the exact candidate.',
    ],
    'consumers': [
        'Promotes the native checker product for certified families.',
        'Provides a stable diagnostic service for CLI, language-service conformance, lint/fix composition, and future framework verticals.',
        'Does not by itself unlock full TypeScript engine retirement.',
    ],
    'sources': [
        'All NCK/NCF authority and `legacy-arch-disposition.toml` entries targeting native checker/type-parity docs.',
        'PER0, PUB0, UAO0, UAP0, and BR0 terminal contracts.',
    ],
},
}

# Base specification library for generate_native_checker_v3.py; do not execute directly.
