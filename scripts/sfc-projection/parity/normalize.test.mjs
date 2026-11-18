import assert from "node:assert/strict";
import test from "node:test";
import { normalize } from "./normalize.mjs";

test("reads only reporter lines, retaining native codes and the first message line", () => {
  assert.deepEqual(
    normalize(
      `checking files...
src/Legacy.vue(3,8): error TS2322: Type 'string' is not assignable.
  continued detail
src/Driver.vue(1,1): warning VTER1002: Script languages differ.
Found 1 error.
`,
      "/repo",
    ),
    [
      {
        file: "src/Legacy.vue",
        line: 3,
        col: 8,
        code: "TS2322",
        messageHead: "Type 'string' is not assignable.",
      },
      {
        file: "src/Driver.vue",
        line: 1,
        col: 1,
        code: "VTER1002",
        messageHead: "Script languages differ.",
      },
    ],
  );
});

test("normalizes Windows and POSIX file and message paths, including spaces and parent paths", () => {
  for (const [root, file, dependency] of [
    ["/repo/app", "/repo/app/src/My File.vue", "/repo/shared/a.ts"],
    ["C:\\repo\\app", "C:\\repo\\app\\src\\My File.vue", "C:\\repo\\shared\\a.ts"],
    ["C:/repo/app", "C:/repo/app/src/My File.vue", "C:/repo/shared/a.ts"],
    [
      "\\\\server\\repo\\app",
      "\\\\server\\repo\\app\\src\\My File.vue",
      "\\\\server\\repo\\shared\\a.ts",
    ],
  ]) {
    assert.deepEqual(
      normalize(`${file}(4,2): error TS2307: Cannot find '${dependency}' in '${file}'.\r\n`, root),
      [
        {
          file: "src/My File.vue",
          line: 4,
          col: 2,
          code: "TS2307",
          messageHead: "Cannot find '../shared/a.ts' in 'src/My File.vue'.",
        },
      ],
    );
  }
  assert.equal(
    normalize("./src/a.vue(2,1): error TS1: /repo/src/a.ts and /repo-other/b.ts", "/repo")[0]
      .messageHead,
    "src/a.ts and ../repo-other/b.ts",
  );
});

test("keeps duplicates and handles a filename containing parentheses", () => {
  const line = "src/a(test).vue(2,4): error TS2339: Missing property.";
  assert.equal(normalize(`${line}\n${line}`, "/repo").length, 2);
  assert.equal(normalize(line, "/repo")[0].file, "src/a(test).vue");
});

test("reads positionless reporter and global stock diagnostics as rows without a position", () => {
  const output = [
    "src/A.vue(2,4): error TS2322: Wrong value.",
    "src/B.vue: error VTER1001: Failed in /repo/src/B.vue.",
    "error TS18003: No inputs were found in config file '/repo/tsconfig.json'.",
  ].join("\n");
  assert.deepEqual(normalize(output, "/repo").slice(1), [
    {
      file: "src/B.vue",
      line: null,
      col: null,
      code: "VTER1001",
      messageHead: "Failed in src/B.vue.",
    },
    {
      file: null,
      line: null,
      col: null,
      code: "TS18003",
      messageHead: "No inputs were found in config file 'tsconfig.json'.",
    },
  ]);
  assert.deepEqual(
    normalize(String.raw`C:\repo\src\B.vue: warning VTER1009: Partial.`, String.raw`C:\repo`),
    [{ file: "src/B.vue", line: null, col: null, code: "VTER1009", messageHead: "Partial." }],
  );
});
