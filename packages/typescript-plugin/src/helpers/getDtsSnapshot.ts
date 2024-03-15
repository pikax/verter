// import { readFileSync } from "fs";
import type tsModule from "typescript/lib/tsserverlibrary";
import { createBuilder } from "@verter/core";

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

export const parseFile = (
  fileName: string,
  content: string,
  logger: tsModule.server.Logger
) => {
  logger.info(`[Verter] parsing ${fileName}`);
  const builder = createBuilder({});
  return builder.process(fileName, content);
};
