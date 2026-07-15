import { readFileSync } from "node:fs";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const SVELTE_BENCHMARK_REVISION_COUNT = 16;

export type SvelteCompilerFixtureName =
  | "basic_runes"
  | "keyed_each"
  | "typescript_instance"
  | "typescript_module"
  | "scoped_css"
  | "component_snippet"
  | "await_block"
  | "legacy_store"
  | "special_window"
  | "large_dashboard";

interface SvelteCompilerManifestEntry {
  name: SvelteCompilerFixtureName;
  slug: string;
  coverage: string[];
}

interface SvelteCompilerManifest {
  schemaVersion: 1;
  fixtures: SvelteCompilerManifestEntry[];
}

export interface SvelteCompilerFixture extends SvelteCompilerManifestEntry {
  filename: string;
  source: string;
  sourceBytes: number;
}

const manifestPath = fileURLToPath(new URL("./svelte-perf-manifest.json", import.meta.url));
const repositoryRoot = resolve(dirname(manifestPath), "../../..");
const fixtureRoot = resolve(
  repositoryRoot,
  "crates/verter_compiler/tests/svelte_oracle_corpus/fixtures",
);

function parseManifest(): SvelteCompilerManifest {
  const value = JSON.parse(readFileSync(manifestPath, "utf8")) as Partial<SvelteCompilerManifest>;
  if (value.schemaVersion !== 1 || !Array.isArray(value.fixtures)) {
    throw new Error("Svelte compiler benchmark manifest must use schemaVersion 1 with fixtures");
  }
  if (value.fixtures.length < 10) {
    throw new Error(
      `Svelte compiler benchmark requires at least 10 fixtures, got ${value.fixtures.length}`,
    );
  }
  return value as SvelteCompilerManifest;
}

function loadFixture(entry: SvelteCompilerManifestEntry): SvelteCompilerFixture {
  if (!/^[a-z0-9_/-]+$/u.test(entry.slug) || entry.slug.includes("..")) {
    throw new Error(`invalid Svelte benchmark fixture slug: ${entry.slug}`);
  }
  if (!Array.isArray(entry.coverage) || entry.coverage.length === 0) {
    throw new Error(`Svelte benchmark fixture ${entry.name} has no coverage labels`);
  }
  const path = resolve(fixtureRoot, `${entry.slug}.svelte`);
  const pathFromRoot = relative(fixtureRoot, path);
  if (pathFromRoot.startsWith(`..${sep}`) || pathFromRoot === "..") {
    throw new Error(`Svelte benchmark fixture escaped the oracle root: ${entry.slug}`);
  }
  const source = readFileSync(path, "utf8");
  if (source.trim().length === 0) {
    throw new Error(`Svelte benchmark fixture is empty: ${entry.slug}`);
  }
  return {
    ...entry,
    filename: `${entry.slug}.svelte`,
    source,
    sourceBytes: Buffer.byteLength(source),
  };
}

const manifest = parseManifest();
const names = new Set<string>();
const slugs = new Set<string>();

export const SVELTE_COMPILER_FIXTURES: readonly SvelteCompilerFixture[] = manifest.fixtures.map(
  (entry) => {
    if (names.has(entry.name)) throw new Error(`duplicate Svelte benchmark name: ${entry.name}`);
    if (slugs.has(entry.slug)) throw new Error(`duplicate Svelte benchmark slug: ${entry.slug}`);
    names.add(entry.name);
    slugs.add(entry.slug);
    return loadFixture(entry);
  },
);

/**
 * Cycle semantically inert authored revisions so every sample reparses and
 * recompiles the same component through both backends. Verter additionally
 * attests `cacheHit === false` and `actualMode === "stateless"` per sample.
 */
export function sourceForBenchmarkSequence(
  fixture: SvelteCompilerFixture,
  sequence: number,
): string {
  if (!Number.isSafeInteger(sequence) || sequence < 0) {
    throw new Error(`Svelte benchmark sequence must be a non-negative integer, got ${sequence}`);
  }
  return `${fixture.source}\n<!-- @verter-benchmark-revision:${sequence % SVELTE_BENCHMARK_REVISION_COUNT} -->`;
}
