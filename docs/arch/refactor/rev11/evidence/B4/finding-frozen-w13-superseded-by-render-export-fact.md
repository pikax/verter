# Finding — frozen layer-1 rules W-13/W-13′ are superseded by production's
# `TemplateRenderExport` fact; V19 is now unreachable

Disposition: **ADOPT-NOW, via escalation** (codex ruling on the two B4 findings). B4 has no
amendment authority over `assembled-map-composition-layer1.md` — see that document's own
freeze clause, quoted below — so this record is the escalation, not the fix.

## What changed in production

`crates/verter_session/src/compile.rs`'s `assemble_vue_main_module` used to decide the
render-function binding by scanning the template's own generated code:

```rust
if template.code.contains("function ssrRender(") {
    out.push_str("_sfc_main.ssrRender = ssrRender\n");
} else if template.code.contains("function render(") {
    out.push_str("_sfc_main.render = render\n");
}
```

This is exactly what frozen layer-1 rules **W-13**/**W-13′** encode (`assembled-map-
composition-layer1.md:1080-1081,1102-1103`: *"template present ∧ its code contains `function
ssrRender(`"* / *"template present ∧ not W-13 ∧ its code contains `function render(`"*, noted
as *"W-13's choice is made by a **text scan of the template code**"*).

B4 replaced the scan with a declared fact: `RuntimeTemplateBlock`/`VerterTemplateBlock` now
carry `render_export: TemplateRenderExport` (`Render` | `SsrRender`), set once at the
producer (`crates/verter_compiler/src/compile/mod.rs`, from the same `verter_options.ssr`
the backend already used to choose VDOM/Vapor vs SSR codegen) and consumed exhaustively:

```rust
match template.render_export {
    TemplateRenderExport::SsrRender => out.push_str("_sfc_main.ssrRender = ssrRender\n"),
    TemplateRenderExport::Render => out.push_str("_sfc_main.render = render\n"),
}
```

`TemplateRenderExport` has exactly two variants and no "neither" arm. A present template
block now ALWAYS carries a binding — there is no code path left in which W-13 and W-13′ both
fail to match.

## Why V19's frozen derivation is now unreachable

`packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`
vector **V19** exists to prove **§5.7** (template rewrite immunity) and **§6.4 case 3** (no
BR-3 boundary segment) using a template body that deliberately contains neither rewrite
target as a real function — its own `derivation` field states: *"The template code contains
neither 'function ssrRender(' nor 'function render(' (it is a scaffolding/tail fragment, not
a render function), so W-13/W-13′ both skip."* Frozen `expected.code` therefore has NO
render-binding line.

Production can no longer produce that outcome for ANY present template block, V19's included
— `render_export` is mandatory and matched exhaustively, so production now emits
`_sfc_main.render = render\n` for V19's input (its `AssembleInput.ssr` is `false`, so
`render_export` resolves to `Render`). This is not a regression in the sense of a wrong
answer: W-13/W-13′'s own text-scan basis is the class of "generated-product reparsing" this
program's architecture rules (`CodeTransform Is the Single Source of Truth`, `Carrier
Geometry From Registered Facts`) forbid production code from doing. There is no legitimate
reachable "neither" case left, and no basis to reject the production fix.

## This requires a real amendment — not a B4 decision

`assembled-map-composition-layer1.md:60` (§ "Freeze"): *"Per AMD-008:161-164, after this
document is frozen, changing layer-1 semantics requires its own amendment. It is not a BV0A
implementation decision, it is not something a vector can do, and it is not something a
change to a shared helper elsewhere in the tree can do."* `assembled-map-composition-
layer1.md:56` adds: *"where a vector and frozen layer 1 could be read to disagree, layer 1
governs."*

B4 therefore did **not** edit `assembled-map-composition-layer1.md`, W-13/W-13′, or hand-fix
the V19 vector in `assembled-map-composition.vectors.json`. This record is the escalation to
the program orchestrator for a ruling on the actual amendment. Recommended scope for that
amendment, for the amendment-owner's consideration (not itself a ruling):

1. Retire W-13/W-13′'s text-scan definition; redefine the render-binding write rule in terms
   of a declared per-template-block fact (mirroring `TemplateRenderExport`) instead of a scan
   over W-11's own output.
2. `packages/framework-conformance-harness/src/assembled-map-write-grammar.mjs:189-192` — the
   JS reference driver — still implements the literal text-scan (`input.template.code.
   includes("function ssrRender(")` / `"function render("`). It needs the same fact-based
   redefinition in the same amendment, or the two implementations (Rust production, JS
   reference) will keep disagreeing on any vector whose template text doesn't already match
   its own declared render kind.
3. V19 itself needs regenerating (or splitting) under the amended rule — it currently cannot
   express "a template present with a scaffolding, non-render body" without also asserting a
   binding line, since that shape no longer exists in production.

## Addendum — V19 now fails final-parse too, not just text divergence

A later change made `assemble_vue_main_module`'s final-parse check dialect-accurate (it
previously hardcoded a permissive `SourceType::tsx()`, whose `Unambiguous` module-kind does
not enforce the "a module cannot have multiple default exports" rule; the accurate dialect —
plain JavaScript by default — uses an explicit `Module` kind, which does). V19's template
body is `"__sfc__\nexport default _sfc_main;\ntail"` — a deliberately synthetic fragment
containing the literal text `export default _sfc_main;` as PART OF ITS OWN (unrewritten,
verbatim) code, to exercise §5.7's "template rewrite immunity." Composed alongside
`assemble_vue_main_module`'s own always-present trailing `export default _sfc_main`, the
result now genuinely has two default exports and fails the (correctly stricter) final-parse
check — `production_outcome` reports `ComposeOutcome::AssemblyFailed` for V19 rather than
`Composed`. This is a DEEPER instance of the same underlying issue this finding already
names (V19's premise — a template fragment carrying literal `export default` text — is
obsolete under the current architecture), not a new, separate problem: it does not change the
escalation above, and strengthens recommendation 3 (V19 needs regenerating). The existing
divergence exclusion in `vector_inventory.rs` covers this outcome the same way it already
covered the text-content divergence — see that file's own updated doc comment.

## Interim measure (this change, not the amendment)

`crates/verter_session/src/compile/map_equality_tests/vector_inventory.rs`'s
`every_positive_vector_reproduces_its_frozen_expected` excludes ONLY V19's divergence from
its pass/fail assertion — V19 is still loaded, still executed through `production_outcome`,
and still counted in the executed-id parity check (`every_vector_in_the_suite_was_exercised`
is untouched and still passes: V19 is never skipped, only its one known, explained divergence
is not fatal). Every other vector in both the positive and fail-closed arrays is still
checked byte-for-byte against its frozen `expected`, unchanged. This keeps the gate green
without touching the frozen artifact, pending the amendment above.
