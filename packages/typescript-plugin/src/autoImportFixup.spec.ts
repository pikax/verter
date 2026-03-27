import { describe, it, expect } from "vitest";

// The fixup logic from the plugin — extracted as pure functions for testing.
// These mirror the transformations in index.ts.

function fixVueDtsInText(text: string): string {
  return text.replace(/\.vue\.d\.ts/g, ".vue");
}

function fixVueDtsInImport(newText: string): string {
  return newText.replace(/\.vue\.d\.ts(['"])/g, ".vue$1");
}

function fixSourceProperty(source: string | undefined): string | undefined {
  if (source?.endsWith(".vue.d.ts")) {
    return source.slice(0, -5); // strip .d.ts
  }
  return source;
}

function fixSourceDisplay(
  parts: Array<{ text: string; kind: string }>,
): Array<{ text: string; kind: string }> {
  return parts.map((part) => ({
    ...part,
    text: part.text.replace(/\.vue\.d\.ts/g, ".vue"),
  }));
}

describe("auto-import .vue.d.ts → .vue fixup", () => {
  // ── Text fixup (descriptions, display text) ──

  it("strips .vue.d.ts in text", () => {
    expect(fixVueDtsInText("./Foo.vue.d.ts")).toBe("./Foo.vue");
    // Negative: non-.vue files unchanged
    expect(fixVueDtsInText("./utils.ts")).toBe("./utils.ts");
  });

  it("strips .vue.d.ts in action description", () => {
    const desc = 'Add import from "./components/Button.vue.d.ts"';
    const fixed = fixVueDtsInText(desc);
    expect(fixed).toBe('Add import from "./components/Button.vue"');
    expect(fixed).not.toContain(".d.ts");
  });

  // ── Import text fixup ──

  it("strips .vue.d.ts in import with double quotes", () => {
    const text = 'import Foo from "./Foo.vue.d.ts"';
    expect(fixVueDtsInImport(text)).toBe('import Foo from "./Foo.vue"');
  });

  it("strips .vue.d.ts in import with single quotes", () => {
    const text = "import Foo from './Foo.vue.d.ts'";
    expect(fixVueDtsInImport(text)).toBe("import Foo from './Foo.vue'");
  });

  it("does not modify non-.vue imports", () => {
    const text = 'import { x } from "./utils"';
    expect(fixVueDtsInImport(text)).toBe(text);
  });

  it("fixes multiple .vue.d.ts references in one text", () => {
    const text = 'import Foo from "./Foo.vue.d.ts"\nimport Bar from "./Bar.vue.d.ts"';
    const fixed = fixVueDtsInImport(text);
    expect(fixed).toContain('./Foo.vue"');
    expect(fixed).toContain('./Bar.vue"');
    expect(fixed).not.toContain(".d.ts");
  });

  // ── Source property fixup ──

  it("strips .d.ts from source property ending in .vue.d.ts", () => {
    expect(fixSourceProperty("./Foo.vue.d.ts")).toBe("./Foo.vue");
  });

  it("does not modify source property not ending in .vue.d.ts", () => {
    expect(fixSourceProperty("./utils")).toBe("./utils");
    expect(fixSourceProperty(undefined)).toBeUndefined();
  });

  it("does not modify source ending in just .d.ts", () => {
    // Only strip when it's specifically .vue.d.ts
    expect(fixSourceProperty("./types.d.ts")).toBe("./types.d.ts");
  });

  // ── Source display fixup ──

  it("fixes source display parts", () => {
    const parts = [{ text: "./components/Button.vue.d.ts", kind: "text" }];
    const fixed = fixSourceDisplay(parts);
    expect(fixed[0].text).toBe("./components/Button.vue");
    expect(fixed[0].kind).toBe("text"); // preserves other properties
  });

  it("preserves non-.vue source display parts", () => {
    const parts = [{ text: "./utils", kind: "text" }];
    const fixed = fixSourceDisplay(parts);
    expect(fixed[0].text).toBe("./utils");
  });

  it("handles empty source display", () => {
    expect(fixSourceDisplay([])).toEqual([]);
  });

  // ── Edge cases ──

  it("handles deeply nested .vue paths", () => {
    const text = 'import Comp from "../../src/components/deep/MyComp.vue.d.ts"';
    expect(fixVueDtsInImport(text)).toBe('import Comp from "../../src/components/deep/MyComp.vue"');
  });

  it("does not double-fix already clean paths", () => {
    const text = 'import Foo from "./Foo.vue"';
    expect(fixVueDtsInImport(text)).toBe(text);
  });
});
