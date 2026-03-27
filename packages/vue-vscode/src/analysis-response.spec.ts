/**
 * @ai-generated - Tests that analysis response handling is resilient to
 * serde skip_serializing_if causing empty Vec fields to be undefined.
 *
 * These tests don't import VS Code APIs — they test the pure data handling
 * logic that the tree providers depend on.
 */
import { describe, it, expect } from "vitest";
import type {
  FileAnalysisSnapshot,
  TemplateComponentUsage,
  TemplatePropUsage,
  TemplateAnalysisSnapshot,
  StyleBlockAnalysis,
  AnalyzedImport,
  AnalyzedMacro,
} from "@verter/language-shared";

/**
 * Simulate a JSON response from the Rust LSP where `skip_serializing_if = "Vec::is_empty"`
 * causes empty arrays to be absent from the payload. JSON.parse produces `undefined`
 * for these fields on the TypeScript side.
 */
function simulateLspResponse(): FileAnalysisSnapshot {
  // This is what the Rust LSP actually sends — empty Vec fields are OMITTED
  const raw = JSON.parse(
    JSON.stringify({
      imports: [
        {
          source: "vue",
          isTypeOnly: false,
          bindings: [{ name: "ref", isTypeOnly: false, vueApi: "Ref", spanStart: 9, spanEnd: 12 }],
          spanStart: 0,
          spanEnd: 30,
        },
      ],
      bindings: [
        {
          name: "count",
          kind: "Const",
          isReactive: true,
          reactivityKind: "Ref",
          spanStart: 37,
          spanEnd: 42,
        },
      ],
      macros: [
        {
          kind: "DefineProps",
          isTypeBased: true,
          // typeReferences: [],  ← OMITTED by skip_serializing_if
          spanStart: 50,
          spanEnd: 80,
        },
      ],
      macroTypeDeps: [],
      scriptFlags: 0,
      styles: [
        {
          lang: "css",
          scoped: true,
          isModule: false,
          // vBinds: [],  ← OMITTED by skip_serializing_if
          // specialPseudos: [],
          css: {
            selectors: [{ text: ".title", specificity: [0, 1, 0] }],
            classes: [{ name: "title" }],
            ids: [],
            customProperties: [],
            atRules: [],
            ruleCount: 1,
          },
          flags: 0,
        },
      ],
      template: {
        components: [
          {
            name: "MyComp",
            // importSource: undefined, ← OMITTED by skip_serializing_if
            isDynamic: false,
            props: [
              {
                name: "title",
                isBound: false,
                constness: "Const",
                // referencedBindings: [],  ← OMITTED by skip_serializing_if
                fromSpread: false,
              },
            ],
            hasSpread: false,
            // slotsUsed: [],  ← OMITTED by skip_serializing_if
            spanStart: 100,
            spanEnd: 120,
          },
        ],
        bindingOccurrences: [],
        // unresolvedBindings: [],  ← OMITTED
        // definedSlots: [],  ← OMITTED
        // templateRefs: [],  ← OMITTED
        // eventHandlers: [],  ← OMITTED
        maxNestingDepth: 2,
        // vIfVForConflicts: [],  ← OMITTED
      },
    }),
  );
  return raw as FileAnalysisSnapshot;
}

describe("Analysis response resilience to skip_serializing_if", () => {
  const analysis = simulateLspResponse();

  it("handles missing slotsUsed on template components", () => {
    const comp = analysis.template!.components[0]!;
    // This is the exact pattern used in AnalysisTreeProvider and ComponentTreeProvider
    const slotsStr = (comp.slotsUsed ?? []).join(", ") || "none";
    expect(slotsStr).toBe("none");
  });

  it("handles missing referencedBindings on props", () => {
    const prop = analysis.template!.components[0]!.props[0]!;
    // Pattern from ComponentTreeProvider tooltip
    const hasBindings = prop.referencedBindings?.length
      ? `Bindings: ${prop.referencedBindings.join(", ")}`
      : "";
    expect(hasBindings).toBe("");
  });

  it("handles missing typeReferences on macros", () => {
    const macro = analysis.macros[0]! as AnalyzedMacro;
    const typesStr = macro.typeReferences?.length
      ? `Types: ${macro.typeReferences.join(", ")}`
      : "";
    expect(typesStr).toBe("");
  });

  it("handles missing vBinds on style blocks", () => {
    const style = analysis.styles[0]! as StyleBlockAnalysis;
    const vbinds: string[] = [];
    for (const vb of style.vBinds ?? []) {
      vbinds.push(`v-bind(${vb.expression})`);
    }
    expect(vbinds).toEqual([]);
  });

  it("handles missing importSource on components", () => {
    const comp = analysis.template!.components[0]!;
    const desc = comp.importSource ? `from "${comp.importSource}"` : "(global)";
    expect(desc).toBe("(global)");
  });

  it("handles missing unresolvedBindings on template", () => {
    const tmpl = analysis.template!;
    const unresolved = tmpl.unresolvedBindings ?? [];
    expect(unresolved).toEqual([]);
  });

  it("handles missing definedSlots on template", () => {
    const tmpl = analysis.template!;
    const slots = tmpl.definedSlots ?? [];
    expect(slots).toEqual([]);
  });

  it("handles missing eventHandlers on template", () => {
    const tmpl = analysis.template!;
    const handlers = tmpl.eventHandlers ?? [];
    expect(handlers).toEqual([]);
  });

  it("handles missing vIfVForConflicts on template", () => {
    const tmpl = analysis.template!;
    const conflicts = tmpl.vIfVForConflicts ?? [];
    expect(conflicts).toEqual([]);
  });

  it("handles null template", () => {
    const noTemplate: FileAnalysisSnapshot = {
      ...analysis,
      template: null,
    };
    // Pattern from AnalysisTreeProvider
    const hasComponents = noTemplate.template?.components?.length;
    expect(hasComponents).toBeFalsy();
  });

  it("handles completely empty analysis response", () => {
    // Worst case: all fields missing (shouldn't happen, but defensive)
    const empty = {} as FileAnalysisSnapshot;
    expect(empty.imports?.length ?? 0).toBe(0);
    expect(empty.bindings?.length ?? 0).toBe(0);
    expect(empty.macros?.length ?? 0).toBe(0);
    expect(empty.styles?.length ?? 0).toBe(0);
    expect(empty.template?.components?.length ?? 0).toBe(0);
  });

  it("iterates component props safely when props array is present", () => {
    const comp = analysis.template!.components[0]!;
    const propNames: string[] = [];
    for (const prop of comp.props ?? []) {
      propNames.push(prop.name);
    }
    expect(propNames).toEqual(["title"]);
  });

  it("builds ComponentTreeProvider tooltip without crashing when props is undefined", () => {
    // Simulate a component with props omitted (e.g., serde skip or malformed response)
    const comp = {
      name: "NoPropsComp",
      isDynamic: false,
      hasSpread: false,
    } as TemplateComponentUsage;
    // This is the exact pattern from ComponentTreeProvider.ts line 205
    // Before the fix: `comp.props.length` would crash with "Cannot read properties of undefined"
    // After the fix: `(comp.props ?? []).length` returns 0
    const tooltip = [
      `Component: ${comp.name}`,
      comp.importSource ? `Import: ${comp.importSource}` : "Global component",
      comp.isDynamic ? "Dynamic component" : "",
      comp.hasSpread ? "Has v-bind spread" : "",
      `Props: ${(comp.props ?? []).length}`,
      `Slots: ${(comp.slotsUsed ?? []).join(", ") || "none"}`,
    ]
      .filter(Boolean)
      .join("\n");

    expect(tooltip).toContain("Props: 0");
    expect(tooltip).toContain("Slots: none");
    // Negative: should NOT crash or produce "Props: undefined"
    expect(tooltip).not.toContain("undefined");
  });

  it("builds tooltip string without crashing on missing fields", () => {
    const comp = analysis.template!.components[0]!;
    // This is the exact tooltip pattern from AnalysisTreeProvider
    const tooltip = [
      `Component: ${comp.name}`,
      comp.importSource ? `Import: ${comp.importSource}` : "Global",
      `Dynamic: ${comp.isDynamic}`,
      `Props: ${(comp.props ?? []).map((p) => p.name).join(", ") || "none"}`,
      `Slots: ${(comp.slotsUsed ?? []).join(", ") || "none"}`,
    ]
      .filter(Boolean)
      .join("\n");

    expect(tooltip).toContain("Component: MyComp");
    expect(tooltip).toContain("Global");
    expect(tooltip).toContain("Props: title");
    expect(tooltip).toContain("Slots: none");
  });
});
