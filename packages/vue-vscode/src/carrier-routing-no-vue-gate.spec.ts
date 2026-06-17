// Static architecture guard (TS mirror of the Rust `carrier_routing_no_vue_gate`
// guards): the `verter-vscode` extension source must classify and route
// framework-carrier documents carrier-GENERICALLY, never via a hardcoded
// Vue-only `languageId === "vue"` / `.endsWith(".vue")` GATE over carrier-
// NEUTRAL data (a document's language id, a carrier-neutral component import
// source). A `.vue` SFC and a `.svelte` component are both framework CARRIERS;
// a Vue-only gate silently strands `.svelte` below parity.
//
// The carrier-generic predicate ALREADY EXISTS and is the SANCTIONED
// replacement: `isFrameworkCarrierLanguageId(languageId)` (manifest-derived, in
// `frameworkWiring.ts` — its set includes "vue" + "svelte"). For a
// carrier-neutral component import source, classify against ANY carrier
// extension rather than a hardcoded `.endsWith(".vue")`.
//
// This guard reads every `src/**/*.ts` in this package (skipping `*.spec.ts`,
// `*.test.ts`, and `generated/**`), strips comments, and FAILS on any literal
// carrier GATE outside a NARROW allowlist of Vue-INTRINSIC homes (the file that
// DEFINES the carrier-language set, and the Vue-API decoration provider whose
// `!== "vue"` gate is Vue-API-specific). Calling
// `isFrameworkCarrierLanguageId(...)` is NEVER flagged — the guard targets
// literal `.vue`/`"vue"` GATES, not the helper identifier.
//
// DISCOVERY CAVEAT (documented): this package's `package.json` `test` script is
// a no-op echo, so `pnpm -r run test` (the root `pnpm test`) does NOT run this
// spec. It runs under the ROOT vitest config (`pnpm vitest run` /
// `pnpm test:coverage` / `pnpm vitest run packages/vue-vscode/src`), which does
// NOT exclude `packages/vue-vscode/src`. Run it via the root vitest.

import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync, mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { tmpdir } from "node:os";

/** This package's `src` directory (the scan root). */
const SRC_ROOT = join(__dirname);

/**
 * Files that legitimately carry carrier/Vue string data and are excluded WHOLE
 * (relative to `src`, POSIX-normalised). These are NOT carrier-neutral routing
 * gates:
 *   - `frameworkWiring.ts` DEFINES `isFrameworkCarrierLanguageId` and the
 *     framework-carrier language-id set (the manifest-derived authority).
 *   - `VueApiDecorationProvider.ts` decorates Vue-API call sites (lifecycle
 *     hooks, watchers, reactivity, provide/inject) — its `!== "vue"` gate is
 *     Vue-API-specific by definition (Vue-INTRINSIC content).
 * (`generated/**` is excluded by prefix.)
 */
const ALLOWLISTED_FILES = new Set<string>(["frameworkWiring.ts", "VueApiDecorationProvider.ts"]);

/** Whether a `src`-relative POSIX path is excluded from the scan. */
function isExcludedFile(relPosix: string): boolean {
  if (relPosix.endsWith(".spec.ts") || relPosix.endsWith(".test.ts")) return true;
  if (relPosix === "generated" || relPosix.startsWith("generated/")) return true;
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
        if (relPosix === "generated" || relPosix.startsWith("generated/")) continue;
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
 * comments are tracked by the caller's `inBlockComment` state.
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
 * structurally distinct from a `languageId` gate. This is the precise,
 * content-based allowlist for those filter lines WITHOUT excusing the whole
 * file (so a real `languageId`/`.vue` gate elsewhere in the same file — e.g.
 * the `:107`/`:166`/`:282` sites in `AnalysisTreeProvider.ts` — still fails).
 */
function isVueRuntimeImportFilter(code: string): boolean {
  return (
    (code.includes('!== "vue"') || code.includes('!=="vue"')) &&
    (code.includes('startsWith("vue/")') || code.includes('.startsWith("vue/")'))
  );
}

/**
 * Whether the quoted literal `lit` (e.g. `.vue.ts`) is used as a CLASSIFIER /
 * comparison GATE — the argument of `endsWith`/`startsWith`/`includes`/
 * `indexOf`, or one side of an `===`/`!==`. A bare display string carrying the
 * literal (a tree-item label like `"API (d.vue.ts)"`) is NOT a gate.
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
 * routing/classification GATE, returning a human-readable gate kind, or `null`
 * when carrier-generic / not a gate.
 *
 * Calling `isFrameworkCarrierLanguageId(...)` is the SANCTIONED replacement and
 * is never a gate. A `?.endsWith(".vue")` (optional-chained) classifier is the
 * same gate as a plain `.endsWith(".vue")`.
 */
function lineGateKind(code: string): string | null {
  // (0) Hardcoded carrier virtual-file suffix literals used as a CLASSIFIER —
  //     not a bare display label.
  for (const lit of [".vue.__verter_test", ".vue.d.ts", ".vue.ts", ".vue.tsx", ".vue.jsx"]) {
    if (literalUsedAsGate(code, lit)) {
      return `hardcoded carrier virtual-suffix classifier (${lit})`;
    }
  }
  if (code.includes("\\.vue\\.")) {
    return "hardcoded carrier virtual-suffix regex literal (\\.vue\\.)";
  }

  // (1) `.endsWith(".vue")` / `.startsWith(".vue")` path-suffix CLASSIFIERS
  //     (incl. the optional-chained `?.endsWith(".vue")` form on a carrier-
  //     neutral `importSource`).
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
  return scanSource(src, rel);
}

/** Core scanner over a source string with a label for the reported path. */
function scanSource(src: string, label: string): Violation[] {
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
      violations.push({ file: label, line: idx + 1, text: trimmed });
    }
  }
  return violations;
}

describe("carrier-routing guard: no hardcoded Vue gate in verter-vscode", () => {
  it("the scan root resolves to real source files", () => {
    const files = collectTsFiles(SRC_ROOT);
    expect(files.length).toBeGreaterThan(0);
    expect(files.some((f) => f.endsWith(join("src", "extension.ts")))).toBe(true);
  });

  it("contains no hardcoded `.vue`/`\"vue\"` routing gate in carrier-neutral code", () => {
    const files = collectTsFiles(SRC_ROOT);
    const violations = files.flatMap(fileViolations);
    const rendered = violations.map((v) => `${v.file}:${v.line}: ${v.text}`).join("\n  ");
    expect(
      violations,
      `Carrier-routing guard violations: production verter-vscode code uses a\n` +
        `Vue-only \`languageId === "vue"\` / \`.endsWith(".vue")\` gate over carrier-\n` +
        `NEUTRAL data. A \`.vue\` SFC and a \`.svelte\` component are both framework\n` +
        `CARRIERS — route through the carrier-generic predicate instead:\n` +
        `  - \`isFrameworkCarrierLanguageId(languageId)\` (manifest-derived,\n` +
        `    frameworkWiring.ts) for a document language-id gate,\n` +
        `  - classify a carrier-neutral component import source against ANY carrier\n` +
        `    extension, not a hardcoded \`.endsWith(".vue")\`.\n` +
        `Allowlisted ONLY: frameworkWiring.ts (defines the set), generated/** (the\n` +
        `manifest mirror), VueApiDecorationProvider.ts (the Vue-API \`!== "vue"\`\n` +
        `gate), and the Vue-runtime npm import filter (\`!== "vue" && startsWith("vue/")\`).\n\n` +
        `Violations:\n  ${rendered}`,
    ).toHaveLength(0);
  });

  // ──────────────────── discrimination self-tests ────────────────────

  it("flags every pre-change violating shape (red proof)", () => {
    // The exact pre-fix executable shapes from the A4 carrier-NEUTRAL sites
    // (extension.ts CSS middleware + providers + startup probe + import gates).
    const preChange = [
      'if (document.languageId !== "vue") {',
      'if (document.languageId !== "vue") return;',
      'if (e.document.languageId === "vue") {',
      'if (editor?.document?.languageId !== "vue") {',
      'const isVue = window.activeTextEditor?.document?.languageId === "vue";',
      'if (lang === "vue") {',
      'if (!editor || editor.document.languageId !== "vue" || !this.enabled) {',
      'if (imp.source.endsWith(".vue")) {',
      'if (comp.importSource?.endsWith(".vue")) {',
      'if (!comp.importSource?.endsWith(".vue")) return;',
    ];
    for (const line of preChange) {
      expect(lineGateKind(stripComment(line)), `should flag: ${line}`).not.toBeNull();
    }
  });

  it("does not flag the carrier-generic post-change shapes (green proof)", () => {
    const postChange = [
      "if (!isFrameworkCarrierLanguageId(document.languageId)) {",
      "if (!isFrameworkCarrierLanguageId(document.languageId)) return;",
      "if (isFrameworkCarrierLanguageId(e.document.languageId)) {",
      "if (!isFrameworkCarrierLanguageId(editor?.document?.languageId)) {",
      "const isCarrier = isFrameworkCarrierLanguageId(window.activeTextEditor?.document?.languageId);",
      "if (isFrameworkCarrierLanguageId(lang)) {",
      "if (!editor || !isFrameworkCarrierLanguageId(editor.document.languageId) || !this.enabled) {",
      "if (isCarrierComponentImport(imp.source)) {",
      "if (comp.importSource && isCarrierComponentImport(comp.importSource)) {",
    ];
    for (const line of postChange) {
      expect(lineGateKind(stripComment(line)), `should NOT flag: ${line}`).toBeNull();
    }
  });

  it("does not flag comments, the npm import filter, or a display label", () => {
    // A `//` line comment mentioning the gate is stripped before classification.
    expect(
      lineGateKind(stripComment('const ok = isCarrier; // not a hardcoded === "vue" check anymore')),
    ).toBeNull();
    expect(
      lineGateKind(stripComment('// strands .svelte: if (languageId === "vue") ...')),
    ).toBeNull();
    // (A JSDoc/block-comment BODY line — ` * ... === "vue" ...` — never reaches
    // lineGateKind in a real scan; the scanner's inBlockComment state skips it.
    // That path is covered by the "multi-line block comments are tracked" test.)
    // The Vue-runtime npm-package import filter is intrinsic and allowlisted.
    expect(
      lineGateKind(stripComment('if (imp.source !== "vue" && !imp.source.startsWith("vue/")) continue;')),
    ).toBeNull();
    // A bare display-label string carrying `.vue.ts` is NOT a classifier/gate.
    expect(
      lineGateKind(stripComment('? "API (d.vue.ts)"')),
    ).toBeNull();
  });

  it("the file-level allowlist is precise (whole-file exclusion vs line-precise filter)", () => {
    // A synthetic tree: a carrier-neutral file with both a `.vue` languageId
    // gate AND the Vue-runtime import filter — the gate flags, the filter does
    // NOT, proving the line-precise (not whole-file) allowlist for the filter.
    const dir = mkdtempSync(join(tmpdir(), "vue-vscode-carrier-guard-"));
    try {
      writeFileSync(
        join(dir, "MixedProvider.ts"),
        [
          "export class MixedProvider {",
          "  refresh(e: any) {",
          '    if (e.document.languageId === "vue") this.render();', // line 3 — flags
          "  }",
          "  filterImports(imports: any[]) {",
          "    for (const imp of imports) {",
          '      if (imp.source !== "vue" && !imp.source.startsWith("vue/")) continue;', // line 7 — allowlisted
          "    }",
          "  }",
          "}",
        ].join("\n"),
      );
      const src = readFileSync(join(dir, "MixedProvider.ts"), "utf8");
      const v = scanSource(src, "MixedProvider.ts");
      expect(v).toHaveLength(1);
      expect(v[0].line).toBe(3);
      expect(v[0].text).toContain('=== "vue"');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("multi-line block comments are tracked (inner gate not flagged, trailing gate flagged)", () => {
    const src = [
      "/* a multi-line block",
      '   mentioning languageId === "vue" inside the comment',
      "   which must not flag */",
      'function f(d: any) { return d.languageId === "vue"; }',
    ].join("\n");
    const v = scanSource(src, "block.ts");
    expect(v).toHaveLength(1);
    expect(v[0].line).toBe(4);
    expect(v[0].text).toContain('=== "vue"');
  });
});
