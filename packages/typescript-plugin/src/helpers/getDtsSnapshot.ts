// import { readFileSync } from "fs";
import type tsModule from "typescript/lib/tsserverlibrary";
import { createBuilder, mergeFull } from "@verter/core";
import { dirname, resolve, basename } from "node:path";

// export const getDtsSnapshot = (
//   ts: typeof tsModule,
//   fileName: string,
//   logger: tsModule.server.Logger
//   //   compilerOptions: tsModule.CompilerOptions,
//   //   directory: string
// ): tsModule.IScriptSnapshot => {
//   const source = readFileSync(fileName, "utf-8");
//   const builder = createBuilder({});

//   let result = builder.process(fileName, source);
//   result = [
//     "/* @jsxImportSource vue */",
//     "import { ComponentInstance } from 'vue';",
//     `type ExtractRenderComponents<T> = {};`,
//     result,
//   ].join("\n");

//   logger.info(`parsing ${fileName}`);
//   logger.info("result\n" + result);

//   //   const dts = createDtsExports({ cssExports, fileName, logger, options });
//   return ts.ScriptSnapshot.fromString(result);
// };

function getRequire(packageName: string) {
  return require(packageName);
}

export function getPackage(packageName: string, path: string) {
  const paths = [__dirname, path];

  // TODO handle untrusted workspacesF

  const pkgPath = require.resolve(`${packageName}/package.json`, {
    paths,
  });

  return {
    path: dirname(pkgPath),
  };
}

export function importVueCompiler(
  fromPath: string
): typeof import("vue/compiler-sfc") {
  const pkg = getPackage("vue", fromPath);
  const main = resolve(pkg.path, "compiler-sfc");
  return getRequire(main);
}

export const parseFile = (
  fileName: string,
  content: string,
  logger: tsModule.server.Logger
) => {
  logger.info(`[Verter] parsing ${fileName}`);

  return `export default {
    test: 1
  }`;

  const builder = createBuilder({});
  const compiler = importVueCompiler(fileName)!;
  const parsed = compiler.parse(content, {
    filename: basename(fileName),
    sourceMap: true,
    ignoreEmpty: false,
    templateParseOptions: {
      parseMode: "sfc",
    },
  });

  const { locations, context } = builder.fromCompiled(parsed);
  const result = mergeFull(locations, context);

  return result.content;
  // const p = builder.preProcess(fileName, content);

  // const merged = mergeFull(p.locations, p.context);

  // console.log('meerd ', merged.content)
  // return merged.content;
};
