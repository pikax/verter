/**
 * Admission of a compile response and of a native failure: what is published,
 * what is refused by name, and which missing inputs the embedder can supply.
 */
import { describe, expect, it } from "vitest";
import type { HostCompileResponse, HostCompileVirtualNode, HostMissingInput } from "@verter/native";
import {
  admitCompile,
  fillableInputs,
  missingInputKey,
  refusalFromCompileError,
} from "./admission";

const CARRIER = "/p/App.vue";

function node(
  kind: HostCompileVirtualNode["node"]["kind"],
  code: string,
  index?: number,
): HostCompileVirtualNode {
  return { node: { kind, index }, code, lang: kind === "style" ? "css" : "js", meta: {} };
}

function responseOf(
  nodes: HostCompileVirtualNode[],
  kind: "runtimeClient" | "runtimeServer" = "runtimeClient",
): HostCompileResponse {
  return {
    canonicalId: CARRIER,
    diagnostics: {} as HostCompileResponse["diagnostics"],
    products: [{ kind, nodes }],
  };
}

describe("compile admission", () => {
  it("a response without a runtime main for the requested target is refused by carrier", () => {
    const response = responseOf([node("main", "export default {}")], "runtimeClient");

    expect(() => admitCompile(response, true)).toThrow(CARRIER);
  });

  it("styles are returned by block ordinal whatever the node order", () => {
    const response = responseOf([
      node("main", "export default {}"),
      node("style", ".b{}", 1),
      node("style", ".a{}", 0),
    ]);

    const admitted = admitCompile(response, false);

    expect(admitted.styles.map((style) => style.code)).toEqual([".a{}", ".b{}"]);
    expect(admitted.main.code).toBe("export default {}");
  });

  it("a hole in the style ordinals is refused", () => {
    const response = responseOf([
      node("main", "export default {}"),
      node("style", ".a{}", 0),
      node("style", ".c{}", 2),
    ]);

    expect(() => admitCompile(response, false)).toThrow(/compiled style 1 is missing/);
  });

  it("a style published twice is refused", () => {
    const response = responseOf([
      node("main", "export default {}"),
      node("style", ".a{}", 0),
      node("style", ".again{}", 0),
    ]);

    expect(() => admitCompile(response, false)).toThrow(/published twice/);
  });

  it("nodes that are neither main nor style are kept as they are", () => {
    const template = node("template", "export function render() {}");
    const response = responseOf([node("main", "export default {}"), template]);

    expect(admitCompile(response, false).nodes).toEqual([template]);
  });
});

describe("native compile failures", () => {
  it("an incomplete-dependencies failure names the inputs the host reported", () => {
    const missingInputs: HostMissingInput[] = [{ kind: "file", canonicalId: "/p/x.d.ts" }];

    const refusal = refusalFromCompileError(
      Object.assign(new Error("incomplete"), { kind: "dependenciesIncomplete", missingInputs }),
    );

    expect(refusal).toMatchObject({ kind: "need-inputs", inputs: missingInputs });
  });

  it("any other failure is not a refusal", () => {
    expect(refusalFromCompileError(Object.assign(new Error("x"), { kind: "refused" }))).toBeNull();
    expect(refusalFromCompileError("text")).toBeNull();
  });
});

describe("missing inputs", () => {
  const file: HostMissingInput = { kind: "file", canonicalId: "/p/x.d.ts" };
  const route: HostMissingInput = {
    kind: "route",
    importer: "/p/a.d.ts",
    specifier: "./b",
    refused: false,
  };
  const refused: HostMissingInput = { ...route, refused: true };
  const lookup: HostMissingInput = { kind: "lookup", importSource: "./b", typeName: "B" };

  it("keys differ for different inputs and agree for equal ones", () => {
    const keys = [file, route, refused, lookup].map(missingInputKey);

    expect(new Set(keys).size).toBe(3);
    expect(missingInputKey({ ...file })).toBe(missingInputKey(file));
    expect(missingInputKey(route)).toBe(missingInputKey(refused));
  });

  it("only files and unrefused routes are fillable", () => {
    expect(fillableInputs([lookup, file, refused, route], new Set())).toEqual([file, route]);
  });

  it("a report with nothing fillable is terminal", () => {
    expect(fillableInputs([lookup, refused], new Set())).toBe("terminal");
  });

  it("a fillable input already supplied is terminal", () => {
    expect(fillableInputs([file, route], new Set([missingInputKey(route)]))).toBe("terminal");
  });
});
