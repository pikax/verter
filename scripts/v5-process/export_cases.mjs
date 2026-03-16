import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../..");
const V5_PROCESS_ROOT = resolve(REPO_ROOT, "packages/core/src/v5/process");

function walk(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(abs, out);
      continue;
    }
    out.push(abs);
  }
  return out;
}

function toPosix(p) {
  return p.replace(/\\/g, "/");
}

function slugify(input) {
  return input
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80);
}

function unescapeQuoted(value, quote) {
  return value
    .replace(/\\n/g, "\n")
    .replace(/\\r/g, "\r")
    .replace(/\\t/g, "\t")
    .replace(new RegExp(`\\\\${quote}`, "g"), quote)
    .replace(/\\\\/g, "\\");
}

export function collectSpecCases() {
  const files = walk(V5_PROCESS_ROOT)
    .filter((f) => f.endsWith(".spec.ts"))
    .filter((f) => !f.endsWith(".fixtures.spec.ts"))
    .filter((f) => !f.endsWith(".bak"))
    .sort((a, b) => a.localeCompare(b));

  const cases = [];
  const testCall = /\b(?:it|test)\s*\(\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g;

  for (const file of files) {
    const content = readFileSync(file, "utf8");
    let match;
    while ((match = testCall.exec(content)) !== null) {
      const quote = match[1];
      const rawTitle = match[2];
      const title = unescapeQuoted(rawTitle, quote).trim();
      const line = content.slice(0, match.index).split("\n").length;
      const rel = toPosix(relative(REPO_ROOT, file));
      const id = `spec:${rel}:${line}:${slugify(title)}`;
      cases.push({
        kind: "spec_case",
        id,
        file: rel,
        line,
        title,
      });
    }
  }

  return cases;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const out = {
    generatedAt: new Date().toISOString(),
    total: 0,
    cases: [],
  };
  out.cases = collectSpecCases();
  out.total = out.cases.length;
  process.stdout.write(`${JSON.stringify(out, null, 2)}\n`);
}
