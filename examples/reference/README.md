# Public reference examples

These examples are the published user-facing example home. The docs build
(`pnpm docs:build` / `pnpm --filter docs check`) runs them through the
reference harness: every import must resolve to a shipped package export,
and every command must be a bin of a public package.

The example index is [`manifest.json`](./manifest.json). The
[SDK and official-extension guide structure](./sdk/README.md) declares the
six guide topics and their executable example slots. The
[web-product guide recipes](./guides/README.md) declare the eight recipe
topics (CSS, compatibility, debug, tests, runtime, accessibility,
performance, security) with executable example slots and capability
links, plus the executable journeys recorded in the manifest.
Contributor sandboxes (`examples/`, `packages/example`) are not this home
and are not treated as executed user evidence.
