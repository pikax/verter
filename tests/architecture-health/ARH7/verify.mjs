#!/usr/bin/env node
/**
 * ARH7 verifier — sole owning interface of the VS Code activation cutover.
 *
 * Joins the activation-cutover product to the live tree: extension.ts is a
 * composition root over ActivationSession + StartAttemptScope; the
 * module-level locator and in-file StartAttemptScope class are gone;
 * activateExtension registers on the session lifetime. Node builtins only.
 */

import fs from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadProducts as loadArh2Products, validate as validateArh2 } from "../ARH2/verify.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");

export const PRODUCT_FILES = Object.freeze(["activation-cutover.json"]);

const AC3_CONCERNS = Object.freeze([
  "fresh-versus-incremental equivalence",
  "edit/revert",
  "cancellation",
  "stale/partial rejection",
  "deterministic ordering under perturbed discovery or scheduling",
]);
const HEX40 = /^[0-9a-f]{40}$/;
const CUTOVER_IDS = Object.freeze(["ARH7-CUT-1", "ARH7-CUT-2", "ARH7-CUT-3"]);
const DISPLACED_LETS = Object.freeze([
  "getClient",
  "stopHeartbeat",
  "activationContext",
  "currentMcpEndpoint",
  "retryMcpLifecycleSync",
]);

function readRel(rel) {
  return fs.readFileSync(path.join(REPO_ROOT, rel), "utf8");
}

function existsRel(rel) {
  return fs.existsSync(path.join(REPO_ROOT, rel));
}

export function loadProducts() {
  const products = {};
  for (const file of PRODUCT_FILES) {
    products[file.replace(/\.json$/, "")] = JSON.parse(
      fs.readFileSync(path.join(HERE, "products", file), "utf8"),
    );
  }
  return products;
}

export function loadManifest() {
  return JSON.parse(fs.readFileSync(path.join(HERE, "manifest.json"), "utf8"));
}

function err(errors, caseId, code, detail) {
  errors.push({ caseId, code, detail });
}

function fnBody(src, kind, name) {
  const re = new RegExp(`${kind}\\s+${name}\\s*\\(`);
  const match = re.exec(src);
  if (!match) return null;
  const brace = src.indexOf("{", match.index);
  if (brace === -1) return null;
  let depth = 0;
  for (let i = brace; i < src.length; i += 1) {
    if (src[i] === "{") depth += 1;
    else if (src[i] === "}") {
      depth -= 1;
      if (depth === 0) return src.slice(brace + 1, i);
    }
  }
  return null;
}

function hasModuleLet(src, name) {
  return new RegExp(`^let ${name}\\b`, "m").test(src);
}

function hasExport(src, name) {
  return new RegExp(
    String.raw`export\s+(?:async\s+)?(?:function|class|const|type|interface)\s+${name}\b`,
  ).test(src);
}

function hasIt(src, title) {
  return src.includes(`"${title}"`) || src.includes(`'${title}'`);
}

function productionTsImports(src) {
  const node = [];
  const vscode = /\bfrom\s+["']vscode["']/.test(src);
  for (const match of src.matchAll(/\bfrom\s+["'](node:)?([^"']+)["']/g)) {
    const spec = `${match[1] ?? ""}${match[2]}`;
    if (
      spec === "fs" ||
      spec === "path" ||
      spec === "os" ||
      spec.startsWith("node:") ||
      spec === "child_process"
    ) {
      node.push(spec);
    }
  }
  return { vscode, node };
}

export function mandatoryCases(manifest = loadManifest()) {
  return manifest.cases.map((c) => c.id);
}

export function selectedCaseIds(result) {
  return [...new Set(result.errors.map((e) => e.caseId))];
}

export function validate(products, manifest = loadManifest()) {
  const errors = [];
  const cut = products["activation-cutover"];
  const expectedCases = [
    "ARH7-authority",
    "ARH7-cost",
    "ARH7-cutover",
    "ARH7-delivery",
    "ARH7-ratification",
    "ARH7-work",
  ];
  const liveCases = [...mandatoryCases(manifest)].sort();
  if (JSON.stringify(liveCases) !== JSON.stringify(expectedCases)) {
    err(
      errors,
      "ARH7-ratification",
      "manifest-case-drift",
      `manifest cases ${JSON.stringify(liveCases)} !== ${JSON.stringify(expectedCases)}`,
    );
  }
  const expectedProducts = ["ARH7ActivationCutover"];
  if (JSON.stringify(manifest.products) !== JSON.stringify(expectedProducts)) {
    err(
      errors,
      "ARH7-ratification",
      "manifest-product-drift",
      `manifest products ${JSON.stringify(manifest.products)}`,
    );
  }
  const expectedVerify = "node tests/architecture-health/ARH7/verify.mjs";
  const expectedTest = "node --test tests/architecture-health/ARH7/arh7.test.mjs";
  if (manifest.verify !== expectedVerify) {
    err(errors, "ARH7-ratification", "manifest-command-drift", `verify=${manifest.verify}`);
  }
  if (manifest.test !== expectedTest) {
    err(errors, "ARH7-ratification", "manifest-command-drift", `test=${manifest.test}`);
  }
  if (cut?.schema !== "ARH7ActivationCutover" || cut?.contractNode !== "ARH7") {
    err(
      errors,
      "ARH7-ratification",
      "product-schema-drift",
      `schema=${cut?.schema} node=${cut?.contractNode}`,
    );
  }
  if (!HEX40.test(cut?.candidate ?? "")) {
    err(
      errors,
      "ARH7-ratification",
      "candidate-basis-drift",
      `candidate ${JSON.stringify(cut?.candidate)} is not a 40-hex git commit`,
    );
  }

  const arh2 = loadArh2Products();
  const arh2Result = validateArh2(arh2);
  if (!arh2Result.ok) {
    err(
      errors,
      "ARH7-cutover",
      "predecessor-drift",
      `ARH2 validate failed: ${arh2Result.errors.map((e) => `${e.caseId}/${e.code}`).join(",")}`,
    );
  }

  const boundaryPath = cut?.boundary?.path;
  if (!boundaryPath || !existsRel(boundaryPath)) {
    err(errors, "ARH7-cutover", "boundary-path-missing", String(boundaryPath));
  }

  const extSrc = boundaryPath && existsRel(boundaryPath) ? readRel(boundaryPath) : "";
  const displaced = [...(cut?.boundary?.displacedModuleBindings ?? [])].sort();
  if (JSON.stringify(displaced) !== JSON.stringify([...DISPLACED_LETS].sort())) {
    err(
      errors,
      "ARH7-cutover",
      "locator-still-present",
      `displaced bindings ${JSON.stringify(displaced)}`,
    );
  }
  for (const name of DISPLACED_LETS) {
    if (extSrc && hasModuleLet(extSrc, name)) {
      err(errors, "ARH7-cutover", "locator-still-present", `let ${name}`);
    }
  }
  if (extSrc && /(?:export\s+)?class StartAttemptScope\b/.test(extSrc)) {
    err(errors, "ARH7-cutover", "start-attempt-still-in-extension", "class StartAttemptScope");
  }
  if (extSrc && !hasExport(extSrc, "activate")) {
    err(errors, "ARH7-cutover", "composition-root-missing", "activate");
  }
  if (extSrc && !hasExport(extSrc, "deactivate")) {
    err(errors, "ARH7-cutover", "composition-root-missing", "deactivate");
  }
  if (extSrc && !extSrc.includes('from "./activationSession"')) {
    err(errors, "ARH7-cutover", "composition-root-missing", "import activationSession");
  }
  if (extSrc && !extSrc.includes('from "./startAttemptScope"')) {
    err(errors, "ARH7-cutover", "composition-root-missing", "import startAttemptScope");
  }
  const deactivateBody = extSrc ? fnBody(extSrc, "export function", "deactivate") : null;
  if (deactivateBody && !deactivateBody.includes("activation.deactivate()")) {
    err(errors, "ARH7-cutover", "composition-root-missing", "deactivate does not dispose the root");
  }
  const activateExtensionBody = extSrc
    ? fnBody(extSrc, "async function", "activateExtension")
    : null;
  if (activateExtensionBody && activateExtensionBody.includes("context.subscriptions")) {
    err(
      errors,
      "ARH7-cutover",
      "activate-extension-still-uses-context-subscriptions",
      "activateExtension still registers on context.subscriptions",
    );
  }
  if (extSrc && /\b(ServiceLocator|createContainer|MegaContext)\b/.test(extSrc)) {
    err(errors, "ARH7-cutover", "forbidden-service-locator", "generic container symbol");
  }

  const cutIds = (cut?.cutover ?? []).map((row) => row.id);
  if (JSON.stringify(cutIds) !== JSON.stringify([...CUTOVER_IDS])) {
    err(errors, "ARH7-cutover", "cutover-cardinality", `ids ${JSON.stringify(cutIds)}`);
  }
  for (const row of cut?.cutover ?? []) {
    const ownerPath = row.survivingOwner?.path;
    if (!ownerPath || !existsRel(ownerPath)) {
      err(errors, "ARH7-cutover", "owner-path-missing", `${row.id} ${ownerPath}`);
      continue;
    }
    const ownerSrc = readRel(ownerPath);
    const exported = row.survivingOwner.export ?? row.survivingOwner.typeName;
    if (exported && !hasExport(ownerSrc, exported) && !ownerSrc.includes(`class ${exported}`)) {
      err(errors, "ARH7-cutover", "owner-export-missing", `${row.id} ${exported}`);
    }
    if (row.survivingOwner.method && !ownerSrc.includes(`${row.survivingOwner.method}(`)) {
      err(
        errors,
        "ARH7-cutover",
        "owner-export-missing",
        `${row.id} method ${row.survivingOwner.method}`,
      );
    }
    const imports = productionTsImports(ownerSrc);
    if (imports.vscode || imports.node.length > 0) {
      err(
        errors,
        "ARH7-cutover",
        "owner-export-missing",
        `${row.id} owner is not a pure lifetime module (vscode=${imports.vscode} node=${imports.node.join(",")})`,
      );
    }
  }

  const ac2 = cut?.ac2;
  if (!ac2?.defect || ac2.defect.length < 20) {
    err(errors, "ARH7-authority", "ac2-defect-missing", String(ac2?.defect));
  }
  const ac2Files = (ac2?.evidence ?? []).map((e) => e.file).sort();
  const expectedAc2 = [
    "packages/vue-vscode/src/activationGate.ts",
    "packages/vue-vscode/src/activationSession.spec.ts",
    "packages/vue-vscode/src/extensionStartAttemptLifetime.spec.ts",
  ];
  if (JSON.stringify(ac2Files) !== JSON.stringify(expectedAc2)) {
    err(
      errors,
      "ARH7-authority",
      "ac2-evidence-population-drift",
      `evidence ${JSON.stringify(ac2Files)}`,
    );
  }
  for (const ev of ac2?.evidence ?? []) {
    if (!existsRel(ev.file)) {
      err(errors, "ARH7-authority", "ac2-evidence-missing", ev.file);
      continue;
    }
    if (ev.test && !hasIt(readRel(ev.file), ev.test)) {
      err(errors, "ARH7-authority", "ac2-evidence-unbound", ev.test);
    }
  }

  const concernNames = (cut?.ac3?.concerns ?? []).map((c) => c.concern);
  if (JSON.stringify(concernNames) !== JSON.stringify([...AC3_CONCERNS])) {
    err(errors, "ARH7-work", "ac3-concern-missing", `concerns ${JSON.stringify(concernNames)}`);
  }
  for (const concern of cut?.ac3?.concerns ?? []) {
    if (concern.applicable === true) {
      if (!concern.evidence?.length) {
        err(errors, "ARH7-work", "ac3-concern-without-evidence", concern.concern);
      }
      for (const ev of concern.evidence ?? []) {
        if (!existsRel(ev.file)) {
          err(errors, "ARH7-work", "ac3-evidence-missing", `${concern.concern} ${ev.file}`);
          continue;
        }
        if (ev.test && !hasIt(readRel(ev.file), ev.test)) {
          err(errors, "ARH7-work", "ac3-evidence-not-a-test", `${ev.test} missing in ${ev.file}`);
        }
      }
    } else if (concern.applicable === false) {
      if (!concern.rationale || concern.rationale.length < 20) {
        err(errors, "ARH7-work", "ac3-na-rationale-missing", concern.concern);
      }
    } else {
      err(errors, "ARH7-work", "ac3-concern-invented", concern.concern);
    }
  }

  if (!cut?.ac4Rationale || cut.ac4Rationale.length < 40) {
    err(errors, "ARH7-delivery", "ac4-rationale-missing", "ac4Rationale");
  }
  const examplesRoot = path.join(REPO_ROOT, "examples/reference");
  if (!fs.existsSync(examplesRoot)) {
    err(errors, "ARH7-delivery", "examples-reference-missing", "examples/reference");
  } else {
    const walk = (dir) => {
      for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, ent.name);
        if (ent.isDirectory()) walk(p);
        else if (/\.(md|ts|js|mjs)$/.test(ent.name)) {
          const text = fs.readFileSync(p, "utf8");
          if (
            text.includes("currentMcpEndpoint") ||
            text.includes("retryMcpLifecycleSync") ||
            text.includes("class StartAttemptScope")
          ) {
            err(
              errors,
              "ARH7-delivery",
              "public-example-still-names-displaced-route",
              path.relative(REPO_ROOT, p).replaceAll("\\", "/"),
            );
          }
        }
      }
    };
    walk(examplesRoot);
  }
  const vsc0Inventory = "tests/vscode-product/VSC0/products/vscode-product-inventory.v1.json";
  if (!existsRel(vsc0Inventory)) {
    err(errors, "ARH7-delivery", "vsc0-evidence-stale", vsc0Inventory);
  } else {
    const inventory = JSON.parse(readRel(vsc0Inventory));
    const row = (inventory.lifecycle?.rows ?? []).find((r) => r.id === "start-attempt-scope");
    const evidence = row?.evidence ?? [];
    if (!evidence.includes("packages/vue-vscode/src/startAttemptScope.ts")) {
      err(
        errors,
        "ARH7-delivery",
        "vsc0-evidence-stale",
        "start-attempt-scope evidence must name startAttemptScope.ts",
      );
    }
  }
  const vsc0Boundary = "tests/vscode-product/VSC0/products/desktop-web-shared-boundary.v1.json";
  if (existsRel(vsc0Boundary)) {
    const boundary = JSON.parse(readRel(vsc0Boundary));
    const modules = (boundary.modules ?? []).map((m) => m.path);
    for (const need of ["activationSession.ts", "startAttemptScope.ts"]) {
      if (!modules.includes(need)) {
        err(errors, "ARH7-delivery", "vsc0-evidence-stale", `boundary missing ${need}`);
      }
    }
  }

  if (!cut?.ac5Rationale || !cut.ac5Rationale.includes("locator")) {
    err(errors, "ARH7-cost", "ac5-rationale-missing", "must record that the locator is retired");
  }
  const numericKeys = JSON.stringify(cut);
  for (const key of ["wallNs", "speedup", "peakRssBytes", "durationMs"]) {
    if (numericKeys.includes(`"${key}"`)) {
      err(errors, "ARH7-cost", "dimension-commits-wall-clock", key);
    }
  }

  const pkg = JSON.parse(readRel("package.json"));
  const scripts = pkg.scripts?.["test:scripts"] ?? "";
  if (!scripts.includes("tests/architecture-health/ARH7/arh7.test.mjs")) {
    err(errors, "ARH7-ratification", "test-scripts-missing", "test:scripts");
  }
  const ci = readRel(".github/workflows/ci.yml");
  if (
    !ci.includes("tests/architecture-health/ARH7/verify.mjs") ||
    !ci.includes("tests/architecture-health/ARH7/arh7.test.mjs")
  ) {
    err(errors, "ARH7-ratification", "ci-lane-missing", "architecture-health job");
  }
  const start = ci.indexOf("\n            arch:\n");
  const rest = start === -1 ? "" : ci.slice(start + 1);
  const next = rest.search(/\n            [a-z_]+:\n/);
  const block = next === -1 ? rest : rest.slice(0, next);
  if (!block.includes("packages/vue-vscode/**")) {
    err(
      errors,
      "ARH7-ratification",
      "arch-filter-missing-vue-vscode",
      "arch filter must include packages/vue-vscode/**",
    );
  }

  return { ok: errors.length === 0, errors };
}

export function validateProvenance(candidate) {
  if (!HEX40.test(candidate ?? "")) {
    return {
      ok: false,
      reason: `candidate ${JSON.stringify(candidate)} is not a 40-hex git commit`,
    };
  }
  const git = (args) => {
    try {
      execFileSync("git", ["-C", REPO_ROOT, ...args], {
        stdio: ["ignore", "ignore", "ignore"],
      });
      return true;
    } catch {
      return false;
    }
  };
  if (!git(["cat-file", "-e", `${candidate}^{commit}`])) {
    return { ok: false, reason: `candidate ${candidate} is not a commit of this repository` };
  }
  if (!git(["merge-base", "--is-ancestor", candidate, "HEAD"])) {
    return { ok: false, reason: `candidate ${candidate} is not an ancestor of HEAD` };
  }
  return { ok: true };
}

const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const products = loadProducts();
  const result = validate(products);
  if (process.argv.includes("--provenance")) {
    const provenance = validateProvenance(products["activation-cutover"].candidate);
    if (!provenance.ok) {
      result.errors.push({
        caseId: "ARH7-ratification",
        code: "candidate-basis-drift",
        detail: provenance.reason,
      });
      result.ok = false;
    }
  }
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH7 verify: PASS cases=${mandatoryCases().join(",")}`);
}
