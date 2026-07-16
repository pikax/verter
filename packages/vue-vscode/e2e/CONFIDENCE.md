# What an all-green IDE parity run proves

An all-green run proves the accepted, hermetic contracts in this repository passed on the
requested provider routes. It does not prove every application shape or every future editor
release works.

Green evidence includes:

- all 73 Vue/Svelte matrix cases;
- the broader 219-test parity and hardening inventory;
- exact loaded-suite and build-content attestation;
- framework contract suites as an additional focused gate;
- no hidden product-gap skips; and
- tsserver, managed tsgo, and shared editor-owned tsgo coverage where applicable.

It still does not establish exhaustive framework syntax, all real-world monorepos, long-run
race freedom, or production-load performance. Those require representative external corpora,
stress testing, and performance gates in addition to this suite.

Confidence should therefore be stated narrowly: high confidence for the covered contracts
on the tested routes, not a universal reliability percentage. See `MISSING_CASES.md` for the
known thin and absent surfaces.
