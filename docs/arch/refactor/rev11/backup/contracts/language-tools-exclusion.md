# Language-tools exclusion contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

`vuejs/language-tools` and `sveltejs/language-tools` are prohibited as oracle, corpus,
expected output, golden, baseline, acceptance source, or production dependency for
the framework compiler conformance program. Their source, snapshots, fixtures, and
generated products cannot be copied, translated, mined, or used to decide that a
candidate is correct.

This prohibition covers runtime JavaScript, semantic analysis, diagnostics, public
API, TSC/TSX, declaration, mapping, and route-equivalence products. A difference from
language-tools is not itself a defect; Verter intentionally corrects behavior that
may be present there.

Ordinary historical repository references remain historical facts only. They cannot
enter an AMD-005 corpus or expectation. TypeScript-visible acceptance uses the
TypeScript compiler/API, ratified Verter contracts, and independently authored local
fixtures as specified in `typescript-product-conformance.md`.
