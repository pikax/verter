import { describe, it, expect } from "vitest";
import { getTypeDiagnosticsSourceMap } from "./diagnosticSourceMap";

describe("getTypeDiagnosticsSourceMap", () => {
  it("uses TSX source map for TS diagnostics mapping", () => {
    const selected = getTypeDiagnosticsSourceMap({
      typesSourceMap: "tsx-map",
      verterSourceMap: "template-map",
    });

    expect(selected).toBe("tsx-map");
  });

  it("returns null when TSX source map is missing", () => {
    const selected = getTypeDiagnosticsSourceMap({
      typesSourceMap: "",
      verterSourceMap: "template-map",
    });

    expect(selected).toBeNull();
  });
});
