# Vapor Full-Parity Plan (codex-architect direction)

> Source: codex-architect (gpt-5.6-sol) direction consult, 2026-07-19. Raw consult: `$CLAUDE_JOB_DIR/tmp/vapor-codex-pure.out`.
> Goal: bring Verter's Vapor backend to full **structural** parity with official Vue 3.6.0-rc.1 Vapor (same helper calls + reactivity/DOM topology; whitespace, var names, line numbers, source maps all waived).
> Status: DRAFT for review — **tweak freely**. This drives the V2 implementer slice briefs. V2 runs after V1a (defineComponent gate + render arity) and V1b (inline-template).
>
> **How to use:** edit this file to adjust scope/order/decisions; when ready, tell the CTO agent and it will turn each slice into a bounded implementer brief (TDD, structural conformance cells + runtime DOM behavioral checks).

---

**Audit verdict (2026-07-22): OUT-OF-SCOPE.** Vapor compiler/codegen parity is explicitly outside this audit mandate.

## Root cause

Verter's Vapor lowering makes template **depth** part of the static-vs-reactive decision: it builds **one HTML string** and special-cases only structural **roots**. Nested (depth>0) `v-for`/`v-if`/components get baked into the static HTML template instead of recursing into child reactive blocks. Root-depth works; nesting silently produces static HTML that never updates — a **runtime reactivity bug**, not a cosmetic diff.

## Direction (the architectural fix)

Stop making depth part of the decision. Lower every Vapor render into a **recursive tree of static shells and reactive block boundaries**, via two mutually-recursive operations:

- **`build_static_shell(node, lexical_scope)`** — serializes only ordinary elements and static content. When it encounters a **reactive boundary** (`v-for` / `v-if` / component / `<component :is>`), it **stops serializing that subtree and records an ordered insertion site**.
- **`lower_block(node, lexical_scope, insertion_context)`** — emits the runtime block for the boundary (`_createFor` / `_createIf` / `_createComponent` / `_createDynamicComponent`).

**The boundary check must happen BEFORE recursively appending a child to the HTML string. Depth must never affect the result.** Make the existing root path call this same machinery (no separate root special-case).

Each lowered block **owns**: its static template + numeric template flags; its instantiated root handle; its local bindings and effects; its recursively-lowered child blocks; its insertion-site / root-return contract.

**Binding is scope-aware, not textual.** A `v-for` callback item is a runtime **cell**; references lower through `_for_itemN.value`. Shadowing creates a **new lexical binding** — do **not** append `.value` based merely on identifier spelling.

Canonical nested loop lowering:
```js
_createFor(() => _ctx.groups, (_for_item0) => {
  const n1 = t0()                                   // section static shell
  const n2 = _createFor(() => _for_item0.value.items, (_for_item1) => {
    const n3 = t1()                                 // span static shell
    _renderEffect(() => _setText(n3, _toDisplayString(_for_item1.value)))
    return n3
  })
  _setInsertionState(n1)                            // anchor the nested block, official ordering
  return n1
})
```
- Nested `_createFor` closures; the **inner source unwraps the outer item via `.value`** (`_for_item0.value.items`).
- `_setInsertionState(iterationRoot)` establishes the child block's DOM anchor when the returned shell contains structural children.
- Each block **returns its root node**.
- Treat a `v-if`/`v-else-if`/`v-else` chain as **one boundary**; each branch closure recursively lowers its own shell + descendants.
- Components and `<component :is>` are boundaries at **every** depth — they must never enter serialized HTML.

## `createFor`, insertion state, component selection

`_createFor` model is **uniform at root and nested depths**:
- Source: a reactive closure evaluated in the enclosing lexical scope.
- Callback: receives the loop cell, creates the iteration root, emits effects + nested blocks, returns that root.
- Alias reads lower through `_for_itemN.value`.
- Nested blocks created **inside** the iteration callback.
- `_setInsertionState(iterationRoot)` when the returned shell has structural children, in official ordering.
- **Flags:** compute a numeric flag set **independently per loop** from its semantic facts — do NOT inherit from parent or infer from depth. Golden-lock keyed/unkeyed/other forms rather than guessing numeric values.
- **Templates:** replace boolean args with an explicit numeric bitmask; emit `1` for a root single-node template.

Centralize **component factory selection**:
- `_createComponent` — compile-time / directly resolved component identity (incl. resolved built-ins after Vapor rename).
- `_createComponentWithFallback` — runtime asset resolution that can fall back.
- `_createDynamicComponent` — runtime `<component :is="expr">`.
- `VaporTeleport` / `VaporKeepAlive` — built-in Vapor identities, not `Teleport` / `KeepAlive`.

Events: compose `withVaporModifiers` and `withVaporKeys` on the delegated handler path in the **same order** as official goldens. Scripts: never default to `defineComponent` — select `defineVaporComponent` when a runtime wrapper is required, else emit the plain component object with `__vapor: true`.

### Gap classification

| Gap | Classification |
|---|---|
| Nested `v-for`/`v-if` recursion | STRUCTURAL |
| Recursive insertion-state ownership | STRUCTURAL |
| Outer loop item `.value` in nested scopes | STRUCTURAL |
| Components inside loops/branches | STRUCTURAL integration |
| `_createComponent` vs fallback | SELECTION |
| `<component :is>` helper choice | SELECTION (nested support depends on recursion) |
| Vapor built-in names (`VaporTeleport`/`VaporKeepAlive`) | RENAME |
| Numeric `_template` flags | SELECTION / encoding |
| `.prop` → `setDOMProp` | SELECTION |
| Object `v-bind` → `setDynamicProps` | SELECTION |
| Vapor event modifier wrappers | SELECTION / wiring |
| Vapor component wrapper / marker | SELECTION |

## Work sequence — 7 bounded slices

Start each slice with **failing structural goldens**. Risk concentrates in slices 3–6; the early selection slices make leaf emitters correct **before** recursive lowering starts invoking them from new positions.

1. **Template / helper normalization** — numeric template flags, `VaporTeleport`/`VaporKeepAlive`, `setDOMProp`, object-form `setDynamicProps`, delegated event wrappers.
   *Cells:* single-root template, Teleport, KeepAlive, `.prop`, `v-bind="obj"`, key + non-key event modifiers.
2. **Component + script-wrapper selection** — centralize component classification (`_createComponent` / `_createComponentWithFallback` / `_createDynamicComponent`); `defineVaporComponent` vs plain `__vapor:true` object.
   *Cells:* directly-bound component, runtime asset component, root `<component :is>`, wrapper-required SFC, wrapper-free SFC.
3. **Recursive Vapor block plan (core)** — introduce static-shell cut points, ordered insertion sites, lexical scopes, recursive block emission; make the existing root path call this same machinery.
   *First target:* the exact nested-`v-for` seed — prove the inner loop is absent from static HTML and emitted inside the outer callback.
4. **Complete `v-for` semantics** — cell-backed aliases, `.value` resolution, per-loop IDs, keys/indices as supported, flags, returned iteration roots, insertion-state emission.
   *Cells:* nested loop, three-level loop, nested source `group.items`, shadowed aliases, keyed/unkeyed loops, structural siblings before/after the nested loop.
5. **Recursive conditional lowering** — lower complete conditional chains at any insertion site; recursively process every branch.
   *Cells:* `v-for`+nested `v-if`, `v-if`+nested `v-if`, `v-if` containing `v-for`, `v-else-if`/`v-else`, conditional first/middle/last child.
6. **Components across structural scopes** — route component boundaries through the recursive block plan; reuse slice-2's factory selector.
   *Cells:* component-in-`v-for`, resolved component in `v-if`, `<component :is v-if>`, dynamic component in a nested loop, Vapor built-ins under conditionals.
7. **Topology + runtime hardening** — multiple dynamic children, deeper mixed nesting, fragments/multiple roots, helper-import exactness, absence of serialized structural content.
   *Validation:* full structural conformance matrix + behavioral mutation tests.

## Top risks + validation

1. **Incorrect shell partitioning / insertion ownership** — duplicated nodes, structural content left in HTML, wrong sibling placement, blocks anchored to wrong root.
   *Validate:* nested blocks in first/middle/last child positions, multiple structural siblings, mixed text/element siblings. Runtime: add/remove/reorder outer + inner list items, assert exact DOM order with no stale nodes.
2. **Incorrect lexical-cell unwrapping** — `_for_item0.items` (missing `.value`), double `.value`, rewriting shadowed identifiers, evaluating nested sources against `_ctx`.
   *Validate:* canonical `_for_item0.value.items` output, three-level scopes, alias shadowing, aliases in text/props/handlers/conditions/nested-sources. Runtime: replace an outer item + mutate its inner collection.
3. **Wrong boundary composition / helper selection** — `v-if`/`v-for` nested in wrong order, dynamic components become fallback components, helper imports disagree with calls.
   *Validate:* all mixed seed cells (loop+if, if+if, component-in-loop, `<component :is v-if>`). Runtime: toggle conditions + `:is`, verify mount/unmount + component identity. Assert exact called/imported helper sets.

## Target model (decision)

**Do NOT** replace Verter's shared AST/IR with a wholesale copy of official Vapor IR — the existing semantic input is sufficient, and an IR migration would mix frontend risk into a backend-topology repair.

**Do** introduce a **backend-local `VaporBlockPlan`** whose concepts directly mirror official Vapor:
- static shell
- reactive boundary
- lexical scope
- insertion site
- returned root
- effects + child blocks

More than patching the current emitter, less than importing official's full IR. Makes structural parity explicit, removes depth-specific behavior, and gives every future Vapor feature one correct recursive lowering path.
