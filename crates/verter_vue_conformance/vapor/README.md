# Native Vapor oracle

These synthetic components isolate fine-grained render contracts. The official
`@vue/compiler-vapor` and `@vue/compiler-sfc` versions are **3.6.0-rc.15**.
`oracle.json` records the source and output digests, diagnostics and script
binding metadata for each golden.

Generation parses each SFC with `compiler-sfc`, passes `compileScript`'s bindings
as `bindingMetadata`, and compiles its template with `compiler-vapor` using
`mode: 'module'`, `prefixIdentifiers: true`, `isTS: false`, and `comments: true`.
Each test compiles the components through the public runtime compilation route.
Neither test claims whole-module Vapor parity for these components.

## Loop sources

The `loop-source-*` components isolate loop-source name resolution. The test
compares the parsed complete loop-source getter expressions, keeping identifier
spellings exact and waiving only the shared canonicalizer's cosmetic syntax
categories.

The nested component deliberately reuses its parent's alias: its inner source
must read the outer reactive accessor before introducing the inner alias.
Callback parameters remain local, while callback free reads and shorthand values
resolve through the same shared binding inventory as other expressions.

Additional cases cover parameter defaults in arrow and ordinary functions,
hoisted function declarations, block-local name containment, and computed keys
in callback and variable patterns. Template-only cases use empty binding metadata.
The compiler's separate block-`var` control preserves JavaScript function scope;
these native goldens make no claim about other source forms.

## Render contracts

Every other component must compile to valid ESM that passes semantic analysis
(no redeclared lexical binding) and whose unresolved names are a subset of the
native output's. Each then compares the one construct it isolates:

- `generated-local-container`: nested elements that carry their own effects,
  events, insertion points and children; every generated local is declared in
  the closure that uses it.
- `setup-read-loop-key`: loop key callbacks, compared as parsed expressions,
  read setup bindings through the context and keep their own (possibly
  shadowing) aliases.
- `component-asset-repeat`: a component used in several blocks is resolved
  once, in the order and number Vue resolves it.
- `component-event-statement`: each component event prop returns its handler.
  A getter that returns a hoisted local is compared through that local's
  initializer.
- `condition-anchor` and `multiple-root-condition`: the node forests of the
  static templates (tags, comments, placeholders and text, ignoring attributes
  and omitted end tags) and the conditional flags. These fix where a
  conditional is inserted among later siblings, and that a multi-root
  conditional body returns separate roots. `slot-fallback-locals` uses the same
  comparison: a slot outlet's fallback children are its block roots.
- `indexed-navigation`: navigation to children at every DOM position, text
  nodes, outlet anchors and later siblings.
- `static-template-ref`: static refs call `setStaticTemplateRef`; a ref inside
  a loop calls the render's `createTemplateRefSetter` setter with `ref_for`.
- `component-slot-props` and `dynamic-slot-name`: each component's prop keys
  and slot functions with their parameter counts, the kinds and order of the
  dynamic slot list, and looped slots through `createForSlots`.
- `slot-roots`: slot templates beside default content, slot functions whose
  roots may render nothing (`{ _: 1 }` NON_STABLE flags), refs inside looped
  slot templates, and dynamic slot order.

Every case also compares the DOM node each runtime operation receives. Each
node local is followed to its structural path under runtime-vapor
3.6.0-rc.15 (`dom/node.ts`): `child(n)` and `txt(n)` are `n.firstChild`,
`next(n)` is `n.nextSibling` (its second argument is only a hydration text
flag), and `nthChild(n, i)` is `n.childNodes[i]`.

To regenerate, install both pinned packages in an isolated directory with
`npm install --ignore-scripts --save-exact @vue/compiler-vapor@3.6.0-rc.15 @vue/compiler-sfc@3.6.0-rc.15`,
then run `node crates/verter_vue_conformance/vapor/generate.cjs <oracle-install-directory>`
from the repository root. The generator refuses either package at another version.
