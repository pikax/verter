/**
 * @ai-generated - Verifies meta-ui benchmark project preparation and component discovery.
 */

import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  EXPECTED_ARTIFACTS_MANIFEST,
  createWorkerBackendInstance,
  maybeRunGarbageCollection,
  parseMetaUiBenchArgs,
  prepareMetaUiProject,
  tryLoadExpectedArtifacts,
} from "./meta-ui-bench.js";
import { compareNormalizedArtifacts } from "./meta-ui-core.js";
import { loadVerterCompatModule } from "./verter-compat.js";
import { resolveVerterCompatSourceEntry } from "./verter-compat.js";

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

  it("supports configuring the per-query timeout", () => {
    const args = parseMetaUiBenchArgs(["--query-timeout-ms=250"]);

    expect(args.queryTimeoutMs).toBe(250);
  });

  it("keeps JS audit off by default", () => {
    const args = parseMetaUiBenchArgs([]);

    expect(args.jsAudit).toBe(false);
  });

  it("supports enabling JS audit explicitly", () => {
    const args = parseMetaUiBenchArgs(["--js-audit"]);

    expect(args.jsAudit).toBe(true);
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

describe("resolveVerterCompatSourceEntry", () => {
  it("prefers the workspace source entry when it exists", () => {
    const repoRoot = mkdtempSync(resolve(tmpdir(), "verter-benchmark-workspace-"));
    const compatEntry = resolve(
      repoRoot,
      "packages",
      "component-meta",
      "src",
      "compat",
      "index.ts",
    );
    mkdirSync(resolve(repoRoot, "packages", "component-meta", "src", "compat"), {
      recursive: true,
    });
    writeFileSync(compatEntry, "export {};\n", "utf8");

    expect(resolveVerterCompatSourceEntry(repoRoot)).toBe(compatEntry);
  });

  it("returns null when the workspace source entry is absent", () => {
    const repoRoot = mkdtempSync(resolve(tmpdir(), "verter-benchmark-no-workspace-"));

    expect(resolveVerterCompatSourceEntry(repoRoot)).toBeNull();
  });
});

describe("createWorkerBackendInstance", () => {
  it("fails fast when a worker never answers a query", async () => {
    const workerRoot = mkdtempSync(resolve(tmpdir(), "verter-meta-ui-worker-"));
    const workerPath = resolve(workerRoot, "stalled-worker.mjs");
    writeFileSync(
      workerPath,
      [
        "process.on('message', (message) => {",
        "  if (message?.type === 'init') {",
        "    process.send?.({ type: 'ready' });",
        "    return;",
        "  }",
        "  if (message?.type === 'query') {",
        "    return;",
        "  }",
        "});",
      ].join("\n"),
      "utf8",
    );

    const instance = await createWorkerBackendInstance(
      {
        backend: "verter",
        uiRoot: workerRoot,
        checkerConfig: {},
        components: [],
      },
      {
        workerEntryPath: workerPath,
        queryTimeoutMs: 25,
        setupTimeoutMs: 1_000,
      },
    );

    await expect(
      instance.query({
        absolutePath: resolve(workerRoot, "Component.vue"),
        relativePath: "src/runtime/components/Component.vue",
        transformedSource: "<template />",
      }),
    ).rejects.toThrow(/timed out after 25ms/i);

    expect(instance.isAvailable()).toBe(false);
    await instance.dispose();
  });
});

describe("verter repeated benchmark queries", () => {
  const defaultUiRoot = parseMetaUiBenchArgs([]).uiRoot;
  const tablePath = resolve(defaultUiRoot, "src", "runtime", "components", "Table.vue");

  it.runIf(existsSync(tablePath))(
    "reuses the declared Table.vue resolved state on a repeated query",
    async () => {
      const prepared = prepareMetaUiProject(
        parseMetaUiBenchArgs(["--components=Table.vue", "--expected=none"]),
      );
      const table = prepared.componentSnapshots.find(
        (component) => component.relativePath === "src/runtime/components/Table.vue",
      );
      expect(table).toBeDefined();

      const { createCheckerByJson } = await loadVerterCompatModule();
      const checker = await createCheckerByJson(
        prepared.uiRoot,
        {
          ...prepared.compilerOptions,
          include: [table!.absolutePath.replace(/\\/g, "/").replace(/\.vue$/, ".vue.ts")],
        },
        {
          forceUseTs: true,
          schema: { literalBooleanSchema: true },
          runtimeMode: "dedicated",
        },
      );

      try {
        checker.updateFile(table!.absolutePath, table!.transformedSource);

        const firstStartedAt = performance.now();
        const first = await checker.getComponentMeta(table!.absolutePath);
        const firstLatencyMs = performance.now() - firstStartedAt;
        const provenanceAfterFirst = (checker as any)._session.getProvenance();

        const secondStartedAt = performance.now();
        const second = await checker.getComponentMeta(table!.absolutePath);
        const secondLatencyMs = performance.now() - secondStartedAt;
        const provenanceAfterSecond = (checker as any)._session.getProvenance();

        expect(second.props.length).toBe(first.props.length);
        expect(provenanceAfterSecond.componentMetaResolvedStateRecomputes).toBe(
          provenanceAfterFirst.componentMetaResolvedStateRecomputes,
        );
        expect(secondLatencyMs).toBeLessThan(firstLatencyMs * 2);
      } finally {
        checker.close();
      }
    },
    20_000,
  );
});

describe("verter benchmark worker parity", () => {
  const defaultUiRoot = parseMetaUiBenchArgs([]).uiRoot;
  const editorPath = resolve(defaultUiRoot, "src", "runtime", "components", "Editor.vue");
  const expectedEditorPath = resolve(
    parseMetaUiBenchArgs([]).expectedDir,
    "src/runtime/components/Editor.vue.json",
  );
  const editorDragHandlePath = resolve(
    defaultUiRoot,
    "src",
    "runtime",
    "components",
    "EditorDragHandle.vue",
  );
  const expectedEditorDragHandlePath = resolve(
    parseMetaUiBenchArgs([]).expectedDir,
    "src/runtime/components/EditorDragHandle.vue.json",
  );
  const popoverPath = resolve(defaultUiRoot, "src", "runtime", "components", "Popover.vue");
  const expectedPopoverPath = resolve(
    parseMetaUiBenchArgs([]).expectedDir,
    "src/runtime/components/Popover.vue.json",
  );

  it.runIf(existsSync(popoverPath) && existsSync(expectedPopoverPath))(
    "returns the pinned Popover.vue artifact within the 5s query budget",
    async () => {
      const prepared = prepareMetaUiProject(
        parseMetaUiBenchArgs([
          "--components=Popover.vue",
          "--expected=none",
          "--query-timeout-ms=5000",
        ]),
      );
      const popover = prepared.componentSnapshots.find(
        (component) => component.relativePath === "src/runtime/components/Popover.vue",
      );
      expect(popover).toBeDefined();

      const instance = await createWorkerBackendInstance(
        {
          backend: "verter",
          uiRoot: prepared.uiRoot,
          checkerConfig: {
            extends: `${prepared.uiRoot}/tsconfig.json`,
            skipLibCheck: true,
            include: [popover!.absolutePath],
            exclude: [],
            compilerOptions: {
              ...(prepared.compilerOptions.baseUrl
                ? { baseUrl: prepared.compilerOptions.baseUrl }
                : {}),
              ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
            },
          },
          components: [popover!],
        },
        {
          queryTimeoutMs: 5_000,
          setupTimeoutMs: 30_000,
        },
      );

      try {
        const result = await instance.query(popover!);
        const expectedArtifact = JSON.parse(readFileSync(expectedPopoverPath, "utf8"));
        const comparison = compareNormalizedArtifacts(result.artifact, expectedArtifact);

        expect(result.outcome).toBe("success");
        expect(comparison.exact).toBe(true);
      } finally {
        await instance.dispose();
      }
    },
    20_000,
  );

  it.runIf(existsSync(editorPath) && existsSync(expectedEditorPath))(
    "returns the pinned Editor.vue artifact within the 5s query budget",
    async () => {
      const prepared = prepareMetaUiProject(
        parseMetaUiBenchArgs([
          "--components=Editor.vue",
          "--expected=none",
          "--query-timeout-ms=5000",
        ]),
      );
      const editor = prepared.componentSnapshots.find(
        (component) => component.relativePath === "src/runtime/components/Editor.vue",
      );
      expect(editor).toBeDefined();

      const instance = await createWorkerBackendInstance(
        {
          backend: "verter",
          uiRoot: prepared.uiRoot,
          checkerConfig: {
            extends: `${prepared.uiRoot}/tsconfig.json`,
            skipLibCheck: true,
            include: [editor!.absolutePath],
            exclude: [],
            compilerOptions: {
              ...(prepared.compilerOptions.baseUrl
                ? { baseUrl: prepared.compilerOptions.baseUrl }
                : {}),
              ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
            },
          },
          components: [editor!],
        },
        {
          queryTimeoutMs: 5_000,
          setupTimeoutMs: 30_000,
        },
      );

      try {
        const result = await instance.query(editor!);
        const expectedArtifact = JSON.parse(readFileSync(expectedEditorPath, "utf8"));
        const comparison = compareNormalizedArtifacts(result.artifact, expectedArtifact);

        expect(result.outcome).toBe("success");
        expect(comparison.exact).toBe(true);
      } finally {
        await instance.dispose();
      }
    },
    20_000,
  );

  it.runIf(existsSync(editorDragHandlePath) && existsSync(expectedEditorDragHandlePath))(
    "returns the pinned EditorDragHandle.vue artifact within the 5s query budget",
    async () => {
      const prepared = prepareMetaUiProject(
        parseMetaUiBenchArgs([
          "--components=EditorDragHandle.vue",
          "--expected=none",
          "--query-timeout-ms=5000",
        ]),
      );
      const editorDragHandle = prepared.componentSnapshots.find(
        (component) => component.relativePath === "src/runtime/components/EditorDragHandle.vue",
      );
      expect(editorDragHandle).toBeDefined();

      const instance = await createWorkerBackendInstance(
        {
          backend: "verter",
          uiRoot: prepared.uiRoot,
          checkerConfig: {
            extends: `${prepared.uiRoot}/tsconfig.json`,
            skipLibCheck: true,
            include: [editorDragHandle!.absolutePath],
            exclude: [],
            compilerOptions: {
              ...(prepared.compilerOptions.baseUrl
                ? { baseUrl: prepared.compilerOptions.baseUrl }
                : {}),
              ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
            },
          },
          components: [editorDragHandle!],
        },
        {
          queryTimeoutMs: 5_000,
          setupTimeoutMs: 30_000,
        },
      );

      try {
        const result = await instance.query(editorDragHandle!);
        const expectedArtifact = JSON.parse(readFileSync(expectedEditorDragHandlePath, "utf8"));
        const comparison = compareNormalizedArtifacts(result.artifact, expectedArtifact);

        expect(result.outcome).toBe("success");
        expect(comparison.exact).toBe(true);
      } finally {
        await instance.dispose();
      }
    },
    20_000,
  );
});
