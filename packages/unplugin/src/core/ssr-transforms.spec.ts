import { encode } from "@jridgewell/sourcemap-codec";
import { describe, expect, it } from "vitest";
import { replaceImportMetaSsr, stripComponents } from "./ssr-transforms.js";

describe("replaceImportMetaSsr", () => {
  describe("SSR build (isSSR = true)", () => {
    it("replaces import.meta.server with true", () => {
      const code = 'if (import.meta.server) { fetch("/api") }';
      const result = replaceImportMetaSsr(code, true).code;
      expect(result).toBe('if (true) { fetch("/api") }');
      expect(result).not.toContain("import.meta.server");
    });

    it("replaces import.meta.client with false", () => {
      const code = "if (import.meta.client) { initCanvas() }";
      const result = replaceImportMetaSsr(code, true).code;
      expect(result).toBe("if (false) { initCanvas() }");
      expect(result).not.toContain("import.meta.client");
    });

    it("replaces import.meta.env.SSR with true", () => {
      const code = "const isServer = import.meta.env.SSR";
      const result = replaceImportMetaSsr(code, true).code;
      expect(result).toBe("const isServer = true");
      expect(result).not.toContain("import.meta.env.SSR");
    });

    it("handles multiple occurrences", () => {
      const code = [
        "const a = import.meta.server",
        "const b = import.meta.client",
        "const c = import.meta.env.SSR",
      ].join("\n");
      const result = replaceImportMetaSsr(code, true).code;
      expect(result).toBe(["const a = true", "const b = false", "const c = true"].join("\n"));
    });
  });

  describe("Client build (isSSR = false)", () => {
    it("replaces import.meta.server with false", () => {
      const code = "if (import.meta.server) { return }";
      const result = replaceImportMetaSsr(code, false).code;
      expect(result).toBe("if (false) { return }");
    });

    it("replaces import.meta.client with true", () => {
      const code = "if (import.meta.client) { initCanvas() }";
      const result = replaceImportMetaSsr(code, false).code;
      expect(result).toBe("if (true) { initCanvas() }");
    });

    it("replaces import.meta.env.SSR with false", () => {
      const code = "const isServer = import.meta.env.SSR";
      const result = replaceImportMetaSsr(code, false).code;
      expect(result).toBe("const isServer = false");
    });
  });

  it("returns unchanged code when no import.meta. present", () => {
    const code = 'const x = 42; console.log("hello")';
    const result = replaceImportMetaSsr(code, true).code;
    expect(result).toBe(code);
  });

  it("returns the map unchanged when no import.meta. present", () => {
    const code = 'const x = 42; console.log("hello")';
    const map = JSON.stringify({ version: 3, sources: [], names: [], mappings: "" });
    const result = replaceImportMetaSsr(code, true, map);
    expect(result.map).toBe(map);
  });

  it("shifts a later token's mapped column by the length the rewrite removed", () => {
    // `import.meta.server` (19 chars) → `false` (5 chars): a 14-byte shrink.
    // A mapping naming a source position at generated column 30 — right
    // after the rewritten token — must land 14 columns earlier post-rewrite.
    const code = "const a = import.meta.server; const later = 1";
    const beforeCol = code.indexOf("later"); // column of `later` in the ORIGINAL code
    const mappings = encode([
      [
        [0, 0, 0, 0],
        [beforeCol, 0, 0, beforeCol],
      ],
    ]);
    const map = JSON.stringify({
      version: 3,
      sources: ["input.js"],
      names: [],
      mappings,
    });

    const result = replaceImportMetaSsr(code, false, map);
    expect(result.code).toBe("const a = false; const later = 1");

    const decodedMap = JSON.parse(result.map!);
    // `later`'s new generated column should have shrunk by the same 14 bytes
    // the replacement removed, not stayed at its stale pre-rewrite column.
    expect(decodedMap.mappings).not.toBe(mappings);
    const newCol = beforeCol - ("import.meta.server".length - "false".length);
    expect(result.code.slice(newCol, newCol + 5)).toBe("later");
  });
});

describe("stripComponents", () => {
  it("replaces _resolveComponent calls for listed components", () => {
    const code = `const _component_GoogleMap = _resolveComponent("GoogleMap")`;
    const result = stripComponents(code, ["GoogleMap"]).code;
    expect(result).not.toContain('_resolveComponent("GoogleMap")');
    expect(result).toContain("GoogleMap");
  });

  it("does not affect unlisted components", () => {
    const code = `const _component_MyComp = _resolveComponent("MyComp")`;
    const result = stripComponents(code, ["GoogleMap"]).code;
    expect(result).toBe(code);
  });

  it("handles multiple components", () => {
    const code = [
      `const a = _resolveComponent("GoogleMap")`,
      `const b = _resolveComponent("VideoPlayer")`,
      `const c = _resolveComponent("SafeComp")`,
    ].join("\n");
    const result = stripComponents(code, ["GoogleMap", "VideoPlayer"]).code;
    expect(result).not.toContain('_resolveComponent("GoogleMap")');
    expect(result).not.toContain('_resolveComponent("VideoPlayer")');
    expect(result).toContain('_resolveComponent("SafeComp")');
  });

  it("returns unchanged code when no components to strip", () => {
    const code = 'const x = _resolveComponent("Foo")';
    const result = stripComponents(code, []).code;
    expect(result).toBe(code);
  });
});
