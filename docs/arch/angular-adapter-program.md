# Angular Adapter — Follow-Up Program (extracted from the framework-adapter program)

**Status: NOT in any executing program. This program runs only after an explicit GO decision
(§3).** It was extracted (decision D-ac of
`docs/arch/multi-framework-adapters-plan.md`) from the "framework adapter substrate + Svelte
proof" program because scope-aware Template-Check-Block (TCB) generation — compilation-scope
database + template IR + sidecar virtual file + strict-template semantics — is a program-sized
effort that deserves its own go/no-go after the Svelte vertical (and the Astro reassessment)
prove the registry model. Nothing here weakens or reinterprets the substrate program's review
resolutions; this document carries the full former-B10/B11 designs unchanged in substance.

This document is self-contained FOR EXECUTION: §2 restates the full program-level invariant set
and §9 inlines every consumed substrate seam contract, so an architect can pick it up cold with
only the live tree as reference. Substrate decision ids (D-*) appear as traceability tags only —
never as load-bearing references an executor must chase.

---

## 1. Context

### 1.1 Verter in one paragraph

Verter is a Vue compiler + LSP over ONE shared type-resolution engine: `SemanticQueryKey` →
`ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five query modes
(`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`). Framework semantics are encoded as
ordinary synthesized TS value symbols resolved through the single shared
`Instantiate { canonical, "default", [] }` identity; adapters are
`shared_resolve(one type) + thin normalise`, never resolvers. All generated-code mutation goes
through `CodeTransform` (sourcemap integrity). Caches follow the fact-based architecture
(R1–R31): five split env hashes, content-addressed vs query-identity key families,
`ReadSetSignature.facts` validation, typed `SignatureAdmission`, `ReturnOnly` never publishes.

### 1.2 What the substrate program lands (this program's foundation)

The "framework adapter substrate + Svelte proof" program
(`docs/arch/multi-framework-adapters-plan.md`) lands, in execution scope:

- **B1 wire completion**: `TypeInfoGraphResponse.framework_surface` response arm; per-kind
  `FrameworkSurfaceKindStatus` (SUPPORTED/UNSUPPORTED/PARTIAL + `GraphExactness` + diagnostics);
  schema 2→3 with an operation-minimum gate; the D-aa tag-semantics doc comment ("a
  `FrameworkTag` value's existence is NOT a support guarantee; support is asserted only by a
  registered adapter"). **`FRAMEWORK_TAG_ANGULAR` is NOT added there — it lands in THIS
  Angular program (§4 A0), paired with its deferred TAG row.**
- **B2 routing**: leaf crate `verter_language` (`FileLanguage { Script, Framework,
  FrameworkTemplate { adapter_id, owner_hint } }`, `LanguageRegistry::classify_static`),
  `HostLanguageClassifier` + `ProjectCapabilitySnapshot` (the project-gated classification
  MECHANISM, empty snapshot), the per-file `file_language_id` column on the `FileArtifactStore`
  key; B2 also proves the row-without-carrier typed `UnsupportedLanguage` intermediate state
  (the `.svelte` row paired with its FFI accepted string — the pairing rule A2 reuses, D-ao).
  **The `.html` gated-candidate row and the first capability bit do NOT land in the substrate
  program — they land in THIS Angular program (capability bit: A1; `.html` row: A2); the
  substrate ships only the gated-classification mechanism with an empty snapshot.**
- **B4 parse artifact**: `FrameworkParseArtifact` with typed `FrameworkParseCommon`
  (`ScriptRegion { span, source_type, kind }`), token-gated carrier downcast confined to the
  owning adapter.
- **B5 registry + executor**: `FrameworkAdapterRegistry`, `FrameworkAdapterDescriptor`
  (including the `VirtualFileNaming` column with `testing_api_suffix` + `sidecar_suffixes`,
  D-x/D-al), the CLOSED-surface `FrameworkAdapterCtx` (D-am/D-as — carrier metadata + validated
  facts ONLY: `carrier_for` + `script_facts_for`; every resolve operation is EXECUTOR-private,
  driven from the adapter's declarative `PlannedDemand` plan; no `ensure_indexed_ready`, no
  raw/eval source), the two-stage `ScriptFactProvider` seam with the exact-gate-indexed
  `ActiveProviderIndex` (D-o/D-z/D-an), the
  `ComponentDefaultSynth` seam (D-n), the generic `FrameworkSurfaceDtoStore` (D-p/D-y/D-aq),
  the `ComponentApiProjector` api-content seam (D-ak), the
  `GRAPH_OPERATION_FRAMEWORK_SURFACES` executor with validation-before-dispatch.
- **B6 compiler scaffold**: `CarrierCompiler` trait
  (parse/eval_source/compile_ide/template_data), compiler registry, known-bug manifest +
  bijection guard, corpus generator config table, sourcemap-e2e helpers.
- **B8a/B8b/B8c Svelte vertical**: the carrier proof — including the LSP single-virtual-file
  path (`VirtualFileNaming`-derived `ide`/`api` files) this program's sidecar work builds on.

### 1.3 Why Angular is worth its own program

`input()`/`output()`/`model()` are real, current Angular surface (signal-era APIs) alongside the
decorator generation (`@Input()`/`@Output()`); a serious Angular story must map BOTH onto the six
wire kinds. The dominant type-check value, however, is the template: child-component bindings
resolved against the component's COMPILATION SCOPE (standalone `imports` / NgModule
declarations), which requires selector-scope facts, a scope database, a real template IR, and a
TCB-style sidecar TSX per component — Angular's own compiler design, mirrored. That is the
program-sized part (consult verdict carried from the substrate program's D-w:
host-only/intrinsic-only template checking "is not a serious long-term architecture").

---

## 2. Program-Level Invariants (the FULL set, restated; violating any is a STOP)

These are the twelve program-level invariants of the framework-adapter substrate program,
restated IN FULL for this program (traceability: substrate plan §2). Each carries its
Angular-specific stress note where this program exercises it hardest.

1. **Exactly ONE type-resolution engine.** `SemanticQueryKey` →
   `ProjectSemanticDispatch::execute` → `SemanticGraphStore`, five modes
   (`Identity`/`Navigate`/`Shallow`/`Expanded`/`Skeleton`). Every framework adapter is
   `shared_resolve(one type) + thin normalise`. Adapters are NEVER handed
   `ProjectSemanticDispatch`, raw source, or ANY resolve method — "raw source" includes host
   artifact state carrying it (`IndexedReady.raw_source`/`eval_source`): `ensure_indexed_ready`
   is executor/session-private, the adapter ctx is the CLOSED op surface of §9 item 8 (carrier
   metadata + validated facts only), and resolution runs ONLY inside the central executor
   consuming the adapter's declarative `PlannedDemand` plan. Angular
   stress: the scope DB cold build, input/output payload types, pipe `transform` signatures, and
   every TCB type resolve through the shared dispatch via capability-scoped contexts; no Angular
   resolver, no decorator walker at query time. Any per-framework resolver, per-surface walker,
   or re-parse-and-resolve is a rule violation to delete.
2. **Vue compiler parse/codegen behavior untouched.** No edits to Vue parser/codegen semantics
   in `verter_parser`/`verter_compiler`; mechanical re-export line updates are allowed and
   flagged. Nothing in A0–A2 touches a Vue module; any shared-substrate fix this program needs
   lands in the substrate owner layer (§9 closing rule), byte-pinned where it borders Vue.
3. **Typeinfo core add-only under the closed wire rules.** Closed-enum discipline, field numbers
   never reused, additive audit, validation-before-execution, schema-version gates, byte-pinned
   TS bindings. This program performs exactly ONE wire change (A0 — an open-enum `FrameworkTag`
   value addition, explicitly NOT a schema bump; see A0's compatibility proof).
4. **Runtime codegen for non-Vue frameworks is OUT of scope.** Angular's own compiler remains
   the runtime authority; this program produces IDE TSX (the TCB sidecar) + analysis facts only;
   unsupported `CompileTarget` bits return typed `CompileUnsupported` diagnostics, never silent
   empties.
5. **CodeTransform is the sole mutation mechanism** for all generated output — here, the TCB
   sidecar TSX (sourcemap integrity; no string replace/regex splicing on built output).
6. **Typed-IR-only.** No text sniffing, no identifier-suffix classification, no
   synthesize-then-reparse. Decorator/signal detection is STRUCTURAL via resolved
   `@angular/core` identity (stage-2 validation, §9 item 2); never name sniffing, never
   `path.contains("node_modules")` — workspace classification through the resolver-owned
   predicates.
7. **Hermetic vendored fixtures** in all non-gated tests; third-party repos only behind the
   `external-corpus` feature; official-Angular-compiler oracle comparisons are feature-gated
   benches only.
8. **Every new CRITICAL rule lands with a registered guard** in `CRITICAL_RULE_GUARDS`
   (`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`) in the same change (the
   R6 meta-guard walks CLAUDE.md + skills and fails any prose-only CRITICAL rule).
9. **No phase/temporal vocabulary** in production code or final commit messages; landed code
   reads as final-state.
10. **Cache rules bind every new adapter cache**: content-addressed or query-identity keys per
    the R1–R31 two-family split; FULLY STRUCTURAL key components (a fixed-width digest used AS a
    key component is forbidden); `ReadSetSignature.facts` validation; typed `SignatureAdmission`;
    `ReturnOnly` never publishes. Angular stress: `AngularTemplateScopeDb` (content-free
    query-identity — R6) and both `ScriptFactProvider` cache slots (content-addressed); the DTO
    store this program's surface adapter writes through is CONTENT-ADDRESSED + fact-validated
    (§9 item 4) — the content-free vocabulary never applies to it.
11. **Shallow-by-default / shallow file processing invariants** apply verbatim: one parse + one
    shallow pass per content hash; Angular facts extracted during that ONE pass (stage 1 — no
    rescan); surfaces published shallow unless the consumer walks the path (the TCB demands
    types per binding, never eager component-surface flattening).
12. **TDD throughout**; pre-existing semantic/typeinfo gaps surfaced by Angular land as
    known-bug ledger rows in the shared `framework_known_bug_manifest.rs` (bijection-guarded,
    discriminating red bodies), never as in-program core changes. Deliberate exclusions are
    OUT-OF-SCOPE rows; the two lists never mix.

---

## 3. Go / No-Go

This program starts only when the owner records a GO against ALL of the following, reviewed
together (the substrate program's B12 exit report is the evidence carrier):

1. **Svelte vertical landed**: B8a/B8b/B8c green on the canonical gate — the registry, the
   two-stage `ScriptFactProvider` seam, the `FrameworkSurfaceDtoStore`, the synth seam, and the
   `VirtualFileNaming`-driven LSP single-virtual-file path are all proven by a real non-Vue
   carrier vertical.
2. **Astro reassessment outcome recorded**: the deferred-B9 evidence-gated reassessment (main
   plan, "Deferred Verticals") has been performed and its outcome — execute / re-scope / stay
   deferred — is recorded. Astro's outcome matters here regardless of direction: if Astro
   executed, its island matrix is direct evidence for cross-adapter recursion under the registry
   model; if it did not, the go/no-go must explicitly accept that Angular becomes the second
   carrier consumer of several seams (template IR discipline, prelude/sidecar typing) without
   that intermediate evidence.
3. **Seam evidence reviewed** (named list — each item is a section of the substrate exit
   report):
   - `ScriptFactProvider` two-stage cost + correctness data from the Svelte provider (stage-1
     capture cost in the one shallow pass; stage-2 validation hit rates; userland look-alike
     rejection in practice) — Angular's import-specifier-gated provider is the seam's stress
     case.
   - `script_fact_providers_zero_cost_on_miss` perf-bound results with a provider registered
     (pure-TS and Vue projects show no shallow/warm regression) — including the exact-gate
     `ActiveProviderIndex` behavior (§9 item 2): Angular's `"@angular/core"` specifier-gated
     provider is reachable only through its exact specifier key, so non-matching script files
     compute an EMPTY provider set pre-invocation.
   - `FrameworkSurfaceDtoStore` behavior with ≥2 adapters (fact validation, generation gating,
     per-adapter typed sub-maps).
   - LSP virtual-file experience from Svelte (`ide` + `api` files through `VirtualFileNaming`;
     ts-plugin generalized regexps) — the sidecar mechanism (A2) extends this and MUST NOT land
     before the single-virtual-file path is proven (carried rule, former "B8c before B11").
   - The gated-classification mechanism status: `ProjectCapabilitySnapshot` landed EMPTY in the
     substrate program — this program is its first real user. The go decision explicitly
     acknowledges the `.html` gated row + capability bit are exercised here for the first time
     (A1 contains the discriminating gate tests).
   - Known-bug ledger yield: any registered shared-resolver gaps that Angular fixtures are
     expected to hit (the go review walks the ledger for rows that would block A1/A2 value).
4. **Fresh Angular API/docs audit performed and recorded (REQUIRED, pre-A0)**: because this
   program executes LATER than its design date, the GO record must carry a fresh audit against
   the then-live official Angular documentation — the recorded Angular version, the audited docs
   scope (at minimum: signal `input()`/`input.required()`/`output()`/`model()` semantics and
   `OutputRef`/`OutputEmitterRef` shapes; decorator `@Input()`/`@Output()` metadata; template
   control flow `@if`/`@for`/`@switch` + `@let` + microsyntax status; `@defer` status; selector
   matching + standalone-default semantics; `templateUrl`/external-template behavior; the
   template type-check (TCB) semantics this program mirrors, incl. strict-template flags), and
   the DELTAS between the audited docs and this document's §4/§5 designs. Any delta UPDATES this
   document before A0 executes — the audit is a design-revalidation gate, not a formality; a GO
   recorded without the audit row is invalid (mirrors the substrate's rule that a docs audit
   precedes any scope claim).

**No-Go handling**: a NO outcome records the blocking evidence and the re-review trigger; this
document stays the design of record. Partial execution (A1 without A2) is a legitimate scoped GO
— A1 has standalone value (facts + surface through the six wire kinds) and A2 depends on A1.

---

## 4. Blocks

Block ids are A0/A1/A2 (this program's own sequence). A1 corresponds to the substrate plan's
former B10, A2 to former B11; design content is carried in full.

### A0 — Angular wire addition

**Context.** The substrate program deliberately did NOT add `FRAMEWORK_TAG_ANGULAR` (D-aa: a tag
lands only with its vertical; tag existence is not a support guarantee). This block adds it under
the typeinfo closed-contract rules.

**Changes.**
- `crates/verter_protocol/proto/verter/v1/typeinfo.proto`: `FrameworkTag` gains
  `FRAMEWORK_TAG_ANGULAR` at the next free value (verify against the live enum at landing time —
  values are add-only, never reused; the D-aa doc comment already governs its semantics).
- **Schema-version rule (explicit — substrate D-aj): a `FrameworkTag` VALUE addition does NOT
  bump `TYPEINFO_GRAPH_SCHEMA_VERSION`.** Rationale, pinned as the rule's compatibility proof:
  `FrameworkTag` is a proto3 OPEN enum — it is NOT one of the four closed `oneof` taxonomies
  whose variant additions the typeinfo wire contract's bump rule governs (`GraphTypeNode.kind`,
  `StructuredTypeExpression.kind`, `TypeInfoGraphRequest.payload`, `TypeInfoRequestError.kind`);
  proto3 enum value additions are decode-safe (an unknown value round-trips as its integer
  representation); and per the D-aa tag-semantics rule a tag value carries NO support contract,
  so no client behavioral contract changes. Consequences, all asserted by this block's tests:
  `TYPEINFO_GRAPH_SCHEMA_VERSION` stays at its then-live value;
  `SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS` and `MIN_TYPEINFO_GRAPH_SCHEMA_VERSION` are
  UNCHANGED; no validation or per-operation-minimum change; no new advertisement content in the
  `UnknownSchemaVersion` error payload's `server_supported_versions`. Downgrade behavior: the
  server emits `FRAMEWORK_TAG_ANGULAR` only from the registered Angular adapter's descriptor
  (A1); a client built against pre-A0 bindings decodes the value as the open-enum
  unknown-integer representation, round-trips it losslessly, and per D-aa must not read any tag
  value as a support signal — support is asserted per-request via `FrameworkSurfaceKindStatus`.
  (If a closed-taxonomy change ever becomes necessary for Angular, it is a separate
  schema-versioned wire decision under the closed-contract rules — not this block.)
- Regenerate Rust + TS bindings via the workspace `buf` + `oxfmt`; byte-pin test green.
- **Session registry tag mapping: register the explicit DEFERRED TAG ROW for
  `FRAMEWORK_TAG_ANGULAR` in the SAME change** (substrate D-aw): the substrate's
  `framework_registry_complete` guard asserts every `FrameworkTag` wire variant maps to a
  registered adapter OR an explicit registered deferred/out-of-scope TAG row (§9 item 9) — the
  real `"angular"` registration arrives only in A1, so without this row A0's own gate is
  structurally red. The row is the same vocabulary the substrate used for `SVELTE`/`REACT`/
  `SOLID` pre-registration; A1 SUPERSEDES it with the real adapter registration. This is a
  session-registry edit, not a wire change — the "no other wire change" rule below is untouched.
- No other wire change: the six `FrameworkSurfaceKind` values cover the Angular surfaces (§5);
  the per-kind status mechanism (substrate D-s) realizes typed-unsupported semantics.

**Tests (failing first).** Taxonomy parity extended for the new tag; byte-pin regen; existing
field numbers/tags unchanged (negative); request-validation suite green across the change WITH
the schema-version constants asserted UNCHANGED (the no-bump rule's discriminating pin — a
version bump or supported-set edit fails this block); decode-compat proof: a payload bearing
`FRAMEWORK_TAG_ANGULAR` decodes under the PRE-A0 generated bindings as the open-enum
unknown-value representation and round-trips byte-losslessly (the wire-compat half of the
no-bump rule); `framework_registry_complete` GREEN across the change with the
`FRAMEWORK_TAG_ANGULAR` deferred TAG row asserted present (DISCRIMINATING: the tag addition
without the row fails the guard — the registration is what makes A0 independently landable).

**Dependencies.** GO decision (§3). No code dependency beyond the landed substrate.

### A1 — Angular facts + surface (former B10)

**Context.** Angular's component unit is a decorated `.ts` class — no carrier for the component
file itself (`.ts` stays `FileLanguage::Script`). Facts are captured in the ONE shallow pass via
the substrate's two-stage `ScriptFactProvider` seam; the surface adapter maps the signal-era +
decorator APIs onto the six wire kinds. Templates and the TCB sidecar trail in A2.

**Changes.**
- NEW `crates/verter_semantic/src/analysis/framework_facts/angular.rs` (the two-stage
  `ScriptFactProvider` seam — NOT a carrier hook):
  - **STAGE 1 (syntax-candidate capture, inside the one shallow OXC pass)**: syntax gate =
    `ScriptFactSyntaxGate::ImportSpecifier("@angular/core")` — the closed exact-valued gate
    vocabulary of §9 item 2 (syntax-only — no capability read inside the OXC pass; the resolved
    `FileLanguage` row is the only host-derived input). The dispatcher's `ActiveProviderIndex`
    indexes this provider under its EXACT specifier key (`by_import_specifier`) — there is no
    per-`Script`-row provider list, so a TS file without the literal `@angular/core` specifier
    computes an EMPTY active provider set before any provider invocation: zero gate evaluations,
    zero provider work, zero allocation (the substrate's
    `script_fact_providers_zero_cost_on_miss` guard is re-asserted here with this provider
    registered — this provider is the seam's production stress case). Stage-1 capture records candidate decorator/signal shapes tied to their
    `(local_binding, raw_import_source)` provenance, stored content-addressed on the per-file
    artifact (`framework_script_candidates` slot, key `(canonical, content_hash, parse_env_hash,
    parser_version, file_language_id, provider_id, provider_version)` — nothing capability- or
    provider-registry-shaped in the global `parse_env_hash`).
  - **STAGE 2 (session-owned resolved-symbol validation, at fact-demand time)**: gates on the
    Angular capability bit AND validates that each candidate's import source RESOLVES to the
    real `@angular/core` package through the existing resolver (userland look-alikes rejected —
    structural, never name-string). Only then does it emit the fact set:
    `AngularComponentFact` (class slot, selector IR, standalone policy, `imports` refs, inline
    `template` string + span / `templateUrl` → `template_canonical` link,
    `@Input()`/`input()`/`input.required()`, `@Output()`/`output()`, `model()`, `exportAs`,
    schemas, public members), `AngularDirectiveFact`, `AngularPipeFact`, `AngularNgModuleFact`
    (declarations/imports/exports, schemas). Validated facts live on the stage-2 slot with
    sub-key `(provider_id, provider_version, consumed_capability_bits, project_identity,
    resolve_env_hash)` — NO `lib_env_hash` (substrate D-ah: stage 2 validates symbol
    identity/package provenance, resolve-env domain; R21 scoping rule; `ResolvedImportFacts`
    precedent — if a future stage-2 validation genuinely consults lib data, the dependence is
    recorded and the dim added THEN); publication only via `SignatureAdmission::Cacheable`.
- `ProjectCapabilitySnapshot` gains the Angular capability bit (`angular.json` present /
  `@angular/core` in the dependency graph / host-config flag) — the provider's stage-2 gate and
  the `.html` classification gate (A2). The snapshot hash is over DERIVED bits, keys the
  CLASSIFICATION cache only; a capability flip invalidates exactly the affected files'
  classification + stage-2 fact slots, never parse artifacts.
- The **`AngularTemplateScopeDb` is NOT built here** (substrate D-ar(iii)): its only consumer is
  A2's TCB, so the scope DB lands in A2 WITH that consumer — under the legitimate partial GO
  (A1 without A2) this block ships facts + surface only, no consumer-less query-identity DB. The
  A1 fact set (selector IR, standalone policy, `imports` refs, NgModule
  declarations/imports/exports) is deliberately complete enough that A2's scope DB cold build
  consumes facts only — no re-extraction.
- Inline-template parse (span-anchored) lands here as data; projection waits for A2.
- NEW `crates/verter_session/src/typeinfo/adapters/angular/`: PROPS = inputs; EMITS = outputs
  (`EventEmitter<T>`/`OutputRef<T>` payload type via one shared_resolve); MODEL = `model()`
  signals; SLOTS = `ng-content` select inventory (inline templates only until A2); EXPOSE =
  public class instance surface; OPTIONS = typed unsupported (decorator metadata is not an
  options surface in v1). Registry row `"angular"` (tag `FRAMEWORK_TAG_ANGULAR` from A0, no
  carrier, NO synth, NO api projector (§9 items 7-8 — the component class is a real TS value
  symbol and the owner `.ts` is served directly), script-fact provider + surface adapter) —
  this registration SUPERSEDES A0's deferred TAG row (the tag now maps to the registered
  adapter; `framework_registry_complete` asserted green across the flip).
  Unsupported kinds surface through the
  substrate's per-kind `FrameworkSurfaceKindStatus` — never silent empties.
- (The `"angular_template"` FFI accepted string does NOT land here — it lands in A2 together
  with the `.html` gated registry row, per the substrate's pairing rule: an accepted string
  lands together with its registry row, never ahead of it. Substrate D-aj.)

**Legacy Deletions.** None.

**Tests (failing first).**
1. `typeinfo_tests/angular_adapter.rs`: decorator + signal fixtures (both API generations),
   imported components, executor e2e.
2. Structural-detection negative (stage-2): a userland `input()` whose import source does NOT
   resolve to `@angular/core` is rejected at validation (DISCRIMINATING: a raw-specifier-only
   match would accept it). Gate negatives: a file with no `@angular/core` specifier runs zero
   stage-1 capture work; the same candidate-bearing file in a NON-Angular project (capability
   off) emits NO stage-2 facts and no Angular surface (candidates stay inert); flipping the
   capability ON invalidates exactly that file's stage-2 fact slot — parse artifacts and
   candidates are untouched (the per-file scoping, asserted).
3. Surface-adapter capability-state negative: the Angular surface adapter serves NO surface for
   a candidate-bearing file while the capability bit is off (rides the stage-2 gate of test 2 —
   asserted at the adapter/executor level too, not only at the fact slot). (The
   `AngularTemplateScopeDb` suite — scope resolution, fact invalidation, key content-freedom,
   capability flip-on/flip-off — moves to A2 with the DB itself, substrate D-ar(iii).)
4. Vendored fixtures `framework_corpus/angular/` + corpus generator config row (hermetic; no
   third-party repo dependency outside `external-corpus`-gated benches).
5. Known-bug rows (pre-existing shared-resolver failures only; deliberate exclusions are
   out-of-scope rows — the two lists never mix).
6. `script_fact_providers_zero_cost_on_miss` re-asserted with the Angular provider registered —
   incl. the exact-gate index assertions: a `Script` file WITHOUT the literal `@angular/core`
   specifier computes an EMPTY active provider set via exact-key misses (no gate evaluation, no
   provider invocation, no allocation); pure-TS + Vue projects: no shallow/warm regression.

**Verification.** The canonical gate (below).

**Dependencies.** A0; the landed substrate (registry, seams, stores).

### A2 — Angular templates + TCB sidecar (former B11)

**Context.** Angular template type-checking via a Type-Check-Block-style sidecar TSX (mirroring
Angular's own TCB design) — a genuinely new host shape: an ADDITIONAL virtual file per component.
V1 template scope = control flow + property/event bindings resolved against the component's
COMPILATION SCOPE (standalone `imports` / NgModule declarations via the
`AngularTemplateScopeDb` built in THIS block over A1's facts — child-component bindings are in
scope) + pipes; the exclusion table
below bounds the rest.

**Changes.**
- NEW query-identity **`AngularTemplateScopeDb`** under `ProjectTypeStore` (built HERE, with its
  only consumer — the TCB; substrate D-ar(iii)): key
  `{ owner_component_slot, consumed_capability_bits, project_identity, resolve_env_hash }` —
  the typed `consumed_capability_bits` dimension (substrate
  D-aj; same precedent as the stage-2 sub-key) closes the capability-state invalidation hole: a
  warm scope entry built with the Angular capability ON cannot be served to a capability-OFF
  view (the flipped bits structurally miss the key — no reliance on env hashes that a capability
  flip never changes). `type_env_hash` and `lib_env_hash` are NOT in the key (substrate D-aw,
  R21 scoping — a dim enters a key only when the value depends on it): the value stores
  selector/scope/pipe-NAME data built from validated stage-2 facts + structural
  import/declaration resolution — symbol-identity work in the resolve-env domain; pipe
  `transform` SIGNATURES resolve in the TCB through the shared semantic queries (which carry
  their own type/lib dims) and are never stored here. If a future scope-DB value genuinely
  stores type-meaning- or lib-dependent data, the dependence is recorded and the dim added THEN
  (the stage-2 sub-key's D-ah pattern). The key stays content-FREE (R6 — derived capability
  bits are env-shaped derived config, never content/version hashes; guard
  `angular_template_scope_db_key_content_free`); value = selector index, pipe map, schemas,
  diagnostics, `self_root_canonicals`, `ReadSetSignature.facts` (validated on every warm read —
  the cold build's stage-2 fact reads are recorded facts, so a base fact change ALSO invalidates
  independently of capability state); cold build resolves `@Component.imports` / NgModule
  declarations/imports/exports structurally through the shared resolver, consuming A1's
  validated facts only (no re-extraction). This is the content-free QUERY-IDENTITY family —
  distinct from the content-addressed fact slots and DTO store (§2 invariant 10).
- NEW `crates/verter_compiler/src/angular/template/{parser.rs, ast.rs}` producing the **Angular
  Template IR**: `Root | Element | Template | Text | Interpolation | IfBlock | ForBlock |
  SwitchBlock | LegacyStructural | PropertyBinding | EventBinding | TwoWayBinding | PipeExpr |
  RefVar | LetVar | NgContent` — `@if/@for/@switch` control flow AND `*ngIf/*ngFor` microsyntax
  desugar into the SAME IR; `[prop]`/`(event)`/`[(banana)]` bindings; pipes; template ref vars;
  `ng-content`. (No type lowering in `verter_compiler` — the thin-adapters guard's compiler leg
  applies to `crates/verter_compiler/src/angular/**` exactly as it does to the Svelte dir.)
- NEW `crates/verter_compiler/src/angular/ide/`: TCB sidecar TSX `<canonical>.ngtcb.tsx` under
  the TCB contract — one `function __tcb(ctx: HostComponent): void` per component; child
  elements matched against the `AngularTemplateScopeDb` above (one component max, many directives
  per element); matched classes materialized as `null! as ChildComponent` (NO constructor calls
  — avoids DI false errors); inputs → assignability checks against the captured input accept
  types; outputs → `$event` synthesized from `EventEmitter<T>` / `OutputRef<T>` / DOM
  event maps (substrate D-aj: `OutputRef<T>` — the common interface `OutputEmitterRef<T>`
  implements — is the payload-extraction contract name throughout this program, consistent with
  §5 and A1); `[(x)]` → input check + writeback check (model signals via model metadata); `@if`
  → real `if` blocks (TS narrowing), `@for` → typed item/index locals (embedded-view context
  typing); refs: DOM → `HTMLElementTagNameMap`, component → matched instance type, `exportAs` →
  directive facts, `ng-template` → `TemplateRef<unknown>`; pipes resolved by name in scope →
  `transform` signature through the shared resolver; safe navigation → TS optional chaining
  under strict-template semantics (no deliberate `any` mode in v1). All output through
  `CodeTransform`.
- `crates/verter_lsp/src/sync_coordinator.rs`: additive sidecar virtual-file support
  (primary-file mapping unchanged); **sidecar ownership** — edits to EITHER the owner `.ts` or a
  linked external `.html` (the A1 `templateUrl → template_canonical` link) re-enqueue the
  owner's TCB. The Angular `VirtualFileNaming` row is
  `{ ide: None, api_suffix: None, testing_api_suffix: None, sidecar_suffixes: [".ngtcb.tsx"] }`
  (the substrate's D-x column —
  append-to-full-canonical semantics; `ide: None` because the component owner is a real `.ts`
  served directly).
- `verter_language`: gated-candidate row — `.html` classifies as
  `FileLanguage::FrameworkTemplate { "angular", owner_hint }` ONLY through
  `HostLanguageClassifier` under the Angular capability bit (`ProjectCapabilitySnapshot.hash` is
  the invalidation rail); external `templateUrl` targets linked by decorator metadata. Arbitrary
  `.html` is never parsed. **Template-language ↔ compiler-registry mapping (substrate D-bd)**:
  the `FrameworkTemplate` row carries NO carrier-compiler obligation —
  `carrier_descriptors_have_compilers` binds `carrier_language: Some(_)` descriptors only, and
  the Angular descriptor's `carrier_language` is `None` (components are real `.ts` Script
  files). A template file is OWNER-ROUTED: the TCB sidecar is produced by
  `crates/verter_compiler/src/angular/ide/` dispatched off the OWNING COMPONENT through the
  sidecar virtual-file pipeline (the `.html` source is pulled by the owner's TCB build via the
  A1 `templateUrl → template_canonical` link), never independently compiled — a standalone
  `.html` compiler row would be a second entry path into template compilation. REJECTED: a
  typed `template_language` descriptor slot (a compiler-registry obligation with no dispatch
  consumer).
- FFI accepted-kind strings extended add-only with `"angular_template"` — landing HERE, in the
  SAME change as the `.html` gated registry row and the sidecar wiring (substrate D-aj; the
  pairing rule: an accepted string lands together with its registry row — between A1 and A2 the
  string would have been a dangling kind naming a `FileLanguage` no registry row produces).
  FFI-time classification stays STATIC-ONLY per the substrate's `ffi_no_silent_vue_default`
  semantics: `.html` can NEVER classify as `FrameworkTemplate` by inference at the FFI boundary
  — the gated row requires the explicit kind string.
- LSP watcher registration for `**/*.html` only when the gate is on.
- **Source maps**: inline templates map through string-literal offsets; external templates map to
  `.html` positions; TCB scaffolding unmapped.

**V1 exclusions (OUT-OF-SCOPE rows, never known-bug ledger rows)**: host directives, custom
structural-directive guards, full schema/custom-element typing, DI provider context, `@defer`,
animations, forms/CVA inference. Known-bug LEDGER rows are reserved for pre-existing
shared-resolver failures Angular surfaces — the two lists never mix.

**Legacy Deletions.** None.

**Tests (failing first).**
1. Template parser/IR snapshots (control flow, microsyntax desugaring into the same IR nodes,
   bindings, pipes) + negatives (no `*ngIf` residue in TCB output).
2. TCB sidecar sourcemap e2e; generated TSX type-checks (binding type errors surface at template
   positions — inline AND external); `@for` item/index local typing; `$event` typing for
   component outputs and DOM events; `[(banana)]` both-direction checks; safe-navigation
   nullability (DISCRIMINATING: a null-path misuse produces an error at the template span).
3. Child-component scope tests: standalone `imports` scope match, NgModule scope match,
   out-of-scope selector → typed diagnostic (not silent intrinsic fallback). PLUS the
   `AngularTemplateScopeDb` suite (moved here with the DB, substrate D-ar(iii)): selector →
   class resolution, scope invalidation on a contributing file edit (`ReadSetSignature` miss),
   key content-freedom guard (`angular_template_scope_db_key_content_free`); **capability
   flip-on/flip-off (DISCRIMINATING both directions, the D-aj hole)**: a scope entry built with
   the Angular capability ON is NOT served after the capability flips OFF (the
   `consumed_capability_bits` key dimension misses — asserted as a structural key miss, not a
   fact-validation failure), the TCB inputs (the scope handed to the TCB builder) reject the
   stale capability state the same way, and flipping back ON rebuilds cold (no resurrection of
   the pre-flip entry across an intervening contributing-file edit).
4. `sync_coordinator` sidecar unit + LSP e2e (hover in external template; `.html` edit re-checks
   the owner TCB).
5. Project-gate tests: `.html` outside an Angular project stays inert (DISCRIMINATING both ways).
6. Out-of-scope rows registered (list above); ledger rows only for pre-existing shared-resolver
   failures surfaced.

**Verification.** The canonical gate (below).

**Dependencies.** A1; the landed Svelte LSP single-virtual-file path (proven before any sidecar
work — carried ordering rule).

---

## 5. Surface Mapping (FrameworkSurfaceKind — Angular column)

| Kind | Angular |
|---|---|
| PROPS | `@Input()` / `input()` / `input.required()` |
| EMITS | `@Output()` / `output()` (`EventEmitter<T>`/`OutputRef<T>` payload) |
| SLOTS | `ng-content` select inventory |
| OPTIONS | unsupported (decorator metadata is not an options surface in v1) |
| EXPOSE | public class instance surface |
| MODEL | `model()` signals |

Unsupported kinds return TYPED unsupported results via the substrate's per-kind
`FrameworkSurfaceKindStatus` (`UNSUPPORTED` + `GRAPH_EXACTNESS_UNSUPPORTED` + diagnostic; one
entry per known kind), never silent empties. No new `FrameworkSurfaceKind` variants are needed.

---

## 6. Architecture Guards

| Guard | Block | Kind | What it asserts |
|---|---|---|---|
| `angular_template_scope_db_key_content_free` | A2 (moved with the DB, substrate D-ar(iii)) | static + test | `AngularTemplateScopeKey` derives `Hash` over EXACTLY `owner_component_slot` + the typed `consumed_capability_bits` dimension + `project_identity` + `resolve_env_hash` — content/version hashes and `fact_dep_signature` forbidden in the key (R6; derived capability bits are env-shaped derived config, not content), and `type_env_hash`/`lib_env_hash` forbidden absent a recorded value dependence (substrate D-aw; R21 — the value is selector/scope/pipe-NAME data, resolve-env domain); a whole-struct destructure unit test (no `..`) forces a conscious decision on any added key field; value-side rooting via `self_root_canonicals` + `ReadSetSignature.facts` validated on every warm read; the flip-on/flip-off tests (A2 test 3) are this guard's behavioral half |
| `framework_registry_complete` (re-assert) | A0 + A1 | runtime | A0: `FRAMEWORK_TAG_ANGULAR` maps to the explicit registered DEFERRED TAG ROW landed in the same change as the proto value (the guard's tag-completeness clause, §9 item 9 — without the row A0's gate is structurally red); A1: the deferred row is SUPERSEDED by the real `"angular"` registration and the guard stays green across the flip |
| `script_fact_providers_zero_cost_on_miss` (re-assert) | A1 | perf/runtime | the substrate guard re-run with the Angular provider registered — no-`@angular/core` files do zero stage-1 work; non-Angular projects (capability off) emit no stage-2 facts |
| `script_fact_capture_is_syntax_only` (re-assert) | A1 | static-grep | `framework_facts/angular.rs` references no import-resolution/route-fact/capability/`verter_session` types |
| `framework_adapters_are_thin_no_second_resolver` (extends) | A1/A2 | static-grep | the substrate guard's scopes extend over `typeinfo/adapters/angular/**` and `verter_compiler/src/angular/**` (no `lower_ts_type`/`oxc_parser::`/`parse_type_annotation` in the compiler dir) |
| `carrier_descriptors_have_compilers` (re-run) | A2 | runtime | re-asserted with the Angular registration landed (substrate D-bd): the guard binds descriptors with `carrier_language: Some(_)` ONLY — the Angular descriptor's `carrier_language` is `None` (components are real `.ts` Script files) and `FileLanguage::FrameworkTemplate` never populates the singular `carrier_language` column, so the `.html` row carries NO carrier-compiler obligation; the re-run asserts the guard stays green and that NO `CarrierCompiler` registration exists for `"angular"` — the TCB sidecar is produced by `crates/verter_compiler/src/angular/ide/` dispatched off the OWNING COMPONENT through the sidecar virtual-file pipeline, never an independent `.html` compile |
| `framework_known_bug_ledger_bijection` (extends) | A1/A2 | manifest | Angular ledger rows bijective with `known-bug:` ignores |
| Retired/taxonomy/byte-pin suites (re-run) | A0 | — | proto/TS parity, byte-equal bindings, audit parity green across the tag addition |

Any new CRITICAL-rule text introduced by this program registers its guard in
`CRITICAL_RULE_GUARDS` in the same change (R6 meta-guard).

---

## 7. Risks (carried + program-specific)

| # | Risk | L/I | Mitigation |
|---|---|---|---|
| AR1 | Template type-check depth balloons (microsyntax, pipes, DI context, host directives, custom structural-directive guards) | High/High | v1 scope = control flow + property/event bindings against the SELECTOR SCOPE + pipes; TCB mirrors Angular's own compiler design (`null!` instantiation, strict-template semantics); official-compiler oracle fixtures feature-gated; out-of-scope rows absorb deliberate exclusions, the ledger absorbs pre-existing resolver gaps — never core changes |
| AR2 | Sidecar virtual files destabilize `sync_coordinator` | Med/Med | Additive sidecar registry with its own unit/e2e tests; lands only after the Svelte single-virtual-file path is proven (go/no-go criterion + A2 dependency) |
| AR3 | First real user of the gated-classification mechanism (`.html` row, capability bit) finds substrate gaps | Med/Med | The mechanism landed unit-tested but unexercised; A1's discriminating gate tests (capability on/off/flip) are written failing-first; substrate fixes, if needed, land in the substrate owner layer — not as Angular-local workarounds |
| AR4 | Scope DB cold-build cost on large NgModule graphs | Med/Med | Query-identity caching with fact validation; cold build resolves structurally through the shared resolver (cached per the canonical dependency rule); perf bench rows added before A2 lands |

---

## 8. Verification gate (every block)

```bash
cargo nextest run --workspace                  # completeness (incl. verter_session integration suite)
cargo test -p verter_session --tests           # shared-process surface
cargo clippy --workspace -- -D warnings
cargo fmt --all --check
pnpm install --frozen-lockfile
pnpm test                                      # where TS was touched
```

Each block lands as ONE squashed conventional commit after the green gate + dual review
(independent reviewer + codex), per the repo's commit conventions (no phase/temporal vocabulary;
existing scopes only).

---

## 9. Consumed Substrate Seam Contracts (INLINED — the operative content, executable cold)

This program consumes the following substrate seams WITHOUT modification. Each contract is
stated here in full operative form; the bracketed substrate decision ids are traceability tags
only.

1. **Routing + parse artifact** [D-a/D-g/D-r]. Language identity is the open descriptor
   `FileLanguage { Script { source_type }, Framework { adapter_id, language_id },
   FrameworkTemplate { adapter_id, owner_hint } }` owned by the leaf crate `verter_language`;
   `LanguageRegistry::classify_static(path)` serves static extension rows + gated-candidate
   descriptors and NEVER reads project config; final classification is
   `HostLanguageClassifier` (session) composing `classify_static` with
   `ProjectCapabilitySnapshot` — `.html` becomes `FrameworkTemplate { "angular", owner_hint }`
   ONLY under the Angular capability bit. `FrameworkParseArtifact` is the open parse wrapper:
   typed `FrameworkParseCommon` (script/template/style regions with per-region
   `ScriptSourceType`, external links, diagnostics) + a private erased carrier; carrier downcast
   is token-gated (`CarrierAccessToken`, minted ONLY inside `verter_language` during
   `LanguageRegistry` carrier-row construction — the SOLE minting authority; adapter descriptors
   RECEIVE the row's token as their registration proof and never construct one; no public
   arbitrary-id constructor and no public by-id token lookup exist)
   and confined to the owning adapter by static guard.
2. **Two-stage `ScriptFactProvider` seam** [D-o/D-z/D-ah/D-an]. STAGE 1 (syntax-candidate
   capture) runs inside the ONE shallow OXC pass behind the CLOSED exact-valued gate
   `ScriptFactSyntaxGate { CarrierLanguage(LanguageId), ImportSpecifier(&'static str) }` — no
   predicate/pattern arm; the dispatcher's `ActiveProviderIndex` is two exact-match maps
   (`by_carrier_language`, `by_import_specifier`) so a non-matching file's active provider set
   is computed EMPTY before any provider invocation, gate evaluation, or allocation (no
   per-`Script`-row provider list exists). Stage 1 may inspect live OXC nodes + `lower_ts_type`;
   it may NOT resolve imports, read capability bits, or emit final facts; candidates store
   content-addressed under `(canonical, content_hash, parse_env_hash, parser_version,
   file_language_id, provider_id, provider_version)`. STAGE 2 (session-owned resolved-symbol
   validation, fact-demand time) consumes stage-1 candidates only, resolves candidate import
   sources through the EXISTING resolver/route facts, REJECTS userland look-alikes, consults the
   DERIVED capability bits, and emits validated facts cached on the owner artifact identity with
   sub-key `(provider_id, provider_version, consumed_capability_bits, project_identity,
   resolve_env_hash)` — NO `type_env_hash`, NO `lib_env_hash` (stage 2 validates symbol
   identity/package provenance, resolve-env domain; if a future stage-2 validation genuinely
   consults lib data, the dependence is recorded and the dim added THEN); publication only via
   `SignatureAdmission::Cacheable`, overflow/cancellation/unresolved provenance → `ReturnOnly`,
   never published.
3. **Gated classification + per-file invalidation** [D-r]. `ProjectCapabilitySnapshot.hash` is
   over DERIVED capability bits (never raw config bytes) and keys the CLASSIFICATION cache only;
   the per-file `FileArtifactStore` key carries the explicit `file_language_id` column, so a
   capability flip invalidates exactly the files whose classification row changed; NOTHING
   capability- or provider-shaped enters the global `parse_env_hash`.
4. **Framework-surface DTO store** [D-p/D-y/D-aq]. ONE generic `FrameworkSurfaceDtoStore`:
   generation-scoped, CONTENT-ADDRESSED + fact-validated (the owner content hash
   `owner_whole_hash` is deliberately IN the key — the content-free query-identity vocabulary
   does NOT apply to this store; it applies to `AngularTemplateScopeDb`); per-adapter typed
   sub-maps behind `dyn ErasedFrameworkSurfaceStore`; generic columns `{ surface_kind,
   query_level, canonical, owner_whole_hash }` + the adapter's typed `Eq + Hash` structural key
   remainder; NO lossy digest component, NO env dims, NO adapter/normalizer version column; warm
   read requires `validated_at_generation == live generation` AND `ReadSetSignature.facts`
   validation; publication only via `SignatureAdmission::Cacheable`; cross-adapter reads merge
   the child's read-set into the parent (non-cacheable child ⇒ non-cacheable parent).
5. **Per-kind status wire vocabulary** [D-s]. `FrameworkSurfaceKindEntry.status =
   FrameworkSurfaceKindStatus { support: UNSPECIFIED/SUPPORTED/UNSUPPORTED/PARTIAL, exactness:
   GraphExactness, diagnostics: [GraphDiagnostic] }`; SUPPORTED ⇒ `members` authoritative (empty
   = supported-empty); UNSUPPORTED ⇒ empty members + `GRAPH_EXACTNESS_UNSUPPORTED` + ≥1
   diagnostic; PARTIAL ⇒ usable subset + explaining diagnostics; a framework-surface response
   carries EXACTLY ONE entry per known kind; the framework-surface operation has a
   per-operation schema minimum (`FRAMEWORK_SURFACE_MIN_SCHEMA_VERSION = 3`) enforced before any
   semantic dispatch (`MalformedPayload` on failure), with `validate_schema_version_for_operation`
   matching exhaustively over operations (no wildcard arm).
6. **Virtual-file naming column** [D-x/D-al]. `FrameworkAdapterDescriptor.virtual_file_naming:
   VirtualFileNaming { ide: Option<IdeSuffixPolicy>, api_suffix, testing_api_suffix,
   sidecar_suffixes }` — TOTAL over the live role enumeration (ide, api, testing-api, the
   uniform `{carrier_ext}.d.ts` acceptance spelling, sidecars; `testing_api_suffix.is_some() ⇒
   api_suffix.is_some()`); ALL suffixes append to the FULL canonical (no stem rewriting); every
   consumer (LSP sync, ts-plugin via the generated byte-pinned `virtual-file-naming.ts` module)
   derives naming from this one column. The Angular row is `{ ide: None, api_suffix: None,
   testing_api_suffix: None, sidecar_suffixes: [".ngtcb.tsx"] }` — the component owner is a real `.ts`
   served directly; the TCB sidecar is the only Angular virtual file.
7. **Api-content producer seam** [D-ak]. The api virtual file's content is produced by a
   session-owned `ComponentApiProjector` registration leg dispatched inside
   `VerterHost::get_public_api_with_mode` (the host method is the single entry); a descriptor
   with `api_suffix: Some(_)` MUST register a projector leg. Angular registers NO api projector
   (`api_suffix: None` — consistent by construction; `get_public_api*` returns `None` for
   Angular components, which are real `.ts` files tsserver reads directly).
8. **Adapter trait, plan vocabulary, ctx surface, registration legs** [D-f/D-ai/D-am/D-as/D-ag].
   `FrameworkSurfaceAdapter { descriptor(), plan_surfaces(ctx, selector, requested) ->
   FrameworkSurfacePlan, normalize(ctx, resolved) -> FrameworkSurfaceDtoBundle }` — the EXECUTOR
   owns resolution BY CONSTRUCTION; the plan is the CLOSED `PlannedDemand { PublicTypeInstance,
   MacroPayload, PathProjection, ShallowSurface }` (no source text, no OXC handles, no raw
   `SemanticQueryKey`, no escape arm; exhaustive executor match). `FrameworkAdapterCtx` — what
   `plan_surfaces`/`normalize` receive — is a CLOSED op surface exposing carrier metadata +
   validated facts ONLY: exactly `carrier_for::<T>(canonical)` + `script_facts_for::<T>`. NO
   resolve method exists on any adapter-visible ctx — the four resolve ops
   (`instantiate_public_type`/`resolve_macro_payload`/`project_path`/`shallow_surface`) and
   `export_graph` live on the executor-private resolve surface (module-private to the
   framework-surface executor, never passed to adapter code); the executor consumes
   `PlannedDemand` and drives them itself, and `normalize` receives the resolved results as
   data — an adapter needing more semantic data adds a `PlannedDemand` variant, never a ctx op.
   `ensure_indexed_ready`/`IndexedReady`/raw/eval source/content snapshots are likewise
   executor-private and statically banned from adapter code. Registration rows are
   `FrameworkRegistration { descriptor, surface: SurfaceRegistration { Adapter | Deferred } }`;
   a `Deferred` surface leg is served structurally as per-kind UNSUPPORTED + diagnostic;
   carrier/synth/script-fact-provider/api-projector legs ride the descriptor row. The
   component-default synth seam [D-n/D-au] exists on the registry and its ctx is PARSE-DOMAIN
   ONLY (synth output lands in content-addressed shallow state — stage-2 validated facts never
   enter synth); Angular registers NO synth — the
   component class is a real TS value symbol resolved directly (no synthesized `default`).
9. **Tag semantics + tag completeness** [D-aa/D-aw]. A `FrameworkTag` value's existence is NOT a
   support guarantee; support is asserted only by a registered adapter and surfaced per-request
   via `FrameworkSurfaceKindStatus`; tag values land only with their vertical (A0 lands
   `FRAMEWORK_TAG_ANGULAR`); `FrameworkTag` is a proto3 OPEN enum outside the four closed
   `oneof` taxonomies, so a value addition does not bump the schema version (A0's pinned rule).
   TAG COMPLETENESS (the `framework_registry_complete` clause this program must satisfy at every
   landing): every `FrameworkTag` wire variant maps to a registered adapter OR an explicit
   registered deferred/out-of-scope TAG row in the session registry's tag mapping — a tag
   addition therefore lands in the SAME change as its registry tag-row (A0 registers the
   deferred `FRAMEWORK_TAG_ANGULAR` row; A1 supersedes it with the real `"angular"`
   registration); a wire tag with neither is a structurally red gate.
10. **Carried reconciliations** [D-aj/D-ar/D-aw/D-bd]. The scope-DB `consumed_capability_bits` key
    dimension; the A0 no-bump schema rule; the A2 FFI accepted-string/registry-row pairing (an
    accepted string lands in the SAME change as its registry row; a row without a registered
    carrier serves the typed `UnsupportedLanguage` state structurally — the substrate proved
    this intermediate state with the `.svelte` row); `OutputRef<T>` as the output
    payload-extraction contract name (the common interface `OutputEmitterRef<T>` implements);
    the scope-DB build landing in A2 with its consumer; the scope-DB key carrying NO
    `type_env_hash`/`lib_env_hash` (R21 — the value is resolve-env domain; the dependence is
    recorded and the dim added only if a future value stores type-meaning- or lib-dependent
    data); the A0 deferred TAG row for `FRAMEWORK_TAG_ANGULAR` superseded by A1's real
    registration; the `FrameworkTemplate` carrier-compiler exemption (D-bd —
    `carrier_descriptors_have_compilers` binds `carrier_language: Some(_)` descriptors only;
    template files are owner-routed, never independently compiled; the Angular descriptor's
    `carrier_language` is `None`).

The substrate plan's decision D-w is the architectural verdict this program implements; its
decision D-ac records the extraction; D-ar records the round-4 self-containment, audit-gate, and
scope-DB-rehoming reconciliations; D-aw records the round-5 scope-DB key-dim scoping, the A0
deferred-TAG-row rule, and the §1.2 scope clarification; D-bd records the `FrameworkTemplate`
carrier-compiler exemption the A2 guard re-run asserts. If this program discovers a
substrate-seam deficiency, the
fix lands in the substrate owner layer under the substrate plan's invariants — never as an
Angular-local fork.

*End of program document.*
