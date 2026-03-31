# Diagnostics & Linting

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter includes a built-in diagnostic engine (`verter_diagnostics`) that provides **~164 lint rules** across **11 categories**. These rules run natively in Rust inside the LSP, providing instant diagnostics without external tooling. They analyze static analysis data from the compiler and do not require the full TypeScript type checker, so diagnostics are fast and available immediately as you type.

The diagnostic engine is separate from the template compiler. It depends on `verter_analysis` for import/export/binding data and template element information, but does not depend on `verter_compiler` or any codegen modules.

## Configuration

By default, only **error-severity** diagnostics from the **Essential** preset are shown. To see warnings and hints, either:

- Create a [`.verterrc.json`](/configuration) in your project root, or
- Enable `verter.lint.enabled` in VS Code settings

See the [Configuration Reference](/configuration) for presets, per-rule overrides, and ESLint migration.

## Rule Categories

Verter organizes rules into 11 categories. For the complete list of all ~164 rules with severity and auto-fix information, see the [Lint Rules Reference](/lint-rules).

### Vue Essential (~95 rules)

Error-preventing rules that catch invalid Vue syntax, common mistakes, and deprecated patterns. This is the largest category, covering directive validation (`valid-v-if`, `valid-v-for`, `valid-v-model`, etc.), component best practices, template structure, and Vue 2→3 migration rules (`no-deprecated-destroyed-lifecycle`, `no-deprecated-v-bind-sync`, etc.).

### Vue Recommended

Code quality and consistency rules for readability and style. Includes `attribute-order`, `html-self-closing`, `v-bind-style`, `v-on-style`, `v-slot-style`, `multi-word-component-names`, `component-name-in-template-casing`, and more.

### Script (~36 rules)

Rules for `<script>` and `<script setup>` blocks, covering define macros (`define-macros-order`, `valid-define-props`, `valid-define-emits`), lifecycle hooks, watchers, computed properties, prop defaults, emit declarations, component API style, and more.

### Accessibility (~15 rules)

WCAG-based accessibility rules: `alt-text`, `anchor-has-content`, `aria-props`, `aria-role`, `click-events-have-key-events`, `form-control-has-label`, `heading-has-content`, `iframe-has-title`, `interactive-supports-focus`, `media-has-caption`, `no-aria-hidden-on-focusable`, `no-autofocus`, `no-distracting-elements`, `role-has-required-aria-props`, `tabindex-no-positive`.

### CSS (3 rules)

Rules that analyze the relationship between `<style>` blocks and the template: `unused-css-selector`, `scoped-css-cascade`, `undefined-css-class`.

### Reactivity (2 rules)

Rules that catch common reactivity mistakes in `<script setup>`: `no-ref-as-operand`, `no-setup-props-reactivity-loss`.

### Performance (2 rules)

Rules that flag patterns impacting rendering performance: `max-template-depth` (configurable), `prefer-static-class`.

### Security (2 rules)

`no-v-html` (XSS risk) and `no-unsafe-url` (dangerous `javascript:` / `data:` URLs).

### HTML Conformance (2 rules)

HTML spec conformance: `no-void-element-content`, `no-deprecated-element`.

### Vapor (4 rules)

Vapor mode compatibility: `no-suspense`, `no-vue-lifecycle-events`, `no-inline-template`, `no-non-vapor-components`.

### Cross-File (3 rules)

Rules that analyze patterns across multiple files (require the host to have compiled related files): `provide-inject-validation`, `deep-composable-tracking`, `no-duplicate-vue`.

## Comment Directives

You can suppress or adjust diagnostics inline using HTML comments with the `@verter:` prefix.

### Disable a Rule

```vue
<template>
  <!-- @verter:disable no-v-html -->
  <div v-html="content" />
  <!-- @verter:enable no-v-html -->
</template>
```

`@verter:disable` without a rule name disables all rules from that point to EOF (or until `@verter:enable`).

### Disable for the Next Line

```vue
<template>
  <!-- @verter:disable-next-line no-v-html -->
  <div v-html="trustedContent" />

  <!-- @verter:disable-next-line -->
  <img :src="decorativeImage" />
</template>
```

### Ignore Regions

```vue
<template>
  <!-- @verter:ignore-start -->
  <div v-html="content" />
  <img :src="image" />
  <!-- @verter:ignore-end -->
</template>
```

### Override Severity

```vue
<template>
  <!-- @verter:level no-v-html warning -->
  <div v-html="content" />
</template>
```

## Next Steps

- [Lint Rules Reference](/lint-rules) -- Full list of all ~164 rules
- [Configuration Reference](/configuration) -- Presets, per-rule overrides, `.verterrc.json`
- [Migrating from ESLint](/migrating-from-eslint) -- Map `eslint-plugin-vue` rules to Verter
- [Features](./features) -- Type safety features overview
- [Architecture](./architecture) -- How the diagnostic engine fits into the system
