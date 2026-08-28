# Third-party exclusion contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

Vize, rsvelte, PrimeVue, `pikax/vue-benchmarks`, `pikax/svelte-benchmarks`, and every
other third-party application, component library, compiler, benchmark corpus, or
fixture repository are prohibited as oracle, conformance corpus, expected output,
golden, baseline, or acceptance source.

The prohibition includes adapting a third-party fixture while retaining its semantic
structure, using its output to select a normalizer rule, using its benchmark as a
correctness oracle, or counting it as official-case coverage. Existing repository
benchmarks and experiments that mention those projects remain historical and cannot
satisfy an AMD-005 acceptance ID.

Allowed inputs are limited to the exact official-core sources/packages, independently
authored Verter-local regressions, the exact TypeScript oracle, Web/ECMAScript
standards where directly applicable, and ratified Verter contracts. Performance
fixtures must be independently authored and correctness-checked before measurement.
