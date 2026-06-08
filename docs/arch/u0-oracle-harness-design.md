# U0 — TS7 TypeExpr-Projection Oracle Harness (LOCKED design)

> Status: **LOCKED.** All five questions and the three implementability
> architecture decisions are resolved to implementable depth below. This harness is
> scoped to **`TypeExpr`-valued type-projection rows/queries only**: it produces
> exact-equality `TypeExpr` snapshots for the subset of oracle rows whose
> assertion is a structured `TypeExpr` projection. It asserts **structural
> `TypeExpr`-projection parity** (Verter's in-process `TypeExpr` vs the `TypeExpr`
> lowered from TS7's hover), NOT full TypeScript nominal/semantic identity (§Scope) —
> nominal constructs `TypeExpr` cannot carry are default-rejected, not force-fit. The
> **`Relate`-free oracle rows are an upper bound of 122** (the rows carrying no
> `SemanticQueryName::Relate` anywhere); the asserting-key relation/call gate, the
> positive construct allowlist (§Q2), and the footprint/audit-row exclusion (§Scope)
> each narrow FURTHER, so the true admissible count is **strictly lower than 122**. The
> relation / call-resolution / assignability families remain future work under a
> separate structured oracle (§Scope). The decided shape is: tsgo
> `textDocument/hover` text, captured via a FIXED, VERSIONED synthesized probe;
> the captured hover is parsed to an OXC type AST and admitted by a **strict
> POSITIVE ALLOWLIST (default-REJECT)** checked on BOTH the hover AST AND the
> fixture SOURCE declaration, plus a strict-lowering drop-counter; only an
> admitted capture is lowered to a `TypeExpr` at generation time, stored as a
> checked-in snapshot, and compared structurally against Verter's in-process
> `TypeExpr`. The snapshot **filename (`snapshot_id`) is derived from
> REGISTRY-ONLY, tsgo-free inputs** (row-ref + query-helper payload + host-project
> + pinned env/algorithm versions, including the pinned `env_corpus_id` of the
> closed vendored oracle-env corpus), so the coverage guards can compute the
> expected filename set from the registry ALONE — without opening any snapshot or
> running tsgo. The resolution-affecting env (`oracle_env_hash` over the **closed,
> VENDORED, checked-in oracle-env corpus** — never live `node_modules`, which is
> gitignored) is STORED IN the snapshot and validated as a VALUE on read, never
> folded into the filename — closing the env-in-filename chicken-and-egg. The
> generator COPIES THE BYTES of every consulted file (the canonical oracle
> `tsconfig.json`, every consulted lib / ambient / package `.d.ts`, and every
> resolution-affecting `package.json` / project-metadata file) into a checked-in
> vendored corpus directory and drives tsgo against THAT frozen corpus root, so the
> oracle env is hermetic + closed + offline-re-enumerable. A
> dedicated checked-in
> **oracle-query-spec registry** is the machine-readable source of truth: it
> carries the executable query payloads (off the test bodies) as PURE
> context-neutral data, and lifted row bodies just call a shared registry driver.
> Each registry entry lives in
> `src/typeinfo/typeinfo_tests/oracle_query_specs.rs` (reachable by the lifted
> unit tests), carries an `oracle_family`, and — in the DEFERRED §Q4 per-row-count
> layer (not yet a shipped `IgnoredTestRow` field) — the manifest generator's
> row-spec will declare an independent `oracle_query_ordinals` count (sourced
> independently of the registry) cross-checked against the registry, so query
> coverage becomes true by construction AND verified, never "true by construction" alone. The DOMINANT
> typeinfo host is `VerterHost::new_standalone` (no project root, no tsconfig); the
> generator drives standalone-host rows under ONE deterministic canonical oracle
> tsconfig + synthetic root (a stable `compiler_options_hash`) vendored into
> `oracle_env_files`. Admissible modes are `Shallow` / `Navigate` plus the
> `Expanded` construct classes whose probe form is validated (the index-signature
> publication + built-in modifier-utility lifts); an `Expanded` class without a
> validated probe form stays `Ignored`. Snapshots are
> vendored in-repo and loaded at test
> time via runtime `std::fs::read` rooted at
> `concat!(env!("CARGO_MANIFEST_DIR"), "/src/typeinfo/typeinfo_tests/oracle_snapshots/", …)`.
> NO production code,
> harness, or row-lift is part of this document — it specifies the design the
> implementation block builds.

---

> **Reframed by the single-spec / correction-overlay model (see [`ts-compat-two-mode-model.md`](ts-compat-two-mode-model.md)).**
> This harness's framing — "lift a row, assert Verter agrees with TypeScript-7's
> answer" (below) — assumed **tsgo is the one true answer per row**. The locked
> single-spec / correction-overlay model **reframes** that: Verter is **correct by default
> vs TypeScript, not bug-for-bug**. The resolver always produces the **`Correct`** value
> (it has no compat mode and no spec dimension on any cache key); TypeScript's
> bug-included output is recorded as **DATA** (the tsgo snapshot), never produced by the
> resolver. The impact on THIS document is a small set of localized reframings, marked
> inline below — **the snapshot schema, `snapshot_id` derivability, the 362-row
> partition, the ≤122 `Relate`-free ceiling, and the hermetic / offline-re-derivable
> guarantees are ALL UNCHANGED**:
> - the snapshot's `oracle_value` is now specifically the **`ts_compat` oracle** — the
>   recorded `TsCompat` (tsgo) value (unchanged bytes — a reframing, not a schema change);
> - a **divergence** row additionally carries a separate, **review-gated correction
>   overlay** (`oracle_corrections/<family>/<snapshot_id>.correction.json`) holding the
>   `correct` value in the same `TypeExpr` codec — never inside the snapshot (which must
>   stay recompute-gated → byte-identical);
> - a **divergence** row is seated as `ProofRequirement::OracleAndGuard { oracle, guard }`
>   whose `guard` is the registered `DivergenceCorrection` prover (one of the five
>   `OracleAndGuard` obligation kinds, §Q4) consulting a per-query
>   `&[QueryCorrection { query_ordinal, correction, divergence_id }]` overlay. Corrections
>   bind at `(row, query_ordinal)`
>   granularity — a row issues N queries and may MIX corrected and ordinary queries. The
>   harness runs the single-spec resolver ONCE and compares it against recorded data
>   (`ts-compat-two-mode-model.md` §7): a CORRECTED query asserts `resolver(query) ==
>   correction.correct_value` (the `Correct` value) while that query's
>   `snapshot.oracle_value` is the recorded `TsCompat` value and must differ; every other
>   query asserts `resolver(query) == snapshot.oracle_value`. There is no per-mode re-run,
>   no family-key comparison, and no spec dimension — the resolver is single-spec;
> - the §6.3 differential budget (referenced via "Relationship to §6.3" below) is
>   reformulated: registered divergences are **excluded** from the per-family defect
>   budget M (M counts only UNREGISTERED `resolver(Correct)`-vs-tsgo disagreements, target
>   M → 0; registered corrections are confirmed by the data comparison, not budgeted);
> - the foundational substrate / row-lift block must adopt this model **before** lifting
>   rows, so a divergence row is born with its correction rather than re-lifted later.
> The marked sites below are reframings only; this document's locked schema and
> invariants are not modified.

## 1. Context

Verter is finishing the **native-typeinfo-PARITY** dimension of its type
system. The parity ledger (`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`)
carries ~362 `#[ignore]`'d test-site rows; the dominant proof requirement is
`ProofRequirement::Ts7Oracle(OracleId)` (manifest line 427). Lifting a row means:
take a `(fixture, query)` pair, compute Verter's in-process answer, and assert it
agrees with **TypeScript-7's** answer for the same query.

> **Reframed by the single-spec / correction-overlay model.** "Agrees with
> TypeScript-7's answer" holds for the ~95% of rows that carry **no correction** (`correct
> == tsgo`), where the harness asserts `resolver(row) == snapshot.oracle_value`. For a
> registered-divergence row, Verter's one (`Correct`) answer is the reviewed correction,
> NOT the tsgo snapshot; that corrected query asserts `resolver(query) ==
> correction.correct_value` while that query's `snapshot.oracle_value` is the recorded
> `TsCompat` (tsgo) value, captured as data and never produced by the resolver (corrections
> bind at `(row, query_ordinal)` granularity — see `ts-compat-two-mode-model.md` §7).

The TS7 answer is supplied by `tsgo` (`@typescript/native-preview`). Per
`docs/arch/native-typeinfo-parity.md` §4.x / §6.3 the oracle answer is **captured
ONCE at build/test time as a checked-in normalized snapshot**; lifted rows compare
against the snapshot, never against a live `tsgo`. The resolver MUST NEVER shell to
`tsgo` at query time — `tsgo` is **oracle GENERATION ONLY**.

This harness is deliberately **narrow**: it serves the `TypeExpr`-projection
families only. The relation / call-resolution / overload-selection / assignability
families are NOT served here (§Scope), because their assertion is a verdict, not a
`TypeExpr`, and tsgo hover cannot supply a verdict.

### 1.1 Grounding facts (verified against the tree)

These are load-bearing and were confirmed by reading source:

1. **A lifted `TypeExpr`-projection row asserts on an in-process `TypeExpr`, not a
   wire payload.** The canonical test helper is
   `resolve_expr` (`crates/verter_session/src/typeinfo/typeinfo_tests/support.rs:132`):

   ```rust
   pub(super) fn resolve_expr(
       host: &VerterHost,
       canonical_id: &str,
       name: &str,
       type_args: &[Arc<TypeExpr>],
       mode: ProjectionMode,
   ) -> (TypeExpr, verter_audit::RequestAuditRecord) {
       let (outcome, record) = host
           .resolve_named_symbol_with_audit(canonical_id, name, type_args, Some(mode))
           .into_parts();
       let node = outcome.ok().flatten();
       let expr = host
           .project_node_to_type_expr(node.unwrap_or_else(|| panic!("{name} must resolve")))
           .unwrap_or_else(|| panic!("{name} resolved node must project to TypeExpr"));
       (expr, record)
   }
   ```

   The chain is `resolve_named_symbol_with_audit` → `project_node_to_type_expr`
   → `verter_type_expr::TypeExpr`. It does **NOT** route through the U8 wire / U10
   result-DB / U12 exporter. The comparable oracle value MUST therefore be
   comparable to a `TypeExpr` (or a normalized projection of one), NOT to a
   serialized `TypeInfoGraphPayload`.

2. **`TypeExpr` is the comparison value, and its JSON codec is internally
   tagged.** `verter_type_expr::TypeExpr` (`crates/verter_type_expr/src/lib.rs:128`)
   is a closed enum: `Primitive(PrimitiveName)`, `Literal(LiteralValue)`,
   `Union(Arc<[TypeExpr]>)`, `Intersection(Arc<[TypeExpr]>)`,
   `Array { element, readonly }`, `Tuple { elements, readonly }`,
   `Object(Arc<ObjectExpr>)`, `Function(Arc<FunctionExpr>)`,
   `ConstructorType(Arc<FunctionExpr>)`, `Ref { name, type_arguments }`,
   `TypeParameter(TypeParam)`, `KeyOf`, `TypeOf`,
   `IndexedAccess { object, index }`,
   `Conditional { check, extends, true_type, false_type }`, `Mapped { … }`,
   `TemplateLiteral { quasis, expressions }`, `Infer { name }`, `Rest`,
   `Parenthesized`, `RecursiveRef { … }`, `SyntheticSlotBinding`,
   `Unknown { raw }`. It has hand-rolled JSON (de)serialisation:
   `TypeExpr::to_json_value` (`crates/verter_type_expr/src/type_expr_json.rs:420`)
   and `type_expr_from_json` (`:35`). **The encoding is INTERNALLY tagged**: every
   node is a JSON object carrying a `"kind"` discriminant string
   (`"primitive"` / `"literal"` / `"union"` / `"object"` / `"ref"` / …,
   `type_expr_json.rs:36,424`); an `Object` carries a flat `"properties"` array
   whose members each carry a `"memberKind"` discriminant
   (`"property"` / `"method"` / `"callSignature"` / `"constructSignature"` /
   `"indexSignature"`, `:299,462-509`) — index signatures are members of the same
   `"properties"` array, NOT a separate field. This is the on-disk encoding for the
   Verter side of a parity comparison and for the structured oracle value.

3. **`tsgo`'s LSP exposes no structured full-type query; `textDocument/hover` is
   the only type-bearing point query, and it collapses to a `String`.**
   `TsgoTypeProvider` (`crates/verter_type_runtime/src/tsgo/ipc.rs:978`) spawns
   `tsgo --lsp --stdio` (`:1020-1022`). `get_hover` (`:1587`) issues the
   `textDocument/hover` request (`:1611`) and reduces every LSP content form
   (MarkupContent / MarkedString / MarkedString[]) to a single `String`
   (`:1635-1675`). The result is `HoverInfo { contents: String, range_start,
   range_end }` (`crates/verter_type_runtime/src/protocol.rs:101-107`), and
   `get_hover` ALWAYS sets `range_start: None, range_end: None` — it **discards the
   hover range** (`ipc.rs:1689`). The transport references many LSP methods
   (`textDocument/hover`, `completion`, `diagnostic`, `definition`,
   `typeDefinition`, `references`, `rename`, `signatureHelp`, `codeAction`,
   `documentHighlight`, `inlayHint`, plus `initialize`/`shutdown` and
   notification-only `didOpen`/`didChange`/`didClose`), but NONE of them is a
   structured full-type query and only `hover` is a type-bearing point query.
   `textDocument/signatureHelp` exists but is a **call-surface** — relevant only to
   the deferred call-resolution families (§Scope), not to a `TypeExpr` projection.
   tsgo's LSP answer for a type query is, today, a **hover string** with no
   range — under the adopted empty-caps driver a bare plaintext alias, per the
   §Q2/Q3 grammar (a markdown-caps driver would instead return a fenced block).

4. **`verter_session` does NOT depend on `verter_type_runtime`.** `tsgo` lives
   entirely behind `verter_type_runtime`; `crates/verter_session/Cargo.toml` has
   no `verter_type_runtime` dependency and no `tsgo` reference. The resolver crate
   therefore **cannot** spawn tsgo today — the runtime-forbidden invariant (§4) is
   already true by crate-graph construction and we only need to pin it.

5. **17-family `OracleId` enum** (`typeinfo_ignored_test_manifest.rs` `OracleId`):
   `UtilityComposition`, `MappedTemplate`, `IndexedAccess`, `EnumProjection`,
   `ClassSurface`, `ApparentType`, `TupleProjection`, `ConditionalInfer`,
   `RelationSemantics`, `TemplateLiteral`, `FlowNarrowing`, `CallResolution`,
   `ContextualTyping`, `ValueInference`, `JsxResolution`, `ModuleAugmentation`,
   `CompositeSurface`. The proof enum is
   `ProofRequirement::Ts7Oracle(OracleId)` / `OracleAndGuard { oracle, guard }`
   (`:427` / `:430`). The row schema `IgnoredTestRow`
   (`typeinfo_ignored_test_manifest.rs:537`) has **13 fields** — 12 manifest columns
   plus the `status` lifecycle field. The columns relevant here are
   `file: &'static str`, `function: &'static str`,
   `semantic_queries: &'static [SemanticQueryName]`, `proof: ProofRequirement`, and
   the lifecycle `status: IgnoreStatus` (`Ignored` | `Lifted { block_id }`); the
   other manifest columns (`substrate`, `capability`, `organ`, `owning_u_block`, `block_id`,
   `mechanism_id`, `consumed_mechanisms`, `unblocker`) are ledger/ownership data. (The
   declared per-row oracle-query count `oracle_query_ordinals` — the §Q4 cross-check
   verified by `registry_entry_count_matches_declared` — is NOT one of those columns and
   NOT yet a shipped `IgnoredTestRow` field: it belongs to the DEFERRED §Q4 per-row-count /
   migration-fidelity layer, alongside the likewise-deferred `migration_fingerprint` /
   `original_body_tokens` body-hash fields, and is added to `IgnoredTestRow` only when that
   layer lands. `status` is the separate lifecycle field the regenerator substitutes on
   lift.)
   **`IgnoredTestRow.file` is a BARE filename** (`"apparent_types.rs"`,
   `manifest_data/typeinfo_ignored_test_manifest_rows.rs:11`), matching the live
   discovery key `path.file_name()` (`:796`/`:902`/`:1012`); the row→query join key reuses that
   exact bare-filename form. Critically, **the row carries NO executable query
   spec** — the actual `(canonical, symbol, type_args, mode)` payloads live today in
   the test body, and **one row can issue N queries** (e.g. `conditional_infer.rs`
   issues several `resolve_expr` calls; `cross_file.rs:6` upserts a four-file
   workspace before querying). This is why the row→query→snapshot join needs a
   dedicated registry that OWNS the executable payloads (§Q4), not a manifest field.

   The three lifted query-helper shapes the registry must encode are all in
   `support.rs`: `resolve_expr` (`:132`, `(canonical, symbol, type_args, mode)` →
   `TypeExpr`), `shallow_surface_expr` (`:160`, `ResolveDecl` + empty-path
   `Shallow` `ProjectPath` → `TypeExpr`), and `evaluate_expr` (`:208`, an arbitrary
   expression string e.g. `typeof f` in a scope → `TypeExpr`). All three return an
   in-process `TypeExpr` and never touch the wire.

6. **The `Relate` ceiling — 122 rows is an UPPER BOUND, not ~340.** The manifest
   has ~340 oracle-backed rows, but **218 of them carry `SemanticQueryName::Relate`**
   (the `RelationSemantics`, `CallResolution`, and other verdict families assert
   assignable/not, overload-selection, and call-resolution verdicts via
   `relation_semantics.rs` / `call_resolution.rs`, NOT a `TypeExpr`). Only **122
   oracle rows do NOT carry `Relate`**. tsgo hover cannot supply a relation verdict,
   so this `TypeExpr`-projection harness covers **at most 122 rows** — an UPPER
   BOUND, not an eligibility count. The TRUE admissible count is **strictly lower
   than 122**, for two independent reasons: (a) the positive construct allowlist
   (§Q2) defers any row whose TS7 type uses a non-admissible construct; (b) some
   non-`Relate` rows ALSO assert dependency-footprint / audit behavior a thin
   `TypeExpr` compare cannot prove (e.g.
   `flow_return_xf04_records_barrel_route_before_selected_leaf`, which calls
   `assert_cross_alias_warm_with_dependency_footprint`,
   `flow_return_catalog.rs:1496`) — those are excluded unless paired via
   `ProofRequirement::OracleAndGuard` (§Scope). The snapshot count may EXCEED the
   row count because one row maps to N query specs.

7. **`TypeExpr` cannot losslessly carry several TS constructs — the positive
   allowlist's REJECT half is grounded in missing fields.** Verified by reading
   the IR: `FunctionParam` (`crates/verter_type_expr/src/lib.rs:927`) has only
   `name`/`ty`/`optional`/`rest`/`span`/`has_ts_annotation` — **no receiver / `this`
   parameter flag**; `TypeParam` (`lib.rs:1018`) has only `name`/`constraint`/
   `default` — **no `const` modifier and no variance (`in`/`out`) field**;
   `ObjectMember` (`lib.rs:426`) is exactly Property / IndexSignature / CallSignature
   / ConstructSignature / Method — **no getter/setter accessor variant**;
   `ConstructorType` (`lib.rs:159`) wraps only a `FunctionExpr` — **no `abstract`
   flag** (and OXC's lowering at `crates/verter_type_expr_oxc/src/lib.rs:126`
   constructs that `FunctionExpr` while IGNORING constructor abstractness);
   `MemberVisibility` (`lib.rs:494`) participates in node identity (Eq/Hash) but OXC
   type-literal lowering stamps every member PUBLIC via `with_spans_public`
   (`oxc/lib.rs:427`). Each REJECT row of the §Q2 allowlist cites the exact missing
   field or lossy lowering.

8. **A TS7 hover answer depends on the FULL resolved file set, not just the
   compiler options — so the oracle env is a CLOSED, VENDORED, checked-in corpus,
   NOT live `node_modules`.** Verter's own env model splits a query's environment
   across `lib_env_hash` — the ambient lib / `@types` / module-augmentation corpus,
   built from `lib_names` + `type_roots` + the `ambient_corpus_fingerprint`
   (`crates/verter_workspace/src/env_hash.rs:84,99,219`) — and `project_identity`
   (`env_hash.rs:239`). tsgo hovers consult the same surface: when a row uses the
   workspace-footprint host the generator copies `@verter/types` (the Vue macro
   decls) into `node_modules` so tsgo resolves them by standard node resolution
   (`crates/verter_type_runtime/src/tsgo/ipc.rs:3651`), and a `package.json`
   `types`/`exports` change re-selects a DIFFERENT `.d.ts` (tsgo writes
   `node_modules/@verter/types/package.json` `{"types":"index.d.ts"}`,
   `ipc.rs:3686`; package-host fixtures select the resolved `.d.ts` via
   `exports.types`, `cache_invalidation.rs:324`) WITHOUT changing the stored
   `.d.ts` hashes. So a `compiler_options_hash` alone is an INCOMPLETE env pin — an
   ambient / lib / package `.d.ts` OR a resolution-metadata change (`package.json`
   `types`/`exports`, tsconfig/project metadata) can change a tsgo answer without
   changing the compiler options. Worse, the lib `.d.ts` set tsgo actually consults
   lives UNDER `node_modules` — tsgo bundles its libs at
   `node_modules/@typescript/native-preview-*/lib/` (`ipc.rs:~2859-2874`) and
   `node_modules` is `.gitignore`d (`.gitignore:9`), so pinning the env against live
   `node_modules` paths is NOT hermetic and breaks the Testing-Hermeticity rule
   (locally-vendored fixtures only). The harness therefore drives tsgo against a
   **CLOSED, VENDORED, checked-in oracle-env corpus**: the generator COPIES THE
   BYTES of every consulted file (the canonical oracle `tsconfig.json`, every
   consulted lib / ambient `@types` / package `.d.ts`, AND every
   resolution-affecting `package.json` / project-metadata file) into a checked-in
   vendored directory and points tsgo's root at THAT frozen corpus, not live
   `node_modules`. The corpus is content-addressed by an `env_corpus_id` (the hash
   of the full vendored file set). The snapshot stores and validates an **oracle env
   hash** over that vendored corpus — every resolution-affecting file: package
   manifests, tsconfig/project metadata, AND the `.d.ts` set — plus a DIRECTORY
   MANIFEST that lets the offline gate RE-ENUMERATE the vendored dir and assert
   set-equality (catching a newly-ADDED file, not just an edit/delete) (§Q1, §Q2).

9. **The DOMINANT typeinfo host is `VerterHost::new_standalone`, NOT a
   `/workspace` + tsconfig host.** Of the oracle-row bodies, only ~9 use
   `make_host_with_workspace_files_footprint` (a `/workspace` `MemoryWorkspace` +
   an `IdeProjectConfig` for `/workspace/tsconfig.json`, `support.rs:97`); the
   dominant ~369 call sites (including the 122 `Relate`-free candidates) use
   `make_host_with_footprint()` = `VerterHost::new_standalone(HostConfig { … })`
   (`support.rs:89`), which builds a default `MemoryWorkspace` with **NO project
   root and NO tsconfig** (`host_construction.rs:249`). The standard host for this
   harness is therefore `standalone`; `workspace_footprint` is the ~9-row minority;
   package-backed / custom-host (`make_package_host_with_workspace`,
   `cache_invalidation.rs:344`) stays a deferred class. Because a standalone host
   has no tsconfig/root, the generator drives tsgo for a standalone-host row under
   ONE deterministic CANONICAL oracle tsconfig + synthetic root — the same config
   for every standalone row, yielding a stable `compiler_options_hash` — and
   vendors that synthesized tsconfig plus the libs it pulls into `oracle_env_files`
   so the env is pinned + offline-re-derivable (§Q2, §Q1).

10. **§6.3 today** (`native-typeinfo-parity.md` §6.3 "Differential `tsgo`-parity
   oracle") describes the differential oracle as comparing **STRUCTURED** results —
   "Compare the projected `TypeInfoGraphPayload` / `RelationPayload` /
   `TypeDescriptor` structurally against tsgo's answer … NOT a text compare of
   display output" — under a **per-family divergence budget**.
   `native-typeinfo-parity.md` §4.4 "`satisfies` — TS7 oracle-pinned" says oracle
   rows are GENERATED, each id "deterministic from `(fixture, query,
   compiler_options_hash, tsgo_version, oracle_schema_version)`", the generator
   "runs `pnpm exec tsgo` at the pinned version … and writes checked-in normalized
   snapshots", default tests "compare Verter ONLY to the checked-in snapshots; they
   never invoke tsgo", and "A guard forbids tsgo execution" outside the gated
   generator. This design **reinterprets §6.3 for the `TypeExpr`-projection
   family** (see §Q2 "Relationship to §6.3"): it uses exact per-(row,query)
   equality on a structured `TypeExpr` rather than a per-family divergence budget,
   and it sources the structured answer from hover text lowered to `TypeExpr` (the
   only verified point-query path) rather than from a structured tsgo reply that
   does not exist. The relation/call families keep §6.3's structured-payload +
   per-family-budget model under a future structured oracle.

---

## Scope + kind-eligibility gate

**The PARITY CLAIM (what this harness asserts).** This harness asserts
**structural `TypeExpr`-projection parity** — Verter's in-process `TypeExpr`
projection (`resolve_expr` → `project_node_to_type_expr`, `support.rs:132`) vs the
`TypeExpr` lowered from TS7's hover answer — NOT full TypeScript nominal/semantic
identity. The comparison value is a normalized `TypeExpr` on BOTH sides (§Q2);
neither side carries the parts of TS's type identity that `TypeExpr` cannot
represent. Any TS construct whose meaning lives in nominal/semantic identity that
`TypeExpr` has no carrier for — enum-member brand, `unique symbol`, private /
protected member brand, `this`-type identity, `const`/variance type-parameter
identity, abstract-constructor identity — is therefore **NOT in scope for hover
sourcing and is DEFAULT-REJECTED** (§Q2 positive allowlist). Where TS7's structural
answer and Verter's structural projection legitimately differ on such a nominal
construct, the `(row, query)` is **deferred** to a future structured/nominal oracle,
NOT force-fit into a `TypeExpr` compare. The relation / call-resolution /
assignability families are deferred for a different reason (their answer is a verdict,
not a `TypeExpr`); both deferrals route to the same out-of-scope future oracle work.

**The construct gate is a CLOSED POSITIVE ALLOWLIST; default-REJECT is THE rule.**
The §Q2 admission gate is a closed positive allowlist (§Q2 enumerates exactly the
ADMITted constructs). The enumerated REJECT entries in §Q2 are **ILLUSTRATIVE
EXAMPLES, not an exhaustive catalogue** — ANY construct (named or unnamed, present in
the tree today or introduced by a future TS/fixture change) that is not on the
positive ADMIT list is **rejected by default**. An un-enumerated lossy construct is
therefore ALREADY handled (rejected), not a coverage gap: the gate's soundness does
NOT depend on the design enumerating every lossy construct, only on the ADMIT list
being a closed minimal set of provably-lossless constructs. This forecloses the "you
missed construct X" failure class by construction.

This harness serves **`TypeExpr`-valued type-projection rows/queries ONLY**. A
query is **eligible** for a hover-lowered snapshot ONLY when ALL of these hold —
the gate is **structural, not by `OracleId` name**:

1. **The assertion is a `TypeExpr` projection.** The query's `oracle_value_kind` in
   the registry (§Q4) is `structured_type_expr`. The query resolves a symbol/
   expression to a `TypeExpr` via `resolve_expr` (or a sibling `TypeExpr`-returning
   helper), and the row asserts on that `TypeExpr`'s structure.
2. **It does NOT assert a relation / call verdict.** A query that asserts
   assignability, overload selection, or call resolution is **ineligible**. The
   `≤122` ceiling below is the count of oracle rows that are **`Relate`-free** (carry
   no `SemanticQueryName::Relate` anywhere) — an UPPER BOUND on the candidate set.
   THIS eligibility rule is a SEPARATE, FURTHER narrowing WITHIN those `Relate`-free
   rows: a row is ineligible whenever its **asserting key** is a relation/call
   verdict — i.e. it asserts a relation verdict — even if it happens to carry no
   `Relate` query. The two are not the same test: "carries `Relate` anywhere"
   determines the `≤122` ceiling; "asserts a relation/call verdict as its key" is the
   eligibility gate applied to a `Relate`-free candidate. A future `ResolveCall` /
   relation-verdict asserting key is treated identically — ineligible.
3. **Its assertion does NOT additionally prove non-`TypeExpr` behavior — and the
   gate reads a MACHINE-READABLE obligation marker.** A thin `TypeExpr` structural
   compare proves the projected SHAPE only. Some non-`Relate` oracle rows ALSO assert
   dependency-footprint / audit behavior — e.g.
   `flow_return_xf04_records_barrel_route_before_selected_leaf`
   (`flow_return_catalog.rs:1496`) asserts the barrel-route footprint via
   `assert_cross_alias_warm_with_dependency_footprint`, not just the resolved
   `TypeExpr`. Once such a body is lifted to `oracle::run_row(…)` its original extra
   footprint/audit assertion is GONE, and `IgnoredTestRow` carries no assertion-kind
   field — so "exclude rows that also assert footprint/audit" is unenforceable as a
   prose rule. The gate is made enforceable by **promoting the proof requirement**: a row
   that ALSO asserts any INDEPENDENT non-`TypeExpr` obligation — dependency-footprint,
   audit-record, warm-cache / declared-dependency facts, OR a divergence correction — MUST
   carry `ProofRequirement::OracleAndGuard { oracle, guard }` (the `oracle` half proves the
   shape; the `guard` half names a REGISTERED LIVE PROVER for the independent obligation),
   rather than a bare `Ts7Oracle`. The obligation is a property of the ROW's original BODY
   (which assertions it carried); it is expressed by the `OracleAndGuard` proof shape plus
   the live prover the `guard` resolves to — NOT stored as a typed obligation SET on any
   ledger record. The five obligation KINDS the `guard` provers cover are
   `DependencyFootprint` / `AuditRecord` / `WarmCache` / `DeclaredDependency` /
   `DivergenceCorrection`; each KIND has a registered prover (conceptually in
   `OBLIGATION_GUARD_REGISTRY`) that re-asserts the ORIGINAL expectation against the live
   result — the dependency footprint's `includes`/`excludes` paths, the audit-record
   fields, the warm-cache facts, the declared-dependency ids, or the divergence-correction
   data tie — not merely re-runs the query (§Q4). The prover re-asserts against the CORRECT
   query's live result (the prover knows which `query_ordinal` it is over), never "any query
   passes". The `kind_eligibility_gate` therefore **REJECTS a BARE `Ts7Oracle` row that
   carries an INDEPENDENT non-`TypeExpr` obligation** (such a row must be promoted to
   `OracleAndGuard`), AND under `OracleAndGuard` verifies that the proof's `guard` resolves
   to a registered live prover in the checked-in CODE `OBLIGATION_GUARD_REGISTRY`
   (`GuardId → { obligation_kind, expectation_tag, prover fn }`, §Q4 — the §4 guard table
   is its human-readable mirror; the membership check is a registry lookup against a
   compiled `fn` symbol, not a Markdown-name match). A row carrying an independent
   non-`TypeExpr` obligation is admissible ONLY under `OracleAndGuard` (a divergence row is
   an `OracleAndGuard` whose `guard` is the `DivergenceCorrection` prover, §Q4 /
   `ts-compat-two-mode-model.md` §9.2). (The round-2 stored obligation ledger was retired:
   obligations live in the proof shape + the registered prover, not a per-row typed set.)

   **Query mode is oracle query IDENTITY, not an obligation.** A row's
   `assert_query_mode(M)` that MATCHES the oracle query's own `projection_mode` is NOT a
   non-`TypeExpr` obligation: the driver resolves Verter's projection in mode `M`, and the
   live audit record reporting mode `M` IS the proof (stronger than duplicating the mode
   into a ledger). That query-mode identity is proven for every registry query by
   `lifted_row_audit_query_mode_matches_spec` (the live audit `query_mode` equals the
   spec's declared `projection_mode`), so a same-mode `assert_query_mode` adds NO obligation
   and the row stays bare `Ts7Oracle`. The four seated first lifts (two index-signature
   publications + two built-in modifier utilities) each carried exactly such a same-mode
   `assert_query_mode(Expanded)` and are therefore seated bare `Ts7Oracle` in `Expanded`.
   Only an INDEPENDENT non-`TypeExpr` assertion (the kinds above) promotes a row to
   `OracleAndGuard`.

**Why structural, not nominal.** Family membership (`OracleId`) is a coarse
category; a single family can mix `TypeExpr`-projection rows and verdict rows.
Eligibility is decided one `(row, query)` at a time on what the query ASSERTS, never
on the family name or the row's identifier — the same role-classification discipline
the Typed-IR-Only Resolver Rule mandates.

**The 122-row UPPER BOUND — one rule, three layers.** State it as ONE rule with
three nested narrowings, never as two conflated tests:

1. **`≤122` = the `Relate`-FREE oracle rows (the UPPER BOUND).** Because 218 of the
   ~340 oracle rows CARRY `SemanticQueryName::Relate` anywhere (fact 6), only **122
   oracle rows are `Relate`-free**. tsgo hover cannot supply a relation verdict, so the
   `Relate`-free set is the ceiling: this harness covers **at most 122 rows**. This is
   a coarse carries-`Relate`-anywhere filter — an UPPER BOUND, not an eligibility count.
2. **The asserting-key gate narrows WITHIN the 122.** Among those 122 `Relate`-free
   rows, a row is still ineligible if its **asserting key** is a relation/call verdict
   (eligibility rule 2 above). This is a STRUCTURAL narrowing of the upper bound, not the
   same test as layer 1 — a `Relate`-free row can still assert a verdict and is rejected.
3. **The construct allowlist + footprint/audit exclusion narrow FURTHER.** The §Q2
   positive construct allowlist defers any remaining row whose TS7 type uses a
   non-admissible construct, and the footprint/audit exclusion above removes any
   bare-`Ts7Oracle` row whose assertion ALSO proves dependency-footprint / audit
   behavior.

The doc never claims "~340" and never claims a flat "122 lifted" — 122 is the
`Relate`-free ceiling; the asserting-key gate, the construct allowlist, and the
footprint/audit exclusion each narrow further, so the lifted set is **strictly lower
than 122**.

**The initial admissible set is a SMALL, PROVABLY-SOUND CORE that GROWS spike-by-spike —
122 is an upper bound, NOT a near-term lift count.** The harness lands first as a small
core of constructs/modes the §4 spike has PROVEN lossless + confluent: empty-`type_args`
`ResolveExpr` / `ShallowSurfaceExpr` and single-root `EvaluateExpr` in `Shallow` /
`Navigate` mode over the spike-validated admissible construct set, standalone-host only.
Every harder class — `Expanded` mode, parameterized `type_args`, multi-referent
`EvaluateExpr`, `workspace_footprint` rows, lib-dependent rows, anti-shadow-needing rows —
is DEFAULT-REJECTED (stays `Ignored`) until its own named blocking spike proves it sound,
at which point it joins the admissible core under a version bump. The honest posture is a
SMALL sound core that grows as spikes discharge each class's proof obligation — NOT "lift
~122 rows soon." The 122 figure is a `Relate`-free CEILING used only to bound the maximum,
never a target or a near-term count.

**Out of scope — future oracle kinds.** Relation / call-resolution /
overload-selection / assignability verdicts need their OWN future oracle kinds —
`relation_verdict` and `call_resolution_verdict` — sourced from a **structured
checker path** (a compiler-API harness or tsserver `quickinfo`), NOT from hover
text. The `oracle_value_kind` field (§Q1) is the documented extension point: a future
block adds a new discriminant + its own structured source and keeps §6.3's
per-family budget model. Those families are explicitly NOT lifted by this harness.

**Out of scope — legitimate `any` / `never` answers (deferred class).** A few
oracle rows have a genuine top/bottom answer: `Parameters<any>` / `InstanceType<any>`
(`typeinfo_ignored_test_manifest_rows.rs:340,343`), `Awaited<never>` /
`NonNullable<never>` (`:347` + siblings) resolve to a real `any` / `never`. The §Q2
backstop REJECTS `any` always and `never` outside a genuine closed empty union, so a
`TypeExpr` snapshot for these would either false-reject or require weakening the
backstop ad hoc. These rows are therefore **permanently INELIGIBLE for this hover
harness** — a deferred class, marked ineligible rather than admitted. They are rare,
and the future structured oracle (which can carry an explicit expected-top/bottom
policy) covers them. The backstop's `any`-always / `never`-mostly reject rule stays
strict and is NOT relaxed for them.

**Host setup kinds — ONLY `standalone` is first-class; `workspace_footprint` and
package-backed / custom-host are BOTH deferred to named spikes.** A TS7 hover answer
depends on the host/project setup, not only the workspace files. The DOMINANT typeinfo
host is `make_host_with_footprint()` = `VerterHost::new_standalone` (fact 9,
`support.rs:89`, `host_construction.rs:249`) — a default `MemoryWorkspace` with **NO
project root and NO tsconfig** — used by ~369 call sites including the 122 `Relate`-free
candidates. Only ~9 rows use `make_host_with_workspace_files_footprint` (`/workspace` +
a `/workspace/tsconfig.json` `IdeProjectConfig`, `support.rs:97`). The
`identity.host_project.host_setup_kind` (§Q1) names the kind, and ONLY the `standalone`
kind is initially admissible:

- **`standalone` (the default — the ONLY first-class kind).** The host has no
  tsconfig/root, so the generator drives tsgo under ONE deterministic CANONICAL oracle
  tsconfig + synthetic root — the same config for every standalone row
  (`oracle.tsconfig.json`, §Q2 "Env pinning") — yielding a stable
  `compiler_options_hash`. That synthesized tsconfig + the libs it pulls are vendored
  into the ONE shared closed corpus (`env_corpus_id`, §Q1), so the standalone-row env is
  pinned and offline-re-derivable. The tsgo-free `snapshot_id` derivation (registry +
  pinned env constants, no per-snapshot env hash in the filename) is airtight EXACTLY
  because every standalone row shares this SINGLE canonical corpus + config. This is the
  resolution for the dominant population — standalone rows are FIRST-CLASS.
- **`workspace_footprint` (the ~9-row minority — DEFERRED to a named spike).** The host
  carries a real `/workspace` root + `/workspace/tsconfig.json`, so each
  `workspace_footprint` row drives tsgo under its OWN project config rather than the one
  shared canonical config. That breaks the single-shared-corpus assumption the
  tsgo-free `snapshot_id` derivation rests on: a per-host project config + per-host
  consulted env would need PER-HOST env/option pins (a distinct `compiler_options_hash`
  and a distinct vendored corpus per host), not the ONE shared `env_corpus_id` /
  `compiler_options_hash` the schema pins. Rather than fork the env schema, the
  `workspace_footprint` class is **DEFERRED** to a named blocking spike (§4 — "the
  `workspace_footprint` per-host env-pin spike") that decides how per-host project
  config + consulted env are pinned and offline-re-derivable. Until that spike lands,
  `workspace_footprint` rows stay `Ignored`. (The `host_setup_kind` enum still CARRIES
  the `workspace_footprint` discriminant so the schema is total, but no row is admitted
  under it initially.)
- **package-backed / custom-host (DEFERRED to a spike).** Rows that inject a
  `node_modules` package + a custom project config (`make_package_host_with_workspace`,
  `cache_invalidation.rs:344`) are a DEFERRED class for the same reason plus an
  un-vendored ambient package corpus and a non-standard host the generator does not
  drive. They stay `Ignored` and re-enter only once their per-host env + full consulted
  `.d.ts` + manifest corpus are pinned by that same env spike. (The row-INJECTED
  package case — a package file a `standalone` row adds to its OWN workspace — is
  resolved separately as a per-row `workspace_files` entry, §Q4
  `row_injected_packages_are_workspace_files`; that is NOT the deferred custom-host
  class.)

**Out of scope — multi-/nested-referent `EvaluateExpr` expressions (default-rejected;
deferred to a named spike).** An `EvaluateExpr` query is admissible ONLY when its
expression matches the closed SINGLE-ROOT grammar (§Q2 "`EvaluateExpr` admission is
restricted to a closed single-root grammar"): one binder reference, optionally with a
trailing index/property path whose roots all resolve to that same binder. A single
`SourceLocator` walks exactly one root declaration, so an expression with MULTIPLE or
NESTED referents (`A | B`, `keyof T`, `ReturnType<typeof f>`, an indexed-access root
chain across two binders, a namespace/property head) would leave a non-leading lossy
contributor un-allowlist-checked → false parity. Such expressions are **DEFAULT-REJECTED**
(stay `Ignored`) and deferred to the named blocking spike for a parsed locator-set walk
(§4). This is THE EvaluateExpr admission posture — `ResolveExpr`/`ShallowSurfaceExpr`
already have a single declaration target by construction. Pinned by
`evaluate_expr_admission_is_single_root`.

**Out of scope — multi-contributor rows (default-rejected; deferred to a named
membership-revalidation spike). This CLOSES the last offline gap.** The offline
`source_admission_digest_consistent` gate re-parses and re-checks every RECORDED
contributor, but it cannot RE-NAVIGATE the import/merge/transitive declaration graph to
PROVE the recorded contributor SET is complete — WHICH files / merged peers / augmenters
are contributors is established at generation time by the live resolver and is not
offline-reproducible. A digest that silently omitted an imported / merged / augmented /
transitive-`typeof` contributor could therefore self-validate offline. To make the offline
guarantee COMPLETE rather than carry that residual, initial admission is RESTRICTED to
**PROVABLY SINGLE-CONTRIBUTOR** rows: the queried symbol's source-side walk
(`resolve_source_declarations`, §Q2) must resolve to EXACTLY ONE contributor declaration in
a SINGLE FILE, with NO import / re-export hop, NO `MergedDecl` peer, NO ambient
`declare module` / `declare global` augmentation contribution, and NO transitive
`typeof` / `ReturnType` / `Parameters` hop to a second declaration — so the contributor set
is trivially `{the one decl}` and is FULLY offline-verifiable (the one recorded file's source
is re-parseable, and there is provably nothing else to discover). The generator
DEFAULT-REJECTS any row whose source-side walk reaches >1 contributor or crosses a
file / merge / augmentation / transitive hop (it stays `Ignored`), and DEFERS the
multi-contributor class to the named blocking spike (§4 — "the offline
contributor-set-membership-revalidation spike") that must define how the recorded
contributor SET is re-validated offline (or default-reject the class). The honest
"contributor-set membership is not offline-re-navigable" residual the
`source_admission_digest` field + `source_admission_digest_consistent` guard both name is
thereby MOOT for the admitted set — re-navigation has nothing to discover when the set is
provably `{one decl}`. Pinned by `source_is_provably_single_contributor`.

**Out of scope — `workspace_footprint` rows (deferred to a named env-corpus spike).**
Only `standalone`-host rows are first-class in the initial harness (§Q1 host-setup, the
host-setup-kinds discussion below). The `workspace_footprint` row class (the ~9-row
minority) is **DEFERRED** to a named blocking spike — its tsgo-free env-derivation is
NOT yet airtight (see the host-setup discussion below for why). It stays `Ignored` until
that spike lands.

**Out of scope — unspiked `Expanded`-mode AND `Skeleton`-mode queries (deferred until
their spikes).** The FIRST implementation block admits only `Shallow` / `Navigate` mode
(and any mode the generation spike, §4, validates as lossless). All unspiked
`Expanded`-mode AND `Skeleton`-mode queries stay `Ignored` until the blocking probe-form
spike lands and versions a demonstrably lossless probe form for that mode (a
`probe_synthesis_version` bump); forcing tsgo to print an alias's expanded body while
preserving methods / call-signatures / optional / readonly modifiers (`Expanded`), or to
print the `TypeParameter`/`Infer` shell semantics for unbound generics so Conditional
branches do not collapse to `never` (`Skeleton`), are both non-trivial tsgo-printing
questions gated on their spikes (§Q2, §4). Only `Shallow` / `Navigate` (and
spike-validated `Expanded` / `Skeleton`) rows are admissible initially.

---

## 2. Design

The harness has three separable concerns:

- **Generation** (build/test-time only): for each registry query spec, synthesize a
  FIXED, VERSIONED probe in the workspace, drive tsgo over it, capture the hover
  answer, parse it to an OXC type AST, admit it through the **positive
  allowlist (default-REJECT)** checked on BOTH the hover AST AND the fixture SOURCE
  declaration + a strict-lowering drop-counter, lower the admitted AST to a
  `TypeExpr`, normalize, write a checked-in snapshot.
- **Storage**: where snapshots live, how they are named, their JSON schema, the
  metadata that pins them to a tsgo version + the EFFECTIVE compiler options + a
  full resolved-file-set oracle env hash + `normalizer_version` +
  `probe_synthesis_version`.
- **Consumption** (default test path): a lifted row carries the `#[oracle_row]` attribute
  proc-macro (which reads the test fn's own `ItemFn` identifier as the row key — no
  hand-typed key, §Q4) and synthesizes a body that calls the **shared registry
  driver**, which reads the row's `(row_file, row_function, *)` registry query specs,
  runs the helper named by each spec's `query_helper_kind`, loads each spec's snapshot
  via runtime `std::fs::read` from
  `concat!(env!("CARGO_MANIFEST_DIR"), "/src/typeinfo/typeinfo_tests/oracle_snapshots/", oracle_family, "/", snapshot_id, ".json")`,
  normalizes Verter's in-process `TypeExpr`, and asserts structural equality under the
  normalization. No tsgo at consumption time. Because the query payloads live in the
  registry (not the body), query coverage is true **by construction** — and (in the
  DEFERRED §Q4 per-row-count layer, not yet a shipped `IgnoredTestRow` field) an
  independent declared `oracle_query_ordinals` count on the manifest row will
  cross-check the registry entry count so an under-counting registry cannot hide a
  missing query.

### Q1 — Snapshot directory + JSON schema (DECIDED)

**Directory.** Snapshots live under the typeinfo test tree, beside the fixtures
they pin, in a dedicated sibling directory:

```
crates/verter_session/src/typeinfo/typeinfo_tests/oracle_snapshots/
```

Justification:

- The lifted rows live in `crates/verter_session/src/typeinfo/typeinfo_tests/`
  and their fixtures live in the sibling `fixtures/` dir (`support.rs`). Co-locating
  snapshots in a sibling `oracle_snapshots/` dir keeps the proof artifact next to the
  row that consumes it. This satisfies the testing-hermeticity rule: snapshots are
  locally-vendored fixtures checked into the repo.
- **Loading is runtime `std::fs::read`, NOT `include_str!`/`include_bytes!`.** The
  `snapshot_id` (Q4) is derived at test time from the registry entry + the current
  fixture/env hashes, so the path is not a compile-time-known string and cannot be a
  macro argument; the driver builds the FULL path
  `concat!(env!("CARGO_MANIFEST_DIR"), "/src/typeinfo/typeinfo_tests/oracle_snapshots/", oracle_family, "/", snapshot_id, ".json")`
  and reads the bytes with `std::fs::read`. The `env!("CARGO_MANIFEST_DIR")` prefix
  alone resolves to `crates/verter_session/`, so the full
  `/src/typeinfo/typeinfo_tests/oracle_snapshots/` infix is REQUIRED — joining only
  `oracle_snapshots/…` to the manifest dir would read from
  `crates/verter_session/oracle_snapshots/`, the wrong place. The driver knows
  `oracle_family` from the registry entry (§Q4 — each entry carries it), so it can
  build the `<family>/<snapshot_id>.json` tail at test-body time. There is NO
  generated `include_str!` table, NO `include_dir!`, NO embedded snapshot blob — a
  second embedded artifact would be a shadow registry that drifts, and the no-orphan
  guard must enumerate the on-disk tree regardless. Rooting at
  `env!("CARGO_MANIFEST_DIR")` keeps the read hermetic and absolute-path-free: it
  resolves to the in-repo crate dir, not a developer-machine path.
- It is NOT under `crates/verter_session/tests/` (the integration-binary tree)
  because the rows that consume them are **unit** tests in
  `src/typeinfo/typeinfo_tests/`, driven by the shared registry driver over
  `resolve_expr` / `shallow_surface_expr` / `evaluate_expr`. The oracle-query-spec
  registry ALSO lives in `src/typeinfo/typeinfo_tests/oracle_query_specs.rs` (§Q4) so
  the unit-test bodies can reach it; the `tests/` guard consumes that same table via a
  shared crate-internal path. The manifest (the row ledger) stays in `tests/`. None of
  these store snapshot bytes.
- It is NOT a top-level `fixtures/` or `.integration-tests/` dir — those are
  reserved for external corpora and for the SFC compile fixtures.

**File naming.** One JSON file per `(row, query)` snapshot — one per registry query
spec, keyed `(row_file, row_function, query_ordinal)`. **Snapshots are NEVER shared
across rows or queries** (no `row_refs: []` many-to-one coupling): a duplicate tiny
JSON file is cheaper and far safer than coupling several rows' proof lifecycles to one
file. Each file is named by the deterministic snapshot id (see Q4):

```
oracle_snapshots/<oracle_family_snake>/<snapshot_id>.json
```

e.g. `oracle_snapshots/utility_composition/u_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2.json`. The family
sub-directory is the snake_case `OracleId` — a **directory / presentation key only**:
it keeps the tree browsable and lets the per-family guard scope a recursive glob, but
it is deliberately **EXCLUDED from `snapshot_id`** (which is keyed by the row-ref +
value-affecting identity, §Q4). The file stem is the content-derived `snapshot_id`.
The snapshot id is ALSO stored inside the JSON (so a misplaced file is detectable).

**JSON schema.** A snapshot is a single JSON object. Required top-level fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `oracle_schema_version` | integer | Version of THIS snapshot FILE SHAPE (the field set + the per-kind `identity` shape). Bumped when the schema fields change AND whenever a new `oracle_value_kind` is added (a new kind carries a different required `identity` shape, so it is a schema change). Forces regeneration; old snapshots fail the version gate. |
| `normalizer_version` | integer | Version of the NORMALIZATION + OXC-lowering ALGORITHM that produced `oracle_value`. DISTINCT from `oracle_schema_version` so a normalizer change (without a file-shape change) forces regeneration on its own. Enters `snapshot_id`. |
| `probe_synthesis_version` | integer | Version of the PROBE-SYNTHESIS + HOVER-DRIVER + HOVER-EXTRACTION + ADMISSIBILITY algorithm — the probe form/naming, the parameterized-`ResolveExpr` `TypeExpr` → TS-source type-argument printer (§Q2), the **pinned tsgo HOVER-DRIVER CONFIG** (the LSP `initialize` payload + declared CLIENT CAPABILITIES, the `workspace/didChangeConfiguration` / `initializationOptions` block, the `textDocument/hover` request payload, and the hover DISPLAY PREFERENCES tsgo formats the hover under — see §Q2 "Hover-driver config"), the hover-extraction grammar (§Q2 "hover-extraction grammar"), the positive-allowlist (default-reject) admission, and the two-sided source+hover check. DISTINCT from `normalizer_version` (which covers normalization + lowering only): a probe-form / printer / driver-config / extraction / admission change can change the produced hover/value without changing normalization, so it forces regeneration on its own. Folding the hover-driver config in here means two sessions with different hover protocol / display config cannot produce different captures under the same `snapshot_id`. Enters `snapshot_id`. |
| `tsgo_version` | string | The pinned tsgo version that produced the value (`"7.0.0-dev.20260526.1"`). |
| `compiler_options_hash` | string (hex) | Stable hash of the exact EFFECTIVE oracle tsgo config (defined in §Q2 "Env pinning"). A compiler-option drift invalidates the snapshot. Necessary but NOT sufficient — see `oracle_env_hash`. |
| `env_corpus_id` | string (hex) | **The content id of the CLOSED, VENDORED oracle-env corpus — the SHARED ambient/lib/package/tsconfig corpus ONLY, NOT per-row workspace files** — the hash of the full vendored SHARED file set (every relative path + content hash + the directory manifest). The shared corpus is the canonical host's vendored ambient corpus: every vendored ambient / lib / package `.d.ts`, package manifests (`package.json` `types`/`exports`), and tsconfig/project metadata (incl. the canonical `oracle.tsconfig.json`). The per-row workspace files are NOT part of this corpus — they live SEPARATELY in `identity.workspace_files` (a distinct identity axis, below) and are hashed into `snapshot_id` through THAT axis. The generator vendors the SHARED corpus (copies the bytes, below) and computes this id once per closed corpus. Because the canonical oracle corpus is the SAME for every standalone-host row, `env_corpus_id` is a STABLE pinned-env constant (like `compiler_options_hash`); it ENTERS `snapshot_id` as a pinned-env constant, keeping the filename registry-derivable (the registry + pinned env know it without opening a snapshot or running tsgo). Regeneration re-vendors the shared corpus and recomputes `env_corpus_id`. |
| `oracle_env_files` | object `{ manifest: [path…], files: [{ path, content_hash }] }` | **The stored SHARED-CORPUS manifest** — the relative path of EVERY file in the closed vendored SHARED corpus directory (`manifest`, the COMPLETE listing) plus each file's content hash (`files`). The shared corpus contains the SHARED ambient/lib/package/tsconfig env ONLY: every ambient / lib / package `.d.ts` tsgo consulted, AND the resolution METADATA — package manifests (`package.json` `types`/`exports`) and tsconfig/project metadata (incl. the canonical `oracle.tsconfig.json` for standalone-host rows). It does NOT include per-row workspace files — those are a distinct domain in `identity.workspace_files`. REQUIRED in every snapshot. The bytes are CHECKED IN under the vendored corpus dir (below) — NOT live `node_modules` paths (gitignored, non-hermetic; tsgo bundles libs under `node_modules/@typescript/native-preview-*/lib/`, `ipc.rs:~2859-2874`, `.gitignore:9`). Including the manifests is load-bearing: a `package.json` `types`/`exports` change re-selects a DIFFERENT `.d.ts` while the stored `.d.ts` hashes stay unchanged (fact 8, `ipc.rs:3686`, `cache_invalidation.rs:324`), so a `.d.ts`-only corpus would miss that drift. This is what makes `oracle_env_hash` (and the snapshot's validity) **re-derivable / re-validatable OFFLINE without tsgo**: the consumption test AND `no_orphan_snapshot` RE-ENUMERATE the vendored corpus directory, assert SET-EQUALITY against the stored `manifest` (no unlisted file present, none missing — catching a newly-ADDED file), THEN recompute `oracle_env_hash` by re-hashing the `files` list against current on-disk content — never re-running tsgo. |
| `oracle_env_hash` | string (hex) | Content hash over the **CLOSED VENDORED SHARED corpus** — exactly the FULL set of files enumerated in `oracle_env_files.files` (the complete `oracle_env/<env_corpus_id>/` directory listing, NOT a per-query-consulted subset): every vendored ambient / lib / package `.d.ts` (the `@verter/types` Vue macro decls, lib `*.d.ts`, ambient `@types`, module-augmentation files) PLUS the resolution metadata (package manifests, tsconfig/project metadata). It covers the SHARED ambient/lib/package/tsconfig corpus ONLY; per-row workspace files are NOT in this hash — they enter `snapshot_id` separately via `identity.workspace_files`. The vendored corpus is the CONSERVATIVE SUPERSET for the canonical host config — every file tsgo could resolve through under that config, vendored once — not the minimal slice any single query touches. Spans the resolve / type / lib / project dimensions Verter's own model splits across `lib_env_hash` + `project_identity` (fact 8, `env_hash.rs:84,99,219,239`). Because tsgo is driven against the FROZEN vendored corpus (not live `node_modules`), the set is closed and re-enumerable: the offline gate asserts the vendored directory's CURRENT listing set-equals the stored `manifest` BEFORE content-hashing, so an ADDITION (membership) is caught as well as an edit/delete (content). `env_corpus_id` and `oracle_env_hash` are TWO DOMAIN-SEPARATED digests with DIFFERENT roles + DIFFERENT recipes — they are NOT required to be byte-equal (the example's `env_corpus_id` and `oracle_env_hash` are deliberately different values). `env_corpus_id` is the STABLE corpus-content IDENTITY that enters the FILENAME (`snapshot_id`): BLAKE3, domain-separated under the `env_corpus_id` tag, over the CORPUS LISTING — the canonical-path-sorted `[{ path, content_hash }]` pairs of the full vendored SHARED corpus (path + per-file content hash, no file bytes inline). `oracle_env_hash` is the VALUE-validated content hash recomputed ON READ: BLAKE3, domain-separated under the `oracle_env_hash` tag, over `oracle_env_files.files` (the same canonical-path-sorted `{ path, content_hash }` list) recomputed against current on-disk content. The two digests share the same underlying file set but are domain-separated (distinct tags, distinct roles) so they are intentionally distinct hex values; neither is derived from the other. `oracle_env_hash` is recomputable offline from the stored `oracle_env_files` + current on-disk content. **`oracle_env_hash` does NOT enter `snapshot_id`** (the filename) — it is validated as a VALUE on read (consumption + `no_orphan_snapshot` re-enumerate + recompute it and FAIL on mismatch). This keeps the filename registry-derivable while still catching env drift; the STABLE corpus identity that DOES enter `snapshot_id` is `env_corpus_id`. A change to ANY vendored ambient / package / project / manifest file (edit, add, or delete) recomputes a different `oracle_env_hash` (or fails set-equality) and INVALIDATES the snapshot — a `compiler_options_hash` match alone does NOT validate it. |
| `oracle_family` | string | The `OracleId` (snake_case) — DIRECTORY / presentation key only; selects the sub-directory. EXCLUDED from `snapshot_id`. |
| `oracle_value_kind` | string enum | `"structured_type_expr"` for every snapshot this harness writes. The documented EXTENSION POINT for future non-`TypeExpr` oracle kinds (`relation_verdict`, `call_resolution_verdict`), each from its own future structured driver — out of scope here. A new kind is a CLOSED-TAGGED addition that requires its own `identity` shape and an `oracle_schema_version` bump. Enters `snapshot_id` so a future kind cannot collide with a `structured_type_expr` one for the same identity. |
| `snapshot_id` | string | The deterministic id from Q4 (see derivation). Must equal the file stem. Derived from REGISTRY-ONLY, tsgo-free inputs (row-ref + query-helper payload + `host_project` + pinned env/algorithm versions, including the STABLE `env_corpus_id` pinned-env constant) — `oracle_env_hash` is NOT an input, so a coverage guard can compute the expected filename set from the registry ALONE. The full ≥256-bit BLAKE3 digest (not a 12-byte truncation). INCLUDES the row-ref. |
| `row_ref` | object | `{ row_file, row_function, query_ordinal }` — the registry key (§Q4) this snapshot serves. `row_file` is the BARE filename (e.g. `"utility_composition.rs"`, fact 5). This row-ref is PART of `snapshot_id` (one file per `(row, query)`); it is duplicated in the JSON for drift detection but the registry is the source of truth. |
| `identity` | object | The value-affecting query identity, a **closed tagged shape keyed by `oracle_value_kind`**. For `structured_type_expr` the required axes are the `TypeExpr`-projection axes below. A future `relation_verdict` kind carries a DIFFERENT required axis set (source / target / relation / policy / inference-context) — kinds are a closed tagged schema, not an additive bag of optional fields, so adding one bumps `oracle_schema_version`. Re-derivable from the registry + fixture content; carried for inspection + drift detection. |
| `oracle_value` | object | The captured, NORMALIZED oracle answer — a `TypeExpr::to_json_value()` document (the internally-tagged `"kind"`/`"properties"`/`"memberKind"` codec, fact 2). **Single-spec reframing (no schema change):** this is specifically the **`ts_compat`** oracle — the recompute-gated tsgo answer, bug included, the recorded `TsCompat` value. The **correct** value for a divergence row lives in a SEPARATE review-gated correction overlay (`ts-compat-two-mode-model.md` §3), never as a second field here; injecting a non-tsgo-regenerable `correct` value would break this artifact's "regenerate → byte-identical" guarantee. |
| `raw_capture` | object (REQUIRED) | The verbatim un-normalized tsgo hover response plus the probe header: `{ probe_name, probe_header, hover_contents }`. **MANDATORY in every snapshot** — it is what lets the default (tsgo-free) guards AUDIT the wrong-hover fence offline: `probe_header_names_target` re-checks that `raw_capture.hover_contents` contains a `type __oracle_probe__N = …` header naming exactly `raw_capture.probe_name`, without re-running tsgo. Never compared against Verter's `TypeExpr` / asserted as the parity value (the parity compare is on `oracle_value` only); it is the offline audit + regeneration record. |
| `source_admission_digest` | object (REQUIRED) | The recorded GENERATION-TIME source-side admission record, MANDATORY in every snapshot: an ORDERED, KEYED `contributors` vector the §Q2 source-side walk (`resolve_source_declarations`, across import chains + merged contributors + transitive `typeof`/`ReturnType` hops) resolved — EACH entry a `{ contributor_ordinal, decl_span, decl_canonical, name, symbol_space, decl_kind, raw_surface, lowered_body, verdict }` record carrying that contributor's STABLE identity (`contributor_ordinal` = the 0-based source/binder position in the merge group, `decl_span` = the stable decl id — together disambiguating same-`(canonical,name,symbol_space)` merged peers), a verbatim copy of its retained parse-time `RawSourceSurface` raw-fact record (`raw_member_keys`, `member_kinds`, `member_visibility`, `unique_symbol_ops`, `abstract_ctor`, `type_param_modifiers`, `this_type_or_param`, `value_const_assertion`, `overload_signatures`, `tuple_element_shape`, `utility_referent_names`, `transitive_referents`), a verbatim copy of its already-lowered `lowered_body` `TypeExpr` (the non-erased rejectable variants the raw facts do not carry), and that contributor's ADMIT/REJECT `verdict` — plus the final two-sided admission verdict, AND the PROVENANCE TIE — the `source_locator` it walked from + the content hashes of every source declaration file it observed (the same `{path, content_hash}` recipe `workspace_files` uses). `raw_capture` stores only the HOVER, so `raw_capture_matches_oracle_value` can re-run the HOVER-side admission + lowering offline but CANNOT re-run the SOURCE-side contributor NAVIGATION (binding/import/merge/transitive-`typeof` resolution needs the live resolver, a generation-time step). The digest closes that offline asymmetry NOT by self-checking its own recorded data (circular — a hand-edited digest that omits a rejected fact would still self-agree) but by re-deriving the raw facts + lowered body FROM CURRENT SOURCE: because `RawSourceSurface` is a PARSE-TIME artifact (captured by shallow parsing, NOT type resolution) and the lowered body is the deterministic `lower_ts_type` of the parsed decl, the offline tsgo-free + resolver-free gate CAN, for EACH recorded contributor, take that file's CURRENT source — resolved BY CANONICAL PATH through the total canonical-path→source mapping. The SOURCE-BYTE AUTHORITY is the REGISTRY: a leading-slash `/fixtures/...` / `/workspace/...` row-or-workspace file's source is the matching `workspace_files` entry's UPSERTED SOURCE BYTES in the row's registry entry (§Q4 — the registry carries `{ path, source }`, the bytes the test upserts); the snapshot's `identity.workspace_files` carries only `{ path, content_hash }`, so the gate re-parses the REGISTRY source for that canonical path and VERIFIES it against the snapshot's stored `content_hash` for the same path (a mismatch FAILS — the registry bytes must hash to the snapshot's recorded hash). A vendored corpus file's source IS on-disk under `oracle_env/<env_corpus_id>/` (verified against the corpus's recorded hash the same way). The gate then RE-PARSEs the resolved source, RE-CAPTUREs the `RawSourceSurface` raw facts AND RE-LOWERs the body for the recorded contributor identified by `(decl_span, contributor_ordinal, name, symbol_space, decl_kind)`, and COMPAREs the freshly-captured `(raw_surface, lowered_body)` pair to the digest's recorded pair (catching a within-file fact OMISSION or TAMPER in EITHER half of EITHER merged peer), then RE-RUNs the CURRENT-version source-side positive allowlist over the freshly-captured pair (so a snapshot admitted under an OLDER allowlist version a later version would REJECT now FAILS — allowlist-version drift is caught, not trusted). The gate then asserts: the digest's `source_locator` EQUALS the registry entry's `source_locator`; each recorded `{path, content_hash}` EQUALS the hash of the CURRENT registry-`workspace_files` SOURCE (or vendored-corpus on-disk content) for that canonical path (a post-capture source edit to ANY recorded contributor — which re-keys the registry bytes' hash away from the snapshot's stored hash — invalidates the snapshot); for each recorded contributor (keyed by `(decl_span, contributor_ordinal)`) the freshly RE-PARSED+RE-LOWERED pair EQUALS the recorded one AND the re-run current-allowlist verdict over it is ADMIT; and the final verdict is ADMIT (matching the snapshot's existence). **Honest residual (moot for the admitted set):** the only thing this offline gate canNOT reproduce is the contributor-set MEMBERSHIP — WHICH files / which merged peers are contributors is established by GENERATION-TIME import/merge/transitive navigation through the live resolver, which is not offline-reproducible (see §Q5 cross-reference). This residual is MOOT for the admitted set, because initial admission is RESTRICTED to PROVABLY SINGLE-CONTRIBUTOR rows (§Scope — single-file, no import / merge / augmentation / transitive `typeof`/`ReturnType` hop, so the contributor set is trivially `{the one decl}` and fully offline-verifiable); any row whose source-side walk reaches >1 contributor or crosses a file/merge/augmentation/transitive hop is DEFAULT-REJECTED and deferred to the named offline contributor-set-membership-revalidation spike (§4). The gate therefore catches (a) any within-file fact omission/tamper in a RECORDED contributor (re-parse + re-lower + compare, per `(ordinal, decl_span)`), and (b) any content change to a recorded contributor (via the per-contributor content-hash gate — for the admitted single-contributor set, a content edit changes the one recorded file's hash and misses the warm read). It does NOT re-NAVIGATE to discover a contributor the digest never recorded; full re-navigation is a generation-time step, deferred for multi-contributor rows. It is an AUDIT/validation record, never re-asserted as the parity value and never an `oracle_value` / `snapshot_id` input. Validated by `source_admission_digest_consistent`. |

`identity` for the `structured_type_expr` kind carries every value-affecting input so
the snapshot is fully self-describing and re-derivable. (A future kind tags its own
required axes; these are the `structured_type_expr` axes specifically.)

| `structured_type_expr` `identity` field | Meaning |
| --- | --- |
| `query_helper_kind` | The closed helper-kind discriminant (§Q4) — `ResolveExpr` / `ShallowSurfaceExpr` / `EvaluateExpr` — that produces the in-process `TypeExpr`. Determines which of the remaining axes are populated. |
| `workspace_files` | The set of files the row's workspace contains, each `{ path, content_hash }` — **the snapshot stores the PATH + the CONTENT HASH ONLY, never the source bytes.** The source-byte AUTHORITY is the row's REGISTRY entry (§Q4), whose `workspace_files` payload carries the upserted `{ path, source }`; an offline guard that needs to re-parse a workspace file reads the REGISTRY source by canonical path and VERIFIES it against this stored `content_hash`. Entries are keyed + canonicalized BY PATH. A `TypeExpr`-projection row is often multi-file (`cross_file.rs:6` upserts leaf/unused/barrel/consumer) — a single `fixture_path`/`fixture_content_hash` cannot represent it. **The semantic input is each file's FINAL CONTENT, not the upsert SEQUENCE** — the resolved `TypeExpr` depends on what each path holds at query time, not on the order the test upserted them, and a path is never upserted twice with different content in an admitted row. So `workspace_files` is canonicalized: entries are SORTED by canonical path before hashing into `snapshot_id` (the canonical-encoding manifest-ordering rule), and a DUPLICATE path is a SCHEMA VIOLATION (the same path appearing twice is rejected — there is exactly one final content per path). Upsert order is NOT a `snapshot_id` input. The path is the CANONICAL leading-slash form (`"/fixtures/conditional-infer.ts"`, fact below); the hash is over each file's source text under the canonical line-ending normalization. |
| `primary_canonical` | The canonical id queried (leading-slash form, e.g. `"/fixtures/conditional-infer.ts"` — `conditional_infer.rs:12`). |
| `symbol_or_expression` | The symbol resolved (`ResolveExpr` / `ShallowSurfaceExpr`) OR the expression string (`EvaluateExpr`, e.g. `typeof f`). |
| `type_arguments` | The canonicalized `TypeExpr`-JSON of each type arg (`ResolveExpr` only; distinguishes `Box<string>` from `Box<number>`). |
| `projection_mode` | `Shallow` / `Navigate` / `Expanded` / `Skeleton` (`ResolveExpr` / `EvaluateExpr`; `ShallowSurfaceExpr` is always empty-path `Shallow`). |
| `host_project` | The host/project setup axes: `{ project_root, workspace_root, tsconfig_path, host_setup_kind }`. `host_setup_kind` is a closed enum: **`standalone`** (the DEFAULT — `make_host_with_footprint()` = `VerterHost::new_standalone`, fact 9, `support.rs:89`; no project root / no tsconfig, so `project_root`/`workspace_root`/`tsconfig_path` reference the generator's canonical synthetic root + `oracle.tsconfig.json`), **`workspace_footprint`** (the ~9-row minority — `make_host_with_workspace_files_footprint`, `support.rs:97`; `/workspace`, `/workspace`, `/workspace/tsconfig.json`), and the DEFERRED package-backed / custom-host kind (`make_package_host_with_workspace`, `cache_invalidation.rs:344`, §Scope). The hover answer depends on these (a different workspace root, tsconfig path, or host kind resolves differently), so they enter `identity` AND `snapshot_id`. |
| `probe_locator` | The synthesized probe's name + offset used for the hover capture (§Q2) — INSPECTION/debug only. It is DERIVABLE from `probe_synthesis_version` + the query (the probe is fixed + versioned, §Q2), so it is NOT a direct `snapshot_id` input; `probe_synthesis_version` is what enters the id. |

A workspace-file edit changes a `content_hash` → changes the `snapshot_id` (Q4), so
a stale snapshot is detected as a missing-snapshot at lift time rather than silently
comparing against an outdated answer.

**Canonical path form.** Fixture paths in `workspace_files` / `primary_canonical` use
the **leading-slash** form the live tests use (`upsert_ts(&host, "/fixtures/…")` —
`conditional_infer.rs:8,12`). The registry stores that exact form and the snapshot
mirrors it; the normalizer is identity (no stripping) so the registry path, the live
upsert path, and the hashed-identity path are byte-identical.

**Canonical encoding + hash families (snapshot_id reproducibility).** Every hash and
every serialized identity in a snapshot is produced under ONE pinned canonical
encoding so a regeneration on a different machine reproduces byte-identical snapshots
(and so the offline guards re-derive the same ids/hashes). The rules:

- **Canonical JSON.** Wherever a structured value is hashed or compared structurally
  (the `snapshot_id` length-prefixed fields, the per-arm structural sort key in
  normalization, the `oracle_env_files` listing, the `identity` axes): object KEYS are
  sorted lexicographically by their UTF-8 bytes; there is NO insignificant whitespace
  (no spaces, no newlines between tokens); strings use minimal JSON escaping (only the
  mandatory `"`, `\`, and `U+0000..U+001F` control escapes — never gratuitous
  `\uXXXX` for printable characters); numbers are emitted as minimal decimal integers
  (the schema carries no floats — versions and ordinals are integers — so there is no
  float-format ambiguity); arrays preserve their semantic order (the `oracle_env_files`
  `manifest` / `files` lists are sorted by canonical path before hashing so listing
  order is not an input).
- **Path-separator + leading-slash rule.** Per-row `workspace_files` /
  `primary_canonical` paths are the LEADING-SLASH form (`/fixtures/…`) verbatim;
  vendored-corpus paths in `oracle_env_files` are CORPUS-RELATIVE (no leading slash,
  rooted at `oracle_env/<env_corpus_id>/`). All path separators are normalized to `/`
  (forward slash) on every platform before hashing/listing — a Windows backslash never
  enters a hash input. (This is the spelling the `workspace_files_not_in_oracle_env_files`
  disjointness guard normalizes both domains to before comparison.)
- **Line-ending + trailing-newline normalization for content hashes (pinned exactly).**
  Every file content hash (`workspace_files[].content_hash`,
  `oracle_env_files.files[].content_hash`) is taken over the file bytes after TWO pinned
  normalizations, in this order: (1) **line endings → `\n`** — every CRLF (`\r\n`) and
  lone CR (`\r`) is rewritten to a single LF (`\n`); (2) **trailing-newline →
  EXACTLY-ONE** — all trailing `\n`s at end-of-content are stripped and then a SINGLE
  `\n` is appended, so a file with no final newline, one final newline, or several blank
  trailing lines all hash IDENTICALLY (the canonical form ends in exactly one `\n`). An
  EMPTY file (zero bytes) hashes as the empty byte string (no newline appended — the
  exactly-one rule applies only to non-empty content). The text is hashed AFTER both
  normalizations, NEVER the raw on-disk bytes, so a checkout under a CRLF-translating Git
  config, an editor that adds/removes a final newline, or a `core.autocrlf` setting all
  produce the same content hash as a clean LF-with-one-trailing-newline checkout. This
  exact rule is pinned by `canonical_encoding_is_pinned` and versioned with
  `oracle_schema_version`.
- **Manifest ordering.** The `oracle_env_files.manifest` and `.files` lists are emitted
  in canonical-path-sorted order; the set-equality re-enumeration sorts the on-disk
  listing the same way before comparison, so listing/iteration order is never a
  difference.
- **Two distinct hash FAMILIES, by role — the `sha256:`/`blake3:` mix in the example is
  intentional, not a drift.** Every hash string is PREFIXED with its family
  (`sha256:` / `blake3:`) so the family is self-describing on disk. The roles:
  - **`snapshot_id`, `env_corpus_id`, and `oracle_env_hash` are BLAKE3** (the `blake3:`
    prefix in the example). These are the harness's OWN identity/content ids over
    canonical encodings; BLAKE3 is the one id-family the harness computes, length-
    prefixed + domain-separated (§Q4) for `snapshot_id`. `env_corpus_id` and
    `oracle_env_hash` are TWO DOMAIN-SEPARATED BLAKE3 digests over the same vendored
    SHARED corpus file set but under DISTINCT domain tags + DIFFERENT roles (the corpus
    LISTING identity in the filename vs the on-read-revalidated `oracle_env_files.files`
    content hash, §Q1 `env_corpus_id` / `oracle_env_hash`) — they are NOT required to be
    byte-equal (the example's values deliberately differ) and neither is derived from the
    other.
  - **`compiler_options_hash` and the per-file `content_hash`es are SHA-256** (the
    `sha256:` prefix in the example). These are plain CONTENT digests of a single
    canonicalized blob (the effective-options canonical JSON; a file's normalized
    bytes); SHA-256 is the content-digest family. They are NOT identity ids the harness
    derives by composing fields — they are one-shot digests of one canonical input.
  The split is by ROLE (harness-derived identity = BLAKE3; one-shot content digest =
  SHA-256), the family prefix makes it unambiguous on disk, and `compiler_options_hash`
  / `content_hash` / `env_corpus_id` / `oracle_env_hash` / `snapshot_id` each pin their
  family in the schema (a snapshot whose hash carries the wrong family prefix FAILS
  decode). The canonical-encoding rules above are versioned with `oracle_schema_version`
  (encoding) and `normalizer_version` (the structural-sort key), so a change to the
  canonical encoding forces regeneration. Pinned by the `canonical_encoding_is_pinned`
  guard.

**Concrete example** (`oracle_snapshots/utility_composition/u_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2.json`).
The `oracle_value` is exactly a `TypeExpr::to_json_value()` document — the
internally-tagged `"kind"` codec from fact 2, so the two sides compare with one
shared decoder. For a fixture
`type ComposedProps = { id: number; label: string; tag?: "a" | "b" }` resolved in
`Expanded` mode, tsgo's hover lowers to this `TypeExpr`-JSON:

```json
{
  "oracle_schema_version": 1,
  "normalizer_version": 1,
  "probe_synthesis_version": 1,
  "tsgo_version": "7.0.0-dev.20260526.1",
  "compiler_options_hash": "sha256:9f1c0e7b…",
  "env_corpus_id": "blake3:2b9d61fa…",
  "oracle_env_files": {
    "manifest": [
      "oracle.tsconfig.json",
      "lib/lib.es2020.d.ts",
      "node_modules/@verter/types/package.json",
      "node_modules/@verter/types/index.d.ts"
    ],
    "files": [
      { "path": "oracle.tsconfig.json", "content_hash": "sha256:c0de…" },
      { "path": "lib/lib.es2020.d.ts", "content_hash": "sha256:1f88…" },
      { "path": "node_modules/@verter/types/package.json", "content_hash": "sha256:5e7f…" },
      { "path": "node_modules/@verter/types/index.d.ts", "content_hash": "sha256:90ab…" }
    ]
  },
  "oracle_env_hash": "blake3:7c4e2a90…",
  "oracle_family": "utility_composition",
  "oracle_value_kind": "structured_type_expr",
  "snapshot_id": "u_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2",
  "row_ref": {
    "row_file": "utility_composition.rs",
    "row_function": "composed_props_expands",
    "query_ordinal": 0
  },
  "identity": {
    "query_helper_kind": "ResolveExpr",
    "workspace_files": [
      { "path": "/fixtures/utility_composition.ts", "content_hash": "sha256:4d2a…" }
    ],
    "primary_canonical": "/fixtures/utility_composition.ts",
    "symbol_or_expression": "ComposedProps",
    "type_arguments": [],
    "projection_mode": "Expanded",
    "host_project": {
      "project_root": "/",
      "workspace_root": "/",
      "tsconfig_path": "/oracle.tsconfig.json",
      "host_setup_kind": "standalone"
    },
    "probe_locator": { "probe_name": "__oracle_probe__0", "offset": 412 }
  },
  "oracle_value": {
    "kind": "object",
    "properties": [
      { "memberKind": "property", "name": "id", "optional": false, "readonly": false,
        "ty": { "kind": "primitive", "name": "number" } },
      { "memberKind": "property", "name": "label", "optional": false, "readonly": false,
        "ty": { "kind": "primitive", "name": "string" } },
      { "memberKind": "property", "name": "tag", "optional": true, "readonly": false,
        "ty": { "kind": "union", "types": [
          { "kind": "literal", "literalKind": "string", "value": "a" },
          { "kind": "literal", "literalKind": "string", "value": "b" }
        ] } }
    ]
  },
  "raw_capture": {
    "probe_name": "__oracle_probe__0",
    "probe_header": "type __oracle_probe__0 = ComposedProps;",
    "hover_contents": "```typescript\ntype __oracle_probe__0 = {\n    id: number;\n    label: string;\n    tag?: \"a\" | \"b\";\n}\n```"
  },
  "source_admission_digest": {
    "source_locator": {
      "reference_canonical": "/fixtures/utility_composition.ts",
      "reference_name": "ComposedProps",
      "symbol_space": "Type"
    },
    "observed_source_files": [
      { "path": "/fixtures/utility_composition.ts", "content_hash": "sha256:4d2a…" }
    ],
    "contributors": [
      {
        "contributor_ordinal": 0,
        "decl_span": { "file": "/fixtures/utility_composition.ts", "start": 142, "end": 198 },
        "decl_canonical": "/fixtures/utility_composition.ts",
        "name": "ComposedProps",
        "symbol_space": "Type",
        "decl_kind": "TypeAlias",
        "raw_surface": {
          "raw_member_keys": ["Static(id)", "Static(label)", "Static(tag)"],
          "member_kinds": ["Property", "Property", "Property"],
          "member_visibility": ["Public", "Public", "Public"],
          "unique_symbol_ops": [],
          "abstract_ctor": false,
          "type_param_modifiers": [],
          "this_type_or_param": false,
          "value_const_assertion": null,
          "overload_signatures": [],
          "tuple_element_shape": [],
          "utility_referent_names": [],
          "transitive_referents": []
        },
        "lowered_body": {
          "kind": "object",
          "properties": [
            { "memberKind": "property", "name": "id", "optional": false, "readonly": false,
              "ty": { "kind": "primitive", "name": "number" } },
            { "memberKind": "property", "name": "label", "optional": false, "readonly": false,
              "ty": { "kind": "primitive", "name": "string" } },
            { "memberKind": "property", "name": "tag", "optional": true, "readonly": false,
              "ty": { "kind": "union", "types": [
                { "kind": "literal", "literalKind": "string", "value": "a" },
                { "kind": "literal", "literalKind": "string", "value": "b" }
              ] } }
          ]
        },
        "verdict": "Admit"
      }
    ],
    "final_verdict": "Admit"
  }
}
```

The `oracle_value` is byte-for-byte the encoding `TypeExpr::to_json_value` emits and
`type_expr_from_json` decodes (`type_expr_json.rs:35,420`): every node is an object
with a `"kind"` string; the `Object` carries a flat `"properties"` array; each
member carries a `"memberKind"`; an index signature would appear inline in
`"properties"` with `"memberKind":"indexSignature"` (there is NO separate
`index_signatures` array). A future non-`TypeExpr` oracle kind adds a new
`oracle_value_kind` discriminant + its own value shape without changing this field
set.

**The two file domains are DISJOINT in the example (and in every snapshot).** Note in
the example above that `oracle_env_files` lists ONLY the SHARED ambient/lib/package/
tsconfig corpus (`oracle.tsconfig.json`, `lib/lib.es2020.d.ts`, the `@verter/types`
manifest + decls) and does NOT list the per-row fixture
`/fixtures/utility_composition.ts`. The fixture appears ONLY in
`identity.workspace_files`, in its canonical leading-slash form. These are two
SEPARATE identity domains: the SHARED env corpus (content-addressed by
`env_corpus_id`, the same for every standalone-host row) vs the PER-ROW workspace
files (hashed into `snapshot_id` through `identity.workspace_files`). A per-row
workspace file path appearing in BOTH `oracle_env_files` and `identity.workspace_files`
is a SCHEMA VIOLATION — it would double-count the file across the two domains and
poison the shared corpus's content id with per-row content. The
`workspace_files_not_in_oracle_env_files` guard (§4) REJECTS any snapshot in which a
path listed under `identity.workspace_files` also appears in `oracle_env_files.manifest`
/ `oracle_env_files.files` (modulo the leading-slash vs corpus-relative spelling — the
guard normalizes both to a comparable form before the disjointness check).

**Row-INJECTED `node_modules` packages are WORKSPACE FILES, not shared corpus — the ONE
rule for the split.** Some existing helpers inject package files into the row's OWN
workspace under a `/workspace/node_modules/...` path (e.g. `flow_return_catalog.rs:209`
upserts a package `.d.ts` as a workspace file). This is the one place the env/workspace
split could be implemented two ways, so it is pinned to EXACTLY ONE rule: **a package
file the ROW itself injects into its workspace is a PER-ROW WORKSPACE FILE — it lives in
`identity.workspace_files` (the workspace identity, hashed into `snapshot_id`), NOT in
the SHARED `oracle_env_files` corpus.** Equivalently, `env_corpus_id` /
`oracle_env_files` is narrowed to SHARED AMBIENT packages ONLY — the ambient/lib/package
corpus that is the SAME for every standalone-host row (the canonical host's vendored
`@verter/types`, libs, ambient `@types`) — and NEVER a package a single row injects into
its own workspace. The criterion is OWNERSHIP, not the `node_modules` substring: a file
the row upserts as part of its workspace setup is workspace-owned regardless of whether
its path contains `node_modules`; a file the canonical host's shared vendored corpus
supplies is shared regardless. A row-injected `/workspace/node_modules/...` file
therefore enters `snapshot_id` through `identity.workspace_files` (the generator must
vendor it into the row's workspace the tsgo driver sees, and hash it as a workspace
file), and the `workspace_files_not_in_oracle_env_files` disjointness guard keeps it out
of the shared corpus. (Rows whose package corpus is NOT row-injected but supplied by a
custom package host — `make_package_host_with_workspace`, `cache_invalidation.rs:344` —
remain the DEFERRED package-backed / custom-host class, §Scope; this rule resolves the
row-INJECTED case, the custom-host case stays deferred.) Pinned by the
`row_injected_packages_are_workspace_files` guard.

### Q2 — Oracle value shape, probe-driven generation, and normalization (DECIDED)

> **Reframed by the single-spec / correction-overlay model.** The tsgo-hover-sourced
> value defined in this section is the **`ts_compat`** oracle. Its generation path,
> admission gate, lowering, normalization, and confluence properties are **unchanged**.
> What changes is only its ROLE: it is the recorded `TsCompat` value (data captured from
> tsgo), against which a no-correction row's `resolver(row)` is asserted, and which a
> registered-divergence row's recorded compat side names. A registered-divergence row
> carries an additional review-gated **correction overlay** (the **`Correct`** answer that
> the single-spec resolver must produce), authored by hand — NOT captured from tsgo, since
> tsgo emits the bug — in this same lowered-`TypeExpr` codec (`ts-compat-two-mode-model.md`
> §3).

**Decision: the oracle value is sourced from tsgo's `textDocument/hover` text,
captured via a FIXED, VERSIONED synthesized PROBE, admitted by a strict POSITIVE
ALLOWLIST (default-REJECT) run BEFORE lowering on BOTH the hover OXC type AST AND the
fixture SOURCE declaration, plus a strict-lowering drop-counter, then — only if
admitted — lowered to a `TypeExpr` via `verter_type_expr_oxc::lower_ts_type` at
GENERATION time and stored as `TypeExpr` JSON.** Tests compare a normalized `TypeExpr`
against a normalized `TypeExpr` — they NEVER compare hover text. Every snapshot carries
`oracle_value_kind = "structured_type_expr"`.

The admission gate is a **closed positive allowlist, default-REJECT**: a query is
hover-admissible ONLY when EVERY construct in its type is on an explicitly enumerated
minimal allowlist; ANY construct not on the list — known or unknown — REJECTS the
query. This forecloses the "you missed construct X" class: a construct the design did
not anticipate falls through to REJECT, never to a silent admit. The allowlist admits
ONLY constructs that are (a) representable in `TypeExpr` without loss AND (b) not
subject to hover elision / summarization.

The comparison must IGNORE cosmetic differences (whitespace, member ordering where
TS treats the set as unordered, alias display vs structural expansion) while CATCHING
real type divergence (wrong member, wrong optionality, wrong primitive, wrong union
arm). The decision rests on two facts: (a) tsgo's LSP exposes no structured type
reply — only hover text (fact 3), so a structured-from-LSP source does not exist
in-tree; (b) `TypeExpr` already has the right JSON codec (fact 2), so lowering the
hover text once, at generation time, through the SAME OXC lowering Verter uses for
real source yields a directly comparable `TypeExpr`. Driving a non-LSP compiler-API
harness is the only route to a genuinely structured tsgo value, but no such harness
exists in-repo today, and tsgo's compiler-API surface at the pinned preview version
is less battle-tested than its LSP — so it is reserved for the future structured
oracle that the out-of-scope verdict families need.

#### Probe-driven generation (the primary soundness mechanism)

Raw symbol-offset hover is **not sound**: `get_hover` discards the hover range
(`range_start: None, range_end: None`, `ipc.rs:1689`), so the generator cannot prove
a returned hover string is for the intended symbol rather than an adjacent token. The
generator therefore drives a **FIXED, deterministic, VERSIONED probe**, constructed
PER `query_helper_kind` so the probe answers the SAME query Verter does. The probe
form is FIXED so that `snapshot_id` is re-derivable WITHOUT tsgo, and the probe
algorithm is versioned by `probe_synthesis_version` (in `snapshot_id`); the
`probe_locator` is then DERIVABLE from `probe_synthesis_version` + the query, so the
id derivation hashes the version, not a raw locator that is not yet fixed.

**The probe mirrors how Verter itself resolved the query — same-file append for
`ResolveExpr` / `ShallowSurfaceExpr`, the scratch-file + `eval_source`-prelude model
for `EvaluateExpr`.** The probe must be constructed so it sees the EXACT lookup
environment Verter resolved the query in:

- **`ResolveExpr` / `ShallowSurfaceExpr` (same-file append).** `resolve_expr`
  (`support.rs:132`) / `shallow_surface_expr` (`support.rs:160`) resolve `name` IN the
  `primary_canonical` file's own scope, so the generator **clones the
  `primary_canonical` file content and appends**
  `type __oracle_probe__N = <symbol with type_args applied>;` at end-of-file in that
  file's own scope. A SEPARATE appended file would be unsound: (a) it cannot see
  non-exported locals or file-local imports the symbol resolves against, so the probe
  could fail to bind or bind to a DIFFERENT (exported/ambient) declaration; (b) it
  cannot prove the probe's RHS binds to the SAME declaration Verter resolved.
- **`EvaluateExpr` (scratch-file + `eval_source` prelude — mirrors Verter).** Verter's
  `evaluate_type_expression` does NOT append to the scope file: it synthesizes a
  SEPARATE scratch file, inlines the scope's `IndexedReady.eval_source` as a PRELUDE,
  then wraps the expression as a trailing `type __VerterScratch = <expression>;`
  (`crates/verter_session/src/typeinfo/evaluate_type_expression.rs:~314`). The generator MIRRORS that scratch-file +
  `eval_source`-prelude model for `EvaluateExpr`: synthesize a scratch file = the
  scope's `eval_source` prelude + a trailing
  `type __oracle_probe__N = <expression>;`, so the probe's lookup environment matches
  Verter's scratch environment (the same top-level bindings the scope publishes) rather
  than a raw in-place append. The `probe_binds_to_registry_target` guard backstops any
  residual divergence — it proves the probe's RHS binds to the registry's
  `source_locator` declaration regardless of the synthesis mechanism.

Both paths use the fixed naming `__oracle_probe__<query_ordinal>` and place the
expression / symbol in type position; for `EvaluateExpr` the expression must be
type-position-legal (see "EvaluateExpr type-position gate").

**Parameterized `ResolveExpr` — synthesizing the probe RHS for a generic query
(`type_arguments` non-empty).** A `ResolveExpr { symbol, type_args, projection_mode }`
query may carry NON-EMPTY `type_args` — the registry stores them as canonicalized
`TypeExpr`-JSON (the `identity.type_arguments` axis, which distinguishes
`GenericBox<string>` from `GenericBox<number>`). The probe RHS for such a query is NOT
the bare `symbol`; it must be `symbol<A, B, …>` with each type-argument printed back as
TS SOURCE so tsgo instantiates the SAME generic application Verter resolved. The
`identity` stores type-arguments as canonical `TypeExpr` JSON, so synthesizing the
probe RHS requires a **deterministic, VERSIONED `TypeExpr` → TS-source printer** for
the type-argument list. The probe RHS is then
`type __oracle_probe__N = <symbol>< printer(arg0), printer(arg1), … >;`.

The printer is part of the PROBE-SYNTHESIS algorithm and is therefore versioned by
`probe_synthesis_version` (a printer change forces regeneration via the
`snapshot_id` it feeds). To stay sound it is held to a deliberately NARROW,
default-reject contract — it is NOT a general `TypeExpr` pretty-printer:
- It prints ONLY the `TypeExpr` variants whose TS-source spelling is unambiguous and
  round-trips losslessly through the SAME OXC lowering the hover side uses:
  `Primitive`, `Literal`, `Array`/`Tuple` of printable elements,
  `Ref { name, type_arguments }` of printable args (recursively), `Union`/
  `Intersection` of printable arms, `KeyOf` / `IndexedAccess` / `TypeOf` of printable
  operands. These are exactly the constructs already on the §Q2 ADMIT allowlist, so a
  printable type-argument is by construction an admissible one.
- Any type-argument whose `TypeExpr` is NOT in that printable set (a `Mapped`,
  `Conditional`, `Infer`, `TemplateLiteral`, `Function`/`ConstructorType`,
  `Unknown { raw }`, `SyntheticSlotBinding`, `RecursiveRef`, or any future variant)
  is **un-printable → the `(row, query)` is DEFAULT-REJECTED / deferred** — the
  generator never emits a best-effort or `Unknown`-shaped argument that could
  instantiate a DIFFERENT type in tsgo than Verter resolved. This is the same
  default-reject discipline as the admission allowlist and the cosmetic-name axis.
- The printer is the INVERSE of the hover-side lowering for the printable set:
  printing `arg` then re-lowering the printed source through `lower_ts_type` MUST
  yield a `TypeExpr` structurally equal (under §Q2 normalization) to `arg`. The
  generator ASSERTS this round-trip at synthesis time; a round-trip mismatch FAILS
  generation (the printer is unsound for that argument and the query is deferred,
  never silently mis-probed).

**Initial-scope decision: non-empty-`type_arguments` rows are DEFERRED until the
printer is spiked.** Standing up the versioned printer + its round-trip proof is a
BLOCKING spike item (§4), in the same posture as the `Expanded`-mode probe form: until
the spike validates and versions the printer (a `probe_synthesis_version` bump), a
`ResolveExpr` query with NON-EMPTY `type_args` is NOT admissible via this harness and
stays `Ignored`. Empty-`type_arguments` `ResolveExpr` rows (the common case — a probe
RHS that is the bare `symbol`) are admissible NOW under the fixed probe form. Pinned by
the `parameterized_probe_rhs_synthesis` and `probe_form_is_deterministic_and_versioned`
guards.

**Binding-identity check (the probe must bind to the registry's target).** Because the
probe sees the real query environment (the same-file scope for `ResolveExpr` /
`ShallowSurfaceExpr`, the scratch-file + `eval_source` prelude for `EvaluateExpr`), the
generator MUST verify the probe's RHS binds to the INTENDED declaration — not a shadow,
an ambient re-declaration, or an unrelated import. The generator does this via a tsgo
definition / diagnostic check on the probe (the probe alias introduces ZERO new
diagnostics AND the `textDocument/definition` of the symbol in the probe lands on the
same declaration the registry's `source_locator` / `(primary_canonical, symbol)` names);
a mismatch FAILS generation. This also backstops any divergence in the `EvaluateExpr`
scratch model. This is the `probe_binds_to_registry_target` guard.

1. **Synthesize the probe in the environment Verter resolved in.** For each query
   spec the generator synthesizes a probe declaration (deterministic name + content),
   shaped by the spec's `query_helper_kind`:
   - `ResolveExpr` / `ShallowSurfaceExpr` (a type query): clone the `primary_canonical`
     file content and APPEND `type __oracle_probe__N = <symbol with its type_args
     applied>;` at end-of-file in that file's own scope.
   - `EvaluateExpr` (an arbitrary expression, e.g. `typeof f`): mirror Verter's
     scratch-file + `eval_source`-prelude model (`crates/verter_session/src/typeinfo/evaluate_type_expression.rs:~314`) —
     synthesize a scratch file = the scope's `eval_source` prelude + a trailing
     `type __oracle_probe__N = <expression>;` (the expression in type position, e.g.
     `type __oracle_probe__N = typeof f;`). `EvaluateExpr` is admissible ONLY for
     **type-position-valid** expressions (see "EvaluateExpr type-position gate" below).

   `N` is the registry `query_ordinal` (unique within the row).
2. **Hover the probe alias name at a KNOWN offset.** The offset of
   `__oracle_probe__N` is exact (the generator authored the text), so the hover
   target is unambiguous.
3. **Require the hover header to name the probe.** The captured hover MUST contain a
   `type __oracle_probe__N = …` header naming exactly that probe. Because `get_hover`
   returns no range, this header check is the ONLY fence that the captured text is for
   the intended target — it rejects a wrong-position or empty hover that would
   otherwise produce false parity.

**Hover-driver config (PINNED, versioned by `probe_synthesis_version`).** The hover TEXT
is the oracle source, so anything that changes HOW tsgo formats that text is a
value-affecting input and must be a versioned identity axis — not an implicit ambient of
the driver process. The full tsgo LSP DRIVER CONFIG is therefore PINNED and FOLDED INTO
`probe_synthesis_version` (which already versions the probe form + extraction + admission),
so two generator sessions with identical query + env but different hover protocol / display
config CANNOT produce different captures under the same `snapshot_id`. The pinned driver
config is a CLOSED set:
- **The LSP `initialize` payload + declared CLIENT CAPABILITIES** — the exact
  `processId` / `rootUri` / `workspaceFolders` shape AND the `capabilities` block the
  generator declares. The capabilities are pinned EXACTLY: the adopted driver
  (`TsgoTypeProvider::get_hover`, Q3) initializes tsgo with EMPTY hover capabilities
  (`capabilities: {}`, `crates/verter_type_runtime/src/tsgo/ipc.rs`), which produce /
  reduce to a BARE PLAINTEXT hover — the `type __oracle_probe__N = <RHS>` text with NO
  markdown fence (this is the shape the §Q2 extraction grammar's plaintext branch
  parses). A markdown-capable driver (declaring `textDocument.hover.contentFormat =
  markdown`) instead wraps the type in a ```` ```typescript ```` fence (the grammar's
  fenced branch). Any capability / content-shape change — empty caps ↔ markdown caps, or
  any capability that alters tsgo's response shape — is a `probe_synthesis_version` bump.
- **The config-delivery block** — the `workspace/didChangeConfiguration` /
  `initializationOptions` payload that carries the pinned `compilerOptions` (the concrete
  shape is the §4 option-delivery spike) plus any tsgo formatting/preference keys.
- **The `textDocument/hover` request payload** — the exact request fields the generator
  sends for the probe-name hover.
- **Hover DISPLAY PREFERENCES** — the tsgo preferences that change how a type is PRINTED
  in the hover (alias-expansion / truncation-length / quote-style / member-ordering
  display preferences). These are pinned to the values the §Q2 extraction grammar +
  normalizer assume; a change to any of them is a `probe_synthesis_version` bump
  (`noErrorTruncation` itself is pinned via the effective-option map, §Q2 "Env pinning",
  and is also delivery-proven).
The driver config is recorded as a single CANONICAL `hover_driver_config` blob folded into
`probe_synthesis_version`; a change to ANY field bumps the version and forces regeneration.
Pinned by `hover_driver_config_pinned`.

**Hover-extraction grammar (versioned by `probe_synthesis_version`).** The probe-header
fence above and the lowering step both need to extract the type expression `<T>` from
the `type __oracle_probe__N = <T>` header inside tsgo's hover. The extraction is a small
fixed GRAMMAR — not an ad-hoc regex / substring scan over the whole blob — versioned with
the rest of probe synthesis. It has TWO ordered shapes, chosen by whether the hover
carries a fenced code block; BOTH require the probe text to be EXACTLY one top-level
type-alias declaration:
- **Shape 1 — fenced (markdown-caps driver).** If ANY fenced code block is present, ONLY
  fenced \`\`\`typescript / \`\`\`ts blocks are parsed (the example
  `"```typescript\ntype __oracle_probe__0 = {…}\n```"`); any surrounding Markdown prose,
  inline `code` spans, or non-TS fences are ignored. The extractor selects the FIRST
  \`\`\`typescript block whose trimmed body is EXACTLY the probe alias declaration
  naming the expected probe (the probe-header fence), tolerating only leading/trailing
  `/** … */` / `//` comments / JSDoc, and rejects a hover with NO such block (the
  probe-header fence FAILS). The presence of ANY fence DISABLES the Shape-2 plaintext
  fallback.
- **Shape 2 — plaintext (empty-caps driver).** If ZERO fenced code blocks of any
  language are present, the WHOLE trimmed hover is parsed as the plaintext driver shape:
  the adopted driver sends EMPTY hover capabilities (Q3), so tsgo returns the BARE
  `type __oracle_probe__N = <RHS>` text with NO markdown fence. The whole trimmed hover
  (modulo leading/trailing comments/docblocks) must be EXACTLY the probe alias
  declaration naming the expected probe — nothing before or after it. A markdown hover's
  prose / inline / other-language blocks NEVER trigger this fallback (any fence disables
  it), so it cannot pick a probe header embedded in markdown prose.
- **Exact whole-hover alias grammar (BOTH shapes).** A candidate (the fenced block body,
  or the whole plaintext hover) is accepted ONLY when it is EXACTLY one top-level
  `type __oracle_probe__<N> = <RHS>` declaration with an optional trailing `;`: the
  CORRECT probe name (anchored, the wrong-position fence), NO `export` / `declare`
  modifier, NO type parameters on the alias header (`type P<T> = …` is out of grammar),
  and NO surrounding prose / trailing extra declarations / trailing non-comment text.
  This is a STRICT top-level alias parse (full-consumption check via the SAME OXC TS
  parser), NOT a loose substring scan — a loose scan would accept a header embedded in
  prose, behind an `export`/`declare` modifier, on a parameterized alias, or followed by
  extra declarations. The RHS `<RHS>` is the alias type-annotation span; an
  unbalanced / truncated / invalid candidate FAILS to parse and is rejected (and the
  `…` truncation marker FAILS the admission backstop). The extracted RHS bytes are
  handed UNCHANGED to the admission gate's OXC parser.
- **Qualified / imported display forms.** tsgo may print a qualified name
  (`Namespace.Member`) or an imported alias's display form; the extracted RHS string is
  parsed by the SAME OXC type parser the admission gate uses (it is not re-interpreted
  textually), so a qualified-name display is parsed to its `TSQualifiedName` AST and
  then runs through the positive allowlist (a qualified-name `Ref` falls through to the
  enum-member / qualified-name REJECT row unless explicitly admitted). The extractor's
  only job is to hand the OXC parser the exact RHS bytes; all SEMANTIC interpretation
  is the allowlist's, not the extractor's.
The grammar is FIXED and versioned by `probe_synthesis_version` (a grammar change
forces regeneration); it is re-runnable OFFLINE from the stored `raw_capture`
(`raw_capture_matches_oracle_value` and `probe_header_names_target` both re-run it).
Pinned by the `hover_extraction_grammar_is_versioned` guard. This is the extraction
central to `raw_capture_matches_oracle_value`.

**`EvaluateExpr` type-position gate.** A probe `type __oracle_probe__N = <expr>;` is
only valid when `<expr>` is **type-position-legal**: `typeof x` works in type position;
a value call `f()` does NOT. So `EvaluateExpr` is admissible via this harness ONLY for
type-position-valid expressions (`typeof …`, an indexed access, a `keyof`, etc.). An
`EvaluateExpr` whose expression is not type-position-legal cannot be probed as a type
alias and is **deferred** — it stays `Ignored` for a future structured oracle, never
force-fit. The generator REJECTS a non-type-position `EvaluateExpr` expression at probe
synthesis (the probe would be a parse error), so it never reaches a snapshot.

**`EvaluateExpr` admission is restricted to a CLOSED SINGLE-ROOT grammar (default-reject
multi-/nested-referent).** A `SourceLocator` names exactly ONE root binder
(`reference_name` + `symbol_space`), so the source-side allowlist walk starts from ONE
declaration. But an arbitrary type-position expression can have MULTIPLE or NESTED
referents — `A | B` (two roots), `keyof T` (root `T`), `ReturnType<typeof f>` (root `f`
behind a utility application), an indexed-access root chain, a namespace/property path
(`NS.Inner.Member`). A single `SourceLocator` cannot start the walk from every referent,
so a lossy contributor in a NON-leading referent would never be allowlist-checked →
false parity. The harness DOES NOT walk a partial set; it DEFAULT-REJECTS. An
`EvaluateExpr` expression is admissible ONLY when it matches this closed single-root
grammar:

```
single_root_expr :=
      typeof <binder>                 // `typeof f` — root binder f (VALUE space)
    | <binder>                        // a bare type-position binder reference (TYPE space)
    | single_root_expr <index_or_property_path>   // a trailing index / property path
                                                   // (`X['a']['b']`, `X.a.b`) whose every
                                                   // path-segment root resolves to the SAME
                                                   // root binder as the head — no second binder
```

i.e. exactly ONE binder reference, optionally followed by a trailing index-access /
property path whose roots ALL resolve to that same single root binder. Any expression
introducing a SECOND referent — a `union`/`intersection` of two type expressions, a
`keyof`/`typeof` applied to a compound, a utility application whose type argument is
itself a referent (`ReturnType<typeof f>` nests `f` inside `ReturnType`), a
namespace-qualified head, or any path segment whose index expression names a different
binder — is OUT OF THE GRAMMAR and **DEFAULT-REJECTED** (stays `Ignored`). Those
multi-/nested-referent expressions are deferred to a NAMED blocking spike (§4 — "the
multi-referent `EvaluateExpr` locator-set spike"): admitting them requires the registry
to store a parsed locator SET / referent tree and the source-side walk to allowlist-check
EVERY referent, which the current single-`SourceLocator` shape cannot express. The
generator REJECTS an out-of-grammar `EvaluateExpr` at synthesis. Pinned by
`evaluate_expr_admission_is_single_root`.

**Shallow / Navigate hover-expansion gate (symmetric to the Expanded `Ref` reject).**
Hover display is NOT a mode-aware structured query — tsgo's hover may EXPAND a shallow
userland alias's body even when Verter (correctly, per the Component-Meta
Shallow-By-Default Rule) keeps a bare `Ref { name }`. In `Shallow` / `Navigate` mode the
expected surface for a userland alias is the `Ref`, so a hover that PRINTS the expanded
object instead of the alias NAME is a tsgo display artefact, not a real divergence —
admitting it would manufacture false divergence against Verter's correct `Ref`.
Therefore: in `Shallow` / `Navigate` mode, if the hover expands a userland alias instead
of printing its name, the `(row, query)` is **REJECTED / deferred** (it stays `Ignored`).
This is the exact mirror of the existing Expanded-mode reject of an UNexpanded userland
`Ref` (backstop rule 4): Expanded rejects a `Ref` that should have expanded; shallow
modes reject an expansion that should have stayed a `Ref`. Pinned by the
`shallow_hover_expansion_rejected` guard.

**Expanded-mode AND Skeleton-mode probe FORMS are BLOCKING spikes, not hand-waved.** Only
the `Shallow` / `Navigate` modes use the FIXED probe form above NOW. `Expanded` and
`Skeleton` are BOTH default-REJECTED in the first block until a NAMED blocking spike
validates a demonstrably lossless probe form for each. `Skeleton`-mode rows are
default-REJECTED for the SAME reason as `Expanded`: forcing tsgo to PRINT the
`TypeParameter`/`Infer` shell semantics for unbound generics (so Conditional branches do
not collapse to `never`) is an unsettled tsgo-printing question — there is no proven probe
form that reliably elicits the skeleton surface — so a `Skeleton`-mode row stays `Ignored`
until its blocking spike validates and versions a lossless probe form / oracle answer (a
`probe_synthesis_version` bump), exactly as `Expanded` is gated. (The line above naming
`Shallow` / `Navigate` / `Skeleton` together describes the §Q2 normalization handling
of a Skeleton surface once admitted; it does NOT license admitting an unspiked
`Skeleton`-mode ROW in the first block.) `Expanded` mode's added difficulty:
forcing tsgo to print an alias's expanded body
while preserving methods / call-signatures / optional / readonly modifiers is
non-trivial — a `{ [K in keyof T]: T[K] }` mapped wrapper strips call/method
signatures and rewrites optional/readonly, manufacturing false divergence or false
parity. The doc does NOT name a single magic construct. Instead, the generation SPIKE
(§4 "Spike") MUST validate a concrete, demonstrably lossless `Expanded`-probe form (and,
separately, a lossless `Skeleton`-probe form) against the pinned tsgo and FIX + VERSION it
(a `probe_synthesis_version` bump) BEFORE any `Expanded`-mode (resp. `Skeleton`-mode) row
is admitted. Until the spike proves and fixes a lossless form for a given construct class,
`Expanded`-mode AND `Skeleton`-mode rows in that class are **NOT admissible** and stay
`Ignored`; the initial scope admits only the modes/constructs the spike proves lossless.
This is a gating prerequisite, captured by the `expanded_probe_form_validated`,
`skeleton_probe_form_validated`, and `probe_form_is_deterministic_and_versioned`
guards.

#### The positive-allowlist admission gate (default-REJECT, pre-lowering, two-sided)

A check-AFTER lowering is **unsound**: by the time a hover has been lowered to a
`TypeExpr`, OXC has ALREADY silently erased the lossy construct, so a post-lowering
check cannot see — and cannot reject — what was dropped. The concrete miss:
`IdBranded<T> = T & { readonly [idBrand]: T }` (`fixtures/branded_types.ts:9-11`)
hovers as an intersection with a `unique symbol`-keyed brand member; OXC lowers
`unique symbol` straight through (`verter_type_expr_oxc/src/lib.rs:171`) and DROPS the
symbol-keyed member (`property_key_name` returns `None` for any non-static key —
`oxc/lib.rs:921` — so the `filter_map(lower_ts_signature)` at `oxc/lib.rs:99` silently
elides it). The result collapses toward `string & {}`, which would pass a post-lowering
check and produce **false parity** against a Verter side that also lost the brand.

Therefore the gate is a **closed POSITIVE ALLOWLIST, default-REJECT**, run **BEFORE
lowering** on the RAW PARSED AST. A construct is admissible ONLY when it appears on the
explicit ALLOWLIST below; ANY construct not on the list — anticipated or not — REJECTS.

**Two-sided admission (the allowlist is checked on BOTH the hover AST AND the fixture
SOURCE declaration).** tsgo hover can HIDE facts: it can summarize an overload set as a
single signature, elide `private`/`protected` from a public surface, or print an
accessor as a plain property. An admissibility check on hover text ALONE is therefore
unsound — a rejected construct could be present in the declaration yet invisible in the
hover. The generator MUST ALSO walk the fixture's REAL SOURCE declaration(s) of the
queried symbol against the SAME positive allowlist.

**The source-side walk resolves the REAL defining declaration(s) through the SHARED
resolver — NOT a new walker.** Verter has EXACTLY ONE type-resolution engine (the
typed-IR five-mode dispatch — CLAUDE.md "Exactly one type-resolution engine"); the
source-side allowlist walk MUST NOT introduce a second one. Starting from the typed
`source_locator` (`reference_canonical`, `reference_name`, `symbol_space`), the
generator resolves the actual DEFINING declaration(s) by consulting the SAME shared
declaration graph the resolver uses — the `ShallowFileState` symbol inventory + the
shared resolver's import/export/barrel routing + the `MergedDecl` peer-merge
contributor set — so that:

- a name resolved in the wrong space is disambiguated (`symbol_space` selects the
  TYPE vs VALUE inventory — `typeof f` walks `f`'s VALUE declaration, a type query on
  `Foo` walks `Foo`'s TYPE declaration; a name that exists in both spaces does not
  cross-contaminate);
- an IMPORT / REEXPORT / barrel alias is followed to its ultimate defining
  declaration through the shared import-graph routing (a `Relate`-free row whose
  symbol is `import { X } from "./barrel"` walks `X`'s real definition in the leaf
  module, not the bare re-export node) — the walk follows ONLY the import graph
  reachable from the requested declaration (the Macro Type Traversal Rule), never
  unrelated imports;
- a MERGED declaration is walked across EVERY contributor: when the shared resolver
  reports the symbol is a `MergedDecl` (multiple same-name `interface` /
  ambient-augmentation contributors, or an ordered `ValueDeclGroup` of function
  overloads), the source-side walk checks the allowlist against EACH contributor
  surface — a single contributor being allowlist-clean does NOT admit the merge if
  another contributor carries a REJECT construct;
- an OVERLOAD SET (an ordered `Vec<FunctionSignature>` with ≥2 signatures, or a
  callable intersection) is seen as the multi-signature group it is — and REJECTED
  per the overload-set REJECT row — rather than collapsed to the single signature a
  hover summary would show;
- a CLASS member surface is walked with its declared `MemberVisibility` and
  accessor-kind intact (so a `private`/`protected` member or a getter/setter
  accessor REJECTS even when the hover prints a public plain-property summary).

Because the walk reuses the shared resolver's declaration graph it is NOT a parallel
resolution path: it asks the one engine "what declaration(s) does this locator bind
to, and what is each contributor's surface" and then runs the SAME positive-allowlist
predicate over the returned surfaces. It does NOT re-implement import resolution,
merge ordering, or space selection.

**Admission requires the HOVER capture AND EVERY resolved source defining declaration
(across import chains + merged contributors) to be allowlist-clean** — if either the
hover OR any contributor surface carries a non-allowlisted (REJECT) construct, the
query is deferred, even if the other side hid it. A source locator that the shared
resolver CANNOT bind to a defining declaration inside the controlled fixture set (an
unresolved import, a missing leaf, an ambiguous unresolved name) REJECTS the query —
the generator never admits a capture whose real source it could not reach and walk.
This is the two-sided, defining-declaration-resolving admissibility rule, pinned by
the `source_declaration_allowlist_clean` guard (which now requires defining-decl
resolution through the shared graph, including type-vs-value space, import/reexport
chains, and merged contributors).

**The source-side walk's concrete entry API + return shape (a real harness component,
reusing the ONE shared resolver).** The source-side walk is a named, implementable
generator-side component, NOT just a guard detail. It is built on the SHARED resolver's
already-retained source representation — it adds NO parallel resolution path:

- **What the shallow inventory provides — and the lossy facts it has ALREADY erased.**
  Per the Shallow File Processing core invariant, when a canonical file is processed the
  host stores its `ShallowFileState` symbol inventory (`IndexedReady`) — imports /
  exports+reexports / type declarations / interfaces / enums / classes /
  variables+constants / functions+method signatures / `typeof`-relevant value
  declarations / local + cross-file dependency edges — plus the `MergedDecl` peer-merge
  contributor set. This inventory is the AUTHORITATIVE DECLARATION GRAPH the source-side
  walk navigates: it resolves a `source_locator` to its defining contributor(s), follows
  import/reexport/barrel hops, enumerates `MergedDecl` contributors, and supplies the
  `typeof`/`ReturnType`/`Parameters` dependency edges for the transitive walk. The
  contributor's declaration BODY is stored **already-lowered** — `ShallowTypeSymbol.body:
  TypeDeclBody` wraps a `TypeExpr`
  (`Single(TypeExpr)` / `Merged(Vec<TypeExpr>)`,
  `crates/verter_session/src/resolver_core/shallow_file_state.rs:155`,
  `crates/verter_semantic/src/analysis/type_eval.rs:130`) and `TypeDeclInfo.body` is a
  bare `TypeExpr` (`type_eval.rs:36`). That lowered body IS the admission surface for the
  NON-erased rejectable variants (it carries `Conditional` / `Mapped` / callable /
  `TemplateLiteral` / `Infer` / `KeyOf` / `IndexedAccess` / `TypeOf` / enum-member `Ref` /
  `RecursiveRef` faithfully — see the combined-input rule below). What it is NOT is the
  surface for the SILENTLY-ERASED facts: by the time a body reaches `TypeExpr` those
  admission-relevant facts the source-side allowlist EXISTS to catch are ALREADY GONE —
  exactly the OXC-lowering erasures §Q2 enumerates: a `unique symbol` operator lowered
  straight through (`oxc/lib.rs:171`); a computed / `symbol`-keyed member dropped at
  `oxc/lib.rs:99,921`; an accessor that is not even an `ObjectMember` variant
  (`lib.rs:426`); `private`/`protected` visibility stamped public (`oxc/lib.rs:427`); an
  `abstract` constructor flag ignored (`oxc/lib.rs:126`); `as const` / `readonly` value
  provenance collapsed; an overload SET summarized; a utility-type referent's raw
  spelling normalized away. Reading the lowered body would therefore make the source side
  blind to precisely THOSE constructs it was added to reject — the post-lowering blindness
  §Q2 already proves UNSOUND on the hover side. So for the **silently-ERASED** admission
  facts (`unique symbol`, computed/`symbol`/unique-symbol keys, member visibility,
  accessors, `abstract` ctor, `const`/variance type-param, `this` type/param, `as const`
  provenance, the overload SET, the raw utility referent spelling, tuple optional-vs-`|
  undefined`) the source-side walk does NOT trust the lowered body; it reads a SEPARATE
  retained **raw-fact inventory** (below) captured before lowering.

  But the default-REJECT set is NOT exhausted by the erased facts. The other rejectable
  constructs — a `Conditional`, a `Mapped` type, a callable surface (`FunctionType` /
  `ConstructorType`), a `TemplateLiteral`, an `Infer`, a `KeyOf` / `IndexedAccess` /
  `TypeOf` outside its spike-proven mode, an enum-member `Ref`, a `RecursiveRef` — are
  NOT silently erased: they survive lowering as FIRST-CLASS `TypeExpr` variants and are
  fully visible in the lowered body. For those, the lowered body IS the authoritative
  surface; the `RawSourceSurface` does not (and need not) carry them. So the **source-side
  admission input is the COMBINED pair** `(RawSourceSurface raw facts) + (the contributor's
  already-lowered source body `TypeExpr`)`: the raw-fact inventory supplies the erased
  facts the lowered body lost, and the lowered body supplies the non-erased rejectable
  `TypeExpr` variants. The allowlist predicate walks BOTH halves and admission requires
  BOTH clean — neither half alone is sufficient (the raw facts alone miss a `Mapped`/
  `Conditional`/`TemplateLiteral` source body; the lowered body alone misses an erased
  brand/visibility/overload fact). Reading the lowered body for the non-erased half is NOT
  the unsound post-lowering hover check §Q2 rejects: that check was unsound because it
  read the lowered body for the ERASED constructs, which are gone by then; here the lowered
  body is read ONLY for the variants that DO survive lowering losslessly, while the erased
  half is taken from the raw-fact inventory. The shallow inventory's role is graph
  navigation (binding, import/merge routing, dependency edges); the COMBINED raw-fact-
  inventory + lowered-body pair is the admission surface.
- **The retained `RawSourceSurface` raw-fact inventory — captured at INITIAL PARSE,
  before lowering, by the SAME parse pass the shallow inventory rides on.** Because the
  lowered body has erased the admission-relevant facts, the harness retains them
  SEPARATELY. During the file's INITIAL PARSE — the one parse pass that already produces
  the shallow inventory (the transient per-file OXC arena is live exactly here, before it
  is dropped) — a parse-time capture records, per top-level declaration, a
  `RawSourceSurface` of the admission-relevant RAW facts the lowering would erase. This is
  a parse-time fact SCRAPE whose `(canonical, name, symbol_space)` triple — the same triple
  the shallow inventory keys — maps NOT to a single `RawSourceSurface` but to an ORDERED,
  CONTRIBUTOR-KEYED vector of raw surfaces, each keyed within the merge group by its
  `(contributor_ordinal, decl_span)` (the "Stable contributor identity" bullet below): a
  MERGED decl (same-file merged interfaces, an overload group, repeated `declare`s) shares
  one triple across several contributors, so a lossy single-value map would silently drop
  all but one contributor's raw facts. The vector is stored on the file's content-addressed
  artifact alongside `IndexedReady`
  (it is per-file, per-content-hash, `Send + Sync`, dropped/recomputed with the file's
  artifact entry — never a borrowed AST pointer or a retained parser arena). The
  `RawSourceSurface` retains EXACTLY this closed set of pre-lowering admission facts (each
  the catch-target of a §Q2 REJECT row), and NOTHING that survives lowering losslessly
  (those are read from the lowered body):

  ```
  RawSourceSurface {
      decl_canonical:  leading-slash file id of THIS contributor,
      decl_kind:       TypeAlias | Interface | Enum | Class | Function | Variable | …,
      raw_member_keys: per object/class member, the RAW key form
                       — Static(name) | Computed | SymbolKeyed | UniqueSymbolKeyed
                       (so a computed / symbol / unique-symbol key is visible as such,
                        not silently dropped — oxc/lib.rs:99,921),
      member_kinds:    per member, Property | IndexSignature | Getter | Setter | Method
                       | CallSignature | ConstructSignature
                       (so an accessor is visible as Getter/Setter, not collapsed to a
                        plain property — lib.rs:426),
      member_visibility: per member, Public | Private | Protected
                       (the DECLARED modifier, before oxc/lib.rs:427 stamps it public),
      unique_symbol_ops: each `unique symbol` type-operator occurrence
                       (before oxc/lib.rs:171 lowers it straight through),
      abstract_ctor:   whether a constructor type / class carries `abstract`
                       (before oxc/lib.rs:126 ignores it),
      type_param_modifiers: per type parameter, the `const` flag + `in`/`out` variance
                       (no TypeParam carrier — lib.rs:1018),
      this_type_or_param: whether the decl uses a `this` type or a `this` parameter
                       (erased to Ref("this") / unrepresentable — oxc/lib.rs:223,
                        type_expr/src/lib.rs:927),
      value_const_assertion: for a value/`typeof` referent, whether the initializer is
                       `as const` and the readonly/literal-tuple provenance
                       (collapsed by lowering),
      overload_signatures: the ORDERED raw signature group as written
                       (≥2 ⟹ an overload SET; the source side sees the multi-signature
                        group a hover summary would collapse),
      utility_referent_names: for a utility-type application
                       (`ReturnType<…>`/`Parameters<…>`/`Pick<…>`/…), the RAW referent
                       identifier(s) as written,
      tuple_element_shape: per tuple element, Optional | Labelled | `| undefined`
                       presence (the optional-vs-`| undefined` distinction tsgo/lowering
                        collapse),
      transitive_referents: Vec<SourceLocator>,  // typeof/ReturnType/Parameters next hops
  }
  ```

  This capture is **PARSING, not type resolution** — it reads syntax facts off the OXC
  parse tree during the file's own initial parse and stores owned data; it never
  RESOLVES a type, instantiates a generic, walks a member surface, or consults the
  five-mode dispatch. The one shared type-resolution engine
  (`SemanticQueryKey → ProjectSemanticDispatch::execute`) is UNTOUCHED — `RawSourceSurface`
  carries no resolved types and does not re-resolve anything. It is the same category of
  artifact as the shallow symbol inventory itself (a parse-time index of syntax facts),
  extended with the admission-relevant facts lowering happens to erase. A reviewer must
  NOT read this as a second resolver: there is one parse (which produces both the shallow
  inventory and the raw-fact inventory) and one resolver (the five-mode dispatch the
  source-side walk uses ONLY for binding/import/merge navigation, never for admission).
  The source-side admission predicate runs over the COMBINED input — the
  `RawSourceSurface` raw facts (for the silently-erased constructs) PLUS the contributor's
  already-lowered source body `TypeExpr` (for the non-erased rejectable variants:
  `Conditional` / `Mapped` / callable / `TemplateLiteral` / `Infer` / `KeyOf` /
  `IndexedAccess` / `TypeOf` / enum-member `Ref` / `RecursiveRef`). The lowered body is
  the SAME `ShallowTypeSymbol.body` / `TypeDeclInfo.body` `TypeExpr` the navigation step
  already retains — no extra lowering, no re-resolve, no re-parse; reading it for the
  non-erased half is a field read off the existing shallow artifact, not a second engine.
- **Entry API (the one shared engine for NAVIGATION; the combined raw-fact + lowered-body
  pair for ADMISSION).** The walk's entry
  point is a generator-side helper, e.g.
  `resolve_source_declarations(&ResolverContext, &SourceLocator) -> SourceWalkResult`,
  which asks the shared resolver "what declaration(s) does this typed locator
  (`reference_canonical`, `reference_name`, `symbol_space`) bind to", and for EACH bound
  contributor fetches BOTH that contributor's retained `RawSourceSurface` (the parse-time
  raw-fact inventory above, for the erased facts) AND its already-lowered source body
  `TypeExpr` (`ShallowTypeSymbol.body` / `TypeDeclInfo.body`, for the non-erased rejectable
  variants), keyed by that contributor's STABLE per-decl identity (below). Binding /
  import/export/barrel routing / `MergedDecl` contributor enumeration / TYPE-vs-VALUE
  space selection all route through the SAME `ResolverContext` / `ShallowFileState`
  inventory the resolver uses; the COMBINED admission surface is the per-contributor
  `(raw facts, lowered body)` pair, read off the existing shallow artifact. It is a
  NON-OWNING query into the shared graph for navigation (it chooses hops and reads
  contributor identities) plus a content-keyed lookup of each contributor's retained raw
  facts + lowered body — never a private drill-down and never a re-resolution.
- **Stable contributor identity (a MERGED decl is an ORDERED KEYED VECTOR).** A
  `(canonical, name, symbol_space)` triple does NOT uniquely name a contributor when a
  symbol is MERGED in one file — same-file merged interfaces, an overload group, or
  repeated `declare`s all share that triple. So each contributor carries a STABLE
  per-decl identity beyond the triple: its **contributor ordinal** (the 0-based
  source/binder position within the merge group, the same order
  `EvalEnv`'s `TypeDeclGroup` / `ValueDeclGroup` append in, CLAUDE.md Declaration
  Merging) PLUS its declaration **span** (a stable decl id). The shared resolver already
  enumerates the `MergedDecl` peer-merge contributor set in this binder order; the walk
  preserves it. A merged symbol therefore resolves to an ORDERED, KEYED contributor
  vector — `[(ordinal 0, span, raw facts, body), (ordinal 1, …), …]` — each member
  INDEPENDENTLY allowlist-checked and INDEPENDENTLY re-capturable, so a rejected merged
  contributor is unambiguously addressable by `(ordinal, span)`.
- **Return shape (`SourceWalkResult`).** The helper returns a closed result the
  allowlist predicate consumes directly:

  ```
  SourceWalkResult =
      Resolved { contributors: Vec<SourceContributor> }  // ORDERED, ≥1 defining decl
    | Unresolved                                          // locator did not bind in the fixture set → REJECT
    | Cycle                                               // a visited-set re-entry on the transitive walk → REJECT

  SourceContributor {
      ordinal:    u16,            // 0-based source/binder position in the merge group
      decl_span:  Span,           // stable declaration id, disambiguates same-triple peers
      raw_surface: RawSourceSurface,  // the retained parse-time raw-fact record (erased facts)
      lowered_body: TypeExpr,     // the already-lowered ShallowTypeSymbol.body / TypeDeclInfo.body
                                  //   (non-erased rejectable variants: Conditional / Mapped /
                                  //    callable / TemplateLiteral / Infer / KeyOf / IndexedAccess /
                                  //    TypeOf / enum-member Ref / RecursiveRef)
  }
  ```

  Each `SourceContributor` pairs the retained parse-time raw-fact record with the
  contributor's lowered body (NOT a re-derived view — the lowered body is the shallow
  artifact the resolver already holds). `Resolved.contributors` is an ORDERED vector
  carrying EVERY contributor surface (so a merged decl or overload group is seen as the
  multi-surface group it is, each member keyed by `(ordinal, decl_span)`); the allowlist
  runs over every contributor's `(raw_surface, lowered_body)` pair and admission requires
  ALL clean — a single allowlist-clean contributor does NOT admit the merge if another
  contributor (a different `ordinal`) carries a REJECT construct in EITHER half.
  `Unresolved` (the locator did not bind, OR a bound contributor has NO retained
  `RawSourceSurface` — a stale/absent capture) and `Cycle` both REJECT the `(row, query)`.
  The `RawSourceSurface.transitive_referents` drive the transitive walk below; the walk
  re-enters `resolve_source_declarations` for each, under the visited-set + cycle guard.
  This is the implementable depth: one named entry API, one closed return shape, the
  shared resolver for navigation + the per-file parse-time raw-fact inventory + the
  retained lowered body for admission — no second resolver, no query-time re-parse, no
  re-resolve.

**The source-side walk is TRANSITIVE through `typeof` / `ReturnType` / `Parameters`,
following the SHARED resolver's declaration graph at each hop.** Walking ONLY the alias
declaration's own AST is insufficient when the alias DERIVES its type from a value or
function. `type ObjectConstType = typeof objectConst` over an `as const` object
(`value_inference.rs`'s `upsert_value_fixture` value source; the `readonly`-tuple
contract at `value_inference.rs:106`) carries its lossy facts in the VALUE
INITIALIZER, not in the `typeof` alias node; `ReturnType<typeof f>` carries them in
`f`'s BODY. So the source-side allowlist walk must follow `typeof x` to `x`'s value
INITIALIZER (resolving `x` in VALUE space through the shared graph), and
`ReturnType<…>` / `Parameters<…>` / `InstanceType<…>` / `ConstructorParameters<…>` to
the referenced function/constructor's DEFINING declaration (resolved through the same
import/export/merge-aware graph as above), walking the TRANSITIVE source against the
same positive allowlist. Each transitive hop re-enters the SHARED resolver's
declaration graph (it does not re-implement resolution) — so a `typeof`/`ReturnType`
referent that lives behind an import / barrel / merge is reached through the one
engine, and a merged or overloaded referent is walked across all its contributors. If
the transitive source cannot be resolved through the shared graph (the referent is not
in the controlled fixture set) OR any contributor it reaches contains a
non-allowlisted construct, the query is **REJECTED**.

**The transitive walk has a VISITED-SET + cycle→REJECT termination guard.** A
`typeof`/`ReturnType`/`Parameters` chain can be CYCLIC (`type A = typeof b; const b: A`,
a mutually-`ReturnType`-referencing pair, a self-referential helper) — an unguarded
walk would loop forever and HANG generation, and an undefined-admissibility hang is
itself a false-parity door (a class that neither admits nor cleanly rejects). The
walk therefore maintains a VISITED SET of `(canonical, name, symbol_space)` declaration
keys it has already entered; re-entering an already-visited key is a CYCLE and the
`(row, query)` is **REJECTED / deferred** (a cyclic source surface is not admissible via
this harness — it routes to the future structured oracle, never to a best-effort or
hung admit). Because every hop re-enters the SHARED resolver's declaration graph rather
than a private walker, the allowlist-walk wrapper ALSO inherits the shared resolver's
own termination guarantee for parameterized/generic helper recursion (the host-cached
transitive cycle detection backed by `RefCycleResultDb`, CLAUDE.md) — the wrapper's
visited-set is the harness-side belt-and-suspenders on top of that engine-side guard, so
the walk terminates whether the cycle is in plain `typeof`/`ReturnType` referents or in
generic-helper instantiation. Pinned by the
`source_walk_is_transitive_through_typeof` and `source_walk_cycle_rejected` guards.

The admission order — a capture is admitted only when ALL of these pass:

1. **Parse the hover type text to the OXC type AST; resolve the source contributors.**
   Extract the type expression from the `type __oracle_probe__N = <T>;` header and parse
   `<T>` to an OXC `TSType`. On the SOURCE side, resolve the queried symbol's defining
   contributor(s) via `resolve_source_declarations` (above) to the ordered
   `Vec<SourceContributor>`, each carrying its retained `RawSourceSurface` raw facts +
   already-lowered body `TypeExpr` — NOT a query-time re-parse (the raw facts were
   captured at the file's initial parse; the lowered body is the shallow artifact).
2. **Walk the hover AST AND every source contributor against the POSITIVE ALLOWLIST**
   (below). On the hover side the walk runs over the parsed OXC type AST (the lossy
   constructs STILL EXIST there — the `unique symbol`, computed/symbol key, `this`
   type/param, accessor, overload set — before OXC erases them, and before a hover
   summary can hide a source-side one). On the source side the walk runs over the
   COMBINED `(raw_surface, lowered_body)` pair of EVERY contributor: the raw facts catch
   the erased constructs, the lowered body catches the non-erased rejectable `TypeExpr`
   variants (`Conditional` / `Mapped` / callable / `TemplateLiteral` / `Infer` / `KeyOf`
   / `IndexedAccess` / `TypeOf` / enum-member `Ref` / `RecursiveRef`). A construct NOT on
   the allowlist — on the hover OR in EITHER half of ANY contributor — rejects the
   capture. The default verdict for an unanticipated node is REJECT, never admit.
3. **Lower with a STRICT drop-counter.** Lower the admitted hover AST via
   `lower_ts_type` instrumented so that any member/param the lowering would
   `filter_map`-drop (`oxc/lib.rs:99`; the JSON-codec `filter_map` sites at
   `type_expr_json.rs:72,130,153,190,218,336`) INCREMENTS a drop count; a non-zero
   drop count REJECTS the capture. This is the belt-and-suspenders backstop to the
   allowlist walk: the walk should already have rejected everything droppable, and a
   non-zero count means the allowlist admitted something it should not have (a bug in
   the allowlist, not an admit).
4. **Produce the snapshot** from the lowered, zero-drop `TypeExpr`.

**The closed POSITIVE ALLOWLIST.** The `hover_construct_whitelist` guard walks THIS
closed positive allowlist over the OXC type-AST node kinds and the resulting `TypeExpr`
variants; anything not enumerated as ADMIT here is REJECT by default. `ADMIT` = always
lossless + not hover-elidable; `ADMIT(pred)` = admit only when the predicate holds, else
REJECT.

**ADMITTED constructs** (everything else is REJECT by default):

| OXC type-AST node → `TypeExpr` | Verdict |
| --- | --- |
| keyword primitives (`string`/`number`/`boolean`/`bigint`/`symbol`/`void`/`null`/`undefined`/`object`/`unknown`) → `Primitive` | ADMIT |
| string / number / boolean / bigint literal → `Literal` | ADMIT |
| `T[]` / `Array<T>` array → `Array` | ADMIT(element admissible) |
| `readonly T[]` / `readonly [A,B]` type operator → `Array`/`Tuple { readonly }` | ADMIT (readonly preserved, `oxc/lib.rs:165-167`) |
| FIXED tuple `[A, B]` → `Tuple` | ADMIT(every element admissible AND each element is non-optional + unlabeled + not `\| undefined`; labelled / optional `[A, B?]` / `\| undefined` members are NOT admitted — TS collapses the optional-element vs `\| undefined` distinction in hover — unless the spike proves them representable+lossless) |
| object type-literal `{ … }` with ONLY public data members → `Object` | ADMIT(every member is a static-keyed `name: T` Property or `[k: K]: V` IndexSignature whose value is admissible; carries name / optional / readonly / type only) |
| object member: `name: T` static-keyed property → `Property` | ADMIT(type admissible) |
| object member: index signature `[k: string]: T` → `IndexSignature` | ADMIT(value admissible) |
| `Ref` with type-args → `Ref { name, type_arguments }` | ADMIT(the `Ref` is NOT an enum-member / qualified-name ref AND every type-arg admissible). A qualified-name ref (`Color.Red`, `Status.Idle`) lowers to a plain `Ref` (`qualified_name_to_string`, `oxc/lib.rs:~305`), losing its enum-member nominal brand — those are NOT on the ADMIT list and fall through to REJECT (see the enum-member REJECT row). Among non-qualified `Ref`s: shallow modes ADMIT a userland `Ref` as the correct surface; a package/builtin `Ref` (`Promise`, `Array`) is ADMITted as a canonical `Ref`, not expanded; an `Expanded`-mode userland `Ref` is ADMITted ONLY when the spike-validated forced-expansion probe expanded it — an UNexpanded `Expanded`-mode userland `Ref` is REJECT |
| `T \| U` union → `Union` | ADMIT(every arm admissible AND NON-CALLABLE) |
| `T & U` intersection → `Intersection` | ADMIT(every arm admissible AND NON-CALLABLE — a callable intersection is an overload group, REJECT) |
| `T['K']` indexed access → `IndexedAccess` | ADMIT(object + index admissible) ONLY in modes the spike proves tsgo prints losslessly; else deferred |
| `keyof T` → `KeyOf` | ADMIT(operand admissible) ONLY in spike-proven modes; else deferred |
| `typeof x` → `TypeOf` | ADMIT(the hover prints the resolved type, admissible) ONLY in spike-proven modes; else deferred |
| `T extends U ? X : Y` conditional → `Conditional` | ADMIT(all four arms admissible) ONLY in spike-proven modes (tsgo may collapse open conditionals); else deferred |
| mapped type `{ [K in …]: … }` → `Mapped` | ADMIT(modifiers preserved) ONLY in spike-proven modes; else deferred |
| template-literal type → `TemplateLiteral` | ADMIT(all quasis + expressions admissible; order preserved) ONLY in spike-proven modes; else deferred |
| `infer X` → `Infer` | ADMIT(inside an admissible spike-proven conditional); else deferred |

**REJECTED constructs** (each cites WHY it cannot be admitted; this list is
illustrative of the default-REJECT half — any construct absent from the ADMIT table is
equally rejected):

| Construct | Why REJECTED |
| --- | --- |
| `unique symbol` type operator | `TSTypeOperatorOperator::Unique` lowers straight through to the inner type, erasing `unique` (`oxc/lib.rs:171`) |
| computed / `symbol`-keyed object key (`[k]: T`, `[uniqueSym]: T`) | `property_key_name` returns `None` for any non-static key (`oxc/lib.rs:921`), so the member is silently dropped at `oxc/lib.rs:99` |
| `this` TYPE | `TSThisType` lowers to `Ref("this")` (`oxc/lib.rs:223`), erasing the `this`-type distinction |
| `this` PARAMETER (`fn(this: T, …)`) | `FunctionParam` has no receiver / `this` flag — only name/ty/optional/rest/span/has_ts_annotation (`type_expr/src/lib.rs:927`); a `this` parameter is unrepresentable |
| `const` / variance (`in`/`out`) TYPE PARAMETER | `TypeParam` has only name/constraint/default — no `const` modifier and no variance field (`lib.rs:1018`) |
| `abstract` constructor type | `ConstructorType` wraps only a `FunctionExpr` with no `abstract` flag (`lib.rs:159`), and OXC's `TSConstructorType` lowering IGNORES constructor abstractness (`oxc/lib.rs:126`) |
| `private` / `protected` member visibility | `MemberVisibility` participates in `TypeExpr` identity (`lib.rs:494`) and the JSON emits non-public visibility, but OXC type-literal lowering stamps every member PUBLIC via `with_spans_public` (`oxc/lib.rs:427`), so a non-public surface is unrepresentable from a hover/type-literal |
| getter / setter accessor | NOT an `ObjectMember` variant — `lib.rs:426` is only Property / IndexSignature / CallSignature / ConstructSignature / Method — so an accessor cannot be represented |
| enum-member type (`Color.Red`, `Status.Idle`, `Direction.Up`; the alias `type ColorRed = Color.Red`, `fixtures/enums.ts:21,26`; the branded-member contracts `enums.rs:18,39`) | `TypeExpr` has NO enum-member / brand carrier (the enum closed at `lib.rs:128` has no enum-member variant); an enum-member type is a NOMINAL brand whose identity `TypeExpr` cannot represent (per the structural-parity claim, §Scope). It would lower at best to a bare `Ref` or a stripped literal, losing the brand → false parity. DEFAULT-REJECTED: an enum-member `Ref` is not on the positive ADMIT list, so it falls through to REJECT. Deferred to a future nominal/structured oracle. |
| OVERLOAD SET / callable intersection (≥2 call/construct/method sigs of one name; an intersection-of-functions) | An overload set cannot be reconstructed losslessly from a hover summary; the canonical ordered overload group is the production `ValueDeclGroup::merged_signatures` ordered `Vec<FunctionSignature>` (`crates/verter_semantic/src/analysis/type_eval.rs:~265,317`; the `declaration_merge.rs:33` `overload_param_primitives` helper is the TEST that characterizes the intersection-of-functions encoding, not the production site), so a callable union/intersection arm is REJECT |
| single call / construct / method / function / constructor signature in a hover-summarizable position | DEFERRED to the spike: a single signature is representable, but a hover that summarizes an overload as one signature is indistinguishable from a genuine single signature on the hover side alone — so callable surfaces are admitted only when the SOURCE-side allowlist confirms exactly one signature (the two-sided rule) and the spike validates the print form |
| optional / labelled tuple element, `\| undefined` tuple member | TS collapses the optional-element vs `\| undefined` distinction in hover; not admitted unless the spike proves it representable + lossless |
| recursive self-reference → `RecursiveRef` | a self-referential surface cannot be captured as a finite hover |
| `any` keyword → `Primitive(any)` | `any` in a concrete-type position (backstop rule 3) |
| `never` outside a genuine closed empty union | `never` where a concrete type is expected (backstop rule 3) |
| anything that lowers to `TypeExpr::Unknown` | parse leftovers / unrepresentable (backstop rule 2) |

A query is hover-admissible ONLY when BOTH its SOURCE declaration and its TS7 hover
walk the positive allowlist with EVERY construct ADMITted (and every `ADMIT(pred)`
predicate satisfied), AND the strict drop-counter is zero. **Any query whose SOURCE or
hover uses a non-allowlisted (REJECT) construct is DEFERRED to a future structured
oracle** — it is never lifted via this harness.

#### Backstop reject rules (retained)

The four reject rules below are folded into the table above and remain as an
independent BACKSTOP. The generator HARD-REJECTS a capture — writes no snapshot, the
row is NOT lifted via hover — when ANY hold:

1. **Truncation marker.** The hover text contains an ellipsis / truncation marker
   (`…` / `...`) — TS truncated a large type. (Checked on the raw hover text before
   parsing.)
2. **Parse leftovers / `Unknown`.** Parsing the hover type text leaves parse leftovers,
   or the post-admission lowering produces a `TypeExpr::Unknown`.
3. **Unexpected `any` / `never`.** The captured type contains an `any` (always REJECT)
   or a `never` in a position where a concrete type is expected.
4. **Unexpanded userland `Ref` in `Expanded` mode.** For an `Expanded`-mode query the
   hover yields an unresolved userland `Ref` (e.g. tsgo printed `Pick<Foo,"bar">`
   instead of the expanded object) — UNLESS the query's projection policy explicitly
   states alias preservation IS the semantic answer (the shallow modes, where a `Ref`
   is the correct surface).

Admissibility is decided one `(row, query)` at a time — a per-ROW/per-QUERY split,
NOT per-`OracleId`-family. A family may have some queries that hover losslessly and
some that do not. The contract is pinned by the `hover_construct_whitelist` (walks the
closed POSITIVE allowlist, default-REJECT), `source_declaration_allowlist_clean`
(the two-sided source+hover check), `strict_lowering_drop_counter`,
`pre_lowering_loss_rejected`, `class_visibility_accessor_rejected`,
`probe_header_names_target`, and `hover_capture_is_lossless_or_rejected` guards (§5);
`non_admissible_query_not_lifted_via_hover` ensures a non-admissible query never
appears as `Lifted` with a hover snapshot.

#### Env pinning (compiler options, tsgo version)

- **tsgo version.** `package.json:69` declares `"@typescript/native-preview":
  "latest"`. The implementation MUST pin it to the exact
  `7.0.0-dev.20260526.1`. This is a docs-only design; the pin is an implementation
  step plus the `tsgo_version_is_pinned` guard — the doc does not edit
  `package.json`.
- **The CANONICAL oracle tsconfig + VENDORED corpus root (the standalone-host
  resolution).** The DOMINANT population resolves under `VerterHost::new_standalone`
  — no project root, no tsconfig (fact 9). To give tsgo a deterministic, HERMETIC
  config for those rows, the generator vendors a single CLOSED oracle-env corpus under
  a checked-in directory
  `crates/verter_session/src/typeinfo/typeinfo_tests/oracle_env/<env_corpus_id>/`
  containing the canonical `oracle.tsconfig.json`, a synthetic root, and the COPIED
  BYTES of every lib / ambient / package `.d.ts` + resolution-affecting manifest the
  standalone rows pull — and drives EVERY standalone-host row's tsgo `--lsp` / root
  pointed at THAT vendored corpus root, NEVER live `node_modules` (gitignored,
  non-hermetic, and where tsgo bundles its libs, `ipc.rs:~2859-2874`, `.gitignore:9`).
  The same config + corpus for all standalone rows yields a stable, shared
  `compiler_options_hash` AND a stable `env_corpus_id`, and the corpus's relative paths
  + content hashes + directory manifest are recorded in each standalone snapshot's
  `oracle_env_files` (§Q1) so the env is pinned, closed, and offline-re-enumerable.
  Regeneration re-vendors the corpus and recomputes `env_corpus_id`. This single shared
  canonical corpus + config is what makes the tsgo-free `snapshot_id` derivation airtight,
  and it covers the ONLY first-class population: `standalone`-host rows. The
  `workspace_footprint` rows (the ~9-row minority) are NOT first-class — they drive tsgo
  under their OWN per-host `/workspace/tsconfig.json`, which would need PER-HOST env/option
  pins rather than this one shared corpus, so they are DEFERRED to the named
  `workspace_footprint` per-host env-pin spike (§Scope, §4) and stay `Ignored` until it
  lands. The env-pinning model here is internally consistent precisely because it pins
  exactly ONE shared corpus for the one first-class population.
- **`compiler_options_hash` — a CLOSED, ENUMERATED recipe over the EFFECTIVE config,
  rooted in the COMMITTED canonical `oracle.tsconfig.json`.** The canonical
  `oracle.tsconfig.json` is a LITERAL CHECKED-IN file vendored as a corpus member
  (`oracle_env/<env_corpus_id>/oracle.tsconfig.json`, listed in `oracle_env_files` and
  hashed into `oracle_env_hash` like any other corpus file). It is the SOURCE OF TRUTH —
  two implementers derive the SAME `compiler_options_hash` because both read the SAME
  committed bytes, never a reconstructed map. The hash is computed over the final
  EFFECTIVE config (NOT the literal file text, so a cosmetic edit that does not change
  any effective option does not churn the hash), via this CLOSED recipe:

  > **Two distinct axes, NOT a contradiction (the `oracle.tsconfig.json` cosmetic-edit
  > tradeoff).** `oracle.tsconfig.json` is BOTH the source for the effective-option map
  > AND a VENDORED CORPUS MEMBER (it is listed in `oracle_env_files`, hashed into
  > `oracle_env_hash`, and folded into `env_corpus_id`). These two roles have OPPOSITE
  > cosmetic-edit behavior, deliberately:
  > - **`compiler_options_hash` is cosmetically STABLE** — it hashes the EFFECTIVE option
  >   map, so a comment, key reorder, or whitespace edit to `oracle.tsconfig.json` that
  >   leaves every effective option unchanged does NOT churn `compiler_options_hash`.
  > - **`env_corpus_id` (and thus every `snapshot_id`, since `env_corpus_id` enters the
  >   filename) IS cosmetically SENSITIVE** — because `oracle.tsconfig.json` is a vendored
  >   corpus member content-hashed by its BYTES, a cosmetic edit to it DOES change the
  >   corpus content id and RE-KEYS `env_corpus_id` + all standalone `snapshot_id`s.
  >
  > This is content-addressed CHURN (a cheap full regeneration — re-vendor the corpus,
  > re-pin `CURRENT_ENV_CORPUS_ID`, rewrite the snapshot filenames), NOT a collision or an
  > inconsistency. The two axes measure different things: `compiler_options_hash` is the
  > EFFECTIVE-OPTION-MAP identity (cosmetically stable on purpose, so an option-neutral
  > edit does not invalidate the option pin), while `env_corpus_id` is the VENDORED-CORPUS
  > BYTE identity (cosmetically sensitive on purpose, so the offline re-enumeration stays
  > exact). A cosmetic tsconfig edit therefore leaves `compiler_options_hash` untouched but
  > re-keys `env_corpus_id` — both behaviors are correct for their respective role, and
  > the regeneration is the (cheap) cost of the corpus being byte-content-addressed.

  1. **Read the committed `oracle.tsconfig.json` `compilerOptions`.**
  2. **Expand `strict`** into its EXACT subflag set:
     `strictNullChecks`, `strictFunctionTypes`, `strictBindCallApply`,
     `strictPropertyInitialization`, `noImplicitAny`, `noImplicitThis`, `alwaysStrict`,
     `useUnknownInCatchVariables` (these eight, no more).
  3. **Overlay the CLOSED EFFECTIVE-OPTION MAP** — exactly the keys below, each with the
     committed-file value if present, else the version-pinned default. This is a CLOSED
     SET: there is NO open-ended "every option that affects the printed type". An option
     not in this table is NEITHER hashed NOR sent; if a future tsgo print behavior is
     found to depend on an option not listed here, the option is ADDED to this table
     under a `tsgo_version` bump (a schema change), never folded in via a vague "every
     option" clause. The closed effective-option key set is:

     | Effective-option key | Source |
     | --- | --- |
     | the eight `strict` subflags above | `strict` expansion |
     | `target` | committed value (default `es2020`) |
     | `lib` | committed value (the canonical lib set) |
     | `module` | committed value |
     | `moduleResolution` | committed value |
     | `moduleDetection` | committed value-or-default |
     | `paths` | committed value (corpus-rooted) |
     | `typeRoots` | committed value (corpus-rooted) |
     | `exactOptionalPropertyTypes` | committed value-or-default |
     | `noUncheckedIndexedAccess` | committed value-or-default |
     | `useDefineForClassFields` | committed value-or-default |
     | `jsx` | committed value-or-default |
     | `jsxImportSource` | committed value-or-default |
     | `verbatimModuleSyntax` | committed value-or-default |
     | `noErrorTruncation` | committed value (pinned `true` — fences the `…` truncation backstop) |

  4. **Canonicalize** the resulting map under the §Q1 canonical-JSON rules (lexicographic
     key order, normalized value spellings — `lib` entries lowercased + sorted, boolean
     literals, no whitespace) and **hash once** (SHA-256, the `sha256:` content-digest
     family).

  **The source of truth for `compiler_options_hash` is two COMMITTED ARTIFACTS — not
  prose.** Two implementers derive the SAME hash because both READ the same committed
  bytes, never reconstruct a map from this document:
  1. **The committed `oracle.tsconfig.json`** (a vendored corpus member,
     content-hashed into `oracle_env_hash`) supplies every key it sets explicitly.
  2. **The committed version-pinned DEFAULTS TABLE** supplies the default for each closed
     effective-option key the tsconfig leaves unset. The default VALUES are an EXTERNAL
     FACT — they are whatever tsgo@`tsgo_version` applies for that key — CAPTURED ONCE at
     the pinned version and committed as a checked-in table beside the registry (the same
     channel as `CURRENT_ENV_CORPUS_ID` / `tsgo_version`). The effective map is computed
     ONCE at the pinned `tsgo_version` from `oracle.tsconfig.json` ∪ this committed
     defaults table, and that computed map is the `compiler_options_hash` source. An
     implementer reads the committed tsconfig + the committed defaults table and recomputes
     the same hash; they do NOT re-derive any default value from this prose or from a live
     tsgo invocation.
  The defaults table is pinned to `tsgo_version`: a tsgo upgrade that changes any default
  RE-CAPTURES the table under the new `tsgo_version` and recomputes the hash (consistent
  with the option-delivery spike, which proves tsgo actually APPLIED each pinned value).
  **Every key in the effective map
  is OWNED and the SET is CLOSED** — the `compiler_options_hash_is_closed` guard asserts
  `committed_tsconfig.compilerOptions.keys ⊆ closed_effective_option_keys`: the committed
  `oracle.tsconfig.json`'s OWN `compilerOptions` keys must ALL be members of the closed
  effective-option key table above (a key PRESENT IN the committed tsconfig but OUTSIDE the
  closed table FAILS — tsgo reads that committed tsconfig from the corpus, so an un-hashed /
  un-owned key in it would let tsgo apply an option the hash does not cover, un-pinning the
  env). Equivalently the generator may supply a SANITIZED effective config containing
  exactly the closed map. Either way an unowned option — whether injected via the recipe OR
  smuggled in through the committed tsconfig itself — cannot silently enter the oracle
  config AND the recipe cannot silently widen via an open-ended clause. The
  generator MUST SEND exactly this closed effective-option map to tsgo: the in-repo LSP
  `initialize` sends only `processId` / `capabilities` / `rootUri` / `workspaceFolders`
  (`ipc.rs:1111`) and the `workspace/didChangeConfiguration` paths-config sends only
  `paths` (no `baseUrl` — tsgo 7.0 rejects it, `ipc.rs:1303`), so the generator extends
  the init/config it sends with the pinned `compilerOptions` (the exact concrete payload
  shape is a BLOCKING spike item, §4). Relying on tsgo's defaults instead would let a
  default drift silently change tsgo answers without invalidating snapshots — which is
  why the defaults are pinned to `tsgo_version` and the set is closed.
- **Proving tsgo actually APPLIED the oracle options (NOT just hashed them) — a
  MULTI-OPTION delivery-proof MATRIX, not a single fixture.**
  `compiler_options_hash_is_closed` only checks the hash RECIPE — it cannot prove the
  LSP session tsgo ran actually USED the options the generator sent. A SINGLE
  `strictNullChecks`-sensitive fixture proves only that ONE flag was delivered; a flag
  the closed effective-option map pins but tsgo silently dropped (e.g.
  `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`) would go unproven. The
  generator therefore runs a DELIVERY-PROOF MATRIX: for EACH option whose effective
  value DIFFERS from the tsgo default and that AFFECTS a printed type, a DISCRIMINATING
  fixture whose TS7 hover answer differs under the oracle value vs the default, and
  asserts tsgo returned the ORACLE-value answer. The matrix covers at minimum the
  print-affecting options the closed effective-option map pins away from the default —
  e.g. `strictNullChecks` (strict `T | undefined` vs `T`),
  `exactOptionalPropertyTypes` (an optional property's `?` vs `| undefined` surface),
  `noUncheckedIndexedAccess` (an index access's `T | undefined` vs `T`) — each as its own
  fixture + assertion. A default answer on ANY matrix row means that option was not
  delivered and FAILS generation. This proves the option-send path delivered EVERY
  print-affecting option, not just one. Pinned by the `oracle_options_delivery_proven`
  guard (the matrix form).
- **`oracle_env_hash` — the CLOSED VENDORED-corpus pin (NOT just compiler options),
  validated on the VALUE; `env_corpus_id` is the STABLE corpus identity in
  `snapshot_id`.**
  A `compiler_options_hash` match is necessary but NOT sufficient: a TS7 hover answer
  also depends on the ambient / lib / package `.d.ts` corpus AND the resolution
  metadata (package manifests, tsconfig/project metadata) the query resolves
  through (fact 8 — Verter's own model splits this across `lib_env_hash` +
  `project_identity`, `env_hash.rs:84,99,219,239`; tsgo consults `@verter/types` under
  `node_modules`, `ipc.rs:3651`; a `package.json` `types`/`exports` change re-selects a
  different `.d.ts`, `ipc.rs:3686`, `cache_invalidation.rs:324`). The generator drives
  tsgo against the CLOSED, VENDORED corpus (above) — never live `node_modules` — and
  records the FULL vendored SHARED file set: every vendored ambient / lib / package
  `.d.ts` plus the resolution manifests/project metadata, with a DIRECTORY MANIFEST
  (the complete listing). This is the SHARED ambient/lib/package/tsconfig corpus ONLY —
  per-row workspace files are NOT part of it; they are a distinct domain in
  `identity.workspace_files` and enter `snapshot_id` through that axis. It content-hashes
  that vendored shared set into `oracle_env_hash` (spanning the resolve / type / lib /
  project dims) and computes the STABLE `env_corpus_id` over the whole vendored shared
  corpus. `oracle_env_hash` does NOT
  enter `snapshot_id` (that would be circular, §Q4): it is STORED in the snapshot and
  validated as a VALUE on read — the consumption test and `no_orphan_snapshot`
  RE-ENUMERATE the vendored corpus directory, assert SET-EQUALITY against the stored
  manifest (catching a newly-ADDED file as well as an edit/delete), then recompute the
  hash from the stored `oracle_env_files` against current on-disk content and FAIL on
  mismatch — so any ambient / lib / package / manifest / project file change (edit,
  add, or delete) invalidates the snapshot even when the compiler options are
  unchanged. The stable `env_corpus_id` (the closed-corpus content id) DOES enter
  `snapshot_id` as a pinned-env constant (like `compiler_options_hash`), keeping the
  filename registry-derivable. The `oracle_env_hash_pins_resolved_file_set` and
  `oracle_env_corpus_is_closed` guards pin this.
- **Corpus COMPLETENESS vs corpus STABILITY — two distinct properties, two distinct
  mechanisms.** The set-equality re-enumeration above proves only **STABILITY**: that
  the vendored directory's CURRENT listing still set-equals the stored manifest (no
  file ADDED, edited, or removed since capture). It does NOT, on its own, prove
  **COMPLETENESS**: that EVERY file tsgo actually consulted to produce the hover was
  vendored in the first place. A naïve generator that vendored only the files it
  THOUGHT were consulted, while tsgo silently fell back to a live `node_modules` lib or
  an un-vendored ambient `.d.ts`, would produce a set-equality-clean-but-INCOMPLETE
  corpus: the snapshot would re-validate offline yet depend on a file no guard tracks.
  Completeness is therefore enforced at GENERATION time by a structural mechanism, not
  by an enumeration of what "should" be present:
  - **The generator drives tsgo against ONLY the frozen vendored corpus root — there is
    NO live `node_modules`, no ambient fallback path, no second resolution root, and tsgo
    is forced OFF its native-bundled libs.** The generator points tsgo's `rootUri` /
    `workspaceFolders` / module-resolution root at `oracle_env/<env_corpus_id>/` and runs
    it in an environment where the live `node_modules` (and any developer-machine ambient
    root) is NOT on any resolution path tsgo can reach.
  - **The CONCRETE vendored-lib mechanism (the named candidate, not an open question).**
    The hard part is forcing tsgo to use the VENDORED lib files rather than the lib
    `.d.ts` set it bundles natively under `node_modules/@typescript/native-preview-*/lib/`
    (`ipc.rs:~2859-2874`). The NAMED candidate the spike must validate: the canonical
    `oracle.tsconfig.json` sets **`"noLib": true`** (which suppresses tsgo's automatic
    bundled-lib inclusion) PLUS an EXPLICIT vendored lib FILE LIST — the exact
    `lib.es2020.d.ts` + dependency-closure `.d.ts` set, copied as corpus members under
    `oracle_env/<env_corpus_id>/lib/` and referenced by a triple-slash
    `/// <reference lib="…" />` set or an explicit `files`/`include` listing rooted at the
    corpus lib dir — so the ONLY libs tsgo sees are the vendored ones. The ALTERNATIVE
    candidate (if `noLib` + explicit list proves unworkable at the pinned tsgo) is to keep
    tsgo's `lib` enabled but redirect lib + type resolution into the corpus via
    `"lib": [...]` + corpus-rooted `"typeRoots"` / `"paths"` and a working-directory /
    resolution-root pin to the corpus that makes the bundled-lib dir unreachable. Either
    candidate is delivered via the LSP **init / config `compilerOptions` payload** (the
    same channel `oracle_options_delivery_proven` requires for the rest of the effective
    options) — `initializationOptions`, a `workspace/didChangeConfiguration` block, or a
    tsconfig tsgo reads from the corpus root. The EXACT accepted wire-payload shape that
    makes tsgo honour the vendored libs at `7.0.0-dev.20260526.1` is a BLOCKING spike item
    (§4) — but the spike validates the NAMED `noLib`+vendored-list candidate above, it does
    not open-endedly "find a way".
  - **Deferred-row fallback if tsgo cannot be forced off its bundled libs.** If the spike
    finds NEITHER candidate forces tsgo off its native-bundled libs at the pinned version
    (the corpus cannot be made the SOLE lib source), then the lib corpus is not hermetic
    and EVERY lib-dependent row stays a DEFERRED class (`Ignored`) — the harness does NOT
    fall back to driving tsgo against its bundled libs (that would be a non-hermetic,
    non-re-enumerable env no guard could pin). Only rows whose answer depends on NO lib
    surface would remain admissible in that fallback world. The spike must report which of
    the two candidates works (and the exact payload), or declare the lib-dependent class
    deferred.
  - **Vendoring the bundled libs into the corpus.** Under the validated candidate, the
    generator copies the vendored lib `.d.ts` set (the bytes of `lib.es2020.d.ts` + its
    dependency closure, normally under `node_modules/@typescript/native-preview-*/lib/`,
    `ipc.rs:~2859-2874`) INTO the corpus, vendors the canonical `oracle.tsconfig.json`
    with the `noLib`+explicit-list (or `lib`+corpus-rooted-`typeRoots`/`paths`) config, and
    launches tsgo with its working directory / resolution root pinned to the corpus so
    node-resolution cannot escape it.
  - **Consequence: any un-vendored resolution-affecting file becomes a GENERATION-TIME
    FAILURE, not a silent dependency.** If the symbol the probe resolves needs a file
    that was NOT vendored, tsgo — driven against the closed corpus only — CANNOT resolve
    it: the hover surfaces as a resolution failure (an `any`/`error`/`Unknown`-shaped
    answer, a missing-module diagnostic, or an unresolved `Ref`), which the existing
    backstop (`any`/`Unknown` reject) and the probe-binding check
    (`probe_binds_to_registry_target`: zero-NEW-diagnostics on the probe alias) BOTH
    catch. The generator additionally runs a **zero-NEW-diagnostics gate over the WHOLE
    vendored project** for the probe synthesis: the probe alias must introduce zero new
    diagnostics AND the project must carry no unresolved-module / missing-lib diagnostic
    introduced by the corpus being incomplete. A missing vendored file therefore FAILS
    GENERATION — the snapshot is never written — rather than producing a clean-looking
    answer that secretly leaned on an un-tracked file. This is the COMPLETENESS
    mechanism: completeness is proven by tsgo's OWN inability to resolve against an
    incomplete frozen root, surfaced as a generation-time diagnostic, distinct from the
    on-read STABILITY set-equality. Pinned by the `oracle_env_corpus_is_complete` guard;
    `oracle_env_corpus_is_closed` continues to pin STABILITY (the on-read set-equality
    re-enumeration).

#### Confluence is the soundness property (idempotence is necessary but INSUFFICIENT)

The parity comparison is sound ONLY if the normalizer is **CONFLUENT over the
admissible set**: for any two admissible `TypeExpr` inputs `a`, `b` that denote the
SAME TS type (`a ≡ b`), the normalizer must produce BYTE-EQUAL canonical forms —
`a ≡ b ⟹ normalize(a) == normalize(b)`. This is the real obligation, because the two
sides of every comparison are spelled DIFFERENTLY by construction: the Verter side is
Verter's own `TypeExpr` projection and the oracle side is the `TypeExpr` lowered from
tsgo's hover, and tsgo's display normalization spells a type differently from Verter's
projection (it collapses `true|false→boolean` co-present-arm-insensitively, may print
`string` for an absorbed `string|"a"`, drops parentheses, reorders unordered members,
etc.). If two equal-but-differently-spelled inputs normalize to DIFFERENT byte streams,
the strict-equality gate manufactures a FALSE DIVERGENCE; if two UNEQUAL inputs ever
normalize to the same form, it manufactures FALSE PARITY.

**Idempotence (`normalize(normalize(x)) == normalize(x)`) is NECESSARY but NOT
SUFFICIENT.** Idempotence only proves the normalizer is a fixed-point on its OWN output
(one side, one spelling); it says NOTHING about whether two DIFFERENTLY-spelled equal
inputs converge. A rule can be perfectly idempotent yet non-confluent — the original
exact-two-arm boolean rule was: `normalize(boolean|X) == boolean|X` and
`normalize(true|false|X) == true|false|X` are both idempotent, yet
`boolean|X ≠ true|false|X` byte-wise even though `boolean|X ≡ true|false|X`. The
soundness gate is therefore confluence, asserted directly by feeding two
differently-spelled equal inputs through the SAME pipeline and requiring byte-equality
(the `oracle_normalization_is_confluent` guard), DISTINCT from the existing
`oracle_normalization_discriminates` guard (which only mutates ONE side to prove the
reduction catches real divergence) and `oracle_normalization_is_idempotent` (which
re-runs ONE spelling). Idempotence + discrimination + confluence together are the
complete soundness obligation.

**The CLOSED set of neutral-element / absorption / canonicalization forms the
normalizer collapses — and the DEFAULT-SAFE confluence posture.** Confluence is proven
construct-by-construct, so the set of equal-spelling rewrites the normalizer applies is
a CLOSED ENUMERATED list, each member of which is PROVEN semantics-preserving AND
PROVEN to converge both spellings:

- **`X | never → X`** (the bottom element is the union identity).
- **`X & unknown → X`** (the top element is the intersection identity).
- **`X | unknown → unknown`, `X & never → never`** (absorbing elements per TS set
  semantics).
- **dedup** — exact duplicate arms in a `Union` / `Intersection` collapse to one.
- **sort** — `Union` / `Intersection` arms are sorted by the recursively-normalized
  structural key (the unordered-set canonicalization).
- **flatten** — nested same-kind `Union`-in-`Union` / `Intersection`-in-`Intersection`
  is flattened (associativity).
- **boolean** — `{true, false} ⊆ arms ⟹ {boolean} ∪ (arms ∖ {true,false})`
  (co-presence form, confluent over the ≥3-arm case, step 5).
- **bounded literal subsumption** — a literal arm absorbed by a CO-PRESENT base
  primitive collapses (`string|"a"→string`), strictly bounded (step 5).
- **literal-value spelling canonicalization** — numeric/bigint literal spellings that
  denote the SAME literal type are canonicalized to one decimal form (`1`/`1.0`/`0x1`),
  closing the literal-ONLY-union false divergence subsumption does not reach (step 5).
- **parenthesis strip** (step 1), **span/`SyntheticSlotBinding` strip** (step 7),
  **cosmetic-name → positional placeholder** (step 6), **alias display per mode**
  (step 4).

**DEFAULT-SAFE confluence posture (symmetric to the admission allowlist's
default-REJECT).** Any ADMITTED construct whose two-sided spelling is NOT on this closed
list AND whose two spellings are not PROVEN to converge under the enumerated rules is
**REJECTED / deferred** — the `(row, query)` stays `Ignored` rather than risk a
false-divergence from an un-proven spelling difference. The normalizer never
"best-effort passes through" a construct whose confluence is unproven. So every
admissible construct is EITHER (a) spelled identically on both sides by construction
(no rewrite needed — e.g. a `Primitive`, a static-keyed `Property`), OR (b) canonicalized
by an enumerated, proven-confluent rule above, OR (c) rejected. This makes confluence
default-safe by construction: a missed equal-spelling axis cannot become a silent false
divergence — it rejects. The confluence-validation spike (§4) discharges the proof
obligation over the admissible corpus before any class is admitted.

**The pipeline is run to a FIXPOINT over a CONFLUENT, TERMINATING rewrite system —
single-pass is UNSOUND because step-5 reductions re-expose step-2 obligations.** The
enumerated rules above are not applied as a single ordered pass: a step-5 reduction can
produce a term that re-satisfies an earlier step's precondition. Concretely
`true | false | boolean` runs the step-5 boolean co-presence rule to
`boolean | boolean`, which is NOT re-deduped if step 2 already ran — a single pass would
leave `boolean | boolean` on one side and `boolean` on the other, a FALSE DIVERGENCE.
The normalizer is therefore defined as the **least fixpoint of the rewrite relation**:
the full enumerated rule set (flatten, dedup, sort, neutral/absorbing, boolean
co-presence, bounded literal subsumption, cosmetic-name rename, parenthesis/span strip,
alias display) is applied repeatedly until no rule fires (a pass that produces a
byte-identical term). Equivalently — and this is the implementable form — each pass ends
with an explicit **post-step-5 re-canonicalization** (re-flatten, re-dedup, re-sort,
1-element-union/intersection collapse) so a reduction that re-exposes an earlier
obligation is caught within the same fixpoint iteration.

This is sound because the rewrite system is **locally confluent AND terminating, so by
Newman's lemma it is GLOBALLY confluent** — every term has a unique normal form
regardless of rule-application order:

- **Termination (the rewrite relation is well-founded).** Each rewrite STRICTLY DECREASES
  a fixed lexicographic measure `(node_count, union_intersection_arm_count,
  non_canonical_name_count)` and NO rule increases any earlier component: flatten/dedup/
  neutral/absorbing/literal-subsumption/boolean-co-presence all strictly reduce
  `node_count` or `arm_count` (they remove arms/nodes; the boolean rule replaces the
  `{true,false}` pair with one `boolean`, a net arm decrease); cosmetic-name rename
  strictly reduces `non_canonical_name_count` and touches neither earlier component. **The
  SORT step is the one rewrite that does NOT decrease the measure (it permutes arms without
  changing `node_count` / `arm_count` / `non_canonical_name_count`), so it is handled
  explicitly and EXCLUDED from the fixpoint "did-a-rule-fire" predicate.** Sort is proven
  SEPARATELY-IDEMPOTENT — it is a total order over the cosmetic-name-neutralized structural
  key (§Q2 "the sort key is computed on the cosmetic-name-neutralized projection"), so
  applying it to an already-sorted arm multiset yields a byte-identical term; a second sort
  NEVER reorders. The fixpoint loop's termination predicate ("a pass produced a
  byte-identical term") therefore counts only the measure-decreasing rules; sort is run as a
  deterministic finalize WITHIN each pass and, being idempotent, can never be the rule that
  keeps the loop alive. Equivalently, one may fold sortedness into the measure as a 4th
  descending-`(remaining-inversions)` component that sort strictly reduces and no other rule
  increases — the two framings are interchangeable, and this doc adopts the
  separately-idempotent-and-excluded framing so a settled sort is never mistaken for a
  perpetually-firing rule. **Literal-value spelling canonicalization (step 5; e.g.
  `0x1 → 1`) is the SECOND measure-non-decreasing rewrite and gets the EXACT same
  treatment as sort.** It rewrites a leaf's literal spelling without removing a node/arm
  or changing a `non_canonical_name_count`, so it does NOT decrease the measure; it is
  therefore likewise EXCLUDED from the fixpoint "did-a-rule-fire" predicate and proven
  SEPARATELY-IDEMPOTENT — one application maps a `LiteralValue` to its canonical spelling
  (numeric → canonical decimal with radix/exponent/redundant-fraction neutralized; bigint
  → canonical decimal with the `n` suffix; string → decoded value), and re-application of
  canonicalization to an already-canonical spelling is a NO-OP (a second pass NEVER
  rewrites the spelling). Like sort it is run as a deterministic finalize WITHIN each pass
  and, being idempotent, can never be the rule that keeps the loop alive; equivalently it
  could be folded into the measure as a descending `non_canonical_literal_spellings`
  component, but this doc adopts the separately-idempotent-and-excluded framing to mirror
  sort. The measure (the first three components) is a finite product
  order over non-negative integers, so no infinite rewrite chain exists — the fixpoint is
  reached in finitely many passes. (Termination over a CYCLIC input `TypeExpr` is treated
  separately in step 0 of the reduction — a `RecursiveRef` is an opaque leaf — so the
  node count is finite; see "Normalizer termination over arbitrary `TypeExpr`" below.)
- **Local confluence (one-step diverging rewrites re-converge).** The critical pairs are
  the rule overlaps: dedup-vs-sort (commute — sorting then deduping equals deduping then
  sorting over a multiset), boolean-co-presence-vs-dedup (the boolean rule's output
  `boolean | boolean | …` re-converges under dedup, which the fixpoint re-applies),
  literal-subsumption-vs-boolean (disjoint preconditions — different base types — so they
  do not overlap destructively), neutral/absorbing-vs-flatten (flatten exposes the
  arm a neutral rule then removes; either order reaches the same set),
  literal-spelling-vs-dedup (`1 | 0x1` — dedup is step 2 and runs BEFORE spelling-canon at
  step 5, so the differently-spelled duplicate is NOT collapsed by the first dedup; it is
  canonicalized to `1 | 1` at step 5 and then collapsed by the fixpoint's post-step-5
  re-dedup — either order of the two rules reaches `1`), and literal-spelling-vs-sort
  (`0x2 | 0x1` vs `1 | 2` — the sort key is computed PRE-canon at step 2 so the two sides
  can leave step 2 in different arm orders; the post-step-5 spelling-canon makes the keys
  equal and the post-step-5 re-sort — which RECOMPUTES the structural sort key over the
  now-canonicalized literal spellings, NOT the cached step-2 sort key — reconciles arm
  order, so both converge to the same sorted canonical term). The confluence
  spike (§4) exhibits each critical pair re-converging.

**The SORT key is computed on the COSMETIC-NAME-NEUTRALIZED projection — so sort (steps
2/3) and rename (step 6) are NOT circular.** A naïve coupling would make the step-2/3
unordered-set sort key depend on the JSON that step-6's cosmetic rename later rewrites
(member/binder/parameter names), so the sort order would change after the rename — a
circular dependency producing two different canonical orders on the two sides. The
resolution: the sort key is computed on the **cosmetic-name-NEUTRALIZED projection** of
each arm/member — the recursively-normalized JSON with every cosmetic name axis (index-
signature parameter name, function/method parameter names, generic type-parameter +
`infer` names, mapped-key binder) ALREADY replaced by its positional placeholder for the
purpose of the key. Because type-parameter binders are positional (`T0,T1,…`) and bound
to STRUCTURE (not the raw identifier, §step 6) and value-parameter / index names are
positional/intrinsic, the sort key NEVER contains a cosmetic name, so the rename cannot
change it. Unordered-set MEMBERS whose only difference is a cosmetic name sort to a
deterministic order by their neutralized key, and the rename is then applied by post-sort
POSITION over that fixed order — one canonical order results on BOTH sides. Sort and
rename are thus sequenced as: compute the neutralized sort key → sort → apply the
positional rename, never the reverse, so they are not mutually dependent.

#### The normalization reduction

Both Verter's `TypeExpr` and the oracle `TypeExpr` are reduced to a canonical
**normal form** before structural equality. The algorithm below is **confluent over the
admissible set** (the property above), not merely idempotent. It is applied recursively
AND run to a FIXPOINT: the numbered rules are repeated (each pass re-running the
post-step-5 re-canonicalization — re-flatten / re-dedup / re-sort / 1-element-union-
intersection collapse) until a pass produces a byte-identical term. Single-pass is
unsound (a step-5 reduction can re-expose a step-2 obligation, e.g.
`true | false | boolean → boolean | boolean`); the fixpoint + Newman's-lemma argument
(local confluence + termination ⟹ global confluence, §Q2 "Confluence is the soundness
property") gives every admissible term a UNIQUE normal form regardless of rule order. The
sort key in steps 2/3 is computed on the cosmetic-name-NEUTRALIZED projection (so sort
and the step-6 rename are not circular, §Q2). Termination over a possibly-CYCLIC input is
guaranteed by step 0:

0. **Bound the walk over cyclic input (`RecursiveRef` is an opaque leaf).** A Verter
   `TypeExpr` may carry a `RecursiveRef` back-edge (a first-class variant), so the
   recursive normalization is NOT structurally guaranteed to terminate on its own. The
   normalizer treats a `RecursiveRef` as an OPAQUE LEAF — it is canonicalized to a stable
   `RecursiveRef` token and NEVER followed/expanded — and additionally maintains a
   VISITED-SET of node identities so re-entering an already-visited node folds to that
   opaque leaf rather than recursing. This makes the walked term FINITE (a DAG over a
   finite node set), so the termination measure in §Q2 is well-founded and the fixpoint
   is reached in finitely many passes. The hover-lowered oracle side cannot carry a
   back-edge (hover text is finite), so this guard is a Verter-side belt-and-suspenders
   symmetric to the source-side `source_walk_cycle_rejected`. Pinned by
   `oracle_normalizer_terminates_on_cyclic_input`.

1. **Strip `Parenthesized`** — unwrap to the inner type (it is transparent to
   evaluation).
2. **Flatten + canonicalize unordered sets.** `TypeExpr::union` /
   `TypeExpr::intersection` do NOT flatten nested same-kind arms (they only collapse
   the 0- and 1-element cases, `lib.rs:1072,1081`), so the normalizer must:
   (a) FLATTEN nested `Union`-in-`Union` / `Intersection`-in-`Intersection`;
   (b) sort arms by a stable structural key (the recursively-normalized JSON of each
   arm); (c) DEDUP exact duplicates; (d) reduce neutral / absorbing elements
   (`X | never → X`, `X & unknown → X`, an absorbing `any`/`unknown` per TS set
   semantics). Tuples, function parameters, and template-literal quasis/expressions
   are ORDERED and are NOT flattened or sorted.
3. **Canonicalize object members — preserving order where order is semantic.** Sort
   only the GENUINELY-UNORDERED members (`property`, `indexSignature`) by a **TOTAL
   structural key** — the recursively-normalized JSON of the WHOLE member, not just
   `(member-kind, name)`. A `(member-kind, name)` key alone is not a total order: two
   index signatures (`[k: string]: A` and `[k: number]: B`) share the synthetic key
   name, and a degenerate surface can carry duplicate property names; sorting by the
   full normalized member JSON gives a deterministic, total order in those cases.
   KEEP `callSignature` / `constructSignature` / overload `method` groups in SOURCE
   ORDER — overload order is semantic and must NOT be sorted. (Verter's component-meta
   props map is presented sorted — `basic.rs:24` asserts a sorted prop-name vector via
   the `object_props`/`prop_names` helpers — but that is the props-map order, not raw
   `TypeExpr::Object` member order; the normalizer sorts both sides' unordered members
   anyway, so the comparison is order-insensitive for those and order-sensitive for
   overloads.) Member `optional` and `readonly` flags
   are PRESERVED and compared (catching wrong optionality — a real divergence).
4. **Normalize alias display vs structure per query mode.** The snapshot's
   `identity.projection_mode` governs:
   - `Shallow` / `Navigate`: a `Ref { name }` stays a `Ref { name }` on BOTH sides —
     the comparison is on the shallow surface; alias bodies are NOT expanded (aligns
     with the Component-Meta Shallow-By-Default Rule).
   - `Expanded`: alias `Ref`s to userland aliases are expected expanded to their
     structural body on both sides; the generator's forced-expansion probe captures
     the expanded form. A residual `Ref` to a package/builtin name (e.g. `Promise`,
     `Array`) is normalized to a canonical `Ref { name, type_arguments }` (NOT
     expanded) on both sides.
   - `Skeleton`: unbound type parameters are `TypeParameter`/`Infer` shells on both
     sides (preserving Conditional branches) — never collapsed to `never`.
5. **Normalize primitive spellings + literal-value SPELLING + bounded literal
   subsumption.** First, **literal-value spelling canonicalization** (a separate axis from
   subsumption, and the one false-divergence the subsumption rule does NOT cover): a
   `LiteralValue` numeric / bigint literal is canonicalized to ONE spelling regardless of
   how either side printed it — `1`, `1.0`, `0x1`, `1e0` are the SAME TS literal type but
   byte-differ in the lowered `TypeExpr`, and a literal-ONLY union (`1 | 2` vs
   `0x1 | 0x2`) is the same set yet byte-differs (the bounded subsumption rule below does
   NOT fire — there is no co-present base primitive — and the cosmetic-name default-reject
   covers BINDER names, not LITERAL spellings, so without this rule the strict-equality
   gate FALSE-DIVERGES). The rule: numeric literals normalize to a canonical DECIMAL form
   (radix `0x`/`0o`/`0b` → decimal, redundant `.0` fraction dropped, exponent expanded);
   bigint literals to a canonical decimal carrying the `n` suffix; string literals are
   compared by their DECODED value (quoting / escaping neutralized), never by source
   spelling. This neutralizes SPELLING, never VALUE — `1.5` and `1` remain DISTINCT
   literals and still DIVERGE. A literal-spelling axis NOT covered here is
   DEFAULT-REJECTED (the default-safe posture), never passed through with raw spelling.
   Pinned by `oracle_literal_spelling_canonicalized`. Second, primitive boxing is a real
   difference: `String` (boxed object type) vs `string` (primitive) is NOT collapsed; but
   `true | false` ⇄ `boolean` IS collapsed (TS itself treats `boolean = true | false`). The boolean-collapse rule is stated CO-PRESENCE-WISE, NOT
   as an exact-two-arm-set match, so it is CONFLUENT regardless of co-present arms: a
   `Union` whose normalized arm set CONTAINS both `Literal(true)` and `Literal(false)`
   reduces to `{ Primitive(boolean) } ∪ (arms ∖ { Literal(true), Literal(false) })` —
   i.e. `{true, false} ⊆ arms` ALWAYS collapses the `true`/`false` pair to `boolean`
   and KEEPS every other arm. This is unconditionally semantics-preserving (`true | false
   ≡ boolean` by TS set semantics, so substituting `boolean` for the `{true,false}`
   subset changes no type) and covers the ≥3-arm case: `true | false | X` (Verter) and
   `boolean | X` (tsgo display, which collapses `true|false→boolean` regardless of
   co-present arms) BOTH normalize to `{ Primitive(boolean), X }` and COMPARE EQUAL. An
   exact-two-arm-set rule would FALSE-DIVERGE on that ≥3-arm case (tsgo prints
   `boolean|X`, Verter carries `true|false|X`, and the exact rule fires on neither),
   which is precisely the confluence defect the co-presence form repairs. Beyond
   boolean, a **BOUNDED, CLOSED literal-subsumption rule** applies inside a union:
   collapse a literal arm that is absorbed by a CO-PRESENT primitive of its own base
   type — `string | "a"` → `string`, `number | 1` → `number`, `boolean | true` →
   `boolean`. The rule is bounded: it fires ONLY when the absorbing primitive is
   co-present in the SAME union and is exactly the literal's base type; it never widens a
   lone literal, never crosses base types, and never touches intersections. Anything
   outside this bounded rule is **NON-ADMISSIBLE** — the strict-equality gate would
   false-diverge on an un-normalized subsumption, so such a query is DEFERRED (not
   silently failed). The subsumption + boolean-absorption rule is deterministic,
   idempotent, AND confluent over the admissible set (§Q2 "Confluence is the soundness
   property").

   **Why the rule is semantics-preserving, and why it is NEEDED (justification against
   tsgo behavior).** The rule fires ONLY when the absorbing primitive is CO-PRESENT, and
   `"a" | string` IS, by TS set semantics, exactly `string` — so collapsing the absorbed
   literal removes a redundant arm without changing the type. It is needed because the
   TWO sides can spell the same set differently: TS's own display normalization
   sometimes PRINTS `string | "a"` (retaining the literal for documentation) and
   sometimes prints the absorbed `string`, and Verter's projection may carry the
   un-absorbed `Union`; without the rule the strict-equality gate would FALSE-DIVERGE on
   a purely cosmetic display difference over a semantically identical set. The rule is
   STRICTLY BOUNDED so it cannot mask a real difference: it requires the absorbing
   primitive to be PRESENT (it never widens a lone literal `"a"` to `string`), so a
   genuine `"a"` vs `string` difference (different sets) still DIVERGES — the rule only
   removes an arm the co-present primitive already subsumes. It is NOT extended to
   collapse a literal-only union (`"a" | "b"` stays `"a" | "b"` — that is a distinct,
   narrower set than `string`). The `oracle_literal_subsumption_discriminates` guard
   pins this with a DISCRIMINATING case where the subsumption input DIFFERS between the
   two sides and the compare STILL diverges: side A = `"a" | string` (normalizes to
   `string`), side B = `"b" | string` (also normalizes to `string`) compare EQUAL (both
   are `string` — correct); but side A = `"a" | string` (→ `string`) vs side B = `"a" |
   "b"` (a literal-only union, NOT collapsed) MUST DIVERGE (`string` ≠ `"a" | "b"`),
   proving the rule does not over-collapse a narrower literal-only set into the absorbing
   primitive. A further discriminating case: `"a" | number` (no co-present `string`)
   MUST NOT collapse the `"a"` arm (the absorbing primitive is absent), so `"a" |
   number` vs `number` DIVERGES — proving the rule fires only on the co-present base
   type. If the spike finds tsgo never emits the un-absorbed `string | "a"` form for
   any admissible row (i.e. tsgo always prints the absorbed `string`), the rule becomes
   a defensive no-op on the oracle side and still correctly absorbs an un-absorbed
   Verter projection; it is retained either way because Verter's side may carry the
   un-absorbed union.
6. **Canonicalize cosmetic, non-identity-bearing binder/parameter names to positional
   placeholders — under a DEFAULT-SAFE posture (an un-canonicalizable cosmetic
   construct REJECTS, never silently passes).** TS treats certain binder/parameter
   NAMES as cosmetic — they do NOT participate in structural type identity — but
   `TypeExpr` carries them as identity-bearing fields, so two structurally-equal TS
   types can lower to unequal `TypeExpr` JSON and produce a FALSE DIVERGENCE. The worked
   worked example is the index-signature parameter name: `{ [key: string]: T }`
   and `{ [x: string]: T }` are the SAME TS type, but `IndexSignature.key_name` differs
   (`lib.rs:711/714`, emitted at `type_expr_json.rs:482`; OXC preserves the source name
   verbatim at `oxc/lib.rs:488`) — and index-signature rows exist (manifest rows ~:138,
   fixture `index_signatures.ts`).

   **The default-safe posture (symmetric to the admission allowlist).** The admission
   allowlist (§Q2) is default-REJECT; the NORMALIZATION axis is given the SAME default
   so a MISSED cosmetic axis cannot become a silent FALSE DIVERGENCE. The set of
   cosmetic-name axes the normalizer canonicalizes is a CLOSED ENUMERATED set (the four
   bullets below). A `TypeExpr` node carrying an identity-bearing name field that the
   normalizer does NOT have an enumerated canonicalization rule for is treated as an
   **un-canonicalizable cosmetic construct and the `(row, query)` is REJECTED / deferred**
   — it is never admitted with the raw name left in place (which would risk a false
   divergence on a name TS considers cosmetic). The normalizer never "best-effort
   passes through" an unknown name-bearing construct. This makes the cosmetic axis
   default-safe by construction: a name axis is EITHER on the closed canonicalization
   list (rewritten deterministically) OR it rejects the capture.

   The closed canonicalization list, applied consistently at every binding + use site:
   - **Index-signature parameter name** (`IndexSignature.key_name`, `lib.rs:715`) → a
     fixed token `key`.
   - **Function / method / call-signature / construct-signature parameter names**
     (`FunctionParam.name`, `lib.rs:928`) → positional `p0, p1, …` in declaration
     order.
   - **Generic type-parameter names** (`TypeParam.name`, `lib.rs:1019`) → positional
     `T0, T1, …`, rewritten consistently at the binding site AND at every `Ref` /
     `TypeParameter` use site that BINDS to that binder.
   - **Mapped-type key binder + `infer` names** (`Mapped` key binder, `Infer { name }`)
     → positional placeholders on the same scheme.

   **The use-site binder-resolution algorithm (which `Ref` binds to which binder).**
   The hard subtlety is the type-parameter / `infer` rename: a `Ref { name: "T" }` use
   site may bind to a generic type-parameter `T` introduced by an enclosing
   binder — in which case it MUST be renamed to that binder's positional placeholder —
   OR to an UNRELATED same-named top-level alias `type T = …` — in which case it MUST
   be left as the alias `Ref` (renaming it would corrupt a real alias reference). The
   normalizer cannot decide this by name alone; it runs a scope-tracked binder
   resolution over the `TypeExpr` tree:
   - **Maintain a SCOPE STACK of in-scope binders.** Walking the tree top-down, each
     construct that INTRODUCES type-parameter / `infer` binders pushes a scope frame:
     a `Function` / `ConstructorType` / call-/construct-signature / `Method` pushes its
     `type_params` (and its value parameters for the `p0,p1,…` axis); a `Mapped` pushes
     its key binder; a `Conditional`'s `extends` clause pushes every `Infer { name }`
     binder declared within it (an `infer X` is in scope for the conditional's
     `true_type` branch). Each frame maps the binder's SOURCE name → its assigned
     positional placeholder (`T0,T1,…` in declaration order within the introducing
     construct; `infer` names share the type-parameter placeholder space of their
     conditional). The frame pops when the walk leaves the introducing construct.
   - **At a `Ref { name }` / `TypeParameter` use site, resolve `name` against the scope
     stack INNERMOST-first.** If `name` matches an in-scope binder frame, rewrite the
     use site to that binder's positional placeholder (shadowing is respected —
     innermost wins). If `name` matches NO in-scope binder, it is a free reference (a
     top-level alias / package `Ref` / builtin) and is left UNCHANGED — it is not a
     cosmetic binder name and must not be renamed. This is the same lexical
     binder-resolution discipline a binder pass uses; it makes the rename bind to the
     STRUCTURE (which binder a use site resolves to) rather than to the raw identifier,
     so an unrelated same-named alias is never captured.
   - Value-parameter names (`FunctionParam.name` → `p0,p1,…`) and index-signature key
     names (→ `key`) are LOCAL to their signature/member and need no cross-scope
     resolution — they are renamed positionally in place.

   **CROSS-SIDE binder-list / arity stability (the positional rename is confluent ONLY
   when both sides agree on the binder list AND its order).** Positional `T0,T1,…` /
   `p0,p1,…` assignment is "in declaration order within the construct" — but that is a
   CONFLUENT canonicalization only if the Verter side and the hover-lowered side present
   the SAME binder list in the SAME order. They can DIVERGE: tsgo's hover may REORDER
   type parameters (e.g. surface defaulted/inferred params in a different position),
   OMIT an inferred or defaulted type parameter the source declared, or COLLAPSE a
   post-instantiation binder that Verter's projection still carries — in which case
   `T0,T1,…` on one side binds to a DIFFERENT source binder than on the other, and the
   positional rename manufactures a FALSE PARITY (two different types renamed to the same
   positional shape) or a FALSE DIVERGENCE (the same type renamed differently). The
   exposure is non-empty: a `Method` / `Function` / `ConstructorType` / call-/construct-
   signature member carries `type_params`. The rule is therefore DEFAULT-REJECT on
   cross-side instability: a construct that introduces type-parameter binders is
   admissible ONLY when its binder LIST and ORDER are PROVEN cross-side-stable — the
   spike (§4) must exhibit, for the admitted construct class, that tsgo's hover presents
   the SAME ordered binder list the source declares (same arity, same order, no
   reordering / omission / post-instantiation collapse). A construct whose binder
   list/order is NOT proven cross-side-stable is REJECTED / deferred, NEVER admitted with
   a best-effort positional rename. Concretely, the initial scope admits the positional
   type-parameter rename ONLY for constructs whose binders are syntactically present and
   ordered identically on both sides (no defaulted/inferred-param reordering); generic
   constructs that risk cross-side binder divergence stay `Ignored` until the spike
   proves stability for their class (a `probe_synthesis_version` / `normalizer_version`
   bump as appropriate). Pinned by the `binder_order_is_cross_side_stable` guard with a
   DISCRIMINATING case: a construct whose hover-side binder order differs from its
   source-side order (or whose hover omits a declared binder) FAILS / is rejected, rather
   than being silently positional-renamed into false parity.

   **The template-literal cosmetic axis (default-reject until canonicalized).** A
   `TemplateLiteral { quasis, expressions }` (`lib.rs`) can present the SAME TS type
   with cosmetically-different quasi/expression spellings that the closed list above
   does NOT cover (e.g. an embedded type-parameter reference inside an expression slot,
   or a quasi/placeholder boundary tsgo renders differently from the authored source).
   There is no enumerated canonicalization rule for the template-literal cosmetic axis
   in the initial scope, so — per the default-safe posture — a `TemplateLiteral`-bearing
   `(row, query)` is **DEFAULT-REJECTED / deferred** until the spike (§4) proves and
   versions a lossless template-literal canonicalization (a `normalizer_version` bump).
   This mirrors the §Q2 ADMIT-only-in-spike-proven-modes treatment of the
   template-literal construct: it is not admitted via this harness until its cosmetic
   axis is enumerated and proven, rather than passed through with raw quasi/expression
   spellings that could false-diverge.

   This canonicalization is **purely structural** — TS ignores the enumerated names for
   type identity, so renaming them changes no meaning. It is a RENAME only: it does NOT
   collapse, drop, or reorder members and so does NOT mask a real member/type
   difference (a differently-named member with a different type still diverges on the
   type; only the cosmetic name is neutralized). It is idempotent (re-running over an
   already-canonicalized form is a no-op, since the placeholders are already
   positional), preserving the overall `normalize(normalize(x)) == normalize(x)`
   property. Combined with the default-safe posture (un-enumerated name axes and the
   template-literal axis REJECT rather than pass through), it forecloses the
   cosmetic-name false-divergence class without ASSERTING completeness it cannot
   prove: completeness is replaced by the provable property "every name axis is either
   canonicalized or rejected."
7. **Drop display-only carriers.** `Unknown { raw }` on the Verter side is a
   COMPARISON FAILURE, not normalized away (a query that produces `Unknown` has not
   lifted). `SyntheticSlotBinding` and span sidecars are stripped (spans are
   cosmetic; `TypeExpr::clear_spans` exists, `span_transform.rs:154`).
   The parity claim ASSUMES Verter's OWN projection of an admitted (allowlist-clean,
   single-contributor) source is itself within the admissible construct set; a Verter
   projection that emits a NON-admissible form (e.g. a resolver-bug `never` / bare-`Ref`
   collapse where the admissible surface was a structured type) is a Verter BUG to fix at
   the resolver, not a property of this harness — and this step-7 `Unknown { raw }`-fails-hard
   rule already guards the obvious case (a Verter-side `Unknown` fails the compare rather
   than normalizing to a false match).
8. **Compare** the two normalized JSON documents for exact structural equality. Any
   difference fails the query with a structural diff (which member, which field) —
   NOT a string diff.

Steps 0–7 are deterministic, idempotent, AND confluent over the admissible set
(§Q2 "Confluence is the soundness property"): run to a FIXPOINT (each pass re-running the
post-step-5 re-canonicalization), `normalize(normalize(x)) == normalize(x)` (idempotence,
trivial once the fixpoint is reached) AND `a ≡ b ⟹ normalize(a) == normalize(b)` over
admissible inputs (confluence — the real soundness obligation, since the two compared
sides are spelled differently by construction — established by local confluence +
termination ⟹ global confluence, Newman's lemma). The reduction CATCHES: wrong member name, missing/extra
member, wrong optionality, wrong readonly, wrong primitive, wrong union arm, wrong
nesting, overload-order divergence. It IGNORES: whitespace, unordered member/arm
ordering, parens, span offsets, `true|false`-co-present vs `boolean` (the co-presence
boolean rule, confluent over ≥3 arms), neutral/absorbing elements (`X|never→X`,
`X&unknown→X`, `X|unknown→unknown`, `X&never→never`), dedup, cosmetic binder/parameter /
type-parameter / index-signature / mapped-key / `infer` names (canonicalized to
positional placeholders), alias display where the mode is shallow. Any admissible
construct whose two spellings are not proven to converge under these enumerated rules is
REJECTED (default-safe confluence posture), not passed through. The algorithm is
versioned by `normalizer_version`; a change forces regeneration.

#### Relationship to §6.3

This design **reinterprets** §6.3 for the `TypeExpr`-projection family rather than
implementing it verbatim. §6.3 prescribes a structured `TypeInfoGraphPayload` /
`RelationPayload` / `TypeDescriptor` compare under a per-family divergence budget.
For the `TypeExpr`-projection family this harness uses **exact per-(row,query)
equality** on a normalized `TypeExpr` (a stricter gate than a budget) and sources the
structured answer from hover text lowered to `TypeExpr` (the only verified
point-query path), since the structured tsgo reply §6.3 assumes does not exist in the
LSP (fact 3). The relation / call-resolution / assignability families remain under
§6.3's structured-payload + per-family-budget model, served by a FUTURE structured
oracle (§Scope). This harness does NOT claim to honor §6.3 verbatim — it honors
§6.3's "structured, not text-compare" INTENT (the comparison is never a text compare;
text appears only at generation time inside a non-resolver tool and is retained in
`raw_capture` for audit) while tightening the gate for the `TypeExpr` family. §6.3 is
reconciled in `native-typeinfo-parity.md` with a pointer to this document.

> **Reframed by the single-spec / correction-overlay model — §6.3 budget
> reformulation.** §6.3's per-family divergence budget M measures **UNINTENDED**
> divergence (defects); a **registered** divergence (`ts-compat-two-mode-model.md` §5) is
> INTENTIONAL and is **excluded from M**. Under single-spec there is one budget run, not
> two modes: M compares `resolver(Correct)` against tsgo and counts only the UNREGISTERED
> disagreements (target **M → 0**), because a registered divergence's `resolver(Correct)`
> equals `correction.correct_value` (which differs from tsgo by construction) and is
> subtracted from M. The corrections are confirmed separately by the per-query data
> comparison (`resolver(query) == correction.correct_value`), not budgeted. This separates "did we
> accidentally diverge" (unregistered resolver-vs-tsgo, budgeted) from "did we
> intentionally diverge correctly" (resolver-vs-correction, registry-gated). For THIS
> exact-equality harness the same applies per (row, query): a no-correction query asserts
> `resolver(query) == snapshot`, a corrected query asserts `resolver(query) == correction`
> while that query's snapshot holds the recorded tsgo value.

This is consistent with the Typed-IR-Only Resolver Rule: the parsing happens in a
build/test generator OUTSIDE the resolver / projector / registry / policy /
materialiser / compat pipeline (and `verter_session` cannot even depend on tsgo,
fact 4); the lowering is the SAME mechanism the producer uses for real source, so it
is not a second resolver; and tests compare normalized `TypeExpr` to normalized
`TypeExpr`, never hover text.

### Q3 — tsgo driver: LSP (DECIDED)

**Decision: the LSP driver is ADOPTED.** The snapshot GENERATOR reuses the existing
`TsgoTypeProvider::spawn` (`ipc.rs:978,1006`) + `get_hover` (`ipc.rs:1587`) machinery
(`tsgo --lsp --stdio`, `ipc.rs:1020-1022`) to issue `textDocument/hover`
(`ipc.rs:1611`) at the synthesized probe's known offset. It already exists, is
exercised by the LSP tests, handles framing / priority-lanes / crash-restart, and
gives the per-symbol point-query granularity each `(row, query)` needs — exactly
matching the probe-driven generation in §Q2. The adopted `get_hover` path initializes
tsgo with EMPTY client capabilities (`capabilities: {}`, `ipc.rs:~1113`), so tsgo
returns a BARE PLAINTEXT hover — the reduced `type __oracle_probe__N = <RHS>` text with
NO markdown fence — and the oracle parses that bare hover text via the §Q2
extraction-grammar plaintext branch. Its only output is hover TEXT (fact 3), which feeds
the §Q2 pre-lowering admission + generation-time lowering, and it discards the hover
range (fact 3, `ipc.rs:1689`) — handled by the probe-header fence. The pinned
capabilities (and thus the bare-plaintext content shape) fold into
`probe_synthesis_version`; a future markdown-caps driver would produce the fenced shape
under a version bump (§Q2 "Hover-driver config", "Hover-extraction grammar").

A CLI / compiler-API driver (`pnpm exec tsgo …`, a tsserver `quickinfo`, or
`checker.getTypeAtLocation` from a Node harness) is the only route to a genuinely
structured tsgo value, but no such harness exists in-tree, and tsgo's compiler-API
surface at the pinned preview version is less battle-tested — so it is reserved for
the FUTURE structured oracle the out-of-scope verdict families need (§Scope). The
snapshot SCHEMA (Q1), the registry (Q4), and the normalization reduction (Q2) are
independent of the driver: a future structured oracle slots in via a new
`oracle_value_kind` without touching them.

### Q4 — Row → query → snapshot join: the oracle-query-spec registry (DECIDED)

The manifest is a CLOSED ROW LEDGER, not an executable query contract. `IgnoredTestRow`
carries no query payload (fact 5), the actual query payloads live today in the test
body, and **one row issues N queries** (fact 5). A guard cannot prove body-issued query
coverage while the bodies stay hand-authored, and a snapshot-only "source of truth" is
circular — it can find orphan row-refs but cannot know that query ordinal 3 of 4 is
MISSING. Extending `IgnoredTestRow` would mix executable harness data into a generated
ownership ledger. The decided model is a dedicated **checked-in, registry-DRIVEN
oracle-query-spec registry**: the query payloads MOVE off the test bodies and INTO the
registry, and a lifted row body just calls a shared registry driver. Then query
coverage is true **by construction**.

1. **`OracleId` stays the coarse family** carried by `ProofRequirement::Ts7Oracle`
   on the manifest row. It selects the snapshot sub-directory
   (`oracle_snapshots/<family_snake>/`) — a directory / presentation key only. It is
   NOT the snapshot filename, NOT a `snapshot_id` input, and NOT the row→query join
   key.

2. **The oracle-query-spec registry is a closed Rust table of PURE context-neutral
   DATA, reachable by BOTH the lifted unit tests AND the `tests/` guard.** The lifted
   rows that consume it are `#[cfg(test)]` UNIT tests under `src/typeinfo/`
   (`mod.rs:62`); the no-orphan / coverage guards live in `tests/`. A registry in
   `tests/` would be UNREACHABLE from the unit-test bodies. So the registry lives at
   **`crates/verter_session/src/typeinfo/typeinfo_tests/oracle_query_specs.rs`** —
   reachable directly by the lifted unit tests — and the `tests/` guard consumes it via
   a shared crate-internal path (an `include!` / `#[path]` of the same data table), so
   there is ONE table, not two. For that `include!`/`#[path]` sharing to compile in
   BOTH the `src` unit-test driver AND the `tests/` integration guard, the table MUST
   be **pure context-neutral data**: closed enums + owned strings (`&'static str` /
   `String`), NO reference to `super::support`, NO private unit-test types, NO helper
   calls. The helper-CALLING shared registry driver stays in `src` (it can reach
   `support.rs`); the `tests/` side consumes the DATA table only — it never needs the
   helpers. This is the `oracle_query_specs_is_pure_data` constraint. It is keyed by
   **`(row_file, row_function, query_ordinal)`** — where `row_file` is EXACTLY
   `IgnoredTestRow.file`, the BARE filename (`"apparent_types.rs"`, fact 5) — and each
   entry carries the full executable query spec:

   - `oracle_family` — the `OracleId` (snake_case) for this entry. CARRIED ON THE
     ENTRY so the test-body-time driver can build the snapshot path
     `oracle_snapshots/<oracle_family>/<snapshot_id>.json` — at test-body time the
     driver otherwise knows only `(file, function)` and could not name the family
     sub-directory. This is the directory / presentation key (excluded from
     `snapshot_id`, §Q1);
   - `workspace_files` — the ordered file set the row upserts (leading-slash path +
     the source the test upserts), so multi-file rows (`cross_file.rs:6`) are
     representable;
   - `primary_canonical` — the leading-slash file id passed to the helper; for
     `ResolveExpr` / `ShallowSurfaceExpr` it is the `resolve_expr` scope file (the probe
     is appended to THIS file — the same-file probe, §Q2), for `EvaluateExpr` it is the
     `evaluate_expr` `scope` file whose `eval_source` is the scratch prelude (§Q2; the
     probe is the scratch file, NOT a same-file append) (`support.rs:132,160,208`);
   - `host_project` — the host/project setup axes
     `{ project_root, workspace_root, tsconfig_path, host_setup_kind }`. The ONLY
     first-class `host_setup_kind` is `standalone` (`make_host_with_footprint` =
     `VerterHost::new_standalone`, fact 9, `support.rs:89`), driven under the canonical
     `oracle.tsconfig.json` + synthetic root. Carried so the generator drives tsgo with
     the same host/project identity and the snapshot's `host_project` (§Q1) is
     re-derivable. Both `workspace_footprint` (`make_host_with_workspace_files_footprint`,
     `support.rs:97`, the ~9-row minority) AND package-backed / custom-host entries
     (`make_package_host_with_workspace`) are DEFERRED classes (§Scope, §4 env-pin spike)
     — the enum carries their discriminants for schema totality but no row is admitted
     under them initially;
   - `oracle_value_kind` — `structured_type_expr` for an eligible query (the
     kind-eligibility gate, §Scope);
   - **An independent non-`TypeExpr` obligation promotes the row to `OracleAndGuard`, it
     is NOT stored as a typed set on a ledger record.** An obligation is a property of the
     ROW's original BODY (which assertions it carried), not of one of the N
     per-`query_ordinal` registry entries; the registry entries above are per-query and
     carry NO obligation field. A bare `Ts7Oracle` row that only calls `oracle::run_row(…)`
     + a `TypeExpr` compare proves the projected shape ONLY — it SILENTLY DROPS any other
     assertion the original body carried, e.g. dependency-footprint / audit-record checks
     like `flow_return_xf04_records_barrel_route_before_selected_leaf`
     (`flow_return_catalog.rs:1496`). (A same-mode `assert_query_mode(…)` is NOT such an
     obligation — it is oracle query identity, proven live by
     `lifted_row_audit_query_mode_matches_spec`.) A row that DOES carry an independent
     non-`TypeExpr` obligation is therefore PROMOTED to
     `ProofRequirement::OracleAndGuard { oracle, guard }`: the `oracle` half proves the
     shape, and the `guard` half names a REGISTERED LIVE PROVER (conceptually in
     `OBLIGATION_GUARD_REGISTRY`) that re-asserts the original expectation. The proof shape
     itself — bare `Ts7Oracle` vs `OracleAndGuard` — is what records that the row carries an
     obligation; there is no separate typed obligation SET stored on a ledger entry. (The
     round-2 stored obligation ledger was retired.)

     **The five obligation KINDS the `OracleAndGuard.guard` provers cover.** Each KIND has a
     registered live prover that re-asserts the ORIGINAL expectation against the live result
     — not merely re-runs the query:

     - `DependencyFootprint` — the body asserted a resolved dependency footprint
       (`includes`/`excludes` leading-slash canonical paths);
     - `AuditRecord` — the body asserted on a returned `RequestAuditRecord` (`(field, value)`
       pairs);
     - `WarmCache` — the body asserted a warm-cache / cache-hit fact (fact keys);
     - `DeclaredDependency` — the body asserted a declared-dependency fact (ids);
     - `DivergenceCorrection` — a divergence row's correction linkage
       (`ts-compat-two-mode-model.md` §9.2) — the divergence-data tie
       (`resolver(query) == correction.correct_value` while that query's
       `snapshot.oracle_value` is the recorded `TsCompat` value and differs), per corrected
       `query_ordinal`.

     **The prover re-PROVES the original assertion, not merely re-runs the query.** A prover
     that only re-ran the row's oracle execution could not re-prove "footprint includes
     `/foo` excludes `/bar`", "audit field X `== Y`", or "warm-cache fact Z present". Each
     registered prover therefore re-asserts the original expectation (the recorded paths /
     fields / facts / ids / correction tie) against the LIVE result. A divergence /
     footprint / audit prover knows WHICH `query_ordinal` it is over and selects that query's
     result — never "any query passes" (unsound) nor "every query must pass" (stricter than
     the original body).

     **The `GuardId` enum is EXTENDED with obligation-proving variants.** The existing
     `GuardId` (`typeinfo_ignored_test_manifest.rs:356`) carries only the six legacy
     structural-guard variants (`ModeBoundaryExactness`, `ExpansionBoundaryPrecision`,
     `DemandBoundaryPrecision`, `CacheInvalidationRoute`, `AuditFootprintAttachment`,
     `CrossFileRouteFact`) — none of which is a generic obligation prover. The obligation
     model adds a CLOSED set of obligation-proving `GuardId` variants, ONE per obligation
     KIND, each a REAL registered guard in the §4 table:

     | obligation KIND | `GuardId` variant | proving guard / test (a §4-table row) |
     | --- | --- | --- |
     | `DependencyFootprint` | `GuardId::DependencyFootprintObligation` | `dependency_footprint_obligation_reproved` — re-runs the row's oracle execution and asserts the recorded `includes`/`excludes` paths against the live resolved dependency set |
     | `AuditRecord` | `GuardId::AuditRecordObligation` | `audit_record_obligation_reproved` — re-asserts the recorded `(field, value)` pairs against the returned `RequestAuditRecord` |
     | `WarmCache` | `GuardId::WarmCacheObligation` | `warm_cache_obligation_reproved` — re-asserts the recorded warm-cache / cache-hit fact keys against the live cache state |
     | `DeclaredDependency` | `GuardId::DeclaredDependencyObligation` | `declared_dependency_obligation_reproved` — re-asserts the recorded declared-dependency ids against the live declared-dependency set |
     | `DivergenceCorrection` | `GuardId::DivergenceCorrectionObligation` | `divergence_correction_obligation_reproved` — re-asserts the per-query divergence-data tie (`resolver(query) == correction.correct_value` while that query's `snapshot.oracle_value` is the recorded `TsCompat` value and differs), and that the recorded `correction_id` resolves to the named correction overlay and `divergence_id` to a registry entry whose id equals it (`ts-compat-two-mode-model.md` §9.2) |

     This `KIND → GuardId → guard/test` mapping is the human-readable MIRROR of a CHECKED-IN
     CODE registry — the gate's actual input is the code, not this Markdown table. A Rust
     test cannot introspect a Markdown table nor prove a named function exists by reading
     prose, so the AUTHORITY is a checked-in `const` registry in the oracle module
     (alongside `oracle_query_specs.rs`): a closed, total static slice keyed by `GuardId`
     whose entry binds the obligation prover and its typed shape, e.g.

     ```
     struct ObligationGuardEntry {
         guard_id:         GuardId,                    // the obligation-proving variant
         prover:           fn(&RowExecutionResults) -> Result<(), GuardFailure>,
     }
     // OBLIGATION_GUARD_REGISTRY: &[ObligationGuardEntry] — one entry per *Obligation GuardId,
     //   `prover` is the ACTUAL re-proving fn path (e.g. dependency_footprint_obligation_reproved),
     //   a real callable symbol the compiler resolves — not a string naming a Markdown row.
     // `RowExecutionResults` is the per-`query_ordinal`-indexed execution results the driver
     //   already produces for the row (each query's RequestAuditRecord / projection mode /
     //   resolved footprint), plus the row-level aggregate.
     ```

     **Helper-result availability is helper-specific — the per-helper slots are OPTIONAL and
     fail-closed.** `resolve_expr` / `evaluate_expr` produce an `AuditRecord` (carrying the
     query-mode / footprint data); `shallow_surface_expr` is `TypeExpr`-only and carries NO
     audit record. An `AuditRecord` prover targeting a query whose helper produced no such
     record CANNOT prove it, so it FAILS the gate (the absent slot is modeled as a missing
     `Option`, never silently waved through) — the obligation must target a helper whose
     execution actually emits the record it re-asserts.

     **The prover SELECTS its target's result, never an arbitrary or all-queries blob.**
     The prover receives the per-`query_ordinal`-indexed `RowExecutionResults`: a per-query
     prover asserts against THAT ordinal's result (its `RequestAuditRecord` / projection
     mode / footprint), and a row-level prover asserts against the row-level aggregate.
     Selecting the right query guarantees the original expectation is asserted against the
     CORRECT query's live result — never "any query passes" (unsound) nor "every query must
     pass" (stricter than the original body).

     The §4 guard table is the human-readable mirror of this registry; the registry is the
     authority `kind_eligibility_gate` reads. The gate looks the proof's `OracleAndGuard.guard`
     UP in `OBLIGATION_GUARD_REGISTRY` and asserts the entry EXISTS (membership is a registry
     lookup, not a Markdown-name match). A complementary parity check asserts the §4 guard
     table lists exactly the registry's prover fns (the mirror stays in sync). The registry's
     `prover` field is a real `fn` symbol the compiler resolves, so a `guard` naming a prover
     with no registry entry — or a registry entry whose `prover` does not compile — FAILS at
     build time, not by a prose cross-reference. (`AuditFootprintAttachment` among the legacy
     `GuardId` variants is NOT in this registry — it is a specific structural guard, not a
     generic obligation prover; obligation proving uses the dedicated `*Obligation`
     variants above, so the gate never conflates a legacy structural guard with an
     obligation re-prover.)

     The `kind_eligibility_gate` then enforces:
     - a BARE `Ts7Oracle` row that carries an INDEPENDENT non-`TypeExpr` obligation FAILS
       (the shape compare cannot prove the obligation) — it must be promoted to
       `OracleAndGuard`;
     - a row carrying an independent non-`TypeExpr` obligation is admissible ONLY under
       `OracleAndGuard` (a divergence row is an `OracleAndGuard` whose `guard` is the
       `DivergenceCorrection` prover, §9.2);
     - under `OracleAndGuard`, the proof's `guard` must resolve to a registered live prover
       in the checked-in CODE `OBLIGATION_GUARD_REGISTRY` (a registry lookup whose `prover`
       is a real `fn` symbol — NOT a Markdown-name match against the §4 table) — a `guard`
       with no registry entry FAILS;
     - a per-query prover (`AuditRecord`, `DivergenceCorrection`) selects a valid in-range
       `query_ordinal` (the "Per-kind targeting rule" below).

     **Per-kind targeting rule.** A row may issue N oracle queries keyed by `query_ordinal`
     `0..N` (§Q5 `oracle_query_ordinals`), so a per-query prover must select WHICH query it
     is over or it cannot soundly prove anything — checking "any" query is unsound and "all"
     is stricter than the original body. The provers select per kind:
     - `AuditRecord` is INHERENTLY per-query — a `RequestAuditRecord` is produced BY a
       specific oracle query, not by the row's whole execution. Its prover selects a
       `query_ordinal < oracle_query_ordinals` (the row's declared query count, §Q5). An
       out-of-range / unselectable ordinal FAILS the gate.
     - `DependencyFootprint`, `WarmCache`, and `DeclaredDependency` provers MAY assert over
       the whole-row aggregate (the footprint/cache/deps over the row's whole execution) OR
       a SPECIFIC query's footprint/cache/deps, whichever the original body asserted.
     - `DivergenceCorrection` is INHERENTLY per-query — the divergence-data tie
       (`resolver(query) == correction.correct_value` while that query's
       `snapshot.oracle_value` is the recorded `TsCompat` value and differs) is a property of
       a SPECIFIC corrected query, not the row's whole execution. Its prover selects a
       `query_ordinal < oracle_query_ordinals`; an out-of-range ordinal FAILS the gate. A
       row may carry one `DivergenceCorrection` per corrected query, mixing corrected and
       ordinary queries.
     - DEFAULT-REJECT BACKSTOP: for a row with `oracle_query_ordinals > 1`, ANY per-query
       prover that cannot select a valid in-range `query_ordinal` FAILS admission — the row
       is default-rejected and DEFERRED rather than admitted against an unaddressable
       obligation. A single-query row (`oracle_query_ordinals == 1`) resolves ordinal `0`
       unambiguously.

     This makes the §Scope "exclude footprint/audit rows" rule enforceable through the
     PROOF SHAPE + a real registered prover, not as a single coarse boolean nor a vague
     "transitively asserts" claim (the lifted body no longer carries the extra assertions,
     and `IgnoredTestRow` has no assertion-kind field). The schema change is: the `GuardId`
     enum gains the five `*Obligation` variants above (one per obligation KIND, including
     `DivergenceCorrectionObligation` for the divergence case), and the coverage registry
     the gate reads is the checked-in CODE `OBLIGATION_GUARD_REGISTRY`
     (`GuardId → prover fn`), of which the §4 guard table is the human-readable mirror — so
     the gate's membership check is a real registry lookup against compiled `fn` symbols,
     not a Markdown cross-reference. The driver does NOT preserve the original generic
     assertions inline (that would re-introduce hand-authored body assertions the registry
     model removes); instead each obligation is proven by the `OracleAndGuard.guard`'s
     registered prover re-asserting its recorded expectation — the proof shape records WHICH
     guard must exist AND the prover records WHAT it must prove;
   - `query_helper_kind` — a **CLOSED enum** naming which `support.rs` helper produces
     the in-process `TypeExpr`, with kind-specific payload:
     - `ResolveExpr { symbol, type_args, projection_mode }` — drives `resolve_expr`
       (`support.rs:132`);
     - `ShallowSurfaceExpr { symbol }` — drives `shallow_surface_expr`
       (`support.rs:160`; = `ResolveDecl` + empty-path `Shallow` `ProjectPath`, always
       `Shallow`);
     - `EvaluateExpr { expression, source_locator, projection_mode }` — drives
       `evaluate_expr` (`support.rs:208`) over a **type-position-valid SINGLE-ROOT**
       expression string (e.g. `typeof f`; §Q2 EvaluateExpr type-position gate + the
       closed single-root grammar). The expression must match the closed single-root
       grammar — one binder reference + an optional same-binder index/property path — so
       its ONE `source_locator` covers every referent; a multi-/nested-referent
       expression is DEFAULT-REJECTED (§Scope, deferred to the multi-referent locator-set
       spike, §4). The `source_locator` is the TYPED locator (below) of the
       value/function the single-root expression derives from (the `f` in `typeof f`) —
       NOT just the expression string —
       so the source-side allowlist walk (§Q2) and its transitive `typeof` / `ReturnType`
       follow (§Q2) have a concrete declaration to start from. `ResolveExpr` /
       `ShallowSurfaceExpr` carry the same TYPED `source_locator` formed from
       `(primary_canonical, symbol, symbol_space)`.

       **The TYPED `source_locator` shape.** `source_locator` is NOT a bare
       `(canonical, name)` pair — that cannot disambiguate the many cross-file /
       merged / overloaded / type-vs-value rows among the 122 `Relate`-free candidates
       (a name can resolve to a TYPE-space declaration, a VALUE-space declaration, both
       under declaration merging, an overload SET, an import/reexport alias, a barrel
       hop, or a shadowed local). It is a closed-tagged locator carrying the
       **symbol-space** explicitly:

       ```
       SourceLocator {
           reference_canonical: leading-slash file id of the REFERENCING site
                                (= primary_canonical for ResolveExpr/ShallowSurfaceExpr;
                                 the EvaluateExpr scope file for EvaluateExpr),
           reference_name:      the identifier as written at the reference site
                                (the symbol for Resolve*, the leading binder of the
                                 expression for EvaluateExpr — e.g. `f` in `typeof f`),
           symbol_space:        Type | Value   // closed enum — which lookup table the
                                               // name is resolved IN; `typeof f`
                                               // resolves `f` in VALUE space, `Foo` in a
                                               // type query resolves in TYPE space
       }
       ```

       **`source_locator` is GUARD-ONLY — deliberately NOT part of `identity` /
       `snapshot_id`, value-irrelevant for ALL THREE helper kinds.** It is registry payload
       consumed by `probe_binds_to_registry_target`, the source-side allowlist walk, and
       the migration fingerprint (where it IS hashed — it is a fidelity coordinate of the
       original body), but it is NOT a value-affecting identity input. The value-
       irrelevance holds the SAME way for every `query_helper_kind`, and the reason is the
       same in each case: the locator's `symbol_space` is DERIVED FROM THE QUERY, not an
       independent input that could re-select a different answer.
       - **`EvaluateExpr`** — the value is fully determined by `(scope, expression,
         projection_mode)`; `evaluate_expr` passes EMPTY `extra_imports` and
         `cacheable: false` (`support.rs:208,218,220`). The `symbol_space` is fixed by the
         EXPRESSION's position (`typeof f` resolves `f` in VALUE space because `typeof`
         demands a value; a type query resolves in TYPE space) — so the locator's
         `symbol_space` merely RECORDS the space the expression already forces; it cannot
         steer the resolver to a different declaration.
       - **`ResolveExpr`** — the value is determined by `(primary_canonical, symbol,
         type_args, projection_mode)`. `resolve_expr` resolves `symbol` in TYPE space (it
         is a type-position resolution by construction, `support.rs:132`), so the locator's
         `symbol_space` is ALWAYS `Type` for a `ResolveExpr` entry — derived from the
         helper kind, not chosen. The locator names the same `(reference_canonical,
         reference_name)` the query already resolves; spelling it out changes nothing the
         resolver reaches.
       - **`ShallowSurfaceExpr`** — identical to `ResolveExpr` (it is `ResolveDecl` +
         empty-path `Shallow`, `support.rs:160`): TYPE space, value determined by
         `(primary_canonical, symbol)` + the always-`Shallow` mode; the locator is again a
         derived `Type`-space coordinate.

       In all three cases a `source_locator` change CANNOT change the computed answer: it
       names the SAME referencing site + the symbol-space the query ALREADY forces, and the
       shared resolver reaches the identical defining declaration(s) regardless of whether
       the registry SPELLS the locator out. It is purely a GENERATION-TIME / GUARD
       coordinate (and a migration-fidelity coordinate) that tells the source-side walk +
       binding-identity check WHERE to start and WHICH space to resolve in — never read on
       the value-producing path. So it stays out of `identity`/`snapshot_id` (pinning it
       there would over-key the snapshot to a coordinate that does not change the value),
       while still being covered by the migration fingerprint so a lift that mis-records it
       FAILS the fidelity guard. (Contrast `symbol_or_expression` + `projection_mode` +
       `workspace_files` + `host_project`, which DO determine the value and DO enter
       `snapshot_id`.)

   The `query_ordinal` distinguishes the N queries a single row issues (e.g. a
   multi-`resolve_expr` `conditional_infer.rs` row maps to ordinals `0..N`). The
   SHARED REGISTRY DRIVER, the GENERATOR, and the GUARD all consume the registry.

   **The lifted body is an ATTRIBUTE PROC-MACRO that CAPTURES the enclosing fn name — a
   hand-typed key string is FORBIDDEN, and a no-arg declarative macro is INFEASIBLE.** A
   naïve body `oracle::run_row(file!(), "<fn_name>")` passes a hand-typed `"<fn_name>"`
   string literal: if `foo_test`'s body mistyped (or copy-pasted)
   `oracle::run_row(file!(), "bar_test")`, EVERY guard would still pass (the row-ref is
   internally consistent — it just points at `bar_test`'s registry entries) while
   `foo_test` silently validates `bar_test`'s snapshots — a wrong-row execution that no
   coverage/biconditional/count guard catches, because the row-ref it runs against is a
   real, fully-covered registry key. A no-arg DECLARATIVE (`macro_rules!`) macro CANNOT
   fix this: a function-like declarative macro invoked INSIDE a fn body is given NO access
   to the enclosing item's identifier — Rust does not pass the surrounding `fn` name into
   a body-position macro invocation — so `oracle_row!()` literally could not recover the
   key. The mechanism is therefore an **ATTRIBUTE PROC-MACRO `#[oracle_row]` placed ON the
   test fn**: an attribute macro receives the WHOLE `ItemFn` token stream (including its
   identifier `sig.ident`) as input, so it can read the fn's own name and synthesize the
   body deterministically — the author never types the key.

   ```rust
   #[oracle_row]        // attribute proc-macro — sees the ItemFn incl. its identifier
   #[test]
   fn composed_props_expands() {}   // body is synthesized by #[oracle_row]
   ```

   **Concrete mechanism + file layout.** `#[oracle_row]` is a `#[proc_macro_attribute]`
   exported from a NEW dedicated proc-macro crate (a proc-macro crate cannot also export
   non-proc-macro items, so it is its own crate):
   `crates/verter_session_oracle_macro/` (`proc-macro = true`, deps `syn` + `quote` +
   `proc-macro2`), added as a `dev-dependency` of `verter_session` (the lifted bodies are
   `#[cfg(test)]` unit tests, so a `dev-dependency` keeps it out of the production build +
   the resolver's dep closure — preserving `tsgo_not_reachable_from_resolver` /
   `oracle_consumption_path_has_no_tsgo_spawn`). The macro parses its input as
   `syn::ItemFn`, reads `item_fn.sig.ident` (the fn's OWN identifier), preserves the fn's
   other attributes (notably `#[test]`) and signature, and REPLACES the body with the
   synthesized driver call `oracle::run_row(file!(), "<sig.ident>")` where the
   `"<sig.ident>"` string literal is emitted FROM the parsed identifier — never typed by
   the author. (Attribute ordering: `#[oracle_row]` is the OUTER attribute so it expands
   first and re-emits the fn carrying `#[test]`, which the test harness then sees.)
   Because the key is sourced from the fn's own `sig.ident` token, a `foo_test` CANNOT
   name `bar_test`'s key — the wrong-row-execution class is foreclosed at the macro
   boundary, not by a downstream guard. Pinned by the `lifted_body_is_self_keyed_macro`
   guard: it SOURCE-WALKS every `#[test]` fn in `src/typeinfo/typeinfo_tests/` that reaches
   the oracle driver and asserts (a) the fn carries the `#[oracle_row]` ATTRIBUTE and has
   NO hand-written body that calls `oracle::run_row(file!(), "…")` with a string-literal
   key, AND (b) the key the attribute synthesizes (= the attributed fn's own `sig.ident`)
   equals the enclosing fn's identifier — so each lifted fn invokes EXACTLY its own
   `(file, function)` registry key, and a hand-typed key is impossible (the body is
   generated, not authored).

   Because `file!()` yields a path (e.g.
   `crates/verter_session/src/typeinfo/typeinfo_tests/apparent_types.rs`) but
   `IgnoredTestRow.file` and the registry key are the BARE filename (manifest discovery
   uses `path.file_name()`, `:796`/`:902`/`:1012`), the driver
   **basename-normalizes `file!()` via `Path::file_name()`** before the registry lookup.
   It then reads the row's entries, runs the helper named by `query_helper_kind`, builds
   each snapshot path from the entry's `oracle_family` + re-derived `snapshot_id`, loads
   each snapshot, and asserts. The snapshot DUPLICATES the spec (in `identity` +
   `row_ref`) for drift detection but is NOT the source of truth.

   **Independent declared query-count cross-check.** A registry that
   UNDER-counts — e.g. 3 entries for a row that actually issues 4 queries — is
   INVISIBLE to a purely registry-driven driver + guard: the driver would run 3 and
   the guard would see 3, with no signal that a 4th is missing. Coverage is therefore
   NOT "true by construction" from the registry alone. The honest fix is an
   INDEPENDENT declared count: a NEW `IgnoredTestRow` field
   **`oracle_query_ordinals: u16`** (the number of oracle queries the row declares it
   issues). The manifest table is SCRIPT-GENERATED — both tables and every per-column
   value are produced by `scripts/gen-typeinfo-ignore-manifest.py`
   (`typeinfo_ignored_test_manifest.rs:20`) and emitted into the `include!`'d
   `manifest_data/typeinfo_ignored_test_manifest_rows.rs` data file
   (`typeinfo_ignored_test_manifest.rs:557`); the guards only diff/fail, never write.
   So `oracle_query_ordinals` is declared as an INPUT to that manifest generator's
   row-spec source — the SAME place each row is declared, sourced INDEPENDENTLY of
   `oracle_query_specs.rs`. If it were derived FROM the registry it would not be an
   independent source and the count could not detect a registry under-count. The guard
   `registry_entry_count_matches_declared` asserts the registry holds EXACTLY that many
   `(row_file, row_function, *)` entries per manifest oracle row — the STRONGER all-rows
   form, including the zero-count rows (a declared-`0` `Ignored` / non-oracle / un-lifted
   row MUST have ZERO registry entries). The manifest-generator
   declared count and the registry entry count are two independent sources; the guard
   is the non-circular cross-check between them.

   The cross-check is STRONGER than count-equality: `registry_entry_count_matches_declared`
   ALSO requires the registry's `query_ordinal`s for a row to be **UNIQUE and CONTIGUOUS
   `0..count-1`** (no gaps, no duplicates, no off-by-one). A registry that emits ordinals
   `{0, 1, 3}` for a declared count of 3 (a missing `2`) or `{0, 0, 1}` (a duplicate)
   matches the count but is malformed; the contiguity + uniqueness requirement catches it
   on top of the count.

   **The count + contiguity verify CARDINALITY, not registry PAYLOAD correctness — the
   migration fingerprint closes the gap, and it is THE migration-fidelity authority.**
   `registry_entry_count_matches_declared` (count + contiguity), the biconditional, and
   the binding guards (`probe_binds_to_registry_target`) all prove the registry TARGET is
   internally consistent and reachable — but NONE of them proves the registry's payload
   actually MATCHES the query the ORIGINAL hand-authored row body issued, NOR that the
   lift preserved the row's non-`TypeExpr` obligations. A registry entry with the right
   COUNT but a wrong `symbol_or_expression` (or wrong canonical, mode, host setup, or
   resolved symbol-space) would pass every cardinality + binding guard while validating a
   DIFFERENT query than the row authored — a silent migration error. The honest closure
   is a **migration fingerprint extracted from the original body BEFORE the body is
   replaced by `#[oracle_row]`**, and the `migration_fingerprint` (on the retained-lift
   metadata) is the SOLE migration-fidelity authority — NOT a hand-maintained registry
   field. (This `migration_fingerprint` body-hash fidelity layer is a GENUINELY DEFERRED
   TODO — not yet wired; the description is the planned design.) The fingerprint covers
   EVERY value-affecting input AND the row's proof shape (`Ts7Oracle` vs `OracleAndGuard` +
   its `guard` id) the lift must preserve:

   - The lifting step, in the SAME block, mechanically extracts from the row's
     pre-replacement body (a `syn` AST parse of the body — "Extraction method" below) the
     ORDERED tuple of executable query payloads it issued — each `resolve_expr` /
     `shallow_surface_expr` / `evaluate_expr` call's FULL fidelity tuple:

     ```
     (
        helper_kind,            // ResolveExpr | ShallowSurfaceExpr | EvaluateExpr
        primary_canonical,
        symbol_or_expression,
        type_arguments,         // canonical TypeExpr-JSON of each arg
        projection_mode,
        workspace_files,        // each {path, content_hash}, sorted by path
        source_locator,         // the TYPED locator {reference_canonical, reference_name,
                                //   symbol_space} the call resolves through (incl. the
                                //   Type|Value symbol_space) — §Q4
        host_project            // {project_root, workspace_root, tsconfig_path,
                                //   host_setup_kind} the call's helper constructed
     )
     ```

     PLUS — captured ONCE per row, NOT per query — the **proof shape**: the lift-time AST
     extraction DETECTS every INDEPENDENT non-`TypeExpr` assertion the original body carried
     beyond the `TypeExpr` shape compare — dependency-footprint assertions, audit-record
     assertions, warm-cache / cache-hit assertions, declared-dependency assertions — and a
     row that carries one is seated as `OracleAndGuard { oracle, guard }` whose `guard` is
     the registered prover for that obligation KIND, rather than bare `Ts7Oracle`. (A
     same-mode `assert_query_mode(…)` is NOT extracted as an obligation — the query's
     `projection_mode` is part of the per-query fidelity tuple, i.e. query identity, proven
     live by `lifted_row_audit_query_mode_matches_spec`.) The extraction maps each detected
     obligation-bearing assertion to its KIND (`DependencyFootprint` / `AuditRecord` /
     `WarmCache` / `DeclaredDependency`) and the registered `GuardId` prover that re-proves
     it post-lift. A body that asserts such behavior but is seated bare `Ts7Oracle` (no
     `guard`), or whose assertion arguments are NOT statically const-foldable, is a CAPTURE
     BUG: the fidelity guard FAILS rather than letting a bare `Ts7Oracle` row silently drop
     the original assertions.

     The canonical-JSON **`migration_fingerprint`** is computed over the ordered per-query
     fidelity tuple ABOVE ∪ the row's proof shape (`Ts7Oracle` vs `OracleAndGuard` + its
     `guard` id), recorded ONCE in the retained-lift metadata at lift time. Because
     `source_locator`, `host_project`, and the proof shape are now IN the fingerprint, a
     lift that drops or mis-records any of them — a `workspace_footprint` row migrated as
     `standalone`, a `source_locator` whose `symbol_space` flipped Type↔Value, or a bare
     `Ts7Oracle` seating for a row that asserted a footprint — FAILS the fidelity guard
     rather than passing. (This `migration_fingerprint` / `original_body_tokens` body-hash
     fidelity layer is a GENUINELY DEFERRED TODO — not yet wired; the description is the
     planned design.)
   - `registry_payload_matches_migration_fingerprint` asserts that the registry entries
     for the row (their `helper_kind`, `primary_canonical`, `symbol_or_expression`,
     `type_arguments`, `projection_mode`, `workspace_files`, `source_locator`, and
     `host_project`), taken in `query_ordinal` order, ∪ the row's proof shape
     (`Ts7Oracle` vs `OracleAndGuard` + its `guard` id), re-canonicalize to the SAME
     `migration_fingerprint` recorded in the retained-lift metadata. A registry payload that
     drifted from the original body's query (wrong symbol, canonical, mode, type-args, file
     set, source-locator/space, host setup), OR a proof shape that drifted (a row demoted to
     bare `Ts7Oracle` that originally asserted a footprint/audit/warm-cache check), FAILS —
     proving the registry faithfully reproduces the ORIGINAL hand-authored query AND the
     proof shape preserves its obligations, not merely a self-consistent target. (Deferred
     alongside `migration_fingerprint`.)

   So the claim is NOT the weaker "the registry is authoritative AFTER lift, original
   coverage unverified" — the planned migration fingerprint VERIFIES the full registry
   payload + proof shape against the original body at lift time, and the retained-lift
   metadata holds it so the verification re-runs on every regeneration. The fingerprint is
   the migration-fidelity AUTHORITY: the registry's `source_locator` / `host_project` (and
   the row's proof shape) are validated AGAINST it, never hand-maintained as the truth.

   **The ORIGINAL extraction input stays AUDITABLE — the CHECKED-IN `original_body_tokens`.** Once the
   `#[ignore]` body is replaced by `#[oracle_row]`, every downstream guard compares only
   the registry payload to the RETAINED `migration_fingerprint`. That closes registry
   DRIFT but not a WRONG INITIAL extraction: a fingerprint computed from a mis-extracted
   body is self-consistent with the registry forever, validating the wrong query. To make
   the extraction INPUT auditable rather than only self-compared, the retained-lift
   metadata STORES THE EXTRACTION INPUT ITSELF — `original_body_tokens`, the canonical
   original `#[test]` body `syn` token stream (`Span`-stripped, whitespace-insignificant
   token-tree print — the EXACT bytes the extractor read) — captured at lift time in the
   SAME audited lift command that computes `migration_fingerprint`. The
   `original_extraction_input_auditable` guard re-runs the extractor over this CHECKED-IN
   `original_body_tokens` artifact and asserts the re-derived fingerprint EQUALS the recorded
   `migration_fingerprint` — HERMETICALLY, from the retained-lift artifact alone, with NO VCS
   archaeology (no shallow-checkout / archive / CI-clone dependence). So a fingerprint that
   never matched its own claimed input is detectable, because the recorded extraction input
   is pinned IN the retained-lift metadata, not just the derived fingerprint. The token
   stream is a migration/audit record, NOT a `snapshot_id` input. (This whole
   `migration_fingerprint` / `original_body_tokens` body-hash fidelity layer is a GENUINELY
   DEFERRED TODO — not yet wired; the description is the planned design.) (The fingerprint
   is a LIFT-TIME migration check on the payload's FIDELITY; it is NOT a `snapshot_id`
   input — the value-affecting axes
   `symbol_or_expression`/`primary_canonical`/`type_arguments`/`projection_mode`/
   `workspace_files`/`host_project` already enter `snapshot_id` directly, and
   `source_locator` is deliberately guard-only, §Q4. The proof shape is a migration/coverage
   record, not a value-affecting axis, so it too stays out of `snapshot_id`.)

   **Extraction method — a `syn` AST parse that resolves known setup helpers, or REJECTS a
   non-statically-extractable body.** Many rows do NOT call the helpers inline with literal
   payloads — they hide upserts behind setup helpers (`upsert_cross_file_fixture`,
   `make_host_with_workspace_files_footprint`, package/workspace helpers,
   `support.rs`). A naive text/regex scan would miss those, so the extraction is a
   STRUCTURED `syn` parse run ONCE per row as a one-time AUDITED lift command (a
   `cargo run`-style generator step, never a `#[test]`), NOT a runtime guard:
   - Parse the original `#[test]` fn body as a `syn::Block` and walk its statements in
     order, collecting every call to a KNOWN oracle helper (`resolve_expr` /
     `shallow_surface_expr` / `evaluate_expr`) and every KNOWN workspace-setup helper. The
     **initial CLOSED helper model covers exactly these concrete `support.rs` corpus
     wrappers** (the named set the first auto-lift block models — adding a wrapper is an
     explicit model extension, not a silent widening):
     - `make_host_with_footprint()` → `VerterHost::new_standalone` under the canonical
       config (`support.rs:89`) — the `standalone` `host_project`;
     - `make_host_with_workspace_files_footprint(...)` → the `/workspace` host
       (`support.rs:97`) — `workspace_footprint` host_project (deferred class, but modeled
       for fidelity capture);
     - `upsert_ts(&host, path, source)` → one `workspace_files` entry (`{path, source}`);
     - `upsert_cross_file_fixture(host, leaf, barrel, consumer, …)` → the fixed ordered
       multi-file upsert set (`cross_file.rs:6`);
     - `resolve_expr` / `shallow_surface_expr` / `evaluate_expr` (`support.rs:132/160/208`)
       → the per-query fidelity tuple.
     Each known setup helper is RESOLVED to
     its concrete upsert(s) / host-construction by this closed, hand-maintained helper model
     (the helper → concrete-effect mapping the implementer pins once), so a row that builds
     its workspace through `upsert_cross_file_fixture(host, leaf, barrel, consumer)`
     expands to the same ordered `workspace_files` + `host_project` a literal-inline row
     would produce. A row that uses ONLY these modeled wrappers with const-foldable
     arguments is auto-liftable; any other wrapper is non-modeled and defers (below).
   - Constants, simple `let`-bound string locals, and inline string/array literals are
     canonicalized by const-folding the `syn` expression to its literal value; the helper
     model knows which argument positions are paths vs sources vs symbols.
   - **A body whose upserts/queries/obligations are NOT statically extractable by the
     closed helper model REJECTS — that row STAYS `Ignored`, NEVER auto-lifted with a
     partial fingerprint.** The closed helper model above cannot cover every wrapper helper,
     macro-generated row body, or closure-bearing assertion in the real corpus — and it is
     not required to. If the body computes a path/symbol/source through a loop, a
     NON-MODELED wrapper helper (one not in the named set above), a MACRO-GENERATED body the
     `syn` walk cannot fold, a CLOSURE-bearing assertion the walk cannot reduce to a literal,
     a runtime value, or any expression the const-folder cannot reduce to a literal, the
     extraction FAILS LOUDLY and the row stays `Ignored` (it is HAND-LIFTED later under an
     EXTENDED helper model, or deferred) — it is NEVER auto-lifted with a guessed or partial
     fingerprint. So the **initial auto-liftable set is a SOUND SUBSET** of the eligible rows:
     exactly the rows whose entire body folds through the named closed helper model, never a
     superset reached by guessing. This is the honest small-core framing — the auto-lift
     core grows ONLY as the helper model is explicitly extended (and re-audited), never by
     admitting an unmodeled body with an approximate fingerprint. Pinned by
     `migration_fingerprint_extraction_is_static`.

   **Edit-scope note.** Adding `oracle_query_ordinals` to `IgnoredTestRow`
   requires touching ALL ~362 row literals in the `include!`'d
   `manifest_data/typeinfo_ignored_test_manifest_rows.rs` data file: a lifted oracle row
   gets its real declared query count, every other row gets `0`. Because the rows are a
   flat literal table in one `include!`'d data file, the edit is MECHANICAL (a single
   field added to each struct literal, `0` for the non-oracle/un-lifted majority) — the
   implementer must plan the bulk edit but it carries no per-row design decision beyond
   the lifted rows.

3. **The per-snapshot id (`snapshot_id`) is a deterministic, REGISTRY-DERIVABLE
   string** — derived from REGISTRY-ONLY, tsgo-free inputs, computed at generation
   time and stored in both the filename and the JSON. Because there is **one file per
   `(row, query)`** (no shared snapshots), the id INCLUDES the row-ref. The crucial
   identity rule: every `snapshot_id` input is something a guard can read from the
   REGISTRY + the pinned env WITHOUT opening any snapshot or running tsgo. In
   particular **`oracle_env_hash` is NOT a `snapshot_id` input** — folding the FULL
   resolved-file-set hash into the filename would be circular (the env file list lives
   only INSIDE the snapshot, so a MISSING snapshot's filename could not be computed).
   Instead `oracle_env_hash` + `oracle_env_files` are STORED IN the snapshot and
   validated as a VALUE on read (point below + §Q5). The STABLE `env_corpus_id` (the
   content id of the CLOSED VENDORED oracle-env corpus) IS a `snapshot_id` input — it is
   a pinned-env constant (like `compiler_options_hash` / `tsgo_version`): the canonical
   oracle corpus is the SAME for every standalone-host row, so the registry + pinned env
   know it WITHOUT opening a snapshot, keeping the filename registry-derivable. (`env_corpus_id`
   and `oracle_env_hash` are two domain-separated digests over the same vendored file set with
   DIFFERENT roles + recipes — see §Q1 — and are NOT required to be byte-equal: `env_corpus_id`
   is the stable, registry-known corpus-content IDENTITY in the filename, `oracle_env_hash` is
   the on-read-validated content hash recomputed from `oracle_env_files.files`.) The derivation
   hashes EVERY value-affecting
   REGISTRY-derivable input:

   The hash input uses a **canonical, domain-separated, LENGTH-PREFIXED encoding**:
   each field is encoded as `u32-LE byte-length || field-bytes` (length-prefixed
   canonical-JSON for structured fields, raw UTF-8 for strings, fixed-width LE for
   integers), concatenated in the FIXED order below under a leading domain-separation
   tag `b"verter.oracle.snapshot_id.v1"`. Length-prefixing prevents any two distinct
   field tuples from producing the same byte stream (loose concatenation /
   `0x00`-separation is ambiguous when a field can itself contain the separator). The
   ordering and field set are fixed; a change to either is a schema change. The id is
   the **FULL (≥256-bit) BLAKE3 digest** hex-encoded — NOT a 12-byte truncation — and
   the `snapshot_id_is_unique` guard asserts no two distinct registry
   `(row, query_ordinal)` entries derive the same id (a proven uniqueness check, not an
   unproven "never collide" claim).

   ```
   snapshot_id = "u_" + hex( blake3(   // FULL 32-byte / 256-bit digest, not truncated
           DOMAIN_TAG            ||   // b"verter.oracle.snapshot_id.v1"
           lp(row_file)          ||   // bare filename, e.g. "apparent_types.rs"
           lp(row_function)      ||
           lp(query_ordinal)     ||   // the row-ref — one file per (row,query)
           lp(query_helper_kind) ||   // ResolveExpr | ShallowSurfaceExpr | EvaluateExpr
           lp(workspace_file_set)||   // each { path, content_hash } SORTED BY canonical path, canonical JSON (upsert order is NOT an input; duplicate paths rejected)
           lp(primary_canonical) ||
           lp(symbol_or_expression) ||
           lp(canonical_type_args)  ||   // normalized TypeExpr-JSON of each arg
           lp(projection_mode)   ||
           lp(host_project)      ||   // {project_root, workspace_root, tsconfig_path, host_setup_kind}
           lp(oracle_value_kind) ||   // structured_type_expr (future: relation_verdict …)
           lp(normalizer_version)     ||
           lp(probe_synthesis_version)||   // probe form + hover extraction + admissibility
           lp(compiler_options_hash)  ||
           lp(env_corpus_id)     ||   // STABLE content id of the closed vendored oracle-env corpus (pinned-env constant)
           lp(tsgo_version)      ||
           lp(oracle_schema_version)
           // NOTE: env_corpus_id (the STABLE closed-vendored-corpus content id) IS
           // an input — it is a pinned-env constant the registry knows. But the
           // per-snapshot oracle_env_hash (the on-read-validated FULL-file-set hash)
           // is DELIBERATELY NOT an input — folding the full resolved-file-set into
           // the filename would be circular; it is validated on the stored value, not
           // the filename (see oracle_env_files / §Q5).
       )
   )
   //  lp(x) = u32_le(byte_len(encode(x))) || encode(x)
   ```

   Note: `probe_locator` is NOT hashed directly — the probe is FIXED + VERSIONED
   (§Q2), so `probe_synthesis_version` + the query fully determine the locator; hashing
   a raw locator that is derived from the version would be redundant and would couple
   the id to an implementation detail rather than the versioned algorithm.

   - The `oracle_family` (`OracleId`) is DELIBERATELY EXCLUDED — it is a directory key
     only; a row re-categorised to a different family must NOT change its id.
   - **`env_corpus_id` has a CHECKED-IN, PINNED SOURCE-OF-TRUTH the registry reads — the
     `CURRENT_ENV_CORPUS_ID` pin.** For the registry to derive a snapshot filename
     WITHOUT opening any snapshot, it must know the current `env_corpus_id` from a
     checked-in source, not from a snapshot. That source is a single pinned constant
     committed ALONGSIDE the registry — **`CURRENT_ENV_CORPUS_ID`** (a `&'static str` in
     `oracle_query_specs.rs`'s pinned-env block, beside `tsgo_version` /
     `compiler_options_hash` / `normalizer_version` / `probe_synthesis_version`). It is
     the content id of the one CURRENT closed vendored corpus, and is mirrored by a
     committed `oracle_env/CURRENT` pointer file (a one-line text file holding the same
     id) so the on-disk corpus root `oracle_env/<env_corpus_id>/` is locatable from the
     filesystem alone. The registry + every guard read `CURRENT_ENV_CORPUS_ID` (NOT a
     snapshot) to derive `snapshot_id` and to locate the corpus directory; a snapshot's
     stored `env_corpus_id` MUST equal `CURRENT_ENV_CORPUS_ID` (validated on read).
   - **Exactly ONE current corpus directory; stale corpus dirs are GC'd or REJECTED.**
     Because `env_corpus_id` enters `snapshot_id`, a regeneration that re-vendors the
     corpus produces a NEW `oracle_env/<new_id>/` directory and orphans every snapshot
     under the old id (they no longer round-trip — their stored `env_corpus_id` ≠
     `CURRENT_ENV_CORPUS_ID`). The rule is: **`oracle_env/` holds exactly ONE corpus
     directory, and it is the one named by `CURRENT_ENV_CORPUS_ID`** (plus the `CURRENT`
     pointer file). The regenerator DELETES the prior `oracle_env/<old_id>/` directory
     in the same pass that writes the new one (a clean cutover — no dual corpus). The
     `oracle_env_single_current_corpus` guard asserts (a) `oracle_env/` contains exactly
     one `<env_corpus_id>/` directory, (b) its name equals `CURRENT_ENV_CORPUS_ID` and the
     `CURRENT` pointer's contents, and (c) every snapshot's stored `env_corpus_id` equals
     `CURRENT_ENV_CORPUS_ID` — so a leftover stale corpus dir, a `CURRENT`-pointer
     mismatch, or a snapshot pinned to a retired corpus all FAIL rather than silently
     coexisting. That guard checks the four spellings (constant, pointer, dir name,
     snapshot field) agree WITH EACH OTHER; it does NOT recompute the id from content. The
     separate `env_corpus_id_recomputes_from_corpus` guard closes the remaining loop: it
     RECOMPUTES the id from the on-disk corpus (the canonical-path-sorted listing + each
     file's normalized content, the same BLAKE3 recipe the generator used) and asserts the
     recomputed id EQUALS `CURRENT_ENV_CORPUS_ID`, the `CURRENT` pointer, the dir name, AND
     every snapshot's `env_corpus_id` — so a corpus whose content drifted WITHOUT re-
     pinning (all four spellings still mutually equal, but no longer naming what is on
     disk) FAILS offline, which mutual-equality alone cannot catch.
   - **`env_corpus_id` is STABLE-ACROSS-ROWS, not IMMUTABLE — its regeneration
     semantics.** "Stable pinned-env constant" means STABLE ACROSS ROWS WITHIN ONE
     GENERATION: `env_corpus_id` is the SAME value for every standalone-host row in a
     given generation (the canonical vendored corpus is shared), which is exactly what
     makes the registry able to know it WITHOUT opening any snapshot. It is NOT immutable
     across generations: a regeneration that re-vendors the corpus (a tsgo-version bump,
     a lib-set change, an added/edited ambient `.d.ts`) RECOMPUTES a new `env_corpus_id`,
     which — because it enters `snapshot_id` — changes EVERY standalone snapshot's
     filename and forces a full regeneration of those snapshots. So `env_corpus_id`
     behaves like `compiler_options_hash` / `tsgo_version`: a constant the registry +
     pinned env share within a generation, recomputed on each regeneration. Regeneration
     UPDATES THE PINNED POINTER in lockstep: the regenerator re-vendors the corpus into
     `oracle_env/<new_id>/`, rewrites `CURRENT_ENV_CORPUS_ID` in `oracle_query_specs.rs`
     and the `oracle_env/CURRENT` pointer file to the new id, DELETES the prior
     `oracle_env/<old_id>/` directory, and rewrites every standalone snapshot under its
     new id — one atomic regeneration pass, never a stale pin or a dual corpus. `env_corpus_id`
     and the per-snapshot `oracle_env_hash` are **two DOMAIN-SEPARATED digests with DIFFERENT
     ROLES + DIFFERENT RECIPES** — they range over the same vendored SHARED corpus file set but
     are NOT required to be byte-equal (they are intentionally distinct hex values, see §Q1).
     Stated explicitly so an implementer never conflates them: `env_corpus_id` is the **filename
     INPUT** — the registry-KNOWN, `snapshot_id`-bearing pinned-env CONSTANT, BLAKE3 domain-
     separated under the `env_corpus_id` tag over the canonical-path-sorted CORPUS LISTING
     (`[{ path, content_hash }]`); the registry + pinned env know it without opening a snapshot,
     so the filename is registry-derivable. `oracle_env_hash` is the **offline-REVALIDATED
     VALUE** — stored in the snapshot and recomputed-on-read, BLAKE3 domain-separated under the
     `oracle_env_hash` tag over `oracle_env_files.files` (re-enumerate the vendored corpus +
     re-hash against on-disk content) to catch env drift. DIFFERENT recipe, DIFFERENT role,
     DIFFERENT digest: one is the stable constant that NAMES the file, the other is the value
     re-checked when the file is read; neither is derived from the other. Only `env_corpus_id`
     enters `snapshot_id`; `oracle_env_hash` is value-validated and NEVER in the filename (folding
     it in would be circular). A `oracle_env_hash` recompute mismatch on read means the on-disk
     corpus drifted without re-pinning — exactly the stale-pin case the value revalidation FAILS.
   - **The STABLE `env_corpus_id` IS an input; the per-snapshot `oracle_env_hash` is
     DELIBERATELY EXCLUDED from the id.** `env_corpus_id` is the content id of the
     CLOSED VENDORED oracle-env corpus — a pinned-env constant the registry + pinned env
     know (the canonical corpus is the same for every standalone-host row), so it enters
     the id like `compiler_options_hash` without breaking registry-derivability. The
     FULL-resolved-file-set `oracle_env_hash` is NOT in the id (folding the per-snapshot
     env file list into the filename would be circular — the list lives only INSIDE the
     snapshot). Every OTHER input is readable from the registry + pinned env, so a
     coverage guard derives the EXPECTED filename set from the REGISTRY ALONE — without
     opening any snapshot or running tsgo. The env corpus is validated SEPARATELY on the
     stored VALUE: the consumption test and `no_orphan_snapshot` RE-ENUMERATE the
     vendored corpus directory, assert SET-EQUALITY against the stored manifest (catching
     a newly-ADDED file as well as an edit/delete), then recompute `oracle_env_hash` by
     re-hashing the stored `oracle_env_files` against current on-disk content and FAIL on
     mismatch (stale env). This catches env drift (membership AND content) on the value
     while keeping the filename registry-derivable — closing the chicken-and-egg the
     env-in-filename model created.
   - The ROW-REF (`row_file, row_function, query_ordinal`) is part of the id, so each
     `(row, query)` gets its own file and no two rows can share a snapshot.
   - It is reproducible from the registry entry + workspace content + pinned env, so
     the driver RE-DERIVES the id and reads its snapshot from
     `concat!(env!("CARGO_MANIFEST_DIR"), "/src/typeinfo/typeinfo_tests/oracle_snapshots/", oracle_family, "/", snapshot_id, ".json")`
     via `std::fs::read` without storing a map.
   - The WORKSPACE FILE SET (not a single fixture path) is hashed, so a multi-file
     row is uniquely identified and any contributing file's edit changes the id.
   - `oracle_value_kind` + `query_helper_kind` partition kinds and helpers, so a future
     `relation_verdict` snapshot cannot collide with a `structured_type_expr` one for
     the same row/query.
   - `normalizer_version` AND `probe_synthesis_version` AND `compiler_options_hash` AND
     `env_corpus_id` AND `tsgo_version` AND `oracle_schema_version` all enter the id, so
     any compiler-option / corpus / algorithm / probe / tsgo / schema bump changes every
     affected id → regeneration, never a stale silent compare. (A per-snapshot
     resolved-file-set drift WITHIN the same corpus is additionally caught on the stored
     `oracle_env_hash` value + corpus re-enumeration, above.)
   - `host_project` (the project/workspace/tsconfig/host-kind axes — `standalone` for
     the dominant population, fact 9) enters the id so the same query under a different
     host setup gets a distinct snapshot. Package-backed / custom-host rows are deferred
     (§Scope) unless their env corpus is vendored into `oracle_env_files`.
   - `canonical_type_args` distinguishes `GenericBox<string>` from
     `GenericBox<number>` — two queries on the same symbol get distinct snapshots.

The manifest's `(file, function)` identifies the ROW; the registry's
`(row_file, row_function, query_ordinal)` identifies the QUERY; the shared driver reads
the registry entry, derives `snapshot_id` from it + the pinned env (registry-only,
including the stable `env_corpus_id` — but NOT the per-snapshot `oracle_env_hash`), and
reads `oracle_snapshots/<oracle_family>/<snapshot_id>.json` (under the full
`src/typeinfo/typeinfo_tests/` prefix) via runtime `std::fs::read`, then re-enumerates
the vendored corpus + validates the loaded snapshot's `oracle_env_hash` against the
recomputed value. The registry
carries the executable spec; the ONE new `IgnoredTestRow` field is the declared
`oracle_query_ordinals` count (the independent cross-check, point 2).

### Q5 — Guard scope: lifted rows only, biconditional via the registry (DECIDED)

**Decision: the snapshot-coverage guards scope to LIFTED rows only**
(`status: IgnoreStatus::Lifted { .. }`), NOT all ~340, and they compute the EXPECTED
snapshot set from the REGISTRY (not from the snapshots themselves). SIX guards —
`registry_covers_every_lifted_oracle_query` (forward), `no_orphan_snapshot` (reverse,
set-equality + offline env re-derivation), `registry_entry_count_matches_declared` (the
independent declared cross-check + ordinal contiguity — DEFERRED, pending the new
`oracle_query_ordinals` field),
`registry_family_matches_manifest_oracle_id` (entry-family ≡ proof-family),
`oracle_env_corpus_is_closed` (the vendored-corpus set-equality re-enumeration), and
`raw_capture_matches_oracle_value` (the stored hover re-run through the HOVER-SIDE
lowering + normalization equals the stored `oracle_value`) — these guards, together with
`source_admission_digest_consistent` (the offline SOURCE-SIDE re-derivation guard, defined
later at the §4 guard table), form the closure (so the six-guard list above is NOT
exhaustive). The first two form the registry⋈snapshot
biconditional; the third closes the registry-under-count gap (a registry alone cannot
prove it is not missing an entry); the fourth pins each entry's snapshot sub-directory
to the row's proof family; the fifth closes the env-corpus membership gap (re-enumerate
the vendored dir, catch additions as well as edits/deletes); the sixth ties the stored
`oracle_value` back to the captured TS7 hover so a hand-edited/wrong value cannot pass
strict decode while no longer reflecting the capture; and
`source_admission_digest_consistent` closes the offline hover-vs-source asymmetry by
re-deriving each recorded contributor's source-side admission FROM CURRENT SOURCE (the
half `raw_capture_matches_oracle_value` leaves open, since `raw_capture` stores only the
hover).

Justification against Verter's per-block-lift / demand-driven discipline:

- The project is explicitly **per-block-lift / demand-driven** (CLAUDE.md Build
  Philosophy; the manifest's current state is 355 `Ignored` + 7 `Lifted` = 362 total
  — the first seven rows are seated, the remainder lift block-by-block). Snapshots
  are a rescope-gate deliverable produced block-by-block (§6.3 — "the oracle grows
  … rather than landing as one monolith"). Requiring all snapshots NOW would force
  generating oracle answers for rows whose lifting mechanism does not yet exist —
  dead weight that drifts before its row lifts.
- Therefore FOUR coverage guards together close the registry⋈snapshot loop (and TWO
  further guards — `oracle_env_corpus_is_closed` + `raw_capture_matches_oracle_value` —
  close the env-corpus membership and capture↔value rails on top): the first two assert
  the BICONDITIONAL between the registry's lifted query specs and the on-disk snapshot set
  (using the registry as the expected-set authority), the third independently
  cross-checks the registry entry count + ordinal contiguity against the
  manifest-declared count, and the fourth pins each entry's family to the row's proof:
  - **`registry_covers_every_lifted_oracle_query` (forward — coverage by
    construction, AND the registry→row biconditional)**: every manifest row with
    `status = Lifted` AND `proof = Ts7Oracle(_) | OracleAndGuard{..}` has ≥1 registry entry
    (by `(row_file, row_function, *)`),
    and EVERY such registry entry has an existing snapshot (a divergence row's
    `OracleAndGuard.oracle` field supplies its `OracleId`; its per-query review-gated
    `correction` overlays are
    SEPARATE artifacts governed by `ts-compat-two-mode-model.md` §3, not by the snapshot rail). Re-derives each `snapshot_id` from the spec + pinned env and
    FAILS if any expected snapshot is missing. Because the row body just calls the
    shared driver over its registry entries, this is the coverage-by-construction
    guarantee — a missing query ordinal (e.g. 3 of 4) is caught because the registry,
    not the snapshot dir, defines how many queries the row owns. The relation is
    BICONDITIONAL, not just forward: EVERY registry entry MUST belong to a `Lifted`
    oracle row — i.e. its `(row_file, row_function)` joins to a manifest row that is
    `status = Lifted` AND oracle-bearing. A registry entry whose row is still `Ignored`
    (or non-oracle) is REJECTED: an `Ignored` row owns NO snapshot and its lifted body
    does not yet exist, so a registry entry for it is an unreachable orphan that could
    silently pass the count cross-check (the deferred §Q4 `oracle_query_ordinals` count
    would be `0` for an un-lifted row, so an entry with no snapshot would otherwise go
    undetected). The
    biconditional closes that gap: no registry entry exists for a non-`Lifted`-oracle
    row. Discriminating: a registry entry whose `(row_file, row_function)` is an
    `Ignored` row FAILS this guard.
  - **`no_orphan_snapshot` (reverse — SET-EQUALITY)**: derive `expected_paths` from
    the lifted-oracle-rows ⋈ registry-entries join, recompute the CURRENT workspace
    file content hashes, recompute each `snapshot_id` (registry-only, with the stable
    `env_corpus_id`), and assert SET-EQUALITY against a RECURSIVE on-disk enumeration of
    `oracle_snapshots/<family>/*.json`. Then strictly decode each actual file and verify
    its stored `row_ref` / `snapshot_id` / env-pins (`tsgo_version` /
    `compiler_options_hash` / `env_corpus_id` / `normalizer_version` /
    `oracle_schema_version`) / `oracle_value_kind` / `identity` match the
    registry-derived expectation. Set-equality (not a one-way orphan scan) catches a
    leftover snapshot from an un-lifted row, a fixture-edit orphan, AND a missing file,
    in one comparison.
  - **`oracle_env_corpus_is_closed` (the vendored-corpus membership + content gate)**:
    for every lifted snapshot, RE-ENUMERATE the CLOSED VENDORED oracle-env corpus
    directory (`oracle_env/<env_corpus_id>/`) and assert SET-EQUALITY between the
    directory's CURRENT file listing and the snapshot's stored `oracle_env_files.manifest`
    (no unlisted file present, none missing) BEFORE content-hashing — so a newly-ADDED
    resolution-affecting file under the corpus root is caught (membership) as well as an
    edit/delete (content, caught by the subsequent re-hash). Because tsgo is driven
    against the FROZEN vendored corpus, a developer's `node_modules` change is irrelevant
    to the oracle; the corpus stays hermetic + closed + checked-in (Testing-Hermeticity:
    locally-vendored fixtures only). Discriminating: an unlisted `.d.ts` dropped into the
    corpus dir FAILS set-equality even though every listed file still hashes clean.
  - **`registry_entry_count_matches_declared` (DEFERRED — the independent cross-check — ALL rows,
    including zero-count; pending the new `oracle_query_ordinals` field)**: for EVERY manifest oracle row (NOT only `Lifted` — the
    stronger all-rows form), the number of registry entries (`(row_file, row_function,
    *)`) EQUALS the row's declared `oracle_query_ordinals` count (the new `IgnoredTestRow`
    field, §Q4), AND the registry's `query_ordinal`s for that row are UNIQUE and
    CONTIGUOUS `0..count-1` (no gap, no duplicate, no off-by-one — `{0,1,3}` or `{0,0,1}`
    for a declared 3 FAIL even though the count matches). This INCLUDES the ZERO-COUNT
    rows: a row with declared `oracle_query_ordinals == 0` (every `Ignored` / non-oracle /
    un-lifted row) MUST have ZERO registry entries — a stray entry on a zero-count row
    FAILS (the count-side complement of the `registry_covers_every_lifted_oracle_query`
    biconditional that no registry entry exists for a non-`Lifted`-oracle row). The
    declared count and the registry are two INDEPENDENT sources, so this catches a
    registry that under-counts (3 of 4) — a gap the forward/reverse pair cannot see,
    because both derive their expected set FROM the registry. This is what makes coverage
    genuinely verified, not "true by construction" assumed.
  - **`registry_family_matches_manifest_oracle_id` (family agreement)**: for every
    `Lifted` oracle row, each registry entry's `oracle_family` EQUALS the family carried
    by the manifest row's `ProofRequirement::Ts7Oracle(OracleId)` /
    `OracleAndGuard { oracle, .. }` (a divergence row carries its `OracleId` in the
    `OracleAndGuard.oracle` field)
    (`typeinfo_ignored_test_manifest.rs` `OracleId` / `ProofRequirement`). A
    registry entry that names a different family than the row's proof would read/write the
    wrong `oracle_snapshots/<family>/` sub-directory; this guard pins entry-family ≡
    proof-family.
  - **Offline env re-derivation** is a property of the reverse guard: `no_orphan_snapshot`
    re-enumerates the vendored corpus + recomputes each `oracle_env_hash` by re-hashing
    the snapshot's stored `oracle_env_files.files` list against current on-disk content
    (NOT by re-running tsgo), so the entire coverage closure runs in the default,
    tsgo-free gate. Pinned by `oracle_env_files_redrive_offline`.
  - **`raw_capture_matches_oracle_value` (the captured-hover ↔ stored-value tie)**:
    `raw_capture` is mandatory to re-check the probe header (`probe_header_names_target`),
    but no rail otherwise ties the stored `oracle_value` back to the captured hover — a
    hand-edited or wrong `oracle_value` would pass strict decode (`strict_snapshot_decode`)
    while no longer reflecting the captured TS7 hover. This guard EXTRACTS the probe type
    from the stored `raw_capture` (the `type __oracle_probe__N = <T>;` header / hover
    body), re-runs the FULL HOVER-SIDE pipeline that PRODUCES `oracle_value` on that stored
    hover — the HOVER positive allowlist (default-REJECT) + strict drop-counter + backstop,
    THEN `lower_ts_type` lowering + normalization — and asserts the admitted-and-normalized
    result EQUALS the stored `oracle_value`. It runs OFFLINE (no tsgo — `raw_capture` is the
    stored verbatim hover). Re-running the hover ADMISSION (not just lowering+normalization)
    means a stored hover carrying a REJECTED lossy construct FAILS offline even if it lowers
    to the stored value. Admission is two-sided (the §Q2 positive allowlist is checked on
    BOTH the hover AST AND the fixture SOURCE declaration, transitively through `typeof` /
    `ReturnType`); the SOURCE-side admissibility walk's contributor NAVIGATION is a
    GENERATION-TIME step, NOT an `oracle_value` input and NOT offline-runnable from
    `raw_capture` (which stores only the hover) — the source-side ALLOWLIST is instead
    re-checked offline by `source_admission_digest_consistent`, which RE-PARSES each recorded
    contributor's current source BY CANONICAL PATH through the total canonical-path→source
    mapping (a row/workspace file's source bytes come from the row's REGISTRY `workspace_files`
    payload — the source-byte authority — verified against the snapshot's stored `content_hash`
    for that path; a vendored corpus file's source is on-disk under `oracle_env/<env_corpus_id>/`)
    — `RawSourceSurface` is a parse-time artifact and the lowered body is the deterministic
    `lower_ts_type` — and re-runs the current allowlist over the freshly-captured combined
    `(raw_surface, lowered_body)` pair per `(ordinal, decl_span)`-keyed contributor. This
    guard re-runs the entire HOVER side; that guard re-checks the SOURCE side.
    Discriminating: a snapshot whose `oracle_value` is mutated away from what its
    `raw_capture` lowers to FAILS, even though the mutated value still decodes strictly; a
    stored hover carrying a non-allowlisted construct FAILS the re-run hover allowlist.
  - **Contributor-set MEMBERSHIP — not offline-re-navigable, but CLOSED by a default-reject
    RESTRICTION (moot for the admitted set).** Neither `source_admission_digest_consistent`
    nor any other offline guard re-NAVIGATES the import/merge/transitive declaration graph to
    discover a contributor (or a merged peer) the digest never recorded — WHICH files/peers
    are contributors is established at GENERATION time by the live resolver and is NOT
    offline-reproducible. Rather than carry that as a live residual, initial admission is
    RESTRICTED to PROVABLY SINGLE-CONTRIBUTOR rows (§Scope, `source_is_provably_single_contributor`):
    a single-file declaration with NO import / merge / augmentation / transitive
    `typeof`/`ReturnType` hop, whose contributor set is trivially `{the one decl}`. For that
    admitted set the offline gate is COMPLETE — re-navigation has nothing to discover, and the
    offline gate still catches (a) within-file fact tamper/omission in the ONE recorded
    contributor (re-parse + re-lower + compare, keyed by `(ordinal, decl_span)`) and (b) any
    content change to it (the per-contributor content-hash — a content edit changes the one
    recorded file's hash and misses the warm read). Multi-contributor rows are DEFAULT-REJECTED
    (stay `Ignored`) and deferred to the named offline contributor-set-membership-revalidation
    spike (§4). The "membership not offline-re-navigable" text §Q2's `source_admission_digest`
    field + the `source_admission_digest_consistent` guard both name remains TRUE but is MOOT
    for the admitted single-contributor set; it is recorded once here so the narrowing is
    impossible to overlook.
- The footprint/audit exclusion (§Scope) means a `Lifted` row whose proof is bare
  `Ts7Oracle` MUST NOT also assert non-`TypeExpr` (footprint/audit) behavior; such a
  row is only `Lifted` under `OracleAndGuard`, where the oracle proves the shape and
  the named guard proves the footprint. The forward guard treats `Ts7Oracle(_)` and
  `OracleAndGuard{..}` proofs as oracle-bearing.
- **The divergence case is an `OracleAndGuard` whose `guard` is the `DivergenceCorrection`
  prover, consulting a per-query
  `&[QueryCorrection { query_ordinal, correction, divergence_id }]` overlay
  (the single-spec / correction-overlay model, `ts-compat-two-mode-model.md` §9.2).** A
  registered-divergence row is seated as `OracleAndGuard { oracle, guard }`. Its `oracle` is
  the existing recompute-gated `ts_compat` oracle family (the `OracleId`, unchanged
  machinery) holding the recorded `TsCompat` values; each `QueryCorrection` names one
  corrected `query_ordinal`, its review-gated overlay `correction` (`oracle_corrections/`)
  holding the `Correct` value for that query, and a `divergence_id` resolving to the
  divergence registry. A row may MIX corrected and ordinary queries. **Correction-linkage
  rule.** `DivergenceCorrection` is one of the five `OracleAndGuard` obligation KINDS (§Q4);
  the divergence row's `guard` is the registered `DivergenceCorrection` prover, which runs
  PER corrected `query_ordinal`, asserting that the named correction overlay and a registry
  entry whose id equals `divergence_id` resolve to the SAME `(correction, registry-entry)`,
  proved by the per-query data comparison
  (`resolver(query) == correction.correct_value` while that query's snapshot `oracle_value`
  is the recorded `TsCompat` value and differs). A row asserting NO independent
  non-`TypeExpr` obligation stays bare `Ts7Oracle`; a row asserting one (footprint / audit /
  warm-cache / declared-dependency / divergence-correction) is promoted to `OracleAndGuard`.
  A divergence row counts toward the 362 total and carries no `Relate`
  (projection-divergence rows are not `Relate`-bearing).
- An `Ignored` oracle row has NO snapshot requirement (it is not yet proven). When a
  block lifts a row (`Ignored → Lifted { block_id }`), the SAME block adds the
  registry query specs AND the snapshots (and, once the DEFERRED §Q4 per-row-count
  layer lands, the declared `oracle_query_ordinals` count);
  the guards then enforce presence both ways, entry-family agreement, vendored-corpus
  closure, and the capture↔value tie — plus, when the deferred layer lands, the
  declared-count + contiguity cross-check.
- **Implementation prerequisite (REALIZED) — the work the first `Ignored → Lifted` row
  required, now landed.** Lifting a row was NOT a matter of updating "only THREE
  guards." The realized set is THREE distinct kinds of work, all now in the tree: (1) the
  regenerator's SOURCE MODEL was redesigned (it now carries `LIFTED_ROW_OVERRIDES` and
  emits each row's `status`), (2) the `ignored_test_row_table_holds_exactly_362_rows`
  count-table guard was reconciled (total `.len() == 362`, live-ignore count `== 358`),
  and (3) THREE all-row-sensitive manifest guards were status-filtered. There are FOUR
  all-row-sensitive manifest guards in total, but only THREE needed edits — the fourth
  (the `EXPECTED_TOTAL_IGNORED_COUNT` COUNT guard, `:595`) was ALREADY status-filtered via
  `count_ignored_rows`. The set below records what landed:

  1. **The regenerator source-model redesign (a `Lifted` row SURVIVES
     regeneration).** `scripts/gen-typeinfo-ignore-manifest.py` would, in the original
     all-`Ignored` model, build the row table EXCLUSIVELY from live `#[ignore]` discovery
     and hardcode `status: IgnoreStatus::Ignored` — under which a lifted row (carrying NO
     live `#[ignore]`) would VANISH on regeneration and the `!= 362` build assertion would
     fail (361 discovered). The regenerator now UNIONS live discovery with a retained
     `Lifted`-row ledger so a `Lifted` row stays in the table with `status: Lifted { block_id }`
     despite having no live `#[ignore]`:
     - A **retained-lift metadata map** — a checked-in source-of-truth list
       (`LIFTED_ROW_OVERRIDES` in the generator's row-spec source, alongside the
       §10.4.1 partition the generator already reads). It retains the FULL row record for
       each lifted row, NOT just `(file, function, block_id)` — because once a row is lifted
       its `#[ignore]` is GONE and live discovery can no longer supply ANY of the 13 columns
       the generator previously scraped from the `#[ignore]` site, yet downstream guards
       still read those columns on EVERY row (status-independent). In particular
       `every_manifest_row_has_non_empty_unblocker`
       (`typeinfo_ignored_test_manifest.rs:852`) iterates ALL rows and asserts each has a
       non-empty `unblocker`, and the generator today sources `unblocker` from the live
       `#[ignore = "…"]` text (`:1096`) — which is removed on lift. The retained-lift
       metadata therefore stores the COMPLETE `IgnoredTestRow` payload so the regenerated
       table reproduces the lifted row VERBATIM, only with `status: Lifted { block_id }`
       substituted. The retained schema is the FULL 13-column `IgnoredTestRow` record. The
       `proof` column is one of `Ts7Oracle(_)` | `OracleAndGuard{..}` (a divergence row is
       an `OracleAndGuard` whose `guard` is the `DivergenceCorrection` prover) — there is NO
       stored `non_typeexpr_obligations` ledger field; an independent non-`TypeExpr`
       obligation is expressed by the `OracleAndGuard` proof shape + its registered prover,
       not a typed set on a record. The `block_id` column is INTENTIONALLY IDENTICAL to the
       `IgnoreStatus::Lifted { block_id }` status payload the regenerated row carries — the
       lifting block IS the row's block; the regenerator emits `status = Lifted { block_id:
       rec.block_id }` and the `lifted_row_block_id_matches_status` guard asserts the
       equality so they never drift.

       The retained-lift metadata is the authority for BOTH `status` AND every retained
       column of a lifted row, NOT a hardcoded constant and NOT a re-scrape of a now-absent
       `#[ignore]`. (The round-2 full-record obligation ledger — a `LiftedRowRecord` storing
       a typed `non_typeexpr_obligations` set — was retired. The deferred
       `migration_fingerprint` / `original_body_tokens` body-hash fidelity layer, §Q4, would
       ride on this same retained-lift metadata when wired — it is a GENUINELY DEFERRED
       TODO, not yet implemented.)
     - The generator's row set becomes the **UNION** of live-`#[ignore]` discovery (rows
       still `Ignored`, columns scraped from the live `#[ignore]` site as today) and the
       retained `Lifted`-row ledger (rows now `Lifted`, columns sourced from the ledger
       record). A row in the ledger is emitted with `status: IgnoreStatus::Lifted {
       block_id: rec.block_id }` (the status payload's `block_id` IS the row's own
       `block_id` manifest column — they are INTENTIONALLY the same value, asserted equal by
       `lifted_row_block_id_matches_status`) and every other column taken from its
       retained-lift metadata record, and is NOT
       expected to have a live `#[ignore]`; a row in live discovery is emitted with
       `status: IgnoreStatus::Ignored` and columns from the `#[ignore]` site. The two
       sets are DISJOINT by construction (a row is either still `#[ignore]`d OR lifted,
       never both) — the generator asserts this disjointness and FAILS if a
       `(file, function)` appears in both (a lift that forgot to remove the `#[ignore]`,
       or a ledger entry whose `#[ignore]` came back).
     - `status` AND the retained columns are sourced FROM the retained-lift metadata,
       never from the hardcoded `IgnoreStatus::Ignored` literal in `emit_ignored_rows`
       (replaced by a per-row status derived from `(file, function) ∈ LIFTED_ROW_OVERRIDES`)
       and never from a re-scrape of the removed `#[ignore]` text. Because the retained-lift
       metadata supplies a NON-EMPTY `unblocker` for every lifted row,
       `every_manifest_row_has_non_empty_unblocker` (`:852`) STILL APPLIES to `Lifted` rows
       unchanged and is NOT status-filtered — the retained-lift metadata is what keeps the
       all-rows `unblocker` invariant satisfiable after lift. The retained record carries
       all 13 `IgnoredTestRow` columns with a non-empty `unblocker` and a `proof` of
       `Ts7Oracle(_)` | `OracleAndGuard{..}` (a divergence row is an `OracleAndGuard` whose
       `guard` is the `DivergenceCorrection` prover) — there is NO stored
       `non_typeexpr_obligations` set: an independent non-`TypeExpr` obligation is expressed
       by the `OracleAndGuard` proof shape + its registered prover, so a regenerated lifted
       row cannot drop a column the all-rows guards read NOR demote a footprint/audit/
       divergence row to bare `Ts7Oracle`.

     - **Realized first-lift state — the four lifts are bare `Ts7Oracle`, verified in
       their original `Expanded` mode; no obligation ledger.** The four first-lifted rows
       (two index-signature publication + two built-in modifier-utility) carried ONE
       non-`TypeExpr` assertion in their original bodies: `assert_query_mode(Expanded)`. A
       same-mode `assert_query_mode` that matches the oracle query's own `projection_mode`
       is oracle query IDENTITY, **not** a §Q4 `non_typeexpr_obligation`: the driver
       resolves Verter's projection in that mode and the live audit record reports that
       mode, which IS the proof (stronger than duplicating the mode into a ledger). The
       four rows are therefore seated with `projection_mode: Expanded` (`oracle_query_specs.rs`),
       their tsgo snapshots captured + compared in `Expanded`, and the query-mode identity
       is proven live by `lifted_row_audit_query_mode_matches_spec`
       (`tests/typeinfo_ignored_test_manifest.rs`) — which asserts every registry query's
       live audit `query_mode` equals its spec's declared `projection_mode`. They stay bare
       `ProofRequirement::Ts7Oracle` (no `OracleAndGuard`, no obligation set). The only
       genuinely-deferred fidelity is the cryptographic `migration_fingerprint` +
       `original_body_tokens` body-hash artifact (a `syn`-AST lift-time auto-extractor that
       would catch a fully self-consistent wrong (spec ∧ snapshot) pair) — DEFERRED to the
       §4 migration-ledger spike.
     - The build-count assertion changes from "exactly 362 discovered" to "exactly 362
       rows TOTAL in the union (discovered-Ignored ∪ ledger-Lifted)", so the table
       length stays 362 as rows migrate from the discovered set to the ledger. The
       count of `Ignored` rows DECREASES as lifts accrue; the TOTAL row count stays 362.

  2. **The `ignored_test_row_table_holds_exactly_362_rows` count-table guard
     reconciliation (`:1155`).** This guard asserts BOTH
     `EXPECTED_IGNORE_MANIFEST.len() == 362` (the RAW table length, `:1157`) AND
     `EXPECTED_TOTAL_IGNORED_COUNT == 362` (the status-filtered `Ignored` count,
     `:1171`). With the regenerator redesign above, the raw `.len()` STAYS 362 (a lifted
     row remains in the table as a `Lifted` row), so the FIRST assertion holds unchanged
     — this is the architectural reason the regenerator must retain lifted rows rather
     than drop them. But the SECOND assertion (`EXPECTED_TOTAL_IGNORED_COUNT == 362`)
     BREAKS the moment any row becomes `Lifted` (the `Ignored` count drops below 362).
     The guard MUST be reconciled to keep the binding TOTAL pinned at 362 while letting
     the `Ignored` count fall: assert `EXPECTED_IGNORE_MANIFEST.len() == 362` (the total
     row count — UNCHANGED) AND `EXPECTED_TOTAL_IGNORED_COUNT == 362 - lifted_count`
     (the `Ignored` count equals 362 minus the number of `Lifted` rows), where
     `lifted_count` is `EXPECTED_IGNORE_MANIFEST` rows with `status == Lifted`. This
     keeps the guard discriminating (a spuriously-added/dropped row, or a row silently
     flipped to `Lifted` without a ledger entry, still FAILS) while admitting the lift.

  3. **THREE of the FOUR all-row-sensitive manifest guards must be status-filtered (the
     fourth is already filtered).** Of the four all-row-sensitive manifest guards, the
     COUNT guard `EXPECTED_TOTAL_IGNORED_COUNT = count_ignored_rows(EXPECTED_IGNORE_MANIFEST)`
     (`:595`) is ALREADY correctly status-filtered — `count_ignored_rows` counts only
     `status == IgnoreStatus::Ignored` rows (`:566-575`/`:561`), so it decrements
     automatically and needs NO edit. The other THREE all-row-sensitive guards DO need
     the lift update:
     - `manifest_length_matches_documented_total` (`:989`) asserts
       `EXPECTED_IGNORE_MANIFEST.len() == EXPECTED_TOTAL_IGNORED_COUNT` — the raw
       `.len()` (362, all rows) vs the status-filtered `Ignored` count (< 362 after a
       lift). The equality breaks; it must compare the status-filtered `Ignored` count
       on BOTH sides (count `Ignored` rows in the table == `EXPECTED_TOTAL_IGNORED_COUNT`).
     - `every_manifest_row_corresponds_to_a_live_ignored_test` (the orphan check, `:828`)
       iterates EVERY row and treats a row WITHOUT a live `#[ignore]` as an ORPHAN — a
       `Lifted` row (whose `#[ignore]` was removed) would falsely register as an orphan
       unless `Lifted` rows are EXCLUDED from the orphan iteration.
     - `per_file_ignored_test_counts_match_manifest` (`:1018`) asserts a PER-FILE
       partition where the expected per-file count tallies EVERY row regardless of
       status — a lifted row whose `#[ignore]` was removed no longer has a live
       `#[ignore]` to count, so the partition must be built over `status == Ignored`
       rows only.

  So lifting a row is gated on, IN THE SAME BLOCK: (a) redesigning the regenerator to
  union live discovery with a retained `Lifted`-row ledger and source `status` from the
  ledger (not a hardcoded constant), keeping the TOTAL at 362; (b) reconciling
  `ignored_test_row_table_holds_exactly_362_rows` so the total stays 362 while the
  `Ignored` count is `362 - lifted_count`; and (c) status-filtering the THREE
  all-row-sensitive guards (length, orphan, per-file partition) over `status == Ignored`
  rows. The status-filtered COUNT guard (`:595`) needs no edit. This is a hard
  implementation prerequisite, not optional cleanup. The earlier "only THREE guards"
  framing was INACCURATE — the regenerator source-model and the 362-table guard also
  require changes, and the prerequisite set above is the complete, accurate list.

This is the architecturally consistent choice: snapshots are demand-driven, the
registry is the machine-readable expected-set authority, the row body is a thin driver
call so coverage is by construction, and the set-equality is a true cross-check (not a
snapshot validated against itself).

---

## 3. Invariants

> **Reframed by the single-spec / correction-overlay model (additive — these invariants
> are otherwise intact).** Every invariant below holds for the **`ts_compat`** oracle and
> the no-correction case. The correction-overlay model layers ON TOP, without weakening
> any of them: (a) invariant 6's `oracle_value` is the recorded **`TsCompat`** value; the
> **correct** answer for a divergence row is a separate review-gated correction overlay,
> never a second snapshot field (so invariants 1–5's recompute-gated snapshot integrity is
> untouched); (b) invariant 1's "tsgo forbidden at query time" is unchanged and
> strengthened — the resolver is single-spec, has no compat mode, and never shells to
> tsgo; tsgo stays generation-only; (c) the divergence proof
> (`ProofRequirement::OracleAndGuard` with the `DivergenceCorrection` prover) and its data
> comparison are NEW harness
> checks, not changes to these invariants. The harness runs the single-spec resolver ONCE
> per query (corrections bind at `(row, query_ordinal)` granularity): a corrected query
> asserts `resolver(query) == correction.correct_value` (while that query's
> `snapshot.oracle_value` is the recorded `TsCompat` value and differs); an ordinary query
> asserts `resolver(query) == snapshot.oracle_value`. There is no per-mode
> re-run and no family-key comparison — the resolver carries no spec dimension. See
> `ts-compat-two-mode-model.md`.

1. **`tsgo` is forbidden at query time.** The resolver
   (`resolve_named_symbol_with_audit` → `project_node_to_type_expr` →
   `ProjectSemanticDispatch`) must never spawn or contact tsgo. Already true by
   crate-graph construction: `verter_session` has no `verter_type_runtime`
   dependency (fact 4). The harness must NOT introduce such a dependency on the
   consumption path. The snapshot GENERATOR (which does use tsgo) lives in a separate
   dev-only / feature-gated tool target (`#[cfg(feature = "oracle-gen")]`), never in
   the default resolver build. **Guard:** `tsgo_not_reachable_from_resolver`.

2. **Default tests never invoke tsgo.** Lifted rows load checked-in snapshots only;
   regeneration is feature/env-gated. **Guard:**
   `oracle_consumption_path_has_no_tsgo_spawn`.

3. **Snapshots are content-addressed by a REGISTRY-DERIVABLE filename + FULL-env-pinned
   on the VALUE.** The `snapshot_id` (filename) derives from REGISTRY-ONLY, tsgo-free
   inputs — row-ref, `query_helper_kind` + payload, `host_project`,
   `compiler_options_hash`, the STABLE `env_corpus_id` (the closed-vendored-corpus
   content id, a pinned-env constant), `tsgo_version`, `oracle_schema_version`,
   `normalizer_version`, `probe_synthesis_version` — and is the FULL (≥256-bit) BLAKE3
   digest. The per-snapshot `oracle_env_hash` (the FULL resolved-file-set hash) is
   DELIBERATELY NOT a `snapshot_id` input (that would be circular); it is STORED and
   validated as a VALUE on read. A snapshot is valid only when its `tsgo_version`,
   `compiler_options_hash`, `env_corpus_id`, `normalizer_version`,
   `probe_synthesis_version`, `oracle_schema_version`, and re-derived `snapshot_id`
   match the pinned env (Q4) AND its vendored corpus re-enumerates SET-EQUAL to the
   stored `oracle_env_files.manifest` AND its stored `oracle_env_hash` re-hashes EQUAL
   from `oracle_env_files.files` against current on-disk content (the closed-vendored-
   corpus pin, fact 8). A `compiler_options_hash` match ALONE does not validate a
   snapshot — the corpus set-equality + recomputed `oracle_env_hash` must also match.
   Any mismatch is a hard failure, never a silent stale compare. **Guards:**
   `snapshot_env_pin_matches_workspace`, `snapshot_id_redrives_from_identity`,
   `snapshot_id_is_unique`, `tsgo_version_is_pinned`, `normalizer_version_in_snapshot_id`,
   `oracle_env_hash_pins_resolved_file_set`, `oracle_env_corpus_is_closed`,
   `oracle_env_single_current_corpus`,
   `probe_form_is_deterministic_and_versioned`, `snapshot_id_includes_row_ref`,
   `compiler_options_hash_is_closed`, `canonical_encoding_is_pinned`.

4. **Comparison is structural, never text; cosmetic-name canonicalization is
   DEFAULT-SAFE.** The lift compares normalized `TypeExpr` JSON, not display strings
   (§Q2 reduction). `raw_capture` text is debug-only and never asserted. The
   normalization reduction canonicalizes a CLOSED ENUMERATED set of cosmetic,
   non-identity-bearing binder/parameter names — index-signature parameter name,
   function/method/call/construct-signature parameter names, generic type-parameter
   names (resolved at use sites via SCOPE-TRACKED binder resolution so a `Ref` binds to
   the enclosing binder, never an unrelated same-named alias), and mapped-key + `infer`
   names — to deterministic positional placeholders (§Q2 step 6), since TS ignores
   those names for type identity. The cosmetic axis is DEFAULT-SAFE, symmetric to the
   admission allowlist's default-REJECT: an un-enumerated identity-bearing name axis,
   and the template-literal cosmetic axis (un-canonicalized until the spike), REJECT
   the `(row, query)` rather than passing the raw name through — so a missed cosmetic
   axis cannot become a silent false divergence. This is a structural rename that
   forecloses the cosmetic-name false-divergence class without masking any real
   member/type difference, and without ASSERTING completeness (the provable property is
   "every name axis is either canonicalized or rejected"). The bounded literal-
   subsumption rule (§Q2 step 5) is semantics-preserving and held to a discriminating
   guard proving it does not over-collapse a literal-only union. The SOUNDNESS property
   of the reduction is **CONFLUENCE over the admissible set** — `a ≡ b ⟹ normalize(a)
   == normalize(b)` for differently-spelled equal inputs (§Q2 "Confluence is the
   soundness property"); idempotence is necessary but INSUFFICIENT. The boolean rule is
   the CO-PRESENCE form (`{true,false} ⊆ arms ⟹ {boolean} ∪ rest`, confluent over the
   ≥3-arm case), and any admitted construct whose two-sided spelling is not proven
   confluent under the closed neutral-element / absorption / canonicalization rule set
   is REJECTED (default-safe confluence posture). The positional type-parameter rename
   is admissible only when the binder list/order is PROVEN cross-side-stable (no tsgo
   reordering/omission), else the construct is rejected. **Guards:**
   `oracle_normalization_canonicalizes_cosmetic_names`,
   `oracle_literal_subsumption_discriminates`, `oracle_normalization_is_confluent`,
   `oracle_normalization_is_idempotent`, `binder_order_is_cross_side_stable`.

5. **Snapshots are demand-driven, joined through the registry in `src`, and query
   coverage is by construction AND verified.** Query payloads live in the
   `src/typeinfo/typeinfo_tests/oracle_query_specs.rs` registry (reachable by the
   lifted unit tests), not the test body; a lifted row carries the `#[oracle_row]`
   attribute proc-macro, which reads the ENCLOSING fn's own `ItemFn` identifier as the row
   key (NO hand-typed key string — a `foo_test` cannot name `bar_test`'s key, foreclosing
   wrong-row execution; a no-arg declarative macro is infeasible because a body-position
   macro cannot see the enclosing fn name, §Q4) and synthesizes a body that calls the
   shared registry driver (basename-normalizing
   `file!()` via `Path::file_name()`, §Q4). Each entry carries `oracle_family` so the
   driver can name the snapshot sub-directory. A snapshot exists iff its registry query
   spec belongs to a `Lifted` row (Q5). Coverage is NOT "true by construction" from the
   registry alone — the DEFERRED §Q4 `IgnoredTestRow.oracle_query_ordinals` declared
   count (not yet a shipped field) would be an INDEPENDENT cross-check against the
   registry entry count, catching a registry that under-counts. Per-block lift adds the
   registry specs and the snapshots together (and, once the deferred layer lands, the
   declared count). **Guards:** `registry_covers_every_lifted_oracle_query`,
   `no_orphan_snapshot`, `registry_entry_count_matches_declared` (DEFERRED — pending the
   new `oracle_query_ordinals` field), `lifted_body_is_self_keyed_macro`.

6. **The oracle value reconciles with the IN-PROCESS output, not the wire.** The
   snapshot's `oracle_value` is a `TypeExpr`-shaped normal form (hover text lowered to
   `TypeExpr`), comparable to the `TypeExpr` returned by `resolve_expr`
   (`support.rs:132`). It is NOT a `TypeInfoGraphPayload` / wire DTO. The
   `oracle_value_kind` field is the documented extension point for future
   non-`TypeExpr` kinds (`relation_verdict`, `call_resolution_verdict`, a wire-parity
   kind), each with its own structured source — out of scope. The hover-lowered path
   is exactly the `"structured_type_expr"` kind. **Guard:**
   `oracle_value_decodes_to_type_expr_strict`.

7. **Eligibility is structural, scoped to `TypeExpr` projections, and the
   footprint/audit exclusion reads a MACHINE-READABLE flag.** A query is
   lifted via this harness ONLY when its assertion is a `TypeExpr` projection
   (`oracle_value_kind == structured_type_expr`) AND it does NOT assert a relation /
   call verdict; `SemanticQueryName::Relate` as the asserting key is a HARD
   ineligibility signal, AND — enforcing the 122-row `Relate`-free CEILING — a row whose
   manifest `semantic_queries` CONTAINS `SemanticQueryName::Relate` ANYWHERE (not only as
   the asserting key) is REJECTED, until/unless a future oracle kind explicitly owns the
   relation path. A non-`Relate` row that ALSO asserts non-`TypeExpr`
   obligations — dependency-footprint / audit-record / query-mode behavior (e.g.
   `flow_return_xf04_records_barrel_route_before_selected_leaf`,
   `flow_return_catalog.rs:1496`) — is PROMOTED to
   `ProofRequirement::OracleAndGuard { oracle, guard }` whose `guard` names the registered
   live prover for that obligation KIND (one of the five kinds: `DependencyFootprint` /
   `AuditRecord` / `WarmCache` / `DeclaredDependency` / `DivergenceCorrection`). There is NO
   stored typed obligation SET on a ledger record — the obligation is expressed by the proof
   shape + its registered prover.
   `kind_eligibility_gate` REJECTS
   such a row under a BARE `Ts7Oracle` proof (the shape compare cannot prove the
   obligation). It is admissible only under `ProofRequirement::OracleAndGuard` (a divergence
   row is an `OracleAndGuard` whose `guard` is the `DivergenceCorrection` prover, §Q4 /
   `ts-compat-two-mode-model.md` §9.2, running per corrected query). The
   gate verifies the proof's `guard` resolves to an entry in the
   checked-in CODE `OBLIGATION_GUARD_REGISTRY` (`GuardId → prover fn` — the §4 guard table
   is its human-readable mirror) — so the rule is a discriminating check over the proof
   shape read against compiled `fn` symbols, not a prose stub. **Guard:**
   `kind_eligibility_gate`.

8. **Hover admission is a PRE-LOWERING POSITIVE ALLOWLIST (default-REJECT), checked
   TWO-SIDED on the hover AST AND the source declaration.** The gate runs on the RAW
   PARSED AST BEFORE `lower_ts_type`, walking the closed POSITIVE allowlist (§Q2) over
   BOTH the hover AST AND the fixture SOURCE declaration: any construct not on the
   allowlist — on either side — REJECTS, and admission requires BOTH sides clean (a
   hover can hide an overload / private / accessor that the source carries). The
   source-side input is the COMBINED per-contributor `(RawSourceSurface raw facts,
   already-lowered body `TypeExpr`)` pair — raw facts for the SILENTLY-ERASED constructs
   (`unique symbol`, computed/symbol keys, visibility, accessors, `abstract`, `const`/
   variance type-param, `this`, `as const`, overload set), the lowered body for the
   NON-erased rejectable `TypeExpr` variants (`Conditional` / `Mapped` / callable /
   `TemplateLiteral` / `Infer` / `KeyOf` / `IndexedAccess` / `TypeOf` / enum-member `Ref`
   / `RecursiveRef`); a merged symbol resolves to an ORDERED, KEYED contributor vector
   (`(ordinal, decl_span)` per peer) each checked independently. It then
   lowers with a strict drop-counter that fails on ANY dropped member/param. A
   post-lowering check is unsound — OXC has already silently erased the lossy construct
   (the branded `IdBranded<T>` / `unique symbol` member, `oxc/lib.rs:171,99,921`) so it
   can no longer be seen or rejected. The source-side walk is TRANSITIVE through
   `typeof` / `ReturnType` / `Parameters` to the referenced value initializer /
   function body (§Q2) — a `typeof objectConst` over an `as const` object carries its
   lossy facts in the initializer, not the alias node. The transitive walk carries a
   VISITED-SET + cycle→REJECT guard (a cyclic source defers, never hangs) and inherits
   the shared resolver's `RefCycleResultDb` termination guarantee. The four backstop
   reject rules (ellipsis/truncation, parse leftovers / `Unknown`, unexpected
   `any`/`never`, unexpanded userland `Ref` in `Expanded` mode) remain on top.
   **Guards:** `pre_lowering_loss_rejected`, `hover_construct_whitelist`,
   `source_declaration_allowlist_clean`, `source_walk_is_transitive_through_typeof`,
   `source_walk_cycle_rejected`,
   `strict_lowering_drop_counter`, `hover_capture_is_lossless_or_rejected`.

9. **Class-member visibility + accessors are hover-unrepresentable and rejected.**
   `MemberVisibility` participates in `TypeExpr` identity (`lib.rs:494,503`) and the
   JSON emits non-public visibility (`type_expr_json.rs:473,504`), but OXC type-literal
   lowering stamps every member PUBLIC (`with_spans_public`, `oxc/lib.rs:427`), and
   getter/setter accessors are NOT an `ObjectMember` variant (`lib.rs:426`). So a
   `private`/`protected` member or an accessor cannot be represented from hover; both
   are in the REJECT/defer set and need a future structured oracle. **Guard:**
   `class_visibility_accessor_rejected`.

10. **Probe fencing — the probe is FIXED + VERSIONED + placed in the query's own
    resolution environment.** A snapshot is generated ONLY when the hover header names the
    synthesized probe `__oracle_probe__N` (the wrong-position fence, since `get_hover`
    returns no range, fact 3, `ipc.rs:1689`). The probe is placed so it sees the identical
    lookup environment Verter resolved in: a SAME-FILE append into the query's
    `primary_canonical` file for `ResolveExpr` / `ShallowSurfaceExpr` (`support.rs:132,160`),
    and a SEPARATE scratch file = the scope's `eval_source` prelude + the trailing probe
    for `EvaluateExpr` (mirroring `evaluate_type_expression.rs:~314`, `support.rs:208`) —
    so the probe sees the identical file-local scope (non-exported locals /
    file-local imports) Verter resolved in. The generator verifies the probe's RHS binds
    to the registry's intended declaration (not a shadow/ambient) via a tsgo
    definition/diagnostic check; a mismatch fails generation. The probe has deterministic
    naming (`__oracle_probe__<query_ordinal>`), is constructed per `query_helper_kind`
    (§Q2), and is VERSIONED by `probe_synthesis_version` so `snapshot_id` is re-derivable
    without tsgo. The `Expanded`-mode probe FORM stays spike-blocking (admission deferred
    until the spike fixes + versions it). Admissibility is a per-row / per-query decision,
    never per-family; a query that fails stays `Ignored` and waits for a future
    structured oracle kind. For a parameterized `ResolveExpr` (non-empty `type_args`)
    the probe RHS is synthesized via the versioned `TypeExpr` → TS-source printer (§Q2);
    the `<T>` is extracted from the hover via the versioned hover-extraction
    grammar (§Q2). **Guards:** `probe_header_names_target`,
    `probe_binds_to_registry_target`, `probe_form_is_deterministic_and_versioned`,
    `parameterized_probe_rhs_synthesis`, `hover_extraction_grammar_is_versioned`,
    `non_admissible_query_not_lifted_via_hover`.

11. **Snapshots load via runtime `std::fs::read` from the FULL `src` prefix, NOT
    `include_str!`.** The snapshot path is derived at test time (`snapshot_id` depends
    on current fixture/env hashes) so it cannot be a macro argument; the driver reads
    the bytes via `std::fs::read` from
    `concat!(env!("CARGO_MANIFEST_DIR"), "/src/typeinfo/typeinfo_tests/oracle_snapshots/", oracle_family, "/", snapshot_id, ".json")`
    — the full `src/typeinfo/typeinfo_tests/` infix is REQUIRED (`CARGO_MANIFEST_DIR`
    alone is `crates/verter_session/`). No `include_str!`, `include_bytes!`, generated
    include table, or `include_dir!` — a second embedded artifact would be a shadow
    registry. **Guard:** `snapshot_loading_is_runtime_fs`.

12. **`identity` is kind-specific; a new oracle kind bumps the schema.** `identity` is
    a closed tagged shape keyed by `oracle_value_kind`. The
    `structured_type_expr` axes (helper-kind, workspace files, symbol/expression,
    type-args, mode, `host_project`, probe locator) are NOT a future kind's axes; adding
    a new `oracle_value_kind` requires an `oracle_schema_version` bump. **Guard:**
    `identity_is_kind_specific_schema_bumped`.

13. **The tsgo toolchain is pinned.** `package.json` must pin
    `@typescript/native-preview` to exactly `7.0.0-dev.20260526.1` (not `"latest"`).
    This is a docs-only design that PRESCRIBES the pin as an implementation step.
    **Guard:** `tsgo_version_is_pinned`.

14. **Snapshot decode is strict.** `type_expr_from_json` silently drops malformed
    members/params via `filter_map` (`type_expr_json.rs:72,336`), so a permissive
    "decodes without `Unknown`" check would NOT catch silent member loss. The
    consumption-side decode + the generator's audit decode MUST be strict — a
    decode→re-encode→byte-equality round-trip that FAILS on any dropped/unknown
    member. **Guard:** `strict_snapshot_decode`.

15. **Snapshot validity pins the CLOSED VENDORED corpus on the VALUE, not just compiler
    options; the corpus is hermetic + closed.** A TS7 hover answer depends on the ambient
    / lib / package `.d.ts` corpus AND the resolution metadata (package manifests,
    tsconfig/project metadata) the query resolves through (fact 8 —
    `env_hash.rs:84,99,219,239`, `ipc.rs:3651`, `ipc.rs:3686`, `cache_invalidation.rs:324`),
    not only the compiler options — and tsgo bundles its libs under
    `node_modules/@typescript/native-preview-*/lib/` (`ipc.rs:~2859-2874`), which is
    gitignored (`.gitignore:9`). The harness therefore VENDORS the closed oracle-env
    corpus (copies the bytes into a checked-in `oracle_env/<env_corpus_id>/` dir) and
    drives tsgo against THAT frozen root, never live `node_modules`. The
    `oracle_env_hash` content-hashes every resolution-affecting file in the vendored
    corpus; the stored `oracle_env_files.manifest` is the complete directory listing. The
    per-snapshot `oracle_env_hash` is NOT a `snapshot_id` input (that would be circular,
    §Q4) — it is validated on the stored VALUE: the consumption test and
    `no_orphan_snapshot` RE-ENUMERATE the vendored corpus, assert SET-EQUALITY against the
    stored manifest (catching a newly-ADDED file), THEN recompute `oracle_env_hash` from
    the stored `oracle_env_files.files` against current on-disk content and FAIL on
    mismatch, so an ambient / lib / package / manifest / project change (edit, add, or
    delete) invalidates the snapshot even when the compiler options are unchanged. The
    STABLE `env_corpus_id` (the closed-corpus content id) is what enters `snapshot_id`.
    COMPLETENESS (every file tsgo consulted was vendored) is a SEPARATE property from
    STABILITY (the set still set-equals the manifest): completeness is enforced at
    GENERATION time by driving tsgo against ONLY the frozen corpus root (no live
    `node_modules` / ambient fallback), so an un-vendored resolution-affecting file
    becomes a generation-time resolution failure / diagnostic that FAILS generation
    (§Q2 "Corpus COMPLETENESS"). The stored `oracle_env_files.manifest` and `.files[].path`
    sets are asserted EQUAL (sorted, duplicate-free) BEFORE any hashing, so no corpus member
    is in the manifest/dir yet excluded from the content-hash list. **Guards:**
    `oracle_env_hash_pins_resolved_file_set`,
    `env_corpus_includes_resolution_metadata`, `oracle_env_corpus_is_closed` (STABILITY),
    `oracle_env_corpus_is_complete` (COMPLETENESS), `oracle_env_files_manifest_matches_files`.

16. **Query coverage is cross-checked by an independent declared count + ordinal
    contiguity.** A registry-driven driver + guard cannot see a registry that
    under-counts (3 entries for a 4-query row), so coverage is NOT verified by the
    registry alone. The new `IgnoredTestRow.oracle_query_ordinals` field declares the
    query count independently; the count is cross-checked against the registry entry
    count per lifted oracle row AND the registry's ordinals must be UNIQUE + CONTIGUOUS
    `0..count-1`. Cardinality + contiguity verify the COUNT, not the registry PAYLOAD's
    fidelity to the original hand-authored query; a lift-time `migration_fingerprint`
    (recorded in the retained-lift metadata from the original body BEFORE it is replaced —
    a GENUINELY DEFERRED TODO) is re-asserted
    against the registry payload so a wrong-symbol/canonical/mode migration FAILS.
    **Guards (both DEFERRED — pending the new `oracle_query_ordinals` field /
    `migration_fingerprint`):** `registry_entry_count_matches_declared`,
    `registry_payload_matches_migration_fingerprint`.

17. **The PARITY CLAIM is structural `TypeExpr`-projection, NOT nominal/semantic
    identity.** This harness asserts that Verter's in-process `TypeExpr` projection
    equals the `TypeExpr` lowered from TS7's hover (§Scope) — both sides normalized
    `TypeExpr`. It does NOT assert full TypeScript nominal/semantic identity. A construct
    whose meaning lives in nominal identity `TypeExpr` cannot carry (enum-member brand,
    `unique symbol`, private/protected brand, `this`-type, `const`/variance type-param,
    abstract-ctor) is out of scope for hover sourcing and DEFAULT-REJECTED; such a
    legitimate structural divergence is DEFERRED, never force-fit. **Guard:**
    `parity_claim_is_structural_type_expr`.

18. **Default-REJECT is the rule; the reject list is illustrative.** The §Q2 construct
    gate is a CLOSED POSITIVE ALLOWLIST. The enumerated REJECT entries are EXAMPLES, not
    an exhaustive catalogue — ANY construct not on the positive ADMIT list (named or
    unnamed, present or future) is rejected by default. The gate's soundness does NOT
    depend on enumerating every lossy construct; an un-enumerated lossy construct is
    already handled (rejected). **Guard:** `default_reject_is_the_rule`.

19. **Enum-member types are rejected (no `TypeExpr` carrier).** An enum-member type
    (`Color.Red`, `Status.Idle`, `Direction.Up`; `fixtures/enums.ts:21,26`; the branded
    contracts `enums.rs:18,39`) is a nominal brand `TypeExpr` cannot represent (the enum
    closed at `lib.rs:128` has no enum-member variant). An enum-member `Ref` is not on
    the positive ADMIT list, so it falls through to DEFAULT-REJECT. **Guard:**
    `enum_member_refs_rejected`.

20. **`any` / `never` answers and package-backed / custom-host rows are deferred
    classes.** Rows with a genuine `any` / `never` answer (`Parameters<any>`,
    `Awaited<never>`, …, `typeinfo_ignored_test_manifest_rows.rs:340,343,347`) are
    permanently INELIGIBLE for this hover harness (the backstop rejects `any` always /
    `never` mostly; they stay strict, not weakened). Package-backed / custom-host rows
    (`make_package_host_with_workspace`, `cache_invalidation.rs:344`) are ineligible
    unless their full consulted `.d.ts` corpus is vendored into `oracle_env_files`; the
    standard host's project/workspace/tsconfig axes ARE pinned in `identity` +
    `snapshot_id` (`host_project`). **Guard:** `deferred_classes_not_lifted_via_hover`.

21. **The probe is placed in the query's own resolution environment and binds to the
    registry target.** The synthesized probe is placed per `query_helper_kind`: a same-file
    append into the query's `primary_canonical` file for `ResolveExpr` / `ShallowSurfaceExpr`,
    and a scratch file + `eval_source` prelude for `EvaluateExpr` (`support.rs:132,160,208`),
    so it sees the identical file-local scope Verter resolved in; the generator proves the
    probe's RHS binds to the intended declaration (not a shadow/ambient) via a tsgo
    definition/diagnostic check. The anti-shadow half rests on a binding-identity
    primitive NOT YET verified at the pinned tsgo: no anti-shadow-needing row is
    admissible until the spike PROVES a concrete primitive; only a PROVABLY-un-shadowable
    symbol (a unique top-level name in a single-file standalone fixture with no ambient
    corpus contribution for that name) is admissible without it. **Guards:**
    `probe_binds_to_registry_target`, `anti_shadow_needs_proven_binding_primitive`.

22. **`oracle_env_files` stores the CLOSED VENDORED-CORPUS manifest so the env hash is
    re-validatable offline; the filename is registry-derivable; `raw_capture` is mandatory
    and ties back to `oracle_value`.** Every snapshot STORES `oracle_env_files` (the
    vendored corpus's directory `manifest` + per-file `{path, content_hash}` list — `.d.ts`
    + package manifests + project metadata) so the default tsgo-free guards RE-ENUMERATE
    the vendored corpus, assert SET-EQUALITY against the stored manifest (catching an
    ADDED file), recompute `oracle_env_hash` by re-hashing the stored list against current
    on-disk content, and validate it against the stored value, never re-running tsgo. The
    per-snapshot `oracle_env_hash` is NOT a `snapshot_id` input (the STABLE `env_corpus_id`
    is), so the EXPECTED filename set is derivable from the REGISTRY ALONE (no snapshot
    opened, no tsgo) — env drift is caught on the value, not the filename. Every snapshot
    also stores `raw_capture` (verbatim hover + probe header) so the wrong-hover fence
    (`probe_header_names_target`) is auditable offline AND so `raw_capture_matches_oracle_value`
    can re-lower the stored hover and assert it equals the stored `oracle_value`. The two
    file domains are DISJOINT: a per-row `identity.workspace_files` path never appears in
    `oracle_env_files` (the SHARED ambient/lib/package/tsconfig corpus), and a package a
    ROW injects into its own workspace (`/workspace/node_modules/...`) is a per-row
    workspace file, NOT a shared corpus member (the one split rule, by ownership). **Guards:**
    `oracle_env_files_redrive_offline`, `oracle_env_corpus_is_closed`,
    `oracle_env_files_manifest_matches_files`,
    `oracle_env_corpus_is_complete`, `workspace_files_not_in_oracle_env_files`,
    `row_injected_packages_are_workspace_files`,
    `raw_capture_present_for_audit`, `raw_capture_matches_oracle_value`.

23. **The dominant host is `standalone`, driven under ONE canonical oracle config against
    a VENDORED corpus.** The DOMINANT population resolves under
    `VerterHost::new_standalone` (no project root, no tsconfig, fact 9, `support.rs:89`,
    `host_construction.rs:249`). The generator drives every standalone-host row under the
    SAME canonical `oracle.tsconfig.json` + synthetic root (a stable
    `compiler_options_hash`) inside the CLOSED VENDORED corpus
    (`oracle_env/<env_corpus_id>/`) — tsgo's `--lsp` / root points at THAT frozen corpus,
    NEVER live `node_modules`. The corpus's relative paths + content hashes + directory
    manifest are recorded in `oracle_env_files` and the stable `env_corpus_id` enters
    `snapshot_id`. `host_setup_kind` is a closed enum — `standalone` (default) /
    `workspace_footprint` (the ~9-row minority) / deferred package-backed-custom — and
    enters `identity` + `snapshot_id`. **Guard:** `standalone_host_is_default_canonical_config`.

24. **The generator PROVES tsgo applied the oracle options — a MULTI-OPTION matrix.**
    Beyond hashing the effective options (`compiler_options_hash_is_closed`), the
    generator runs a DELIVERY-PROOF MATRIX: for EACH print-affecting option the closed
    effective-option map pins away from the tsgo default (at minimum `strictNullChecks`,
    `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`), a discriminating fixture
    whose hover answer differs under the oracle value vs the default, asserting tsgo
    returned the oracle-value answer; a default answer on ANY matrix row FAILS
    generation. A single-flag probe would miss a dropped second flag. **Guard:**
    `oracle_options_delivery_proven`.

25. **The oracle-query-spec registry is PURE context-neutral data.**
    `oracle_query_specs.rs` is closed enums + owned strings — NO `super::support`, NO
    private unit-test types, NO helper calls — so it is `include!`/`#[path]`-shareable
    by BOTH the `#[cfg(test)]` `src` unit driver AND the `tests/` guard as ONE table.
    The helper-calling driver stays in `src`; `tests/` consumes data only. **Guard:**
    `oracle_query_specs_is_pure_data`.

26. **`snapshot_id` is the FULL BLAKE3 digest and is unique.** The id is the FULL
    (≥256-bit) BLAKE3 output hex-encoded, NOT a 12-byte truncation, and no two distinct
    registry `(row, query_ordinal)` entries derive the same id. **Guard:**
    `snapshot_id_is_unique`.

27. **The first implementation block EXCLUDES all unspiked `Expanded`-mode AND
    `Skeleton`-mode queries.** Only `Shallow` / `Navigate` (and any spike-validated
    `Expanded` / `Skeleton`) rows are admissible initially; every unspiked `Expanded`-mode
    OR `Skeleton`-mode query stays `Ignored` until the blocking probe-form spike (§4) lands
    and versions a lossless probe form for that mode. `Skeleton` is gated for the same
    reason as `Expanded`: eliciting tsgo's `TypeParameter`/`Infer` shell printing for
    unbound generics (so Conditional branches do not collapse to `never`) is an unsettled
    tsgo-printing question with no proven probe form. **Guards:**
    `expanded_probe_form_validated`, `skeleton_probe_form_validated`.

28. **`EvaluateExpr` mirrors Verter's scratch-file + `eval_source`-prelude model.** For
    `EvaluateExpr` the generator synthesizes a SEPARATE scratch file = the scope's
    `eval_source` prelude + a trailing `type __oracle_probe__N = <expression>;`,
    mirroring `crates/verter_session/src/typeinfo/evaluate_type_expression.rs:~314` (NOT a same-file append). For
    `ResolveExpr` / `ShallowSurfaceExpr` the same-file append remains correct
    (`support.rs:132,160`). `probe_binds_to_registry_target` backstops any residual
    divergence. **Guard:** `evaluate_expr_uses_scratch_prelude_model`.

29. **The oracle-env corpus is VENDORED + CLOSED.** The generator copies the BYTES of
    every consulted file (canonical `oracle.tsconfig.json`, every consulted lib / ambient
    / package `.d.ts`, every resolution-affecting `package.json` / project-metadata file)
    into a CHECKED-IN vendored directory `oracle_env/<env_corpus_id>/` and drives tsgo
    against THAT frozen corpus root — NOT live `node_modules` (gitignored, where tsgo
    bundles its libs, `ipc.rs:~2859-2874`, `.gitignore:9`). `oracle_env_files` stores the
    corpus's directory `manifest` (complete listing) + per-file content hashes; the
    offline gate RE-ENUMERATES the vendored dir and asserts SET-EQUALITY against the
    stored manifest (catching an ADDED file — membership) BEFORE content-hashing
    (catching an edit/delete). The corpus is content-addressed by the STABLE
    `env_corpus_id`, which enters `snapshot_id` as a pinned-env constant; regeneration
    re-vendors the corpus and recomputes `env_corpus_id`. This satisfies the
    Testing-Hermeticity rule (locally-vendored fixtures only) and is independent of any
    developer-machine `node_modules` state. **Guard:** `oracle_env_corpus_is_closed`.

30. **The stored `oracle_value` reflects the captured TS7 hover.** `raw_capture` is
    mandatory (Invariant 22); beyond the probe-header fence, a guard ties the stored
    `oracle_value` back to the capture: it extracts the probe type from `raw_capture` and
    re-runs the FULL HOVER-SIDE pipeline that PRODUCES `oracle_value` on that stored hover
    — (1) the HOVER-side POSITIVE ALLOWLIST (default-REJECT, §Q2), (2) the STRICT
    drop-counter, (3) the backstop reject rules (`any`/`never`/`Unknown`/truncation/
    unexpanded-`Ref`), THEN (4) `lower_ts_type` lowering + normalization — asserting the
    admitted-and-normalized result EQUALS the stored `oracle_value`. Re-running the hover
    ADMISSION (not just lowering + normalization) is what makes a LOSSY stored hover fail
    offline: a stored hover carrying a non-allowlisted construct FAILS the re-run hover
    allowlist even if it lowers to the stored value. The SOURCE-side admissibility walk's
    contributor NAVIGATION (the two-sided §Q2 allowlist's binding/import/merge resolution
    on the fixture SOURCE declaration) is a GENERATION-TIME step that `raw_capture`
    (hover-only) cannot replay; the source-side ALLOWLIST is re-checked instead by
    `source_admission_digest_consistent` (re-parse each recorded contributor's current
    registry/corpus source + re-run the current allowlist over the fresh combined facts).
    This guard re-runs the entire HOVER side; that guard re-checks the SOURCE side. A
    hand-edited / wrong `oracle_value` that still decodes strictly (`strict_snapshot_decode`)
    but no longer matches the captured hover FAILS. Runs offline (no tsgo). **Guard:**
    `raw_capture_matches_oracle_value`.

---

## 4. Verification (guards/tests this design implies)

For the later implementation block to build. Every `(CRITICAL)`-class claim above
carries a named guard here.

> **Reframed by the single-spec / correction-overlay model — additive guards.** The
> model adds guards (owned by the mechanism block in `ts-compat-two-mode-model.md`
> §8/§12), it does not relax any below. The impact on THIS table: `kind_eligibility_gate`
> learns that a divergence row is seated as `ProofRequirement::OracleAndGuard { oracle,
> guard }` whose `guard` is the registered `DivergenceCorrection` prover, consulting a
> per-query `&[QueryCorrection { query_ordinal, correction, divergence_id }]` overlay — a
> divergence row carries one `QueryCorrection` per
> corrected query, each whose `divergence_id` resolves to the divergence registry; the
> harness runs the single-spec resolver ONCE per query and asserts, FOR EACH corrected
> query, `resolver(query) == correction.correct_value` while that query's
> `snapshot.oracle_value` is the recorded `TsCompat` value and differs. An uncorrected
> query asserts `resolver(query) == snapshot.oracle_value`. There is no per-mode re-run,
> no family-key comparison, and no spec dimension — the resolver is single-spec
> (`ts-compat-two-mode-model.md` §4, §7). The 362 partition, the
> ≤122 `Relate`-free ceiling, and the obligation model are unchanged; a divergence
> row counts toward 362 and does not carry `Relate`.

| Guard / test | Asserts |
| --- | --- |
| `kind_eligibility_gate` | A query is lifted via this harness only when its registry `oracle_value_kind == structured_type_expr` AND it asserts a `TypeExpr` projection, not a relation/call verdict; `SemanticQueryName::Relate` as the asserting key is rejected; AND — enforcing the 122-row `Relate`-free CEILING (§Q2) — ANY row whose manifest `semantic_queries` CONTAINS `SemanticQueryName::Relate` ANYWHERE (not only as the asserting key) is REJECTED, until/unless a future oracle kind explicitly owns the relation path (so a `Relate`-carrying row marked `structured_type_expr` cannot slip past the ceiling even if its asserting key is not a relation). A row that asserts an INDEPENDENT non-`TypeExpr` obligation is REJECTED under a bare `Ts7Oracle` proof and admissible only under `OracleAndGuard` (a divergence row is an `OracleAndGuard` whose `guard` is the `DivergenceCorrection` prover, `ts-compat-two-mode-model.md` §9.2: the prover runs PER CORRECTED query — each corrected query asserted as a single-spec data comparison, `resolver(query) == correction.correct_value` while that query's `snapshot.oracle_value` is the recorded `TsCompat` value and differs, and the named correction overlay AND a registry entry whose id equals `divergence_id` resolve to the SAME `(correction, registry-entry)`). The obligation is expressed by the `OracleAndGuard` proof shape + the registered live prover the `guard` resolves to — NOT a typed set stored on a ledger record; the five obligation KINDS the provers cover are `DependencyFootprint` / `AuditRecord` / `WarmCache` / `DeclaredDependency` / `DivergenceCorrection`. PER-KIND targeting (this sub-rule depends on the DEFERRED §Q4 per-row-count layer and only activates once the `oracle_query_ordinals` field lands — it is not yet a shipped column): the per-query provers (`AuditRecord`, `DivergenceCorrection`) select a `query_ordinal` IN RANGE for the row's `oracle_query_ordinals` (a multi-query row default-rejects a per-query prover that cannot select a valid in-range ordinal — the multi-query default-reject backstop), so a per-query assertion is proved against the CORRECT query, never "any query passes". The gate READS the checked-in CODE `OBLIGATION_GUARD_REGISTRY` (the closed static slice `&[ObligationGuardEntry { guard_id, prover: fn(&RowExecutionResults) -> Result<(), GuardFailure> }]` keyed by `GuardId`, §Q4 — the §4 guard table is its human-readable mirror; the prover selects the relevant ordinal's `RowExecutionResults` slot) and verifies that the proof's `OracleAndGuard.guard` resolves to an `OBLIGATION_GUARD_REGISTRY` entry whose `prover` is a real compiled `fn` symbol (a registry lookup, NOT a Markdown-name match against the §4 table). A complementary parity check asserts the §4 guard table lists exactly the registry's prover fns (mirror in sync). Not a coarse bool, not a vague "transitively asserts" claim, not a prose stub. Discriminating: a row whose `semantic_queries` carries `Relate` ANYWHERE (even if its asserting key is not a relation), a `Relate`-asserting row, and a bare-`Ts7Oracle` row that carries an independent non-`TypeExpr` obligation must all be rejected; a pure `TypeExpr`-projection row (no obligation, no `Relate`) accepted; a row under `OracleAndGuard` whose `guard` resolves to a registered prover with an `OBLIGATION_GUARD_REGISTRY` entry accepted; a row whose `guard` has NO registry entry (an unproven obligation), or a per-query prover that cannot select a valid in-range ordinal on a multi-query row (so the proof would land on the wrong query), REJECTED. |
| `dependency_footprint_obligation_reproved` | The registered live prover for the `DependencyFootprint` obligation KIND (`GuardId::DependencyFootprintObligation`). For an `OracleAndGuard` lifted row whose `guard` is this prover, re-runs the row's oracle execution and asserts the recorded `includes`/`excludes` paths against the live resolved dependency set, so the footprint behavior the original body proved is re-proven post-lift (the `TypeExpr` compare cannot). Discriminating: a row migrated as a bare `Ts7Oracle` compare that DROPS its original footprint assertion is caught because such a row is promoted to `OracleAndGuard` and this prover re-asserts the RECORDED footprint; a prover that merely re-ran the query without comparing to the recorded `includes`/`excludes` would NOT catch a footprint regression. |
| `audit_record_obligation_reproved` | The registered live prover for the `AuditRecord` obligation KIND (`GuardId::AuditRecordObligation`). Re-asserts the recorded `(field, value)` pairs against the returned `RequestAuditRecord`, for an `OracleAndGuard` lifted row whose `guard` is this prover (selecting the originating query's ordinal). Discriminating: a lift that drops the audit-record assertion (demoting to bare `Ts7Oracle`), OR a regression that changes a recorded field value, fails. |
| `warm_cache_obligation_reproved` | The registered live prover for the `WarmCache` obligation KIND (`GuardId::WarmCacheObligation`). Re-asserts the recorded warm-cache / cache-hit fact keys against the live cache state, for an `OracleAndGuard` lifted row whose `guard` is this prover. Discriminating: a lift that drops the warm-cache assertion, OR a regression that loses a recorded warm fact, fails. |
| `declared_dependency_obligation_reproved` | The registered live prover for the `DeclaredDependency` obligation KIND (`GuardId::DeclaredDependencyObligation`). Re-asserts the recorded declared-dependency ids against the live declared-dependency set, for an `OracleAndGuard` lifted row whose `guard` is this prover. Discriminating: a lift that drops the declared-dependency assertion, OR a regression that changes the recorded ids, fails. |
| `divergence_correction_obligation_reproved` | The registered live prover for the `DivergenceCorrection` obligation KIND (`GuardId::DivergenceCorrectionObligation`). For a divergence row (an `OracleAndGuard` whose `guard` is this prover), re-runs PER corrected `query_ordinal`: re-asserts the per-query divergence-data tie — `resolver(query) == correction.correct_value` while that query's `snapshot.oracle_value` is the recorded `TsCompat` value and differs from it — and that the named correction overlay AND a divergence-registry entry whose id equals `divergence_id` resolve to the SAME `(correction, registry-entry)` (`ts-compat-two-mode-model.md` §9.2, Guard C / `every_correction_is_discharged`). Discriminating: a corrected query whose `resolver(query)` does not equal `correction.correct_value`, whose `snapshot.oracle_value` equals the correction (no real divergence), whose selected ordinal is out-of-range, or whose `correction_id`/`divergence_id` does not resolve to the SAME `(correction, registry-entry)` it names, fails. |
| `registry_covers_every_lifted_oracle_query` | Every `Lifted` oracle row (proof `Ts7Oracle(_)` or `OracleAndGuard{..}`) has ≥1 registry entry (by `(row_file, row_function, *)`), every entry joins to an existing manifest row, and every entry has an existing snapshot (re-derived `snapshot_id` resolves to a file). A divergence row's `OracleAndGuard.oracle` field supplies the snapshot `OracleId`; its per-query `correction` overlays are SEPARATE review-gated artifacts (`ts-compat-two-mode-model.md` §3) checked by the overlay guards (`no_orphan_correction`, keyed by `(row_file, row_function, query_ordinal, snapshot_id)`), not by this snapshot-coverage rail. |
| `migration_fingerprint_extraction_is_static` (DEFERRED) | The `migration_fingerprint` is extracted by the one-time audited lift command via a STRUCTURED `syn` AST parse of the original `#[test]` body (NOT a text/regex scan): it walks the body's statements in order, collects every known oracle-helper + workspace-setup-helper call, RESOLVES each known setup helper through the CLOSED named helper model (§Q4 "Extraction method" — `make_host_with_footprint`, `make_host_with_workspace_files_footprint`, `upsert_ts`, `upsert_cross_file_fixture`, `resolve_expr`/`shallow_surface_expr`/`evaluate_expr`) to its concrete upserts / host-construction / query tuple, const-folds literal/constant payloads, and detects each obligation-bearing assertion (promoting the row to `OracleAndGuard` with the matching registered prover). A body whose upserts, queries, or obligation arguments are NOT statically extractable by that closed model — a loop, a NON-MODELED wrapper helper, a MACRO-GENERATED body, a CLOSURE-bearing assertion, a runtime value, an unfoldable expression — REJECTS LOUDLY: the row is NOT auto-lifted (it stays `Ignored` for hand-lift under an EXTENDED helper model, or defers), never auto-lifted with a guessed/partial fingerprint. So the initial auto-liftable set is a SOUND SUBSET of the eligible rows (only bodies that fold entirely through the named model), never a guessed superset. Discriminating: a row that hides its upserts behind `upsert_cross_file_fixture(...)` extracts the same ordered `workspace_files` + `host_project` a literal-inline row would; a row that computes a path in a loop, or whose body is macro-generated / closure-bearing, FAILS extraction (stays `Ignored`) rather than producing an unverifiable fingerprint. |
| `original_extraction_input_auditable` (DEFERRED) | The retained-lift metadata STORES `original_body_tokens` — the canonical ORIGINAL `#[test]` body `syn` token stream (the EXACT extraction INPUT ITSELF, `Span`-stripped, whitespace-insignificant token-tree print), CHECKED IN at lift time alongside `migration_fingerprint`. This makes the INITIAL extraction input auditable, not merely self-compared to the derived fingerprint: this guard RE-RUNS the extractor over the CHECKED-IN `original_body_tokens` artifact and asserts the re-derived fingerprint EQUALS the recorded `migration_fingerprint` — HERMETICALLY, from the retained-lift artifact alone, with NO VCS archaeology (no shallow-checkout / archive / CI-clone dependence). So a WRONG INITIAL extraction — a `migration_fingerprint` computed from a mis-read body that is self-consistent with the registry forever — is detectable, because the extraction INPUT is pinned in the retained-lift metadata independently of the derived fingerprint. The token stream is an audit record, NOT a `snapshot_id` input. Discriminating: a retained-lift record whose `original_body_tokens` re-extract to a fingerprint that differs from the recorded `migration_fingerprint` FAILS; a record missing `original_body_tokens` FAILS. (GENUINELY DEFERRED — the `migration_fingerprint` / `original_body_tokens` body-hash fidelity layer is not yet wired; this is the planned guard.) |
| `registry_entry_count_matches_declared` (DEFERRED) | (DEFERRED — reads the not-yet-added `IgnoredTestRow.oracle_query_ordinals` field; lands with the §Q4 per-row-count layer alongside `migration_fingerprint` / `original_body_tokens`.) For EVERY manifest oracle row (not only `Lifted`), the registry entry count (`(row_file, row_function, *)`) EQUALS the row's declared `IgnoredTestRow.oracle_query_ordinals`, AND the registry's `query_ordinal`s are UNIQUE and CONTIGUOUS `0..count-1` (a `{0,1,3}` gap or `{0,0,1}` duplicate FAILS even at matching count). Includes the ZERO-COUNT rows: a row with declared `oracle_query_ordinals == 0` (every `Ignored` / non-oracle / un-lifted row) MUST have ZERO registry entries — a stray entry on a zero-count row FAILS (this is the count-side complement to the `registry_covers_every_lifted_oracle_query` biconditional that no registry entry exists for a non-`Lifted`-oracle row). The two are independent sources — catches a registry that under-counts (3 of 4), which the forward/reverse pair (both registry-derived) cannot see. Discriminating: a 3-entry registry for a row declaring 4 must FAIL; ordinals `{0,1,3}` must FAIL; any registry entry on a declared-`0` (Ignored) row must FAIL. |
| `registry_family_matches_manifest_oracle_id` | Each registry entry's `oracle_family` EQUALS the family carried by the manifest row's `ProofRequirement::Ts7Oracle(OracleId)` / `OracleAndGuard { oracle, .. }` (a divergence row carries its `OracleId` in `OracleAndGuard.oracle`) (`typeinfo_ignored_test_manifest.rs` `OracleId` / `ProofRequirement`). Discriminating: an entry naming a different family than its row's proof must FAIL (it would read/write the wrong `oracle_snapshots/<family>/` sub-directory). |
| `registry_payload_matches_migration_fingerprint` (DEFERRED) | Cardinality (count + contiguity) does NOT prove the registry PAYLOAD matches the ORIGINAL hand-authored query, nor that the lift preserved the row's non-`TypeExpr` obligations. `migration_fingerprint` (on the retained-lift metadata) is THE migration-fidelity authority — NOT a hand-maintained registry field. At lift time the body (parsed as a `syn` AST, see §Q4 "Extraction method") yields the ordered per-query FIDELITY tuple — each call's `(helper_kind, primary_canonical, symbol_or_expression, type_arguments, projection_mode, workspace_files, source_locator incl symbol_space, host_project)` — ∪ the row's proof shape (`Ts7Oracle` vs `OracleAndGuard` + its `guard` id) DETECTED from the body's dependency-footprint / audit-record / warm-cache / declared-dependency assertions; the canonical-JSON over that is recorded BEFORE the body is replaced by `#[oracle_row]`. This guard asserts the row's registry entries (their full fidelity tuple INCLUDING `source_locator` + `host_project`), in `query_ordinal` order, ∪ the row's proof shape, re-canonicalize to the SAME `migration_fingerprint`. Discriminating: a registry entry with the correct COUNT but a wrong `symbol_or_expression` / `primary_canonical` / `projection_mode` / `type_arguments` / `workspace_files` / `source_locator` (incl. a flipped Type↔Value `symbol_space`) / `host_project` (e.g. a `workspace_footprint` row migrated as `standalone`), OR a proof shape that drifted (a row demoted to bare `Ts7Oracle` that asserted a footprint/audit/warm-cache check), FAILS — proving the registry reproduces the ORIGINAL query AND the proof shape preserves its obligations, not merely a self-consistent target. (GENUINELY DEFERRED alongside `migration_fingerprint`.) |
| `oracle_env_files_redrive_offline` | `no_orphan_snapshot` re-enumerates the vendored corpus + recomputes each `oracle_env_hash` by re-hashing the snapshot's stored `oracle_env_files.files` `{path, content_hash}` list against current on-disk content and validates it against the stored value — WITHOUT re-running tsgo, and WITHOUT folding it into `snapshot_id` (the filename is registry-derived from the STABLE `env_corpus_id`). Discriminating: an env file edited on disk re-derives a different `oracle_env_hash` from the stored list (snapshot invalidated as a VALUE mismatch) with no tsgo invocation in the default gate. |
| `oracle_env_corpus_is_closed` | The vendored oracle-env corpus (`oracle_env/<env_corpus_id>/`) is CLOSED (STABILITY): the offline gate RE-ENUMERATES the vendored directory's current file listing and asserts SET-EQUALITY against the snapshot's stored `oracle_env_files.manifest` (no unlisted file, none missing) BEFORE content-hashing, and tsgo is driven against that frozen corpus root (not live `node_modules`, gitignored — `.gitignore:9`; tsgo bundles libs under `node_modules/@typescript/native-preview-*/lib/`, `ipc.rs:~2859-2874`). Discriminating: an UNLISTED `.d.ts` dropped into the corpus dir FAILS set-equality even though every listed file still hashes clean (catches an ADDITION the content re-hash alone would miss); a developer's `node_modules` change is irrelevant (the corpus is hermetic + checked-in). |
| `oracle_env_files_manifest_matches_files` | The snapshot's two stored `oracle_env_files` path sets are INTERNALLY consistent BEFORE any hashing or on-disk re-enumeration: asserts `oracle_env_files.manifest` (the directory listing) EQUALS `oracle_env_files.files[].path` (the hashed file list) as the SAME SET — both canonical-path-sorted, duplicate-free, with no path in `manifest` absent from `files` and none in `files` absent from `manifest`. This is the precondition `oracle_env_corpus_is_closed` (manifest set-equality vs the on-disk dir) and `oracle_env_hash` recomputation (re-hashes `files`) both rely on: if `manifest` and `files` diverged, a file could be listed in `manifest` / present in the dir yet OMITTED from the `files` hash list, so it would pass the on-disk set-equality (against `manifest`) yet never be content-hashed — an un-hashed corpus member. Discriminating: a snapshot whose `manifest` lists a path absent from `files` (or vice versa), an unsorted list, or a duplicate path in either, FAILS — closing the gap where a corpus member is in the manifest/dir but excluded from the hash. |
| `oracle_env_single_current_corpus` | `env_corpus_id` has a CHECKED-IN pinned source-of-truth: the `CURRENT_ENV_CORPUS_ID` constant in `oracle_query_specs.rs` (mirrored by the `oracle_env/CURRENT` pointer file), which the registry + every guard read to derive `snapshot_id` and locate the corpus dir WITHOUT opening a snapshot. Asserts (a) `oracle_env/` contains EXACTLY ONE `<env_corpus_id>/` directory, (b) its name == `CURRENT_ENV_CORPUS_ID` == the `CURRENT` pointer's contents, (c) every snapshot's stored `env_corpus_id` == `CURRENT_ENV_CORPUS_ID`. This is a NAME/POINTER/CONSTANT/SNAPSHOT-FIELD EQUALITY check — it does NOT recompute the id from content; that loop is closed by `env_corpus_id_recomputes_from_corpus`. Regeneration re-vendors into `oracle_env/<new_id>/`, rewrites the pin + pointer, DELETES the prior corpus dir, and rewrites every snapshot under its new id (clean cutover, no dual corpus). Discriminating: a leftover stale `oracle_env/<old_id>/` dir, a `CURRENT`-pointer / constant mismatch, or a snapshot pinned to a retired corpus all FAIL. |
| `env_corpus_id_recomputes_from_corpus` | Closes the "the pinned id still NAMES the actual current corpus" loop offline: RECOMPUTES `env_corpus_id` from the canonical ON-DISK corpus under the EXACT pinned recipe (§Q1) — BLAKE3, domain-separated under the `env_corpus_id` tag, over the CORPUS LISTING: the canonical-path-sorted `[{ path, content_hash }]` pairs of the full `oracle_env/<CURRENT_ENV_CORPUS_ID>/` vendored corpus (each file's path + its per-file content hash under the pinned content normalization, NO file bytes hashed inline) — and asserts the recomputed id EQUALS `CURRENT_ENV_CORPUS_ID`, the `oracle_env/CURRENT` pointer's contents, the corpus DIRECTORY NAME, AND every snapshot's stored `env_corpus_id`. Where `oracle_env_single_current_corpus` checks the four spellings agree WITH EACH OTHER, this guard checks they all agree WITH THE ACTUAL CONTENT — so a corpus whose content was edited/added/removed WITHOUT re-pinning (the four spellings still mutually equal, but no longer naming what is on disk) FAILS. Runs OFFLINE (no tsgo). Discriminating: a one-byte edit to any vendored corpus file, or an added/removed corpus file, that leaves `CURRENT_ENV_CORPUS_ID` / `CURRENT` / dir-name / snapshot fields unchanged recomputes a DIFFERENT id and FAILS — catching a stale pin that mutual-equality alone cannot. |
| `oracle_env_corpus_is_complete` | COMPLETENESS (distinct from `oracle_env_corpus_is_closed`'s STABILITY): the GENERATOR drives tsgo against ONLY the frozen vendored corpus root with NO live `node_modules` / ambient fallback on any resolution path AND tsgo forced off its native-bundled libs (vendored `oracle.tsconfig.json` setting `"noLib": true` + an explicit corpus-rooted vendored lib file list — the NAMED candidate, §Q2 "Env pinning" — or the `"lib"`+corpus-rooted-`typeRoots`/`paths` fallback; tsgo's bundled libs copied INTO the corpus, working-dir/resolution-root pinned to the corpus; the exact wire-payload is the §4 BLOCKING spike, and if no candidate forces tsgo off bundled libs the lib-dependent class stays DEFERRED). Any un-vendored resolution-affecting file the probe needs becomes a GENERATION-TIME failure: tsgo cannot resolve it → the hover is a resolution-failure/`any`/`Unknown`/missing-module shape that the backstop + the WHOLE-project zero-NEW-diagnostics gate catch, FAILING generation (no snapshot written). Discriminating: a row whose symbol needs an ambient `.d.ts` that the generator forgot to vendor FAILS generation (a missing-module diagnostic) rather than producing a clean snapshot that secretly leaned on an un-tracked live file. |
| `workspace_files_not_in_oracle_env_files` | No path listed in a snapshot's `identity.workspace_files` appears in that snapshot's `oracle_env_files.manifest` / `.files` (the two file domains — per-row workspace files vs the SHARED ambient/lib/package/tsconfig corpus — are DISJOINT). The guard normalizes the leading-slash workspace spelling and the corpus-relative spelling to a comparable form before the disjointness check. Discriminating: a snapshot that lists a per-row fixture (`/fixtures/foo.ts`) in BOTH domains FAILS (it would double-count the file across domains and poison the shared corpus content id with per-row content). |
| `row_injected_packages_are_workspace_files` | A package file a ROW injects into its OWN workspace (`/workspace/node_modules/...`, e.g. `flow_return_catalog.rs:209`) is modeled as a PER-ROW `identity.workspace_files` entry (hashed into `snapshot_id`), NOT a SHARED `oracle_env_files` corpus member — the ONE rule for the split, by OWNERSHIP not the `node_modules` substring. `env_corpus_id` / `oracle_env_files` is the SHARED ambient package corpus only. Discriminating: a snapshot that lists a row-injected `/workspace/node_modules/...` file in `oracle_env_files` (rather than `identity.workspace_files`) FAILS — it would poison the shared corpus id with per-row content and break the disjointness rule. The custom package-host class (`make_package_host_with_workspace`) stays DEFERRED (§Scope); this rule resolves only the row-INJECTED case. |
| `parameterized_probe_rhs_synthesis` | For a `ResolveExpr` query with NON-EMPTY `type_args`, the probe RHS is `symbol<…>` with each type-argument printed back to TS source by the deterministic, VERSIONED `TypeExpr` → TS-source printer (versioned by `probe_synthesis_version`); the printer covers only the printable construct set (the §Q2 ADMIT constructs) and a non-printable argument DEFAULT-REJECTS the `(row, query)`; the print → re-lower round-trip must be structurally-equal-under-normalization or generation FAILS. Non-empty-`type_args` rows are NOT admissible until the printer is spiked + versioned; empty-`type_args` rows use the bare-`symbol` RHS. Discriminating: a `GenericBox<string>` query emits `type __oracle_probe__N = GenericBox<string>;` (distinct from `GenericBox<number>`); a type-argument whose `TypeExpr` is a `Mapped`/`Conditional`/`Unknown` is rejected, not best-effort printed. |
| `canonical_encoding_is_pinned` | Every hashed/structurally-compared value uses the pinned canonical encoding: canonical JSON (lexicographic key order, no insignificant whitespace, minimal escaping, integer-only numbers), `/`-normalized paths (leading-slash for workspace files, corpus-relative for the env corpus), content hashes taken over the EXACT pinned normalization (CRLF/CR → `\n`, THEN trailing newlines collapsed to EXACTLY ONE `\n` for non-empty content / empty for a zero-byte file), canonical-path-sorted manifest ordering, and the two hash FAMILIES by role (BLAKE3 for `snapshot_id`/`env_corpus_id`/`oracle_env_hash`; SHA-256 for `compiler_options_hash`/per-file `content_hash`), each family self-described by its `sha256:`/`blake3:` prefix and pinned in the schema. Discriminating: a snapshot whose hash carries the wrong family prefix, or whose canonical JSON has non-sorted keys / a CRLF-unnormalized content hash, FAILS; the encoding is versioned with `oracle_schema_version` (encoding) + `normalizer_version` (structural-sort key). |
| `hover_extraction_grammar_is_versioned` | The hover-extraction grammar (§Q2) extracts `<RHS>` from the `type __oracle_probe__N = <RHS>` header inside tsgo's hover via a FIXED grammar with TWO ordered shapes, BOTH requiring the candidate to be EXACTLY one top-level probe-alias declaration: (1) if ANY fence is present, ONLY fenced \`\`\`typescript / \`\`\`ts blocks are parsed (prose/inline/non-TS ignored), first probe-naming block among multiple, leading JSDoc/comment tolerated — and any fence DISABLES the plaintext fallback; (2) if ZERO fences are present, the WHOLE trimmed hover is parsed as the BARE PLAINTEXT driver shape (the empty-caps `type __oracle_probe__N = <RHS>` text, no markdown fence). The candidate is accepted ONLY as an exact top-level alias via the SAME OXC TS parser (full-consumption strict parse, NOT a loose substring scan): correct probe name, NO `export`/`declare` modifier, NO type parameters, NO surrounding prose / trailing declarations; the RHS is the alias type-annotation span handed UNCHANGED to the admission gate's parser (qualified/imported display forms parsed, not text-interpreted). The exact capabilities + reduced content shape fold into `probe_synthesis_version`; re-runnable OFFLINE from `raw_capture`. Discriminating: a clean bare plaintext `type __oracle_probe__0 = {…}` (empty-caps shape) extracts; a nested-`;` object body extracts the WHOLE balanced RHS; a header embedded in prose, an `export`/`declare` alias, a parameterized `type P<T> = …`, a wrong probe name, or a trailing extra declaration is REJECTED; a truncated/unclosed candidate FAILS; a hover with NO probe-naming \`\`\`typescript fence (when any fence is present) FAILS the header fence (the plaintext fallback does NOT fire). |
| `hover_driver_config_pinned` | The tsgo LSP HOVER-DRIVER CONFIG is a value-affecting identity axis FOLDED INTO `probe_synthesis_version` (§Q2 "Hover-driver config", Q3): the canonical `hover_driver_config` blob covers the LSP `initialize` payload + declared CLIENT CAPABILITIES, the `workspace/didChangeConfiguration` / `initializationOptions` delivery block, the `textDocument/hover` request payload, and the hover DISPLAY PREFERENCES (alias-expansion / truncation-length / quote-style / member-ordering print preferences). The capabilities are pinned EXACTLY: the adopted driver (`TsgoTypeProvider::get_hover`) uses EMPTY hover capabilities (`capabilities: {}`, `crates/verter_type_runtime/src/tsgo/ipc.rs`), which produce / reduce to a BARE PLAINTEXT hover (no markdown fence) — the §Q2 extraction-grammar plaintext branch parses that shape; a markdown-caps driver would produce the fenced shape. The guard asserts the generator drives tsgo under exactly this pinned config and that the config blob (capabilities + reduced content shape included) is hashed into `probe_synthesis_version` (a change bumps the version + forces regeneration). Discriminating: two sessions with identical query + env but a different declared capability/content shape (empty-caps plaintext ↔ markdown fence), a different display preference (e.g. an alias-expansion or truncation-length toggle that changes the printed hover), or a different hover-request payload MUST resolve to a different `probe_synthesis_version` (and thus a different `snapshot_id`) — they can NEVER produce different captures under the SAME `snapshot_id`; a driver whose config is not folded into `probe_synthesis_version` FAILS. |
| `oracle_literal_subsumption_discriminates` | The bounded literal-subsumption rule (§Q2 step 5) collapses an absorbed literal arm ONLY when its base primitive is CO-PRESENT in the SAME union (`string | "a"` → `string`), never widens a lone literal, never crosses base types, never collapses a literal-only union, never touches intersections. Discriminating: `"a" | string` (→ `string`) vs `"a" | "b"` (a literal-only union, NOT collapsed) MUST DIVERGE (`string` ≠ `"a" | "b"`); `"a" | number` (no co-present `string`) MUST NOT collapse the `"a"` arm, so `"a" | number` vs `number` DIVERGES — proving the rule fires only on the co-present base type and does not mask a real difference. |
| `raw_capture_matches_oracle_value` | Extract the probe type from the stored `raw_capture` (the verbatim `type __oracle_probe__N = <T>;` hover) via the versioned hover-extraction grammar, then re-run the FULL HOVER-SIDE pipeline that PRODUCES `oracle_value` on that stored hover OFFLINE (no tsgo): (1) the HOVER-side POSITIVE ALLOWLIST (default-REJECT, §Q2) over the parsed OXC type AST, (2) the STRICT drop-counter (`lower_ts_type` instrumented; non-zero drop ⟹ reject), (3) the backstop reject rules (`any`/`never`/`Unknown`/truncation/unexpanded-`Ref`), and only then (4) the `lower_ts_type` lowering + normalization — and assert the admitted-and-normalized result EQUALS the stored `oracle_value`. Re-running the allowlist + drop-counter + backstop (not just lowering+normalization) means a snapshot whose stored hover contains a REJECTED lossy construct FAILS even if that hover lowers to the stored value — a non-allowlisted hover construct can never be silently warm-validated offline. The two-sided §Q2 admission's SOURCE-side contributor NAVIGATION (binding/import/merge) is a generation-time step; the source-side ALLOWLIST is re-checked instead by `source_admission_digest_consistent`, which re-parses each recorded contributor's current source BY CANONICAL PATH (the row's registry `workspace_files` payload source for row/workspace files — verified against the snapshot's stored `content_hash` — on-disk for vendored corpus files) and re-runs the current allowlist over the fresh combined `(raw_surface, lowered_body)` pair per `(ordinal, decl_span)`-keyed contributor (contributor-set membership stays generation-time-established, but is moot because admission is restricted to single-contributor rows, §Scope). This guard re-runs the entire HOVER side. Discriminating: a snapshot whose `oracle_value` is mutated away from what its `raw_capture` lowers to FAILS (even though the mutated value still passes `strict_snapshot_decode`); a snapshot whose stored `raw_capture.hover_contents` carries a non-allowlisted construct (e.g. a `unique symbol`-keyed member or an accessor) that the original generation should have rejected FAILS the re-run hover allowlist, not merely the lowering. |
| `source_admission_digest_consistent` | `source_admission_digest` is REQUIRED in every snapshot (a snapshot missing it FAILS). It RECORDS the generation-time source-side admission as the ORDERED, KEYED `contributors` vector (each entry a `{ contributor_ordinal, decl_span, name, symbol_space, decl_kind, raw_surface, lowered_body, verdict }`) + final ADMIT/REJECT verdict from `resolve_source_declarations`, PLUS the provenance tie (the `source_locator` walked from + each observed source-declaration file's `{path, content_hash}`), closing the offline hover-vs-source asymmetry that `raw_capture_matches_oracle_value` leaves open (`raw_capture` stores only the hover, so the source-side contributor NAVIGATION is not offline-replayable). The guard is NOT a self-consistency check over the digest's own recorded data — it re-derives the COMBINED input FROM CURRENT SOURCE. Because `RawSourceSurface` is a PARSE-TIME artifact (shallow parsing, not type resolution) and the lowered body is the deterministic `lower_ts_type` of the parsed decl, the gate runs OFFLINE (no tsgo, no resolver): for EACH recorded contributor it resolves that file's CURRENT source BY CANONICAL PATH through the total canonical-path→source mapping (a leading-slash `/fixtures/...` / `/workspace/...` row-or-workspace file → the row's REGISTRY `workspace_files` payload source — the source-byte authority — re-parsed and VERIFIED against the snapshot's stored `content_hash` for that path, NOT read from an on-disk file; a vendored corpus file → on-disk under `oracle_env/<env_corpus_id>/`), RE-PARSES it, RE-CAPTURES the `RawSourceSurface` AND RE-LOWERS the body for the contributor identified by `(decl_span, contributor_ordinal, name, symbol_space, decl_kind)`, and RE-RUNS the CURRENT-version source-side positive allowlist over the freshly-captured `(raw_surface, lowered_body)` pair. It then asserts: (a) the digest's `source_locator` EQUALS the registry entry's `source_locator`; (b) each recorded source-declaration `content_hash` EQUALS the hash of the CURRENT registry-`workspace_files` SOURCE (or vendored-corpus on-disk content) for that canonical path (so a post-capture source edit, which re-keys the registry bytes' hash, invalidates the snapshot); (c) for each recorded contributor (keyed by `(decl_span, contributor_ordinal)`) the freshly RE-PARSED+RE-LOWERED `(raw_surface, lowered_body)` pair EQUALS the recorded one (catching a within-file fact OMISSION or TAMPER in the digest, in EITHER half of EITHER merged peer) AND the re-run CURRENT allowlist verdict over the FRESH pair is ADMIT (NOT a replay of the stored verdict — re-running the current allowlist catches a snapshot admitted under an OLDER allowlist version a newer version would reject); (d) the FINAL verdict is ADMIT (a snapshot exists ⟹ the two-sided check passed). **Honest scope (complete for the admitted set):** the guard catches any within-file fact tamper/omission in a RECORDED contributor (re-parse + re-lower + compare, per `(ordinal, decl_span)`) and any content change to a recorded contributor (per-contributor content-hash). It does NOT re-NAVIGATE the import/merge/transitive graph to discover a contributor / merged peer the digest never recorded — contributor-set MEMBERSHIP is generation-time-established and not offline-reproducible (§Q5 cross-reference). This membership gap is MOOT for the admitted set: initial admission is RESTRICTED to PROVABLY SINGLE-CONTRIBUTOR rows (`source_is_provably_single_contributor`, §Scope) whose contributor set is trivially `{the one decl}`, so re-navigation has nothing to discover; multi-contributor rows are DEFAULT-REJECTED and deferred to the offline contributor-set-membership-revalidation spike (§4). For the admitted single-contributor set a content edit is caught directly because it changes the one recorded file's hash. Discriminating: a snapshot with NO `source_admission_digest` FAILS; a digest whose `source_locator` differs from the registry's, whose recorded source-file hash no longer matches the hash of the registry-`workspace_files` SOURCE (or corpus content) for its canonical path, whose recorded `raw_surface` OR `lowered_body` for a contributor DIFFERS from the freshly re-parsed/re-lowered pair (a tampered/hollowed-out fact set, OR a swapped/dropped merged peer detected by `(ordinal, decl_span)`), whose freshly-captured pair carries a construct the CURRENT allowlist rejects (even if the STORED verdict was Admit under an older allowlist), whose `contributors` vector has MORE than one entry (an admitted row must be single-contributor), or whose final verdict is REJECT, all FAIL on an existing snapshot. |
| `source_is_provably_single_contributor` | Initial admission is RESTRICTED to PROVABLY SINGLE-CONTRIBUTOR rows so the offline guarantee is COMPLETE (not residual): an admitted `(row, query)`'s source-side walk (`resolve_source_declarations`, §Q2) must resolve to EXACTLY ONE contributor declaration in a SINGLE FILE — NO import / re-export hop, NO `MergedDecl` peer, NO ambient `declare module` / `declare global` augmentation contribution, NO transitive `typeof` / `ReturnType` / `Parameters` hop to a second declaration — so the contributor set is trivially `{the one decl}` and fully offline-verifiable. The generator DEFAULT-REJECTS any row whose walk reaches >1 contributor or crosses a file / merge / augmentation / transitive hop (it stays `Ignored`, deferred to the offline contributor-set-membership-revalidation spike, §4). The walk PROVES REJECTION NOW — reaching a transitive `typeof` / `ReturnType` / `Parameters` hop (or any cross-file / merge / augmentation hop) is sufficient to default-reject the row in this block; it is only ADMISSION of such transitive-hop rows that waits for the named contributor-set-membership BLOCKING spike (§4). So the earlier examples that describe the walk traversing such hops describe the walk's rejection-PROVING capability, not first-block admission of the traversed row. This narrows admission (it adds NO offline re-navigation machinery); it makes the `source_admission_digest_consistent` membership residual MOOT for the admitted set. Discriminating: a single-file, no-import, no-merge, no-augmentation declaration is admissible (its `source_admission_digest.contributors` has exactly ONE entry); an imported / re-exported symbol, a `MergedDecl` peer group, an ambient-augmented declaration, OR a transitive-`typeof` chain to a second decl is REJECTED (stays `Ignored`) — and any admitted snapshot whose `source_admission_digest.contributors` vector has MORE than one entry FAILS, proving the admitted set is exactly the offline-verifiable single-contributor class. |
| `no_orphan_snapshot` | SET-EQUALITY: derive `expected_paths` by recomputing each `snapshot_id` from the lifted-oracle-rows ⋈ registry-entries join using REGISTRY-ONLY inputs (the STABLE `env_corpus_id`, NOT the per-snapshot `oracle_env_hash` — the filename is registry-derivable, §Q4), recompute current workspace content hashes, and assert set-equality against a RECURSIVE on-disk enumeration of `oracle_snapshots/<family>/*.json`; then strict-decode each actual file, RE-ENUMERATE the vendored corpus for set-equality against the stored manifest + re-hash its stored `oracle_env_files.files` against on-disk content to recompute + validate `oracle_env_hash` (OFFLINE, no tsgo), and verify its `row_ref` / `snapshot_id` / env-pins / `oracle_value_kind` / `identity` match the registry-derived expectation. Catches leftover, fixture-edit-orphan, AND missing files in one comparison, with env drift (membership + content) caught on the value. |
| `snapshot_id_includes_row_ref` | The `snapshot_id` derivation includes `(row_file, row_function, query_ordinal)`; two distinct `(row, query)` get distinct files (no shared snapshots); `oracle_family` is NOT an input; the STABLE `env_corpus_id` IS an input (a pinned-env constant); the per-snapshot `oracle_env_hash` is NOT an input (it is validated on the value). |
| `snapshot_id_is_unique` | No two distinct registry `(row, query_ordinal)` entries derive the same `snapshot_id` (a proven uniqueness check over the registry, replacing an unproven "never collide" claim); the id is the FULL ≥256-bit BLAKE3 digest, not a 12-byte truncation. Discriminating: two registry entries differing only in `query_ordinal` must produce distinct ids. |
| `env_corpus_includes_resolution_metadata` | The captured `oracle_env_files` includes resolution METADATA — package manifests (`package.json` `types`/`exports`) and tsconfig/project metadata — not just `.d.ts` files. Discriminating: a `package.json` `types`/`exports` edit that re-selects a different `.d.ts` (`ipc.rs:3686`, `cache_invalidation.rs:324`) recomputes a different `oracle_env_hash` from the stored corpus (snapshot invalidated), which a `.d.ts`-only corpus would miss. |
| `standalone_host_is_default_canonical_config` | The default `host_setup_kind` is `standalone` (`make_host_with_footprint` = `VerterHost::new_standalone`, `support.rs:89`, `host_construction.rs:249`); standalone rows are driven under the SINGLE canonical `oracle.tsconfig.json` + synthetic root (a stable shared `compiler_options_hash`) INSIDE the closed vendored corpus (`oracle_env/<env_corpus_id>/`, a stable shared `env_corpus_id`), with tsgo's root pointed at that frozen corpus (not live `node_modules`). `standalone` is the ONLY first-class kind initially: EVERY admitted (`Lifted` oracle) row's `host_setup_kind` is `standalone`. `workspace_footprint` (the ~9-row minority) AND package-backed/custom-host are BOTH DEFERRED to the named env-pin spike (§4) — the `host_setup_kind` enum carries those discriminants for schema totality but no row is admitted under them. Discriminating: every standalone-row snapshot shares one `compiler_options_hash` AND one `env_corpus_id`; an admitted row whose `host_setup_kind` is `workspace_footprint` or package-backed/custom (before the env-pin spike lands) FAILS — it would assume per-host env the schema does not yet pin. |
| `oracle_options_delivery_proven` | A MULTI-OPTION delivery-proof MATRIX (not a single fixture): for EACH print-affecting option the closed effective-option map pins away from the tsgo default — at minimum `strictNullChecks`, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess` — a DISCRIMINATING fixture whose TS7 hover answer differs under the oracle value vs the default, asserting tsgo returned the ORACLE-value answer. This proves the generator's `compilerOptions` send (`ipc.rs:1111,1303`) delivered EVERY print-affecting option, not just one. Discriminating: a default answer on ANY matrix row FAILS generation; `compiler_options_hash_is_closed` alone (hash recipe only) cannot prove delivery, and a single-flag probe would miss a dropped second flag. |
| `oracle_query_specs_is_pure_data` | `oracle_query_specs.rs` is PURE context-neutral data (closed enums + owned strings, NO `super::support`, NO private unit-test types, NO helper calls) so it `include!`/`#[path]`-compiles in BOTH the `src` unit driver AND the `tests/` guard as ONE table; the helper-calling driver stays in `src`. Discriminating: the `tests/` guard `include!`s the same table and compiles without the unit-test support module. |
| `evaluate_expr_uses_scratch_prelude_model` | For `EvaluateExpr` the generator synthesizes a SEPARATE scratch file = the scope's `eval_source` prelude + a trailing `type __oracle_probe__N = <expression>;`, mirroring `crates/verter_session/src/typeinfo/evaluate_type_expression.rs:~314` (NOT a same-file append); `ResolveExpr` / `ShallowSurfaceExpr` use the same-file append (`support.rs:132,160`). Discriminating: an `EvaluateExpr` whose expression depends on a scope binding only reachable via the `eval_source` prelude binds correctly under the scratch model and would fail to bind under a bare append. |
| `evaluate_expr_admission_is_single_root` | An `EvaluateExpr` query is admissible (a `structured_type_expr` snapshot may exist) ONLY when its expression matches the CLOSED SINGLE-ROOT grammar (§Q2): one binder reference, optionally followed by a trailing index-access / property path whose every path-segment root resolves to that SAME single root binder (so its ONE `SourceLocator` walks every referent). A multi-/nested-referent expression — a `union`/`intersection` of two type expressions, a `keyof`/`typeof` over a compound, a utility application whose type argument is itself a referent (`ReturnType<typeof f>`), a namespace-qualified head, or a path segment naming a second binder — is OUT OF GRAMMAR and DEFAULT-REJECTED (stays `Ignored`, deferred to the multi-referent locator-set spike, §4). The generator rejects an out-of-grammar `EvaluateExpr` at probe synthesis. Discriminating: `typeof f` (single root) and `Foo['a']['b']` (single root + same-binder path) are admissible; `A \| B`, `ReturnType<typeof f>`, and `keyof T \| keyof U` are REJECTED — proving a non-leading lossy contributor can never slip through unchecked because the multi-referent expression is never admitted before the spike. |
| `snapshot_loading_is_runtime_fs` | The lift driver loads snapshots via runtime `std::fs::read` from `concat!(env!("CARGO_MANIFEST_DIR"), "/src/typeinfo/typeinfo_tests/oracle_snapshots/", oracle_family, "/", snapshot_id, ".json")` — the full `src/typeinfo/typeinfo_tests/` infix is present (not the bare `CARGO_MANIFEST_DIR` + `oracle_snapshots/` that resolves to the wrong dir); no `include_str!` / `include_bytes!` / `include_dir!` / generated include table appears in the consumption path. |
| `oracle_env_hash_pins_resolved_file_set` | The snapshot STORES an `oracle_env_hash` content-hashing the CLOSED VENDORED SHARED corpus tsgo consulted (vendored ambient + lib + package `.d.ts` + manifests + project metadata, spanning resolve/type/lib/project dims — the SHARED ambient/lib/package/tsconfig corpus ONLY; per-row workspace files are a distinct domain in `identity.workspace_files`, not in this hash), validated on read by recomputing it from `oracle_env_files.files` against on-disk content; a `compiler_options_hash` match alone does NOT validate a snapshot. The per-snapshot `oracle_env_hash` is NOT a `snapshot_id` input (the STABLE `env_corpus_id` is — the filename stays registry-derivable). Discriminating: a vendored ambient/lib/package `.d.ts` or manifest change with unchanged compiler options must recompute a different `oracle_env_hash` and invalidate the snapshot. |
| `registry_in_src_carries_oracle_family` | The oracle-query-spec registry lives at `src/typeinfo/typeinfo_tests/oracle_query_specs.rs` (reachable by the lifted unit tests), the `tests/` guard consumes the SAME table via a shared crate-internal path, and every entry carries an `oracle_family`. |
| `oracle_driver_basenames_file_macro` | The shared registry driver basename-normalizes `file!()` via `Path::file_name()` before the registry lookup, matching the bare-filename `IgnoredTestRow.file` / registry key (manifest discovery uses `path.file_name()`, `:796`/`:902`/`:1012`). |
| `lifted_body_is_self_keyed_macro` | Every lifted oracle row carries the `#[oracle_row]` ATTRIBUTE proc-macro (from the `verter_session_oracle_macro` dev-dependency crate), which reads the test fn's OWN `ItemFn` identifier (`sig.ident`) and synthesizes the `oracle::run_row(file!(), "<sig.ident>")` body — NOT a hand-typed string literal, and NOT a no-arg `oracle_row!()` declarative macro (a body-position declarative macro cannot see the enclosing fn name, so it is infeasible). The guard source-walks every `#[test]` fn in `src/typeinfo/typeinfo_tests/` that reaches the oracle driver and asserts (a) the fn carries the `#[oracle_row]` attribute and has NO hand-written `oracle::run_row(file!(), "…")` string-keyed body, AND (b) the key the attribute synthesizes (the attributed fn's own `sig.ident`) EQUALS the enclosing fn's identifier — so each lifted fn invokes EXACTLY its own `(file, function)` registry key. Discriminating: a fn that hand-writes `oracle::run_row(file!(), "bar_test")` (a mistyped/copy-pasted key naming another row) instead of carrying `#[oracle_row]` FAILS — without the attribute it would silently validate `bar_test`'s snapshots while every coverage/biconditional/count guard still passed (the wrong key is itself a real, fully-covered registry entry). |
| `identity_is_kind_specific_schema_bumped` | `identity` is a closed tagged shape keyed by `oracle_value_kind`; the `structured_type_expr` required axes are present, and the schema-version constant is bumped in lockstep with any new `oracle_value_kind` discriminant. |
| `snapshot_env_pin_matches_workspace` | Every snapshot's `tsgo_version == 7.0.0-dev.20260526.1`, `oracle_schema_version` == current, `normalizer_version` == current, `probe_synthesis_version` == current, `compiler_options_hash` == the oracle tsconfig effective-config hash, `env_corpus_id` == the closed vendored-corpus content id, and `oracle_env_hash` == the recomputed vendored-corpus hash (after corpus set-equality re-enumeration). |
| `tsgo_version_is_pinned` | `package.json` pins `@typescript/native-preview` to the exact `7.0.0-dev.20260526.1` (not `"latest"`). |
| `compiler_options_hash_is_closed` | The `compiler_options_hash` is computed over the EFFECTIVE tsgo config rooted in the COMMITTED canonical `oracle.tsconfig.json` (a vendored corpus member — the source of truth two implementers both read), via the CLOSED recipe (§Q2 "Env pinning"): the eight `strict` subflags expanded, the CLOSED ENUMERATED effective-option key table overlaid (committed value or `tsgo_version`-pinned default), canonicalized, SHA-256-hashed once. The option SET is CLOSED — there is NO open-ended "every option that affects the printed type" clause. The guard asserts `committed_tsconfig.compilerOptions.keys ⊆ closed_effective_option_keys` — the committed `oracle.tsconfig.json`'s OWN `compilerOptions` keys must ALL be members of the closed effective-option key table (OR the generator supplies a sanitized effective config containing exactly the closed map). Discriminating: a key PRESENT IN the committed `oracle.tsconfig.json` but OUTSIDE the closed effective-option key table FAILS (tsgo reads that committed tsconfig, so an un-hashed key in it would apply an un-owned option and un-pin the env — it is NOT enough to be "in the tsconfig"); a key in NEITHER the committed tsconfig NOR the closed table FAILS (an unowned injected option cannot enter); a future option dependency must be ADDED to the table under a `tsgo_version` bump, never folded in via a vague clause; the `tsgo_version`-pinned defaults table changes the hash on a tsgo upgrade that moves a default. |
| `normalizer_version_in_snapshot_id` | The `snapshot_id` derivation includes `normalizer_version`; bumping it changes every id (forces regeneration). |
| `snapshot_id_redrives_from_identity` | Re-deriving `snapshot_id` from a snapshot's `identity` (helper-kind + workspace file set + symbol/expression + type_args + mode + `host_project`) + row-ref + pinned env (`compiler_options_hash` + STABLE `env_corpus_id` + `tsgo_version` + `normalizer_version` + `probe_synthesis_version` + `oracle_schema_version`) under the canonical length-prefixed domain-separated encoding equals the filename stem and the stored `snapshot_id` — using REGISTRY-ONLY inputs, NO per-snapshot `oracle_env_hash`, so the expected filename is derivable from the registry alone. SEPARATELY, the stored `oracle_env_hash` is validated as a VALUE: the vendored corpus is re-enumerated for set-equality against the stored manifest, then `oracle_env_hash` is recomputed from the stored `oracle_env_files.files` against current on-disk content and asserted equal (env-drift caught on the value, not the filename). |
| `oracle_normalization_is_idempotent` | `normalize(normalize(x)) == normalize(x)` over a `TypeExpr` corpus, including nested-union/intersection flattening, total-structural-key member ordering, and overload-order preservation. Necessary but NOT sufficient for soundness — see `oracle_normalization_is_confluent`. |
| `oracle_normalization_is_confluent` | The CONFLUENCE soundness property (§Q2 "Confluence is the soundness property"): for DIFFERENTLY-SPELLED but equal admissible inputs `a ≡ b`, `normalize(a)` and `normalize(b)` are BYTE-EQUAL. The pipeline is run to a FIXPOINT (post-step-5 re-canonicalization each pass) so a step-5 reduction that re-exposes a step-2 obligation re-converges; soundness is local confluence + termination ⟹ global confluence (Newman's lemma). Distinct from `_discriminates` (which mutates ONE side) and `_is_idempotent` (which re-runs ONE spelling) — this guard feeds TWO equal-but-differently-spelled inputs through the SAME pipeline and asserts byte-equality. Discriminating, INCLUDING the rule-COMPOSITION cases a single pass would miss: side A `true \| false \| X` and side B `boolean \| X` (the ≥3-arm boolean case the exact-two-arm rule MISSED) MUST normalize byte-equal; `true \| false \| boolean` (step-5 boolean → `boolean \| boolean`, which the fixpoint re-dedups) and `boolean` MUST normalize byte-equal; `boolean \| true \| false` (arm-order-permuted) and `boolean` MUST normalize byte-equal; `string \| "a" \| "b"` (two literals both subsumed by a co-present `string`) and `string` MUST normalize byte-equal; `X \| never` (where X also reduces to `never`, so the whole union collapses to `never`) and `never` MUST normalize byte-equal; `X \| never` and `X` MUST normalize byte-equal; `X & unknown` and `X` MUST normalize byte-equal; `("a" \| string)` and `string` MUST normalize byte-equal; `1 \| 0x1` (a mixed-SPELLING duplicate — dedup at step 2 does NOT collapse it, the post-step-5 spelling-canon + re-dedup does) and `1` MUST normalize byte-equal; `0x2 \| 0x1` and `1 \| 2` (spelling-vs-sort-order — the step-2 sort key is computed pre-canon, the post-step-5 re-sort over the post-canon key reconciles arm order) MUST normalize byte-equal; an admissible construct whose two spellings are NOT proven to converge under the closed enumerated rule set is REJECTED (default-safe), not silently passed. Conversely, two UNEQUAL inputs (`true \| false \| X` vs `boolean \| Y`) MUST still diverge — confluence must not over-collapse. |
| `oracle_normalization_discriminates` | A wrong-member / wrong-optionality / wrong-primitive / wrong-overload-order mutation FAILS the structural compare (proves the reduction catches real divergence, not a stub). |
| `oracle_normalizer_terminates_on_cyclic_input` | The normalizer TERMINATES on an arbitrary `TypeExpr` including a CYCLIC one (§Q2 reduction step 0). A `RecursiveRef` back-edge is canonicalized to an opaque leaf and NEVER followed, and a VISITED-SET folds any re-entered node to that opaque leaf — so the walked term is a finite DAG, the §Q2 termination measure is well-founded, and the fixpoint is reached in finitely many passes (symmetric to the source-side `source_walk_cycle_rejected`, but Verter-side; the hover side cannot carry a back-edge). Discriminating: a Verter `TypeExpr` carrying a `RecursiveRef` (e.g. `type L = { next: L }`) normalizes in bounded time to a finite form with the `RecursiveRef` as an opaque leaf, rather than recursing without bound; the visited-set re-entry folds to the same leaf. |
| `oracle_literal_spelling_canonicalized` | Numeric / bigint LITERAL spellings that denote the SAME TS literal type are canonicalized to ONE form before compare (§Q2 step 5), closing the literal-only false-divergence the bounded subsumption rule does NOT cover: `1` / `1.0` / `0x1` / `1e0` all canonicalize to the same `LiteralValue` decimal form, and a literal-only union (`1 \| 2` vs `0x1 \| 0x2`) normalizes byte-equal. Numeric literals normalize to a canonical decimal (radix/exponent/redundant-fraction neutralized); bigint to a canonical decimal with the `n` suffix; string literals are compared by decoded value, not source quoting. A literal-spelling axis NOT covered by this canonicalization is DEFAULT-REJECTED (the default-safe posture), never passed through with raw spelling. Discriminating: `1 \| 2` and `0x1 \| 0x2` MUST normalize byte-equal (same literal-only set); `1 \| 0x1` (a mixed-spelling duplicate) MUST normalize to `1` (canonicalized at step 5 then collapsed by the post-step-5 re-dedup, since step-2 dedup ran before canon); `0x2 \| 0x1` and `1 \| 2` (spelling-vs-sort-order) MUST normalize byte-equal (the post-step-5 re-sort over the post-canon key reconciles arm order); `1.5` vs `1` MUST still DIVERGE (different literals — the canonicalization neutralizes SPELLING, not VALUE); a literal whose spelling axis is un-enumerated is rejected, not silently compared raw. |
| `binder_order_is_cross_side_stable` | The positional type-parameter / `infer` rename (`T0,T1,…`, §Q2 step 6) is confluent only when the Verter side and the hover-lowered side present the SAME ordered type-parameter binder list. A construct introducing type-parameter binders (a `Method` / `Function` / `ConstructorType` / call-/construct-signature carrying `type_params`) is admissible ONLY when its binder LIST + ORDER are PROVEN cross-side-stable (no tsgo reordering / omission of defaulted-or-inferred params / post-instantiation collapse); a construct whose binder list/order is not proven stable is REJECTED / deferred, never best-effort positional-renamed. Discriminating: a construct whose hover-side binder order differs from its source-side order (or whose hover omits a declared binder) FAILS / is rejected (it would otherwise positional-rename into false parity); a construct with identical ordered binders on both sides is admitted and renames byte-equal. |
| `oracle_normalization_canonicalizes_cosmetic_names` | The normalization reduction (§Q2 step 6) rewrites the CLOSED ENUMERATED set of cosmetic, non-identity-bearing binder/parameter names to deterministic positional placeholders before structural comparison: index-signature parameter name (`IndexSignature.key_name`, `lib.rs:715`, emitted `type_expr_json.rs:482`, source-preserved `oxc/lib.rs:488`) → `key`; function/method/call-signature/construct-signature parameter names (`FunctionParam.name`, `lib.rs:928`) → positional `p0,p1,…`; generic type-parameter names (`TypeParam.name`, `lib.rs:1019`) → positional `T0,T1,…` at binding + use sites via SCOPE-TRACKED binder resolution; mapped-key + `infer` names likewise. The axis is DEFAULT-SAFE: an un-enumerated identity-bearing name axis OR a `TemplateLiteral` (un-canonicalized in the initial scope) REJECTS the `(row, query)`, never passes the raw name through. Discriminating: (a) `{ [key: string]: T }` vs `{ [x: string]: T }` (the SAME TS type, differing `key_name`) normalizes to byte-equal `oracle_value` and COMPARES EQUAL; (b) a `Ref { name: "T" }` use site that binds to an enclosing generic binder `T` is renamed to that binder's `T0`, while a `Ref { name: "T" }` that binds to an UNRELATED top-level `type T = …` alias (no enclosing binder in scope) is left UNCHANGED — proving use-site resolution binds to STRUCTURE, not the raw identifier; (c) the rename does NOT mask a real difference (a member with a different TYPE still diverges); (d) a `TemplateLiteral`-bearing capture is REJECTED (default-safe), not passed through with raw quasi/expression spellings. Idempotent: re-running over an already-canonicalized form is a no-op. |
| `tsgo_not_reachable_from_resolver` | Crate-graph: `verter_session` default-build dep closure excludes `verter_type_runtime`; no tsgo spawn symbol reachable from the resolver. |
| `oracle_consumption_path_has_no_tsgo_spawn` | The lift driver + typeinfo test module reference no tsgo spawn. |
| `oracle_value_decodes_to_type_expr_strict` | Every snapshot's `oracle_value` (for `structured_type_expr`) decodes via `type_expr_from_json` AND round-trips byte-equal under re-encode. |
| `strict_snapshot_decode` | The decode path rejects any dropped/unknown member/param (no silent `filter_map` loss at `type_expr_json.rs:72,336`); a snapshot with a malformed member FAILS rather than decoding to a smaller `TypeExpr`. |
| `probe_header_names_target` | The generator rejects a hover whose header does not name the synthesized `__oracle_probe__N` (fences wrong-position false parity, since `get_hover` returns no range, `ipc.rs:1689`). The probe is built per `query_helper_kind` and placed in the query's own resolution environment — same-file append for `ResolveExpr` / `ShallowSurfaceExpr`, scratch-file + `eval_source` prelude for `EvaluateExpr` (`support.rs:132,160,208`); the check is re-runnable offline from the stored `raw_capture`. |
| `pre_lowering_loss_rejected` | The admissibility gate runs on the RAW PARSED AST BEFORE `lower_ts_type`. Discriminating: the branded `IdBranded<T> = T & { readonly [idBrand]: T }` (`branded_types.ts:9-11`) capture is REJECTED before lowering (a post-lowering check would pass it as `string & {}` → false parity). |
| `hover_construct_whitelist` | The generator walks the CLOSED POSITIVE ALLOWLIST (default-REJECT, §Q2) over OXC type-AST node kinds + resulting `TypeExpr` variants; ANY construct not explicitly ADMITted — `unique symbol` / computed-or-symbol-key / `this`-type / `this`-param / `const`-or-variance type-param / `abstract` ctor / overload-set / callable-intersection / optional-or-labelled-tuple-`\|undefined` / accessor / non-public-visibility / `RecursiveRef` / anything unanticipated — is rejected and its query is NOT lifted. |
| `source_declaration_allowlist_clean` | The positive allowlist is checked on BOTH the hover AST AND EVERY real DEFINING declaration of the queried symbol, resolved through the SHARED resolver's declaration graph (NOT a new walker) via the named entry API `resolve_source_declarations(&ResolverContext, &SourceLocator) -> SourceWalkResult` (§Q2): the typed `source_locator` (`reference_canonical`, `reference_name`, `symbol_space`) is resolved through the shared `ShallowFileState`/`IndexedReady` inventory + import/export/barrel routing + `MergedDecl` contributor set to a `Resolved { contributors: Vec<SourceContributor> }` — an ORDERED, KEYED vector where each `SourceContributor` carries its `(ordinal, decl_span)` identity (disambiguating same-`(canonical,name,symbol_space)` merged peers), its retained parse-time `RawSourceSurface` raw-fact record, AND its already-lowered body `TypeExpr`. The admission predicate reads the COMBINED input per contributor: the `RawSourceSurface` raw-fact record (§Q2 — `raw_member_keys`, `member_kinds`, `member_visibility`, `unique_symbol_ops`, `abstract_ctor`, `type_param_modifiers`, `this_type_or_param`, `value_const_assertion`, `overload_signatures`, `tuple_element_shape`, `utility_referent_names`) for the SILENTLY-ERASED facts (which the lowered body lost at `oxc/lib.rs:99,126,171,223,427,921`), PLUS the already-lowered `ShallowTypeSymbol.body`/`TypeDeclInfo.body` `TypeExpr` for the NON-erased rejectable variants that survive lowering as first-class `TypeExpr` (`Conditional`, `Mapped`, callable `FunctionType`/`ConstructorType`, `TemplateLiteral`, `Infer`, `KeyOf`/`IndexedAccess`/`TypeOf` outside spike-proven modes, enum-member `Ref`, `RecursiveRef`). Admission requires the hover AND EVERY resolved contributor's BOTH halves clean; an `Unresolved` (locator not bindable in the controlled fixture set, OR a bound contributor missing a retained `RawSourceSurface`) or `Cycle` result REJECTS. Discriminating: (a) a source decl whose body LOWERS clean of erased facts but whose RETAINED raw facts carry a dropped symbol-keyed member / accessor / `private` member / `unique symbol` / `abstract` ctor / overload SET must REJECT — proving admission reads the raw-fact record for erased facts; (b) a source decl whose RAW facts are clean but whose lowered body is a `Mapped` / `Conditional` / `TemplateLiteral` / callable must ALSO REJECT — proving admission reads the lowered body for the non-erased rejectable variants (the raw facts alone would miss these); (c) a symbol resolved in the wrong space (a `Foo` that exists as both a TYPE and a VALUE) must walk the space `symbol_space` names; (d) an imported/reexported symbol must REJECT/admit on its LEAF defining declaration (not the bare re-export node); (e) a `MergedDecl` whose contributors share a `(canonical,name,symbol_space)` triple but where ONE `(ordinal, decl_span)` carries a REJECT construct (in either half) must REJECT even when another `ordinal` is clean — proving the ordered keyed vector checks every contributor independently. |
| `raw_source_surface_captured_pre_lowering` | The `RawSourceSurface` raw-fact inventory (§Q2) is captured during the file's INITIAL PARSE (the same parse pass that builds the shallow inventory, while the transient OXC arena is live), stored on the file's content-addressed artifact beside `IndexedReady` keyed by `(canonical, name, symbol_space)`, and carries the closed pre-lowering admission-fact set (raw member keys incl. computed/symbol/unique-symbol, member kinds incl. accessors, declared visibility, `unique symbol` ops, `abstract` ctor, type-param `const`/variance, `this` type/param, `as const` provenance, ordered overload signatures, tuple element shape, raw utility referent names). It is OWNED `Send + Sync` data dropped/recomputed with its file artifact, never a borrowed AST pointer or retained parser arena, and is PARSE-derived (no type resolution, no five-mode dispatch) — so it is NOT a second resolver. Discriminating: a source declaration whose lowered body lost a `unique symbol`-keyed brand member still has that fact present in its retained `RawSourceSurface.raw_member_keys` (so the source-side allowlist can reject it); a file whose content hash changed recomputes the inventory rather than serving a stale capture. |
| `source_walk_is_transitive_through_typeof` | The source-side allowlist walk follows `typeof x` to `x`'s value INITIALIZER and `ReturnType<…>` / `Parameters<…>` to the referenced function's DEFINING declaration, re-entering the SHARED resolver's declaration graph at each hop (resolving `typeof` referents in VALUE space; following imports/barrels/merges through the one engine), walking the transitive source against the allowlist; unresolvable-through-the-shared-graph or non-allowlisted transitive source REJECTS. Discriminating: `type ObjectConstType = typeof objectConst` over an `as const` object (`value_inference.rs`, readonly-tuple contract `:106`) is admitted/rejected by the INITIALIZER's constructs, not the bare `typeof` alias node; a `ReturnType<typeof f>` where `f` is imported from a barrel walks `f`'s leaf body. |
| `source_walk_cycle_rejected` | The transitive `typeof`/`ReturnType`/`Parameters` source-side walk maintains a VISITED SET of `(canonical, name, symbol_space)` keys and REJECTS the `(row, query)` on re-entering an already-visited key (a cyclic source surface is not admissible — it defers, never hangs or best-effort admits). The walk re-enters the shared resolver's declaration graph at each hop, so it also inherits the engine's own generic-helper recursion termination guarantee (`RefCycleResultDb`, CLAUDE.md); the visited-set is the harness-side belt-and-suspenders. Discriminating: a cyclic `type A = typeof b; const b: A` (or a mutually-`ReturnType`-referencing pair) is REJECTED rather than hanging generation; an acyclic chain walks to its leaf and admits/rejects on the leaf's constructs. |
| `enum_member_refs_rejected` | An enum-member-typed query (`Color.Red`, `Status.Idle`, `Direction.Up`; alias `type ColorRed = Color.Red`, `fixtures/enums.ts:21,26`; branded contracts `enums.rs:18,39`) is DEFAULT-REJECTED — `TypeExpr` has no enum-member carrier (`lib.rs:128`), so an enum-member `Ref` is not on the positive ADMIT list. Discriminating: an enum-member capture must be rejected, a plain literal/object accepted. |
| `shallow_hover_expansion_rejected` | In `Shallow` / `Navigate` mode, a hover that EXPANDS a userland alias instead of printing its name is REJECTED/deferred (symmetric to the Expanded-mode unexpanded-`Ref` reject) — Verter correctly keeps the `Ref`. Discriminating: a shallow-mode capture where tsgo printed the expanded object must be rejected; one that printed the alias name accepted. |
| `probe_form_is_deterministic_and_versioned` | The probe is ALWAYS placed in the query's own resolution environment — same-file append into `primary_canonical` for `ResolveExpr` / `ShallowSurfaceExpr`, scratch-file + `eval_source` prelude for `EvaluateExpr` — with deterministic naming `__oracle_probe__<query_ordinal>`, versioned by `probe_synthesis_version` (in `snapshot_id`); the locator is derivable from version+query without tsgo. The `Expanded`-mode probe form is admissible only after the spike fixes + versions it. |
| `probe_binds_to_registry_target` | The generator verifies the appended probe's RHS binds to the registry's intended declaration (`(primary_canonical, symbol)` / `source_locator`), not a shadow/ambient, via a tsgo definition/diagnostic check (zero new diagnostics + the probe symbol's definition lands on the intended decl); a mismatch FAILS generation. Discriminating: a same-name shadow/ambient that the probe would bind to instead must FAIL generation. |
| `anti_shadow_needs_proven_binding_primitive` | The anti-shadow half of `probe_binds_to_registry_target` rests on a binding-identity primitive (`textDocument/definition` or a versioned equivalent) NOT YET VERIFIED at the pinned tsgo (§4 spike). NO row that NEEDS the anti-shadow check (any symbol that is not PROVABLY un-shadowable) is admissible until the spike PROVES a concrete primitive at `7.0.0-dev.20260526.1`. The ONLY rows admissible WITHOUT it are PROVABLY UN-SHADOWABLE: a UNIQUE TOP-LEVEL name in a SINGLE-FILE standalone fixture with NO ambient-corpus contribution for that name (no augmentation, no same-named import/reexport, no second declaration on the resolution path). Discriminating: a multi-file / imported / merged / corpus-co-named symbol is BLOCKED (stays `Ignored`) absent the proven primitive; a single-file unique-top-level-name symbol is admissible via the vacuous-binding + zero-new-diagnostics path. |
| `strict_lowering_drop_counter` | `lower_ts_type` is instrumented so any `filter_map`-dropped member/param (`oxc/lib.rs:99`; `type_expr_json.rs:72,336`) increments a drop count; a non-zero count REJECTS the capture (belt-and-suspenders on the AST walk). |
| `class_visibility_accessor_rejected` | A `private`/`protected` class member or a getter/setter accessor capture is REJECTED (unrepresentable from hover: visibility is identity-bearing — `lib.rs:494` — but OXC lowers public only — `oxc/lib.rs:427`; accessors are not an `ObjectMember` variant — `lib.rs:426`). |
| `hover_capture_is_lossless_or_rejected` | Backstop: the generator rejects a capture with an ellipsis/truncation marker, parse leftovers / `Unknown`, an unexpected `any`/`never`, or an unexpanded userland `Ref` in `Expanded` mode (unless alias preservation is the explicit answer). Discriminating: a truncated/`Unknown`/unexpanded-`Ref` fixture must be rejected, an admissible one accepted. |
| `non_admissible_query_not_lifted_via_hover` | No registry query that fails pre-lowering/whitelist/drop-counter/backstop admissibility is marked `Lifted` with a `structured_type_expr` snapshot; it stays `Ignored` until a future oracle kind exists. |
| `parity_claim_is_structural_type_expr` | The harness compares normalized `TypeExpr` (Verter projection vs hover-lowered) — NOT full TS nominal/semantic identity. A nominal construct `TypeExpr` cannot carry (enum-member brand, `unique symbol`, private/protected brand, `this`-type, `const`/variance type-param, abstract-ctor) is out of scope for hover sourcing and DEFAULT-REJECTED; a legitimate structural divergence on such a construct is DEFERRED, never force-fit. |
| `default_reject_is_the_rule` | The construct gate is a CLOSED POSITIVE ALLOWLIST; the enumerated REJECT entries are ILLUSTRATIVE. ANY construct not on the positive ADMIT list (named or unnamed) is rejected by default. Discriminating: an un-enumerated lossy construct introduced into a fixture is rejected without a new reject-list entry — the gate's soundness does not depend on enumerating it. |
| `deferred_classes_not_lifted_via_hover` | Rows with a genuine `any`/`never` answer (`Parameters<any>` / `Awaited<never>` …, `typeinfo_ignored_test_manifest_rows.rs:340,343,347`), `workspace_footprint` rows (per-host project config, deferred to the `workspace_footprint` env-pin spike, §4), and package-backed / custom-host rows (`make_package_host_with_workspace`, `cache_invalidation.rs:344`) are NOT lifted via this hover harness; the deferred host classes re-enter only once the named env-pin spike pins their per-host env + full consulted corpus (§Scope), NOT automatically; the backstop's `any`/`never` reject stays strict and is not weakened for them. |
| `raw_capture_present_for_audit` | Every snapshot stores `raw_capture` (`{ probe_name, probe_header, hover_contents }`); `probe_header_names_target` audits the wrong-hover fence offline from it without re-running tsgo. Discriminating: a snapshot missing `raw_capture` FAILS the guard. |
| `expanded_probe_form_validated` | An `Expanded`-mode query is admissible only after the spike has validated a concrete lossless `Expanded`-probe form for its construct class; un-validated `Expanded` classes stay `Ignored`. |
| `skeleton_probe_form_validated` | A `Skeleton`-mode query is admissible only after the spike has validated a concrete lossless `Skeleton`-probe form for its construct class — one that elicits tsgo's `TypeParameter`/`Infer` shell printing for unbound generics so Conditional branches do not collapse to `never`; un-validated `Skeleton` classes stay `Ignored` (default-REJECT, mirroring `Expanded`). Discriminating: a `Skeleton`-mode row whose construct class has no spike-validated probe form must stay `Ignored`/rejected; one in a spike-validated class is admissible. |
| First-lift PREREQUISITE set: regenerator source-model + the 362-table guard + THREE all-row-sensitive guards (the COUNT guard `:595` is already status-filtered) | The COMPLETE, accurate prerequisite set for the first `Ignored → Lifted` row, all landing in the SAME block (§Q5): **(1) Regenerator source-model redesign** — under the original all-`Ignored` model `scripts/gen-typeinfo-ignore-manifest.py` built rows ONLY from live `#[ignore]` discovery, hardcoded `status: IgnoreStatus::Ignored`, and asserted exactly 362 built, so a `Lifted` row (no live `#[ignore]`) would VANISH on regeneration. The generator now UNIONS live discovery with a retained `Lifted`-row ledger (`LIFTED_ROW_OVERRIDES`), sources `status` FROM the ledger (not a hardcoded constant), asserts the two sets are disjoint, and asserts the TOTAL union is 362 (the `Ignored` count falls as lifts accrue; the total stays 362). **(2) `ignored_test_row_table_holds_exactly_362_rows` (`:1155`) reconciliation** — it asserts BOTH `EXPECTED_IGNORE_MANIFEST.len() == 362` (raw total — STAYS 362 because a `Lifted` row remains in the table) AND the status-filtered live-ignore count, now `EXPECTED_TOTAL_IGNORED_COUNT == 362 - lifted_count` (= 358 after the 4 lifts). **(3) THREE all-row-sensitive guards status-filtered** — `manifest_length_matches_documented_total` (`:989`, raw `.len()` vs status-filtered count), `every_manifest_row_corresponds_to_a_live_ignored_test` (the orphan-on-no-`#[ignore]` check, `:828`), `per_file_ignored_test_counts_match_manifest` (the per-file partition, `:1018`) all over `status == Ignored` rows only (a `Lifted` row carries NO live `#[ignore]` by design). The status-filtered COUNT guard `EXPECTED_TOTAL_IGNORED_COUNT = count_ignored_rows(…)` (`:595`/`:561`/`:566-575`) needed NO edit. This landed with the first lift; the "only THREE guards" framing was inaccurate. |
| `lifted_row_overrides_retain_full_record` | The retained-lift metadata map (`LIFTED_ROW_OVERRIDES`) stores the FULL `IgnoredTestRow` payload per lifted row — the 12 data columns (`file`, `function`, `block_id`, `semantic_queries`, `proof`, `substrate`, `capability`, `organ`, `owning_u_block`, `mechanism_id`, `consumed_mechanisms`, `unblocker`) plus `status` (the 13th field) — so the regenerated table reproduces the lifted row VERBATIM with only `status: Lifted { block_id }` substituted, since live `#[ignore]` discovery can no longer supply ANY column once the `#[ignore]` is removed. Asserts every retained-lift record has a NON-EMPTY `unblocker` (so `every_manifest_row_has_non_empty_unblocker` at `:852` STILL holds on lifted rows, unchanged + un-status-filtered — the retained-lift metadata is its source) and a `proof` of `Ts7Oracle(_)` / `OracleAndGuard{..}` (a divergence row is an `OracleAndGuard` whose `guard` is the `DivergenceCorrection` prover). An independent non-`TypeExpr` obligation is expressed by the `OracleAndGuard` proof shape + its registered live prover — there is NO stored `non_typeexpr_obligations` ledger set (the round-2 full-record obligation ledger was retired). Discriminating: a retained-lift record that drops `unblocker` (or any column the all-rows manifest guards read), or demotes a footprint/audit/divergence row to bare `Ts7Oracle`, FAILS; a regenerated lifted row whose columns differ from the retained-lift record FAILS. (When the deferred §Q4 `oracle_query_ordinals` per-row-count field lands it will ride on this same retained-lift metadata, as will the deferred `migration_fingerprint` / `original_body_tokens` body-hash fidelity layer when wired — §Q4. Neither is a currently-stored column.) |
| `lifted_row_block_id_matches_status` | The retained-lift record's `block_id` field (the row's own `block_id` manifest column, one of the 13 `IgnoredTestRow` columns) and the `IgnoreStatus::Lifted { block_id }` status-payload `block_id` the regenerated row carries are INTENTIONALLY the SAME value — the lifting block IS the row's block, recorded once. The regenerator emits `status: IgnoreStatus::Lifted { block_id: rec.block_id }`, so they cannot drift in a single regeneration; this guard asserts the equality on every lifted row in the table (the emitted row's `status` payload `block_id` EQUALS the row's `block_id` column). Discriminating: a regenerated `Lifted` row whose `status` payload `block_id` differs from its `block_id` manifest column FAILS — proving the two are one value, not two independently-maintained fields that could disagree. |
| Per-row lift tests (added per block) | Each lifted oracle query runs the single-spec resolver ONCE and asserts against recorded data (`ts-compat-two-mode-model.md` §7), NOT a per-mode re-run or a family-key comparison; corrections bind at `(row, query_ordinal)` granularity, so a row may MIX the two cases below: **(a) ordinary query (no correction)** — `resolver(query) == normalized oracle_value` (the recorded `TsCompat`/tsgo value, which here equals the correct value); **(b) corrected query** (a correction overlay exists for that `query_ordinal`) — `resolver(query) == correction.correct_value` (the `Correct` value), while that query's `snapshot.oracle_value` is the recorded `TsCompat` value and must differ. Both keep the explicit negative assertions (no `Unknown`, no `any`/`never` where a concrete type is expected). |

Generator-side (feature-gated, NOT in the default gate):

| Tool / test | Asserts |
| --- | --- |
| `oracle-gen` binary/target (`cargo run`-style, per the "generators are scripts, not tests" rule) | VENDORS the closed oracle-env corpus (copies the BYTES of every consulted file — canonical `oracle.tsconfig.json`, lib / ambient / package `.d.ts`, resolution manifests — into the checked-in `oracle_env/<env_corpus_id>/` dir, computing `env_corpus_id`), synthesizes the fixed+versioned probe per `query_helper_kind`, drives tsgo via the LSP hover path (Q3) AGAINST THE VENDORED CORPUS ROOT (not live `node_modules`), applies the PRE-LOWERING POSITIVE ALLOWLIST (default-REJECT, two-sided source+hover) + strict drop-counter + backstop (Q2), lowers + normalizes, records the `oracle_env_files` manifest + `oracle_env_hash` over the vendored corpus, writes snapshots. Idempotent. NEVER writes from a `#[test]`. |
| `oracle_gen_is_idempotent` (gated) | Re-running the generator over an unchanged workspace + env produces byte-identical snapshots. |

### Spike (pre-mass-lift — BLOCKING)

Before any mass row lift, run a generation SPIKE. It is a GATING design input, not a
confirmation step: a construct class is not admissible until the spike proves it.
Confirm three things:

1. **Probe-form + positive-allowlist admission validation.** For type-level,
   value-level, and `Expanded`-mode captures, confirm the per-`query_helper_kind` FIXED
   probe forms hover correctly against the pinned tsgo (header names the probe), and
   confirm the PRE-LOWERING POSITIVE ALLOWLIST (default-REJECT, checked two-sided on
   source + hover) + strict drop-counter correctly admit/reject across the hard
   families. Validate the allowlist's COMPLETENESS: every construct present in the
   spiked corpus is either an enumerated ADMIT or falls through to the default REJECT
   — no construct silently admitted. Any family — or any individual query within a
   family — whose source or hover is NOT admissible is recorded as needing a future
   structured oracle and is NOT lifted
   via this harness.
1a. **Normalizer CONFLUENCE validation over the admissible set (BLOCKING — the central
   soundness obligation).** Confluence (`a ≡ b ⟹ normalize(a) == normalize(b)` for
   differently-spelled equal inputs, §Q2 "Confluence is the soundness property") — NOT
   merely idempotence — is what makes the strict-equality compare sound. **This is a TRUE
   GATE, not a formality or a confirmation step: NO construct class is admitted until its
   confluence has been EMPIRICALLY proven over tsgo's ACTUAL hover spellings at the pinned
   `7.0.0-dev.20260526.1`.** A class whose two-sided spellings have not been spike-observed
   and proven to converge stays `Ignored` — the spike's per-class verdict GATES admission,
   it does not retroactively bless an already-admitted class. The spike MUST
   discharge the confluence proof obligation over the admissible construct corpus: for
   each candidate construct class, exhibit the two distinct spellings the two sides
   actually produce (Verter projection vs tsgo hover-lowered, observed from the real pinned
   tsgo — not assumed) and confirm the closed
   neutral-element / absorption / canonicalization rule set (§Q2: `X|never→X`,
   `X&unknown→X`, `X|unknown→unknown`, `X&never→never`, dedup, sort, flatten, the
   co-presence boolean rule, bounded literal subsumption, parenthesis/span strip,
   cosmetic-name → positional placeholder) drives BOTH spellings to a BYTE-EQUAL canonical
   form. Any admitted construct whose two spellings are NOT proven to converge under that
   closed rule set is recorded as needing a confluence rule (a `normalizer_version` bump)
   OR is DEFAULT-REJECTED until one is proven — it is never admitted with an unproven
   spelling difference. The spike specifically validates the ≥3-arm boolean case
   (`true|false|X` vs `boolean|X`) the exact-two-arm rule missed AND the rule-COMPOSITION
   cases a single pass would miss (`true|false|boolean`, `boolean|true|false`,
   `string|"a"|"b"`, `X|never` where X reduces to `never`), confirming each re-converges
   under the FIXPOINT (the post-step-5 re-canonicalization), and the literal-value
   spelling cases (`1|2` vs `0x1|0x2`). It also confirms the rewrite system is locally
   confluent (each critical pair re-converges) AND terminating (the lexicographic measure
   strictly decreases), so by Newman's lemma the normal form is unique — and that the
   normalizer terminates on a CYCLIC Verter `TypeExpr` (a `RecursiveRef` opaque leaf +
   visited-set, reduction step 0). Pinned by `oracle_normalization_is_confluent`,
   `oracle_normalizer_terminates_on_cyclic_input`, and
   `oracle_literal_spelling_canonicalized`.
2. **`Expanded`-mode probe form (per construct class).** An `Expanded`-mode row is admissible
   once a concrete, demonstrably lossless `Expanded`-probe form is validated for its construct
   class (preserving methods / call-signatures / optional / readonly — a `{ [K in keyof T]: T[K] }`
   wrapper does NOT, and is rejected); a class without a validated probe form stays `Ignored`.
   Pinned by `expanded_probe_form_validated`. VALIDATED classes: index-signature publication
   (`NumericIndexed` / `SymbolIndexed`) and built-in modifier-utility composition
   (`Required<T>` / `Readonly<Required<T>>`) — the four lifted rows are captured + compared in
   `Expanded` (the alias body is already a terminal structural surface, so the append-probe
   hover is lossless).
2-skeleton. **`Skeleton`-mode probe form (BLOCKING).** `Skeleton`-mode rows are NOT
   admissible until the spike validates a concrete, demonstrably lossless
   `Skeleton`-probe form for their construct class — one that elicits tsgo's
   `TypeParameter`/`Infer` shell printing for unbound generics so Conditional branches do
   NOT collapse to `never`. Until then those rows stay `Ignored` (default-REJECT, mirroring
   `Expanded`). Pinned by `skeleton_probe_form_validated`.
2a. **Parameterized-`ResolveExpr` `TypeExpr` → TS-source printer (BLOCKING).** A
   `ResolveExpr` query with NON-EMPTY `type_args` is NOT admissible until the spike
   stands up the deterministic, versioned `TypeExpr` → TS-source type-argument printer
   (§Q2) and proves its round-trip property (print → re-lower → structurally-equal-
   under-normalization) over the printable construct set; a non-printable argument
   DEFAULT-REJECTS. Until the printer is validated + versioned (a
   `probe_synthesis_version` bump), non-empty-`type_args` rows stay `Ignored`;
   empty-`type_args` `ResolveExpr` rows use the fixed bare-`symbol` RHS NOW. Pinned by
   `parameterized_probe_rhs_synthesis`.
3. **Registry encodes every helper kind.** Confirm the oracle-query-spec registry's
   closed `query_helper_kind` enum can encode every lifted query — `ResolveExpr`
   (`resolve_expr`, `support.rs:132`), `ShallowSurfaceExpr` (`shallow_surface_expr`,
   `:160`), `EvaluateExpr` (`evaluate_expr`, `:208`) — plus multi-file workspaces, so
   no lifted query is unrepresentable in the registry.

The hard families to spike for admissibility:

- **Mapped / conditional / infer types** — `Mapped`, `Conditional`, `Infer` shells;
  check tsgo does not collapse open conditionals or elide mapped modifiers in hover.
- **Overloads** — multi-signature call/construct groups; expected NON-admissible
  (overload sets are a REJECT construct) — confirm they are rejected, not silently
  collapsed.
- **Branded / nominal types** — `unique symbol` + symbol/computed-keyed brand members
  (`branded_types.ts`); expected NON-admissible — confirm pre-lowering rejection.
- **Class surfaces** — `private`/`protected` visibility and getter/setter accessors;
  expected NON-admissible (REJECT set).
- **Large object surfaces** — many-member objects; the prime truncation risk (`…`).
- **Template literals** — `TemplateLiteral` quasis/expressions preserved in order.
- **Variadic tuples** — `Rest` elements and labelled tuple members; watch the
  optional-tuple `| undefined` ambiguity (a REJECT construct).
- **Apparent types** — primitive/boxed apparent-type surfaces.

Additional BLOCKING spike items (each gates admission of the class it covers):

- **`ConstructorType` ⇄ `Object { ConstructSignature }` normalizer equivalence
  (BLOCKING before admitting single construct signatures).** A construct signature can
  surface as a top-level `ConstructorType` OR as an `Object` carrying a single
  `constructSignature` member, and tsgo's hover may print either spelling for the same
  type. Before any single-construct-signature row is admitted, the spike MUST validate a
  normalizer equivalence that canonicalizes the two spellings to ONE form (or default-
  reject the class if no lossless equivalence exists). Until validated + versioned (a
  `normalizer_version` bump), single-construct-signature rows stay `Ignored`. This
  prevents a false divergence between a `ConstructorType`-spelled Verter projection and
  an `Object{constructSignature}`-spelled hover.
- **The anti-shadow binding-identity primitive (BLOCKING; the anti-shadow check rests
  on an UNVERIFIED `textDocument/definition`).** `probe_binds_to_registry_target`'s
  anti-shadow half depends on tsgo answering a binding-identity query for the probe
  symbol (the symbol's definition must land on the registry's `source_locator`
  declaration). The transport REFERENCES `textDocument/definition` (`ipc.rs`) but it is
  NOT VERIFIED to be implemented/usable at `7.0.0-dev.20260526.1`. The rule is therefore
  EXPLICIT and gating:
  - **No row that NEEDS the anti-shadow check is admissible until a CONCRETE
    binding-identity primitive is PROVEN at the pinned tsgo.** A row needs the
    anti-shadow check whenever its probe could bind to a shadow / ambient re-declaration
    / unrelated import rather than the intended declaration — i.e. any row whose symbol
    is NOT provably un-shadowable. The spike MUST CONFIRM a concrete primitive that
    returns a usable binding location for a probe symbol at the pinned version
    (`textDocument/definition` if it works; else a versioned EQUIVALENT — a
    rename/references probe, or a diagnostic-based binding proof). If NO such primitive
    is provable at the pinned tsgo, EVERY anti-shadow-needing row is BLOCKED (stays
    `Ignored`) — the spike must name the concrete proven fallback OR block those rows;
    it may not hand-wave "find an equivalent if unavailable".
  - **The ONLY rows admissible WITHOUT a proven binding-identity primitive are
    PROVABLY UN-SHADOWABLE ones:** a query whose symbol is a UNIQUE TOP-LEVEL name in a
    SINGLE-FILE standalone fixture with NO ambient corpus contribution for that name (no
    `declare module`/`declare global` augmentation, no import/reexport of a same-named
    symbol, no second declaration of the name anywhere on the resolution path). For such
    a row the probe's RHS can bind to exactly one declaration by construction, so the
    anti-shadow check is vacuously satisfied and the zero-NEW-diagnostics half of
    `probe_binds_to_registry_target` suffices. Multi-file rows, imported/reexported
    symbols, merged declarations, and any name that ALSO appears in the ambient/lib/
    package corpus are NOT provably un-shadowable and remain blocked until the primitive
    is proven. Pinned by the `anti_shadow_needs_proven_binding_primitive` guard
    (admissibility of an anti-shadow-needing row requires the proven primitive; only the
    provably-un-shadowable single-file unique-name class is admissible without it).
- **The multi-referent `EvaluateExpr` locator-set spike (BLOCKING for multi-/nested-
  referent expressions).** Initial `EvaluateExpr` admission is restricted to the closed
  SINGLE-ROOT grammar (§Scope, §Q2): one binder reference + an optional same-binder
  index/property path, walked from ONE `SourceLocator`. Multi-/nested-referent
  expressions (`A | B`, `keyof T`, `ReturnType<typeof f>`, indexed-access roots across
  two binders, namespace heads) are DEFAULT-REJECTED because a single `SourceLocator`
  cannot allowlist-check every referent. Admitting them is gated on this spike, which
  MUST: (a) define a parsed referent SET / tree the registry stores in place of a single
  `SourceLocator` (every leaf referent named with its `symbol_space`); (b) extend the
  source-side walk to resolve + allowlist-check EVERY referent's defining contributor(s)
  through the shared resolver (each under the existing visited-set + cycle guard); (c)
  prove the source_admission_digest records every referent's `RawSourceSurface` so the
  offline re-derivation covers them all. Until it lands, multi-/nested-referent
  `EvaluateExpr` rows stay `Ignored`. Pinned by `evaluate_expr_admission_is_single_root`
  (which enforces the default-reject posture until the spike admits the broader grammar).
- **The offline contributor-set-membership-revalidation spike (BLOCKING for
  multi-contributor rows).** Initial admission is RESTRICTED to PROVABLY SINGLE-CONTRIBUTOR
  rows (§Scope): a single-file declaration with NO import / merge / augmentation / transitive
  `typeof`/`ReturnType` hop, whose contributor set is trivially `{the one decl}` and so is
  fully offline-verifiable. Multi-contributor rows (an imported / re-exported symbol, a
  `MergedDecl` peer group, an ambient-augmented declaration, a transitive-`typeof` chain) are
  DEFAULT-REJECTED because the offline `source_admission_digest_consistent` gate cannot
  re-navigate the declaration graph to PROVE the recorded contributor SET is complete — a
  digest that omitted a contributor could self-validate. This spike MUST define how the
  recorded contributor SET is re-validated OFFLINE without the live resolver — concretely
  whether to (a) record an offline-checkable structural witness of the membership walk (e.g.
  the resolved import/merge/augmentation edge set, each edge re-derivable from the recorded
  contributor sources) that a guard re-walks tsgo-free, (b) prove a bounded class of
  multi-contributor shapes whose membership IS offline-reconstructible from the recorded
  sources, or (c) leave the multi-contributor class permanently deferred to a future
  structured oracle. Until it lands, multi-contributor rows stay `Ignored`; the source-side
  walk still RECORDS every contributor it navigated (so the spike has the data to design
  against), but no row with >1 contributor is admitted. Pinned by
  `source_is_provably_single_contributor` (which enforces the single-contributor default-reject
  until the spike admits the broader class).
- **The `workspace_footprint` per-host env-pin spike (BLOCKING for the ~9-row workspace
  class).** Only `standalone`-host rows are first-class initially (§Scope, §Q1
  host-setup): they share ONE canonical `oracle.tsconfig.json` + ONE closed
  `env_corpus_id`, which is what makes the tsgo-free `snapshot_id` derivation airtight.
  A `workspace_footprint` row drives tsgo under its OWN `/workspace/tsconfig.json`, so its
  effective options + consulted env are PER-HOST, not the one shared corpus. This spike
  MUST decide how a per-host project config + consulted env are pinned and
  offline-re-derivable WITHOUT breaking the single-shared-corpus `snapshot_id` derivation
  — concretely whether to (a) introduce a per-host `compiler_options_hash` + per-host
  vendored corpus keyed into `snapshot_id` as additional pinned-env constants, or (b)
  prove the ~9 workspace rows actually reduce to the canonical standalone config (and
  fold them in), or (c) leave them permanently deferred to a future structured oracle.
  Until the spike lands, `workspace_footprint` rows stay `Ignored`; the `host_setup_kind`
  enum carries the discriminant for schema totality but no row is admitted under it.
  Pinned by `standalone_host_is_default_canonical_config` (which asserts every admitted
  row is `standalone` under the one shared config + corpus until the spike admits the
  workspace class).
- **`EvaluateExpr` `eval_source` prelude must reproduce EVERY synthesised lookup binding
  — ambient augmentation AND any SFC-synthesised prelude binding (BLOCKING).** The
  `EvaluateExpr` probe synthesizes a scratch file = the scope's `eval_source` prelude +
  the trailing probe. If the scope's real lookup environment depends on a binding the
  flattened `eval_source` prelude does NOT reproduce, the probe's lookup env would
  DIVERGE from Verter's. Two distinct binding sources can be synthesised into the scope
  rather than written verbatim in `eval_source`: (1) AMBIENT module/global AUGMENTATION
  (a `declare module` / `declare global` contribution); and (2) ANY OTHER
  SFC-SYNTHESISED PRELUDE BINDING the SFC→TSX transform injects — e.g. the
  `.vue`-synthesised `default` export binding, synthesised macro-surface bindings
  (`defineProps`/`defineEmits`-derived locals), or any binding the flattened `eval_source`
  prelude does not carry verbatim. If the scope's lookup env depends on EITHER source and
  the flattened prelude omits it, the probe diverges. The spike MUST either (a) confirm
  the admitted `EvaluateExpr` rows have a SELF-CONTAINED `eval_source` prelude — free of
  ambient augmentation AND of any non-reproduced SFC-synthesised binding — restricting
  admission accordingly; OR (b) validate that the flattened prelude faithfully reproduces
  tsgo's lookup env INCLUDING every such synthesised binding. Until one holds,
  `EvaluateExpr` rows whose lookup env depends on ambient augmentation OR any
  SFC-synthesised prelude binding stay `Ignored`.
- **The concrete tsgo `compilerOptions` init/config payload shape + the vendored-lib
  forcing mechanism (BLOCKING for option delivery AND corpus hermeticity).**
  `oracle_options_delivery_proven` requires the generator to SEND the pinned
  `compilerOptions` to tsgo, but the in-repo init sends only
  `processId`/`capabilities`/`rootUri`/`workspaceFolders` (`ipc.rs:1111`) and the
  paths-config sends only `paths` (no `baseUrl`, `ipc.rs:1303`). The spike MUST establish
  the CONCRETE payload shape tsgo `7.0.0-dev.20260526.1` accepts for the effective
  `compilerOptions` (whether via `initializationOptions`, a
  `workspace/didChangeConfiguration` block, or a vendored `tsconfig` tsgo reads from the
  corpus root) and prove — via the MULTI-OPTION delivery-proof matrix (one discriminating
  fixture per print-affecting pinned option, §Q2 / `oracle_options_delivery_proven`) — that
  EVERY pinned option was actually applied. The SAME payload-shape spike MUST validate the
  NAMED vendored-lib forcing candidate (§Q2 "Env pinning" → "the CONCRETE vendored-lib
  mechanism"): the canonical `oracle.tsconfig.json` setting `"noLib": true` + an EXPLICIT
  vendored lib file list (corpus-rooted), with the ALTERNATIVE `"lib"`+corpus-rooted
  `typeRoots`/`paths` candidate as the fallback — proving (via a lib-sensitive fixture
  whose hover answer changes if tsgo's NATIVE-bundled libs leak in) that tsgo honours ONLY
  the vendored libs. If NEITHER candidate forces tsgo off its bundled libs at the pinned
  version, the lib-dependent row class stays DEFERRED (`Ignored`) — the harness never
  drives tsgo against its non-hermetic bundled libs. Until the payload shape is
  established, the vendored-lib forcing is proven (or the lib class is declared deferred),
  AND every matrix row is proven, no row whose answer depends on a non-default option OR a
  lib surface is admitted.

The spike output drives the per-query admissibility verdicts that gate the first lift
block; it is generator-side work (feature-gated), never part of the default gate.

---

## 5. Open risks

1. **tsgo hover truncation / aliasing.** tsgo hover may print `Pick<Foo, "bar">`
   rather than the expanded object, or truncate large types with `…`. For
   `Expanded`-mode queries the generator depends on a spike-validated forced-expansion
   probe (a BLOCKING prerequisite, §4), and the backstop rejects `…`/truncation and
   unexpanded userland `Ref`. A rejected capture defers its query to the future
   structured oracle.

2. **OXC-lowering the hover text could diverge from how OXC lowered the original
   fixture.** Both sides go through `lower_ts_type`, so systematic lowering quirks
   cancel out — but a quirk that differs between "hover-printed TS" and "authored TS"
   could create a false divergence. Mitigation: the normalization reduction (§Q2) is
   the canonicalization layer (versioned by `normalizer_version`); extend it if a
   quirk surfaces, never patch the resolver. The known-lossy lowerings (`unique
   symbol`, computed/symbol keys, `this` type/param, `const`/variance type-params,
   `abstract` ctors, accessors, non-public visibility, overload sets) are fenced ahead
   of time by the PRE-LOWERING POSITIVE ALLOWLIST (default-REJECT, checked two-sided on
   source + hover) — they are rejected on the raw OXC AST before lowering can erase
   them, so they can never reach a snapshot as silent false parity.

3. **A future structured / compiler / nominal oracle is unverified at the pinned
   version.** Three classes are out of scope here and need a future oracle: the
   relation / call-resolution / assignability families (218 `Relate` rows); the nominal
   constructs the structural-parity claim defers (enum-member brand, `unique symbol`,
   private/protected brand, abstract-ctor — §Scope); and the genuine `any`/`never` +
   package-backed/custom-host deferred classes (§Scope). They need a future structured /
   nominal oracle (compiler-API or tsserver `quickinfo`), whose capability is unverified
   at `7.0.0-dev.20260526.1`. That source must be spiked before any such row is lifted —
   it does not block the `TypeExpr`-projection lift, and none of these are force-fit into
   a hover-lowered `TypeExpr` compare.

4. **Snapshot churn on workspace edits.** Because `snapshot_id` hashes the whole
   `workspace_files` set — every file the row upserts, not only the one a given query
   reads — editing any contributing fixture orphans the row's snapshots and requires
   regeneration. This whole-set keying is INTENTIONAL OVER-KEYING for churn-safety, NOT a
   collision concern: keying on the FULL upserted set (rather than trying to compute the
   minimal per-query file subset) guarantees no snapshot ever compares against an
   answer computed under a since-edited workspace — the cost is that an edit to a file a
   particular query did not actually read STILL re-derives that query's `snapshot_id` (an
   over-conservative orphan, never a stale compare or a silent collision). A multi-file
   row edit is therefore a generator-rerun event; the per-block discipline must account
   for it. The over-keying trades a few extra regenerations for the guarantee that a
   workspace edit can never silently invalidate the parity proof.

5. **Env-pin completeness (`compiler_options_hash` + the closed vendored corpus).** Two
   independent pins must both be complete, and they sit at DIFFERENT layers. (a) The
   `compilerOptions` set must be pinned in one place (the COMMITTED canonical
   `oracle.tsconfig.json` for standalone rows — a vendored corpus member, the source of
   truth — the `/workspace` tsconfig for workspace-footprint rows) and hashed over the
   EFFECTIVE config via the CLOSED recipe (the eight `strict` subflags expanded, the
   CLOSED ENUMERATED effective-option key table overlaid with committed-value-or-
   `tsgo_version`-pinned-default, NO open-ended "every option" clause; unowned options
   rejected by `compiler_options_hash_is_closed`); the generator must
   SEND those effective options to tsgo (the in-repo init sends only
   `processId`/`capabilities`/`rootUri`/`workspaceFolders` and the paths-config only
   `paths`, `ipc.rs:1111,1303`) and PROVE delivery via the MULTI-OPTION delivery-proof
   matrix — one discriminating fixture per print-affecting pinned option
   (`oracle_options_delivery_proven`), not a single strict-null fixture.
   `compiler_options_hash` enters `snapshot_id`. (b) The resolved file set must be VENDORED into a CLOSED corpus
   (`oracle_env/<env_corpus_id>/`) — the generator copies the BYTES of the full SHARED set
   tsgo consulted (ambient + lib + package `.d.ts` PLUS resolution metadata — package
   manifests, project metadata, fact 8; the SHARED ambient/lib/package/tsconfig corpus
   ONLY, with per-row workspace files kept separate in `identity.workspace_files`) and
   drives tsgo against that frozen root,
   NOT live `node_modules` (gitignored, where tsgo bundles its libs,
   `ipc.rs:~2859-2874`, `.gitignore:9`; pinning against live `node_modules` would break
   hermeticity). An under-captured corpus would let an ambient / lib / package / manifest
   change silently alter a tsgo answer; the offline gate closes this by RE-ENUMERATING
   the vendored dir for set-equality (catching an ADDED file) before content-hashing.
   The per-snapshot `oracle_env_hash` does NOT enter `snapshot_id` (that would be
   circular, §Q4) — it is STORED and validated on the VALUE; the STABLE `env_corpus_id`
   (the closed-corpus content id) is the pinned-env constant that enters `snapshot_id`,
   so the filename stays registry-derivable while env drift (membership + content) is
   still caught.

6. **Snapshot proliferation from one-file-per-`(row,query)`.** Since snapshots are
   never shared (each `(row, query)` owns a file), an N-query row produces N tiny
   files. This is the deliberate trade — duplicate tiny JSON is cheaper and far safer
   than coupling several rows' proof lifecycles to one shared file — but the snapshot
   tree grows linearly with lifted queries, and the `no_orphan_snapshot` set-equality
   guard must enumerate it recursively on every run.

7. **The first lift's prerequisite set was the regenerator source-model + the 362-table
   guard + THREE all-row-sensitive guards — NOT "only three guards" (REALIZED).** The lift
   mechanism removes a row's `#[ignore]` and flips `status` to `Lifted`. Under the original
   all-`Ignored` model, `scripts/gen-typeinfo-ignore-manifest.py` built the table ONLY from
   live `#[ignore]` discovery, hardcoded `status: IgnoreStatus::Ignored`, and asserted
   exactly 362 built — so a `Lifted` row would VANISH on the next regeneration. The
   regenerator now UNIONS live discovery with a retained `Lifted`-row ledger
   (`LIFTED_ROW_OVERRIDES`), sources `status` from the ledger, and keeps the TOTAL union at
   362. The `ignored_test_row_table_holds_exactly_362_rows` guard (`:1155`) was reconciled —
   its raw `.len() == 362` STAYS true (a `Lifted` row stays in the table) while its
   live-ignore assertion is now `EXPECTED_TOTAL_IGNORED_COUNT == 362 - lifted_count` (= 358).
   Of the four guards in `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`,
   the COUNT guard `EXPECTED_TOTAL_IGNORED_COUNT = count_ignored_rows(…)` (`:595`,
   `:561`/`:566-575`) was ALREADY status-filtered; the other THREE all-row-sensitive guards
   (`manifest_length_matches_documented_total` `:989`,
   `every_manifest_row_corresponds_to_a_live_ignored_test` `:828`,
   `per_file_ignored_test_counts_match_manifest` `:1018`) are status-filtered over
   `status == Ignored` rows. All of this — regenerator redesign + 362-table-guard
   reconciliation + the three guard edits — landed with the first lift; it is recorded in
   full in §Q5 and the Verification table. The earlier "only THREE guards" framing was inaccurate.

8. **Vendored-corpus byte size + regeneration cost.** Vendoring the closed oracle-env
   corpus checks in the BYTES of the consulted lib / ambient / package `.d.ts` set
   (rather than referencing gitignored `node_modules`), so the repo carries the corpus
   weight and a tsgo-version or lib-set change re-vendors it (new `env_corpus_id` →
   every standalone snapshot's id changes → full regeneration). This is the deliberate
   cost of hermeticity + closure: the alternative (live `node_modules`) is non-hermetic
   and not offline-re-enumerable. The single shared canonical corpus for all
   standalone-host rows keeps the duplicated weight to ONE copy, not one per row.
