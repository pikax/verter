/**
 * Guards for the in-context `ts.createLanguageService` wiring over Verter
 * carriers. Pinned to COMMITTED WASM-produced carrier
 * fixtures (`__fixtures__/wasm-carriers.json`) — no live WASM host load.
 */
import { describe, it, expect } from "vitest";
import ts from "typescript";
import { carrierRootMembership, toImportSurfaceFileName } from "@verter/language-shared";
import { createFixtureLs, diagRows, fixtures, surfacesOf, VROOT } from "./__fixtures__/wasmLsKit";

const MAIN_TS = `import Comp from "./Comp.vue";
type P = NonNullable<ConstructorParameters<typeof Comp>[0]>;
const bad: P["count"] = "not-a-number";
const good: P["count"] = 1;
export { bad, good };
`;

describe("wasm_in_context_ls_carrier_membership (#1)", () => {
  it("a plain .ts `import Comp from './Comp.vue'` puts the API carrier in the program and misuse yields a real prop-type TS2322, not any", () => {
    const { ls, path } = createFixtureLs(ts, {
      user: { "main.ts": MAIN_TS },
      carriers: { "Comp.vue": surfacesOf(fixtures.compVue) },
    });

    const program = ls.languageService.getProgram();
    expect(program).toBeDefined();
    // The API carrier — the SAME surface the tsserver plugin redirects to — is
    // the program member the import reaches.
    expect(program!.getSourceFile(path("Comp.vue.verter.ts"))).toBeDefined();
    // Never the extension-LAST legacy spelling.
    expect(program!.getSourceFile(path("Comp.vue.d.ts"))).toBeUndefined();

    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    const assignability = diags.filter((d) => d.code === 2322);
    // Exactly the `bad` line errs — a real `{ count: number }` prop type
    // flowed through the API carrier. If the import resolved to nothing (or to
    // `any`), there would be no 2322 at all; if BOTH lines erred, the prop type
    // would be corrupt.
    expect(assignability).toHaveLength(1);
    expect(assignability[0].message).toMatch(/not assignable to type 'number'/);
    // No unresolved-module fallback: the import itself resolves.
    expect(diags.some((d) => d.code === 2307)).toBe(false);
  });
});

describe("wasm_import_surface_only_resolution (#5)", () => {
  it("bare './Comp.vue' resolves to Comp.vue.verter.ts — never Comp.d.vue.ts, never Comp.vue.d.ts", () => {
    const { ls, path } = createFixtureLs(ts, {
      user: { "main.ts": MAIN_TS },
      carriers: { "Comp.vue": surfacesOf(fixtures.compVue) },
    });
    const program = ls.languageService.getProgram()!;
    expect(program.getSourceFile(path("Comp.vue.verter.ts"))).toBeDefined();
    expect(program.getSourceFile(path("Comp.vue.d.ts"))).toBeUndefined();
    // The import's type identity comes from the API surface: it carries the
    // `__OmitNew` wrapper the declaration carrier does not, so resolution did
    // not fall through to `.d.vue.ts`.
    const apiText = program.getSourceFile(path("Comp.vue.verter.ts"))!.text;
    expect(apiText).toBe(fixtures.compVue.api!.code);
    expect(apiText).toContain("__OmitNew");
    expect(fixtures.compVue.decl!.code).not.toContain("__OmitNew");
  });

  it("fail-closed: with NO published API carrier the bare import does NOT fall through to .tsx/.d.vue.ts", () => {
    const { ls, path } = createFixtureLs(ts, {
      user: { "main.ts": MAIN_TS },
      carriers: {
        // IDE + declaration published, API NOT — the redirect must fail closed
        // rather than serve another surface for the bare import.
        "Comp.vue": surfacesOf(fixtures.compVue, { ide: true, api: false, decl: true }),
      },
    });
    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    expect(diags.some((d) => d.code === 2307)).toBe(true);
    expect(ls.languageService.getProgram()!.getSourceFile(path("Comp.d.vue.ts"))).toBeUndefined();
  });
});

describe("wasm_real_wasm_produced_declaration_carrier (#4)", () => {
  it("the served declaration blob is the real getPublicApi(Declaration) output: declaration-only shape, distinct from the API carrier", () => {
    const { store, path } = createFixtureLs(ts, {
      user: {},
      carriers: { "Comp.vue": surfacesOf(fixtures.compVue) },
    });

    const declPath = path("Comp.d.vue.ts");
    const ready = store.readyFile(declPath);
    expect(ready).toBeDefined();
    const blob = store.readBlobSync(ready!.blob_rel, declPath);
    expect(blob).toBe(fixtures.compVue.decl!.code);

    // Declaration-ONLY structural shape: every top-level statement is
    // ambient/type-space; no runtime value code.
    const sf = ts.createSourceFile(declPath, blob!, ts.ScriptTarget.ESNext, true);
    expect(sf.isDeclarationFile).toBe(true);
    for (const statement of sf.statements) {
      const ambient =
        ts.isImportDeclaration(statement) ||
        ts.isTypeAliasDeclaration(statement) ||
        ts.isInterfaceDeclaration(statement) ||
        ts.isModuleDeclaration(statement) ||
        ts.isExportDeclaration(statement) ||
        (ts.isExportAssignment(statement) && !statement.isExportEquals) ||
        (ts.isVariableStatement(statement) &&
          (statement.modifiers ?? []).some((m) => m.kind === ts.SyntaxKind.DeclareKeyword));
      expect(ambient, `non-declaration statement: ${statement.getText(sf)}`).toBe(true);
    }

    // NOT the API carrier relabeled (the legacy "tscCode as .d.ts" shim):
    // the API surface carries runtime value code the decl surface must not.
    expect(blob).not.toBe(fixtures.compVue.api!.code);
    expect(fixtures.compVue.api!.code).toContain("defineComponent(");
    expect(blob).not.toContain("defineComponent(");
  });
});

describe("wasm_ide_carrier_is_self_diagnostic_root (#10)", () => {
  it("IDE carrier is a root; declaration is import-driven only; API is redirect-reached", () => {
    // NO user import — only the fixture carriers.
    const { ls, path } = createFixtureLs(ts, {
      user: {},
      carriers: { "Comp.vue": surfacesOf(fixtures.compVue) },
    });

    const roots = ls.host.getScriptFileNames();
    expect(roots).toContain(path("Comp.vue.tsx"));
    expect(roots).not.toContain(path("Comp.d.vue.ts"));
    expect(roots).not.toContain(path("Comp.vue.verter.ts"));

    const program = ls.languageService.getProgram()!;
    // Self-diagnostic root: present without any importer.
    expect(program.getSourceFile(path("Comp.vue.tsx"))).toBeDefined();
    // Redirect-reached: the IDE carrier's `export { default } from
    // './Comp.vue.verter.ts'` pulls the API surface in — NOT root membership.
    expect(program.getSourceFile(path("Comp.vue.verter.ts"))).toBeDefined();
    // Import-driven: nothing imports './Comp.vue', so the declaration
    // carrier stays OUT of the program.
    expect(program.getSourceFile(path("Comp.d.vue.ts"))).toBeUndefined();

    // The shared CORE policy agrees with the wiring.
    expect(carrierRootMembership(path("Comp.vue.tsx"))).toBe("selfDiagnosticRoot");
    expect(carrierRootMembership(path("Comp.d.vue.ts"))).toBe("importDriven");
    expect(carrierRootMembership(path("Comp.vue.verter.ts"))).toBe("redirectReached");
  });

  it("the API carrier ENTERS the program through a user import alone, with no IDE root", () => {
    // No IDE carrier published, so its `export { default } from
    // './Comp.vue.verter.js'` re-export cannot be what pulls the API surface
    // in — the user's own bare import is the only path.
    const { ls, path } = createFixtureLs(ts, {
      user: { "main.ts": MAIN_TS },
      carriers: { "Comp.vue": surfacesOf(fixtures.compVue, { ide: false, decl: true, api: true }) },
    });
    const program = ls.languageService.getProgram()!;
    expect(program.getSourceFile(path("Comp.vue.tsx"))).toBeUndefined();
    expect(program.getSourceFile(path("Comp.vue.verter.ts"))).toBeDefined();
    // The declaration carrier is published but nothing reaches it: no host in
    // this repo redirects there. It stays out of the program.
    expect(program.getSourceFile(path("Comp.d.vue.ts"))).toBeUndefined();
  });
});

describe("wasm_incremental_edit_invalidation (#6)", () => {
  const PROBE_TS = `import Comp from "./Comp.vue";
type P = NonNullable<ConstructorParameters<typeof Comp>[0]>;
const asNumber: number = ({} as P)["count"];
export { asNumber };
`;

  it("a source edit atomically updates all three carriers, bumps every version monotonically, and the program sees the new types; removal cleans up all carriers", () => {
    const { store, ls, path } = createFixtureLs(ts, {
      user: { "main.ts": PROBE_TS },
      carriers: { "Comp.vue": surfacesOf(fixtures.compVue) },
    });
    const sourcePath = path("Comp.vue");
    const idePath = path("Comp.vue.tsx");
    const declPath = path("Comp.d.vue.ts");
    const apiPath = path("Comp.vue.verter.ts");

    // v1: count is number — the probe assignment is clean.
    const v1 = {
      ide: store.readyFile(idePath)!.version,
      decl: store.readyFile(declPath)!.version,
      api: store.readyFile(apiPath)!.version,
    };
    let diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    expect(diags.filter((d) => d.code === 2322)).toHaveLength(0);

    // ONE atomic upsert flips the source to the edited fixture
    // (count: string, + label prop).
    store.upsertSource(sourcePath, surfacesOf(fixtures.compVueEdited));

    // Every carrier's version bumped monotonically…
    const v2 = {
      ide: store.readyFile(idePath)!.version,
      decl: store.readyFile(declPath)!.version,
      api: store.readyFile(apiPath)!.version,
    };
    expect(v2.ide).toBeGreaterThan(v1.ide);
    expect(v2.decl).toBeGreaterThan(v1.decl);
    expect(v2.api).toBeGreaterThan(v1.api);
    // …and the host reports the new versions.
    expect(ls.host.getScriptVersion(apiPath)).not.toBe(String(v1.api));

    // The Program sees the NEW types: count is now string → 2322.
    diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    expect(diags.filter((d) => d.code === 2322)).toHaveLength(1);
    // The new `label` prop is visible on the surface the import reaches.
    const apiText = ls.languageService.getProgram()!.getSourceFile(apiPath)!.text;
    expect(apiText).toContain("label");

    // Delete: removal cleans up ALL of the source's carriers + the root list.
    store.removeSource(sourcePath);
    expect(store.readyFile(idePath)).toBeUndefined();
    expect(store.readyFile(declPath)).toBeUndefined();
    expect(store.readyFile(apiPath)).toBeUndefined();
    expect(ls.host.getScriptFileNames()).not.toContain(idePath);
    expect(ls.host.fileExists(declPath)).toBe(false);
    const afterRemove = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    expect(afterRemove.some((d) => d.code === 2307)).toBe(true);
  });
});

describe("wasm_svelte_declaration_carrier_parity (#9)", () => {
  const SVELTE_MAIN = `import SComp from "./Comp.svelte";
type SP = Parameters<typeof SComp>[1];
const bad: SP["count"] = "not-a-number";
const good: SP["count"] = 1;
export { bad, good };
`;

  it("the SAME membership + import-surface resolution holds for a Svelte .svelte.verter.ts carrier", () => {
    const { ls, path } = createFixtureLs(ts, {
      user: { "svelteMain.ts": SVELTE_MAIN },
      carriers: { "Comp.svelte": surfacesOf(fixtures.compSvelte) },
    });

    const program = ls.languageService.getProgram()!;
    expect(program.getSourceFile(path("Comp.svelte.verter.ts"))).toBeDefined();
    expect(program.getSourceFile(path("Comp.svelte.d.ts"))).toBeUndefined();

    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("svelteMain.ts")));
    const assignability = diags.filter((d) => d.code === 2322);
    expect(assignability).toHaveLength(1);
    expect(assignability[0].message).toMatch(/not assignable to type 'number'/);
    expect(diags.some((d) => d.code === 2307)).toBe(false);
  });

  it("fail-closed parity: no published Svelte API carrier ⇒ unresolved import, no fallthrough", () => {
    // No IDE carrier in this program either: nothing imports `svelte`, so the
    // svelte package's ambient `declare module '*.svelte'` wildcard stays out
    // and the bare import surfaces the raw fail-closed resolution result.
    const { ls, path } = createFixtureLs(ts, {
      user: { "svelteMain.ts": SVELTE_MAIN },
      carriers: {
        "Comp.svelte": surfacesOf(fixtures.compSvelte, { ide: false, api: false, decl: true }),
      },
    });
    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("svelteMain.ts")));
    expect(diags.some((d) => d.code === 2307)).toBe(true);
    expect(
      ls.languageService.getProgram()!.getSourceFile(path("Comp.svelte.verter.ts")),
    ).toBeUndefined();
  });

  it("fail-closed under svelte's ambient wildcard: the bare import binds to `declare module '*.svelte'`, never a Verter companion", () => {
    // With the IDE carrier published, its `import ... from "svelte"` pulls the
    // svelte package's ambient `*.svelte` wildcard into the program, and
    // TypeScript itself binds the otherwise-unresolved bare import ambiently
    // (no 2307) — exactly what a real project importing `svelte` sees. The
    // fail-closed boundary: the import binds to the WILDCARD's generic
    // component shape, never to a Verter companion — the declaration carrier
    // stays out of the program and the carrier's specific `{ count: number }`
    // props do NOT flow (no specific-prop 2322 on either probe assignment).
    const { ls, path } = createFixtureLs(ts, {
      user: { "svelteMain.ts": SVELTE_MAIN },
      carriers: { "Comp.svelte": surfacesOf(fixtures.compSvelte, { api: false, decl: true }) },
    });
    const program = ls.languageService.getProgram()!;
    expect(program.getSourceFile(path("Comp.svelte.verter.ts"))).toBeUndefined();
    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("svelteMain.ts")));
    expect(diags.some((d) => d.code === 2307)).toBe(false);
    expect(diags.filter((d) => d.code === 2322)).toHaveLength(0);
  });

  it("the Svelte import surface exposes only the official native Component contract", () => {
    const code = fixtures.compSvelte.api!.code;
    expect(code).not.toMatch(/from\s+["']svelte["']/);
    expect(code).toContain('import("svelte").Component<');
    expect(code).not.toContain("__VerterPublicInstance");
    expect(code).not.toContain("new (...args: any[])");
    expect(code).not.toContain(": any");
  });
});

/**
 * The two carrier surfaces a component publishes are NOT interchangeable, so
 * "which one does an ordinary import reach" is a semantic question, not a
 * filename question. A runtime-object `defineExpose({ count })` is the sharpest
 * case in the tree: the API surface emits `count: typeof count` (the setup body
 * is emitted, so the inferred `number` survives) while the declaration surface
 * can only emit `count: unknown` (the body is omitted, so the binding is not in
 * scope — see the TODO at `crates/verter_compiler/src/tsc/script.rs`).
 *
 * Every host that CAN choose its import target must therefore choose the same
 * one, or the same source types differently depending on where the user reads
 * it. These guards pin that at the type level, not at the path level.
 */
describe("cross_host_import_surface_semantic_parity (#11)", () => {
  const EXPOSE_MAIN = `import Expose from "./Expose.vue";
type Inst = InstanceType<typeof Expose>;
const asNumber: number = ({} as Inst).count;
export { asNumber };
`;
  const EXPOSE_MAIN_WRONG = `import Expose from "./Expose.vue";
type Inst = InstanceType<typeof Expose>;
const asString: string = ({} as Inst).count;
export { asString };
`;

  it("the captured surfaces really are type-incompatible for an inferred exposed member", () => {
    // The premise the rest of this describe rests on, read off the REAL
    // captured host output rather than assumed.
    expect(fixtures.exposeVue.api!.code).toContain("count: typeof count");
    expect(fixtures.exposeVue.decl!.code).toContain("count: unknown");
  });

  it("an inferred `defineExpose`d member types as its REAL type in the browser host", () => {
    const { ls, path } = createFixtureLs(ts, {
      user: { "main.ts": EXPOSE_MAIN },
      carriers: { "Expose.vue": surfacesOf(fixtures.exposeVue) },
    });
    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    // `count` is `number`: assigning it to `number` is clean, and the module
    // resolved (an unresolved import would surface 2307 instead).
    expect(diags.some((d) => d.code === 2307)).toBe(false);
    expect(diags.filter((d) => d.code === 2322)).toHaveLength(0);
  });

  it("…and is NOT `any`/`unknown`: assigning it to the wrong type still errs", () => {
    const { ls, path } = createFixtureLs(ts, {
      user: { "main.ts": EXPOSE_MAIN_WRONG },
      carriers: { "Expose.vue": surfacesOf(fixtures.exposeVue) },
    });
    const diags = diagRows(ts, ls.languageService.getSemanticDiagnostics(path("main.ts")));
    const assignability = diags.filter((d) => d.code === 2322);
    expect(assignability).toHaveLength(1);
    expect(assignability[0].message).toMatch(/'number' is not assignable to type 'string'/);
  });

  it("the browser reaches the SAME descriptor-derived surface the tsserver plugin redirects to", () => {
    const { ls, path, store } = createFixtureLs(ts, {
      user: { "main.ts": EXPOSE_MAIN },
      carriers: { "Expose.vue": surfacesOf(fixtures.exposeVue) },
    });
    // Pinned to the shared derivation, not to a literal: both hosts call the
    // same policy over the same `importSurface` descriptor column.
    const expected = toImportSurfaceFileName(path("Expose.vue"));
    expect(expected).toBe(path("Expose.vue.verter.ts"));
    const program = ls.languageService.getProgram()!;
    expect(program.getSourceFile(expected!)).toBeDefined();
    // The weaker declaration surface is published and still not reached.
    expect(store.readyFile(path("Expose.d.vue.ts"))).toBeDefined();
    expect(program.getSourceFile(path("Expose.d.vue.ts"))).toBeUndefined();
  });
});
