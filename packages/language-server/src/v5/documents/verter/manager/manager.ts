import type LSP from "vscode-languageserver-protocol";
import ts, { sortAndDeduplicateDiagnostics } from "typescript";
import fsPath from "path";
import { DocumentManager } from "../../manager";
import { TypescriptDocument } from "../typescript/typescript";
import {
  createSubDocumentUri,
  isVueFile,
  isVueSubDocument,
  uriToPath,
  toVueParentDocument,
  uriToVerterVirtual,
} from "../../utils";
import { VueDocument } from "../vue";
import { VueTypescriptDocument } from "../vue/sub";

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
      // allowJs: true,
      // noImplicitAny: false,
      // noImplicitThis: false,
      // noImplicitReturns: false,
    };
    // this is quite damaging for types I reckon
    // when the jsxImportSource is set to vue
    // it will prevent the v-slots patch from working
    delete tsconfigOptions.jsxImportSource;
  }

  const compilerOptions: ts.CompilerOptions = {
    ...tsconfigOptions,
    // allowJs: true,
    jsx: ts.JsxEmit.Preserve,
    declaration: false,
    // target: ts.ScriptTarget.ESNext,
    // module: ts.ModuleKind.NodeNext,
    // // jsxFactory: "vue",
    // alwaysStrict: true,
    // noImplicitAny: true,
    // esModuleInterop: true,
  };

  const tsSystem = ts.sys;

  const moduleCache = ts.createModuleResolutionCache(
    workspacePath,
    (e) => {
      // console.log("p", e);
      // console.log("pp", tsSystem.resolvePath(e));
      return tsSystem.resolvePath(e);
    },
    compilerOptions
  );

  const host: ts.LanguageServiceHost = {
    log: (message) => console.info(`[ts] ${message}`),
    getCompilationSettings: () => compilerOptions,
    getCurrentDirectory: () => workspacePath,
    getDefaultLibFileName: ts.getDefaultLibFilePath,

    readFile: (filepath, encoding) => {
      return documentManager.readFile(filepath, encoding);
    },
    fileExists: (filepath) => {
      return documentManager.fileExists(filepath);
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
      //   if (fileName.endsWith(".vue.options.ts")) {
      //     const doc = documentManager.getDocument(fileName);
      //     if (isVueDocument(doc)) {
      //       ext = doc.getDocument(fileName).extension;
      //     }
      //   }
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
      const doc = documentManager.getDocument(filename);
      return doc?.version?.toString() ?? "";
    },
    getScriptSnapshot: (filename) => {
      const doc = documentManager.getDocument(filename);
      if (doc instanceof TypescriptDocument) {
        return doc.snapshot;
      }
      if (doc instanceof VueDocument) {
        const sub = doc.getSubDoc(filename);
        if (sub && sub instanceof VueTypescriptDocument) {
          return sub.snapshot;
        }
      }
      return undefined;
    },

    getScriptFileNames() {
      const files = tsconfigFileExists
        ? parsedConfig?.fileNames ?? []
        : documentManager.textDocuments.keys();

      return files.flatMap((x) => {
        if (isVueFile(x)) {
          const doc = documentManager.getDocument(x) as VueDocument;
          if (!doc) {
            return createSubDocumentUri(x, "bundle.ts");
          }
          return doc.blocks.map((x) => x.uri);
        }
        return x;
      });
    },

    resolveModuleNameLiterals(
      moduleLiterals,
      containingFile,
      redirectedReference,
      options,
      containingSourceFile,
      reusedNames
    ) {
      if (isVueFile(containingFile)) {
        containingFile = createSubDocumentUri(containingFile, "bundle.ts");
      }
      const modules = moduleLiterals.map((x) => {
        const h = this;
        let moduleLoc = x.text;

        if (isVueFile(moduleLoc)) {
          // .ts is inferred
          moduleLoc = createSubDocumentUri(moduleLoc, "bundle.ts");
        }
        switch (options.moduleResolution) {
          case ts.ModuleResolutionKind.Classic: {
            return ts.resolveModuleName(
              moduleLoc,
              containingFile,
              options,
              ts.sys,
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

            if (isVueFile(x.text)) {
              if (r.resolvedModule) {
                if (
                  r.resolvedModule.resolvedFileName.endsWith(
                    createSubDocumentUri(".vue", "bundle.ts")
                  )
                ) {
                  const originalPath = toVueParentDocument(
                    r.resolvedModule.resolvedFileName
                  );
                  r.resolvedModule.originalPath = originalPath;
                  r.resolvedModule.resolvedFileName = uriToVerterVirtual(
                    r.resolvedModule.resolvedFileName
                  );
                  r.resolvedModule.resolvedUsingTsExtension = false;
                }
              }
            }

            return r;
        }
      });

      // // TODO add debug flag
      // if (true) {
      //   const emptyModules = modules
      //     .map((x, i) => (!x.resolvedModule ? i : false))
      //     .filter((x) => x !== false)
      //     .map((x: number) => moduleLiterals[x].text);
      //   if (emptyModules.length > 0) {
      //     console.warn("Modules missing!", emptyModules);
      //   }
      // }

      return modules;
    },

    realpath: (filename) => {
      return tsSystem.realpath?.(filename) ?? filename;
    },
  };
  const languageService = ts.createLanguageService(
    host,
    ts.createDocumentRegistry(tsSystem.useCaseSensitiveFileNames)
  );

  return languageService;
}
