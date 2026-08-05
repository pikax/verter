#!/usr/bin/env node

import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { compileStyle } from "@vue/compiler-sfc";

const fixtureUrl = new URL(
  "../../crates/verter_compiler/tests/fixtures/vue_style_pseudo_oracle.json",
  import.meta.url,
);
const fixturePath = fileURLToPath(fixtureUrl);
const rows = JSON.parse(await readFile(fixtureUrl, "utf8"));

function officialSelector(selector) {
  const result = compileStyle({
    source: `${selector} { color: red }`,
    filename: "vue-style-pseudo-oracle.vue",
    id: "data-v-sc1",
    scoped: true,
  });
  if (result.errors.length !== 0) {
    throw new Error(`${selector}: ${result.errors.map(String).join("\n")}`);
  }
  return result.code.slice(0, result.code.indexOf("{")).trim();
}

const generated = rows.map(({ selector }) => ({
  selector,
  expected: officialSelector(selector),
}));

if (process.argv.includes("--check")) {
  const drift = rows.filter((row, index) => row.expected !== generated[index].expected);
  if (drift.length !== 0) {
    for (const row of drift) {
      const actual = generated.find((candidate) => candidate.selector === row.selector);
      console.error(`${row.selector}: expected ${row.expected}; Vue emitted ${actual.expected}`);
    }
    process.exit(1);
  }
  console.log(`Vue style pseudo oracle: ${rows.length} rows match @vue/compiler-sfc`);
} else {
  await writeFile(fixturePath, `${JSON.stringify(generated, null, 2)}\n`);
  console.log(`Wrote ${generated.length} Vue style pseudo oracle rows to ${fixturePath}`);
}
