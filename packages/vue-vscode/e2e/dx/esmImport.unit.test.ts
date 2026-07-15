import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

import ts from "typescript";
import { describe, expect, it } from "vitest";

import { importEsm } from "./esmImport";

const fixtureUrl = new URL("./fixtures/sampleEsm.mjs", import.meta.url);
const moduleSource = readFileSync(new URL("./esmImport.ts", import.meta.url), "utf-8");

describe("importEsm", () => {
  it("exports a callable loader", () => {
    expect(typeof importEsm).toBe("function");
  });

  it("uses a real dynamic import() built from a Function string, never require", () => {
    // The whole point of the helper: hide `import(...)` from the TS CommonJS module
    // transform so it is not downleveled to `require`. Guard that the mechanism is
    // intact — a refactor to a bare `await import()` would silently downlevel and
    // re-break the launcher under the CommonJS `tsconfig.test.json` emit. Strip
    // comments first so prose mentioning `require`/`await import` doesn't false-trip.
    const code = moduleSource.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
    expect(code).toMatch(/new Function\(/);
    expect(code).toMatch(/return import\(specifier\)/);
    expect(code).not.toMatch(/\brequire\(/);
    expect(code).not.toMatch(/await import\(/);
  });

  it("the fixture is genuinely ESM-only (require throws) — the discriminating control", () => {
    // If `importEsm` ever collapsed to `require`, loading this fixture would fail
    // exactly the way this control does. Top-level `await` makes it un-`require`-able.
    const require = createRequire(import.meta.url);
    expect(() => require(fixtureUrl.pathname)).toThrow();
  });

  it("calls the REAL importEsm() (compiled to CommonJS) to load the ESM-only fixture in real Node", () => {
    // Faithful, hermetic exercise of the launcher/in-host runtime calling the HELPER
    // ITSELF (not an inline reconstruction): compile the actual `esmImport.ts` to CommonJS
    // with the same TypeScript emit the launcher uses, then a real Node CJS process calls
    // the compiled `importEsm` against the ESM-only fixture. `require` would throw on the
    // fixture (the control above); a printed marker proves the compiled helper performed a
    // genuine dynamic `import()` and never downleveled to `require`.
    //
    // It must run in a child process: the helper's `new Function("…","return import(…)")`
    // needs the host dynamic-import callback, which a real Node process installs but
    // Vitest's worker realm does not (an in-process call throws "A dynamic import callback
    // was not specified"). The child is that real-Node CommonJS runtime.
    const helperCjs = ts.transpileModule(moduleSource, {
      compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2021 },
    }).outputText;
    const driver = [
      helperCjs,
      `exports.importEsm(${JSON.stringify(fixtureUrl.href)})`,
      "  .then((ns) => process.stdout.write(ns.marker))",
      "  .catch((err) => { console.error(err); process.exit(2); });",
    ].join("\n");
    const out = execFileSync(process.execPath, ["-e", driver], { encoding: "utf-8" });
    expect(out).toBe("esm-only-via-dynamic-import");
  });
});
