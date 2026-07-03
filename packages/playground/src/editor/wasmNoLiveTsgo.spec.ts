/**
 * Guard `wasm_no_live_tsgo_worker_path`.
 *
 * Grep/structural guard: NO reference to any retired Go-WASM engine surface
 * remains in the playground's tracked source (`src`, `scripts`, `public`,
 * `e2e`, plus the tracked root config files). Targeted terms only — a bare
 * mention of the NATIVE engine elsewhere in the repo is a different subject
 * and is deliberately NOT hunted.
 *
 * Build artifacts (`dist/`) and dependencies (`node_modules/`) are excluded:
 * this guard asserts over tracked source, not build output.
 */
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { join, relative, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const playgroundRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

/** Tracked source surfaces to scan. */
const SCAN_DIRS = ["src", "scripts", "public", "e2e"] as const;
const SCAN_ROOT_FILES = [
  "package.json",
  "vite.config.ts",
  "vitest.config.ts",
  "index.html",
  ".gitignore",
] as const;
const SKIP_DIR_NAMES = new Set(["node_modules", "dist", "test-results", "playwright-report"]);
/** Content-scannable text files; anything else (e.g. `.wasm`) is name-checked only. */
const TEXT_FILE =
  /\.(ts|tsx|mts|cts|js|mjs|cjs|jsx|vue|svelte|json|html|css|md|txt|ya?ml|svg|gitignore)$/i;

// The retired Go-WASM engine surface terms, assembled so THIS guard file
// never contains the literal tokens it hunts (no self-match, no file
// exclusions needed — the guard scans itself too).
const g = "ts" + "go";
const we = "wasm" + "_exec";
const TERMS: ReadonlyArray<{ label: string; pattern: RegExp }> = [
  { label: `${g}Worker`, pattern: new RegExp(`${g}worker`, "i") },
  { label: `${g}Service`, pattern: new RegExp(`${g}service`, "i") },
  { label: `fetch-${g}`, pattern: new RegExp(`fetch-${g}`, "i") },
  { label: `${g}.wasm`, pattern: new RegExp(`${g}\\.wasm`, "i") },
  { label: we, pattern: new RegExp(we, "i") },
  { label: `the "${g}" TypeCheckerMode`, pattern: new RegExp(`["']${g}["']`) },
];

function scanFile(path: string, hits: string[], visited: string[]): void {
  const rel = relative(playgroundRoot, path).replace(/\\/g, "/");
  visited.push(rel);
  const name = path.split(/[\\/]/).pop() ?? path;
  for (const { label, pattern } of TERMS) {
    if (pattern.test(name)) {
      hits.push(`${rel} (file name matches ${label})`);
    }
  }
  if (!TEXT_FILE.test(name) && !(SCAN_ROOT_FILES as readonly string[]).includes(name)) return;
  const lines = readFileSync(path, "utf8").split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const { label, pattern } of TERMS) {
      if (pattern.test(line)) {
        hits.push(`${rel}:${index + 1}: [${label}] ${line.trim().slice(0, 140)}`);
      }
    }
  });
}

function walk(dir: string, hits: string[], visited: string[]): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIR_NAMES.has(entry.name)) continue;
      walk(join(dir, entry.name), hits, visited);
      continue;
    }
    scanFile(join(dir, entry.name), hits, visited);
  }
}

function runScan(): { hits: string[]; visited: string[] } {
  const hits: string[] = [];
  const visited: string[] = [];
  for (const dir of SCAN_DIRS) {
    const abs = join(playgroundRoot, dir);
    if (existsSync(abs)) walk(abs, hits, visited);
  }
  for (const file of SCAN_ROOT_FILES) {
    const abs = join(playgroundRoot, file);
    if (existsSync(abs)) scanFile(abs, hits, visited);
  }
  return { hits, visited };
}

describe("wasm_no_live_tsgo_worker_path (#8)", () => {
  it("no reference to the retired Go-WASM engine surface remains in tracked playground source", () => {
    const { hits } = runScan();
    expect(hits).toEqual([]);
  });

  it("the scan is non-vacuous (it really walks the tracked source tree)", () => {
    const { visited } = runScan();
    expect(visited.length).toBeGreaterThan(50);
    expect(visited).toContain("src/editor/Editor.vue");
    expect(visited).toContain("src/core/types.ts");
    expect(visited).toContain("package.json");
    expect(visited).toContain(".gitignore");
  });
});
