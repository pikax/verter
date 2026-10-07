import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const key = ([binary, test]) => JSON.stringify([binary, test]);

function population(inventory) {
  const rows = new Map();
  for (const [binary, suite] of Object.entries(inventory["rust-suites"])) {
    if (suite.status !== "listed") throw new Error(`Unlisted test binary: ${binary}`);
    for (const [test, attributes] of Object.entries(suite.testcases)) {
      rows.set(key([binary, test]), { kind: attributes.kind, ignored: attributes.ignored });
    }
  }
  return rows;
}

export function compareTestRelocation(before, after, relocations) {
  const oldRows = population(before);
  const newRows = population(after);
  const mapping = new Map();
  const targets = new Set();
  for (const { from, to } of relocations) {
    const oldKey = key(from);
    const newKey = key(to);
    if (mapping.has(oldKey) || targets.has(newKey)) throw new Error("Duplicate relocation");
    if (!oldRows.has(oldKey) || !newRows.has(newKey))
      throw new Error(`Stale relocation: ${oldKey}`);
    mapping.set(oldKey, newKey);
    targets.add(newKey);
  }
  const expected = new Map();
  for (const [oldKey, attributes] of oldRows) {
    const newKey = mapping.get(oldKey) ?? oldKey;
    if (expected.has(newKey)) throw new Error(`Relocation collision: ${newKey}`);
    expected.set(newKey, attributes);
  }
  if (expected.size !== newRows.size) throw new Error("Discovered test population changed");
  for (const [name, attributes] of expected) {
    if (JSON.stringify(newRows.get(name)) !== JSON.stringify(attributes)) {
      throw new Error(`Missing test or changed disposition: ${name}`);
    }
  }
  return expected.size;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [before, after, mapping] = process.argv.slice(2);
  if (!before || !after || !mapping)
    throw new Error("Usage: check-test-relocation.mjs BEFORE AFTER MAPPING");
  const read = (file) => JSON.parse(readFileSync(file, "utf8"));
  const count = compareTestRelocation(read(before), read(after), read(mapping));
  console.log(`Test relocation parity: ${count} discovered tests; dispositions preserved`);
}
