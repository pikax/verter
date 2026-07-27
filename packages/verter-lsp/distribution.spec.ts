/**
 * `verter-lsp` distribution guards.
 *
 * The suite itself is shared with every other Verter binary family — one
 * implementation of the platform-package, resolution and CLI contracts, run
 * here against this family's matrix. See
 * `packages/binary-launcher/test-support/family-assertions.ts`.
 */

import { createRequire } from "node:module";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

import { describeBinaryFamily } from "../binary-launcher/test-support/family-assertions.ts";

const require = createRequire(import.meta.url);

describeBinaryFamily({
  packageDir: dirname(fileURLToPath(import.meta.url)),
  packageName: "verter-lsp",
  module: require("./index.js"),
  platforms: require("./platforms.js"),
});
