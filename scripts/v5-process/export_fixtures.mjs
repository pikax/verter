import { readFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, "../..");

const FIXTURE_FILES = [
  resolve(REPO_ROOT, "packages/core/src/v5/process/script/plugins/macros/macros.fixtures.ts"),
  resolve(
    REPO_ROOT,
    "packages/core/src/v5/process/script/plugins/infer-function/infer-function.fixtures.ts",
  ),
];

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

export function collectFixtureCases() {
  const fixtures = [];
  const nameMatcher = /\bname\s*:\s*(["'`])((?:\\.|(?!\1)[\s\S])*?)\1/g;

  for (const file of FIXTURE_FILES) {
    const content = readFileSync(file, "utf8");
    let match;
    while ((match = nameMatcher.exec(content)) !== null) {
      const quote = match[1];
      const rawName = match[2];
      const name = unescapeQuoted(rawName, quote).trim();
      const line = content.slice(0, match.index).split("\n").length;
      const rel = toPosix(relative(REPO_ROOT, file));
      const id = `fixture:${rel}:${line}:${slugify(name)}`;
      fixtures.push({
        kind: "fixture_case",
        id,
        file: rel,
        line,
        name,
      });
    }
  }

  fixtures.sort((a, b) => a.id.localeCompare(b.id));
  return fixtures;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const out = {
    generatedAt: new Date().toISOString(),
    total: 0,
    fixtures: [],
  };
  out.fixtures = collectFixtureCases();
  out.total = out.fixtures.length;
  process.stdout.write(`${JSON.stringify(out, null, 2)}\n`);
}
