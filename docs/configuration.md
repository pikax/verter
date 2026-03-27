# Configuration Reference

Verter's linting engine can be configured through project-level config files, VS Code settings, or ESLint migration. This page covers all configuration options, their priority, and how they interact.

## Configuration Priority

When multiple configuration sources exist, they are resolved in this order (highest priority first):

1. **`.verterrc.json`** -- project-level config file (checked into version control)
2. **VS Code settings** -- user or workspace `settings.json`
3. **ESLint config** -- auto-detected from `.eslintrc.*` / `eslint.config.*`

A higher-priority source overrides lower-priority sources. For example, a rule set to `"off"` in `.verterrc.json` will be off even if the VS Code setting enables it.

## Default Behavior

Without any explicit configuration, Verter shows only **error-severity** diagnostics from the **Essential** preset. This means you get protection against common Vue errors out of the box, without any setup required.

## `.verterrc.json`

Place a `.verterrc.json` file in your project root (next to `package.json`). The LSP and MCP servers automatically detect it.

### Schema

```json
{
  "lint": {
    "enabled": true,
    "preset": "recommended",
    "rules": {
      "no-v-html": "error",
      "unused-css-selector": "off",
      "no-deprecated-destroyed-lifecycle": ["error", {}]
    }
  }
}
```

### Fields

| Field          | Type      | Default       | Description                                         |
| -------------- | --------- | ------------- | --------------------------------------------------- |
| `lint.enabled` | `boolean` | `true`        | Enable or disable linting entirely                  |
| `lint.preset`  | `string`  | `"essential"` | Base preset to apply (see [Presets](#presets))      |
| `lint.rules`   | `object`  | `{}`          | Per-rule overrides that extend or modify the preset |

### Rule Severity Values

Each rule can be set to one of these severity values:

| Value     | Alias | Meaning                                                  |
| --------- | ----- | -------------------------------------------------------- |
| `"off"`   | `0`   | Disable the rule                                         |
| `"warn"`  | `1`   | Report as a warning (yellow squiggle, does not block CI) |
| `"error"` | `2`   | Report as an error (red squiggle, blocks CI if checked)  |

Rules can also use the **array form** to pass options:

```json
{
  "lint": {
    "rules": {
      "html-self-closing": ["warn", { "html": { "void": "always" } }],
      "attribute-order": ["error", {}]
    }
  }
}
```

The first element is the severity, and the second is a rule-specific options object.

## VS Code Settings

These settings are available in VS Code's `settings.json` (user or workspace level):

| Setting               | Type      | Default       | Description                                                                                   |
| --------------------- | --------- | ------------- | --------------------------------------------------------------------------------------------- |
| `verter.lint.enabled` | `boolean` | `false`       | Enable Verter's built-in linting                                                              |
| `verter.lint.preset`  | `enum`    | `"essential"` | Preset to use: `"essential"`, `"recommended"`, `"all"`, `"performance"`, `"a11y"`, `"strict"` |

VS Code settings are overridden by `.verterrc.json` if both exist. This lets teams enforce project-level config while individual developers can use VS Code settings for personal projects.

## Presets

Presets are curated collections of rules grouped by purpose:

| Preset          | Description                                            | Includes                                                                                                  |
| --------------- | ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| **Essential**   | Prevents common errors and invalid syntax              | `valid-v-*`, `no-reserved-*`, `require-component-is`, and other error-preventing rules                    |
| **Recommended** | Default preset; Essential + strongly recommended rules | Essential + `attribute-order`, `html-self-closing`, `v-bind-style`, `component-*`, and more               |
| **All**         | Every available rule at its default severity           | All ~163 rules                                                                                            |
| **Performance** | Performance-focused rules                              | Rules that detect patterns causing unnecessary re-renders or expensive operations                         |
| **A11y**        | Accessibility rules                                    | `aria-props`, `role-has-required-aria-props`, `media-has-caption`, `interactive-supports-focus`, and more |
| **Strict**      | Recommended + all deprecation + all style rules        | Recommended + `no-deprecated-*`, strict style enforcement                                                 |

### Preset + Rule Overrides

The `rules` object in `.verterrc.json` extends the chosen preset. This lets you start from a preset and tweak individual rules:

```json
{
  "lint": {
    "preset": "recommended",
    "rules": {
      "no-v-html": "off",
      "unused-css-selector": "error"
    }
  }
}
```

This applies the Recommended preset, then disables `no-v-html` and promotes `unused-css-selector` to error severity.

## ESLint Migration

If your project uses `eslint-plugin-vue`, Verter can auto-detect and map your ESLint configuration. See [Migrating from ESLint](./migrating-from-eslint.md) for the full mapping guide.

The key points:

- `extends: ["plugin:vue/vue3-essential"]` maps to the **Essential** preset
- `extends: ["plugin:vue/vue3-recommended"]` maps to the **Recommended** preset
- Individual `vue/*` rule overrides are mapped by dropping the `vue/` prefix
- ESLint severity `0`/`"off"` maps to `"off"`, `1`/`"warn"` maps to `"warn"`, `2`/`"error"` maps to `"error"`

ESLint config is the lowest priority source -- `.verterrc.json` and VS Code settings both override it.
