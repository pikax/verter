# API Stability

This document defines the stability guarantees for Verter's public APIs during the beta period and beyond.

## Versioning Policy

Verter follows [semver](https://semver.org/) with pre-release qualifiers:

- **alpha.x** — no stability guarantees; APIs may change without notice
- **beta.x** — APIs listed as **Stable** below will not break without a changelog entry and at least one beta release of deprecation notice. **Experimental** APIs may still change freely.
- **rc.x / stable** — all Stable APIs follow strict semver; breaking changes require a major version bump

## Stable APIs

These APIs are considered stable as of beta.1. Breaking changes will be documented in the changelog with migration guidance.

### `@verter/unplugin` Options

The bundler plugin options passed to `verter()`:

| Option                | Type                                       | Status |
| --------------------- | ------------------------------------------ | ------ |
| `include` / `exclude` | `string \| RegExp \| (string \| RegExp)[]` | Stable |
| `compiler`            | resolved `vue/compiler-sfc` module         | Stable |
| `root`                | `string`                                   | Stable |
| `compileProfile`      | `{ filename, target, mode }`               | Stable |

### `.verterrc.json` Schema

The project-level configuration file format:

| Field          | Status                                                    |
| -------------- | --------------------------------------------------------- |
| `lint.enabled` | Stable                                                    |
| `lint.preset`  | Stable (`"essential"`, `"recommended"`, `"all"`, `"off"`) |
| `lint.rules`   | Stable (rule names and severity values)                   |

### VS Code Settings

| Setting                          | Status                                             |
| -------------------------------- | -------------------------------------------------- |
| `verter.lint.enabled`            | Stable                                             |
| `verter.lint.preset`             | Stable                                             |
| `verter.lint.rules`              | Stable                                             |
| `verter.typeProvider`            | Stable (`"auto"`, `"tsgo"`, `"tsserver"`, `"off"`) |
| `verter.viteConfig.enabled`      | Stable                                             |
| `verter.viteConfig.trustedFiles` | Stable                                             |

### Diagnostic Rule Names

All ~164 lint rule names (e.g., `no-v-html`, `unused-css-selector`) are stable identifiers. Rules may be added in minor releases but existing rule names will not be renamed or removed without a deprecation period.

### Comment Directives

The inline suppression syntax is stable:

- `<!-- @verter:disable [rule] -->`
- `<!-- @verter:enable [rule] -->`
- `<!-- @verter:disable-next-line [rule] -->`
- `<!-- @verter:ignore-start -->` / `<!-- @verter:ignore-end -->`
- `<!-- @verter:level rule severity -->`

### LSP Custom Methods

| Method                             | Status                |
| ---------------------------------- | --------------------- |
| `$/verter/getVirtualFiles`         | Stable                |
| `$/verter/getAnalysis`             | Stable                |
| `$/verter/applyStyleOverrides`     | Stable                |
| `$/verter/getRouteTree`            | Stable                |
| `$/verter/heartbeat`               | Stable                |
| `$/verter/tsgoLimitation`          | Stable (notification) |
| `$/verter/viteConfigTrustRequired` | Stable (notification) |

### Native Host API (`@verter/native`)

| Method                               | Status |
| ------------------------------------ | ------ |
| `new VerterHost(config)`             | Stable |
| `host.upsert(request)`               | Stable |
| `host.resolve(rawId)`                | Stable |
| `host.getVirtualFile(query)`         | Stable |
| `host.listVirtualFiles(canonicalId)` | Stable |
| `host.remove(canonicalOrAlias)`      | Stable |
| `host.applyBlockOverrides(request)`  | Stable |

## Experimental APIs

These features are behind opt-in flags and may change or be removed in any release without prior notice.

| Feature                    | Setting                                        | Description                                                  |
| -------------------------- | ---------------------------------------------- | ------------------------------------------------------------ |
| Strict Slots               | `verter.experimental.strictSlots`              | Type-checks slot children against `defineSlots()` signatures |
| Conditional Root Narrowing | `verter.experimental.conditionalRootNarrowing` | Narrows component root element type based on prop values     |
| Expose Bindings Testing    | `verter.experimental.exposeBindingsTesting`    | Exposes binding type information for testing                 |

### `@verter/component-meta/compat` (Volar Drop-in)

| API                                           | Status                                 |
| --------------------------------------------- | -------------------------------------- |
| `createChecker(tsconfig, options?)`           | Stable                                 |
| `createCheckerByJson(root, config, options?)` | Stable                                 |
| `checker.getComponentMeta(filePath)`          | Stable                                 |
| `checker.getExportNames(filePath)`            | Stable                                 |
| `checker.updateFile(path, content)`           | Stable                                 |
| `checker.deleteFile(path)`                    | Stable                                 |
| `checker.reload()` / `clearCache()`           | Stable                                 |
| `PropertyMeta` shape                          | Stable (matches Volar)                 |
| `PropertyMetaSchema` shape                    | Stable (matches Volar)                 |
| `MetaCheckerOptions`                          | Stable                                 |
| `VolarComponentMeta._verter`                  | Experimental (opt-in Verter extension) |

## Internal / Unstable

The following are explicitly **not** part of the public API and may change at any time:

- Generated TSX/JSX output format (IDE codegen path)
- Generated VDOM/Vapor render function output format
- Internal host data structures (`FileAnalysisSnapshot`, `CompileSlot`, etc.)
- `verter_analysis` crate types (used by diagnostics/actions, not published)
- MCP server tool schemas (still evolving)
- WASM binding API (`@verter/wasm`) — mirrors native but not independently versioned
- `@verter/component-meta` type IR — schema still stabilizing

## Deprecation Process

1. The API is marked `@deprecated` in source and documentation
2. A warning is logged/shown when the deprecated API is used
3. The deprecated API is removed in the next beta or minor release
4. The changelog includes migration instructions
