// Static architecture guard (TS mirror of the Rust `carrier_routing_no_vue_gate`
// guards): the `@verter/typescript-plugin` source must classify and route
// framework-carrier files carrier-GENERICALLY, never via a hardcoded Vue-only
// `.vue`-suffix / `"vue"`-language-id GATE over carrier-NEUTRAL data.
//
// Verter is one shared, carrier-generic substrate. A `.vue` SFC and a `.svelte`
// component are both framework CARRIERS. The carrier-generic predicates ALREADY
// EXIST and are the SANCTIONED replacement:
//   - `isVue(fileName)` / `carrierFor(...)` (manifest-derived, in the
//     `@verter/language-shared` carrier naming CORE — backed by the generated
//     byte-pinned virtual-file-naming column mirror),
//   - the suffix helpers (`stripVueVirtualSuffix`, `getVueVirtualFileInfo`,
//     `cleanupCarrierVirtualImportPath`) which generalise over every carrier.
// A Vue-only `.endsWith(".vue")` path classifier or a `.vue.ts`/`.vue.d.ts`
// virtual-suffix literal used as a GATE silently strands `.svelte` below parity.
//
// This guard reads every `src/**/*.ts` in this package (skipping `*.spec.ts`
// and `*.test.ts`), strips comments, and FAILS on any literal carrier GATE
// outside a NARROW allowlist of files that legitimately carry carrier/Vue
// string data (the `verterTypesStub.ts` Vue-runtime type import; the carrier
// table itself lives in `@verter/language-shared`, outside this package).
// Calling `isVue(...)` / `carrierFor(...)` is NEVER flagged — the guard
// targets literal `.vue`/`"vue"` GATES, not the helper identifiers.

import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { tmpdir } from "node:os";

/** This package's `src` directory (the scan root). */
const SRC_ROOT = join(__dirname);

/**
 * Files that legitimately carry carrier/Vue string data and are excluded WHOLE
 * (relative to `src`, POSIX-normalised). These are NOT routing gates:
 *   - `helpers/verterTypesStub.ts` holds `import ... from "vue"` (the Vue
 *     runtime package type import).
 * The carrier suffix table (`isVue`/`carrierFor`/the manifest-derived
 * virtual-suffix regexes) and the byte-pinned naming-column mirror live in
 * `@verter/language-shared`, outside this package's scan root.
 */
const ALLOWLISTED_FILES = new Set<string>(["helpers/verterTypesStub.ts"]);

/** Whether a `src`-relative POSIX path is excluded from the scan. */
function isExcludedFile(relPosix: string): boolean {
  if (relPosix.endsWith(".spec.ts") || relPosix.endsWith(".test.ts")) return true;
  if (ALLOWLISTED_FILES.has(relPosix)) return true;
  return false;
}

/** Recursively collect non-excluded `.ts` files under `dir`. */
function collectTsFiles(dir: string): string[] {
  const out: string[] = [];
  const walk = (current: string): void => {
    for (const name of readdirSync(current)) {
      const abs = join(current, name);
      const relPosix = relative(SRC_ROOT, abs).split(sep).join("/");
      if (statSync(abs).isDirectory()) {
        walk(abs);
        continue;
      }
      if (!name.endsWith(".ts")) continue;
      if (isExcludedFile(relPosix)) continue;
      out.push(abs);
    }
  };
  walk(dir);
  out.sort();
  return out;
}

/**
 * Strip the comment portion of a single source line — line `//` comments and a
 * `/* … *\/` that opens-and-closes on the same line — while NOT treating a `//`
 * or `/*` inside a string/template literal as a comment. Multi-line block
 * comments are tracked by the caller's `inBlockComment` state. Returns only the
 * executable code prefix.
 */
function stripComment(code: string): string {
  let out = "";
  let i = 0;
  let inStr: string | null = null;
  while (i < code.length) {
    const ch = code[i];
    if (inStr !== null) {
      out += ch;
      if (ch === "\\" && i + 1 < code.length) {
        out += code[i + 1];
        i += 2;
        continue;
      }
      if (ch === inStr) inStr = null;
      i += 1;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      inStr = ch;
      out += ch;
      i += 1;
      continue;
    }
    if (ch === "/" && code[i + 1] === "/") break; // line comment
    if (ch === "/" && code[i + 1] === "*") {
      const end = code.indexOf("*/", i + 2);
      if (end !== -1) {
        i = end + 2;
        continue;
      }
      break; // opens here, closes on a later line — handled by caller state
    }
    out += ch;
    i += 1;
  }
  return out;
}

/**
 * Whether `line` leaves a block comment OPEN (a trailing `/*` with no matching
 * `*\/` after it on the same line). String- and line-comment-aware.
 */
function opensBlockComment(line: string): boolean {
  let i = 0;
  let inStr: string | null = null;
  while (i < line.length) {
    const ch = line[i];
    if (inStr !== null) {
      if (ch === "\\" && i + 1 < line.length) {
        i += 2;
        continue;
      }
      if (ch === inStr) inStr = null;
      i += 1;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      inStr = ch;
      i += 1;
      continue;
    }
    if (ch === "/" && line[i + 1] === "/") return false; // line comment
    if (ch === "/" && line[i + 1] === "*") {
      const close = line.indexOf("*/", i + 2);
      if (close !== -1) {
        i = close + 2;
        continue;
      }
      return true;
    }
    i += 1;
  }
  return false;
}

/**
 * The Vue-runtime npm-package import filter — `imp.source !== "vue" &&
 * !imp.source.startsWith("vue/")` — is a Vue-INTRINSIC gate (it keeps ONLY the
 * `vue` runtime package and its `vue/*` subpaths, e.g. lifecycle hooks /
 * composables). Its co-occurrence of `!== "vue"` with `startsWith("vue/")` is
 * structurally distinct from a `languageId` gate (a language-id check never
 * tests an npm subpath). This is the precise allowlist for those filter lines
 * WITHOUT excusing the whole file (so a real `languageId`/`.vue` gate elsewhere
 * in the same file still fails).
 */
function isVueRuntimeImportFilter(code: string): boolean {
  return (
    (code.includes('!== "vue"') || code.includes('!=="vue"')) &&
    (code.includes('startsWith("vue/")') || code.includes('.startsWith("vue/")'))
  );
}

/**
 * Whether the quoted literal `lit` (e.g. `.vue.ts`) is used as a CLASSIFIER /
 * comparison GATE on this line — the argument of `endsWith`/`startsWith`/
 * `includes`/`indexOf`, or one side of an `===`/`!==`. A bare display string
 * carrying the literal (a tree-item label) is NOT a gate and must not flag.
 */
function literalUsedAsGate(code: string, lit: string): boolean {
  const quoted = `"${lit}"`;
  if (!code.includes(quoted)) return false;
  for (const fn of ["endsWith(", "startsWith(", "includes(", "indexOf("]) {
    if (code.includes(`${fn}${quoted}`)) return true;
  }
  if (code.includes(`=== ${quoted}`) || code.includes(`!== ${quoted}`)) return true;
  if (code.includes(`${quoted} ===`) || code.includes(`${quoted} !==`)) return true;
  return false;
}

/**
 * Classify a single (comment-stripped) executable code line as a Vue-only
 * routing/classification GATE, returning a human-readable gate kind for the
 * failure diagnostic, or `null` when the line is carrier-generic / not a gate.
 *
 * Calling `isVue(...)` / `carrierFor(...)` / `isFrameworkCarrierLanguageId(...)`
 * is the SANCTIONED replacement and is never a gate. An `import ... from "vue"`
 * runtime import is not a gate either (the bare `"vue"` is an npm specifier, not
 * a language-id equality).
 */
function lineGateKind(code: string): string | null {
  // (0) Hardcoded carrier virtual-file suffix literals used as a GATE — the
  //     `.vue.ts` / `.vue.tsx` / `.vue.d.ts` / `.vue.__verter_test.ts`
  //     classifier-artifact paths. A bare display-label string ("API
  //     (d.vue.ts)") is NOT a gate — only a CLASSIFIER use is: the literal as
  //     the argument of `endsWith`/`startsWith`/`includes`/`indexOf`, or in an
  //     equality (`=== ".vue.ts"`), or inside a `/\.vue\.../` regex. These are
  //     exactly the per-carrier suffixes that must come from the manifest
  //     table, never a hardcoded `.vue` literal.
  for (const lit of [".vue.__verter_test", ".vue.d.ts", ".vue.ts", ".vue.tsx", ".vue.jsx"]) {
    if (literalUsedAsGate(code, lit)) {
      return `hardcoded carrier virtual-suffix classifier (${lit})`;
    }
  }
  // A `/\.vue\.../` regex literal (escaped-dot `\.vue\.` form) is the same
  // hardcoded virtual-suffix classifier in regex shape — always a gate.
  if (code.includes("\\.vue\\.")) {
    return "hardcoded carrier virtual-suffix regex literal (\\.vue\\.)";
  }

  // (1) `.endsWith(".vue")` / `.startsWith(".vue")` path-suffix CLASSIFIERS.
  for (const m of ['endsWith(".vue")', 'startsWith(".vue")']) {
    if (code.includes(m)) {
      return ".vue path-suffix classification gate (endsWith/startsWith)";
    }
  }

  // (2) `.vue` EQUALITY gate.
  if (code.includes('=== ".vue"') || code.includes('!== ".vue"')) {
    return ".vue equality gate (=== / !==)";
  }

  // (3) `"vue"`-as-language-id EQUALITY gate — EXCEPT the Vue-runtime npm
  //     import filter (`!== "vue" && startsWith("vue/")`), which is intrinsic.
  if (
    (code.includes('=== "vue"') || code.includes('!== "vue"')) &&
    !isVueRuntimeImportFilter(code)
  ) {
    return '"vue" language-id equality gate (=== / !==)';
  }

  return null;
}

/** One flagged violation: `src`-relative path, 1-based line, executable text. */
interface Violation {
  file: string;
  line: number;
  text: string;
}

/** Walk one file, tracking multi-line block comments, and flag every gate. */
function fileViolations(absPath: string): Violation[] {
  const src = readFileSync(absPath, "utf8");
  const rel = relative(SRC_ROOT, absPath).split(sep).join("/");
  const violations: Violation[] = [];
  let inBlockComment = false;

  const lines = src.split(/\r?\n/);
  for (let idx = 0; idx < lines.length; idx += 1) {
    let line = lines[idx];
    if (inBlockComment) {
      const end = line.indexOf("*/");
      if (end !== -1) {
        line = line.slice(end + 2);
        inBlockComment = false;
      } else {
        continue; // entire line inside a block comment
      }
    }

    const code = stripComment(line);
    if (opensBlockComment(line)) inBlockComment = true;

    const trimmed = code.trim();
    if (trimmed.length === 0) continue;

    const kind = lineGateKind(code);
    if (kind !== null) {
      violations.push({ file: rel, line: idx + 1, text: trimmed });
    }
  }
  return violations;
}

describe("carrier-routing guard: no hardcoded Vue gate in @verter/typescript-plugin", () => {
  it("the scan root resolves to real source files", () => {
    const files = collectTsFiles(SRC_ROOT);
    // Sanity: the scan must actually cover the package (catches a drifted root).
    expect(files.length).toBeGreaterThan(0);
    expect(files.some((f) => f.endsWith(join("src", "index.ts")))).toBe(true);
  });

  it('contains no hardcoded `.vue`/`"vue"` routing gate in carrier-neutral code', () => {
    const files = collectTsFiles(SRC_ROOT);
    const violations = files.flatMap(fileViolations);
    const rendered = violations.map((v) => `${v.file}:${v.line}: ${v.text}`).join("\n  ");
    expect(
      violations,
      `Carrier-routing guard violations: production typescript-plugin code uses a\n` +
        `Vue-only \`.vue\`/\`"vue"\` gate over carrier-NEUTRAL data. A \`.vue\` SFC and a\n` +
        `\`.svelte\` component are both framework CARRIERS — route through the\n` +
        `carrier-generic helpers instead:\n` +
        `  - \`isVue(fileName)\` / \`carrierFor(...)\` (manifest-derived, from\n` +
        `    \`@verter/language-shared\`),\n` +
        `  - \`stripVueVirtualSuffix\` / \`getVueVirtualFileInfo\` /\n` +
        `    \`cleanupCarrierVirtualImportPath\` for the carrier virtual suffixes.\n` +
        `Allowlisted ONLY: helpers/verterTypesStub.ts (the Vue-runtime import) and\n` +
        `the Vue-runtime npm import filter (\`!== "vue" && startsWith("vue/")\`).\n` +
        `(The carrier table + byte-pinned naming mirror live in\n` +
        `\`@verter/language-shared\`, outside this package.)\n\n` +
        `Violations:\n  ${rendered}`,
    ).toHaveLength(0);
  });

  // ──────────────────── discrimination self-tests ────────────────────
  // Prove the detector FAILS against the pre-change (`.vue`-gated) shapes and
  // PASSES against the carrier-generic post-change shapes.

  it("flags every pre-change violating shape (red proof)", () => {
    // The exact pre-fix executable shapes from the A3 carrier-NEUTRAL sites.
    const preChange = [
      'if (definition.fileName.endsWith(".vue")) {',
      'if (!literal.text.endsWith(".vue")) {',
      'if (normalized.endsWith(".vue.d.ts")) {',
      'if (normalized.endsWith(".vue.ts")) {',
      'return normalizePath(fileName).endsWith(".vue");',
      'return text.replace(/\\.vue\\.__verter_test\\.ts/g, ".vue").replace(/\\.vue\\.(d\\.)?ts/g, ".vue");',
      'if (document.languageId === "vue") {',
      'if (lang === "vue") {',
    ];
    for (const line of preChange) {
      expect(lineGateKind(stripComment(line)), `should flag: ${line}`).not.toBeNull();
    }
  });

  it("does not flag the carrier-generic post-change shapes (green proof)", () => {
    const postChange = [
      "if (isVue(definition.fileName)) {",
      "if (!isVue(literal.text)) {",
      "return stripVueVirtualSuffix(fileName);",
      "return cleanupCarrierVirtualImportPath(text);",
      // The `fileKind` hint is dropped; the host classifies by path. The bare
      // `host.upsert` with no `fileKind` carries no `.vue` literal.
      "host.upsert({ inputId: sourcePath, source: nextSource });",
      "if (isFrameworkCarrierLanguageId(document.languageId)) {",
      "const c = carrierFor(normalized);",
    ];
    for (const line of postChange) {
      expect(lineGateKind(stripComment(line)), `should NOT flag: ${line}`).toBeNull();
    }
  });

  it("does not flag comments, the Vue-runtime import, the npm import filter, or a display label", () => {
    // Comments mentioning `.vue` / `"vue"` are stripped.
    expect(
      lineGateKind(stripComment('// only .vue files count, e.g. languageId === "vue"')),
    ).toBeNull();
    expect(
      lineGateKind(stripComment("/** Matches the testing-API virtual file (`*.vue.ts`). */")),
    ).toBeNull();
    // The Vue runtime type import (bare `"vue"` npm specifier) is not a gate.
    expect(lineGateKind(stripComment('} from "vue";'))).toBeNull();
    expect(lineGateKind(stripComment('import { ref } from "vue";'))).toBeNull();
    // The Vue-runtime npm-package import filter (`!== "vue" && startsWith("vue/")`)
    // is intrinsic and allowlisted.
    expect(
      lineGateKind(
        stripComment('if (imp.source !== "vue" && !imp.source.startsWith("vue/")) continue;'),
      ),
    ).toBeNull();
    // A bare display-label string carrying `.vue.ts` (a tree-item label) is NOT
    // a classifier/gate — it must not flag.
    expect(
      lineGateKind(
        stripComment('const label = element.kind === "api" ? "API (d.vue.ts)" : element.kind;'),
      ),
    ).toBeNull();
    expect(lineGateKind(stripComment('return "API (d.vue.ts)";'))).toBeNull();
  });

  it("DOES flag a virtual-suffix literal used as a classifier (not just a label)", () => {
    // The same `.vue.ts` literal IS a gate when it is the argument of a
    // classifier call or an equality — the discriminator the label test relies
    // on. This proves rule (0) is context-aware, not blanket-substring.
    expect(lineGateKind(stripComment('if (name.endsWith(".vue.ts")) {'))).not.toBeNull();
    expect(lineGateKind(stripComment('if (path.includes(".vue.d.ts")) {'))).not.toBeNull();
    expect(lineGateKind(stripComment('if (ext === ".vue.ts") {'))).not.toBeNull();
  });

  it("the file-level allowlist + multi-line block-comment tracking are precise", () => {
    // A whole-file scan over a synthetic tree: a carrier-neutral file with a
    // `.vue` gate IS flagged; an allowlisted file with the same gate is NOT.
    const dir = mkdtempSync(join(tmpdir(), "ts-plugin-carrier-guard-"));
    try {
      // A block comment that opens on one line and closes two lines later must
      // NOT have its inner `.endsWith(".vue")` flagged; the executable gate
      // AFTER the close MUST flag.
      writeFileSync(
        join(dir, "neutral.ts"),
        [
          "/* a multi-line block",
          '   mentioning .endsWith(".vue") which is inside the comment',
          "   and must not be flagged */",
          'export function f(name: string) { return name.endsWith(".vue"); }',
        ].join("\n"),
      );
      const v = fileViolationsAt(dir, "neutral.ts");
      expect(v).toHaveLength(1);
      expect(v[0].line).toBe(4);
      expect(v[0].text).toContain('endsWith(".vue")');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

/**
 * Test-only helper: run `fileViolations` against a synthetic file but rebase
 * the reported relative path on the synthetic dir (so the self-test does not
 * depend on the real `SRC_ROOT`).
 */
function fileViolationsAt(dir: string, name: string): Violation[] {
  const abs = join(dir, name);
  const src = readFileSync(abs, "utf8");
  const violations: Violation[] = [];
  let inBlockComment = false;
  const lines = src.split(/\r?\n/);
  for (let idx = 0; idx < lines.length; idx += 1) {
    let line = lines[idx];
    if (inBlockComment) {
      const end = line.indexOf("*/");
      if (end !== -1) {
        line = line.slice(end + 2);
        inBlockComment = false;
      } else {
        continue;
      }
    }
    const code = stripComment(line);
    if (opensBlockComment(line)) inBlockComment = true;
    const trimmed = code.trim();
    if (trimmed.length === 0) continue;
    const kind = lineGateKind(code);
    if (kind !== null) {
      violations.push({ file: name, line: idx + 1, text: trimmed });
    }
  }
  return violations;
}
