# @verter/language-server

Language-server that handles the heavy-lifting

## Concepts

### VueDocument

`VueDocument` will be broken down into multiple files, based on the SFC regions:

- `{path}.vue.ts`: Contains the bundled modules
- `options.{lang}`: Contains generated options from vue component
- `<script>`:
  - `{path}.vue.script.ts`: Contains the exposed module it imports the Render
- `<template>`:
  - `{path}.vue.render.tsx`: Contains the render TSX
- `<style>`:
  - `{path}.vue.style.css`: Contains the style blocks
- `<{block}>`:
  - `{path}.vue.{block}.ts`: Contains custom block information - TBD

## inspired by

https://github.com/sveltejs/language-tools

https://github.com/vuedx/languagetools

https://github.com/typescript-language-server

https://code.visualstudio.com/api/language-extensions/language-server-extension-guide
