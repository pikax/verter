from __future__ import annotations

from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
PIN_COMMIT = "5a8ca4a391ea6d748f2891f1e39de6aaeec7987e"
PIN_TREE = "7ab8e3317fb8c8a15b7d63c6114a76f79f79f46d"

FAMILIES: list[dict[str, Any]] = [
    dict(id='NCF-BD-SCOPE', family='binding-and-declarations', name='Lexical scope, shadowing, and name binding diagnostics',
         scope='lexical/module/function/class/block/catch/parameter scope construction, shadowing, unresolved names, temporal visibility, and namespace selection',
         facts=['canonical symbol/declaration identity', 'scope and binding tables', 'source-order/region identity', 'module/global environment facts'],
         examples=['unresolved identifier', 'illegal shadowing/capture', 'wrong value/type namespace', 'scope leakage across regions'], preds=['NCK6','IDX0']),
    dict(id='NCF-BD-DUP', family='binding-and-declarations', name='Duplicate and conflicting declaration diagnostics',
         scope='duplicate declarations, conflicting symbol kinds, incompatible merged declarations, duplicate parameters/members, and declaration-order conflicts',
         facts=['merged declaration authority', 'symbol namespace/kind facts', 'relation proofs for compatibility', 'module/global augmentation facts'],
         examples=['duplicate block binding', 'conflicting interface/property merge', 'duplicate private name', 'illegal value/type merge'], preds=['NCK6','IDX0','D8']),
    dict(id='NCF-BD-INIT', family='binding-and-declarations', name='Use-before-declaration and initialization diagnostics',
         scope='temporal dead zones, use before assignment, initialization ordering, parameter/property initializer visibility, and module initialization hazards',
         facts=['executable-region graph', 'declaration/initialization facts', 'flow reachability and definite-assignment facts', 'module dependency order'],
         examples=['block-scoped use before declaration', 'property used before initialization', 'parameter initializer forward reference', 'module cycle initialization hazard'], preds=['NCK6','D8','IDX0']),
    dict(id='NCF-RO-ASSIGN', family='relations-and-operators', name='Assignment and return assignability diagnostics',
         scope='assignment, variable initialization, return/yield/awaited return, destructuring, argument/result relation sites, and satisfies assertions',
         facts=['Relate outcomes and proofs', 'contextual target types', 'FlowReturn/awaited/generator facts', 'source semantic subjects'],
         examples=['incompatible initializer', 'wrong return type', 'invalid destructuring target', 'failed satisfies relation'], preds=['NCK6','D8']),
    dict(id='NCF-RO-OPER', family='relations-and-operators', name='Operator and property/index access diagnostics',
         scope='unary/binary/logical/comparison/operator applicability, property access, element access, optional chaining, delete/in/instanceof/typeof semantics',
         facts=['resolved operand types', 'operator applicability relation proofs', 'object/member/index signatures', 'flow narrowing and optionality facts'],
         examples=['operator on incompatible operands', 'missing property', 'invalid index type', 'possibly null/undefined access'], preds=['NCK6','D8']),
    dict(id='NCF-RO-EXCESS', family='relations-and-operators', name='Freshness, excess-property, and object conformance diagnostics',
         scope='fresh object literal checks, excess properties, weak targets, exactness/freshness, spread interactions, discriminated object conformance, and contextual object sites',
         facts=['object projection facts', 'freshness/contextual typing state', 'Relate proofs', 'spread/correlation evidence'],
         examples=['unknown object literal property', 'weak target no common properties', 'spread-induced excess behavior', 'discriminant mismatch'], preds=['NCK6','D8']),
    dict(id='NCF-CO-CALL', family='calls-overloads-inference', name='Call, construct, tag, and invocation applicability diagnostics',
         scope='function/method/constructor/tagged-template/new/call signatures, arity, optional/rest parameters, this argument, and callable/constructable checks',
         facts=['ResolveCall result', 'signature/this/parameter facts', 'argument contextual types', 'relation proofs'],
         examples=['not callable/constructable', 'wrong arity', 'argument mismatch', 'invalid this context'], preds=['NCK6','D8']),
    dict(id='NCF-CO-OVER', family='calls-overloads-inference', name='Overload resolution and implementation conformance diagnostics',
         scope='overload candidate applicability, ambiguity, best-signature selection, implementation signature compatibility, and overload declaration ordering',
         facts=['ResolveOverloadSet/ResolveCall proofs', 'signature identity/effects', 'relation and inference sessions', 'declaration merge/order facts'],
         examples=['no overload matches', 'ambiguous call', 'implementation incompatible with overload', 'invalid overload ordering'], preds=['NCK6','D8']),
    dict(id='NCF-CO-INFER', family='calls-overloads-inference', name='Generic inference, constraints, defaults, and instantiation diagnostics',
         scope='type argument count, constraint satisfaction, inference failure, NoInfer/const/default parameters, partial inference, and generic instantiation depth/budget',
         facts=['inference session result/evidence', 'type parameter constraints/defaults/variance', 'Relate proofs', 'instantiation cycle/budget facts'],
         examples=['type argument violates constraint', 'cannot infer type parameter', 'wrong type argument count', 'excessive instantiation'], preds=['NCK6','D8']),
    dict(id='NCF-CF-CONTEXT', family='contextual-functions-this', name='Contextual typing and expression conformance diagnostics',
         scope='contextual object/array/function/JSX expression typing, best common type, contextual return/parameter typing, and context loss or mismatch',
         facts=['ContextualTypeAt', 'expression observed types', 'Relate proofs', 'inference/call context'],
         examples=['contextual callback mismatch', 'array/object contextual incompatibility', 'implicit any due missing context', 'context-sensitive union failure'], preds=['NCK6','D8']),
    dict(id='NCF-CF-VAR', family='contextual-functions-this', name='Function variance, predicate, assertion, and effect diagnostics',
         scope='parameter/return variance, strictFunctionTypes behavior, predicates, assertion signatures, async/generator effects, and override callable compatibility',
         facts=['signature effects and variance', 'Relate proofs', 'FlowReturn', 'override/implementation edges'],
         examples=['unsafe parameter variance', 'invalid predicate target/type', 'assertion signature misuse', 'async/generator return mismatch'], preds=['NCK6','D8']),
    dict(id='NCF-CF-THIS', family='contextual-functions-this', name='This, super, private environment, and call-context diagnostics',
         scope='this parameter/context, super call/property ordering, derived constructor initialization, static/instance context, private fields, and lexical this',
         facts=['This/call context facts', 'class/heritage/private environment', 'flow initialization/reachability', 'relation/member facts'],
         examples=['this before super', 'super outside derived class', 'private field access violation', 'wrong this argument'], preds=['NCK6','D8','IDX0']),
    dict(id='NCF-FD-NARROW', family='flow-and-definite-assignment', name='Control-flow narrowing and impossible-condition diagnostics',
         scope='typeof/instanceof/in/equality/truthiness/discriminant/user-predicate narrowing, unreachable branches from impossible conditions, and narrowing invalidation',
         facts=['FlowNarrowingAt and ProgramAnalysisGraph', 'relation/comparability proofs', 'assignment/capture effects', 'flow graph edges'],
         examples=['condition always true/false', 'unreachable narrowed branch', 'invalid discriminant comparison', 'stale narrowing after assignment/call'], preds=['NCK6','D8']),
    dict(id='NCF-FD-DEF', family='flow-and-definite-assignment', name='Definite assignment and initialization coverage diagnostics',
         scope='local/class property definite assignment, constructor paths, loop/try/finally assignment, captured writes, and use-before-assigned checks',
         facts=['flow fixed points/completion algebra', 'assignment facts', 'constructor/field initialization regions', 'capture freshness/effects'],
         examples=['variable used before assigned', 'strict property initialization failure', 'assignment missing on one path', 'finally invalidates coverage'], preds=['NCK6','D8']),
    dict(id='NCF-FD-CFLOW', family='flow-and-definite-assignment', name='Reachability, return coverage, and control-flow legality diagnostics',
         scope='unreachable code, missing returns, not-all-paths-return, break/continue/labels, switch exhaustiveness/fallthrough, try/finally completion, and async/generator completion',
         facts=['Function/ExecutableRegion flow graph', 'FlowReturn', 'completion algebra', 'loop/switch/label region facts'],
         examples=['not all paths return', 'unreachable statement', 'illegal break/continue', 'non-exhaustive discriminated switch'], preds=['NCK6','D8']),
    dict(id='NCF-OC-MEM', family='objects-classes-merging', name='Object, class, interface, and member declaration diagnostics',
         scope='member duplicates, accessor/property/method compatibility, optional/readonly/static/private rules, index signatures, computed names, and constructor/member declarations',
         facts=['class/object/member surfaces', 'symbol/declaration merge facts', 'Relate proofs', 'private environment and computed-name facts'],
         examples=['duplicate member', 'getter/setter type mismatch', 'invalid index signature/member', 'private/static modifier misuse'], preds=['NCK6','IDX0','D8']),
    dict(id='NCF-OC-HERIT', family='objects-classes-merging', name='Heritage, override, abstract, and implementation diagnostics',
         scope='extends/implements constraints, override compatibility, abstract members/classes, constructor/base compatibility, cyclic heritage, mixins, and protected/private nominal restrictions',
         facts=['heritage/override target edges', 'class/interface surfaces', 'Relate/variance proofs', 'cycle facts'],
         examples=['incorrectly implements interface', 'override incompatible/missing override', 'non-abstract class missing member', 'cyclic base type'], preds=['NCK6','IDX0','D8']),
    dict(id='NCF-OC-MERGE', family='objects-classes-merging', name='Enums, namespaces, ambient declarations, and declaration merging diagnostics',
         scope='enum value/type rules, namespace/value/type merging, ambient/module/global declarations, augmentation legality, merge ordering, and duplicate exported surfaces',
         facts=['merged declarations', 'enum value/type facts', 'ambient/module/global augmentation authority', 'module/export index facts'],
         examples=['invalid enum initializer/member access', 'illegal namespace merge', 'ambient initializer error', 'invalid augmentation target'], preds=['NCK6','IDX0','D8']),
    dict(id='NCF-MP-MODULE', family='modules-and-projects', name='Module resolution, import/export, and package-boundary diagnostics',
         scope='module specifier resolution, import/export forms, missing exports, type-only/value usage, CommonJS/ESM interop, package exports/imports, and resolution-mode compatibility',
         facts=['ModuleResolverCore results and proofs', 'canonical paths/source lineage', 'export graph/index', 'project/compiler option environment'],
         examples=['cannot find module', 'module has no exported member', 'type-only import used as value', 'ESM/CommonJS mode violation'], preds=['NCK6','IDX0','TCM4']),
    dict(id='NCF-MP-AUG', family='modules-and-projects', name='Module/global augmentation and cross-file declaration diagnostics',
         scope='augmentation target existence, external-module context, duplicate/incompatible augmented members, global augmentation placement, and cross-file merge visibility',
         facts=['module/global augmentation facts', 'merged declaration authority', 'project membership/index', 'Relate proofs'],
         examples=['invalid module augmentation name', 'global augmentation outside module', 'incompatible augmented property', 'augmentation not visible in project'], preds=['NCK6','IDX0','TCM4','D8']),
    dict(id='NCF-MP-PROJECT', family='modules-and-projects', name='Project references, configuration, library, and program diagnostics',
         scope='project/reference graph, root/include/exclude membership, declaration/output/config compatibility, lib/type acquisition inputs, duplicate source inclusion, and project-cycle diagnostics',
         facts=['project graph/membership indexes', 'captured configuration and environment', 'source identity/outputs', 'provider/native capability requirements'],
         examples=['project reference cycle', 'file outside root/include', 'incompatible composite/declaration settings', 'missing lib/type inputs'], preds=['NCK6','IDX0','TCM4','PUB0']),
    dict(id='NCF-AT-QUERY', family='advanced-types', name='Keyof, indexed access, type query, alias, and reference diagnostics',
         scope='invalid type/value queries, key/index constraints, alias/type parameter use, qualified names, unique symbols, and type argument application at type sites',
         facts=['KeyOf/IndexedAccess/TypeOf/ProjectPath reducers', 'symbol/type namespace facts', 'relation constraints', 'alias/reference identity'],
         examples=['value used as type/type used as value', 'invalid indexed access key', 'generic type requires arguments', 'unique symbol misuse'], preds=['NCK6','D8']),
    dict(id='NCF-AT-REDUCE', family='advanced-types', name='Mapped, conditional, infer, template-literal, and utility-type diagnostics',
         scope='mapped/conditional/template reduction legality, infer placement, modifier/name remapping, distributivity, utility constraints, and reduction budget/degradation',
         facts=['mapped/conditional/template reducers', 'inference/relation proofs', 'type parameter and key domains', 'cycle/budget evidence'],
         examples=['infer outside conditional', 'invalid mapped key remap', 'utility constraint failure', 'excessive reduction depth'], preds=['NCK6','D8']),
    dict(id='NCF-AT-CYCLE', family='advanced-types', name='Recursive type, instantiation cycle, and complexity diagnostics',
         scope='recursive aliases, circular base/constraint/default/reference relationships, excessive instantiation, query depth, union/intersection complexity, and cycle-safe degradation',
         facts=['CheckerReentryGraph/cycle IDs', 'instantiation/query budgets', 'type dependency graph', 'complete/partial admission evidence'],
         examples=['type alias circularly references itself', 'base/constraint cycle', 'excessively deep instantiation', 'complex union representation limit'], preds=['NCK6','D8','G2']),
    dict(id='NCF-JF-JSX', family='jsx-and-frameworks', name='JSX intrinsic/component, props, children, and attribute diagnostics',
         scope='JSX element/tag resolution, intrinsic/component callability, props/attributes/spreads, children, refs/events, namespaces, and JSX runtime/configuration',
         facts=['JSX semantic surfaces and call resolution', 'component/intrinsic contracts', 'Relate/contextual proofs', 'module/config/runtime facts'],
         examples=['unknown intrinsic/component', 'missing/invalid prop', 'invalid children/ref/event', 'wrong JSX runtime namespace'], preds=['NCK6','NCK5','IDX0','D8']),
    dict(id='NCF-JF-VUE', family='jsx-and-frameworks', name='Vue template and component-contract diagnostics',
         scope='Vue template regions, local/global components, directives, props/emits/events/slots/models/refs, template narrowing, and custom-element exclusions',
         facts=['Vue SemanticContributionBatch', 'component contracts/global registrations', 'template executable regions/contextual/narrowing facts', 'shared relation/call proofs'],
         examples=['unknown/missing/wrong prop', 'unknown event/slot/directive/component', 'template expression type error', 'global component/custom element resolution'], preds=['NCK6','NCK5','IDX0','D8']),
    dict(id='NCF-JF-SVELTE', family='jsx-and-frameworks', name='Svelte template, rune, event, slot/snippet, and component-contract diagnostics',
         scope='Svelte template regions, runes/reactivity, props/events/bindings/actions/transitions/snippets/slots, await/each/control narrowing, and component contracts',
         facts=['Svelte SemanticContributionBatch', 'component contracts and template regions', 'reactivity/flow/contextual facts', 'shared relation/call proofs'],
         examples=['invalid binding/event/action/transition', 'rune misuse', 'snippet/slot/component contract mismatch', 'template expression/narrowing error'], preds=['NCK6','NCK5','IDX0','D8']),
    dict(id='NCF-JD-JS', family='javascript-jsdoc-decorators', name='JavaScript and CommonJS semantic diagnostics',
         scope='checkJs JavaScript semantics, CommonJS imports/exports, constructor/prototype patterns, property inference, implicit any, and JS-specific assignment/call behavior',
         facts=['JS parser/lowering facts', 'CommonJS module/export graph', 'shared relation/call/flow facts', 'captured checkJs/config environment'],
         examples=['implicit any in checked JS', 'invalid prototype/property use', 'CommonJS export/import mismatch', 'constructor/call mismatch'], preds=['NCK6','PAR0','IDX0','D8']),
    dict(id='NCF-JD-JSDOC', family='javascript-jsdoc-decorators', name='JSDoc type, template, import, and tag diagnostics',
         scope='JSDoc type parsing/resolution, @template/@param/@returns/@type/@typedef/@import tags, tag placement, duplicate/missing tags, and JS declaration conformance',
         facts=['dedicated JSDoc parse path', 'symbol/type/module resolution', 'signature/declaration facts', 'Relate proofs'],
         examples=['unresolved/invalid JSDoc type', 'tag/parameter mismatch', 'invalid template constraint/default', 'JSDoc import/typedef conflict'], preds=['NCK6','PAR0','IDX0','D8']),
    dict(id='NCF-JD-DEC', family='javascript-jsdoc-decorators', name='Decorator, metadata, and auto-accessor diagnostics',
         scope='legacy/standard decorator applicability, decorator call signatures/return types/context, emit metadata constraints, parameter/property/class decorators, and auto-accessor semantics',
         facts=['decorator executable regions and resolved calls', 'class/member surfaces', 'configuration/emit metadata environment', 'Relate/contextual proofs'],
         examples=['decorator not callable', 'wrong decorator return/context', 'invalid parameter decorator location', 'auto-accessor/decorator incompatibility'], preds=['NCK6','PAR0','D8']),
]


def q(s: str) -> str:
    return '"' + s.replace('\\','\\\\').replace('"','\\"') + '"'

def arr(xs: list[str]) -> str:
    return '[' + ', '.join(q(x) for x in xs) + ']'

# Write manifest.
manifest = [
    'schema = 1',
    'product = "native_checker"',
    'generator_owner = "NCK4"',
    'promotion_owner = "NCK6"',
    'terminal_owner = "NCK8"',
    'required_default = true',
    '',
]
for row in FAMILIES:
    manifest += [
        '[[slice]]',
        f'id = {q(row["id"])}',
        f'family = {q(row["family"])}',
        f'name = {q(row["name"])}',
        'required = true',
        f'predecessors = {arr(row["preds"])}',
        f'scope = {q(row["scope"])}',
        f'fact_sources = {arr(row["facts"])}',
        f'example_obligations = {arr(row["examples"])}',
        'oracle = "pinned-tsgo-diagnostic-oracle"',
        'correction_overlay = "review-gated-single-spec-overlay"',
        'max_production_loc = 800',
        'max_production_files = 8',
        'max_related_packages = 2',
        '',
    ]
(ROOT / 'catalogs').mkdir(exist_ok=True)
(ROOT / 'catalogs/native-checker-family-manifest.toml').write_text('\n'.join(manifest).rstrip()+'\n', encoding='utf-8')

# Write generated DAG module.
dag = ['schema = 4', 'module = "expansion-native-checker-families"', f'pinned_commit = {q(PIN_COMMIT)}', f'pinned_tree = {q(PIN_TREE)}', '']
for row in FAMILIES:
    preds = ['NCK4'] + [p for p in row['preds'] if p != 'NCK4']
    dag += [
        '[[node]]', f'id = {q(row["id"])}', f'name = {q(row["name"])}', f'predecessors = {arr(preds)}',
        'conditional_predecessors = []', 'phase = "expansion"', 'train = "expansion.native-checker"', 'product = "native_checker"',
        'kind = "implementation"', 'semantic_role = "delivery"', 'class = "successor-generated"',
        'owner = "expansion.native-checker:one certified semantic diagnostic feature slice"',
        'conflict_domains = ["semantic_authority", "diagnostic_action_service", "vertical_manifest"]',
        'resource_class = "rust-mixed"', 'gate_profile = "targeted-domain"', 'review_profile = "semantic-3"',
        'dispatchable = true', 'optional = false', 'release_gating = "none"',
        'source_refs = ["catalog:docs/arch/refactor/rev11/catalogs/native-checker-family-manifest.toml"]',
        'external_requirements = []', 'activation_gate = "ORC0"',
        f'charter = {q("charters/expansion-native-checker/generated-families/" + row["id"] + ".md")}',
        'implementation_effort_min = "high"', 'implementation_effort_default = "high"',
        'review_effort_min = "high"', 'review_effort_default = "high"',
        'verification_effort_min = "high"', 'verification_effort_default = "high"',
        'confirmation_effort_min = "high"', 'confirmation_effort_default = "high"',
        'size = "M"', 'max_production_loc = 800', 'max_production_files = 8', 'max_related_packages = 2',
        'rescope_loc = 1500', 'rescope_files = 12', 'rescope_unrelated_packages = 3', '',
    ]
(ROOT / 'expansion-native-checker-families.example.toml').write_text('\n'.join(dag).rstrip()+'\n', encoding='utf-8')

# Write detailed generated charters.
outdir = ROOT / 'charters/expansion-native-checker/generated-families'
outdir.mkdir(parents=True, exist_ok=True)
for row in FAMILIES:
    preds = ['NCK4'] + [p for p in row['preds'] if p != 'NCK4']
    pred_csv = ','.join(preds)
    facts = '\n'.join(f'- {x}' for x in row['facts'])
    examples = '\n'.join(f'- {x}' for x in row['examples'])
    text = f'''<!-- unified-charter-v2
id={row['id']}
name={row['name']}
phase=expansion
train=expansion.native-checker
product=native_checker
kind=implementation
semantic_role=delivery
class=successor-generated
predecessors={pred_csv}
conditional_predecessors=
owner=expansion.native-checker:one certified semantic diagnostic feature slice
conflict_domains=semantic_authority,diagnostic_action_service,vertical_manifest
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=catalog:docs/arch/refactor/rev11/catalogs/native-checker-family-manifest.toml
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/generated-families/{row['id']}.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# {row['id']} - {row['name']}

## Independently acceptable outcome

Implement, certify, and promote exactly one native checker feature slice for **{row['scope']}**. The slice consumes shared semantic facts and proofs, emits first-class authored diagnostics through NCK2/NCK3, passes the NCK4 hermetic oracle/correction-overlay contract, and promotes through NCK6 at exact family/slice/profile identity. It does not broaden adjacent semantic families.

## Architectural boundary

This node is generated from `native-checker-family-manifest.toml`. Its identity, predecessor set, scope, evidence keys, and budgets are manifest-derived. The node owns diagnostic evaluation for this slice only. Symbol/type/relation/call/flow/module/project facts remain owned by their existing semantic authorities; provider TypeScript remains oracle/fallback until promotion and is never called from the native query.

### Required fact and proof inputs

{facts}

### Required oracle obligations

{examples}

## Expected production changes

- Add the exact `DiagnosticRuleSpec` rows and rule evaluators for this slice.
- Add any missing typed fact view or proof reference at the lowest existing semantic owner; do not create a checker-private fact store or resolver.
- Add hermetic source fixtures, canonical expected diagnostics, and pinned tsgo oracle rows.
- Add reviewed correction-overlay entries only for clear conceded TypeScript bugs; the runtime resolver remains single-spec.
- Add incremental invalidation/read-set wiring, complete-only cache admission, cancellation/budget handling, and PER0 work counters.
- Add promotion evidence mapping this generated node to exact `DiagnosticAuthorityKey` rows in NCK6.

## Internal subblocks

### {row['id']}-SB1 - Oracle and RED characterization

- Partition the exact semantic cases, negative controls, recovery/profile dimensions, and expected authored anchors.
- Produce failing native tests before implementation and a pinned external oracle snapshot.
- Prove the harness detects wrong code, missing/extra diagnostic, wrong primary/related anchor, wrong completeness, and wrong fix intent.

### {row['id']}-SB2 - Fact-demand and proof contract

- Enumerate every fact/proof/query read and exact environment dimension.
- Add a typed read-only fact view if necessary at the existing owner.
- Abort/rescope if the proposed evaluator would walk source text/types or duplicate semantic resolution.

### {row['id']}-SB3 - Bounded rule implementation

- Implement one or more small `DiagnosticRule` evaluators sharing NCK3 infrastructure.
- Use semantic subjects and proof references; no message-text/source-regex/generated-TSX semantic decisions.
- Emit stable diagnostic/family/rule/subject/anchor identity and typed fix intents only where safely specified.

### {row['id']}-SB4 - Incremental, cancellation, and admission closure

- Key by exact semantic subject/profile/env/read-set identity.
- Admit only complete results; cancelled, stale, budget-exceeded, NeedInputs, unsupported, or partial results are ReturnOnly.
- Prove targeted invalidation and incremental-equals-fresh across relevant edits/config/lib/project changes.

### {row['id']}-SB5 - Parity and correction-overlay certification

- Compare canonical native diagnostics against the pinned oracle across all manifest rows/property fixtures.
- Classify every mismatch as implementation defect, unsupported/rescope, oracle defect, or reviewed correction overlay.
- Bind implementation, oracle, overlay, fixture, toolchain, and evidence digests in the certification receipt.

### {row['id']}-SB6 - Family-scoped promotion and zero-provider proof

- Promote only exact profile/family/slice rows through NCK6.
- Prove ObserveNative was non-publishing, CertifiedNative atomically suppresses external diagnostics, and rollback is receipt-backed.
- Prove warm certified requests perform zero external diagnostic work for this slice.

## Acceptance IDs

- **{row['id']}-AC-ORACLE:** exact row coverage and planted wrong-answer sensitivity.
- **{row['id']}-AC-ONE-ENGINE:** no checker-private resolver/walker/source-text/generated-text semantic path.
- **{row['id']}-AC-FACTS:** every result records exact shared fact/proof read set and environment identity.
- **{row['id']}-AC-INCREMENTAL:** incremental equals fresh; targeted edits invalidate exactly.
- **{row['id']}-AC-ADMISSION:** cancelled/stale/partial/budget/NeedInputs outcomes never warm-admit as complete.
- **{row['id']}-AC-PARITY:** all required rows match oracle or carry accepted correction-overlay evidence.
- **{row['id']}-AC-PROMOTION:** exact slice promotion has no duplicate/gap and certified warm runtime performs zero provider diagnostic work.
- **{row['id']}-AC-WORK:** equivalent-work, allocation, latency, and retained-memory thresholds pass.

## Forbidden designs

- A second type/relation/call/flow/module/project resolver or checker-private semantic store.
- Runtime tsgo/tsserver invocation from native rule evaluation.
- Source slicing, regex/type-text parsing, synthesize-then-reparse, or generated TSX as semantic truth. The only text carve-out is the dedicated JSDoc parser for the JSDoc slice.
- Message/range-only diagnostic identity or deduplication.
- Whole-program eager checking for a scoped region/file demand.
- Extending adjacent manifest slices without a generator amendment.

## Budgets and mandatory rescope

Target ceiling is 800 production LOC, 8 production files, and 2 related packages. Rescope before mutation above 1,500 LOC, 12 files, 3 unrelated packages, or whenever the slice requires a new major semantic algorithm rather than consuming accepted facts. A slice that cannot fit one independent review context must be split in the manifest before implementation.

## Verification

1. Run generated hermetic rows and negative controls for `{row['id']}`.
2. Run pinned oracle comparison and correction-overlay validation.
3. Run incremental/fresh, cancellation/budget, cache-admission, authority promotion/rollback, provider-zero-work, allocation/latency/RSS tests.
4. Run the canonical targeted gate and independent semantic review on the landing-frozen candidate.

## Deletion and residual ownership

Delete only legacy diagnostic routes/tests/ignored rows exactly mapped to this slice after promotion. Neighboring diagnostic families and remaining external provider ownership are untouched and remain capability-visible.
'''
    (outdir / f"{row['id']}.md").write_text(text, encoding='utf-8')

print('wrote', len(FAMILIES), 'checker family charters, manifest, and DAG example')

# Generated family convergence node: no external "all slices complete" assertion.
all_ids = [row['id'] for row in FAMILIES]
convergence = [
    '', '[[node]]', 'id = "NCKF0"', 'name = "Required native checker diagnostic-family convergence"',
    f'predecessors = {arr(all_ids)}', 'conditional_predecessors = []', 'phase = "expansion"',
    'train = "expansion.native-checker"', 'product = "native_checker"', 'kind = "convergence"',
    'semantic_role = "convergence"', 'class = "successor-generated-convergence"',
    'owner = "expansion.native-checker:machine-generated required-family receipt convergence"',
    'conflict_domains = ["vertical_manifest", "program_authority", "performance_evidence"]',
    'resource_class = "rust-mixed"', 'gate_profile = "targeted-domain"', 'review_profile = "architecture-3"',
    'dispatchable = true', 'optional = false', 'release_gating = "contract"',
    'source_refs = ["catalog:docs/arch/refactor/rev11/catalogs/native-checker-family-manifest.toml"]',
    'external_requirements = []', 'activation_gate = "ORC0"',
    'charter = "charters/expansion-native-checker/generated-families/NCKF0.md"',
    'implementation_effort_min = "high"', 'implementation_effort_default = "high"',
    'review_effort_min = "high"', 'review_effort_default = "high"',
    'verification_effort_min = "high"', 'verification_effort_default = "high"',
    'confirmation_effort_min = "high"', 'confirmation_effort_default = "high"',
    'size = "S"', 'max_production_loc = 300', 'max_production_files = 3', 'max_related_packages = 1',
    'rescope_loc = 1500', 'rescope_files = 12', 'rescope_unrelated_packages = 3',
]
dag_path = ROOT / 'expansion-native-checker-families.example.toml'
dag_path.write_text(dag_path.read_text(encoding='utf-8').rstrip() + '\n' + '\n'.join(convergence) + '\n', encoding='utf-8')

nckf0 = f'''<!-- unified-charter-v2
id=NCKF0
name=Required native checker diagnostic-family convergence
phase=expansion
train=expansion.native-checker
product=native_checker
kind=convergence
semantic_role=convergence
class=successor-generated-convergence
predecessors={','.join(all_ids)}
conditional_predecessors=
owner=expansion.native-checker:machine-generated required-family receipt convergence
conflict_domains=vertical_manifest,program_authority,performance_evidence
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=contract
source_refs=catalog:docs/arch/refactor/rev11/catalogs/native-checker-family-manifest.toml
external_requirements=
activation_gate=ORC0
charter=charters/expansion-native-checker/generated-families/NCKF0.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# NCKF0 - Required native checker diagnostic-family convergence

## Independently acceptable outcome

Prove that every manifest row marked `required = true` has an accepted implementation receipt, current oracle/correction-overlay certification receipt, exact NCK6 promotion receipt, provider-zero-work evidence, and current charter/source digest. NCKF0 is generated from the manifest and adds no semantic rule or diagnostic algorithm.

## Architecture

- The predecessor set is generated from the exact required slice rows; hand-maintained lists and external “all complete” assertions are forbidden.
- A slice is complete only when implementation, certification, authority promotion, incremental/admission, and performance evidence all bind the same candidate and manifest row identity.
- Optional/residual rows remain explicit and do not block NCKF0 unless promoted to required by an amendment.
- Any changed manifest, charter, source atom, implementation, oracle, overlay, toolchain, authority, or evidence digest invalidates convergence.
- NCKF0 emits one immutable `NativeCheckerFamilyConvergenceReceipt` consumed by NCK8.

## Internal subblocks

### NCKF0-SB1 - Manifest/predecessor bijection

Generate the predecessor set from required rows and prove exact set equality, stable ordering, no duplicate/unknown IDs, and no required row without a DAG node/charter.

### NCKF0-SB2 - Receipt chain validation

For every slice, validate exact implementation, oracle, correction-overlay, certification, promotion, provider-zero-work, gate, and review receipts against the current tree.

### NCKF0-SB3 - Cross-slice authority consistency

Prove no overlapping family/profile/feature-slice publishing authority, no gaps for required applicability, stable diagnostic identity namespaces, and no conflicting correction overlays.

### NCKF0-SB4 - Global incremental/admission invariants

Run generated class-wide mutations proving no required slice caches cancelled/stale/partial/NeedInputs/budget outcomes as complete and that combined incremental results equal fresh results.

### NCKF0-SB5 - Equivalent-work and retained-state convergence

Aggregate PER0 counters without hiding per-slice regressions; require provider diagnostic work zero for every certified slice and bounded memory across combined workloads.

### NCKF0-SB6 - Immutable convergence receipt

Emit a receipt binding manifest, generated DAG/charters, all predecessor receipts, source atoms, authority snapshot, toolchains, evidence, and reviews. Any input change invalidates the receipt.

## Acceptance IDs

- **NCKF0-AC-BIJECTION:** required manifest rows equal generated predecessors/charters exactly.
- **NCKF0-AC-RECEIPTS:** every slice has one current complete implementation/certification/promotion/evidence chain.
- **NCKF0-AC-AUTHORITY:** required applicability has no duplicate or missing publishing authority.
- **NCKF0-AC-ADMISSION:** class-wide mutations preserve complete-only admission and incremental/fresh equality.
- **NCKF0-AC-ZERO-PROVIDER:** every certified required slice performs zero external diagnostic work at runtime.
- **NCKF0-AC-PERF:** aggregate and per-slice equivalent-work/allocation/latency/RSS thresholds pass.

## Forbidden designs

- Manual/external attestation that “all slices are complete”.
- Patching a semantic mismatch, rule, or feature in this convergence block.
- Aggregate pass percentages that hide one stale/missing slice receipt.
- Promoting optional/residual rows without a manifest amendment.

## Verification

Run the manifest generator/bijection validator, every generated receipt validator, class-wide authority/admission/provider-zero-work mutations, combined incremental/fresh and churn/performance suites, canonical gate, and independent architecture review on the landing-frozen candidate.
'''
(outdir / 'NCKF0.md').write_text(nckf0, encoding='utf-8')

# Enrich manifest header for the generated convergence authority.
mp = ROOT / 'catalogs/native-checker-family-manifest.toml'
mt = mp.read_text(encoding='utf-8')
mt = mt.replace('terminal_owner = "NCK8"\n', 'family_convergence_owner = "NCKF0"\nterminal_owner = "NCK8"\n')
mp.write_text(mt, encoding='utf-8')
print('wrote NCKF0 generated convergence node and charter')
