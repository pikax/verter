/**
 * @ai-generated - Verifies meta-ui benchmark project preparation and component discovery.
 */

import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  EXPECTED_ARTIFACTS_MANIFEST,
  maybeRunGarbageCollection,
  parseMetaUiBenchArgs,
  prepareMetaUiProject,
  tryLoadExpectedArtifacts,
} from "./meta-ui-bench.js";

function writeJson(filePath: string, value: unknown): void {
  writeFileSync(filePath, JSON.stringify(value, null, 2), "utf8");
}

describe("prepareMetaUiProject", () => {
  it("discovers nested vue components under src/runtime/components recursively", () => {
    const uiRoot = mkdtempSync(resolve(tmpdir(), "verter-meta-ui-bench-"));
    mkdirSync(resolve(uiRoot, ".nuxt"), { recursive: true });
    mkdirSync(resolve(uiRoot, "src", "runtime", "components", "nested"), { recursive: true });
    writeJson(resolve(uiRoot, ".nuxt", "tsconfig.app.json"), { compilerOptions: { paths: {} } });
    writeJson(resolve(uiRoot, ".nuxt", "tsconfig.shared.json"), { compilerOptions: { paths: {} } });
    writeFileSync(
      resolve(uiRoot, "src", "runtime", "components", "Root.vue"),
      "<template />",
      "utf8",
    );
    writeFileSync(
      resolve(uiRoot, "src", "runtime", "components", "nested", "Nested.vue"),
      "<template />",
      "utf8",
    );

    const args = parseMetaUiBenchArgs([`--ui-root=${uiRoot}`]);
    const prepared = prepareMetaUiProject(args);

    expect(prepared.componentSnapshots.map((component) => component.relativePath)).toEqual([
      "src/runtime/components/nested/Nested.vue",
      "src/runtime/components/Root.vue",
    ]);
  });
});

describe("parseMetaUiBenchArgs", () => {
  it("supports expected artifact reuse flags", () => {
    const args = parseMetaUiBenchArgs([
      "--output-dir=D:/bench/out",
      "--expected-dir=D:/bench/expected",
      "--build-expected-only",
    ]);

    expect(args.outputDir).toBe(resolve("D:/bench/out"));
    expect(args.expectedDir).toBe(resolve("D:/bench/expected"));
    expect(args.buildExpectedOnly).toBe(true);
  });

  it("derives the default expected-dir from output-dir when not explicitly set", () => {
    const args = parseMetaUiBenchArgs(["--output-dir=D:/bench/out"]);

    expect(args.outputDir).toBe(resolve("D:/bench/out"));
    expect(args.expectedDir).toBe(resolve("D:/bench/out", ".expected-vue-component-meta"));
  });
});

describe("tryLoadExpectedArtifacts", () => {
  it("reuses expected artifacts only when the manifest matches the prepared project", () => {
    const uiRoot = mkdtempSync(resolve(tmpdir(), "verter-meta-ui-expected-"));
    const expectedDir = resolve(uiRoot, ".expected");
    mkdirSync(resolve(uiRoot, ".nuxt"), { recursive: true });
    mkdirSync(resolve(uiRoot, "src", "runtime", "components", "nested"), { recursive: true });
    writeJson(resolve(uiRoot, ".nuxt", "tsconfig.app.json"), { compilerOptions: { paths: {} } });
    writeJson(resolve(uiRoot, ".nuxt", "tsconfig.shared.json"), { compilerOptions: { paths: {} } });
    writeFileSync(
      resolve(uiRoot, "src", "runtime", "components", "Root.vue"),
      "<template />",
      "utf8",
    );
    writeFileSync(
      resolve(uiRoot, "src", "runtime", "components", "nested", "Nested.vue"),
      "<template />",
      "utf8",
    );

    const prepared = prepareMetaUiProject(parseMetaUiBenchArgs([`--ui-root=${uiRoot}`]));
    mkdirSync(resolve(expectedDir, "src", "runtime", "components", "nested"), { recursive: true });
    writeJson(resolve(expectedDir, EXPECTED_ARTIFACTS_MANIFEST), {
      resolvedTargetSha: prepared.resolvedTargetSha,
      componentPaths: prepared.componentSnapshots.map((component) => component.relativePath),
    });
    for (const component of prepared.componentSnapshots) {
      writeJson(resolve(expectedDir, `${component.relativePath}.json`), { ok: true });
    }

    const loaded = tryLoadExpectedArtifacts(prepared, expectedDir);

    expect(loaded).not.toBeNull();
    expect(loaded?.get("src/runtime/components/Root.vue")).toBe(
      resolve(expectedDir, "src/runtime/components/Root.vue.json"),
    );
    expect(loaded?.get("src/runtime/components/nested/Nested.vue")).toBe(
      resolve(expectedDir, "src/runtime/components/nested/Nested.vue.json"),
    );
  });
});

describe("maybeRunGarbageCollection", () => {
  it("invokes global gc when the benchmark process exposes it", () => {
    const originalGc = globalThis.gc;
    let calls = 0;
    (globalThis as typeof globalThis & { gc?: () => void }).gc = () => {
      calls++;
    };

    try {
      maybeRunGarbageCollection();
    } finally {
      (globalThis as typeof globalThis & { gc?: () => void }).gc = originalGc;
    }

    expect(calls).toBe(1);
  });
});
