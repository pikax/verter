# Diagnostics & Linting

::: warning Experimental
Verter is experimental software at v0.0.1-alpha.3. APIs may change without notice.
:::

Verter includes a built-in diagnostic engine (`verter_diagnostics`) that provides 32 lint rules across 8 categories. These rules run against static analysis data from the compiler -- they do not require the full TypeScript type checker, so diagnostics are fast and available immediately as you type.

The diagnostic engine is separate from the template compiler. It depends on `verter_analysis` for import/export/binding data and template element information, but does not depend on `verter_core` or any codegen modules.

## Rule Categories

### Accessibility (10 rules)

Rules that help ensure your Vue templates produce accessible HTML output.

| Rule | Description |
|------|-------------|
| `alt-text` | Require alt text on `<img>`, `<area>`, `<input type="image">`, and `<object>` elements |
| `anchor-has-content` | Anchors must have visible content (text, aria-label, or slotted content) |
| `aria-role` | Only valid ARIA roles are allowed |
| `click-events-have-key-events` | Elements with `@click` handlers must also have a keyboard event handler |
| `form-control-has-label` | Form controls (`<input>`, `<select>`, `<textarea>`) need associated labels |
| `heading-has-content` | Heading elements (`<h1>` through `<h6>`) must have content |
| `iframe-has-title` | `<iframe>` elements need a `title` attribute for screen readers |
| `no-autofocus` | Avoid the `autofocus` attribute (can disorient screen reader users) |
| `no-distracting-elements` | No `<marquee>` or `<blink>` elements |
| `tabindex-no-positive` | No positive `tabindex` values (disrupts natural tab order) |

### Vue (9 rules)

Rules that enforce Vue template best practices and catch common mistakes.

| Rule | Description |
|------|-------------|
| `require-v-for-key` | Elements using `v-for` must have a `:key` binding |
| `valid-v-for` | Validate `v-for` expression syntax (must use `in` or `of`) |
| `no-duplicate-attributes` | No duplicate attributes or directives on the same element |
| `no-template-key` | `<template>` elements should not have a `key` attribute |
| `no-textarea-mustache` | No mustache interpolation inside `<textarea>` (use `v-model` instead) |
| `no-dupe-v-else-if` | No duplicate conditions in `v-if`/`v-else-if` chains |
| `no-use-v-if-with-v-for` | Do not use `v-if` and `v-for` on the same element |
| `no-unused-components` | Detect imported components that are never used in the template |
| `no-unused-props` | Detect declared props that are never referenced |

### CSS (3 rules)

Rules that analyze the relationship between `<style>` blocks and the template.

| Rule | Description |
|------|-------------|
| `unused-css-selector` | Detect CSS selectors that do not match any template element |
| `scoped-css-cascade` | Warn about cascade issues in scoped CSS (selectors that may not apply as expected) |
| `undefined-css-class` | Detect classes used in the template but not defined in any `<style>` block |

### Performance (2 rules)

Rules that flag patterns that may impact rendering performance.

| Rule | Description |
|------|-------------|
| `max-template-depth` | Warn when template nesting exceeds a configurable threshold |
| `prefer-static-class` | Prefer static `class` over dynamic `:class` when the value is a constant string |

### Security (1 rule)

| Rule | Description |
|------|-------------|
| `no-v-html` | Warn about XSS risk from `v-html` (renders raw HTML from potentially untrusted data) |

### Reactivity (2 rules)

Rules that catch common reactivity mistakes in `<script setup>`.

| Rule | Description |
|------|-------------|
| `no-ref-as-operand` | Do not use a ref directly as an operand (use `.value` instead) |
| `no-setup-props-reactivity-loss` | Avoid destructuring `props` in setup (loses reactivity tracking) |

### Script (2 rules)

Rules for lifecycle hook usage patterns.

| Rule | Description |
|------|-------------|
| `no-inline-lifecycle` | Lifecycle hooks should not be defined inline (extract to named functions) |
| `no-lifecycle-after-await` | No lifecycle hooks after `await` (the component instance may no longer be active) |

### Cross-File (3 rules)

Rules that analyze patterns across multiple files. These require the host to have compiled related files.

| Rule | Description |
|------|-------------|
| `provide-inject-validation` | Validate that `provide()` and `inject()` calls have matching types across files |
| `deep-composable-tracking` | Track deep composable usage patterns for potential issues |
| `no-duplicate-vue` | Detect duplicate `.vue` file names that may cause import conflicts |

## Comment Directives

You can suppress diagnostics for specific lines or blocks using comment directives.

### Suppress All Rules

Use `verter:ignore` to suppress all diagnostic rules for the next element:

```vue
<template>
  <!-- verter:ignore -->
  <div v-html="content" />
</template>
```

### Suppress a Specific Rule

Use `verter/<rule-name>:ignore` to suppress a single rule:

```vue
<template>
  <!-- verter/no-v-html:ignore -->
  <div v-html="trustedContent" />

  <!-- verter/alt-text:ignore -->
  <img :src="decorativeImage" />
</template>
```

The directive applies to the immediately following element. It does not affect sibling or parent elements.

## Next Steps

- [Features](./features) -- Type safety features overview
- [Architecture](./architecture) -- How the diagnostic engine fits into the system
- [Getting Started](./getting-started) -- Install and configure Verter
