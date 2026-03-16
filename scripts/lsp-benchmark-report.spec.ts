/**
 * @ai-generated - Verifies multi-OS LSP benchmark report rendering and legacy key compatibility.
 */

import { describe, expect, it } from "vitest";

import { buildMarkdownReport, normalizeMatrixRun } from "./lsp-benchmark-report.mjs";

describe("normalizeMatrixRun", () => {
  it("maps legacy hover keys into the current shape", () => {
    const run = normalizeMatrixRun({
      fileName: "lsp-benchmark-results-windows.json",
      json: {
        project: "primevue",
        vueFileCount: 100,
        testFile: "src/datatable/DataTable.vue",
        testFileLines: 400,
        timestamp: "2026-03-13T00:00:00.000Z",
        configs: {
          Volar: {
            initialize: 800,
            workspaceScan: 0,
            didOpenToHover: 120,
            hoverWarm: 5,
            hoverMedian: 4,
          },
        },
      },
    });

    expect(run.osKey).toBe("windows");
    expect(run.osLabel).toBe("Windows");
    expect(run.json.configs.Volar.hoverCold).toBe(5);
    expect(run.json.configs.Volar.hoverWarmMedian).toBe(4);
  });
});

describe("buildMarkdownReport", () => {
  it("renders one table per OS with explicit benchmark values", () => {
    const markdown = buildMarkdownReport([
      normalizeMatrixRun({
        fileName: "lsp-benchmark-results-linux.json",
        json: {
          project: "primevue",
          vueFileCount: 520,
          testFile: "src/datatable/DataTable.vue",
          testFileLines: 410,
          timestamp: "2026-03-13T00:00:00.000Z",
          platform: "linux",
          arch: "x64",
          configs: {
            "Verter (no TP)": {
              initialize: 120,
              workspaceScan: 55,
              didOpenToHover: 48,
              hoverCold: 0.21,
              hoverWarmMedian: 0.18,
            },
            Volar: {
              initialize: 860,
              workspaceScan: 0,
              didOpenToHover: 115,
              hoverCold: 4,
              hoverWarmMedian: 3.5,
            },
          },
        },
      }),
      normalizeMatrixRun({
        fileName: "lsp-benchmark-results-macos.json",
        json: {
          project: "primevue",
          vueFileCount: 520,
          testFile: "src/datatable/DataTable.vue",
          testFileLines: 410,
          timestamp: "2026-03-13T00:00:00.000Z",
          platform: "darwin",
          arch: "x64",
          configs: {
            "Verter (no TP)": {
              initialize: 140,
              workspaceScan: 60,
              didOpenToHover: 52,
              hoverCold: 0.3,
              hoverWarmMedian: 0.25,
            },
            Volar: {
              initialize: 910,
              workspaceScan: 0,
              didOpenToHover: 118,
              hoverCold: 4.5,
              hoverWarmMedian: 4.1,
            },
          },
        },
      }),
    ]);

    expect(markdown).toContain("## LSP Benchmark Results");
    expect(markdown).toContain("### Linux");
    expect(markdown).toContain("### macOS");
    expect(markdown).toContain("| Hover (cold) |");
    expect(markdown).toContain("| Hover (median of 5) |");
    expect(markdown).toContain("120ms");
    expect(markdown).toContain("860ms");
    expect(markdown).toContain("0.21ms");
    expect(markdown).toContain("N/A");
  });
});
