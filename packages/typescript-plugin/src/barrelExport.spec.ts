/**
 * Integration tests for barrel re-export type preservation.
 *
 * Verifies that Vue component types (props, emits, slots) survive
 * `export { default as X } from './X.vue'` barrel re-exports when
 * resolved through Verter's TypeScript plugin module resolution.
 *
 * Uses TypeScript's compiler API with a virtual file system that
 * mimics the plugin's .vue → .vue.d.ts resolution behavior.
 */
import { describe, it, expect } from "vitest";
import ts from "typescript";
import { parseFile, FALLBACK_STUB } from "./helpers/getDtsSnapshot";

// ── Minimal Vue type stub ────────────────────────────────────────────────────
// Just enough for `import { defineComponent } from "vue"` and
// `import("vue").ComponentPublicInstance` to resolve without errors.

const VUE_STUB = `
declare module "vue" {
  export interface ComponentPublicInstance<
    P = {}, B = {}, D = {}, C = {}, M = {}, Mixin = {}, Extends = {}, Emits = {}
  > {}
  export function defineComponent<Props = {}, Emits extends string = string>(
    options: Record<string, unknown>
  ): { new(): ComponentPublicInstance<Props> & { $props: Props } }
}
`;

// ── Virtual TS program factory ──────────────────────────────────────────────

function resolveRelative(from: string, moduleName: string): string {
  if (!moduleName.startsWith(".")) return moduleName;
  const dir = from.substring(0, from.lastIndexOf("/"));
  const parts = (dir + "/" + moduleName).split("/").filter(Boolean);
  const resolved: string[] = [];
  for (const p of parts) {
    if (p === "..") resolved.pop();
    else if (p !== ".") resolved.push(p);
  }
  return "/" + resolved.join("/");
}

interface VirtualProgramResult {
  program: ts.Program;
  checker: ts.TypeChecker;
  getSourceFile(path: string): ts.SourceFile | undefined;
}

function createVirtualProgram(
  files: Record<string, string>,
  entryFiles?: string[],
): VirtualProgramResult {
  // Always include the Vue stub
  const allFiles: Record<string, string> = {
    "/node_modules/vue/index.d.ts": VUE_STUB,
    ...files,
  };

  const options: ts.CompilerOptions = {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.ES2020,
    moduleResolution: ts.ModuleResolutionKind.Node10,
    strict: true,
    skipLibCheck: true,
    noEmit: true,
    esModuleInterop: true,
    allowSyntheticDefaultImports: true,
  };

  const host = ts.createCompilerHost(options);

  host.getSourceFile = (fileName, languageVersion) => {
    const content = allFiles[fileName];
    if (content !== undefined) {
      return ts.createSourceFile(fileName, content, languageVersion, true);
    }
    // Return empty default lib to avoid errors
    if (fileName.includes("lib.") && fileName.endsWith(".d.ts")) {
      return ts.createSourceFile(fileName, "", languageVersion, true);
    }
    return undefined;
  };

  host.fileExists = (fileName) => {
    return fileName in allFiles || (fileName.includes("lib.") && fileName.endsWith(".d.ts"));
  };

  host.readFile = (fileName) => allFiles[fileName] ?? "";

  host.resolveModuleNameLiterals = (moduleLiterals, containingFile) => {
    return moduleLiterals.map(({ text: moduleName }): ts.ResolvedModuleWithFailedLookupLocations => {
      // Handle "vue" package
      if (moduleName === "vue") {
        return {
          resolvedModule: {
            resolvedFileName: "/node_modules/vue/index.d.ts",
            extension: ts.Extension.Dts,
            isExternalLibraryImport: true,
          },
        } as ts.ResolvedModuleWithFailedLookupLocations;
      }

      // Simulate plugin: .vue → .vue.d.ts
      if (moduleName.endsWith(".vue")) {
        const resolved = resolveRelative(containingFile, moduleName) + ".d.ts";
        if (resolved in allFiles) {
          return {
            resolvedModule: {
              resolvedFileName: resolved,
              extension: ts.Extension.Dts,
              isExternalLibraryImport: false,
            },
          } as ts.ResolvedModuleWithFailedLookupLocations;
        }
      }

      // Normal .ts resolution
      const base = resolveRelative(containingFile, moduleName);
      const candidates = [base + ".ts", base + "/index.ts", base];
      for (const candidate of candidates) {
        if (candidate in allFiles) {
          const ext = candidate.endsWith(".d.ts") ? ts.Extension.Dts : ts.Extension.Ts;
          return {
            resolvedModule: {
              resolvedFileName: candidate,
              extension: ext,
              isExternalLibraryImport: false,
            },
          } as ts.ResolvedModuleWithFailedLookupLocations;
        }
      }

      return { resolvedModule: undefined } as ts.ResolvedModuleWithFailedLookupLocations;
    });
  };

  const entries = entryFiles ?? Object.keys(allFiles).filter((f) => !f.includes("node_modules"));
  const program = ts.createProgram(entries, options, host);

  return {
    program,
    checker: program.getTypeChecker(),
    getSourceFile: (path) => program.getSourceFile(path),
  };
}

// ── Type inspection helpers ─────────────────────────────────────────────────

/**
 * Get the type of a named import or export from a source file.
 * E.g., for `import { MyComp } from './index'`, returns the type of `MyComp`.
 */
function getImportedSymbolType(
  checker: ts.TypeChecker,
  sourceFile: ts.SourceFile,
  symbolName: string,
): ts.Type | undefined {
  const symbol = checker.getSymbolsInScope(sourceFile, ts.SymbolFlags.Value).find(
    (s) => s.name === symbolName,
  );
  if (!symbol) return undefined;
  return checker.getDeclaredTypeOfSymbol(symbol);
}

/**
 * Get the type of a type alias from a source file.
 */
function getTypeAliasType(
  checker: ts.TypeChecker,
  sourceFile: ts.SourceFile,
  aliasName: string,
): string | undefined {
  for (const stmt of sourceFile.statements) {
    if (ts.isTypeAliasDeclaration(stmt) && stmt.name.text === aliasName) {
      const type = checker.getTypeAtLocation(stmt);
      return checker.typeToString(
        type,
        undefined,
        ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.WriteArrayAsGenericType,
      );
    }
  }
  return undefined;
}

/**
 * Get the type of a variable declaration from a source file.
 */
function getVariableType(
  checker: ts.TypeChecker,
  sourceFile: ts.SourceFile,
  varName: string,
): string | undefined {
  for (const stmt of sourceFile.statements) {
    if (ts.isVariableStatement(stmt)) {
      for (const decl of stmt.declarationList.declarations) {
        if (ts.isIdentifier(decl.name) && decl.name.text === varName) {
          const type = checker.getTypeAtLocation(decl);
          return checker.typeToString(
            type,
            undefined,
            ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.WriteArrayAsGenericType,
          );
        }
      }
    }
  }
  return undefined;
}

// ── Handcrafted declaration (no native binary needed) ───────────────────────
// Mirrors the output shape of generate_code() from verter_core/src/tsc/script.rs

function makeComponentDecl(name: string, propsFields: string, emitOverloads?: string): string {
  const emit = emitOverloads ?? "(event: string, ...args: unknown[]) => void";
  return `
import { defineComponent } from "vue"

const __comp = defineComponent({})

declare const ${name}: typeof __comp & import("vue").ComponentPublicInstance<{}, {}, {}, {}, {}, {}, {}, {}> & {
  new(): {
    $props: { ${propsFields} },
    $emit: ${emit},
    $data: {},
    $attrs: {},
    $refs: {},
  }
}
export default ${name}
`;
}

// ── Mock logger for parseFile() ─────────────────────────────────────────────

const mockLogger = { info: () => {}, msg: () => {} } as any;

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

describe("barrel export type preservation", () => {
  // ── Direct import (baseline) ────────────────────────────────────────────

  describe("direct default import", () => {
    it("preserves $props type from .vue.d.ts", () => {
      const decl = makeComponentDecl("MyComp", "title: string; count?: number");

      const { checker, getSourceFile } = createVirtualProgram({
        "/src/MyComp.vue.d.ts": decl,
        "/src/consumer.ts": `
          import MyComp from './MyComp.vue'
          type CompInstance = InstanceType<typeof MyComp>
          type Props = CompInstance['$props']
          const props: Props = null!
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const propsType = getVariableType(checker, sf!, "props");
      expect(propsType).toBeDefined();
      // Positive: props type contains expected fields
      expect(propsType).toContain("title");
      expect(propsType).toContain("string");
      // Negative: not degraded to empty or any
      expect(propsType).not.toBe("any");
      expect(propsType).not.toBe("{}");
    });
  });

  // ── Barrel re-export: export { default as X } from './X.vue' ───────────

  describe("barrel re-export: export { default as X }", () => {
    it("preserves $props type through barrel re-export", () => {
      const decl = makeComponentDecl("Overlay", "zIndex?: number; show?: boolean; lockScroll?: boolean");

      const { checker, getSourceFile } = createVirtualProgram({
        "/src/Overlay.vue.d.ts": decl,
        "/src/index.ts": `export { default as Overlay } from './Overlay.vue'`,
        "/src/consumer.ts": `
          import { Overlay } from './index'
          type CompInstance = InstanceType<typeof Overlay>
          type Props = CompInstance['$props']
          const props: Props = null!
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const propsType = getVariableType(checker, sf!, "props");
      expect(propsType).toBeDefined();
      // Positive: props type should contain expected fields
      expect(propsType).toContain("zIndex");
      expect(propsType).toContain("show");
      expect(propsType).toContain("lockScroll");
      // Negative: not degraded to empty or any
      expect(propsType).not.toBe("any");
      expect(propsType).not.toBe("{}");
    });

    it("preserves emits type through barrel re-export", () => {
      const decl = makeComponentDecl(
        "Dialog",
        "visible: boolean",
        '((event: "close") => void) & ((event: "confirm", value: string) => void)',
      );

      const { checker, getSourceFile } = createVirtualProgram({
        "/src/Dialog.vue.d.ts": decl,
        "/src/index.ts": `export { default as Dialog } from './Dialog.vue'`,
        "/src/consumer.ts": `
          import { Dialog } from './index'
          type CompInstance = InstanceType<typeof Dialog>
          type Emit = CompInstance['$emit']
          const emit: Emit = null!
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const emitType = getVariableType(checker, sf!, "emit");
      expect(emitType).toBeDefined();
      // Positive: emit type should contain event names
      expect(emitType).toContain("close");
      expect(emitType).toContain("confirm");
      // Negative: not degraded to generic
      expect(emitType).not.toBe("any");
    });
  });

  // ── Multi-level barrel re-export ───────────────────────────────────────

  describe("multi-level barrel re-export", () => {
    it("preserves type through component/index.ts → components/index.ts chain", () => {
      const decl = makeComponentDecl("Button", "label: string; disabled?: boolean; size?: string");

      const { checker, getSourceFile } = createVirtualProgram({
        "/src/components/Button/Button.vue.d.ts": decl,
        "/src/components/Button/index.ts": `export { default as Button } from './Button.vue'`,
        "/src/components/index.ts": `export * from './Button'`,
        "/src/consumer.ts": `
          import { Button } from './components'
          type CompInstance = InstanceType<typeof Button>
          type Props = CompInstance['$props']
          const props: Props = null!
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const propsType = getVariableType(checker, sf!, "props");
      expect(propsType).toBeDefined();
      // Positive: props survive two levels of re-export
      expect(propsType).toContain("label");
      expect(propsType).toContain("disabled");
      expect(propsType).toContain("size");
      // Negative: not degraded
      expect(propsType).not.toBe("any");
      expect(propsType).not.toBe("{}");
    });
  });

  // ── Using parseFile() output (requires native binary) ─────────────────

  describe("barrel re-export with parseFile() output", () => {
    const sfc = `
<script setup lang="ts">
defineProps<{
  zIndex?: number
  duration?: number | string
  show?: boolean
  lockScroll?: boolean
}>()
</script>
<template><div /></template>
`;

    let generatedDecl: string;

    // Try to generate with native binary; skip if unavailable
    try {
      generatedDecl = parseFile("/src/Overlay.vue", sfc, mockLogger);
    } catch {
      generatedDecl = FALLBACK_STUB;
    }

    const hasNative = generatedDecl !== FALLBACK_STUB;

    it.skipIf(!hasNative)("parseFile() generates valid component declaration", () => {
      // Positive: output has expected markers
      expect(generatedDecl).toContain("defineComponent");
      expect(generatedDecl).toContain("ComponentPublicInstance");
      expect(generatedDecl).toContain("export default");
      expect(generatedDecl).toContain("$props");
      expect(generatedDecl).toContain("zIndex");
      expect(generatedDecl).toContain("lockScroll");
      // Negative: not the fallback stub
      expect(generatedDecl).not.toBe(FALLBACK_STUB);
      expect(generatedDecl).not.toContain("as any");
    });

    it.skipIf(!hasNative)("preserves $props through barrel with real parseFile() output", () => {
      const { checker, getSourceFile } = createVirtualProgram({
        "/src/Overlay.vue.d.ts": generatedDecl,
        "/src/index.ts": `export { default as Overlay } from './Overlay.vue'`,
        "/src/consumer.ts": `
          import { Overlay } from './index'
          type CompInstance = InstanceType<typeof Overlay>
          type Props = CompInstance['$props']
          const props: Props = null!
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const propsType = getVariableType(checker, sf!, "props");
      expect(propsType).toBeDefined();
      // Positive: real props are preserved
      expect(propsType).toContain("zIndex");
      expect(propsType).toContain("duration");
      expect(propsType).toContain("show");
      expect(propsType).toContain("lockScroll");
      // Negative: not degraded
      expect(propsType).not.toBe("any");
      expect(propsType).not.toBe("{}");
    });

    it.skipIf(!hasNative)("preserves $props through multi-level barrel with real parseFile() output", () => {
      const { checker, getSourceFile } = createVirtualProgram({
        "/src/components/Overlay/Overlay.vue.d.ts": generatedDecl,
        "/src/components/Overlay/index.ts": `export { default as Overlay } from './Overlay.vue'`,
        "/src/components/index.ts": `export * from './Overlay'`,
        "/src/consumer.ts": `
          import { Overlay } from './components'
          type CompInstance = InstanceType<typeof Overlay>
          type Props = CompInstance['$props']
          const props: Props = null!
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const propsType = getVariableType(checker, sf!, "props");
      expect(propsType).toBeDefined();
      expect(propsType).toContain("zIndex");
      expect(propsType).toContain("lockScroll");
      expect(propsType).not.toBe("any");
      expect(propsType).not.toBe("{}");
    });
  });

  // ── Fallback stub degrades type (demonstrates the bug scenario) ─────

  describe("fallback stub produces degraded type", () => {
    it("FALLBACK_STUB causes barrel re-export to lose $props", () => {
      // When the native binary fails to load, parseFile() returns FALLBACK_STUB.
      // This demonstrates the type degradation the user experiences.
      const { checker, getSourceFile } = createVirtualProgram({
        "/src/Overlay.vue.d.ts": FALLBACK_STUB, // "export default {} as any"
        "/src/index.ts": `export { default as Overlay } from './Overlay.vue'`,
        "/src/consumer.ts": `
          import { Overlay } from './index'
          type CompInstance = InstanceType<typeof Overlay>
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const compType = getTypeAliasType(checker, sf!, "CompInstance");
      // With fallback stub, the type degrades — no $props with actual fields
      expect(compType).toBeDefined();
      // The type should NOT contain any prop names (they're lost)
      expect(compType).not.toContain("zIndex");
      expect(compType).not.toContain("lockScroll");
    });
  });

  // ── Diagnostic check: no type errors in consumer ──────────────────────

  describe("no type errors in barrel consumer", () => {
    it("barrel re-export consumer has no diagnostic errors", () => {
      const decl = makeComponentDecl("Alert", "message: string; type?: string");

      const { program, getSourceFile } = createVirtualProgram({
        "/src/Alert.vue.d.ts": decl,
        "/src/index.ts": `export { default as Alert } from './Alert.vue'`,
        "/src/consumer.ts": `
          import { Alert } from './index'
          const A = Alert
        `,
      });

      const sf = getSourceFile("/src/consumer.ts");
      expect(sf).toBeDefined();

      const diagnostics = ts.getPreEmitDiagnostics(program, sf!);
      const errors = diagnostics.filter((d) => d.category === ts.DiagnosticCategory.Error);

      // Filter out lib-related errors (our minimal environment doesn't have full lib.d.ts)
      const relevantErrors = errors.filter((d) => {
        const msg = ts.flattenDiagnosticMessageText(d.messageText, "\n");
        // Skip missing global types from lib.d.ts and module resolution errors
        return (
          !msg.includes("Cannot find global type") &&
          !msg.includes("Cannot find name") &&
          !msg.includes("Cannot find module")
        );
      });

      // No type errors for a valid barrel import (ignoring missing lib types)
      expect(relevantErrors).toHaveLength(0);
    });
  });
});
