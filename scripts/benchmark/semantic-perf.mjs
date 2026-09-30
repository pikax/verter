#!/usr/bin/env node
// Equivalent-demand semantic benchmark: Verter (production release probe)
// vs TypeScript 7.0.2 (native API, plus `tsc -p` in both thread modes for
// whole-program reference), every invocation under verter-supervise.
//
//   node scripts/benchmark/semantic-perf.mjs [--only a,b] [--repeat N] [--out dir] ...
//
// See docs/contributing/semantic-benchmark.md, or run with --help.

import { main } from "./semantic-perf/run.mjs";

main(process.argv.slice(2)).then(
  (code) => process.exit(code),
  (err) => {
    console.error(`semantic-perf: ${err?.message ?? err}`);
    process.exit(2);
  },
);
