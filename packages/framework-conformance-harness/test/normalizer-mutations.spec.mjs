// Self-test: normalizer positive/negative discrimination with PROVEN-applied
// mutations (BF2 required exit + CLAUDE.md "Verification Must Prove
// Execution": every mutation below is asserted to have actually changed the
// text — and to be genuinely NEW relative to the pre-mutation source — before
// the pass/fail result is trusted).

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compareArtifacts, compareMappings } from "../src/compare.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

const FIXTURE = readFileSync(
  path.join(HARNESS_ROOT, "fixtures/vue/basic-interpolation.vue"),
  "utf8",
);

function goldenVdom() {
  return compileVueFixture(FIXTURE, "fixtures/vue/basic-interpolation.vue", {
    backend: "vdom",
    sourceMap: false,
    isProd: false,
  });
}

/** Proves a mutation actually applied and is distinct from the original. */
function assertMutationApplied(original, mutated) {
  expect(mutated).not.toBe(original);
  expect(mutated.length === original.length && mutated === original).toBe(false);
}

describe("normalizer — allowed cosmetic mutations (must PASS)", () => {
  it("whitespace/line-layout reflow", () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace(/\n/g, "\n\n").replace(/ {2}/g, "    ");
    assertMutationApplied(golden.code, mutated);
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("pass");
  });

  it("quote-delimiter spelling (identical decoded value)", () => {
    const golden = goldenVdom();
    const mutated = golden.code.replaceAll('"root"', "'root'").replaceAll("'vue'", '"vue"');
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("root"); // proves the literal survives, just re-quoted
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("pass");
  });

  it("harmless redundant parentheses proven equivalent by the parser", () => {
    const a = "export default function f(x) { return x + 1; }";
    const b = "export default function f(x) { return (x + 1); }";
    assertMutationApplied(a, b);
    const report = compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("pass");
  });

  it("private generated identifier spelling under scope-aware alpha-renaming", () => {
    const golden = goldenVdom();
    const mutated = golden.code.replaceAll("_sfc_main", "_component_impl_renamed");
    assertMutationApplied(golden.code, mutated);
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("pass");
  });
});

describe("normalizer — forbidden mutations (must be CAUGHT, every category)", () => {
  it("helper-source substitution (renamed imported helper)", () => {
    const golden = goldenVdom();
    const mutated = golden.code.replaceAll("createElementVNode", "createElementVNodeEvil");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("createElementVNodeEvil"); // proves it is genuinely new text
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
    expect(report.reasons.join(" ")).toMatch(/structural divergence/);
  });

  it("prop/attribute value swap (literal changed)", () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace('"root"', '"ROOT_SWAPPED"');
    assertMutationApplied(golden.code, mutated);
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("reordered statements (effect order changed)", () => {
    const a = "const x = 1;\nconst y = 2;\nexport default x + y;";
    const b = "const y = 2;\nconst x = 1;\nexport default x + y;";
    assertMutationApplied(a, b);
    const report = compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("missing hydration/fragment marker (a codegen call argument removed)", () => {
    const golden = goldenVdom();
    // Remove one whole argument (the PatchFlag / fragment marker constant
    // Vue appends to createElementBlock calls) — a real structural removal,
    // not a cosmetic edit.
    const mutated = golden.code.replace(/, 64 \/\* STABLE_FRAGMENT \*\//, "");
    if (mutated === golden.code) {
      // Fixture-shape-dependent constant not present in this cell; fall back
      // to removing a whole statement instead so the category is still
      // exercised as "structure removed", never silently skipped.
      const fallback = golden.code.replace(/const items = \[.*?\];\n?/, "");
      assertMutationApplied(golden.code, fallback);
      const report = compareArtifacts(
        golden,
        { ...golden, code: fallback },
        { linkBaseDir: HARNESS_ROOT },
      );
      expect(report.verdict).toBe("fail");
      return;
    }
    assertMutationApplied(golden.code, mutated);
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("altered escaping (SSR-rendered literal content changed)", () => {
    const ssr = compileVueFixture(FIXTURE, "fixtures/vue/basic-interpolation.vue", {
      backend: "ssr",
      sourceMap: false,
      isProd: false,
    });
    // "zero" is rendered as literal template-string content
    // (`<p>zero</p>`), not a quoted Literal — mutate the TemplateElement
    // content itself.
    const mutated = ssr.code.replace("<p>zero</p>", "<p>ZERO_MUTATED</p>");
    assertMutationApplied(ssr.code, mutated);
    const report = compareArtifacts(ssr, { ...ssr, code: mutated }, { linkBaseDir: HARNESS_ROOT });
    expect(report.verdict).toBe("fail");
  });

  it("diagnostic-span drift", () => {
    const golden = {
      code: "export default 1;",
      diagnostics: [{ kind: "warning", code: "x", start: { line: 1, column: 1 } }],
    };
    const mutated = {
      code: golden.code,
      diagnostics: [{ kind: "warning", code: "x", start: { line: 1, column: 99 } }],
    };
    assertMutationApplied(JSON.stringify(golden.diagnostics), JSON.stringify(mutated.diagnostics));
    const report = compareArtifacts(golden, mutated, { linkBaseDir: HARNESS_ROOT });
    expect(report.verdict).toBe("fail");
    expect(report.diagnostics.equal).toBe(false);
  });

  it("mapping drift (source map mappings string changed)", () => {
    const goldenMap = { mappings: "AAAA,CAAC", sources: ["x.vue"], names: [] };
    const mutatedMap = { mappings: "AAAA,CAAD", sources: ["x.vue"], names: [] };
    assertMutationApplied(goldenMap.mappings, mutatedMap.mappings);
    expect(compareMappings(goldenMap, mutatedMap).equal).toBe(false);
    const report = compareArtifacts(
      { code: "export default 1;", diagnostics: [], map: goldenMap },
      { code: "export default 1;", diagnostics: [], map: mutatedMap },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
    expect(report.mapping.equal).toBe(false);
  });

  it("scope capture/shadowing attack — an inner-scope reference redirected to an outer binding", () => {
    // Two snippets that are textually similar but semantically different: in
    // `a`, the inner function's `return x` refers to the INNER `let x = 2`
    // (shadowing); in `b`, the inner declaration is removed so the same
    // `return x` now refers to the OUTER `let x = 1`. A scope-naive
    // textual/renaming scheme could conflate these; a correct scope-aware
    // one must not.
    const a = "let x = 1;\nfunction f() {\n  let x = 2;\n  return x;\n}\nexport default f;";
    const b = "let x = 1;\nfunction f() {\n  return x;\n}\nexport default f;";
    assertMutationApplied(a, b);
    const report = compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("import/export-source substitution (candidate imports the runtime helpers from a different specifier)", () => {
    const golden = goldenVdom();
    // The compiled fragment imports its VDOM helpers from "vue" — retarget
    // the import SOURCE itself (not a helper name) to a different package
    // specifier, exactly the class checkLinkValidity/normalizer must catch.
    const mutated = golden.code.replace('from "vue"', 'from "vue-evil-fork"');
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain('"vue-evil-fork"');
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("event binding mutation (authored event name changed on emit + declaration)", () => {
    const propsEmit = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/props-emit.vue"), "utf8");
    const golden = compileVueFixture(propsEmit, "fixtures/vue/props-emit.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(golden.code).toContain('emit("toggle"');
    const mutated = golden.code
      .replace('emit("toggle"', 'emit("toggled"')
      .replace('emits: ["toggle"]', 'emits: ["toggled"]');
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain('"toggled"');
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("component-call mutation (candidate mounts a different child component)", () => {
    const a =
      'import { createVNode as _createVNode } from "vue";\nimport Comp from "./Comp.js";\nexport default function render() { return _createVNode(Comp); }';
    const b =
      'import { createVNode as _createVNode } from "vue";\nimport OtherComp from "./OtherComp.js";\nexport default function render() { return _createVNode(OtherComp); }';
    assertMutationApplied(a, b);
    const report = compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("slot-name mutation (renderSlot target renamed — a named slot silently becomes a different slot)", () => {
    const slots = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/slots.vue"), "utf8");
    const golden = compileVueFixture(slots, "fixtures/vue/slots.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(golden.code).toContain('_renderSlot(_ctx.$slots, "header"');
    const mutated = golden.code.replace(
      '_renderSlot(_ctx.$slots, "header"',
      '_renderSlot(_ctx.$slots, "banner"',
    );
    assertMutationApplied(golden.code, mutated);
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("authored/public prop-name mutation (a component's public prop key renamed)", () => {
    const propsEmit = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/props-emit.vue"), "utf8");
    const golden = compileVueFixture(propsEmit, "fixtures/vue/props-emit.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(golden.code).toContain("label: { type: String");
    // Rename the PUBLIC prop key everywhere it is referenced — a real API
    // surface change, not a cosmetic rename of a private generated binding.
    const mutated = golden.code.replaceAll("label", "caption");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("caption: { type: String");
    const report = compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("control-flow mutation (if/else branches swapped — same total text shape, different runtime path)", () => {
    const a = "export default function f(cond) {\n  if (cond) { return 1; } else { return 2; }\n}";
    const b = "export default function f(cond) {\n  if (cond) { return 2; } else { return 1; }\n}";
    assertMutationApplied(a, b);
    const report = compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail");
  });

  it("scope-aware renaming does not FALSELY equate two genuinely different same-named-shadow programs", () => {
    // Both programs use the identifier `v` twice (outer + inner), but bind
    // it to a different VALUE in each — proving canonical renaming is keyed
    // by binding identity within a fixed structural position, not merely by
    // "some local variable exists here".
    const a = "let v = 1;\nfunction f() {\n  let v = 2;\n  return v;\n}\nexport default f;";
    const c = "let v = 1;\nfunction f() {\n  let v = 3;\n  return v;\n}\nexport default f;";
    assertMutationApplied(a, c);
    const report = compareArtifacts(
      { code: a, diagnostics: [] },
      { code: c, diagnostics: [] },
      { linkBaseDir: HARNESS_ROOT },
    );
    expect(report.verdict).toBe("fail"); // literal 2 vs 3 — a genuine value difference
  });
});
