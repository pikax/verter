#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { PACKAGE_ROOT, generatedFiles, loadAuthority, writeGenerated } from "./lib.mjs";

const args = process.argv.slice(2);
const check = args.includes("--check");
const outputIndex = args.indexOf("--output-root");
const outputRoot = outputIndex >= 0 ? path.resolve(args[outputIndex + 1] || "") : PACKAGE_ROOT;
if (outputIndex >= 0 && !args[outputIndex + 1]) throw new Error("--output-root requires a path");
const authority = loadAuthority();
const files = generatedFiles(authority);

if (check) {
  const stale = [];
  for (const [relative, raw] of files) {
    const expected = raw.endsWith("\n") ? raw : `${raw}\n`;
    const file = path.join(PACKAGE_ROOT, relative);
    if (!fs.existsSync(file) || fs.readFileSync(file, "utf8") !== expected) stale.push(relative);
  }
  if (stale.length) {
    console.error(`STALE: ${stale.join(", ")}`);
    process.exit(1);
  }
  console.log(`build-program-dag: PASS (${files.size} generated files fresh)`);
} else {
  writeGenerated(authority, outputRoot);
  console.log(`build-program-dag: wrote ${files.size} files to ${outputRoot}`);
}
