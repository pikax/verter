/**
 * STP6 packed-consumer / declaration-closure proof helpers.
 *
 * Qualifies published declaration closure and fresh-package consumption.
 * Does not implement production declaration emit.
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STP6_DIR = path.dirname(fileURLToPath(import.meta.url));
const PACKED_DIR = path.join(STP6_DIR, "packed");
const PUBLIC_DTS = Object.freeze([
  "types/index.d.ts",
  "types/concrete.d.ts",
  "types/generic.d.ts",
  "types/row.d.ts",
]);
const PUBLIC_MAPS = Object.freeze([
  "types/index.d.ts.map",
  "types/concrete.d.ts.map",
  "types/generic.d.ts.map",
]);
const REQUIRED_EXPORTS = Object.freeze([".", "./concrete", "./generic"]);
const RESOLUTION_MODES = Object.freeze([
  { id: "bundler", module: "ESNext", moduleResolution: "bundler" },
  { id: "node16", module: "Node16", moduleResolution: "Node16" },
  { id: "nodenext", module: "NodeNext", moduleResolution: "NodeNext" },
]);
const VIRTUAL_IMPORT = /__virtual|vue\.__|unpublished-meta/;

export const PROTOCOL_VERSION = 1;

export function err(caseId, code, message) {
  return { caseId, code, message };
}

export function posix(p) {
  return String(p).split(path.sep).join("/");
}

export function loadPackedPackage() {
  return JSON.parse(fs.readFileSync(path.join(PACKED_DIR, "package.json"), "utf8"));
}

export function loadStp6Product(name) {
  return JSON.parse(fs.readFileSync(path.join(STP6_DIR, "products", name), "utf8"));
}

function readPacked(rel) {
  return fs.readFileSync(path.join(PACKED_DIR, rel), "utf8");
}

/** Authored sources a declaration map may name: Vue or TypeScript under the pack's src. */
const AUTHORED_SOURCE = /^src\/.+\.(vue|ts|tsx)$/;

/**
 * Every module specifier a declaration can depend on: `from "…"` clauses,
 * `import("…")` type imports and `import x = require("…")` bindings.
 */
const SPECIFIER_PATTERNS = Object.freeze([
  /\bfrom\s+["']([^"']+)["']/g,
  /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g,
  /\bimport\s+[\w$]+\s*=\s*require\s*\(\s*["']([^"']+)["']\s*\)/g,
]);

export function importSpecifiers(text) {
  const found = [];
  for (const pattern of SPECIFIER_PATTERNS) {
    const re = new RegExp(pattern.source, pattern.flags);
    let match;
    while ((match = re.exec(text))) found.push(match[1]);
  }
  return found;
}

export function scanDeclarationText(text, fileLabel, caseId = "STP6-closure") {
  const errors = [];
  for (const spec of importSpecifiers(text)) {
    if (VIRTUAL_IMPORT.test(spec)) {
      errors.push(err(caseId, "virtual-import", `${fileLabel} imports inaccessible ${spec}`));
    }
  }
  return errors;
}

export function assertPublicDeclarationsClosed({
  files = Object.fromEntries(PUBLIC_DTS.map((rel) => [rel, readPacked(rel)])),
} = {}) {
  const errors = [];
  for (const [rel, text] of Object.entries(files)) {
    errors.push(...scanDeclarationText(text, rel, "STP6-closure"));
  }
  return errors;
}

export function assertLeakyTwinPresent({ text = readPacked("types/leaky.d.ts") } = {}) {
  const leaks = scanDeclarationText(text, "types/leaky.d.ts", "STP6-closure");
  if (leaks.length === 0) {
    return [
      err("STP6-closure", "missed-closure", "leaky declaration twin has no private virtual path"),
    ];
  }
  return [];
}

export function assertDeclMaps({
  maps = Object.fromEntries(PUBLIC_MAPS.map((rel) => [rel, readPacked(rel)])),
  packRoot = PACKED_DIR,
  caseId = "STP6-decl-map",
  published = publishedPackPaths(),
} = {}) {
  const errors = [];
  const publishedSet = new Set(published);
  for (const [rel, raw] of Object.entries(maps)) {
    let doc;
    try {
      doc = JSON.parse(raw);
    } catch {
      errors.push(err(caseId, "invalid-map", `${rel} is not JSON`));
      continue;
    }
    const sources = Array.isArray(doc.sources) ? doc.sources : [];
    if (sources.length === 0) {
      errors.push(err(caseId, "missing-source", `${rel} has no sources`));
      continue;
    }
    const mapDir = path.join(packRoot, path.dirname(rel));
    for (const source of sources) {
      if (VIRTUAL_IMPORT.test(source) || String(source).includes("unpublished-meta")) {
        errors.push(err(caseId, "virtual-map-source", `${rel} maps to inaccessible ${source}`));
        continue;
      }
      const abs = path.resolve(mapDir, source);
      const relToPack = path.relative(path.resolve(packRoot), abs);
      const inside = relToPack !== "" && !relToPack.startsWith("..") && !path.isAbsolute(relToPack);
      if (!inside || !fs.existsSync(abs)) {
        errors.push(
          err(
            caseId,
            "missing-authored-source",
            `${rel} source ${source} is not shipped in the pack`,
          ),
        );
        continue;
      }
      const relPosix = relToPack.split(path.sep).join("/");
      if (!publishedSet.has(relPosix)) {
        errors.push(
          err(
            caseId,
            "unpublished-map-source",
            `${rel} source ${source} is not in the published pack files`,
          ),
        );
        continue;
      }
      if (!AUTHORED_SOURCE.test(relPosix)) {
        errors.push(
          err(
            caseId,
            "non-authored-map-source",
            `${rel} source ${source} is not an authored source under src/`,
          ),
        );
      }
    }
  }
  return errors;
}

export function assertLeakyMapRejected({ raw = readPacked("types/leaky.d.ts.map") } = {}) {
  const errors = assertDeclMaps({
    maps: { "types/leaky.d.ts.map": raw },
    caseId: "STP6-decl-map",
  });
  if (errors.length === 0) {
    return [
      err("STP6-decl-map", "missed-virtual-map", "leaky declaration map twin was not rejected"),
    ];
  }
  return [];
}

export function publishedPackPaths(pkg = loadPackedPackage()) {
  const files = Array.isArray(pkg.files) ? pkg.files : [];
  const out = new Set(["package.json"]);
  for (const entry of files) {
    const abs = path.join(PACKED_DIR, entry);
    if (fs.existsSync(abs) && fs.statSync(abs).isDirectory()) {
      for (const rel of walkFiles(abs, PACKED_DIR)) out.add(rel);
    } else {
      out.add(posix(entry));
    }
  }
  return [...out].sort();
}

function walkFiles(dir, root, acc = []) {
  for (const name of fs.readdirSync(dir)) {
    const abs = path.join(dir, name);
    const rel = posix(path.relative(root, abs));
    if (fs.statSync(abs).isDirectory()) walkFiles(abs, root, acc);
    else acc.push(rel);
  }
  return acc;
}

export function assertPackInventory(pkg = loadPackedPackage()) {
  const errors = [];
  const exports = pkg.exports && typeof pkg.exports === "object" ? pkg.exports : {};
  for (const key of REQUIRED_EXPORTS) {
    if (!exports[key] || !exports[key].types) {
      errors.push(
        err("STP6-resolution", "missing-export", `package.json exports missing ${key} types`),
      );
    }
  }
  if (exports["./unpublished-meta"] || exports["./leaky"]) {
    errors.push(
      err(
        "STP6-hidden-metadata",
        "unpublished-export",
        "unpublished sidecar or leaky declaration is exported",
      ),
    );
  }
  const published = new Set(publishedPackPaths(pkg));
  if (published.has("unpublished-meta.d.ts") || published.has("types/leaky.d.ts")) {
    errors.push(
      err(
        "STP6-hidden-metadata",
        "unpublished-pack",
        "unpublished sidecar or leaky declaration is in the pack files set",
      ),
    );
  }
  for (const rel of PUBLIC_DTS.concat(PUBLIC_MAPS)) {
    if (!published.has(rel) && !published.has(posix(rel))) {
      errors.push(err("STP6-package-instance", "missing-pack-file", `pack omitted ${rel}`));
    }
  }
  if (!published.has("src/Concrete.vue") || !published.has("src/Generic.vue")) {
    errors.push(
      err("STP6-decl-map", "missing-authored-source", "pack omitted authored Vue sources"),
    );
  }
  return errors;
}

export function validateStp6Products({
  feasibility = loadStp6Product("packed-consumer-feasibility.json"),
  closure = loadStp6Product("public-dependency-closure-policy.json"),
  evidenceText = fs.readFileSync(path.join(STP6_DIR, "../evidence/STP6/cases.md"), "utf8"),
} = {}) {
  const errors = [];
  if (feasibility.schema !== "PackedConsumerFeasibility") {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "PackedConsumerFeasibility schema"),
    );
  }
  if (closure.schema !== "PublicDependencyClosurePolicy") {
    errors.push(err("STP6-closure", "removed-fixture", "PublicDependencyClosurePolicy schema"));
  }
  if (!String(feasibility.constructorSpelling || "").includes("declare class Comp")) {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "missing packed constructor spelling"),
    );
  }
  if (feasibility.package?.producerWorkspaceResolution !== false) {
    errors.push(
      err(
        "STP6-package-instance",
        "removed-fixture",
        "packed consumer must not use producer workspace resolution",
      ),
    );
  }
  const modes = new Set((feasibility.consumptionModes || []).map((row) => row.id));
  for (const mode of [
    "direct-import",
    "alias",
    "barrel",
    "namespace",
    "project-references",
    "package-exports",
  ]) {
    if (!modes.has(mode)) {
      errors.push(
        err("STP6-package-instance", "removed-fixture", `missing consumption mode ${mode}`),
      );
    }
  }
  const resolutions = new Set((feasibility.resolutionModes || []).map((row) => row.id));
  for (const mode of ["bundler", "node16", "nodenext"]) {
    if (!resolutions.has(mode)) {
      errors.push(err("STP6-resolution", "removed-fixture", `missing resolution mode ${mode}`));
    }
  }
  const caseIds = new Set((feasibility.cases || []).map((row) => row.id));
  for (const id of [
    "STP6-package-instance",
    "STP6-package-generics",
    "STP6-hidden-metadata",
    "STP6-closure",
    "STP6-decl-map",
    "STP6-resolution",
  ]) {
    if (!caseIds.has(id)) {
      errors.push(err("STP6-package-instance", "removed-fixture", `feasibility missing ${id}`));
    }
  }
  if (!String(feasibility.ac3Rationale || "").trim()) {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "missing AC3 untouched-owner rationale"),
    );
  }
  if (!String(feasibility.ac4Rationale || "").trim()) {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "missing AC4 untouched-owner rationale"),
    );
  }
  if (closure.typeOnlyCheckingContract?.required !== false) {
    errors.push(
      err(
        "STP6-hidden-metadata",
        "removed-fixture",
        "type-only checking contract must not be required for precision",
      ),
    );
  }
  if (closure.hiddenMetadata?.inExports !== false || closure.hiddenMetadata?.inFiles !== false) {
    errors.push(
      err(
        "STP6-hidden-metadata",
        "removed-fixture",
        "unpublished sidecar must stay out of the pack",
      ),
    );
  }
  if (closure.declarationMaps?.sourcesMustBePackRelativeAuthoredFiles !== true) {
    errors.push(
      err(
        "STP6-decl-map",
        "removed-fixture",
        "declaration maps must target shipped authored source",
      ),
    );
  }
  if (!/[0-9a-f]{40}/.test(evidenceText)) {
    errors.push(
      err(
        "STP6-package-instance",
        "missing-source-revision",
        "STP6 evidence must record a 40-character source revision",
      ),
    );
  }
  if (!/6\.0\.3/.test(evidenceText) || !/7\.0\.2/.test(evidenceText)) {
    errors.push(
      err(
        "STP6-package-instance",
        "missing-engine-pins",
        "STP6 evidence must record ts-js 6.0.3 and ts-native 7.0.2 engine pins",
      ),
    );
  }
  return errors;
}

function copyFile(src, dest) {
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.copyFileSync(src, dest);
}

function linkDir(src, dest) {
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  const type = process.platform === "win32" ? "junction" : "dir";
  try {
    fs.symlinkSync(src, dest, type);
  } catch (error) {
    if (error && error.code === "EEXIST") return;
    throw error;
  }
}

export function installPublishedPack(dest, pkg = loadPackedPackage()) {
  const libDest = path.join(dest, "node_modules", "@stp6", "lib");
  fs.mkdirSync(libDest, { recursive: true });
  for (const rel of publishedPackPaths(pkg)) {
    const src = path.join(PACKED_DIR, rel);
    if (!fs.existsSync(src) || fs.statSync(src).isDirectory()) continue;
    copyFile(src, path.join(libDest, rel));
  }
  return libDest;
}

const CONSUME_PUBLIC = `import Comp from "@stp6/lib";
import Concrete from "@stp6/lib/concrete";
import Generic from "@stp6/lib/generic";
export type Instance = InstanceType<typeof Comp>;
export const constructed: Instance = new Comp({ msg: "ok" });
export const fromConcrete: InstanceType<typeof Concrete> = constructed;
void Generic;
void fromConcrete;
`;

const CONSUME_HIDDEN = `import type { Precision } from "@stp6/lib/unpublished-meta";
export const leaked: Precision = { msg: "no", unpublished: true };
`;

const CONSUME_VIRTUAL = `import type { Hidden } from "@stp6/lib/__virtual_sfc";
export const captured: Hidden = null as never;
`;

function writeConsumer(dir, fileName, source) {
  const abs = path.join(dir, fileName);
  fs.writeFileSync(abs, source);
  return abs;
}

function writeTsconfig(dir, mode) {
  const doc = {
    compilerOptions: {
      strict: true,
      noEmit: true,
      target: "ES2022",
      module: mode.module,
      moduleResolution: mode.moduleResolution,
      skipLibCheck: false,
      lib: ["ES2022"],
    },
    include: ["consume-public.ts", "consume-hidden.ts", "consume-virtual.ts"],
  };
  const abs = path.join(dir, `tsconfig.${mode.id}.json`);
  fs.writeFileSync(abs, `${JSON.stringify(doc, null, 2)}\n`);
  return abs;
}

function flattenDiagnostics(ts, diags) {
  return (diags || []).map((diag) => {
    const message =
      typeof diag.messageText === "string" ? diag.messageText : diag.messageText?.messageText || "";
    const file = diag.file?.fileName ? posix(diag.file.fileName) : "";
    return { code: diag.code, message: String(message), file };
  });
}

export function typecheckDir(ts, tsconfigAbs) {
  const raw = ts.readConfigFile(tsconfigAbs, ts.sys.readFile);
  if (raw.error) {
    throw new Error(ts.flattenDiagnosticMessageText(raw.error.messageText, "\n"));
  }
  const parsed = ts.parseJsonConfigFileContent(
    raw.config,
    ts.sys,
    path.dirname(tsconfigAbs),
    undefined,
    tsconfigAbs,
  );
  const host = ts.createCompilerHost(parsed.options, true);
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
    host,
  });
  return flattenDiagnostics(ts, ts.getPreEmitDiagnostics(program));
}

function diagsFor(diags, fileName) {
  const needle = posix(fileName);
  return diags.filter((diag) => posix(diag.file).endsWith(needle));
}

export function evaluateLiveResolution(ts, repoRoot) {
  const errors = [];
  const vueSrc = path.join(repoRoot, "node_modules", "vue");
  if (!fs.existsSync(vueSrc)) {
    return [err("STP6-resolution", "missing-vue", `vue missing at ${vueSrc}`)];
  }
  const dest = fs.mkdtempSync(path.join(os.tmpdir(), "stp6-consumer-"));
  try {
    fs.writeFileSync(
      path.join(dest, "package.json"),
      `${JSON.stringify({ name: "stp6-consumer", private: true, type: "module" }, null, 2)}\n`,
    );
    installPublishedPack(dest);
    linkDir(vueSrc, path.join(dest, "node_modules", "vue"));
    writeConsumer(dest, "consume-public.ts", CONSUME_PUBLIC);
    writeConsumer(dest, "consume-hidden.ts", CONSUME_HIDDEN);
    writeConsumer(dest, "consume-virtual.ts", CONSUME_VIRTUAL);
    for (const mode of RESOLUTION_MODES) {
      const tsconfigAbs = writeTsconfig(dest, mode);
      const diags = typecheckDir(ts, tsconfigAbs);
      const packedDiags = diags.filter((diag) =>
        posix(diag.file).includes("/node_modules/@stp6/lib/"),
      );
      if (packedDiags.length > 0) {
        errors.push(
          err(
            "STP6-closure",
            "packed-declaration-error",
            `${mode.id} packed public declarations: ${JSON.stringify(packedDiags)}`,
          ),
        );
      }
      const publicDiags = diagsFor(diags, "consume-public.ts");
      if (publicDiags.length > 0) {
        errors.push(
          err(
            "STP6-resolution",
            "packed-consumer-error",
            `${mode.id} public consumer: ${JSON.stringify(publicDiags)}`,
          ),
        );
      }
      const hidden = diagsFor(diags, "consume-hidden.ts");
      if (!hidden.some((diag) => diag.code === 2307)) {
        errors.push(
          err(
            "STP6-hidden-metadata",
            "missing-negative",
            `${mode.id} unpublished sidecar lacked TS2307; got ${JSON.stringify(hidden)}`,
          ),
        );
      }
      const virtual = diagsFor(diags, "consume-virtual.ts");
      if (!virtual.some((diag) => diag.code === 2307)) {
        errors.push(
          err(
            "STP6-closure",
            "missing-negative",
            `${mode.id} virtual path lacked TS2307; got ${JSON.stringify(virtual)}`,
          ),
        );
      }
    }
  } finally {
    fs.rmSync(dest, { recursive: true, force: true });
  }
  return errors;
}

export function evaluateStp6Static() {
  const errors = [];
  errors.push(...validateStp6Products());
  errors.push(...assertPackInventory());
  errors.push(...assertPublicDeclarationsClosed());
  errors.push(...assertLeakyTwinPresent());
  errors.push(...assertDeclMaps());
  errors.push(...assertLeakyMapRejected());
  return errors;
}

export async function evaluateStp6({ ts = null, repoRoot = null, skipLive = false } = {}) {
  const errors = evaluateStp6Static();
  if (!skipLive) {
    if (!ts || !repoRoot) {
      errors.push(
        err(
          "STP6-resolution",
          "missing-probes",
          "live packed-consumer resolution needs a TypeScript engine",
        ),
      );
    } else {
      errors.push(...evaluateLiveResolution(ts, repoRoot));
    }
  }
  return { errors };
}
