import type LSP from "vscode-languageserver-protocol";
import ts, { sortAndDeduplicateDiagnostics } from "typescript";
import {
  isVerterVirtual,
  isVueFile,
  uriToPath,
  uriToVerterVirtual,
  vueToBundle,
} from "../utils";
import { DocumentManager, isVueDocument } from "./document.js";
import fsPath from "path";
import { urlToPath } from "../../../utils";
import {
  isFileInVueBlock,
  isVueSubDocument,
  retrieveVueFileFromBlockUri,
} from "../../processor/utils";

export class VerterManager {
  /**
   * A map of ts.LanguageService instances keyed by tsconfig folder.
   */
  readonly tsServices = new Map<string, ts.LanguageService>();

  readonly folderToTsConfigMap = new Map<string, string>();

  readonly documentRegistry = ts.createDocumentRegistry(
    ts.sys.useCaseSensitiveFileNames
  );

  /**
   * A map of tsconfig filepaths to ts.ParsedCommandLine instances.
   */
  readonly tsConfigMap = new Map<string, ts.ParsedCommandLine>();

  constructor(private readonly documentManager: DocumentManager) {}

  init(params: {
    workspaceFolders?: Array<{ uri: LSP.URI; name: string }>;
    rootUri: LSP.DocumentUri | null;
  }) {
    // this.workspaceFolders = params.workspaceFolders;

    if (params.workspaceFolders) {
      for (const workspace of params.workspaceFolders) {
        const filepath = uriToPath(workspace.uri);

        this.getTsService(filepath);
      }
    }
  }

  getTsService(folderOrPath: string) {
    let folder = folderOrPath;
    let tsconfigPath = this.folderToTsConfigMap.get(folder);
    if (!tsconfigPath) {
      folder = uriToPath(folderOrPath) ?? folderOrPath;
      tsconfigPath = ts.findConfigFile(folder, ts.sys.fileExists);

      if (!tsconfigPath) {
        folder = fsPath.dirname(folder);
        if (!tsconfigPath) {
          tsconfigPath = ts.findConfigFile(folder, ts.sys.fileExists);
        }
      }
      if (tsconfigPath) {
        this.folderToTsConfigMap.set(folderOrPath, tsconfigPath);
      } else {
        // tsconfig should have resolved by now
        tsconfigPath = process.cwd();
      }
    }

    let tsService = this.tsServices.get(tsconfigPath!);
    if (!tsService) {
      tsService = getTypescriptService(
        fsPath.dirname(tsconfigPath),
        this.documentManager
      );
      this.tsServices.set(tsconfigPath!, tsService);
    }

    return tsService;
  }
}

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
  const configFile = fsPath.resolve(workspacePath, "./tsconfig.json");
  const tsconfigFileExists = ts.sys.fileExists(configFile);

  if (tsconfigFileExists) {
    // // TODO watch the config file
    // ts.sys.watchFile!(configFile, (f) => {});

    const tsConfigStr = ts.sys.readFile(
      fsPath.resolve(workspacePath, "./tsconfig.json"),
      "utf-8"
    );

    const { config } = ts.parseConfigFileTextToJson(
      fsPath.resolve(workspacePath, "./tsconfig.json"),
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
    // this is quite damaging for types I reckon
    // when the jsxImportSource is set to vue
    // it will prevent the v-slots patch from working
    delete tsconfigOptions.jsxImportSource;
  }

  const compilerOptions: ts.CompilerOptions = {
    ...tsconfigOptions,
    allowJs: true,
    jsx: ts.JsxEmit.Preserve,
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.NodeNext,
    // jsxFactory: "vue",
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
    log: (message) => console.info(`[ts] ${message}`),
    getCompilationSettings: () => compilerOptions,
    getCurrentDirectory: () => workspacePath,
    getDefaultLibFileName: ts.getDefaultLibFilePath,

    readFile: (filepath, encoding) => {
      // if (isVueSubDocument(filepath)) {
      //   debugger;
      // }
      if (filepath.endsWith(".vue.bundle.ts") || isVueSubDocument(filepath)) {
        const snap = documentManager.getSnapshotIfExists(filepath);
        return snap?.getText(0, snap.getLength());
      }
      if (isVerterVirtual(filepath)) {
        filepath = urlToPath(filepath)!;
      }
      if (filepath.endsWith(".vue") || filepath.endsWith(".vue.bundle.ts")) {
        debugger;
        6;
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
      if (filepath.endsWith(".vue.bundle.ts") || isVueSubDocument(filepath)) {
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
      let ext = fileName.slice(fileName.lastIndexOf("."));
      if (fileName.endsWith(".vue.options.ts")) {
        const doc = documentManager.getDocument(fileName);
        if (isVueDocument(doc)) {
          ext = doc.getDocument(fileName).extension;
        }
      }
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
        case ".vue": {
          return ts.ScriptKind.TS;
        }
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
      // if (isVueSubDocument(filename)) {
      //   debugger;
      // }
      if (filename.endsWith(".vue")) {
        console.log("vvv");
      }
      if (filename.endsWith(".vue.bundle.ts")) {
        console.log("sssd");
      }
      return documentManager.getSnapshotIfExists(filename);
    },

    getScriptFileNames() {
      //   if (parsedConfig) {
      //     parsedConfig.fileNames;
      //   }

      const files = tsconfigFileExists
        ? parsedConfig?.fileNames ?? []
        : Array.from(documentManager.files.values()).map((x) => x.uri);

      const map =
        files.flatMap((x) => {
          let doc = documentManager.files.get(x);

          if (!doc) {
            doc = documentManager.preloadDocument(x);
          }
          if (isVueDocument(doc)) {
            return doc.subDocumentPaths;
          }
          return x;
          const r = uriToVerterVirtual(x);
          if (r.startsWith("verter-virtual")) {
            return r + ".bundle.ts";
          }
          return r;
        }) ?? [];

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
        containingFile = uriToPath(containingFile)!;
      }
      if (containingFile.endsWith(".vue")) {
        // containingFile =
        //   retrieveVueFileFromBlockUri(containingFile) + ".bundle.ts";
        containingFile += ".bundle.ts";
      }
      const modules = moduleLiterals.map((x) => {
        const h = this;
        // const h = ts.sys;

        let moduleLoc = x.text;

        // if (~x.text.indexOf("vue")) {
        //   debugger;
        // }

        if (moduleLoc.endsWith(".vue")) {
          moduleLoc = x.text + ".bundle.ts";
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
                if (
                  r.resolvedModule.resolvedFileName.endsWith(".vue.bundle.ts")
                ) {
                  // console.log(
                  //   "up resolved",
                  //   r,
                  //   uriToVerterVirtual(
                  //     r.resolvedModule.resolvedFileName.slice(0, -4)
                  //   )
                  // );

                  const originalPath = retrieveVueFileFromBlockUri(
                    r.resolvedModule.resolvedFileName
                  );
                  // @ts-expect-error
                  r.resolvedModule.originalPath = originalPath;
                  r.resolvedModule.resolvedFileName = uriToVerterVirtual(
                    r.resolvedModule.resolvedFileName
                  );

                  // r.resolvedModule.resolvedFileName =
                  //   uriToVerterVirtual(originalPath);
                  r.resolvedModule.resolvedUsingTsExtension = false;

                  // r.resolvedModule.resolvedFileName =
                  //   r.resolvedModule.resolvedFileName.slice(0, -4);

                  // r.resolvedModule.extension = ".vue";
                } else {
                  debugger;
                }
              } else {
                console.log("no resolved module", containingFile, x.text);
                // debugger;
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
      if (filename.endsWith(".vue") || filename.endsWith(".vue.bundle.ts")) {
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
    //             sourceFile.fileName.endsWith(".vue.bundle.ts")
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
