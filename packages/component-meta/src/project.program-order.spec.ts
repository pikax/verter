/**
 * A session configured from a tsconfig orders declarations merged across
 * files by the program's file order — the tsconfig's `files` sequence — the
 * way TypeScript does, never by directory-walk order.
 *
 * Each `bus-<i>.ts` adds one overload `emit(e: { k<i>: 1 }): <answer>` to the
 * global `interface BenchBus`; `scenario.ts` calls `emit` with every key, so
 * the call resolves to the overload of the file listed LAST. With ten files
 * answering `9 - i`, listing them in reverse path order answers `9`, in path
 * order `0`.
 *
 * Requires @verter/native to be built.
 */
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { openComponentMetaSession, shutdownMetaRuntime } from "./project.js";
import type { ComponentMetaSession } from "./project.js";
import { getMetaRuntime } from "./runtime/index.js";
import { createChecker, type ComponentMetaChecker } from "./compat/checker.js";

const COUNT = 10;
const sessions = new Set<ComponentMetaSession | ComponentMetaChecker>();
const roots = new Set<string>();

afterEach(() => {
  for (const session of sessions) session.close();
  sessions.clear();
  shutdownMetaRuntime();
  for (const root of roots) rmSync(root, { recursive: true, force: true });
  roots.clear();
});

function busFile(i: number): string {
  return `bus-${String(i).padStart(4, "0")}.ts`;
}

function busSource(i: number, answer: number): string {
  return `interface BenchBus { emit(e: { k${i}: 1 }): ${answer}; }\n`;
}

/** A tsconfig listing the bus files in `listed` order, through `files` or `include`. */
function tsconfig(listed: number[], through: "files" | "include" = "files"): string {
  const roots = [...listed.map(busFile), "scenario.ts", "Probe.vue"];
  return JSON.stringify({
    compilerOptions: { strict: true, noLib: true, noEmit: true },
    ...(through === "files"
      ? { files: ["lib.bench.d.ts", ...roots] }
      : { files: ["lib.bench.d.ts"], include: roots }),
  });
}

/** A project listing the bus files in `listed` order; file `i` answers `answers[i]`. */
function writeProgram(
  listed: number[],
  answers: number[],
  through: "files" | "include" = "files",
): string {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "verter-program-order-")));
  roots.add(root);
  writeFileSync(join(root, "tsconfig.json"), tsconfig(listed, through));
  writeFileSync(join(root, "lib.bench.d.ts"), "interface Object { toString(): string; }\n");
  for (let i = 0; i < COUNT; i++) {
    writeFileSync(join(root, busFile(i)), busSource(i, answers[i]));
  }
  const keys = Array.from({ length: COUNT }, (_, key) => `k${key}: 1`).join("; ");
  writeFileSync(
    join(root, "scenario.ts"),
    `declare const bus: BenchBus;\ndeclare const ev: { ${keys} };\n` +
      `export const r = bus.emit(ev);\n`,
  );
  writeFileSync(
    join(root, "Probe.vue"),
    `<script setup lang="ts">\nimport { r } from "./scenario";\n` +
      `defineProps<{ value: typeof r }>();\n</script>\n<template><div /></template>\n`,
  );
  return root;
}

async function open(root: string): Promise<ComponentMetaSession> {
  const session = await openComponentMetaSession({
    root,
    tsconfig: join(root, "tsconfig.json"),
  });
  sessions.add(session);
  return session;
}

/**
 * Opens `root` and overlays every program file in path order, independently
 * of the configured root sequence used for declaration precedence.
 */
async function openLoaded(root: string): Promise<ComponentMetaSession> {
  const session = await open(root);
  for (const file of ["lib.bench.d.ts", ...forward.map(busFile), "scenario.ts"]) {
    session.updateFile(file, readFileSync(join(root, file), "utf8"));
  }
  return session;
}

/** The literal the probe component's `value` prop settles on. */
async function answer(session: ComponentMetaSession): Promise<unknown> {
  const meta = await session.getComponentMeta("Probe.vue");
  const value = meta._verter?.props.find((prop) => prop.name === "value");
  return value?.type;
}

function literal(value: number) {
  return { kind: "literal", value };
}

const forward = Array.from({ length: COUNT }, (_, i) => i);
const reverse = [...forward].reverse();
const answers = forward.map((i) => COUNT - 1 - i);

describe("tsconfig program file order", () => {
  it("orders merged overloads by the tsconfig's authored files", async () => {
    expect(await answer(await openLoaded(writeProgram(reverse, answers)))).toEqual(literal(9));
    expect(await answer(await openLoaded(writeProgram(forward, answers)))).toEqual(literal(0));
  });

  it("answers the same cold, warm and after an edit as a cold read of the edited files", async () => {
    const session = await openLoaded(writeProgram(reverse, answers));
    const cold = await answer(session);
    expect(cold).toEqual(literal(9));
    expect(await answer(session)).toEqual(cold);

    // `bus-0000.ts` is listed last and tried first: editing its answer moves
    // the probe; editing the first-listed file does not.
    const edited = [...answers];
    edited[0] = 1000;
    edited[COUNT - 1] = 2000;
    session.updateFile(busFile(0), busSource(0, edited[0]));
    session.updateFile(busFile(COUNT - 1), busSource(COUNT - 1, edited[COUNT - 1]));
    const editedCold = await answer(await openLoaded(writeProgram(reverse, edited)));
    expect(editedCold).toEqual(literal(1000));
    expect(await answer(session)).toEqual(editedCold);
  });

  it.each(["json", "jsonc", "schema-and-aliases", "extends", "array-extends", "package-extends"])(
    "gives a %s tsconfig whose include order changed its own engine",
    async (format) => {
      const root = writeProgram(reverse, answers, "include");
      const writeConfig = (listed: number[]) => {
        const config: {
          $schema?: string;
          compilerOptions: Record<string, unknown>;
          files: string[];
          include: string[];
        } = {
          compilerOptions: { strict: true, noLib: true, noEmit: true },
          files: ["lib.bench.d.ts"],
          include: [...listed.map(busFile), "scenario.ts", "Probe.vue"],
        };
        let path = join(root, "tsconfig.json");
        if (format === "schema-and-aliases") {
          config.$schema = "https://json.schemastore.org/tsconfig";
          config.compilerOptions.baseUrl = ".";
          config.compilerOptions.paths = { "@/*": ["src/*"] };
          config.include.push("src/**/*.vue");
        } else if (format.endsWith("extends")) {
          if (format === "package-extends") {
            const packageDir = join(root, "node_modules", "config-package");
            mkdirSync(packageDir, { recursive: true });
            writeFileSync(join(packageDir, "package.json"), '{ "tsconfig": "../../base.json" }');
            writeFileSync(path, '{ "extends": "config-package" }');
          } else {
            writeFileSync(
              path,
              JSON.stringify({
                extends: format === "array-extends" ? ["./base.json"] : "./base.json",
              }),
            );
          }
          path = join(root, "base.json");
        }
        const raw = JSON.stringify(config, null, 2);
        writeFileSync(path, format === "jsonc" ? `// roots\n${raw.slice(0, -1)},\n}` : raw);
      };
      writeConfig(reverse);
      expect(await answer(await openLoaded(root))).toEqual(literal(9));
      const checker = await createChecker(join(root, "tsconfig.json"));
      sessions.add(checker);
      expect(getMetaRuntime().diagnostics.enginesCreated).toBe(1);
      checker.close();
      writeConfig(forward);
      expect(await answer(await openLoaded(root))).toEqual(literal(0));
      expect(getMetaRuntime().diagnostics.enginesCreated).toBe(2);
    },
  );
});

describe("tsconfig ownership and path aliases", () => {
  it.each(["files", "include"])(
    "keeps aliases through unlisted imports with %s roots",
    async (membership) => {
      const root = realpathSync(mkdtempSync(join(tmpdir(), "verter-imported-alias-")));
      roots.add(root);
      mkdirSync(join(root, "src"));
      writeFileSync(
        join(root, "tsconfig.json"),
        JSON.stringify({
          compilerOptions: { strict: true, baseUrl: ".", paths: { "@/*": ["src/*"] } },
          ...(membership === "files"
            ? { files: ["src/Member.vue"] }
            : { include: ["src/**/*.vue"] }),
        }),
      );
      writeFileSync(
        join(root, "src/Member.vue"),
        `<script setup lang="ts">
import type { P } from "./props";
defineProps<P>();
</script><template><div /></template>`,
      );
      writeFileSync(
        join(root, "src/props.ts"),
        `import type { Item } from "@/types";
export interface P extends Item {}`,
      );
      writeFileSync(join(root, "src/types.ts"), `export interface Item { label: "item" }`);
      const session = await open(root);
      const read = async () =>
        (await session.getComponentMeta("src/Member.vue"))._verter?.props.find(
          (prop) => prop.name === "label",
        )?.type;
      expect(await read()).toEqual({ kind: "literal", value: "item" });
      expect(await read()).toEqual({ kind: "literal", value: "item" });
      session.updateFile("src/types.ts", `export interface Item { label: "edited" }`);
      expect(await read()).toEqual({ kind: "literal", value: "edited" });
    },
  );

  function writeAliasProject(): string {
    const root = realpathSync(mkdtempSync(join(tmpdir(), "verter-program-alias-")));
    roots.add(root);
    mkdirSync(join(root, "src"), { recursive: true });
    mkdirSync(join(root, "outside"), { recursive: true });
    writeFileSync(
      join(root, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: { strict: true, baseUrl: ".", paths: { "@/*": ["src/*"] } },
        include: ["src/**/*.ts", "src/**/*.vue"],
      }),
    );
    writeFileSync(join(root, "src", "types.ts"), `export interface Item { label: "item" }\n`);
    writeFileSync(
      join(root, "src", "Member.vue"),
      `<script setup lang="ts">\nimport type { Item } from "@/types";\n` +
        `defineProps<{ label: Item["label"] }>();\n</script>\n<template><div /></template>\n`,
    );
    writeFileSync(
      join(root, "outside", "Outside.vue"),
      `<script setup lang="ts">\nimport type { Item } from "@/types";\n` +
        `import type { Item as Relative } from "../src/types";\n` +
        `defineProps<{ label: Item["label"]; relative: Relative["label"] }>();\n` +
        `</script>\n<template><div /></template>\n`,
    );
    return root;
  }

  it("resolves a tsconfig path alias from a component the tsconfig includes", async () => {
    const session = await open(writeAliasProject());
    const meta = await session.getComponentMeta("src/Member.vue");
    const label = meta._verter?.props.find((prop) => prop.name === "label");
    expect(label?.type).toEqual({ kind: "literal", value: "item" });
  });

  it("publishes a component outside the tsconfig's membership without the tsconfig's aliases", async () => {
    const session = await open(writeAliasProject());
    const meta = await session.getComponentMeta("outside/Outside.vue");
    const prop = (name: string) => meta._verter?.props.find((p) => p.name === name)?.type;
    // A relative import resolves as anywhere else.
    expect(prop("relative")).toEqual({ kind: "literal", value: "item" });
    // The file is not in the tsconfig's program, so — as in TypeScript — the
    // tsconfig's `paths` do not govern it: the aliased import stays an
    // unresolved shallow carrier.
    expect(prop("label")).toEqual({
      kind: "indexedAccess",
      objectType: { kind: "ref", name: "Item" },
      indexType: { kind: "literal", value: "label" },
    });
  });
});
