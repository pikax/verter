# webpack Integration

::: warning Pre-Release
Verter is pre-release software. APIs may change between releases — see the [API Stability](/api-stability) document.
:::

Verter provides a webpack plugin through `@verter/unplugin`. It replaces `vue-loader` for compiling Vue Single File Components.

## Installation

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

## Configuration

```js
// webpack.config.js
const VerterWebpack = require('@verter/unplugin/webpack').default

module.exports = {
  plugins: [VerterWebpack()],
}
```

Or with ES module syntax:

```ts
// webpack.config.ts
import VerterWebpack from '@verter/unplugin/webpack'

export default {
  plugins: [VerterWebpack()],
}
```

## Style Loaders

Verter handles Vue SFC compilation, but you still need standard webpack loaders for CSS processing. Make sure you have the appropriate style loaders configured:

```js
// webpack.config.js
const VerterWebpack = require('@verter/unplugin/webpack').default

module.exports = {
  plugins: [VerterWebpack()],
  module: {
    rules: [
      {
        test: /\.css$/,
        use: ['style-loader', 'css-loader'],
      },
      // Add loaders for preprocessors as needed:
      {
        test: /\.scss$/,
        use: ['style-loader', 'css-loader', 'sass-loader'],
      },
      {
        test: /\.less$/,
        use: ['style-loader', 'css-loader', 'less-loader'],
      },
    ],
  },
}
```

## HMR Behavior

When using webpack-dev-server, Verter supports Hot Module Replacement. The HMR strategy is automatically set to `"webpack"` when running under webpack, which uses webpack's built-in HMR API instead of Vite's.

## Options

All options from the [Vite integration guide](./vite#options) are available:

- [`include`](./vite#include) -- File patterns to include
- [`componentId`](./vite#componentid) -- Custom component ID generator
- [`preCompile`](./vite#precompile) -- Pre-compile `.vue` files at build start
- [`crossFileOptimize`](./vite#crossfileoptimize) -- Cross-file prop constness optimization
- [`template`](./vite#template) -- Template compiler options

```js
const VerterWebpack = require('@verter/unplugin/webpack').default

module.exports = {
  plugins: [
    VerterWebpack({
      preCompile: true,
      template: {
        compilerOptions: {
          isCustomElement: (tag) => tag.startsWith('my-'),
        },
      },
    }),
  ],
}
```
