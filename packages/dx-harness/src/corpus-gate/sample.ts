/**
 * Deterministic, representative corpus sampling.
 *
 * Enumeration walks the corpus root read-only, collects every `.vue` file
 * (skipping `node_modules`, dot-directories, and build-output directories),
 * and sorts lexicographically so the candidate order is stable run-over-run.
 *
 * Selection is bucketed: each structural feature (slots, events, directives,
 * styles, props/macros, deep imports, barrel imports, generics, script+template
 * pairing, large files) forms a bucket; selection round-robins the buckets in a
 * fixed order taking the highest-scoring unused file from each, then fills any
 * remainder from the global score order. Everything is a pure function of the
 * `(relativePath, text)` pairs — no randomness, no clock — so the same corpus
 * content always yields the same sample (receipts prove it with a manifest hash).
 */
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";

/** Structural features a corpus SFC can exhibit (detection is text-shape only). */
export interface CorpusFileFeatures {
  readonly script: boolean;
  readonly template: boolean;
  readonly slots: boolean;
  readonly events: boolean;
  readonly directives: boolean;
  readonly styles: boolean;
  readonly props: boolean;
  readonly emits: boolean;
  readonly deepImport: boolean;
  readonly barrelImport: boolean;
  readonly generic: boolean;
  readonly large: boolean;
}

export interface CorpusFileProfile {
  readonly relativePath: string;
  readonly features: CorpusFileFeatures;
  /** Count of distinct present features — the representativeness score. */
  readonly score: number;
  readonly bytes: number;
}

const SKIPPED_DIRECTORIES = new Set(["node_modules", "dist", "build", "coverage", "target"]);
const LARGE_FILE_BYTES = 20_000;

/**
 * Recursively enumerate `.vue` files under `corpusDir` (read-only), returning
 * forward-slashed relative paths in stable lexicographic order.
 */
export function enumerateCorpusVueFiles(corpusDir: string): string[] {
  const found: string[] = [];
  const visit = (dir: string): void => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const absolute = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (!entry.name.startsWith(".") && !SKIPPED_DIRECTORIES.has(entry.name)) visit(absolute);
      } else if (entry.isFile() && entry.name.endsWith(".vue")) {
        found.push(path.relative(corpusDir, absolute).replaceAll("\\", "/"));
      }
    }
  };
  visit(corpusDir);
  return found.sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}

/** Profile one SFC's structural features from its text (pure). */
export function profileCorpusFile(relativePath: string, text: string): CorpusFileProfile {
  const features: CorpusFileFeatures = {
    script: /<script[\s>]/.test(text),
    template: /<template[\s>]/.test(text),
    slots: /<slot\b|<template\s+#|v-slot/.test(text),
    events: /@[a-z][\w-]*\s*=|v-on:/.test(text),
    directives: /v-(?:if|else|for|show|model|bind|memo|once)\b/.test(text),
    styles: /<style[\s>]/.test(text),
    props: /defineProps|withDefaults|\bprops\s*:/.test(text),
    emits: /defineEmits|\bemits\s*:/.test(text),
    deepImport: /import[^\r\n]*from\s+["'](?:\.\.\/(?:\.\.\/)+|@\/[\w-]+\/)[^"']+["']/.test(text),
    barrelImport: /import[^\r\n]*from\s+["']@?[A-Za-z][\w.-]*["']/.test(text),
    generic: /<script[^>]*\sgeneric\s*=/.test(text),
    large: Buffer.byteLength(text, "utf8") > LARGE_FILE_BYTES,
  };
  const score = Object.values(features).filter(Boolean).length;
  return { relativePath, features, score, bytes: Buffer.byteLength(text, "utf8") };
}

/** Enumerate + read + profile the whole corpus (read-only). */
export function profileCorpus(corpusDir: string): CorpusFileProfile[] {
  return enumerateCorpusVueFiles(corpusDir).map((relativePath) =>
    profileCorpusFile(relativePath, readFileSync(path.join(corpusDir, relativePath), "utf8")),
  );
}

/** Fixed bucket order — part of the deterministic-selection contract. */
const BUCKET_ORDER: readonly (keyof CorpusFileFeatures)[] = [
  "generic",
  "slots",
  "events",
  "directives",
  "styles",
  "props",
  "emits",
  "deepImport",
  "barrelImport",
  "large",
  "script",
  "template",
];

function byScoreThenPath(left: CorpusFileProfile, right: CorpusFileProfile): number {
  if (left.score !== right.score) return right.score - left.score;
  return left.relativePath < right.relativePath
    ? -1
    : left.relativePath > right.relativePath
      ? 1
      : 0;
}

/**
 * Deterministically select up to `n` representative profiles: round-robin the
 * feature buckets (highest score first inside each), then fill the remainder
 * from the global score order. Pure — same input, same output.
 */
export function selectRepresentativeSample(
  profiles: readonly CorpusFileProfile[],
  n: number,
): CorpusFileProfile[] {
  if (n <= 0) return [];
  const buckets = new Map<keyof CorpusFileFeatures, CorpusFileProfile[]>();
  for (const bucket of BUCKET_ORDER) {
    buckets.set(
      bucket,
      profiles.filter((profile) => profile.features[bucket]).sort(byScoreThenPath),
    );
  }
  const selected: CorpusFileProfile[] = [];
  const taken = new Set<string>();
  const take = (profile: CorpusFileProfile): void => {
    if (taken.has(profile.relativePath)) return;
    taken.add(profile.relativePath);
    selected.push(profile);
  };

  // Round-robin the buckets until the target is reached or no bucket advances.
  for (let round = 0; selected.length < n; round += 1) {
    let advanced = false;
    for (const bucket of BUCKET_ORDER) {
      if (selected.length >= n) break;
      const candidates = buckets.get(bucket) ?? [];
      const next = candidates.find((profile) => !taken.has(profile.relativePath));
      if (next) {
        take(next);
        advanced = true;
      }
    }
    if (!advanced) break;
  }
  // Remainder: global score order (covers corpora whose files sit in no bucket).
  if (selected.length < n) {
    for (const profile of [...profiles].sort(byScoreThenPath)) {
      if (selected.length >= n) break;
      take(profile);
    }
  }
  // Stable output order: lexicographic by path, so probing order is deterministic.
  return selected.sort((left, right) =>
    left.relativePath < right.relativePath ? -1 : left.relativePath > right.relativePath ? 1 : 0,
  );
}

/**
 * Stable hash of a sampled relative-path list. Receipts embed this instead of
 * the file names so two runs can prove they used the same sample without any
 * corpus name ever entering a receipt.
 */
export function sampleManifestHash(relativePaths: readonly string[]): string {
  const hash = createHash("sha256");
  for (const relativePath of relativePaths) hash.update(relativePath).update("\n");
  return hash.digest("hex").slice(0, 16);
}
