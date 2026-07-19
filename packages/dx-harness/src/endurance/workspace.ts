/** Synthetic, framework-neutral fixture workspaces for endurance scenarios. */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { DEFAULT_ENDURANCE_LANE, type EnduranceLane, type EnduranceLanguageMode } from "./types.js";

export type WorkspaceFiles = Readonly<Record<string, string>>;

export function materializeWorkspace(
  files: WorkspaceFiles,
  prefix = "verter-endurance-ws-",
): string {
  const dir = mkdtempSync(path.join(tmpdir(), prefix));
  for (const [relativePath, contents] of Object.entries(files)) {
    const absolute = path.join(dir, relativePath);
    mkdirSync(path.dirname(absolute), { recursive: true });
    writeFileSync(absolute, contents);
  }
  return dir;
}

export function disposeWorkspace(dir: string): void {
  const resolved = path.resolve(dir);
  if (
    path.dirname(resolved) !== path.resolve(tmpdir()) ||
    !path.basename(resolved).startsWith("verter-endurance-")
  ) {
    return;
  }
  let lastError: unknown;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      rmSync(resolved, { recursive: true, force: true });
      return;
    } catch (error) {
      lastError = error;
      if ((error as NodeJS.ErrnoException)?.code !== "EBUSY") break;
      const until = Date.now() + 250 * (attempt + 1);
      while (Date.now() < until) {
        /* wait for a just-killed provider to release Windows file handles */
      }
    }
  }
  throw lastError;
}

export const ENDURANCE_TSCONFIG = JSON.stringify(
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
);

export function laneDirectory(lane: EnduranceLane): string {
  return `src/${lane.id}`;
}

export function carrierPath(lane: EnduranceLane, stem: string): string {
  return `${laneDirectory(lane)}/${stem}.${lane.framework}`;
}

function vueScriptOpen(mode: EnduranceLanguageMode): string {
  return mode === "ts" ? '<script setup lang="ts">' : "<script setup>\n// @ts-check";
}

function svelteScriptOpen(mode: EnduranceLanguageMode): string {
  return mode === "ts" ? '<script lang="ts">' : "<script>\n  // @ts-check";
}

export function heavyUpdateChildContent(lane: EnduranceLane = DEFAULT_ENDURANCE_LANE): string {
  if (lane.framework === "svelte") {
    const props =
      lane.mode === "ts"
        ? [
            "  interface ChildProps {",
            "    label: string;",
            "    count?: number;",
            "    onselect?: (value: string) => void;",
            "  }",
            "  let { label, count, onselect }: ChildProps = $props();",
          ]
        : [
            "  /** @type {{ label: string, count?: number, onselect?: (value: string) => void }} */",
            "  let { label, count, onselect } = $props();",
          ];
    return [
      svelteScriptOpen(lane.mode),
      ...props,
      "  let greeting = $derived(`hi:${label}`);",
      "  const greetingLength = greeting.length;",
      "  function pick() {",
      "    onselect?.(label);",
      "  }",
      "</script>",
      "",
      '<button class="child" title={label} onclick={pick}>{greeting}</button>',
      "",
    ].join("\n");
  }

  const props =
    lane.mode === "ts"
      ? [
          "interface ChildProps {",
          "  label: string;",
          "  count?: number;",
          "}",
          "const props = defineProps<ChildProps>();",
          'const emit = defineEmits<{ (e: "select", value: string): void }>();',
        ]
      : [
          "const props = defineProps({",
          "  label: { type: String, required: true },",
          "  count: Number,",
          "});",
          'const emit = defineEmits(["select"]);',
        ];
  return [
    vueScriptOpen(lane.mode),
    ...props,
    "const greeting = `hi:${props.label}`;",
    "function pick() {",
    '  emit("select", props.label);',
    "}",
    "</script>",
    "",
    "<template>",
    '  <button class="child" :title="props.label" @click="pick">{{ greeting }}</button>',
    "</template>",
    "",
  ].join("\n");
}

export function childConsumerContent(lane: EnduranceLane = DEFAULT_ENDURANCE_LANE): string {
  const childPath = `./Child.${lane.framework}`;
  if (lane.framework === "svelte") {
    return [
      svelteScriptOpen(lane.mode),
      `  import Child from "${childPath}";`,
      '  let heading = $state("parent");',
      "  const headingLength = heading.length;",
      lane.mode === "ts" ? "  function onSelect(value: string) {" : "  function onSelect(value) {",
      "    console.log(value.length);",
      "  }",
      "</script>",
      "",
      "<main>",
      "  <h1>{heading}</h1>",
      "  <Child onselect={onSelect} />",
      "</main>",
      "",
    ].join("\n");
  }
  return [
    vueScriptOpen(lane.mode),
    `import Child from "${childPath}";`,
    'const heading = "parent";',
    lane.mode === "ts" ? "function onSelect(value: string) {" : "function onSelect(value) {",
    "  console.log(value.length);",
    "}",
    "</script>",
    "",
    "<template>",
    "  <main>",
    "    <h1>{{ heading }}</h1>",
    '    <Child @select="onSelect" />',
    "  </main>",
    "</template>",
    "",
  ].join("\n");
}

export function carrierContent(
  index: number,
  childImport?: { path: string; tag: string },
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): string {
  const prop = `carrierProp${index}`;
  const unusedProp = `carrierUnusedOnly${String(index).padStart(6, "0")}`;
  const local = `carrierLocal${index}`;
  const handler = `onCarrier${index}`;
  if (lane.framework === "svelte") {
    const lines = [svelteScriptOpen(lane.mode)];
    if (childImport) lines.push(`  import ${childImport.tag} from "${childImport.path}";`);
    if (lane.mode === "ts") {
      lines.push(
        '  import type { Snippet } from "svelte";',
        `  interface Carrier${index}Props {`,
        `    ${prop}: string;`,
        `    ${unusedProp}?: boolean;`,
        "    onfire?: (value: string) => void;",
        "    content?: Snippet<[string]>;",
        "  }",
        `  let { ${prop}, onfire, content }: Carrier${index}Props = $props();`,
      );
    } else {
      lines.push(
        `  /** @type {{ ${prop}: string, ${unusedProp}?: boolean, onfire?: (value: string) => void, content?: import("svelte").Snippet<[string]> }} */`,
        `  let { ${prop}, onfire, content } = $props();`,
      );
    }
    lines.push(
      `  let ${local} = $derived(\`c${index}:\${${prop}}\`);`,
      `  const ${prop}Length = ${prop}.length;`,
      `  const ${local}Length = ${local}.length;`,
      "  const contentRef = content;",
      `  function ${handler}() {`,
      `    onfire?.(${prop});`,
      "  }",
      `  const ${handler}Ref = ${handler};`,
      "</script>",
      "",
      "{#snippet carrierSnippet(value)}",
      "  <strong>{value}</strong>",
      "{/snippet}",
      "<section>",
      `  <span title={${prop}}>{${local}}</span>`,
      `  <button onclick={${handler}}>go</button>`,
      `  {@render carrierSnippet(${local})}`,
    );
    if (childImport) {
      lines.push(`  <${childImport.tag} carrierProp${index - 1}="x" />`);
      lines.push(`  <${childImport.tag} />`);
    }
    lines.push("</section>", "");
    return lines.join("\n");
  }

  const lines: string[] = [vueScriptOpen(lane.mode)];
  if (childImport) lines.push(`import ${childImport.tag} from "${childImport.path}";`);
  if (lane.mode === "ts") {
    lines.push(
      `interface Carrier${index}Props {`,
      `  ${prop}: string;`,
      `  ${unusedProp}?: boolean;`,
      "}",
      `const props = defineProps<Carrier${index}Props>();`,
      'const emit = defineEmits<{ (e: "fire", value: string): void }>();',
    );
  } else {
    lines.push(
      `const props = defineProps({ ${prop}: { type: String, required: true }, ${unusedProp}: Boolean });`,
      'const emit = defineEmits(["fire"]);',
    );
  }
  lines.push(
    `const ${local} = \`c${index}:\${props.${prop}}\`;`,
    `function ${handler}() {`,
    `  emit("fire", props.${prop});`,
    "}",
    "</script>",
    "",
    "<template>",
    "  <section>",
    `    <span :title="props.${prop}">{{ ${local} }}</span>`,
    `    <button @click="${handler}">go</button>`,
  );
  if (childImport) {
    lines.push(`    <${childImport.tag} :carrierProp${index - 1}="'x'" />`);
    lines.push(`    <${childImport.tag} />`);
  }
  lines.push("  </section>", "</template>", "");
  return lines.join("\n");
}

export interface CarrierSet {
  readonly files: WorkspaceFiles;
  readonly carriers: readonly string[];
  readonly lane: EnduranceLane;
}

export function buildCarrierSet(
  count: number,
  lane: EnduranceLane = DEFAULT_ENDURANCE_LANE,
): CarrierSet {
  const files: Record<string, string> = { "tsconfig.json": ENDURANCE_TSCONFIG };
  const carriers: string[] = [];
  for (let index = 0; index < count; index += 1) {
    const relativePath = `${laneDirectory(lane)}/carriers/Carrier${index}.${lane.framework}`;
    const childImport =
      index === 0
        ? undefined
        : {
            path: `./Carrier${index - 1}.${lane.framework}`,
            tag: `Carrier${index - 1}`,
          };
    files[relativePath] = carrierContent(index, childImport, lane);
    carriers.push(relativePath);
  }
  return { files, carriers, lane };
}
