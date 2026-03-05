import { describe, it, expect } from "vitest";
import {
  buildComponentImportEdit,
  collectTemplateCompletions,
  collectTemplateInterpolationCompletions,
  computeAutoCloseTagText,
  relativeImportPath,
} from "./templateIde";
import type { FileAnalysis } from "../core/types";

function makeAnalysis(overrides: Partial<FileAnalysis> = {}): FileAnalysis {
  return {
    imports: [],
    bindings: [],
    macros: [],
    macroTypeDeps: [],
    scriptFlags: 0,
    styles: [],
    template: null,
    ...overrides,
  };
}

describe("templateIde tag completions", () => {
  it("suggests html tags when typing open tag", () => {
    const source = "<template>\n  <di\n</template>";
    const offset = source.indexOf("<di") + 3;

    const items = collectTemplateCompletions({
      source,
      offset,
      activeFilename: "App.vue",
      openFilenames: ["App.vue"],
      analysis: null,
    });

    expect(items.some((item) => item.label === "div")).toBe(true);
  });

  it("suggests sibling component names with auto-import edits", () => {
    const source = "<template>\n  <my\n</template>";
    const offset = source.indexOf("<my") + 3;

    const items = collectTemplateCompletions({
      source,
      offset,
      activeFilename: "src/App.vue",
      openFilenames: ["src/App.vue", "src/components/my-card.vue"],
      analysis: null,
    });

    const match = items.find((item) => item.label === "MyCard");
    expect(match).toBeTruthy();
    expect(match?.importEdit?.text).toContain("import MyCard from './components/my-card.vue'");
  });

  it("suggests closing tag for current open element", () => {
    const source = "<template><section><div></</template>";
    const offset = source.indexOf("</") + 2;

    const items = collectTemplateCompletions({
      source,
      offset,
      activeFilename: "App.vue",
      openFilenames: ["App.vue"],
      analysis: null,
    });

    expect(items).toHaveLength(1);
    expect(items[0].label).toBe("div");
    expect(items[0].insertText).toBe("div>");
  });

  it("suggests directive/attribute completions in tag attribute context", () => {
    const source = "<template><div ></div></template>";
    const offset = source.indexOf("<div ") + "<div ".length;

    const items = collectTemplateCompletions({
      source,
      offset,
      activeFilename: "App.vue",
      openFilenames: ["App.vue"],
      analysis: null,
    });

    expect(items.some((item) => item.label === "v-if")).toBe(true);
    expect(items.some((item) => item.label === "class")).toBe(true);
  });
});

describe("templateIde interpolation completions", () => {
  it("includes bindings and template globals inside interpolation", () => {
    const source = `<template><div>{{ cou }}</div></template>`;
    const offset = source.indexOf("cou") + 3;

    const analysis = makeAnalysis({
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          typeAnnotation: null,
          initializer: null,
        },
      ],
    });

    const items = collectTemplateInterpolationCompletions({
      source,
      offset,
      activeFilename: "App.vue",
      openFilenames: ["App.vue"],
      analysis,
    });

    expect(items.some((item) => item.label === "count")).toBe(true);
    expect(items.some((item) => item.label === "$props")).toBe(true);
    expect(items.some((item) => item.label === "Math")).toBe(true);
  });

  it("excludes type-only imports in interpolation completion list", () => {
    const source = `<template><div>{{ Pr }}</div></template>`;
    const offset = source.indexOf("Pr") + 2;

    const analysis = makeAnalysis({
      imports: [
        {
          source: "./types",
          isTypeOnly: true,
          bindings: [{ name: "Props", isTypeOnly: true, vueApi: null }],
        },
      ],
    });

    const items = collectTemplateInterpolationCompletions({
      source,
      offset,
      activeFilename: "App.vue",
      openFilenames: ["App.vue"],
      analysis,
    });

    expect(items.some((item) => item.label === "Props")).toBe(false);
  });
});

describe("templateIde import edits", () => {
  it("inserts script setup block when no script exists", () => {
    const source = "<template><MyCard /></template>";
    const edit = buildComponentImportEdit(source, "MyCard", "./MyCard.vue");

    expect(edit).toBeTruthy();
    expect(edit?.offset).toBe(0);
    expect(edit?.text).toContain('<script setup lang="ts">');
  });

  it("adds import after existing imports in script setup", () => {
    const source = `<script setup lang=\"ts\">\nimport Foo from './Foo.vue'\nconst x = 1\n</script>\n<template><MyCard /></template>`;
    const edit = buildComponentImportEdit(source, "MyCard", "./MyCard.vue");

    expect(edit).toBeTruthy();
    expect(edit?.text).toContain("import MyCard from './MyCard.vue'");
  });

  it("builds stable relative import paths", () => {
    expect(relativeImportPath("src/App.vue", "src/components/MyButton.vue")).toBe(
      "./components/MyButton.vue",
    );
    expect(relativeImportPath("src/pages/App.vue", "src/components/MyButton.vue")).toBe(
      "../components/MyButton.vue",
    );
  });
});

describe("templateIde auto close", () => {
  it("auto-closes non-void template tags", () => {
    const source = "<template><div></template>";
    const offset = source.indexOf("<div>") + "<div>".length;

    expect(computeAutoCloseTagText(source, offset)).toBe("</div>");
  });

  it("does not auto-close void tags", () => {
    const source = "<template><img></template>";
    const offset = source.indexOf("<img>") + "<img>".length;

    expect(computeAutoCloseTagText(source, offset)).toBeNull();
  });

  it("does not duplicate an existing close tag", () => {
    const source = "<template><div></div></template>";
    const offset = source.indexOf("<div>") + "<div>".length;

    expect(computeAutoCloseTagText(source, offset)).toBeNull();
  });
});
