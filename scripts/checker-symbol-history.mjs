// Reproduce the pinned native checker's symbol-ID-dependent abbreviation.
// This optional oracle experiment needs the installed TypeScript package;
// provider-free consumer regressions live in flow_narrowing_parity_tests.rs.
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { version } from "typescript";
import { API } from "typescript/unstable/sync";

assert.equal(version, "7.0.2", "reassess this compatibility exception on oracle upgrades");
const dir = mkdtempSync(join(tmpdir(), "verter-symbol-history-")).replaceAll("\\", "/");
const keys = Array.from({ length: 12 }, (_, i) => `declare const s${i}: unique symbol;`).join("\n");
const props = Array.from({ length: 12 }, (_, i) => `[s${i}]: number;`).join(" ");
const source =
  `${keys}\ndeclare function one(x: string): number;\n` +
  Array.from(
    { length: 35 },
    (_, i) =>
      `export function row${i}(x: { padding: "${"x".repeat(80 + i)}"; ${props} last: string }) { return one(x); }`,
  ).join("\n") +
  "\nexport function plain(x: { a: number }) { return one(x); }" +
  "\nexport function short(x: { [s0]: number }) { return one(x); }";

function read(prewarm, strictNullChecks) {
  const api = new API({ cwd: dir });
  try {
    if (prewarm) {
      const snapshot = api.updateSnapshot({ openProjects: [`${dir}/warm.json`] });
      try {
        assert.deepEqual(snapshot.getProjects()[0].program.getSemanticDiagnostics(), []);
      } finally {
        snapshot.dispose();
      }
    }
    const config = `${dir}/${strictNullChecks ? "strict" : "loose"}.json`;
    const snapshot = api.updateSnapshot({ openProjects: [config] });
    try {
      return snapshot
        .getProject(config)
        .program.getSemanticDiagnostics()
        .map(({ code, pos, end, text, messageChain }) => ({ code, pos, end, text, messageChain }));
    } finally {
      snapshot.dispose();
    }
  } finally {
    api.close();
  }
}

try {
  writeFileSync(`${dir}/test.ts`, source);
  const warm =
    Array.from({ length: 10000 }, (_, i) => `declare const u${i}: unique symbol;`).join("\n") +
    "\nexport type Warm = {" +
    Array.from({ length: 10000 }, (_, i) => `[u${i}]: number;`).join("\n") +
    "};";
  writeFileSync(`${dir}/warm.ts`, warm);
  for (const name of ["warm", "strict", "loose"]) {
    writeFileSync(
      `${dir}/${name}.json`,
      JSON.stringify({
        compilerOptions: {
          strict: true,
          strictNullChecks: name !== "loose",
          noEmit: true,
          types: [],
        },
        files: [name === "warm" ? "warm.ts" : "test.ts"],
      }),
    );
  }
  for (const strictNullChecks of [true, false]) {
    const fresh = read(false, strictNullChecks);
    const warmed = read(true, strictNullChecks);
    assert.equal(fresh.length, 37, "one diagnostic per boundary row and each unaffected control");
    assert(fresh.every(({ code }) => code === 2345));
    assert.deepEqual(
      fresh.map(({ text, ...rest }) => rest),
      warmed.map(({ text, ...rest }) => rest),
      "history must not change code, location, order or chains",
    );
    const differences = fresh.flatMap((diagnostic, i) =>
      diagnostic.text === warmed[i].text ? [] : [{ fresh: diagnostic, warmed: warmed[i] }],
    );
    assert(differences.length > 0, "the oracle must reproduce the history-sensitive abbreviation");
    assert.deepEqual(fresh.slice(-2), warmed.slice(-2), "unaffected exact-checker controls");
    assert.equal(
      fresh.at(-2).text,
      "Argument of type '{ a: number; }' is not assignable to parameter of type 'string'.",
    );
    assert.equal(
      fresh.at(-1).text,
      "Argument of type '{ [s0]: number; }' is not assignable to parameter of type 'string'.",
    );
    console.log(JSON.stringify({ strictNullChecks, differences }, null, 2));
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}
