#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";

const EXPECTED_VUE = "3adb225775c9b28223a56e07f7a2f874b6fbb138";
const EXPECTED_SVELTE = "44a7813730579b94004e182e5a67aab27aa9d2a6";

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i]?.startsWith("--") || argv[i + 1] === undefined) {
      throw new Error(`invalid arguments near ${argv[i] ?? "<end>"}`);
    }
    out[argv[i].slice(2)] = resolve(argv[i + 1]);
  }
  for (const key of ["vue-source", "svelte-source", "vue-modules", "out-dir"]) {
    if (!out[key]) throw new Error(`missing --${key}`);
  }
  return out;
}

function git(cwd, ...gitArgs) {
  return execFileSync("git", ["-C", cwd, ...gitArgs], { encoding: "utf8" }).trim();
}

function assertCheckout(root, expected) {
  const head = git(root, "rev-parse", "HEAD");
  if (head !== expected) throw new Error(`${root}: expected ${expected}, got ${head}`);
  if (git(root, "status", "--porcelain") !== "") throw new Error(`${root}: dirty checkout`);
}

function walk(root, accept) {
  const files = [];
  const visit = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && accept(path)) files.push(path);
    }
  };
  visit(root);
  return files;
}

function hash(value) {
  return createHash("sha256").update(value).digest("hex");
}

function clean(value) {
  const cleaned = String(value).replaceAll("\t", " ").replaceAll("\r", " ").replaceAll("\n", " ");
  return cleaned === "" ? "-" : cleaned;
}

function tsv(rows, columns) {
  return (
    [
      columns.join("\t"),
      ...rows.map((row) => columns.map((column) => clean(row[column] ?? "")).join("\t")),
    ].join("\n") + "\n"
  );
}

function rootTestName(node) {
  if (!node) return null;
  if (node.type === "Identifier" && (node.name === "test" || node.name === "it")) return node.name;
  if (node.type === "MemberExpression" || node.type === "OptionalMemberExpression")
    return rootTestName(node.object);
  if (node.type === "CallExpression" || node.type === "OptionalCallExpression")
    return rootTestName(node.callee);
  return null;
}

function calleeText(source, callee) {
  return source.slice(callee.start, callee.end).replaceAll(/\s+/g, " ");
}

function testCalls(ast, source) {
  const calls = [];
  const seen = new Set();
  function visit(node, parent) {
    if (!node || typeof node !== "object" || seen.has(node)) return;
    seen.add(node);
    if (node.type === "CallExpression" || node.type === "OptionalCallExpression") {
      const root = rootTestName(node.callee);
      const isInnerCallee =
        (parent?.type === "CallExpression" || parent?.type === "OptionalCallExpression") &&
        parent.callee === node;
      if (root && !isInnerCallee) {
        const first = node.arguments?.[0];
        const titleSource =
          first && typeof first.start === "number"
            ? source.slice(first.start, first.end)
            : "<dynamic>";
        const spelling = calleeText(source, node.callee);
        calls.push({
          root,
          line: node.loc.start.line,
          column: node.loc.start.column + 1,
          parameterization: spelling.includes(".each")
            ? "parameterized-declaration"
            : "single-declaration",
          title_kind: first?.type ?? "dynamic",
          title_hash: hash(titleSource),
          callee_hash: hash(spelling),
        });
      }
    }
    for (const [key, value] of Object.entries(node)) {
      if (key === "loc" || key === "start" || key === "end") continue;
      if (Array.isArray(value)) for (const child of value) visit(child, node);
      else if (value && typeof value === "object") visit(value, node);
    }
  }
  visit(ast, null);
  return calls;
}

function vueManifest(vueRoot, parser) {
  const packages = [
    "compiler-core",
    "compiler-dom",
    "compiler-sfc",
    "compiler-ssr",
    "compiler-vapor",
  ];
  const rows = [];
  for (const packageName of packages) {
    const testRoot = join(vueRoot, "packages", packageName, "__tests__");
    const files = walk(testRoot, (path) => /\.(?:spec|test)\.[cm]?[jt]sx?$/.test(path));
    for (const file of files) {
      const source = readFileSync(file, "utf8");
      const rel = relative(vueRoot, file).split(sep).join("/");
      const blob = git(vueRoot, "rev-parse", `HEAD:${rel}`);
      const ast = parser.parse(source, {
        sourceType: "module",
        errorRecovery: false,
        plugins: ["typescript", "jsx", ["decorators", { decoratorsBeforeExport: true }]],
      });
      for (const call of testCalls(ast, source)) {
        const locator = `${rel}:${call.line}:${call.column}`;
        const key = `vue-rc3\0${locator}\0${call.callee_hash}`;
        rows.push({
          case_id: `VUE-${hash(key).slice(0, 20).toUpperCase()}`,
          suite: packageName,
          source_locator: locator,
          source_object: blob,
          declaration_kind: call.parameterization,
          title_kind: call.title_kind,
          title_sha256: call.title_hash,
          disposition: "blocked",
          provisional_owner: packageName === "compiler-sfc" ? "B2/BV1" : "BV1",
          reason:
            "BF2 must runner-enumerate profiles and attach imported/equivalent/not-applicable/unsupported evidence.",
          evidence_id: "",
        });
      }
    }
  }
  rows.sort(
    (a, b) =>
      a.source_locator.localeCompare(b.source_locator) || a.case_id.localeCompare(b.case_id),
  );
  return rows;
}

const SVELTE_NOT_APPLICABLE = new Map([
  ["migrate", "Official migration-code product is outside Verter's compiler product boundary."],
  [
    "preprocess",
    "External preprocessor implementation is outside Verter's compiler product boundary.",
  ],
  ["print", "Official AST printer product is outside Verter's compiler product boundary."],
]);

const SVELTE_NON_SAMPLE_SUITES = new Map([
  ["manual", "Manual playgrounds are not automated official compiler cases."],
  ["motion", "Official runtime motion package tests are outside compiler-output conformance."],
  ["signals", "Official runtime signals tests are outside compiler-output conformance."],
  ["store", "Official runtime store package tests are outside compiler-output conformance."],
  ["types", "Official package API type tests are outside Verter SFC TypeScript-product cases."],
]);

function svelteOwner(suite) {
  if (suite.startsWith("parser") || suite === "compiler-errors" || suite === "validator")
    return "B2/BS1";
  if (suite === "sourcemaps") return "BS1/B4";
  return "BS1";
}

function svelteManifest(svelteRoot) {
  const testsRoot = join(svelteRoot, "packages", "svelte", "tests");
  const rows = [];
  for (const suiteEntry of readdirSync(testsRoot, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    if (!suiteEntry.isDirectory()) continue;
    const suite = suiteEntry.name;
    const samples = join(testsRoot, suite, "samples");
    let sampleDirs = [];
    try {
      sampleDirs = readdirSync(samples, { withFileTypes: true })
        .filter((entry) => entry.isDirectory())
        .sort((a, b) => a.name.localeCompare(b.name));
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    for (const sample of sampleDirs) {
      const dir = join(samples, sample.name);
      const rel = relative(svelteRoot, dir).split(sep).join("/");
      const tree = git(svelteRoot, "rev-parse", `HEAD:${rel}`);
      const notApplicable = SVELTE_NOT_APPLICABLE.get(suite);
      const locator = `${rel}/`;
      rows.push({
        case_id: `SVELTE-${hash(`svelte-5.56.8\0${locator}`).slice(0, 20).toUpperCase()}`,
        suite,
        source_locator: locator,
        source_object: tree,
        declaration_kind: "sample-directory",
        disposition: notApplicable ? "not_applicable" : "blocked",
        provisional_owner: notApplicable ? "BF1" : svelteOwner(suite),
        reason:
          notApplicable ??
          "BF2 must execute/classify the sample and attach profile/product evidence.",
        evidence_id: "",
      });
    }
    if (sampleDirs.length === 0) {
      const rel = relative(svelteRoot, join(testsRoot, suite)).split(sep).join("/");
      const tree = git(svelteRoot, "rev-parse", `HEAD:${rel}`);
      const reason =
        SVELTE_NON_SAMPLE_SUITES.get(suite) ??
        "Suite contains no sample-case root; BF2 must inspect and classify it before claiming complete coverage.";
      rows.push({
        case_id: `SVELTE-${hash(`svelte-5.56.8\0${rel}/`).slice(0, 20).toUpperCase()}`,
        suite,
        source_locator: `${rel}/`,
        source_object: tree,
        declaration_kind: "suite-sentinel",
        disposition: SVELTE_NON_SAMPLE_SUITES.has(suite) ? "not_applicable" : "blocked",
        provisional_owner: SVELTE_NON_SAMPLE_SUITES.has(suite) ? "BF1" : "BF2",
        reason,
        evidence_id: "",
      });
    }
  }
  rows.sort((a, b) => a.source_locator.localeCompare(b.source_locator));
  return rows;
}

const options = args(process.argv.slice(2));
assertCheckout(options["vue-source"], EXPECTED_VUE);
assertCheckout(options["svelte-source"], EXPECTED_SVELTE);
const requireFromOracle = createRequire(join(options["vue-modules"], "package.json"));
const parser = requireFromOracle("@babel/parser");

const vueRows = vueManifest(options["vue-source"], parser);
const svelteRows = svelteManifest(options["svelte-source"]);
if (vueRows.length === 0 || svelteRows.length === 0)
  throw new Error("manifest extraction produced zero rows");

writeFileSync(
  join(options["out-dir"], "vue-official-cases.tsv"),
  tsv(vueRows, [
    "case_id",
    "suite",
    "source_locator",
    "source_object",
    "declaration_kind",
    "title_kind",
    "title_sha256",
    "disposition",
    "provisional_owner",
    "reason",
    "evidence_id",
  ]),
);
writeFileSync(
  join(options["out-dir"], "svelte-official-cases.tsv"),
  tsv(svelteRows, [
    "case_id",
    "suite",
    "source_locator",
    "source_object",
    "declaration_kind",
    "disposition",
    "provisional_owner",
    "reason",
    "evidence_id",
  ]),
);

process.stdout.write(
  JSON.stringify({ vue_rows: vueRows.length, svelte_rows: svelteRows.length }) + "\n",
);
