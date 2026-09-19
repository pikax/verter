#!/usr/bin/env node
/**
 * VSW0 constitution harness: web extension feature contract /
 * main-browser entrypoint policy / host-status vocabulary / web product
 * ownership checks.
 *
 * Grounds itself in the live repository (packages/vue-vscode manifest and
 * build config, the shipped VSC0 / BWH0 / DX0 machine products) only. It
 * never reads a roadmap/DAG store (those are database-owned) and mints no
 * parallel vocabulary. Each check has a named negative twin that mutates an
 * in-memory copy and must fail, so a clean pass is discrimination, not
 * vacuity.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { builtinModules, createRequire } from "node:module";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PKG_DIR = path.join(REPO_ROOT, "packages/vue-vscode");
const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));

const dxReceiptBasis = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/product-receipt-basis.v1.json"),
);
const dxHostClasses = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/host-execution-class.v1.json"),
);
const dxOwnership = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/exposure-ownership-map.json"),
);
const dxExposureContract = readJson(
  path.join(REPO_ROOT, "tests/product-experience/DX0/products/feature-exposure-contract.v1.json"),
);
const vscFeatureMatrix = readJson(
  path.join(REPO_ROOT, "tests/vscode-product/VSC0/products/feature-ownership-matrix.v1.json"),
);
const vscBoundary = readJson(
  path.join(REPO_ROOT, "tests/vscode-product/VSC0/products/desktop-web-shared-boundary.v1.json"),
);
const bwhPlatformServices = readJson(
  path.join(REPO_ROOT, "tests/browser-host/BWH0/products/platform-host-services.v1.json"),
);
const bwhExecManifest = readJson(
  path.join(REPO_ROOT, "tests/browser-host/BWH0/products/browser-execution-manifest.v1.json"),
);

const contract = readJson(path.join(HERE, "products/web-extension-feature-contract.v1.json"));
const entryPolicy = readJson(path.join(HERE, "products/entrypoint-contribution-policy.v1.json"));
const hostVocab = readJson(path.join(HERE, "products/host-status-vocabulary.v1.json"));
const ownershipMap = readJson(path.join(HERE, "products/web-product-ownership-map.v1.json"));

const livePkg = readJson(path.join(PKG_DIR, "package.json"));
const liveEsbuildCfg = fs.readFileSync(path.join(PKG_DIR, "esbuild.config.mjs"), "utf8");
const liveExtensionTs = fs.readFileSync(path.join(PKG_DIR, "src/extension.ts"), "utf8");

const bwhServiceIds = new Set(bwhPlatformServices.services.map((s) => s.id));
const vscFeatures = Object.fromEntries(vscFeatureMatrix.features.map((f) => [f.id, f]));
const dxOperations = Object.fromEntries(dxExposureContract.operations.map((o) => [o.id, o]));
const bwhSemanticHostRequires = (bwh) =>
  bwh.portabilityProof.semanticHostRequires.map((s) => s.match(/^[A-Z0-9]+/)[0]).sort();

/** Structural clone with a mutation applied; used for negative twins only. */
function perturb(obj, mutate) {
  const copy = structuredClone(obj);
  mutate(copy);
  return copy;
}

const filesExist = (rels) => {
  for (const rel of rels) {
    assert.ok(
      fs.existsSync(path.join(REPO_ROOT, rel)),
      `evidence path missing in live tree: ${rel}`,
    );
  }
};

// ---------------------------------------------------------------------------
// VSW0-AC1: a build with only a Node main entry cannot pass as a web
// extension. The admission predicate is factored so every twin re-runs the
// exact discriminator the live facts pass. Facts are derived live and fail
// closed: a build config without a platform value is an error, never an
// implicit browser verdict, and the browser-entry Node-import fact is
// derived from a real entry module graph, never from package.json#browser
// (which VS Code defines as absent-or-path, not a bundler remap map).
// ---------------------------------------------------------------------------

// The Node builtin set is the runtime's own inventory, never a hand-kept
// subset: a builtin the list forgot (process, timers, perf_hooks, ...) would
// otherwise admit a browser entry that imports it.
const NODE_BUILTINS = new Set(builtinModules);
function isNodeBuiltinImport(spec) {
  if (spec.startsWith("node:")) return true;
  return NODE_BUILTINS.has(spec) || NODE_BUILTINS.has(spec.split("/")[0]);
}
// The extension package's own TypeScript parses its sources: specifiers are
// read from the syntax tree, so a comment inside import(...) does not hide a
// specifier and import-shaped text inside comments or strings is not one.
const ts = createRequire(path.join(PKG_DIR, "package.json"))("typescript");
const isSpecifierLiteral = (node) =>
  ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node);

/**
 * Module specifiers a bundler resolves for `src`: static import and
 * export-from declarations (type-only ones are erased and skipped),
 * `import x = require(...)`, `require(...)` and dynamic `import(...)` with
 * a literal argument.
 */
function moduleSpecifiers(src, fileName = "module.ts") {
  const sf = ts.createSourceFile(fileName, src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const specs = [];
  const visit = (node) => {
    if (ts.isImportDeclaration(node)) {
      if (!node.importClause?.isTypeOnly && isSpecifierLiteral(node.moduleSpecifier)) {
        specs.push(node.moduleSpecifier.text);
      }
    } else if (ts.isExportDeclaration(node)) {
      if (!node.isTypeOnly && node.moduleSpecifier && isSpecifierLiteral(node.moduleSpecifier)) {
        specs.push(node.moduleSpecifier.text);
      }
    } else if (ts.isImportEqualsDeclaration(node)) {
      const ref = node.moduleReference;
      if (
        !node.isTypeOnly &&
        ts.isExternalModuleReference(ref) &&
        isSpecifierLiteral(ref.expression)
      ) {
        specs.push(ref.expression.text);
      }
    } else if (ts.isCallExpression(node)) {
      const callee = node.expression;
      const dynamicImport = callee.kind === ts.SyntaxKind.ImportKeyword;
      const requireCall = ts.isIdentifier(callee) && callee.text === "require";
      const arg = node.arguments[0];
      if ((dynamicImport || requireCall) && arg && isSpecifierLiteral(arg)) specs.push(arg.text);
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return specs;
}

function resolveModuleFile(spec) {
  for (const candidate of [spec, `${spec}.ts`, `${spec}.tsx`, `${spec}/index.ts`]) {
    const abs = path.join(PKG_DIR, candidate);
    if (fs.existsSync(abs) && fs.statSync(abs).isFile()) return candidate;
  }
  return null;
}

/**
 * Walks the real import graph of a source entry under packages/vue-vscode
 * (relative imports only; type-only imports are erased by the bundler and
 * skipped) and reports whether any module in the graph imports a Node
 * builtin or the desktop-owned vscode-languageclient/node transport (VSC0
 * transport-desktop-owned). Fail-closed: an unresolvable relative import is
 * an error, never a silent pass.
 */
function entryGraphImportsNode(entryRel) {
  const seen = new Set();
  const stack = [entryRel.replace(/^\.?\//, "")];
  while (stack.length > 0) {
    const rel = stack.pop();
    if (seen.has(rel)) continue;
    seen.add(rel);
    let src;
    try {
      src = fs.readFileSync(path.join(PKG_DIR, rel), "utf8");
    } catch {
      throw new Error(`browser entry source not readable: ${rel}`);
    }
    for (const spec of moduleSpecifiers(src, rel)) {
      if (isNodeBuiltinImport(spec) || spec === "vscode-languageclient/node") return true;
      if (spec.startsWith(".")) {
        const joined = path.posix
          .join(path.posix.dirname(rel), spec)
          .replace(/^\.\//, "")
          .replace(/^\.\.\//, "");
        const resolved = resolveModuleFile(joined);
        if (!resolved) throw new Error(`unresolvable relative import ${spec} in ${rel}`);
        stack.push(resolved);
      }
    }
  }
  return false;
}

/**
 * The build configs declared in the esbuild config text, one per
 * `entryPoints:` block: each block's platform value and raw entry
 * expression. Fail-closed: a config without a platform value is an error,
 * never an implicit browser verdict.
 */
function parseBuildConfigs(esbuildText) {
  const literals = [];
  const re = /entryPoints:/g;
  for (let m = re.exec(esbuildText); m; m = re.exec(esbuildText)) {
    literals.push(enclosingObjectLiteral(esbuildText, m.index));
  }
  if (literals.length === 0) {
    throw new Error("no build config found in esbuild config: admission facts fail closed");
  }
  return literals.map((literal, i) => {
    // The platform is read from the object literal that owns the
    // entryPoints key, wherever the key sits inside it: a platform declared
    // before entryPoints is this build's, not the previous build's.
    const platform = literal.match(/platform:\s*["']([^"']+)["']/);
    if (!platform) {
      throw new Error(`build config #${i + 1} declares no platform: admission facts fail closed`);
    }
    const entry = literal.slice(literal.indexOf("entryPoints:")).match(/\[([^\]]*)\]/);
    return { platform: platform[1], entryRaw: entry ? entry[1].trim() : "" };
  });
}

/** The innermost `{ ... }` object literal containing `index`, as text. */
function enclosingObjectLiteral(text, index) {
  let depth = 0;
  let open = -1;
  for (let i = index; i >= 0; i--) {
    if (text[i] === "}") depth++;
    else if (text[i] === "{") {
      if (depth === 0) {
        open = i;
        break;
      }
      depth--;
    }
  }
  if (open === -1) {
    throw new Error("entryPoints outside an object literal: admission facts fail closed");
  }
  depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}") {
      depth--;
      if (depth === 0) return text.slice(open, i + 1);
    }
  }
  throw new Error("unterminated build config object literal: admission facts fail closed");
}

/** Resolves a build entry expression to a source path under the package. */
function resolveEntrySource(entryRaw, esbuildText) {
  const literal = entryRaw.match(/["']([^"']+)["']/);
  if (literal) return literal[1];
  const name = entryRaw.trim();
  const decl = esbuildText.match(new RegExp(`const\\s+${name}\\s*=\\s*path\\.join\\(([^)]*)\\)`));
  if (!decl) {
    throw new Error(`cannot resolve build entry ${entryRaw}: admission facts fail closed`);
  }
  return decl[1]
    .match(/["']([^"']*)["']/g)
    .map((s) => s.slice(1, -1))
    .join("/");
}

function webAdmissionFacts({
  pkg = livePkg,
  esbuildText = liveEsbuildCfg,
  scanEntryGraph = entryGraphImportsNode,
} = {}) {
  const builds = parseBuildConfigs(esbuildText);
  const desktopBuild = builds.find((b) => /PRODUCTION_ENTRY_POINT|extension\.ts/.test(b.entryRaw));
  if (!desktopBuild) {
    throw new Error("no desktop/main build config found: admission facts fail closed");
  }
  const manifestMain = typeof pkg.main === "string";
  const manifestBrowser = typeof pkg.browser === "string";
  // The desktop main bundle keeps its own build; the browser bundle is the
  // non-desktop build config. A declared browser entry with no browser
  // bundle build is not browser-targeted and stays not-admitted.
  const browserBuild = builds.find((b) => b !== desktopBuild);
  const browserBundlePlatform = manifestBrowser && browserBuild ? browserBuild.platform : null;
  const browserEntryImportsNode =
    manifestBrowser && browserBuild
      ? scanEntryGraph(resolveEntrySource(browserBuild.entryRaw, esbuildText))
      : null;
  return {
    manifestMain,
    manifestBrowser,
    desktopBuildPlatform: desktopBuild.platform,
    browserBundlePlatform,
    browserEntryImportsNode,
  };
}

function admitWebExtension(facts) {
  if (!facts.manifestMain) return { admitted: false, reason: "main-required" };
  if (!facts.manifestBrowser) return { admitted: false, reason: "node-main-only" };
  if (facts.browserBundlePlatform !== "browser") {
    return { admitted: false, reason: "browser-entry-node-platform" };
  }
  if (facts.browserEntryImportsNode) {
    return { admitted: false, reason: "node-import-in-browser-entry" };
  }
  return { admitted: true, reason: "admitted" };
}

function assertPolicyDeclaresAdmissionLaw(policy) {
  const rules = policy.entrypoint.webAdmissionRules.join("\n");
  assert.match(
    rules,
    /both main .* browser .* entries|browser .* entries .*main/,
    "an admission rule must require both main and browser entries",
  );
  assert.match(
    rules,
    /browser platform/i,
    "an admission rule must require a browser-platform build",
  );
  assert.match(
    rules,
    /desktop main bundle keeps its node platform/,
    "an admission rule must keep the builds dual: node main bundle, browser browser bundle",
  );
  assert.match(
    rules,
    /node-platform browser bundle is not a web extension/,
    "a node-platform browser bundle must be rejected too",
  );
  assert.match(
    rules,
    /no Node builtins|never spawns a child process/,
    "an admission rule must exclude Node imports and child processes from the web entry",
  );
  assert.match(
    rules,
    /re-derived from the live manifest/,
    "admission must be a re-derived predicate, not a packaging-time claim",
  );
  assert.match(
    rules,
    /fail closed/,
    "admission facts must fail closed, never default to a browser verdict",
  );
}

function assertLiveStateGrounded(policy, facts) {
  const live = policy.entrypoint.liveState;
  assert.equal(
    live.manifestBrowser,
    facts.manifestBrowser ? "<declared>" : "absent",
    "liveState.manifestBrowser must match the live manifest",
  );
  assert.equal(
    live.desktopBuildPlatform,
    facts.desktopBuildPlatform,
    "liveState.desktopBuildPlatform must match the live esbuild config",
  );
  assert.equal(
    live.browserBundlePlatform,
    facts.browserBundlePlatform ?? "absent",
    "liveState.browserBundlePlatform must match the live esbuild config",
  );
  assert.equal(
    live.verdict,
    "not-admitted: node-main-only",
    "the live verdict is node-main-only until a browser entry ships",
  );
  assert.match(
    liveEsbuildCfg,
    /entryPoints:\s*\[PRODUCTION_ENTRY_POINT\]/,
    "the shipped build has exactly the production entry point",
  );
  assert.match(
    liveExtensionTs,
    /from "vscode-languageclient\/node"/,
    "the shipped entry graph carries the desktop-owned Node transport",
  );
}

// A compliant dual-entry product built from perturbed live inputs for the
// non-vacuity twins: the desktop build and its node platform stay exactly as
// shipped; only a browser entry declaration and a browser-targeted browser
// bundle build are added.
const dualEntryPkg = { ...structuredClone(livePkg), browser: "dist/web/extension.js" };
const dualEntryEsbuild = (entry, platform) =>
  `${liveEsbuildCfg}\nconst webBundleConfig = { entryPoints: ["${entry}"], platform: "${platform}" };\n`;

test("VSW0-AC1 clean pass: the live build is not-admitted node-main-only with fail-closed live-derived facts", () => {
  const facts = webAdmissionFacts();
  assert.deepEqual(facts, {
    manifestMain: true,
    manifestBrowser: false,
    desktopBuildPlatform: "node",
    browserBundlePlatform: null,
    browserEntryImportsNode: null,
  });
  assert.deepEqual(admitWebExtension(facts), {
    admitted: false,
    reason: "node-main-only",
  });
  assertPolicyDeclaresAdmissionLaw(entryPolicy);
  assertLiveStateGrounded(entryPolicy, facts);
});

test("VSW0-AC1 twin: web-admission-without-policy (dropping the both-entries rule) fails", () => {
  const weakened = perturb(entryPolicy, (c) => {
    c.entrypoint.webAdmissionRules = c.entrypoint.webAdmissionRules.filter(
      (r) => !/both main/.test(r),
    );
  });
  assert.throws(() => assertPolicyDeclaresAdmissionLaw(weakened), /both main and browser/);
});

test("VSW0-AC1 twin: browser-only-entry is not admitted either", () => {
  const pkg = { ...structuredClone(livePkg) };
  delete pkg.main;
  pkg.browser = "dist/web/extension.js";
  const facts = webAdmissionFacts({ pkg });
  assert.equal(facts.manifestMain, false);
  const verdict = admitWebExtension(facts);
  assert.equal(verdict.admitted, false);
  assert.equal(verdict.reason, "main-required");
});

test("VSW0-AC1 twin: browser-entry-node-platform (browser entry declared, node-platform browser bundle) fails", () => {
  const facts = webAdmissionFacts({
    pkg: dualEntryPkg,
    esbuildText: dualEntryEsbuild("src/activationGate.ts", "node"),
  });
  assert.equal(facts.browserBundlePlatform, "node");
  assert.deepEqual(admitWebExtension(facts), {
    admitted: false,
    reason: "browser-entry-node-platform",
  });
});

test("VSW0-AC1 twin: browser-entry-node-platform (browser entry declared, no browser bundle build at all) fails", () => {
  const facts = webAdmissionFacts({ pkg: dualEntryPkg });
  assert.equal(facts.browserBundlePlatform, null);
  assert.deepEqual(admitWebExtension(facts), {
    admitted: false,
    reason: "browser-entry-node-platform",
  });
});

test("VSW0-AC1 twin: node-import-in-browser-entry (the real desktop entry graph) fails admission", () => {
  // The browser bundle targets the browser platform, but its entry is the
  // real src/extension.ts whose live graph imports vscode-languageclient/node:
  // the import fact is derived from that graph, not hand-set.
  const facts = webAdmissionFacts({
    pkg: dualEntryPkg,
    esbuildText: dualEntryEsbuild("src/extension.ts", "browser"),
  });
  assert.equal(facts.browserEntryImportsNode, true);
  assert.deepEqual(admitWebExtension(facts), {
    admitted: false,
    reason: "node-import-in-browser-entry",
  });
});

test("VSW0-AC1 twin: node-import-in-browser-entry (a graph reaching a Node builtin) fails admission", () => {
  const facts = webAdmissionFacts({
    pkg: dualEntryPkg,
    esbuildText: dualEntryEsbuild("src/sharedTsgoLaunch.ts", "browser"),
  });
  assert.equal(facts.browserEntryImportsNode, true);
  assert.deepEqual(admitWebExtension(facts), {
    admitted: false,
    reason: "node-import-in-browser-entry",
  });
});

test("VSW0-AC1 twin: dual-entry-admitted (main kept, browser-targeted bundle over a real clean entry graph) is the admitted shape", () => {
  const facts = webAdmissionFacts({
    pkg: dualEntryPkg,
    esbuildText: dualEntryEsbuild("src/activationGate.ts", "browser"),
  });
  assert.equal(facts.desktopBuildPlatform, "node", "the desktop build keeps its node platform");
  assert.equal(facts.browserBundlePlatform, "browser");
  assert.equal(facts.browserEntryImportsNode, false, "derived from the real pure-tier entry graph");
  assert.deepEqual(admitWebExtension(facts), { admitted: true, reason: "admitted" });
});

test("VSW0-AC1 twin: platform-before-entrypoints (key order inside the owning build literal) reads that literal", () => {
  const reordered = `${liveEsbuildCfg}\nconst webBundleConfig = { platform: "browser", entryPoints: ["src/activationGate.ts"] };\n`;
  const facts = webAdmissionFacts({ pkg: dualEntryPkg, esbuildText: reordered });
  assert.equal(facts.desktopBuildPlatform, "node", "the desktop build keeps its own platform");
  assert.equal(facts.browserBundlePlatform, "browser", "the browser build owns its platform key");
  assert.deepEqual(admitWebExtension(facts), { admitted: true, reason: "admitted" });
});

test("VSW0-AC1 twin: dynamic-import-of-a-node-builtin is a Node import specifier", () => {
  assert.deepEqual(
    moduleSpecifiers(`export async function load() { return await import("node:fs"); }`),
    ["node:fs"],
  );
  assert.ok(isNodeBuiltinImport("node:fs"));
});

test("VSW0-AC1 twin: comment-inside-dynamic-import still yields the specifier", () => {
  assert.deepEqual(
    moduleSpecifiers(`const m = await import(/* lazily */ "node:fs" /* builtin */);`),
    ["node:fs"],
  );
  assert.deepEqual(moduleSpecifiers(`const r = require(// desktop only\n  "node:path");`), [
    "node:path",
  ]);
});

test("VSW0-AC1 twin: import-shaped text in comments and strings is not a specifier", () => {
  const src = [
    `// import "node:fs";`,
    `/* const x = require("node:child_process"); */`,
    `const doc = 'import { a } from "node:os";';`,
    `const tpl = \`import("node:net")\`;`,
    `import type { T } from "node:tls";`,
    `export type { U } from "node:dns";`,
    `import { real } from "./activationGate";`,
  ].join("\n");
  assert.deepEqual(moduleSpecifiers(src), ["./activationGate"]);
});

test("VSW0-AC1 twin: an unprefixed builtin outside any hand-kept shortlist (process) is a Node import", () => {
  for (const spec of ["process", "timers", "perf_hooks", "fs/promises", "node:test"]) {
    assert.ok(isNodeBuiltinImport(spec), `${spec} is a Node builtin`);
  }
  for (const spec of ["vue", "vscode", "./activationGate", "vscode-languageclient/browser"]) {
    assert.equal(isNodeBuiltinImport(spec), false, `${spec} is not a Node builtin`);
  }
});

test("VSW0-AC1 twin: missing-platform-fact-fails-closed (dropping the platform value is an error, not a browser verdict)", () => {
  const noPlatform = liveEsbuildCfg.replace(/platform:\s*["']node["'],/, "");
  assert.doesNotMatch(noPlatform, /platform:/);
  assert.throws(() => webAdmissionFacts({ esbuildText: noPlatform }), /fail closed/);
});

test("VSW0-AC1 twin: platform facts parse the declared value, never default (quoting drift is not fail-open)", () => {
  const singleQuoted = liveEsbuildCfg.replace('platform: "node"', "platform: 'node'");
  assert.equal(webAdmissionFacts({ esbuildText: singleQuoted }).desktopBuildPlatform, "node");
});

test("VSW0-AC1 twin: live-state-drift (policy claiming a shipped browser entry) fails", () => {
  const drifted = perturb(entryPolicy, (c) => {
    c.entrypoint.liveState.manifestBrowser = "dist/web/extension.js";
    c.entrypoint.liveState.browserBundlePlatform = "browser";
  });
  assert.throws(
    () => assertLiveStateGrounded(drifted, webAdmissionFacts()),
    /manifestBrowser must match/,
  );
});

// ---------------------------------------------------------------------------
// VSW0-AC2: the active host/backend is visible and correct in a
// browser-connected remote workspace. The vocabulary owns the terms; the
// feature contract and every mode join them.
// ---------------------------------------------------------------------------

function assertHostStatusLaw(vocab) {
  for (const term of vocab.statusEnum) {
    assert.ok(vocab.terms[term], `status term ${term} has no definition`);
    assert.match(vocab.terms[term].definition, /\S/, `status term ${term} definition is empty`);
    assert.ok(vocab.terms[term].derivesFrom, `status term ${term} must state how it is derived`);
  }
  assert.match(
    vocab.terms["remote-native"].definition,
    /never claim browser-local receipts|does not make this browser-local/,
    "remote-native must explicitly deny browser-local parity",
  );
  assert.ok(
    vocab.claims.some((c) => /not browser-local parity/.test(c)),
    "a claim rule must state that a remote Node service is not browser-local parity",
  );
  assert.ok(
    vocab.visibility.binding.hostIdentity,
    "visibility must bind the DX0 hostIdentity receipt field",
  );
  assert.match(vocab.visibility.rule, /surfaced/, "the host status must be surfaced, not latent");
  assert.match(
    vocab.visibility.rule,
    /never guessed from the client/,
    "host status derives from executing entry and workspace topology, not the client kind",
  );
  assert.match(vocab.law, /visible and correct/, "the law restates the AC2 visibility obligation");
}

// Every mode and vocabulary receiver joins real status terms.
function assertHostModesJoinVocabulary(c, vocab) {
  for (const mode of c.hostModes) {
    assert.ok(
      vocab.statusEnum.includes(mode.hostStatus),
      `host mode ${mode.mode} uses unknown status ${mode.hostStatus}`,
    );
  }
}

test("VSW0-AC2 clean pass: vocabulary terms, visibility binding and non-parity law hold", () => {
  assertHostStatusLaw(hostVocab);
  assertHostModesJoinVocabulary(contract, hostVocab);
  assert.equal(contract.hostModes.length, 3, "exactly the three charter execution placements");
  const browserLocalMode = contract.hostModes.find((m) => m.hostStatus === "browser-local");
  assert.equal(browserLocalMode.existsToday, false, "no browser-local mode exists today (AC1)");
});

test("VSW0-AC2 twin: remote-native-claimed-browser-local (dropping the non-parity claim) fails", () => {
  const weakened = perturb(hostVocab, (c) => {
    c.claims = c.claims.filter((x) => !/not browser-local parity/.test(x));
  });
  assert.throws(() => assertHostStatusLaw(weakened), /not browser-local parity/);
});

test("VSW0-AC2 twin: missing-hostidentity-binding fails", () => {
  const dropped = perturb(hostVocab, (c) => {
    delete c.visibility.binding.hostIdentity;
  });
  assert.throws(() => assertHostStatusLaw(dropped), /hostIdentity/);
});

test("VSW0-AC2 twin: hidden-host-status (weakening the surfaced rule) fails", () => {
  const hidden = perturb(hostVocab, (c) => {
    c.visibility.rule = c.visibility.rule
      .replace("and it is surfaced", "and it is recorded")
      .replace(", and it is recorded", "");
  });
  assert.throws(() => assertHostStatusLaw(hidden), /surfaced/);
});

test("VSW0-AC2 twin: status-term-drift (renamed term not joined by the modes) fails", () => {
  const drifted = perturb(hostVocab, (c) => {
    c.statusEnum = c.statusEnum.map((s) => (s === "browser-local" ? "web-local" : s));
  });
  const mode = contract.hostModes.find((m) => m.hostStatus === "browser-local");
  assert.ok(mode, "the contract still names the browser-local mode");
  // The renamed term loses its definition, and separately the host modes no
  // longer join the vocabulary: each discriminator fires on its own.
  assert.throws(() => assertHostStatusLaw(drifted), /has no definition/);
  assert.throws(
    () => assertHostModesJoinVocabulary(contract, drifted),
    /unknown status browser-local/,
  );
});

// ---------------------------------------------------------------------------
// VSW0-AC-OWNER: one final owner (expansion.vscode-web) pinned on the
// ownership map and every delivered product, with an empty deletion
// population and the retirement obligation stated (charter Binding
// architecture + VSW0-AC-OWNER).
// ---------------------------------------------------------------------------

function assertFinalOwnerJoins(map = ownershipMap, products = [contract, entryPolicy, hostVocab]) {
  assert.equal(map.contractNode, "VSW0", "the ownership map pins its contract node");
  assert.equal(
    map.finalOwner,
    "expansion.vscode-web",
    "one final owner: expansion.vscode-web (charter Binding architecture)",
  );
  assert.match(
    map.finalOwnerStatement,
    /expansion\.vscode-web owns this product outcome/,
    "the final owner statement keeps producing owners with their existing owners",
  );
  assert.deepEqual(
    map.deletionPopulationThisNode,
    [],
    "VSW0 is contract-only: the deletion population is empty",
  );
  assert.match(
    map.retirementObligation,
    /VSW0 itself retires nothing/,
    "the retirement obligation is stated and binds the cutovers, not this node",
  );
  assert.equal(map.outcomes.length, 3, "exactly the three charter interfaces are owned");
  assert.deepEqual(
    map.outcomes.map((o) => o.id).sort(),
    products.map((p) => p.id).sort(),
    "the owned outcome ids are exactly the delivered product ids",
  );
  for (const p of products) {
    assert.equal(
      p.finalOwner,
      "expansion.vscode-web",
      `${p.id}: the delivered product pins the same single final owner`,
    );
  }
}

test("VSW0-AC-OWNER clean pass: one final owner on the map and every delivered product", () => {
  assertFinalOwnerJoins();
});

test("VSW0-AC-OWNER twin: outcome-id-drift (an owned outcome renamed away from its product) fails", () => {
  const drifted = perturb(ownershipMap, (m) => {
    m.outcomes.find((o) => o.id === "HostStatusVocabulary").id = "HostStatusVocabularyV2";
  });
  assert.throws(() => assertFinalOwnerJoins(drifted), /exactly the delivered product ids/);
});

test("VSW0-AC-OWNER twin: product-id-drift (a delivered product renamed away from its outcome) fails", () => {
  const renamed = perturb(entryPolicy, (p) => {
    p.id = "BrowserEntrypointPolicy";
  });
  assert.throws(
    () => assertFinalOwnerJoins(ownershipMap, [contract, renamed, hostVocab]),
    /exactly the delivered product ids/,
  );
});

test("VSW0-AC-OWNER twin: owner-flipped (a second authority claiming the product) fails", () => {
  const flipped = perturb(ownershipMap, (c) => {
    c.finalOwner = "expansion.vscode-product";
  });
  assert.throws(() => assertFinalOwnerJoins(flipped), /one final owner/);
});

test("VSW0-AC-OWNER twin: split-authority (a delivered product under a different owner) fails", () => {
  const split = perturb(contract, (c) => {
    c.finalOwner = "expansion.vscode-product";
  });
  assert.throws(
    () => assertFinalOwnerJoins(ownershipMap, [split, entryPolicy, hostVocab]),
    /same single final owner/,
  );
});

test("VSW0-AC-OWNER twin: missing-final-owner on a delivered product fails", () => {
  const dropped = perturb(entryPolicy, (c) => {
    delete c.finalOwner;
  });
  assert.throws(
    () => assertFinalOwnerJoins(ownershipMap, [contract, dropped, hostVocab]),
    /same single final owner/,
  );
});

test("VSW0-AC-OWNER twin: premature-deletion (a route retired under VSW0) fails", () => {
  const premature = perturb(ownershipMap, (c) => {
    c.deletionPopulationThisNode = ["packages/vue-vscode/src/extension.ts"];
  });
  assert.throws(() => assertFinalOwnerJoins(premature), /deletion population/);
});

// ---------------------------------------------------------------------------
// Ratification: the feature contract joins VSC0 (feature matrix, boundary
// rules and module tiers), BWH0 (platform services, execution manifest) and
// DX0 (receipt basis, host classes, exposure operations) exactly, the
// entrypoint policy joins the live manifest exactly, and ownership is
// single-owner docs-only.
// ---------------------------------------------------------------------------

function assertFeatureContractJoins(
  c,
  { dxOps = dxOperations, boundary = vscBoundary, bwhExec = bwhExecManifest } = {},
) {
  const boundaryRuleIds = new Set(boundary.rules.map((r) => r.id));
  const boundaryModuleTier = Object.fromEntries(boundary.modules.map((m) => [m.path, m.tier]));
  const vscIds = vscFeatureMatrix.features.map((f) => f.id).sort();
  assert.deepEqual(
    c.featureRows.map((r) => r.id).sort(),
    vscIds,
    "featureRows must cover every VSC0 feature id exactly once (generated, not curated)",
  );
  // The consumed VSC0 DesktopWebSharedBoundary rule ids join by id: a VSC0
  // rename or drop of a consumed rule is a failing case here (F3).
  assert.ok(
    c.consumedBoundaryRules.length >= 2,
    "the contract names the VSC0 boundary rules it consumes",
  );
  for (const ruleId of c.consumedBoundaryRules) {
    assert.ok(
      boundaryRuleIds.has(ruleId),
      `consumed boundary rule ${ruleId} is missing from the VSC0 DesktopWebSharedBoundary`,
    );
  }
  // semanticHostLaw mirrors the live BWH0 portabilityProof verdict and the
  // exact proof-owner list (F5).
  assert.equal(
    c.semanticHostLaw.provedByBWH0,
    bwhExec.portabilityProof.provesSemanticHost,
    "semanticHostLaw must mirror the BWH0 portabilityProof semantic-host verdict",
  );
  assert.deepEqual(
    [...c.semanticHostLaw.requires].sort(),
    bwhSemanticHostRequires(bwhExec),
    "semanticHostLaw.requires must equal the BWH0 semanticHostRequires node list",
  );
  for (const term of c.vocabulary.webDisposition) {
    assert.ok(
      c.vocabulary.webDispositionDefinitions[term],
      `webDisposition term ${term} has no definition`,
    );
  }
  const routeDispositions = [
    "browser-local-route",
    "gap-pending-class-port",
    "gap-pending-semantic-host",
  ];
  for (const row of c.featureRows) {
    const vsc = vscFeatures[row.id];
    assert.ok(vsc, `unknown VSC0 feature id: ${row.id}`);
    assert.equal(
      row.vsc0Class,
      vsc.hostExecutionClass,
      `${row.id}: host-execution class drift against VSC0`,
    );
    assert.ok(
      dxHostClasses.enum.includes(row.vsc0Class),
      `${row.id}: class ${row.vsc0Class} is outside the DX0 enum`,
    );
    assert.ok(
      c.vocabulary.webDisposition.includes(row.webDisposition),
      `${row.id}: unknown webDisposition ${row.webDisposition}`,
    );
    // dx0ClassPortLaw (F1): a NativeOnly row is never browser-local-route or
    // static-web-asset; gap dispositions exist only on NativeOnly rows.
    if (row.vsc0Class === "NativeOnly") {
      assert.ok(
        !["browser-local-route", "static-web-asset"].includes(row.webDisposition),
        `${row.id}: a NativeOnly row may not claim ${row.webDisposition} (dx0ClassPortLaw: labelled gap, companion or remote-native only until the DX0 amendment)`,
      );
    } else {
      assert.ok(
        !String(row.webDisposition).startsWith("gap-pending-"),
        `${row.id}: gap dispositions exist only on NativeOnly rows (dx0ClassPortLaw)`,
      );
    }
    if (row.webDisposition === "gap-pending-semantic-host") {
      assert.match(
        row.webEngine ?? "",
        /WASM host/,
        `${row.id}: gap-pending-semantic-host names the wasm semantic surface it waits on`,
      );
    }
    // DX0 exposure-operation join (F1): where VSC0 pins a dx0Row, the row's
    // disposition must track the live DX0 class/browserExecutable verdict.
    const dx0Id = row.dx0Row ?? vsc.dx0Row;
    if (dx0Id) {
      const op = dxOps[dx0Id];
      assert.ok(op, `${row.id}: joined DX0 operation ${dx0Id} does not exist`);
      if (op.hostExecutionClass === "NativeOnly" && op.browserExecutable === false) {
        assert.equal(
          row.webDisposition,
          "gap-pending-class-port",
          `${row.id}: DX0 ${dx0Id} is NativeOnly and not browser-executable, so the row is a labelled gap until the DX0 class-port amendment`,
        );
      } else if (op.hostExecutionClass === "Portable" && op.browserExecutable === true) {
        assert.notEqual(
          row.webDisposition,
          "gap-pending-class-port",
          `${row.id}: DX0 ${dx0Id} is browser-executable; the gap disposition is stale — land the class-port update`,
        );
      }
    }
    if (row.status === "absent") {
      assert.equal(row.routeOwner, null, `${row.id}: absent row has no owner`);
    } else {
      assert.match(
        row.routeOwner ?? "",
        /^VSW[1-6]/,
        `${row.id}: routeOwner must be a VSW train node`,
      );
    }
    if (routeDispositions.includes(row.webDisposition)) {
      assert.ok(row.webEngine, `${row.id}: ${row.webDisposition} rows name their web engine`);
    }
    if (row.webDisposition === "remote-native-only" || row.webDisposition === "companion-route") {
      assert.ok(
        row.hostIdentityOverride,
        `${row.id}: ${row.webDisposition} rows must override the browser-local hostIdentity`,
      );
      assert.match(
        row.hostIdentityOverride,
        /browser-local/,
        `${row.id}: the override must name what it may not claim`,
      );
    }
    // DesktopWebSharedBoundary module-tier join (F4): a row planning a
    // browser route that cites live desktop-tier modules must list them and
    // name the VSW1 conversion or the desktop-by-design route — never
    // host-neutral readiness.
    if (routeDispositions.includes(row.webDisposition)) {
      const desktopTierModules = (vsc.evidence ?? [])
        .filter((e) => e.startsWith("packages/vue-vscode/src/"))
        .map((e) => e.slice("packages/vue-vscode/src/".length))
        .filter((m) => boundaryModuleTier[m] === "desktop")
        .sort();
      assert.deepEqual(
        row.desktopTierModules ?? [],
        desktopTierModules,
        `${row.id}: desktopTierModules must list exactly the live desktop-tier VSC0 boundary modules it cites`,
      );
      if (desktopTierModules.length > 0) {
        assert.match(
          row.desktopTierRoute ?? "",
          /VSW1|desktop-by-design/,
          `${row.id}: a row citing desktop-tier modules must name the VSW1 conversion or the desktop-by-design route`,
        );
        assert.doesNotMatch(
          `${row.note ?? ""} ${row.desktopTierRoute ?? ""}`,
          /already host-neutral/,
          `${row.id}: may not claim host-neutral readiness while citing desktop-tier modules`,
        );
      }
    }
    for (const svc of row.bwhServices ?? []) {
      assert.ok(bwhServiceIds.has(svc), `${row.id}: unknown BWH platform service ${svc}`);
    }
  }
  assert.deepEqual(
    Object.keys(c.browserLocalReceipt).sort(),
    [...dxReceiptBasis.fields].sort(),
    "browserLocalReceipt must bind exactly the DX0 receipt-basis fields",
  );
  for (const mode of c.hostModes) {
    assert.equal(
      typeof mode.existsToday,
      "boolean",
      `host mode ${mode.mode} states whether it exists today`,
    );
    assert.match(
      mode.deliveryOwner,
      /^VSW[1-6]|^expansion\./,
      `host mode ${mode.mode} names its delivery owner`,
    );
  }
  for (const inv of c.startupAdaptationInventory) {
    assert.ok(inv.nodeAssumption && inv.webRoute, `${inv.id}: inventory row states both sides`);
  }
  assert.match(c.acBasisDownstreamTestOwner, /VSW1/, "AC-BASIS binds the browser-entrypoint owner");
  assert.match(c.acExposureRegistration, /DX1/, "AC-EXPOSURE registration stays DX1-owned");
  assert.match(
    c.acResourceRationale,
    /0 production LOC/,
    "AC-RESOURCE rationale records the contract-only basis",
  );
}

function liveContributionCounts() {
  const c = livePkg.contributes;
  return {
    languages: c.languages.length,
    grammars: c.grammars.length,
    commands: c.commands.length,
    menus: Object.keys(c.menus).length,
    viewsContainers: c.viewsContainers.activitybar.length,
    views: Object.values(c.views).flat().length,
    colors: c.colors.length,
    configuration: Object.keys(c.configuration.properties).length,
    configurationDefaults: Object.keys(c.configurationDefaults).length,
    typescriptServerPlugins: c.typescriptServerPlugins.length,
    mcpServerDefinitionProviders: c.mcpServerDefinitionProviders.length,
    activationEvents: livePkg.activationEvents.length,
  };
}

function assertEntryPolicyJoins(policy) {
  const live = liveContributionCounts();
  const categories = policy.contributions.map((x) => x.category).sort();
  // liveContributionCounts covers every live contributes key plus activationEvents.
  assert.deepEqual(
    categories,
    Object.keys(live).sort(),
    "contribution categories must cover exactly the live contributes keys plus activationEvents",
  );
  for (const row of policy.contributions) {
    assert.equal(
      row.liveCount,
      live[row.category],
      `${row.category}: liveCount drift against the live manifest (${row.liveCount} vs ${live[row.category]})`,
    );
    assert.ok(
      policy.webCompatibilityEnum.includes(row.webCompatibility),
      `${row.category}: unknown webCompatibility ${row.webCompatibility}`,
    );
    assert.ok(
      row.rule && row.rule.length > 20,
      `${row.category}: rule states the compatibility obligation`,
    );
  }
}

function assertOwnershipJoins(dxMap = dxOwnership, vscMatrix = vscFeatureMatrix) {
  const dx = dxMap.receivingAmendments.find((a) => a.receiver === "VSW0");
  assert.ok(dx, "DX0 exposure ownership map names VSW0 as a receiver");
  assert.equal(
    dx.productionCapable,
    false,
    "VSW0 is a contract-only receiver; VSW1+ own vscode-web production",
  );
  const vsc = vscMatrix.receivingAmendments.find((a) => a.receiver === "VSW0");
  assert.ok(vsc, "VSC0 feature matrix names VSW0 as a receiving amendment");
  assert.equal(vsc.productionCapable, false, "VSC0 keeps VSW0 docs-only");
  assert.deepEqual(vscMatrix.docsOnlyReceivers, ["VSW0"], "VSW0 stays the docs-only receiver list");
  assert.ok(
    bwhExecManifest.receivingAmendments.crossTrainDirect.includes("VSW0"),
    "BWH0 browser-execution-manifest records VSW0 as a cross-train direct receiver",
  );
  for (const a of hostVocab.receivingAmendments) {
    assert.match(a.receiver, /^VSW[1-6][A-Z]?$/, `unknown vocabulary receiver ${a.receiver}`);
    assert.equal(typeof a.productionCapable, "boolean");
  }
}

test("VSW0 ratification clean pass: products join VSC0/BWH0/DX0 and the live manifest exactly", () => {
  assertFeatureContractJoins(contract);
  assertEntryPolicyJoins(entryPolicy);
  assertOwnershipJoins();
  assertFinalOwnerJoins();
});

test("VSW0 ratification twin: feature-row-missing fails coverage", () => {
  const m = perturb(contract, (c) => {
    c.featureRows = c.featureRows.filter((r) => r.id !== "decorations");
  });
  assert.throws(() => assertFeatureContractJoins(m), /exactly once/);
});

test("VSW0 ratification twin: feature-disposition-drift (synonymous disposition) fails", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "css-intellisense").webDisposition = "portable-direct";
  });
  assert.throws(() => assertFeatureContractJoins(m), /unknown webDisposition/);
});

test("VSW0 ratification twin: nativeonly-claimed-browser-local fails the dx0ClassPortLaw join", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "virtual-files-view").webDisposition = "browser-local-route";
  });
  assert.throws(() => assertFeatureContractJoins(m), /dx0ClassPortLaw/);
});

test("VSW0 ratification twin: gap-disposition-on-non-native-row fails", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "provider-selection").webDisposition =
      "gap-pending-class-port";
  });
  assert.throws(() => assertFeatureContractJoins(m), /only on NativeOnly rows/);
});

test("VSW0 ratification twin: dx0-browser-executable-drift (the DX0 class-port amendment landing) invalidates the stale gap label", () => {
  const amended = structuredClone(dxOperations);
  amended["vscode.source-map"].hostExecutionClass = "Portable";
  amended["vscode.source-map"].browserExecutable = true;
  assert.throws(() => assertFeatureContractJoins(contract, { dxOps: amended }), /stale/);
});

test("VSW0 ratification twin: semantic-host-evidence-drift (claiming the proof before BWH delivers it) fails", () => {
  const m = perturb(contract, (c) => {
    c.semanticHostLaw.provedByBWH0 = true;
  });
  assert.throws(() => assertFeatureContractJoins(m), /portabilityProof semantic-host verdict/);
});

test("VSW0 ratification twin: bwh0-semantic-host-requirement-drift (BWH0 changing the proof owners) fails", () => {
  const drifted = structuredClone(bwhExecManifest);
  drifted.portabilityProof.semanticHostRequires =
    drifted.portabilityProof.semanticHostRequires.filter((s) => !s.startsWith("BWH2"));
  assert.throws(() => assertFeatureContractJoins(contract, { bwhExec: drifted }), /requires/);
});

test("VSW0 ratification twin: boundary-rule-missing (VSC0 dropping a consumed boundary rule) fails", () => {
  const dropped = structuredClone(vscBoundary);
  dropped.rules = dropped.rules.filter((r) => r.id !== "browser-is-distinct-product");
  assert.throws(() => assertFeatureContractJoins(contract, { boundary: dropped }), /boundary rule/);
});

test("VSW0 ratification twin: boundary-rule-unknown (citing a rule id VSC0 never had) fails", () => {
  const m = perturb(contract, (c) => {
    c.consumedBoundaryRules.push("browser-is-a-viewport");
  });
  assert.throws(() => assertFeatureContractJoins(m), /boundary rule/);
});

test("VSW0 ratification twin: desktop-tier-module-unconverted (hiding the desktop-tier citation) fails", () => {
  const m = perturb(contract, (c) => {
    delete c.featureRows.find((r) => r.id === "virtual-files-view").desktopTierModules;
  });
  assert.throws(() => assertFeatureContractJoins(m), /desktopTierModules/);
});

test("VSW0 ratification twin: host-neutral-claim-on-desktop-tier fails", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "virtual-files-view").note =
      "presentation/pure tiers are already host-neutral (VSC0 boundary)";
  });
  assert.throws(() => assertFeatureContractJoins(m), /host-neutral readiness/);
});

test("VSW0 ratification twin: vsc0 boundary module-tier drift must update the desktop-tier citation", () => {
  const reclassified = structuredClone(vscBoundary);
  reclassified.modules.find((m) => m.path === "UnifiedVirtualFilesProvider.ts").tier =
    "presentation";
  assert.throws(
    () => assertFeatureContractJoins(contract, { boundary: reclassified }),
    /desktopTierModules/,
  );
});

test("VSW0 ratification twin: receipt-field-missing fails the DX0 field loop", () => {
  const m = perturb(contract, (c) => {
    delete c.browserLocalReceipt.hostIdentity;
  });
  assert.throws(() => assertFeatureContractJoins(m), /exactly the DX0 receipt-basis fields/);
});

test("VSW0 ratification twin: remote-row-without-host-override fails", () => {
  const m = perturb(contract, (c) => {
    delete c.featureRows.find((r) => r.id === "engine-tiers").hostIdentityOverride;
  });
  assert.throws(
    () => assertFeatureContractJoins(m),
    /must override the browser-local hostIdentity/,
  );
});

test("VSW0 ratification twin: unknown-bwh-service fails the service join", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "virtual-files-view").bwhServices = ["filesystem"];
  });
  assert.throws(() => assertFeatureContractJoins(m), /unknown BWH platform service/);
});

test("VSW0 ratification twin: contribution-category-missing fails the live-manifest join", () => {
  const m = perturb(entryPolicy, (c) => {
    c.contributions = c.contributions.filter((x) => x.category !== "colors");
  });
  assert.throws(() => assertEntryPolicyJoins(m), /exactly the live contributes keys/);
});

test("VSW0 ratification twin: contribution-compat-drift (synonymous compatibility) fails", () => {
  const m = perturb(entryPolicy, (c) => {
    c.contributions.find((x) => x.category === "commands").webCompatibility = "web-safe";
  });
  assert.throws(() => assertEntryPolicyJoins(m), /unknown webCompatibility/);
});

test("VSW0 ratification twin: counts-drift against the live manifest fails", () => {
  const m = perturb(entryPolicy, (c) => {
    c.contributions.find((x) => x.category === "commands").liveCount = 99;
  });
  assert.throws(() => assertEntryPolicyJoins(m), /liveCount drift/);
});

test("VSW0 ratification twin: dx0-receiver-row-missing fails ownership", () => {
  const m = perturb(dxOwnership, (c) => {
    c.receivingAmendments = c.receivingAmendments.filter((a) => a.receiver !== "VSW0");
  });
  assert.throws(() => assertOwnershipJoins(m), /names VSW0 as a receiver/);
});

test("VSW0 ratification twin: vsc0-receiver-drift (VSW0 flipped production-capable) fails", () => {
  const flipped = perturb(vscFeatureMatrix, (c) => {
    c.receivingAmendments.find((a) => a.receiver === "VSW0").productionCapable = true;
  });
  assert.throws(() => assertOwnershipJoins(dxOwnership, flipped), /VSC0 keeps VSW0 docs-only/);
});

test("VSW0 ratification twin: downstream-owner-unknown fails the train join", () => {
  const m = perturb(contract, (c) => {
    c.featureRows.find((r) => r.id === "statistics").routeOwner = "VSC9";
  });
  assert.throws(() => assertFeatureContractJoins(m), /routeOwner must be a VSW train node/);
});

test("VSW0 grounding: cited evidence paths exist in the live repository", () => {
  filesExist(hostVocab.visibility.evidence);
  for (const term of Object.values(hostVocab.terms)) filesExist(term.evidence);
  filesExist(entryPolicy.entrypoint.liveState.evidence);
  for (const inv of contract.startupAdaptationInventory) filesExist(inv.evidence);
  // The charter's placement surfaces are characterized, not created, by VSW0.
  assert.equal(
    fs.existsSync(path.join(REPO_ROOT, "packages/vue-vscode/src/browser")),
    false,
    "VSW0 adds no production browser entry (contract-only node)",
  );
  assert.equal(
    fs.existsSync(path.join(REPO_ROOT, "packages/vue-vscode/src/shared")),
    false,
    "VSW0 adds no production shared directory (contract-only node)",
  );
  filesExist(["packages/vue-vscode/src/statusBar.ts", "packages/vue-vscode/src/activationGate.ts"]);
});

test("VSW0 grounding twin: missing-evidence-file fails", () => {
  assert.throws(
    () => filesExist(["packages/vue-vscode/src/does-not-exist.ts"]),
    /evidence path missing/,
  );
});
