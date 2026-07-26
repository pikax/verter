#!/usr/bin/env node

/**
 * publish-platform-dirs.mjs
 *
 * Print the per-platform binary package directories the release publishes, one
 * repo-relative path per line, derived from scripts/lib/publish-set.mjs — the
 * same authority release.yml publishes the main packages from.
 *
 * The release workflow loops over this output instead of hand-listing one glob
 * per binary family (`packages/native/npm/*`, `packages/verter-lsp/npm/*`,
 * `packages/verter-tsc/npm/*`), so a new platform family is published the
 * moment it enters the product dependency closure.
 *
 * Usage:
 *   node scripts/publish-platform-dirs.mjs
 */

import { computePublishSet } from "./lib/publish-set.mjs";

const dirs = computePublishSet().platform;

if (dirs.length === 0) {
  console.error(
    "publish-platform-dirs: the platform publish set is empty — refusing to emit nothing",
  );
  process.exit(1);
}

// Forward slashes: the consumer is a POSIX shell loop in CI, and the paths are
// repo-relative so they work from the workspace root on any platform.
for (const dir of dirs) {
  process.stdout.write(`${dir.split("\\").join("/")}\n`);
}
