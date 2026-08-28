# Server/SSR and hydration contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

`RuntimeServer` is generated JavaScript executed by the exact official server
runtime. It is not a Verter runtime. Vue server output uses the official RC.3 SSR
topology; Svelte server output uses the official 5.56.8 server topology. The harness
does not equate these families or reuse Vue lowering in Svelte.

Server acceptance checks module parse/link, helper import sources, escaping,
attributes/props, component/slot topology, async behavior, markers, deterministic
rendered output, diagnostics, and source maps. Client and server profiles remain
separate capability cells.

Hydration tests run in a deterministic DOM environment against locked official
runtimes and cover, where the official protocol makes pairing meaningful:

1. official server / official client (harness control);
2. Verter server / Verter client; and
3. official server / Verter client (cross-pair compatibility).

If the official topology also defines a meaningful inverse pairing, BF2 records and
runs it; otherwise the manifest marks it `not_applicable` with source evidence.
Assertions cover initial DOM/markers, successful hydration without replacement,
events/effects, state updates, async/boundary behavior, warnings/diagnostics, and
post-hydration DOM.

Vue `SSR x Vapor` is not invented as a Cartesian backend. RC.3 server compilation
uses the official SSR compiler and officially defined Vapor metadata/topology. A
request for a nonexistent combined compiler mode is rejected during B3 request
construction.

The harness cannot patch output, synthesize missing markers/helpers, relax import
resolution, or substitute a simplified runtime. A hydration mismatch is semantic and
cannot be normalized away.
