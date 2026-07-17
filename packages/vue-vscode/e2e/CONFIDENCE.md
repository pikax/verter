# What an all-green IDE parity run proves

An all-green run proves the accepted, hermetic contracts in this repository passed on the
requested provider routes. It does not prove every application shape or every future editor
release works.

Green evidence includes:

- every Vue/Svelte matrix case discovered from the authored matrix;
- every literal parity and hardening test discovered from the authored suite tree;
- exact loaded-suite and build-content attestation;
- framework contract suites as an additional focused gate;
- no hidden product-gap skips; and
- tsserver and managed tsgo coverage for every standard fixture, plus shared editor-owned
  tsgo for every fixture with a configured project binding.

The build manifest records the current tree-derived test IDs and suite files, while one canonical
route inventory drives both the local runner and CI. Documentation deliberately does not repeat
hand-maintained totals that can drift from those executable contracts.

It still does not establish exhaustive framework syntax, all real-world monorepos, long-run
race freedom, or production-load performance. Those require representative external corpora,
stress testing, and performance gates in addition to this suite.

Confidence should therefore be stated narrowly: high confidence for the covered contracts
on the tested routes, not a universal reliability percentage. See `MISSING_CASES.md` for the
known thin and absent surfaces.
