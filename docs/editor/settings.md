# Settings Reference

::: warning Experimental
Verter is experimental software at v0.0.1-alpha.3. APIs may change without notice.
:::

All VS Code extension settings for Verter, configurable via `settings.json` or the Settings UI.

## General

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `verter.enable` | `boolean` | `true` | Enable or disable the Verter extension entirely |
| `verter.lspBinaryPath` | `string` | `""` | Absolute path to the `verter-lsp` binary. Leave empty to auto-detect (bundled binary or `PATH`). |

## Statistics

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `verter.statistics.enabled` | `boolean` | `false` | Collect processing statistics (diagnostics, file reads) |
| `verter.statistics.persistToFile` | `boolean` | `false` | Persist statistics to disk for cumulative analysis |
| `verter.statistics.filePath` | `string` | `""` | Absolute path for persisted statistics. Defaults to `<workspace>/.verter/statistics.json` when persistence is enabled. |
| `verter.statistics.maxSessionEntries` | `number` | `500` | Maximum number of in-memory statistics events to retain per session |
| `verter.statistics.maxPersistedEntries` | `number` | `2000` | Maximum number of events persisted to disk |

## Analysis

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `verter.analysis.enabled` | `boolean` | `false` | Enable the Verter Analysis sidebar (virtual files, component tree, analysis, source map sync) |

## Decorations

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `verter.decorations.vueApiCalls` | `boolean` | `true` | Show inline annotations for Vue API calls (lifecycle hooks, watchers, reactivity, provide/inject) |
| `verter.decorations.bindingColors` | `boolean` | `true` | Show faint color decorations on bindings based on their reactivity type (ref, computed, reactive, prop, etc.) |
| `verter.decorations.bindingColorsScope` | `"template"` \| `"all"` | `"template"` | Where to apply binding color decorations. `"template"` colors bindings only in `<template>` (where they are consumed far from declarations). `"all"` colors bindings in both `<template>` and `<script>`. |
| `verter.decorations.bindingColorsStyle` | `"background"` \| `"underline"` | `"background"` | Visual style for binding color decorations. `"background"` applies a faint background tint behind binding text. `"underline"` applies a subtle colored dotted underline beneath binding text. |

### Binding Color Categories

Binding colors are determined by the reactivity type of each binding:

| Category | Description | Default Color (Dark) |
|----------|-------------|---------------------|
| `verter.binding.ref` | `ref()` bindings (needs `.value`) | Blue `#4285f4` |
| `verter.binding.computed` | `computed()` bindings | Purple `#a142f4` |
| `verter.binding.reactive` | `reactive()` bindings | Teal `#00bcd4` |
| `verter.binding.prop` | `defineProps` bindings | Orange `#ff9800` |
| `verter.binding.composable` | Composable return values (MaybeRef) | Pink `#e91e63` |
| `verter.binding.mutable` | Mutable (`let`) bindings | Amber `#ffc107` |
| `verter.binding.function` | Function bindings | Green `#4caf50` |

These colors are theme-aware and have separate defaults for dark, light, high contrast, and high contrast light themes. You can override them in your `workbench.colorCustomizations` settings:

```json
{
  "workbench.colorCustomizations": {
    "verter.binding.ref": "#5599ff20",
    "verter.binding.computed": "#bb66ff20"
  }
}
```

## Server

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `verter.trace.server` | `"off"` \| `"messages"` \| `"verbose"` | `"off"` | Traces the communication between VS Code and the Verter language server. Useful for debugging LSP issues. |
| `verter.server.logLevel` | `"error"` \| `"warn"` \| `"info"` \| `"debug"` \| `"trace"` | `"info"` | Log level for the Verter language server process. Changing this setting restarts the server. |

## Emmet Configuration

The extension automatically configures Emmet to include Vue in its language list:

```json
{
  "emmet.includeLanguages": {
    "vue": "html"
  }
}
```

This enables Emmet abbreviation expansion inside Vue `<template>` blocks.

## Example Configuration

A recommended configuration for development:

```json
{
  "verter.enable": true,
  "verter.decorations.vueApiCalls": true,
  "verter.decorations.bindingColors": true,
  "verter.decorations.bindingColorsScope": "template",
  "verter.decorations.bindingColorsStyle": "background",
  "verter.server.logLevel": "info"
}
```

For debugging or extension development:

```json
{
  "verter.enable": true,
  "verter.analysis.enabled": true,
  "verter.statistics.enabled": true,
  "verter.trace.server": "verbose",
  "verter.server.logLevel": "debug"
}
```
