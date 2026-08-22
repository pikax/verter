# Official-core oracle contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

## Immutable domains

| framework | upstream identity | allowed use |
|---|---|---|
| Vue | `vuejs/core v3.6.0-rc.5`, commit `f11c8f2639ce15559d64ea054e409081bd8a0ce1`, tree `980693b602cff54d492a1d6ada18470596cbf978` | compiler oracle and official runtime executor in hermetic tests |
| Svelte | `sveltejs/svelte svelte@5.56.8`, tag object `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c`, commit `44a7813730579b94004e182e5a67aab27aa9d2a6`, tree `63390158bfe8f997c474e35215a4fa627194c229` | compiler oracle and official runtime executor in hermetic tests |

The package locks under `evidence/framework-conformance/oracles/` are part of each
domain. The harness rejects any source SHA/tree, package version, integrity, or
transitive closure mismatch before generating expectations or running candidate
output. Network resolution is forbidden during conformance execution.

## Oracle roles

Official compilers may generate immutable expected artifacts and diagnostics and may
compile the official side of execution/hydration pairings. They cannot run in any
production path, repair candidate output, supply missing candidate helpers, or serve
as fallback.

Official runtimes may parse/link/execute the exact generated output without patching.
The harness resolves imports against the locked real packages; it cannot mock a
missing export or replace a runtime with a simplified implementation.

For TSC, TSX, declaration, and public API products, the oracle is instead the exact
Revision 11 TypeScript domain, TypeScript compiler/API observations, ratified Verter
contracts, and independently authored local fixtures. Official framework compilers
may contribute framework behavior but cannot replace that TypeScript oracle.

## Domain changes

A newer Vue RC, Vue stable, or Svelte release is a distinct domain. It requires an
amendment, immutable source and package lock, capability review, complete regenerated
case/golden evidence, independent challenges, and maintainer ratification. A range,
dist-tag, moving branch, or automatic lock refresh is invalid.
