# v2 design review — codex xhigh, verification pass

Verbatim final answer verifying v2 against the four v1 defects, plus new findings.
VERDICT: NEEDS REVISION — findings incorporated into the v3 amendment appended to
binding-index-design.md.

```
v2 is not ready. The one-bind/span-correlation idea fixes the cross-build identity problem, and the outer-driver placement is correct, but the rewrite still mis-models Vue’s runtime scope and overclaims downstream `DeclBindingKey` support.

## 1. Verification of the four v1 defects

1. **`with` / Annex-B moot claim: only partially fixed.**

   v2 correctly retracts the “always an ES module” claim and adds `Indeterminate`, sloppy Annex-B, and strict/module tests. But its replacement source-type description remains factually wrong:

   - No `lang` defaults to `ScriptSourceType::Ts`, not classic JS; only explicit `lang="js"` selects `JsModuleKind::Script` ([vue_bridge.rs:111](crates/verter_compiler/src/framework_common/vue_bridge.rs:111), [vue_bridge.rs:127](crates/verter_compiler/src/framework_common/vue_bridge.rs:127)).
   - `SourceType::ts()`/`tsx()` are initially unambiguous ([parse.rs:1215](crates/verter_session/src/parse.rs:1215)); the parser stores the resolved script/module kind on `program.source_type`.
   - `build_script_analysis_inner` receives the pre-parse `source_type` separately ([build.rs:360](crates/verter_semantic/src/analysis/build.rs:360)), while `SemanticBuilder` reads `Program.source_type`, not that parameter (`oxc_semantic-0.126.0/src/builder.rs:258`).

   Therefore v2’s instruction to reuse the caller’s `source_type` parameter can restore the unresolved pre-parse type and change strictness/Annex-B behavior. The clone must preserve the retained `program.source_type`.

2. **Flat combined program: the abstract parent/child fix works, but the concrete Vue model does not.**

   An OXC function body really is a child lexical/var scope, so for ordinary statements the wrapper eliminates the false same-scope redeclaration problem and implements the owner table’s instance→module relationship ([top_level_owners.rs:365](crates/verter_semantic/src/analysis/top_level_owners.rs:365)).

   It still has material gaps, detailed in §2: directives are unhandled, the synthetic function must not introduce a declaration binding, and—most importantly—the wrapper does not mirror where Vue places setup imports and runtime macro arguments.

3. **Cross-build `reference_id`: fixed.**

   One clone, one bind, and span lookup avoids comparing semantic IDs from independent builds. That correction holds, subject to the collision/fail-closed requirements in §3.

4. **`is_value()` and type-only import-equals: directionally fixed, not yet fully specified.**

   The explicit pre-bind erasure of `import type X = …` fixes the pinned OXC binder bug. Running `is_value()` only after a Vue-specific runtime projection is also the correct architecture.

   The remaining gap is that v2 still delegates the supposedly exhaustive Vue classification to future factoring. The existing Svelte classifier is explicitly Svelte-specific: it erases every enum, namespace, and import-equals declaration ([component_scope_facts.rs:494](crates/verter_compiler/src/svelte/runtime/component_scope_facts.rs:494), [component_scope_facts.rs:562](crates/verter_compiler/src/svelte/runtime/component_scope_facts.rs:562)). v2 describes Vue deltas but does not close the exact exhaustive match/parameterization. That must be part of the design, not an open implementation exercise.

## 2. Is the single synthetic-function bind sound?

It is mechanically sound for creating one real Program→function scope tree, but it is not a sound model of Vue runtime-constructor resolution as written.

Vue’s actual compilation does not put all setup statements and the constructor expression in that function:

- Setup imports are hoisted outside the setup body ([process.rs:192](crates/verter_compiler/src/script/process.rs:192)).
- The authored `defineProps` runtime argument is copied into `props_section`, and the macro call is removed/replaced ([macros.rs:398](crates/verter_compiler/src/script/macros.rs:398), [macros.rs:431](crates/verter_compiler/src/script/macros.rs:431)).
- That `props_section` is emitted in the component options object before `setup()` ([process.rs:837](crates/verter_compiler/src/script/process.rs:837), [process.rs:872](crates/verter_compiler/src/script/process.rs:872)).

Consequently:

- A setup-local `class String {}` is not visible to the emitted runtime `props` expression. Vue should reject that macro capture or Verter should fail it; the proposed wrapper incorrectly resolves it as `Local(instance, "String")`.
- A setup import named `String` is hoisted and is visible to the runtime props expression. It is also module-scoped in emitted code, whereas v2 derives declaration ownership purely from “Program versus wrapper scope.”
- A setup import can affect a companion module Options expression after code generation, contradicting v2’s unconditional reverse-visibility `Global` fixture.

The binding projection must therefore model the relevant Vue lowering: hoist setup imports into the outer binding scope, move runtime macro argument references into their outer options context, keep ordinary setup declarations in the setup child scope, and retain authored owner provenance independently of OXC scope placement.

There are two additional wrapper requirements:

- `Program.directives` are not covered by the owner vector, which is parallel only to `Program.body` ([top_level_owners.rs:232](crates/verter_semantic/src/analysis/top_level_owners.rs:232)). An instance `"use strict"` parsed as a body expression will not become a function directive; one parsed as a Program directive can incorrectly strictify the outer owner. Directives must be classified using owner regions ([top_level_owners.rs:397](crates/verter_semantic/src/analysis/top_level_owners.rs:397)) and rebuilt into the correct directive prologues.
- The wrapper must be an anonymous, non-binding function expression or equivalent. A named `FunctionDeclaration` binds its name in the Program before entering its function scope (`oxc_semantic-0.126.0/src/builder.rs:1849`) and can create a synthetic collision.

Moving cloned nodes within one arena does not itself require reparsing or span adjustment. The missing work is semantic projection, directive reconstruction, and non-binding wrapper construction.

## 3. Span-based correlation

For the current move-only clone, span correlation is reliable for clean, parser-authored identifier references:

- The Vue script source is position-preserving and retains original SFC byte offsets ([parse.rs:2065](crates/verter_session/src/parse.rs:2065)).
- Cloning, moving statements, erasing statements, or unwrapping TS expression carriers does not change the surviving identifier token’s span.
- Two distinct source identifier tokens cannot occupy the same non-empty byte range.

It is not safe to let `FxHashMap::insert` silently overwrite an existing entry. Synthetic/recovery nodes can use empty/default spans, and a future transform that duplicates a macro argument into an outer context would deliberately create two nodes with the same authored span. The index should:

- exclude synthetic identifiers;
- reject duplicate or missing queried spans as `Indeterminate`;
- ensure the transform moves—or otherwise uniquely tags—the authoritative constructor occurrence rather than binding two copies.

With those invariants, span is a suitable correlation key.

## 4. Can the existing admission gate consume `Local(DeclBindingKey)`?

No—not in the sense v2 claims.

A `DeclBindingKey` is the right input-shaped lookup key, but the existing gate is specifically a `defineExpose` admission path:

- It accepts requested keys, calls `visible_value_binding`, and admits only `LexicalValueBinding::Local`; imports are rejected by the pattern match ([eval_env.rs:924](crates/verter_session/src/host_manage/eval_env.rs:924), [eval_env.rs:942](crates/verter_session/src/host_manage/eval_env.rs:942)).
- `visible_value_binding` explicitly distinguishes `Import` from `Local` ([shallow_file_state.rs:299](crates/verter_session/src/resolver_core/shallow_file_state.rs:299), [shallow_file_state.rs:1219](crates/verter_session/src/resolver_core/shallow_file_state.rs:1219)).
- Requested demands are currently collected only from `defineExpose` fields ([component_meta/mod.rs:29](crates/verter_session/src/resolver_core/component_meta/mod.rs:29)).
- Its output becomes `FieldKind::Binding` and `ExpandedComponentTypes.bindings`, not prop type rows ([type_eval_build.rs:5111](crates/verter_semantic/src/analysis/type_eval_build.rs:5111), [type_eval_build.rs:5466](crates/verter_semantic/src/analysis/type_eval_build.rs:5466)).

The “same route a custom class already takes; no new resolution path” claim is also false on the shallow paths inspected: macro runtime constructors populate display text but no payload ([macros.rs:3057](crates/verter_semantic/src/analysis/macros.rs:3057), [macros.rs:3139](crates/verter_semantic/src/analysis/macros.rs:3139)); Options stores only the constructor spelling ([options.rs:163](crates/verter_semantic/src/analysis/options.rs:163)).

v2 must specify a prop-specific authored-value demand/resolution lane that handles both local declarations and imports, or explicitly generalize the existing gate and expansion output. `DeclBindingKey` alone does not complete that architecture.

## 5. Outer-driver placement

The corrected initial placement works.

`build_script_analysis_inner` has the Program, owners, parse status, and source-type context before its body walk ([build.rs:360](crates/verter_semantic/src/analysis/build.rs:360)). Macro extraction occurs during that walk ([build.rs:457](crates/verter_semantic/src/analysis/build.rs:457), [build.rs:504](crates/verter_semantic/src/analysis/build.rs:504)), while Options extraction is called from the `ExportDefaultDeclaration` arm ([build.rs:653](crates/verter_semantic/src/analysis/build.rs:653)). Building once before the loop and threading `&RootBindingIndex` into both producers is feasible. The later `component_meta` Options fallback sees only `AnalyzedOptionsApi`, so it is correctly too late ([component_meta.rs:1580](crates/verter_semantic/src/analysis/component_meta.rs:1580)).

There is still a replay integration gap: `lower_macro_field_payload_at_with_owners` rebuilds macro assembly through `analyze_macros_from_program_with_owners` without source-type, parse-cleanliness, or an index ([macros.rs:763](crates/verter_semantic/src/analysis/macros.rs:763)). v2 must state that runtime-constructor enrichment is stored and excluded from payload replay, or provide an equivalent deterministic replay input.

## 6. Other new defects

- **Direct-eval handling is overbroad.** OXC marks any direct `eval`, including strict eval, and propagates `DirectEval` from a child scope to its parent (`oxc_semantic-0.126.0/src/builder.rs:669`, `oxc_semantic-0.126.0/src/builder.rs:2498`). Reading that flag literally would let an eval in the synthetic setup child make unrelated module references `Indeterminate`, and would reject strict direct eval even though it cannot inject bindings into the surrounding scope. The design needs a strictness- and ancestry-aware rule, not just “scope contains `DirectEval`.”
- **The carrier omits the fact/cache representation.** New fields only on `AnalyzedPropField` and `AnalyzedOptionsProp` would be lost when narrowed into `AnalyzedPropFieldFact` and `AnalyzedOptionsPropFact`, whose current schemas contain no constructor-resolution carrier ([facts.rs:2582](crates/verter_type_expr/src/facts.rs:2582), [facts.rs:2686](crates/verter_type_expr/src/facts.rs:2686)). Those fact types and their conversion/publication path must be part of the design.
- **Nullable-array semantics remain undecided.** v2 simultaneously gives `PrimitiveName::Null` as the expected green result and says that interpretation still requires maintainer confirmation. That decision must be ratified before it can be an implementation requirement.

Before implementation, v2 must be revised to: preserve `program.source_type`; model Vue’s actual setup-import and runtime-macro hoisting; reconstruct per-owner directive prologues; use a non-binding wrapper; define the exhaustive Vue projection and precise dynamic-scope rule; specify a prop authored-value resolution lane supporting imports and locals; carry its typed result through the narrowed fact/cache and publication schemas; and define deterministic macro replay and nullable-array semantics.

VERDICT: NEEDS REVISION
```
