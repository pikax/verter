import ts, {
  CompletionEntry,
  ModuleResolutionHost,
  ResolvedModuleFull,
  ScriptKind,
  version,
} from "typescript";
import lsp, {
  CompletionItem,
  CompletionItemKind,
  DiagnosticRelatedInformation,
  DiagnosticSeverity,
  DiagnosticTag,
  InsertTextFormat,
  Range,
} from "vscode-languageserver/node";
import path from "node:path";
import { DocumentManager } from "../documents/manager";
import logger from "../../../logger";
import {
  isVerterVirtual,
  pathToUrl,
  uriToVerterVirtual,
  urlToPath,
} from "../documents/utils";
import { VueDocument } from "../documents/document";

export function getTypescriptService(
  workspacePath: string,
  documentManager: DocumentManager
) {
  let tsconfigOptions: ts.CompilerOptions = {
    allowJs: true,
    noImplicitAny: false,
    noImplicitThis: false,
    noImplicitReturns: false,
  };

  let parsedConfig: ts.ParsedCommandLine | null = null;
  const configFile = path.resolve(workspacePath, "./tsconfig.json");

  if (ts.sys.fileExists(configFile)) {
    // // TODO watch the config file
    // ts.sys.watchFile!(configFile, (f) => {});

    const tsConfigStr = ts.sys.readFile(
      path.resolve(workspacePath, "./tsconfig.json"),
      "utf-8"
    );

    const { config } = ts.parseConfigFileTextToJson(
      path.resolve(workspacePath, "./tsconfig.json"),
      tsConfigStr!
    );

    const parseConfigHost: ts.ParseConfigHost = {
      ...ts.sys,

      readDirectory(rootDir, extensions, excludes, includes, depth) {
        return ts.sys.readDirectory(
          rootDir,
          [...extensions, ".vue"],
          excludes,
          includes,
          depth
        );
      },

      useCaseSensitiveFileNames: ts.sys.useCaseSensitiveFileNames,
    };

    parsedConfig = ts.parseJsonConfigFileContent(
      config,
      parseConfigHost,
      workspacePath
    );

    tsconfigOptions = {
      ...parsedConfig.options,
      // allowArbitraryExtensions: true,
      allowJs: true,
      noImplicitAny: false,
      noImplicitThis: false,
      noImplicitReturns: false,
    };
  }

  const compilerOptions: ts.CompilerOptions = {
    ...tsconfigOptions,
    jsx: ts.JsxEmit.Preserve,
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    noImplicitAny: true,
  };

  const tsSystem = ts.sys;

  const map = {
    "/vue.ts": "vue",
    "@vue/runtime-dom.ts": "@vue/runtime-dom",
    "@vue/runtime-core.ts": "@vue/runtime-core",
    "@vue/reactivity.ts": "@vue/reactivity",
    "/vue/jsx-runtime.ts": "vue/jsx-runtime",
    "@vue/shared.ts": "@vue/shared",
  };

  const moduleCache = ts.createModuleResolutionCache(
    workspacePath,
    (e) => {
      // console.log("p", e);
      // console.log("pp", tsSystem.resolvePath(e));
      return tsSystem.resolvePath(e);
      return e;
    },
    compilerOptions
  );

  const host: ts.LanguageServiceHost = {
    log: (message) => logger.info(`[ts] ${message}`),
    getCompilationSettings: () => compilerOptions,
    getCurrentDirectory: () => workspacePath,
    getDefaultLibFileName: ts.getDefaultLibFilePath,

    readFile: (filepath, encoding) => {
      if (filepath.endsWith(".vue.tsx")) {
        const snap = documentManager.getSnapshotIfExists(filepath);
        return snap?.getText(0, snap.getLength());
      }
      if (isVerterVirtual(filepath)) {
        filepath = urlToPath(filepath)!;
      }
      if (filepath.endsWith(".vue") || filepath.endsWith(".vue.tsx")) {
        debugger;
      }
      try {
        return ts.sys.readFile(filepath, encoding);
      } catch (e) {
        console.log("ee", e);
        debugger;
        return "";
      }
    },
    fileExists: (filepath) => {
      // const d = Object.keys(map).find((x) => filepath.endsWith(x));
      // if (d) {
      //   return true;
      // }
      if (filepath.endsWith(".vue.tsx")) {
        return !!documentManager.getSnapshotIfExists(filepath);
      }

      return ts.sys.fileExists(filepath);
    },
    getDirectories: ts.sys.getDirectories,
    useCaseSensitiveFileNames: () => tsSystem.useCaseSensitiveFileNames,
    getNewLine: () => tsSystem.newLine,

    readDirectory(path, extensions, exclude, include, depth) {
      return tsSystem.readDirectory(
        path,
        [...extensions, ".vue"],
        exclude,
        include,
        depth
      );
    },
    getScriptKind: (fileName: string) => {
      const ext = fileName.slice(fileName.lastIndexOf("."));
      switch (ext.toLowerCase()) {
        case ts.Extension.Js:
          return ts.ScriptKind.JS;
        case ts.Extension.Jsx:
          return ts.ScriptKind.JSX;
        case ".mts":
        case ".cts":
        case ts.Extension.Ts:
          return ts.ScriptKind.TS;
        case ts.Extension.Tsx:
          return ts.ScriptKind.TSX;
        case ts.Extension.Json:
          return ts.ScriptKind.JSON;
        default:
          console.log("unknown", ext);
          return ts.ScriptKind.Unknown;
      }
    },

    getScriptVersion: (filename) => {
      return (
        documentManager.getSnapshotIfExists(filename)?.version.toString() ?? ""
      );
    },
    getScriptSnapshot: (filename) => {
      if (filename.endsWith(".vue")) {
        debugger;
      }
      if (filename.endsWith(".vue.tsx")) {
        console.log("sssd");
      }
      return documentManager.getSnapshotIfExists(filename);
    },

    getScriptFileNames() {
      //   if (parsedConfig) {
      //     parsedConfig.fileNames;
      //   }

      const files = parsedConfig?.fileNames ?? [];

      const map = files.map(uriToVerterVirtual) ?? [];

      console.log("mnm", map.length);

      return map;
    },

    resolveModuleNameLiterals(
      moduleLiterals,
      containingFile,
      redirectedReference,
      options,
      containingSourceFile,
      reusedNames
    ) {
      if (isVerterVirtual(containingFile)) {
        containingFile = urlToPath(containingFile)!;
      }
      const modules = moduleLiterals.map((x) => {
        const h = this;
        // const h = ts.sys;

        let moduleLoc = x.text;

        // if (~x.text.indexOf("vue")) {
        //   debugger;
        // }

        if (moduleLoc.endsWith(".vue")) {
          moduleLoc = x.text + ".tsx";
        }

        switch (options.moduleResolution) {
          case ts.ModuleResolutionKind.Classic: {
            return ts.resolveModuleName(
              moduleLoc,
              containingFile,
              options,
              h,
              moduleCache,
              redirectedReference
            );
          }
          case ts.ModuleResolutionKind.NodeJs:
          case ts.ModuleResolutionKind.Node10:
          case ts.ModuleResolutionKind.Node16:
          case ts.ModuleResolutionKind.NodeNext: {
            return ts.nodeModuleNameResolver(
              moduleLoc,
              containingFile,
              options,
              h,
              moduleCache,
              redirectedReference
            );
          }
          case ts.ModuleResolutionKind.Bundler:
          default:
            const r = ts.bundlerModuleNameResolver(
              moduleLoc,
              containingFile,
              options,
              h,
              moduleCache,
              redirectedReference
            );

            if (x.text.endsWith(".vue")) {
              if (r.resolvedModule) {
                if (r.resolvedModule.resolvedFileName.endsWith(".vue.tsx")) {
                  // console.log(
                  //   "up resolved",
                  //   r,
                  //   uriToVerterVirtual(
                  //     r.resolvedModule.resolvedFileName.slice(0, -4)
                  //   )
                  // );

                  const originalPath = r.resolvedModule.resolvedFileName.slice(
                    0,
                    -4
                  );
                  // @ts-expect-error
                  r.resolvedModule.originalPath = originalPath;
                  r.resolvedModule.resolvedFileName =
                    uriToVerterVirtual(originalPath);
                  r.resolvedModule.resolvedUsingTsExtension = false;

                  // r.resolvedModule.resolvedFileName =
                  //   r.resolvedModule.resolvedFileName.slice(0, -4);

                  // r.resolvedModule.extension = ".vue";
                } else {
                  debugger;
                }
              } else {
                debugger;
              }
            }
            // console.log("sss", r);
            return r;
        }
      });

      // TODO add debug flag
      if (true) {
        const emptyModules = modules
          .map((x, i) => (!x.resolvedModule ? i : false))
          .filter((x) => x !== false)
          .map((x: number) => moduleLiterals[x].text);
        if (emptyModules.length > 0) {
          console.warn("Modules missing!", emptyModules);
        }
      }

      console.log("mmmm", modules.length);

      return modules;
    },

    realpath: (filename) => {
      if (filename.endsWith(".vue") || filename.endsWith(".vue.tsx")) {
        debugger;
        0;
      }
      return tsSystem.realpath?.(filename) ?? filename;
    },

    // getCustomTransformers() {
    //   return {
    //     before: [
    //       (context: ts.TransformationContext) => {
    //         return (sourceFile: ts.SourceFile) => {
    //           if (
    //             sourceFile.fileName.endsWith(".vue") ||
    //             sourceFile.fileName.endsWith(".vue.tsx")
    //           ) {
    //             debugger;
    //             return sourceFile;
    //           }
    //           // Transformation logic here
    //           return sourceFile;
    //         };
    //       },
    //     ],
    //   };
    // },
  };
  const languageService = ts.createLanguageService(
    host,
    ts.createDocumentRegistry(tsSystem.useCaseSensitiveFileNames)
    // ts.LanguageServiceMode.Semantic
  );

  return languageService;
}

export function mapDiagnostic(
  diagnostic: ts.Diagnostic,
  document: VueDocument
): lsp.Diagnostic | undefined {
  const range = mapTextSpanToRange(diagnostic, document);
  if (!range) {
    return;
  }

  const severity = categoryToSeverity(diagnostic.category);

  return {
    severity,
    range,
    source: "Verter",
    code: diagnostic.code,

    message: ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n"),

    tags: [
      diagnostic.reportsUnnecessary && DiagnosticTag.Unnecessary,
      diagnostic.reportsDeprecated && DiagnosticTag.Deprecated,
    ].filter(Boolean) as DiagnosticTag[],
  };
}

export function mapCompletion(
  tsCompletion: ts.CompletionEntry,
  data: {
    virtualUrl: string;
    index: number;
    triggerKind: lsp.CompletionTriggerKind | undefined;
    triggerCharacter: string | undefined;
  }
): lsp.CompletionItem | lsp.CompletionItem[] {
  const item: lsp.CompletionItem = {
    label: tsCompletion.name,
    kind:
      retrieveKindBasedOnSymbol(tsCompletion.symbol) ||
      KindMap[tsCompletion.kind] ||
      CompletionItemKind.Text,
    detail: tsCompletion.source, // Example mapping, adjust as needed
    insertText: tsCompletion.insertText || tsCompletion.name,
    insertTextFormat: InsertTextFormat.PlainText,
    sortText: tsCompletion.sortText,
    filterText: tsCompletion.filterText,
    documentation: tsCompletion.source,
    data: Object.assign(data, tsCompletion.data),
  };

  // todo add extra or process completion

  return item;
}

export function itemToMarkdown(item: ts.CompletionEntryDetails | ts.QuickInfo) {
  return {
    kind: "markdown" as "markdown",
    value:
      ts.displayPartsToString(item.documentation) +
      (item.tags
        ? "\n\n" +
          item.tags
            .map(
              (tag) =>
                `_@${tag.name}_ — \`${ts.displayPartsToString(
                  tag.text?.slice(0, 1)
                )}\` ${ts.displayPartsToString(tag.text?.slice(1))}\n`
            )
            .join("\n")
        : ""),
  };
}

export function mapTextSpanToRange(
  textSpan: {
    start: number | undefined;
    length: number | undefined;
  },
  document: VueDocument
) {
  if (!textSpan.start || !textSpan.length) return undefined;
  const start = document.originalPosition(textSpan.start);
  const end = document.originalPosition(textSpan.start + textSpan.length);

  try {
    return Range.create(start, end);
  } catch (e) {
    console.error("invalid range", e);
    // debugger
    return undefined;
  }
}

export function formatQuickInfo(
  quickInfo: ts.QuickInfo,
  document: VueDocument
) {
  const contents = itemToMarkdown(quickInfo);

  console.log("sssd", quickInfo.displayParts);

  const str =
    "```ts\n" +
    displayPartsToString(quickInfo.displayParts) +
    "\n```\n" +
    "\n\n" +
    contents.value;

  // remove the verter virtual
  // contents.value = str.replace(/verter-virtual:\/\/\//gi, "");
  contents.value = str;

  // Convert the text span to an LSP range
  const range = mapTextSpanToRange(quickInfo.textSpan, document);

  return {
    contents,
    range,
  };
}

function displayPartsToString(parts: ts.SymbolDisplayPart[]) {
  return ts.displayPartsToString(
    parts.map((x) => {
      // patch the verter-virtual paths
      if (x.kind === "stringLiteral") {
        let text = x.text;
        if (
          text.startsWith('"verter-virtual:///') ||
          text.startsWith("'verter-virtual:///")
        ) {
          return {
            ...x,
            text:
              x.text.slice(0, 1) +
              decodeURIComponent(x.text).slice("'verter-virtual:///".length),
          };
        } else if (text.startsWith("verter-virtual:///")) {
          console.log("found textX ", text);
          debugger;
        }
      }
      return x;
    })
  );
}

function categoryToSeverity(
  category: ts.DiagnosticCategory
): lsp.DiagnosticSeverity {
  switch (category) {
    case ts.DiagnosticCategory.Error: {
      return DiagnosticSeverity.Error;
    }
    case ts.DiagnosticCategory.Warning: {
      return DiagnosticSeverity.Warning;
    }

    case ts.DiagnosticCategory.Message: {
      return DiagnosticSeverity.Information;
    }
    case ts.DiagnosticCategory.Suggestion: {
      return DiagnosticSeverity.Hint;
    }
  }
}

const KindMap: Record<ts.ScriptElementKind, lsp.CompletionItemKind> = {
  [ts.ScriptElementKind.unknown]: CompletionItemKind.Text,
  [ts.ScriptElementKind.warning]: CompletionItemKind.Text,
  [ts.ScriptElementKind.keyword]: CompletionItemKind.Keyword,
  [ts.ScriptElementKind.scriptElement]: CompletionItemKind.File,
  [ts.ScriptElementKind.moduleElement]: CompletionItemKind.Module,
  [ts.ScriptElementKind.classElement]: CompletionItemKind.Class,
  [ts.ScriptElementKind.localClassElement]: CompletionItemKind.Class,
  [ts.ScriptElementKind.interfaceElement]: CompletionItemKind.Interface,
  [ts.ScriptElementKind.typeElement]: CompletionItemKind.Class, // No direct equivalent in LSP; Class is a reasonable approximation.
  [ts.ScriptElementKind.enumElement]: CompletionItemKind.Enum,
  [ts.ScriptElementKind.enumMemberElement]: CompletionItemKind.EnumMember,
  [ts.ScriptElementKind.variableElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.localVariableElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.functionElement]: CompletionItemKind.Function,
  [ts.ScriptElementKind.localFunctionElement]: CompletionItemKind.Function,
  [ts.ScriptElementKind.memberFunctionElement]: CompletionItemKind.Method,
  [ts.ScriptElementKind.memberGetAccessorElement]: CompletionItemKind.Property,
  [ts.ScriptElementKind.memberSetAccessorElement]: CompletionItemKind.Property,
  [ts.ScriptElementKind.memberVariableElement]: CompletionItemKind.Field,
  [ts.ScriptElementKind.constructorImplementationElement]:
    CompletionItemKind.Constructor,
  [ts.ScriptElementKind.callSignatureElement]: CompletionItemKind.Method,
  [ts.ScriptElementKind.indexSignatureElement]: CompletionItemKind.Property,
  [ts.ScriptElementKind.constructSignatureElement]:
    CompletionItemKind.Constructor,
  [ts.ScriptElementKind.parameterElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.typeParameterElement]: CompletionItemKind.TypeParameter,
  [ts.ScriptElementKind.primitiveType]: CompletionItemKind.Class,
  [ts.ScriptElementKind.label]: CompletionItemKind.Text,
  [ts.ScriptElementKind.alias]: CompletionItemKind.Reference,
  [ts.ScriptElementKind.constElement]: CompletionItemKind.Constant,
  [ts.ScriptElementKind.letElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.directory]: CompletionItemKind.Folder,
  [ts.ScriptElementKind.externalModuleName]: CompletionItemKind.Module,
  [ts.ScriptElementKind.jsxAttribute]: CompletionItemKind.Property,
  [ts.ScriptElementKind.string]: CompletionItemKind.Constant,
  [ts.ScriptElementKind.link]: CompletionItemKind.Reference,
  [ts.ScriptElementKind.linkName]: CompletionItemKind.Reference,
  [ts.ScriptElementKind.linkText]: CompletionItemKind.Text,

  [ts.ScriptElementKind.variableAwaitUsingElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.variableUsingElement]: CompletionItemKind.Variable,
  [ts.ScriptElementKind.memberAccessorVariableElement]:
    CompletionItemKind.Property,
};

function retrieveKindBasedOnSymbol(
  symbol?: ts.Symbol
): CompletionItemKind | null {
  if (!symbol) return null;
  const flags = symbol.getFlags();

  if (flags & ts.SymbolFlags.Function) {
    return CompletionItemKind.Function;
  } else if (flags & ts.SymbolFlags.Method) {
    return CompletionItemKind.Method;
  } else if ((flags & ts.SymbolFlags.Class) !== 0) {
    return CompletionItemKind.Class;
  } else if ((flags & ts.SymbolFlags.Interface) !== 0) {
    return CompletionItemKind.Interface;
  } else if (
    (flags & ts.SymbolFlags.Enum) !== 0 ||
    (flags & ts.SymbolFlags.ConstEnum) !== 0 ||
    (flags & ts.SymbolFlags.RegularEnum) !== 0
  ) {
    return CompletionItemKind.Enum;
  } else if (
    (flags & ts.SymbolFlags.ValueModule) !== 0 ||
    (flags & ts.SymbolFlags.NamespaceModule) !== 0
  ) {
    return CompletionItemKind.Module;
  } else if (
    (flags & ts.SymbolFlags.Variable) !== 0 ||
    (flags & ts.SymbolFlags.BlockScopedVariable) !== 0 ||
    (flags & ts.SymbolFlags.FunctionScopedVariable) !== 0
  ) {
    return CompletionItemKind.Variable;
  } else if (flags & ts.SymbolFlags.ClassMember) {
    return CompletionItemKind.Class;
  } else if (
    (flags & ts.SymbolFlags.Property) !== 0 ||
    (flags & ts.SymbolFlags.Accessor) !== 0
  ) {
    return CompletionItemKind.Property;
  } else if ((flags & ts.SymbolFlags.GetAccessor) !== 0) {
    return CompletionItemKind.Property; // No direct mapping for getters; Property is a reasonable approximation
  } else if ((flags & ts.SymbolFlags.SetAccessor) !== 0) {
    return CompletionItemKind.Property; // Similar to GetAccessor
  } else if ((flags & ts.SymbolFlags.EnumMember) !== 0) {
    return CompletionItemKind.EnumMember;
  } else if ((flags & ts.SymbolFlags.TypeAlias) !== 0) {
    return CompletionItemKind.TypeParameter; // Using TypeParameter for TypeAlias as a close match
  } else if ((flags & ts.SymbolFlags.Alias) !== 0) {
    return CompletionItemKind.Reference; // Using Reference for Alias
  }

  return null;
}
