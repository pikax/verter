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
const selectors = [
  ":deep(.b)",
  "::v-deep(.b)",
  ":deep()",
  "::v-deep()",
  ":deep(.b, .c)",
  "::v-deep(.b, .c)",
  ".a :deep(.b)",
  ".a ::v-deep(.b)",
  ".a[data-x] :deep(.b)",
  "a:hover :deep(.b)",
  ".a > :deep(.b)",
  ".a + ::v-deep(.b)",
  ":is(:deep(.b))",
  ":where(::v-deep(.b))",
  ":deep(:is(.b, .c))",
  "::v-deep(:where(.b, .c))",

  ":slotted(.c)",
  "::v-slotted(.c)",
  ":slotted(.b, .c)",
  "::v-slotted(.b, .c)",
  ".a :slotted(.c)",
  ".a > ::v-slotted(.c)",
  ":is(:slotted(.c))",
  ":where(::v-slotted(.c))",
  ":slotted(:is(.b, .c))",

  ":global(.a)",
  "::v-global(.a)",
  ":global(.a, .b)",
  "::v-global(.a, .b)",
  ":global(.a) .b",
  "::v-global(.a) > .b",
  ".a :global(.b)",
  ".a > ::v-global(.b) + .c",
  ":is(:global(.b))",
  ":where(::v-global(.b))",
  "::v-global(:is(.a, .b))",

  "::slotted(.a)",
  ".a::slotted(.b)",
  "::deep(.b)",
  "::global(.a)",
  ":v-deep(.b)",
  ":v-slotted(.c)",
  ":v-global(.a)",

  ".x :deep(.a) .y :deep(.b)",
  ":deep(.b) :slotted(.c)",
  ":slotted(.b) :slotted(.c)",
  ".a:deep(.b)",
  ".a:hover:deep(.b)",
  ":not(:deep(.b))",
];

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

const generated = selectors.map((selector) => ({
  selector,
  expected: officialSelector(selector),
}));

if (process.argv.includes("--check")) {
  const expected = `${JSON.stringify(generated, null, 2)}\n`;
  const actual = `${JSON.stringify(rows, null, 2)}\n`;
  if (actual !== expected) {
    console.error("Vue style pseudo oracle fixture is stale; regenerate it");
    process.exit(1);
  }
  console.log(`Vue style pseudo oracle: ${rows.length} rows match @vue/compiler-sfc`);
} else {
  await writeFile(fixturePath, `${JSON.stringify(generated, null, 2)}\n`);
  console.log(`Wrote ${generated.length} Vue style pseudo oracle rows to ${fixturePath}`);
}
