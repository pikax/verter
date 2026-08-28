# ADR-015 — Core Dependency Direction Is Inward and Cycle-Free

**Status:** Accepted  
**Decision owner:** crate/module ownership  
**Reopen only if:** a new stable boundary cannot be represented without a cycle and an alternative decomposition has been disproven.

## Context

Where `CompileTypeInfo`, framework projection DTOs, and request contracts live can accidentally create compiler↔semantic or compiler↔session cycles, forcing traits, erasure, and shared ownership.

## Decision

Binding direction is:

```text
identity/span/language/contracts
-> shared syntax frontends and dependency-neutral DTOs
-> semantic kernel/module resolver/relation/flow
-> compiler
-> managed engine/session
-> LSP/provider/MCP/NAPI/WASM/CLI adapters
```

Rules:

- syntax/contracts do not depend on compiler, session, provider, or LSP;
- semantic kernel does not depend on compiler, session, provider, or LSP;
- compiler may depend on syntax, sealed semantic facade, and neutral closed DTOs;
- managed engine depends on compiler/semantic, never the reverse;
- provider lifecycle never enters direct compiler or semantic kernel;
- adapters depend inward only;
- durable build tests reject crate dependency cycles and forbidden edges.

Logical owners do not automatically require crates; use modules/functions until a real dependency firewall or multi-consumer stable contract exists.

## Consequences

- direct compiler cannot become a session mode;
- semantic kernel remains reusable across lifecycles;
- fewer traits, erased bags, and `Arc` workarounds are required.

## Rejected alternatives

- **Mutual compiler/semantic callbacks:** creates cycles and alternate behavior.
- **Everything in session/host crate:** preserves catch-all ownership.
