import * as path from "path";
import Mocha from "mocha";
import * as fs from "fs";
import { getTimer } from "../timer";

/** Recursively find all *.test.js files under a directory. */
function findTestFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...findTestFiles(fullPath));
    } else if (entry.name.endsWith(".test.js")) {
      results.push(fullPath);
    }
  }
  return results;
}

export async function run(): Promise<void> {
  const mocha = new Mocha({
    ui: "tdd",
    timeout: 120_000,
    color: true,
  });

  const testsRoot = path.resolve(__dirname);
  const onlyPattern = process.env.VERTER_E2E_ONLY || process.env.E2E_ONLY;
  const files = findTestFiles(testsRoot).filter((file) =>
    onlyPattern ? file.includes(onlyPattern) : true,
  );

  for (const f of files) {
    mocha.addFile(f);
  }

  return new Promise((resolve, reject) => {
    mocha.run((failures: number) => {
      // Write timing report regardless of test outcome
      getTimer().flush();

      if (failures > 0) {
        reject(new Error(`${failures} tests failed`));
      } else {
        resolve();
      }
    });
  });
}
