// Self-test: normalizer positive/negative discrimination with PROVEN-applied
// mutations (BF2 required exit + CLAUDE.md "Verification Must Prove
// Execution": every mutation below is asserted to have actually changed the
// text — and to be genuinely NEW relative to the pre-mutation source — before
// the pass/fail result is trusted).
//
// The forbidden-mutation categories are a closed harness contract, with one
// labelled test per
// category, each planting a mutation genuinely REPRESENTATIVE of that
// category — a literal substitution never stands in for a prop/attribute
// test, swapped constants never stand in for an effect-order test, and a
// property-key mutation never stands in for an authored-local-name test.
//
// Identifier rule under test (see `matchLocalBindings` in src/compare.mjs):
// a module-local binding renamed consistently — its declaration and every
// use, resolved by scope on each side — is cosmetic, whether authored or
// compiler-generated. Names that are observable or not local stay
// structural: exported names, imported names and module paths, property
// keys, function/class names and bindings that name an anonymous function
// (`.name`), names reachable by direct `eval`, and free globals. A rename
// that re-binds a use to a different declaration (shadow capture) is a
// structural difference.

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

/** Every identifier spelling in `code`, to prove a rename target is fresh. */
function identifierNames(code) {
  const names = new Set();
  const visit = (value) => {
    if (Array.isArray(value)) {
      for (const item of value) visit(item);
      return;
    }
    if (value === null || typeof value !== "object" || typeof value.type !== "string") return;
    if (value.type === "Identifier") names.add(value.name);
    for (const [key, child] of Object.entries(value)) if (key !== "loc") visit(child);
  };
  visit(parseModule(code, "identifier-names"));
  return names;
}

/** `parent[key]` names a property or an import/export name, not a binding. */
function isNonBindingName(parent, key) {
  return (
    (parent.type === "MemberExpression" && key === "property" && !parent.computed) ||
    (["Property", "MethodDefinition", "PropertyDefinition"].includes(parent.type) &&
      key === "key" &&
      !parent.computed) ||
    (parent.type === "ImportSpecifier" && key === "imported") ||
    (parent.type === "ExportSpecifier" && key === "exported")
  );
}

/**
 * Renames the bindings spelled like the keys of `renames` — declarations and
 * uses — by splicing at parser-reported identifier positions. Property and
 * import/export names are kept: `{ a }` becomes `{ a: renamed }`,
 * `import { a }` becomes `import { a as renamed }`, `export { a }` becomes
 * `export { renamed as a }`. Every target must be absent from the module (so
 * the rename cannot capture) and every source name must be renamed.
 */
function renameLocals(code, renames) {
  const existing = identifierNames(code);
  for (const target of Object.values(renames)) expect(existing.has(target), target).toBe(false);
  const edits = [];
  const renamed = new Set();
  const edit = (id, text) => {
    edits.push({ start: id.start, end: id.end, text });
    renamed.add(id.name);
  };
  const visit = (node, parent, key) => {
    if (Array.isArray(node)) {
      for (const item of node) visit(item, parent, key);
      return;
    }
    if (node === null || typeof node !== "object" || typeof node.type !== "string") return;
    if (node.type === "Identifier") {
      if (Object.hasOwn(renames, node.name) && !isNonBindingName(parent, key))
        edit(node, renames[node.name]);
      return;
    }
    const local = node.type === "Property" ? node.value : node.local;
    if (local?.type === "Identifier" && Object.hasOwn(renames, local.name)) {
      const target = renames[local.name];
      if (node.type === "Property" && node.shorthand)
        return edit(local, `${local.name}: ${target}`);
      if (node.type === "ImportSpecifier" && node.imported === local)
        return edit(local, `${local.name} as ${target}`);
      if (node.type === "ExportSpecifier" && node.exported === local)
        return edit(local, `${target} as ${local.name}`);
    }
    for (const [childKey, child] of Object.entries(node))
      if (childKey !== "loc") visit(child, node, childKey);
  };
  visit(parseModule(code, "rename-locals"), null, null);
  for (const source of Object.keys(renames)) expect(renamed.has(source), source).toBe(true);
  let out = code;
  for (const { start, end, text } of edits.sort((a, b) => b.start - a.start))
    out = out.slice(0, start) + text + out.slice(end);
  assertMutationApplied(code, out);
  return out;
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

  it("real Vue output: consistent local renames, reflow and a prose comment are cosmetic", async () => {
    const golden = goldenVdom();
    const renamed = renameLocals(golden.code, {
      ref: "makeRef", // import alias; the imported name stays `ref`
      count: "counter", // authored setup binding; its setup-return key stays `count`
      items: "entries",
      item: "entry", // authored v-for iteration variable
      _sfc_main: "component", // compiler-generated bindings
      __returned__: "setupState",
      __expose: "exposeFn",
      _hoisted_1: "rootProps",
      _toDisplayString: "display",
      $setup: "setupBindings",
    });
    const mutated = `// prose note, consumed by no tool\n${renamed.replace(/\n/g, "\n\n")}`;
    // Public names survive: setup-return keys, template reads, the render name.
    expect(mutated).toContain("count: counter, items: entries, ref: makeRef");
    expect(mutated).toContain("setupBindings.count");
    expect(mutated).toContain("function render(");
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("pass");
    expect(report.fidelity).toEqual({ status: "equivalent", firstDivergence: null });
    expect(report.structural.localBindingMatching).toEqual({ golden: true, candidate: true });
  });

  it("real Svelte output: consistent local renames (namespace import included), reindent and a prose comment are cosmetic", async () => {
    const golden = goldenSvelteClient();
    const renamed = renameLocals(golden.code, {
      $: "internal", // namespace import alias; the module path stays
      $$anchor: "anchor",
      root_3: "template_root",
      div: "host",
      node: "branch_anchor",
      ul: "list",
      li: "row",
      text: "label",
      item: "value",
      count: "total",
      items: "rows",
    });
    const mutated = `/* prose note, consumed by no tool */\n${renamed.replace(/\t/g, "  ")}`;
    expect(mutated).toContain("import * as internal from 'svelte/internal/client'");
    expect(mutated).toContain("export default function Basic_runes(anchor)");
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: SVELTE_BASE },
    );
    expect(report.verdict).toBe("pass");
    expect(report.fidelity).toEqual({ status: "equivalent", firstDivergence: null });
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

  // Category: bindings/capture — a rename on real official output that
  // re-binds a use to another declaration (`var item` shares the callback
  // parameter's binding, so `set_text` now reads the child node twice).
  it("real Svelte output: a rename that captures another binding (shadow capture) is caught", async () => {
    const golden = goldenSvelteClient();
    const mutated = golden.code
      .replace("var text = $.child(li, true);", "var item = $.child(li, true);")
      .replace("$.set_text(text, item)", "$.set_text(item, item)");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).not.toMatch(/\btext\b/);
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: SVELTE_BASE },
    );
    expect(report.candidateParse.ok).toBe(true);
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: names observable through reflection (`Function.prototype.name`).
  it("real Vue output: renaming the render function changes its observable name", async () => {
    const golden = goldenVdom();
    const mutated = renameLocals(golden.code, { render: "renderFn" });
    expect(mutated).toContain("_sfc_main.render = renderFn"); // the options key stays
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: VUE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("real Svelte output: renaming a binding that names an arrow function changes its observable name", async () => {
    const golden = goldenSvelteClient();
    const mutated = renameLocals(golden.code, { consequent: "when_true" });
    expect(mutated).toContain("$$render(when_true)");
    const report = await compareArtifacts(
      golden,
      { ...golden, code: mutated },
      { linkBaseDir: SVELTE_BASE },
    );
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  // Category: public names — a setup-return key is the template's public
  // binding name (the local-rename twin above keeps the key and passes).
  it("real Vue output: renaming a setup-return key and its template reads is caught", async () => {
    const golden = goldenVdom();
    const mutated = golden.code.replace(/\bcount\b/g, "counter");
    assertMutationApplied(golden.code, mutated);
    expect(mutated).toContain("{ counter, items, ref }");
    expect(mutated).toContain("$setup.counter");
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

// Import-specifier order. Named specifier order WITHIN ONE import declaration
// is cosmetic — two modules importing the same names from the same source in a
// different order are the same program (ESM bindings are hoisted, and the
// binding set is what the module sees). EVERY other import fact stays
// structural: membership, imported name, source module, default/namespace
// form, the top-level order of the declarations themselves, and the
// side-effect import sequence. A local alias is a local binding, matched by
// scope like any other (see the local-binding tables at the end).
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

// Local-binding matching over the scoping forms the resolver models. Each
// EQUIVALENT pair renames only local bindings, capture-free; each DISTINCT
// pair differs in which declaration a use reaches, or renames a name that is
// observable or not local.

const EQUIVALENT_LOCAL_RENAMES = [
  [
    "a used import alias",
    'import { a as _a } from "x";\nexport default _a;',
    'import { a as helper } from "x";\nexport default helper;',
  ],
  [
    "an unused import alias",
    'import { a as _a, b as _b } from "x";\nexport default _b;',
    'import { a as _renamed, b as _b } from "x";\nexport default _b;',
  ],
  [
    "a namespace import",
    'import * as $ from "x";\nexport default $.a;',
    'import * as ns from "x";\nexport default ns.a;',
  ],
  [
    "a shorthand property expanded around a renamed local",
    "const a = 1;\nexport default { a };",
    "const _a = 1;\nexport default { a: _a };",
  ],
  [
    "an export specifier's local (exported name kept)",
    "const a = 1;\nexport { a };",
    "const _a = 1;\nexport { _a as a };",
  ],
  [
    "a hoisted var, a block let and a parameter",
    "function f(c) {\n  { var v = 1; let w = 2; v += w; }\n  return v + c;\n}\nexport default f;",
    "function f(k) {\n  { var x = 1; let y = 2; x += y; }\n  return x + k;\n}\nexport default f;",
  ],
  [
    "shadowing parameters",
    "export default (a) => [a, (a) => a];",
    "export default (b) => [b, (c) => c];",
  ],
  [
    "a destructured catch parameter",
    "export default () => {\n  try { f(); } catch ({ message: m }) { g(m); }\n};",
    "export default () => {\n  try { f(); } catch ({ message: text }) { g(text); }\n};",
  ],
  [
    "method parameters and static-block locals (exported class name kept)",
    "export class K { m(a) { return a; } static { let s = 1; K.s = s; } }",
    "export class K { m(b) { return b; } static { let t = 1; K.s = t; } }",
  ],
];

const DISTINCT_BINDINGS = [
  [
    "shadow capture (a use re-bound to an inner declaration)",
    "let a = 1;\nfunction f() {\n  let b = 2;\n  return a + b;\n}\nexport default f;",
    "let a = 1;\nfunction f() {\n  let a = 2;\n  return a + a;\n}\nexport default f;",
  ],
  [
    "var hoisting captures a use of the outer binding",
    "let a = 1;\nfunction f() {\n  { var b = 2; }\n  return a;\n}\nexport default f;",
    "let a = 1;\nfunction f() {\n  { var a = 2; }\n  return a;\n}\nexport default f;",
  ],
  [
    "a var re-declaring a parameter keeps the parameter's value",
    "export default function f(p = 1) {\n  var p;\n  return p;\n}",
    "export default function f(q = 1) {\n  var r;\n  return r;\n}",
  ],
  [
    "a parameter default does not see body declarations",
    "let x = 0;\nexport function f(g = () => x) {\n  let x = 1;\n  return g();\n}",
    "let x = 0;\nexport function f(g = () => z) {\n  let z = 1;\n  return g();\n}",
  ],
  [
    "a local renamed onto a global it then shadows",
    "const t = 1;\nexport default () => [t, Math];",
    "const Math = 1;\nexport default () => [Math, Math];",
  ],
  [
    "import aliases swapped between imported names",
    'import { a as x, b as y } from "m";\nexport default x;',
    'import { a as y, b as x } from "m";\nexport default x;',
  ],
  ["an exported declaration renamed", "export const answer = 1;", "export const result = 1;"],
  [
    "an exported name changed through a specifier",
    "const a = 1;\nexport { a as b };",
    "const a = 1;\nexport { a as c };",
  ],
  [
    "a function declaration renamed (Function.prototype.name)",
    "function helper() {}\nexport default helper;",
    "function util() {}\nexport default util;",
  ],
  [
    "a binding that names an anonymous function renamed",
    "const helper = () => 1;\nexport default helper;",
    "const util = () => 1;\nexport default util;",
  ],
  [
    "a binding named through assignment renamed",
    "let helper;\nhelper = function () {};\nexport default helper;",
    "let util;\nutil = function () {};\nexport default util;",
  ],
  ["two different globals", "export default () => console;", "export default () => window;"],
];

describe("local-binding matching — capture-free local renames are cosmetic (must PASS)", () => {
  it.each(EQUIVALENT_LOCAL_RENAMES)("%s", async (_label, a, b) => {
    const report = await compareSynthetic(a, b);
    expect(report.verdict).toBe("pass");
    expect(report.structural.equal).toBe(true);
  });
});

describe("local-binding matching — capture, observable and non-local names stay structural (must be CAUGHT)", () => {
  it.each(DISTINCT_BINDINGS)("%s", async (_label, a, b) => {
    const report = await compareSynthetic(a, b);
    expect(report.candidateParse.ok).toBe(true);
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });

  it("a direct eval makes every name in the module observable", async () => {
    const control = await compareSynthetic(
      "const a = 1;\nexport default (s) => s;",
      "const b = 1;\nexport default (s) => s;",
    );
    expect(control.verdict).toBe("pass");
    const report = await compareSynthetic(
      "const a = 1;\nexport default (s) => eval(s);",
      "const b = 1;\nexport default (s) => eval(s);",
    );
    expect(report.structural.localBindingMatching).toEqual({ golden: false, candidate: false });
    expect(report.verdict).toBe("fail");
    expect(report.structural.equal).toBe(false);
  });
});
