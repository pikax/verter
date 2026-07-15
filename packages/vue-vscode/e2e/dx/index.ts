/**
 * Mocha entry for the extension-host DX run (`extensionTestsPath`).
 *
 * Distinct from `../suite/index.ts`: it loads ONLY the DX suite and runs no
 * fixture-warm root hook, so the DX driver consumes a materialized workspace
 * directly instead of the fixture-matrix `App.vue`. The fixture matrix never loads
 * the DX suite (it lives outside `suite/`), so the two paths stay isolated.
 */
import * as path from "path";

import Mocha from "mocha";

export async function run(): Promise<void> {
  const mocha = new Mocha({ ui: "tdd", timeout: 120_000, color: true });
  mocha.addFile(path.resolve(__dirname, "dx-harness.test.js"));

  return new Promise((resolve, reject) => {
    mocha.run((failures: number) => {
      if (failures > 0) {
        reject(new Error(`${failures} DX tests failed`));
      } else {
        resolve();
      }
    });
  });
}
