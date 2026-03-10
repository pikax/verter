import { describe, expect, it } from "vitest";

import { collectResolvableModuleReferenceSpecifiers } from "./dependency-resolution";

describe("collectResolvableModuleReferenceSpecifiers", () => {
  it("keeps exact and finite-set candidates in encounter order", () => {
    const specifiers = collectResolvableModuleReferenceSpecifiers([
      {
        syntax: "staticImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "'./Foo.vue'",
        literalSpecifier: "./Foo.vue",
        finiteSpecifiers: [],
        analyzability: "exact",
        spanStart: 0,
        spanEnd: 10,
        exprSpanStart: 0,
        exprSpanEnd: 10,
      },
      {
        syntax: "dynamicImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "`./${name}.vue`",
        finiteSpecifiers: ["./Bar.vue", "./Foo.vue", "./types"],
        analyzability: "finiteSet",
        spanStart: 12,
        spanEnd: 28,
        exprSpanStart: 12,
        exprSpanEnd: 28,
      },
    ]);

    expect(specifiers).toEqual(["./Foo.vue", "./Bar.vue", "./types"]);
  });

  it("skips unknown dynamic references without speculative prefix matching", () => {
    const specifiers = collectResolvableModuleReferenceSpecifiers([
      {
        syntax: "dynamicImport",
        semantics: "import",
        isTypeOnly: false,
        rawText: "`./${name}.vue`",
        finiteSpecifiers: [],
        staticPrefix: "./",
        analyzability: "unknownDynamic",
        spanStart: 0,
        spanEnd: 15,
        exprSpanStart: 0,
        exprSpanEnd: 15,
      },
    ]);

    expect(specifiers).toEqual([]);
  });
});
