import ts, { ScriptKind } from "typescript";
import Logger from "../../logger";
import { documentManager } from "../../lib/documents/Manager";
import { readFileSync } from "node:fs";
import { VueDocument } from "../../lib/documents/VueDocument";

function getScriptFileNames() {
  const d = documentManager.getAllOpened();

  const vueFiles = d.filter(x => x.endsWith('.vue'))/*.map(x => x.replace('file:', 'virtual:'))*/.flatMap(x => [
    x + '.d.tsx',
    // x + '.template.d.ts',
    // TODO add more blocks
    // x + '.css'
  ])
  // const vueFiles = []



  return d.concat(vueFiles);
}
function getSnapshotIfExists(
  fileName: string
): ts.IScriptSnapshot & { version: number } {
  if (fileName.endsWith('.vue')) {
    fileName = fileName.replace('.vue', '.ts')
  }

  // if(fileName.endsWith(''))
  fileName = fileName.replace('Test.vue.d.tsx', 'test.ts')

  // if (fileName.startsWith("file:///")) {
  //   fileName = decodeURIComponent(
  //     fileName.replace("Test.vue", "Test.my.ts").replace("file:///", "")
  //   );
  // }
  if (snapshots.has(fileName)) {
    return snapshots.get(fileName);
  }

  if (fileName.startsWith('virtual:') || fileName.endsWith('.vue.d.tsx')) {
    debugger
    const doc = documentManager.getDocument(fileName)

    const vueDoc = new VueDocument(doc?.uri, doc._content)


    const snap = ts.ScriptSnapshot.fromString(vueDoc.template.content);
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
  console.log("file exists", fileName);
  if (~fileName.indexOf('.vue') || fileName.endsWith('.vue')) {
    debugger
  }

  if (fileName.startsWith('virtual:')) {
    debugger
  }

  return e || snapshots.has(fileName) || ts.sys.fileExists(fileName);
}

function readFile(fileName: string) {
  console.log('reading file', fileName)

  if (fileName.startsWith('virtual:')) {
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
  const compilerOptions: ts.CompilerOptions = {};

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
    getDirectories: tsSystem.getDirectories,
    useCaseSensitiveFileNames: () => tsSystem.useCaseSensitiveFileNames,
    getScriptKind: (fileName: string) => {
      return ScriptKind.TSX
    },
    // getProjectVersion: () => projectVersion.toString(),
    getNewLine: () => tsSystem.newLine,
    // resolveTypeReferenceDirectiveReferences:
    //   svelteModuleLoader.resolveTypeReferenceDirectiveReferences,
    // hasInvalidatedResolutions:
    //   svelteModuleLoader.mightHaveInvalidatedResolutions,
    // getModuleResolutionCache: svelteModuleLoader.getModuleResolutionCache,
  };
  const languageService = ts.createLanguageService(
    host,
    ts.createDocumentRegistry(),
    ts.LanguageServiceMode.PartialSemantic
  );

  return languageService;
}
