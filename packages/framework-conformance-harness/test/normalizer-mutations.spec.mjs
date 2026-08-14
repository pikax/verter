// Self-test: normalizer positive/negative discrimination with PROVEN-applied
// mutations (BF2 required exit + CLAUDE.md "Verification Must Prove
// Execution": every mutation below is asserted to have actually changed the
// text — and to be genuinely NEW relative to the pre-mutation source — before
// the pass/fail result is trusted).
//
// The forbidden-mutation categories are derived DIRECTLY from
// docs/arch/refactor/rev11/contracts/conformance-normalizer.md ("Forbidden
// normalization" + "Required discrimination"), one labelled test per
// category, each planting a mutation genuinely REPRESENTATIVE of that
// category — a literal substitution never stands in for a prop/attribute
// test, swapped constants never stand in for an effect-order test, and a
// property-key mutation never stands in for an authored-local-name test.
//
// Identifier rule under test (see src/normalize.mjs header): identifiers
// are STRUCTURAL — the pinned official compilers emit no private-generated
// provenance marker, so NO binding is ever alpha-renamed away. Renaming any
// binding — authored or generated-looking — is a structural difference.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { compareArtifacts } from "../src/compare.mjs";
import { decodeMappings, encodeMappings } from "../src/sourcemap.mjs";
import { FIXTURE_ANCHORS, MAPPING_PROFILES } from "../src/mapping-oracle.mjs";
import { parseModule } from "../src/normalize.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";
import { oracleLinkBaseDir } from "../src/oracle-install.mjs";

const VUE_BASE = oracleLinkBaseDir("vue");
const SVELTE_BASE = oracleLinkBaseDir("svelte");

const VUE_FIXTURE_PATH = "fixtures/vue/basic-interpolation.vue";
const FIXTURE = readFileSync(path.join(HARNESS_ROOT, VUE_FIXTURE_PATH), "utf8");

function goldenVdom() {
  return compileVueFixture(FIXTURE, "fixtures/vue/basic-interpolation.vue", {
    backend: "vdom",
    sourceMap: false,
    isProd: false,
  });
}

function goldenSvelteClient() {
  const source = readFileSync(
    path.join(HARNESS_ROOT, "fixtures/svelte/basic-runes.svelte"),
    "utf8",
  );
  return compileSvelteFixture(source, "fixtures/svelte/basic-runes.svelte", {
    generate: "client",
    runes: true,
    dev: false,
    sourceMap: false,
  });
}

/** Proves a mutation actually applied and is distinct from the original. */
function assertMutationApplied(original, mutated) {
  expect(mutated).not.toBe(original);
  expect(mutated.length === original.length && mutated === original).toBe(false);
}

describe("normalizer — allowed cosmetic mutations (must PASS)", () => {
  it("whitespace/line-layout reflow", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace(/\n/g, "\n\n").replace(/ {2}/g, "    ");
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("pass");
  });

  it("quote-delimiter spelling (identical decoded value)", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replaceAll('"root"', "'root'");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("root"); // proves the literal survives, just re-quoted
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("pass");
  });

  it("harmless redundant parentheses proven equivalent by the parser", async () => {
    const a = "export default function f(x) { return x + 1; }";
    const b = "export default function f(x) { return (x + 1); }";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("pass");
  });

  it("NON-tagged template raw escape-spelling change (identical cooked value) is cosmetic", async () => {
    // Companion to the forbidden tagged-template test below: an ordinary
    // (untagged) template literal exposes only its COOKED value to any
    // receiver, so a raw-spelling-only difference stays free — proving the
    // tagged-template fix did not overcorrect into raw-comparing every
    // template.
    const a = "const s = `a\\u0041b`;\nexport default s;";
    const b = "const s = `aAb`;\nexport default s;";
    assertMutationApplied(a, b);
    // Precondition: cooked values identical, raw spellings different.
    const cookedOf = (code) =>
      parseModule(code).body[0].declarations[0].init.quasis.map((q) => q.value.cooked);
    const rawOf = (code) =>
      parseModule(code).body[0].declarations[0].init.quasis.map((q) => q.value.raw);
    expect(cookedOf(a)).toEqual(cookedOf(b));
    expect(rawOf(a)).not.toEqual(rawOf(b));
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("pass");
  });

  it("plain prose comments (no semantic force) are cosmetic", async () => {
    const golden = goldenVdom();
    const mutated = `// harness prose note, consumed by no tool\n${golden.code}`;
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("pass");
  });
});

describe("normalizer — forbidden mutations (must be CAUGHT, every contract category)", () => {
  // Category: import/export sources (import half) + helper-source substitution.
  it("import-source substitution (helpers imported from a different specifier)", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace('from "vue"', 'from "vue-evil-fork"');
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain('"vue-evil-fork"');
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: import/export sources (export/re-export half).
  it("export-source substitution (re-export retargeted to a different specifier)", async () => {
    const a = 'export { ref } from "vue";';
    const b = 'export { ref } from "vue-evil-fork";';
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: helper families / "canonicalize different helpers to one label".
  it("helper-family substitution (renamed imported helper)", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replaceAll("createElementVNode", "createElementVNodeEvil");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("createElementVNodeEvil");
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.reasons.join(" ")).toMatch(/structural divergence|missing named exports/);
  });

  // Category: declarations removed ("remove declarations").
  it("declaration removal (a hoisted declaration statement deleted, module still parses)", async () => {
    const golden = goldenVdom();
    expect(golden.code).toContain("const _hoisted_3 = { key: 1 }");
    const mutated = golden.code.replace(/const _hoisted_3 = \{ key: 1 \}\n/, "");
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.candidateParse.ok).toBe(true); // removal is NOT a parse error — only structure catches it
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: side effects reordered — two GENUINELY effectful calls (real
  // official Svelte output mutating shared DOM state), not constants.
  it("reordered side effects (two real effectful runtime calls swapped)", async () => {
    const golden = goldenSvelteClient();
    expect(golden.code).toContain("$.reset(ul);");
    expect(golden.code).toContain("$.reset(div);");
    expect(golden.code.indexOf("$.reset(ul);")).toBeLessThan(golden.code.indexOf("$.reset(div);"));
    const mutated = golden.code.replace(
      "$.reset(ul);\n\t$.reset(div);",
      "$.reset(div);\n\t$.reset(ul);",
    );
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: SVELTE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: side effects reordered — two assignments to a shared variable.
  it("reordered side effects (two assignments to one shared variable swapped)", async () => {
    const a = "let t = 0;\nt = t + 1;\nt = t * 2;\nexport default t;";
    const b = "let t = 0;\nt = t * 2;\nt = t + 1;\nexport default t;";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: DOM nodes erased (a real template element removed from
  // official Svelte client output's DOM template).
  it("DOM-node removal (an element deleted from the compiled DOM template)", async () => {
    const golden = goldenSvelteClient();
    expect(golden.code).toContain('<div class="root"><!> <ul></ul></div>');
    const mutated = golden.code.replace(
      '<div class="root"><!> <ul></ul></div>',
      '<div class="root"><!> </div>',
    );
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: SVELTE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: blocks/effects erased (a real reactive-effect statement removed).
  it("effect removal (a template_effect statement deleted from official output)", async () => {
    const golden = goldenSvelteClient();
    expect(golden.code).toContain("$.template_effect(() => $.set_text(text, item));");
    const mutated = golden.code.replace("$.template_effect(() => $.set_text(text, item));\n", "");
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: SVELTE_BASE },
    );
    expect(report.candidateParse.ok).toBe(true);
    expect(report.verdict).toBe("fail");
  });

  // Category: events.
  it("event binding mutation (authored event name changed on emit + declaration)", async () => {
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
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: props/attributes — the PROP NAME swapped, value untouched
  // (a literal substitution must not stand in for this).
  it("prop/attribute-name swap (prop key changed, value identical)", async () => {
    const golden = goldenVdom();
    expect(golden.code).toContain('{ class: "root" }');
    const mutated = golden.code.replace('{ class: "root" }', '{ id: "root" }');
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain('{ id: "root" }');
    expect(mutated).not.toContain('{ class: "root" }');
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: props/attributes — a prop erased entirely.
  it("prop/attribute erasure (prop removed from the element's props object)", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace('{ class: "root" }', "{}");
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: component calls.
  it("component-call mutation (candidate mounts a different child component)", async () => {
    const a =
      'import { createVNode as _createVNode } from "vue";\nimport Comp from "./Comp.js";\nexport default function render() { return _createVNode(Comp); }';
    const b =
      'import { createVNode as _createVNode } from "vue";\nimport OtherComp from "./OtherComp.js";\nexport default function render() { return _createVNode(OtherComp); }';
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: slots.
  it("slot-name mutation (renderSlot target renamed — a named slot silently becomes a different slot)", async () => {
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
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: hydration markers (the patch-flag argument Vue appends to a
  // fragment block call — its removal changes hydration/patch behavior).
  it("missing hydration/fragment marker (the STABLE_FRAGMENT patch-flag argument removed)", async () => {
    const golden = goldenVdom();
    expect(golden.code).toContain(", 64 /* STABLE_FRAGMENT */"); // precondition, no fallback
    const mutated = golden.code.replace(", 64 /* STABLE_FRAGMENT */", "");
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: SSR structure (static SSR-rendered content mutated).
  it("SSR structure mutation (server-rendered literal content changed)", async () => {
    const ssr = compileVueFixture(FIXTURE, "fixtures/vue/basic-interpolation.vue", {
      backend: "ssr",
      sourceMap: false,
      isProd: false,
    });
    expect(ssr.code).toContain("<p>zero</p>");
    const mutated = ssr.code.replace("<p>zero</p>", "<p>ZERO_MUTATED</p>");
    assertMutationApplied(ssr.code, mutated);
    const report = await compareArtifacts(
      ssr,
      { ...ssr, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: altered SSR escaping — an escaped entity un-escaped in place.
  it("altered SSR escaping (an HTML entity un-escaped inside emitted static markup)", async () => {
    const a = "export default function ssrRender() { return `<p>&lt;script&gt;</p>`; }";
    const b = "export default function ssrRender() { return `<p><script></p>`; }";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: diagnostics (span drift; full-field discrimination is
  // exhaustively covered in diagnostic-mapping-discrimination.spec.mjs).
  it("diagnostic-span drift", async () => {
    const golden = {
      code: "export default 1;",
      diagnostics: [{ kind: "warning", code: "x", start: { line: 1, column: 1 } }],
    };
    const mutated = {
      code: golden.code,
      diagnostics: [{ kind: "warning", code: "x", start: { line: 1, column: 99 } }],
    };
    assertMutationApplied(JSON.stringify(golden.diagnostics), JSON.stringify(mutated.diagnostics));
    const report = await compareArtifacts(golden, mutated);
    expect(report.verdict).toBe("fail");
    expect(report.diagnostics.equal).toBe(false);
  });

  // Category: mappings. The axis is SELF-REFERENTIAL — the candidate's map
  // is validated against the candidate's own generated code and the authored
  // fixture, never against the golden's map (mapping-oracle.mjs explains why
  // the latter cannot work). Exhaustive discrimination lives in
  // test/mapping-oracle*.spec.mjs; what is locked here is that the axis is
  // wired into compareArtifacts and can fail a report.
  it("mapping drift (a candidate map that lies about its own output)", async () => {
    const golden = compileVueFixture(FIXTURE, VUE_FIXTURE_PATH, {
      backend: "vdom",
      sourceMap: true,
      isProd: false,
    });
    const segments = decodeMappings(golden.map.mappings);
    const shifted = segments.map((segment, index) =>
      index === 0 && segment.srcCol !== null ? { ...segment, srcCol: segment.srcCol + 1 } : segment,
    );
    const mutatedMappings = encodeMappings(shifted);
    assertMutationApplied(golden.map.mappings, mutatedMappings);
    const mappingContext = {
      sourceMapRequested: true,
      fixture: {
        path: VUE_FIXTURE_PATH,
        absolutePath: path.join(HARNESS_ROOT, VUE_FIXTURE_PATH),
      },
      sourceResolveBases: [HARNESS_ROOT],
      profile: MAPPING_PROFILES["vue:vdom"],
      anchors: FIXTURE_ANCHORS[VUE_FIXTURE_PATH],
    };
    const clean = await compareArtifacts(golden, golden, { mappingContext });
    expect(clean.mapping.ok).toBe(true);
    const report = await compareArtifacts(
      golden,
      { ...golden, map: { ...golden.map, mappings: mutatedMappings } },
      { mappingContext },
    );
    expect(report.verdict).toBe("fail");
    expect(report.mapping.ok).toBe(false);
  });

  // Category: literal values.
  it("literal value change (a string literal's decoded value swapped)", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace('"root"', '"ROOT_SWAPPED"');
    assertMutationApplied(golden.code, mutated);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: source-authored names — a REAL authored LOCAL BINDING (the
  // v-for iteration variable `item`, authored in the fixture template)
  // consistently renamed. Under an alpha-renaming normalizer both spellings
  // canonicalize to the same fresh name and this FALSELY passes; under the
  // conservative structural-identifier rule it must fail.
  it("authored local-binding rename (v-for iteration variable renamed consistently)", async () => {
    const golden = goldenVdom();
    expect(golden.code).toMatch(/\(item\) => \{/); // the authored binding, as a parameter
    const mutated = golden.code.replace(/\bitem\b/g, "entry");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toMatch(/\(entry\) => \{/);
    expect(mutated).not.toMatch(/\bitem\b/);
    expect(mutated).toContain("items"); // proves the sibling authored name was untouched
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: public names (a component's public prop key renamed).
  it("public prop-name mutation (a component's public prop key renamed everywhere)", async () => {
    const propsEmit = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/props-emit.vue"), "utf8");
    const golden = compileVueFixture(propsEmit, "fixtures/vue/props-emit.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(golden.code).toContain("label: { type: String");
    const mutated = golden.code.replaceAll("label", "caption");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("caption: { type: String");
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Conservative identifier rule: a generated-LOOKING binding carries no
  // explicit provenance, so renaming it is ALSO structural — never silently
  // equated. (This inverts the pre-rule behavior, which alpha-renamed it
  // away and passed.)
  it("generated-looking identifier rename without provenance is structural (must FAIL)", async () => {
    const golden = goldenVdom();
    expect(golden.code).toContain("_sfc_main");
    const mutated = golden.code.replaceAll("_sfc_main", "_component_impl_renamed");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("_component_impl_renamed");
    expect(mutated).not.toContain("_sfc_main");
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: fold control flow.
  it("control-flow mutation (if/else branches swapped — same total text shape, different runtime path)", async () => {
    const a = "export default function f(cond) {\n  if (cond) { return 1; } else { return 2; }\n}";
    const b = "export default function f(cond) {\n  if (cond) { return 2; } else { return 1; }\n}";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: sort statements (two real import statements reordered).
  it("statement sort (the two import declarations of real official output swapped)", async () => {
    const golden = goldenVdom();
    const lines = golden.code.split("\n");
    const importLines = lines.filter((l) => l.startsWith("import "));
    expect(importLines.length).toBe(2);
    const [first, second] = importLines;
    const mutated = golden.code.replace(first, "\0").replace(second, first).replace("\0", second);
    assertMutationApplied(golden.code, mutated);
    expect(mutated.indexOf(second)).toBeLessThan(mutated.indexOf(first));
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
  });

  // Category: tagged-template raw spelling. A TAGGED template's tag
  // function receives the raw spellings too (`strings.raw`), so a raw-only
  // change with an identical cooked value is observable program input —
  // semantically real, never cosmetic. (The allowed-cosmetic companion
  // above proves the UNTAGGED case correctly stays free.)
  it("TAGGED template raw escape-spelling change (identical cooked value) is caught", async () => {
    const a = "const tag = (strings) => strings.raw[0];\nexport default tag`a\\u0041b`;";
    const b = "const tag = (strings) => strings.raw[0];\nexport default tag`aAb`;";
    assertMutationApplied(a, b);
    // Precondition: the mutation changed ONLY the raw spelling — cooked
    // values identical, raw spellings different — so nothing but the
    // tagged-template raw rule can catch it.
    const quasisOf = (code) => parseModule(code).body[1].declaration.quasi.quasis;
    expect(quasisOf(a).map((q) => q.value.cooked)).toEqual(quasisOf(b).map((q) => q.value.cooked));
    expect(quasisOf(a).map((q) => q.value.raw)).not.toEqual(quasisOf(b).map((q) => q.value.raw));
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Scope capture/shadowing attack (required discrimination list).
  it("scope capture/shadowing attack — an inner-scope reference redirected to an outer binding", async () => {
    const a = "let x = 1;\nfunction f() {\n  let x = 2;\n  return x;\n}\nexport default f;";
    const b = "let x = 1;\nfunction f() {\n  return x;\n}\nexport default f;";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("same-named-shadow programs with different bound values are not equated", async () => {
    const a = "let v = 1;\nfunction f() {\n  let v = 2;\n  return v;\n}\nexport default f;";
    const c = "let v = 1;\nfunction f() {\n  let v = 3;\n  return v;\n}\nexport default f;";
    assertMutationApplied(a, c);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: c, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail"); // literal 2 vs 3 — a genuine value difference
  });
});

// ---------------------------------------------------------------------------
// Import-specifier order. Named specifier order WITHIN ONE import declaration
// is cosmetic — two modules importing the same names from the same source in a
// different order are the same program (ESM bindings are hoisted, and the
// binding set is what the module sees). EVERY other import fact stays
// structural: membership, imported name, local alias, source module,
// default/namespace form, the top-level order of the declarations themselves,
// and the side-effect import sequence.
//
// This is deliberately NARROWER than the Rust structural comparator, which is
// not the authority this normalizer mirrors: `compare.rs`'s own
// `merge_imports`/`diff_imports` merges EVERY declaration sharing a source
// into one set before comparing, so it treats declaration GROUPING (and with
// it declaration order) as cosmetic, keeping only the side-effect sequence
// ordered. The two comparators agree on ONE point — named-specifier
// membership compares as a set — and that is the only distinction adopted
// here. Keeping declaration order and grouping structural is this
// normalizer's intentionally stricter reading, and it is what the negative
// controls below enforce.
//
// The negative half of this block is the over-broadening control: a fix that
// canonicalized "all import facts" as a set, or that sorted whole declarations,
// would pass the permutation test and FAIL these.
// ---------------------------------------------------------------------------

/**
 * ROTATES the named specifiers of `code`'s import declaration for `source` —
 * a pure permutation: the same specifier TEXTS, every one in a new slot,
 * every other byte of the module untouched.
 */
function rotateNamedSpecifiers(code, source) {
  const ast = parseModule(code, "specifier-rotation");
  const decl = ast.body.find((s) => s.type === "ImportDeclaration" && s.source.value === source);
  if (decl === undefined) throw new Error(`no import declaration from "${source}"`);
  const named = decl.specifiers.filter((s) => s.type === "ImportSpecifier");
  if (named.length < 2) throw new Error("rotation needs at least two named specifiers");
  const texts = named.map((s) => code.slice(s.start, s.end));
  const rotated = [...texts.slice(1), texts[0]];
  let out = "";
  let cursor = 0;
  named.forEach((specifier, i) => {
    out += code.slice(cursor, specifier.start) + rotated[i];
    cursor = specifier.end;
  });
  return out + code.slice(cursor);
}

/** The named specifier source texts of `code`'s import declaration for `source`. */
function namedSpecifierTexts(code, source) {
  const ast = parseModule(code, "specifier-read");
  const decl = ast.body.find((s) => s.type === "ImportDeclaration" && s.source.value === source);
  return decl.specifiers
    .filter((s) => s.type === "ImportSpecifier")
    .map((s) => code.slice(s.start, s.end));
}

/** Compares two synthetic modules with no link oracle (structure in isolation). */
async function compareSynthetic(a, b) {
  assertMutationApplied(a, b);
  return compareArtifacts({ code: a, diagnostics: [] }, { code: b, diagnostics: [] });
}

describe("normalizer — named import-specifier ORDER is cosmetic (must PASS)", () => {
  it("real official output: rotating every named specifier of the `vue` import is cosmetic", async () => {
    // The concrete blocker this rule corrects: official and candidate emit the
    // same helper import with the same names in a different insertion order.
    const slots = readFileSync(path.join(HARNESS_ROOT, "fixtures/vue/slots.vue"), "utf8");
    const golden = compileVueFixture(slots, "fixtures/vue/slots.vue", {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    const mutated = rotateNamedSpecifiers(golden.code, "vue");
    assertMutationApplied(golden.code, mutated);
    // Preconditions: the same specifier MULTISET, a genuinely different order,
    // and no other byte of the module touched.
    const before = namedSpecifierTexts(golden.code, "vue");
    const after = namedSpecifierTexts(mutated, "vue");
    expect(before.length).toBeGreaterThan(1);
    expect([...after].sort()).toEqual([...before].sort());
    expect(after).not.toEqual(before);
    expect(mutated.length).toBe(golden.code.length);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("pass");
    expect(report.structural.equal).toBe(true);
  });

  it("synthetic: a pure permutation of named specifiers canonicalizes identically", async () => {
    const report = await compareSynthetic(
      'import { alpha, beta, gamma } from "x";\nexport default alpha + beta + gamma;',
      'import { gamma, alpha, beta } from "x";\nexport default alpha + beta + gamma;',
    );
    expect(report.verdict).toBe("pass");
    expect(report.structural.equal).toBe(true);
  });

  it("synthetic: aliased named specifiers permute freely (alias pairing preserved)", async () => {
    const report = await compareSynthetic(
      'import { a as _a, b as _b } from "x";\nexport default _a + _b;',
      'import { b as _b, a as _a } from "x";\nexport default _a + _b;',
    );
    expect(report.verdict).toBe("pass");
  });

  it("synthetic: a DEFAULT specifier keeps its leading slot while the named tail permutes", async () => {
    const report = await compareSynthetic(
      'import D, { a, b } from "x";\nexport default D + a + b;',
      'import D, { b, a } from "x";\nexport default D + a + b;',
    );
    expect(report.verdict).toBe("pass");
  });
});

describe("normalizer — every OTHER import fact stays structural (must be CAUGHT)", () => {
  it("adding a named specifier is caught", async () => {
    const report = await compareSynthetic(
      'import { a, b } from "x";\nexport default a + b;',
      'import { a, b, c } from "x";\nexport default a + b;',
    );
    expect(report.candidateParse.ok).toBe(true); // structure catches it, not the parser
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("removing a named specifier is caught", async () => {
    const report = await compareSynthetic(
      'import { a, b } from "x";\nexport default a;',
      'import { a } from "x";\nexport default a;',
    );
    expect(report.candidateParse.ok).toBe(true);
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("renaming a named specifier's LOCAL alias is caught (same imported name)", async () => {
    const report = await compareSynthetic(
      'import { a as _a, b as _b } from "x";\nexport default _b;',
      'import { a as _renamed, b as _b } from "x";\nexport default _b;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("renaming a named specifier's IMPORTED name is caught (same local alias)", async () => {
    // The local binding set is IDENTICAL on both sides — only which export of
    // the module it is bound to changed, so nothing but the imported-name
    // comparison can catch it.
    const report = await compareSynthetic(
      'import { a as _x, b as _y } from "x";\nexport default _x + _y;',
      'import { c as _x, b as _y } from "x";\nexport default _x + _y;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("changing the import SOURCE module is caught", async () => {
    const report = await compareSynthetic(
      'import { a, b } from "x";\nexport default a + b;',
      'import { a, b } from "y";\nexport default a + b;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("adding a DEFAULT specifier is caught", async () => {
    const report = await compareSynthetic(
      'import { a } from "x";\nexport default a;',
      'import D, { a } from "x";\nexport default a;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("default → NAMESPACE form change is caught (same local name)", async () => {
    // Same local binding spelling on both sides: only the specifier FORM
    // changed, so form must be structural for this to fail.
    const report = await compareSynthetic(
      'import D from "x";\nexport default D;',
      'import * as D from "x";\nexport default D;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("two import DECLARATIONS reordered is caught (module-item order stays structural)", async () => {
    const report = await compareSynthetic(
      'import { a } from "x";\nimport { b } from "y";\nexport default a + b;',
      'import { b } from "y";\nimport { a } from "x";\nexport default a + b;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("two SIDE-EFFECT imports reordered is caught (side-effect sequence stays ordered)", async () => {
    const report = await compareSynthetic(
      'import "x";\nimport "y";\nexport default 1;',
      'import "y";\nimport "x";\nexport default 1;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("regrouping the same named specifiers across two declarations is caught", async () => {
    // Declaration GROUPING is not merged by this normalizer: the same binding
    // set split across two declarations from one source is a different module
    // shape. The over-broadening control against a set-merging fix.
    const report = await compareSynthetic(
      'import { a, b } from "x";\nexport default a + b;',
      'import { a } from "x";\nimport { b } from "x";\nexport default a + b;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("import ATTRIBUTES are caught (same specifiers, same source)", async () => {
    const report = await compareSynthetic(
      'import { a } from "x" with { type: "json" };\nexport default a;',
      'import { a } from "x";\nexport default a;',
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });
});

describe("normalizer — semantic comments (tool-consumed comments are structure)", () => {
  const PURE_GOLDEN =
    "const f = () => 1;\nconst g = () => 2;\nconst a = /*#__PURE__*/ f();\nconst b = g();\nexport default a + b;";

  it("deleting a /*#__PURE__*/ annotation is caught", async () => {
    const mutated = PURE_GOLDEN.replace("/*#__PURE__*/ ", "");
    assertMutationApplied(PURE_GOLDEN, mutated);
    expect(mutated).not.toContain("__PURE__");
    const report = await compareArtifacts(
      { code: PURE_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("mutating a /*#__PURE__*/ annotation's content is caught", async () => {
    const mutated = PURE_GOLDEN.replace("#__PURE__", "#__NO_SIDE_EFFECTS__");
    assertMutationApplied(PURE_GOLDEN, mutated);
    const report = await compareArtifacts(
      { code: PURE_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("relocating a /*#__PURE__*/ annotation to a different expression is caught", async () => {
    const mutated =
      "const f = () => 1;\nconst g = () => 2;\nconst a = f();\nconst b = /*#__PURE__*/ g();\nexport default a + b;";
    assertMutationApplied(PURE_GOLDEN, mutated);
    // Same comment text, same count — ONLY the attachment moved.
    expect((mutated.match(/__PURE__/g) ?? []).length).toBe(
      (PURE_GOLDEN.match(/__PURE__/g) ?? []).length,
    );
    const report = await compareArtifacts(
      { code: PURE_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("deleting a trailing sourceMappingURL directive is caught", async () => {
    const a = "export default 1;\n//# sourceMappingURL=out.js.map\n";
    const b = "export default 1;\n";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("mutating a sourceMappingURL directive's target is caught", async () => {
    const a = "export default 1;\n//# sourceMappingURL=out.js.map\n";
    const b = "export default 1;\n//# sourceMappingURL=evil.js.map\n";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("deleting a license/preserve comment is caught", async () => {
    const a = "/*! (c) Example Corp — preserved */\nexport default 1;";
    const b = "export default 1;";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("deleting a JSDoc block is caught", async () => {
    const a = "/** @param {number} n */\nexport function f(n) { return n; }";
    const b = "export function f(n) { return n; }";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  it("deleting a TS directive comment is caught", async () => {
    const a = "// @ts-expect-error deliberate\nexport default 1;";
    const b = "export default 1;";
    assertMutationApplied(a, b);
    const report = await compareArtifacts(
      { code: a, diagnostics: [] },
      { code: b, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });

  // Directive-shaped semantic comments — Istanbul / ESLint / Prettier.
  // Same discrimination discipline as the PURE-annotation class above: one
  // deletion and one relocation test per family, relocation keeping the
  // comment text and count identical so ONLY the attachment moves.

  const ISTANBUL_GOLDEN =
    "function f(c) {\n  /* istanbul ignore next */\n  if (c) { return 1; }\n  if (!c) { return 2; }\n}\nexport default f;";

  it("deleting an istanbul ignore directive is caught", async () => {
    const mutated = ISTANBUL_GOLDEN.replace("  /* istanbul ignore next */\n", "");
    assertMutationApplied(ISTANBUL_GOLDEN, mutated);
    expect(mutated).not.toContain("istanbul");
    const report = await compareArtifacts(
      { code: ISTANBUL_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("relocating an istanbul ignore directive to a different statement is caught", async () => {
    const mutated =
      "function f(c) {\n  if (c) { return 1; }\n  /* istanbul ignore next */\n  if (!c) { return 2; }\n}\nexport default f;";
    assertMutationApplied(ISTANBUL_GOLDEN, mutated);
    // Same comment text, same count — ONLY the attachment moved.
    expect((mutated.match(/istanbul ignore next/g) ?? []).length).toBe(
      (ISTANBUL_GOLDEN.match(/istanbul ignore next/g) ?? []).length,
    );
    const report = await compareArtifacts(
      { code: ISTANBUL_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  const ESLINT_GOLDEN =
    "// eslint-disable-next-line no-console\nconsole.log(1);\nconsole.log(2);\nexport default 1;";

  it("deleting an eslint-disable directive is caught", async () => {
    const mutated = ESLINT_GOLDEN.replace("// eslint-disable-next-line no-console\n", "");
    assertMutationApplied(ESLINT_GOLDEN, mutated);
    expect(mutated).not.toContain("eslint-disable");
    const report = await compareArtifacts(
      { code: ESLINT_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("relocating an eslint-disable directive to a different statement is caught", async () => {
    const mutated =
      "console.log(1);\n// eslint-disable-next-line no-console\nconsole.log(2);\nexport default 1;";
    assertMutationApplied(ESLINT_GOLDEN, mutated);
    expect((mutated.match(/eslint-disable-next-line/g) ?? []).length).toBe(
      (ESLINT_GOLDEN.match(/eslint-disable-next-line/g) ?? []).length,
    );
    const report = await compareArtifacts(
      { code: ESLINT_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("opening a blank line between an eslint-disable-next-line directive and its target line is caught", async () => {
    // The directive suppresses literally the NEXT LINE, so a blank line
    // between the directive and console.log(1) changes what ESLint
    // suppresses — while the comment text, the comment count, AND the
    // nearest-node attachment (still console.log(1)) are all unchanged.
    // Only the line-adjacency relationship moved.
    const mutated =
      "// eslint-disable-next-line no-console\n\nconsole.log(1);\nconsole.log(2);\nexport default 1;";
    assertMutationApplied(ESLINT_GOLDEN, mutated);
    expect((mutated.match(/eslint-disable-next-line/g) ?? []).length).toBe(
      (ESLINT_GOLDEN.match(/eslint-disable-next-line/g) ?? []).length,
    );
    // Precondition: the statement order is untouched — the mutation is the
    // blank line alone.
    expect(mutated.replace("\n\n", "\n")).toBe(ESLINT_GOLDEN);
    const report = await compareArtifacts(
      { code: ESLINT_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  const PRETTIER_GOLDEN =
    "// prettier-ignore\nconst m1 = [1, 2, 3];\nconst m2 = [4, 5, 6];\nexport default m1.concat(m2);";

  it("deleting a prettier-ignore directive is caught", async () => {
    const mutated = PRETTIER_GOLDEN.replace("// prettier-ignore\n", "");
    assertMutationApplied(PRETTIER_GOLDEN, mutated);
    expect(mutated).not.toContain("prettier-ignore");
    const report = await compareArtifacts(
      { code: PRETTIER_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("relocating a prettier-ignore directive to a different statement is caught", async () => {
    const mutated =
      "const m1 = [1, 2, 3];\n// prettier-ignore\nconst m2 = [4, 5, 6];\nexport default m1.concat(m2);";
    assertMutationApplied(PRETTIER_GOLDEN, mutated);
    expect((mutated.match(/prettier-ignore/g) ?? []).length).toBe(
      (PRETTIER_GOLDEN.match(/prettier-ignore/g) ?? []).length,
    );
    const report = await compareArtifacts(
      { code: PRETTIER_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("mutating a directive comment's payload (the disabled rule name) is caught", async () => {
    const mutated = ESLINT_GOLDEN.replace("no-console", "no-undef");
    assertMutationApplied(ESLINT_GOLDEN, mutated);
    const report = await compareArtifacts(
      { code: ESLINT_GOLDEN, diagnostics: [] },
      { code: mutated, diagnostics: [] },
    );
    expect(report.verdict).toBe("fail");
  });
});
