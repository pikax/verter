# Migrating from ESLint

This guide covers how to migrate from `eslint-plugin-vue` to Verter's built-in linting. Verter implements the vast majority of `eslint-plugin-vue` rules natively in Rust, running inside the LSP for instant feedback without a separate ESLint process.

## Preset Mapping

If your ESLint config uses one of the standard `eslint-plugin-vue` presets, the mapping is straightforward:

| ESLint `extends` value                 | Verter preset   |
| -------------------------------------- | --------------- |
| `plugin:vue/vue3-essential`            | **Essential**   |
| `plugin:vue/vue3-strongly-recommended` | **Recommended** |
| `plugin:vue/vue3-recommended`          | **Recommended** |

Verter merges the "strongly recommended" and "recommended" tiers into a single **Recommended** preset. The rules from both tiers are included.

## Rule Name Mapping

`eslint-plugin-vue` prefixes all rule names with `vue/`. Verter drops this prefix:

| ESLint rule                             | Verter rule                         |
| --------------------------------------- | ----------------------------------- |
| `vue/no-v-html`                         | `no-v-html`                         |
| `vue/html-self-closing`                 | `html-self-closing`                 |
| `vue/attribute-order`                   | `attribute-order`                   |
| `vue/no-deprecated-destroyed-lifecycle` | `no-deprecated-destroyed-lifecycle` |
| `vue/valid-v-if`                        | `valid-v-if`                        |
| `vue/multi-word-component-names`        | `multi-word-component-names`        |

The pattern is consistent: strip the `vue/` prefix, and the rest of the name is identical.

## Severity Mapping

| ESLint severity | ESLint alias | Verter equivalent          |
| --------------- | ------------ | -------------------------- |
| `0`             | `"off"`      | `"off"` (or omit the rule) |
| `1`             | `"warn"`     | `"warn"`                   |
| `2`             | `"error"`    | `"error"`                  |

Array form is also supported. ESLint's `["error", { ... }]` maps directly to Verter's `["error", { ... }]`.

## Example: Before and After

### Before (`.eslintrc.json`)

```json
{
  "extends": ["plugin:vue/vue3-recommended"],
  "rules": {
    "vue/no-v-html": "error",
    "vue/html-self-closing": [
      "warn",
      {
        "html": { "void": "always" }
      }
    ],
    "vue/multi-word-component-names": "off",
    "vue/max-attributes-per-line": [
      "error",
      {
        "singleline": 3,
        "multiline": 1
      }
    ]
  }
}
```

### After (`.verterrc.json`)

```json
{
  "lint": {
    "enabled": true,
    "preset": "recommended",
    "rules": {
      "no-v-html": "error",
      "html-self-closing": [
        "warn",
        {
          "html": { "void": "always" }
        }
      ],
      "multi-word-component-names": "off"
    }
  }
}
```

Note that `max-attributes-per-line` is not carried over -- it is a formatting rule (see below).

## Rules NOT Implemented

Some `eslint-plugin-vue` rules are intentionally not implemented in Verter. They fall into these categories:

### Formatting Rules (~15 rules)

These rules enforce code style that is better handled by dedicated formatters like Prettier, dprint, or Biome:

- `vue/html-indent`
- `vue/max-attributes-per-line`
- `vue/html-closing-bracket-newline`
- `vue/html-closing-bracket-spacing`
- `vue/multiline-html-element-content-newline`
- `vue/singleline-html-element-content-newline`
- `vue/script-indent`
- `vue/max-len`
- `vue/array-bracket-spacing`
- `vue/comma-dangle`
- `vue/key-spacing`
- `vue/object-curly-spacing`
- `vue/operator-linebreak`
- `vue/quote-props`
- `vue/space-in-parens`

**Rationale**: Formatting rules conflict with formatters. The ecosystem consensus (including ESLint's own recommendation) is to use a formatter for style and a linter for correctness.

### Restriction / Meta Rules (~12 rules)

These are highly project-specific restriction rules that enforce team conventions rather than catching bugs:

- `vue/no-restricted-v-bind`
- `vue/no-restricted-v-on`
- `vue/no-restricted-html-elements`
- `vue/no-restricted-static-attribute`
- `vue/no-restricted-custom-event`
- `vue/no-restricted-component-options`
- `vue/no-restricted-component-names`
- `vue/no-restricted-props`
- `vue/no-restricted-block`
- `vue/no-restricted-call-after-await`
- `vue/no-restricted-class`
- `vue/no-restricted-syntax`

**Rationale**: These are "configurable deny-list" rules. They require extensive configuration to be useful and are rarely enabled in practice. If needed, they can be maintained in ESLint alongside Verter.

Note: `vue/no-bare-strings-in-template` and `vue/no-static-inline-styles` are available in Verter as `no-bare-strings-in-template` and `no-static-inline-styles`.

### Vue 2→3 Migration Rules

Verter implements ~15 deprecation rules (`no-deprecated-filter`, `no-deprecated-functional-template`, `no-deprecated-inline-template`, `no-deprecated-dollar-listeners-api`, `no-deprecated-events-api`, and more). These are included in the **Essential** preset and help catch deprecated Vue 2 patterns.

### ESLint-Specific Rules (1 rule)

- `vue/comment-directive` -- enables `<!-- eslint-disable -->` comment processing

**Rationale**: Verter has its own comment directive system (`<!-- @verter:disable rule-name -->`). The ESLint comment directive format is not needed.

## Running ESLint and Verter Side by Side

You can run both tools during migration. To avoid duplicate warnings, disable the Vue rules in ESLint that Verter now handles:

```json
{
  "extends": ["plugin:vue/vue3-recommended"],
  "rules": {
    "vue/no-v-html": "off",
    "vue/valid-v-if": "off"
  }
}
```

Or, if you are ready to fully migrate, remove `eslint-plugin-vue` from your ESLint config entirely and rely on Verter for all Vue-specific linting:

```json
{
  "extends": []
}
```

Keep ESLint for JavaScript/TypeScript rules (e.g., `no-unused-vars`, `@typescript-eslint/*`) and let Verter handle Vue SFC rules.
