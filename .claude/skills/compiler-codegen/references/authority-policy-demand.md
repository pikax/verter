# Compiler authority, policy, demand, and admission

Operational pointer for implementers. **Normative text** lives in
[`roadmap/0.1.0-tama/contracts/compiler-architecture.md`](../../../../roadmap/0.1.0-tama/contracts/compiler-architecture.md).
Do not treat this skill file as a second constitution.

The production seam still routes through the combined carrier-compiler
registry and host compile routes. Those routes remain mechanically live
until later migration nodes delete them. Combined-registry identity is
displaced as *authority*, not preserved behind aliases.

## Sole owner (summary)

Final owner: **verter_compiler capability traits plus immutable registration
catalog**.

Displaced as authority:

- combined `CarrierCompiler` trait and `CarrierCompilerRegistry`
- mixed framework/options buckets on one runtime option struct
- tooling-only runtime stubs that pretend a missing compiler product exists
- `CompileTarget` bitflags as compiler product/pipeline selector

No sixth compiler authority exists. CSS-family syntax and lossless tokens stay
with the CSS syntax crate. TypeInfo / typed IR stay shared analysis machinery,
not a framework semantic authority.

`compile_bundle` is a **combined product pass**, not an owner: runtime legs
(`RuntimeClient`/`RuntimeServer`) → `RuntimeCompilerBackend`; IDE/public-API/
declarations → `ProjectionBackend`; analysis facts →
`FrameworkSemanticAuthority`. Assembly splits: `vue_module` topology →
runtime backend; `publish` decoration → host integration. `style_planner` /
`style_usage` / `css_vars` / Svelte CSS are displaced combined interpreters.

The combined `CarrierCompiler` trait is a **temporary selector** only: it
selects the live adapter row. It is not an authority. No new methods may be
added to it.

## Catalog (summary)

Catalog key = **adapter × epoch × capability**. Every `register_*`
constructor derives the catalog epoch id from an `E: FrameworkEpoch`
type parameter; none takes a separate spelling. `CarrierFrontend` and
`ProjectionBackend` themselves do not take an epoch type parameter.
The table is process-lifetime immutable. There is no runtime plugin load.

Identity methods (`adapter_id`, `carrier_language_id`) belong on the catalog
row, not on a combined-authority trait.

## Five authorities (summary)

| Authority | Role |
| --- | --- |
| `CarrierFrontend` | Parse, registered geometry, unregistered parse artifacts, parse diagnostics (artifact-retained), syntax reject (no ParseAdmission) |
| `FrameworkSemanticAuthority<FrameworkEpoch>` | Per-framework interpretation: eval-source, template facts, framework style meaning |
| `ProjectionBackend` | IDE companion, public-API, and declarations (TSC / `.d.ts`) projection |
| `RuntimeCompilerBackend<FrameworkEpoch>` | Runtime emit with statically selected targets |
| `FrameworkHostIntegrationBackend<FrameworkEpoch, HostEpoch>` | Host/unplugin/session integration; **issues** `CompileAdmission` |

## Admission (summary)

| Token | Issued by | Consumed by |
| --- | --- | --- |
| `ParseAdmission` | `CarrierFrontend` | semantic authority, host composition |
| `SemanticAdmission` | `FrameworkSemanticAuthority` | host composition |
| `CompileAdmission` | **only** `FrameworkHostIntegrationBackend` (composes parse + semantic) | `ProjectionBackend` and `RuntimeCompilerBackend` |

There is one `CompileAdmission` type. Product backends do not mint their own.

## Policy (summary)

`CompilePolicy::{Default, Optimized}`. Only `Default` is initially supported.
`Optimized` is unsupported fail-closed: no implementation, no token, no silent
fallback into Default.

`DefaultCompilationContractId` cells are **1:1 with live `ProductKind`**
(`runtime-client`, `runtime-server`, `ide-companion`, `public-api`,
`declarations`, `analysis`). There is no `facts` dump family.

Cheap Default local-fact corrections are `SemanticFact` + `SemanticAdmission`,
issued only by `FrameworkSemanticAuthority` over already-admitted parse. No
backend-private type environment; no second resolve around TypeInfo.

Session eval-source is one `built_in_semantic_catalog` lookup keyed adapter ×
artifact epoch × Semantic, then the selected row's eval-source payload. The
generic selector has no Vue/Svelte match. Catalog miss is typed refusal
before parse/lease/publication. The combined `CarrierCompiler::eval_source`
method is gone.

Session template facts are the same catalog lookup, then the selected row's
template-fact payload. `compile_source` must bind the artifact `parse_key`
(and adapter/language identity); a retained parse of another revision is
typed refusal. Catalog miss, parse-key mismatch, and producer failure are
typed refusal, never empty success. A valid template-free carrier is
`Some` empty facts. `compile_bundle` fills `template_data` from that catalog
payload when asked; it does not independently extract. The combined
`CarrierCompiler::template_data` method and `TemplateFacts` wrapper are gone.

Spelling/versioning, the per-kind equivalence matrix, demand kinds,
reason-edge types, semantic namespaces, and the caller→owner table are in the
bound contract.

## Residual firewall

The crate-graph guard proves **host/session/transport crate closure only**.
It does not prove an in-crate second analyzer is absent. Generic versus
framework split is locked in prose until typed catalog surfaces exist. The
in-crate firewall is **not** proven.

## Forbidden

Vue/Svelte V2 implementation, CSS matcher changes, native preprocessors,
project-wide optimization, dynamic plugin/ABI, preserving combined authority
behind aliases, dual-running authority, successors implemented in this lock,
re-interpreting framework style meaning in the runtime emitter, grouping
`ProductKind` cells without amendment, treating `compile_bundle` as a product
owner, backend-private type environments for Default corrections.
