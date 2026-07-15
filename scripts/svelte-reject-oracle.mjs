/**
 * The OFFICIAL-REJECT freshness oracle.
 *
 * Compiles every committed reject-corpus `.svelte` fixture under
 * `crates/verter_compiler/tests/svelte_oracle_corpus/rejects/block4_core/` through
 * the PINNED official `svelte@5.56.3` compiler (client backend) and emits a JSON map
 * `{ "<fixture-basename>": "<official-error-code>" | "ACCEPT" }` on stdout.
 *
 * The Rust freshness gate (`svelte_client_official_reject_matrix.rs`, behind the
 * `svelte-oracle` feature) runs this once and asserts every committed reject row STILL
 * rejects with its recorded `official_code` under the pinned compiler — so a corpus
 * row can never silently drift from the official compiler's disposition. This is the
 * live-compiler half of the official-reject parity quadrant (the committed JSON
 * metadata is the offline half).
 *
 * Mirrors the pinned-compiler loading + node-toolchain assertions of
 * `gen-svelte-goldens.mjs`; reuses the SHARED `loadPinnedCompiler` (so the oracle pin
 * is single-sourced in `svelte-golden-lib.mjs`).
 */

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadPinnedCompiler } from "./svelte-golden-lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "..");
const REJECT_DIR = resolve(
  REPO_ROOT,
  "crates/verter_compiler/tests/svelte_oracle_corpus/rejects/block4_core",
);

/** Compile one source through the pinned client compiler, returning the official
 * error `code` (`script_duplicate`, `dollar_prefix_invalid`, …), or `"ACCEPT"` when
 * the pinned compiler accepts it (which would be a corpus error the Rust gate flags). */
function officialOutcome(compiler, source, filename) {
  try {
    compiler.compile(source, { generate: "client", dev: false, filename });
    return "ACCEPT";
  } catch (err) {
    // A Svelte `CompileError` carries a stable `code`; a generic parse error surfaces
    // its `code` too (`js_parse_error`). Fall back to the message when no code.
    return err && err.code ? err.code : `error:${err && err.message}`;
  }
}

function main() {
  const compiler = loadPinnedCompiler(REPO_ROOT);
  const entries = readdirSync(REJECT_DIR, { withFileTypes: true })
    .filter((e) => e.isFile() && e.name.endsWith(".svelte"))
    .map((e) => e.name)
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

  const out = {};
  for (const file of entries) {
    const base = file.slice(0, -".svelte".length);
    const source = readFileSync(join(REJECT_DIR, file), "utf8");
    out[base] = officialOutcome(compiler, source, file);
  }
  process.stdout.write(JSON.stringify(out, null, 2) + "\n");
}

main();
