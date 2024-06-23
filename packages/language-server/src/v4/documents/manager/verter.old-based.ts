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
        debugger;
      }
    }

    let tsService = this.tsServices.get(tsconfigPath);
    if (!tsService) {
      tsService = this.createTsService(tsconfigPath);
      this.tsServices.set(tsconfigPath, tsService);
    }

    return tsService;
  }

  createTsService(tsconfigPath: string) {
    if (!tsconfigPath || !ts.sys.fileExists(tsconfigPath)) {
      console.error("tsconfig not found", tsconfigPath);
      return;
    }

    const workspacePath = fsPath.dirname(tsconfigPath);

    const config = this.fromTsConfigFilepath(tsconfigPath);

    // TODO double check these options
    const compilerOptions: ts.CompilerOptions = {
      ...config.options,
      allowNonTsExtensions: true,
      jsx: ts.JsxEmit.Preserve,
      target: ts.ScriptTarget.ESNext,
      module: ts.ModuleKind.ESNext,
      alwaysStrict: true,
      noImplicitAny: true,
    };

    const moduleCache = ts.createModuleResolutionCache(
      workspacePath,
      (x) => ts.sys.resolvePath(x),
      compilerOptions
    );

    const self = this;

    // TODO fix host
    const host: ts.LanguageServiceHost = {
      log: (message) => console.info(`[ts] ${message}`),
      getCompilationSettings: () => compilerOptions,
      getCurrentDirectory: () => workspacePath,
      getDefaultLibFileName: ts.getDefaultLibFilePath,

      readFile: (filepath, encoding) => {
        if (filepath.endsWith(".vue.tsx")) {
          const snap = this.documentManager.getSnapshotIfExists(filepath);
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
          return !!this.documentManager.getSnapshotIfExists(filepath);
        }

        if (isVerterVirtual(filepath)) {
          return !!this.documentManager.getSnapshotIfExists(filepath);
        }

        return ts.sys.fileExists(filepath);
      },
      getDirectories: ts.sys.getDirectories,
      useCaseSensitiveFileNames: () => ts.sys.useCaseSensitiveFileNames,
      getNewLine: () => ts.sys.newLine,

      readDirectory(path, extensions, exclude, include, depth) {
        return ts.sys.readDirectory(
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
          this.documentManager
            .getSnapshotIfExists(filename)
            ?.version.toString() ?? ""
        );
      },
      getScriptSnapshot: (filename) => {
        // if (filename.endsWith(".vue")) {
        //   debugger;
        // }
        if (filename.endsWith(".vue.tsx")) {
          console.log("sssd");
        }
        return this.documentManager.getSnapshotIfExists(filename);
      },

      getScriptFileNames() {
        //   if (parsedConfig) {
        //     parsedConfig.fileNames;
        //   }

        const files = config?.fileNames ?? [];
        // const map = files.map(uriToVerterVirtual) ?? [];

        const map = files.flatMap((x) => {
          if (isVueFile(x)) {
            let doc = self.documentManager.files.get(x);
            if (!doc) {
              doc = self.documentManager.preloadDocument(x);
            }
            if (isVueDocument(doc)) {
              return [...doc.subDocumentPaths];
            }

            return x;
          }
          return x;
        });

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
        // if (isVerterVirtual(containingFile)) {
        //   containingFile = urlToPath(containingFile)!;
        // }
        // if (isVerterVirtual(containingFile)) {
        //   containingFile = retrieveVueFileFromBlockUri(
        //     uriToPath(containingFile)!
        //   );
        // }
        const modules = moduleLiterals.map((x) => {
          const h = this;
          // const h = ts.sys;

          let moduleLoc = x.text;
          const isVueFileModule = isVueFile(moduleLoc);

          if (isVueFileModule) {
            moduleLoc = vueToBundle(moduleLoc);
          }

          // if (~x.text.indexOf("vue")) {
          //   debugger;
          // }

          // if (moduleLoc.endsWith(".vue")) {
          //   moduleLoc = x.text + ".tsx";
          // }

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
                  if (isVueSubDocument(r.resolvedModule.resolvedFileName)) {
                    const originalPath = retrieveVueFileFromBlockUri(
                      r.resolvedModule.resolvedFileName
                    );
                    // @ts-expect-error
                    r.resolvedModule.originalPath = originalPath;
                    r.resolvedModule.resolvedFileName =
                      uriToVerterVirtual(originalPath);
                    r.resolvedModule.resolvedUsingTsExtension = false;
                  }

                  if (r.resolvedModule.resolvedFileName.endsWith(".vue.tsx")) {
                    // console.log(
                    //   "up resolved",
                    //   r,
                    //   uriToVerterVirtual(
                    //     r.resolvedModule.resolvedFileName.slice(0, -4)
                    //   )
                    // );

                    const originalPath =
                      r.resolvedModule.resolvedFileName.slice(0, -4);
                    // @ts-expect-error
                    r.resolvedModule.originalPath = originalPath;
                    r.resolvedModule.resolvedFileName =
                      uriToVerterVirtual(originalPath);
                    r.resolvedModule.resolvedUsingTsExtension = false;

                    // r.resolvedModule.resolvedFileName =
                    //   r.resolvedModule.resolvedFileName.slice(0, -4);

                    // r.resolvedModule.extension = ".vue";
                  } else {
                    // debugger;
                  }
                } else {
                  // TODO this is a legit issue and should be investigated
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
        if (filename.endsWith(".vue") || filename.endsWith(".vue.tsx")) {
          debugger;
          0;
        }
        return ts.sys.realpath?.(filename) ?? filename;
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

    // / TODO fix host
    const languageService = ts.createLanguageService(
      host,
      ts.createDocumentRegistry(ts.sys.useCaseSensitiveFileNames)
      // ts.LanguageServiceMode.Semantic
    );
    this.tsServices.set(tsconfigPath, languageService);

    return languageService;
  }

  fromTsConfigFilepath(tsconfigPath: string) {
    let content = "";
    const file = this.documentManager.getSnapshotIfExists(tsconfigPath);
    if (file) {
      content = file.getText(0, file.getLength());
    } else {
      content = this.documentManager.readFile(tsconfigPath);
    }
    return this.fromTsConfig(content, tsconfigPath);
  }

  fromTsConfig(tsconfigContent: string, configPath: string) {
    const tsconfig = ts.parseConfigFileTextToJson(configPath, tsconfigContent);

    if (tsconfig.error) {
      console.error(tsconfig.error);
      throw tsconfig.error;
      return;
    }

    // TODO move to use this.documentManager
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

    const config = ts.parseJsonConfigFileContent(
      tsconfig.config,
      parseConfigHost,
      fsPath.dirname(configPath)
    );

    return config;
  }
}
