/**
 * Synthetic fixture workspaces for the endurance harness.
 *
 * Every workspace is generated at runtime into a fresh os.tmpdir() directory —
 * no real project path, name, or code is ever committed or referenced. SFCs
 * deliberately avoid `import … from "vue"` (temp workspaces carry no
 * node_modules), relying on the defineProps/defineEmits/defineSlots compiler
 * macros and plain locals so hover/completion/definition have typed targets,
 * matching the repo's existing raw-LSP fixture idiom.
 */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

/** A `{ relativePath: contents }` map describing a synthetic workspace. */
export type WorkspaceFiles = Readonly<Record<string, string>>;

/** Materialize `files` under a fresh temp dir and return its absolute path. */
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

/** Remove a workspace previously created by {@link materializeWorkspace}. */
export function disposeWorkspace(dir: string): void {
  const resolved = path.resolve(dir);
  if (
    path.dirname(resolved) !== path.resolve(tmpdir()) ||
    !path.basename(resolved).startsWith("verter-endurance-")
  ) {
    return;
  }
  // Windows: a just-killed provider child (or an indexer) can hold a transient
  // lock on the tree — retry briefly instead of failing the suite with EBUSY.
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
        /* spin briefly before retrying the rmdir */
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

/**
 * The child SFC used by the heavy-update loops: a props interface with a
 * toggleable `badge` member, one local, one emit-driven handler, and template
 * usages of each — every probe token has a stable, needle-findable position.
 */
export function heavyUpdateChildContent(): string {
  return [
    '<script setup lang="ts">',
    "interface ChildProps {",
    "  label: string;",
    "  count?: number;",
    "}",
    "const props = defineProps<ChildProps>();",
    'const emit = defineEmits<{ (e: "select", value: string): void }>();',
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

/** A parent consuming `Child.vue` (label/count/select) — cross-file invalidation target. */
export function childConsumerContent(): string {
  return [
    '<script setup lang="ts">',
    'import Child from "./Child.vue";',
    'const heading = "parent";',
    "function onSelect(value: string) {",
    "  console.log(value.length);",
    "}",
    "</script>",
    "",
    "<template>",
    "  <main>",
    "    <h1>{{ heading }}</h1>",
    // No props pre-bound on purpose: attr-name completion on this element must
    // offer the child's FULL prop list (bound attrs are filtered by verter).
    '    <Child @select="onSelect" />',
    "  </main>",
    "</template>",
    "",
  ].join("\n");
}

/**
 * A storm/soak carrier: distinct prop/local/handler names per index, optional
 * chain import of the previous carrier (index 0 is the chain root the typer
 * churns to force downstream invalidation).
 */
export function carrierContent(index: number, childImport?: { path: string; tag: string }): string {
  const prop = `carrierProp${index}`;
  const local = `carrierLocal${index}`;
  const handler = `onCarrier${index}`;
  const lines: string[] = ['<script setup lang="ts">'];
  if (childImport) lines.push(`import ${childImport.tag} from "${childImport.path}";`);
  lines.push(
    `interface Carrier${index}Props {`,
    `  ${prop}: string;`,
    "}",
    "const props = defineProps<" + `Carrier${index}Props` + ">();",
    `const emit = defineEmits<{ (e: "fire", value: string): void }>();`,
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
  // The child is always Carrier<index-1>, whose only prop is carrierProp<index-1>.
  // The bare twin tag is the attr-name completion probe site (a bound prop is
  // filtered from completions on the element that already binds it).
  if (childImport) {
    lines.push(`    <${childImport.tag} :carrierProp${index - 1}="'x'" />`);
    lines.push(`    <${childImport.tag} />`);
  }
  lines.push("  </section>", "</template>", "");
  return lines.join("\n");
}

export interface CarrierSet {
  readonly files: WorkspaceFiles;
  /** Carrier relative paths in chain order (index 0 = root). */
  readonly carriers: readonly string[];
}

/** Build `count` chained carriers under `src/carriers/`. */
export function buildCarrierSet(count: number): CarrierSet {
  const files: Record<string, string> = { "tsconfig.json": ENDURANCE_TSCONFIG };
  const carriers: string[] = [];
  for (let index = 0; index < count; index += 1) {
    const relativePath = `src/carriers/Carrier${index}.vue`;
    const childImport =
      index === 0 ? undefined : { path: `./Carrier${index - 1}.vue`, tag: `Carrier${index - 1}` };
    files[relativePath] = carrierContent(index, childImport);
    carriers.push(relativePath);
  }
  return { files, carriers };
}
