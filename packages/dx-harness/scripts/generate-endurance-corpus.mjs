#!/usr/bin/env node
/**
 * generate-endurance-corpus.mjs — emit a synthetic Vue corpus for the
 * endurance scale lane.
 *
 *   node scripts/generate-endurance-corpus.mjs <targetDir> [count=300] [seed=42]
 *
 * Emits `count` synthetic `.vue` SFCs under `<targetDir>/src/components/`
 * (varied props/emits/slots, each importing 0–2 EARLIER components so the
 * graph is a DAG), a `<targetDir>/src/App.vue` consuming a sample of them,
 * and a strict `<targetDir>/tsconfig.json`. Deterministic for (count, seed):
 * layouts are computed in one PRNG pass, so parents always bind REAL props
 * of their children. No network, no external deps — plain Node ESM.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

const [, , targetDirArg, countArg, seedArg] = process.argv;
if (!targetDirArg) {
  console.error("usage: node generate-endurance-corpus.mjs <targetDir> [count=300] [seed=42]");
  process.exit(2);
}
const targetDir = path.resolve(targetDirArg);
const count = countArg ? Number.parseInt(countArg, 10) : 300;
const seed = seedArg ? Number.parseInt(seedArg, 10) : 42;
if (!Number.isInteger(count) || count < 2) {
  console.error(`count must be an integer >= 2, got ${JSON.stringify(countArg)}`);
  process.exit(2);
}

/** mulberry32 — tiny deterministic PRNG. */
function prng(seedValue) {
  let state = seedValue >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const NOUNS = ["alpha", "beta", "gamma", "delta", "omega", "sigma", "kappa", "zeta"];
const PROP_WORDS = [
  "label",
  "title",
  "count",
  "size",
  "tone",
  "mode",
  "caption",
  "value",
  "status",
  "level",
  "summary",
  "detail",
  "header",
  "footer",
  "prefix",
  "suffix",
];
const EVENT_WORDS = ["select", "submit", "close", "toggle", "change", "focus"];
const PROP_TYPES = ["string", "number", "boolean"];

// ── Pass 1: deterministic layouts (in index order; imports only go backwards) ─
const random = prng(seed);
const pickFrom = (items) => items[Math.floor(random() * items.length)];
const layouts = [];
for (let index = 0; index < count; index += 1) {
  const propCount = 2 + Math.floor(random() * 3); // 2..4 props
  const props = [{ name: `${pickFrom(PROP_WORDS)}${index}`, type: "string", optional: false }];
  while (props.length < propCount) {
    const name = `${pickFrom(PROP_WORDS)}${index}`;
    if (props.some((prop) => prop.name === name)) continue;
    props.push({ name, type: pickFrom(PROP_TYPES), optional: random() < 0.4 });
  }
  const importCount = index === 0 ? 0 : Math.min(index, Math.floor(random() * 3));
  const imports = [];
  while (imports.length < importCount) {
    const dep = Math.floor(random() * index);
    if (!imports.includes(dep)) imports.push(dep);
  }
  imports.sort((a, b) => a - b);
  layouts.push({
    name: `Corpus${index}`,
    noun: pickFrom(NOUNS),
    props,
    eventName: `${pickFrom(EVENT_WORDS)}${index}`,
    local: `padLocal${index}`,
    handler: `onCorpus${index}Fire`,
    hasSlot: random() < 0.5,
    imports,
  });
}

// ── Pass 2: emit sources from layouts ─────────────────────────────────────
function componentSource(layout) {
  const lines = ['<script setup lang="ts">'];
  for (const dep of layout.imports) {
    lines.push(`import Corpus${dep} from "./Corpus${dep}.vue";`);
  }
  lines.push(`interface ${layout.name}Props {`);
  for (const prop of layout.props) {
    lines.push(`  ${prop.name}${prop.optional ? "?" : ""}: ${prop.type};`);
  }
  lines.push(
    "}",
    `const props = defineProps<${layout.name}Props>();`,
    `const emit = defineEmits<{ (e: "${layout.eventName}", value: string): void }>();`,
  );
  if (layout.hasSlot) {
    lines.push("defineSlots<{ default(props: { row: number }): any }>();");
  }
  lines.push(
    `const ${layout.local} = \`${layout.noun}:\${props.${layout.props[0].name}}\`;`,
    `function ${layout.handler}() {`,
    `  emit("${layout.eventName}", String(props.${layout.props[0].name}));`,
    "}",
    "</script>",
    "",
    "<template>",
    "  <section>",
    `    <span :title="props.${layout.props[0].name}">{{ ${layout.local} }}</span>`,
    `    <button @click="${layout.handler}">fire</button>`,
  );
  for (const dep of layout.imports) {
    // The child's FIRST prop is always a required string — bind it cleanly.
    lines.push(`    <Corpus${dep} :${layouts[dep].props[0].name}="'x'" />`);
  }
  lines.push("  </section>", "</template>", "");
  return lines.join("\n");
}

mkdirSync(path.join(targetDir, "src", "components"), { recursive: true });
for (const layout of layouts) {
  writeFileSync(
    path.join(targetDir, "src", "components", `${layout.name}.vue`),
    componentSource(layout),
  );
}

const appImports = [];
for (let k = 0; k < Math.min(10, count); k += 1) {
  appImports.push(Math.floor((k * count) / 10));
}
writeFileSync(
  path.join(targetDir, "src", "App.vue"),
  [
    '<script setup lang="ts">',
    ...appImports.map((i) => `import Corpus${i} from "./components/Corpus${i}.vue";`),
    'const appHeading = "corpus";',
    "</script>",
    "",
    "<template>",
    "  <main>",
    "    <h1>{{ appHeading }}</h1>",
    ...appImports.map((i) => `    <Corpus${i} :${layouts[i].props[0].name}="'x'" />`),
    "  </main>",
    "</template>",
    "",
  ].join("\n"),
);

writeFileSync(
  path.join(targetDir, "tsconfig.json"),
  `${JSON.stringify(
    {
      compilerOptions: {
        allowArbitraryExtensions: true,
        allowImportingTsExtensions: true,
        allowJs: true,
        checkJs: true,
        jsx: "preserve",
        module: "ESNext",
        moduleResolution: "Bundler",
        noEmit: true,
        skipLibCheck: true,
        strict: true,
        target: "ES2022",
      },
      include: ["src/**/*"],
    },
    null,
    2,
  )}\n`,
);

console.log(
  `[generate-endurance-corpus] wrote ${count} components + App.vue to ${targetDir} (seed=${seed})`,
);
