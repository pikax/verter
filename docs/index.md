---
layout: home

hero:
  name: Verter
  text: Fast Next Gen Unofficial Vue Tools
  tagline: 9x faster template compilation, full TypeScript type safety, universal bundler support
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/pikax/verter
    - theme: alt
      text: Playground
      link: https://play.verterjs.dev

features:
  - icon: ⚡
    title: 9x Faster Compilation
    details: Rust-powered template compiler processes Vue SFCs at up to 32 MB/s throughput.
    link: /guide/performance
    linkText: Benchmarks
  - icon: 🛡️
    title: Full TypeScript Safety
    details: Generic components, typed directives, automatic event handler inference — strict-first approach.
    link: /guide/features
    linkText: Features
  - icon: 📦
    title: Universal Bundler Plugin
    details: Drop-in replacement for @vitejs/plugin-vue. Works with Vite, webpack, Rollup, esbuild, rspack, Rolldown, Farm.
    link: /guide/getting-started
    linkText: Getting Started
  - icon: 🔍
    title: Rust LSP Server
    details: 30+ language features including completions, diagnostics, hover, go-to-definition, rename, and more.
    link: /editor/lsp-features
    linkText: LSP Features
  - icon: 🖥️
    title: VS Code Extension
    details: Reactivity color decorations, Vue API annotations, analysis sidebar, compiled code viewer.
    link: /editor/vscode
    linkText: Editor Setup
  - icon: 🔎
    title: Built-in Diagnostics
    details: 22+ rules for accessibility, Vue best practices, performance, security, and CSS analysis.
    link: /guide/linting
    linkText: Diagnostic Rules
  - icon: 🔗
    title: Cross-File Optimization
    details: Whole-program analysis detects constant props across components, skipping runtime tracking.
    link: /guide/cross-file-optimization
    linkText: Learn More
---
