/**
 * A style sub-request of an SSR build is served the component's scoped CSS
 * whatever the order of the hooks and however the drive letter is spelled.
 * It is never handed the whole single-file component.
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build, type Plugin } from "vite";
import { VerterVue } from "../index";

const APP = `<script setup lang="ts">
import { ref } from 'vue';
const n = ref(1);
</script>
<template><p class="a">{{ n }}</p></template>
<style scoped>
.a{color:red}
</style>
`;

const MAIN_INLINE = `import App from './App.vue';
import css from './App.vue?vue&type=style&index=0&inline&lang.css';
export { css, App };
`;

const SCOPED_RED = /\.a\[data-v-[0-9a-f]{8}\]\s*\{\s*color:\s*red/;

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "verter-ssr-inline-style-")).replace(/\\/g, "/");
  writeFileSync(join(root, "App.vue"), APP);
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

function verterPlugin(): Plugin {
  const plugins = ([] as Plugin[]).concat(VerterVue.vite() as Plugin | Plugin[]);
  return plugins[0]!;
}

/** Everything but the fixture's own modules stays outside the bundle. */
function isExternal(id: string): boolean {
  return (
    !/App\.vue/.test(id) &&
    !/main\.ts$/.test(id) &&
    !id.startsWith("\0") &&
    !id.includes("export-helper")
  );
}

interface BuiltFile {
  readonly type: "chunk" | "asset";
  readonly fileName: string;
  readonly code?: string;
  readonly source?: string | Uint8Array;
}

async function buildSsr(
  main: string,
  plugins: Plugin[] = [verterPlugin()],
  extra: Record<string, unknown> = {},
): Promise<BuiltFile[]> {
  const entry = `${root}/main.ts`;
  writeFileSync(entry, main);
  const result = await build({
    root,
    configFile: false,
    logLevel: "silent",
    plugins,
    build: {
      write: false,
      minify: false,
      ssr: entry,
      rollupOptions: { external: isExternal },
      ...extra,
    },
  });
  const outputs = (Array.isArray(result) ? result : [result]) as Array<{ output: BuiltFile[] }>;
  return outputs.flatMap((output) => output.output);
}

function entryCode(files: readonly BuiltFile[]): string {
  const chunk = files.find((file) => file.type === "chunk");
  expect(chunk).toBeDefined();
  return chunk!.code!;
}

/** The chunk's text with the string escapes of an inlined CSS string undone. */
function readable(code: string): string {
  return code.replace(/\\n/g, "\n").replace(/\\t/g, "\t").replace(/\\"/g, '"');
}

function expectScopedCssOnly(code: string): void {
  const text = readable(code);
  expect(text).toMatch(SCOPED_RED);
  expect(text).not.toContain("<script");
  expect(text).not.toContain("<template");
  // The rendered component carries the same scope id as its CSS.
  const scope = /\.a\[(data-v-[0-9a-f]{8})\]/.exec(text)![1];
  expect(text.split(scope).length - 1).toBeGreaterThan(1);
}

/** The default export of a chunk that imports nothing. */
async function evaluate(code: string): Promise<Record<string, unknown>> {
  return import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
}

function deferred(): { promise: Promise<void>; release: () => void } {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

/** The Verter plugin with `onStyleLoad` called whenever a style sub-request starts loading. */
function withStyleLoadSignal(plugin: Plugin, onStyleLoad: () => void): Plugin {
  const original = plugin.load as
    | ((this: unknown, id: string, ...rest: unknown[]) => unknown)
    | { handler: (this: unknown, id: string, ...rest: unknown[]) => unknown }
    | undefined;
  if (!original) throw new Error("the Verter plugin has no load hook");
  const handler = typeof original === "function" ? original : original.handler;
  const load = function (this: unknown, id: string, ...rest: unknown[]) {
    if (id.includes("type=style")) onStyleLoad();
    return handler.call(this, id, ...rest);
  };
  return {
    ...plugin,
    load: (typeof original === "function"
      ? load
      : { ...original, handler: load }) as Plugin["load"],
  };
}

describe("SSR inline styles", () => {
  it("an inline style import in an SSR build is the component's scoped CSS", async () => {
    expectScopedCssOnly(entryCode(await buildSsr(MAIN_INLINE)));
  });

  it("the same when the style is requested before the component", async () => {
    const styleRequested = deferred();
    // Holds the component's own transform until its style sub-request is loading.
    const holdComponent: Plugin = {
      name: "hold-component-until-style-load",
      enforce: "pre",
      async transform(_code, id) {
        if (id.endsWith("/App.vue")) await styleRequested.promise;
        return null;
      },
    };
    const verter = withStyleLoadSignal(verterPlugin(), styleRequested.release);

    expectScopedCssOnly(entryCode(await buildSsr(MAIN_INLINE, [holdComponent, verter])));
  });

  it.runIf(process.platform === "win32")(
    "the same when the request spells the drive letter in lowercase",
    async () => {
      const lower = root.charAt(0).toLowerCase() + root.slice(1);
      const main = `import App from './App.vue';
import css from '${lower}/App.vue?vue&type=style&index=0&inline&used&lang.css';
export { css, App };
`;
      expectScopedCssOnly(entryCode(await buildSsr(main)));
    },
  );

  it("the inline body equals the linked body", async () => {
    const inlineChunk = entryCode(
      await buildSsr(
        `import css from './App.vue?vue&type=style&index=0&inline&lang.css';\nexport { css };\n`,
        [verterPlugin()],
        { cssCodeSplit: false },
      ),
    );
    const inline = (await evaluate(inlineChunk)).css;
    expect(typeof inline).toBe("string");

    const linkedFiles = await buildSsr(
      `import './App.vue?vue&type=style&index=0&lang.css';\nexport const linked = true;\n`,
      [verterPlugin()],
      { cssCodeSplit: false, ssrEmitAssets: true },
    );
    const stylesheet = linkedFiles.find(
      (file) => file.type === "asset" && file.fileName.endsWith(".css"),
    );
    expect(stylesheet).toBeDefined();
    const linked =
      typeof stylesheet!.source === "string"
        ? stylesheet!.source
        : Buffer.from(stylesheet!.source!).toString("utf8");

    // Vite marks where each module's CSS starts in an emitted stylesheet.
    const linkedBody = linked.replace(/\/\*\$vite\$:\d+\*\//g, "");
    expect((inline as string).trim()).toBe(linkedBody.trim());
    expect(linked).toMatch(SCOPED_RED);
  });

  it("a sub-request for a missing block fails the build by name", async () => {
    await expect(
      buildSsr(
        `import css from './App.vue?vue&type=style&index=3&inline&lang.css';\nexport { css };\n`,
      ),
    ).rejects.toThrow(/App\.vue\?vue&type=style&index=3&inline&lang\.css/);
  });
});
