---
ruling_id: "CSS-CLEAN-CUTOVER"
type: "maintainer-directive"
date: "2026-08-20"
date_source: "stated"
binds: ["Track J (J1-J4)", "CSS/style pipeline architecture"]
source_file: "MAINTAINER-DIRECTIVE-CSS-CLEAN-CUTOVER.md"
summary: "Updates the Track J plan: remove lightningcss completely, verter_css_syntax becomes the sole CSS-family syntax authority for CSS/SCSS/Sass/Less/Stylus, no legacy parser/printer path retained. Specifies Verter's CSS responsibility boundary (owns parsing/facts/Vue semantics/CodeTransform/source-maps; does not own SCSS-family lowering, normalization, minification, autoprefixing, arbitrary PostCSS/JS callbacks), the required style pipeline stages, generic multi-source-map composition (not a CSS-specific merge_source_maps), the preprocessor contract, and J1-J4 planning implications with required acceptance evidence."
supersedes:
  - ruling: "NO-LIGHTNINGCSS"
    claim: "That document's final state ('ALL CSS WORK STOPS UNTIL THE J TRAIN' — the J1 charter draft PARKED, not ratified, not landed, and not to be advanced; no further CSS consults, drafts or amendments) is superseded: this directive updates the Track J plan now, on the finding that CSS costs zero schedule today so this is the cheapest moment to do the planning. The underlying architectural decision (lightningcss removal, verter_css_syntax as sole authority) is NOT superseded — it is carried forward and detailed further."
superseded_by: []
contradicts: []
notes: "States explicitly it 'SUPERSEDES the CSS re-entry proposal's \"keep the suspension\" recommendation' and that the scope of this directive is to update the PLAN, not itself dispatch J1 implementation (charters ratified per-artifact under the batched-ratification protocol)."
---

# Program Orchestrator Directive — CSS Architecture Clean Cutover

**Status: RATIFIED by the maintainer, 2026-08-20.** Recorded verbatim below. This directive SUPERSEDES
the CSS re-entry proposal's "keep the suspension" recommendation: the maintainer has decided to update
the Track J plan now. Note the proposal's underlying finding still holds and argues FOR this timing —
CSS costs zero schedule today, so this is the cheapest moment to do the planning.

Scope of this directive: it updates the PLAN. It does not by itself dispatch J1 implementation;
resulting charters are ratified per artifact under the batched-ratification protocol.

---

Update the current architecture/program plan for Track J to reflect the following decisions. These are intentional architectural decisions, including where they introduce breaking changes. Do not preserve the legacy Lightning CSS path for compatibility.

## Architectural decision

Remove `lightningcss` completely from Verter.

`verter_css_syntax::StyleSyntaxIr` becomes the sole CSS-family syntax authority for CSS, SCSS, Sass, Less, and Stylus.

There must be no legacy CSS parser/printer/normalizer path retained beside it, and no compatibility requirement to preserve Lightning CSS-normalized output bytes.

The previous BCSS0 concern about changing standalone CSS output from normalized bytes to authored-preserving bytes is explicitly resolved in favor of the new architecture. The clean cutover is intentional.

## Verter's CSS responsibility

Verter is not a general-purpose CSS compiler.

Verter owns:

* parsing CSS-family syntax;
* extracting CSS and framework semantic facts;
* Vue-specific style semantics;
* source provenance and `CodeTransform`-based transformations;
* source-map generation for Verter-owned transforms;
* generic source-map composition where required for final products.

Verter does **not** own:

* SCSS/Sass/Less/Stylus lowering to CSS;
* CSS normalization or AST serialization;
* nesting lowering;
* autoprefixing;
* minification;
* browser-target lowering;
* arbitrary PostCSS processing;
* arbitrary user JS callbacks.

External preprocessors remain owned by the builder/JS environment.

## Required style pipeline

The target pipeline is:

```text
authored CSS-family source
        |
        | Verter parse
        v
StyleSyntaxIr + semantic facts
        |
        | optional Verter compatibility transform
        | ONLY where required to prevent a selected
        | preprocessor from breaking on framework syntax
        v
preprocessor input
        |
        | external builder / JS preprocessor
        | Sass / SCSS / Less / Stylus -> CSS
        v
plain CSS + upstream source map + dependencies
        |
        | re-enter Verter as a NEW exact style identity
        | parse once as CSS
        v
StyleSyntaxIr
        |
        | Verter Vue-specific transforms
        | using CodeTransform
        v
Vue-transformed CSS + Verter source map
        |
        | optional external builder CSS stages
        | modules / PostCSS / minification / etc.
        v
final CSS
```

A byte-changing external preprocessor creates a new explicit style identity. Parsing the resulting CSS is therefore not duplicate parsing of the authored identity.

## Vue-specific CSS ownership

Vue-specific CSS transformations remain implemented in Verter.

This includes, as applicable:

* `v-bind()`;
* scoped selector injection;
* `:deep()`;
* `:global()`;
* `:slotted()`;
* scoped keyframes and associated animation-name rewriting;
* other Vue-owned style semantics.

The builder may orchestrate when these transformations run, but it must not become their implementation authority.

Do not move these transforms into ad-hoc JS string processing or another independent CSS parser.

The architectural distinction is:

> Verter does not compile CSS, but Verter does compile Vue semantics expressed inside CSS.

## Preprocessor ordering

Prefer the same semantic ordering as Vue's official compiler:

1. parse/analyze authored style;
2. externally preprocess non-CSS languages;
3. parse the resulting CSS;
4. apply Vue-owned CSS transformations.

Do **not** automatically replace `v-bind()` or other framework constructs before preprocessing.

The authored preprocessor input should remain byte-identical unless a specific construct is proven incompatible with the selected preprocessor.

If compatibility rewriting is required before preprocessing, treat it as a narrow transport/protection transform rather than conflating it with the final Vue runtime lowering.

Every such pre-preprocessor rewrite must be:

* explicitly justified per dialect/preprocessor;
* surgical;
* represented with `CodeTransform`;
* source-mapped;
* covered by a discriminating test.

## No CSS normalization or printing

Delete the Lightning CSS normalization/serialization model.

Verter transformations must operate as:

```text
output = input bytes + explicit framework edits
```

not:

```text
output = parse(input) -> serialize(AST)
```

Untouched authored or preprocessed bytes must remain untouched.

Comments, whitespace, formatting, modern CSS syntax, and preprocessor output formatting must survive unless a framework-owned transform explicitly changes the relevant bytes.

## CodeTransform is the sole Verter transform mechanism

All Verter-owned CSS byte transformations must use `CodeTransform`.

The same edit representation must be responsible for:

* producing output bytes;
* producing the stage-local source map;
* preserving unmapped synthetic regions correctly.

Do not introduce a second style-specific edit/mapping implementation.

Where there are no edits, avoid materializing transformed output. Preserve/borrow the existing exact identity where ownership permits.

Generate source maps only when demanded by the product boundary.

## Generic source-map composition

Do not implement this as a CSS-specific `merge_source_maps()` helper.

Introduce or evolve a generic Verter source-space composition facility capable of composing ordinary source-map stages:

```text
C -> B
B -> A

becomes

C -> A
```

Conceptually this should operate over qualified source spaces, not merely source-map JSON strings.

For example:

```text
final CSS
    -> Vue-transformed CSS
    -> preprocessor output
    -> preprocessor-safe authored style
    -> authored style block
    -> .vue source
```

The final build product may flatten this chain into a terminal source map.

IDE paths do not need to eagerly flatten the chain. They may retain the source-space/map graph and resolve through it on demand.

## Do not make `CodeTransform::chain_source_map` the universal composer

The existing `CodeTransform::chain_source_map` may remain as a specialized optimization, but its current fail-closed chunk restrictions must not define the semantics of general source-map composition.

The clean separation should be:

```text
CodeTransform:
    edits -> output bytes + stage-local map

SourceMap composer:
    stage-local maps -> composed terminal map
```

Once a `CodeTransform` has generated its normal source map, generic composition should not need to know whether that transform contained inserts, overwrites, moves, or other edit shapes.

Any optimized specialized composition route must be proven equivalent to the generic composition semantics.

## Multi-source maps are required

The generic composer must support multiple upstream source files from the beginning.

Do not bake in a single-source assumption.

Preprocessors can legitimately produce maps involving imported files, for example:

```text
Component.vue
_variables.scss
_mixins.scss
theme/_colors.scss
```

Source-map `sources` strings are presentation metadata, not sufficient identity.

Composition should use Verter's source-space/artifact identities internally and preserve multiple source origins in the resulting map.

Sourceless/unmapped segments must remain sourceless. Composition must never "look through" an intentionally unmapped region and fabricate provenance from an earlier mapping.

## Preprocessor contract

External style preprocessors must return an explicit result carrying at least:

* processed bytes;
* source map;
* dependencies;
* diagnostics where available;
* processor identity/version;
* relevant configuration fingerprint.

The Rust compiler core must not gain arbitrary filesystem/process authority in order to perform these preprocessors internally.

The builder/host owns invoking external tooling and feeds the sealed result back into Verter.

## Parsing policy

For any dialect declared `Native` by Verter:

> Valid syntax that Verter cannot parse is a Verter bug.

Do not retain Lightning CSS or another parser as a fallback.

Distinguish this from invalid or incomplete user source.

Invalid/incomplete user source should produce diagnostics and recovery information.

Recovery may still support analysis where facts are sound, but byte-changing transformations must not guess across structurally ambiguous or recovered regions.

A framework rewrite should require sufficient completeness around the exact rewrite target.

## Semantic extraction

CSS semantic analysis must continue to come from the shared `StyleSyntaxIr`.

This includes applicable facts such as:

* selectors;
* classes;
* IDs;
* custom properties;
* `var()` usages;
* at-rules;
* framework-specific pseudos;
* `v-bind()` expressions;
* source spans;
* completeness/recovery state.

Remove private scanners or grammar authorities when equivalent facts are available from the shared syntax substrate.

## CSS Modules

Treat CSS Modules separately from core Vue selector transforms.

Verter should own syntax/semantic analysis required for IDE/compiler understanding of module classes.

Runtime CSS Modules transformation may remain externally owned where arbitrary JS configuration or callbacks such as custom `generateScopedName` are part of the public ecosystem contract.

Do not execute arbitrary user JS configuration inside the Rust compiler solely to claim ownership convergence.

A deterministic built-in native modules capability may exist later if its configuration domain is fully explicit and typed.

## Public API cleanup

Do not preserve the existing standalone `processStyle` semantics simply for compatibility.

The current abstraction implies that Verter is a standalone CSS compiler, which is no longer the desired architecture.

Reshape the boundary around explicit responsibilities, conceptually similar to:

```rust
analyze_style(authored_input)
    -> StyleAnalysis

prepare_style_for_preprocessor(authored_input)
    -> PreparedStyle

transform_vue_style(plain_css_input, vue_options)
    -> VueStyleOutput

compose_source_maps(map_chain)
    -> SourceMap
```

Exact API names and placement may differ, but the ownership boundaries must remain explicit.

Breaking the existing standalone `processStyle` contract is acceptable.

## Lightning CSS removal

Track J must include complete removal of:

* the `lightningcss` dependency;
* `normalize_css`;
* Lightning CSS parse/print normalization;
* the legacy `css::process_style` transformation authority;
* Lightning-backed scoped/modules/walk implementations where superseded by the canonical planner;
* tests whose only purpose is to preserve normalized Lightning CSS output;
* compatibility assumptions derived from the old standalone CSS route.

Do not retain a hidden or feature-gated fallback implementation.

## J1/J2/J3/J4 planning implications

Update the Track J plan so the work is sequenced cleanly.

### J1 — authority and clean cutover

J1 should:

* ratify `StyleSyntaxIr` as the sole CSS-family syntax authority;
* inventory and migrate all consumers;
* define the external preprocessor boundary;
* define Vue-owned versus builder-owned transformations;
* supersede the old standalone CSS API/contract;
* remove Lightning CSS and duplicate scanners/owners;
* carry forward the useful BCSS0 source-map test intent, but not its legacy compatibility assumptions.

### J2 — exact identities

J2 should ensure exact style identity includes, as relevant:

* exact bytes;
* dialect;
* parse-affecting options;
* compatibility/grammar epoch;
* preprocessor processor/config identity for processed results.

Authored input, prepared preprocessor input, external preprocessor output, and Vue-transformed CSS are distinct identities when their bytes differ.

### J3 — shared plans and source-map materialization

J3 should:

* ensure Verter-owned transforms use shared syntax/fact walks where appropriate;
* use `CodeTransform` for all byte edits;
* avoid copies/materialization for unchanged surfaces;
* build maps/provenance only when demanded;
* introduce the generic source-space/source-map composition engine;
* prove map composition over multi-stage and multi-source cases.

### J4 — dialect/preprocessor/recovery contract

J4 should:

* declare Native / External / Unsupported capabilities per dialect operation;
* prove CSS/SCSS/Sass/Less/Stylus parser coverage;
* prove recovery behaviour;
* prove preprocessor handoff contracts;
* prove no private duplicate grammar remains;
* prove no valid supported syntax relies on Lightning CSS or another fallback parser.

## Required acceptance evidence

The revised plan must require discriminating tests, not merely shape-valid maps.

At minimum cover:

* plain CSS passthrough with no transformation;
* `v-bind()` transformation;
* scoped selectors;
* `:deep`;
* `:global`;
* `:slotted`;
* keyframe scoping;
* CSS nesting;
* SCSS;
* Sass;
* Less;
* Stylus;
* preprocessing with imports;
* multiple upstream source files;
* preprocessing followed by Vue transformation;
* Vue transformation followed by an external builder transform;
* unchanged source-map identity;
* exact coordinate mapping before and after byte-length-changing edits;
* unmapped synthetic regions;
* UTF-16/non-ASCII positions;
* multiline transforms;
* option-off/no-map product behaviour;
* parse recovery without unsafe rewriting;
* valid dialect constructs that previously exposed parser gaps.

A source-map test must verify actual generated-to-authored coordinate discrimination at multiple points. Valid JSON, `sourcesContent`, or merely having one mapped token is insufficient acceptance evidence.

## Final invariant

The target state should satisfy all of the following:

```text
one CSS-family parser authority:
    verter_css_syntax

one Verter byte-transform mechanism:
    CodeTransform

one generic source-map composition model:
    Verter source-space composition

external preprocessors:
    builder/JS owned

Vue CSS semantics:
    Verter owned

generic CSS compilation:
    not Verter owned

Lightning CSS:
    absent
```

Do not introduce compatibility shims that compromise these boundaries.

Where existing public behaviour conflicts with this architecture, prefer the architecture and record the breaking change explicitly.
