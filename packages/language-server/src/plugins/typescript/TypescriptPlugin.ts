import ts, { ResolvedModuleFull, ScriptKind, version } from "typescript";
import Logger from "../../logger";
import { documentManager } from "../../lib/documents/Manager";
import { readFileSync, existsSync } from "fs";
import { VueDocument } from "../../lib/documents/VueDocument";
import { findTsConfigPath } from "../../utils";

import { VirtualFiles } from "@verter/language-shared";

import path from "node:path";
import { resolve } from "vscode-languageserver/lib/node/files";

function getScriptFileNames(resolvedFileNames: string[]) {
  const d = documentManager.getAllOpened();

  const vueFiles = [...d, ...resolvedFileNames]
    .filter((x) => x.endsWith(".vue"))
    .map((x) =>
      x.startsWith("file:")
        ? x.replace("file:", "verter-virtual:")
        : `verter-virtual:///${x.replace(":", "%3A")}`
    )
    // .map((x) => x.replace("%3A", ":"))
    .flatMap((x) => [
      x + ".tsx",
      // x + '.template.d.ts',
      // TODO add more blocks
      // x + '.css'
    ]);

  // const r = Array.from(
  //   new Set(d.concat(vueFiles).concat(resolvedFileNames)).values()
  // );

  return vueFiles.concat(resolvedFileNames);
}

const map = {
  "/vue.ts": "vue",
  "@vue/runtime-dom.ts": "@vue/runtime-dom",
  "@vue/runtime-core.ts": "@vue/runtime-core",
  "@vue/reactivity.ts": "@vue/reactivity",
  "/vue/jsx-runtime.ts": "vue/jsx-runtime",
  "@vue/shared.ts": "@vue/shared",
};

function getSnapshotIfExists(
  fileName: string
): (ts.IScriptSnapshot & { version: number }) | undefined {
  const doc = documentManager.getDocument(fileName);
  if (!doc) {
    console.log("no doc found", fileName);
    return undefined;
  }
  let snap = snapshots.get(fileName);
  if (snap?.version === doc.version) {
    return snap;
  }

  // snap = { ...ts.ScriptSnapshot.fromString(doc.template?.content ?? doc.getText()), version: doc.version }
  snap = Object.assign(
    ts.ScriptSnapshot.fromString(doc.template?.content ?? doc.getText()),
    { version: doc.version }
  );
  snapshots.set(fileName, snap);

  console.log("update snapshot for", snap.version, fileName);

  return snap;
}

function fileExists(fileName: string) {
  const e = !!documentManager.getDocument(fileName);
  console.log("checking if file exists", fileName);
  if (~fileName.indexOf(".vue") || fileName.endsWith(".vue")) {
    if (fileName.endsWith(".vue")) {
      debugger;
    }
  }

  // these are virtual files
  if (fileName.startsWith("verter:")) {
    const name = fileName.slice("verter:".length);
    return name in VirtualFiles;
  }

  if (fileName.startsWith("verter-virtual:")) {
    if (snapshots.has(fileName)) {
      return true;
    }

    const d = Object.keys(map).find((x) => fileName.endsWith(x));
    if (d) {
      return true;
    }

    if (
      fileName.endsWith("/vue.ts") ||
      fileName.endsWith("/vue/jsx-runtime.ts")
    ) {
      return true;
    }

    fileName = fileName.replace("verter-virtual:", "file:");
    console.log("updated fileName to", fileName);
    // debugger
  }

  if (fileName.startsWith("virtual:")) {
    debugger;
  }

  return e || snapshots.has(fileName) || ts.sys.fileExists(fileName);
}

function readFile(fileName: string) {
  console.log("reading file", fileName);

  if (fileName.startsWith("verter:")) {
    const name = fileName.slice("verter:".length);
    return VirtualFiles[name];
  }

  // if (fileName.endsWith('/vue') || fileName.endsWith('/vue/jsx-runtime')) {
  //   return true
  // }

  // if (fileName.startsWith("verter-virtual:")) {
  //   debugger;
  // }
  return ts.sys.readFile(fileName);

  //   if (fileExists(fileName)) {
  //   }
  //   const doc = documentManager.getDocument(fileName);
  //   return doc?.getText() ?? "";
}

const snapshots = new Map<string, ts.IScriptSnapshot & { version: number }>();

export function getTypescriptService(workspacePath: string) {
  // console.log('current workspace ', workspacePath)

  // const dd = findTsConfigPath(path.resolve(workspacePath, './tsconfig.json'), [workspacePath], existsSync, e => e);

  let tsconfigOptions: ts.CompilerOptions = {
    allowJs: true,
    noImplicitAny: false,
    noImplicitThis: false,
    noImplicitReturns: false,
  };
  let fileNames: string[] = [];
  const configFile = path.resolve(workspacePath, "./tsconfig.json");

  if (fileExists(configFile)) {
    const tsConfigStr = readFileSync(
      path.resolve(workspacePath, "./tsconfig.json"),
      "utf-8"
    );

    const { config } = ts.parseConfigFileTextToJson(
      path.resolve(workspacePath, "./tsconfig.json"),
      tsConfigStr
    );

    const parseConfigHost: ts.ParseConfigHost = {
      ...ts.sys,
      readDirectory: (rootDir, extensions, excludes, includes, depth) => {
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

    // const config = ts.parseJsonConfigFileContent(
    //   config,
    //   parseConfigHost,
    //   workspacePath
    // );

    const parsedConfig = ts.parseJsonConfigFileContent(
      config,
      parseConfigHost,
      workspacePath
    );

    parsedConfig.wildcardDirectories;
    tsconfigOptions = parsedConfig.options;
    fileNames = parsedConfig.fileNames;
  }

  // const fileCompilerOptions = ts.parseJsonConfigFileContent(
  //   config,
  //   parseConfigHost,
  //   workspacePath
  // ).options;

  const compilerOptions: ts.CompilerOptions = {
    // moduleResolution: ts.ModuleResolutionKind.Bundler,
    jsx: ts.JsxEmit.Preserve,
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    noImplicitAny: true,
    ...tsconfigOptions,
    // jsxFactory: 'vue'
    // // jsxFactory: 'vue'
    // "allowJs": true,
    // "checkJs": true,
    // "strictNullChecks": false,
    // "jsxImportSource": "vue",
    // // "moduleResolution": "Bundler",
    // "allowImportingTsExtensions": true
  };

  const tsSystem = ts.sys;

  const host: ts.LanguageServiceHost = {
    log: (message) => Logger.info(`[ts] ${message}`),
    getCompilationSettings: () => compilerOptions,
    getScriptFileNames: () => getScriptFileNames(fileNames),
    getScriptVersion: (fileName: string) => {
      const snap = getSnapshotIfExists(fileName);
      // if (fileName.indexOf('.vue')) {
      //   debugger
      // }

      return snap?.version?.toString() || "";
    },
    getScriptSnapshot: getSnapshotIfExists,
    getCurrentDirectory: () => workspacePath,
    getDefaultLibFileName: ts.getDefaultLibFilePath,
    fileExists: fileExists,
    readFile: readFile,
    // readDirectory: svelteModuleLoader.readDirectory,
    readDirectory: (path, extensions, exclude, include) => {
      console.log("reading dir", {
        path,
        extensions,
        exclude,
        include,
      });
      return tsSystem.readDirectory(
        path,
        [...extensions, ".vue"],
        exclude,
        include
      );
    },
    getDirectories: tsSystem.getDirectories,
    useCaseSensitiveFileNames: () => tsSystem.useCaseSensitiveFileNames,
    getScriptKind: (fileName: string) => {
      const ext = fileName.slice(fileName.lastIndexOf("."));
      switch (ext.toLowerCase()) {
        case ts.Extension.Js:
          return ts.ScriptKind.JS;
        case ts.Extension.Jsx:
          return ts.ScriptKind.JSX;
        case ts.Extension.Ts:
          return ts.ScriptKind.TS;
        case ts.Extension.Tsx:
          return ts.ScriptKind.TSX;
        case ts.Extension.Json:
          return ts.ScriptKind.JSON;
        default:
          return ts.ScriptKind.Unknown;
      }
    },
    // getProjectVersion: () => projectVersion.toString(),
    getNewLine: () => tsSystem.newLine,

    // resolveTypeReferenceDirectiveReferences:
    //   svelteModuleLoader.resolveTypeReferenceDirectiveReferences,
    // hasInvalidatedResolutions:
    //   svelteModuleLoader.mightHaveInvalidatedResolutions,
    // getModuleResolutionCache: svelteModuleLoader.getModuleResolutionCache,

    resolveModuleNameLiterals(
      moduleLiterals,
      containingFile,
      redirectedReference,
      options,
      containingSourceFile,
      reusedNames
    ) {
      if (containingFile.startsWith("verter-virtual:")) {
        containingFile = decodeURIComponent(
          containingFile.replace("verter-virtual:///", "")
        );
      }

      const modules = moduleLiterals.map((x) => {
        const h = ts.sys;

        if (x.text.startsWith("verter:")) {
          return {
            resolvedModule: {
              extension: "ts",
              resolvedFileName: x.text,
              packageId: {
                name: "verter",
              },
              resolvedUsingTsExtension: true,
              isExternalLibraryImport: true,
            } as ResolvedModuleFull,
          };
        }

        switch (options.moduleResolution) {
          case ts.ModuleResolutionKind.Classic: {
            return ts.resolveModuleName(
              x.text,
              containingFile,
              options,
              h,
              undefined,
              redirectedReference
            );
          }
          case ts.ModuleResolutionKind.NodeJs:
          case ts.ModuleResolutionKind.Node10:
          case ts.ModuleResolutionKind.Node16:
          case ts.ModuleResolutionKind.NodeNext: {
            return ts.nodeModuleNameResolver(
              x.text,
              containingFile,
              options,
              h,
              undefined,
              redirectedReference
            );
          }
          case ts.ModuleResolutionKind.Bundler:
          default:
            return ts.bundlerModuleNameResolver(
              x.text,
              containingFile,
              options,
              h,
              undefined,
              redirectedReference
            );
        }
      });

      // TODO add debug flag
      if (true) {
        const emptyModules = modules
          .map((x, i) => (!x.resolvedModule ? i : false))
          .filter(Boolean)
          .map((x) => moduleLiterals[x].text);
        if (emptyModules.length > 0) {
          console.warn("Modules missing!", emptyModules);
        }
      }

      return modules;
    },
  };

  // ts.readConfigFile()
  const languageService = ts.createLanguageService(
    host,
    ts.createDocumentRegistry(tsSystem.useCaseSensitiveFileNames)
    // ts.LanguageServiceMode.Semantic
  );

  // // Read and add files specified in tsconfig.json to the virtual file system
  // function includeProjectFiles() {
  //   const fileNames = parsedConfig.fileNames;
  //   fileNames.forEach((fileName) => {
  //     const fileContent = readFileSync(fileName, "utf8");
  //     files[fileName] = { version: 0, text: fileContent };
  //   });
  // }

  // includeProjectFiles();

  return languageService;
}

const DEFAULT_REGEXP = /\.vue$/;
const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export const isVue = (fileName: string) => DEFAULT_REGEXP.test(fileName);
export const isRelativeVue = (fileName: string) =>
  isVue(fileName) && isRelative(fileName);
