import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { collectSpecCases } from "./export_cases.mjs";
import { collectFixtureCases } from "./export_fixtures.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../..");
const DEFAULT_MANIFEST = resolve(
  REPO_ROOT,
  "crates/verter_core/tests/parity/v5_process_manifest.toml",
);
const RUST_ROOT = resolve(REPO_ROOT, "crates/verter_core/src");

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(abs, out);
    } else {
      out.push(abs);
    }
  }
  return out;
}

function parseTomlValue(raw) {
  const trimmed = raw.trim();
  if (trimmed === "true") return true;
  if (trimmed === "false") return false;
  if (/^-?\d+$/.test(trimmed)) return Number.parseInt(trimmed, 10);

  const str = trimmed.match(/^"([\s\S]*)"$/);
  if (str) {
    return str[1]
      .replace(/\\n/g, "\n")
      .replace(/\\r/g, "\r")
      .replace(/\\t/g, "\t")
      .replace(/\\"/g, '"')
      .replace(/\\\\/g, "\\");
  }

  throw new Error(`unsupported TOML value: ${raw}`);
}

function parseManifestToml(text) {
  let schema = "";
  const entries = [];
  let current = null;

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;

    if (line === "[[entry]]") {
      if (current) entries.push(current);
      current = {};
      continue;
    }

    const m = line.match(/^([A-Za-z0-9_]+)\s*=\s*(.+)$/);
    if (!m) {
      continue;
    }

    const key = m[1];
    const value = parseTomlValue(m[2]);

    if (current) {
      current[key] = value;
    } else if (key === "schema") {
      schema = String(value);
    }
  }

  if (current) entries.push(current);
  return { schema, entries };
}

function extractRustFnNames() {
  const files = walk(RUST_ROOT).filter((f) => f.endsWith(".rs"));
  const names = new Set();
  const fnRe = /\bfn\s+([A-Za-z0-9_]+)\s*\(/g;

  for (const file of files) {
    const content = readFileSync(file, "utf8");
    let m;
    while ((m = fnRe.exec(content)) !== null) {
      names.add(m[1]);
    }
  }

  return names;
}

function finalRustFn(rustTestPath) {
  return String(rustTestPath).split("::").filter(Boolean).at(-1) || "";
}

function fail(errors) {
  for (const error of errors) {
    console.error(`manifest validation error: ${error}`);
  }
  process.exit(1);
}

function main() {
  const manifestPath = process.argv[2] ? resolve(REPO_ROOT, process.argv[2]) : DEFAULT_MANIFEST;

  const { schema, entries } = parseManifestToml(readFileSync(manifestPath, "utf8"));
  if (schema !== "v5_process_manifest.v1") {
    fail([`unexpected schema '${schema}', expected 'v5_process_manifest.v1'`]);
  }

  const specCases = collectSpecCases();
  const fixtureCases = collectFixtureCases();
  const expected = [...specCases, ...fixtureCases];
  const expectedIds = new Set(expected.map((c) => c.id));

  const errors = [];
  const seen = new Set();
  const byId = new Map();

  for (const entry of entries) {
    if (!entry.id) {
      errors.push("entry missing id");
      continue;
    }
    if (seen.has(entry.id)) {
      errors.push(`duplicate manifest id '${entry.id}'`);
      continue;
    }
    seen.add(entry.id);
    byId.set(entry.id, entry);
  }

  for (const expectedCase of expected) {
    if (!byId.has(expectedCase.id)) {
      errors.push(`missing manifest entry for '${expectedCase.id}'`);
    }
  }

  for (const id of byId.keys()) {
    if (!expectedIds.has(id)) {
      errors.push(`manifest entry '${id}' does not correspond to an extracted case`);
    }
  }

  for (const entry of entries) {
    if (!entry.id) continue;

    if (entry.status !== "ported") {
      errors.push(`entry '${entry.id}' must be status='ported'`);
    }

    if (!entry.rust_test || String(entry.rust_test).trim() === "") {
      errors.push(`entry '${entry.id}' missing rust_test`);
    }
  }

  const rustFnNames = extractRustFnNames();
  for (const entry of entries) {
    if (!entry.id || !entry.rust_test) continue;
    const fn = finalRustFn(entry.rust_test);
    if (!fn || !rustFnNames.has(fn)) {
      errors.push(`entry '${entry.id}' references missing rust test function '${entry.rust_test}'`);
    }
  }

  if (errors.length > 0) {
    fail(errors);
  }

  console.log(
    `v5_process manifest valid: ${entries.length} entries (${specCases.length} spec, ${fixtureCases.length} fixture)`,
  );
  console.log(`manifest: ${relative(REPO_ROOT, manifestPath)}`);
}

main();
