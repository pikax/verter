/**
 * Deterministic, representative sampling of plain `.ts`/`.tsx` corpus files.
 *
 * Structural mirror of the corpus gate's SFC sampler (`corpus-gate/sample.ts`):
 * read-only enumeration in stable lexicographic order, text-shape feature
 * profiling, fixed-order bucket round-robin selection, then a global
 * score-order fill. Pure function of `(relativePath, text)` — no randomness,
 * no clock — so the same corpus content always yields the same sample, proven
 * by the shared `sampleManifestHash`.
 *
 * `.d.ts` files are excluded: the reference measures authored editor targets,
 * and a declaration file is not the position class an editor operation lands
 * on. Generated/output directories are skipped exactly like the SFC sampler.
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";

/** Structural features a plain TS corpus file can exhibit (text-shape only). */
export interface NativeTsFileFeatures {
  readonly imports: boolean;
  readonly deepImport: boolean;
  readonly barrelImport: boolean;
  readonly exportFn: boolean;
  readonly exportType: boolean;
  readonly classDecl: boolean;
  readonly genericDecl: boolean;
  readonly jsx: boolean;
  readonly memberAccess: boolean;
  readonly large: boolean;
}

export interface NativeTsFileProfile {
  readonly relativePath: string;
  readonly features: NativeTsFileFeatures;
  readonly score: number;
  readonly bytes: number;
}

const SKIPPED_DIRECTORIES = new Set(["node_modules", "dist", "build", "coverage", "target"]);
const LARGE_FILE_BYTES = 20_000;

/**
 * Recursively enumerate `.ts`/`.tsx` files (excluding `.d.ts`) under
 * `corpusDir` read-only, returning forward-slashed relative paths in stable
 * lexicographic order.
 */
export function enumerateCorpusTsFiles(corpusDir: string): string[] {
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
      } else if (
        entry.isFile() &&
        (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx")) &&
        !entry.name.endsWith(".d.ts")
      ) {
        found.push(path.relative(corpusDir, absolute).replaceAll("\\", "/"));
      }
    }
  };
  visit(corpusDir);
  return found.sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}

/** Profile one TS file's structural features from its text (pure). */
export function profileNativeTsFile(relativePath: string, text: string): NativeTsFileProfile {
  const features: NativeTsFileFeatures = {
    imports: /^\s*import\s/m.test(text),
    deepImport: /import[^\r\n]*from\s+["'](?:\.\.\/(?:\.\.\/)+|@\/[\w-]+\/)[^"']+["']/.test(text),
    barrelImport: /import[^\r\n]*from\s+["']@?[A-Za-z][\w.-]*["']/.test(text),
    exportFn: /export\s+(?:async\s+)?function\s|export\s+const\s+[\w$]+\s*=\s*(?:async\s*)?\(/.test(
      text,
    ),
    exportType: /export\s+(?:type|interface|enum)\s/.test(text),
    classDecl: /\bclass\s+[A-Z]/.test(text),
    genericDecl: /(?:function|type|interface|class)\s+[\w$]+\s*<[A-Z]/.test(text),
    jsx: relativePath.endsWith(".tsx"),
    memberAccess: /(?<![\w$.'"`])[a-zA-Z_$][\w$]*\.[a-zA-Z_$][\w$]*/.test(text),
    large: Buffer.byteLength(text, "utf8") > LARGE_FILE_BYTES,
  };
  const score = Object.values(features).filter(Boolean).length;
  return { relativePath, features, score, bytes: Buffer.byteLength(text, "utf8") };
}

/** Enumerate + read + profile the whole corpus (read-only). */
export function profileNativeTsCorpus(corpusDir: string): NativeTsFileProfile[] {
  return enumerateCorpusTsFiles(corpusDir).map((relativePath) =>
    profileNativeTsFile(relativePath, readFileSync(path.join(corpusDir, relativePath), "utf8")),
  );
}

/** Fixed bucket order — part of the deterministic-selection contract. */
const BUCKET_ORDER: readonly (keyof NativeTsFileFeatures)[] = [
  "jsx",
  "genericDecl",
  "classDecl",
  "exportType",
  "exportFn",
  "deepImport",
  "barrelImport",
  "memberAccess",
  "large",
  "imports",
];

function byScoreThenPath(left: NativeTsFileProfile, right: NativeTsFileProfile): number {
  if (left.score !== right.score) return right.score - left.score;
  return left.relativePath < right.relativePath
    ? -1
    : left.relativePath > right.relativePath
      ? 1
      : 0;
}

/**
 * Deterministically select up to `n` representative profiles (bucket
 * round-robin then global score fill), output in lexicographic path order.
 */
export function selectNativeTsSample(
  profiles: readonly NativeTsFileProfile[],
  n: number,
): NativeTsFileProfile[] {
  if (n <= 0) return [];
  const buckets = new Map<keyof NativeTsFileFeatures, NativeTsFileProfile[]>();
  for (const bucket of BUCKET_ORDER) {
    buckets.set(
      bucket,
      profiles.filter((profile) => profile.features[bucket]).sort(byScoreThenPath),
    );
  }
  const selected: NativeTsFileProfile[] = [];
  const taken = new Set<string>();
  const take = (profile: NativeTsFileProfile): void => {
    if (taken.has(profile.relativePath)) return;
    taken.add(profile.relativePath);
    selected.push(profile);
  };

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
  if (selected.length < n) {
    for (const profile of [...profiles].sort(byScoreThenPath)) {
      if (selected.length >= n) break;
      take(profile);
    }
  }
  return selected.sort((left, right) =>
    left.relativePath < right.relativePath ? -1 : left.relativePath > right.relativePath ? 1 : 0,
  );
}
