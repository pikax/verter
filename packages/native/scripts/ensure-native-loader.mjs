// Build-before-use contract for `@verter/native` (issue #90 item 7).
//
// The committed root `index.js` is a thin wrapper that `require`s the
// NAPI-generated `./dist/index.js` loader. On a fresh checkout, before the
// native build has run, that file does not exist — and a bare
// `require("./dist/index.js")` then fails with a confusing
// `MODULE_NOT_FOUND` deep inside vitest. This guard runs as the package's
// `pretest` step and fails FAST with an actionable message naming the build
// command to run.
//
// It deliberately does NOT synthesize a stand-in loader: the loader is a
// build artifact of the canonical napi generation path (`napi build` +
// `build:types`). Synthesizing one would mask a broken build and could ship
// a hand-rolled loader — exactly the issue #90 regression. The only correct
// remediation is to run the real build, which this message tells you to do.

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = dirname(dirname(fileURLToPath(import.meta.url)));
const loaderPath = join(packageDir, "dist", "index.js");

if (!existsSync(loaderPath)) {
  process.stderr.write(
    "\n[@verter/native] Missing generated napi loader: dist/index.js\n" +
      "\n" +
      "  The root index.js wrapper requires ./dist/index.js, which is a\n" +
      "  build artifact produced by the napi build. It is not present, so\n" +
      "  the tests cannot load the binding.\n" +
      "\n" +
      "  Run the native build first:\n" +
      "    pnpm --filter @verter/native build:debug   (debug, faster)\n" +
      "    pnpm --filter @verter/native build         (release)\n" +
      "\n",
  );
  process.exit(1);
}
