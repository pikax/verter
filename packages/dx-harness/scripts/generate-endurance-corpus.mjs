#!/usr/bin/env node
/** Emit a deterministic Vue/Svelte × TypeScript/JavaScript endurance corpus. */
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

const LANES = [
  { id: "vue-ts", framework: "vue", mode: "ts" },
  { id: "vue-js", framework: "vue", mode: "js" },
  { id: "svelte-ts", framework: "svelte", mode: "ts" },
  { id: "svelte-js", framework: "svelte", mode: "js" },
];

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

const WORDS = ["label", "title", "count", "size", "tone", "mode", "caption", "value"];
const random = prng(seed);
const layouts = [];
for (let index = 0; index < count; index += 1) {
  const first = `${WORDS[Math.floor(random() * WORDS.length)]}${index}`;
  const second = `${WORDS[Math.floor(random() * WORDS.length)]}${index}Extra`;
  const imports = [];
  if (index > 0) imports.push(Math.floor(random() * index));
  layouts.push({
    name: `Corpus${index}`,
    props: [first, second],
    unusedProp: `unusedonly${String(index).padStart(6, "0")}`,
    event: `select${index}`,
    local: `padLocal${index}`,
    handler: `onCorpus${index}Fire`,
    imports,
  });
}

function vueSource(layout, lane) {
  const lines = [lane.mode === "ts" ? '<script setup lang="ts">' : "<script setup>\n// @ts-check"];
  for (const dep of layout.imports) lines.push(`import Corpus${dep} from "./Corpus${dep}.vue";`);
  if (lane.mode === "ts") {
    lines.push(
      `interface ${layout.name}Props {`,
      `  ${layout.props[0]}: string;`,
      `  ${layout.props[1]}?: number;`,
      `  ${layout.unusedProp}?: boolean;`,
      "}",
      `const props = defineProps<${layout.name}Props>();`,
      `const emit = defineEmits<{ (e: "${layout.event}", value: string): void }>();`,
    );
  } else {
    lines.push(
      `const props = defineProps({ ${layout.props[0]}: { type: String, required: true }, ${layout.props[1]}: Number, ${layout.unusedProp}: Boolean });`,
      `const emit = defineEmits(["${layout.event}"]);`,
    );
  }
  lines.push(
    `const ${layout.local} = \`lane:\${props.${layout.props[0]}}\`;`,
    `const ${layout.local}Length = ${layout.local}.length;`,
    `function ${layout.handler}() {`,
    `  emit("${layout.event}", String(props.${layout.props[0]}));`,
    "}",
    "</script>",
    "<template>",
    "  <section>",
    `    <span :title="props.${layout.props[0]}">{{ ${layout.local} }}</span>`,
    `    <button @click="${layout.handler}">fire</button>`,
  );
  for (const dep of layout.imports) {
    lines.push(`    <Corpus${dep} :${layouts[dep].props[0]}="'x'" />`);
    lines.push(`    <Corpus${dep} />`);
  }
  lines.push("  </section>", "</template>", "");
  return lines.join("\n");
}

function svelteSource(layout, lane) {
  const lines = [lane.mode === "ts" ? '<script lang="ts">' : "<script>\n  // @ts-check"];
  for (const dep of layout.imports)
    lines.push(`  import Corpus${dep} from "./Corpus${dep}.svelte";`);
  if (lane.mode === "ts") {
    lines.push(
      '  import type { Snippet } from "svelte";',
      `  interface ${layout.name}Props {`,
      `    ${layout.props[0]}: string;`,
      `    ${layout.props[1]}?: number;`,
      `    ${layout.unusedProp}?: boolean;`,
      "    onselect?: (value: string) => void;",
      "    content?: Snippet<[string]>;",
      "  }",
      `  let { ${layout.props[0]}, ${layout.props[1]}, onselect, content }: ${layout.name}Props = $props();`,
    );
  } else {
    lines.push(
      `  /** @type {{ ${layout.props[0]}: string, ${layout.props[1]}?: number, ${layout.unusedProp}?: boolean, onselect?: (value: string) => void, content?: import("svelte").Snippet<[string]> }} */`,
      `  let { ${layout.props[0]}, ${layout.props[1]}, onselect, content } = $props();`,
    );
  }
  lines.push(
    `  let ${layout.local} = $derived(\`lane:\${${layout.props[0]}}\`);`,
    `  const ${layout.props[0]}Length = ${layout.props[0]}.length;`,
    `  const ${layout.local}Length = ${layout.local}.length;`,
    "  const contentRef = content;",
    `  function ${layout.handler}() {`,
    `    onselect?.(String(${layout.props[0]}));`,
    "  }",
    `  const ${layout.handler}Ref = ${layout.handler};`,
    "</script>",
    `{#snippet corpusSnippet(value)}`,
    "  <strong>{value}</strong>",
    "{/snippet}",
    "<section>",
    `  <span title={${layout.props[0]}}>{${layout.local}}</span>`,
    `  <button onclick={${layout.handler}}>fire</button>`,
    `  {@render corpusSnippet(${layout.local})}`,
  );
  for (const dep of layout.imports) {
    lines.push(`  <Corpus${dep} ${layouts[dep].props[0]}="x" />`);
    lines.push(`  <Corpus${dep} />`);
  }
  lines.push("</section>", "");
  return lines.join("\n");
}

for (const lane of LANES) {
  const laneDir = path.join(targetDir, "src", lane.id);
  mkdirSync(laneDir, { recursive: true });
  for (const layout of layouts) {
    writeFileSync(
      path.join(laneDir, `${layout.name}.${lane.framework}`),
      lane.framework === "vue" ? vueSource(layout, lane) : svelteSource(layout, lane),
    );
  }
  const imports = layouts.slice(0, Math.min(10, count));
  const app =
    lane.framework === "vue"
      ? [
          lane.mode === "ts" ? '<script setup lang="ts">' : "<script setup>\n// @ts-check",
          ...imports.map((layout) => `import ${layout.name} from "./${layout.name}.vue";`),
          'const appHeading = "corpus";',
          "const appHeadingLength = appHeading.length;",
          "</script>",
          "<template>",
          "  <main>",
          "    <h1>{{ appHeading }}</h1>",
          ...imports.map((layout) => `    <${layout.name} :${layout.props[0]}="'x'" />`),
          ...imports.map((layout) => `    <${layout.name} />`),
          "  </main>",
          "</template>",
          "",
        ].join("\n")
      : [
          lane.mode === "ts" ? '<script lang="ts">' : "<script>\n  // @ts-check",
          ...imports.map((layout) => `  import ${layout.name} from "./${layout.name}.svelte";`),
          '  let appHeading = $state("corpus");',
          "  const appHeadingLength = appHeading.length;",
          "</script>",
          "<main>",
          "  <h1>{appHeading}</h1>",
          ...imports.map((layout) => `  <${layout.name} ${layout.props[0]}="x" />`),
          ...imports.map((layout) => `  <${layout.name} />`),
          "</main>",
          "",
        ].join("\n");
  writeFileSync(path.join(laneDir, `App.${lane.framework}`), app);
}

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
  `[generate-endurance-corpus] wrote ${count} components + App per lane across ${LANES.length} lanes to ${targetDir} (seed=${seed})`,
);
