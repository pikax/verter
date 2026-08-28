from __future__ import annotations

from pathlib import Path
from copy import deepcopy

from chartergen import write_train

ROOT = Path(__file__).resolve().parent
source = (ROOT / 'generate_native_checker.py').read_text(encoding='utf-8')
source = source.rsplit("write_train('expansion-native-checker.toml'", 1)[0]
namespace: dict[str, object] = {}
exec(compile(source, str(ROOT / 'generate_native_checker.py'), 'exec'), namespace)
specs = namespace['specs']
old_terminal_spec = deepcopy(specs['NCK7'])
sb = namespace['sb']

# Tighten the constitution to the revised successor boundary.
specs['NCK0']['predecessor_contracts'].update({
    'TCM3': 'consume the certified TypeScript semantic capability and observation identity contract; external TypeScript is oracle/fallback authority, never native query-time computation.',
    'TIF1': 'consume the TypeInfo-first public semantic contract and component metadata cutover.',
    'LRA0': 'consume diagnostic, rule, suppression, action, and authored-fix ownership boundaries.',
    'PUB0': 'consume the versioned public result/outcome vocabulary and truthful capability law.',
})
specs['NCK0']['migration'] = [x.replace('NCK0-NCK7', 'NCK0-NCK8') for x in specs['NCK0']['migration']]
specs['NCK0']['deletions'] = [x.replace('NCK0-NCK7', 'NCK0-NCK8') for x in specs['NCK0']['deletions']]

specs['NCK3']['predecessor_contracts'].pop('TCM4', None)
specs['NCK4']['predecessor_contracts']['PER0'] = 'consume equivalent-work, allocation, latency, and retained-memory evidence methodology for certification and generated slices.'

specs['NCK6'] = {
    'outcome': 'Implement the sole family-scoped diagnostic authority registry and atomic publication decision layer: exact External/ObserveNative/CertifiedNative/Disabled state, non-publishing shadow comparison, deterministic deduplication, provider/native epoch coordination, and rollback to an explicit prior certified receipt. This block does not integrate individual consumer surfaces.',
    'current_owner': 'provider-specific LSP merge branches, ad hoc suppression rules, global provider-enabled flags, and diagnostic message-text deduplication',
    'final_owner': 'one immutable DiagnosticAuthoritySnapshot and one atomic diagnostic publication decision for every project profile, family, and semantic feature slice',
    'role': 'NCK6 is the authority cutover block. It prevents a green native implementation from becoming user-visible before certification and prevents external and native producers from publishing the same semantic family. It deliberately stops before LSP/CLI/MCP/NAPI/WASM adapters, which are owned by NCK7.',
    'surfaces': [
        '`crates/verter_diagnostics` for authority registry, comparison, deduplication, and publication plans',
        '`crates/verter_session` for project-scoped immutable authority snapshots and exact basis selection',
        '`crates/verter_type_runtime` for external observation inputs and provider epoch identity only',
        '`crates/verter_lsp` publication coordinator only at the shared publication-plan seam, not feature adapters',
        '`crates/verter_protocol` for authority/certification status exposed under PUB0',
    ],
    'apis': [
        '`DiagnosticAuthorityKey { project_profile, family, feature_slice }`',
        '`DiagnosticAuthorityState::{External, ObserveNative, CertifiedNative, Disabled}`',
        '`DiagnosticAuthoritySnapshot`, `DiagnosticAuthorityEpoch`, and immutable transition receipts',
        '`DiagnosticObservationBatch`, `DiagnosticComparisonResult`, and typed mismatch classes',
        '`DiagnosticPublicationPlan` and `DiagnosticDedupKey`',
        '`DiagnosticPromotionRequest`, `DiagnosticPromotionReceipt`, and `DiagnosticRollbackReceipt`',
    ],
    'predecessor_contracts': {
        'NCK4': 'consume generated family manifests, exact certification receipts, and canonical oracle comparison.',
        'NCK5': 'consume validated framework contribution/profile isolation so authority keys never alias across profiles.',
        'H2': 'consume project-scoped provider bindings and exact provider epochs.',
        'H3': 'consume latest-basis stale-safe publication and supersession behavior.',
        'COX0': 'consume per-profile capability participation and dynamic withdrawal.',
        'PUB0': 'consume typed public outcomes, capability truth, and schema epochs.',
    },
    'principles': [
        'Authority is keyed by exact project profile, diagnostic family, and semantic feature slice; one global checker/provider boolean is forbidden.',
        'ObserveNative computes and compares native output but never contributes user-visible diagnostics, fixes, actions, counts, or success status.',
        'CertifiedNative becomes visible only in the same atomic state transition that suppresses external publication for the exact key.',
        'Deduplication is by semantic identity and authority, never normalized message text or approximate source range.',
        'Provider epoch, native implementation receipt, certification receipt, configuration epoch, and authored basis are all explicit transition inputs.',
        'Rollback names a prior accepted authority snapshot; implicit fallback to whichever provider is available is forbidden.',
        'A mixed-epoch, stale, cancelled, partial, or NeedInputs producer cannot publish as complete.',
    ],
    'subblocks': [
        sb('Immutable authority registry and transition validator', 'Every diagnostic authority key has one exact state and only legal receipt-backed transitions are admitted.', [
            'Implement immutable project-scoped authority snapshots with structural keys.',
            'Define legal transitions and required receipts for External to ObserveNative to CertifiedNative, disablement, and rollback.',
            'Make configuration/profile changes produce a new authority epoch rather than mutate state in place.',
        ], [
            'Replace global provider/native booleans and scattered suppression flags at the authority seam.',
            'Generate transition tables and static guards from NCK0 authority catalogs.',
        ], [
            'Illegal transitions, missing receipts, cross-profile reuse, and stale snapshot publication fail closed.',
            'Incremental reconstruction byte-equals a fresh snapshot for the same inputs.',
        ]),
        sb('Non-publishing shadow observation', 'ObserveNative produces structured comparison evidence without changing user-visible behavior.', [
            'Run native and external owners on the same exact input basis and canonicalize their diagnostic identities.',
            'Classify missing, extra, wrong-code, wrong-anchor, wrong-related-location, wrong-fix-intent, and completeness mismatches.',
            'Keep observation results bounded and non-admitted to ordinary diagnostic publication caches.',
        ], [
            'Add an observation scheduler lane with cancellation and budgets.',
            'Persist only bounded certification evidence or aggregate counters explicitly required by NCK4.',
        ], [
            'Observation on/off produces byte-identical user-visible diagnostics and actions.',
            'A planted native mismatch is detected while the external result remains the sole published result.',
        ]),
        sb('Semantic deduplication and composed publication plan', 'The publication plan contains exactly one authoritative diagnostic per semantic identity and preserves distinct legitimate diagnostics.', [
            'Construct semantic dedup keys from origin/family/rule/subject/authored anchor/profile/basis.',
            'Compose parser, semantic, framework, lint, project/configuration, and external classes under their own authority rules.',
            'Preserve separately owned diagnostics even when wording and ranges coincide.',
        ], [
            'Move deduplication out of consumer-specific merge code into the shared diagnostic authority layer.',
            'Emit a deterministic publication plan with provenance and completeness.',
        ], [
            'Message wording mutations do not change dedup identity.',
            'Two different rules at the same anchor survive; duplicate authorities for one key fail.',
        ]),
        sb('Provider/native epoch coordination', 'A publication plan never combines provider and native results from incompatible bases or epochs.', [
            'Join exact source revision, project profile, provider epoch, native authority epoch, and configuration epoch.',
            'Cancel or discard superseded comparison/publication work on any epoch transition.',
            'Require exact latest-basis settlement from H3 before publication.',
        ], [
            'Thread authority snapshot IDs through shared diagnostic production and publication receipts.',
            'Remove best-effort merge behavior that accepts whichever batch arrives first.',
        ], [
            'Race tests with provider restart, edit, config change, and promotion publish only the newest coherent basis.',
            'No mixed-epoch batch can serialize as complete.',
        ]),
        sb('Promotion and rollback execution', 'Promotion and rollback are atomic, auditable, and leave neither duplicate nor missing authority.', [
            'Validate certification, implementation, profile, provider, and source receipts immediately before transition.',
            'Publish the new authority snapshot and invalidate displaced result routes atomically.',
            'Rollback only to an explicitly named accepted snapshot with compatible inputs.',
        ], [
            'Implement transition receipts and negative guards against implicit fallback.',
            'Expose truthful capability/maturity status through PUB0/COX0.',
        ], [
            'Crash/failure injection at every transition point results in either old or new complete authority, never half-transition.',
            'Promotion immediately drives external diagnostic work for the certified key to zero.',
        ]),
        sb('Authority observability and bounded counters', 'Operators and tests can prove which authority ran and how much equivalent work it performed without leaking provider internals into semantic APIs.', [
            'Count native/external requests by family/slice/state, comparisons, discarded stale batches, promotions, rollbacks, and dedup decisions.',
            'Keep counters keyed by stable IDs and bounded cardinality.',
            'Separate certification/test telemetry from production result identity.',
        ], [
            'Add audit events and PER0-compatible work counters.',
            'Remove consumer-local diagnostic count heuristics used as authority evidence.',
        ], [
            'Certified warm requests show zero provider diagnostic work for that key.',
            'Counter reset/restart does not affect semantic or publication identity.',
        ]),
    ],
    'laws': [
        'Authority snapshots are immutable and project/profile scoped; no process-global mutable map is semantic truth.',
        'Observation results never enter public caches or consumer responses.',
        'Promotion invalidates displaced producer routes by exact key, not by broad provider shutdown.',
        'Publication ordering is deterministic after authority selection and semantic deduplication.',
        'Uncertified families remain externally owned and are reported honestly.',
    ],
    'migration': [
        'Introduce the registry in External state for every existing family, proving behavior identity before observation.',
        'Enable ObserveNative only for accepted NCF slices and compare without publication.',
        'Promote one canary slice, validate zero duplicates/gaps, then expand only through accepted receipts.',
        'Leave consumer adapters on the shared publication plan seam for NCK7 migration.',
    ],
    'deletions': [
        'Delete global checker/provider diagnostic booleans displaced by the exact authority registry.',
        'Delete message-text and approximate-range deduplication used as an authority substitute.',
        'Delete provider/native first-arrival merge arbitration for migrated diagnostic classes.',
    ],
    'forbidden': [
        'Publishing ObserveNative results or fixes.',
        'Promoting an entire provider/project when only bounded families are certified.',
        'Implicit rollback to any available provider or stale authority snapshot.',
        'Consumer-specific authority decisions after the shared publication plan exists.',
        'Counting diagnostic equality as certification without identity/provenance/completeness comparison.',
    ],
    'acceptance': [
        '**NCK6-AC-STATE:** exhaustive state-machine mutations reject illegal, stale, cross-profile, and receipt-less transitions.',
        '**NCK6-AC-SHADOW:** observation is user-invisible and detects planted semantic mismatches.',
        '**NCK6-AC-ATOMIC:** failure injection proves old-or-new atomic authority with no duplicate or missing publication.',
        '**NCK6-AC-ZERO-PROVIDER:** certified warm slices perform zero external diagnostic work.',
        '**NCK6-AC-DEDUP:** semantic dedup preserves distinct owners and removes only exact duplicate authority.',
    ],
    'performance': [
        'External-only state adds no native semantic work; Disabled adds no producer work; ObserveNative cost is explicit and budgeted.',
        'Authority lookup is allocation-free after snapshot construction and does not scan all families for a leaf request.',
    ],
    'abort': [
        'Abort if a producer cannot name exact family/slice/profile/basis identity.',
        'Abort if promotion cannot atomically suppress the displaced authority.',
    ],
    'verification': [
        'Authority-state, epoch-race, observation-invisibility, semantic-dedup, promotion/rollback, and zero-provider-work suites.',
        'Provider restart and concurrent edit failure injection under H2/H3 publication semantics.',
        'Architecture guard proving consumer adapters cannot independently choose diagnostic authority.',
    ],
    'consumers': [
        'Unlocks NCK7 shared consumer integration.',
        'Supplies the exact diagnostic authority snapshot consumed by language-service conformance when NCK is opened.',
        'Provides truthful family maturity to COX0 and PUB0.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` authority/cutover clauses.',
        '`docs/arch/provider-*` and diagnostic merge designs containing provider/native arbitration behavior.',
    ],
}

specs['NCK7'] = {
    'outcome': 'Expose one shared DiagnosticService across LSP, CLI, MCP, NAPI, WASM, and library consumers. Consumers receive authored-coordinate, provenance-complete diagnostic batches from NCK6 and apply only presentation policy; they cannot call semantic/provider engines directly or re-arbitrate authority.',
    'current_owner': 'consumer-local diagnostic DTOs, LSP-specific provider merge code, command-local typecheck composition, and inconsistent mapping/drop behavior',
    'final_owner': 'one shared DiagnosticService request/result contract with thin surface adapters and one authored-coordinate projection law',
    'role': 'NCK7 completes product integration without mixing it into authority arbitration. It makes diagnostic semantics and completeness identical across consumers while allowing each surface to format, stream, or serialize the same authoritative batch appropriately.',
    'surfaces': [
        '`crates/verter_diagnostics` and `crates/verter_session` for the shared service and project snapshot access',
        '`crates/verter_protocol` for versioned public requests/results and stable IDs',
        '`crates/verter_lsp` for diagnostics publication and code-action references',
        '`crates/verter_mcp_server`, `crates/verter_napi`, `crates/verter_wasm`, and FFI/public packages for thin adapters',
        '`packages/binary-launcher`, `packages/verter-lsp`, and CLI application services when their conditional predecessors are opened',
    ],
    'apis': [
        '`DiagnosticRequest { scope, profile, demand, basis, cancellation, budget }`',
        '`DiagnosticService::check_region`, `check_file`, and `check_project_rules`',
        '`DiagnosticBatch { basis, completeness, diagnostics, authority_snapshot }`',
        '`AuthoredDiagnostic`, `AuthoredRelatedLocation`, `DiagnosticProofRef`, and `DiagnosticFixIntentRef`',
        '`DiagnosticSurfaceAdapter` as serialization/presentation only, not semantic extension',
        '`DiagnosticStreamCursor` for bounded project/watch enumeration where supported',
    ],
    'predecessor_contracts': {
        'NCK6': 'consume the exact authority snapshot, publication plan, semantic deduplication, and promotion law.',
        'PUB0': 'consume common public request/result outcomes, schema epochs, cancellation, budgets, and capability truth.',
        'CLI2': 'when opened, integrate the Verter-native typecheck command as a thin DiagnosticService consumer.',
        'CLI4': 'when opened, integrate CLI LSP/MCP adapters without command-local diagnostic semantics.',
    },
    'principles': [
        'Core diagnostic batches are authored-coordinate results; generated/provider coordinates do not cross the service boundary.',
        'Every surface observes the same semantic diagnostics, authority state, basis, completeness, related locations, proof refs, and fix-intent refs.',
        'Presentation fields such as LSP severity tags, terminal colors, JSON layout, progress UI, and streaming framing are adapter policy.',
        'A surface cannot convert NeedInputs, unsupported, cancelled, stale, or partial into empty complete success.',
        'Fixes remain typed intents/references until LSO8/LRA0 validates an authored edit transaction.',
        'Project checks are bounded coordinators with streaming/pagination; a consumer cannot request hidden unbounded workspace work.',
        'Provider calls and semantic queries occur inside the shared service/authority layer only.',
    ],
    'subblocks': [
        sb('Shared service request and scope contract', 'All consumers request the same region/file/project-rule diagnostic operations with exact basis, demand, cancellation, and budgets.', [
            'Define scope selectors without LSP URI or CLI presentation fields.',
            'Require exact project/profile/source basis and capability availability.',
            'Model project-rule enumeration as bounded pages/streams with explicit completeness.',
        ], [
            'Add the shared service facade over NCK6 publication plans and NCK2 queries.',
            'Replace consumer-local project loading and diagnostic plan selection.',
        ], [
            'Equivalent requests from two surfaces produce the same core request identity.',
            'Unbounded or ambiguous project selection is rejected rather than silently choosing the first project.',
        ]),
        sb('Authored-coordinate diagnostic projection', 'Every returned primary and related location is mapped to exact authored source or refused with typed provenance loss.', [
            'Use UAI0/TCM authored mapping and source lineage for native, framework, and external diagnostics.',
            'Preserve source unit, profile, revision, mapping chain, and anchor confidence.',
            'Drop or return a typed incomplete result for unmappable provider artifacts; never synthesize 0:0 or nearest ranges.',
        ], [
            'Centralize diagnostic range projection before consumer adapters.',
            'Delete LSP-only range fallbacks and duplicated carrier mapping branches.',
        ], [
            'UTF-8/UTF-16/CRLF/emoji/embedded carrier cases round-trip exact authored spans.',
            'Stale mapper/source revisions are rejected and cannot publish.',
        ]),
        sb('LSP diagnostics and code-action reference adapter', 'LSP publication consumes shared authored batches and exposes exact code-action references without rechecking or remapping semantics.', [
            'Translate authored spans through negotiated position encoding only at the LSP edge.',
            'Publish latest-basis batches under H3 and clear only capabilities withdrawn by COX0.',
            'Resolve fix-intent references through LRA0/LSO8 rather than embedding unchecked workspace edits.',
        ], [
            'Route foreground/background diagnostic publication through one adapter.',
            'Delete provider/native merge and authority selection from LSP code.',
        ], [
            'Foreground and background paths publish identical core diagnostic identities.',
            'Dynamic capability withdrawal cancels work and clears only owned diagnostics.',
        ]),
        sb('CLI, MCP, NAPI, WASM, and library adapters', 'Non-LSP surfaces preserve core semantics and report unavailable inputs/capabilities truthfully.', [
            'Define stable JSON/protobuf/FFI projections from PUB0 without surface-specific semantic DTOs.',
            'CLI typecheck writes nothing and uses explicit project/reference/watch selection.',
            'WASM/MCP report NeedInputs when filesystem/provider/project services are unavailable.',
        ], [
            'Replace command-local or binding-local diagnostic composition.',
            'Generate bindings and compatibility tests from the public schema.',
        ], [
            'Cross-surface differential fixtures match diagnostic identity, basis, completeness, provenance, related/fix refs.',
            'A missing input never becomes empty success.',
        ]),
        sb('Watch, cancellation, streaming, and supersession', 'Long-running and watch consumers receive deterministic latest-basis batches without stale cache admission or retained-work growth.', [
            'Use cancellation/deadline/budget tokens through region/file/project coordinators.',
            'Supersede in-flight work on source/profile/authority/provider epoch changes.',
            'Bound stream cursors and release snapshots after completion/cancellation.',
        ], [
            'Unify watch and one-shot paths over the same service.',
            'Remove polling/sleep readiness and consumer-owned debounce semantics from diagnostic correctness.',
        ], [
            'Rapid edit/revert/provider restart tests publish only the latest basis.',
            'Cancelled project streams release retained regions/results and admit nothing partial.',
        ]),
        sb('Consumer route inventory and migration proof', 'Every public diagnostic consumer is known, migrated, and structurally prevented from bypassing the shared service.', [
            'Generate a call-site inventory for direct provider diagnostics, native checker calls, and legacy DTO construction.',
            'Migrate one surface at a time behind behavior characterization, then delete bypasses.',
            'Keep optional conditional consumers zero-work and unclaimed when unopened.',
        ], [
            'Add static architecture guards and generated consumer matrix.',
            'Record exact deletions and residual unsupported surfaces.',
        ], [
            'Planting a direct provider/checker call in a consumer crate fails the guard.',
            'The inventory reaches zero unexplained bypasses before NCK8.',
        ]),
    ],
    'laws': [
        'Core result identity is independent of surface encoding and presentation.',
        'Authored span projection validates the exact source/mapping basis used to obtain the range.',
        'Consumers may filter only explicitly policy-filterable classes under a named capability/configuration rule; they cannot suppress semantic families silently.',
        'Project stream cursors are scoped to an immutable basis and become stale on any authority/source/profile change.',
        'No consumer adapter owns semantic caching; it may cache serialization only by full core result identity.',
    ],
    'migration': [
        'Characterize each consumer surface against existing behavior and identify intentional corrections.',
        'Introduce the shared service with LSP as first consumer, then CLI/MCP/NAPI/WASM/library surfaces.',
        'Delete direct provider/native merge paths immediately after the last consumer moves.',
        'Keep unopened conditional CLI predecessors outside acceptance and prove zero hidden integration work.',
    ],
    'deletions': [
        'Delete consumer-local diagnostic authority arbitration, semantic deduplication, and provider/native merge logic.',
        'Delete Range::default/0:0/nearest-position diagnostic fallbacks and surface-specific semantic DTOs.',
        'Delete command-local project/checker construction displaced by shared application/service integration.',
    ],
    'forbidden': [
        'A surface adapter calling tsgo/tsserver or native Check queries directly.',
        'LSP URI/Position, terminal formatting, or provider handles in core diagnostic results.',
        'Embedding raw text edits in diagnostics instead of typed fix-intent references.',
        'Converting unavailable/partial/stale results to empty success.',
        'Hidden full-workspace checks on file-open, hover, completion, or unrelated leaf operations.',
    ],
    'acceptance': [
        '**NCK7-AC-SURFACES:** all opened consumers match core diagnostic identity, basis, completeness, provenance, related locations, and fix refs.',
        '**NCK7-AC-AUTHORED:** no public diagnostic leaves the service with generated coordinates or unvalidated mapping basis.',
        '**NCK7-AC-NO-BYPASS:** static inventory proves consumer crates cannot call diagnostic providers/resolvers directly.',
        '**NCK7-AC-WATCH:** watch/stream cancellation and supersession publish only latest complete batches and release retained state.',
        '**NCK7-AC-NEEDINPUTS:** unavailable surfaces return typed NeedInputs/unsupported, never empty complete success.',
    ],
    'performance': [
        'Thin adapters add no parse/resolve/provider/checker work and perform bounded serialization/allocation proportional to returned diagnostics.',
        'Repeated serialization may cache only by full result/basis/schema identity and must plateau in retained bytes.',
    ],
    'abort': [
        'Abort if any consumer requires a surface-specific semantic result not representable under PUB0; amend PUB0 rather than forking.',
        'Abort if exact authored projection is unavailable and a fallback location is proposed.',
    ],
    'verification': [
        'Cross-surface differential matrix, authored mapping/encoding tests, watch/cancel/supersession tests, and no-bypass architecture guard.',
        'LSP foreground/background equivalence and dynamic capability withdrawal tests.',
        'CLI/MCP/NAPI/WASM NeedInputs and schema-compatibility tests for every opened consumer.',
    ],
    'consumers': [
        'Unlocks NCK8 terminal closure.',
        'Provides the checker diagnostic service consumed conditionally by LSO9 and future verticals.',
        'Supports CLI typecheck without claiming full TypeScript engine retirement.',
    ],
    'sources': [
        '`docs/arch/native-checker.md` public query/result clauses.',
        '`docs/arch/ide-error-recovery-design.md` diagnostic publication and strict mapping clauses.',
        'Legacy LSP/provider diagnostic merge and CLI typecheck plans classified by the reconciliation catalog.',
    ],
}

# Reuse the old terminal specification as NCK8, recursively updating IDs.
def _terminal_replace(value):
    if isinstance(value, str):
        return value.replace('NCK0-NCK7', 'NCK0-NCK8').replace('NCK7', 'NCK8')
    if isinstance(value, list):
        return [_terminal_replace(v) for v in value]
    if isinstance(value, dict):
        return {k: _terminal_replace(v) for k, v in value.items()}
    return value

specs['NCK8'] = _terminal_replace(deepcopy(old_terminal_spec))
specs['NCK8']['outcome'] = 'Close the native checker product only after the required generated diagnostic slices, framework ingress, authority promotions, shared consumer integrations, performance/cancellation/memory proofs, and legacy authority deletion are complete on one exact terminal basis. This block adds no new diagnostic semantics.'
specs['NCK8']['current_owner'] = 'accepted NCK/NCF nodes plus residual displaced diagnostic routes, stores, tests, flags, and legacy architecture documents'
specs['NCK8']['final_owner'] = 'the promoted native checker product receipt, exact certified-family authority snapshot, and structurally enforced absence of displaced diagnostic authority'
specs['NCK8']['role'] = 'NCK8 is a proof, deletion, and promotion terminal. Any missing diagnostic algorithm, unsupported required family, semantic mismatch, or public-contract gap reopens its owning NCF/NCK predecessor; terminal cleanup may not patch semantics locally.'
specs['NCK8']['predecessor_contracts'] = {
    'NCK7': 'consume the shared consumer service and zero-bypass surface integration.',
    'NCKF0': 'consume the machine-generated required-family convergence receipt, exact manifest/predecessor bijection, current certification/promotion chains, provider-zero-work, and per-slice performance/admission closure.',
    'PER0': 'consume equivalent-work, latency, allocation, cancellation, and RSS terminal methodology.',
    'UAO0': 'consume activation, TypeInfo, index, and performance contract lock.',
    'UAP0': 'consume capability, coexistence, diagnostic/action, and public contract lock.',
    'BR0': 'consume successor product promotion authority and exact release law.',
}

write_train('expansion-native-checker.toml', specs, 'charters/expansion-native-checker')
print('wrote native checker charters:', len(specs))
