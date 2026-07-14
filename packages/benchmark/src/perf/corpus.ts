/**
 * Corpus loader + content-hash verifier for the perf gate.
 *
 * Every perf entry point (the workload runner, the self-referential
 * gate, the offline vize bench) MUST go through `ensureCorpus` before any
 * measurement. It regenerates the synthetic-15k corpus on demand and asserts
 * the produced bytes hash to the committed manifest — so a corpus drift can
 * never silently read as a perf improvement, and a stale checkout never
 * benchmarks the wrong tree.
 *
 * The corpus itself is generate-on-demand (the materialized ~39 MB tree is
 * gitignored); what is committed is the generator and the content-hashed
 * `manifest.json`.
 */
import { readFileSync, existsSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

/** Absolute path to the synthetic-15k corpus root (workspace-relative). */
export const CORPUS_ROOT = join(
  __dirname,
  "..",
  "..",
  "..",
  "..",
  "test-corpora",
  "perf",
  "synthetic-15k",
);
export const GENERATOR_PATH = join(CORPUS_ROOT, "generator", "generate.mjs");
export const MANIFEST_PATH = join(CORPUS_ROOT, "manifest.json");
/** Default materialization directory (gitignored). */
export const DEFAULT_CORPUS_DIR = join(CORPUS_ROOT, "corpus");

export interface CorpusManifest {
  generatorVersion: string;
  seed: number;
  config: {
    fileCount: number;
    moduleCount: number;
    importsPerFile: number;
    compositeModuleCount: number;
  };
  counts: { sfc: number; types: number; tsconfig: number; totalFiles: number; totalBytes: number };
  contentHash: string;
}

/** Read and parse the committed manifest. */
export function readManifest(): CorpusManifest {
  return JSON.parse(readFileSync(MANIFEST_PATH, "utf-8")) as CorpusManifest;
}

interface GeneratorModule {
  buildCorpus: (config?: unknown) => {
    config: unknown;
    files: { relPath: string; bytes: Buffer }[];
  };
  buildManifest: (corpus: unknown) => CorpusManifest;
  hashCorpus: (files: { relPath: string; bytes: Buffer }[]) => string;
  DEFAULT_CONFIG: Record<string, number>;
  GENERATOR_VERSION: string;
}

/** The dynamic-import URL for the corpus generator (.mjs) — Windows-portable. */
export function generatorFileUrl(): string {
  return pathToFileURL(GENERATOR_PATH).href;
}

let _gen: Promise<GeneratorModule> | null = null;
function loadGenerator(): Promise<GeneratorModule> {
  if (_gen === null) {
    // The generator is an ESM .mjs; import it dynamically by file URL.
    _gen = import(generatorFileUrl()) as Promise<GeneratorModule>;
  }
  return _gen;
}

export interface EnsureCorpusOptions {
  /** Override the corpus shape (smoke-testing only — NOT the gate corpus). */
  readonly config?: Record<string, number>;
  /** Where to materialize. Defaults to the gitignored `corpus/` dir. */
  readonly outDir?: string;
  /**
   * Skip the manifest hash check (smoke-testing a custom slice). The gate and
   * the full runs MUST leave this false so the manifest gate is in force.
   */
  readonly skipHashCheck?: boolean;
  /** Suppress progress logging. */
  readonly quiet?: boolean;
  /**
   * Re-materialize even if a corpus dir already exists. Default: re-materialize
   * (a stale on-disk tree would benchmark the wrong bytes).
   */
  readonly force?: boolean;
}

export interface EnsuredCorpus {
  readonly dir: string;
  readonly manifest: CorpusManifest;
  readonly contentHash: string;
  /** Whether the run is the committed gate corpus (no config override). */
  readonly isGateCorpus: boolean;
  readonly appTsconfig: string;
  readonly kernelTsconfig: string;
  readonly rootTsconfig: string;
}

/**
 * Materialize the corpus on disk and verify its content hash against the
 * committed manifest (unless an override config or `skipHashCheck` is given).
 * Returns the materialized directory + the tsconfig entry points.
 *
 * The hash gate is the integrity rail: with the default (committed) config, a
 * mismatch THROWS — the run refuses to measure a tree whose bytes do not match
 * the manifest. A corpus change must refresh `manifest.json` in the same
 * change, which is a deliberate, reviewed act (see the corpus README +
 * `packages/benchmark/baselines/README.md`).
 */
export async function ensureCorpus(options: EnsureCorpusOptions = {}): Promise<EnsuredCorpus> {
  const gen = await loadGenerator();
  const isGateCorpus = options.config === undefined;
  const config = options.config ?? gen.DEFAULT_CONFIG;
  const dir = options.outDir ?? DEFAULT_CORPUS_DIR;

  const corpus = gen.buildCorpus(config);
  const producedHash = gen.hashCorpus(corpus.files);

  if (isGateCorpus && !options.skipHashCheck) {
    const manifest = readManifest();
    if (producedHash !== manifest.contentHash) {
      throw new Error(
        `Corpus content hash mismatch.\n` +
          `  produced: ${producedHash}\n` +
          `  manifest: ${manifest.contentHash}\n` +
          `The generator output no longer matches the committed manifest. A corpus change\n` +
          `must refresh test-corpora/perf/synthetic-15k/manifest.json in the SAME change\n` +
          `(treat it like a baseline refresh). Refusing to benchmark a drifted corpus.`,
      );
    }
    if (manifest.generatorVersion !== gen.GENERATOR_VERSION) {
      throw new Error(
        `Generator version mismatch: manifest ${manifest.generatorVersion} vs generator ` +
          `${gen.GENERATOR_VERSION}. Refresh the manifest.`,
      );
    }
  }

  // Materialize (the runner needs real files for the verter-tsc/tsgo project
  // typecheck and the LSP workspace).
  if (options.force !== false) {
    if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    for (const f of corpus.files) {
      const dest = join(dir, f.relPath);
      mkdirSync(dirname(dest), { recursive: true });
      writeFileSync(dest, f.bytes);
    }
  }

  const manifest = isGateCorpus ? readManifest() : gen.buildManifest(corpus);
  if (!options.quiet) {
    process.stderr.write(
      `corpus ready @ ${dir}\n  ${manifest.counts.sfc} SFCs, ${manifest.counts.totalFiles} files, ` +
        `hash ${producedHash}${isGateCorpus ? " (gate corpus, manifest-verified)" : " (override slice)"}\n`,
    );
  }

  return {
    dir,
    manifest: { ...manifest, contentHash: producedHash },
    contentHash: producedHash,
    isGateCorpus,
    appTsconfig: join(dir, "app", "tsconfig.json"),
    kernelTsconfig: join(dir, "kernel", "tsconfig.json"),
    rootTsconfig: join(dir, "tsconfig.json"),
  };
}
