# AST0 evidence index

Selected case IDs: `AST0-AC1`, `AST0-AC2`, `AST0-AC3`, `AST0-AC4`, `AST0-AC5`.

This node lands reviewed contract data only. It adds no validator, spec, CI
gate, xtask check or root `package.json` script. The rejection proofs for the
planted rows in `tests/framework-astro/AST0/cases.md` run at AST1G
(`AST1G-ACV`) through `node --test tests/framework-astro/AST0/astro-lock.spec.ts`,
which REG0's `scripts/run-framework-locks.mjs` discovers.

Deletion population this node: empty. No production route is displaced or
retired; ASTP's displaced-route inventory is recorded by AST1M (`AST1M-AC6`)
into `tests/framework-astro/AST0/products/astro-displaced-routes.json`.

Grounding per acceptance item:

- **AC1:** `products/astro-version-lock.json` pins `astro` 7.3.8 and the three
  charter oracles with registry, repository, licence and integrity, excludes
  7.4.0-beta.1 with its reason, and records the WDX1 `astro@5.0.0` row as a
  diverged pin. The registry facts were read from npm on 2026-10-09.
- **AC2:** `products/astro-capability-matrix.json` gives every cell exactly
  one producer and one receiving acceptance ID taken from the producer's
  charter, or a reasoned exclusion. Cells whose operation a producer's outcome
  names but no acceptance item receives are exclusions, not claims.
- **AC3:** `products/astro-activation-policy.json` names FWA1 as the only
  activation source and lists the forbidden routes; the matrix orders the
  hosts tsgo, verter-lsp, typescript-plugin and keeps parser authority with
  Verter.
- **AC4:** every cell carries `claimBasis: none`; the matrix's `notEvidence`
  list excludes ASTP, installed parsers or oracles, and syntax highlighting.
- **AC5:** the version lock's `wireTag` records `FRAMEWORK_TAG_ASTRO = 10` and
  rejects 0, 5 and the class-A range.

Incremental equivalence and bounded work are not applicable: no cache, query,
cancellation or hot path is touched.

Open question: `ast0-grammar-oracle` (the admitted Astro 7.3.8 ships
`@astrojs/compiler-rs`, not the charter's `@astrojs/compiler` oracle). Until
it is ruled, the lock keeps the charter's oracle set and records the observed
packages without making them oracles.

This file does not claim that any case executed.
