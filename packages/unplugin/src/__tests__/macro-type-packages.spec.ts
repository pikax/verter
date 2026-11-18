/**
 * Macro types declared in packages, through a real native host and a real
 * Vite build. The bundler marks every module that is not a component, the
 * entry or a virtual id as external, so a package's runtime files are never
 * handed to the plugin as source: the declarations have to be found next to
 * them.
 */
import { afterEach, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { build } from "vite";
import { VerterVue } from "../index";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function makeRoot(): string {
  const root = mkdtempSync(join(tmpdir(), "verter-macro-type-packages-"));
  roots.push(root);
  return root;
}

function writeFiles(root: string, files: Record<string, string>): void {
  for (const [path, content] of Object.entries(files)) {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, content);
  }
}

const PACKAGE_MANIFEST = (name: string) =>
  JSON.stringify({
    name,
    version: "0.0.0",
    type: "module",
    main: "./dist/index.js",
    types: "./dist/index.d.ts",
    exports: { ".": { types: "./dist/index.d.ts", default: "./dist/index.js" } },
  });

const isOwnModule = (id: string) =>
  /\.vue(\?|$)/.test(id) ||
  /main\.ts$/.test(id) ||
  id.startsWith("\0") ||
  id.includes("export-helper");

interface BuildOutcome {
  readonly code: string;
  readonly warnings: readonly string[];
}

async function buildProject(root: string): Promise<BuildOutcome> {
  const warnings: string[] = [];
  const verter = VerterVue.vite() as any;
  const result = (await build({
    root,
    configFile: false,
    logLevel: "silent",
    plugins: [verter] as any,
    build: {
      write: false,
      minify: false,
      lib: { entry: join(root, "main.ts"), formats: ["es"], fileName: "out" },
      rollupOptions: {
        external: (id: string) => !isOwnModule(id),
        onwarn: (warning: { message: string }) => {
          warnings.push(warning.message);
        },
      },
    },
  })) as any;
  const outputs = Array.isArray(result) ? result : [result];
  const code = outputs
    .flatMap((output: any) => output.output)
    .filter((chunk: any) => chunk.type === "chunk")
    .map((chunk: any) => chunk.code as string)
    .join("\n");
  return { code, warnings };
}

const MODEL_WITH_BOOLEAN_TYPE = /modelValue["']?\s*:\s*\{[^}]*type:\s*Boolean/;
const MODEL_WITH_NULL_TYPE = /modelValue["']?\s*:\s*\{[^}]*type:\s*null/;

function expectNoUnresolvedMacroType(outcome: BuildOutcome): void {
  const unresolved = outcome.warnings.filter(
    (message) =>
      message.includes("XUnresolvedImportedMacroType") ||
      message.includes("degrades to null") ||
      message.includes("missing macro type dependency"),
  );
  expect(unresolved).toEqual([]);
}

describe("macro types declared in packages", () => {
  it("keeps a model's runtime type when a package barrel is external", async () => {
    const root = makeRoot();
    writeFiles(root, {
      "main.ts": "import App from './App.vue';\nexport default App;\n",
      "App.vue": [
        '<script setup lang="ts">',
        "import type { Enabled } from '@repro/protocol';",
        "const model = defineModel<Enabled>({ required: true });",
        "</script>",
        '<template><select v-model="model"><option :value="true">on</option></select></template>',
        "",
      ].join("\n"),
      "node_modules/@repro/protocol/package.json": PACKAGE_MANIFEST("@repro/protocol"),
      "node_modules/@repro/protocol/dist/index.d.ts": 'export * from "./video.js";\n',
      "node_modules/@repro/protocol/dist/video.d.ts": [
        "export declare const CHOICES: readonly [true, false];",
        "export type Enabled = (typeof CHOICES)[number];",
        "export type Plain = true | false;",
        "",
      ].join("\n"),
      "node_modules/@repro/protocol/dist/index.js": 'export * from "./video.js";\n',
      "node_modules/@repro/protocol/dist/video.js": "export const CHOICES = [true, false];\n",
    });

    const outcome = await buildProject(root);

    expectNoUnresolvedMacroType(outcome);
    expect(outcome.code).toMatch(MODEL_WITH_BOOLEAN_TYPE);
    expect(outcome.code).not.toMatch(MODEL_WITH_NULL_TYPE);
  });

  it("keeps a model's runtime type through a workspace package reached by a link", async () => {
    const root = makeRoot();
    const targets = Array.from({ length: 2 }, (_, index) => `m${index}`);
    const modules = [...targets, "recording-video"];
    writeFiles(root, {
      "main.ts": "import App from './App.vue';\nexport default App;\n",
      "App.vue": [
        '<script setup lang="ts">',
        "import type { RecordingVideoEnabled } from '@fixture/protocol';",
        "const model = defineModel<RecordingVideoEnabled>({ required: true });",
        "</script>",
        '<template><select v-model="model"><option :value="true">on</option></select></template>',
        "",
      ].join("\n"),
      "packages/protocol/package.json": PACKAGE_MANIFEST("@fixture/protocol"),
      "packages/protocol/dist/index.d.ts": modules
        .map((name) => `export * from "./${name}.js";`)
        .join("\n")
        .concat("\n"),
      "packages/protocol/dist/index.js": modules
        .map((name) => `export * from "./${name}.js";`)
        .join("\n")
        .concat("\n"),
      ...Object.fromEntries(
        targets.flatMap((name) => [
          [
            `packages/protocol/dist/${name}.d.ts`,
            `export declare const ${name.toUpperCase()}: number;\n`,
          ],
          [`packages/protocol/dist/${name}.js`, `export const ${name.toUpperCase()} = 0;\n`],
        ]),
      ),
      "packages/protocol/dist/recording-video.d.ts": [
        "export declare const RECORDING_VIDEO_CHOICES: readonly [true, false];",
        "export type RecordingVideoEnabled = (typeof RECORDING_VIDEO_CHOICES)[number];",
        "",
      ].join("\n"),
      "packages/protocol/dist/recording-video.js":
        "export const RECORDING_VIDEO_CHOICES = [true, false];\n",
    });
    mkdirSync(join(root, "node_modules", "@fixture"), { recursive: true });
    symlinkSync(
      join(root, "packages", "protocol"),
      join(root, "node_modules", "@fixture", "protocol"),
      "junction",
    );

    const outcome = await buildProject(root);

    expectNoUnresolvedMacroType(outcome);
    expect(outcome.code).toMatch(MODEL_WITH_BOOLEAN_TYPE);
    expect(outcome.code).not.toMatch(MODEL_WITH_NULL_TYPE);
  });
});
