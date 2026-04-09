import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { type NormalizedMetaArtifact } from "./meta-ui-core.js";
import {
  compareResultArtifactToExpected,
  findTraceLogPath,
  resolveExpectedArtifactPath,
  resolveTraceLogCandidatePaths,
  resolveTraceResultArtifactPath,
} from "./trace-check-core.js";

function createArtifact(componentPath: string): NormalizedMetaArtifact {
  return {
    componentPath,
    componentName: "Sample",
    props: [],
    events: [],
    slots: [],
    exposed: [],
    models: [],
    propsJsonSchema: {},
    diagnostics: [],
  };
}

describe("trace-check expected artifact comparison", () => {
  it("resolves both flat and structured trace log candidate paths", () => {
    const candidates = resolveTraceLogCandidatePaths({
      componentName: "Accordion",
      componentPath: "src/runtime/components/Accordion.vue",
      traceDir: "D:/tmp/trace",
    });

    expect(candidates).toContain(join(resolve("D:/tmp/trace"), "Accordion.trace.log"));
    expect(candidates).toContain(
      join(resolve("D:/tmp/trace"), "traces", "src__runtime__components__Accordion__vue.trace.log"),
    );
  });

  it("maps component paths to result and expected artifact paths", () => {
    expect(resolveTraceResultArtifactPath("D:/tmp/trace", "src/runtime/components/App.vue")).toBe(
      join(resolve("D:/tmp/trace"), "results", "src", "runtime", "components", "App.vue.json"),
    );
    expect(
      resolveExpectedArtifactPath(
        "D:/repo/packages/benchmark/benchmark-results/meta-ui/.expected-vue-component-meta",
        "src/runtime/components/App.vue",
      ),
    ).toBe(
      join(
        resolve(
          "D:/repo/packages/benchmark/benchmark-results/meta-ui/.expected-vue-component-meta",
        ),
        "src",
        "runtime",
        "components",
        "App.vue.json",
      ),
    );
  });

  it("passes when the normalized result matches the expected artifact", () => {
    const traceDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const expectedDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const componentPath = "src/runtime/components/App.vue";
    const actualPath = resolveTraceResultArtifactPath(traceDir, componentPath);
    const expectedPath = resolveExpectedArtifactPath(expectedDir, componentPath);

    mkdirSync(join(traceDir, "results", "src", "runtime", "components"), { recursive: true });
    mkdirSync(join(expectedDir, "src", "runtime", "components"), { recursive: true });

    const artifact = createArtifact(componentPath);
    writeFileSync(actualPath, JSON.stringify(artifact, null, 2), "utf8");
    writeFileSync(expectedPath, JSON.stringify(artifact, null, 2), "utf8");

    const comparison = compareResultArtifactToExpected({
      componentPath,
      traceDir,
      expectedDir,
    });

    expect(comparison.passed).toBe(true);
    expect(comparison.comparison?.exact).toBe(true);
    expect(comparison.message).toContain("matches expected");
  });

  it("fails when the normalized result diverges from the expected artifact", () => {
    const traceDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const expectedDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const componentPath = "src/runtime/components/App.vue";
    const actualPath = resolveTraceResultArtifactPath(traceDir, componentPath);
    const expectedPath = resolveExpectedArtifactPath(expectedDir, componentPath);

    mkdirSync(join(traceDir, "results", "src", "runtime", "components"), { recursive: true });
    mkdirSync(join(expectedDir, "src", "runtime", "components"), { recursive: true });

    writeFileSync(
      actualPath,
      JSON.stringify(
        {
          ...createArtifact(componentPath),
          props: [
            {
              name: "tone",
              type: "string",
              required: false,
              default: null,
              description: null,
              tags: [],
              schema: null,
            },
          ],
        },
        null,
        2,
      ),
      "utf8",
    );
    writeFileSync(expectedPath, JSON.stringify(createArtifact(componentPath), null, 2), "utf8");

    const comparison = compareResultArtifactToExpected({
      componentPath,
      traceDir,
      expectedDir,
    });

    expect(comparison.passed).toBe(false);
    expect(comparison.comparison?.exact).toBe(false);
    expect(comparison.message).toContain("extra props: tone");
  });

  it("fails when the trace run did not emit a normalized result artifact", () => {
    const traceDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const expectedDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const componentPath = "src/runtime/components/App.vue";
    const expectedPath = resolveExpectedArtifactPath(expectedDir, componentPath);

    mkdirSync(join(expectedDir, "src", "runtime", "components"), { recursive: true });
    writeFileSync(expectedPath, JSON.stringify(createArtifact(componentPath), null, 2), "utf8");

    const comparison = compareResultArtifactToExpected({
      componentPath,
      traceDir,
      expectedDir,
    });

    expect(comparison.passed).toBe(false);
    expect(comparison.comparison).toBeNull();
    expect(comparison.message).toContain("missing normalized result artifact");
  });

  it("finds trace logs inside the structured traces directory", () => {
    const traceDir = mkdtempSync(resolve(tmpdir(), "verter-trace-check-"));
    const tracePath = join(
      traceDir,
      "traces",
      "src__runtime__components__Accordion__vue.trace.log",
    );

    mkdirSync(join(traceDir, "traces"), { recursive: true });
    writeFileSync(tracePath, "trace log", "utf8");

    expect(
      findTraceLogPath({
        componentName: "Accordion",
        componentPath: "src/runtime/components/Accordion.vue",
        traceDir,
      }),
    ).toBe(tracePath);
  });
});
