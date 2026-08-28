<!-- unified-charter-v2
id=UAK1
name=Universal-tooling constitution and program split
phase=expansion
train=expansion.kernel
product=kernel
kind=constitution
semantic_role=delivery
class=successor
predecessors=UAK0
conditional_predecessors=
owner=expansion.kernel:typed immutable universal catalog and demand-selected kernel services
conflict_domains=carrierprofileid
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
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
source_refs=source:successor-expansion.md:L770
external_requirements=
activation_gate=ORC0
charter=charters/expansion-kernel/UAK1.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# UAK1 — Universal-tooling constitution and program split

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Universal-tooling constitution and program split. The current owner is **framework-shaped host/session registries and untagged public boundaries**. The final and sole owner is **typed immutable universal catalog and demand-selected kernel services**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `crates/verter_protocol/src`, `crates/verter_identity/src`.
- Named API/data boundaries: `CarrierProfileId`, `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`, `DemandPlan`, `TypeInfoRequest`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **UAK0:** exact current receipt ID and digest for “Current-head authority and displacement reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** ratify the dependency directions and the boundary between universal kernel, horizontal products, verticals, project profiles, and optional compilers.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **central framework switch**, **untagged coordinate/public identity**, **duplicate component information authority** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **UAK1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **UAK1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **UAK1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **UAK1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_language/tests`, `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **central framework switch**.
- Delete or structurally reject: **untagged coordinate/public identity**.
- Delete or structurally reject: **duplicate component information authority**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_protocol -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L770`

## Reconciled source-plan contract

**Intent:** ratify the dependency directions and the boundary between universal kernel, horizontal products, verticals, project profiles, and optional compilers.
**Predecessors:** `UAK0`.
**Subblocks:** (1) lock no-runtime/no-compiler-creep rules; (2) lock carrier→semantic-profile→project-profile layering; (3) lock public capability truth and partial outcomes; (4) lock independent vertical/product terminals and continuous soak joins; (5) lock static registration/no dynamic plugin ABI; (6) bind exact-digest Codex Architect review plus maintainer adoption.
**Acceptance:** dependency-firewall tests reject imports from kernel into vertical, project, editor-host, CLI presentation, or compiler-backend owners; the DAG validator proves acyclicity and no global release join.
**Forbidden:** universal framework IR, one parser implementation requirement, project profiles selecting TS programs, or a compiler capability inferred from tooling support.
**Deletion/abort:** supersede the old 251-block release universe; abort if the constitution needs a named future framework to define a supposedly universal core contract.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L770-B34A86B5F58A

- Kind: `context`
- Source: `successor-expansion.md:770-770`
- Applicability: `UAK1`
- Exact text SHA-256: `b34a86b5f58a071b2c224817bc7074ab1248eaf0e3283172f71a8e9e24d7cd6e`

~~~~markdown
### `UAK1.md` — Universal-tooling constitution and program split
~~~~

### SRC-EXP-L772-BAC2C878F14C

- Kind: `forbidden`
- Source: `successor-expansion.md:772-777`
- Applicability: `UAK1`
- Exact text SHA-256: `bac2c878f14c130d7d096e7c40f3cb3a8b088c5cdf8b867bd2d182188e603db7`

~~~~markdown
**Intent:** ratify the dependency directions and the boundary between universal kernel, horizontal products, verticals, project profiles, and optional compilers.
**Predecessors:** `UAK0`.
**Subblocks:** (1) lock no-runtime/no-compiler-creep rules; (2) lock carrier→semantic-profile→project-profile layering; (3) lock public capability truth and partial outcomes; (4) lock independent vertical/product terminals and continuous soak joins; (5) lock static registration/no dynamic plugin ABI; (6) bind exact-digest Codex Architect review plus maintainer adoption.
**Acceptance:** dependency-firewall tests reject imports from kernel into vertical, project, editor-host, CLI presentation, or compiler-backend owners; the DAG validator proves acyclicity and no global release join.
**Forbidden:** universal framework IR, one parser implementation requirement, project profiles selecting TS programs, or a compiler capability inferred from tooling support.
**Deletion/abort:** supersede the old 251-block release universe; abort if the constitution needs a named future framework to define a supposedly universal core contract.
~~~~

### SRC-EXP-L1094-3C0F933AED33

- Kind: `context`
- Source: `successor-expansion.md:1094-1094`
- Applicability: `UAK1`
- Exact text SHA-256: `3c0f933aed33d53d694b591d0fe4909eb7252ab3fefb41553916bdcbb34b63dd`

~~~~markdown
## 10. First architecture implementation: HTML + Custom Elements
~~~~

### SRC-EXP-L1222-C20C89BDBC84

- Kind: `context`
- Source: `successor-expansion.md:1222-1222`
- Applicability: `UAK1`
- Exact text SHA-256: `c20c89bdbc84db907a8a46eceeacebfa72016029d06022d031790fc136bc2f5a`

~~~~markdown
## 11. Sequential architecture falsification slices
~~~~

### SRC-EXP-L1224-C6F142185503

- Kind: `requirement`
- Source: `successor-expansion.md:1224-1224`
- Applicability: `UAK1`
- Exact text SHA-256: `c6f1421855035f1a485647eab558c90b284e89b07222582f5558809b92ffff6e`

~~~~markdown
These blocks are deliberately thin and initially unshipped. Each proves parse/extract, activation, exact authored maps, one TypeInfo contribution, one type-aware IDE request, one safe lint/action, formatter-view/map feasibility through the shared `FMT1` private harness, one `PUB0` surface-neutral path, zero-work behavior, and a vertical-specific counterexample. Proof code cannot register public LSP/CLI capabilities or survive as a second product authority. They do not imply “full support.”
~~~~

### SRC-EXP-L1565-74428633D172

- Kind: `context`
- Source: `successor-expansion.md:1565-1565`
- Applicability: `UAK1`
- Exact text SHA-256: `74428633d172ddeec4d5b3be21147d9669a87ffdcee59825eb84b3449a4196c3`

~~~~markdown
## 15. Future vertical portfolio—not active kernel charters
~~~~

### SRC-EXP-L1567-21678E757833

- Kind: `context`
- Source: `successor-expansion.md:1567-1567`
- Applicability: `UAK1`
- Exact text SHA-256: `21678e7578332887c2f96256b40fc31ae42826f518234890a4d3738440667d5a`

~~~~markdown
### 15.1 Definition of a complete tooling vertical
~~~~

### SRC-EXP-L1569-75B8690436D2

- Kind: `requirement`
- Source: `successor-expansion.md:1569-1569`
- Applicability: `UAK1`
- Exact text SHA-256: `75b8690436d2be9e0538fc8afdf8d07588548cbb92d1d7c3ef0df66442476c5d`

~~~~markdown
A future vertical is first-class only when its manifest truthfully covers every applicable cell below. “Full LSP” means all applicable operations, not registered no-op handlers.
~~~~

### SRC-EXP-L1571-0C44C2D0DA47

- Kind: `requirement`
- Source: `successor-expansion.md:1571-1571`
- Applicability: `UAK1`
- Exact text SHA-256: `0c44c2d0da47da5d528e64026832239c18541b3f42aaf380f02de31561cf434f`

~~~~markdown
| Domain | Required disposition |
~~~~

### SRC-EXP-L1572-AC92D1B091B2

- Kind: `context`
- Source: `successor-expansion.md:1572-1572`
- Applicability: `UAK1`
- Exact text SHA-256: `ac92d1b091b25afced4265ab8cc2f40af0ad8ffa13d3ba560710a1caa14bfa06`

~~~~markdown
|---|---|
~~~~

### SRC-EXP-L1573-54E2A4E787F3

- Kind: `context`
- Source: `successor-expansion.md:1573-1573`
- Applicability: `UAK1`
- Exact text SHA-256: `54e2a4e787f3c91bea105aad2956b7e0d16c9a41d0e5e865abf4f419952642ec`

~~~~markdown
| Syntax | Owned carrier parser, admitted shared parser, or explicit host-language reuse |
~~~~

### SRC-EXP-L1574-AF0F1DB39E88

- Kind: `requirement`
- Source: `successor-expansion.md:1574-1574`
- Applicability: `UAK1`
- Exact text SHA-256: `af0f1db39e885991c59cbbdd7c0befbdeb65a1ff0d222cabb5de07add32e5d97`

~~~~markdown
| Activation | Pre-projection and post-snapshot claims with exact provenance |
~~~~

### SRC-EXP-L1575-FF4165A5436B

- Kind: `context`
- Source: `successor-expansion.md:1575-1575`
- Applicability: `UAK1`
- Exact text SHA-256: `ff4165a5436bc5a2e54d71e34babd32683c25c7401d72fcd60566b933fafea35`

~~~~markdown
| Semantics | Framework-local facts and TypeInfo operation ownership |
~~~~

### SRC-EXP-L1576-F2E37B55D083

- Kind: `context`
- Source: `successor-expansion.md:1576-1576`
- Applicability: `UAK1`
- Exact text SHA-256: `f2e37b55d083950f309cf3127f3746c97578f4da16c85a46070717ef6c5c31f9`

~~~~markdown
| Component information | TypeInfo-backed `ComponentInfo` facets and public projection |
~~~~

### SRC-EXP-L1577-2480CD94DE24

- Kind: `requirement`
- Source: `successor-expansion.md:1577-1577`
- Applicability: `UAK1`
- Exact text SHA-256: `2480cd94de2430a33566f07eecae2fa432cf7b3ff4e697dd91a2c225bf3a91ae`

~~~~markdown
| Maps | Exact authored/generated/embedded/format/action map cells |
~~~~

### SRC-EXP-L1578-5EF146D159A0

- Kind: `context`
- Source: `successor-expansion.md:1578-1578`
- Applicability: `UAK1`
- Exact text SHA-256: `5ef146d159a06cf0d3966649675bf1d5499676f4db2af746ddd6973fa5a54977`

~~~~markdown
| Diagnostics | Parse, framework, type, configuration, and project diagnostics as applicable |
~~~~

### SRC-EXP-L1579-5AABCF7926CC

- Kind: `context`
- Source: `successor-expansion.md:1579-1579`
- Applicability: `UAK1`
- Exact text SHA-256: `5aabcf7926cc901cdb213131613b965c41015c06fc3af0988aba6b1c588db47e`

~~~~markdown
| Lint/actions | Native rules, safe fixes, refactors, and applicability/version matrix |
~~~~

### SRC-EXP-L1580-00C145848900

- Kind: `context`
- Source: `successor-expansion.md:1580-1580`
- Applicability: `UAK1`
- Exact text SHA-256: `00c145848900a4e3e5b6f720b237520cb99d1fc1430e4ecee195c9652b874204`

~~~~markdown
| Formatter | Outer carrier/embedded composition or explicit host-printer reuse |
~~~~

### SRC-EXP-L1581-FAC5E1064170

- Kind: `context`
- Source: `successor-expansion.md:1581-1581`
- Applicability: `UAK1`
- Exact text SHA-256: `fac5e1064170e562121b10a1bf6510016b0d958dbbda467a9c8b31edbb6b55c1`

~~~~markdown
| IDE/LSP | Completion, hover, signature help, definition/type-definition/implementation, references, highlights, symbols, rename, code actions, formatting, links, colors, folding, selection, semantic tokens, inlay hints, call/type hierarchy, linked editing, and file operations where applicable |
~~~~

### SRC-EXP-L1582-62E81931E554

- Kind: `context`
- Source: `successor-expansion.md:1582-1582`
- Applicability: `UAK1`
- Exact text SHA-256: `62e81931e554f6129aece705cf770122746baa53a469301ed7f2e4067b96cec6`

~~~~markdown
| Workspace | Index contributions, imports/consumers, auto-import, moves, assets/links/routes as applicable |
~~~~

### SRC-EXP-L1583-DA9AD50D99D7

- Kind: `context`
- Source: `successor-expansion.md:1583-1583`
- Applicability: `UAK1`
- Exact text SHA-256: `da9ad50d99d7d2554aaca7e67fdc2b23222c313bc2ee59d271bbab050f888d2e`

~~~~markdown
| Custom Elements | Separate producer and consumer disposition with evidence |
~~~~

### SRC-EXP-L1584-3720A779721C

- Kind: `context`
- Source: `successor-expansion.md:1584-1584`
- Applicability: `UAK1`
- Exact text SHA-256: `3720a779721c653104537115182657626e8ed3cda80f38843ba3efd1ca21dedf`

~~~~markdown
| Coexistence | `auto|disabled|workspace|full` presets, per-capability ownership mask, and zero-work proof |
~~~~

### SRC-EXP-L1585-E73E346B74CE

- Kind: `context`
- Source: `successor-expansion.md:1585-1585`
- Applicability: `UAK1`
- Exact text SHA-256: `e73e346b74ce9e532e73368c86e6019f3a27f00e9679beb542e3443f38ebf925`

~~~~markdown
| Public products | Rust, NAPI, WASM, LSP, MCP, and CLI capability truth |
~~~~

### SRC-EXP-L1586-0EA29938123C

- Kind: `context`
- Source: `successor-expansion.md:1586-1586`
- Applicability: `UAK1`
- Exact text SHA-256: `0ea29938123c958b452570d913700938ec443c05cd454208dea29df59603a6ca`

~~~~markdown
| Quality | Immutable oracle/corpus, incremental=fresh, cancellation, Unicode/maps, equivalent-work performance, RSS plateau, security, and three independent exact-candidate reviews |
~~~~

### SRC-EXP-L1587-9E3B979D2B48

- Kind: `forbidden`
- Source: `successor-expansion.md:1587-1587`
- Applicability: `UAK1`
- Exact text SHA-256: `9e3b979d2b4800ef8c10118ba67da807cdcde6ab6bff335e5054720bbb8d0881`

~~~~markdown
| Compiler | `Supported`, `FutureSeparateTrain`, or `NotApplicable`; never inferred from tooling |
~~~~

### SRC-EXP-L1589-3FBF146F8596

- Kind: `requirement`
- Source: `successor-expansion.md:1589-1589`
- Applicability: `UAK1`
- Exact text SHA-256: `3fbf146f85962ae7f06cb2336f20f937b5c2d2e535c6f9097b2e1ec94107c8d3`

~~~~markdown
The two workflow skills instantiate the generic vertical lifecycle from §5.3 only when that vertical becomes the selected next investment. This keeps detailed implementation charters current with the then-accepted kernel and exact framework release.
~~~~

### SRC-EXP-L1591-72449C54424D

- Kind: `context`
- Source: `successor-expansion.md:1591-1591`
- Applicability: `UAK1`
- Exact text SHA-256: `72449c54424d13b8c7fc14a972765478fde601a55c1d8e71d9c9bbbfe3088c0a`

~~~~markdown
### 15.2 Portfolio dossiers
~~~~

### SRC-EXP-L1593-7A2863AF6C92

- Kind: `requirement`
- Source: `successor-expansion.md:1593-1593`
- Applicability: `UAK1`
- Exact text SHA-256: `7a2863af6c9278f90dcdabf5ce4f32b519f7438af9b64d9dea6cae909866f564`

~~~~markdown
| Vertical | Geometry and parser decision hypothesis | Verter-specific product value | Required special cells | Compiler disposition |
~~~~

### SRC-EXP-L1594-1DE25071DFBC

- Kind: `context`
- Source: `successor-expansion.md:1594-1594`
- Applicability: `UAK1`
- Exact text SHA-256: `1de25071dfbcd236934eb9f3678ebfec9dd4c3536111762658ac30cb74aca430`

~~~~markdown
|---|---|---|---|---|
~~~~

### SRC-EXP-L1595-6B55B478272B

- Kind: `context`
- Source: `successor-expansion.md:1595-1595`
- Applicability: `UAK1`
- Exact text SHA-256: `6b55b478272b9464ff68e0e052a3e67364f55962530fae23bd04d65636ee9e87`

~~~~markdown
| **MDX** | Dedicated Markdown/MDX carrier; reuse OXC for embedded ESM/JSX, not Volar architecture | React-component auto-import, path/link completion, file-move link updates, refactors, bounded CPU, cross-framework component graph; reusable Vue `<block lang="md">` content tooling | Markdown/MDX recovery, ESM/JSX maps, generic provider first and React-specific provider after `RCTP`, assets/links/headings, remark config capture without plugin execution | `NotApplicable` |
~~~~

### SRC-EXP-L1596-2D385B5EC431

- Kind: `requirement`
- Source: `successor-expansion.md:1596-1596`
- Applicability: `UAK1`
- Exact text SHA-256: `2d385b5ec431d36294699391c91b1a0b0ae31c4cff950b8ef6a3ee1a4110d750`

~~~~markdown
| **Astro** | Dedicated heterogeneous `.astro` carrier; owned frontend selected by proof, with embedded OXC/HTML/CSS products | First-class IDE, lint/fixes, formatting, TypeInfo, component metadata, island navigation, assets, cross-framework graph, Rust/NAPI/WASM/CLI | Frontmatter, directives, component kinds, client islands, script/style regions, exact nested maps, all applicable LSP rows, CE consumption | `FutureSeparateTrain` |
~~~~

### SRC-EXP-L1597-74E4E7A97E30

- Kind: `context`
- Source: `successor-expansion.md:1597-1597`
- Applicability: `UAK1`
- Exact text SHA-256: `74e4e7a97e30c24a403746dc6a4ff948358bbd284d7c51c353a96ed2eab313be`

~~~~markdown
| **Lit** | TS/JS host parsed by OXC plus symbol-proven tagged HTML attachments | High-quality embedded HTML/CSS, TypeInfo/WC production, cross-framework consumption | Hole maps, directives, decorators/static properties, reactive fields, events, slots, parts/CSS properties | `NotApplicable` |
~~~~

### SRC-EXP-L1598-1BA24ABD8B5B

- Kind: `requirement`
- Source: `successor-expansion.md:1598-1598`
- Applicability: `UAK1`
- Exact text SHA-256: `1ba24abd8b5b61952234c4e48209cc4260d7926c4384f415900fec1ddfcd3539`

~~~~markdown
| **React** | OXC/TypeScript TSX plus semantic overlay | Component/hook/props/ref/children metadata, React Compiler lint, cross-framework graph; later Next project semantics | Server/client directives remain framework/project scoped; CE consumption and explicit wrapper production only when proven | `NotApplicable` |
~~~~

### SRC-EXP-L1599-614228CDD10E

- Kind: `requirement`
- Source: `successor-expansion.md:1599-1599`
- Applicability: `UAK1`
- Exact text SHA-256: `614228cdd10e3dc73c0ae680885d794b98eaed3097805f79fe02c1bcb5e29076`

~~~~markdown
| **Solid** | Same OXC/TypeScript TSX geometry, separate semantic profile | Signals/effects/resources/control flow and component semantics; prevents React-shaped kernel | Immediate React counterfixtures, JSX binding/event differences, SolidStart later | `NotApplicable` |
~~~~

### SRC-EXP-L1600-AD5BD7E9F2C8

- Kind: `context`
- Source: `successor-expansion.md:1600-1600`
- Applicability: `UAK1`
- Exact text SHA-256: `ad5bd7e9f2c8d7d12a13bc00ba501c06fa61298e6179f920df672eeacbfb8760`

~~~~markdown
| **Preact** | OXC/TypeScript TSX; separate native and `preact/compat` profiles/evidence | Low-cost reach after React while retaining real semantic differences | Compat resolution, signals where admitted, CE consumption; no claim that React support makes Preact automatic | `NotApplicable` |
~~~~

### SRC-EXP-L1601-3B106C004D31

- Kind: `requirement`
- Source: `successor-expansion.md:1601-1601`
- Applicability: `UAK1`
- Exact text SHA-256: `3b106c004d312149538c8f96f5611eefc2a00b63d07b0360b09b8afe1b061b93`

~~~~markdown
| **Qwik 2** | OXC/TypeScript TSX semantic overlay for one exact Qwik 2 epoch only | Resumability/QRL/component semantics and underserved tooling if release stabilizes | QRLs, `$` boundaries, optimizer directives, serializability, Qwik City later; Qwik1 negative fixtures | `NotApplicable` |
~~~~

### SRC-EXP-L1602-240B889D8B4E

- Kind: `context`
- Source: `successor-expansion.md:1602-1602`
- Applicability: `UAK1`
- Exact text SHA-256: `240b889d8b4e09382c8aa8439ce9086e37c5fed176888faab6222c0ebdd165e6`

~~~~markdown
| **Stencil** | OXC/TypeScript TSX semantic overlay plus CE producer | Strong standards interoperability and component metadata across consumers | Decorators, props/state/events/methods/watchers, generated declarations/CEM as oracle inputs, CE production | `NotApplicable` |
~~~~

### SRC-EXP-L1603-CB1400F4EEDC

- Kind: `requirement`
- Source: `successor-expansion.md:1603-1603`
- Applicability: `UAK1`
- Exact text SHA-256: `cb1400f4eedc16352a25c513aa5f6c759df19511a087258be1e0c1a85b080564`

~~~~markdown
| **Angular** | Neutral HTML plus TS host roles; external and embedded attachment profiles | Large ecosystem, exact template TypeInfo, metadata, lint/actions, cross-framework CE consumption | Decorators, standalone/modules, signals, directives/pipes, microsyntax/control flow, project association, Angular Elements separately | `NotApplicable` |
~~~~

### SRC-EXP-L1604-686A563E3C6F

- Kind: `context`
- Source: `successor-expansion.md:1604-1604`
- Applicability: `UAK1`
- Exact text SHA-256: `686a563e3c6fc9ffca098e58919b26af739f9af961bfaa655c72cd2ea143d0e9`

~~~~markdown
| **Alpine** | Neutral HTML with attribute-level claims and embedded JS expressions | High marginal DX: scope-aware definition/rename/hover/completion/diagnostics | `x-data` descendant scopes, refs, loops, modifiers, dynamic uncertainty, CE consumption | `NotApplicable` |
~~~~

### SRC-EXP-L1605-EC4E57135127

- Kind: `context`
- Source: `successor-expansion.md:1605-1605`
- Applicability: `UAK1`
- Exact text SHA-256: `ec4e57135127e178d48346d0237d034c5be45684b609aa7519c084d090f5d988`

~~~~markdown
| **HTMX** | Neutral HTML with attribute-level request/target/trigger/swap claims | Low-cost navigation from selectors and captured project routes; underserved HTML-first DX | attribute inheritance, selectors, extensions, route metadata as optional project input, CE consumption | `NotApplicable` |
~~~~

### SRC-EXP-L1606-05398680787C

- Kind: `requirement`
- Source: `successor-expansion.md:1606-1606`
- Applicability: `UAK1`
- Exact text SHA-256: `05398680787c5667528a2f1a1e5b9404cbcbfbb8ed480e8dc46d46eec2424eb2`

~~~~markdown
| **Marko** | Dedicated `.marko` carrier/parser | Native performance, unified metadata/index/public surfaces; cross-framework intelligence beyond incumbent tooling | tag resolution, params/attributes/events, control flow, style/script, exact recovery/maps | `NotApplicable` |
~~~~

### SRC-EXP-L1607-FE71D1E900E1

- Kind: `requirement`
- Source: `successor-expansion.md:1607-1607`
- Applicability: `UAK1`
- Exact text SHA-256: `fe71d1e900e1ab0757e5054f857c8a05e9a0cfd3568c05307f962a562a148925`

~~~~markdown
| **Ember/Glimmer** | Dedicated `.gjs/.gts`/template-tag or attached template geometry per exact Glimmer/Ember epoch | Framework-aware TypeInfo, component metadata, layout/resolution, cross-project graph | strict/classic modes, helpers/modifiers, named/positional args, Ember resolver/layouts; CE consumption | `NotApplicable` |
~~~~

### SRC-EXP-L1608-B9AAF9FE81C1

- Kind: `context`
- Source: `successor-expansion.md:1608-1608`
- Applicability: `UAK1`
- Exact text SHA-256: `b9aaf9fe81c1bf926e90408ceb55c4b10847b1dc4ecf138edf710552fc0b94a1`

~~~~markdown
| **HTML** | Independent neutral parser initially forked from Vue and de-Vue’d | Standards source tooling and substrate for Angular/Alpine/HTMX/WC | standards recovery, a11y, links/assets/selectors, formatter, full applicable LSP | `NotApplicable` |
~~~~

### SRC-EXP-L1609-D544A865DF58

- Kind: `context`
- Source: `successor-expansion.md:1609-1609`
- Applicability: `UAK1`
- Exact text SHA-256: `d544a865df581953677ef91d9a08da042647f561094a85789105b7f34af4e34e`

~~~~markdown
| **Web Components** | Standards semantic/interoperability facet, not a source carrier | Cross-framework props/attrs/events/slots/methods/parts intelligence and CEM | scoped registries, declaration/registration/consumer distinction, producer/consumer dispositions | `NotApplicable` |
~~~~

### SRC-EXP-L1611-C569FE839A71

- Kind: `requirement`
- Source: `successor-expansion.md:1611-1611`
- Applicability: `UAK1`
- Exact text SHA-256: `c569fe839a71607517af01402f1e16a86d136d1646964d348ee3027ab521f75b`

~~~~markdown
The user’s “htmlx” item is interpreted as **HTMX**, consistent with the supplied examples and `hx-*` semantics. If a separate language named HTMLX was intended, it requires its own feasibility dossier rather than silently sharing the HTMX profile.
~~~~

### SRC-EXP-L1613-4F619EDF2AAD

- Kind: `context`
- Source: `successor-expansion.md:1613-1613`
- Applicability: `UAK1`
- Exact text SHA-256: `4f619edf2aadf2cafd1f334182ac6d5c1c3cdc784a7ecf2d062152333ef0b13b`

~~~~markdown
### 15.3 Astro first-class commitment
~~~~

### SRC-EXP-L1615-3D5985CF2C1D

- Kind: `requirement`
- Source: `successor-expansion.md:1615-1615`
- Applicability: `UAK1`
- Exact text SHA-256: `3d5985cf2c1dd9889578f925dfdeb7071d561018393ea53676e47c0506ecbf23`

~~~~markdown
Astro is not an adapter around the official language server and is not blocked on a compiler. Its eventual full vertical must own:
~~~~

### SRC-EXP-L1617-8A407E033B54

- Kind: `requirement`
- Source: `successor-expansion.md:1617-1617`
- Applicability: `UAK1`
- Exact text SHA-256: `8a407e033b54a77107a16ff295f4623742bc73fdd5072c81b025555dc1529f92`

~~~~markdown
- Astro parsing/recovery and exact authored region/source-unit maps;
~~~~

### SRC-EXP-L1618-E8F46F3E0565

- Kind: `context`
- Source: `successor-expansion.md:1618-1618`
- Applicability: `UAK1`
- Exact text SHA-256: `e8f46f3e056540385621ed1bbabeed275bb278f2985e198b6a6242b5199b117a`

~~~~markdown
- frontmatter and all admitted embedded language composition;
~~~~

### SRC-EXP-L1619-43F894ACF3F9

- Kind: `context`
- Source: `successor-expansion.md:1619-1619`
- Applicability: `UAK1`
- Exact text SHA-256: `43f894acf3f901f6e585baa44eecf1aa6017f9bb95271331f44be4a67640fcea`

~~~~markdown
- component/directive/island/asset semantics;
~~~~

### SRC-EXP-L1620-6139E7474AB3

- Kind: `context`
- Source: `successor-expansion.md:1620-1620`
- Applicability: `UAK1`
- Exact text SHA-256: `6139e7474ab33f9d53b208b6c55543457371a90807d333ca64f04c1c2e0f0859`

~~~~markdown
- TypeInfo projections and public component information;
~~~~

### SRC-EXP-L1621-C1A720422F45

- Kind: `context`
- Source: `successor-expansion.md:1621-1621`
- Applicability: `UAK1`
- Exact text SHA-256: `c1a720422f450bcb29aa8bbafef309a6b9b15876729a450a2df30b33d5fc6846`

~~~~markdown
- diagnostics plus native Astro-specific lint rules and safe fixes/actions;
~~~~

### SRC-EXP-L1622-C7445A768017

- Kind: `context`
- Source: `successor-expansion.md:1622-1622`
- Applicability: `UAK1`
- Exact text SHA-256: `c7445a768017db0de4a1243f79c0c67d600c36f87137031d91aab509c8874f80`

~~~~markdown
- complete applicable IDE/LSP, including cross-framework island navigation, auto-import, rename, and file moves;
~~~~

### SRC-EXP-L1623-EFC47DC0D03B

- Kind: `context`
- Source: `successor-expansion.md:1623-1623`
- Applicability: `UAK1`
- Exact text SHA-256: `efc47dc0d03bae41b6fbc5bf38c32606a2dfc6175b6ff66e892cfad6e6eaf59d`

~~~~markdown
- native whole-document and range formatting;
~~~~

### SRC-EXP-L1624-A7AA0E2CCBCA

- Kind: `context`
- Source: `successor-expansion.md:1624-1624`
- Applicability: `UAK1`
- Exact text SHA-256: `a7aa0e2ccbcade80068db2172048e2a12c2bda4046406f908f5330cc0842c87f`

~~~~markdown
- workspace graph/index contributions;
~~~~

### SRC-EXP-L1625-6D038ACFC802

- Kind: `context`
- Source: `successor-expansion.md:1625-1625`
- Applicability: `UAK1`
- Exact text SHA-256: `6d038acfc802f90c26e26475c01cb3dffd6241ca247791dd06b12dfb76ddf514`

~~~~markdown
- Rust, NAPI, WASM, MCP, LSP, and CLI surfaces with capability truth;
~~~~

### SRC-EXP-L1626-E25E3EB46D0E

- Kind: `context`
- Source: `successor-expansion.md:1626-1626`
- Applicability: `UAK1`
- Exact text SHA-256: `e25e3eb46d0ebfa22f6ddfc68d994be13eff886971704bbe2e2a9a2bc937349a`

~~~~markdown
- cancellation, incremental=fresh, Unicode/map, performance, RSS, security, and conformance evidence.
~~~~

### SRC-EXP-L1628-2FC51E27153A

- Kind: `context`
- Source: `successor-expansion.md:1628-1628`
- Applicability: `UAK1`
- Exact text SHA-256: `2fc51e27153ae3a4d824858ee6dc90f56c9a53de2984e0059705ed9e7daf566f`

~~~~markdown
A future Astro compiler train may investigate an owned Rust frontend/code generator or an `@astrojs/compiler-rs`-like product. That train has its own oracle, ABI, runtime-output scope, and terminal. Astro tooling neither waits for it nor claims it.
~~~~

### SRC-EXP-L1630-0E89908529D9

- Kind: `context`
- Source: `successor-expansion.md:1630-1630`
- Applicability: `UAK1`
- Exact text SHA-256: `0e89908529d98cc03da3bc9be377f3181bfc5fce980453d177af4a38c3c9c900`

~~~~markdown
### 15.4 MDX first-wedge commitment
~~~~

### SRC-EXP-L1632-AE2F89FB8F47

- Kind: `requirement`
- Source: `successor-expansion.md:1632-1632`
- Applicability: `UAK1`
- Exact text SHA-256: `ae2f89fb8f478b30a548a4302cb7df7054f7cf9bc22a8961d3c4a7d6e7ed5884`

~~~~markdown
The MDX vertical is intended to replace the Volar-based integration path, while respecting existing MDX syntax/type semantics as oracles rather than importing Volar as Verter architecture. Its full release must specifically close:
~~~~

### SRC-EXP-L1634-7833D6F52835

- Kind: `context`
- Source: `successor-expansion.md:1634-1634`
- Applicability: `UAK1`
- Exact text SHA-256: `7833d6f528354bb10520790041eac9c398e21b94af9e3324a9e197133c97271e`

~~~~markdown
- React-component discovery and auto-import intelligence;
~~~~

### SRC-EXP-L1635-F90DD96B3B9F

- Kind: `context`
- Source: `successor-expansion.md:1635-1635`
- Applicability: `UAK1`
- Exact text SHA-256: `f90dd96b3b9fdbe52c31d23e45a6e34795cd313793f4c93a8da0e6ac73b38e33`

~~~~markdown
- Markdown/MDX path and link completion;
~~~~

### SRC-EXP-L1636-E7D4A5C94259

- Kind: `context`
- Source: `successor-expansion.md:1636-1636`
- Applicability: `UAK1`
- Exact text SHA-256: `e7d4a5c942599bf53cbb04c735ad8b119dfa8072d8f3603f4e1b0e4022d76437`

~~~~markdown
- exact, atomic link updates on file moves;
~~~~

### SRC-EXP-L1637-8A7FBA888912

- Kind: `context`
- Source: `successor-expansion.md:1637-1637`
- Applicability: `UAK1`
- Exact text SHA-256: `8a7fba888912d4347d71e758ebf408ed517da215a30afbb06a89112be379d472`

~~~~markdown
- component/link/heading-aware refactoring;
~~~~

### SRC-EXP-L1638-541BD4A963B1

- Kind: `context`
- Source: `successor-expansion.md:1638-1638`
- Applicability: `UAK1`
- Exact text SHA-256: `541bd4a963b161296c10d2c2e566f6d1eee2530982900a56835035639e6e9784`

~~~~markdown
- measurable high-CPU and long-session memory regressions;
~~~~

### SRC-EXP-L1639-5D4AB647DDC7

- Kind: `context`
- Source: `successor-expansion.md:1639-1639`
- Applicability: `UAK1`
- Exact text SHA-256: `5d4ab647ddc776ed86a3b443e0828fe9879ffba927d681a1439f8d0f9b8713a1`

~~~~markdown
- TypeInfo/component metadata for MDX exports/props/provided components;
~~~~

### SRC-EXP-L1640-D270F6E46E9E

- Kind: `context`
- Source: `successor-expansion.md:1640-1640`
- Applicability: `UAK1`
- Exact text SHA-256: `d270f6e46e9ed551f29a0437fca39451dd05b047535824b0b7f3900c3b63559e`

~~~~markdown
- reusable Markdown/MDX embedded-content service for admitted Vue custom blocks.
~~~~

### SRC-EXP-L1642-1C342461EC37

- Kind: `forbidden`
- Source: `successor-expansion.md:1642-1642`
- Applicability: `UAK1`
- Exact text SHA-256: `1c342461ec37a48f56420489672ce8fe27811c083a1c9bc21ebc07873116ef92`

~~~~markdown
`MDXR0` is evidence only. Before full MDX can advertise React-specific discovery/auto-import, the future vertical program must ratify a bounded production train: `RCP0-FUTURE` locks one exact React release, provider API, oracle, maturity, zero-work, and performance gates; `RCP1-FUTURE` implements/promotes the React `ComponentInfo` provider over accepted React facts; `RCP2-FUTURE` migrates or deletes proof code and passes public/index/performance conformance. The full MDX terminal depends on `RCP2-FUTURE`, never on `RCTP` or `MDXR0`. Generic MDX links, moves, refactors, and framework-neutral component candidates remain independently useful.
~~~~

### SRC-EXP-L1644-6B83F96A73DD

- Kind: `requirement`
- Source: `successor-expansion.md:1644-1644`
- Applicability: `UAK1`
- Exact text SHA-256: `6b83f96a73dd7f96d8fc38ecbb3cd8a32cfc2d5317ae3e3a623bd9583e9ef833`

~~~~markdown
Arbitrary remark/rehype plugin execution remains outside Rust/WASM. Static captured configuration may select admitted syntax extensions; unsupported executable transforms return `NeedInputs`/`Unsupported` or run only behind a separately trusted host contract.
~~~~

### SRC-EXP-L1646-E173F1B1F2B0

- Kind: `context`
- Source: `successor-expansion.md:1646-1646`
- Applicability: `UAK1`
- Exact text SHA-256: `e173f1b1f2b00fb0a5ba05a98bc0138d41a5143e8ea9f285cd32f17a9efe872d`

~~~~markdown
### 15.5 Project-profile roadmap
~~~~

### SRC-EXP-L1648-D7B53496C732

- Kind: `forbidden`
- Source: `successor-expansion.md:1648-1648`
- Applicability: `UAK1`
- Exact text SHA-256: `d7b53496c732ac6bfef43743412dd88e59da1ffa7a5bf3b040e47d69fb22c2ea`

~~~~markdown
Project profiles are semantic overlays over already-resolved carrier/framework facts and captured project structure. They never become TypeScript project owners.
~~~~

### SRC-EXP-L1650-3F69323A855B

- Kind: `requirement`
- Source: `successor-expansion.md:1650-1650`
- Applicability: `UAK1`
- Exact text SHA-256: `3f69323a855bc648521bf65a9e1e3d7dbee5c2b0bfa2904b0308f95adf834e62`

~~~~markdown
Only Next is selected as an implementation candidate. Every other row below is explicitly deferred and unordered; row position carries no priority. Scored profiles show the dated §6 hypothesis, while `unscored` means a feasibility lock must produce evidence before the profile may enter any investment order.
~~~~

### SRC-EXP-L1652-5F920926D38B

- Kind: `requirement`
- Source: `successor-expansion.md:1652-1652`
- Applicability: `UAK1`
- Exact text SHA-256: `5f920926d38bbf01d46593c19dfa32068f38034a8737cb2e078594856b7d2bc6`

~~~~markdown
| Decision | Profile | Score | Required semantic focus | Prerequisites / reason deferred |
~~~~

### SRC-EXP-L1653-71A052F34971

- Kind: `context`
- Source: `successor-expansion.md:1653-1653`
- Applicability: `UAK1`
- Exact text SHA-256: `71a052f349718c7d91eabe711aa366c3b7596d5b44cf4a66e3628000e1a0c64a`

~~~~markdown
|---|---|---:|---|---|
~~~~

### SRC-EXP-L1654-4C87868016B2

- Kind: `context`
- Source: `successor-expansion.md:1654-1654`
- Applicability: `UAK1`
- Exact text SHA-256: `4c87868016b21ff82a1ca722cfa5244294774ced419e43dfb17bb701f438a680`

~~~~markdown
| First candidate | Next.js | 4.2 | App Router file roles, layouts/pages/loading/error/metadata, RSC, client/server directives, Server Functions, route/cache/rendering semantics | React + MDX full prerequisites; generic project identity/index |
~~~~

### SRC-EXP-L1655-BC16613EA574

- Kind: `context`
- Source: `successor-expansion.md:1655-1655`
- Applicability: `UAK1`
- Exact text SHA-256: `bc16613ea574bab6bbb772b3fd2503ea026c7bcad77c0798dded623759864a47`

~~~~markdown
| Deferred; counterfixture first | Nuxt 4 | 3.3 | pages/layouts/plugins/middleware/server routes, auto-imports, client/server boundaries, Vue/Nitro associations | Vue accepted; challenge generic project vocabulary before any implementation rank |
~~~~

### SRC-EXP-L1656-4F0FBD59DF2F

- Kind: `context`
- Source: `successor-expansion.md:1656-1656`
- Applicability: `UAK1`
- Exact text SHA-256: `4f0fbd59df2f4bfb5ba9b2393afa9e541dfc185ce16d607f9f5f7077f549d5bf`

~~~~markdown
| Deferred; counterfixture first | SvelteKit | 3.1 | routes/layouts/load/actions/hooks, universal/server files, Svelte associations | Svelte accepted; challenge generic project vocabulary before any implementation rank |
~~~~

### SRC-EXP-L1657-D319192A5FD9

- Kind: `context`
- Source: `successor-expansion.md:1657-1657`
- Applicability: `UAK1`
- Exact text SHA-256: `d319192a5fd9effe02691d02dc499e4259f9bf21c70683003cff1a159dc0196f`

~~~~markdown
| Deferred; unranked | Astro project | unscored | file-based routes/endpoints, layouts, content collections, integrations, assets, islands and source-observable build-mode facts | full Astro tooling vertical plus independent feasibility/score lock |
~~~~

### SRC-EXP-L1658-A74BFA1F9C9E

- Kind: `requirement`
- Source: `successor-expansion.md:1658-1658`
- Applicability: `UAK1`
- Exact text SHA-256: `a74bfa1f9c9e900c520332e346fe0c05d1bc477b9c51406ae6ea281d440d7886`

~~~~markdown
| Deferred; unranked | Angular workspace | unscored | projects/configurations, routes/lazy boundaries, templates/styles/assets, build targets and library/app relationships that are source/config-observable | full Angular semantic vertical plus exact project/config epoch and feasibility/score lock |
~~~~

### SRC-EXP-L1659-FCE8A5A15912

- Kind: `requirement`
- Source: `successor-expansion.md:1659-1659`
- Applicability: `UAK1`
- Exact text SHA-256: `fce8a5a15912a40a016190eab2c757f3a386007d8d8b86cb6131508a00b6535d`

~~~~markdown
| Deferred; unranked | React Router | unscored | route modules, loaders/actions, server/client boundaries, framework-mode conventions | exact release and React prerequisite plus independent feasibility/score lock |
~~~~

### SRC-EXP-L1660-8EBEBCB68BA6

- Kind: `requirement`
- Source: `successor-expansion.md:1660-1660`
- Applicability: `UAK1`
- Exact text SHA-256: `8ebebcb68ba6180f66ad5a5a2bb4711f7014cdfb47ac0803fec1e8705f83ccaa`

~~~~markdown
| Deferred; unranked | Remix | unscored | route/file conventions, loaders/actions, server/client data and deployment-visible source semantics | exact release/lineage decision and React prerequisite plus independent feasibility/score lock |
~~~~

### SRC-EXP-L1661-27836FACAC96

- Kind: `context`
- Source: `successor-expansion.md:1661-1661`
- Applicability: `UAK1`
- Exact text SHA-256: `27836facac96168c8f896ea9c2381d95b1babffa91a1da439063a500a6b29e0b`

~~~~markdown
| Deferred; unranked | SolidStart | unscored | routing, server functions, islands/hydration, data/cache boundaries | Solid vertical plus independent feasibility/score lock |
~~~~

### SRC-EXP-L1662-A255343860DC

- Kind: `requirement`
- Source: `successor-expansion.md:1662-1662`
- Applicability: `UAK1`
- Exact text SHA-256: `a255343860dc7f01492915df1a56cdb5f8a7b23a152b58c5c052daf2a41c1d58`

~~~~markdown
| Deferred; blocked | Qwik City | unscored | routes/layouts/loaders/actions, resumability boundaries and source-observable optimizer facts | accepted exact Qwik 2 profile first; Qwik 1 remains excluded |
~~~~

### SRC-EXP-L1663-0D4FDDA88B3C

- Kind: `requirement`
- Source: `successor-expansion.md:1663-1663`
- Applicability: `UAK1`
- Exact text SHA-256: `0d4fdda88b3c589ed3de451471fdafbde41dde8bc97dcad1420c0110fc0afefa`

~~~~markdown
| Deferred; unranked | TanStack Start | unscored | file/code routing, server functions, loaders/cache and client/server boundaries | exact stable product/release evidence plus independent feasibility/score lock |
~~~~

### SRC-EXP-L1664-4549834204AA

- Kind: `context`
- Source: `successor-expansion.md:1664-1664`
- Applicability: `UAK1`
- Exact text SHA-256: `4549834204aaaa5d47a88a2810bf2368cf2adf6c7618bf75eb4bfb6f945c4905`

~~~~markdown
| Deferred; unranked | Docusaurus | unscored | docs routes, MDX component environment, sidebars, links/assets and plugin-config facts | MDX vertical plus independently bounded static-config contract and feasibility/score lock |
~~~~

### SRC-EXP-L1665-7D23E670F14E

- Kind: `context`
- Source: `successor-expansion.md:1665-1665`
- Applicability: `UAK1`
- Exact text SHA-256: `7d23e670f14e7659e341fc594c9bd81d9e31eb8d04003e083a8a050df9f1fba0`

~~~~markdown
| Deferred; unranked | VitePress | unscored | Markdown/Vue content, routes, theme/components, links/assets and static config facts | Vue + Markdown/MDX substrate plus independent feasibility/score lock |
~~~~

### SRC-EXP-L1667-719A6418ABCD

- Kind: `requirement`
- Source: `successor-expansion.md:1667-1667`
- Applicability: `UAK1`
- Exact text SHA-256: `719a6418abcd2c3fc52a8f8a6211301eee6ca961a577ce05f66a16c89dfe919e`

~~~~markdown
Next is the intended first implementation because it combines reach and high-value semantics TypeScript does not know, not because it defines the generic schema. Before any project-profile contract becomes Stable, Nuxt and SvelteKit adversarial fixtures must demonstrate that route/module/client-server vocabulary is not merely Next renamed. Promotion of Next does not automatically rank any deferred row.
~~~~

### SRC-EXP-L1703-C18AE0F4B2E5

- Kind: `context`
- Source: `successor-expansion.md:1703-1703`
- Applicability: `UAK1`
- Exact text SHA-256: `c18ae0f4b2e5d90615f2308df39b61cbf066ad599ddd4eba96a1fc2ea5213e18`

~~~~markdown
## 17. Non-active future HTML-family consolidation record
~~~~

### SRC-EXP-L1705-20895205E06D

- Kind: `context`
- Source: `successor-expansion.md:1705-1705`
- Applicability: `UAK1`
- Exact text SHA-256: `20895205e06d925f839826297dbbe9e67835c2d6e88e85fac36795c8740ca036`

~~~~markdown
`HFC-FUTURE` is a reserved investigation, not a promised refactor and not an active DAG predecessor:
~~~~

### SRC-EXP-L1707-6CBB942DC0FB

- Kind: `context`
- Source: `successor-expansion.md:1707-1707`
- Applicability: `UAK1`
- Exact text SHA-256: `6cbb942dc0fbc8dd46bbafff10fea9bfb922882179091eadb103970c4d4c8961`

~~~~markdown
1. `HFC0`: after at least three accepted HTML-family parser profiles, measure duplicated mechanics, fuzz/differential behavior, cache invalidation, allocation, latency, and license seams. It may conclude “keep independent.”
~~~~

### SRC-EXP-L1708-209617EA9DE5

- Kind: `requirement`
- Source: `successor-expansion.md:1708-1708`
- Applicability: `UAK1`
- Exact text SHA-256: `209617ea9de5fdfa8dd6249416ae4b4df6fec3ecbb171b9b2f9505afa56230e2`

~~~~markdown
2. `HFC1`: only if ratified, extract proven-neutral lexer/entity/span primitives without moving AST, grammar, recovery, or semantic authority.
~~~~

### SRC-EXP-L1709-F9538A4126CF

- Kind: `deletion`
- Source: `successor-expansion.md:1709-1709`
- Applicability: `UAK1`
- Exact text SHA-256: `f9538a4126cf2b19064e0ca32a01384491e6742bc68960c0d69a633f4c4a719b`

~~~~markdown
3. `HFC2.<profile>`: migrate one parser at a time with exact corpus, fuzz, map, performance, and rollback proof; delete replaced code in that slice.
~~~~

### SRC-EXP-L1710-1E62FB024350

- Kind: `requirement`
- Source: `successor-expansion.md:1710-1710`
- Applicability: `UAK1`
- Exact text SHA-256: `1e62fb024350f596df499edcdccec7e488b3e6c29f8aabf7015bc7eaa32652f8`

~~~~markdown
4. `HFCG`: read-only convergence review.
~~~~

### SRC-EXP-L1712-12E4EF8FC705

- Kind: `context`
- Source: `successor-expansion.md:1712-1712`
- Applicability: `UAK1`
- Exact text SHA-256: `12e4ef8fc705f1f437540df977e0e6f12eebd753b5f2a967527d0f56ad695964`

~~~~markdown
No current kernel/product/vertical terminal waits for this record.
~~~~

### SRC-EXP-L1716-0892C6F30B91

- Kind: `context`
- Source: `successor-expansion.md:1716-1716`
- Applicability: `UAK1`
- Exact text SHA-256: `0892c6f30b9101795b45d0987c23614c7a0620eee880dcaa084be42073f53598`

~~~~markdown
### 18.1 Primary external evidence
~~~~

### SRC-EXP-L1718-EC305CB6719E

- Kind: `context`
- Source: `successor-expansion.md:1718-1718`
- Applicability: `UAK1`
- Exact text SHA-256: `ec305cb6719e0b3dc11be76602de905fa00f42d3cf03e26998f2595b4cbaf2e8`

~~~~markdown
- [Verter repository](https://github.com/pikax/verter) — current public product and architecture context.
~~~~

### SRC-EXP-L1719-8E1A573120C8

- Kind: `context`
- Source: `successor-expansion.md:1719-1719`
- Applicability: `UAK1`
- Exact text SHA-256: `8e1a573120c8e622377f4921a9b150ccad2e44b68290c579e89c952b34a7db53`

~~~~markdown
- [State of JavaScript 2025 front-end frameworks](https://2025.stateofjs.com/en-US/libraries/front-end-frameworks/) — ecosystem reach and satisfaction signals; self-selected survey evidence only.
~~~~

### SRC-EXP-L1720-57241430B199

- Kind: `context`
- Source: `successor-expansion.md:1720-1720`
- Applicability: `UAK1`
- Exact text SHA-256: `57241430b199d6ddcb98c16e8dec7f82f88830dc98f0da88e00d2485962e3eb9`

~~~~markdown
- [State of JavaScript 2025 meta-frameworks](https://2025.stateofjs.com/en-US/libraries/meta-frameworks/) — project-profile prioritization evidence; self-selected survey evidence only.
~~~~

### SRC-EXP-L1721-F3FE3C329C02

- Kind: `context`
- Source: `successor-expansion.md:1721-1721`
- Applicability: `UAK1`
- Exact text SHA-256: `f3fe3c329c02cd4b6ef74fb5b85a5a3c23c0acc73cc2074420855185722f42c3`

~~~~markdown
- [MDX Analyzer](https://github.com/mdx-js/mdx-analyzer) — current Volar-based MDX language-service architecture and supported TypeScript integration.
~~~~

### SRC-EXP-L1722-DE224124D82D

- Kind: `requirement`
- Source: `successor-expansion.md:1722-1722`
- Applicability: `UAK1`
- Exact text SHA-256: `de224124d82d90c99da4e89c7840e79b87731e7ce2a20479e697d3de102395df`

~~~~markdown
- [Astro editor setup](https://docs.astro.build/en/editor-setup/) — current incumbent editor/LSP capabilities that Verter must exceed, not merely match.
~~~~

### SRC-EXP-L1723-8A4AEB045542

- Kind: `context`
- Source: `successor-expansion.md:1723-1723`
- Applicability: `UAK1`
- Exact text SHA-256: `8a4aeb0455421d8f26cc81cd2cb3675d84fe0574c3d8ff6e15a7f8a3994e1b76`

~~~~markdown
- [Angular Language Service](https://angular.dev/tools/language-service) — incumbent template capabilities and marginal-DX baseline.
~~~~

### SRC-EXP-L1724-E43EB4854037

- Kind: `context`
- Source: `successor-expansion.md:1724-1724`
- Applicability: `UAK1`
- Exact text SHA-256: `e43eb48540372413f8094cbbaa400da6edc1b9684b6a08703987837cb1616bdb`

~~~~markdown
- [Custom Elements Manifest](https://github.com/webcomponents/custom-elements-manifest) — interchange schema, not internal semantic authority.
~~~~

### SRC-EXP-L1725-721C508467A4

- Kind: `context`
- Source: `successor-expansion.md:1725-1725`
- Applicability: `UAK1`
- Exact text SHA-256: `721c508467a4b24e984bfdcf35a53782580f1e15bdd5a59b64349d63142801d4`

~~~~markdown
- [Vue custom elements guide](https://vuejs.org/guide/extras/web-components.html) — Vue producer/consumer behavior oracle input.
~~~~

### SRC-EXP-L1726-D1929F6E579F

- Kind: `context`
- Source: `successor-expansion.md:1726-1726`
- Applicability: `UAK1`
- Exact text SHA-256: `d1929f6e579f6b64e98661d63f38fb8a50573954c35b4e9cedfaf5132639cc42`

~~~~markdown
- [Alpine `x-data`](https://alpinejs.dev/directives/data) — nested scope semantics.
~~~~

### SRC-EXP-L1727-429BC9DDD692

- Kind: `context`
- Source: `successor-expansion.md:1727-1727`
- Applicability: `UAK1`
- Exact text SHA-256: `429bc9ddd692e11e6ae87d1aec7a092a38f22131984e06fe1cf9432971c1cc41`

~~~~markdown
- [HTMX `hx-target`](https://htmx.org/attributes/hx-target/) — selector/inheritance semantics.
~~~~

### SRC-EXP-L1728-18AEF85479BF

- Kind: `context`
- Source: `successor-expansion.md:1728-1728`
- Applicability: `UAK1`
- Exact text SHA-256: `18aef85479bf609f5d790b1cdb60db7ee1650b02ada7e212c72e33a803ef77c4`

~~~~markdown
- [Qwik releases](https://github.com/QwikDev/qwik/releases) — Qwik 2 remains a prerelease line at this proposal date, so the vertical is blocked rather than weakened to Qwik 1.
~~~~

### SRC-EXP-L1743-32A15873BDAD

- Kind: `context`
- Source: `successor-expansion.md:1743-1743`
- Applicability: `UAK1`
- Exact text SHA-256: `32a15873bdad181c4f31ed1695c60290025590752e54bcaae2e157f62f9e4e6e`

~~~~markdown
### 18.3 Candid risks
~~~~

### SRC-EXP-L1745-6C3C4A07A0B5

- Kind: `requirement`
- Source: `successor-expansion.md:1745-1745`
- Applicability: `UAK1`
- Exact text SHA-256: `6c3c4a07a0b584676412bbbd0605370616f1f6f45229b1f3f29eb9c5222af2a7`

~~~~markdown
- “Universal frontend tooling” is credible only after cross-framework operations outperform or materially complement incumbents. A large capability table alone has no market value.
~~~~

### SRC-EXP-L1746-6C49C2DB3E76

- Kind: `context`
- Source: `successor-expansion.md:1746-1746`
- Applicability: `UAK1`
- Exact text SHA-256: `6c49c2db3e7615a05af93924fead2b68df73f4e4b1e3dce7c454ea136b60cd2f`

~~~~markdown
- HTML parser reuse is not free. Standards recovery, namespaces, entities, accessibility, maps, and formatting are substantial; the initial fork is an architectural choice, not an effort estimate.
~~~~

### SRC-EXP-L1747-7708F5744F5A

- Kind: `requirement`
- Source: `successor-expansion.md:1747-1747`
- Applicability: `UAK1`
- Exact text SHA-256: `7708f5744f5a45c7904226a280feb794a1c75c2ffcf974b9de0e6b210432a298`

~~~~markdown
- React has enormous reach but excellent basic TSX tooling. Verter’s differentiation must be semantic graph, component intelligence, React Compiler rules, cross-framework use, performance, and later Next semantics.
~~~~

### SRC-EXP-L1748-BB88C677FA31

- Kind: `context`
- Source: `successor-expansion.md:1748-1748`
- Applicability: `UAK1`
- Exact text SHA-256: `bb88c677fa31ff6cbd0d1a6d5848a9473edabcc5e488a2dad9c6c85ca6c830b9`

~~~~markdown
- Angular and Astro already have capable language tooling. Reaching parity is insufficient; Verter needs measurable integration, metadata, lint, cross-framework, public API, or performance wins.
~~~~

### SRC-EXP-L1749-202190D09045

- Kind: `context`
- Source: `successor-expansion.md:1749-1749`
- Applicability: `UAK1`
- Exact text SHA-256: `202190d090452611b2377ef8fde943643bcc5573bcd6dcb8e91ff3b0318aa8ce`

~~~~markdown
- Alpine and HTMX are smaller ecosystems but offer unusually favorable marginal-DX-to-effort ratios.
~~~~

### SRC-EXP-L1750-FDE78ACF7082

- Kind: `context`
- Source: `successor-expansion.md:1750-1750`
- Applicability: `UAK1`
- Exact text SHA-256: `fde78acf70823cefb3a50e4400b48e4a64729dad00c5c5b2624393be1a5a0f21`

~~~~markdown
- MDX is the best first product wedge, but link/file-move graphs and plugin ecosystems can become unbounded. The locked index and no-plugin-core rules are essential.
~~~~

### SRC-EXP-L1751-C9B3772FCB39

- Kind: `context`
- Source: `successor-expansion.md:1751-1751`
- Applicability: `UAK1`
- Exact text SHA-256: `c9b3772fcb392f1c0e2e3c780dc6475056ac30350fed1d7bbe25ed1d76ce2125`

~~~~markdown
- Static Custom Element registry reachability is sometimes unknowable. Typed ambiguity is correct; a fabricated global answer is not.
~~~~

### SRC-EXP-L1752-F49FFE09C8E4

- Kind: `requirement`
- Source: `successor-expansion.md:1752-1752`
- Applicability: `UAK1`
- Exact text SHA-256: `f49ffe09c8e4f73cf4c9b29a030f4a6cee7e9d77aab34ac29c23ee63772d0189`

~~~~markdown
- A framework release can move faster than Verter’s oracle. One exact supported release per vertical makes this visible and intentionally trades breadth for correctness.
~~~~

### SRC-EXP-L1753-72391A907ED4

- Kind: `context`
- Source: `successor-expansion.md:1753-1753`
- Applicability: `UAK1`
- Exact text SHA-256: `72391a907ed4771462ef4577db7a92a1a020ff8612d34f13dfa23df00db24690`

~~~~markdown
- Rust does not automatically make the system fast. Allocation, cloning, map composition, backend/process lifecycle, workspace invalidation, and overly broad demand plans can erase the advantage; every claim remains measured.
~~~~
