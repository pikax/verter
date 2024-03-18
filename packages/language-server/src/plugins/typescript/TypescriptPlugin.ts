import ts, { ScriptKind } from "typescript";
import Logger from "../../logger";
import { documentManager } from "../../lib/documents/Manager";
import { readFileSync, existsSync } from "node:fs";
import { VueDocument } from "../../lib/documents/VueDocument";
import { findTsConfigPath } from "../../utils";

import path from 'node:path'
import { resolve } from "vscode-languageserver/lib/node/files";

function getScriptFileNames() {
  const d = documentManager.getAllOpened();

  const vueFiles = d.filter(x => x.endsWith('.vue')).map(x => x.replace('file:', 'verter-virtual:')).flatMap(x => [
    x + '.tsx',
    // x + '.template.d.ts',
    // TODO add more blocks
    // x + '.css'
  ])
  // const vueFiles = []



  return d.concat(vueFiles);
}

const map = {
  '/vue.ts': 'vue',
  '@vue/runtime-dom.ts': '@vue/runtime-dom',
  '@vue/runtime-core.ts': '@vue/runtime-core',
  '@vue/reactivity.ts': '@vue/reactivity',
  '/vue/jsx-runtime.ts': 'vue/jsx-runtime',
  '@vue/shared.ts': '@vue/shared'
}

function getSnapshotIfExists(
  fileName: string
): ts.IScriptSnapshot & { version: number } {
  // if (fileName.endsWith('.vue')) {
  //   fileName = fileName.replace('.vue', '.ts')
  // }

  // if(fileName.endsWith(''))
  // fileName = fileName.replace('Test.vue.d.tsx', 'test.ts')

  // if (fileName.startsWith("file:///")) {
  //   fileName = decodeURIComponent(
  //     fileName.replace("Test.vue", "Test.my.ts").replace("file:///", "")
  //   );
  // }


  if (snapshots.has(fileName)) {
    return snapshots.get(fileName);
  }

  const d = Object.keys(map).find(x => fileName.endsWith(x))

  if (d) {
    const name = map[d];
    const { resolvedModule } = ts.nodeModuleNameResolver(name, './test.ts', {}, ts.sys)
    if (resolvedModule) {
      console.log('ddasdasd', resolvedModule)
      const f = ts.sys.readFile(resolvedModule.resolvedFileName, 'utf-8')
      const snap = ts.ScriptSnapshot.fromString(f);
      snapshots.set(fileName, snap)
      return snap
    }
  }


  // if (fileName.endsWith('/vue.ts') || fileName.endsWith('/vue/jsx-runtime.ts')) {




  //   const name = fileName.endsWith('/vue.ts') ? 'vue' : 'vue/jsx-runtime';
  //   const { resolvedModule } = ts.nodeModuleNameResolver(name, './test.ts', {}, ts.sys)
  //   if (resolvedModule) {
  //     console.log('ddasdasd', resolvedModule)
  //     const f = ts.sys.readFile(resolvedModule.resolvedFileName, 'utf-8')
  //     const snap = ts.ScriptSnapshot.fromString(f);
  //     snapshots.set(fileName, snap)
  //     return snap
  //   }

  // }

  if (fileName.startsWith('verter-virtual:')) {
    const doc = documentManager.getDocument(fileName)!

    const snap = ts.ScriptSnapshot.fromString(doc.template.content);
    snapshots.set(fileName, snap)

    return snap;
  }


  const doc = documentManager.getDocument(fileName);

  let text = doc?.getText() ?? readFileSync(fileName, "utf-8");
  if (fileName.endsWith("Test.my.ts")) {
    text = `atemplate.



declare const atemplate = { 
    a: '1'
} `;
  }
  const snap = ts.ScriptSnapshot.fromString(text);
  snap.version = doc?.version ?? 0;

  snapshots.set(fileName, snap);
  if (doc) {
    snapshots.set(doc?._uri, snap)
  }
  return snap;
}

function fileExists(fileName: string) {
  const e = !!documentManager.getDocument(fileName);
  if (fileName.endsWith("Test.my.ts")) {
    debugger;
    return true;
  }
  console.log("checking if file exists", fileName);
  if (~fileName.indexOf('.vue') || fileName.endsWith('.vue')) {
    // debugger
  }

  if (fileName.startsWith('verter-virtual:')) {
    if (snapshots.has(fileName)) {
      return true
    }

    const d = Object.keys(map).find(x => fileName.endsWith(x))
    if (d) {
      return true
    }

    if (fileName.endsWith('/vue.ts') || fileName.endsWith('/vue/jsx-runtime.ts')) {
      return true
    }

    fileName = fileName.replace('verter-virtual:', 'file:')
    console.log('updated fileName to', fileName)
    // debugger
  }

  if (fileName.startsWith('virtual:')) {
    debugger
  }

  return e || snapshots.has(fileName) || ts.sys.fileExists(fileName);
}

function readFile(fileName: string) {
  console.log('reading file', fileName)


  if (fileName.endsWith('/vue') || fileName.endsWith('/vue/jsx-runtime')) {
    return true
  }

  if (fileName.startsWith('verter-virtual:')) {
    debugger
  }
  if (fileName.endsWith("Test.my.ts")) {
    return `atemplate.



declare const atemplate = { 
    a: '1'
} `;
  }
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

  const tsConfigStr = readFileSync(path.resolve(workspacePath, './tsconfig.json'), 'utf-8')

  const { config } = ts.parseConfigFileTextToJson(path.resolve(workspacePath, './tsconfig.json'), tsConfigStr)


  const compilerOptions: ts.CompilerOptions = {
    // ...config.compilerOptions,
    // moduleResolution: ts.ModuleResolutionKind.Bundler,
    jsx: ts.JsxEmit.Preserve,
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    alwaysStrict: true,
    noImplicitAny: true,
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
    getScriptFileNames,
    getScriptVersion: (fileName: string) =>
      getSnapshotIfExists(fileName)?.version?.toString() || "",
    getScriptSnapshot: getSnapshotIfExists,
    getCurrentDirectory: () => workspacePath,
    getDefaultLibFileName: ts.getDefaultLibFilePath,
    fileExists: fileExists,
    readFile: readFile,
    // resolveModuleNames: svelteModuleLoader.resolveModuleNames,
    // readDirectory: svelteModuleLoader.readDirectory,
    readDirectory: (path, extensions, exclude, include) => {
      console.log('reading dir', {
        path, extensions, exclude, include
      })
      return tsSystem.readDirectory(path, extensions, exclude, include)
    },
    getDirectories: tsSystem.getDirectories,
    useCaseSensitiveFileNames: () => tsSystem.useCaseSensitiveFileNames,
    getScriptKind: (fileName: string) => {
      const ext = fileName.slice(fileName.lastIndexOf('.'));
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

    resolveModuleNameLiteralsXX: (moduleLiterals, containingFile, redirectedReference, options, containingSourceFile, reusedNames) => {
      const resolvedModules = moduleLiterals.map(({ text: moduleName }) => {
        const r = ts.resolveModuleName(moduleName, containingFile, options, ts.sys)

        // TODO remove this
        const n = ts.nodeModuleNameResolver(moduleName, './test.ts', options, ts.sys)

        console.log('xxasdsad', {
          r,
          n
        })

        return {
          resolvedModule: r.resolvedModule ?? n.resolvedModule
        };
      });

      console.log('dddd', resolvedModules)

      return resolvedModules;
    }
  };


  const createModuleResolver =
    (containingFile: string) =>
      (
        moduleName: string,
        resolveModule: () =>
          | (ts.ResolvedModuleWithFailedLookupLocations & {
            failedLookupLocations: readonly string[];
          })
          | undefined
      ): ts.ResolvedModuleFull | undefined => {
        // logger.info(
        //   "[[[" +
        //     moduleName +
        //     "| " +
        //     containingFile +
        //     "| " +
        //     isRelativeVue(moduleName) +
        //     "| " +
        //     isVue(moduleName) +
        //     "]]]"
        // );

        if (isRelativeVue(moduleName)) {
          // logger.info(
          //   "[Verter] createModuleResolver relative vue - " +
          //     moduleName +
          //     " -- " +
          //     path.resolve(path.dirname(containingFile), moduleName)
          // );
          return {
            extension: ts.Extension.Tsx,
            isExternalLibraryImport: false,
            resolvedFileName: path.resolve(
              path.dirname(containingFile),
              moduleName
            ),
          };
        }
        if (!isVue(moduleName)) {
          return;
        }

        const resolvedModule = resolveModule();

        if (!resolvedModule) return;

        const baseUrl = workspacePath;
        const match = "/index.ts";

        const failedLocations = resolvedModule.failedLookupLocations;
        // Filter to only one extension type, and remove that extension. This leaves us with the actual file name.
        // Example: "usr/person/project/src/dir/File.module.css/index.d.ts" > "usr/person/project/src/dir/File.module.css"
        const normalizedLocations = failedLocations.reduce<string[]>(
          (locations, location) => {
            if (
              (baseUrl ? location.includes(baseUrl) : true) &&
              location.endsWith(match)
            ) {
              return [...locations, location.replace(match, "")];
            }
            return locations;
          },
          []
        );

        // Find the imported CSS module, if it exists.
        const vueModulePath = normalizedLocations.find((location) =>
          existsSync(location)
        );

        // logger.info(
        //   "[Verter] createModuleResolver vue - " +
        //     resolvedModule +
        //     " -ModulePath-  " +
        //     vueModulePath
        // );
        if (vueModulePath) {
          // logger.info("wwww -- Vue 3 Plugin found path" + vueModulePath);
          return {
            extension: ts.Extension.Tsx,
            isExternalLibraryImport: false,
            resolvedFileName: path.resolve(vueModulePath),
          };
        }

        // logger.info("--- Vue 3 Plugin NOT found path" + vueModulePath);

        // const vueModulePath = failedLocations.find(
        //   (x) =>
        //     (baseUrl ? x.includes(baseUrl) : true) &&
        //     x.endsWith(match) &&
        //     fs.existsSync(x)
        // );

        // if (!vueModulePath) return;
        // return {
        //   extension: ts.Extension.Dts,
        //   isExternalLibraryImport: false,
        //   resolvedFileName: path.resolve(vueModulePath),
        // };
      };


  // ts.readConfigFile()
  const languageService = ts.createLanguageService(
    host,
    ts.createDocumentRegistry(tsSystem.useCaseSensitiveFileNames),
    // ts.LanguageServiceMode.Semantic
  );


  return languageService;
}




const DEFAULT_REGEXP = /\.vue$/;
const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export const isVue = (fileName: string) => DEFAULT_REGEXP.test(fileName);
export const isRelativeVue = (fileName: string) =>
  isVue(fileName) && isRelative(fileName);