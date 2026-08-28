# A5 — Option classification

Classifies every behaviour-affecting configuration field in the tree against the six classes in
[`contracts/semantic-profile.md`](../../contracts/semantic-profile.md) §1. Data:
[`option-classification.tsv`](option-classification.tsv) — 84 fields across five owner structs,
one row each.

The contract's tie-break governs every ambiguous row: **"A field belongs to the earliest class
whose observable meaning it can change. It is never copied into every class 'for safety.'"**

| class | count |
|---|---|
| `output` (`OutputProfileId`) | 34 |
| `semantic` (`TypeScriptSemanticProfileId`) | 28 |
| `execution-only` (`ExecutionPolicy`) | 19 |
| `serialization` (`SerializationProfileId`) | 2 |
| `presentation` (`PresentationProfileId`) | 1 |

Recompute with
`awk -F'\t' 'NR>1{c[$4]++}END{for(k in c)print k,c[k]}' option-classification.tsv`.

Owner structs, all source-verified in this checkout:

| struct | source | fields |
|---|---|---|
| `HostConfig` | `crates/verter_session/src/types.rs:500` | **23** declared fields; 26 TSV rows (the 23 plus 3 nested sub-rows, marked `parent.child` — see below) |
| `CompileProfile` | `crates/verter_session/src/types.rs:1322` | 22 |
| `CodegenOptions` | `crates/verter_compiler/src/compile/types.rs:176` | 21 |
| `IdeProjectCompilerOptions` | `crates/verter_workspace/src/resolver.rs:30` | 6 |
| `EnvHashInputs` | `crates/verter_workspace/src/env_hash.rs:68` | 9 |

Every count is the struct's declared `pub` field count, verified in this checkout. Only `HostConfig`
has TSV rows that are not top-level fields: three sub-rows reach into nested option structs, and
they are **not** part of its 23-field tally. They carry a `parent.child` field name in the TSV so
the distinction is readable without cross-referencing this note:

| TSV field | parent field (already counted in the 23) | nested struct |
|---|---|---|
| `recursion_budget_overrides.synthesis_steps` | `recursion_budget_overrides` | `RecursionBudgetOverrides` (`types.rs:739`) |
| `recursion_budget_overrides.walker_pathological_cap` | `recursion_budget_overrides` | `RecursionBudgetOverrides` |
| `resource_policy.{host_cpu_pool,decl_lowering}.{spawn,size}` | `resource_policy` | `HostResourcePolicy` (`types.rs:875`) → two `PoolPolicy` fields |

They are classified separately because each is independently settable and the parent field's own
class does not determine theirs; they are listed as sub-rows because counting them alongside the
top-level fields would double-count the parent. `23 + 3 = 26` is the whole of the discrepancy —
there is no 24th, 25th or 26th `HostConfig` field.

## Scope, stated plainly

This classifies the **Rust** configuration surface — the structs that actually reach the host,
the compiler, and the env-hash producer. The TypeScript-side option surfaces
(`@verter/unplugin`, `@verter/nuxt`, the VS Code settings) are *projections* that terminate in
these structs; classifying them separately would create two classifications of one field, which
is exactly what §1's "never copied into every class" forbids. A later block that adds a TS option
with no Rust counterpart adds a row here, not a second table.

`CompileTarget` (`crates/verter_compiler/src/compile/types.rs:17`) is a `u8` bitflag set
(`STYLE`, `SCRIPT`, `TEMPLATE`, `TSX`, `TSC`, `TEMPLATE_DATA`) carried by the `target` field of
both `CompileProfile` and `CodegenOptions`. It is classified once, as `output`, on those two rows
rather than as six pseudo-fields: each flag selects whether a codegen step runs, which is the same
observable class for all six.

## Non-obvious classifications, and why

**`delimiters` and `custom_elements` are `semantic`, not `output`.** The instinct is that
interpolation delimiters are a codegen knob. They are not: `CompileProfile::has_parse_affecting_template_options()`
(`crates/verter_session/src/types.rs`) exists precisely because a template extracted under
non-default delimiters "describes a DIFFERENT parse of the same bytes", so a cached parse cannot
be reused and the extraction must not populate the profileless default-extraction slot. A field
that changes what the source *means* is semantic under §1's earliest-class rule, whatever it also
does downstream.

**`strict_slots` and `conditional_root_narrowing` are `semantic`.** Both change what type-checks:
`strict_slots` emits `strictRenderSlot` calls that enforce typed slot children, and
`conditional_root_narrowing` changes root generic narrowing. They are spelled as codegen flags and
classified by their observable meaning.

**`resolve_extensions` is `semantic`.** Extension priority decides which file an import resolves
to, i.e. which declarations a program contains. It is already folded into `resolve_env_hash`,
which corroborates the classification from the cache side.

**`svelte_css_hash_override` is `output`, not `execution-only`, despite gating cache mode.** Its
value is used verbatim as the scope class in emitted CSS, so it changes output bytes. That a
present override additionally makes a requested `Content` compile non-admissible
(`DowngradeReason::CssHashOverridePresent`) is a consequence of it being an output dimension, not
a separate class.

**`requested_mode` is `execution-only`.** The host may downgrade it (`CompileCacheMode::Session` →
`Content` → `Stateless`), and §4 requires that a policy change never produce a different value
labelled `Complete`. A downgrade that changed output bytes would be a defect, not a classification.

**Every audit field is `execution-only`.** `audit_enabled`, `footprint_capture`,
`audit_timing_capture`, `max_derivation_edges`, `audit_caps`. This is the same invariant A4 holds
structurally for `verter_audit::attribution`, restated as a profile classification: observability
never enters result identity.

**`lsp_scheme` is the only `presentation` field in the tree.** It is a URI scheme prefix for
virtual file ids — a display/exchange concern with no semantic content. §4's cross-class rule
("presentation changes do not invalidate semantic facts") is trivially satisfied because nothing
keys on it.

## Open notes

### QP-1 — `query_profile` classifies `semantic` but is mostly inert

`HostConfig::query_profile` is classified `semantic` because
`HostConfig::from_query_profile` (`types.rs:1166`) derives `analysis_scope` from
`profile.recommended_analysis_scope_bits()`, and `analysis_scope` decides which analysis passes
run — i.e. which facts exist.

But the *live* profile slot is close to inert. `QueryProfile`'s three behavioural predicates —
`allows_background_materialization`, `is_interactive`, `allows_cross_file_queries`
(`crates/verter_semantic/src/profile.rs:35-51`) — have **no production reader**:
`grep -rn` over `crates/*/src` finds call sites only inside `profile.rs`'s own tests. The mutable
host slot (`lib.rs:523`, `host_semantic.rs:18-23`) is written by `set_query_profile` and read back
only by an assertion in `crates/verter_tsc/src/checker.rs:3046`.

So today the profile's *whole* behavioural effect is the one-shot scope derivation at
construction. That is a real effect, so the classification stands; but a later block must not read
`query_profile`'s presence as evidence that a profile-driven execution regime exists. It does not
yet. Owner: `B1` (profile schemas), which either gives the predicates production consumers or
collapses the field into `analysis_scope`.

### SM-1 — `source_map` / `skip_source_map` are `serialization`, with a standing constraint

Both are classified `serialization`: they decide whether an encoded map is produced, not what the
generated program means. §4's rule ("serialization changes do not rerun semantic computation when
the typed result remains available") is the target behaviour.

The constraint a later block must not lose: `B4`'s exit condition is that **required IDE maps
cannot be skipped by a runtime-map flag**. `skip_source_map` is exactly such a flag today
(`CodegenOptions::skip_source_map` "skip source map generation and base64 encoding … returns empty
strings"). After `B4`, `RuntimeSourceMapData` is optional but `SourceProjectionMap` is required,
so this field's reach narrows. Recorded here so `B4` does not have to rediscover which field it
constrains.

### OC-1 — §2 fail-closed is satisfied for unsupported *values*, not for unknown *fields*

§2 requires that "unknown fields or unsupported values fail closed". Split the two halves,
because the tree's answer differs:

**Unsupported values: satisfied.** The string-typed FFI fields are parsed, not trusted.
`crates/verter_ffi/src/convert/input.rs:18-50` matches `compile_error_policy` and
`analysis_level` case-insensitively and returns typed
`FfiConversionError::InvalidCompileErrorPolicy` / `InvalidAnalysisLevel` on anything else; the
NAPI constructor propagates that as a thrown error
(`crates/verter_napi/src/lib.rs:1554-1558`). That is fail-closed.

**Unknown fields: not satisfied, and structurally cannot be at this boundary.**
`NapiHostConfig` and `NapiCompileProfile` are `#[napi(object)]` types, so extra JS properties on
the caller's object are silently dropped in conversion. A caller who mistypes `strictSlots` as
`strictslots` gets the default, with no error.

**A third, related fact a later block needs:** the FFI config surface is a *subset* of the Rust
one. `NapiHostConfig` carries 10 fields against `HostConfig`'s 26
(`crates/verter_napi/src/lib.rs:201-244`); `NapiCompileProfile` carries 18 against
`CompileProfile`'s 22, and represents `target` as a preset string
(`"bundler" | "ide" | "analysis"`) rather than the `CompileTarget` bitflags. So a field
classified `semantic` in the table above may have no FFI ingress at all — `analysis_scope`,
`generic_root_propagation` and the whole strict family are Rust-only today.

Owner `B1`, which lands the closed typed profile schema; the unknown-field half needs an explicit
deny at the FFI boundary rather than a schema alone.
