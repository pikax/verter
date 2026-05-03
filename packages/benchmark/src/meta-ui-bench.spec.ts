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
  detectUnquotedCsvSpillover,
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

describe("Tier 6 §8.2 / T9.2 — detectUnquotedCsvSpillover", () => {
  // Discriminator: when a user passes `--scenarios=single_cold
  // repo_first_pass` (unquoted, with whitespace) the shell splits
  // on whitespace and `repo_first_pass` arrives as a positional
  // arg. The detector must surface this so the caller can warn.
  it("detects positional scenario tokens left over by an unquoted CSV", () => {
    const spill = detectUnquotedCsvSpillover(["--scenarios=single_cold", "repo_first_pass"]);
    expect(spill.scenarioSpillover).toEqual(["repo_first_pass"]);
    expect(spill.backendSpillover).toEqual([]);
    expect(spill.unrecognizedPositional).toEqual([]);
  });

  it("detects positional backend tokens left over by an unquoted CSV", () => {
    const spill = detectUnquotedCsvSpillover(["--backends=verter", "vue-component-meta"]);
    expect(spill.backendSpillover).toEqual(["vue-component-meta"]);
    expect(spill.scenarioSpillover).toEqual([]);
  });

  it("ignores positional tokens that are not known scenario or backend names", () => {
    // Unrecognized positionals are still tracked but NOT flagged
    // as scenario/backend spillover — the warning text would be
    // misleading otherwise.
    const spill = detectUnquotedCsvSpillover(["--scenarios=single_cold", "some-unrelated-thing"]);
    expect(spill.scenarioSpillover).toEqual([]);
    expect(spill.unrecognizedPositional).toEqual(["some-unrelated-thing"]);
  });

  it("treats correctly-quoted CSV as a single arg with no spillover", () => {
    // The properly-quoted form arrives as a single argv token. The
    // detector returns empty spillover lists, which is the
    // post-change passing state.
    const spill = detectUnquotedCsvSpillover(["--scenarios=single_cold,repo_first_pass"]);
    expect(spill.scenarioSpillover).toEqual([]);
    expect(spill.backendSpillover).toEqual([]);
    expect(spill.unrecognizedPositional).toEqual([]);
  });

  it("does not flag flag-form args (e.g., --json, --build-expected-only, --js-audit)", () => {
    // These flags are valid positional-looking tokens (no `=`) and
    // must not trigger the spillover warning.
    const spill = detectUnquotedCsvSpillover(["--json", "--build-expected-only", "--js-audit"]);
    expect(spill.scenarioSpillover).toEqual([]);
    expect(spill.backendSpillover).toEqual([]);
    expect(spill.unrecognizedPositional).toEqual([]);
  });

  it("emits a stderr warning when parseMetaUiBenchArgs sees unquoted scenario CSV", () => {
    // End-to-end: run the parser against the broken-form args and
    // capture the stderr warning. This is the user-visible
    // behavior the README documents.
    const originalWrite = process.stderr.write.bind(process.stderr);
    let captured = "";
    process.stderr.write = ((chunk: string | Uint8Array) => {
      captured += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString();
      return true;
    }) as typeof process.stderr.write;
    try {
      parseMetaUiBenchArgs(["--scenarios=single_cold", "repo_first_pass"]);
    } finally {
      process.stderr.write = originalWrite;
    }
    expect(captured).toMatch(/look like scenario names/);
    expect(captured).toMatch(/--scenarios="single_cold,repo_first_pass"/);
    // Discriminator: warning text must reference the README so a
    // future refactor that relocates the docs surfaces here.
    expect(captured).toMatch(/packages\/benchmark\/README\.md/);
  });

  it("does not emit a warning when scenarios are correctly quoted (single CSV arg)", () => {
    const originalWrite = process.stderr.write.bind(process.stderr);
    let captured = "";
    process.stderr.write = ((chunk: string | Uint8Array) => {
      captured += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString();
      return true;
    }) as typeof process.stderr.write;
    try {
      parseMetaUiBenchArgs(["--scenarios=single_cold,repo_first_pass"]);
    } finally {
      process.stderr.write = originalWrite;
    }
    // No spillover warning on the quoted form.
    expect(captured).not.toMatch(/look like scenario names/);
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
  const themePath = resolve(defaultUiRoot, "src", "runtime", "components", "Theme.vue");
  const expectedThemePath = resolve(
    parseMetaUiBenchArgs([]).expectedDir,
    "src/runtime/components/Theme.vue.json",
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

  it.runIf(existsSync(themePath) && existsSync(expectedThemePath))(
    "returns the pinned Theme.vue artifact within the 5s query budget",
    async () => {
      const prepared = prepareMetaUiProject(
        parseMetaUiBenchArgs([
          "--components=Theme.vue",
          "--expected=none",
          "--query-timeout-ms=5000",
        ]),
      );
      const theme = prepared.componentSnapshots.find(
        (component) => component.relativePath === "src/runtime/components/Theme.vue",
      );
      expect(theme).toBeDefined();

      const instance = await createWorkerBackendInstance(
        {
          backend: "verter",
          uiRoot: prepared.uiRoot,
          checkerConfig: {
            extends: `${prepared.uiRoot}/tsconfig.json`,
            skipLibCheck: true,
            include: [theme!.absolutePath],
            exclude: [],
            compilerOptions: {
              ...(prepared.compilerOptions.baseUrl
                ? { baseUrl: prepared.compilerOptions.baseUrl }
                : {}),
              ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
            },
          },
          components: [theme!],
        },
        {
          queryTimeoutMs: 5_000,
          setupTimeoutMs: 30_000,
        },
      );

      try {
        const result = await instance.query(theme!);
        const expectedArtifact = JSON.parse(readFileSync(expectedThemePath, "utf8"));
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
