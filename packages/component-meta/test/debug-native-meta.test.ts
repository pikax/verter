/**
 * Diagnostic test: capture raw native metadata for failing nuxt-ui props.
 * DELETE after debugging.
 */
import { existsSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterAll, beforeAll, describe, it, expect } from "vitest";
import { parseMetaUiBenchArgs, prepareMetaUiProject } from "../../benchmark/src/meta-ui-bench.ts";
import { createCheckerByJson } from "../src/compat/checker.js";

const repoRoot = resolve(import.meta.dirname, "../../..");
const uiRoot = resolve(repoRoot, ".integration-tests", "repos", "nuxt-ui");

function tryResolveTypesDeclaration(fullPath: string): string {
  if (!fullPath.includes("node_modules") || !fullPath.endsWith(".vue")) {
    return fullPath;
  }
  const patterns = [
    fullPath.replace(".vue", ".d.vue.ts"),
    fullPath.replace(".vue", ".vue.d.ts"),
    fullPath.replace(".vue", ".d.ts"),
  ];
  for (const candidate of patterns) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return fullPath;
}

describe("debug: native meta output for Alert.vue", () => {
  let checker: Awaited<ReturnType<typeof createCheckerByJson>>;
  const prepared = prepareMetaUiProject(parseMetaUiBenchArgs(["--expected=none"]));

  beforeAll(async () => {
    checker = await createCheckerByJson(
      uiRoot,
      {
        extends: `${prepared.uiRoot}/tsconfig.json`,
        skipLibCheck: true,
        include: prepared.componentSnapshots.map((component) =>
          tryResolveTypesDeclaration(component.absolutePath),
        ),
        exclude: [],
        compilerOptions: {
          ...(prepared.compilerOptions.baseUrl
            ? { baseUrl: prepared.compilerOptions.baseUrl }
            : {}),
          ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
        },
      },
      {
        forceUseTs: true,
        schema: { literalBooleanSchema: true },
        runtimeMode: "dedicated",
      },
    );

    for (const component of prepared.componentSnapshots) {
      checker.updateFile(component.absolutePath, component.transformedSource);
    }
  });

  afterAll(() => {
    checker.close();
  });

  // 30s timeout on this diagnostic test — creating a full
  // nuxt-ui checker + loading Alert.vue takes longer than the
  // default 5s when `pnpm -r --parallel run test` saturates CPU.
  // The test is marked "DELETE after debugging" at the top of the
  // file; bumping the timeout is a minimal flake-fix until that
  // deletion lands.
  it(
    "dump Alert.vue native meta for color, avatar, variant, orientation",
    { timeout: 30_000 },
    async () => {
      const alertPath = resolve(uiRoot, "src/runtime/components/Alert.vue");

      // Access the internal session to get raw native meta
      const session = (checker as any)._session;
      expect(session).toBeDefined();

      const getDeclaredComponentMeta =
        session.getDeclaredComponentMeta ??
        session.getResolvedComponentMeta ??
        session.getComponentMeta;
      const nativeMeta = getDeclaredComponentMeta.call(session, alertPath);
      expect(nativeMeta).toBeDefined();

      const targetProps = ["color", "avatar", "variant", "orientation", "actions"];
      for (const propName of targetProps) {
        const prop = nativeMeta.props.find((p: any) => p.name === propName);
        if (prop) {
          console.log(`\n=== NATIVE PROP: ${propName} ===`);
          console.log("rawType:", prop.rawType);
          console.log("type:", JSON.stringify(prop.type, null, 2).substring(0, 500));
          console.log("description:", prop.description);
          console.log("tags:", JSON.stringify(prop.tags));
          if (prop.typeExpansion) {
            console.log("expansion:", JSON.stringify(prop.typeExpansion));
          }
        } else {
          console.log(`\n=== NATIVE PROP: ${propName} === NOT FOUND`);
        }
      }

      // Also check type registry
      if (nativeMeta.typeRegistry?.length > 0) {
        console.log(`\n=== TYPE REGISTRY: ${nativeMeta.typeRegistry.length} entries ===`);
        for (const entry of nativeMeta.typeRegistry.slice(0, 10)) {
          console.log(`  ${entry.name}: ${JSON.stringify(entry.type).substring(0, 200)}`);
        }
      }

      // Check resolution macros
      if (nativeMeta.resolution?.macros?.length > 0) {
        console.log(`\n=== RESOLUTION MACROS: ${nativeMeta.resolution.macros.length} ===`);
        for (const macro of nativeMeta.resolution.macros) {
          console.log(`  macro: ${macro.declaration?.canonicalSource ?? "unknown"}`);
          console.log(
            `  props: ${macro.props?.length ?? 0}, nativeProps: ${macro.nativeProps?.length ?? 0}`,
          );
          const colorNative = macro.nativeProps?.find((p: any) => p.name === "color");
          if (colorNative) {
            console.log(`  color nativeProp:`, JSON.stringify(colorNative).substring(0, 500));
          }
        }
      }

      const lines: string[] = [];
      for (const propName of targetProps) {
        const prop = nativeMeta.props.find((p: any) => p.name === propName);
        if (prop) {
          lines.push(`\n=== NATIVE PROP: ${propName} ===`);
          lines.push(`rawType: ${prop.rawType}`);
          lines.push(`type: ${JSON.stringify(prop.type, null, 2).substring(0, 800)}`);
          lines.push(`description: ${prop.description}`);
          lines.push(`tags: ${JSON.stringify(prop.tags)}`);
          if (prop.typeExpansion) {
            lines.push(`expansion: ${JSON.stringify(prop.typeExpansion)}`);
          }
        } else {
          lines.push(`\n=== NATIVE PROP: ${propName} === NOT FOUND`);
        }
      }
      if (nativeMeta.typeRegistry?.length > 0) {
        lines.push(`\n=== TYPE REGISTRY: ${nativeMeta.typeRegistry.length} entries ===`);
        for (const entry of nativeMeta.typeRegistry) {
          const full = JSON.stringify(entry.type);
          lines.push(`  ${entry.name} (${full.length} chars): ${full.substring(0, 2000)}`);
        }
      }
      if (nativeMeta.resolution?.macros?.length > 0) {
        lines.push(`\n=== RESOLUTION MACROS: ${nativeMeta.resolution.macros.length} ===`);
        for (const macro of nativeMeta.resolution.macros) {
          lines.push(`  macro: ${macro.declaration?.canonicalSource ?? "unknown"}`);
          lines.push(
            `  props: ${macro.props?.length ?? 0}, nativeProps: ${macro.nativeProps?.length ?? 0}`,
          );
        }
      }
      writeFileSync(
        resolve(process.env.TEMP ?? "D:/tmp", "debug-alert-native.txt"),
        lines.join("\n"),
      );
      expect(true).toBe(true);
    },
  );

  it("dump Tooltip.vue native meta for description-missing props", async () => {
    const tooltipPath = resolve(uiRoot, "src/runtime/components/Tooltip.vue");
    const session = (checker as any)._session;
    const getDeclaredComponentMeta =
      session.getDeclaredComponentMeta ??
      session.getResolvedComponentMeta ??
      session.getComponentMeta;
    const nativeMeta = getDeclaredComponentMeta.call(session, tooltipPath);
    expect(nativeMeta).toBeDefined();

    const targetProps = ["content", "defaultOpen", "delayDuration", "disabled", "portal"];
    for (const propName of targetProps) {
      const prop = nativeMeta.props.find((p: any) => p.name === propName);
      if (prop) {
        console.log(`\n=== TOOLTIP NATIVE PROP: ${propName} ===`);
        console.log("rawType:", prop.rawType);
        console.log("description:", prop.description);
        console.log("tags:", JSON.stringify(prop.tags));
      } else {
        console.log(`\n=== TOOLTIP NATIVE PROP: ${propName} === NOT FOUND`);
      }
    }

    // Check resolution macros for tooltip
    if (nativeMeta.resolution?.macros?.length > 0) {
      for (const macro of nativeMeta.resolution.macros) {
        const defaultOpenNative = macro.nativeProps?.find((p: any) => p.name === "defaultOpen");
        if (defaultOpenNative) {
          console.log(`\n=== Tooltip macro nativeProp: defaultOpen ===`);
          console.log(JSON.stringify(defaultOpenNative).substring(0, 500));
        }
      }
    }

    const lines: string[] = [];
    for (const propName of targetProps) {
      const prop = nativeMeta.props.find((p: any) => p.name === propName);
      if (prop) {
        lines.push(`\n=== TOOLTIP NATIVE PROP: ${propName} ===`);
        lines.push(`rawType: ${prop.rawType}`);
        lines.push(`type: ${JSON.stringify(prop.type, null, 2).substring(0, 500)}`);
        lines.push(`description: ${prop.description}`);
        lines.push(`tags: ${JSON.stringify(prop.tags)}`);
      } else {
        lines.push(`\n=== TOOLTIP NATIVE PROP: ${propName} === NOT FOUND`);
      }
    }
    if (nativeMeta.resolution?.macros?.length > 0) {
      for (const macro of nativeMeta.resolution.macros) {
        for (const propName of ["defaultOpen", "delayDuration", "disabled", "content"]) {
          const np = macro.nativeProps?.find((p: any) => p.name === propName);
          if (np) {
            lines.push(`\n=== Tooltip macro nativeProp: ${propName} ===`);
            lines.push(JSON.stringify(np, null, 2).substring(0, 500));
          }
        }
      }
    }
    writeFileSync(
      resolve(process.env.TEMP ?? "D:/tmp", "debug-tooltip-native.txt"),
      lines.join("\n"),
    );
    expect(true).toBe(true);
  });

  it("dump App.vue native meta for missing props (dir, nonce, scrollBody)", async () => {
    const appPath = resolve(uiRoot, "src/runtime/components/App.vue");
    const session = (checker as any)._session;
    const getDeclaredComponentMeta =
      session.getDeclaredComponentMeta ??
      session.getResolvedComponentMeta ??
      session.getComponentMeta;
    const nativeMeta = getDeclaredComponentMeta.call(session, appPath);
    expect(nativeMeta).toBeDefined();

    const targetProps = ["dir", "nonce", "scrollBody", "locale", "portal"];
    for (const propName of targetProps) {
      const prop = nativeMeta.props.find((p: any) => p.name === propName);
      if (prop) {
        console.log(`\n=== APP NATIVE PROP: ${propName} ===`);
        console.log("rawType:", prop.rawType);
        console.log("type:", JSON.stringify(prop.type).substring(0, 300));
      } else {
        console.log(`\n=== APP NATIVE PROP: ${propName} === NOT FOUND`);
      }
    }

    console.log(`\n=== APP total props: ${nativeMeta.props.length} ===`);
    console.log("prop names:", nativeMeta.props.map((p: any) => p.name).join(", "));

    const lines: string[] = [];
    for (const propName of targetProps) {
      const prop = nativeMeta.props.find((p: any) => p.name === propName);
      if (prop) {
        lines.push(`\n=== APP NATIVE PROP: ${propName} ===`);
        lines.push(`rawType: ${prop.rawType}`);
        lines.push(`type: ${JSON.stringify(prop.type).substring(0, 300)}`);
      } else {
        lines.push(`\n=== APP NATIVE PROP: ${propName} === NOT FOUND`);
      }
    }
    lines.push(`\n=== APP total props: ${nativeMeta.props.length} ===`);
    lines.push(`prop names: ${nativeMeta.props.map((p: any) => p.name).join(", ")}`);
    writeFileSync(resolve(process.env.TEMP ?? "D:/tmp", "debug-app-native.txt"), lines.join("\n"));
    expect(true).toBe(true);
  });
});
