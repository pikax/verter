import type LSP from "vscode-languageserver-protocol";
import { WorkspaceFolder } from "vscode-languageserver/node";
import vscode from "vscode-languageserver";
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
  normalisePath,
  pathToUri,
  isVerterVirtual,
} from "../../utils";
import { VueDocument } from "../vue";
import { VueTypescriptDocument } from "../vue/sub";

import { globSync } from "glob";
import { minimatch } from "minimatch";
import { resolveGlobalYarnPath } from "vscode-languageserver/lib/node/files";

const ConfigHost: ts.ParseConfigHost = {
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

export class VerterManager {
  /**
   * A map of ts.LanguageService instances keyed by tsconfig folder.
   */
  readonly tsServices = new Map<string, ts.LanguageService>();

  readonly folderToTsConfigMap = new Map<string, string>();

  readonly newTsServices = new Map<string, ts.LanguageService>();

  readonly documentRegistry = ts.createDocumentRegistry(
    ts.sys.useCaseSensitiveFileNames
  );

  /**
   * A map of tsconfig filepaths to ts.ParsedCommandLine instances.
   */
  readonly tsConfigMap = new Map<string, ts.ParsedCommandLine>();

  constructor(private readonly documentManager: DocumentManager) {}

  workspaceFolders: Array<WorkspaceFolder> = [];

  init(params: {
    workspaceFolders?: Array<WorkspaceFolder> | null | undefined;
    // rootUri: LSP.DocumentUri | null;
  }) {
    // this.workspaceFolders = params.workspaceFolders;
    if (params.workspaceFolders) {
      this.workspaceFolders = params.workspaceFolders;
      for (const workspace of params.workspaceFolders) {
        const filepath = uriToPath(workspace.uri);
        //   const s = this.getTsService(filepath, true);
        //   // no default found, we should check if there's tsconfig files there
        //   if (!s) {
        //     const found = globSync("**/tsconfig.json", {
        //       ignore: "node_modules",
        //       cwd: filepath,
        //       absolute: true,
        //     });

        //     found.map((f) => this.getTsService(f, true));
        //   }

        this.findTsServices(filepath);
      }
    }
  }

  getTsService(folderOrPath: string, tsSystem = ts.sys): ts.LanguageService {
    folderOrPath = normalisePath(folderOrPath);

    let f = this.folderToTsConfigMap.get(folderOrPath);
    if (!f) {
      for (const [key, v] of this.newTsServices) {
        if (
          minimatch(folderOrPath, key, {
            nocase: !tsSystem.useCaseSensitiveFileNames,
          })
        ) {
          this.folderToTsConfigMap.set(folderOrPath, key);
          return v;
        }
      }
      console.warn("tsconfig not found");
      throw new Error("tsconfgig not found");
    }

    return this.newTsServices.get(f)!;
  }

  findTsServices(root: string) {
    const paths = globSync("**/tsconfig.json", {
      ignore: "node_modules",
      cwd: root,
      absolute: true,
    });

    // const tsConfigs = paths.map((x) => resolveTSConfig(x));

    for (const tsconfig of paths.map((x) => resolveTSConfig(x))) {
      if (!tsconfig) continue;

      if (tsconfig.projectReferences && tsconfig.projectReferences.length) {
        const referencedTsConfigs = tsconfig.projectReferences.map((x) =>
          resolveTSConfig(x.path)
        );

        console.log("f r", referencedTsConfigs);

        for (const c of referencedTsConfigs) {
          if (
            c?.raw?.include &&
            c.raw.include.some((x: string) => x.endsWith(".vue"))
          ) {
            if (!c.wildcardDirectories) continue;
            Object.entries(c.wildcardDirectories).map(([dir, recursive]) => {
              const d = normalisePath(dir);
              this.newTsServices.set(
                `${d}${recursive ? "/**" : ""}`,
                createService(d, c, this.documentManager)
              );
            });
          }
        }
      }
      if (
        tsconfig?.raw?.include &&
        tsconfig.raw.include.some((x: string) => x.endsWith(".vue"))
      ) {
        if (!tsconfig.wildcardDirectories) continue;
        Object.entries(tsconfig.wildcardDirectories).map(([dir, recursive]) => {
          const d = normalisePath(dir);
          this.newTsServices.set(
            `${d}${recursive ? "/**" : ""}`,
            createService(d, tsconfig, this.documentManager)
          );
        });
      }
    }
  }
}

function resolveTSConfig(fp: string, host: ts.ParseConfigHost = ConfigHost) {
  if (!ts.sys.fileExists(fp)) return null;

  const tsConfigStr = ts.sys.readFile(fp, "utf-8");
  const { config, error } = ts.parseConfigFileTextToJson(fp, tsConfigStr!);
  if (error) {
    throw error;
  }

  const parsed = ts.parseJsonConfigFileContent(
    config,
    host,
    fsPath.dirname(fp)
  );
  return parsed;
}

function createService(
  tsconfigPath: string,
  config: ts.ParsedCommandLine,
  documentManager: DocumentManager,
  tsSystem = ts.sys
) {
  const options: ts.CompilerOptions = {
    strict: true,
    jsx: ts.JsxEmit.Preserve,
    ...config.options,

    // it breaks the slots patch, if is 'vue'
    jsxImportSource: undefined,
  };

  const dir = fsPath.dirname(tsconfigPath);
  const moduleCache = ts.createModuleResolutionCache(
    dir,
    (e) => {
      return tsSystem.resolvePath(e);
    },
    options
  );

  const host: ts.LanguageServiceHost = {
    log: (message) => console.info(`[ts] ${message}`),
    getCompilationSettings: () => options,
    getCurrentDirectory: () => dir,
    getDefaultLibFileName: ts.getDefaultLibFilePath,

    readFile: (filepath, encoding) => {
      return documentManager.readFile(
        normalisePath(filepath),
        encoding as BufferEncoding
      );
    },
    fileExists: (filepath) => {
      if (filepath.endsWith("/react/index.d.ts")) return false;

      return documentManager.fileExists(normalisePath(filepath));
    },
    getDirectories: ts.sys.getDirectories,
    useCaseSensitiveFileNames: () => tsSystem.useCaseSensitiveFileNames,
    getNewLine: () => tsSystem.newLine,

    readDirectory(path, extensions, exclude, include, depth) {
      console.log("reading dir", path);
      return tsSystem.readDirectory(
        path,
        [...(extensions ?? []), ".vue"],
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
      const files = config.fileNames;

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
      const realContainingFile = isVueSubDocument(containingFile)
        ? uriToPath(toVueParentDocument(containingFile))
        : containingFile;

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
                  // @ts-expect-error not part of the object
                  r.resolvedModule.originalPath = originalPath;
                  r.resolvedModule.resolvedFileName = uriToVerterVirtual(
                    r.resolvedModule.resolvedFileName
                  );
                  r.resolvedModule.resolvedUsingTsExtension = false;
                }
              }
            }

            // because we use the virtual file to find the required dep, we need to convert to the actual path
            if (r.resolvedModule) {
              if (
                isVerterVirtual(r.resolvedModule.resolvedFileName) &&
                !isVueSubDocument(r.resolvedModule.resolvedFileName)
              ) {
                r.resolvedModule.resolvedFileName = uriToPath(
                  r.resolvedModule.resolvedFileName
                );
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
      if (isVerterVirtual(filename)) {
        filename = uriToPath(filename);
      }

      return tsSystem.realpath?.(filename) ?? filename;
    },
  };

  return ts.createLanguageService(
    host,
    ts.createDocumentRegistry(tsSystem.useCaseSensitiveFileNames)
  );
}

// const ServiceMap = new WeakMap<ts.LanguageService, ts.ParsedCommandLine>();

function getTypescriptServiceOld(
  workspacePath: string,
  documentManager: DocumentManager,
  absolute = false
): ts.LanguageService | ts.LanguageService[] {
  let tsconfigOptions: ts.CompilerOptions = {
    // allowJs: true,
    // noImplicitAny: true,
    // noImplicitThis: true,
    // noImplicitReturns: true,
    // declaration: true,
    // strict: true,
    // alwaysStrict: true,

    // verbatimModuleSyntax: true,

    // target: ts.ScriptTarget.ESNext,
    // esModuleInterop: true,
    // forceConsistentCasingInFileNames: true,
    // skipLibCheck: true,

    strict: true,

    jsxFactory: "vue",
  };

  let parsedConfig: ts.ParsedCommandLine | null = null;
  const configFile = absolute
    ? workspacePath
    : fsPath.resolve(workspacePath, "./tsconfig.json");
  const tsconfigFileExists = ts.sys.fileExists(configFile);

  if (tsconfigFileExists) {
    // // TODO watch the config file
    // ts.sys.watchFile!(configFile, (f) => {});

    let finalConfig = undefined as any | undefined;
    let mainTsConfigPath = configFile;

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

    const loadConfig = (path: string) => {
      const tsConfigStr = ts.sys.readFile(path, "utf-8");
      const { config } = ts.parseConfigFileTextToJson(path, tsConfigStr!);
      return config;
    };
    const c = loadConfig(mainTsConfigPath);
    if (c.references) {
      const configs = [] as ts.LanguageService[];
      try {
        for (const { path } of c.references) {
          if (path) {
            const fp = fsPath.resolve(workspacePath, path);
            // const bp = fsPath.dirname(fp);
            const config = loadConfig(fp);

            // const r = ts.parseJsonConfigFileContent(config, parseConfigHost, bp);
            const r = getTypescriptServiceOld(fp, documentManager, true);

            if (Array.isArray(r)) {
              configs.push(...config);
            } else {
              configs.push(config);
            }

            // console.log("parsed", parsed);

            // yield* getTypescriptService(fp, documentManager, true);

            // if (r.include.some((x: string) => x.endsWith(".vue"))) {
            //   finalConfig = r;
            // }
          }
        }
      } catch (e) {
        console.error(e);
      }
      return configs;
    } else {
      finalConfig = c;
    }

    const config = finalConfig;

    parsedConfig = ts.parseJsonConfigFileContent(
      config,
      parseConfigHost,
      absolute ? fsPath.dirname(workspacePath) : workspacePath
    );

    tsconfigOptions = {
      ...parsedConfig.options,

      // it breaks the slots patch, if is 'vue'
      jsxImportSource: undefined,

      // allowArbitraryExtensions: true,
      // allowJs: true,
      // noImplicitAny: false,
      // noImplicitThis: false,
      // noImplicitReturns: false,
    };
    // this is quite damaging for types I reckon
    // when the jsxImportSource is set to vue
    // it will prevent the v-slots patch from working
    // delete tsconfigOptions.jsxImportSource;
  }

  const compilerOptions: ts.CompilerOptions = {
    ...tsconfigOptions,
    // allowJs: true,
    jsx: ts.JsxEmit.Preserve,
    // declaration: false,
    // jsxFactory: 'vue'
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
      return documentManager.readFile(
        normalisePath(filepath),
        encoding as BufferEncoding
      );
    },
    fileExists: (filepath) => {
      return documentManager.fileExists(normalisePath(filepath));
    },
    getDirectories: ts.sys.getDirectories,
    useCaseSensitiveFileNames: () => tsSystem.useCaseSensitiveFileNames,
    getNewLine: () => tsSystem.newLine,

    readDirectory(path, extensions, exclude, include, depth) {
      console.log("reading dir", path);
      return tsSystem.readDirectory(
        path,
        [...(extensions ?? []), ".vue"],
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
      const realContainingFile = isVueSubDocument(containingFile)
        ? uriToPath(toVueParentDocument(containingFile))
        : containingFile;

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
                  // @ts-expect-error not part of the object
                  r.resolvedModule.originalPath = originalPath;
                  r.resolvedModule.resolvedFileName = uriToVerterVirtual(
                    r.resolvedModule.resolvedFileName
                  );
                  r.resolvedModule.resolvedUsingTsExtension = false;
                }
              }
            }

            // because we use the virtual file to find the required dep, we need to convert to the actual path
            if (r.resolvedModule) {
              if (
                isVerterVirtual(r.resolvedModule.resolvedFileName) &&
                !isVueSubDocument(r.resolvedModule.resolvedFileName)
              ) {
                r.resolvedModule.resolvedFileName = uriToPath(
                  r.resolvedModule.resolvedFileName
                );
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
      if (isVerterVirtual(filename)) {
        filename = uriToPath(filename);
      }

      return tsSystem.realpath?.(filename) ?? filename;
    },
  };
  const languageService = ts.createLanguageService(
    host,
    ts.createDocumentRegistry(tsSystem.useCaseSensitiveFileNames)
  );

  return languageService;
}
