---
ruling_id: "B3-SCOPE"
type: "architecture-ruling"
date: "2026-08-16"
date_source: "file-mtime (no in-document date)"
binds: ["B3"]
source_file: "B3-scope-ruling-codex-1.md"
summary: "Primary ruling RESCOPE_REQUIRED: B3 must atomically convert every presently-reachable production route (internal one-shot, host per-file/virtual, host/NAPI compile_many, NAPI compile_with_audit, WASM compile/audit/virtual, existing project-aware staged compilation, bundler/unplugin) at its request-construction point — no route stays on its current option type until K2. Requires maintainer ratification to amend B3.md's predecessor/scope, product-inventory.md, and program.md's K2 scope. Additional rulings: 'exhaustive capability reachability' means constructor reachability + exact typed refusal, not emitted-product correctness; inline+SSR rejects at construction, inline+Vapor constructs but refuses pre-codegen; framework_extras moves to an ephemeral execution-input carrier excluded from request identity; CompileTargetTag is a public audit schema outside the Typeinfo protobuf contract; Svelte output liveness DEFERs to BS1 as debt row FC-SVELTE-001."
supersedes: []
superseded_by:
  - ruling: "AMD-009"
    claim: "This ruling's RESCOPE_REQUIRED amendment demand (transferring atomic all-route migration from K2/later owners to B3) is the substance MAINTAINER-RULING-AMD-009 Ruling 1 ratifies at charters/B3.md:16-18 and AMD-005:129-130 (K2 retains only final typed-carrier representation and Any+Send+Sync removal, not the initial conversion)."
contradicts: []
notes: "Companion document to B2-scope-and-concurrency-ruling-codex-1.md and parallelism-ruling-codex.md, all from the same 2026-08-16 codex session batch."
---

on or its exact refusal, plus product minimality and zero-work planning. Remove end-to-end output-difference requirements from B3’s brief.

2. **Silent inline demotion is forbidden, but the two combinations differ.**

   - `inline=true + SSR`: reject during request construction. The SSR capability has no inline axis; an operation absent from the matrix is unsupported. `docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:5`; `docs/arch/refactor/rev11/architecture.md:147`.
   - `inline=true + Vapor`: construct the request because the Vapor-client cell explicitly claims inline/separate. Until BV1 implements it, execution returns a typed capability-unavailable result before codegen. `docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:4`; `docs/arch/refactor/rev11/charters/BV1.md:13-18`.
   - Neither may fall back to non-inline. Current behavior does exactly that. `crates/verter_compiler/src/compile/mod.rs:701-712`; `crates/verter_compiler/src/framework_common/carrier_compiler.rs:403-407`.

3. **B3 deletes `framework_extras` from the request/options authority, not the underlying Vue facts path.**

   Move block bytes and resolved framework facts into a separate ephemeral execution-input carrier excluded from request identity. The canonical planner emits only a typed prerequisite demand; B3 neither computes nor interprets Vue macro facts. Current facts are session-produced and typed as `VueRuntimeCompileExtras`. `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2834-2847`; `crates/verter_compiler/src/framework_common/vue_bridge.rs:153-172`.

   The residual opaque transport/downcast may remain there until K2, which explicitly owns final `Any + Send + Sync` removal. `docs/arch/refactor/rev11/program.md:413-417`. Do not place macro-runtime facts, prop constness, or style binding facts inside `VueCompileRequest`.

4. **`CompileTargetTag` is a public JSON/TypeScript audit schema, but it is not governed by the Typeinfo protobuf field-number/reserved contract.**

   The Typeinfo rules apply specifically to the typeinfo proto taxonomy and its audit envelope. `CLAUDE.md:247-256`. `CompileTargetTag` is independently serialized with Serde and exported through `ts-rs`; its committed TypeScript union is public package content. `crates/verter_audit/src/payloads/tags.rs:6-20`; `packages/types/audit.generated.ts:600-603`; `packages/types/package.json:8-10,31-34`.

   B3 must replace the lossy singular target mirror with audit fields representing the canonical product set/backend, migrate producers and tests, and regenerate the TS binding in the same candidate. No protobuf schema bump or `reserved` tag applies. The current mapping collapses every non-IDE/non-forced-Vapor request to VDOM. `crates/verter_session/src/host_compile_audit.rs:64-72`.

5. **DEFER Svelte output liveness to BS1. B3 does not close it.**

   B3 must represent and normalize every supported-canonical Svelte option and must never replace a supplied value with the current hardcoded defaults. `docs/arch/refactor/rev11/evidence/framework-conformance/svelte-options.tsv:5-10,20-30`; `crates/verter_compiler/src/svelte/carrier.rs:366-401`.

   Where honoring an option requires unimplemented Svelte semantics, the canonical request constructs successfully but execution fails before emission with a typed “capability not yet accepted” result. BS1 owns namespaces, runes/legacy, dev/prod, whitespace/comments, custom elements, and output correctness. `docs/arch/refactor/rev11/charters/BS1.md:11-21`.

   Record one DEFER debt row naming BS1, resolution gate “BS1 acceptance,” and `FC-SVELTE-001` with a case for every affected option. Do not use a TODO or let B3 claim output propagation. `CLAUDE.md:553-560`.
tokens used
146 480
## Primary ruling — `RESCOPE_REQUIRED`

B3 must atomically convert every presently reachable production route at its request-construction point. No existing route may remain semantically governed by `CompileTarget`, `CompileProfile`, `RuntimeCompileOptions`, `FfiCompileProfile`, or equivalent option defaults until K2.

The current authorities are inconsistent: B3 requires one canonical request and forbids a second option authority, yet assigns transport conversion to K2 and excludes route exposure; meanwhile the route inventory requires every public/default route to map to that request but retains later route owners. `docs/arch/refactor/rev11/charters/B3.md:5-18`; `docs/arch/refactor/rev11/evidence/framework-conformance/product-inventory.md:37-40`.

### Convert in B3

Convert all routes that exist now:

- internal compiler one-shot;
- host per-file/virtual products;
- host/NAPI `compile_many`;
- NAPI `compile_with_audit`;
- WASM compile/audit/virtual products;
- existing project-aware staged compilation;
- bundler/unplugin publication.

These are the presently reachable routes enumerated at `docs/arch/refactor/rev11/evidence/framework-conformance/product-inventory.md:24-35`.

No existing route stays on its current semantic option type. The final direct one-shot, prepared, and direct-batch routes remain absent until B5/B6; B3 cannot convert nonexistent routes. `docs/arch/refactor/rev11/evidence/framework-conformance/product-inventory.md:31-33`.

At each boundary, decode directly into the canonical constructor. Transport structs may exist only as syntax/serialization DTOs: they apply no defaults, select no products, interpret no capabilities, and never form cache or semantic authority. The present NAPI → FFI → host-profile chain does interpret defaults and therefore must be cut through or reduced to pure decoding. `crates/verter_napi/src/lib.rs:250-299`; `crates/verter_ffi/src/convert/input.rs:68-132`; `crates/verter_session/src/types.rs:1316-1411`.

That is not a migration adapter: it is the permanent transport decoder targeting the sole domain constructor. No old-options-to-new-request wrapper survives. Clean-cutover governance requires one implementation and migration of every in-scope caller; legacy shims are forbidden. `docs/arch/refactor/rev11/governance.md:315-332`; `CLAUDE.md:549-558`.

### Required amendment

Maintainer ratification is mandatory before implementation. Amend:

1. `charters/B3.md:16-18` and `AMD-005:126-144`: transfer atomic migration of all current request-construction sites from K2/later route owners to B3.
2. `product-inventory.md:37-40`: distinguish B3’s ownership of request construction from later ownership of route exposure, publication, and equivalence.
3. `program.md:413-417`: restrict K2 to final framework-private carrier typing and removal of `Any + Send + Sync`; K2 must not perform a second semantic request conversion.
4. B3’s bound charter: enumerate the seven existing route families above and their DTO/declaration deletion or explicit compatibility-retention set.
5. `emitter-mapping-dispositions.tsv`: add explicit dispositions for the NAPI, protocol/FFI, session-profile, WASM, and unplugin ingress carriers; existing rows already assign the compiler request/options seam to B3. `docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv:6-10,21,37`.

No DAG change is required: keep B3 at `program-dag.toml:100-104` and K2 at `program-dag.toml:322-325`. This is an ownership amendment, not a dependency insertion.

Only the maintainer can ratify it because it changes charter authority and consumer scope; governance expressly reserves amendment/split/abort decisions after a disproven charter assumption to the maintainer. `docs/arch/refactor/rev11/governance.md:285-299`.

## Additional rulings

1. **“Exhaustive capability reachability” means constructor reachability, exact typed refusal, and prerequisite-plan correctness—not emitted-product correctness.**

   B3’s exit explicitly requires exhaustive *constructor* tests and excludes framework lowering/codegen. `docs/arch/refactor/rev11/charters/B3.md:18-22`. BV1 and BS1 own output semantics and conformance. `docs/arch/refactor/rev11/charters/BV1.md:11-23,46-54`; `docs/arch/refactor/rev11/charters/BS1.md:11-21,27-34`.

   Implementer consequence: test every matrix/route combination for canonical construction or its exact refusal, plus product minimality and zero-work planning. Remove end-to-end output-difference requirements from B3’s brief.

2. **Silent inline demotion is forbidden, but the two combinations differ.**

   - `inline=true + SSR`: reject during request construction. The SSR capability has no inline axis; an operation absent from the matrix is unsupported. `docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:5`; `docs/arch/refactor/rev11/architecture.md:147`.
   - `inline=true + Vapor`: construct the request because the Vapor-client cell explicitly claims inline/separate. Until BV1 implements it, execution returns a typed capability-unavailable result before codegen. `docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:4`; `docs/arch/refactor/rev11/charters/BV1.md:13-18`.
   - Neither may fall back to non-inline. Current behavior does exactly that. `crates/verter_compiler/src/compile/mod.rs:701-712`; `crates/verter_compiler/src/framework_common/carrier_compiler.rs:403-407`.

3. **B3 deletes `framework_extras` from the request/options authority, not the underlying Vue facts path.**

   Move block bytes and resolved framework facts into a separate ephemeral execution-input carrier excluded from request identity. The canonical planner emits only a typed prerequisite demand; B3 neither computes nor interprets Vue macro facts. Current facts are session-produced and typed as `VueRuntimeCompileExtras`. `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2834-2847`; `crates/verter_compiler/src/framework_common/vue_bridge.rs:153-172`.

   The residual opaque transport/downcast may remain there until K2, which explicitly owns final `Any + Send + Sync` removal. `docs/arch/refactor/rev11/program.md:413-417`. Do not place macro-runtime facts, prop constness, or style binding facts inside `VueCompileRequest`.

4. **`CompileTargetTag` is a public JSON/TypeScript audit schema, but it is not governed by the Typeinfo protobuf field-number/reserved contract.**

   The Typeinfo rules apply specifically to the typeinfo proto taxonomy and its audit envelope. `CLAUDE.md:247-256`. `CompileTargetTag` is independently serialized with Serde and exported through `ts-rs`; its committed TypeScript union is public package content. `crates/verter_audit/src/payloads/tags.rs:6-20`; `packages/types/audit.generated.ts:600-603`; `packages/types/package.json:8-10,31-34`.

   B3 must replace the lossy singular target mirror with audit fields representing the canonical product set/backend, migrate producers and tests, and regenerate the TS binding in the same candidate. No protobuf schema bump or `reserved` tag applies. The current mapping collapses every non-IDE/non-forced-Vapor request to VDOM. `crates/verter_session/src/host_compile_audit.rs:64-72`.

5. **DEFER Svelte output liveness to BS1. B3 does not close it.**

   B3 must represent and normalize every supported-canonical Svelte option and must never replace a supplied value with the current hardcoded defaults. `docs/arch/refactor/rev11/evidence/framework-conformance/svelte-options.tsv:5-10,20-30`; `crates/verter_compiler/src/svelte/carrier.rs:366-401`.

   Where honoring an option requires unimplemented Svelte semantics, the canonical request constructs successfully but execution fails before emission with a typed “capability not yet accepted” result. BS1 owns namespaces, runes/legacy, dev/prod, whitespace/comments, custom elements, and output correctness. `docs/arch/refactor/rev11/charters/BS1.md:11-21`.

   Record one DEFER debt row naming BS1, resolution gate “BS1 acceptance,” and `FC-SVELTE-001` with a case for every affected option. Do not use a TODO or let B3 claim output propagation. `CLAUDE.md:553-560`.
