import { generateScript } from "../dist/index.js";
import { glob } from "glob";
import { dirname } from "path";
import fs from "fs-extra";
import { parse } from "@vue/compiler-sfc";

const genFiles = glob.sync("./src/**/_gen.ts");

console.log("gen", genFiles);

const dirs = genFiles.map((f) => dirname(f));

console.log("dirs", dirs);

async function processDir(dir) {
  const genFilePath = `${dir}/_gen.ts`;
  const compFilePath = `${dir}/Comp.vue`;

  const [genFile, compFile] = await Promise.all([
    fs.readFile(genFilePath, "utf8"),
    fs.readFile(compFilePath, "utf8"),
  ]);

  const outputFilePath = `${dir}\\index.ts`;

  const parsed = parse(compFile, { filename: "Comp.vue" });
  //   console.log('parsed', parsed)
  const output = generateScript(parsed);

  await fs.outputFile(
    outputFilePath,
    genFile.replace("type __COMP__ = {};", output),
    {
      encoding: "utf8",
      // flag: "w",
    }
  );
}

await Promise.all(dirs.map(processDir));
