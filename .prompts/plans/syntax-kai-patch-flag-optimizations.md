# Patch Flag Optimizations — Remaining Steps

## Context

The prop-level patch flag estimation in `syntax_kai/syntax.rs` (`estimate_patch_flag()`) is complete.
It handles all `PropKind` variants, `dynamic_props` tracking, component CLASS/STYLE→PROPS,
`has_ref`, and `has_vnode_hook` detection.

Three optimization steps remain that require information beyond props:
1. **Binding refinement pass** — re-evaluate patch flags after script setup binding analysis
2. **Children-step optimizations** — computed at tag close or during children traversal
3. **Block tree optimization** — computed as a post-tree pass after all elements are known

### Architectural difference: Event-driven vs AST-based

Vue's official compiler builds a full AST first, then walks it top-down to compute
optimizations (patch flags, block tree, hoisting). It has the luxury of seeing the
complete tree structure before making any decisions.

Verter's `syntax_kai` does **not** build an AST. Instead, it emits **sequential events**
(SAX-style streaming) as the template is tokenized: `ElementOpenTagStart`, `Prop`,
`ElementOpenTagEnd`, `Text`, `Interpolation`, `ElementCloseTag`, etc. Optimizations
must be computed incrementally as events arrive.

**What this means for each optimization step:**

| Step | Vue (AST) | Verter (events) |
|------|-----------|-----------------|
| Patch flags (props) | Walk node's `props` array after full parse | Accumulate in `estimate_patch_flag()` as each `Prop` event fires |
| Children analysis | Walk node's `children` array | Track child state on a stack, finalize at `ElementCloseTag` |
| Block tree | Post-AST `transform` pass with full tree visibility | Must be deferred to codegen or a post-pass over collected events |
| Binding refinement | `transformExpression` during AST walk, bindings already known | Separate post-pass — syntax events fire before setup analysis |

**Implications:**
- **Forward-only**: When a `Prop` event fires, we don't yet know the element's children.
  Optimizations that depend on children (TEXT flag, NEED_PATCH finalization) must be deferred
  to the close tag event.
- **No backtracking**: Once an event is emitted, it's done. If later information would change
  a decision (e.g., binding refinement downgrades a flag), this must be a separate post-pass
  that mutates the collected events.
- **Stack-based context**: Parent element state lives on a stack (`last_event_open_tag`).
  When a child element opens, the parent is pushed; when the child closes, context resumes.
- **Trade-off**: No AST allocation cost, lower memory footprint, single-pass where possible.
  But multi-pass optimizations (binding refinement, block tree) require storing events and
  revisiting them.

---

## TODO 1: Binding Refinement Pass (Post-Setup Analysis)

**When**: After `<script setup>` bindings have been analyzed (binding types are known).

**Why**: The prop-level `estimate_patch_flag()` runs during template parsing, before we know
whether a bound expression references a constant or reactive binding. It conservatively marks
all `:prop="expr"` as dynamic. A second pass can downgrade or remove flags when the expression
only references constant bindings.

### Example

```vue
<script setup>
const id = ''
</script>

<template>
  <p :id="id"></p>
</template>
```

Vue's compiler knows `id` is a `setup-const` binding, so `:id="id"` is effectively static.
The compiled output has **no patch flag** — just a static props object:

```js
_createElementBlock("p", { id: $setup.id })
```

Without the refinement pass, `estimate_patch_flag()` would set `PROPS` and add `"id"` to
`dynamic_props`, which is safe but sub-optimal.

### Binding categories (from Vue's `<script setup>` analysis)

| Binding type       | Example                          | Dynamic? |
|--------------------|----------------------------------|----------|
| `setup-const`      | `const id = ''`                  | No — value never changes |
| `setup-const`      | `const obj = { x: 1 }`          | No — reference never changes (but object may mutate) |
| `setup-ref`        | `const count = ref(0)`           | Yes — `.value` changes |
| `setup-let`        | `let x = 0`                      | Yes — variable reassignable |
| `setup-reactive-const` | `const state = reactive({})` | Yes — properties are reactive |
| `setup-maybe-ref`  | `const x = useSomething()`       | Yes — might be a ref |
| `props`            | `defineProps<{ msg: string }>`   | Yes — parent can change props |
| `props-aliased`    | `const { msg: m } = defineProps` | Yes |

Only `setup-const` with a literal or non-reactive initializer produces a truly static binding.

### Approach

1. After script setup analysis, build a `HashMap<&str, BindingType>` of all top-level bindings
2. Walk each element's props that contributed to `PROPS` / `dynamic_props`
3. For each bound prop, analyze the expression to check if **all** referenced identifiers
   are `setup-const` bindings
4. If so, remove that prop from `dynamic_props` (and its contribution to the patch flag)
5. After processing all props: if `dynamic_props` is now empty and the only flag was `PROPS`,
   clear the `PROPS` flag entirely

### Scope

This also affects:
- **`:class` / `:style`** on plain elements: if the expression is const, no CLASS/STYLE flag needed
- **`:ref`**: if the ref expression is const, `has_ref` can be cleared (though this is rare)
- **Interpolations**: `{{ constVar }}` in text doesn't need TEXT flag (children-step concern)

### Where

This pass sits between syntax event collection and codegen. It needs:
- The syntax events (element list with patch flags and dynamic_props)
- The binding analysis results from `<script setup>`
- The raw template source (to re-read expression text and resolve identifiers)

Could live in `crates/verter_core/src/syntax_kai/` as a `refine_patch_flags()` post-pass,
or in a new module between syntax and codegen.

---

## TODO 2: Children-Step Optimizations

**When**: At element close tag (all children are known) or during child event processing.

**File**: `crates/verter_core/src/syntax_kai/syntax.rs` — `handle_tag_close()` or a new
finalize step on `ElementOpenTagEnd`.

### 2.1 TEXT flag (PatchFlags::Text)

When an element's only children are interpolations (`{{ expr }}`), set `PatchFlags::Text`.
This enables the fast path where Vue only patches `textContent`.

Vue's compiler logic:
```
if (child_count == 1 && child is interpolation or compound with interpolations) {
    patchFlag |= TEXT
}
```

**Approach**: Track child types during event processing. When the close tag fires, check if
the only child content was interpolation(s) (no static text siblings, no element children).

### 2.2 Conditional NEED_PATCH finalization

In Vue's compiler, `NEED_PATCH` is only added when no other substantial flags exist:
```
if (patchFlag === 0 || patchFlag === NEED_HYDRATION)
    && (hasRef || hasVnodeHook || hasRuntimeDirectives) {
    patchFlag |= NEED_PATCH
}
```

**Approach**: At tag close, check `has_ref` / `has_vnode_hook` and the current `patch_flag`.
If only 0 or NEED_HYDRATION is set, add NEED_PATCH. This makes the current eager NEED_PATCH
for `Show`/`Directive` slightly over-approximate (safe but not minimal); optionally refactor
those to also defer.

### 2.3 Fragment flags

Fragments are created by structural directives and multi-root templates:

| Flag              | Condition                                            |
|-------------------|------------------------------------------------------|
| STABLE_FRAGMENT   | Fragment children order never changes (e.g., v-if branches, static multi-root) |
| KEYED_FRAGMENT    | v-for children with `:key`                           |
| UNKEYED_FRAGMENT  | v-for children without `:key`                        |
| DEV_ROOT_FRAGMENT | Root-level comments in dev mode                      |

**Approach**: These are set on the fragment wrapper, not on individual elements. Need to detect:
- v-for elements → check if any child has `:key` → KEYED vs UNKEYED
- v-if/else chains → STABLE_FRAGMENT (children order is structurally fixed)
- Multi-root template → STABLE_FRAGMENT
- Root comments in dev → DEV_ROOT_FRAGMENT

### 2.4 DYNAMIC_SLOTS (PatchFlags::DynamicSlots)

Set on components when slots reference `v-for` iterated values or have dynamic slot names.

**Approach**: During slot processing, detect if a `v-slot` has a dynamic name or references
bindings from an ancestor `v-for`. Set on the component's patch flag.

---

## TODO 3: Block Tree Optimization

**When**: Post-tree pass — after all elements and their children have been processed.

**Where**: A new pass between syntax event collection and codegen, or integrated into codegen.

### What is the block tree?

Vue 3's block tree is the core rendering optimization. Elements that are "blocks" use
`openBlock() + createBlock()` instead of `createVNode()`. A block collects all its
dynamically-patched descendants into a flat `dynamicChildren` array, enabling O(n) diffing
where n = number of dynamic nodes (not total nodes).

### Which elements become blocks?

| Element type                        | Block? | Reason |
|-------------------------------------|--------|--------|
| Root template element               | Yes    | Always |
| Components                          | Yes    | Always (they manage their own subtree) |
| Elements with v-if / v-else-if / v-else | Yes | Structural — children change entirely |
| Elements with v-for                 | Yes    | Structural — children count changes |
| Elements with `v-on` (shouldUseBlock) | Conditional | Prevents event handler de-opt in some cases |
| `<template v-if>` / `<template v-for>` | Yes (fragment block) | Structural wrappers |

### What does the block tree track?

Each block needs:
- `dynamicChildren: Vec<ElementId>` — flat list of dynamic descendants (non-block children
  that have a non-zero patchFlag)
- `patchFlag` on each child — already computed at prop step
- Block boundaries — where `openBlock()` / `closeBlock()` calls go in codegen

### Implementation approach

**Step**: This should be a **codegen-time concern**, not a syntax-step concern.

The syntax step provides all the raw data:
- Element tree structure (parent_id, nested_level)
- Patch flags per element
- Structural directive info (v-if, v-for, v-slot)

The block tree is then computed during codegen by:
1. Walking the element tree top-down
2. Marking block roots (root element, components, v-if/v-for elements)
3. For each block root, collecting descendant elements with non-zero patch flags
   (stopping at nested block boundaries)
4. Emitting `openBlock()` / `createBlock()` vs `createVNode()` accordingly

**Rationale**: The block tree is a codegen optimization strategy, not a parsing/syntax concern.
Keeping it in codegen means:
- Syntax step stays focused on per-element/per-prop analysis
- Block tree logic can see the full tree structure
- Easier to support different codegen modes (e.g., vapor mode skips blocks entirely)

### Suggested location

`crates/verter_core/src/codegen/` — either as a pre-pass that annotates elements before
the main codegen walk, or integrated into the codegen walk itself.

A pre-pass that produces a `BlockTree` structure (mapping block roots → dynamic children)
would be cleanest, as it separates the optimization logic from the output formatting.

---

## Future: Cross-Component Prop Constness Analysis (Whole-Program Optimization)

**When**: Requires full project analysis — file dependency graph must be built first.

**Prerequisite**: A project-wide file graph tracking component imports, usage sites, and
prop-passing patterns across all `.vue` and `.ts` files.

### Idea

The binding refinement pass (TODO 1) only looks within a single SFC: it knows that
`const id = ''` is a `setup-const`, so `:id="id"` is static. But it cannot reason about
**props received from parent components**, because `defineProps` bindings are always
considered dynamic — any parent could pass a reactive value.

With a project-wide dependency graph, we could trace **all call sites** of a component and
determine if a specific prop is *always* passed a constant value:

```vue
<!-- Parent A -->
<MyComp :title="'Hello'" />

<!-- Parent B -->
<MyComp :title="'World'" />
```

If every usage of `<MyComp>` passes a `setup-const` or literal for `:title`, then inside
`MyComp`, `props.title` could be treated as effectively static for patch flag purposes —
no PROPS flag needed for bindings that only reference it.

### Levels of analysis

| Level | Scope | Const detection |
|-------|-------|-----------------|
| **L0** (DONE) | Per-prop during parsing | None — all bindings assumed dynamic |
| **L1** (TODO 1) | Per-SFC after setup analysis | `setup-const` bindings within same file |
| **L2** (future) | Cross-component, single file | Props from parent in same file (rare) |
| **L3** (future) | Whole-project graph | All call sites across the project |

### Challenges

- **Computational cost**: Requires building and maintaining a full import/usage graph.
  Must be incremental — re-analyzing only changed files and their dependents.
- **Dynamic component usage**: `<component :is="x">` makes call-site tracking harder.
- **Re-exports and barrel files**: `export { default as MyComp } from './MyComp.vue'`
  adds indirection to the graph.
- **Conditional constness**: If 9/10 parents pass a const but one passes a reactive value,
  the prop must remain dynamic. Analysis must be conservative (all-or-nothing).
- **HMR / dev mode**: In development, the graph changes frequently. Could limit L3 analysis
  to production builds only.
- **Slot content**: `<MyComp><template #default="{ item }">` — slot props flow back from
  child to parent, adding another dimension.

### Architecture: Optimizer Plugin

This analysis should be implemented as an **optimizer plugin**, not baked into the core
compilation pipeline. This keeps the core fast and simple while allowing the heavy
cross-file analysis to be opt-in:

- **Core pipeline** (syntax → codegen): Always runs, single-file scope, no project graph needed.
- **Optimizer plugin**: Optionally runs before/after core, has access to the project file graph,
  can annotate elements with refined constness information that codegen consults.

The plugin interface would receive:
- The project dependency graph (component imports, usage sites)
- Per-file binding analysis results
- Per-file syntax events (elements, props, patch flags)

And produce:
- A `PropConstnessMap` mapping `(ComponentId, PropName) → is_always_const`
- Codegen checks this map to skip patch flags for props proven const across all call sites

This keeps the door open for other optimizer plugins (tree-shaking unused slots, dead code
in v-if branches, etc.) using the same plugin architecture.

---

## Vapor Mode Considerations

### What is Vapor mode?

Vue Vapor mode is an alternative compilation strategy that bypasses the virtual DOM entirely.
Instead of generating `createVNode()` / `createBlock()` calls that produce a vnode tree for
diffing, Vapor compiles templates to **imperative DOM operations** with fine-grained reactivity
— similar to Svelte or Solid.

### What Vapor changes

| Concept | VDOM mode (current) | Vapor mode |
|---------|---------------------|------------|
| Element creation | `createVNode("div", props)` | `document.createElement("div")` |
| Prop updates | Diff vnode props using patch flags | Direct `element.setAttribute()` inside `renderEffect()` |
| Text updates | `patchFlag & TEXT` → patch `textContent` | `renderEffect(() => node.textContent = expr)` |
| Children diffing | `dynamicChildren` flat array | No diffing — structural directives compile to imperative control flow |
| Block tree | `openBlock()` / `createBlock()` | Not used — no vnode tree to optimize |
| Patch flags | Bitmask hints for the differ | **Not used** — each binding has its own effect |

### Skip `estimate_patch_flag()` in Vapor mode

Since Vapor doesn't use patch flags, `dynamic_props`, or the block tree, the entire
`estimate_patch_flag()` call can be **skipped** when the template has the `vapor` attribute.
This avoids unnecessary bitmask operations, `Vec` pushes, and byte comparisons for every
prop on every element.

The static vs dynamic distinction that Vapor needs is already encoded in `PropKind` itself:
- `Value`, `ClassValue`, `StyleValue` → static (set once)
- `Bind`, `ClassBind`, `StyleBind`, `On`, `Model`, etc. → dynamic (needs `renderEffect`)

No additional analysis is needed at the prop step for Vapor. Codegen can read `PropKind`
directly.

**What to skip in Vapor mode:**
- `estimate_patch_flag()` call in `handle_attribute_end`
- `patch_flag`, `dynamic_props`, `has_ref`, `has_vnode_hook` fields can be left at defaults
- Children-step flag finalization (TODO 2)
- Block tree pass (TODO 3)

**What still runs in Vapor mode:**
- All syntax event emission (element open/close, props, text, interpolations)
- `PropKind` resolution (static vs dynamic classification)
- Structural directive detection (v-if, v-for, v-slot — Vapor still needs these)
- Binding refinement (TODO 1) — even more valuable, determines `renderEffect` vs one-time set

**Implementation**: Gate the call site with a `is_vapor` flag on the syntax state:
```rust
if !self.is_vapor {
    if let Some(parent) = &mut self.last_event_open_tag {
        estimate_patch_flag(parent, &ev, ctx.bytes);
    }
}
```

### Impact on this optimization plan

| Step | Relevant to Vapor? | Notes |
|------|---------------------|-------|
| Prop step (DONE) | **Skip** | `estimate_patch_flag()` not called. `PropKind` alone encodes static/dynamic |
| Binding refinement (TODO 1) | Yes | Even more valuable — a const binding means no `renderEffect`, just a one-time DOM set |
| Children step (TODO 2) | **Skip** | TEXT, fragment flags, NEED_PATCH are VDOM-only concepts |
| Block tree (TODO 3) | **Skip** | No vnode tree to flatten |
| Cross-component analysis (Future) | Yes | Proving a prop is always const eliminates a `renderEffect`, bigger win than skipping a patch flag |

### Dual-mode codegen

The syntax step (`syntax_kai`) stays **mode-aware only for skipping VDOM work** — it does
not add Vapor-specific logic. The raw events (element tree, props, text, directives) are
the same in both modes; only the VDOM optimization metadata is conditionally computed.

The codegen layer then selects the output strategy:

```
syntax_kai events
       │
       ├──→ VDOM codegen (uses patch flags, block tree, dynamicChildren)
       │
       └──→ Vapor codegen (reads PropKind directly, groups renderEffects)
```

- **VDOM**: Needs patch flags, `dynamic_props`, block tree. Computed during syntax + codegen.
- **Vapor**: Needs static/dynamic per-prop from `PropKind`. No extra syntax-step work needed.

### Vapor-specific optimizations (future)

Vapor has its own optimization opportunities that go beyond patch flags:

- **Effect grouping**: Multiple dynamic props on the same element can share a single
  `renderEffect()` instead of one per prop
- **Template literal optimization**: Static HTML chunks can use `innerHTML` or `template`
  cloning instead of element-by-element creation
- **Signal granularity**: With deep reactivity tracking, only the specific signal that
  changed triggers its effect — no parent/child traversal needed

These would be separate TODO items in a Vapor-specific plan document.

---

## Summary

| Step | When | What |
|------|------|------|
| Prop step (DONE) | During attribute parsing | Patch flags, dynamic_props, has_ref, has_vnode_hook, component CLASS/STYLE |
| Binding refinement (TODO 1) | After script setup analysis | Downgrade/remove flags for props bound to `setup-const` bindings |
| Children step (TODO 2) | At tag close | TEXT, conditional NEED_PATCH, fragment flags, DYNAMIC_SLOTS |
| Block tree (TODO 3) | Codegen pre-pass or codegen walk | Block root detection, dynamicChildren collection, openBlock/createBlock emit |
