# Lint Rules Reference

Verter ships with approximately **164 built-in lint rules** organized into 11 categories. Rules run natively in Rust inside the LSP, providing instant diagnostics without external tooling.

## Comment Directives

You can disable or adjust rule severity inline using HTML comments in templates:

```html
<!-- @verter:disable no-v-html -->
<div v-html="content"></div>
<!-- @verter:enable no-v-html -->

<!-- @verter:disable-next-line no-v-html -->
<div v-html="content"></div>

<!-- @verter:level no-v-html warning -->
<div v-html="content"></div>
```

## Rule Categories

### VueEssential

Error-preventing rules that catch invalid Vue syntax and common mistakes. These are included in the **Essential** preset and all higher presets.

| Rule Name                       | Default Severity | Auto-fix | Description                                              |
| ------------------------------- | ---------------- | -------- | -------------------------------------------------------- |
| `valid-v-if`                    | error            | --       | Validate `v-if` directive usage                          |
| `valid-v-else-if`               | error            | --       | Validate `v-else-if` directive usage                     |
| `valid-v-else`                  | error            | --       | Validate `v-else` directive usage                        |
| `valid-v-for`                   | error            | --       | Validate `v-for` directive usage                         |
| `valid-v-show`                  | error            | --       | Validate `v-show` directive usage                        |
| `valid-v-on`                    | error            | --       | Validate `v-on` directive usage                          |
| `valid-v-bind`                  | error            | --       | Validate `v-bind` directive usage                        |
| `valid-v-model`                 | error            | --       | Validate `v-model` directive usage                       |
| `valid-v-slot`                  | error            | --       | Validate `v-slot` directive usage                        |
| `valid-v-once`                  | error            | --       | Validate `v-once` directive usage                        |
| `valid-v-pre`                   | error            | --       | Validate `v-pre` directive usage                         |
| `no-reserved-component-names`   | error            | --       | Disallow using reserved HTML elements as component names |
| `require-component-is`          | error            | --       | Require `is` attribute on `<component>` elements         |
| `no-v-text-v-html-on-component` | error            | --       | Disallow `v-text`/`v-html` on component elements         |
| `no-useless-template-attrs`     | error            | --       | Disallow useless attribute on `<template>`               |
| `no-child-content`              | error            | --       | Disallow child content when using `v-html`/`v-text`      |

...and more `valid-v-*`, `require-*`, `no-dupe-*`, `no-unused-*` rules.

### VueRecommended

Code quality and consistency rules. Included in the **Recommended** preset.

| Rule Name                          | Default Severity | Auto-fix | Description                                                               |
| ---------------------------------- | ---------------- | -------- | ------------------------------------------------------------------------- |
| `attribute-order`                  | warn             | yes      | Enforce consistent attribute ordering                                     |
| `html-self-closing`                | warn             | yes      | Enforce self-closing style on elements                                    |
| `v-bind-style`                     | warn             | yes      | Enforce `v-bind` shorthand style (`:prop` vs `v-bind:prop`)               |
| `v-on-style`                       | warn             | yes      | Enforce `v-on` shorthand style (`@event` vs `v-on:event`)                 |
| `v-slot-style`                     | warn             | yes      | Enforce `v-slot` shorthand style (`#name` vs `v-slot:name`)               |
| `multi-word-component-names`       | warn             | --       | Require multi-word component names                                        |
| `component-definition-name-casing` | warn             | yes      | Enforce casing of component definition names                              |
| `prefer-true-attribute-shorthand`  | warn             | yes      | Prefer shorthand for boolean attributes (`disabled` vs `disabled="true"`) |

...and more `component-*`, `html-*`, `order-in-*` rules.

### Vue Deprecation

Rules for Vue 2 to Vue 3 migration, catching deprecated patterns.

| Rule Name                              | Default Severity | Auto-fix | Description                                                     |
| -------------------------------------- | ---------------- | -------- | --------------------------------------------------------------- |
| `no-deprecated-destroyed-lifecycle`    | error            | yes      | Disallow deprecated `destroyed`/`beforeDestroy` lifecycle hooks |
| `no-deprecated-v-on-native-modifier`   | error            | yes      | Disallow deprecated `.native` modifier on `v-on`                |
| `no-deprecated-scope-attribute`        | error            | yes      | Disallow deprecated `scope` attribute                           |
| `no-deprecated-slot-attribute`         | error            | yes      | Disallow deprecated `slot` attribute                            |
| `no-deprecated-v-bind-sync`            | error            | yes      | Disallow deprecated `v-bind` `.sync` modifier                   |
| `no-deprecated-v-on-number-modifiers`  | error            | --       | Disallow deprecated number modifiers on `v-on`                  |
| `no-deprecated-dollar-listeners-api`   | error            | --       | Disallow deprecated `$listeners` usage                          |
| `no-deprecated-dollar-scopedslots-api` | error            | --       | Disallow deprecated `$scopedSlots` usage                        |

...and more `no-deprecated-*` rules (~12 total).

### Script

Rules for the `<script>` and `<script setup>` blocks.

| Rule Name                     | Default Severity | Auto-fix | Description                                                        |
| ----------------------------- | ---------------- | -------- | ------------------------------------------------------------------ |
| `define-macros-order`         | warn             | yes      | Enforce order of `defineProps`/`defineEmits`/`defineSlots`         |
| `no-export-in-script-setup`   | error            | --       | Disallow `export` in `<script setup>`                              |
| `prefer-use-template-ref`     | warn             | --       | Prefer `useTemplateRef()` over `ref()` for template refs           |
| `require-symbol-provide`      | warn             | --       | Require Symbol keys for `provide()`                                |
| `no-async-in-computed`        | error            | --       | Disallow async functions in `computed()`                           |
| `no-watch-after-await`        | error            | --       | Disallow `watch()` after `await` in setup                          |
| `require-default-prop`        | warn             | --       | Require default value for optional props                           |
| `no-unused-emit-declarations` | warn             | --       | Disallow unused emit declarations                                  |
| `no-ref-as-operand`           | error            | yes      | Disallow using ref values directly in expressions without `.value` |

...and more `define-*`, `no-*`, `prefer-*`, `require-*` rules (~20 total).

### CSS

Rules for `<style>` blocks.

| Rule Name                | Default Severity | Auto-fix | Description                                                      |
| ------------------------ | ---------------- | -------- | ---------------------------------------------------------------- |
| `unused-css-selector`    | warn             | --       | Detect CSS selectors that don't match any template element       |
| `no-undefined-css-class` | warn             | --       | Detect class names used in template but not defined in `<style>` |

### HTML

Rules for HTML elements and attributes.

| Rule Name                  | Default Severity | Auto-fix | Description                                                   |
| -------------------------- | ---------------- | -------- | ------------------------------------------------------------- |
| `no-void-element-content`  | error            | --       | Disallow content inside void elements (`<br>`, `<img>`, etc.) |
| `no-deprecated-element`    | warn             | --       | Disallow deprecated HTML elements                             |
| `html-button-has-type`     | warn             | --       | Require `type` attribute on `<button>` elements               |
| `no-template-target-blank` | warn             | --       | Disallow `target="_blank"` without `rel="noopener"`           |

### A11y (Accessibility)

Accessibility rules based on WAI-ARIA best practices.

| Rule Name                      | Default Severity | Auto-fix | Description                                  |
| ------------------------------ | ---------------- | -------- | -------------------------------------------- |
| `aria-props`                   | warn             | --       | Validate `aria-*` attribute names            |
| `no-aria-hidden-on-focusable`  | warn             | --       | Disallow `aria-hidden` on focusable elements |
| `media-has-caption`            | warn             | --       | Require captions on `<audio>` and `<video>`  |
| `role-has-required-aria-props` | warn             | --       | Require ARIA attributes for roles            |
| `interactive-supports-focus`   | warn             | --       | Require focusability on interactive elements |

...and more accessibility rules (~10 total).

### Vapor

Rules specific to Vue Vapor mode.

| Rule Name                 | Default Severity | Auto-fix | Description                                                  |
| ------------------------- | ---------------- | -------- | ------------------------------------------------------------ |
| `no-suspense`             | warn             | --       | Disallow `<Suspense>` (not supported in Vapor)               |
| `no-vue-lifecycle-events` | warn             | --       | Disallow Vue 3 lifecycle event hooks (use Vapor equivalents) |
| `no-inline-template`      | warn             | --       | Disallow `inline-template` attribute                         |
| `no-non-vapor-components` | warn             | --       | Disallow components incompatible with Vapor mode             |

### Security

Security-focused rules.

| Rule Name       | Default Severity | Auto-fix | Description                                               |
| --------------- | ---------------- | -------- | --------------------------------------------------------- |
| `no-v-html`     | warn             | --       | Disallow `v-html` to prevent XSS                          |
| `no-unsafe-url` | warn             | --       | Disallow potentially unsafe URLs (`javascript:`, `data:`) |

### Performance

Rules that detect patterns causing unnecessary re-renders or poor runtime performance.

| Rule Name               | Default Severity | Auto-fix | Description                                      |
| ----------------------- | ---------------- | -------- | ------------------------------------------------ |
| `no-constant-condition` | warn             | --       | Disallow constant expressions in `v-if`/`v-show` |
| `no-v-for-index-as-key` | warn             | --       | Disallow using `v-for` index as `:key`           |

...and more performance-focused rules.

### Reactivity

Rules that catch common reactivity mistakes in `<script setup>`.

| Rule Name                        | Default Severity | Auto-fix | Description                                                         |
| -------------------------------- | ---------------- | -------- | ------------------------------------------------------------------- |
| `no-ref-as-operand`              | error            | yes      | Disallow using ref values directly in expressions without `.value`  |
| `no-setup-props-reactivity-loss` | warn             | --       | Disallow destructuring `props` in setup (loses reactivity tracking) |

### CrossFile

Rules that analyze patterns across multiple files. These require the host to have compiled related files.

| Rule Name                   | Default Severity | Auto-fix | Description                                                                     |
| --------------------------- | ---------------- | -------- | ------------------------------------------------------------------------------- |
| `provide-inject-validation` | warn             | --       | Validate that `provide()` and `inject()` calls have matching types across files |
| `deep-composable-tracking`  | warn             | --       | Track deep composable usage patterns for potential issues                       |
| `no-duplicate-vue`          | warn             | --       | Detect duplicate `.vue` file names that may cause import conflicts              |

## Preset Contents

Each preset includes a different subset of the above rules:

| Preset          | Categories Included                                                      |
| --------------- | ------------------------------------------------------------------------ |
| **Essential**   | VueEssential                                                             |
| **Recommended** | VueEssential + VueRecommended                                            |
| **All**         | All categories, all rules                                                |
| **Performance** | Performance rules only                                                   |
| **A11y**        | A11y rules only                                                          |
| **Strict**      | VueEssential + VueRecommended + Vue Deprecation + Script (strict subset) |

## Adding Rule Overrides

Individual rules can be adjusted regardless of the chosen preset. See [Configuration](./configuration.md) for the full `.verterrc.json` schema and [Migrating from ESLint](./migrating-from-eslint.md) for mapping `eslint-plugin-vue` rules to their Verter equivalents.
