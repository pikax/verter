#!/usr/bin/env node
/**
 * Deterministic generator for the synthetic-15k perf corpus.
 *
 * Materializes a hermetic, version-pinned corpus of TypeScript-typed Vue
 * SFCs that exercises the external-TS-engine workloads: cross-file
 * prop/emits type edges, a `composite` project-reference closure, a
 * `paths`/`baseUrl` alias layer, and a `lib` floor (explicit empty
 * `types: []` — no ambient `@types`, by design). The corpus
 * is deliberately COMPARABLE in scale to a generic SFC bench (file count,
 * module count, imports/SFC) but is independent Verter-owned work — it is
 * NOT a copy of any third-party generator, and it carries real TS types so
 * a carrier typecheck has something to check.
 *
 * Scope: Vue `.vue` SFCs ONLY (Block-6). The perf gate makes no claim of current
 * Svelte perf coverage; a Svelte (`.svelte`) corpus + carrier-extension discovery
 * is deferred to B8 (Svelte LSP/IDE) — see the baseline manifest `deferred` and
 * design §2.7/§2.7.1.
 *
 * The corpus is generate-on-demand (not committed) because the
 * materialized 15k-SFC tree is large; what is committed is the manifest
 * (`manifest.json`) recording the generator version, seed, config, and a
 * content hash over normalized relative paths + file bytes. A run that
 * regenerates the corpus verifies the produced bytes hash to the committed
 * manifest before any measurement, so a corpus drift can never silently
 * read as a perf improvement.
 *
 * Usage:
 *   node generate.mjs [--out <dir>] [--count <n>] [--modules <n>] \
 *                     [--imports <n>] [--seed <n>] [--hash-only] [--quiet]
 *
 * Exit codes:
 *   0  corpus materialized (or, with --hash-only, hash computed)
 *   1  invalid argument / IO failure
 */
import { writeFileSync, mkdirSync, rmSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// ── Generator identity (bump on any shape change) ──────────────────────────
// The generator version is part of the corpus identity; a change to the
// emitted shape MUST bump this so a stale manifest is detectable.
export const GENERATOR_VERSION = "1.1.0";

// ── Default configuration (the §2.7 corpus shape) ──────────────────────────
export const DEFAULT_CONFIG = Object.freeze({
  /** Total SFC count. */
  fileCount: 15000,
  /** Number of modules (directories) the SFCs are partitioned across. */
  moduleCount: 600,
  /** Mean cross-file type imports per SFC. */
  importsPerFile: 8,
  /** Deterministic PRNG seed. */
  seed: 0x5e_ed_15,
  /**
   * Number of leaf "kernel" modules placed in a separate `composite`
   * project-reference closure (so the redirect-ON / project-reference
   * boundary is exercised). The remaining modules form the app project
   * that references the kernel via `references`.
   */
  compositeModuleCount: 40,
});

// ── Deterministic PRNG (mulberry32 — small, fast, fully reproducible) ──────
// A seeded PRNG keeps the corpus byte-identical across runs and machines.
export function mulberry32(seed) {
  let a = seed >>> 0;
  return function next() {
    a |= 0;
    a = (a + 0x6d_2b_79_f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

// ── Naming helpers ─────────────────────────────────────────────────────────
// All generated names are lowercase-with-hyphens or PascalCase ASCII — no
// NTFS-illegal characters, no reserved device basenames, no case-collisions
// (every basename is unique within its directory by construction). This
// keeps `tracked_paths_are_portable` trivially green even though the corpus
// itself is not tracked.
const pad = (n, width) => String(n).padStart(width, "0");
const moduleDir = (m) => `m${pad(m, 4)}`;
const compModuleDir = (m) => `k${pad(m, 4)}`;
const sfcBase = (m, i) => `Comp${pad(m, 4)}_${pad(i, 3)}`;
const typesBase = (m) => `types`;

// Cross-platform emission helpers. Generated relative paths and file CONTENT
// MUST be byte-identical regardless of the generating OS, so the content hash
// is platform-independent (a Windows dev refreshing the manifest must produce
// the SAME hash Linux CI verifies). NEVER use `path.join` for an emitted
// relPath or for a path embedded in file content — it yields backslashes on
// Windows. Use `posixJoin` for relPaths/embedded paths and `lf` to force LF
// line endings (so a CRLF checkout of this generator cannot change the bytes).
const posixJoin = (...parts) => parts.filter((p) => p !== "").join("/");
const lf = (text) => text.replace(/\r\n/g, "\n");

/**
 * Compute the full corpus file plan as an ordered list of
 * `{ relPath, bytes }` entries. Deterministic given the config. Building
 * the plan separately from writing lets the content hash be computed
 * WITHOUT touching disk (`--hash-only`).
 */
export function buildCorpus(config = DEFAULT_CONFIG) {
  const cfg = { ...DEFAULT_CONFIG, ...config };
  const { fileCount, moduleCount, importsPerFile, seed, compositeModuleCount } = cfg;

  if (compositeModuleCount >= moduleCount) {
    throw new Error("compositeModuleCount must be < moduleCount");
  }

  const rng = mulberry32(seed);
  const files = [];
  const add = (relPath, bytes) => files.push({ relPath, bytes });

  // Partition the SFCs across modules as evenly as possible. The first
  // `compositeModuleCount` modules are the `kernel` project (composite);
  // the rest are the `app` project that references the kernel.
  const perModule = Math.floor(fileCount / moduleCount);
  const remainder = fileCount % moduleCount;
  const moduleSfcCount = [];
  for (let m = 0; m < moduleCount; m++) {
    moduleSfcCount.push(perModule + (m < remainder ? 1 : 0));
  }

  const isComposite = (m) => m < compositeModuleCount;
  const dirOf = (m) =>
    posixJoin(isComposite(m) ? "kernel" : "app", isComposite(m) ? compModuleDir(m) : moduleDir(m));

  // ── Per-module shared type file (the cross-file type edge target) ──────
  // Each module exports a props interface and an emits type that sibling
  // and downstream SFCs import — this is the cross-file prop/emits edge.
  for (let m = 0; m < moduleCount; m++) {
    const dir = dirOf(m);
    const propsName = `Props${pad(m, 4)}`;
    const emitsName = `Emits${pad(m, 4)}`;
    const modelName = `Model${pad(m, 4)}`;
    const body = `// Shared types for ${dir} (generator v${GENERATOR_VERSION})
export interface ${propsName} {
  id: number;
  label: string;
  count: number;
  active: boolean;
  tags: readonly string[];
  meta: { readonly key: string; readonly weight: number };
}

export type ${emitsName} = {
  change: [value: number];
  select: [item: ${propsName}];
  toggle: [];
};

export interface ${modelName} {
  value: string;
  dirty: boolean;
}

export type Pick${pad(m, 4)} = Pick<${propsName}, "id" | "label">;
`;
    add(posixJoin(dir, `${typesBase(m)}.ts`), Buffer.from(lf(body), "utf-8"));
  }

  // ── The SFCs ────────────────────────────────────────────────────────────
  let emitted = 0;
  for (let m = 0; m < moduleCount; m++) {
    const count = moduleSfcCount[m];
    const dir = dirOf(m);
    for (let i = 0; i < count; i++) {
      // Choose `importsPerFile` import targets. Targets are other modules'
      // type files; an `app` SFC may import a `kernel` module (the
      // cross-project-reference edge), a `kernel` SFC imports only kernel.
      const targets = new Set();
      const lowerBound = isComposite(m) ? 0 : 0;
      const upperBound = isComposite(m) ? compositeModuleCount : moduleCount;
      let guard = 0;
      while (targets.size < importsPerFile && guard < importsPerFile * 8) {
        guard++;
        const t = lowerBound + Math.floor(rng() * (upperBound - lowerBound));
        if (t === m) continue;
        targets.add(t);
      }
      const targetList = [...targets].sort((a, b) => a - b);
      add(
        posixJoin(dir, `${sfcBase(m, i)}.vue`),
        Buffer.from(lf(renderSfc(m, i, targetList, dirOf, isComposite)), "utf-8"),
      );
      emitted++;
    }
  }

  if (emitted !== fileCount) {
    throw new Error(`internal: emitted ${emitted} SFCs, expected ${fileCount}`);
  }

  // ── tsconfig topology ─────────────────────────────────────────────────
  // One solution-style root tsconfig with project references; a kernel
  // `composite` project; an app project that references the kernel and
  // declares the `paths`/`baseUrl` alias layer + the lib/@types floor.
  addTsconfigs(add, cfg);

  // Sort deterministically by normalized relative path so the content hash
  // is independent of emission order.
  files.sort((a, b) => (normRel(a.relPath) < normRel(b.relPath) ? -1 : 1));
  return { config: cfg, files };
}

/** Render one typed SFC importing the chosen modules' types. */
function renderSfc(m, i, targetList, dirOf, isComposite) {
  const selfProps = `Props${pad(m, 4)}`;
  const selfEmits = `Emits${pad(m, 4)}`;
  const importerIsComposite = isComposite(m);
  // The SFC's own module exports the props/emits it extends — import them
  // from the sibling `./types` so the carrier typechecks cleanly under
  // `strict` (an unimported `extends Props####` would be TS2304).
  const selfImport = `import type { ${selfProps}, ${selfEmits} } from "./types";`;
  const imports = targetList
    .map((t) => {
      const alias = `T${pad(t, 4)}`;
      const targetIsKernel = dirOf(t).startsWith("kernel");
      // The `@kernel/*` path alias is declared ONLY in the app project, so it
      // applies ONLY to an app→kernel cross-project edge. Every same-project
      // import (kernel→kernel, app→app) uses a relative specifier — exercising
      // both the alias-layer resolution and ordinary relative resolution.
      const useKernelAlias = !importerIsComposite && targetIsKernel;
      const spec = useKernelAlias
        ? `@kernel/${dirOf(t).split(/[\\/]/).pop()}/types`
        : relSpec(dirOf(m), dirOf(t));
      return `import type { Props${pad(t, 4)} as ${alias} } from "${spec}";`;
    })
    .join("\n");

  // The template references the imported types through computed props so
  // tsgo's carrier typecheck actually checks the cross-file edges.
  const usages = targetList
    .map((t, idx) => `  const ref${idx}: T${pad(t, 4)} = props.nested${idx} ?? defaults${idx};`)
    .join("\n");
  const defaultDecls = targetList
    .map(
      (t, idx) =>
        `const defaults${idx}: T${pad(t, 4)} = { id: ${t}, label: "${pad(t, 4)}", count: 0, active: false, tags: [], meta: { key: "k", weight: 1 } };`,
    )
    .join("\n");
  const nestedProps = targetList.map((t, idx) => `  nested${idx}?: T${pad(t, 4)};`).join("\n");
  // The trailing `ref0.id` term references the FIRST imported-type usage, which is
  // declared ONLY when this SFC has >=1 import target. A degenerate config that
  // satisfies no import target (e.g. importsPerFile 0, or too few modules) would
  // otherwise reference an undeclared `ref0`; drop the term so EVERY generated SFC
  // is well-formed for ANY config. The pinned corpus (importsPerFile 8) always has
  // targets, so `returnExpr` is `props.id + ref0.id` there and the bytes are unchanged.
  const returnExpr = targetList.length > 0 ? "props.id + ref0.id" : "props.id";

  return `<script setup lang="ts">
${selfImport}
${imports}

interface LocalProps extends ${selfProps} {
${nestedProps}
}

const props = defineProps<LocalProps>();
const emit = defineEmits<${selfEmits}>();

${defaultDecls}

function recompute(): number {
${usages}
  emit("change", props.count + ${i});
  return ${returnExpr};
}
</script>

<template>
  <div :class="{ active: props.active }" :data-id="props.id" @click="recompute()">
    <span>{{ props.label }}</span>
    <ul>
      <li v-for="(t, idx) in props.tags" :key="idx">{{ t }}</li>
    </ul>
    <em v-if="props.count > 0">{{ props.count }}</em>
  </div>
</template>
`;
}

/** A relative module specifier from one corpus dir to another's \`types\`. */
function relSpec(fromDir, toDir) {
  const from = normRel(fromDir).split("/");
  const to = normRel(toDir).split("/");
  // Both are 2-segment (project/module); compute a relative climb.
  let common = 0;
  while (common < from.length && common < to.length && from[common] === to[common]) common++;
  const up = from.length - common;
  const climb = up === 0 ? "./" : "../".repeat(up);
  const down = to.slice(common).join("/");
  const prefix = down ? `${climb}${down}/` : climb;
  return `${prefix}types`.replace(/\/+/g, "/");
}

function addTsconfigs(add, cfg) {
  const lib = ["ES2022", "DOM", "DOM.Iterable"];
  // Root solution tsconfig — references both sub-projects.
  add(
    "tsconfig.json",
    Buffer.from(
      JSON.stringify(
        {
          files: [],
          references: [{ path: "./kernel" }, { path: "./app" }],
        },
        null,
        2,
      ) + "\n",
      "utf-8",
    ),
  );

  // Kernel project — composite, the project-reference closure leaf.
  add(
    posixJoin("kernel", "tsconfig.json"),
    Buffer.from(
      JSON.stringify(
        {
          compilerOptions: {
            composite: true,
            declaration: true,
            rootDir: ".",
            outDir: "../.out/kernel",
            module: "ESNext",
            moduleResolution: "Bundler",
            target: "ES2022",
            lib,
            strict: true,
            types: [],
            skipLibCheck: true,
          },
          include: ["**/*.ts", "**/*.vue"],
        },
        null,
        2,
      ) + "\n",
      "utf-8",
    ),
  );

  // App project — references the composite kernel, declares the
  // `paths`/`baseUrl` alias layer (`@kernel/*`, `@app/*`) and the lib floor.
  add(
    posixJoin("app", "tsconfig.json"),
    Buffer.from(
      JSON.stringify(
        {
          compilerOptions: {
            composite: true,
            declaration: true,
            rootDir: ".",
            outDir: "../.out/app",
            module: "ESNext",
            moduleResolution: "Bundler",
            target: "ES2022",
            lib,
            strict: true,
            baseUrl: ".",
            paths: {
              "@kernel/*": ["../kernel/*"],
              "@app/*": ["./*"],
            },
            types: [],
            skipLibCheck: true,
          },
          references: [{ path: "../kernel" }],
          include: ["**/*.ts", "**/*.vue"],
        },
        null,
        2,
      ) + "\n",
      "utf-8",
    ),
  );
}

// ── Content hash (over normalized relative paths + file bytes) ─────────────
/** Normalize a relative path to forward slashes for cross-platform hashing. */
export function normRel(relPath) {
  return relPath.split(/[\\/]/).join("/");
}

/**
 * Hash the corpus as `sha256` over, for each file in sorted normalized-path
 * order: the normalized relative path (utf-8), a NUL separator, the byte
 * length (as a decimal string), a NUL, then the file bytes, then a record
 * separator. Path normalization + sorting make the hash identical on every
 * OS regardless of path separators or directory-walk order.
 */
export function hashCorpus(files) {
  const sorted = [...files].sort((a, b) => (normRel(a.relPath) < normRel(b.relPath) ? -1 : 1));
  const h = createHash("sha256");
  for (const f of sorted) {
    h.update(normRel(f.relPath), "utf-8");
    h.update(Buffer.from([0]));
    h.update(String(f.bytes.length), "utf-8");
    h.update(Buffer.from([0]));
    h.update(f.bytes);
    h.update(Buffer.from([0x1e]));
  }
  return `sha256:${h.digest("hex")}`;
}

/** Build the manifest object recording identity + content hash. */
export function buildManifest(corpus) {
  const { config, files } = corpus;
  const sfcCount = files.filter((f) => f.relPath.endsWith(".vue")).length;
  const typeCount = files.filter((f) => f.relPath.endsWith(".ts")).length;
  const tsconfigCount = files.filter((f) => f.relPath.endsWith("tsconfig.json")).length;
  const totalBytes = files.reduce((s, f) => s + f.bytes.length, 0);
  return {
    generatorVersion: GENERATOR_VERSION,
    seed: config.seed,
    config: {
      fileCount: config.fileCount,
      moduleCount: config.moduleCount,
      importsPerFile: config.importsPerFile,
      compositeModuleCount: config.compositeModuleCount,
    },
    topology: {
      projects: ["kernel (composite)", "app (references kernel)"],
      projectReferences: "root solution tsconfig references kernel + app; app references kernel",
      aliasLayer: { baseUrl: "app/.", paths: ["@kernel/*", "@app/*"] },
      libFloor: ["ES2022", "DOM", "DOM.Iterable"],
      typesFloor: "explicit empty `types: []` (no ambient @types beyond lib)",
    },
    counts: {
      sfc: sfcCount,
      types: typeCount,
      tsconfig: tsconfigCount,
      totalFiles: files.length,
      totalBytes,
    },
    contentHash: hashCorpus(files),
  };
}

// ── CLI ────────────────────────────────────────────────────────────────────
function parseArgs(argv) {
  const out = { hashOnly: false, quiet: false };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--hash-only") out.hashOnly = true;
    else if (a === "--quiet") out.quiet = true;
    else if (a === "--out") out.out = argv[++i];
    else if (a === "--count") out.fileCount = Number(argv[++i]);
    else if (a === "--modules") out.moduleCount = Number(argv[++i]);
    else if (a === "--imports") out.importsPerFile = Number(argv[++i]);
    else if (a === "--seed") out.seed = Number(argv[++i]);
    else if (a === "--composite") out.compositeModuleCount = Number(argv[++i]);
    else {
      console.error(`Unknown argument: ${a}`);
      process.exit(1);
    }
  }
  return out;
}

function materialize(outDir, files, quiet) {
  if (existsSync(outDir)) rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
  let written = 0;
  for (const f of files) {
    const dest = join(outDir, f.relPath);
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, f.bytes);
    written++;
    if (!quiet && written % 2000 === 0) {
      process.stderr.write(`  …${written}/${files.length} files\n`);
    }
  }
  return written;
}

function main() {
  const args = parseArgs(process.argv);
  const config = {
    ...DEFAULT_CONFIG,
    ...(args.fileCount !== undefined ? { fileCount: args.fileCount } : {}),
    ...(args.moduleCount !== undefined ? { moduleCount: args.moduleCount } : {}),
    ...(args.importsPerFile !== undefined ? { importsPerFile: args.importsPerFile } : {}),
    ...(args.seed !== undefined ? { seed: args.seed } : {}),
    ...(args.compositeModuleCount !== undefined
      ? { compositeModuleCount: args.compositeModuleCount }
      : {}),
  };

  const corpus = buildCorpus(config);
  const manifest = buildManifest(corpus);

  if (args.hashOnly) {
    process.stdout.write(`${manifest.contentHash}\n`);
    return;
  }

  const outDir = args.out ?? join(__dirname, "..", "corpus");
  if (!args.quiet) {
    process.stderr.write(
      `Generating synthetic-15k corpus → ${outDir}\n` +
        `  generator v${manifest.generatorVersion}, seed ${manifest.seed}\n` +
        `  ${manifest.counts.sfc} SFCs, ${manifest.counts.types} type files, ` +
        `${manifest.counts.tsconfig} tsconfigs (${manifest.counts.totalFiles} files, ` +
        `${(manifest.counts.totalBytes / (1024 * 1024)).toFixed(1)} MB)\n`,
    );
  }
  const written = materialize(outDir, corpus.files, args.quiet);
  if (!args.quiet) {
    process.stderr.write(
      `Done — ${written} files written.\n  content hash: ${manifest.contentHash}\n`,
    );
  }
}

// Run `main()` only when invoked directly as a script — never when this
// module is imported for the hash check or by the runner. A basename match
// on `process.argv[1]` is robust across Windows drive-letter casing, where
// a full `fileURLToPath(import.meta.url) === argv[1]` comparison is brittle.
const invokedDirectly =
  process.argv[1] !== undefined &&
  process.argv[1].replace(/\\/g, "/").endsWith("generator/generate.mjs");
if (invokedDirectly) {
  main();
}
