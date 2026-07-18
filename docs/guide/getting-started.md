# Getting Started

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

This guide walks you through adding Verter to an existing Vue project. Verter replaces `@vitejs/plugin-vue` (or your current Vue compiler plugin) with its own universal bundler plugin.

## Prerequisites

- **Node.js** 18 or later
- **pnpm**, npm, or yarn
- An existing Vue 3 project (or create one with `npm create vue@latest`)

## Install the Bundler Plugin

Install `@verter/unplugin`, which provides Vue SFC compilation for all major bundlers:

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

## Configure Vite

Replace `@vitejs/plugin-vue` with Verter in your Vite config:

```ts
// vite.config.ts
import { defineConfig } from "vite";
import VerterVite from "@verter/unplugin/vite";

export default defineConfig({
  plugins: [VerterVite()],
});
```

You can remove `@vitejs/plugin-vue` from your dependencies if you no longer need it:

```sh
pnpm remove @vitejs/plugin-vue
```

## Install the VS Code Extension

For IDE support (completions, hover types, diagnostics, go-to-definition), install the Verter VS Code extension:

1. Open VS Code
2. Go to the Extensions view (`Ctrl+Shift+X` / `Cmd+Shift+X`)
3. Search for **Verter**
4. Click **Install**

::: warning Marketplace listing coming soon
The Marketplace listing is not published yet. Until it is, build and run the extension from source using the steps below.
:::

Alternatively, build the extension from source:

```sh
git clone https://github.com/pikax/verter.git
cd verter
pnpm install
pnpm build
```

Then press `F5` in VS Code to launch the extension in a development host.

::: tip
If you have the Volar extension installed, you may want to disable it for your Verter projects to avoid conflicting language features.
:::

## Verify the Setup

Start your dev server:

```sh
pnpm dev
```

You should see Verter's compilation output in the terminal. Open a `.vue` file in VS Code -- if the Verter extension is active, you will see its language features (hover types, completions, diagnostics) instead of Volar's.

## Try the Playground

Want to try Verter without installing anything? Visit the [online playground](https://play.verterjs.dev) to experiment with Vue SFC compilation in your browser.

## Next Steps

- [Vite Integration](./vite) -- Full configuration options for Vite projects
- [webpack Integration](./webpack) -- Set up Verter with webpack
- [Other Bundlers](./other-bundlers) -- Rollup, esbuild, rspack, Rolldown, and Farm
