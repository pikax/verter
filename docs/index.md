---
layout: home

hero:
  name: Verter
  text: Fast Next Gen Unofficial Vue Tools
  tagline: Rust-powered template compilation, full TypeScript type safety, universal bundler support
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
    title: Rust-Powered Compilation
    details: Native Rust template compiler for both production render functions and IDE type-checking. Benchmarks are reported per-revision, not as a fixed marketing number.
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
    details: 180+ rules for accessibility, Vue best practices, performance, security, SSR, and CSS analysis.
    link: /guide/linting
    linkText: Diagnostic Rules
  - icon: 🔗
    title: Cross-File Optimization
    details: Whole-program analysis detects constant props across components, skipping runtime tracking.
    link: /guide/cross-file-optimization
    linkText: Learn More
---
