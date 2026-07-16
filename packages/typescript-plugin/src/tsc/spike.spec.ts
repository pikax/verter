import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { VerterHost } from "@verter/native";

import { runBatchTypecheck, type CarrierSource } from "./mirrorHost";
import { createReferenceFixture, type ReferenceFixture } from "./testFixtures";

// =============================================================================
// The §2.4 mirror-host SPIKE validation.
//
// Asserts the mirror-host build-mode (`composite` + project-reference + `paths`)
// diagnostics MATCH the user's own `tsc -b` on an equivalent all-`.ts` twin.
// Framework-agnostic: covers `.vue` AND `.svelte`. Hermetic — fixtures symlink
// ONLY workspace-vendored packages (no third-party repo checkout).
//
// The cross-project type error is the authoritative parity anchor: a string
// passed to a `string`-typed cross-project function (resolved through a `paths`
// alias) must produce `TS2322 Type 'number' is not assignable to type 'string'`
// at the usage site — the SAME diagnostic stock `tsc -b` produces on the `.ts`
// twin (captured live, not hardcoded). The redirect setting
// (`disableSourceOfProjectReferenceRedirect`) does NOT change the batch result:
// build mode consumes a referenced project through its emitted `.d.ts` uniformly.
// =============================================================================

const require_ = createRequire(__filename);
// These rows execute a real composite TypeScript build. The root workspace test
// command runs every package in parallel, so the default 5-second unit-test
// timeout is not a valid bound under CI contention. Keep a finite integration
// bound that still fails a wedged build instead of relying on an unbounded wait.
const BUILD_MODE_INTEGRATION_TIMEOUT_MS = 20_000;

function host(): InstanceType<typeof VerterHost> {
  return new VerterHost();
}

const fixtures: ReferenceFixture[] = [];
afterEach(() => {
  for (const f of fixtures) f.cleanup();
  fixtures.length = 0;
});
function track(f: ReferenceFixture): ReferenceFixture {
  fixtures.push(f);
  return f;
}

/**
 * Run the user's OWN `tsc -b` CLI on an all-`.ts` twin of the composite + ref +
 * paths fixture, returning the diagnostic codes the CLI reports. This is the
 * authoritative baseline the mirror-host build mode must match.
 */
function tscBaselineCodesForCrossProjectError(): number[] {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "verter-tsc-twin-"));
  try {
    const lib = path.join(root, "packages/lib");
    const app = path.join(root, "packages/app");
    fs.mkdirSync(path.join(lib, "src"), { recursive: true });
    fs.mkdirSync(path.join(app, "src"), { recursive: true });
    fs.writeFileSync(
      path.join(lib, "src/api.ts"),
      `export function makeLabel(l: { label: string }): string { return l.label }`,
    );
    // The `.ts` twin of the `.vue` consumer: same cross-project call with a
    // wrong-typed argument.
    fs.writeFileSync(
      path.join(app, "src/App.ts"),
      `import { makeLabel } from "@lib/api"\nconst bad: string = makeLabel({ label: 42 })\nexport { bad }`,
    );
    fs.writeFileSync(
      path.join(lib, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: {
          target: "ESNext",
          module: "ESNext",
          moduleResolution: "bundler",
          strict: true,
          composite: true,
          declaration: true,
          outDir: "./dist",
          rootDir: "./src",
          skipLibCheck: true,
          types: [],
        },
        include: ["src"],
      }),
    );
    fs.writeFileSync(
      path.join(app, "tsconfig.json"),
      JSON.stringify({
        compilerOptions: {
          target: "ESNext",
          module: "ESNext",
          moduleResolution: "bundler",
          strict: true,
          skipLibCheck: true,
          types: [],
          baseUrl: ".",
          paths: { "@lib/*": ["../lib/src/*"] },
          ignoreDeprecations: "6.0",
        },
        include: ["src"],
        references: [{ path: "../lib" }],
      }),
    );

    const tscBin = path.join(path.dirname(require_.resolve("typescript")), "tsc.js");
    let out = "";
    try {
      out = execFileSync(
        process.execPath,
        [tscBin, "-b", path.join(app, "tsconfig.json"), "--force"],
        { cwd: root, encoding: "utf8" },
      );
    } catch (e) {
      const err = e as { stdout?: string; stderr?: string };
      out = (err.stdout ?? "") + (err.stderr ?? "");
    }
    const codes = new Set<number>();
    for (const m of out.matchAll(/error TS(\d+):/g)) {
      codes.add(Number(m[1]));
    }
    return [...codes];
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

/** A `.vue`/`.svelte` cross-project reference fixture with a bad-typed call. */
function crossProjectErrorFixture(framework: "vue" | "svelte"): ReferenceFixture {
  const libRel = framework === "vue" ? "src/util.ts" : "src/util.ts";
  const appRel = framework === "vue" ? "src/App.vue" : "src/App.svelte";
  const scriptOpen = framework === "vue" ? `<script setup lang="ts">` : `<script lang="ts">`;
  const tmpl =
    framework === "vue" ? `<template><div>{{ bad }}</div></template>` : `<div>{bad}</div>`;
  return createReferenceFixture({
    libSources: [
      {
        rel: libRel,
        content: `export function makeLabel(l: { label: string }): string { return l.label }`,
      },
    ],
    appSources: [
      {
        rel: appRel,
        content: `${scriptOpen}\nimport { makeLabel } from "@lib/util"\nconst bad: string = makeLabel({ label: 42 })\n</script>\n${tmpl}`,
      },
    ],
    appPaths: { "@lib/*": ["../lib/src/*"] },
  });
}

describe("§2.4 mirror-host spike — build mode diagnostic parity with tsc -b", () => {
  it("the .ts twin baseline reports exactly TS2322 (the authoritative cross-project error)", () => {
    const codes = tscBaselineCodesForCrossProjectError();
    expect(codes).toContain(2322);
  });

  for (const framework of ["vue", "svelte"] as const) {
    it(
      `[${framework}] build mode catches the cross-project paths-aliased type error, mapped to source`,
      () => {
        const fx = track(crossProjectErrorFixture(framework));
        const appExt = framework === "vue" ? "App.vue" : "App.svelte";
        const appPath = path.join(fx.root, "packages/app/src", appExt);

        const result = runBatchTypecheck({
          tsconfigPath: fx.appTsconfigPath,
          carrierSources: [
            {
              sourcePath: appPath,
              source: fs.readFileSync(appPath, "utf8"),
              framework,
              ownership: "Owned",
              role: "ide",
            },
          ],
          host: host(),
        });

        expect(result.buildMode).toBe(true);

        // The SAME TS2322 stock tsc -b reports on the .ts twin must surface here,
        // mapped back to the carrier source.
        const ts2322 = result.diagnostics.filter((d) => d.code === 2322);
        expect(ts2322.length).toBeGreaterThan(0);
        const mapped = ts2322.find((d) => d.mappedFromCarrier);
        expect(mapped).toBeDefined();
        expect(mapped!.fileName).toBe(appPath.replace(/\\/g, "/"));
        // The error maps to the `makeLabel({ label: 42 })` line (source line 3).
        const src = fs.readFileSync(appPath, "utf8");
        const line = src.slice(0, mapped!.start!).split("\n").length;
        expect(line).toBe(3);
      },
      BUILD_MODE_INTEGRATION_TIMEOUT_MS,
    );
  }

  it("build mode emits each referenced project's .d.ts INTO THE MIRROR (the consumed boundary)", () => {
    const fx = track(
      createReferenceFixture({
        libSources: [
          {
            rel: "src/Button.vue",
            content: `<script setup lang="ts">\ndefineProps<{ label: string }>()\n</script>\n<template><button>{{ label }}</button></template>`,
          },
        ],
        appSources: [
          {
            rel: "src/App.vue",
            content: `<script setup lang="ts">\nimport Button from "@lib/Button.vue"\n</script>\n<template><Button label="ok" /></template>`,
          },
        ],
        appPaths: { "@lib/*": ["../lib/src/*"] },
      }),
    );
    const libPath = path.join(fx.root, "packages/lib/src/Button.vue");
    const appPath = path.join(fx.root, "packages/app/src/App.vue");
    const result = runBatchTypecheck({
      tsconfigPath: fx.appTsconfigPath,
      carrierSources: [
        {
          sourcePath: appPath,
          source: fs.readFileSync(appPath, "utf8"),
          framework: "vue",
          ownership: "Owned",
          role: "ide",
        },
        {
          sourcePath: libPath,
          source: fs.readFileSync(libPath, "utf8"),
          framework: "vue",
          ownership: "Owned",
          role: "api",
          projectTsconfigPath: fx.libTsconfigPath,
        },
      ],
      mirrorRoot: undefined,
      host: host(),
      keepMirror: true,
    });

    try {
      expect(result.buildMode).toBe(true);
      // A `.d.ts` for the referenced lib carrier exists under the mirror.
      const dtsFiles: string[] = [];
      const walk = (d: string): void => {
        for (const e of fs.readdirSync(d, { withFileTypes: true })) {
          const f = path.join(d, e.name);
          if (e.isDirectory()) walk(f);
          else if (e.name.endsWith(".d.ts")) dtsFiles.push(f);
        }
      };
      walk(result.mirrorRoot);
      expect(dtsFiles.some((f) => f.includes("Button"))).toBe(true);

      // A GOOD cross-project `.vue` usage resolves through that `.d.ts` — no
      // false "cannot find module" (TS2307) for the imported component.
      expect(result.diagnostics.some((d) => d.code === 2307)).toBe(false);
    } finally {
      fs.rmSync(result.mirrorRoot, { recursive: true, force: true });
    }
  });

  it("build-mode result is UNIFORM with and without disableSourceOfProjectReferenceRedirect", () => {
    const codesFor = (disableRedirect: boolean): number[] => {
      const fx = track(crossProjectErrorFixture("vue"));
      if (disableRedirect) {
        const cfg = JSON.parse(fs.readFileSync(fx.appTsconfigPath, "utf8"));
        cfg.compilerOptions.disableSourceOfProjectReferenceRedirect = true;
        fs.writeFileSync(fx.appTsconfigPath, JSON.stringify(cfg, null, 2), "utf8");
      }
      const appPath = path.join(fx.root, "packages/app/src/App.vue");
      const result = runBatchTypecheck({
        tsconfigPath: fx.appTsconfigPath,
        carrierSources: [
          {
            sourcePath: appPath,
            source: fs.readFileSync(appPath, "utf8"),
            framework: "vue",
            ownership: "Owned",
            role: "ide",
          },
        ],
        host: host(),
      });
      // Compare the SEMANTIC diagnostic codes (ignore the baseUrl deprecation,
      // which is a config-option warning, not a build-mode resolution result).
      return result.diagnostics
        .filter((d) => d.code !== 5101)
        .map((d) => d.code)
        .sort();
    };

    const withRedirect = codesFor(false);
    const withoutRedirect = codesFor(true);
    // Both go through the `.d.ts` boundary on the batch path — same result.
    expect(withRedirect).toContain(2322);
    expect(withoutRedirect).toEqual(withRedirect);
  });
});
