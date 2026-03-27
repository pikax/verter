import { describe, expect, it } from "vitest";

import {
  buildDiagnosticDiff,
  buildReviewQueue,
  normalizeTypeCheckArtifacts,
  parseTypeScriptDiagnostics,
  stripAnsi,
} from "./diagnostics.mjs";

describe("diagnostics", () => {
  it("strips ansi sequences and parses TS diagnostics", () => {
    const raw =
      '\u001b[96msrc/App.vue\u001b[0m:\u001b[93m12\u001b[0m:\u001b[93m5\u001b[0m - \u001b[91merror\u001b[0m TS2322: Type "string" is not assignable to type "number".';
    const clean = stripAnsi(raw);
    expect(clean).toContain("src/App.vue:12:5 - error TS2322");

    const parsed = parseTypeScriptDiagnostics(raw, {
      tool: "verter-tsc",
      pass: "warm",
      cwd: "D:\\repo",
    });

    expect(parsed).toEqual([
      expect.objectContaining({
        file: "src/App.vue",
        line: 12,
        column: 5,
        code: "TS2322",
        tool: "verter-tsc",
        pass: "warm",
      }),
    ]);
  });

  it("diffs vue-tsc and verter-tsc diagnostics into reviewable classes", () => {
    const normalized = normalizeTypeCheckArtifacts(
      {
        tsconfig: "tsconfig.json",
        vueTsc: {
          cold: {
            ms: 10,
            exitCode: 0,
            errorCount: 1,
            timedOut: false,
            stdout: "",
            stderr: "src/App.vue:1:1 - error TS2322: Shared issue\n",
          },
          warm: {
            ms: 8,
            exitCode: 0,
            errorCount: 1,
            timedOut: false,
            stdout: "",
            stderr: "src/App.vue:1:1 - error TS2322: Shared issue\n",
          },
        },
        verterTsc: {
          cold: {
            ms: 9,
            exitCode: 0,
            errorCount: 2,
            timedOut: false,
            stdout: "",
            stderr:
              "src/App.vue:1:1 - error TS2322: Shared issue\nsrc/Strict.vue:2:3 - error TS2345: Extra strict issue\n",
          },
          warm: {
            ms: 7,
            exitCode: 0,
            errorCount: 2,
            timedOut: false,
            stdout: "",
            stderr:
              "src/App.vue:1:1 - error TS2322: Shared issue\nsrc/Strict.vue:2:3 - error TS2345: Extra strict issue\n",
          },
        },
      },
      "D:\\repo",
    );

    const diff = buildDiagnosticDiff(normalized);
    expect(diff.summary.shared).toBe(1);
    expect(diff.summary.verter_only_likely_legit).toBe(1);

    const queue = buildReviewQueue(diff, {
      repoRoot: "D:\\repo",
      projectName: "strict-app",
    });
    expect(queue.items).toHaveLength(1);
    expect(queue.items[0]).toMatchObject({
      status: "pending",
      classification: "verter_only_likely_legit",
      code: "TS2345",
    });
  });
});
