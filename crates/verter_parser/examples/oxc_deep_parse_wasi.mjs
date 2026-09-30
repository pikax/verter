// Runs `oxc_deep_parse` built for wasm32-wasip1 under Node.js's WASI host,
// parsing on the module's own stack:
//
//   cargo build -p verter_parser --example oxc_deep_parse --release --target wasm32-wasip1
//   node crates/verter_parser/examples/oxc_deep_parse_wasi.mjs <form> <depth> [module]
//
// Node's `--stack-size=<KiB>` sets the engine stack the module's calls run on.
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { WASI } from "node:wasi";

const [form, depth, module] = process.argv.slice(2);
const path =
  module ??
  fileURLToPath(
    new URL("../../../target/wasm32-wasip1/release/examples/oxc_deep_parse.wasm", import.meta.url),
  );
const wasi = new WASI({ version: "preview1", args: ["oxc_deep_parse", form, depth, "0"] });
const instance = await WebAssembly.instantiate(
  await WebAssembly.compile(await readFile(path)),
  wasi.getImportObject(),
);
try {
  wasi.start(instance);
} catch (error) {
  console.log(`${error.constructor.name}: ${error.message}`);
  process.exitCode = 1;
}
