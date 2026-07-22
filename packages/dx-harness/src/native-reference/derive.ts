/**
 * Derived plain-TypeScript analogue generation for the native reference lane.
 *
 * THE NORTH-STAR METRIC is "overhead over native TypeScript, per operation",
 * so the reference program must make TypeScript do the SAME type-resolution
 * work the `.vue` path induces — not the work of some unrelated `.ts` file
 * that happens to exist. This module derives, mechanically and at RUN TIME,
 * one plain `.ts` analogue per sampled corpus SFC:
 *
 *  1. **Mirror workspace.** The corpus tree is mirrored into a temp directory:
 *     source/config files are COPIED (so the mirror is a real configured
 *     project with the same tsconfig chain, path aliases, ambient shims and
 *     sibling modules), and every `node_modules` directory is JUNCTIONED (so
 *     dependency resolution hits the identical installed packages). The real
 *     corpus is never written to; the mirror lives outside the repository and
 *     is never committed.
 *
 *  2. **Script extraction + macro lowering.** Each sampled SFC's script
 *     content becomes `<Name>__nref.ts` beside its mirrored `.vue`: a plain
 *     `<script>` block is kept verbatim (it already is plain TS), and a
 *     `<script setup>` block is appended with compiler macros lowered to
 *     plain-TS equivalents that force the same type resolution:
 *       - `defineProps<T>()`            → `({} as T)`
 *       - `withDefaults(expr, D)`       → `Object.assign(expr, D)`
 *       - `defineEmits<T>()`            → `({} as T)`
 *       - `defineSlots<T>()`            → `({} as T)`
 *       - `defineModel<T>(…)`           → `(null as unknown as { value: T })`
 *       - `defineExpose(X)` / `defineOptions(X)` → `void (X)`
 *       - runtime-object forms keep their object/array literal expression.
 *     A generic SFC (`<script setup generic="…">`) is wrapped best-effort in
 *     `export function __nref<G>() { … }` with inner `export` markers
 *     stripped (imports stay top-level).
 *
 *  3. **Declared limitations (no honest plain-TS equivalent).** The template
 *     is DROPPED — template-expression checking is Vue-specific work with no
 *     plain-TS counterpart. `import X from './X.vue'` statements are KEPT and
 *     resolve through the corpus's own ambient `*.vue` shim, exactly as any
 *     plain `.ts` file in that project would — the deep per-component
 *     type-surface Verter materialises for such imports is Vue-specific work
 *     the reference deliberately does not emulate. Both limitations are
 *     reported, never silently dropped.
 */
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

/** File extensions copied into the mirror (resolution-relevant text files). */
const COPY_EXTENSIONS = new Set([
  ".ts",
  ".tsx",
  ".mts",
  ".cts",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".json",
  ".vue",
  ".svelte",
  ".css",
  ".scss",
  ".less",
]);
const SKIPPED_DIRECTORIES = new Set([".git", "node_modules"]);
const MAX_COPY_BYTES = 4 * 1024 * 1024;

export interface MirrorStats {
  copiedFiles: number;
  junctionedNodeModules: number;
  skippedLargeFiles: number;
}

/**
 * Mirror `corpusDir` into `mirrorRoot`: copy resolution-relevant files,
 * junction every `node_modules`. Read-only against the corpus. Idempotent —
 * an existing identical mirror entry is overwritten, junctions are reused.
 */
export function mirrorCorpusWorkspace(corpusDir: string, mirrorRoot: string): MirrorStats {
  const stats: MirrorStats = { copiedFiles: 0, junctionedNodeModules: 0, skippedLargeFiles: 0 };
  const visit = (dir: string, mirrorDir: string): void => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    mkdirSync(mirrorDir, { recursive: true });
    for (const entry of entries) {
      const absolute = path.join(dir, entry.name);
      const mirrored = path.join(mirrorDir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "node_modules") {
          // Junction to the REAL installed dependencies (no copy, no admin
          // rights needed). lstat-reported as a symlink, so cleanup can
          // unlink it without ever recursing into the target.
          if (!existsSync(mirrored)) {
            try {
              symlinkSync(absolute, mirrored, "junction");
              stats.junctionedNodeModules += 1;
            } catch {
              // A mirror without deps resolves less — visible in results.
            }
          } else {
            stats.junctionedNodeModules += 1;
          }
          continue;
        }
        if (!SKIPPED_DIRECTORIES.has(entry.name) && !entry.name.startsWith(".")) {
          visit(absolute, mirrored);
        }
      } else if (entry.isFile()) {
        const ext = path.extname(entry.name);
        const isConfigish =
          COPY_EXTENSIONS.has(ext) ||
          entry.name === "package.json" ||
          entry.name.startsWith("tsconfig") ||
          entry.name.endsWith(".d.ts");
        if (!isConfigish) continue;
        try {
          if (lstatSync(absolute).size > MAX_COPY_BYTES) {
            stats.skippedLargeFiles += 1;
            continue;
          }
          copyFileSync(absolute, mirrored);
          stats.copiedFiles += 1;
        } catch {
          // Unreadable source file: the mirror simply lacks it.
        }
      }
    }
  };
  visit(corpusDir, mirrorRoot);
  return stats;
}

/** Tallies of every lowering / limitation applied during derivation. */
export interface DerivationTallies {
  definePropsType: number;
  definePropsRuntime: number;
  withDefaults: number;
  defineEmits: number;
  defineSlots: number;
  defineModel: number;
  defineExpose: number;
  defineOptions: number;
  genericWrapped: number;
  plainScriptBlocks: number;
  setupScriptBlocks: number;
  templateDropped: number;
  vueImportsKept: number;
}

export function emptyTallies(): DerivationTallies {
  return {
    definePropsType: 0,
    definePropsRuntime: 0,
    withDefaults: 0,
    defineEmits: 0,
    defineSlots: 0,
    defineModel: 0,
    defineExpose: 0,
    defineOptions: 0,
    genericWrapped: 0,
    plainScriptBlocks: 0,
    setupScriptBlocks: 0,
    templateDropped: 0,
    vueImportsKept: 0,
  };
}

export type DeriveOutcome =
  | { readonly kind: "derived"; readonly text: string }
  | { readonly kind: "skipped"; readonly reason: "no-script" | "non-ts-script" };

interface ScriptBlock {
  readonly attrs: string;
  readonly content: string;
  readonly setup: boolean;
}

function extractScriptBlocks(vueText: string): ScriptBlock[] {
  const blocks: ScriptBlock[] = [];
  const re = /<script\b([^>]*)>([\s\S]*?)<\/script>/gi;
  let m: RegExpExecArray | null;
  while ((m = re.exec(vueText))) {
    blocks.push({ attrs: m[1], content: m[2], setup: /\bsetup\b/.test(m[1]) });
  }
  return blocks;
}

/** Lower Vue compiler macros in `<script setup>` source to plain TS (text-shape). */
function lowerMacros(source: string, tallies: DerivationTallies): string {
  let out = source;
  // withDefaults FIRST so the inner defineProps lowering composes under it.
  out = out.replace(/\bwithDefaults\s*\(/g, () => {
    tallies.withDefaults += 1;
    return "Object.assign(";
  });
  // Type-argument macro forms: force the SAME generic resolution.
  out = out.replace(/\bdefineProps\s*(<[\s\S]*?>)\s*\(\s*\)/g, (_all, generic: string) => {
    tallies.definePropsType += 1;
    return `({} as ${generic.slice(1, -1)})`;
  });
  out = out.replace(/\bdefineEmits\s*(<[\s\S]*?>)\s*\(\s*\)/g, (_all, generic: string) => {
    tallies.defineEmits += 1;
    return `({} as ${generic.slice(1, -1)})`;
  });
  out = out.replace(/\bdefineSlots\s*(<[\s\S]*?>)\s*\(\s*\)/g, (_all, generic: string) => {
    tallies.defineSlots += 1;
    return `({} as ${generic.slice(1, -1)})`;
  });
  out = out.replace(/\bdefineModel\s*(<[\s\S]*?>)?\s*\(/g, (_all, generic: string | undefined) => {
    tallies.defineModel += 1;
    const t = generic ? generic.slice(1, -1) : "unknown";
    return `(null as unknown as { value: ${t} }); void (`;
  });
  // Runtime-object forms: keep the literal so its shape is still checked.
  out = out.replace(/\bdefineProps\s*\(/g, () => {
    tallies.definePropsRuntime += 1;
    return "(";
  });
  out = out.replace(/\bdefineEmits\s*\(/g, "(");
  out = out.replace(/\bdefineExpose\s*\(/g, () => {
    tallies.defineExpose += 1;
    return "void (";
  });
  out = out.replace(/\bdefineOptions\s*\(/g, () => {
    tallies.defineOptions += 1;
    return "void (";
  });
  return out;
}

/** Split top-level import statements from the rest (line-shape, for wrapping). */
function splitImports(source: string): { imports: string; body: string } {
  const importLines: string[] = [];
  const bodyLines: string[] = [];
  let inImport = false;
  for (const line of source.split(/\r\n|\n/)) {
    if (inImport) {
      importLines.push(line);
      if (/["'][^"']*["']\s*;?\s*$/.test(line)) inImport = false;
      continue;
    }
    if (/^\s*import\b/.test(line)) {
      importLines.push(line);
      // A multi-line import keeps collecting until the specifier line.
      inImport = !/["'][^"']*["']\s*;?\s*$/.test(line);
    } else {
      bodyLines.push(line);
    }
  }
  return { imports: importLines.join("\n"), body: bodyLines.join("\n") };
}

/**
 * Derive one SFC's plain-TS analogue text. Deterministic, text-shape only —
 * the honest mechanical translation, with every lowering tallied.
 */
export function deriveVueAnalogue(vueText: string, tallies: DerivationTallies): DeriveOutcome {
  const blocks = extractScriptBlocks(vueText);
  if (blocks.length === 0) return { kind: "skipped", reason: "no-script" };
  const nonTs = blocks.some(
    (b) => /\blang\s*=\s*["']/.test(b.attrs) && !/\blang\s*=\s*["']tsx?["']/.test(b.attrs),
  );
  if (nonTs) return { kind: "skipped", reason: "non-ts-script" };
  if (/<template\b/.test(vueText)) tallies.templateDropped += 1;
  tallies.vueImportsKept += (vueText.match(/from\s+["'][^"']+\.vue["']/g) ?? []).length;

  const parts: string[] = [];
  for (const block of blocks) {
    if (!block.setup) {
      tallies.plainScriptBlocks += 1;
      parts.push(block.content);
      continue;
    }
    tallies.setupScriptBlocks += 1;
    const lowered = lowerMacros(block.content, tallies);
    const genericMatch = block.attrs.match(/\bgeneric\s*=\s*"([^"]+)"|\bgeneric\s*=\s*'([^']+)'/);
    if (genericMatch) {
      tallies.genericWrapped += 1;
      const generic = genericMatch[1] ?? genericMatch[2];
      const { imports, body } = splitImports(lowered);
      const innerBody = body.replace(/^(\s*)export\s+/gm, "$1");
      parts.push(`${imports}\nexport function __nref<${generic}>() {\n${innerBody}\n}`);
    } else {
      parts.push(lowered);
    }
  }
  parts.push("export {};\n");
  return { kind: "derived", text: parts.join("\n") };
}

/** The derived analogue's mirror-relative path for a sampled `.vue` path. */
export function derivedRelativePath(vueRelativePath: string): string {
  return vueRelativePath.replace(/\.vue$/, "__nref.ts");
}

export interface DeriveSampleResult {
  readonly derivedRelativePaths: string[];
  readonly skipped: { readonly noScript: number; readonly nonTsScript: number };
  readonly tallies: DerivationTallies;
}

/**
 * Derive analogues for every sampled `.vue` into the mirror, returning the
 * mirror-relative derived paths in the sample's order.
 */
export function deriveSampleIntoMirror(
  corpusDir: string,
  mirrorRoot: string,
  vueRelativePaths: readonly string[],
): DeriveSampleResult {
  const tallies = emptyTallies();
  const derivedRelativePaths: string[] = [];
  let noScript = 0;
  let nonTsScript = 0;
  for (const relativePath of vueRelativePaths) {
    let vueText: string;
    try {
      vueText = readFileSync(path.join(corpusDir, relativePath), "utf8");
    } catch {
      noScript += 1;
      continue;
    }
    const outcome = deriveVueAnalogue(vueText, tallies);
    if (outcome.kind === "skipped") {
      if (outcome.reason === "no-script") noScript += 1;
      else nonTsScript += 1;
      continue;
    }
    const derivedRelative = derivedRelativePath(relativePath);
    const absolute = path.join(mirrorRoot, derivedRelative);
    mkdirSync(path.dirname(absolute), { recursive: true });
    writeFileSync(absolute, outcome.text);
    derivedRelativePaths.push(derivedRelative.replaceAll("\\", "/"));
  }
  return { derivedRelativePaths, skipped: { noScript, nonTsScript }, tallies };
}
