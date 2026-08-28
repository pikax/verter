# v1 design review — codex xhigh, neutral framing

Verbatim final answer from `codex exec --sandbox read-only -m gpt-5.6-sol -c model_reasoning_effort=xhigh`
reviewing binding-index-design.md v1 (superseded). Full prompt: see git history of this file's
sibling commit. VERDICT: NEEDS REVISION — findings incorporated into v2 of binding-index-design.md.

```
codex
The design has the right direction—delegate lexical binding to OXC—but it is not correct or complete enough to implement as written.

1. No: its intended boundary is stateable, but the actual design binds the unprojected AST and assumes that OXC value-space identity equals runtime-surviving binding identity; ambient TS declarations, dynamic scopes, owner projection, and cross-view IDs remain unbounded outside that claim.

A defensible replacement sentence would be: “Constructor resolution is the result of OXC binding over an exhaustively defined runtime-surviving AST projection, composed through a closed owner graph, with dynamic or uncorrelatable scopes returning `Indeterminate`.”

2. Gap-by-gap:

| Gap | Assessment | Reason |
|---|---|---|
| Hoisted `var` at arbitrary depth | Correct in a proper owner view | OXC hoists `var` through arbitrary block nesting to the nearest var scope. The proposed fixture should resolve `String` locally. |
| TS namespaces | Partly correct | An instantiated non-ambient `namespace String { export const x = 1 }` is a value binding. But `is_value()` is insufficient generally: ambient declarations such as `declare namespace String { export const x: 1 }` are marked ambient and may still have value flags despite emitting no runtime binding. The reused Svelte implementation explicitly projects ambient declarations away before relying on `is_value()` ([component_scope_facts.rs:37](crates/verter_compiler/src/svelte/runtime/component_scope_facts.rs:37)); this design omits that essential step. |
| Ordinary type-only imports | Correct | `import type { String }` produces no value binding, so a value reference stays unresolved and can take the global-constructor path. |
| `with` | Incorrect rationale and incomplete treatment | Vue component-meta scripts are not invariably parsed as modules. Moreover, OXC explicitly documents unresolved references inside `with` as a global-reference limitation (`oxc_semantic-0.126.0/src/is_global_reference.rs:8`). If constructor sites are structurally confined to top level and therefore cannot occur inside `with`, the design must state and test that boundary; otherwise a reference under a `With` scope must be `Indeterminate`. Hoisted declarations inside `with` still rely on normal binder behavior. |
| Array/Object/Function/Symbol/Date/RegExp/Promise | Correct policy, incomplete mechanism | A shadowed spelling must not receive hardcoded display text, while an unshadowed global can retain the display-only behavior. But `Local(SymbolId)` is not connected to a usable Verter declaration identity, so the authored-reference path is not actually specified. |
| `TSImportEqualsDeclaration` | Value form correct; type-only form wrong | In pinned OXC 0.126, the binder unconditionally calls `declare_symbol_for_import_specifier(..., false)` for this node (`oxc_semantic-0.126.0/src/binder.rs:367`), ignoring the AST’s `import_kind` field (`oxc_ast-0.126.0/src/ast/ts.rs:1688`). Therefore `import type String = X.Y` is incorrectly treated as a value import by the exact binder version this repository pins. |
| Annex-B function hoisting | Binder supports it; design does not cover it correctly | OXC has explicit sloppy-JavaScript Annex-B hoisting (`oxc_semantic-0.126.0/src/binder.rs:155`). Since Vue JS analysis can be script-mode, this is not N/A. Tests must cover sloppy JS, strict JS, TypeScript, and actual module source. |
| Owner topology | Relationship correct; proposed views incorrect | `TopLevelOwnerTable` correctly states instance → unique module and no reverse visibility ([top_level_owners.rs:365](crates/verter_semantic/src/analysis/top_level_owners.rs:365)). It does not itself construct that scope graph. The flat and erased views fail to model it faithfully. |

3. No, “Vue scripts always parse as ES modules” is false in this codebase.

For the component-meta/session path, Vue classifies:

- `lang="js"` as `JsModuleKind::Script`;
- `lang="jsx"` as module;
- default/TS as `ScriptSourceType::Ts`.

See [vue_bridge.rs:106](crates/verter_compiler/src/framework_common/vue_bridge.rs:106) and its mapping to OXC at [parse.rs:1215](crates/verter_session/src/parse.rs:1215). `SourceType::ts()` and `tsx()` are unambiguous, not unconditionally modules; absent ESM syntax they resolve to script source.

Other Vue lanes are inconsistent with that: prepared compiler scripts map JS to `SourceType::mjs()` ([process.rs:44](crates/verter_compiler/src/script/process.rs:44)), while TSC setup parsing deliberately uses unambiguous source types ([tsc/script.rs:1690](crates/verter_compiler/src/tsc/script.rs:1690)). Therefore the design cannot globally dismiss `with` or Annex B.

4. Vue’s conceptual direction is correct: module is the instance scope’s lexical parent, and visibility is one-way. The two-view implementation is not correct.

- Binding the flat combined `Program` puts module and instance declarations in the same root scope, not parent and child scopes. Legal same-name declarations across the two Vue scopes become redeclarations, can produce semantic errors, and cannot express “nearest instance binding wins.”
- `ReferenceId` and `SymbolId` are local dense indices belonging to one `SemanticBuilder` result. An identifier’s ID from `instance_view` cannot index `module_view`. Cloning an OXC AST clears semantic IDs (`oxc_syntax-0.126.0/src/reference.rs:21`); rebinding assigns fresh IDs. Rebinding the original AST instead overwrites its cells and invalidates the first view. Thus the claim at [binding-index-design.md:159](docs/arch/refactor/rev11/evidence/CM1/binding-index-design.md:159) is false for two views.
- Erasing only `Program.body` statements is also incomplete because `Program.directives` have no owner entries. An instance-owned `"use strict"` directive can survive into `module_view` and change its semantics. The owner table is parallel only to `Program.body` ([top_level_owners.rs:232](crates/verter_semantic/src/analysis/top_level_owners.rs:232)).

The implementation needs separate owner-correct semantic programs or an explicit instance-then-parent lookup, plus a stable per-view correlation key. It cannot reuse an `IdentifierReference.reference_id` across views.

5. The OXC binding decision is structural in a single semantic view, but the design’s end-to-end identity claim is overstated.

The ten-spelling catalog lookup after a proven `Global` result is legitimate intrinsic dispatch, not a shadowing heuristic. However:

- `Local(SymbolId)` lacks view provenance and owner identity.
- Verter’s downstream resolver is keyed by `DeclBindingKey(owner, name)`, not OXC `SymbolId`; the actual admission gate performs exactly that owner/name lookup ([eval_env.rs:942](crates/verter_session/src/host_manage/eval_env.rs:942)).
- With the flat view, duplicate module/instance spellings cannot be converted to the correct declaration owner.

So the gate can be structural after revision, but the proposed result and handoff are not currently structural binding identity all the way through.

6. The closed-fact boundary itself is correct: only String/Number/Boolean should produce primitive closed facts; the other seven should not acquire new closed semantic shapes.

The overall CM1 treatment nevertheless underreaches the charter:

- CM1 requires one producer-owned typed runtime-constructor fact/enum shared by both paths ([CM1.md:110](docs/arch/refactor/rev11/charters/CM1.md:110)). This design defines only a binding-resolution enum.
- CM1 explicitly requires constructor arrays and nullable forms. Current macro extraction handles only a direct identifier or asserted identifier ([macros.rs:3057](crates/verter_semantic/src/analysis/macros.rs:3057)); Options extraction explicitly drops constructor arrays as “no single constructor” ([options.rs:244](crates/verter_semantic/src/analysis/options.rs:244)). The design supplies no replacement.
- The statement that primitive folding is existing macro behavior is factually wrong: the macro path currently writes display text only at [macros.rs:3059](crates/verter_semantic/src/analysis/macros.rs:3059).
- Shadow-correcting the other seven spellings is sensible and does not violate the closed-fact boundary, but it is additional display-route behavior outside CM1’s explicit acceptance matrix. It should be explicitly ratified as blast-radius scope, not described as entirely unchanged behavior.

7. Other defects:

- Raw `is_value()` is a source-language value-space test, not a runtime-survival test. `declare const`, ambient functions/classes/namespaces, and related TS-erased constructs require a Vue/TS runtime projection. The cited Svelte precedent proves this rather than supporting the proposed raw-binder approach.
- The proposed placement is wrong. Macro extraction occurs in `analyze_macros_from_program_with_owners`, but Options extraction is separately called from [build.rs:653](crates/verter_semantic/src/analysis/build.rs:653). A shared index must be owned by the outer analysis driver and passed to both.
- Parser cleanliness is unavailable inside the proposed function. `parse_errors` is carried by `build_script_analysis_inner` ([build.rs:360](crates/verter_semantic/src/analysis/build.rs:360)), not by `analyze_macros_from_program_with_owners`. The claimed parse-error → `Indeterminate` behavior therefore needs explicit signature and replay changes.
- No typed carrier is specified for propagating `Local` or `Indeterminate` from shallow extraction into `AnalyzedPropField`/`AnalyzedOptionsProp` and eventually `SourcePosition`. Saying it uses Finding B’s failure channel does not establish that wiring.
- Sloppy direct `eval` is another dynamic-binding boundary. OXC already marks scopes containing direct eval, but this design never checks that flag. Such a scope must either be proven irrelevant or return `Indeterminate`.

What must change before implementation:

1. Replace the flat/erased dual-view model with an owner-correct model that preserves module-parent/instance-child shadowing and never reuses semantic IDs across views.
2. Define an exhaustive runtime-surviving projection or equivalent closed classification for ambient declarations, enums/namespaces, imports—including `TSImportEqualsDeclaration.import_kind`—and other TS-erased constructs.
3. Handle actual source types, Annex B, `with`, and direct eval explicitly, using `Indeterminate` where static identity is not provable.
4. Return a canonical binding identity usable by Verter—at minimum exact declaration owner plus name/locator—not bare `SymbolId`.
5. Build the shared index in the outer analysis driver, thread parser cleanliness through it, and specify concrete `Local`/`Indeterminate` publication carriers for both macro and Options extraction.
6. Complete the CM1 typed runtime-constructor carrier, including constructor arrays and nullable forms, and explicitly ratify or remove the seven-spelling display-route expansion.
7. Add tests for cross-owner duplicate names, ambient declarations, type-only import-equals, sloppy/strict Annex B, `with`, direct eval, per-view identity correlation, arrays, and nullable constructors.

VERDICT: NEEDS REVISION
tokens used
```
