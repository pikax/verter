# Other Bundlers

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter supports all major JavaScript bundlers through `@verter/unplugin`. This page covers Rollup, esbuild, rspack, Rolldown, and Farm. For Vite and webpack, see their dedicated guides:

- [Vite Integration](./vite)
- [webpack Integration](./webpack)

All bundler plugins accept the same [configuration options](./vite#options).

## Installation

Install `@verter/unplugin` in your project:

::: code-group

```sh [pnpm]
pnpm add -D @verter/unplugin
```

```sh [npm]
npm install -D @verter/unplugin
```

```sh [yarn]
yarn add -D @verter/unplugin
```

:::

## Rollup

```js
// rollup.config.js
import VerterRollup from "@verter/unplugin/rollup";

export default {
  input: "src/main.js",
  plugins: [VerterRollup()],
  output: {
    dir: "dist",
    format: "es",
  },
};
```

You will likely also need `@rollup/plugin-node-resolve` and `@rollup/plugin-replace` for a working Vue build:

```js
import { nodeResolve } from "@rollup/plugin-node-resolve";
import replace from "@rollup/plugin-replace";
import VerterRollup from "@verter/unplugin/rollup";

export default {
  input: "src/main.js",
  plugins: [
    replace({
      "process.env.NODE_ENV": JSON.stringify("production"),
      preventAssignment: true,
    }),
    nodeResolve(),
    VerterRollup(),
  ],
  output: {
    dir: "dist",
    format: "es",
  },
};
```

## esbuild

```js
// esbuild.config.js
import { build } from "esbuild";
import VerterEsbuild from "@verter/unplugin/esbuild";

await build({
  entryPoints: ["src/main.js"],
  bundle: true,
  outdir: "dist",
  plugins: [VerterEsbuild()],
});
```

## rspack

```js
// rspack.config.js
const VerterRspack = require("@verter/unplugin/rspack").default;

module.exports = {
  entry: "./src/main.js",
  plugins: [VerterRspack()],
};
```

rspack is API-compatible with webpack, so style loaders work the same way. See the [webpack guide](./webpack#style-loaders) for CSS loader configuration.

## Rolldown

```js
// rolldown.config.js
import VerterRolldown from "@verter/unplugin/rolldown";

export default {
  input: "src/main.js",
  plugins: [VerterRolldown()],
  output: {
    dir: "dist",
    format: "es",
  },
};
```

## Farm

```js
// farm.config.js
import { defineConfig } from "@farmfe/core";
import VerterFarm from "@verter/unplugin/farm";

export default defineConfig({
  plugins: [VerterFarm()],
});
```

## Options

All bundler plugins accept the same options. See the [Vite configuration reference](./vite#options) for the full list:

| Option              | Type                                       | Default      | Description                                                    |
| ------------------- | ------------------------------------------ | ------------ | -------------------------------------------------------------- |
| `include`           | `string \| RegExp \| (string \| RegExp)[]` | `[/\.vue$/]` | File patterns to include                                       |
| `componentId`       | `(filename, source, isProd) => string`     | Hash-based   | Custom component ID generator                                  |
| `preCompile`        | `boolean`                                  | `false`      | Pre-compile all `.vue` files at build start                    |
| `crossFileOptimize` | `boolean`                                  | `false`      | Cross-file prop constness optimization (requires `preCompile`) |
| `template`          | `object`                                   | --           | Template compiler options                                      |
