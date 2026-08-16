// Self-test: TypeScript-observation validator (BF2 owned scope:
// "TypeScript-observable product validation" — the VALIDATOR MECHANISM;
// actual product conformance results using it belong to the downstream
// candidate-producing blocks).
//
// Proves the mechanism drives the REAL TypeScript compiler and genuinely
// discriminates: a planted TS-observation drift — a prop type that silently
// changed with ZERO diagnostics anywhere — is caught, with the difference
// attributed to the exact export and member; diagnostic drift is caught
// with full spans; identical artifact sets compare equal.

import { describe, expect, it } from "vitest";

import { compareObservations, observeTypeScript } from "../src/typescript-observe.mjs";

const PROPS_GOLDEN = [
  {
    fileName: "/component-props.ts",
    code:
      "export interface ButtonProps { label: string; disabled?: boolean }\n" +
      'export const defaults: ButtonProps = { label: "ok" };\n' +
      "export type ButtonEmits = { toggle: [boolean] };\n",
  },
];

describe("TypeScript observation — exact-domain capture", () => {
  it("captures every export with its checker-assigned type, structurally expanded, and zero diagnostics", () => {
    const observation = observeTypeScript(PROPS_GOLDEN);
    expect(observation.diagnostics).toEqual([]);
    const exports = observation.modules["/component-props.ts"].exports;
    expect(Object.keys(exports).sort()).toEqual(["ButtonEmits", "ButtonProps", "defaults"]);
    // A NAMED type is not just its display string — its members are
    // captured too, so a silently-changed member cannot hide behind the
    // name.
    expect(exports.defaults.type.display).toBe("ButtonProps");
    expect(exports.defaults.type.members.label.display).toBe("string");
    expect(exports.defaults.type.members.disabled.display).toContain("boolean");
    expect(exports.ButtonEmits.type.members.toggle.display).toBe("[boolean]");
    expect(exports.ButtonProps.flags).toContain("Interface");
  });

  it("identical artifact sets observe identically", () => {
    const a = observeTypeScript(PROPS_GOLDEN);
    const b = observeTypeScript(PROPS_GOLDEN.map((f) => ({ ...f })));
    const comparison = compareObservations(a, b);
    expect(comparison.equal).toBe(true);
    expect(comparison.differences).toEqual([]);
  });

  it("a SILENTLY changed prop type — no diagnostic on either side — is caught and attributed", () => {
    const mutatedSource = PROPS_GOLDEN[0].code
      .replace("label: string", "label: number")
      .replace('label: "ok"', "label: 1");
    expect(mutatedSource).not.toBe(PROPS_GOLDEN[0].code); // plant proven applied
    const golden = observeTypeScript(PROPS_GOLDEN);
    const candidate = observeTypeScript([{ fileName: "/component-props.ts", code: mutatedSource }]);
    // BOTH sides are diagnostic-clean — only the observed TYPE differs.
    expect(golden.diagnostics).toEqual([]);
    expect(candidate.diagnostics).toEqual([]);
    const comparison = compareObservations(golden, candidate);
    expect(comparison.equal).toBe(false);
    expect(
      comparison.differences.some(
        (d) => d.includes("label") && d.includes('"string"') && d.includes('"number"'),
      ),
    ).toBe(true);
  });

  it("a dropped export is caught", () => {
    const golden = observeTypeScript(PROPS_GOLDEN);
    const candidate = observeTypeScript([
      {
        fileName: "/component-props.ts",
        code:
          "export interface ButtonProps { label: string; disabled?: boolean }\n" +
          'export const defaults: ButtonProps = { label: "ok" };\n',
      },
    ]);
    const comparison = compareObservations(golden, candidate);
    expect(comparison.equal).toBe(false);
    expect(comparison.differences.some((d) => d.includes("ButtonEmits"))).toBe(true);
  });

  it("diagnostic drift is caught with code, message, and full start/end spans", () => {
    const clean = observeTypeScript(PROPS_GOLDEN);
    const broken = observeTypeScript([
      {
        fileName: "/component-props.ts",
        code: "export const defaults: { label: string } = { label: 1 };\n",
      },
    ]);
    expect(broken.diagnostics.length).toBeGreaterThan(0);
    const [diagnostic] = broken.diagnostics;
    expect(diagnostic.kind).toBe("error");
    expect(diagnostic.code).toBeTypeOf("number");
    expect(diagnostic.message.length).toBeGreaterThan(0);
    expect(diagnostic.source).toBe("/component-props.ts");
    expect(diagnostic.start).not.toBeNull();
    expect(diagnostic.end).not.toBeNull();
    const comparison = compareObservations(clean, broken);
    expect(comparison.equal).toBe(false);
  });

  it("observes multi-file artifact graphs with relative imports (the produced-artifact shape)", () => {
    const observation = observeTypeScript([
      {
        fileName: "/types.ts",
        code: "export interface Emitted { toggle: [boolean] }\n",
      },
      {
        fileName: "/component.ts",
        code:
          'import type { Emitted } from "./types.js";\n' + "export declare const emits: Emitted;\n",
      },
    ]);
    expect(observation.diagnostics).toEqual([]);
    expect(observation.modules["/component.ts"].exports.emits.type.display).toBe("Emitted");
    expect(observation.modules["/component.ts"].exports.emits.type.members.toggle.display).toBe(
      "[boolean]",
    );
  });

  it("the observation record carries the full query identity: version, options, libs, inputs, digest", () => {
    const observation = observeTypeScript(PROPS_GOLDEN);
    expect(observation.typescript.version).toMatch(/^\d+\.\d+\.\d+/);
    expect(observation.compilerOptions).toEqual({
      strict: true,
      target: "ES2022",
      module: "ESNext",
      moduleResolution: "Bundler",
      skipLibCheck: true,
      noEmit: true,
      // The OBSERVATION-DOMAIN half of the identity. A domain-less observation
      // enables no JSX and maps no package; a framework or workspace domain
      // changes these, so two observations taken in different domains are never
      // reported as comparable results of the same query.
      jsx: null,
      pathMappings: null,
    });
    expect(observation.libs.length).toBeGreaterThan(0);
    expect(observation.inputs).toEqual([
      { fileName: "/component-props.ts", sha256: expect.stringMatching(/^[0-9a-f]{64}$/) },
    ]);
    expect(observation.queryIdentity).toMatch(/^[0-9a-f]{64}$/);
    // The digest is a genuine function of the identity fields: a different
    // input set produces a different queryIdentity.
    const other = observeTypeScript([
      { fileName: "/component-props.ts", code: "export const x = 1;\n" },
    ]);
    expect(other.queryIdentity).not.toBe(observation.queryIdentity);
  });

  it("(a) a readonly-modifier-only mutation — invisible to display strings and symbol flags — is caught", () => {
    const golden = observeTypeScript([
      {
        fileName: "/component-props.ts",
        code: "export interface ButtonProps { readonly label: string }\nexport declare const p: ButtonProps;\n",
      },
    ]);
    const candidate = observeTypeScript([
      {
        fileName: "/component-props.ts",
        code: "export interface ButtonProps { label: string }\nexport declare const p: ButtonProps;\n",
      },
    ]);
    // Both sides are diagnostic-clean and the member's DISPLAY is
    // identical ("string") — only the readonly modifier differs.
    expect(golden.diagnostics).toEqual([]);
    expect(candidate.diagnostics).toEqual([]);
    expect(golden.modules["/component-props.ts"].exports.p.type.members.label.display).toBe(
      candidate.modules["/component-props.ts"].exports.p.type.members.label.display,
    );
    expect(golden.modules["/component-props.ts"].exports.p.type.members.label.readonly).toBe(true);
    expect(candidate.modules["/component-props.ts"].exports.p.type.members.label.readonly).toBe(
      false,
    );
    const comparison = compareObservations(golden, candidate);
    expect(comparison.equal).toBe(false);
    expect(comparison.differences.some((d) => d.includes("readonly"))).toBe(true);
  });

  it("(b) an assignability-relevant change (required member becomes optional) is caught", () => {
    const golden = observeTypeScript([
      {
        fileName: "/component-props.ts",
        code: "export interface ButtonProps { label: string }\nexport declare const p: ButtonProps;\n",
      },
    ]);
    const candidate = observeTypeScript([
      {
        fileName: "/component-props.ts",
        code: "export interface ButtonProps { label?: string }\nexport declare const p: ButtonProps;\n",
      },
    ]);
    expect(golden.diagnostics).toEqual([]);
    expect(candidate.diagnostics).toEqual([]);
    const comparison = compareObservations(golden, candidate);
    expect(comparison.equal).toBe(false);
    expect(comparison.differences.some((d) => d.includes("optional") || d.includes("label"))).toBe(
      true,
    );
  });

  it("(c) a mutated version, options, libs, inputs, or queryIdentity field is caught by the comparison", () => {
    const golden = observeTypeScript(PROPS_GOLDEN);
    const mutations = [
      (record) => {
        record.typescript = { ...record.typescript, version: "0.0.0-mutated" };
      },
      (record) => {
        record.compilerOptions = { ...record.compilerOptions, strict: false };
      },
      (record) => {
        record.compilerOptions = { ...record.compilerOptions, target: "ES5" };
      },
      (record) => {
        record.libs = [...record.libs.slice(1)];
      },
      (record) => {
        record.inputs = record.inputs.map((input) => ({ ...input, sha256: "0".repeat(64) }));
      },
      (record) => {
        record.queryIdentity = "0".repeat(64);
      },
      (record) => {
        delete record.queryIdentity; // MISSING field, not just mutated
      },
    ];
    for (const mutate of mutations) {
      const candidate = JSON.parse(JSON.stringify(golden));
      mutate(candidate);
      expect(JSON.stringify(candidate)).not.toBe(JSON.stringify(golden)); // plant proven applied
      const comparison = compareObservations(golden, candidate);
      expect(comparison.equal).toBe(false);
      expect(comparison.differences.length).toBeGreaterThan(0);
    }
  });

  // Signature-bearing and depth-boundary drift classes. Each pair below
  // differs ONLY in the named class of type structure, both sides are
  // diagnostic-clean, and the assertion is on the SEMANTIC observation
  // (`modules`) — never on `inputs[].sha256` / `queryIdentity`, which
  // differ for ANY byte-distinct pair and would mask a real observation
  // miss behind a byte-difference detector.
  function observeModules(code) {
    const observation = observeTypeScript([{ fileName: "/m.ts", code }]);
    expect(observation.diagnostics).toEqual([]);
    return observation.modules;
  }

  function expectSemanticDrift(codeA, codeB, attributedTo) {
    expect(codeA).not.toBe(codeB); // plant proven applied
    const golden = observeModules(codeA);
    const candidate = observeModules(codeB);
    expect(JSON.stringify(golden)).not.toBe(JSON.stringify(candidate));
    const comparison = compareObservations({ modules: golden }, { modules: candidate });
    expect(comparison.equal).toBe(false);
    expect(comparison.differences.some((d) => d.includes(attributedTo))).toBe(true);
    return comparison;
  }

  it("a CALLABLE interface member's RETURN type drift is observed", () => {
    expectSemanticDrift(
      "export interface Fn { (value: string): string }\n",
      "export interface Fn { (value: string): number }\n",
      "callSignatures",
    );
  });

  it("a CALLABLE interface member's PARAMETER type drift is observed", () => {
    expectSemanticDrift(
      "export interface Fn { (value: string): string }\n",
      "export interface Fn { (value: number): string }\n",
      "callSignatures",
    );
  });

  it("a CONSTRUCT signature's return type drift is observed", () => {
    expectSemanticDrift(
      "export interface Ctor { new (value: string): { a: string } }\n",
      "export interface Ctor { new (value: string): { a: number } }\n",
      "constructSignatures",
    );
  });

  it("an INDEX signature's value type drift is observed", () => {
    expectSemanticDrift(
      "export interface Bag { [key: string]: string }\n",
      "export interface Bag { [key: string]: number }\n",
      "indexSignatures",
    );
  });

  it("a depth-4 member reached through NAMED types at every hop is observed (named boundary hop)", () => {
    // Root -> a: L2 -> b: L3 -> c: L4 -> value drifts at depth 4. Every hop
    // is a NAMED type, so before the named-boundary hop the depth-3
    // fallback collapsed L4 to its bare display name and hid the drift.
    expectSemanticDrift(
      "interface L4 { value: string }\ninterface L3 { c: L4 }\ninterface L2 { b: L3 }\n" +
        "export interface Root { a: L2 }\n",
      "interface L4 { value: number }\ninterface L3 { c: L4 }\ninterface L2 { b: L3 }\n" +
        "export interface Root { a: L2 }\n",
      "value",
    );
  });

  it("(regression) an ordinary METHOD member's parameter drift is still observed", () => {
    expectSemanticDrift(
      "export interface Api { run(value: string): void }\n",
      "export interface Api { run(value: number): void }\n",
      "run",
    );
  });

  it("(regression) a deep ANONYMOUS-type drift is still observed at any depth", () => {
    expectSemanticDrift(
      "export interface Root { a: { b: { c: { value: string } } } }\n",
      "export interface Root { a: { b: { c: { value: number } } } }\n",
      "Root",
    );
  });

  it("a NESTED member's silent type drift is caught through structural expansion", () => {
    const golden = observeTypeScript([
      {
        fileName: "/nested.ts",
        code: "export declare const config: { inner: { deep: string } };\n",
      },
    ]);
    const candidate = observeTypeScript([
      {
        fileName: "/nested.ts",
        code: "export declare const config: { inner: { deep: number } };\n",
      },
    ]);
    expect(golden.diagnostics).toEqual([]);
    expect(candidate.diagnostics).toEqual([]);
    const comparison = compareObservations(golden, candidate);
    expect(comparison.equal).toBe(false);
    expect(comparison.differences.some((d) => d.includes("deep"))).toBe(true);
  });
});
