/**
 * The PARSER-STRICTNESS PARITY freshness oracle.
 *
 * Compiles every committed parse-parity `.svelte` fixture under
 * `crates/verter_compiler/tests/svelte_oracle_corpus/parse_parity/` through the PINNED
 * official `svelte@5.56.3` compiler (client backend) and emits a JSON map
 * `{ "<fixture-basename>": "<official-error-code>" | "ACCEPT" }` on stdout.
 *
 * The Rust freshness gate (`svelte_parse_parity_matrix.rs`, behind the `svelte-oracle`
 * feature) runs this once and asserts every committed fixture STILL yields its recorded
 * disposition (an accepted fixture → `ACCEPT`; a rejected fixture → its recorded
 * `official_code`) under the pinned compiler — so a fixture can never silently drift
 * from the official compiler's disposition. Sibling of `svelte-reject-oracle.mjs`;
 * reuses the SHARED `loadPinnedCompiler` (the single oracle pin).
 */

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadPinnedCompiler } from "./svelte-golden-lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "..");
const PARSE_PARITY_DIR = resolve(
  REPO_ROOT,
  "crates/verter_compiler/tests/svelte_oracle_corpus/parse_parity",
);

/** Compile one source through the pinned client compiler, returning the official error
 * `code`, or `"ACCEPT"` when the pinned compiler accepts it. */
function officialOutcome(compiler, source, filename) {
  try {
    compiler.compile(source, { generate: "client", dev: false, filename });
    return "ACCEPT";
  } catch (err) {
    return err && err.code ? err.code : `error:${err && err.message}`;
  }
}

function main() {
  const compiler = loadPinnedCompiler(REPO_ROOT);
  const entries = readdirSync(PARSE_PARITY_DIR, { withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith(".svelte"))
    .map((e) => e.name)
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

  const out = {};
  for (const file of entries) {
    const base = file.slice(0, -".svelte".length);
    const source = readFileSync(join(PARSE_PARITY_DIR, file), "utf8");
    out[base] = officialOutcome(compiler, source, file);
  }
  process.stdout.write(JSON.stringify(out, null, 2) + "\n");
}

main();
