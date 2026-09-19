# Web-product guide recipes

This directory holds the recipe templates for the web-product guides:
CSS, compatibility, debug, tests, runtime, accessibility, performance and
security. It is part of the published example home validated by the
[docs reference harness](../../../docs/scripts/reference-harness.mjs) via
[`../manifest.json`](../manifest.json).

Each recipe below is a structure contract: it fixes what the finished guide
must cover, which existing shipped page owns related truth, and the
executable example slot with a capability link that the recipe requires.
Until the producing feature nodes land, every slot is recorded as
**pending** in the manifest — no recipe claims a working example it does
not have.

- [Accessibility](./accessibility.md) — accessibility analysis and evidence recipes
- [Compatibility](./compatibility.md) — supported targets and compatibility data recipes
- [CSS](./css.md) — CSS semantics and style-analysis recipes
- [Debug](./debug.md) — debugging, mapping and diagnostics recipes
- [Performance](./performance.md) — application-performance measurement recipes
- [Runtime](./runtime.md) — runtime observation and source-correlation recipes
- [Security](./security.md) — security analysis and threat-model recipes
- [Tests](./tests.md) — test-product and runner integration recipes

The recipes gate: once the producing nodes land executable examples with
capability links, the docs build runs `node docs/scripts/reference-harness.mjs
--recipes-gate`. A topic that is still pending, or one whose sample is a
static manifest without an executable extension or cites no capability
surface, fails that gate.

Executable journeys are declared in the [`journeys`](../manifest.json)
section of the manifest and executed by `node
docs/scripts/reference-harness.mjs --run-journeys` on lanes whose pinned
artifacts are present; each journey records exit codes and output digests
as runtime-observation evidence.
