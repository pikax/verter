# Documentation Guide

This document defines how all Verter project documentation should be written and structured.

## Project Narrative

Verter started as a Vue Language Server Protocol (LSP) implementation and SFC-to-TSX transformation tool for VS Code, aiming to provide better TypeScript support than Volar. The project is now evolving into a **full Vue compiler**, with the long-term goal of replacing the TypeScript-based transformation packages (`@verter/core`) with Rust implementations (`verter_core`).

**This is an experimental, actively changing project.** Every piece of documentation must reflect this reality.

## Tone & Wording

- **Be concise.** Avoid filler words and unnecessary detail.
- **Be honest about stability.** Every README must include an experimental warning banner.
- **Use "currently" to flag things that will change.** Example: "Currently, template compilation is handled by `@verter/core` in TypeScript, but this is being migrated to Rust."
- **Mention Rust migration plans where relevant.** When documenting a TypeScript package that has a Rust counterpart or will be replaced, note it.
- **Don't oversell.** This is alpha software. Don't promise features that aren't implemented.
- **Use active voice.** "The plugin transforms `.vue` files" not "`.vue` files are transformed by the plugin."

## README Template

Every package and crate README must follow this structure. Sections marked "if applicable" can be omitted when not relevant.

```markdown
# {Package Name}

{One-line description of what this package does.}

> [!WARNING]
> This project is **experimental and under active development**. APIs, architecture, and package boundaries may change without notice.

## Overview

{2-3 paragraphs: what it does, why it exists, how it fits in the Verter ecosystem.
Mention Rust migration plans if relevant.}

## Installation <!-- if publishable to npm/crates.io -->

## Architecture <!-- include Mermaid diagram -->

## API / Usage

## Configuration <!-- if applicable -->

## Development / Build

## Testing <!-- if applicable -->

## Dependencies

## License
```

### Section Guidelines

- **Overview**: Explain purpose, context within the monorepo, and any planned changes.
- **Installation**: Only for packages published (or planned to be published) to npm or crates.io.
- **Architecture**: Include a Mermaid diagram showing the package's internal structure or how it connects to other packages. Keep it focused — show only what's relevant to this package.
- **API / Usage**: Show the primary exports and how to use them. Include code examples.
- **Configuration**: Document all user-facing options, settings, or config file formats.
- **Development / Build**: How to build, watch, and develop locally.
- **Testing**: How to run tests, what testing patterns are used.
- **Dependencies**: List key dependencies and what they're used for.
- **License**: Always `MIT` (matching the root LICENSE).

## Diagram Conventions

### Mermaid (preferred)

Use Mermaid diagrams for architecture and flow visualizations. GitHub renders Mermaid natively in markdown code blocks.

````markdown
​`mermaid
graph TD
    A[Input] --> B[Processing]
    B --> C[Output]
​`
````

**Guidelines:**

- Use `graph TD` (top-down) for hierarchies and dependency graphs
- Use `graph LR` (left-right) for pipelines and data flows
- Use `flowchart` for complex flows with decisions
- Keep diagrams focused — max ~15 nodes per diagram
- Use `subgraph` to group related components
- Include brief labels inside nodes: `A["@verter/core<br/>(SFC → TSX)"]`

### ASCII (fallback)

Use ASCII diagrams only for simple inline illustrations (e.g., directory trees or very simple flows):

```
src/
├── parser/     # SFC parsing
├── process/    # Plugin pipeline
└── utils/      # Shared utilities
```

## Linking

- Always use **relative paths** when linking between project files.
- Link to related package READMEs when mentioning other packages.
- Example: `See [@verter/core](../core/README.md) for the transformation engine.`

## Status Indicators

Every README includes the experimental warning banner (shown in the template above). Additionally, if a package has specific stability notes, add them in the Overview:

- **Experimental**: APIs are unstable and may change significantly
- **Alpha**: Core functionality works but has known limitations
- **Migrating to Rust**: TypeScript implementation being replaced

## Version

All packages currently share version `0.0.1-alpha.1`. When referencing versions, use the workspace version from root `package.json` / `Cargo.toml`.
