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
  //   workspaceFolders: string[];

  /**
   * A map of ts.LanguageService instances keyed by tsconfig folder.
   */
  readonly tsServices = new Map<string, ts.LanguageService>();
  readonly documentRegistry = ts.createDocumentRegistry(
    ts.sys.useCaseSensitiveFileNames
  );

  constructor(private _documentManager: DocumentManager) {
    this._documentManager.onDocumentOpen((doc) => {
      const uri = doc.uri;
      const filepath = uriToPath(uri);
      const parent = fsPath.dirname(filepath);
      this.loadConfig(parent);
    });
  }

  init(params: {
    workspaceFolders?: Array<{ uri: LSP.URI; name: string }>;
    rootUri: LSP.DocumentUri | null;
  }) {
    // this.workspaceFolders = params.workspaceFolders;

    if (params.workspaceFolders) {
      for (const workspace of params.workspaceFolders) {
        const filepath = uriToPath(workspace.uri);

        this.loadConfig(filepath);
      }
    }
  }

  loadConfig(folderPath: string, force = false) {
    // TODO maybe we could support a config file to specify the tsconfig path
    const configPath = ts.findConfigFile(folderPath, ts.sys.fileExists);
    const configFolder = fsPath.dirname(configPath);
    let tsService = this.tsServices.get(configFolder);
    if (tsService && !force) {
      return tsService;
    }

    let options = this.defaultOptions();

    let finalConfig: ts.ParsedCommandLine | null = null;

    if (configPath) {
      const tsConfigStr = this._documentManager.readFile(configPath);

      const parsedConfig = ts.parseConfigFileTextToJson(
        configPath,
        tsConfigStr
      );

      const parseConfigHost: ts.ParseConfigHost = {
        useCaseSensitiveFileNames: ts.sys.useCaseSensitiveFileNames,
        fileExists: this._documentManager.fileExists,
        readFile: this._documentManager.readFile,
        readDirectory(root, extensions, excludes, includes, depth) {
          return ts.sys.readDirectory(
            root,
            [...extensions, ".vue"],
            excludes,
            includes,
            depth
          );
        },
      };
      finalConfig = ts.parseJsonConfigFileContent(
        parsedConfig.config,
        parseConfigHost,
        configFolder
      );

      console.log("final ", finalConfig.options);
      options = {
        ...options,
        ...finalConfig.options,
        jsxImportSource: "vue",
        moduleResolution: ts.ModuleResolutionKind.NodeNext,
      };

      finalConfig.fileNames.forEach((file) => {
        this._documentManager.preloadDocument(file);
      });

      // TODO handle reference tsconfig
      // finalConfig.projectReferences
    }

    console.log("config", configPath, options);

    const matched = new Map<string, number>();

    function debounce(func, delay) {
      let timeoutId;

      return function (...args) {
        if (timeoutId) {
          clearTimeout(timeoutId);
        }

        timeoutId = setTimeout(() => {
          func.apply(this, args);
        }, delay);
      };
    }

    const log = debounce(() => {
      Object.entries(matched)
        .filter(([_, v]) => v > 1)
        .forEach(([k, v]) => {
          console.log("matched multiple times", k, v);
        });
      matched.clear();
    }, 1000);
    const moduleCache = ts.createModuleResolutionCache(
      configFolder,
      (e) => {
        // console.log("p", e);
        // console.log("pp", ts.sys.resolvePath(e));

        // return ts.sys.resolvePath(e);
        const r = ts.sys.resolvePath(e);
        let c = matched.get(r);
        if (c === undefined) {
          c = 0;
        }
        matched.set(r, c + 1);
        log();

        return r;
      },
      options
    );

    const self = this;

    const host = {
      log: (message) => console.log("[TS]", message),
      getDefaultLibFileName: ts.getDefaultLibFilePath,
      getDirectories: ts.sys.getDirectories,
      useCaseSensitiveFileNames: () => ts.sys.useCaseSensitiveFileNames,
      getNewLine: () => ts.sys.newLine,
      realpath: (filepath) => {
        if (isVerterVirtual(filepath)) {
          return uriToPath(filepath);
        }
        if (filepath.indexOf(".vue") > 0) {
          debugger;
        }
        return ts.sys.realpath(filepath);
      },

      readDirectory: (root, extensions, excludes, includes, depth) => {
        return ts.sys.readDirectory(
          root,
          [...extensions, ".vue"],
          excludes,
          includes,
          depth
        );
      },
      getScriptKind: (filename) => {
        const ext = filename.slice(filename.lastIndexOf("."));
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
          case ".vue":
            return ts.ScriptKind.TSX;
          default:
            console.log("unknown", ext);
            return ts.ScriptKind.Unknown;
        }
      },
      readFile: (filepath, encoding) => {
        return this._documentManager.readFile(filepath, encoding);
      },
      fileExists: (filepath) => {
        return this._documentManager.fileExists(filepath);
      },
      getScriptVersion: (fileName) => {
        const snap = this._documentManager.getSnapshotIfExists(fileName);
        return snap.version?.toString();
      },
      getScriptSnapshot: (fileName) => {
        return this._documentManager.getSnapshotIfExists(fileName);
      },

      resolveModuleNameLiterals(
        moduleLiterals,
        containingFile,
        redirectedReference,
        options,
        containingSourceFile,
        reusedNames
      ) {
        const modules = moduleLiterals.map((x) => {
          const h = this;

          let moduleLoc = x.text;

          const isVueFileModule = isVueFile(moduleLoc);

          if (isVueFileModule) {
            moduleLoc = vueToBundle(moduleLoc);
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

              if (isVueFileModule) {
              }
              if (x.text.endsWith(".vue")) {
                if (r.resolvedModule) {
                  if (isVueSubDocument(r.resolvedModule.resolvedFileName)) {
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
                    r.resolvedModule.resolvedFileName =
                      uriToVerterVirtual(originalPath);
                    // r.resolvedModule.resolvedFileName =
                    //   uriToVerterVirtual(originalPath);
                    r.resolvedModule.resolvedUsingTsExtension = false;

                    r.resolvedModule.extension = ".vue";

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
        if (false) {
          const emptyModules = modules
            .map((x, i) => (!x.resolvedModule ? i : false))
            .filter((x) => x !== false)
            .map((x: number) => moduleLiterals[x].text);

          if (emptyModules.length > 0) {
            console.warn("Modules missing!", emptyModules);
          }
        }

        console.log("mmmmm", modules.length);

        return modules;
      },
      getCurrentDirectory: () => configFolder,
      getCompilationSettings: () => options,

      getScriptFileNames() {
        const files = finalConfig.fileNames.flatMap((filename) => {
          const doc = self._documentManager.files.get(filename);
          if (isVueDocument(doc)) {
            // return uriToVerterVirtual(doc.uri);
            return [...doc.subDocumentPaths];
            // return doc.bundleDoc.uri;
          }
          return filename;
        });

        return files;
      },
    } satisfies ts.LanguageServiceHost;

    // const host: ts.LanguageServiceHost = Object.assign(
    //   this.defaultServiceHost(moduleCache),
    //   {
    //     getCurrentDirectory: () => configFolder,
    //     getCompilationSettings: () => options,

    //     getScriptFileNames() {
    //       const files = finalConfig.fileNames.flatMap((filename) => {
    //         const doc = self._documentManager.files.get(filename);
    //         if (isVueDocument(doc)) {
    //           // return uriToVerterVirtual(doc.uri);
    //           return [...doc.subDocumentPaths];
    //           // return doc.bundleDoc.uri;
    //         }
    //         return filename;
    //       });

    //       return files;
    //     },
    //   }
    // );

    const languageService = ts.createLanguageService(
      host,
      // this.documentRegistry
      ts.createDocumentRegistry(ts.sys.useCaseSensitiveFileNames)
    );

    this.tsServices.set(configFolder, languageService);
    return languageService;
  }

  retrieveService(uri: string) {
    const filepath = uriToPath(uri);
    const parent = fsPath.dirname(filepath);
    return this.loadConfig(parent);
  }

  protected defaultOptions(): ts.CompilerOptions {
    return {
      // jsx: ts.JsxEmit.Preserve,
      //   allowJs: true,
      //   noImplicitThis: false,
      //   noImplicitReturns: false,
      //   target: ts.ScriptTarget.ESNext,
      //   module: ts.ModuleKind.ESNext,
      //   alwaysStrict: true,
      //   noImplicitAny: true,
      jsx: ts.JsxEmit.Preserve,
      // target: ts.ScriptTarget.ESNext,
      // module: ts.ModuleKind.ESNext,
      alwaysStrict: true,
      noImplicitAny: true,
      allowJs: true,
      noImplicitThis: false,
      noImplicitReturns: false,
    };
  }
  protected defaultServiceHost(moduleCache: ts.ModuleResolutionCache) {
    const manager = this;

    return {} satisfies Partial<ts.LanguageServiceHost>;
  }
}
