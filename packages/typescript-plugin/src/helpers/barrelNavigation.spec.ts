import { describe, expect, it } from "vitest";
import ts from "typescript";
import {
  getAliasedNavigationResult,
  getAliasedQuickInfo,
  getModuleSpecifierNavigationResult,
  retargetAliasedDefinitionInfos,
} from "./barrelNavigation";

function resolveRelative(from: string, moduleName: string): string {
  if (!moduleName.startsWith(".")) return moduleName;
  const dir = from.substring(0, from.lastIndexOf("/"));
  const parts = (dir + "/" + moduleName).split("/").filter(Boolean);
  const resolved: string[] = [];
  for (const part of parts) {
    if (part === "..") resolved.pop();
    else if (part !== ".") resolved.push(part);
  }
  return "/" + resolved.join("/");
}

interface LanguageServiceFixture {
  languageService: ts.LanguageService;
  program: ts.Program;
  getSourceFile(path: string): ts.SourceFile | undefined;
}

function createLanguageServiceFixture(files: Record<string, string>): LanguageServiceFixture {
  const allFiles = {
    "/node_modules/vue/index.d.ts": "export interface PublicProps {}",
    ...files,
  };

  const options: ts.CompilerOptions = {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.ES2020,
    moduleResolution: ts.ModuleResolutionKind.Node10,
    noEmit: true,
    skipLibCheck: true,
    types: [],
  };

  const host = ts.createCompilerHost(options);
  host.getSourceFile = (fileName, languageVersion) => {
    const content = allFiles[fileName];
    if (content !== undefined) {
      return ts.createSourceFile(fileName, content, languageVersion, true);
    }
    return undefined;
  };
  host.fileExists = (fileName) => fileName in allFiles;
  host.readFile = (fileName) => allFiles[fileName] ?? "";
  host.resolveModuleNameLiterals = (moduleLiterals, containingFile) =>
    moduleLiterals.map(({ text: moduleName }) => {
      if (moduleName === "vue") {
        return {
          resolvedModule: {
            resolvedFileName: "/node_modules/vue/index.d.ts",
            extension: ts.Extension.Dts,
            isExternalLibraryImport: true,
          },
        } as ts.ResolvedModuleWithFailedLookupLocations;
      }

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

      const base = resolveRelative(containingFile, moduleName);
      for (const candidate of [base + ".ts", base + "/index.ts", base]) {
        if (candidate in allFiles) {
          return {
            resolvedModule: {
              resolvedFileName: candidate,
              extension: candidate.endsWith(".d.ts") ? ts.Extension.Dts : ts.Extension.Ts,
              isExternalLibraryImport: false,
            },
          } as ts.ResolvedModuleWithFailedLookupLocations;
        }
      }

      return {
        resolvedModule: undefined,
      } as ts.ResolvedModuleWithFailedLookupLocations;
    });

  const entryFiles = Object.keys(allFiles).filter((fileName) => !fileName.includes("node_modules"));
  const program = ts.createProgram(entryFiles, options, host);

  const languageServiceHost: ts.LanguageServiceHost = {
    getCompilationSettings: () => options,
    getScriptFileNames: () => entryFiles,
    getScriptVersion: () => "1",
    getScriptSnapshot: (fileName) =>
      allFiles[fileName] !== undefined
        ? ts.ScriptSnapshot.fromString(allFiles[fileName])
        : undefined,
    getCurrentDirectory: () => "/",
    getDefaultLibFileName: () => "/node_modules/typescript/lib/lib.d.ts",
    fileExists: (fileName) => fileName in allFiles,
    readFile: (fileName) => allFiles[fileName],
    readDirectory: () => [],
    resolveModuleNameLiterals: host.resolveModuleNameLiterals,
  };

  const languageService = ts.createLanguageService(languageServiceHost);

  return {
    languageService,
    program,
    getSourceFile: (path) => program.getSourceFile(path),
  };
}

function positionOf(sourceFile: ts.SourceFile, needle: string): number {
  const idx = sourceFile.text.indexOf(needle);
  if (idx === -1) {
    throw new Error(`Needle "${needle}" not found in ${sourceFile.fileName}`);
  }
  return idx;
}

describe("barrel navigation helpers", () => {
  it("resolves import bindings through barrel re-exports to the terminal vue declaration", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean, zIndex?: number } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
      "/src/App.ts": "import { Overlay } from './components'; const value = Overlay;",
    });

    const sourceFile = fixture.getSourceFile("/src/App.ts");
    expect(sourceFile).toBeDefined();

    const result = getAliasedNavigationResult(
      ts,
      fixture.program.getTypeChecker(),
      sourceFile!,
      positionOf(sourceFile!, "Overlay"),
    );

    expect(result).toBeDefined();
    expect(result?.definitions).toHaveLength(1);
    expect(result?.definitions[0]?.fileName).toBe("/src/Overlay.vue.d.ts");
  });

  it("resolves barrel export aliases to the terminal vue declaration", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
    });

    const sourceFile = fixture.getSourceFile("/src/components/index.ts");
    expect(sourceFile).toBeDefined();

    const result = getAliasedNavigationResult(
      ts,
      fixture.program.getTypeChecker(),
      sourceFile!,
      positionOf(sourceFile!, "Overlay"),
    );

    expect(result).toBeDefined();
    expect(result?.definitions).toHaveLength(1);
    expect(result?.definitions[0]?.fileName).toBe("/src/Overlay.vue.d.ts");
  });

  it("uses target quick info for aliased imports instead of alias hover text", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean, zIndex?: number } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
      "/src/App.ts": "import { Overlay } from './components'; const value = Overlay;",
    });

    const sourceFile = fixture.getSourceFile("/src/App.ts");
    expect(sourceFile).toBeDefined();

    const quickInfo = getAliasedQuickInfo(
      ts,
      fixture.languageService,
      fixture.program.getTypeChecker(),
      sourceFile!,
      positionOf(sourceFile!, "Overlay"),
    );

    expect(quickInfo).toBeDefined();
    const display = ts.displayPartsToString(quickInfo?.displayParts ?? []);
    expect(display).toContain("zIndex");
    expect(display).not.toContain("import Overlay");
  });

  it("retargets broad barrel definition spans to the terminal vue declaration", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean, zIndex?: number } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
    });

    const sourceFile = fixture.getSourceFile("/src/components/index.ts");
    expect(sourceFile).toBeDefined();

    const retargeted = retargetAliasedDefinitionInfos(
      ts,
      fixture.program.getTypeChecker(),
      fixture.getSourceFile,
      [
        {
          fileName: "/src/components/index.ts",
          textSpan: { start: 0, length: sourceFile!.text.length },
          contextSpan: { start: 0, length: sourceFile!.text.length },
          name: "Overlay",
          kind: ts.ScriptElementKind.alias,
          containerKind: ts.ScriptElementKind.unknown,
          containerName: "",
          isLocal: false,
          isAmbient: false,
          unverified: false,
        },
      ],
    );

    expect(retargeted).toBeDefined();
    expect(retargeted).toHaveLength(1);
    expect(retargeted?.[0]?.fileName).toBe("/src/Overlay.vue.d.ts");
  });

  it("retargets coarse barrel definition spans when the caller provides the symbol name", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
    });

    const sourceFile = fixture.getSourceFile("/src/components/index.ts");
    expect(sourceFile).toBeDefined();

    const retargeted = retargetAliasedDefinitionInfos(
      ts,
      fixture.program.getTypeChecker(),
      fixture.getSourceFile,
      [
        {
          fileName: "/src/components/index.ts",
          textSpan: { start: 0, length: sourceFile!.text.length },
          contextSpan: { start: 0, length: sourceFile!.text.length },
          name: "",
          kind: ts.ScriptElementKind.unknown,
          containerKind: ts.ScriptElementKind.unknown,
          containerName: "",
          isLocal: false,
          isAmbient: false,
          unverified: false,
        },
      ],
      "Overlay",
    );

    expect(retargeted).toBeDefined();
    expect(retargeted).toHaveLength(1);
    expect(retargeted?.[0]?.fileName).toBe("/src/Overlay.vue.d.ts");
  });

  it("prefers the caller-provided symbol name over an unhelpful definition name", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
    });

    const sourceFile = fixture.getSourceFile("/src/components/index.ts");
    expect(sourceFile).toBeDefined();

    const retargeted = retargetAliasedDefinitionInfos(
      ts,
      fixture.program.getTypeChecker(),
      fixture.getSourceFile,
      [
        {
          fileName: "/src/components/index.ts",
          textSpan: { start: 0, length: sourceFile!.text.length },
          contextSpan: { start: 0, length: sourceFile!.text.length },
          name: "default",
          kind: ts.ScriptElementKind.unknown,
          containerKind: ts.ScriptElementKind.unknown,
          containerName: "",
          isLocal: false,
          isAmbient: false,
          unverified: false,
        },
      ],
      "Overlay",
    );

    expect(retargeted).toBeDefined();
    expect(retargeted).toHaveLength(1);
    expect(retargeted?.[0]?.fileName).toBe("/src/Overlay.vue.d.ts");
  });

  it("resolves vue module specifier definitions directly", () => {
    const fixture = createLanguageServiceFixture({
      "/src/Overlay.vue.d.ts":
        "declare const Overlay: { new(): { $props: { show?: boolean } } }; export default Overlay",
      "/src/components/index.ts": "export { default as Overlay } from '../Overlay.vue'",
    });

    const sourceFile = fixture.getSourceFile("/src/components/index.ts");
    expect(sourceFile).toBeDefined();

    const result = getModuleSpecifierNavigationResult(
      ts,
      sourceFile!,
      positionOf(sourceFile!, "'../Overlay.vue'") + 3,
      (moduleName) => (moduleName === "../Overlay.vue" ? "/src/Overlay.vue.d.ts" : undefined),
    );

    expect(result).toBeDefined();
    expect(result?.definitions).toHaveLength(1);
    expect(result?.definitions[0]?.fileName).toBe("/src/Overlay.vue.d.ts");
  });
});
