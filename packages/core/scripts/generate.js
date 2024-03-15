// import { generateScript } from "../dist/index.js";
import { createBuilder } from "../dist/index.js";
import { glob } from "glob";
import { dirname } from "path";
import fs from "fs-extra";
import { parse } from "@vue/compiler-sfc";

// const genFiles = glob.sync("./generator/**/_gen.ts");
const genFiles = glob.sync("./generator/**/import/_gen.ts");
// const genFiles = glob.sync("./generator/**/slots-typed/_gen.ts");
// const genFiles = glob.sync("./generator/**/template/**/_gen.ts");

console.log("gen", genFiles);

const dirs = genFiles.map((f) => dirname(f));

console.log("dirs", dirs);

const builder = createBuilder();

async function processDir(dir) {
  const genFilePath = `${dir}/_gen.ts`;
  const compFilePath = `${dir}/Index.vue`;

  const [genFile, compFile] = await Promise.all([
    fs.readFile(genFilePath, "utf8"),
    fs.readFile(compFilePath, "utf8"),
  ]);

  const outputFilePath = `.\\generated\\${dir}\\index.tsx`;

  // const parsed = parse(compFile, {
  //   filename: "Comp.vue",
  //   templateParseOptions: {},
  // });
  //   console.log('parsed', parsed)
  const output = builder.process("Comp.vue", compFile);
  // .replace(
  //   "export default __options as __COMP__",
  //   "const Comp = __options as any as __COMP__;"
  // );

  await fs.outputFile(
    outputFilePath,
    "/* @jsxImportSource vue */\n" +
      genFile.replace("type __COMP__ = {};", output),
    {
      encoding: "utf8",
      // flag: "w",
    }
  );
}

await Promise.all(dirs.map(processDir));
